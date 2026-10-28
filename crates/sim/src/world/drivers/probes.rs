//! The probes of `drivers` (`kjk sim probes`).

use super::super::{Avatar, Fighting, Id, Player, SimWorld};
use super::*;
use crate::combat::fighter::Life;
use crate::combat::{Fighter, Sheet};
use crate::input::{Command, Seat, TickInput};
use crate::module::{ObjectData, ObjectKind, Placement};
use crate::motion::MotionState;
use crate::notice::{Notice, Screen};
use crate::probe::check;
use crate::script::actions::Goal;
use crate::script::{MODULE, Pending};
use crate::snapshot::{EntityState, Kind, SimId, Snapshot};
use crate::{Guests, Level, StepReason, WorldConfig};
use std::sync::Arc;

/// The companions' templates, in the party's joining order.
const COMPANIONS: [&str; 2] = ["probe_comp_a", "probe_comp_b"];

/// The party check's script (`pcs_script`).
const PCS: &str = "probe_pcs";
/// The first companion's end-of-round hook, and a story's script: each
/// moves the first companion to the second (`pcs_script`).
const AI: &str = "probe_ai";
const STORY: &str = "probe_story";
/// The first companion's heartbeat (an AI hook) and a story's script: each
/// signals the first companion's user-defined event 1001 to it, whose
/// script (`USER`) moves it as `AI` does.
const AI_SIGNAL: &str = "probe_ai_signal";
const STORY_SIGNAL: &str = "probe_story_signal";
const USER: &str = "probe_user";
/// A locked door's fail-to-open script: the global `LED` set to whether
/// the party's leader (`GetPartyMemberByIndex(0)`) is the first companion,
/// as the Endar Spire's first door asks whether Trask leads
/// (`guest_leads_its_acts`).
const LEADS: &str = "probe_leads";
const LED: &str = "probe_led";

/// The checks (`kjk sim probes`; see each part).
pub fn probes() -> Vec<(&'static str, Result<String, String>)> {
    let mut checks = layout();
    checks.extend(kept());
    checks.push(("hosted_save_companions", hosted_save()));
    checks.push(("standing_before_downed", standing_first()));
    checks.push(("benched_seat_chooses_first", benched_first()));
    checks.push(("own_ai_skipped_when_driven", own_ai()));
    checks.push(("leader_is_seat_0s_body", leader_body()));
    checks.push(("seat_bodies_agree", seat_bodies_agree()));
    checks.push(("leader_by_body_seat_0s_only", leader_by_body()));
    checks.push(("leader_keeps_the_view", leader_view()));
    checks.push(("leader_uses_from_its_body", leader_uses()));
    checks.push(("guest_leads_its_acts", guest_leads()));
    checks.extend(kept_for_seats());
    match scenario() {
        Ok(ran) => {
            checks.extend(assigned(&ran));
            checks.extend(arrival(&ran));
        }
        Err(why) => checks.push(("drivers_scenario", Err(why))),
    }
    checks
}

/// What a new world holds: under `Guests::Companions` seat 0's player
/// alone, the seats all the same; under `Guests::Avatars` a player each.
fn layout() -> Vec<(&'static str, Result<String, String>)> {
    [
        ("companions_hold_seats", Guests::Companions, 3, 1),
        ("avatars_hold_seats", Guests::Avatars, 2, 2),
    ]
    .into_iter()
    .map(|(name, guests, seats, players)| {
        let start = party_world(guests, seats).map(|(mut world, ..)| world.snapshot());
        let result = start.and_then(|start| {
            check(
                start.seats() == seats && start.avatars() == players,
                format!("{} seats held, {} players", start.seats(), start.avatars()),
            )
        });
        (name, result)
    })
    .collect()
}

/// A creature of the probes' party made from `template`, standing at `at`
/// (metres) as the dev soldier.
fn companion(template: &str, at: [f32; 3]) -> ObjectData {
    ObjectData::creature(template, template, at, Sheet::dev_soldier())
}

/// The probes' scripts (the party check, `PCS`) and the companions'
/// templates (what an install makes them again from).
fn library() -> crate::script::Library {
    let creatures = COMPANIONS
        .iter()
        .map(|template| {
            (
                template.to_string(),
                Arc::new(companion(template, [0.0; 3])),
            )
        })
        .collect();
    pcs_script().with_creatures(creatures)
}

/// The party check, run by the module (`PCS`): three message boxes for
/// every player, numbered `GetIsPC(GetPartyMemberByIndex(1))`,
/// `GetPartyMemberCount()` and `GetIsObjectValid(GetNextPC())` after
/// `GetFirstPC()`: 0, 3 and 0 when the scripts see one player and two
/// companions.
fn pcs_script() -> crate::script::Library {
    use fmt_odyssey::ncs::{Const, Op};
    const CATALOG: &str = r#"
// 0
void DisplayMessageBox(int nStrRef, string sIcon = "");
// 1
object GetPartyMemberByIndex(int nIndex);
// 2
int GetIsPC(object oCreature);
// 3
int GetPartyMemberCount();
// 4
object GetFirstPC();
// 5
object GetNextPC();
// 6
int GetIsObjectValid(object oObject);
// 7
object GetObjectByTag(string sTag, int nNth=0);
// 8
void ActionMoveToObject(object oMoveTo, int bRun=FALSE, float fRange=1.0f);
// 9
void SignalEvent(object oObject, event evToRun);
// 10
event EventUserDefined(int nUserDefinedEventNumber);
// 11
void SetGlobalNumber(string sIdentifier, int nValue);
"#;
    let call = |routine: u16, args: u8| Op::Action { routine, args };
    let icon = || Op::Const(Const::String(Vec::new()));
    let mut asm = script_ncs::probes::Asm::default();
    asm.op(icon())
        .op(Op::Const(Const::Int(1)))
        .op(call(1, 1))
        .op(call(2, 1))
        .op(call(0, 2))
        .op(icon())
        .op(call(3, 0))
        .op(call(0, 2))
        .op(icon())
        .op(call(4, 0))
        .op(Op::MovSp(-4))
        .op(call(5, 0))
        .op(call(6, 1))
        .op(call(0, 2))
        .op(Op::Retn);
    // `ActionMoveToObject(GetObjectByTag(the second companion), TRUE)`:
    // the first companion's end-of-round hook (`AI`), and a story's script
    // (`STORY`).
    let mut moves = script_ncs::probes::Asm::default();
    moves
        .op(Op::Const(Const::Float(1.0)))
        .op(Op::Const(Const::Int(1)))
        .op(Op::Const(Const::Int(0)))
        .op(Op::Const(Const::String(COMPANIONS[1].as_bytes().to_vec())))
        .op(call(7, 2))
        .op(call(8, 3))
        .op(Op::Retn);
    // `SignalEvent(OBJECT_SELF, EventUserDefined(1001))`.
    let mut signals = script_ncs::probes::Asm::default();
    signals
        .op(Op::Const(Const::Int(1001)))
        .op(call(10, 1))
        .op(Op::Const(Const::Object(0)))
        .op(call(9, 2))
        .op(Op::Retn);
    // `SetGlobalNumber(LED, GetPartyMemberByIndex(0) ==
    // GetObjectByTag(the first companion))`.
    let mut leads = script_ncs::probes::Asm::default();
    leads
        .op(Op::Const(Const::Int(0)))
        .op(call(1, 1))
        .op(Op::Const(Const::Int(0)))
        .op(Op::Const(Const::String(COMPANIONS[0].as_bytes().to_vec())))
        .op(call(7, 2))
        .op(Op::Equal(fmt_odyssey::ncs::Pair::ObjectObject))
        .op(Op::Const(Const::String(LED.as_bytes().to_vec())))
        .op(call(11, 2))
        .op(Op::Retn);
    crate::probe::library(
        CATALOG,
        [
            (PCS, asm.build()),
            (LEADS, leads.build()),
            (AI, moves.build()),
            (STORY, moves.build()),
            (USER, moves.build()),
            (AI_SIGNAL, signals.build()),
            (STORY_SIGNAL, signals.build()),
        ],
    )
}

