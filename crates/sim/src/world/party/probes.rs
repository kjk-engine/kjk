//! The probes of `party` (`kjk sim probes`).

use super::super::{Avatar, Clock, Fighting, Id, Player, SimWorld, StepReason};
use super::*;
use crate::action::Action;
use crate::combat::FriendlyFire;
use crate::input::{Command, Seat, TickInput};
use crate::math::{self};
use crate::module::{Link, ModuleData, ObjectData, ObjectKind, Placement, Repute};
use crate::notice::Notice;
use crate::probe::check;
use crate::snapshot::{Kind, ReadyCheck, SimId, Snapshot};
use crate::world::{Level, WorldConfig};
use collision::UNIT;

/// The module the pod's `OnEnter` starts (`StartNewModule`).
const POD_MODULE: &str = "elsewhere_too";

/// The probes' ground: the proving ground with a creature hostile to the
/// party in its east half (behind the cross wall from the entry) and the
/// pod, a trigger whose `OnEnter` starts `POD_MODULE`; `seats` seats send
/// commands, the first `bodies` of them with a body (the rest watch).
/// Scripted, the pod's script runs and creatures pick no foes of their own
/// (KOTOR's: the scripts name them); unscripted, minds pick their own.
/// One probe step: what went in, what came out.
struct Step {
    input: TickInput,
    snapshot: Snapshot,
    notices: Vec<(Seat, Notice)>,
}

struct Ground {
    world: SimWorld,
    level: Level,
    seats: u8,
    way_out: [f32; 3],
    pod: [f32; 3],
    entry: [f32; 3],
    hostile: SimId,
}

/// `HOSTILE_1` and the player hate each other (`Repute::at_war`).
const AT_WAR: [u8; 2] = [
    crate::combat::sheet::factions::HOSTILE_1,
    crate::combat::sheet::factions::PLAYER,
];

/// The pod's `OnEnter`: `StartNewModule(POD_MODULE)`.
fn leave_script() -> crate::script::Library {
    use fmt_odyssey::ncs::{Const, Op};
    const CATALOG: &str = r#"
// 0
void StartNewModule(string sModuleName, string sWayPoint = "", string sMovie1 = "", string sMovie2 = "", string sMovie3 = "", string sMovie4 = "", string sMovie5 = "", string sMovie6 = "");
"#;
    let mut asm = script_ncs::probes::Asm::default();
    // The last argument first: six movies and the waypoint, all empty.
    for _ in 0..7 {
        asm.op(Op::Const(Const::String(Vec::new())));
    }
    asm.op(Op::Const(Const::String(POD_MODULE.as_bytes().to_vec())))
        .op(Op::Action {
            routine: 0,
            args: 8,
        })
        .op(Op::Retn);
    crate::probe::library(CATALOG, [("probe_leave", asm.build())])
}

fn ground(seats: u8, bodies: u8, scripted: bool) -> Result<Ground, String> {
    built(seats, bodies, scripted, |_| {})
}

/// The ground's door, its first object.
const GATE: u32 = 0;

/// The ground's door made an open door that leads out (to `elsewhere`).
fn open_exit_gate(data: &mut ModuleData) {
    if let Some(Placement::Own(gate)) = data.placements.first_mut() {
        gate.state = 1;
        gate.link = Some(Link {
            module: "elsewhere".into(),
            tag: String::new(),
            destination: None,
        });
    }
}

