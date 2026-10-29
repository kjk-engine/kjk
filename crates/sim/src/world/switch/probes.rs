//! The probes of `switch` (`kjk sim probes`).

use super::super::{Benched, Fighting, Id, LEADER_SEAT, SimWorld};
use crate::combat::Sheet;
use crate::input::{Command, Seat, TickInput};
use crate::module::{ObjectData, ObjectKind, Placement};
use crate::probe::check;
use crate::script::Pending;
use crate::snapshot::{Kind, SimId, Snapshot};
use crate::{Guests, Level, StepReason, WorldConfig};
use std::sync::Arc;

/// The companions' `NPC_*` numbers: one with no creature in the module
/// (made when switched to), one standing in it.
const MADE: u8 = 2;
const STANDING: u8 = 3;
/// Their templates (and tags).
const TEMPLATES: [(u8, &str); 2] = [(MADE, "probe_switch"), (STANDING, "probe_switch_b")];
/// A number no companion has.
const UNKNOWN: u8 = 7;

/// The probes' scripts (`script`), each setting custom tokens everyone sees
/// (`Snapshot::talk`):
/// - `probe_sw_to<n>`: 1 `SwitchPlayerCharacter(n)`, then in the same run
///   2 `GetTag(GetFirstPC())`, 3 `GetTag(GetPartyMemberByIndex(0))`, 4
///   `GetIsPC(GetFirstPC())`;
/// - `probe_sw_seen`: 5 `GetTag(GetPCSpeaker())` (no conversation: the
///   first PC), 6 `GetPartyMemberCount()`;
/// - `probe_sw_back`: 7 `SwitchPlayerCharacter(NPC_PLAYER)`, then 8
///   `GetTag(GetFirstPC())` (the player's own: no tag), 9
///   `GetIsPC(GetObjectByTag(the made companion))`, 10
///   `SwitchPlayerCharacter(UNKNOWN)`.
fn library() -> crate::script::Library {
    use fmt_odyssey::ncs::{Const, Op};
    const CATALOG: &str = r#"
// 0
void SetCustomToken(int nCustomTokenNumber, string sTokenValue);
// 1
string IntToString(int nInteger);
// 2
int SwitchPlayerCharacter(int nNPC);
// 3
string GetTag(object oObject);
// 4
object GetFirstPC();
// 5
object GetPartyMemberByIndex(int nIndex);
// 6
int GetIsPC(object oCreature);
// 7
object GetPCSpeaker();
// 8
int GetPartyMemberCount();
// 9
object GetObjectByTag(string sTag, int nNth=0);
// 10
void SaveNPCState(int nNPC);
// 11
object SpawnAvailableNPC(int nNPC, location lPosition);
// 12
location GetLocation(object oObject);
"#;
    let call = |routine: u16, args: u8| Op::Action { routine, args };
    let int = |value: i32| Op::Const(Const::Int(value));
    // Token `n` set to what `value` leaves on the stack (`string`, or an
    // int as text).
    let token = |asm: &mut script_ncs::probes::Asm, n: i32, value: &[Op], text: bool| {
        for op in value {
            asm.op(op.clone());
        }
        if !text {
            asm.op(call(1, 1));
        }
        asm.op(int(n)).op(call(0, 2));
    };
    let switch = |npc: i32| [int(npc), call(2, 1)];
    let first_tag = [call(4, 0), call(3, 1)];
    let to = |npc: u8| {
        let mut asm = script_ncs::probes::Asm::default();
        token(&mut asm, 1, &switch(i32::from(npc)), false);
        token(&mut asm, 2, &first_tag, true);
        token(&mut asm, 3, &[int(0), call(5, 1), call(3, 1)], true);
        token(&mut asm, 4, &[call(4, 0), call(6, 1)], false);
        asm.op(Op::Retn);
        (format!("probe_sw_to{npc}"), asm.build())
    };
    let mut seen = script_ncs::probes::Asm::default();
    token(&mut seen, 5, &[call(7, 0), call(3, 1)], true);
    token(&mut seen, 6, &[call(8, 0)], false);
    seen.op(Op::Retn);
    let mut back = script_ncs::probes::Asm::default();
    token(&mut back, 7, &switch(-1), false);
    token(&mut back, 8, &first_tag, true);
    let made = Op::Const(Const::String(TEMPLATES[0].1.as_bytes().to_vec()));
    token(&mut back, 9, &[int(0), made, call(9, 2), call(6, 1)], false);
    token(&mut back, 10, &switch(i32::from(UNKNOWN)), false);
    back.op(Op::Retn);
    // `probe_keep`: `SaveNPCState(STANDING)`; `probe_spawn`: 11
    // `GetTag(SpawnAvailableNPC(STANDING, GetLocation(GetFirstPC())))`.
    let mut keep = script_ncs::probes::Asm::default();
    keep.op(int(i32::from(STANDING)))
        .op(call(10, 1))
        .op(Op::Retn);
    let mut spawn = script_ncs::probes::Asm::default();
    let made_here = [
        call(4, 0),
        call(12, 1),
        int(i32::from(STANDING)),
        call(11, 2),
        call(3, 1),
    ];
    token(&mut spawn, 11, &made_here, true);
    spawn.op(Op::Retn);
    let scripts = TEMPLATES.iter().map(|(npc, _)| to(*npc)).chain([
        ("probe_sw_seen".to_string(), seen.build()),
        ("probe_sw_back".to_string(), back.build()),
        ("probe_keep".to_string(), keep.build()),
        ("probe_spawn".to_string(), spawn.build()),
    ]);
    let creatures = TEMPLATES
        .iter()
        .map(|(_, template)| {
            (
                template.to_string(),
                Arc::new(companion(template, [0.0; 3])),
            )
        })
        .collect();
    crate::probe::library(CATALOG, scripts).with_creatures(creatures)
}