/// The proving ground with the two companions standing 1.5 and 3 m ahead
/// of its entry, facing its way, and `library`: the level, and the
/// companions' object numbers.
fn ground() -> Result<(Level, [u32; 2]), String> {
    let ground = crate::determinism::proving_ground();
    let mut data = (**ground.module().ok_or("the ground has no module")?).clone();
    let entry = data.entry;
    let (sin, cos) = crate::math::sin_cos_deg(entry.yaw);
    let [x, y, z] = entry.position;
    let first = u32::try_from(data.placements.len()).map_err(|error| error.to_string())?;
    data.placements.extend(
        COMPANIONS
            .iter()
            .zip([1.5f32, 3.0])
            .map(|(template, ahead)| {
                let made = companion(template, [x + ahead * cos, y + ahead * sin, z]);
                // The first's end of round moves it (`AI`), its heartbeat
                // signals its user-defined event (`AI_SIGNAL`, `USER`).
                let scripts = if *template == COMPANIONS[0] {
                    vec![
                        ("ScriptEndRound".to_string(), AI.to_string()),
                        ("ScriptHeartbeat".to_string(), AI_SIGNAL.to_string()),
                        ("ScriptUserDefine".to_string(), USER.to_string()),
                    ]
                } else {
                    Vec::new()
                };
                Placement::Own(ObjectData {
                    yaw: entry.yaw,
                    scripts,
                    ..made
                })
            }),
    );
    Ok((
        ground.with_module(data).with_scripts(library()),
        [first, first + 1],
    ))
}

/// A world of `seats` on the ground, guests playing as `guests`, the two
/// companions in the party following seat 0, as `AddPartyMember` leaves
/// them (made again from its snapshot and script state, as a save loads):
/// the world, its level and the companions' ids in the party's order.
fn party_world(guests: Guests, seats: u8) -> Result<(SimWorld, Level, [SimId; 2]), String> {
    party_on(ground()?, guests, seats)
}

/// `party_world` on `ground` (a level and its companions' numbers).
fn party_on(
    (level, numbers): (Level, [u32; 2]),
    guests: Guests,
    seats: u8,
) -> Result<(SimWorld, Level, [SimId; 2]), String> {
    let config = WorldConfig {
        seed: 1,
        seats,
        wanderers: 0,
        spawn: [0.0; 3],
        yaw: 0.0,
        sheet: Sheet::dev_soldier(),
        guests,
    };
    let mut world = SimWorld::new(config, level.clone()).map_err(|error| error.to_string())?;
    let mut start = world.snapshot();
    let mut state = world.script_state().clone();
    let mut ids = [SimId(0); 2];
    for (npc, (number, template)) in (0u8..).zip(numbers.into_iter().zip(COMPANIONS)) {
        let entity = start
            .entities
            .iter_mut()
            .find(|entity| entity.kind == Kind::Object(ObjectKind::Creature, number))
            .ok_or(format!("no companion {template}"))?;
        let fighter = entity
            .fighter
            .as_mut()
            .ok_or("a companion without a fight")?;
        fighter.mind.leader = Some(Seat(0));
        fighter.faction = Some(crate::combat::sheet::factions::PLAYER);
        let slot = crate::script::party::Npc {
            template: template.to_string(),
            object: None,
            kept: None,
            template_data: None,
        };
        state.party.available.insert(npc, slot);
        if !state.party.join(npc, entity.id.0) {
            return Err(format!("{template} did not join"));
        }
        ids[usize::from(npc)] = entity.id;
    }
    let mut world =
        SimWorld::from_snapshot(&start, level.clone()).map_err(|error| error.to_string())?;
    world.set_script_state(state);
    Ok((world, level, ids))
}

/// A driver rides in the snapshot: set on a companion, it encodes and
/// decodes, a world restored from it is the same world with the companion
/// as seat 1's body, still an object; a driver past the last seat a world
/// holds is refused.
fn kept() -> Vec<(&'static str, Result<String, String>)> {
    let made = party_world(Guests::Companions, 3).map(|(mut world, level, [a, _])| {
        let mut start = world.snapshot();
        drive(&mut start, a, Seat(1));
        (start, level, a)
    });
    let (start, level, a) = match made {
        Ok(made) => made,
        Err(why) => return vec![("driver_kept", Err(why))],
    };
    let decoded = Snapshot::decode(&start.encode()).is_ok_and(|decoded| decoded == start);
    let restored = SimWorld::from_snapshot(&start, level.clone())
        .is_ok_and(|mut world| world.snapshot() == start);
    let body = start.body(Seat(1)).map(|entity| (entity.id, entity.kind));
    let mut past = start.clone();
    drive(&mut past, a, Seat(super::super::MAX_SEATS));
    let refused = SimWorld::from_snapshot(&past, level).err();
    vec![
        (
            "driver_kept",
            check(
                decoded
                    && restored
                    && body.is_some_and(|(id, kind)| id == a && matches!(kind, Kind::Object(..))),
                format!("seat 1's body {body:?}, decoded {decoded}, restored {restored}"),
            ),
        ),
        (
            "driver_past_max_refused",
            check(
                refused == Some(crate::InstallError::Seat(Seat(super::super::MAX_SEATS))),
                format!("a driver of seat {}: {refused:?}", super::super::MAX_SEATS),
            ),
        ),
    ]
}