/// `ground`, its module changed by `edit` first.
fn built(
    seats: u8,
    bodies: u8,
    scripted: bool,
    edit: impl FnOnce(&mut ModuleData),
) -> Result<Ground, String> {
    use crate::combat::sheet::{Sheet, factions};
    let proving = crate::determinism::proving_ground();
    let placed = |tag: &str| {
        proving
            .placed()
            .iter()
            .find(|facts| facts.tag == tag)
            .map(|facts| facts.position)
            .ok_or(format!("the ground has no {tag}"))
    };
    let (gate, way_out) = (placed("gate")?, placed("way_out")?);
    let mut data = (**proving.module().ok_or("the ground has no module")?).clone();
    let entry = data.entry.position;
    // The ground's own axis, a metre long: from the gate to the way out.
    let axis = math::scale(math::sub(way_out, gate), 1.0 / 5.5);
    let mut sheet = Sheet::commoner();
    sheet.faction = factions::HOSTILE_1;
    let hostile = ObjectData {
        tag: "probe_hostile".into(),
        sheet: Some(sheet),
        ..ObjectData::unknown(
            "probe_hostile",
            math::add(way_out, math::scale(axis, 1.75)),
            0.0,
        )
    };
    let pod_at = math::sub(entry, math::scale(axis, 2.0));
    let pod = ObjectData {
        kind: ObjectKind::Trigger,
        tag: "pod".into(),
        geometry: vec![
            [-0.75, -0.75, 0.0],
            [0.75, -0.75, 0.0],
            [0.75, 0.75, 0.0],
            [-0.75, 0.75, 0.0],
        ],
        scripts: vec![("ScriptOnEnter".into(), "probe_leave".into())],
        ..ObjectData::unknown("", pod_at, 0.0)
    };
    data.repute = Repute::at_war(AT_WAR);
    data.placements.extend([hostile, pod].map(Placement::Own));
    edit(&mut data);
    let level = proving.with_module(data);
    let level = if scripted {
        level.with_scripts(leave_script())
    } else {
        level
    };
    let mut world = SimWorld::new(
        WorldConfig {
            seats: bodies,
            ..WorldConfig::one_seat()
        },
        level.clone(),
    )
    .map_err(|error| error.to_string())?;
    let hostile = world
        .snapshot()
        .entities
        .iter()
        .find(|entity| match entity.kind {
            Kind::Object(ObjectKind::Creature, index) => level
                .placed()
                .get(index as usize)
                .is_some_and(|facts| facts.tag == "probe_hostile"),
            _ => false,
        })
        .map(|entity| entity.id)
        .ok_or("no hostile on the ground")?;
    Ok(Ground {
        world,
        level,
        seats,
        way_out,
        pod: pod_at,
        entry,
        hostile,
    })
}

impl Ground {
    /// Steps once, every seat standing, with `actions`.
    fn step(&mut self, actions: &[(u8, Action)]) -> Step {
        self.step_moving(actions, &[])
    }

    /// Steps once with `actions`, the seats in `forward` pushing forward,
    /// the rest standing.
    fn step_moving(&mut self, actions: &[(u8, Action)], forward: &[u8]) -> Step {
        let input = inputs(self.seats, forward, actions);
        let tick = self.world.tick() + 1;
        let snapshot = crate::step(&mut self.world, tick, &input, 8, StepReason::Authority);
        Step {
            notices: self.world.take_notices(),
            input,
            snapshot,
        }
    }

    /// Steps, standing, until `done` holds of a snapshot, at most `limit`
    /// steps.
    fn until(&mut self, limit: usize, done: impl Fn(&Snapshot) -> bool) -> Option<Snapshot> {
        (0..limit)
            .map(|_| self.step(&[]).snapshot)
            .find(|snapshot| done(snapshot))
    }

    /// Puts `seat`'s body at `at` (metres) on the ground's flat floor.
    fn put(&mut self, seat: u8, at: [f32; 3]) {
        let mut bodies = self.world.world.query::<(&Avatar, &mut Player)>();
        for (avatar, mut player) in bodies.iter_mut(&mut self.world.world) {
            if avatar.0 == Seat(seat) {
                player.0.origin[0] = at[0] / UNIT;
                player.0.origin[1] = at[1] / UNIT;
                player.0.velocity = [0.0; 3];
            }
        }
    }

    /// The hostile in a fight (struck a moment ago, for a minute) or out of
    /// one.
    fn fight(&mut self, on: bool) {
        let now = self.world.world.resource::<Clock>().time_ms;
        let hostile = self.hostile;
        let mut fighters = self.world.world.query::<(&Id, &mut Fighting)>();
        for (id, mut fighting) in fighters.iter_mut(&mut self.world.world) {
            if id.0 == hostile {
                fighting.0.combat_until = if on { now + 60_000 } else { 0 };
            }
        }
    }

    /// The hostile dead.
    fn kill(&mut self) {
        let hostile = self.hostile;
        let mut fighters = self.world.world.query::<(&Id, &mut Fighting)>();
        for (id, mut fighting) in fighters.iter_mut(&mut self.world.world) {
            if id.0 == hostile {
                fighting.0.life = crate::combat::fighter::Life::Dead;
            }
        }
    }
}

/// A tick's input for `seats` seats: those in `forward` pushing forward,
/// the rest standing, with `actions`.
fn inputs(seats: u8, forward: &[u8], actions: &[(u8, Action)]) -> TickInput {
    let mut input = TickInput::default();
    for seat in 0..seats {
        let push = if forward.contains(&seat) { 127 } else { 0 };
        input.insert(
            Seat(seat),
            Command {
                forward: push,
                ..Command::default()
            },
        );
    }
    for (seat, action) in actions {
        input.act(Seat(*seat), action.clone());
    }
    input
}

