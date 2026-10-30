//! The playthrough's minigame players (`race`, `turrets`): commands a
//! player's hands would give, from what the snapshot shows of the course
//! (`sim::minigame`), never the course's state set by hand.
//!
//! - A race: fire (the gear shift, the race's own script: K1's `OnFire`,
//!   TSL's `OnAccelerate`) pressed and let go once the countdown is over,
//!   which shifts as soon as the gear allows; steered for the nearest pad
//!   ahead, round the buoys in the way (a TSL race's mines too, jumped when
//!   close).
//! - The turret: the view turned to the nearest fighter, ahead of it by the
//!   bolts' flight, fire held while the aim is on it; in a turret with no
//!   fighters, whose bolts meet the area (TSL's 107PER: the Sith troopers
//!   running at the Ebon Hawk), the nearest foe there, as a player shoots
//!   them.

use bevy::math::{Quat, Vec3};
use collision::trig;
use sim::minigame::{Course, Rules};
use sim::{Command, buttons};

/// Where `snapshot`'s living creatures not of the party stand (metres,
/// their bodies' origins, as the course's places are): what a turret's
/// bolts meet in an area (`sim::world::minigame`'s `area_bodies`), so a
/// turret with no fighters shoots at them (`turret`).
pub fn area_foes(snapshot: &sim::Snapshot) -> Vec<[f32; 3]> {
    snapshot
        .entities
        .iter()
        .filter(|e| e.player.is_some() && e.fighter.is_some_and(|f| f.alive()))
        .filter(|e| !sim::party_member(e))
        .map(|e| e.position)
        .collect()
}

/// How long a driver's key is pressed, then let go (ms): fire in a race,
/// use and a medpac in a playthrough.
pub const PRESS_MS: u64 = 150;

/// Whether a key pressed and let go by turns, `PRESS_MS` each, is down at
/// `now_ms`.
pub fn pressed(now_ms: u64) -> bool {
    (now_ms / PRESS_MS).is_multiple_of(2)
}
/// How far ahead a pad is made for, and a buoy steered round (metres).
const PAD_AHEAD_M: f32 = 160.0;
const BUOY_AHEAD_M: f32 = 90.0;
/// Room kept from a buoy and from the tunnel's walls (metres).
const BUOY_ROOM_M: f32 = 2.0;
const WALL_ROOM_M: f32 = 1.0;
/// How near a thing in the way a TSL race's swoop jumps (metres).
const JUMP_AHEAD_M: f32 = 12.0;
/// How far ahead a TSL race's driver looks, at least, in the seconds the
/// swoop takes to get there: a pad, a buoy, the jump (its swoop tops
/// 200 m/s, where the metres above are a fraction of a second; the jump's
/// by the world's distance to the thing's box, measured on Telos's track:
/// later or sooner, the swoop comes down on the walls).
const PAD_AHEAD_S: f32 = 2.0;
const BUOY_AHEAD_S: f32 = 1.2;
const JUMP_AHEAD_S: f32 = 0.1;
/// How fast the swoop is steered toward where it heads (a second, per
/// metre off), at most (metres a second).
const STEER_GAIN: f32 = 2.0;
const STEER_MAX: f32 = 30.0;
/// The share of its gear's top speed a TSL race's swoop shifts at.
const TOPPED_OUT: f32 = 0.97;
/// How near the aim must be to fire (degrees).
const ON_TARGET_DEG: f32 = 4.0;

/// A box: its middle and half its size along each axis.
type Box3 = ([f32; 3], [f32; 3]);

/// The global the race's script keeps its gear in (below 0 in the
/// countdown).
pub const GEAR: &str = "MIN_RACE_GEAR";