/// A game saved alone (one seat, its two companions following), hosted
/// for three (I31): the session holds three seats, seat 0 plays the save's
/// own character, and seats 1 and 2 take the companions in the party's
/// order as they speak.
fn hosted_save() -> Result<String, String> {
    let (mut alone, level, ids) = party_world(Guests::Avatars, 1)?;
    let save = crate::Save {
        hz: crate::TICK_HZ,
        level: level.id(),
        snapshot: alone.snapshot(),
        script: alone.script_state().clone(),
    };
    let mut hosted = save.world(&level, 3, None)?;
    let host = save
        .snapshot
        .body(Seat(0))
        .map(|entity| entity.id)
        .ok_or("the save has no character of its own")?;
    let after = run(&mut hosted, &BOTH);
    let bodies: Vec<Option<SimId>> = (0..3)
        .map(|seat| after.body(Seat(seat)).map(|entity| entity.id))
        .collect();
    check(
        save.snapshot.seats() == 1
            && after.seats() == 3
            && bodies == [Some(host), Some(ids[0]), Some(ids[1])],
        format!(
            "saved for {} seat(s), hosted for {}: bodies {bodies:?} (the save's character {host:?}, companions {ids:?})",
            save.snapshot.seats(),
            after.seats()
        ),
    )
}

/// The view's held world (`Snapshot::holds_world`) counts the seats' bodies
/// as the sim's step does: with the second companion leading seat 0 and
/// seat 1 driving the first, the snapshot's `seat_bodies` are those
/// `scene::seat_bodies` makes of the avatars and the one lent.
fn seat_bodies_agree() -> Result<String, String> {
    let (mut world, _, [a, b]) = party_world(Guests::Companions, 2)?;
    scripts_of(&mut world).state.party.leader = Some(1);
    let after = run(&mut world, &[(0, 0), (1, 0)]);
    let lent = super::super::lent(&scripts_of(&mut world).state.party);
    let mut avatars = world.world.query::<(&Id, &Avatar)>();
    let seats: Vec<(Seat, SimId)> = avatars
        .iter(&world.world)
        .map(|(id, avatar)| (avatar.0, id.0))
        .collect();
    let mut sim = super::super::seat_bodies(seats, lent);
    let mut view = after.seat_bodies();
    sim.sort_by_key(|id| id.0);
    view.sort_by_key(|id| id.0);
    let ids: Vec<u32> = sim.iter().map(|id| id.0).collect();
    check(
        view == sim && ids.contains(&a.0) && ids.contains(&b.0),
        format!(
            "the sim's {ids:?}, the snapshot's {:?} (driven {a:?}, lent {b:?})",
            view.iter().map(|id| id.0).collect::<Vec<_>>()
        ),
    )
}

/// The second companion leading the party (`world::lent`): the snapshot
/// carries it as seat 0's body, which seat 0's run moves, through an
/// encoding; seat 0's own character stays in the party, and seat 1 drives
/// the first companion.
fn leader_body() -> Result<String, String> {
    let (mut world, _, [a, b]) = party_world(Guests::Companions, 2)?;
    scripts_of(&mut world).state.party.leader = Some(1);
    let before = run(&mut world, &[(0, 0), (1, 0)]);
    let after = run(&mut world, &[(0, 127), (1, 0)]);
    let decoded = Snapshot::decode(&after.encode()).is_ok_and(|decoded| decoded == after);
    let body = after.body(Seat(0)).map(|entity| entity.id);
    let own = after.own(Seat(0)).is_some_and(|own| after.in_party(own));
    let driven = after.body(Seat(1)).map(|entity| entity.id);
    let ran = moved(&before, &after, b);
    check(
        after.leading == Some(b)
            && body == Some(b)
            && decoded
            && own
            && driven == Some(a)
            && ran > 0.0,
        format!(
            "leading {:?}: seat 0's body {body:?} ran {ran:.3} m, its own in the party {own}; seat 1 drives {driven:?}; decoded {decoded}",
            after.leading
        ),
    )
}

/// A Use seat 0 sends while a companion leads is reached from the
/// companion's body, as the target block walks that body there: the
/// ground's door, 1.2 m from the leader and 4 m from the player's own
/// character, opens, and a chest beside the leader opens and stays held
/// (the reach was once measured from the player's own, so a leader walked
/// up to a door could not open it, nor keep a chest open).
fn leader_uses() -> Result<String, String> {
    /// The ground's door (`uses::probes`' gate), and where the leader, the
    /// player's own and the chest stand (metres from its doorway's middle).
    const GATE: u32 = 0;
    const NEAR: [f32; 2] = [-1.2, 0.0];
    const FAR: [f32; 2] = [-4.0, 0.0];
    const CHEST_AT: [f32; 2] = [-1.2, 1.0];
    let (level, numbers) = ground()?;
    let mut data = (**level.module().ok_or("the ground has no module")?).clone();
    let chest = u32::try_from(data.placements.len()).map_err(|error| error.to_string())?;
    let [x, y] = CHEST_AT;
    data.placements.push(Placement::Own(ObjectData {
        kind: ObjectKind::Placeable,
        container: true,
        useable: true,
        ..ObjectData::unknown(
            "probe_chest",
            crate::determinism::ground_point(x, y, 0.0),
            0.0,
        )
    }));
    let (mut world, _, [_, b]) =
        party_on((level.with_module(data), numbers), Guests::Companions, 1)?;
    scripts_of(&mut world).state.party.leader = Some(1);
    let own = world.snapshot().own(Seat(0)).map(|entity| entity.id);
    let own = own.ok_or("seat 0 has no own character")?;
    for (id, [x, y]) in [(b, NEAR), (own, FAR)] {
        let at = crate::determinism::ground_point(x, y, 0.0);
        poke(&mut world, id, |_, ps| {
            ps.origin[0] = at[0] / collision::UNIT;
            ps.origin[1] = at[1] / collision::UNIT;
        });
    }
    let mut open = |object: u32| {
        let mut input = TickInput::default();
        input.insert(Seat(0), command(0));
        let verb = crate::action::Verb::Open;
        input.act(Seat(0), crate::action::Action::Use { object, verb });
        let tick = world.tick() + 1;
        crate::step(&mut world, tick, &input, 8, StepReason::Authority)
    };
    let state = |snapshot: &Snapshot, kind, index| {
        snapshot
            .entities
            .iter()
            .find(|entity| entity.kind == Kind::Object(kind, index))
            .and_then(|entity| entity.object)
    };
    let after = open(GATE);
    let gate = state(&after, ObjectKind::Door, GATE);
    open(chest);
    let held = state(&run(&mut world, &[(0, 0)]), ObjectKind::Placeable, chest);
    check(
        after.leading == Some(b)
            && gate.is_some_and(|gate| gate.state != 0)
            && held.is_some_and(|chest| chest.tenants & 1 != 0),
        format!(
            "leading {:?}; the door {gate:?}; the chest a step after opening {held:?}",
            after.leading
        ),
    )
}

