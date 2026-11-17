//! The playthrough's minigame players (`race`, `turrets`): commands a
//! player's hands would give, from what the snapshot shows of the course
//! (`sim::minigame`), never the course's state set by hand.
//!
//! - A race: fire (the gear shift, the race's own script: K1's `OnFire`,
//!   TSL's `OnAccelerate`) pressed and let go as each gear nears its top
//!   speed. The swoop is steered for the first pad ahead it can still
//!   reach, by the push that gets it there in time and leaves it able to
//!   reach the pad after. Each push is played out with the course's own
//!   sideways motion. The swoop goes round the buoys in the way of that
//!   path (a TSL race's mines too, jumped when close).
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
/// How far ahead the driver looks, at least, in the seconds the swoop
/// takes to get there: a pad, a buoy, the jump (the swoops top 200 m/s and
/// more, where the metres above are a fraction of a second; the jump's by
/// the world's distance to the thing's box, measured on Telos's track:
/// later or sooner, the swoop comes down on the walls).
const PAD_AHEAD_S: f32 = 2.0;
const BUOY_AHEAD_S: f32 = 1.2;
const JUMP_AHEAD_S: f32 = 0.1;
/// How far inside the edge of a pad's reach the swoop aims (metres).
const PAD_ROOM_M: f32 = 1.0;
/// The driver plans its steering in steps this long (seconds), and tries
/// this many pushes to find the right one.
const PLAN_STEP_S: f32 = 0.03;
const PUSH_GUESSES: usize = 10;
/// How many pushes, evenly spread, the driver weighs to reach two pads.
const PUSH_CHOICES: usize = 40;
/// With no pad to make for, the driver steers for the middle of the track
/// this many seconds ahead.
const AIM_AHEAD_S: f32 = 1.0;
/// The share of its gear's top speed a TSL race's swoop shifts at, and a K1
/// race's.
const TOPPED_OUT: f32 = 0.97;
const K1_TOPPED_OUT: f32 = 0.99;
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
    // When to shift. A TSL race's shift counts only once the gear's speed
    // is reached: one pressed early only sets the gear's slower
    // acceleration again (`onaccelerate`). A K1 race's shift sets the next
    // gear's acceleration, lower than this one's (and without the pads'
    // bonus to it), so it pays only once this gear nears its top speed.
    // Both shift as the gear tops out, as a player watches the meter fill
    // (the first gear at once).
    let topped = gear == 0 || mover.speed >= mover.max_speed * shift_share(rules);
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
    // How fast the swoop goes along its track (metres a second).
    let along = {
        let step = 0.01;
        let ahead = rules.player.path.at(mover.track + step);
        let metres = [0, 1, 2]
            .map(|i| ahead[i] - track[i])
            .iter()
            .map(|d| d * d)
            .sum::<f32>()
            .sqrt();
        metres / step * mover.speed / rules.movement_per_sec.max(1.0)
    };
    // The seconds until the swoop reaches `y` (in the frame) at its speed.
    let until = |y: f32| (y - at[1]) / along.max(1.0);
    // A thing in the way counts until its far end is passed: one the swoop
    // is already up against, knocked back off it, is still steered round
    // (371NAR's track, the swoop driven into the same obstacle again and
    // again, its hull worn to nothing and the race lost).
    let coming = |far_end: f32, reach: f32| far_end > at[1] && far_end < at[1] + reach;
    let reach = |metres: f32, seconds: f32| metres.max(mover.speed * seconds);
    let (low, high) = (
        centre + mover.tunnel_neg[0] + WALL_ROOM_M,
        centre + mover.tunnel_pos[0] - WALL_ROOM_M,
    );
    let walls = |x: f32| {
        if low < high {
            x.clamp(low, high)
        } else {
            centre
        }
    };
    // The swoop across its track (its offset and drift), and the offset of
    // a place in the frame.
    let state = (mover.offset[0], mover.drift[0]);
    let offset_of = |x: f32| x - centre;
    // The pads ahead, nearest first: the stretch across the track where the
    // swoop touches each (within their two radii, the swoop's height above
    // the pad counting too), and how far down the track it lies.
    let mut pads: Vec<(f32, f32, f32)> = (1..rules.followers())
        .filter(|&n| rules.enemies[n - 1].trigger && course.movers[n].alive)
        .filter_map(|n| Some((frame(course.position(rules, n)?), course.movers[n].radius)))
        .filter(|(there, _)| there[1] > at[1] && there[1] < at[1] + reach(PAD_AHEAD_M, PAD_AHEAD_S))
        .map(|(there, radius)| {
            let touch = mover.radius + radius;
            let rise = at[2] - there[2];
            let side = ((touch * touch - rise * rise).max(0.0).sqrt() - PAD_ROOM_M).max(0.0);
            (walls(there[0] - side), walls(there[0] + side), there[1])
        })
        .collect();
    pads.sort_by(|a, b| a.2.total_cmp(&b.2));
    // The aim (an offset, and the seconds until it is to be reached): the
    // first pad the swoop can still reach, at the point of its stretch
    // nearest the pad after it. Else the middle of the track.
    let aim = pads.iter().enumerate().find_map(|(k, &(least, most, y))| {
        let seconds = until(y);
        let (left, right) = (
            held(rules, course, state, -1.0, seconds),
            held(rules, course, state, 1.0, seconds),
        );
        let (least, most) = (offset_of(least).max(left), offset_of(most).min(right));
        if least > most {
            return None;
        }
        let next = pads
            .get(k + 1)
            .map_or(offset_of(centre), |&(a, b, _)| offset_of((a + b) / 2.0));
        Some((k, (next.clamp(least, most), seconds)))
    });
    let (aimed, aim) = match aim {
        Some((k, aim)) => (Some(k), aim),
        None => (None, (offset_of(centre), AIM_AHEAD_S)),
    };
    // Better still, a push that reaches the pad aimed at and leaves the
    // swoop able to reach the pad after it: of those, the one with the
    // most room to spare at both.
    let both = aimed
        .and_then(|k| Some((pads[k], *pads.get(k + 1)?)))
        .and_then(|((least, most, y), (next_least, next_most, next_y))| {
            let (first, second) = (until(y), until(next_y));
            (0..=PUSH_CHOICES)
                .filter_map(|k| {
                    let push = k as f32 / PUSH_CHOICES as f32 * 2.0 - 1.0;
                    let there = slid(rules, course, state, push, first);
                    let room = (there.0 - offset_of(least)).min(offset_of(most) - there.0);
                    let rest = second - first;
                    let (left, right) = (
                        held(rules, course, there, -1.0, rest),
                        held(rules, course, there, 1.0, rest),
                    );
                    let next =
                        (right.min(offset_of(next_most)) - left.max(offset_of(next_least))) / 2.0;
                    (room >= 0.0 && next >= 0.0).then_some((push, room.min(next)))
                })
                .max_by(|a, b| a.1.total_cmp(&b.1))
                .map(|(push, _)| push)
        });
    let push = both.unwrap_or_else(|| timed_push(rules, course, state, aim));
    // A thing in the way of that path (a TSL race's mines too), the
    // nearest: round it on the side the swoop is nearer, unless the wall
    // leaves no room there.
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
    // Each thing in the way: its box in the frame, and in the world, and
    // how near the swoop's middle may pass it. A buoy is met only where the
    // swoop's middle passes into its box. A TSL race's swoop is kept its
    // own radius further off, as it always was.
    let room = if rules.tsl {
        mover.radius + BUOY_ROOM_M
    } else {
        BUOY_ROOM_M
    };
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
    // Where the path leaves the swoop across the track at `y`.
    let across = |y: f32| held(rules, course, state, push, until(y).min(aim.1)) + centre;
    let in_way = blocks
        .iter()
        .filter(|(([x, y, z], extent), _)| {
            let room = extent[0] + room;
            // A TSL race meets only what lies at the swoop's height.
            let level = !rules.tsl || (at[2] - z).abs() <= extent[2] + mover.radius;
            // A TSL race's swoop, near the tunnel's walls, also meets what
            // stands where it is now (the pillars along the sides).
            coming(y + extent[1], reach(BUOY_AHEAD_M, BUOY_AHEAD_S))
                && level
                && ((across(*y) - x).abs() < room || rules.tsl && (at[0] - x).abs() < room)
        })
        .min_by(|a, b| a.0.0[1].total_cmp(&b.0.0[1]));
    let push = match in_way {
        Some(&(([x, y, _], extent), (world, world_extent))) => {
            // Close, and the swoop on the ground: over it (a TSL race's
            // jump), timed by how far its box is in the world (the frame's
            // distances bend with the track's curve).
            let gap = [0, 1, 2]
                .map(|i| ((world[i] - world_at[i]).abs() - world_extent[i]).max(0.0))
                .iter()
                .map(|d| d * d)
                .sum::<f32>()
                .sqrt();
            if rules.tsl && gap < reach(JUMP_AHEAD_M, JUMP_AHEAD_S) && mover.offset[2] <= 2.0 {
                command.up = 127;
            }
            let room = extent[0] + room;
            let (left, right) = (x - room, x + room);
            let near_left = across(y) < x;
            let side = if left >= low && (near_left || right > high) {
                left
            } else {
                right
            };
            let seconds = until(y - extent[1]).max(PLAN_STEP_S);
            timed_push(rules, course, state, (offset_of(walls(side)), seconds))
        }
        None => push,
    };
    command.right = (push.clamp(-1.0, 1.0) * 127.0).round() as i8;
    command
}