/// The calls ride the snapshot: through bytes and back, and a world rebuilt
/// from it gives them back.
fn round_trip() -> Result<String, String> {
    let mut ground = ground(2, 2, true)?;
    let mut snapshot = ground.step(&[]).snapshot;
    snapshot.in_combat = true;
    snapshot.paused_by = Some(Seat(1));
    snapshot.ready = Some(ReadyCheck {
        module: "elsewhere".into(),
        tag: "door".into(),
        via: SimId(3),
        asked_by: Seat(1),
        ready: 0b10,
        since_ms: 1234,
    });
    let decoded = Snapshot::decode(&snapshot.encode()).map_err(|error| error.to_string())?;
    let rebuilt = SimWorld::from_snapshot(&decoded, ground.level.clone())
        .map_err(|error| error.to_string())?
        .snapshot();
    let calls = |s: &Snapshot| (s.in_combat, s.paused_by, s.ready.clone());
    check(
        decoded == snapshot && calls(&rebuilt) == calls(&snapshot),
        format!(
            "decoded {:?}, rebuilt {:?}",
            calls(&decoded),
            calls(&rebuilt)
        ),
    )
}

const NOW: u64 = 60_000;

/// Hand-made characters, the friendly fire, and what the rule should say
/// of them.
fn rule_cases() -> Vec<(&'static str, bool, FriendlyFire, Vec<Combatant>)> {
    use crate::combat::ai::Task;
    use crate::combat::fighter::Life;
    use crate::combat::sheet::factions;
    let party = Combatant {
        id: SimId(0),
        at: [0.0; 3],
        party: true,
        faction: factions::PLAYER,
        life: Life::Alive,
        target: None,
        task: Task::Idle,
        combat_until: 0,
        struck_by: None,
        plot: false,
    };
    let foe = Combatant {
        id: SimId(1),
        at: [400.0, 0.0, 0.0],
        party: false,
        faction: factions::HOSTILE_1,
        ..party
    };
    let fighting = Combatant {
        target: Some(SimId(0)),
        ..foe
    };
    let far = [Rules::REACH + 1.0, 0.0, 0.0];
    let case = |name, expect, characters: &[Combatant]| {
        (name, expect, FriendlyFire::Off, characters.to_vec())
    };
    // Two players, the second struck by the first `ago` ms before `NOW`.
    let duel = |name, expect, friendly_fire, ago: u64| {
        let struck = Combatant {
            id: SimId(2),
            struck_by: Some((party.id, NOW - ago)),
            ..party
        };
        (name, expect, friendly_fire, vec![party, struck])
    };
    let window = super::super::COMBAT_MS;
    vec![
        duel(
            "a player struck by another, friendly fire on",
            true,
            FriendlyFire::Full,
            1000,
        ),
        duel(
            "a player struck by another, area friendly fire",
            true,
            FriendlyFire::Area,
            1000,
        ),
        duel(
            "a player struck by another, friendly fire off",
            false,
            FriendlyFire::Off,
            1000,
        ),
        duel(
            "a player struck by another, the window past",
            false,
            FriendlyFire::Full,
            window,
        ),
        case(
            "a hostile fighting near the party",
            true,
            &[party, fighting],
        ),
        case("a hostile standing idle", false, &[party, foe]),
        case(
            "a hostile that cannot fall fighting (plot)",
            false,
            &[
                party,
                Combatant {
                    plot: true,
                    ..fighting
                },
            ],
        ),
        case(
            "a hostile struck a moment ago",
            true,
            &[
                party,
                Combatant {
                    combat_until: NOW + 6000,
                    ..foe
                },
            ],
        ),
        case(
            "a hostile struck long ago",
            false,
            &[
                party,
                Combatant {
                    combat_until: NOW,
                    ..foe
                },
            ],
        ),
        case(
            "a hostile chasing",
            true,
            &[
                party,
                Combatant {
                    task: Task::Chase,
                    ..foe
                },
            ],
        ),
        case(
            "a neutral fighting",
            false,
            &[
                party,
                Combatant {
                    faction: factions::NEUTRAL,
                    ..fighting
                },
            ],
        ),
        case(
            "a dead hostile",
            false,
            &[
                party,
                Combatant {
                    life: Life::Dead,
                    ..fighting
                },
            ],
        ),
        case(
            "a hostile fighting out of keeping range",
            false,
            &[
                party,
                Combatant {
                    at: far,
                    ..fighting
                },
            ],
        ),
        case(
            "a hostile by a dead party member",
            false,
            &[
                Combatant {
                    life: Life::Dead,
                    ..party
                },
                fighting,
            ],
        ),
        case(
            "the party fighting no one",
            false,
            &[Combatant {
                target: Some(SimId(9)),
                combat_until: NOW + 6000,
                ..party
            }],
        ),
    ]
}

