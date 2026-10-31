//! The party members nobody plays, in a fight and out (`kjk sim probes
//! "party fights"`; `world::minds`), on the proving ground with scripts (as
//! the games' stages have, so no mind picks its own foes), a player, two
//! companions and a foe, no install:
//! - the player playing itself: a foe aiming at it is joined by both
//!   companions though no script names it to them;
//! - a foe nobody fights is left alone: they start no fight, and solo mode
//!   and a script's move hold them out of one;
//! - a companion led: the player's own character and the other companion
//!   follow the one led, not the player's standing body, and both join its
//!   fight and attack;
//! - taken back: the player's character is its player's again, its fight over.

use script_ncs::vm::ObjectId;

use super::{Fighting, Id, SimWorld};
use crate::action::Action;
use crate::combat::Sheet;
use crate::combat::ai::Task;
use crate::combat::sheet::factions;
use crate::input::{Command, Seat, TickInput, buttons};
use crate::module::{ObjectData, ObjectKind, Placement, Repute};
use crate::probe::{Checks, check};
use crate::script::actions::Intent;
use crate::script::party::Npc;
use crate::snapshot::{EntityState, SimId};
use crate::{Snapshot, StepReason, WorldConfig};

/// A step's milliseconds.
const STEP_MS: u64 = 8;
const COMPANIONS: [&str; 2] = ["comp_a", "comp_b"];
const FOE: &str = "foe";

struct Run {
    world: SimWorld,
    last: Snapshot,
    host: u32,
    a: u32,
    b: u32,
    foe: u32,
    /// The steps each of the host and `b` held attack.
    attacks: [u32; 2],
    /// A guest at seat 1, driving a companion (`drivers`).
    guest: bool,
}

impl Run {
    /// The ground, its foe at `foe` (metres in the room's frame).
    fn new(foe: (f32, f32)) -> Result<Run, String> {
        Run::with(foe, (-5.0, -2.0))
    }

    /// `new`'s, the first companion at `first`.
    fn with(foe: (f32, f32), first: (f32, f32)) -> Result<Run, String> {
        Run::build(foe, first, false)
    }

    /// `new`'s, a guest at seat 1 driving the first companion.
    fn with_guest(foe: (f32, f32)) -> Result<Run, String> {
        Run::build(foe, (-5.0, -2.0), true)
    }

    fn build(foe: (f32, f32), first: (f32, f32), guest: bool) -> Result<Run, String> {
        let ground = crate::determinism::proving_ground();
        let mut data = (**ground.module().ok_or("the ground has no module")?).clone();
        data.repute = Repute::at_war([factions::PLAYER, factions::HOSTILE_1]);
        for (tag, (x, y)) in COMPANIONS.into_iter().zip([first, (-5.0, 2.0)]) {
            data.placements.push(Placement::Own(ObjectData {
                sheet: Some(Sheet {
                    faction: factions::PLAYER,
                    ..Sheet::dev_soldier()
                }),
                template: tag.to_string(),
                ..ObjectData::bare(
                    ObjectKind::Creature,
                    tag,
                    crate::determinism::ground_point(x, y, 0.0),
                )
            }));
        }
        data.placements.push(Placement::Own(ObjectData {
            sheet: Some(Sheet {
                faction: factions::HOSTILE_1,
                ..Sheet::dev_soldier()
            }),
            ..ObjectData::bare(
                ObjectKind::Creature,
                FOE,
                crate::determinism::ground_point(foe.0, foe.1, 0.0),
            )
        }));
        let library = crate::probe::library("", Vec::<(String, _)>::new());
        // The navigation mesh over the room.
        let [p, q, r, s] = [[-7.5, -5.5], [7.5, -5.5], [7.5, 5.5], [-7.5, 5.5]]
            .map(|[x, y]| crate::determinism::ground_point(x, y, 0.0));
        let mut nav = collision::nav::NavMesh::build(
            &[[p, q, r], [p, r, s]],
            crate::walk::CLEARANCE * collision::UNIT,
        );
        if let Some(level) = ground.collision().level() {
            nav.mark(level);
        }
        let level = ground.with_module(data).with_scripts(library).with_nav(nav);
        let config = WorldConfig {
            seats: if guest { 2 } else { 1 },
            guests: crate::Guests::Companions,
            ..WorldConfig::one_seat()
        };
        let mut world = SimWorld::new(config, level).map_err(|e| e.to_string())?;
        let start = world.snapshot();
        let host = start.body(Seat(0)).ok_or("no host")?.id.0;
        let objects = crate::Instances::in_snapshot(world.level(), &start);
        let tagged = |tag: &str| {
            start
                .entities
                .iter()
                .find(|entity| {
                    objects
                        .of_entity(entity)
                        .is_some_and(|data| data.tag == tag)
                })
                .map(|entity| entity.id.0)
                .ok_or(format!("no {tag}"))
        };
        let (a, b, foe) = (tagged(COMPANIONS[0])?, tagged(COMPANIONS[1])?, tagged(FOE)?);
        let mut state = world.script_state().clone();
        for (npc, (tag, object)) in COMPANIONS.into_iter().zip([a, b]).enumerate() {
            state.party.available.insert(
                npc as u8,
                Npc {
                    template: tag.to_string(),
                    object: Some(object),
                    kept: None,
                    template_data: None,
                },
            );
            state.party.members.push(npc as u8);
        }
        world.set_script_state(state);
        let mut fighters = world.world.query::<(&Id, &mut Fighting)>();
        for (id, mut fighting) in fighters.iter_mut(&mut world.world) {
            if [a, b].contains(&id.0.0) {
                fighting.0.mind.leader = Some(Seat(0));
            }
        }
        Ok(Run {
            last: start,
            world,
            host,
            a,
            b,
            foe,
            attacks: [0; 2],
            guest,
        })
    }