/// A companion of the probes made from `template`, standing at `at`
/// (metres) as the dev soldier.
fn companion(template: &str, at: [f32; 3]) -> ObjectData {
    ObjectData::creature(template, template, at, Sheet::dev_soldier())
}

/// The proving ground with the standing companion 3 m ahead of its entry
/// and `library`: the level and that companion's object number.
fn ground() -> Result<(Level, u32), String> {
    let ground = crate::determinism::proving_ground();
    let mut data = (**ground.module().ok_or("the ground has no module")?).clone();
    let entry = data.entry;
    let (sin, cos) = crate::math::sin_cos_deg(entry.yaw);
    let [x, y, z] = entry.position;
    let number = u32::try_from(data.placements.len()).map_err(|error| error.to_string())?;
    let at = [x + 3.0 * cos, y + 3.0 * sin, z];
    data.placements.push(Placement::Own(ObjectData {
        yaw: entry.yaw,
        ..companion(TEMPLATES[1].1, at)
    }));
    Ok((ground.with_module(data).with_scripts(library()), number))
}

/// A world of `seats` on the ground (guests driving companions), both
/// companions available, the standing one on its creature (and in the
/// party, following the first seat, with `member`): the world, its level
/// and the standing companion's id.
fn switch_world(seats: u8, member: bool) -> Result<(SimWorld, Level, SimId), String> {
    let (level, number) = ground()?;
    let config = WorldConfig {
        seats,
        guests: Guests::Companions,
        ..WorldConfig::one_seat()
    };
    let mut world = SimWorld::new(config, level.clone()).map_err(|error| error.to_string())?;
    let standing = world
        .snapshot()
        .entities
        .iter()
        .find(|entity| entity.kind == Kind::Object(ObjectKind::Creature, number))
        .map(|entity| entity.id)
        .ok_or("no standing companion")?;
    let mut state = world.script_state().clone();
    for (npc, template) in TEMPLATES {
        let object = (npc == STANDING).then_some(standing.0);
        let slot = crate::script::party::Npc {
            template: template.to_string(),
            object,
            kept: None,
            template_data: None,
        };
        state.party.available.insert(npc, slot);
    }
    if member {
        state.party.join(STANDING, standing.0);
        let mut fighters = world.world.query::<(&Id, &mut Fighting)>();
        for (id, mut fighting) in fighters.iter_mut(&mut world.world) {
            if id.0 == standing {
                super::super::scripts::join(&mut fighting.0, true);
            }
        }
    }
    world.set_script_state(state);
    Ok((world, level, standing))
}

