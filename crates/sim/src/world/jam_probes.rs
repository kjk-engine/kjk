//! The probes of characters solid to each other and of getting unstuck
//! from one (`kjk sim probes --only jam`; `world::jams`, issue 280), on
//! the proving ground, no install. The player runs at a character standing
//! 1.5 m ahead of it, holding the keys down:
//! - a companion (solo mode, so it stands) holds the player off, as KOTOR's
//!   party is solid to itself; so does a neutral creature;
//! - after `walking.jam.stuck_ms` of running into it, the player walks
//!   through it and on;
//! - turned round once the two have parted, the player is held by it again.
//!
//! The rule's own bookkeeping (`Jams`) is checked step by step too: a break
//! starts the count again, and a third character to walk through makes the
//! oldest of the two before it solid again.

use super::BODY;
use super::jams::{Jams, blocked_by};
use crate::combat::Sheet;
use crate::combat::ai::Mind;
use crate::input::{Command, Seat, TickInput};
use crate::math::Vec3;
use crate::module::{ObjectData, ObjectKind, Placement};
use crate::motion::Jam;
use crate::probe::{Checks, check};
use crate::snapshot::{Kind, SimId};
use crate::{SimWorld, Snapshot, StepReason, WorldConfig};

/// A step's milliseconds.
const STEP_MS: u64 = 8;

/// How long the player runs into the other before it walks through
/// (sim milliseconds).
const STUCK_MS: u64 = rules::walking::jam::STUCK_MS as u64;

/// The player and one other character standing ahead of it.
struct Run {
    world: SimWorld,
    last: Snapshot,
    host: SimId,
    other: SimId,
}

impl Run {
    /// The other character, its mind a companion's (`companion`) or a
    /// creature's, standing 1.5 m ahead of where the player arrives at
    /// (-4, 0), along +Y.
    fn new(companion: bool) -> Result<Run, String> {
        let ground = crate::determinism::proving_ground();
        let mut data = (**ground.module().ok_or("the ground has no module")?).clone();
        let at = crate::determinism::ground_point(-4.0, 1.5, 0.0);
        data.placements.push(Placement::Own(ObjectData {
            sheet: Some(Sheet::commoner()),
            ..ObjectData::bare(ObjectKind::Creature, "other", at)
        }));
        let level = ground.with_module(data);
        let mut world =
            SimWorld::new(WorldConfig::one_seat(), level.clone()).map_err(|e| e.to_string())?;
        let mut start = world.snapshot();
        let host = start.body(Seat(0)).ok_or("no host")?.id;
        let other = start
            .entities
            .iter()
            .find(|entity| entity.fighter.is_some() && entity.kind != Kind::Avatar(Seat(0)))
            .ok_or("no other character")?
            .id;
        if companion {
            for entity in &mut start.entities {
                if entity.id == other
                    && let Some(fighter) = entity.fighter.as_mut()
                {
                    fighter.mind = Mind::companion(Seat(0));
                }
            }
            start.solo = true;
        }
        let world = SimWorld::from_snapshot(&start, level).map_err(|e| e.to_string())?;
        Ok(Run {
            world,
            last: start,
            host,
            other,
        })
    }

    /// `ms` of the player running along `yaw` (degrees).
    fn run(&mut self, yaw: f32, ms: u64) {
        let command = Command {
            angles: [0, crate::math::degrees_to_short(yaw), 0],
            forward: 127,
            ..Command::default()
        };
        for _ in 0..ms / STEP_MS {
            let mut input = TickInput::default();
            input.insert(Seat(0), command);
            let tick = self.last.tick + 1;
            self.last = crate::step(
                &mut self.world,
                tick,
                &input,
                STEP_MS as u32,
                StepReason::Authority,
            );
        }
    }

    fn origin(&self, who: SimId) -> Option<Vec3> {
        let entity = self.last.entities.iter().find(|entity| entity.id == who)?;
        entity.player.map(|ps| ps.origin)
    }

    /// How far (metres) the player stands past the other along +Y: below
    /// about minus a body's width it is held off short of it.
    fn past(&self) -> Option<f32> {
        let (host, other) = (self.origin(self.host)?, self.origin(self.other)?);
        Some((host[1] - other[1]) * collision::UNIT)
    }
}