/// A guest's act is its companion's as its player controls it: the
/// ground's door, locked, its fail-to-open script asking who leads
/// (`LEADS`), used by seat 1 driving the first companion sees that
/// companion lead (KOTOR's player uses a door with the character it
/// controls, the party's leader: the Endar Spire's first door opens for
/// Trask leading, and a guest playing Trask could not open it); used by
/// seat 0, its own character leads; the party's leader is the same after
/// both.
fn guest_leads() -> Result<String, String> {
    /// The ground's door (`uses::probes`' gate), and where the companion
    /// and seat 0's own stand by turns (metres from its doorway's middle).
    const GATE: u32 = 0;
    const NEAR: [f32; 2] = [-1.2, 0.0];
    const FAR: [f32; 2] = [-4.0, 0.0];
    let (level, numbers) = ground()?;
    let mut data = (**level.module().ok_or("the ground has no module")?).clone();
    let Some(Placement::Own(gate)) = data.placements.get_mut(GATE as usize) else {
        return Err("the ground's door is not its first placement".into());
    };
    gate.opens = crate::module::Opens::Locked;
    gate.scripts = vec![("OnFailToOpen".into(), LEADS.into())];
    let (mut world, _, [a, _]) =
        party_on((level.with_module(data), numbers), Guests::Companions, 2)?;
    let driven = run(&mut world, &[(0, 0), (1, 0)])
        .body(Seat(1))
        .map(|e| e.id);
    let own = world.snapshot().own(Seat(0)).map(|entity| entity.id);
    let own = own.ok_or("seat 0 has no own character")?;
    let place = |world: &mut SimWorld, near: SimId, far: SimId| {
        for (id, [x, y]) in [(near, NEAR), (far, FAR)] {
            let at = crate::determinism::ground_point(x, y, 0.0);
            poke(world, id, |_, ps| {
                ps.origin[0] = at[0] / collision::UNIT;
                ps.origin[1] = at[1] / collision::UNIT;
            });
        }
    };
    // Seat `seat` uses the door: what its script saw, and who leads after.
    let tries = |world: &mut SimWorld, seat: u8| {
        scripts_of(world).state.numbers.remove(LED);
        let mut input = TickInput::default();
        for seat in 0..2 {
            input.insert(Seat(seat), command(0));
        }
        let verb = crate::action::Verb::Open;
        input.act(
            Seat(seat),
            crate::action::Action::Use { object: GATE, verb },
        );
        let tick = world.tick() + 1;
        crate::step(world, tick, &input, 8, StepReason::Authority);
        let after = run(world, &[(0, 0), (1, 0)]);
        let led = scripts_of(world).state.numbers.get(LED).copied();
        (led, after.leading)
    };
    place(&mut world, a, own);
    let guest = tries(&mut world, 1);
    place(&mut world, own, a);
    let host = tries(&mut world, 0);
    check(
        driven == Some(a) && guest == (Some(1), None) && host == (Some(0), None),
        format!(
            "seat 1 drives {driven:?} ({a:?}): its use saw the companion lead {:?}, leading after {:?}; seat 0's {:?}, after {:?}",
            guest.0, guest.1, host.0, host.1
        ),
    )
}

/// A portrait click hands the first seat's view on: the companion made
/// leader looks the way its player looked (not where its own aim, a
/// half turn away, would turn it), and the player's own character, leading
/// again, the way the companion looked; the seat's commands turn each from
/// there.
fn leader_view() -> Result<String, String> {
    let (mut world, _, [_, b]) = party_world(Guests::Companions, 1)?;
    let own = world.snapshot().own(Seat(0)).map(|entity| entity.id);
    let own = own.ok_or("seat 0 has no own character")?;
    // The companion aimed half a turn round, through its own commands.
    poke(&mut world, b, |_, ps| {
        ps.command_angles = crate::moves::view_angles(0.0, 90.0);
        ps.set_view([0.0, 180.0, 0.0]);
    });
    let mut step = |yaw: f32, lead: Option<SimId>| {
        let mut input = TickInput::default();
        input.insert(
            Seat(0),
            Command {
                angles: crate::moves::view_angles(0.0, yaw),
                ..Command::default()
            },
        );
        if let Some(body) = lead {
            input.act(Seat(0), crate::action::Action::Lead(body));
        }
        let tick = world.tick() + 1;
        crate::step(&mut world, tick, &input, 8, StepReason::Authority)
    };
    let yaw = |snapshot: &Snapshot, id: SimId| {
        snapshot
            .get(id)
            .and_then(|entity| entity.player)
            .map(|ps| ps.view[1])
    };
    let looked = yaw(&step(30.0, None), own);
    step(30.0, Some(b));
    let led = step(30.0, None);
    let turned = yaw(&step(40.0, None), b);
    step(40.0, Some(own));
    let back = yaw(&step(40.0, None), own);
    // Degrees from `a` to `b`, the short way.
    let apart = |a: Option<f32>, b: Option<f32>| {
        a.zip(b).map_or(f32::MAX, |(a, b)| {
            ((b - a + 540.0).rem_euclid(360.0) - 180.0).abs()
        })
    };
    let leading = yaw(&led, b);
    check(
        led.leading == Some(b)
            && apart(looked, leading) < 0.5
            && (apart(leading, turned) - 10.0).abs() < 0.5
            && apart(turned, back) < 0.5,
        format!(
            "own looked {looked:?}; the companion leading {leading:?}, turned by 10 to {turned:?}; own again {back:?}"
        ),
    )
}

