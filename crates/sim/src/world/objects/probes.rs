//! The probes of `objects` (`kjk sim probes`): blows on the proving
//! ground's door and on placeables added beside it.

use crate::action::{Action, Verb};
use crate::combat::damage::Source;
use crate::combat::sheet::{Weapon, damage_types};
use crate::input::Seat;
use crate::module::{ObjectData, ObjectKind, Opens, Placement, Toughness};
use crate::notice::Notice;
use crate::snapshot::{ObjectState, SimId, lock, look};

/// The probe's scripts: a struck object tells whoever struck it
/// (`SendMessageToPC(<getter>(), words)`), by catalog routine.
const CATALOG: &str = r#"
// 0
void SendMessageToPC(object oPlayer, string szMessage);
// 1
object GetLastDamager();
// 2
object GetLastKiller();
// 3
object GetObjectByTag(string sTag, int nNth = 0);
// 4
void DoDoorAction(object oTargetDoor, int nDoorAction);
int DOOR_ACTION_BASH = 2;
"#;
const SAYS: [(&str, u16, &str); 2] = [("probe_damaged", 1, "damaged"), ("probe_broke", 2, "broke")];

fn library() -> crate::script::Library {
    use fmt_odyssey::ncs::{Const, Op};
    let says = SAYS.map(|(name, getter, words)| {
        let mut asm = script_ncs::probes::Asm::default();
        asm.op(Op::Const(Const::String(words.as_bytes().to_vec())))
            .op(Op::Action {
                routine: getter,
                args: 0,
            })
            .op(Op::Action {
                routine: 0,
                args: 2,
            })
            .op(Op::Retn);
        (name.to_string(), asm.build())
    });
    // The bruiser's heartbeat: `DoDoorAction(GetObjectByTag("box"),
    // DOOR_ACTION_BASH)`.
    let mut bash = script_ncs::probes::Asm::default();
    bash.op(Op::Const(Const::Int(2)))
        .op(Op::Const(Const::Int(0)))
        .op(Op::Const(Const::String(b"box".to_vec())))
        .op(Op::Action {
            routine: 3,
            args: 2,
        })
        .op(Op::Action {
            routine: 4,
            args: 2,
        })
        .op(Op::Retn);
    crate::probe::library(
        CATALOG,
        says.into_iter()
            .chain([("probe_bash".to_string(), bash.build())]),
    )
}

/// The proving ground's door, and the objects added after its trigger.
const GATE: u32 = 0;
const CRATE: u32 = 2;
const STATUE: u32 = 3;
const VAULT: u32 = 4;
const SEALED: u32 = 5;
const THUG: u32 = 6;
const SAFE: u32 = 7;
/// The scripted bash's own ground: its box, after the trigger.
const BOX: u32 = 2;

/// The gate's hit points (as KOTOR's doors: 60 now of 20) and hardness,
/// and what a blow on it deals before strength: two break it.
const GATE_HP: (i32, i32) = (60, 20);
const HARDNESS: u8 = 5;
const GATE_BLOW: i8 = 4;

/// Who strikes in a step: the player, or the thug (no party member).
#[derive(Clone, Copy)]
enum By {
    Player,
    Thug,
}

/// What strikes: a weapon dealing a fixed `n` before its wielder's
/// strength (a round's attack, no crit), a script's `n` points, or the
/// blast of a grenade of `spells.2da` row `row`.
#[derive(Clone, Copy)]
enum Blow {
    Weapon(i8),
    Script(u16),
    Grenade(u16),
}

/// A step: its blows (by whom, on what, with what) and seat 0's uses.
type Step = (&'static [(By, u32, Blow)], &'static [(u32, Verb)]);

