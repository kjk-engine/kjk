//! `kjk script security <game> [module] [--door TAG] [--ranks N]`: locks
//! and the Security skill checked end to end on a module's own locked
//! objects (`sim::locks`; the Endar Spire's room 3 door, Trask's Security
//! tutorial, by default). The module is installed with its item templates
//! and scripts made here: its locks' own hooks (`OnFailToOpen`, `OnOpen`)
//! are stand-ins that note they ran, and `kjk_ask` reads a lock as the
//! lock routines see it.
//!
//! 1. The door's facts as scripts read them (`GetLocked`,
//!    `GetLockUnlockDC`, `GetLockKeyRequired`, `GetIsDoorActionPossible`).
//! 2. Used (E), it fails to open: its `OnFailToOpen`, "This object is
//!    locked."
//! 3. Security with no ranks is refused; with ranks, out of combat, the
//!    check takes 20 once its use's time is up (a spike's bonus when 20 is
//!    not enough, the spike kept on a success, used up on a failure);
//!    unlocked, the door opens (`OnOpen`, `GetLastUnlocked`).
//! 4. Locked again by a script (`ActionLockObject`, `GetLastLocked`); in
//!    combat the check rolls a d20.
//! 5. Where the module has them: a lock only its key opens refuses
//!    Security; a keyed lock opens with its key (used up when the lock
//!    says so); a locked container unlocked by Security stays shut until
//!    opened.
//! 6. The user says its sound set's lines (begun, failed, succeeded), and
//!    in combat its weapons are hidden while it works the lock
//!    (`animations.2da` `hideequippeditems`, `acting::HIDES_ITEMS`); the
//!    players' sound sets (K1's `Player_…_W` rows by the portrait's sex;
//!    none in TSL) are voiced.
//! 7. A save carries the locks; no script faulted.

use std::process::ExitCode;
use std::sync::Arc;

use serde_json::json;
use fmt_odyssey::ssf::slots;
use sim::ObjectState;
use sim::action::Action;
use sim::combat::sheet::skills;
use sim::locks::lines;
use sim::module::{Lock, ObjectData, ObjectKind};
use sim::notice::Notice;
use vfs::Discovery;

use crate::probe::{Call, Probe, int, me, program, text};
use crate::scenes::Report;
use crate::script_verbs::{catalog, open};

/// `SetGlobalNumber(name, <value>)`.
fn set(name: &str, value: &[Call]) -> Vec<Call> {
    let mut calls = value.to_vec();
    calls.extend([text(name), Call::Routine("SetGlobalNumber", 2)]);
    calls
}

/// `GetIsObjectValid(<routine>())`.
fn valid(routine: &'static str) -> Vec<Call> {
    vec![
        Call::Routine(routine, 0),
        Call::Routine("GetIsObjectValid", 1),
    ]
}

/// A creation of `template` on the party: `KJK_GIVEN` its validity.
fn give(template: &str) -> Vec<Vec<Call>> {
    vec![set(
        "KJK_GIVEN",
        &[
            int(1),
            Call::Routine("GetFirstPC", 0),
            text(template),
            Call::Routine("CreateItemOnObject", 3),
            Call::Routine("GetIsObjectValid", 1),
        ],
    )]
}

/// A module object found for a check: its index and facts.
type Found<'a> = (u32, &'a ObjectData);

fn find(objects: &[Arc<ObjectData>], test: impl Fn(&ObjectData) -> bool) -> Option<Found<'_>> {
    objects
        .iter()
        .enumerate()
        .find(|(_, object)| test(object))
        .map(|(index, object)| (index as u32, object.as_ref()))
}

fn locked_placed(object: &ObjectData) -> bool {
    ObjectState::default().locked(object.opens)
}

/// The module object's entity in `snapshot`.
fn entity(snapshot: &sim::Snapshot, kind: ObjectKind, index: u32) -> Option<&sim::EntityState> {
    snapshot
        .entities
        .iter()
        .find(|entity| entity.kind == sim::Kind::Object(kind, index))
}