/// Seat 0 standing, or running forward; the seats past it standing.
fn run(world: &mut SimWorld, seats: u8, forward: i8) -> Snapshot {
    run_as(world, seats, forward, StepReason::Authority)
}

fn run_as(world: &mut SimWorld, seats: u8, forward: i8, reason: StepReason) -> Snapshot {
    let mut input = TickInput::default();
    for seat in 0..seats {
        let command = Command {
            angles: crate::moves::view_angles(0.0, 0.0),
            forward: if seat == 0 { forward } else { 0 },
            ..Command::default()
        };
        input.insert(Seat(seat), command);
    }
    let tick = world.tick() + 1;
    crate::step(world, tick, &input, 8, reason)
}

/// `script` run for the module before the next step.
fn queue(world: &mut SimWorld, script: &str) {
    let mut scripts = world.world.resource_mut::<super::super::scripts::Scripts>();
    let pending = Pending::new(script, crate::script::MODULE);
    scripts.state.pending.push(pending);
}

/// The custom tokens `numbers` set, in order ("" for one not set).
fn tokens(snapshot: &Snapshot, numbers: std::ops::RangeInclusive<i32>) -> Vec<String> {
    numbers
        .map(|n| {
            snapshot
                .talk
                .tokens
                .get(&n)
                .cloned()
                .unwrap_or_else(String::new)
        })
        .collect()
}

/// How far `id` moved across the floor from `from` to `to` (metres).
fn moved(from: &Snapshot, to: &Snapshot, id: SimId) -> f32 {
    match (from.get(id), to.get(id)) {
        (Some(a), Some(b)) => (b.position[0] - a.position[0]).hypot(b.position[1] - a.position[1]),
        _ => f32::MAX,
    }
}

/// What `scenario` saw, by moment.
struct Ran {
    level: Level,
    /// The original player and the companion made for the switch.
    own: SimId,
    made: Option<SimId>,
    /// After the switching script's step, the step after it (switched),
    /// seat 0's run on it, the scripts seeing it, the switching back's
    /// step, the step after, and seat 0's run then.
    switching: Snapshot,
    switched: Snapshot,
    walked: Snapshot,
    /// The scripts' state with `walked` (the switch in the party table).
    script: crate::script::State,
    seen: Snapshot,
    back: Snapshot,
    returned: Snapshot,
    walked_back: Snapshot,
}

/// Steps of seat 0's runs, and of standing still for a body let go to
/// come to rest.
const RUN_STEPS: usize = 30;
const REST_STEPS: usize = 60;

/// One seat on the ground: `probe_sw_to2` switches it to the companion with
/// no creature here (made beside the player), it runs `RUN_STEPS` steps,
/// `probe_sw_seen` reads the PC, `probe_sw_back` switches back, and it runs
/// again once the companion let go has come to rest.
fn scenario() -> Result<Ran, String> {
    let (mut world, level, _) = switch_world(1, false)?;
    let start = run(&mut world, 1, 0);
    let own = start.own(Seat(0)).map(|e| e.id).ok_or("no player")?;
    queue(&mut world, &format!("probe_sw_to{MADE}"));
    let switching = run(&mut world, 1, 0);
    let switched = run(&mut world, 1, 0);
    let made = world
        .script_state()
        .party
        .switched_body()
        .map(SimId)
        .filter(|id| switched.get(*id).is_some());
    let mut walked = switched.clone();
    for _ in 0..RUN_STEPS {
        walked = run(&mut world, 1, 127);
    }
    let script = world.script_state().clone();
    queue(&mut world, "probe_sw_seen");
    let seen = run(&mut world, 1, 0);
    queue(&mut world, "probe_sw_back");
    let back = run(&mut world, 1, 0);
    let mut returned = run(&mut world, 1, 0);
    for _ in 0..REST_STEPS {
        returned = run(&mut world, 1, 0);
    }
    let mut walked_back = returned.clone();
    for _ in 0..RUN_STEPS {
        walked_back = run(&mut world, 1, 127);
    }
    Ok(Ran {
        level,
        own,
        made,
        switching,
        switched,
        walked,
        script,
        seen,
        back,
        returned,
        walked_back,
    })
}