/// The leader picked by its body (`Action::Lead`, the HUD's portrait
/// click): seat 0 makes the second companion its body; a guest's pick (the
/// companion it drives, or seat 0's own) changes nothing; seat 0 picking
/// its own character leads again.
fn leader_by_body() -> Result<String, String> {
    let (mut world, _, [a, b]) = party_world(Guests::Companions, 2)?;
    let mut lead = |seat: u8, body: SimId| {
        let mut input = TickInput::default();
        for seat in 0..2 {
            input.insert(Seat(seat), command(0));
        }
        input.act(Seat(seat), crate::action::Action::Lead(body));
        let tick = world.tick() + 1;
        crate::step(&mut world, tick, &input, 8, StepReason::Authority)
    };
    let picked = lead(0, b);
    let own = picked.own(Seat(0)).map(|entity| entity.id);
    let guest = [lead(1, a).leading, own.and_then(|own| lead(1, own).leading)];
    let back = own.map(|own| lead(0, own).leading);
    check(
        picked.leading == Some(b) && guest == [Some(b); 2] && back == Some(None),
        format!(
            "seat 0 picked {b:?}: leading {:?}; a guest's picks of {a:?} and {own:?}: {guest:?}; seat 0 its own again: {back:?}",
            picked.leading
        ),
    )
}

/// The first companion down: a guest joining takes the second, standing;
/// with the second down too, the first.
fn standing_first() -> Result<String, String> {
    let (mut world, _, [a, b]) = party_world(Guests::Companions, 2)?;
    let down = |fighter: &mut Fighter, _: &mut MotionState| fighter.life = Life::Downed;
    poke(&mut world, a, down);
    let first = run(&mut world, &[(0, 0), (1, 0)])
        .body(Seat(1))
        .map(|e| e.id);
    let (mut world, _, [a, b2]) = party_world(Guests::Companions, 2)?;
    poke(&mut world, a, down);
    poke(&mut world, b2, down);
    let all_down = run(&mut world, &[(0, 0), (1, 0)])
        .body(Seat(1))
        .map(|e| e.id);
    check(
        first == Some(b) && all_down == Some(a),
        format!("one down: seat 1 drives {first:?} (standing {b:?}); both down: {all_down:?}"),
    )
}

/// Seat 2 drives the first companion, seat 1 watches (the second is out of
/// the party); the first leaves the party as the second joins: seat 2,
/// benched, takes the second before seat 1 does.
fn benched_first() -> Result<String, String> {
    let (mut world, _, [a, b]) = party_world(Guests::Companions, 3)?;
    let follows = |leader: Option<Seat>| {
        move |fighter: &mut Fighter, _: &mut MotionState| fighter.mind.leader = leader
    };
    poke(&mut world, b, follows(None));
    scripts_of(&mut world).state.party.leave(1);
    run(&mut world, &[(0, 0), (2, 0)]);
    let before = run(&mut world, &[(0, 0), (1, 0), (2, 0)]);
    poke(&mut world, a, follows(None));
    poke(&mut world, b, follows(Some(Seat(0))));
    let party = &mut scripts_of(&mut world).state.party;
    party.leave(0);
    if !party.join(1, b.0) {
        return Err("the second companion did not join again".into());
    }
    let after = run(&mut world, &[(0, 0), (1, 0), (2, 0)]);
    let bodies = |snapshot: &Snapshot| [1, 2].map(|seat| snapshot.body(Seat(seat)).map(|e| e.id));
    let (before, after) = (bodies(&before), bodies(&after));
    check(
        before == [None, Some(a)] && after == [None, Some(b)],
        format!("seats 1 and 2 drive {before:?}, then {after:?}"),
    )
}

/// Seat 1 drives the first companion and runs. Its own end-of-round hook
/// (`AI`) moving it queues nothing, and it runs on with its seat; the same
/// move from a story's script (`STORY`) steers it. So for the user-defined
/// event its own heartbeat signals it (`AI_SIGNAL`), and the same event a
/// story's script signals (`STORY_SIGNAL`).
fn own_ai() -> Result<String, String> {
    let mut seen = Vec::new();
    for script in [AI, STORY, AI_SIGNAL, STORY_SIGNAL] {
        let (mut world, _, [a, _]) = party_world(Guests::Companions, 2)?;
        let start = run(&mut world, &BOTH[..2]);
        scripts_of(&mut world)
            .state
            .pending
            .push(Pending::new(script, script_ncs::vm::ObjectId(a.0)));
        let mut last = run(&mut world, &BOTH[..2]);
        let queued = world
            .script_state()
            .actions
            .get(&a.0)
            .is_some_and(|queue| !queue.is_empty());
        for _ in 0..30 {
            last = run(&mut world, &BOTH[..2]);
        }
        let steered = last
            .get(a)
            .and_then(|entity| entity.fighter)
            .is_some_and(|fighter| fighter.mind.steered);
        let driven = last.body(Seat(1)).map(|entity| entity.id) == Some(a);
        let ran = moved(&start, &last, a);
        let wanted = script == STORY || script == STORY_SIGNAL;
        if !driven || queued != wanted || steered != wanted || (!wanted && ran < 0.8) {
            return Err(format!(
                "{script}: driven {driven}, queued {queued}, steered {steered}, ran {ran:.2} m"
            ));
        }
        seen.push(format!("{script}: queued {queued}, ran {ran:.2} m"));
    }
    check(true, seen.join("; "))
}

/// Seats 1 and 2 drive the two companions, seat 3 watches; then, as each
/// case has it, seats go silent and come back (`KEEP_MS`):
/// - seat 1 away 10 steps: seat 3 does not take its companion, and seat 1
///   takes it again;
/// - both away, seat 2 back first: each takes its own;
/// - both away, the party's order turned about (a cutscene adding them
///   again), seat 1 back first: its own;
/// - seat 1 away: seat 3 takes its companion as the window ends, not
///   before.
///
/// Away, the planning counts the seats kept for (`players_driving`).
fn kept_for_seats() -> Vec<(&'static str, Result<String, String>)> {
    const AWAY: usize = 10;
    let window = usize::try_from(KEEP_MS / 8).unwrap_or(usize::MAX);
    let ids = match party_world(Guests::Companions, 1) {
        Ok((_, _, ids)) => ids,
        Err(why) => return vec![("kept_scenario", Err(why))],
    };
    let [a, b] = ids.map(Some);
    let driving = [a, b, None];
    let cases: [(&str, &[u8], u8, bool, usize); 4] = [
        ("blip_keeps_companion", &[1], 1, false, AWAY),
        ("back_in_any_order", &[1, 2], 2, false, AWAY),
        ("cutscene_order_kept", &[1, 2], 1, true, AWAY),
        ("kept_for_the_window", &[1], 1, false, window + 2),
    ];
    cases
        .into_iter()
        .map(|(name, away, back, reorder, steps)| {
            let ran = kept_case(away, back, reorder, steps).and_then(|seen| {
                let (wanted_during, wanted_after, changes) = if steps > AWAY {
                    // Seat 3 takes the first companion as the window ends;
                    // seat 1, back, watches.
                    ([None, b, None], [None, b, a], Some(window - 1))
                } else {
                    let during = if away.len() == 1 {
                        [None, b, None]
                    } else {
                        [None; 3]
                    };
                    (during, driving, None)
                };
                check(
                    seen.before == driving
                        && seen.during == wanted_during
                        && seen.counted == 3
                        && seen.changed == changes
                        && seen.after == wanted_after,
                    format!(
                        "before {:?}, away {:?} (planning counts {}), changed at step {:?}, after {:?}",
                        seen.before, seen.during, seen.counted, seen.changed, seen.after
                    ),
                )
            });
            (name, ran)
        })
        .collect()
}