fn state_of(snapshot: &sim::Snapshot, kind: ObjectKind, index: u32) -> Option<ObjectState> {
    entity(snapshot, kind, index).and_then(|entity| entity.object)
}

/// The player stood at (`x`, `y`) on the floor of `object` (metres).
pub(crate) fn stand(
    probe: &mut Probe,
    object: &ObjectData,
    x: f32,
    y: f32,
) -> Result<(), String> {
    probe.edit_me(|me| {
        let player = me.player.as_mut().ok_or("no player")?;
        player.origin = [
            x / collision::UNIT,
            y / collision::UNIT,
            object.position[2] / collision::UNIT + 0.125 - sim::motion::SOLE_Z,
        ];
        player.velocity = [0.0; 3];
        Ok(())
    })
}

/// The player uses (E) the object from a metre away, from whichever side
/// the use reaches it: what the step told it, or none if no side did.
fn use_object(probe: &mut Probe, object: &ObjectData) -> Result<Vec<Notice>, String> {
    for quarter in 0..4 {
        let (x, y) = side(object, quarter);
        stand(probe, object, x, y)?;
        // Past the use's repeat delay.
        let stood = probe.step(16);
        let player = stood
            .get(probe.me)
            .and_then(|me| me.player)
            .ok_or("no player")?;
        let towards = (object.position[1] - y)
            .atan2(object.position[0] - x)
            .to_degrees();
        let (_, told) = probe.press(sim::Command {
            buttons: sim::buttons::USE,
            ..sim::combat::mind::looking(&player, 0.0, towards)
        });
        if told
            .iter()
            .any(|notice| matches!(notice, Notice::Line(lines::LOCKED | lines::KEY_USED)))
        {
            return Ok(told);
        }
    }
    Ok(Vec::new())
}

/// A metre from the object on its `quarter` side (0 to 3, a quarter turn
/// each from its facing): x, y in metres.
fn side(object: &ObjectData, quarter: u8) -> (f32, f32) {
    let side = (object.yaw + 90.0 * f32::from(quarter)).to_radians();
    (
        object.position[0] + side.cos(),
        object.position[1] + side.sin(),
    )
}

/// The player stood a metre from the object, on the first side where it
/// comes to rest out of a door's doorway (`sim::Level::at_door`): a door
/// does not close on someone standing where its panels shut, so a player
/// left there would hold it open, as in KOTOR (end_m01ab's `end_door05`,
/// whose panels reach a metre along it; TSL 101per's `medclosetdoor`,
/// beside which the room's walls move a player stood there into it).
fn beside(probe: &mut Probe, (index, object): Found) -> Result<(), String> {
    for quarter in 0..4 {
        let (x, y) = side(object, quarter);
        stand(probe, object, x, y)?;
        let settled = probe.step(SETTLE_STEPS);
        let body = settled
            .get(probe.me)
            .and_then(|me| me.player)
            .ok_or("no player")?
            .origin
            .map(|v| v * collision::UNIT);
        if object.kind != ObjectKind::Door || !probe.world.level().at_door(index, body) {
            return Ok(());
        }
    }
    Err(format!("{}: every side leaves the player in its doorway", object.tag))
}

/// Steps for a player stood somewhere to come to rest (a second).
const SETTLE_STEPS: u64 = 60;

fn number(probe: &Probe, name: &str) -> Option<i32> {
    sim::script::global(&probe.world.script_state().numbers, name).copied()
}

/// `kjk_ask` run as the object: its lock as the routines see it.
fn ask(probe: &mut Probe, object: sim::SimId) -> [Option<i32>; 5] {
    probe.queue("kjk_ask", object);
    probe.step(2);
    [
        "KJK_LOCKED",
        "KJK_DC",
        "KJK_KEYONLY",
        "KJK_UNLOCKER",
        "KJK_LOCKER",
    ]
    .map(|name| number(probe, name))
}

/// `GetIsDoorActionPossible(door, DOOR_ACTION_UNLOCK)` asked by the player.
fn can_unlock(probe: &mut Probe, tag: &str) -> Option<i32> {
    probe.queue(&format!("kjk_can_{tag}"), probe.me);
    probe.step(2);
    number(probe, "KJK_CAN")
}

