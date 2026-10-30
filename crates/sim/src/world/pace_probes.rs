//! The probes of KOTOR's pace out of a fight (`kjk sim probes pace`;
//! `world::move_actors`, `walk::paced`), on the proving ground with K1's
//! `creaturespeed.2da` rows `PLAYER` (3.20 m/s walk, 5.40 run) and
//! `DEFAULT`, no install. The player runs at 5.40 m/s out of a fight and
//! walks at 3.20; in one (its weapon out) it runs at JA's 6.35, and leaving
//! it eases back to 5.40 over about half a second; a companion following
//! it runs at its own body's run, 5.40. A body with HUGE's rates walks
//! at 5.00 playing the walk and runs at 10.00. Two companions put on one
//! spot part, and following the player each keeps its own side of it
//! (`world::minds::follow_slot`).

use super::SimWorld;
use crate::combat::Sheet;
use crate::combat::ai::Mind;
use crate::input::{Command, Seat, TickInput, buttons};
use crate::module::{Body, ObjectData, ObjectKind, Placement};
use crate::probe::{Checks, check};
use crate::snapshot::{Kind, SimId};
use crate::{Snapshot, StepReason, WorldConfig};

/// A step's milliseconds.
const STEP_MS: u64 = 8;

/// A creature body at `creaturespeed.2da`'s `NORMAL` row.
fn body() -> Body {
    Body {
        appearance: 0,
        size: 3,
        movement: 4,
        walk_mps: 1.7,
        run_mps: 5.4,
        head_arc_h: 0.0,
        bag: 0,
        destroy_ms: 0,
        fade_ms: None,
    }
}

struct Run {
    world: SimWorld,
    level: crate::Level,
    last: Snapshot,
    host: SimId,
    buddy: SimId,
    buddies: Vec<SimId>,
}

impl Run {
    /// The ground with `player` as the `PLAYER` row's walk and run (m/s).
    fn new(player: (f32, f32)) -> Result<Run, String> {
        Run::with(player, 1)
    }

    /// `new`'s with `count` companions, all on the one spot.
    fn with(player: (f32, f32), count: usize) -> Result<Run, String> {
        Run::with_body(player, count, body())
    }

    /// `with`'s, the companions' bodies `made`.
    fn with_body(player: (f32, f32), count: usize, made: Body) -> Result<Run, String> {
        let ground = crate::determinism::proving_ground();
        let mut data = (**ground.module().ok_or("the ground has no module")?).clone();
        // 3 m behind the player, who arrives at (-4, 0) in the room's
        // frame and runs off the other way.
        let at = crate::determinism::ground_point(-4.0, -3.0, 0.0);
        for _ in 0..count {
            data.placements.push(Placement::Own(ObjectData {
                sheet: Some(Sheet::dev_soldier()),
                body: Some(made),
                ..ObjectData::bare(ObjectKind::Creature, "buddy", at)
            }));
        }
        let rates = crate::script::MoveRates {
            player: Some(player),
            default: Some((1.7, 5.4)),
        };
        let library = crate::probe::library("", Vec::<(String, _)>::new()).with_move_rates(rates);
        // The navigation mesh over the half the player arrives in.
        let [a, b, c, d] = [[-7.5, -5.5], [-0.5, -5.5], [-0.5, 5.5], [-7.5, 5.5]]
            .map(|[x, y]| crate::determinism::ground_point(x, y, 0.0));
        let mut nav = collision::nav::NavMesh::build(
            &[[a, b, c], [a, c, d]],
            crate::walk::CLEARANCE * collision::UNIT,
        );
        if let Some(level) = ground.collision().level() {
            nav.mark(level);
        }
        let level = ground.with_module(data).with_scripts(library).with_nav(nav);
        let mut world =
            SimWorld::new(WorldConfig::one_seat(), level.clone()).map_err(|e| e.to_string())?;
        let mut start = world.snapshot();
        let host = start.body(Seat(0)).ok_or("no host")?.id;
        let buddies: Vec<SimId> = start
            .entities
            .iter()
            .filter(|entity| entity.fighter.is_some() && entity.kind != Kind::Avatar(Seat(0)))
            .map(|entity| entity.id)
            .collect();
        let buddy = *buddies.first().ok_or("no buddy")?;
        for entity in &mut start.entities {
            if buddies.contains(&entity.id)
                && let Some(fighter) = entity.fighter.as_mut()
            {
                fighter.mind = Mind::companion(Seat(0));
            }
        }
        let world = SimWorld::from_snapshot(&start, level.clone()).map_err(|e| e.to_string())?;
        Ok(Run {
            world,
            level,
            last: start,
            host,
            buddy,
            buddies,
        })
    }