/// The rule (`in_combat`) on each case.
fn rule() -> Result<String, String> {
    let repute = Repute::at_war(AT_WAR);
    let cases = rule_cases();
    let wrong: Vec<&str> = cases
        .iter()
        .filter(|(_, expect, friendly_fire, characters)| {
            let rules = Rules {
                repute: Some(repute.with(&crate::module::UNADJUSTED)),
                friendly_fire: *friendly_fire,
                reach: Rules::REACH,
            };
            in_combat(characters, rules, NOW) != *expect
        })
        .map(|(name, ..)| *name)
        .collect();
    check(
        wrong.is_empty(),
        format!("{} cases, wrong: {wrong:?}", cases.len()),
    )
}

/// A hostile's own mind finds a player who walks into its sight: the party
/// is in combat, and out of it once the hostile is dead.
fn spotted() -> Result<String, String> {
    let mut ground = ground(1, 1, false)?;
    let calm = (0..10).all(|_| !ground.step(&[]).snapshot.in_combat);
    let way_out = ground.way_out;
    ground.put(0, way_out);
    let fought = ground.until(250, |snapshot| snapshot.in_combat);
    ground.kill();
    let after = ground.step(&[]).snapshot;
    check(
        calm && fought.is_some() && !after.in_combat,
        format!(
            "calm behind the wall {calm}, a fight from tick {:?}, after the kill {}",
            fought.map(|snapshot| snapshot.tick),
            after.in_combat
        ),
    )
}

/// A break is refused while the party is in combat.
fn break_in_combat() -> Result<String, String> {
    let mut ground = ground(2, 2, true)?;
    ground.fight(true);
    let fighting = ground.step(&[]).snapshot;
    let step = ground.step(&[(1, Action::Break)]);
    check(
        fighting.in_combat
            && step.snapshot.paused_by.is_none()
            && step.notices.contains(&(Seat(1), Notice::InCombat)),
        format!(
            "in combat {}, paused by {:?}, notices {:?}",
            fighting.in_combat, step.snapshot.paused_by, step.notices
        ),
    )
}

/// The positions of the players in `snapshot`.
fn places(snapshot: &Snapshot) -> Vec<[f32; 3]> {
    snapshot
        .entities
        .iter()
        .filter(|entity| matches!(entity.kind, Kind::Avatar(_)))
        .map(|entity| entity.position)
        .collect()
}

/// A break freezes from the step it is called in: players pushing forward
/// do not move, time stands, ticks go on.
fn break_freezes() -> Result<String, String> {
    let mut ground = ground(2, 2, true)?;
    let before = ground.step(&[]).snapshot;
    let called = ground.step_moving(&[(1, Action::Break)], &[0, 1]).snapshot;
    let held = (0..10)
        .map(|_| ground.step_moving(&[], &[0, 1]).snapshot)
        .last()
        .ok_or("no steps")?;
    check(
        called.paused_by == Some(Seat(1))
            && called.time_ms == before.time_ms
            && held.time_ms == before.time_ms
            && held.tick == before.tick + 11
            && places(&held) == places(&before),
        format!(
            "paused by {:?}; time {} → {} → {}; ticks {} → {}",
            called.paused_by, before.time_ms, called.time_ms, held.time_ms, before.tick, held.tick
        ),
    )
}

/// Who ends a break: its caller and the host do, another player is told no.
fn break_resumes() -> Result<String, String> {
    let cases = [
        ("the caller", 1u8, true),
        ("the host", 0, true),
        ("another player", 2, false),
    ];
    for (who, seat, ends) in cases {
        let mut ground = ground(3, 3, true)?;
        ground.step(&[(1, Action::Break)]);
        let step = ground.step(&[(seat, Action::Resume)]);
        let refused = step.notices.contains(&(Seat(seat), Notice::NotYourBreak));
        if step.snapshot.paused_by.is_none() != ends || refused == ends {
            return Err(format!(
                "{who}: paused by {:?}, notices {:?}",
                step.snapshot.paused_by, step.notices
            ));
        }
    }
    Ok(cases
        .map(|(who, _, ends)| format!("{who} {}", if ends { "ends it" } else { "is told no" }))
        .join(", "))
}