/// Whether `id` in `snapshot` is a creature driven by the first seat.
fn driven(snapshot: &Snapshot, id: SimId) -> bool {
    snapshot.get(id).is_some_and(|entity| {
        matches!(entity.kind, Kind::Object(ObjectKind::Creature, _))
            && entity
                .fighter
                .is_some_and(|f| f.mind.driver == Some(LEADER_SEAT))
    })
}

/// The checks (`kjk sim probes`; see each).
pub fn probes() -> crate::probe::Checks {
    let mut checks = match scenario() {
        Ok(ran) => scenario_checks(&ran),
        Err(why) => vec![("switch_scenario", Err(why))],
    };
    checks.push(("standing_creature_taken", standing_taken()));
    checks.push(("party_table_keeps_a_companion", kept()));
    checks.push(("guest_gives_way", guest_gives_way()));
    checks.push(("leads_a_companion_while_switched", leads_while_switched()));
    checks.push(("install_after_switching_back", install_after_back()));
    checks.push(("led_companion_throws_punches", led_punches()));
    checks.push(("led_companion_has_its_players_aids", led_aids()));
    checks
}

/// Seat 0 switched to the made companion leads through the standing one
/// (the portrait click, `Action::Lead`): the standing one runs on seat 0's
/// commands, the one switched to stands (TSL's prologue: 3C-FD led to the
/// security console, T3-M4 waiting in the airlock); clicking the one
/// switched to takes the lead back.
fn leads_while_switched() -> Result<String, String> {
    let (mut world, _, standing) = switch_world(1, true)?;
    run(&mut world, 1, 0);
    queue(&mut world, &format!("probe_sw_to{MADE}"));
    run(&mut world, 1, 0);
    let switched = run(&mut world, 1, 0);
    let made = world
        .script_state()
        .party
        .switched_body()
        .map(SimId)
        .ok_or("nothing switched to")?;
    let lead = |world: &mut SimWorld, body: SimId| {
        let mut input = TickInput::default();
        input.insert(Seat(0), Command::default());
        input.act(Seat(0), crate::action::Action::Lead(body));
        let tick = world.tick() + 1;
        crate::step(world, tick, &input, 8, StepReason::Authority)
    };
    lead(&mut world, standing);
    let mut walked = switched.clone();
    for _ in 0..RUN_STEPS {
        walked = run(&mut world, 1, 127);
    }
    let (ran, stood) = (
        moved(&switched, &walked, standing),
        moved(&switched, &walked, made),
    );
    lead(&mut world, made);
    let leader = world.script_state().party.leader;
    check(
        ran > 0.2 && stood < 0.05 && leader.is_none(),
        format!(
            "the one led ran {ran:.2} m, the one switched to moved {stood:.2} m; its portrait clicked, the leader {leader:?}"
        ),
    )
}

