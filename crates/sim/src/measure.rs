//! Movement's measurements (`kjk move measure`, the approved test
//! `movement_measures`): speeds, jumps, steps, slopes, a roll, drops and a
//! tunnel, each run by a scripted player on a small world of its own, as
//! numbers to compare with JA's (`docs/MOVEMENT.md`).
//!
//! The measuring is not sim state: it may use the platform's `tan` to
//! build its ramps and `powi` for distances.

use std::sync::Arc;

use collision::{Brush, BrushWorld, UNIT};

use crate::input::{Seat, TickInput};
use crate::motion::{self, Anim, AnimTable, MotionState};
use crate::moves::Script;
use crate::rate::TickRate;
use crate::snapshot::Snapshot;
use crate::world::{Level, SimWorld, StepReason, WorldConfig, step};

/// One player's state after each tick of a run, from tick 0.
#[derive(Debug, Clone, Default)]
pub struct Track {
    pub hz: u32,
    /// (ms, state).
    pub states: Vec<(u64, MotionState)>,
    /// When a module install moved the player (ms): a move, not a flight.
    pub moved_at: Vec<u64>,
}

/// Seat 0's player in `snapshot`.
pub fn player(snapshot: &Snapshot) -> MotionState {
    snapshot
        .entities
        .iter()
        .find_map(|e| e.player)
        .expect("seat 0 has a player")
}

impl Track {
    /// The soles at tick `i` (units).
    pub fn feet(&self, i: usize) -> [f32; 3] {
        let o = self.states[i].1.origin;
        [o[0], o[1], o[2] + motion::SOLE_Z]
    }

    /// Horizontal speed (units/s) at each tick.
    pub fn speeds(&self) -> Vec<f32> {
        self.states
            .iter()
            .map(|(_, s)| (s.velocity[0] * s.velocity[0] + s.velocity[1] * s.velocity[1]).sqrt())
            .collect()
    }

    /// Distance covered per second over ticks `from..to` (units/s).
    pub fn mean_speed(&self, from: usize, to: usize) -> f32 {
        let (a, b) = (self.feet(from), self.feet(to));
        let d = ((b[0] - a[0]).powi(2) + (b[1] - a[1]).powi(2)).sqrt();
        let t = (self.states[to].0 - self.states[from].0) as f32 / 1000.0;
        d / t
    }

    /// Each stretch in the air: (take-off ms, landing ms, highest soles
    /// above take-off, horizontal distance).
    pub fn flights(&self) -> Vec<(u64, u64, f32, f32)> {
        let mut flights = Vec::new();
        let mut from: Option<usize> = None;
        for i in 1..self.states.len() {
            let (air_now, air_before) = (
                self.states[i].1.ground.is_none(),
                self.states[i - 1].1.ground.is_none(),
            );
            if air_now && !air_before {
                from = Some(i - 1);
            }
            if !air_now
                && air_before
                && let Some(start) = from.take()
            {
                let z0 = self.feet(start)[2];
                let apex = (start..=i)
                    .map(|k| self.feet(k)[2] - z0)
                    .fold(f32::MIN, f32::max);
                let (a, b) = (self.feet(start), self.feet(i));
                let d = ((b[0] - a[0]).powi(2) + (b[1] - a[1]).powi(2)).sqrt();
                let (from, to) = (self.states[start].0, self.states[i].0);
                if !self.moved_at.iter().any(|ms| (from..=to).contains(ms)) {
                    flights.push((from, to, apex, d));
                }
            }
        }
        flights
    }