/// A paused snapshot stepped as a client predicts it, every seat pushing
/// forward, comes back the same but for its tick.
fn paused_prediction() -> Result<String, String> {
    let mut ground = ground(2, 2, true)?;
    ground.step(&[]);
    let paused = ground.step(&[(1, Action::Break)]).snapshot;
    let mut predicted = SimWorld::from_snapshot(&paused, ground.level.clone())
        .map_err(|error| error.to_string())?;
    let input = inputs(2, &[0, 1], &[]);
    let next = crate::step(
        &mut predicted,
        paused.tick + 1,
        &input,
        8,
        StepReason::Predict,
    );
    let same = Snapshot {
        tick: paused.tick,
        ..next.clone()
    } == paused;
    check(
        same,
        format!(
            "tick {} → {}, time {} → {}",
            paused.tick, next.tick, paused.time_ms, next.time_ms
        ),
    )
}

/// A ready check that goes: who stands in the way out first, the seats
/// with a body (the rest watch), the steps' actions, and the step it should
/// go on.
struct Goes {
    name: &'static str,
    bodies: u8,
    stand_in: &'static [u8],
    steps: Vec<Vec<(u8, Action)>>,
    goes_on: usize,
}

fn goes() -> Result<String, String> {
    let cases = [
        Goes {
            name: "both step in",
            bodies: 2,
            stand_in: &[0, 1],
            steps: vec![vec![]],
            goes_on: 0,
        },
        Goes {
            name: "the other presses ready",
            bodies: 2,
            stand_in: &[0],
            steps: vec![vec![], vec![(1, Action::Ready)]],
            goes_on: 1,
        },
        Goes {
            name: "a watching seat counts ready",
            bodies: 1,
            stand_in: &[0],
            steps: vec![vec![]],
            goes_on: 0,
        },
    ];
    for case in &cases {
        let mut ground = ground(2, case.bodies, true)?;
        for seat in case.stand_in {
            let way_out = ground.way_out;
            ground.put(*seat, way_out);
        }
        let went = case
            .steps
            .iter()
            .map(|actions| ground.step(actions).snapshot)
            .position(|snapshot| {
                snapshot.ready.is_none()
                    && snapshot
                        .transition
                        .as_ref()
                        .is_some_and(|t| t.module == "elsewhere" && t.seat == Seat(0))
            });
        if went != Some(case.goes_on) {
            return Err(format!(
                "{}: went on step {went:?}, not {}",
                case.name, case.goes_on
            ));
        }
    }
    Ok(cases.map(|case| case.name).join(", "))
}

/// One steps in: the proposal waits, and goes when the other steps in.
fn waits() -> Result<String, String> {
    let mut ground = ground(2, 2, true)?;
    let way_out = ground.way_out;
    ground.put(0, way_out);
    let asked = ground.step(&[]).snapshot;
    let waited = (0..20)
        .map(|_| ground.step(&[]).snapshot)
        .all(|snapshot| snapshot.transition.is_none() && snapshot.ready.is_some());
    ground.put(1, way_out);
    let went = ground.step(&[]).snapshot;
    check(
        asked
            .ready
            .as_ref()
            .is_some_and(|r| r.asked_by == Seat(0) && r.ready == 0b01)
            && asked.transition.is_none()
            && waited
            && went.transition.is_some()
            && went.ready.is_none(),
        format!(
            "asked {:?}, waited {waited}, went {:?}",
            asked.ready, went.transition
        ),
    )
}

/// Seat 0 forces only once the proposal has stood `FORCE_AFTER_MS`; a guest
/// never does.
fn forces() -> Result<String, String> {
    let mut ground = ground(2, 2, true)?;
    let way_out = ground.way_out;
    ground.put(0, way_out);
    let since = ground
        .step(&[])
        .snapshot
        .ready
        .ok_or("no proposal")?
        .since_ms;
    let early = ground.step(&[(0, Action::Force)]);
    ground
        .until(800, |snapshot| snapshot.time_ms >= since + FORCE_AFTER_MS)
        .ok_or("time did not pass")?;
    let guest = ground.step(&[(1, Action::Force)]);
    let host = ground.step(&[(0, Action::Force)]);
    let refused = |step: &Step, seat: u8| {
        step.notices.contains(&(Seat(seat), Notice::CannotForce))
            && step.snapshot.transition.is_none()
    };
    check(
        refused(&early, 0) && refused(&guest, 1) && host.snapshot.transition.is_some(),
        format!(
            "early {:?}, guest {:?}, host {:?}",
            early.notices, guest.notices, host.snapshot.transition
        ),
    )
}