    /// `ms` of the host sending `command`, the foe's script aiming it at
    /// `aim`, and `action` sent in the first step.
    fn play(&mut self, ms: u64, command: Command, aim: Option<u32>, action: Option<Action>) {
        let mut action = action;
        for _ in 0..ms / STEP_MS {
            if let Some(aim) = aim {
                let mut state = self.world.script_state().clone();
                state.intents.insert(
                    self.foe,
                    Intent {
                        target: ObjectId(aim),
                        passive: false,
                        talent: None,
                    },
                );
                self.world.set_script_state(state);
            }
            let mut input = TickInput::default();
            input.insert(Seat(0), command);
            if self.guest {
                input.insert(Seat(1), Command::default());
            }
            if let Some(action) = action.take() {
                input.act(Seat(0), action);
            }
            let tick = self.last.tick + 1;
            self.last = crate::step(
                &mut self.world,
                tick,
                &input,
                STEP_MS as u32,
                StepReason::Authority,
            );
            for (held, id) in self.attacks.iter_mut().zip([self.host, self.b]) {
                let fighter = self.last.entities.iter().find(|entity| entity.id.0 == id);
                if fighter
                    .and_then(|entity| entity.fighter)
                    .is_some_and(|fighter| fighter.last_command.buttons & buttons::ATTACK != 0)
                {
                    *held += 1;
                }
            }
        }
    }

    fn entity(&self, id: u32) -> Option<&EntityState> {
        self.last.entities.iter().find(|entity| entity.id.0 == id)
    }

    /// `id` down, as a blow to 0 HP downs a party member.
    fn down(&mut self, id: u32) {
        let now = self.world.world.resource::<super::Clock>().time_ms;
        let mut fighters = self.world.world.query::<(&Id, &mut Fighting)>();
        for (of, mut fighting) in fighters.iter_mut(&mut self.world.world) {
            if of.0.0 == id {
                fighting.0.fall(now);
            }
        }
    }

    /// The body seat 0 moves.
    fn body(&self) -> Option<u32> {
        self.last.body(Seat(0)).map(|entity| entity.id.0)
    }

    /// Where `id` stands.
    fn at(&self, id: u32) -> Option<[f32; 3]> {
        self.entity(id)
            .and_then(|entity| entity.player)
            .map(|ps| ps.origin)
    }

    /// The seat driving `id`.
    fn driver(&self, id: u32) -> Option<Seat> {
        self.entity(id)
            .and_then(|entity| entity.fighter)
            .and_then(|fighter| fighter.mind.driver)
    }