/// Held off: the player's middle stays more than a body's half width
/// (metres) short of the other's, as their personal spaces keep them.
fn held(past: Option<f32>) -> bool {
    let half = crate::motion::HALF_WIDTH * collision::UNIT;
    past.is_some_and(|past| past <= -half)
}

/// The player runs into `who` standing ahead: held, then through after
/// `STUCK_MS`, then held again running back once they have parted.
fn runs_into(
    put: &mut impl FnMut(&'static str, Result<String, String>),
    companion: bool,
    names: [&'static str; 3],
) {
    let mut run = match Run::new(companion) {
        Ok(run) => run,
        Err(error) => return put(names[0], Err(error)),
    };
    let who = if companion {
        "a companion"
    } else {
        "a creature"
    };
    // The player reaches it in about 0.2 s, then runs into it.
    run.run(90.0, STUCK_MS - 100);
    let early = run.past();
    put(
        names[0],
        check(
            held(early),
            format!(
                "{early:?} m past {who} after {} ms of running",
                STUCK_MS - 100
            ),
        ),
    );
    run.run(90.0, 1500);
    let through = run.past();
    put(
        names[1],
        check(
            through.is_some_and(|past| past > 1.0),
            format!("{through:?} m past {who} after {} ms", STUCK_MS + 1400),
        ),
    );
    // Back the other way, from where it stands well clear: held again.
    run.run(270.0, 700);
    let back = run.past().map(|past| -past);
    put(
        names[2],
        check(
            held(back),
            format!("{back:?} m short of {who} again, 0.7 s after turning round"),
        ),
    );
}

/// The rule's bookkeeping, step by step.
fn bookkeeping(put: &mut impl FnMut(&'static str, Result<String, String>)) {
    let [a, b, c, d] = [1, 2, 3, 4].map(|id| BODY | id);
    let stuck = rules::walking::jam::STUCK_MS;
    let fresh = || (Jams::of(std::iter::empty()), Jam::default());
    // Half the time, a free step, half again, then someone else: no pass.
    let (mut jams, mut jam) = fresh();
    for by in [Some(b), None, Some(b), Some(c)] {
        jams.ran_into(a, &mut jam, by, stuck / 2);
    }
    let broken = !jams.passes(a, b) && !jams.passes(a, c);
    jams.ran_into(a, &mut jam, Some(c), stuck / 2);
    put(
        "jam_break_starts_the_count_again",
        check(
            broken && jams.passes(c, a),
            format!(
                "passing {:?} once the last one ran into for {stuck} ms",
                jam.passing
            ),
        ),
    );
    let (mut jams, mut jam) = fresh();
    for by in [b, c, d] {
        jams.ran_into(a, &mut jam, Some(by), stuck);
    }
    put(
        "jam_third_frees_the_oldest",
        check(
            jam.passing == [c, d] && !jams.passes(a, b) && jams.passes(a, c),
            format!("passing {:?} after three", jam.passing),
        ),
    );
    let crawl = rules::walking::jam::CRAWL_MPS / collision::UNIT * STEP_MS as f32 / 1000.0;
    let any = |_| true;
    let cases = [
        blocked_by(true, 0.0, 8, &[7, b, c], c, any) == Some(c),
        blocked_by(true, 0.0, 8, &[7, b, c], 0, any) == Some(b),
        blocked_by(true, crawl, 8, &[b], 0, any).is_none(),
        blocked_by(false, 0.0, 8, &[b], 0, any).is_none(),
        blocked_by(true, 0.0, 8, &[7], 0, any).is_none(),
        blocked_by(true, 0.0, 8, &[b], 0, |_| false).is_none(),
    ];
    put(
        "jam_blocked_only_slow_and_touching",
        check(
            cases.iter().all(|ok| *ok),
            format!("the last one ran into first, a level entity or a foe never: {cases:?}"),
        ),
    );
}

pub fn probes() -> Checks {
    let mut checks: Checks = Vec::new();
    let mut put = |name: &'static str, result: Result<String, String>| checks.push((name, result));
    runs_into(
        &mut put,
        true,
        [
            "companion_holds_the_player_off",
            "player_walks_through_a_companion_once_stuck",
            "companion_solid_again_once_parted",
        ],
    );
    runs_into(
        &mut put,
        false,
        [
            "creature_holds_the_player_off",
            "player_walks_through_a_creature_once_stuck",
            "creature_solid_again_once_parted",
        ],
    );
    bookkeeping(&mut put);
    checks
}