/// What `kept_case` saw: seats 1–3's bodies before, while away (and the
/// players planning counts then), the step away they changed at, if they
/// did, and after.
struct KeptSeen {
    before: [Option<SimId>; 3],
    during: [Option<SimId>; 3],
    counted: u8,
    changed: Option<usize>,
    after: [Option<SimId>; 3],
}

/// Seats 1 and 2 drive, seat 3 watches; seats `away` are silent `steps`
/// steps (the party's order turned about first with `reorder`), then
/// `back` speaks a step before the rest.
fn kept_case(away: &[u8], back: u8, reorder: bool, steps: usize) -> Result<KeptSeen, String> {
    let (mut world, _, [a, _]) = party_world(Guests::Companions, 4)?;
    let speak = |world: &mut SimWorld, seats: &[u8]| {
        let seats: Vec<(u8, i8)> = seats.iter().map(|seat| (*seat, 0)).collect();
        run(world, &seats)
    };
    let bodies = |snapshot: &Snapshot| {
        [1, 2, 3].map(|seat| snapshot.body(Seat(seat)).map(|entity| entity.id))
    };
    let all = [0, 1, 2, 3];
    let before = bodies(&speak(&mut world, &all));
    if reorder {
        let party = &mut scripts_of(&mut world).state.party;
        party.leave(0);
        if !party.join(0, a.0) {
            return Err("the first companion did not join again".into());
        }
    }
    let present: Vec<u8> = all
        .into_iter()
        .filter(|seat| !away.contains(seat))
        .collect();
    let first = speak(&mut world, &present);
    let (during, counted) = (bodies(&first), first.players_driving());
    let mut changed = None;
    for step in 1..steps {
        if changed.is_none() && bodies(&speak(&mut world, &present)) != during {
            changed = Some(step);
        }
    }
    let mut returning = present.clone();
    returning.push(back);
    speak(&mut world, &returning);
    let after = bodies(&speak(&mut world, &all));
    Ok(KeptSeen {
        before,
        during,
        counted,
        changed,
        after,
    })
}

/// `id`'s mind driven by `seat` in `snapshot`.
fn drive(snapshot: &mut Snapshot, id: SimId, seat: Seat) {
    let fighter = snapshot
        .entities
        .iter_mut()
        .find(|entity| entity.id == id)
        .and_then(|entity| entity.fighter.as_mut());
    if let Some(fighter) = fighter {
        fighter.mind.driver = Some(seat);
    }
}

/// What `scenario` saw.
struct Ran {
    /// The snapshot after each tick, the first after tick 1.
    snapshots: Vec<Snapshot>,
    /// The message boxes the party check (`PCS`) showed seat 0.
    boxes: Vec<i32>,
    /// Whether seat 2's companion still carried `Avatar` after tick 52.
    kept_avatar: bool,
    level: Level,
    /// The companions, in the party's order.
    ids: [SimId; 2],
}

/// Seat 0 standing, seat 1 running, seat 2 standing.
const BOTH: [(u8, i8); 3] = [(0, 0), (1, 127), (2, 0)];

/// A command looking along +X (the view a body keeps when it is taken),
/// running forward or standing.
fn command(forward: i8) -> Command {
    Command {
        angles: crate::moves::view_angles(0.0, 0.0),
        forward,
        ..Command::default()
    }
}

/// One step of 8 ms with a command for each of `seats` (seat, forward).
fn run_as(world: &mut SimWorld, seats: &[(u8, i8)], reason: StepReason) -> Snapshot {
    let mut input = TickInput::default();
    for &(seat, forward) in seats {
        input.insert(Seat(seat), command(forward));
    }
    let tick = world.tick() + 1;
    crate::step(world, tick, &input, 8, reason)
}

fn run(world: &mut SimWorld, seats: &[(u8, i8)]) -> Snapshot {
    run_as(world, seats, StepReason::Authority)
}

/// `id`'s fight and movement changed between steps, as a script or a blow
/// would change them.
fn poke(world: &mut SimWorld, id: SimId, change: impl Fn(&mut Fighter, &mut MotionState)) {
    let mut actors = world.world.query::<(&Id, &mut Fighting, &mut Player)>();
    for (who, mut fighting, mut player) in actors.iter_mut(&mut world.world) {
        if who.0 == id {
            change(&mut fighting.0, &mut player.0);
        }
    }
}

fn scripts_of(world: &mut SimWorld) -> Mut<'_, super::super::scripts::Scripts> {
    world.world.resource_mut::<super::super::scripts::Scripts>()
}

/// How far `id` moved across the floor from `from` to `to` (metres).
fn moved(from: &Snapshot, to: &Snapshot, id: SimId) -> f32 {
    match (from.get(id), to.get(id)) {
        (Some(a), Some(b)) => {
            let (dx, dy) = (b.position[0] - a.position[0], b.position[1] - a.position[1]);
            (dx * dx + dy * dy).sqrt()
        }
        _ => f32::MAX,
    }
}

/// The ground's right of its entry's way, a unit vector.
fn right_of_entry(level: &Level) -> [f32; 3] {
    let yaw = level.module().map_or(0.0, |module| module.entry.yaw);
    let (sin, cos) = crate::math::sin_cos_deg(yaw);
    [sin, -cos, 0.0]
}