/// Use held on for `steps` steps where the player looks: what it was told.
fn hold_use(probe: &mut Probe, steps: usize) -> Result<Vec<Notice>, String> {
    let mut told = Vec::new();
    for _ in 0..steps {
        let snapshot = probe.world.snapshot();
        let player = snapshot
            .get(probe.me)
            .and_then(|me| me.player)
            .ok_or("no player")?;
        let (_, now) = probe.press(sim::Command {
            buttons: sim::buttons::USE,
            ..sim::combat::mind::looking(&player, player.view[0], player.view[1])
        });
        told.extend(now);
    }
    Ok(told)
}

/// What the player's Security on an object came to (`try_lock`).
struct Tried {
    /// The last snapshot.
    after: sim::Snapshot,
    /// Every notice it told (a refusal at once).
    told: Vec<Notice>,
    /// Whether its weapons were drawn at a step while it worked
    /// (`weapons::armed`), if it worked a step.
    drawn: Option<bool>,
}

/// The player's Security on the object, through its use's time
/// (`sim::locks::UNLOCK_MS`), until the check or a refusal is told, and the
/// steps that apply what it did.
fn try_lock(probe: &mut Probe, index: u32, spike: Option<u32>) -> Tried {
    let told_out = |told: &[Notice]| {
        told.iter()
            .any(|notice| matches!(notice, Notice::Check { .. } | Notice::Line(_) | Notice::OutOfReach))
    };
    let (mut snapshot, mut told) = probe.act(Action::Unlock {
        object: index,
        spike,
    });
    let until = snapshot.time_ms + sim::locks::UNLOCK_MS + 500;
    let mut drawn = None;
    while !told_out(&told) && snapshot.time_ms < until {
        let fighter = snapshot.get(probe.me).and_then(|me| me.fighter);
        if let Some(fighter) = fighter {
            let now = crate::weapons::armed(&fighter, snapshot.time_ms as f64, false);
            drawn = Some(drawn.unwrap_or(false) || now);
        }
        let (next, now) = probe.step_told(1);
        snapshot = next;
        told.extend(now);
    }
    let (after, now) = probe.step_told(3);
    told.extend(now);
    Tried { after, told, drawn }
}

/// The lines of the player's sound set `told` said (`Notice::Sound`).
fn lines_said(probe: &Probe, told: &[Notice]) -> Vec<usize> {
    told.iter()
        .filter_map(|notice| match notice {
            Notice::Sound(sound) if sound.object == probe.me.0 => sound.sound_set_line(),
            _ => None,
        })
        .collect()
}

/// The line its user says when Security succeeds or fails.
fn outcome_line(success: bool) -> usize {
    if success {
        slots::UNLOCK_SUCCESS
    } else {
        slots::UNLOCK_FAILED
    }
}

fn security_notice(told: &[Notice]) -> Option<(Option<u8>, i16, u8, bool)> {
    told.iter().find_map(|notice| match notice {
        Notice::Check {
            skill: sim::locks::lines::SECURITY,
            roll,
            total,
            dc,
            success,
        } => Some((*roll, *total, *dc, *success)),
        _ => None,
    })
}

/// The first carried item of `template`'s id.
fn carried(snapshot: &sim::Snapshot, template: &str) -> Option<u32> {
    snapshot
        .goods
        .carried(sim::inventory::PARTY)
        .find(|(_, item)| item.template == template)
        .map(|(id, _)| id)
}

const SPIKE: &str = "g_i_secspike02";

/// How many `SPIKE`s the party carries.
fn spikes(snapshot: &sim::Snapshot) -> u16 {
    snapshot
        .goods
        .carried(sim::inventory::PARTY)
        .filter(|(_, item)| item.template == SPIKE)
        .map(|(_, item)| item.stack)
        .sum()
}