/// The companion led (`Action::Lead`) has its player's test aids: with the
/// player's own sheet immortal (`--god`), a blow that would kill the one
/// led leaves it standing at its last hit point (TSL's Bao-Dur led through
/// the old military base's force fields while its droids shot at him).
fn led_aids() -> Result<String, String> {
    let (mut world, _, standing) = switch_world(1, true)?;
    world.add_player_flags(crate::combat::sheet::flags::IMMORTAL);
    run(&mut world, 1, 0);
    let mut input = TickInput::default();
    input.insert(Seat(0), Command::default());
    input.act(Seat(0), crate::action::Action::Lead(standing));
    let tick = world.tick() + 1;
    crate::step(&mut world, tick, &input, 8, StepReason::Authority);
    let player = world
        .snapshot()
        .own(Seat(0))
        .map(|own| own.id)
        .ok_or("no player")?;
    world
        .world
        .resource_mut::<super::super::Hits>()
        .0
        .push(super::super::Hit {
            attacker: player,
            target: standing,
            source: crate::combat::damage::Source::Script {
                damage: u16::MAX,
                damage_type: None,
                cast: false,
                shot: None,
                staged: false,
            },
            point: None,
            direction: None,
        });
    let after = run(&mut world, 1, 0);
    let fighter = after.get(standing).and_then(|entity| entity.fighter);
    check(
        fighter.is_some_and(|f| f.alive() && f.hp > 0),
        format!(
            "the one led after a killing blow: {:?}",
            fighter.map(|f| (f.life, f.hp))
        ),
    )
}

/// The standing companion led (`Action::Lead`), empty-handed, throws a
/// punch at seat 0's attack press as its own body would, with no target
/// of its own (TSL's Bao-Dur led to strike a force field open).
fn led_punches() -> Result<String, String> {
    let (mut world, _, standing) = switch_world(1, true)?;
    run(&mut world, 1, 0);
    let mut input = TickInput::default();
    input.insert(Seat(0), Command::default());
    input.act(Seat(0), crate::action::Action::Lead(standing));
    let tick = world.tick() + 1;
    crate::step(&mut world, tick, &input, 8, StepReason::Authority);
    {
        let mut ids = world.world.query::<(bevy_ecs::entity::Entity, &Id)>();
        let body = ids
            .iter(&world.world)
            .find(|(_, id)| id.0 == standing)
            .map(|(entity, _)| entity);
        if let Some(body) = body {
            let bare = Sheet {
                weapons: [None; 2],
                ..Sheet::dev_soldier()
            };
            world
                .world
                .entity_mut(body)
                .insert(super::super::PlayerSheet(bare));
        }
    }
    let mut attack = TickInput::default();
    attack.insert(
        Seat(0),
        Command {
            buttons: crate::buttons::ATTACK,
            ..Command::default()
        },
    );
    let mut last = None;
    for _ in 0..RUN_STEPS {
        let tick = world.tick() + 1;
        last = Some(crate::step(
            &mut world,
            tick,
            &attack,
            8,
            StepReason::Authority,
        ));
    }
    let fighting = last
        .as_ref()
        .and_then(|snapshot| snapshot.entities.iter().find(|e| e.id == standing))
        .and_then(|entity| entity.fighter)
        .map(|fighter| fighter.combat_until > 0);
    check(
        fighting == Some(true),
        format!("the one led in a fight of its own press: {fighting:?}"),
    )
}

/// A module installed in the step whose script switched back carries the
/// seat's own character, and the companion given back, no member, is no
/// companion carried in (TSL's prologue ends so and starts Peragus at
/// once).
fn install_after_back() -> Result<String, String> {
    let (mut world, level, _) = switch_world(1, false)?;
    let start = run(&mut world, 1, 0);
    let own = start.own(Seat(0)).map(|e| e.id).ok_or("no player")?;
    queue(&mut world, &format!("probe_sw_to{MADE}"));
    run(&mut world, 1, 0);
    run(&mut world, 1, 0);
    let made = world
        .script_state()
        .party
        .switched_body()
        .map(SimId)
        .ok_or("nothing switched to")?;
    queue(&mut world, "probe_sw_back");
    run(&mut world, 1, 0);
    let after = world
        .install(level, None)
        .map_err(|error| error.to_string())?;
    // The same module installed again: the one given back is where the
    // module was left, on its own, not a companion carried in.
    let body = after.body(Seat(0)).map(|e| e.id);
    let follows = after
        .get(made)
        .and_then(|e| e.fighter)
        .map(|f| (f.mind.leader, f.mind.driver));
    check(
        body == Some(own) && follows.is_some_and(|f| f == (None, None)),
        format!(
            "seat 0's body {body:?} (its own {own:?}), the one given back's leader and driver {follows:?}"
        ),
    )
}