/// The party world of three seats, guests driving companions, stepped (the
/// cases name the ticks):
/// - tick 1: seat 0 alone;
/// - ticks 2–31: seats 1 and 2 too, seat 1 running; the party check
///   (`PCS`) runs at tick 30;
/// - ticks 32–51: the same, with a scripted move on seat 1's companion (no
///   route on the ground: its mind stands it still, and it slows to a stop
///   while seat 1 runs);
/// - tick 52: seat 2's companion moved 5.5 m aside first, seats 1 and 2
///   silent;
/// - tick 53: seats 2, 1 and 5 (past the three held) back, in that order;
/// - tick 54: seat 2's companion dead first;
/// - tick 55: seat 1's companion out of the party first.
fn scenario() -> Result<Ran, String> {
    let (mut world, level, ids) = party_world(Guests::Companions, 3)?;
    let [a, b] = ids;
    let mut snapshots = vec![run(&mut world, &[(0, 0)])];
    let mut boxes = Vec::new();
    for tick in 2..=51 {
        if tick == 30 {
            let check = Pending::new(PCS, MODULE);
            scripts_of(&mut world).state.pending.push(check);
        }
        if tick >= 32 {
            // A scripted move 3 m off (`ActionMoveToPoint`'s goal).
            let at = snapshots
                .last()
                .and_then(|snapshot| snapshot.get(a))
                .and_then(|entity| entity.player)
                .map_or([0.0; 3], |ps| ps.origin);
            let goal = Goal {
                point: [at[0] - 120.0, at[1], at[2]],
                range: 0.0,
                run: true,
            };
            scripts_of(&mut world).state.goals.insert(a.0, goal);
        }
        snapshots.push(run(&mut world, &BOTH));
        let shown = world.take_notices().into_iter();
        boxes.extend(shown.filter_map(|(seat, notice)| match notice {
            Notice::Show(Screen::MessageBox { strref }) if seat == Seat(0) => Some(strref),
            _ => None,
        }));
    }
    let right = right_of_entry(&level);
    poke(&mut world, b, |_, ps| {
        ps.origin = crate::math::mad(ps.origin, 5.5 / collision::UNIT, right);
    });
    snapshots.push(run(&mut world, &[(0, 0)]));
    let kept_avatar = world
        .world
        .query_filtered::<&Id, With<Avatar>>()
        .iter(&world.world)
        .any(|id| id.0 == b);
    snapshots.push(run(&mut world, &[(0, 0), (2, 0), (1, 127), (5, 0)]));
    poke(&mut world, b, |fighter, _| fighter.life = Life::Dead);
    snapshots.push(run(&mut world, &BOTH));
    poke(&mut world, a, |fighter, _| fighter.mind.leader = None);
    scripts_of(&mut world).state.party.leave(0);
    snapshots.push(run(&mut world, &BOTH));
    Ok(Ran {
        snapshots,
        boxes,
        kept_avatar,
        level,
        ids,
    })
}

/// Who drives what along the scenario, `Guests::Avatars` as before, a
/// client's prediction, and the same run twice.
fn assigned(ran: &Ran) -> Vec<(&'static str, Result<String, String>)> {
    let [a, b] = ran.ids;
    let at = |tick: usize| &ran.snapshots[tick - 1];
    let bodies =
        |tick: usize| [1, 2, 5].map(|seat| at(tick).body(Seat(seat)).map(|entity| entity.id));
    let drivers = |tick: usize| {
        ran.ids.map(|id| {
            at(tick)
                .get(id)
                .and_then(|entity| entity.fighter?.mind.driver)
        })
    };
    let objects = |tick: usize| {
        ran.ids.iter().all(|id| {
            at(tick)
                .get(*id)
                .is_some_and(|entity| matches!(entity.kind, Kind::Object(..)))
        })
    };
    let walking = at(52)
        .get(b)
        .and_then(|entity| entity.fighter)
        .is_some_and(|fighter| fighter.mind.walker.is_some());
    let (ran_m, stood_m) = (moved(at(1), at(31), a), moved(at(1), at(31), b));
    // Seat 1's companion's speed across the floor (m/s).
    let speed = |tick: usize| {
        at(tick).get(a).map_or(f32::MAX, |entity| {
            entity.velocity[0].hypot(entity.velocity[1])
        })
    };
    let last = ran.snapshots.last().map(Snapshot::hash);
    let again = scenario().map(|again| again.snapshots.last().map(Snapshot::hash));
    vec![
        (
            "watching_without_command",
            check(
                bodies(1) == [None; 3],
                format!("bodies of seats 1, 2, 5: {:?}", bodies(1)),
            ),
        ),
        (
            "takes_first_free",
            check(
                bodies(2) == [Some(a), Some(b), None] && objects(2),
                format!("bodies {:?}, drivers {:?}", bodies(2), drivers(2)),
            ),
        ),
        (
            "command_moves_it",
            check(
                ran_m >= 0.4 && stood_m < 0.1,
                format!("seat 1's companion ran {ran_m:.2} m, seat 2's moved {stood_m:.2} m"),
            ),
        ),
        (
            "dropped_mind_thinks",
            check(
                drivers(52) == [None, None] && !ran.kept_avatar && walking,
                format!(
                    "drivers {:?}, Avatar kept {}, walking back {walking}",
                    drivers(52),
                    ran.kept_avatar
                ),
            ),
        ),
        (
            "back_in_seat_order",
            check(
                bodies(53) == [Some(a), Some(b), None],
                format!("bodies of seats 1, 2, 5: {:?}", bodies(53)),
            ),
        ),
        (
            "dead_body_released",
            check(
                bodies(54) == [Some(a), None, None] && drivers(54)[1].is_none(),
                format!("bodies {:?}, drivers {:?}", bodies(54), drivers(54)),
            ),
        ),
        (
            "leaving_party_clears",
            check(
                bodies(55) == [None; 3] && drivers(55) == [None, None],
                format!("bodies {:?}, drivers {:?}", bodies(55), drivers(55)),
            ),
        ),
        (
            "script_goal_steers",
            check(
                speed(51) < speed(31) / 2.0,
                format!(
                    "seat 1 runs; its companion, given a scripted move, slowed from {:.2} to {:.2} m/s",
                    speed(31),
                    speed(51)
                ),
            ),
        ),
        (
            "scripts_see_one_pc",
            check(
                ran.boxes == [0, 3, 0],
                format!(
                    "GetIsPC(member 1), GetPartyMemberCount, GetNextPC valid: {:?}",
                    ran.boxes
                ),
            ),
        ),
        ("avatars_as_today", as_today()),
        ("predicts_as_authority", predicted(ran)),
        ("facing_kept_by_moves", faced_kept(ran)),
        (
            "same_every_run",
            check(
                again.as_ref().is_ok_and(|hash| *hash == last),
                format!("{last:?} then {again:?}"),
            ),
        ),
    ]
}