    /// The animations the legs played, with when each started (ms).
    pub fn legs(&self) -> Vec<(u64, &'static str)> {
        let mut out: Vec<(u64, &'static str)> = Vec::new();
        let mut last: Option<(Anim, bool)> = None;
        for (ms, s) in &self.states {
            let now = (s.legs.anim, s.legs.flip);
            if last != Some(now) {
                out.push((*ms, s.legs.anim.name()));
                last = Some(now);
            }
        }
        out
    }
}

/// Steps `script` for seat 0 in `level` from `spawn` (soles, metres),
/// looking along `yaw`, for `ms` of sim time, at the default rate.
pub fn drive(level: &Level, spawn: [f32; 3], yaw: f32, script: &Script, ms: u64) -> Track {
    let config = WorldConfig {
        seed: 1,
        seats: 1,
        wanderers: 0,
        spawn,
        yaw,
        sheet: crate::combat::Sheet::dev_soldier(),
        guests: crate::Guests::Avatars,
    };
    let rate = TickRate::default();
    let mut world = SimWorld::new(config, level.clone())
        .unwrap_or_else(|error| panic!("a world to drive in: {error}"));
    let mut last = world.snapshot();
    let mut states = vec![(0, player(&last))];
    let mut tick = last.tick;
    while rate.time_ms(tick) < ms {
        tick += 1;
        let mut input = TickInput::default();
        input.insert(Seat(0), script.command(rate.time_ms(tick - 1)));
        last = step(
            &mut world,
            tick,
            &input,
            rate.msec(tick),
            StepReason::Authority,
        );
        states.push((last.time_ms, player(&last)));
    }
    Track {
        hz: rate.hz(),
        states,
        moved_at: Vec::new(),
    }
}

/// A flat floor at z 0, 200 m square.
fn flat() -> BrushWorld {
    BrushWorld::new().with(Brush::cuboid([-100.0, -100.0, -1.0], [100.0, 100.0, 0.0]))
}

fn level_of(world: BrushWorld, anims: &AnimTable) -> Level {
    Level::new("measure", Arc::new(world), Arc::new(anims.clone()))
}

/// JA's vertical motion in free air as its movement steps it, written out
/// on its own: each 8 ms the move averages the speed before and after
/// gravity, then the speed is snapped to whole units. The apex (units) and
/// the time back at take-off height (ms).
pub fn ja_jump() -> (f32, u32) {
    let (mut z, mut v, mut ms, mut apex) = (0.0f32, motion::JUMP_SPEED, 0u32, 0.0f32);
    loop {
        let end = v - motion::GRAVITY * 0.008;
        z += (v + end) * 0.5 * 0.008;
        v = end.round_ties_even();
        ms += 8;
        apex = apex.max(z);
        if z <= 0.0 {
            return (apex, ms);
        }
    }
}

/// Holding a direction for 3 s, then letting go for 1 s.
#[derive(Debug, Clone, PartialEq)]
pub struct Speed {
    pub name: &'static str,
    /// JA's number (units/s).
    pub expected: f32,
    /// Top horizontal speed while held, and the mean over the last second
    /// held (units/s).
    pub top: f32,
    pub mean: f32,
    /// ms to 95% of the top.
    pub rise_ms: usize,
    /// Units slid after letting go.
    pub stop: f32,
    /// The speed every other tick.
    pub curve: Vec<f32>,
}

/// One flight of a jump run.
#[derive(Debug, Clone, PartialEq)]
pub struct Jump {
    pub name: &'static str,
    /// Highest soles above take-off (units), time in the air, distance
    /// along (units).
    pub apex: f32,
    pub air_ms: u64,
    pub distance: f32,
}

/// Running at a step 2 m ahead.
#[derive(Debug, Clone, PartialEq)]
pub struct Step {
    pub rise: f32,
    pub walked_up: bool,
    /// Highest soles (units).
    pub highest: f32,
    pub step_event: bool,
    /// Where the run ended along Y (metres).
    pub end_y: f32,
}

/// Running up a ramp and back down.
#[derive(Debug, Clone, PartialEq)]
pub struct Slope {
    pub degrees: f32,
    /// The ramp's rise (metres) and the highest the soles got.
    pub rise: f32,
    pub reached_top: bool,
    pub top: f32,
    /// Level speed up the ramp (units/s).
    pub up_speed: f32,
    /// ms off the ground coming down.
    pub air_down_ms: usize,
}

/// A roll: (from ms, to ms, distance in units).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Roll {
    pub from: u64,
    pub to: u64,
    pub distance: f32,
}