/// What the scenario shows (see each check's words).
fn scenario_checks(ran: &Ran) -> crate::probe::Checks {
    let (own, made) = (ran.own, ran.made.unwrap_or(SimId(u32::MAX)));
    let tag = TEMPLATES[0].1;
    let body = |snapshot: &Snapshot| snapshot.body(Seat(0)).map(|entity| entity.id);
    let in_step = tokens(&ran.switching, 1..=4);
    let seen = tokens(&ran.seen, 5..=6);
    let back = tokens(&ran.back, 7..=10);
    let own_kind = ran.switched.get(own).map(|entity| entity.kind);
    let released = ran
        .returned
        .get(made)
        .and_then(|entity| entity.fighter)
        .map(|fighter| (fighter.mind.driver, fighter.mind.leader));
    let again = scenario().map(|again| again.walked_back.hash());
    vec![
        (
            "made_from_template",
            check(
                ran.made.is_some() && ran.switching.spawned.len() == 1,
                format!(
                    "the companion's creature {:?}, made in play {:?}",
                    ran.made, ran.switching.spawned
                ),
            ),
        ),
        (
            "scripts_see_it_at_once",
            check(
                in_step == ["1", tag, tag, "1"],
                format!(
                    "Switch, GetFirstPC's tag, member 0's tag, GetIsPC(GetFirstPC): {in_step:?}"
                ),
            ),
        ),
        (
            "seat_moves_it",
            check(
                body(&ran.switched) == Some(made)
                    && driven(&ran.switched, made)
                    && own_kind == Some(Kind::Avatar(Seat(0))),
                format!(
                    "seat 0's body {:?} (the companion {made:?}), the player's own still {own_kind:?}",
                    body(&ran.switched)
                ),
            ),
        ),
        ("seat_walks_it", {
            let (ran_m, stood_m) = (
                moved(&ran.switched, &ran.walked, made),
                moved(&ran.switched, &ran.walked, own),
            );
            check(
                ran_m >= 0.4 && stood_m < 0.1,
                format!("the companion ran {ran_m:.2} m, the player's own moved {stood_m:.2} m"),
            )
        }),
        (
            "pc_speaker_and_party",
            check(
                seen == [tag, "1"],
                format!("GetPCSpeaker's tag, GetPartyMemberCount: {seen:?}"),
            ),
        ),
        (
            "switched_back",
            check(
                back == ["1", "", "0", "0"]
                    && body(&ran.returned) == Some(own)
                    && released == Some((None, None)),
                format!(
                    "Switch(NPC_PLAYER), GetFirstPC's tag, GetIsPC(companion), Switch(unknown): {back:?}; seat 0's body {:?}; the companion's driver and leader {released:?}",
                    body(&ran.returned)
                ),
            ),
        ),
        ("seat_walks_its_own_again", {
            let (ran_m, stood_m) = (
                moved(&ran.returned, &ran.walked_back, own),
                moved(&ran.returned, &ran.walked_back, made),
            );
            check(
                ran_m >= 0.4 && stood_m < 0.1,
                format!("the player ran {ran_m:.2} m, the companion moved {stood_m:.2} m"),
            )
        }),
        ("kept_in_snapshots", restored(ran, made)),
        ("kept_in_saves", saved(ran)),
        ("predicts_as_authority", predicted(ran, made)),
        (
            "same_every_run",
            check(
                again
                    .as_ref()
                    .is_ok_and(|hash| *hash == ran.walked_back.hash()),
                format!("{} then {again:?}", ran.walked_back.hash().short()),
            ),
        ),
    ]
}