/// `Guests::Avatars` as before: seat 1 plays its own player, and no
/// companion takes a driver, however seat 1 runs.
fn as_today() -> Result<String, String> {
    let (mut world, _, [a, _]) = party_world(Guests::Avatars, 2)?;
    let first = run(&mut world, &[(0, 0), (1, 0)]);
    let mut last = first.clone();
    for _ in 0..10 {
        last = run(&mut world, &[(0, 0), (1, 127)]);
    }
    let body = last.body(Seat(1)).map(|entity| entity.kind);
    let driven = last
        .entities
        .iter()
        .any(|entity| entity.fighter.is_some_and(|f| f.mind.driver.is_some()));
    let stood = moved(&first, &last, a);
    check(
        body == Some(Kind::Avatar(Seat(1))) && !driven && stood < 0.1,
        format!(
            "seat 1's body {body:?}, a companion driven {driven}, the first moved {stood:.2} m"
        ),
    )
}

/// A client's prediction of a driven companion: from the snapshot after
/// `tick`, the next step predicted (no drivers assigned, no minds) leaves
/// it as the authority's step does: after tick 31 moved by its seat's
/// command, after tick 45 by its mind's (its scripted move, which the
/// authority's scripts still hold and the snapshot says steers it), all but
/// the mind's own bookkeeping (its walk's timers), which only the authority
/// keeps, as for every creature.
fn predicted(ran: &Ran) -> Result<String, String> {
    let mut seen = Vec::new();
    for (tick, steered) in [(31, false), (45, true)] {
        let start = &ran.snapshots[tick - 1];
        let a = ran.ids[0];
        let stepped = |reason: StepReason| -> Result<Option<EntityState>, String> {
            let mut world = SimWorld::from_snapshot(start, ran.level.clone())
                .map_err(|error| error.to_string())?;
            if steered {
                let at = start
                    .get(a)
                    .and_then(|e| e.player)
                    .map_or([0.0; 3], |ps| ps.origin);
                let goal = Goal {
                    point: [at[0] - 120.0, at[1], at[2]],
                    range: 0.0,
                    run: true,
                };
                scripts_of(&mut world).state.goals.insert(a.0, goal);
            }
            let next = run_as(&mut world, &BOTH, reason);
            Ok(next.get(a).copied())
        };
        let authority = stepped(StepReason::Authority)?;
        let mut predicted = stepped(StepReason::Predict)?;
        if steered
            && let (Some(mine), Some(theirs)) = (
                predicted.as_mut().and_then(|e| e.fighter.as_mut()),
                authority.and_then(|e| e.fighter),
            )
        {
            mine.mind = theirs.mind;
        }
        let (predicted, authority) = (
            predicted.as_ref().map(Snapshot::record),
            authority.as_ref().map(Snapshot::record),
        );
        let marked = start
            .get(a)
            .and_then(|e| e.fighter)
            .is_some_and(|f| f.mind.steered);
        if predicted.is_none() || predicted != authority || marked != steered {
            return Err(format!(
                "after tick {tick}: steered {marked} (wanted {steered}), predicted as the authority {}",
                predicted == authority
            ));
        }
        seen.push(tick);
    }
    Ok(format!(
        "seat 1's companion the same either way after ticks {seen:?}: moved by its seat, then steered by its script"
    ))
}

/// A facing given from outside the commands (a line's speaker turned to
/// the player, a script's `SetFacing`) is kept by the next move, steered
/// or predicted, on the angles' grid: what a client repeating the last
/// command shows is what the authority faced it to.
fn faced_kept(ran: &Ran) -> Result<String, String> {
    let (a, start) = (ran.ids[0], &ran.snapshots[44]);
    let mut world =
        SimWorld::from_snapshot(start, ran.level.clone()).map_err(|error| error.to_string())?;
    let toward = start
        .get(a)
        .and_then(|e| e.player)
        .map_or(0.0, |ps| ps.view[1])
        + 37.3;
    let faced = crate::math::short_to_degrees(crate::math::degrees_to_short(toward));
    poke(&mut world, a, |_, ps| ps.face(toward));
    let next = run_as(&mut world, &BOTH, StepReason::Predict);
    let kept = next.get(a).and_then(|e| e.player).map(|ps| ps.view[1]);
    check(
        kept == Some(faced),
        format!("faced {faced:.4}°, after a predicted move {kept:?}"),
    )
}

/// The proving ground as another module, entered inside its exit
/// `way_out`, with `library`.
fn exit_ground() -> Result<Level, String> {
    let ground = crate::determinism::proving_ground();
    let mut data = (**ground.module().ok_or("the ground has no module")?).clone();
    let exit = ground
        .placed()
        .iter()
        .find(|facts| facts.tag == "way_out")
        .map(|facts| facts.position)
        .ok_or("the ground has no way out")?;
    data.name = format!("{}_exit", data.name);
    data.entry.position = exit;
    Ok(ground.with_module(data).with_scripts(library()))
}

/// Arriving in a module from the snapshot after tick 31: the guests'
/// companions come still driven, and count as standing in the exit they
/// arrive in, as seat 0's player does, so the next step asks no
/// transition.
fn arrival(ran: &Ran) -> Vec<(&'static str, Result<String, String>)> {
    let arrived = exit_ground().and_then(|level| {
        let mut world = SimWorld::from_snapshot(&ran.snapshots[30], ran.level.clone())
            .map_err(|error| error.to_string())?;
        let after = world
            .install(level, None)
            .map_err(|error| error.to_string())?;
        Ok((after, run(&mut world, &BOTH)))
    });
    let (after, next) = match arrived {
        Ok(arrived) => arrived,
        Err(why) => return vec![("arrival_tenancy", Err(why))],
    };
    let tenants = after
        .entities
        .iter()
        .find(|entity| matches!(entity.kind, Kind::Object(ObjectKind::Trigger, _)))
        .and_then(|entity| entity.object)
        .map(|object| object.tenants);
    let body = after.body(Seat(1)).map(|entity| entity.id);
    vec![(
        "arrival_tenancy",
        check(
            tenants == Some(0b111) && body == Some(ran.ids[0]) && next.transition.is_none(),
            format!(
                "the exit's tenants {tenants:?}, seat 1's body {body:?}, asked {:?}",
                next.transition
            ),
        ),
    )]
}