const STEPS: [Step; 9] = [
    // 0: the player's blow on the locked gate lands, less its hardness.
    (&[(By::Player, GATE, Blow::Weapon(GATE_BLOW))], &[]),
    // 1: the thug's does nothing: only the party bashes.
    (&[(By::Thug, GATE, Blow::Weapon(GATE_BLOW))], &[]),
    // 2: a weapon on what is not locked, on what is plot, and short of
    // the vault's hardness: nothing.
    (
        &[
            (By::Player, CRATE, Blow::Weapon(10)),
            (By::Player, STATUE, Blow::Weapon(10)),
            (By::Player, VAULT, Blow::Weapon(10)),
        ],
        &[],
    ),
    // 3: a script's damage hurts the crate, not the statue.
    (
        &[
            (By::Player, CRATE, Blow::Script(4)),
            (By::Player, STATUE, Blow::Script(4)),
        ],
        &[],
    ),
    // 4: Min1HP holds the sealed box at 1.
    (&[(By::Player, SEALED, Blow::Weapon(100))], &[]),
    // 5: the gate breaks: open, unlocked, its death raised.
    (&[(By::Player, GATE, Blow::Weapon(GATE_BLOW))], &[]),
    // 6: a broken door does not close.
    (&[], &[(GATE, Verb::Close)]),
    // 7: nor is it hurt any more.
    (&[(By::Player, GATE, Blow::Weapon(GATE_BLOW))], &[]),
    // 8: a fragmentation grenade's blast breaks the safe.
    (&[(By::Player, SAFE, Blow::Grenade(FRAG))], &[]),
];

/// `spells.2da`'s fragmentation grenade.
const FRAG: u16 = 87;

/// Where the player stands: by the gate on its near side, on the ground's
/// own axes (metres).
const NEAR: [f32; 2] = [-1.2, 0.0];

fn floor_at([x, y]: [f32; 2]) -> [f32; 3] {
    crate::determinism::ground_point(x, y, 0.0)
}

/// A weapon whose every attack deals `n` before its wielder's strength,
/// never a critical.
fn weapon(n: i8) -> Weapon {
    Weapon {
        dice: 0,
        die: 0,
        crit_mult: 1,
        damage_types: damage_types::BLUDGEONING,
        damage_bonus: n,
        ..Weapon::UNARMED
    }
}

fn source(blow: Blow) -> Source {
    match blow {
        Blow::Weapon(n) => Source::Weapon {
            weapon: weapon(n),
            cadence_ms: crate::combat::damage::ROUND_MS as u32,
            share: 1000,
            area: false,
        },
        Blow::Script(n) => Source::Script {
            damage: n,
            damage_type: None,
            cast: false,
            shot: None,
            staged: true,
        },
        Blow::Grenade(row) => Source::Blast { row },
    }
}

/// The proving ground with its module changed by `edit`, the probe's
/// scripts, and one seat playing the dev soldier.
fn world(
    edit: impl FnOnce(&mut crate::module::ModuleData) -> Result<(), String>,
) -> Result<super::super::SimWorld, String> {
    world_for(crate::combat::Sheet::dev_soldier(), edit)
}

/// `world` with the seat playing `sheet`.
fn world_for(
    sheet: crate::combat::Sheet,
    edit: impl FnOnce(&mut crate::module::ModuleData) -> Result<(), String>,
) -> Result<super::super::SimWorld, String> {
    let ground = crate::determinism::proving_ground();
    let mut data = (**ground.module().ok_or("the ground has no module")?).clone();
    edit(&mut data)?;
    super::super::SimWorld::new(
        crate::WorldConfig {
            seed: 1,
            seats: 1,
            wanderers: 0,
            spawn: [0.0; 3],
            yaw: 0.0,
            sheet,
            guests: crate::Guests::Avatars,
        },
        ground.with_module(data).with_scripts(library()),
    )
    .map_err(|error| error.to_string())
}

/// A placeable tagged `tag` at `at` (locked or not) with `hp` hit points
/// and `toughness`, raising the probe's damaged and death scripts.
fn placeable(tag: &str, at: [f32; 2], locked: bool, hp: i32, toughness: Toughness) -> ObjectData {
    ObjectData {
        kind: ObjectKind::Placeable,
        tag: tag.into(),
        opens: if locked { Opens::Locked } else { Opens::Never },
        hit_points: Some((hp, hp)),
        toughness,
        scripts: hooks(),
        ..ObjectData::unknown(tag, floor_at(at), 0.0)
    }
}

fn hooks() -> Vec<(String, String)> {
    vec![
        ("OnDamaged".into(), "probe_damaged".into()),
        ("OnDeath".into(), "probe_broke".into()),
    ]
}