/// The minigame played by bot `name` (`race`, `turrets`) in `command`'s
/// place while a course is presented (`kjk play --aim race|turrets`, for
/// filming them, and the playthrough's goals); `None` for another name or
/// out of a minigame. A race keeps the script's gear (`GEAR`: the run's
/// own in a co-op race, else `real`); the turret turns the view, a race
/// keeps the view it had. `foes`: where the area's foes stand (metres), for
/// a turret with no fighters (`turret`).
pub fn drive(
    name: &str,
    command: Command,
    (rules, course): (Option<&Rules>, Option<&Course>),
    real: Option<i32>,
    now_ms: u64,
    foes: &[[f32; 3]],
) -> Option<Command> {
    let (rules, course) = (rules?, course?);
    let gear = sim::script::global(&course.overlay.numbers, GEAR)
        .copied()
        .or(real)
        .unwrap_or(-1);
    match name {
        "race" => Some(Command {
            angles: command.angles,
            ..race(rules, course, gear, now_ms)
        }),
        "turrets" => Some(turret(rules, course, foes)),
        _ => None,
    }
}

/// A race's command at `now_ms`, the race's gear (`MIN_RACE_GEAR`, below 0
/// in the countdown) as the script keeps it. It reads the course across
/// and along the track: K1's tracks run straight up +y, so the world's x
/// and y; a TSL race's curve, so the frame its offset turns in
/// (`Course::position`): across its x, along its y. In a TSL race its mines
/// are steered round as the buoys are, and jumped when one is close in
/// the way.
pub fn race(rules: &Rules, course: &Course, gear: i32, now_ms: u64) -> Command {
    let mut command = Command::default();
    let (Some(world_at), true) = (course.position(rules, 0), course.fits(rules)) else {
        return command;
    };
    let mover = &course.movers[0];
    // A TSL race's shift counts only once the gear's speed is reached: one
    // pressed early only sets the gear's slower acceleration again
    // (`onaccelerate`), so the driver shifts as the gear tops out, as a
    // player watches the meter fill (the first gear at once).
    let topped = !rules.tsl || gear == 0 || mover.speed >= mover.max_speed * TOPPED_OUT;
    if (0..5).contains(&gear) && topped && pressed(now_ms) {
        command.buttons |= buttons::ATTACK;
    }
    let track = rules.player.path.at(mover.track);
    let turn = rules.player.path.turn(mover.track);
    // A point in the frame the bot steers in.
    let frame = |p: [f32; 3]| {
        if rules.tsl {
            unrotate(turn, [0, 1, 2].map(|i| p[i] - track[i]))
        } else {
            p
        }
    };
    let (at, centre) = if rules.tsl {
        (mover.offset, 0.0)
    } else {
        (world_at, track[0])
    };
    let ahead = |y: f32, reach: f32| y > at[1] + 4.0 && y < at[1] + reach;
    let reach = |metres: f32, seconds: f32| {
        if rules.tsl {
            metres.max(mover.speed * seconds)
        } else {
            metres
        }
    };
    // The nearest pad ahead, else the middle of the track.
    let pad = (1..rules.followers())
        .filter(|&n| rules.enemies[n - 1].trigger && course.movers[n].alive)
        .filter_map(|n| course.position(rules, n))
        .map(frame)
        .filter(|there| ahead(there[1], reach(PAD_AHEAD_M, PAD_AHEAD_S)))
        .min_by(|a, b| a[1].total_cmp(&b[1]));
    let mut heading = pad.map_or(centre, |pad| pad[0]);
    // The nearest buoy in the way (a TSL race's mines too): round it on
    // the side with more room.
    let (low, high) = (
        centre + mover.tunnel_neg[0] + WALL_ROOM_M,
        centre + mover.tunnel_pos[0] - WALL_ROOM_M,
    );
    let mines = (1..rules.followers())
        .filter(|&n| rules.tsl && !rules.enemies[n - 1].trigger && course.movers[n].alive)
        .filter_map(|n| {
            let radius = course.movers[n].radius;
            let world = course.position(rules, n)?;
            Some(((frame(world), [radius; 3]), (world, [radius; 3])))
        });
    // A box in the frame: its corners' bounds there (K1's as it is).
    let framed = |centre: [f32; 3], extent: [f32; 3]| {
        if !rules.tsl {
            return (centre, extent);
        }
        let corners = (0..8).map(|k| {
            frame([0, 1, 2].map(|i| {
                let sign = if k >> i & 1 == 0 { -1.0 } else { 1.0 };
                centre[i] + sign * extent[i]
            }))
        });
        let (low, high) = corners.fold(([f32::MAX; 3], [f32::MIN; 3]), |(low, high), c| {
            (
                [0, 1, 2].map(|i| low[i].min(c[i])),
                [0, 1, 2].map(|i| high[i].max(c[i])),
            )
        });
        (
            [0, 1, 2].map(|i| (low[i] + high[i]) / 2.0),
            [0, 1, 2].map(|i| (high[i] - low[i]) / 2.0),
        )
    };
    // Each thing in the way: its box in the frame, and in the world.
    let blocks: Vec<(Box3, Box3)> = rules
        .obstacles
        .iter()
        .zip(&course.stills)
        .filter(|(_, still)| !still.gone)
        .map(|(obstacle, _)| {
            let (centre, extent) = framed(obstacle.centre, obstacle.extent);
            ((centre, extent), (obstacle.centre, obstacle.extent))
        })
        .chain(mines)
        .collect();
    let in_way = blocks
        .iter()
        .filter(|(([x, y, z], extent), _)| {
            let room = extent[0] + mover.radius + BUOY_ROOM_M;
            // A TSL race meets only what lies at the swoop's height.
            let level = !rules.tsl || (at[2] - z).abs() <= extent[2] + mover.radius;
            // A TSL race's swoop, near the tunnel's walls, also meets what
            // stands where it is now (the pillars along the sides).
            let near = (heading - x).abs() < room || rules.tsl && (at[0] - x).abs() < room;
            ahead(y + extent[1], reach(BUOY_AHEAD_M, BUOY_AHEAD_S)) && near && level
        })
        .min_by(|a, b| a.0.0[1].total_cmp(&b.0.0[1]));
    if let Some(&((centre_of, extent), (world, world_extent))) = in_way {
        let x = centre_of[0];
        let room = extent[0] + mover.radius + BUOY_ROOM_M;
        // Close, and the swoop on the ground: over it (a TSL race's jump),
        // timed by how far its box is in the world (the frame's distances
        // bend with the track's curve).
        let gap = [0, 1, 2]
            .map(|i| ((world[i] - world_at[i]).abs() - world_extent[i]).max(0.0))
            .iter()
            .map(|d| d * d)
            .sum::<f32>()
            .sqrt();
        if rules.tsl && gap < reach(JUMP_AHEAD_M, JUMP_AHEAD_S) && mover.offset[2] <= 2.0 {
            command.up = 127;
        }
        let (left, right) = (x - room, x + room);
        heading = if left >= low && (heading < x || right > high) {
            left
        } else {
            right
        };
    }
    let heading = if low < high {
        heading.clamp(low, high)
    } else {
        centre
    };
    // Toward the heading: the push the sideways drift settles at, and more
    // while it is not there yet.
    let accel = course.lateral_accel;
    if accel >= 1.0 {
        let want = ((heading - at[0]) * STEER_GAIN).clamp(-STEER_MAX, STEER_MAX);
        let push = (4.0 * want + 8.0 * (want - mover.drift[0])) / accel;
        command.right = (push.clamp(-1.0, 1.0) * 127.0) as i8;
    }
    command
}