/// A switched snapshot encodes and decodes, and a world made from it is the
/// same world: the companion seat 0's body, the player's own benched (still
/// its avatar in the snapshot).
fn restored(ran: &Ran, made: SimId) -> Result<String, String> {
    let start = &ran.walked;
    let decoded = Snapshot::decode(&start.encode()).is_ok_and(|decoded| decoded == *start);
    let mut world =
        SimWorld::from_snapshot(start, ran.level.clone()).map_err(|error| error.to_string())?;
    let again = world.snapshot();
    let benched = world
        .world
        .query::<(&Id, &Benched)>()
        .iter(&world.world)
        .map(|(id, _)| id.0)
        .collect::<Vec<_>>();
    check(
        decoded && again == *start && benched == [ran.own] && driven(&again, made),
        format!(
            "decoded {decoded}, restored the same {}, benched {benched:?}",
            again == *start
        ),
    )
}

/// A game saved while switched and loaded goes on as the saved one does,
/// every step for 200, seat 0 running: the party table's switch rides in
/// the save.
fn saved(ran: &Ran) -> Result<String, String> {
    let (mut world, ..) = switch_world(1, false)?;
    run(&mut world, 1, 0);
    queue(&mut world, &format!("probe_sw_to{MADE}"));
    for _ in 0..3 {
        run(&mut world, 1, 0);
    }
    let save = crate::Save {
        hz: crate::TICK_HZ,
        level: ran.level.id(),
        snapshot: world.snapshot(),
        script: world.script_state().clone(),
    };
    let loaded = crate::Save::decode(&save.encode()).map_err(|error| error.to_string())?;
    let mut again = loaded.world(&ran.level, 1, None)?;
    let switched = again.script_state().party.switched;
    for step in 0..200 {
        let (a, b) = (run(&mut world, 1, 127), run(&mut again, 1, 127));
        if a.hash() != b.hash() {
            return Err(format!("apart at step {step} after loading"));
        }
    }
    check(
        loaded == save && switched == Some(MADE),
        format!("the save read back the same, switched to {switched:?}, alike for 200 steps"),
    )
}

/// A client's prediction from a switched snapshot (no scripts' state, so
/// no switch in a party table, no minds) moves the companion as the
/// authority (with its scripts' state) does: the snapshot's driver is all
/// it needs.
fn predicted(ran: &Ran, made: SimId) -> Result<String, String> {
    let start = &ran.walked;
    let stepped = |reason: StepReason| -> Result<Option<Vec<u8>>, String> {
        let mut world =
            SimWorld::from_snapshot(start, ran.level.clone()).map_err(|error| error.to_string())?;
        if reason == StepReason::Authority {
            world.set_script_state(ran.script.clone());
        }
        let next = run_as(&mut world, 1, 127, reason);
        Ok(next.get(made).map(Snapshot::record))
    };
    let (authority, predicted) = (
        stepped(StepReason::Authority)?,
        stepped(StepReason::Predict)?,
    );
    check(
        authority.is_some() && authority == predicted,
        format!(
            "the companion predicted as the authority moves it: {}",
            authority == predicted
        ),
    )
}

/// The companion standing in the module is switched to on its own
/// creature: nothing is made.
fn standing_taken() -> Result<String, String> {
    let (mut world, _, standing) = switch_world(1, false)?;
    run(&mut world, 1, 0);
    queue(&mut world, &format!("probe_sw_to{STANDING}"));
    run(&mut world, 1, 0);
    let after = run(&mut world, 1, 0);
    let body = after.body(Seat(0)).map(|entity| entity.id);
    check(
        body == Some(standing) && after.spawned.is_empty(),
        format!(
            "seat 0's body {body:?} (the standing companion {standing:?}), made {:?}",
            after.spawned
        ),
    )
}