pub fn security(
    discovery: &Discovery,
    game: &str,
    module: &str,
    door: Option<&str>,
    ranks: u8,
) -> ExitCode {
    let mut report = Report::default();
    let run = |report: &mut Report| -> Result<serde_json::Value, String> {
        let (game, _, base) = open(discovery, game)?;
        let routines = catalog(&base)?;
        let mut templates = None;
        let mut probe = Probe::new(discovery, game, module, |level| {
            let own = level.scripts().ok_or("the module has no scripts")?;
            let items = own.items().clone();
            let objects = level.placed();
            let mut scripts: std::collections::BTreeMap<String, fmt_odyssey::ncs::Ncs> = [
                (
                    "kjk_ask",
                    vec![
                        set("KJK_LOCKED", &[me(), Call::Routine("GetLocked", 1)]),
                        set("KJK_DC", &[me(), Call::Routine("GetLockUnlockDC", 1)]),
                        set(
                            "KJK_KEYONLY",
                            &[me(), Call::Routine("GetLockKeyRequired", 1)],
                        ),
                        set("KJK_UNLOCKER", &valid("GetLastUnlocked")),
                        set("KJK_LOCKER", &valid("GetLastLocked")),
                    ],
                ),
                (
                    "kjk_relock",
                    vec![
                        vec![me(), Call::Routine("ActionCloseDoor", 1)],
                        vec![me(), Call::Routine("ActionLockObject", 1)],
                    ],
                ),
                ("kjk_spike", give(SPIKE)),
            ]
            .into_iter()
            .map(|(name, calls)| Ok((name.to_string(), program(&routines, &calls)?)))
            .collect::<Result<_, String>>()?;
            // The locks' own hooks note that they ran.
            let failed = program(&routines, &[set("KJK_FAILED", &[int(1)])])?;
            let opened = program(&routines, &[set("KJK_OPENED", &valid("GetLastOpenedBy"))])?;
            for object in objects.iter().filter(|object| locked_placed(object)) {
                let object = object.as_ref();
                let can = [
                    int(1),
                    int(0),
                    text(&object.tag),
                    Call::Routine("GetObjectByTag", 2),
                    Call::Routine("GetIsDoorActionPossible", 2),
                ];
                scripts.insert(
                    format!("kjk_can_{}", object.tag),
                    program(&routines, &[set("KJK_CAN", &can)])?,
                );
                for (label, stand_in) in [(sim::script::Hook::FailToOpen, &failed), (sim::script::Hook::Open, &opened)] {
                    if let Some(name) = label.of(object) {
                        scripts.insert(name.to_string(), stand_in.clone());
                    }
                }
                if let Some(key) = object.lock.as_ref().map(|lock| &lock.key)
                    && let Some(facts) = items
                        .iter()
                        .find(|facts| facts.tag.eq_ignore_ascii_case(key))
                {
                    scripts.insert(
                        format!("kjk_key_{}", facts.template),
                        program(&routines, &give(&facts.template))?,
                    );
                }
            }
            let talents = sim::script::Talents {
                skill_abilities: (0..8)
                    .map(|row| {
                        own.skill_ability(row)
                            .map_or(u8::MAX, |ability| ability as u8)
                    })
                    .collect(),
                ..sim::script::Talents::default()
            };
            templates = Some(items.clone());
            Ok(sim::script::Library::new(routines, scripts, talents).with_items(items))
        })?;
        let templates = templates.ok_or("no templates")?;
        let level = probe.world.level().clone();
        let objects = level.placed();
        // By default the first a spike's bonus with the ranks reaches.
        let reached = |object: &ObjectData| {
            object.lock.as_ref().is_none_or(|lock| {
                !lock.key_required && i16::from(lock.open_dc) <= 30 + i16::from(ranks)
            })
        };
        let (index, target) = find(objects, |object| {
            object.kind == ObjectKind::Door
                && locked_placed(object)
                && match door {
                    Some(tag) => object.tag.eq_ignore_ascii_case(tag),
                    None => reached(object),
                }
        })
        .ok_or(format!("no locked door in {module} that Security opens"))?;
        let dc = target.lock.as_ref().map_or(0, |lock| lock.open_dc);
        let snapshot = probe.world.snapshot();
        let door_id = entity(&snapshot, ObjectKind::Door, index)
            .map(|entity| entity.id)
            .ok_or("the door")?;
        let door_state = |snapshot: &sim::Snapshot| state_of(snapshot, ObjectKind::Door, index);

        // 1. The facts.
        let asked = ask(&mut probe, door_id);
        let can = can_unlock(&mut probe, &target.tag);
        report.check(
            asked[..3] == [Some(1), Some(i32::from(dc)), Some(0)] && can == Some(0),
            format!(
                "{}: GetLocked, GetLockUnlockDC, GetLockKeyRequired = {:?} (DC {dc}); GetIsDoorActionPossible(unlock) for the player with no ranks {can:?}",
                target.tag,
                &asked[..3]
            ),
        );

        // 2. Used while locked.
        let told = use_object(&mut probe, target)?;
        let failed_hook = sim::script::Hook::FailToOpen.of(target);
        let failed = number(&probe, "KJK_FAILED") == Some(1);
        let shut = door_state(&probe.world.snapshot()).is_some_and(|state| state.state == 0);
        let held = hold_use(&mut probe, 60)?;
        report.check(
            told.contains(&Notice::Line(lines::LOCKED))
                && shut
                && (failed || failed_hook.is_none())
                && held.is_empty(),
            format!(
                "used, it stays shut: told {told:?}; its OnFailToOpen ({}) ran: {failed}; use held on for 60 steps told {held:?}",
                failed_hook.unwrap_or("none")
            ),
        );

        // 3. Security.
        beside(&mut probe, (index, target))?;
        let told = try_lock(&mut probe, index, None).told;
        report.check(
            told == [Notice::Line(lines::CANNOT_USE_SKILL)],
            format!("with no ranks in Security: told {told:?}"),
        );
        probe.edit_me(|me| {
            let sheet = me.sheet.as_mut().ok_or("no sheet")?;
            sheet.skills[skills::SECURITY] = ranks;
            Ok(())
        })?;
        let can = can_unlock(&mut probe, &target.tag);
        let Tried { after, told, .. } = try_lock(&mut probe, index, None);
        let check = security_notice(&told);
        let total = check.map_or(0, |(_, total, ..)| total);
        let enough = 20 + total >= i16::from(dc);
        let said = lines_said(&probe, &told);
        report.check(
            check.is_some_and(|(roll, _, at, success)| {
                roll.is_none() && at == dc && success == enough
            }) && door_state(&after).is_some_and(|state| state.locked(target.opens) != enough)
                && can == Some(i32::from(enough))
                && said == [slots::BEGIN_UNLOCK, outcome_line(enough)],
            format!(
                "Security {ranks} out of combat takes 20: {check:?} (20 + {total} vs DC {dc}: {}); GetIsDoorActionPossible(unlock) {can:?}; the user's sound set's lines {said:?}",
                if enough { "opens" } else { "not possible" }
            ),
        );
        let after = if enough {
            after
        } else {
            probe.queue("kjk_spike", probe.me);
            let given = probe.step(2);
            let spike = carried(&given, SPIKE).ok_or("no spike given")?;
            let bonus = templates.get(SPIKE).map_or(0, |facts| facts.tools);
            let Tried { after, told, .. } = try_lock(&mut probe, index, Some(spike));
            let check = security_notice(&told);
            let spent = spikes(&probe.step(1)) < spikes(&given);
            let said = lines_said(&probe, &told);
            report.check(
                check.is_some_and(|(_, with, _, success)| {
                    with == total + i16::from(bonus)
                        && spent != success
                        && said == [slots::BEGIN_UNLOCK, outcome_line(success)]
                }),
                format!(
                    "with a {SPIKE} (+{bonus}): {check:?}, the spike used up (only on a failure): {spent}; lines {said:?}"
                ),
            );
            after
        };
        let opened_hook = sim::script::Hook::Open.of(target);
        probe.step(2);
        let asked = ask(&mut probe, door_id);
        let opened = number(&probe, "KJK_OPENED") == Some(1);
        report.check(
            door_state(&after).is_some_and(|state| state.state != 0 && !state.locked(target.opens))
                && asked[0] == Some(0)
                && asked[3] == Some(1)
                && (opened || opened_hook.is_none()),
            format!(
                "unlocked, it opens: GetLocked {:?}, GetLastUnlocked valid {:?}; its OnOpen ({}) ran with GetLastOpenedBy: {opened}",
                asked[0],
                asked[3],
                opened_hook.unwrap_or("none")
            ),
        );

        // 4. Locked again by a script; in combat.
        probe.queue("kjk_relock", door_id);
        let relocked = probe.step(10);
        let asked = ask(&mut probe, door_id);
        report.check(
            door_state(&relocked)
                .is_some_and(|state| state.state == 0 && state.locked(target.opens))
                && asked[0] == Some(1)
                && asked[4] == Some(1),
            format!(
                "ActionCloseDoor, ActionLockObject: GetLocked {:?}, GetLastLocked valid {:?}",
                asked[0], asked[4]
            ),
        );
        let now = probe.world.snapshot().time_ms;
        probe.edit_me(|me| {
            let fighter = me.fighter.as_mut().ok_or("no fighter")?;
            fighter.combat_until = now + 60_000;
            Ok(())
        })?;
        probe.queue("kjk_spike", probe.me);
        let given = probe.step(2);
        let spike = carried(&given, SPIKE);
        let Tried { after, told, drawn } = try_lock(&mut probe, index, spike);
        let check = security_notice(&told);
        let spent = spikes(&probe.step(1)) < spikes(&given);
        let drawn_after = after
            .get(probe.me)
            .and_then(|me| me.fighter)
            .is_some_and(|fighter| crate::weapons::armed(&fighter, after.time_ms as f64, false));
        report.check(
            drawn == Some(false) && drawn_after,
            format!(
                "in combat its weapons are drawn, and hidden while it works the lock (animations.2da hideequippeditems): drawn while working {drawn:?}, after {drawn_after}"
            ),
        );
        let rolled = check.is_some_and(|(roll, total, dc, success)| {
            roll.is_some_and(|roll| {
                (1..=20).contains(&roll) && success == (i16::from(roll) + total >= i16::from(dc))
            })
        });
        let open = door_state(&after).is_some_and(|state| state.state != 0);
        report.check(
            rolled && check.is_some_and(|(.., success)| success == open && spent != success),
            format!(
                "in combat a d20 is rolled: {check:?}, the door open: {open}, the spike used up: {spent}"
            ),
        );

        // 5. What else the module has.
        let key_only = find(objects, |object| {
            locked_placed(object)
                && object
                    .lock
                    .as_ref()
                    .is_some_and(|lock| lock.key_required && lock.key.is_empty())
        });
        match key_only {
            Some((index, object)) => {
                beside(&mut probe, (index, object))?;
                let told = try_lock(&mut probe, index, None).told;
                report.check(
                    told == [Notice::Line(lines::KEY_ONLY)],
                    format!("{}, only its key opens it: told {told:?}", object.tag),
                );
            }
            None => outln!("    (no lock only a key opens in {module})"),
        }
        let keyed = find(objects, |object| {
            locked_placed(object)
                && object.lock.as_ref().is_some_and(|lock| {
                    templates.iter().any(|facts| {
                        !lock.key.is_empty() && facts.tag.eq_ignore_ascii_case(&lock.key)
                    })
                })
        });
        match keyed {
            Some((index, object)) => {
                let lock = object.lock.clone().unwrap_or_else(Lock::default);
                let key = templates
                    .iter()
                    .find(|facts| facts.tag.eq_ignore_ascii_case(&lock.key))
                    .map(|facts| facts.template.clone())
                    .ok_or("no key")?;
                let without = use_object(&mut probe, object)?;
                probe.queue(&format!("kjk_key_{key}"), probe.me);
                probe.step(2);
                let told = use_object(&mut probe, object)?;
                let after = probe.step(1);
                let state = state_of(&after, object.kind, index);
                let kept = carried(&after, &key).is_some();
                report.check(
                    without.contains(&Notice::Line(lines::LOCKED))
                        && told.contains(&Notice::Line(lines::KEY_USED))
                        && state.is_some_and(|state| !state.locked(object.opens))
                        && kept != lock.auto_remove_key,
                    format!(
                        "{} without its key: told {without:?}; with its key {key}: told {told:?}, unlocked, the key {}",
                        object.tag,
                        if kept { "kept" } else { "used up" }
                    ),
                );
            }
            None => outln!("    (no lock with a key the game has in {module})"),
        }
        let chest = find(objects, |object| {
            object.container && locked_placed(object) && reached(object)
        });
        match chest {
            Some((index, object)) => {
                probe.edit_me(|me| {
                    let fighter = me.fighter.as_mut().ok_or("no fighter")?;
                    fighter.combat_until = 0;
                    Ok(())
                })?;
                beside(&mut probe, (index, object))?;
                probe.queue("kjk_spike", probe.me);
                let given = probe.step(2);
                let Tried {
                    after: unlocked,
                    told,
                    ..
                } = try_lock(&mut probe, index, carried(&given, SPIKE));
                let (opened, _) = probe.act(Action::Open(index));
                let (unlocked, opened) = (
                    state_of(&unlocked, ObjectKind::Placeable, index),
                    state_of(&opened, ObjectKind::Placeable, index),
                );
                report.check(
                    unlocked.is_some_and(|state| !state.locked(object.opens) && state.tenants == 0)
                        && opened.is_some_and(|state| state.tenants & 1 != 0),
                    format!(
                        "{} (container): {:?}, then opened by its player",
                        object.tag,
                        security_notice(&told)
                    ),
                );
            }
            None => outln!("    (no locked container Security opens in {module})"),
        }

        // 6. What the views read: the clips that hide a character's
        // equipped items, and the player's sound set's lines.
        let source = crate::modules::ResolverSource(&base);
        let hiding: Vec<String> = asset_odyssey::creature::hiding_animations(&source)
            .into_iter()
            .filter(|name| crate::acting::clips().any(|(_, clip)| clip == name))
            .collect();
        report.check(
            hiding == crate::acting::HIDES_ITEMS,
            format!("animations.2da hides equipped items for {hiding:?} of the clips drawn"),
        );
        let tables = &mut asset_odyssey::tables::Tables::default();
        let portraits = crate::persona::Portraits::read(&base).ok_or("no portraits.2da")?;
        let mut sets = std::collections::BTreeMap::new();
        for portrait in portraits.rows() {
            let row = asset_odyssey::creature::player_sound_set(&source, portrait, tables);
            sets.entry(row).or_insert_with(Vec::new).push(portrait);
        }
        let mut voiced = Vec::new();
        for row in sets.keys().flatten() {
            for slot in [
                slots::BEGIN_UNLOCK,
                slots::UNLOCK_FAILED,
                slots::UNLOCK_SUCCESS,
            ] {
                let sound = asset_odyssey::creature::sound_set_line(&source, *row, slot, tables);
                voiced.push(sound.filter(|sound| crate::sound::kotor_file(&base, sound).is_some()));
            }
        }
        report.check(
            match game {
                vfs::Game::Kotor1 => sets.len() == 2 && !sets.contains_key(&None),
                _ => sets.keys().all(Option::is_none),
            } && voiced.iter().all(Option::is_some),
            format!(
                "the players' sound sets by portrait {sets:?}; their Security lines voiced by {voiced:?}"
            ),
        );

        // 7. A save; faults.
        let snapshot = probe.world.snapshot();
        let save = sim::Save {
            hz: sim::TICK_HZ,
            level: probe.world.level().id(),
            snapshot: snapshot.clone(),
            script: probe.world.script_state().clone(),
        };
        let loaded = sim::Save::decode(&save.encode()).map_err(|e| e.to_string())?;
        report.check(
            loaded.snapshot == snapshot,
            "a save carries the locks as they are".into(),
        );
        let faults = probe.world.script_report().faults.clone();
        report.check(
            faults.is_empty(),
            format!("the scripts ran without faults ({faults:?})"),
        );
        Ok(json!({ "module": module, "door": target.tag, "dc": dc, "ranks": ranks }))
    };
    match run(&mut report) {
        Ok(facts) => report.finish(facts),
        Err(error) => {
            eprintln!("{error}");
            ExitCode::FAILURE
        }
    }
}
