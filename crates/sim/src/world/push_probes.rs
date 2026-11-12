//! The probes of companions fighting round a player who stands in their
//! way (`kjk sim probes --only push`; `world::movement`), on the proving
//! ground with scripts, no install. The player stands between its two
//! companions and a foe, so the companions' way to the foe runs through
//! the player. KOTOR's walker bumps into a player and goes round it, and
//! never pushes one (QUIRKS.md Q-0267). So neither companion may move the
//! player, nor walk into its body:
//! - the companions fighting by their minds alone;
//! - the companions with a script's move to a spot past the player under
//!   way as well, as the party's AI scripts leave one (`ActionFollowLeader`,
//!   `ActionMoveAwayFromObject`): such a companion passed through the
//!   party, and through the player;
//! - the same, with a guest's player (co-op) standing in the way and the
//!   host out of it.

use script_ncs::vm::ObjectId;

use super::{Fighting, Id, Player};
use crate::combat::Sheet;
use crate::combat::sheet::factions;
use crate::input::{Command, Seat, TickInput};
use crate::math::Vec3;
use crate::module::{ObjectData, ObjectKind, Placement, Repute};
use crate::probe::{Checks, check};
use crate::script::actions::{Goal, Intent};
use crate::script::party::Npc;
use crate::{SimWorld, Snapshot, StepReason, WorldConfig};

/// A step's milliseconds.
const STEP_MS: u64 = 8;

/// How long the fight is watched (sim milliseconds).
const FIGHT_MS: u64 = 8000;

/// The most (metres) the standing player may end up from where it stood.
const MOVED_M: f32 = 0.05;

/// The nearest (metres, centre to centre) a companion may come to the
/// player: a little short of their two bodies' widths touching, as the
/// player's own personal space lets a body come.
const NEAREST_M: f32 = 0.6;

/// The farthest (metres) a companion may stay from the foe: it fought.
const FOUGHT_M: f32 = 1.5;

const COMPANIONS: [&str; 2] = ["comp_a", "comp_b"];
const FOE: &str = "foe";

/// Where the player in the way stands, where the companions start, where
/// the foe stands and where the script's move takes the companions
/// (metres, the ground's own axes): the companions behind the player, the
/// foe and the spot ahead of it, all on one line.
const STANDS: (f32, f32) = (-4.0, 0.0);
const STARTS: [(f32, f32); 2] = [(-6.5, -0.3), (-6.5, 0.4)];
const FOE_AT: (f32, f32) = (-1.0, 0.1);
const MOVED_TO: (f32, f32) = (-1.5, 0.0);

/// The host's place when a guest's player stands in the way: off the line.
const HOST_ASIDE: (f32, f32) = (-5.0, -3.0);

struct Run {
    world: SimWorld,
    last: Snapshot,
    /// The player standing in the way.
    standing: u32,
    companions: [u32; 2],
    foe: u32,
    seats: u8,
    /// The script's move the companions have under way, if any.
    goal: Option<Goal>,
}

/// A melee fighter of `faction`: the soldier with its melee weapon alone.
fn fighter(faction: u8) -> Sheet {
    let soldier = Sheet::dev_soldier();
    Sheet {
        faction,
        weapons: [soldier.weapons[1], None],
        ..soldier
    }
}

/// A point of the ground (metres, its own axes) as a body's origin there
/// (units).
fn origin(at: (f32, f32)) -> Vec3 {
    crate::script::standing(crate::determinism::ground_point(at.0, at.1, 0.0))
}