type Ran = (Vec<crate::Snapshot>, Vec<(usize, Notice)>);

fn scenario() -> Result<Ran, String> {
    use crate::{StepReason, TickInput};

    let mut world = world(|data| {
        let Some(Placement::Own(gate)) = data.placements.first_mut() else {
            return Err("the ground's first object is not its door".into());
        };
        gate.opens = Opens::Locked;
        gate.hit_points = Some(GATE_HP);
        gate.toughness = Toughness {
            hardness: HARDNESS,
            ..Toughness::default()
        };
        gate.scripts = hooks();
        let toughness = |hardness, plot, min_one_hp| Toughness {
            hardness,
            plot,
            min_one_hp,
        };
        data.placements.extend(
            [
                placeable("crate", [-2.0, -1.5], false, 10, Toughness::default()),
                placeable("statue", [-2.0, 1.5], true, 10, toughness(0, true, false)),
                placeable("vault", [-2.5, -2.0], true, 10, toughness(50, false, false)),
                placeable("sealed", [-2.5, 2.0], true, 5, toughness(0, false, true)),
                ObjectData::creature(
                    "thug",
                    "thug",
                    floor_at([-3.5, 0.0]),
                    crate::combat::Sheet::commoner(),
                ),
                placeable(
                    "safe",
                    [-3.0, -3.0],
                    true,
                    10,
                    toughness(HARDNESS, false, false),
                ),
            ]
            .map(Placement::Own),
        );
        Ok(())
    })?;
    // Who is who: the player, and each object by its index.
    let mut ids = world.world.query::<(
        &super::super::Id,
        Option<&super::super::Avatar>,
        Option<&super::super::Object>,
    )>();
    let player = ids
        .iter(&world.world)
        .find(|(_, avatar, _)| avatar.is_some())
        .map(|(id, ..)| id.0)
        .ok_or("no player")?;
    let objects: Vec<(u32, SimId)> = ids
        .iter(&world.world)
        .filter_map(|(id, _, object)| Some((object?.index, id.0)))
        .collect();
    let id_of = |index: u32| {
        objects
            .iter()
            .find(|(at, _)| *at == index)
            .map(|(_, id)| *id)
            .ok_or(format!("no object {index}"))
    };
    let mut snapshots = Vec::new();
    let mut notices = Vec::new();
    for (step, (blows, uses)) in STEPS.iter().enumerate() {
        let mut players = world
            .world
            .query::<(&super::super::Avatar, &mut super::super::Player)>();
        for (_, mut player) in players.iter_mut(&mut world.world) {
            let at = floor_at(NEAR);
            player.0.origin[0] = at[0] / collision::UNIT;
            player.0.origin[1] = at[1] / collision::UNIT;
        }
        for &(by, target, blow) in *blows {
            let hit = super::super::Hit {
                attacker: match by {
                    By::Player => player,
                    By::Thug => id_of(THUG)?,
                },
                target: id_of(target)?,
                source: source(blow),
                point: None,
                direction: None,
            };
            world.world.resource_mut::<super::super::Hits>().0.push(hit);
        }
        let mut input = TickInput::default();
        for &(object, verb) in *uses {
            input.act(Seat(0), Action::Use { object, verb });
        }
        snapshots.push(crate::step(
            &mut world,
            step as u64 + 1,
            &input,
            8,
            StepReason::Authority,
        ));
        notices.extend(
            world
                .take_notices()
                .into_iter()
                .map(|(_, notice)| (step, notice)),
        );
    }
    Ok((snapshots, notices))
}

/// How long the scripted bash may take (sim ms), stepped 100 ms at a time.
const BASH_WITHIN_MS: u64 = 30_000;