/// An exit walked into in combat is refused (told once, however long it
/// stands there), and proposed once the fight is over.
fn exit_in_combat() -> Result<String, String> {
    let mut ground = ground(2, 2, true)?;
    ground.fight(true);
    ground.step(&[]);
    let way_out = ground.way_out;
    ground.put(0, way_out);
    let steps: Vec<Step> = (0..4).map(|_| ground.step(&[])).collect();
    let told: usize = steps
        .iter()
        .map(|step| {
            step.notices
                .iter()
                .filter(|n| **n == (Seat(0), Notice::InCombat))
                .count()
        })
        .sum();
    let none = steps
        .iter()
        .all(|step| step.snapshot.ready.is_none() && step.snapshot.transition.is_none());
    ground.fight(false);
    let after = ground.step(&[]).snapshot;
    check(
        told == 1 && none && after.ready.as_ref().is_some_and(|r| r.asked_by == Seat(0)),
        format!(
            "told {told} time(s), held back {none}, then {:?}",
            after.ready
        ),
    )
}

/// A script's new module skips the ready check: the other seat far off,
/// the party goes as the pod is entered.
fn scripted() -> Result<String, String> {
    let mut ground = ground(2, 2, true)?;
    let pod = ground.pod;
    ground.put(0, pod);
    let went = ground
        .until(5, |snapshot| snapshot.transition.is_some())
        .ok_or("no transition")?;
    check(
        went.transition
            .as_ref()
            .is_some_and(|t| t.module == POD_MODULE)
            && went.ready.is_none(),
        format!("{:?}, ready {:?}", went.transition, went.ready),
    )
}

/// A placeable with hit points standing in the pod (bashable, as the
/// Unknown World temple's tile lights on their tiles) enters nothing; the
/// player stepping in after does.
fn placeable_enters_nothing() -> Result<String, String> {
    let mut ground = built(1, 1, true, |data| {
        let pod = data
            .placements
            .iter()
            .find_map(|placement| match placement {
                Placement::Own(facts) if facts.tag == "pod" => Some(facts.position),
                _ => None,
            });
        if let Some(pod) = pod {
            data.placements.push(Placement::Own(ObjectData {
                kind: ObjectKind::Placeable,
                hit_points: Some((20, 20)),
                ..ObjectData::unknown("probe_crate", pod, 0.0)
            }));
        }
    })?;
    let still = ground.until(5, |snapshot| snapshot.transition.is_some());
    let pod = ground.pod;
    ground.put(0, pod);
    let went = ground.until(5, |snapshot| snapshot.transition.is_some());
    check(
        still.is_none() && went.is_some(),
        format!(
            "the crate alone: {:?}; the player in: {:?}",
            still.and_then(|s| s.transition),
            went.and_then(|s| s.transition)
        ),
    )
}

/// The asker steps out: the proposal passes to the lowest seat still in
/// the exit; with no one left there it lapses.
fn passes() -> Result<String, String> {
    let mut ground = ground(3, 3, true)?;
    let (way_out, entry) = (ground.way_out, ground.entry);
    ground.put(0, way_out);
    ground.put(1, way_out);
    let asked = ground.step(&[]).snapshot;
    ground.put(0, entry);
    let passed = ground.step(&[]).snapshot;
    ground.put(1, entry);
    let lapsed = ground.step(&[]).snapshot;
    let by = |snapshot: &Snapshot| snapshot.ready.as_ref().map(|r| (r.asked_by, r.ready));
    check(
        by(&asked) == Some((Seat(0), 0b011))
            && by(&passed) == Some((Seat(1), 0b011))
            && lapsed.ready.is_none()
            && lapsed.transition.is_none(),
        format!("{:?} → {:?} → {:?}", by(&asked), by(&passed), by(&lapsed)),
    )
}