impl Run {
    /// The ground with the party and its foe; with `guest`, a second seat's
    /// player stands in the way and the host stands aside; with `moved`,
    /// the companions have a script's move under way.
    fn new(guest: bool, moved: bool) -> Result<Run, String> {
        let ground = crate::determinism::proving_ground();
        let mut data = (**ground.module().ok_or("the ground has no module")?).clone();
        data.repute = Repute::at_war([factions::PLAYER, factions::HOSTILE_1]);
        let creature = |tag: &str, faction: u8, (x, y): (f32, f32)| {
            Placement::Own(ObjectData {
                sheet: Some(fighter(faction)),
                template: tag.to_string(),
                ..ObjectData::bare(
                    ObjectKind::Creature,
                    tag,
                    crate::determinism::ground_point(x, y, 0.0),
                )
            })
        };
        for (tag, at) in COMPANIONS.into_iter().zip(STARTS) {
            data.placements.push(creature(tag, factions::PLAYER, at));
        }
        data.placements
            .push(creature(FOE, factions::HOSTILE_1, FOE_AT));
        let library = crate::probe::library("", Vec::<(String, _)>::new());
        let [p, q, r, s] = [[-7.5, -5.5], [-0.5, -5.5], [-0.5, 5.5], [-7.5, 5.5]]
            .map(|[x, y]| crate::determinism::ground_point(x, y, 0.0));
        let mut nav = collision::nav::NavMesh::build(
            &[[p, q, r], [p, r, s]],
            crate::walk::CLEARANCE * collision::UNIT,
        );
        if let Some(level) = ground.collision().level() {
            nav.mark(level);
        }
        let level = ground.with_module(data).with_scripts(library).with_nav(nav);
        let seats = if guest { 2 } else { 1 };
        let config = WorldConfig {
            seats,
            guests: crate::Guests::Avatars,
            ..WorldConfig::one_seat()
        };
        let mut world = SimWorld::new(config, level).map_err(|e| e.to_string())?;
        let start = world.snapshot();
        let seat = |seat: u8| {
            start
                .body(Seat(seat))
                .map(|entity| entity.id.0)
                .ok_or(format!("no player at seat {seat}"))
        };
        let host = seat(0)?;
        let standing = if guest { seat(1)? } else { host };
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
        let mut places = vec![(standing, origin(STANDS))];
        if guest {
            places.push((host, origin(HOST_ASIDE)));
        }
        let mut bodies = world.world.query::<(&Id, &mut Player)>();
        for (id, mut player) in bodies.iter_mut(&mut world.world) {
            if let Some((_, at)) = places.iter().find(|(who, _)| *who == id.0.0) {
                player.0.origin[0] = at[0];
                player.0.origin[1] = at[1];
            }
        }
        let mut fighters = world.world.query::<(&Id, &mut Fighting)>();
        for (id, mut fighting) in fighters.iter_mut(&mut world.world) {
            if [a, b].contains(&id.0.0) {
                fighting.0.mind.leader = Some(Seat(0));
            }
        }
        let goal = moved.then(|| Goal {
            point: origin(MOVED_TO),
            range: crate::walk::REACHED,
            run: true,
        });
        Ok(Run {
            last: world.snapshot(),
            world,
            standing,
            companions: [a, b],
            foe,
            seats,
            goal,
        })
    }

    fn at(&self, id: u32) -> Option<Vec3> {
        self.last
            .entities
            .iter()
            .find(|entity| entity.id.0 == id)
            .and_then(|entity| entity.player)
            .map(|ps| ps.origin)
    }

    /// `ms` of every seat standing still, the foe's script aiming it at the
    /// player in the way and the companions' move (if any) kept under way.
    fn fight(&mut self, ms: u64) -> Watched {
        let metres = |a: Vec3, b: Vec3| crate::math::flat_distance(a, b) * collision::UNIT;
        let from = self.at(self.standing).unwrap_or(origin(STANDS));
        let mut watched = Watched {
            moved: 0.0,
            nearest: f32::MAX,
            fought: f32::MAX,
        };
        for _ in 0..ms / STEP_MS {
            let mut state = self.world.script_state().clone();
            state.intents.insert(
                self.foe,
                Intent {
                    target: ObjectId(self.standing),
                    passive: false,
                    talent: None,
                },
            );
            if let Some(goal) = self.goal {
                for id in self.companions {
                    state.goals.insert(id, goal);
                }
            }
            self.world.set_script_state(state);
            let mut input = TickInput::default();
            for seat in 0..self.seats {
                input.insert(Seat(seat), Command::default());
            }
            let tick = self.last.tick + 1;
            self.last = crate::step(
                &mut self.world,
                tick,
                &input,
                STEP_MS as u32,
                StepReason::Authority,
            );
            let now = self.at(self.standing).unwrap_or(from);
            let foe = self.at(self.foe).unwrap_or(now);
            watched.moved = watched.moved.max(metres(from, now));
            for id in self.companions {
                let at = self.at(id).unwrap_or(foe);
                watched.nearest = watched.nearest.min(metres(at, now));
                watched.fought = watched.fought.min(metres(at, foe));
            }
        }
        watched
    }
}

/// What a fight did (metres): the farthest the standing player stood from
/// where it started, the nearest a companion came to it, and the nearest
/// one came to the foe.
struct Watched {
    moved: f32,
    nearest: f32,
    fought: f32,
}

/// The player standing in the way through `FIGHT_MS` of the fight: moved
/// no more than `MOVED_M`, no companion nearer than `NEAREST_M`, and the
/// companions at the foe.
fn stands_its_ground(guest: bool, moved: bool) -> Result<String, String> {
    let mut run = Run::new(guest, moved)?;
    let Watched {
        moved,
        nearest,
        fought,
    } = run.fight(FIGHT_MS);
    check(
        moved <= MOVED_M && nearest >= NEAREST_M && fought <= FOUGHT_M,
        format!(
            "over {FIGHT_MS} ms of the fight, the player in the way moved {moved:.2} m; \
             the companions came within {nearest:.2} m of it and {fought:.2} m of the foe"
        ),
    )
}

pub fn probes() -> Checks {
    vec![
        (
            "companions_fight_round_the_player",
            stands_its_ground(false, false),
        ),
        (
            "companions_moved_by_a_script_meet_the_player",
            stands_its_ground(false, true),
        ),
        (
            "companions_moved_by_a_script_meet_a_guests_player",
            stands_its_ground(true, true),
        ),
    ]
}