/// A bruiser whose heartbeat asks it to bash a locked box beside it
/// (`DoDoorAction(…, DOOR_ACTION_BASH)`, as `k_ai_master` does for a door
/// in its way), stepped until the box breaks: when it broke (sim ms), and
/// the box's state then.
fn scripted() -> Result<(Option<u64>, Option<ObjectState>), String> {
    let mut world = world(|data| {
        data.placements.extend(
            [
                placeable("box", [-2.0, -1.0], true, 10, Toughness::default()),
                ObjectData {
                    scripts: vec![("ScriptHeartbeat".into(), "probe_bash".into())],
                    ..ObjectData::creature(
                        "bruiser",
                        "bruiser",
                        floor_at([-3.5, -1.0]),
                        crate::combat::Sheet::commoner(),
                    )
                },
            ]
            .map(Placement::Own),
        );
        Ok(())
    })?;
    let input = crate::TickInput::default();
    let mut last = None;
    for tick in 1..=BASH_WITHIN_MS / 100 {
        let snapshot = crate::step(&mut world, tick, &input, 100, crate::StepReason::Authority);
        last = state(std::slice::from_ref(&snapshot), 0, BOX);
        if last.is_some_and(|state| state.lock == lock::UNLOCKED) {
            return Ok((Some(snapshot.time_ms), last));
        }
    }
    let report = &world
        .world
        .resource::<super::super::scripts::Scripts>()
        .report;
    Err(format!(
        "not broken: {last:?}; runs {:?} calls {:?} gaps {:?} actions {:?} faults {:?}",
        report.runs, report.calls, report.gaps, report.actions, report.faults
    ))
}

/// How long the player holds attack at the gate by hand (sim ms), stepped
/// 50 ms at a time.
const BY_HAND_MS: u64 = 6_000;

/// The player playing `sheet`, at the gate's near side facing its middle,
/// holding attack: what the gate lost to its own shots or punches, through
/// the sim's delivery (`fire`, `strikes`), not hits put in by hand.
fn by_hand(sheet: crate::combat::Sheet) -> Result<u16, String> {
    let mut gate_at = [0.0; 3];
    let mut world = world_for(sheet, |data| {
        let Some(Placement::Own(gate)) = data.placements.first_mut() else {
            return Err("the ground's first object is not its door".into());
        };
        gate.opens = Opens::Locked;
        gate.hit_points = Some(GATE_HP);
        gate_at = gate.position;
        Ok(())
    })?;
    let at = floor_at(NEAR);
    let mut players = world
        .world
        .query::<(&super::super::Avatar, &mut super::super::Player)>();
    for (_, mut player) in players.iter_mut(&mut world.world) {
        player.0.origin[0] = at[0] / collision::UNIT;
        player.0.origin[1] = at[1] / collision::UNIT;
        player.0.view_offset = [0; 3];
    }
    let eye = [at[0], at[1], at[2] + 1.6];
    let (pitch, yaw) = crate::combat::mind::aim(eye, [gate_at[0], gate_at[1], gate_at[2] + 1.2]);
    let mut input = crate::TickInput::default();
    input.insert(
        Seat(0),
        crate::Command {
            angles: crate::moves::view_angles(pitch, yaw),
            buttons: crate::buttons::ATTACK,
            ..crate::Command::default()
        },
    );
    let mut last = None;
    for tick in 1..=BY_HAND_MS / 50 {
        let snapshot = crate::step(&mut world, tick, &input, 50, crate::StepReason::Authority);
        last = state(std::slice::from_ref(&snapshot), 0, GATE);
    }
    last.map(|gate| gate.damage).ok_or("no gate".into())
}

/// An object's state after `step`.
fn state(snapshots: &[crate::Snapshot], step: usize, index: u32) -> Option<ObjectState> {
    snapshots
        .get(step)?
        .entities
        .iter()
        .find_map(|entity| match entity.kind {
            crate::Kind::Object(_, at) if at == index => entity.object,
            _ => None,
        })
}