/// A fight that starts under a standing proposal holds it: all ready, it
/// waits, the host's force is refused; it goes when the fight ends.
fn combat_holds() -> Result<String, String> {
    let mut ground = ground(2, 2, true)?;
    let way_out = ground.way_out;
    ground.put(0, way_out);
    let since = ground
        .step(&[])
        .snapshot
        .ready
        .ok_or("no proposal")?
        .since_ms;
    ground.fight(true);
    ground.put(1, way_out);
    let held = ground.step(&[]).snapshot;
    ground
        .until(800, |snapshot| snapshot.time_ms >= since + FORCE_AFTER_MS)
        .ok_or("time did not pass")?;
    let forced = ground.step(&[(0, Action::Force)]);
    ground.fight(false);
    let went = ground.step(&[]).snapshot;
    check(
        held.in_combat
            && held.ready.as_ref().is_some_and(|r| r.ready == 0b11)
            && held.transition.is_none()
            && forced.notices.contains(&(Seat(0), Notice::InCombat))
            && forced.snapshot.transition.is_none()
            && went.transition.is_some(),
        format!(
            "held {:?}, forced {:?}, went {:?}",
            held.ready, forced.notices, went.transition
        ),
    )
}

/// What waited during a break applies in the resume's step: the other
/// seat's resume and ready together, and the party goes.
fn queued() -> Result<String, String> {
    let mut ground = ground(2, 2, true)?;
    let way_out = ground.way_out;
    ground.put(0, way_out);
    ground.step(&[]);
    let paused = ground.step(&[(1, Action::Break)]).snapshot;
    let resumed = ground
        .step(&[(1, Action::Resume), (1, Action::Ready)])
        .snapshot;
    check(
        paused.paused_by == Some(Seat(1))
            && resumed.paused_by.is_none()
            && resumed.transition.is_some(),
        format!(
            "paused by {:?}, then {:?} and {:?}",
            paused.paused_by, resumed.paused_by, resumed.transition
        ),
    )
}