    /// Whom `id` fights, and what it is doing.
    fn doing(&self, id: u32) -> (Option<u32>, Option<Task>) {
        let fighter = self.entity(id).and_then(|entity| entity.fighter);
        (
            fighter.and_then(|fighter| fighter.target).map(|id| id.0),
            fighter.map(|fighter| fighter.mind.task),
        )
    }

    /// How far apart `a` and `b` stand, seen from above (units).
    fn apart(&self, a: u32, b: u32) -> f32 {
        let at = |id: u32| self.entity(id).and_then(|entity| entity.player);
        at(a).zip(at(b)).map_or(f32::MAX, |(a, b)| {
            crate::math::flat_distance(a.origin, b.origin)
        })
    }
}

fn lead(id: u32) -> Option<Action> {
    Some(Action::Lead(SimId(id)))
}

pub fn probes() -> Checks {
    let mut checks: Checks = Vec::new();
    let mut scene = |make: &mut dyn FnMut(&mut Checks) -> Result<(), String>| {
        if let Err(why) = make(&mut checks) {
            checks.push(("party_fights_ground", Err(why)));
        }
    };
    let idle = Command::default();
    // Far enough that it stays out of the companions' way; in sight.
    let far = (-1.0, 0.0);

    scene(&mut |checks| {
        let mut run = Run::new(far)?;
        run.play(3000, idle, Some(run.host), None);
        let (a, b) = (run.doing(run.a), run.doing(run.b));
        checks.push((
            "companions_join_the_players_fight",
            check(
                a == (Some(run.foe), Some(Task::Engage))
                    && b == (Some(run.foe), Some(Task::Engage)),
                format!("foe #{}: a {a:?}, b {b:?}", run.foe),
            ),
        ));
        Ok(())
    });
    scene(&mut |checks| {
        let mut run = Run::new(far)?;
        run.play(3000, idle, None, None);
        let (a, b) = (run.doing(run.a), run.doing(run.b));
        checks.push((
            "no_fight_of_their_own",
            check(a.0.is_none() && b.0.is_none(), format!("a {a:?}, b {b:?}")),
        ));
        Ok(())
    });
    scene(&mut |checks| {
        let mut run = Run::new(far)?;
        run.play(3000, idle, Some(run.host), Some(Action::Solo(true)));
        let (a, b) = (run.doing(run.a), run.doing(run.b));
        checks.push((
            "solo_mode_holds_them_out",
            check(a.0.is_none() && b.0.is_none(), format!("a {a:?}, b {b:?}")),
        ));
        Ok(())
    });
    scene(&mut |checks| {
        // The one led stands across the room, the others far behind it.
        let mut run = Run::with((-6.0, 5.0), (4.0, 0.0))?;
        run.play(100, idle, None, lead(run.a));
        let before = (run.apart(run.host, run.a), run.apart(run.b, run.a));
        let start = run
            .entity(run.host)
            .and_then(|e| e.player)
            .map(|ps| ps.origin);
        run.play(5000, idle, None, None);
        let after = (run.apart(run.host, run.a), run.apart(run.b, run.a));
        let moved = start
            .zip(run.entity(run.host).and_then(|e| e.player))
            .map_or(0.0, |(from, ps)| {
                crate::math::flat_distance(from, ps.origin)
            });
        let near = crate::combat::ai::FOLLOW_NEAR;
        checks.push((
            "party_follows_the_one_led",
            check(
                before.0 > near && moved > 40.0 && after.0 <= near * 1.2 && after.1 <= near * 1.2,
                format!(
                    "the player moved {moved:.0} units; apart from the one led \
                     (player, other): {before:.0?}, then {after:.0?}, near {near:.0}"
                ),
            ),
        ));
        Ok(())
    });
    scene(&mut |checks| {
        let mut run = Run::new(far)?;
        run.play(100, idle, None, lead(run.a));
        run.play(3000, idle, Some(run.a), None);
        let (host, b) = (run.doing(run.host), run.doing(run.b));
        checks.push((
            "party_joins_the_one_leds_fight",
            check(
                host == (Some(run.foe), Some(Task::Engage))
                    && b == (Some(run.foe), Some(Task::Engage)),
                format!("foe #{}: the player {host:?}, the other {b:?}", run.foe),
            ),
        ));
        // It fights, by its mind, with nobody at its keys.
        run.attacks = [0; 2];
        run.play(5000, idle, Some(run.a), None);
        let hurt = |run: &Run, id: u32| {
            run.entity(id)
                .and_then(|entity| entity.fighter)
                .map(|fighter| fighter.hp / 100)
        };
        checks.push((
            "unmanned_player_attacks",
            check(
                run.attacks[0] > 0,
                format!(
                    "steps attack held: the player {}, the other companion {}; \
                     the player {:.0} units from the foe; HP: foe {:?}, the one led {:?}; {:?}",
                    run.attacks[0],
                    run.attacks[1],
                    run.apart(run.host, run.foe),
                    hurt(&run, run.foe),
                    hurt(&run, run.a),
                    run.doing(run.host),
                ),
            ),
        ));
        // Taken back: its player's, its fight over.
        run.play(100, idle, Some(run.a), lead(run.host));
        let host = run.doing(run.host);
        checks.push((
            "taken_back_is_its_players",
            check(
                host == (None, Some(Task::Idle)),
                format!("the player {host:?}"),
            ),
        ));
        Ok(())
    });
    checks
}