/// The checks of bashing (see the module documentation). For the approved
/// test `bash_doors`.
pub fn probes() -> Vec<(&'static str, Result<String, String>)> {
    let check = |ok: bool, what: String| if ok { Ok(what) } else { Err(what) };
    let (snapshots, notices) = match scenario() {
        Ok(ran) => ran,
        Err(why) => return vec![("bash_scenario", Err(why))],
    };
    let again = scenario().map(|(snapshots, _)| snapshots.last().map(crate::Snapshot::hash));
    let lost = |step, index| state(&snapshots, step, index).map(|state| state.damage);
    let said = |step: usize, words: &str| {
        notices
            .iter()
            .filter(|(at, notice)| *at == step && *notice == Notice::Said(words.into()))
            .count()
    };
    // The dev soldier's strength (16: +3) adds to the weapon's; the gate's
    // hardness comes off; KOTOR's best chance to hit (95%), a round's pace
    // (×4) and bashing's own (`BASH_TIMES`) scale it.
    let first =
        ((i32::from(GATE_BLOW) + 3 - i32::from(HARDNESS)) * 95 * 4 * super::BASH_TIMES + 50) / 100;
    let gate = |step| state(&snapshots, step, GATE);
    let round_trip = snapshots
        .last()
        .is_some_and(|last| crate::Snapshot::decode(&last.encode()).as_ref() == Ok(last));
    vec![
        (
            "blow_less_hardness",
            check(
                lost(0, GATE) == Some(first as u16) && said(0, "damaged") == 1,
                format!(
                    "gate lost {:?} (expected {first}), {notices:?}",
                    lost(0, GATE)
                ),
            ),
        ),
        (
            "only_the_party_bashes",
            check(
                lost(1, GATE) == lost(0, GATE) && said(1, "damaged") == 0,
                format!("gate lost {:?} then {:?}", lost(0, GATE), lost(1, GATE)),
            ),
        ),
        (
            "unlocked_plot_hardness_hold",
            check(
                [CRATE, STATUE, VAULT].map(|index| lost(2, index)) == [Some(0); 3],
                format!("{:?}", [CRATE, STATUE, VAULT].map(|index| lost(2, index))),
            ),
        ),
        (
            "script_damage",
            check(
                lost(3, CRATE) == Some(4) && lost(3, STATUE) == Some(0),
                format!("crate {:?}, statue {:?}", lost(3, CRATE), lost(3, STATUE)),
            ),
        ),
        (
            "min_one_hp",
            check(
                lost(4, SEALED) == Some(4) && said(4, "broke") == 0,
                format!("sealed lost {:?}", lost(4, SEALED)),
            ),
        ),
        (
            "breaks_open",
            check(
                gate(5).is_some_and(|gate| gate.state != 0 && gate.lock == lock::UNLOCKED)
                    && said(5, "damaged") == 1
                    && said(5, "broke") == 1,
                format!("gate {:?}", gate(5)),
            ),
        ),
        (
            "stays_open",
            check(
                gate(6).is_some_and(|gate| gate.state != 0),
                format!("gate {:?}", gate(6)),
            ),
        ),
        (
            "broken_takes_nothing",
            check(
                lost(7, GATE) == lost(6, GATE) && said(7, "damaged") == 0,
                format!("gate lost {:?} then {:?}", lost(6, GATE), lost(7, GATE)),
            ),
        ),
        (
            "grenade_blast",
            check(
                state(&snapshots, 8, SAFE)
                    .is_some_and(|safe| safe.lock == lock::UNLOCKED && safe.state == look::DEAD)
                    && said(8, "broke") == 1,
                format!("safe {:?}", state(&snapshots, 8, SAFE)),
            ),
        ),
        (
            "scripted_bash",
            match scripted() {
                Ok((Some(at), state)) => Ok(format!("broke at {at} ms: {state:?}")),
                Ok((None, state)) => Err(format!("not broken: {state:?}")),
                Err(why) => Err(why),
            },
        ),
        (
            "shots_bash",
            match by_hand(crate::combat::Sheet::dev_soldier()) {
                Ok(lost) if lost > 0 => Ok(format!("the gate lost {lost} to blaster fire")),
                Ok(_) => Err("the blaster's bolts never hurt the gate".into()),
                Err(why) => Err(why),
            },
        ),
        (
            "fists_bash",
            match by_hand(crate::combat::Sheet {
                weapons: [None; 2],
                ..crate::combat::Sheet::dev_soldier()
            }) {
                Ok(lost) if lost > 0 => Ok(format!("the gate lost {lost} to fists")),
                Ok(_) => Err("the fists never hurt the gate".into()),
                Err(why) => Err(why),
            },
        ),
        (
            "round_trip",
            check(round_trip, "the last snapshot encodes and decodes".into()),
        ),
        (
            "deterministic",
            check(
                again == Ok(snapshots.last().map(crate::Snapshot::hash)),
                format!("{again:?}"),
            ),
        ),
    ]
}