/// Off a ledge.
#[derive(Debug, Clone, PartialEq)]
pub struct Drop {
    pub height: f32,
    pub keys: &'static str,
    pub legs: Vec<(u64, &'static str)>,
    /// The landing animation and when it started.
    pub landed_with: Option<(u64, &'static str)>,
    pub fall_event: bool,
}

/// Crouching into a 43-unit tunnel and letting go inside.
#[derive(Debug, Clone, PartialEq)]
pub struct Tunnel {
    pub stays_crouched_inside: bool,
    /// The soles at the end (units), and whether the player stands there.
    pub end: [f32; 3],
    pub standing_at_end: bool,
}

/// Everything `measure` measured.
#[derive(Debug, Clone, PartialEq)]
pub struct Measures {
    pub speeds: Vec<Speed>,
    /// JA's jump written out (`ja_jump`): apex units, ms in the air.
    pub ja_apex: f32,
    pub ja_air_ms: u32,
    pub jumps: Vec<Jump>,
    /// The standing jump's soles every tick (units, to the hundredth).
    pub standing_jump_arc: Vec<f32>,
    pub steps: Vec<Step>,
    pub slopes: Vec<Slope>,
    pub rolls: Vec<Roll>,
    /// `BOTH_ROLL_F`'s length in the table measured with.
    pub roll_anim_ms: i32,
    pub roll_legs: Vec<(u64, &'static str)>,
    pub drops: Vec<Drop>,
    pub tunnel: Tunnel,
}

/// Speeds, jumps, steps, slopes, crouching, rolls and drops, each on a
/// world of its own, with `table`'s animation lengths.
pub fn measure(table: &AnimTable) -> Measures {
    let flat_level = level_of(flat(), table);
    let script = |text: &str| Script::parse(text).expect("measurement scripts parse");

    // Speeds: hold a direction for 3 s, then let go for 1 s.
    let mut speeds = Vec::new();
    for (name, keys, expected) in [
        ("run", "w", 250.0),
        ("walk", "w walk", 250.0 * 64.0 / 127.0),
        ("backward", "s", 187.5),
        ("strafe", "d", 250.0),
        ("diagonal", "w d", 250.0),
        ("crouched", "w crouch", 125.0),
        ("backward walk", "s walk", 250.0 * 64.0 / 127.0),
    ] {
        let track = drive(
            &flat_level,
            [0.0; 3],
            90.0,
            &script(&format!("3s {keys}; 1s")),
            4000,
        );
        let v = track.speeds();
        let held = 3000 / 8;
        let top = v[..=held].iter().copied().fold(0.0, f32::max);
        let mean = track.mean_speed(held - 125, held);
        let rise_ms = v.iter().position(|&s| s >= 0.95 * top).unwrap_or(0) * 8;
        let a = track.feet(held);
        let b = track.feet(v.len() - 1);
        let stop = ((b[0] - a[0]).powi(2) + (b[1] - a[1]).powi(2)).sqrt();
        speeds.push(Speed {
            name,
            expected,
            top,
            mean,
            rise_ms,
            stop,
            curve: v
                .iter()
                .step_by(2)
                .map(|s| (s * 10.0).round() / 10.0)
                .collect(),
        });
    }

    // Jumps: standing, and running (after 1 s of running).
    let (ja_apex, ja_air_ms) = ja_jump();
    let mut jumps = Vec::new();
    let mut standing_jump_arc = Vec::new();
    for (name, text) in [
        ("standing", "200ms; 100ms jump; 1500ms"),
        ("running", "1s w; 100ms w jump; 1500ms w"),
        ("held", "200ms; 1500ms jump"),
    ] {
        let track = drive(&flat_level, [0.0; 3], 90.0, &script(text), 2000);
        for (take_off, land, apex, distance) in track.flights() {
            jumps.push(Jump {
                name,
                apex,
                air_ms: land - take_off,
                distance,
            });
        }
        if name == "standing" {
            standing_jump_arc = track
                .states
                .iter()
                .map(|(_, s)| ((s.origin[2] + motion::SOLE_Z) * 100.0).round() / 100.0)
                .collect();
        }
    }

    // Steps: run at a step 2 m ahead.
    let mut steps = Vec::new();
    for rise in [8.0f32, 16.0, 17.9, 18.0, 18.2, 20.0, 24.0] {
        let world = flat().with(Brush::cuboid([-2.0, 2.0, 0.0], [2.0, 6.0, rise * UNIT]));
        let track = drive(
            &level_of(world, table),
            [0.0; 3],
            90.0,
            &script("1s w"),
            1000,
        );
        let highest = (0..track.states.len())
            .map(|i| track.feet(i)[2])
            .fold(f32::MIN, f32::max);
        let step_event = track
            .states
            .iter()
            .any(|(_, s)| s.events & motion::events::STEP != 0);
        let end = track.feet(track.states.len() - 1);
        steps.push(Step {
            rise,
            walked_up: (highest - rise).abs() < 0.5,
            highest,
            step_event,
            end_y: end[1] * UNIT,
        });
    }

    // Slopes: run up a ramp 3 m long, then turn round and run down it.
    let mut slopes = Vec::new();
    for degrees in [20.0f32, 30.0, 40.0, 44.0, 45.6, 46.0, 50.0, 60.0] {
        let run = 4.0;
        // The portable pair, not the platform's `tan`.
        let (sin, cos) = collision::trig::sin_cos_deg(degrees);
        let rise = run * (sin / cos);
        let world = flat()
            .with(Brush::ramp(
                [-3.0, 2.0, 0.0],
                [3.0, 2.0 + run, rise],
                1,
                false,
            ))
            .with(Brush::cuboid([-3.0, 2.0 + run, 0.0], [3.0, 12.0, rise]));
        let track = drive(
            &level_of(world, table),
            [0.0; 3],
            90.0,
            &script("3s w; 2500ms w yaw 270"),
            5500,
        );
        let top = track
            .states
            .iter()
            .map(|(_, s)| (s.origin[2] + motion::SOLE_Z) * UNIT)
            .fold(0.0, f32::max);
        // Along the ramp going up: the horizontal speed.
        let on_ramp: Vec<usize> = (1..track.states.len())
            .filter(|&i| {
                let f = track.feet(i);
                let y = f[1] * UNIT;
                y > 2.5 && y < 2.0 + run - 0.5 && track.states[i].0 < 3000
            })
            .collect();
        let up_speed = match (on_ramp.first(), on_ramp.last()) {
            (Some(&a), Some(&b)) if b > a + 10 => track.mean_speed(a, b),
            _ => 0.0,
        };
        let air_down_ms = track
            .states
            .iter()
            .filter(|(ms, s)| *ms > 3000 && s.ground.is_none())
            .count()
            * 8;
        slopes.push(Slope {
            degrees,
            rise,
            reached_top: top > rise - 0.05,
            top,
            up_speed,
            air_down_ms,
        });
    }

    // A roll: run, then crouch.
    let track = drive(
        &flat_level,
        [0.0; 3],
        90.0,
        &script("1s w; 300ms w crouch; 2s"),
        3300,
    );
    let mut rolls = Vec::new();
    let mut start = None;
    for (ms, s) in &track.states {
        let rolling = matches!(
            s.legs.anim,
            Anim::BOTH_ROLL_F | Anim::BOTH_ROLL_B | Anim::BOTH_ROLL_L | Anim::BOTH_ROLL_R
        ) && s.legs.timer > 0;
        match (rolling, start) {
            (true, None) => start = Some(*ms),
            (false, Some(from)) => {
                let (i, j) = ((from / 8) as usize, (*ms / 8) as usize);
                let (a, b) = (track.feet(i), track.feet(j));
                let distance = ((b[0] - a[0]).powi(2) + (b[1] - a[1]).powi(2)).sqrt();
                rolls.push(Roll {
                    from,
                    to: *ms,
                    distance,
                });
                start = None;
            }
            _ => {}
        }
    }
    let roll_legs = track.legs();

    // Drops: off ledges, running on, or letting go at the edge.
    let mut drops = Vec::new();
    for (height, keys) in [40.0f32, 63.0, 65.0, 80.0, 200.0]
        .into_iter()
        .flat_map(|h| [(h, "1500ms w; 1s"), (h, "250ms w; 2250ms")])
    {
        let world = BrushWorld::new()
            .with(Brush::cuboid([-100.0, -100.0, -1.0], [100.0, 100.0, 0.0]))
            .with(Brush::cuboid([-3.0, -3.0, 0.0], [3.0, 1.0, height * UNIT]));
        let track = drive(
            &level_of(world, table),
            [0.0, 0.62, height * UNIT],
            90.0,
            &script(keys),
            2500,
        );
        let legs = track.legs();
        let landed_with = legs
            .iter()
            .find(|(_, n)| n.starts_with("BOTH_LAND"))
            .copied();
        let events: u8 = track.states.iter().fold(0, |all, (_, s)| all | s.events);
        drops.push(Drop {
            height,
            keys,
            legs,
            landed_with,
            fall_event: events & motion::events::FALL != 0,
        });
    }

    // A tunnel 43 units high: crouch in, let go inside, come out.
    let world = flat().with(Brush::cuboid([-3.0, 2.0, 43.0 * UNIT], [3.0, 5.0, 2.0]));
    let track = drive(
        &level_of(world, table),
        [0.0; 3],
        90.0,
        &script("1200ms w crouch; 1s w"),
        2200,
    );
    let stays_crouched_inside = track.states.iter().all(|(ms, s)| {
        let y = (s.origin[1]) * UNIT;
        !(y > 2.0 - 0.4 && y < 5.0 + 0.4 && *ms > 1200) || s.ducked()
    });
    let tunnel = Tunnel {
        stays_crouched_inside,
        end: track.feet(track.states.len() - 1),
        standing_at_end: !track.states.last().expect("a track").1.ducked(),
    };

    Measures {
        speeds,
        ja_apex,
        ja_air_ms,
        jumps,
        standing_jump_arc,
        steps,
        slopes,
        rolls,
        roll_anim_ms: table.get(Anim::BOTH_ROLL_F).length_ms(),
        roll_legs,
        drops,
        tunnel,
    }
}