/// `v` turned back by `q` (x, y, z, w): from the world into the frame `q`
/// turns.
fn unrotate([x, y, z, w]: [f32; 4], v: [f32; 3]) -> [f32; 3] {
    Quat::from_xyzw(-x, -y, -z, w)
        .normalize()
        .mul_vec3(Vec3::from_array(v))
        .to_array()
}

/// The turret's command: the view's angles turned to the nearest fighter,
/// fire held while the aim is on it; with no fighters in the course, the
/// nearest of `foes` (metres) instead.
pub fn turret(rules: &Rules, course: &Course, foes: &[[f32; 3]]) -> Command {
    let mut command = Command::default();
    // Bolts leave the muzzles along the barrels, which are tilted from the
    // view: aim them, not the view, at the fighter.
    let (Some(at), Some(mover), true) = (
        course.muzzle(rules),
        course.movers.first(),
        course.fits(rules),
    ) else {
        return command;
    };
    let (_, barrel) = rules.player.barrels();
    let tilt = [
        trig::atan2_deg(barrel[2], barrel[0].hypot(barrel[1])),
        trig::atan2_deg(-barrel[0], barrel[1]),
    ];
    let speed = rules
        .player
        .banks
        .iter()
        .find(|bank| bank.live())
        .map_or(300.0, |bank| bank.speed);
    let toward = |there: [f32; 3]| [0, 1, 2].map(|i| there[i] - at[i]);
    let length = |v: [f32; 3]| (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
    // Where each live fighter will be when a bolt reaches it.
    let lead = |n: usize| {
        let path = &rules.enemies[n - 1].path;
        let track = course.movers[n].track;
        let now = path.at(track);
        let soon = path.at(track + 0.1);
        let velocity = [0, 1, 2].map(|i| (soon[i] - now[i]) / 0.1);
        let mut there = now;
        for _ in 0..2 {
            let flight = length(toward(there)) / speed;
            there = [0, 1, 2].map(|i| now[i] + velocity[i] * flight);
        }
        there
    };
    let nearest = |a: &[f32; 3], b: &[f32; 3]| length(toward(*a)).total_cmp(&length(toward(*b)));
    let target = (1..rules.followers())
        .filter(|&n| course.movers[n].alive)
        .map(lead)
        .min_by(nearest)
        .or_else(|| {
            (rules.followers() <= 1)
                .then(|| foes.iter().copied().min_by(nearest))
                .flatten()
        });
    let want = match target {
        Some(there) => {
            let d = toward(there);
            let flat = (d[0] * d[0] + d[1] * d[1]).sqrt();
            let pitch = trig::atan2_deg(d[2], flat) - tilt[0];
            let pitch = pitch.clamp(
                mover.tunnel_neg[0].min(mover.tunnel_pos[0]),
                mover.tunnel_pos[0].max(mover.tunnel_neg[0]),
            );
            [pitch, 0.0, trig::atan2_deg(-d[0], d[1]) - tilt[1]]
        }
        None => mover.offset,
    };
    // The view's turn the course reads as that offset (`Course::steer`).
    let start = rules.player.start_offset;
    let delta = |k: usize| {
        let axis = usize::from(rules.mouse[k]).min(2);
        let sign = if rules.flip[k] { -1.0 } else { 1.0 };
        let mut turn = (want[axis] - start[axis]) * sign;
        if axis == 2 {
            // The shortest way round from where the offset is.
            let now = (mover.offset[axis] - start[axis]) * sign;
            turn = now + (turn - now + 540.0).rem_euclid(360.0) - 180.0;
        }
        turn
    };
    let short = |degrees: f32| sim::math::degrees_to_short(degrees);
    command.angles = [
        course.aim_from[0].wrapping_sub(short(delta(1))),
        course.aim_from[1].wrapping_add(short(delta(0))),
        0,
    ];
    if let Some(there) = target {
        let d = toward(there);
        let aim = course.aim(rules);
        let cos = (aim[0] * d[0] + aim[1] * d[1] + aim[2] * d[2]) / length(d).max(f32::EPSILON);
        let range = rules
            .player
            .banks
            .iter()
            .map(|bank| bank.speed * bank.lifespan_ms as f32 / 1000.0)
            .fold(0.0f32, f32::max);
        if cos >= trig::sin_cos_deg(ON_TARGET_DEG).1 && length(d) <= range {
            command.buttons |= buttons::ATTACK;
        }
    }
    command
}