/// A probe's steps: each one's actions, by seat.
type Steps<'a> = &'a [&'a [(u8, Action)]];

/// Both players call a break, together, one during the other's, or one
/// stepped with its resume (the host steps a break's waiting actions with
/// the resume): one break, the host's, and after its resume the world runs.
fn break_once() -> Result<String, String> {
    let cases: [(&str, Steps<'_>); 3] = [
        (
            "together",
            &[
                &[(0, Action::Break), (1, Action::Break)],
                &[(0, Action::Resume)],
            ],
        ),
        (
            "during it",
            &[
                &[(0, Action::Break)],
                &[(1, Action::Break)],
                &[(0, Action::Resume)],
            ],
        ),
        (
            "with the resume",
            &[
                &[(0, Action::Break)],
                &[(0, Action::Resume), (1, Action::Break)],
            ],
        ),
    ];
    for (name, steps) in cases {
        let mut ground = ground(2, 2, true)?;
        let stepped: Vec<Snapshot> = steps
            .iter()
            .map(|actions| ground.step(actions).snapshot)
            .collect();
        let after = ground.step(&[]).snapshot;
        let (resumed, held) = stepped.split_last().ok_or("no steps")?;
        let one = held.iter().all(|s| s.paused_by == Some(Seat(0)));
        let runs = resumed.paused_by.is_none()
            && after.paused_by.is_none()
            && after.time_ms > resumed.time_ms;
        if !(one && runs) {
            return Err(format!(
                "{name}: paused by {:?}, then {:?}, time {} → {}",
                held.iter().map(|s| s.paused_by).collect::<Vec<_>>(),
                after.paused_by,
                resumed.time_ms,
                after.time_ms
            ));
        }
    }
    Ok(cases.map(|(name, _)| name).join(", "))
}

/// The gate, an open door that leads out, used from outside its doorway
/// (within a door's reach): alone the user goes at once; with the other
/// seat far off, the user is ready, the other is asked, and the proposal
/// stands until it answers.
fn exit_used() -> Result<String, String> {
    let near = crate::determinism::ground_point(-1.2, 0.0, 0.0);
    let used = [(
        0u8,
        Action::Use {
            object: GATE,
            verb: crate::action::Verb::Open,
        },
    )];
    let goes = |snapshot: &Snapshot| {
        snapshot.ready.is_none()
            && snapshot
                .transition
                .as_ref()
                .is_some_and(|t| t.module == "elsewhere" && t.seat == Seat(0))
    };
    let mut alone = built(1, 1, true, open_exit_gate)?;
    alone.put(0, near);
    let solo = alone.step(&used).snapshot;
    let mut pair = built(2, 2, true, open_exit_gate)?;
    let entry = pair.entry;
    pair.put(0, near);
    pair.put(1, entry);
    let asked = pair.step(&used).snapshot;
    let stood = (0..10)
        .map(|_| pair.step(&[]).snapshot)
        .all(|snapshot| snapshot.ready.is_some() && snapshot.transition.is_none());
    let went = pair.step(&[(1, Action::Ready)]).snapshot;
    check(
        goes(&solo)
            && asked
                .ready
                .as_ref()
                .is_some_and(|r| r.asked_by == Seat(0) && r.ready == 0b01)
            && asked.transition.is_none()
            && stood
            && goes(&went),
        format!(
            "alone {:?}; together {:?}, stood {stood}, then {:?}",
            solo.transition, asked.ready, went.transition
        ),
    )
}

/// A break and a ready check, recorded, replay to the same end.
fn recorded() -> Result<String, String> {
    let mut ground = ground(2, 2, true)?;
    let way_out = ground.way_out;
    ground.put(0, way_out);
    let mut recorder = crate::Recorder::new(
        crate::TickRate::default().hz(),
        ground.world.snapshot(),
        ground.level.id(),
        2,
    )
    .with_script(ground.world.script_state().clone());
    let script: [&[(u8, Action)]; 4] = [
        &[],
        &[(1, Action::Break)],
        &[(0, Action::Resume)],
        &[(1, Action::Ready)],
    ];
    let mut last = None;
    for actions in script {
        let step = ground.step(actions);
        recorder.record(8, &step.input, &step.snapshot);
        last = Some(step.snapshot);
    }
    let last = last.ok_or("no steps")?;
    let replayed = crate::Recording::decode(&recorder.finish(&last).encode())
        .map_err(|error| error.to_string())?
        .play(&ground.level)
        .map_err(|error| format!("{error:?}"))?;
    check(
        replayed.hash() == last.hash() && last.transition.is_some(),
        format!(
            "ends {}, replayed {}",
            last.hash().short(),
            replayed.hash().short()
        ),
    )
}

/// `kept` with a party conversation up (R5, R11): it keeps a seat it
/// gathers, never its unlocked listener nor one standing in its own
/// conversation, which that one's lock decides.
fn kept_by_party() -> Result<String, String> {
    use crate::script::dialogue::Conversation;
    use script_ncs::vm::ObjectId;
    let (host, guest) = (SimId(1), SimId(5));
    let talk = |listener: SimId, party: bool, locked: bool| Conversation {
        listener: ObjectId(listener.0),
        joined: Vec::new(),
        party,
        locked,
        ..crate::script::dialogue_probes::conversation()
    };
    let scene = talk(host, true, true);
    let cases = [
        (
            "gathered",
            vec![scene.clone()],
            guest,
            Some(IN_CONVERSATION),
        ),
        (
            "its unlocked listener",
            vec![talk(host, true, false)],
            host,
            None,
        ),
        (
            "in its own unlocked one",
            vec![scene.clone(), talk(guest, false, false)],
            guest,
            None,
        ),
        (
            "in its own locked one",
            vec![scene, talk(guest, false, true)],
            guest,
            Some(IN_CONVERSATION),
        ),
    ];
    let wrong: Vec<&str> = cases
        .iter()
        .filter(|(_, talks, body, want)| kept(talks, &[], *body) != *want)
        .map(|(name, ..)| *name)
        .collect();
    check(
        wrong.is_empty(),
        format!("{} cases, wrong: {wrong:?}", cases.len()),
    )
}

/// A probe: what it saw, as a pass or a failure.
type Probe = fn() -> Result<String, String>;

/// Every probe, by name (see each function).
pub fn probes() -> Vec<(&'static str, Result<String, String>)> {
    let cases: Vec<(&'static str, Probe)> = vec![
        ("calls_ride_the_snapshot", round_trip),
        ("in_combat_rule", rule),
        ("in_combat_world", spotted),
        ("break_refused_in_combat", break_in_combat),
        ("break_freezes", break_freezes),
        ("break_resumes", break_resumes),
        ("one_break_at_a_time", break_once),
        ("paused_prediction_holds", paused_prediction),
        ("ready_goes", goes),
        ("ready_waits_for_the_other", waits),
        ("exit_door_used_from_outside", exit_used),
        ("host_forces_after_5s", forces),
        ("exit_refused_in_combat", exit_in_combat),
        ("scripted_move_skips", scripted),
        ("placeable_enters_nothing", placeable_enters_nothing),
        ("proposal_passes_then_lapses", passes),
        ("combat_holds_a_proposal", combat_holds),
        ("queued_actions_at_resume", queued),
        ("recorded_calls_replay", recorded),
        ("kept_by_party", kept_by_party),
    ];
    cases.into_iter().map(|(name, run)| (name, run())).collect()
}