    /// `ms` of the host sending `command`.
    fn hold(&mut self, command: Command, ms: u64) {
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

    /// The host's fight until `ms` from now (0: none), as a blow sets it.
    fn fight_for(&mut self, ms: u64) -> Result<(), String> {
        let mut snapshot = self.last.clone();
        let now = snapshot.time_ms;
        // The companion, measured, keeps out of the player's way.
        snapshot.entities.retain(|entity| entity.id != self.buddy);
        for entity in &mut snapshot.entities {
            if entity.id == self.host
                && let Some(fighter) = entity.fighter.as_mut()
            {
                fighter.combat_until = now + ms;
            }
        }
        self.world =
            SimWorld::from_snapshot(&snapshot, self.level.clone()).map_err(|e| e.to_string())?;
        self.last = snapshot;
        Ok(())
    }

    /// `who`'s legs' animation.
    fn legs(&self, who: SimId) -> Option<crate::motion::Anim> {
        self.last
            .entities
            .iter()
            .find(|entity| entity.id == who)
            .and_then(|entity| entity.player)
            .map(|ps| ps.legs.anim)
    }

    /// `who`'s origin (units).
    fn origin(&self, who: SimId) -> Option<crate::math::Vec3> {
        let entity = self.last.entities.iter().find(|entity| entity.id == who)?;
        entity.player.map(|ps| ps.origin)
    }

    /// `who`'s ground speed (m/s).
    fn speed(&self, who: SimId) -> f32 {
        self.last
            .entities
            .iter()
            .find(|entity| entity.id == who)
            .map_or(0.0, |entity| crate::math::flat_length(entity.velocity))
    }
}

/// Facing `yaw`, the keys at `forward`, walking or not.
fn keys(yaw: f32, forward: i8, walk: bool) -> Command {
    Command {
        angles: [0, crate::math::degrees_to_short(yaw), 0],
        forward,
        buttons: if walk { buttons::WALKING } else { 0 },
        ..Command::default()
    }
}

/// Within 0.06 m/s of `want`.
fn near(speed: f32, want: f32) -> bool {
    (speed - want).abs() < 0.06
}

pub fn probes() -> Checks {
    let mut run = match Run::new((3.2, 5.4)) {
        Ok(run) => run,
        Err(error) => return vec![("pace_ground", Err(error))],
    };
    let mut checks: Checks = Vec::new();
    let mut put = |name: &'static str, ok: bool, what: String| checks.push((name, check(ok, what)));
    // A body its appearance gives no movement (`moverate` NOMOVE: a
    // turret), its mind following the player, in a fight and out: it moves
    // not at all (Telos's hidden turrets ran at JA's pace in a fight).
    let immobile = Body {
        movement: 1,
        walk_mps: 0.0,
        run_mps: 0.0,
        ..body()
    };
    match Run::with_body((3.2, 5.4), 1, immobile) {
        Ok(mut still) => {
            let from = still.origin(still.buddy);
            still.hold(keys(90.0, 127, false), 704);
            let calm = still.origin(still.buddy);
            // Both in a fight now (JA's pace), the player running back.
            let mut snapshot = still.last.clone();
            let now = snapshot.time_ms;
            for entity in &mut snapshot.entities {
                if let Some(fighter) = entity.fighter.as_mut() {
                    fighter.combat_until = now + 60_000;
                }
            }
            let fought = SimWorld::from_snapshot(&snapshot, still.level.clone())
                .map_err(|e| e.to_string())
                .map(|world| {
                    still.world = world;
                    still.last = snapshot;
                    still.hold(keys(270.0, 127, false), 704);
                    still.origin(still.buddy)
                });
            let moved = |at: Option<crate::math::Vec3>| {
                from.zip(at).map(|(a, b)| crate::math::flat_distance(a, b))
            };
            let (calm, fought) = (moved(calm), fought.map(moved));
            // A unit or two: the step's settling where it stands.
            let still_enough = |d: Option<f32>| d.is_some_and(|d| d < 4.0);
            put(
                "immobile_body_never_moves",
                still_enough(calm) && fought.as_ref().is_ok_and(|d| still_enough(*d)),
                format!("moved {calm:?} units out of a fight, {fought:?} in one"),
            );
        }
        Err(error) => put("immobile_ground", false, error),
    }
    // Along the room's half, 12 m across (`determinism::proving_ground`):
    // away from the companion, then back. The keys' yaw turns from where
    // the player arrives facing, along the room.
    let (away, back) = (90.0, 270.0);
    run.hold(keys(away, 127, false), 704);
    let calm = run.speed(run.host);
    put(
        "player_runs_at_kotor_pace_out_of_a_fight",
        near(calm, 5.4),
        format!("{calm:.2} m/s, KOTOR's PLAYER run 5.40"),
    );
    let buddy = run.speed(run.buddy);
    put(
        "companion_follows_at_its_own_run",
        (buddy - 5.4).abs() < 0.1,
        format!("{buddy:.2} m/s, its body's run 5.40"),
    );
    run.hold(keys(back, 64, true), 504);
    let walk = run.speed(run.host);
    put(
        "player_walks_at_kotor_pace_out_of_a_fight",
        near(walk, 3.2),
        format!("{walk:.2} m/s, KOTOR's PLAYER walk 3.20"),
    );
    if let Err(error) = run.fight_for(60_000) {
        put("pace_fight", false, error);
    }
    run.hold(keys(back, 127, false), 304);
    let fighting = run.speed(run.host);
    put(
        "player_runs_at_ja_pace_in_a_fight",
        near(fighting, 250.0 * collision::UNIT),
        format!("{fighting:.2} m/s, JA's run 6.35"),
    );
    if let Err(error) = run.fight_for(0) {
        put("pace_calm", false, error);
    }
    run.hold(keys(back, 127, false), 104);
    let easing = run.speed(run.host);
    run.hold(keys(back, 127, false), 448);
    let eased = run.speed(run.host);
    put(
        "player_eases_to_kotor_pace_leaving_a_fight",
        easing > 5.6 && easing < 6.35 && near(eased, 5.4),
        format!("{easing:.2} m/s 0.1 s after, {eased:.2} m/s 0.55 s after"),
    );
    // A body as fast as `creaturespeed.2da`'s HUGE row (5.00 walk, 10.00
    // run): its walk plays the walk however long its keys, and its run is
    // not held to JA's.
    let mut huge = match Run::new((5.0, 10.0)) {
        Ok(run) => run,
        Err(error) => return vec![("pace_huge_ground", Err(error))],
    };
    huge.hold(keys(away, 64, true), 504);
    let (walk, legs) = (huge.speed(huge.host), huge.legs(huge.host));
    put(
        "fast_walk_plays_the_walk",
        near(walk, 5.0) && legs == Some(crate::motion::Anim::BOTH_WALK1),
        format!("{walk:.2} m/s, legs {legs:?}; KOTOR's walk 5.00"),
    );
    huge.hold(keys(back, 127, false), 504);
    let (run, legs) = (huge.speed(huge.host), huge.legs(huge.host));
    put(
        "fast_run_beyond_ja_run",
        near(run, 10.0) && legs == Some(crate::motion::Anim::BOTH_RUN1),
        format!("{run:.2} m/s, legs {legs:?}; KOTOR's run 10.00"),
    );
    formation(&mut put);
    checks
}

/// Two companions put on one spot behind the standing player: they part.
/// The player runs off and stands: they follow and stand behind it, one on
/// each side of the way it ran, apart.
fn formation(put: &mut impl FnMut(&'static str, bool, String)) {
    let mut run = match Run::with((3.2, 5.4), 2) {
        Ok(run) => run,
        Err(error) => return put("formation_ground", false, error),
    };
    let away = 90.0;
    // Where each stands from the player, along and across `heading`
    // (metres), and how far apart they are.
    let places = |run: &Run, heading: crate::math::Vec3| {
        let host = run.origin(run.host)?;
        let [a, b] = [run.buddies[0], run.buddies[1]].map(|id| run.origin(id));
        let (a, b) = (a?, b?);
        let left = [-heading[1], heading[0], 0.0];
        let from = |at: crate::math::Vec3| {
            let d = crate::math::sub(at, host);
            let m = |v: crate::math::Vec3| crate::math::dot(d, v) * collision::UNIT;
            (m(heading), m(left))
        };
        let apart = crate::math::flat_distance(a, b) * collision::UNIT;
        Some((from(a), from(b), apart))
    };
    let apart_least = rules::minds::tasks::FOLLOW_APART * collision::UNIT;
    run.hold(keys(away, 0, false), 2000);
    let parted = places(&run, [0.0, 1.0, 0.0]).map(|(.., apart)| apart);
    put(
        "companions_on_one_spot_part",
        parted.is_some_and(|apart| apart > apart_least),
        format!("{parted:?} m apart, more than {apart_least:.2}"),
    );
    let start = run.origin(run.host);
    run.hold(keys(away, 127, false), 800);
    let heading = start.zip(run.origin(run.host)).map(|(from, to)| {
        let d = crate::math::sub(to, from);
        let length = crate::math::flat_length(d);
        [d[0] / length, d[1] / length, 0.0]
    });
    run.hold(keys(away, 0, false), 3000);
    let kept = heading.and_then(|heading| places(&run, heading));
    // Its place, within a stride: `FOLLOW_BACK` behind, `FOLLOW_SIDE` aside.
    let [back, side] = [
        rules::minds::tasks::FOLLOW_BACK,
        rules::minds::tasks::FOLLOW_SIDE,
    ]
    .map(|units| units * collision::UNIT);
    let placed = |(along, across): (f32, f32)| {
        (along + back).abs() < apart_least / 2.0 && (across.abs() - side).abs() < apart_least / 2.0
    };
    put(
        "companions_follow_on_either_side",
        kept.is_some_and(|(a, b, apart)| {
            placed(a) && placed(b) && a.1 * b.1 < 0.0 && apart > apart_least
        }),
        format!(
            "(along, across) of each from the player, metres apart: {kept:?}; places {back:.2} behind, {side:.2} aside"
        ),
    );
}