/// Control passing on from a fallen leader (`kjk sim probes --only
/// "downed switch"`; `world::scripts::passes_on`), on `Run`'s ground, the
/// foe fighting the party:
/// - the player's own character down: the first companion leads at once,
///   seat 0's body, and moves by the seat's keys, as Tab would make it;
/// - the companion led down: the next one standing leads;
/// - a guest's companion is passed over: the other one leads, the guest
///   keeps its own.
pub fn passing() -> Checks {
    let mut checks: Checks = Vec::new();
    let mut scene = |make: &mut dyn FnMut(&mut Checks) -> Result<(), String>| {
        if let Err(why) = make(&mut checks) {
            checks.push(("downed_switch_ground", Err(why)));
        }
    };
    let idle = Command::default();
    let far = (-1.0, 0.0);
    let forward = Command {
        forward: 127,
        ..Command::default()
    };
    scene(&mut |checks| {
        let mut run = Run::new(far)?;
        run.play(1500, idle, Some(run.host), None);
        run.down(run.host);
        run.play(100, idle, None, None);
        let led = (run.last.leading.map(|id| id.0), run.body());
        let start = run.at(run.a);
        run.play(1500, forward, None, None);
        let moved = start
            .zip(run.at(run.a))
            .map_or(0.0, |(from, to)| crate::math::flat_distance(from, to));
        checks.push((
            "the_players_fall_passes_control_on",
            check(
                led == (Some(run.a), Some(run.a)) && moved > 100.0,
                format!(
                    "leading, seat 0's body: {led:?} (companion #{}); \
                     it moved {moved:.0} units on the seat's keys",
                    run.a
                ),
            ),
        ));
        Ok(())
    });
    scene(&mut |checks| {
        let mut run = Run::new(far)?;
        run.play(100, idle, None, lead(run.a));
        run.play(1500, idle, Some(run.a), None);
        run.down(run.a);
        run.play(100, idle, None, None);
        let led = (run.last.leading.map(|id| id.0), run.body());
        checks.push((
            "the_led_ones_fall_passes_it_on",
            check(
                led == (Some(run.b), Some(run.b)),
                format!("leading, seat 0's body: {led:?} (next #{})", run.b),
            ),
        ));
        Ok(())
    });
    scene(&mut |checks| {
        let mut run = Run::with_guest(far)?;
        run.play(500, idle, Some(run.host), None);
        let before = run.driver(run.a);
        run.down(run.host);
        run.play(100, idle, None, None);
        let led = (run.last.leading.map(|id| id.0), run.body());
        let after = run.driver(run.a);
        checks.push((
            "a_guests_companion_is_passed_over",
            check(
                before == Some(Seat(1)) && after == before && led == (Some(run.b), Some(run.b)),
                format!(
                    "the guest drives #{} {before:?} then {after:?}; \
                     leading, seat 0's body: {led:?} (other #{})",
                    run.a, run.b
                ),
            ),
        ));
        Ok(())
    });
    checks
}