/// A guest drives the standing companion, in the party; the first seat
/// switched to it takes it, the guest watches (no other free), and it
/// drives it again once the first seat switches back.
fn guest_gives_way() -> Result<String, String> {
    let (mut world, _, standing) = switch_world(2, true)?;
    let bodies = |snapshot: &Snapshot| [0, 1].map(|seat| snapshot.body(Seat(seat)).map(|e| e.id));
    let before = bodies(&run(&mut world, 2, 0));
    queue(&mut world, &format!("probe_sw_to{STANDING}"));
    run(&mut world, 2, 0);
    let switched = run(&mut world, 2, 0);
    queue(&mut world, "probe_sw_back");
    run(&mut world, 2, 0);
    run(&mut world, 2, 0);
    let after = run(&mut world, 2, 0);
    let own = switched.own(Seat(0)).map(|e| e.id);
    let wanted = [
        [own, Some(standing)],
        [Some(standing), None],
        [own, Some(standing)],
    ];
    let seen = [before, bodies(&switched), bodies(&after)];
    check(
        seen == wanted,
        format!("seats 0 and 1 drive {seen:?} (wanted {wanted:?})"),
    )
}

/// The party table keeps a companion as it stood (`SaveNPCState`) and makes
/// it so again (`SpawnAvailableNPC`): hurt, its sheet its own, an item worn,
/// the creature made has the same hit points, sheet and item (KOTOR's
/// companions sent away keep their gear and levels); a save carries what
/// the table kept.
fn kept() -> Result<String, String> {
    use crate::inventory::{FIRST_ITEM, Item};
    let (mut world, _, standing) = switch_world(1, true)?;
    let entity = world
        .world
        .query::<(bevy_ecs::entity::Entity, &Id)>()
        .iter(&world.world)
        .find(|(_, id)| id.0 == standing)
        .map(|(entity, _)| entity)
        .ok_or("no standing companion")?;
    let mut sheet = Sheet::dev_soldier();
    sheet.good_evil = 7;
    let mut found = world.world.entity_mut(entity);
    found.insert(super::super::PlayerSheet(sheet));
    if let Some(mut fighting) = found.get_mut::<Fighting>() {
        fighting.0.hp = 300;
    }
    let mut state = world.script_state().clone();
    let item = FIRST_ITEM + state.next_item;
    state.next_item += 1;
    state.items.insert(
        item,
        Item {
            template: "probe_blade".into(),
            holder: standing.0,
            slot: Some(4),
            stack: 1,
            charges: 0,
            droppable: true,
            plot: false,
            identified: true,
            infinite: false,
            non_equippable: false,
            upgrades: 0,
            fitted: Vec::new(),
        },
    );
    world.set_script_state(state);
    run(&mut world, 1, 0);
    queue(&mut world, "probe_keep");
    run(&mut world, 1, 0);
    queue(&mut world, "probe_spawn");
    run(&mut world, 1, 0);
    let after = run(&mut world, 1, 0);
    let state = world.script_state();
    let made = state
        .party
        .available
        .get(&STANDING)
        .and_then(|slot| slot.object)
        .map(SimId)
        .filter(|id| *id != standing)
        .ok_or("no companion made")?;
    let body = after
        .get(made)
        .ok_or("the made companion is not in the world")?;
    let hp = body.fighter.map_or(0, |f| f.hp);
    let good_evil = body.sheet.map_or(0, |sheet| sheet.good_evil);
    let holder = state.items.get(&item).map_or(0, |item| item.holder);
    let mut out = crate::wire::Writer::default();
    state.encode(&mut out);
    let bytes = out.0;
    let back = crate::script::State::decode(&mut crate::wire::Reader::new("state", &bytes))
        .map_err(|error| error.to_string())?;
    check(
        hp == 300 && good_evil == 7 && holder == made.0 && back.party == state.party,
        format!(
            "made #{}: {hp} hundredths of HP, good/evil {good_evil}, the item with #{holder}; the table {}",
            made.0,
            if back.party == state.party {
                "round-trips"
            } else {
                "differs after a round trip"
            }
        ),
    )
}