/// The swoop's offset and drift after `seconds` with the strafe axis held
/// at `push` (−1 to 1), from its offset and drift now (`state`): played out
/// with the course's own sideways motion (`Course::slide`).
fn slid(rules: &Rules, course: &Course, state: (f32, f32), push: f32, seconds: f32) -> (f32, f32) {
    let mut slid = state;
    let mut left = seconds;
    while left > 0.0 {
        let dt = left.min(PLAN_STEP_S);
        slid = course.slide(rules, slid, push, dt);
        left -= dt;
    }
    slid
}

/// The swoop's offset after `seconds` with the strafe axis held at `push`.
fn held(rules: &Rules, course: &Course, state: (f32, f32), push: f32, seconds: f32) -> f32 {
    slid(rules, course, state, push, seconds).0
}

/// The strafe axis (−1 to 1) that, held, brings the swoop's offset to the
/// aim (an offset) in the aim's seconds: full when even that is not enough.
/// The driver finds it again every step, so it steers onto the aim as the
/// time runs out, with no need to stop the drift there.
fn timed_push(
    rules: &Rules,
    course: &Course,
    state: (f32, f32),
    (target, seconds): (f32, f32),
) -> f32 {
    let seconds = seconds.max(PLAN_STEP_S);
    let at = |push: f32| held(rules, course, state, push, seconds);
    if at(1.0) <= target {
        return 1.0;
    }
    if at(-1.0) >= target {
        return -1.0;
    }
    let (mut low, mut high) = (-1.0f32, 1.0f32);
    for _ in 0..PUSH_GUESSES {
        let middle = (low + high) / 2.0;
        if at(middle) < target {
            low = middle;
        } else {
            high = middle;
        }
    }
    (low + high) / 2.0
}

/// The share of its gear's top speed the race's driver shifts at.
fn shift_share(rules: &Rules) -> f32 {
    if rules.tsl { TOPPED_OUT } else { K1_TOPPED_OUT }
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
        .map_or(300.0, |bank| course.bolt_speed(rules, bank.speed));
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
            .map(|bank| course.bolt_speed(rules, bank.speed) * bank.lifespan_ms as f32 / 1000.0)
            .fold(0.0f32, f32::max);
        if cos >= trig::sin_cos_deg(ON_TARGET_DEG).1 && length(d) <= range {
            command.buttons |= buttons::ATTACK;
        }
    }
    command
}
