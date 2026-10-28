//! Where a rig timeline's origin goes over the ground: the sim's own
//! movement (`sim::motion`, JA multiplayer's `Pmove`) on a flat floor
//! (`collision::BoxWorld`), one 8 ms step at a time, fed what a player's
//! client and JA's game code give it:
//!
//! - wishes become commands: the forward key toward a velocity at its
//!   speed (the step's base speed), or the movement keys as the view
//!   points them (backward runs at 0.75, walking on half the axes); a
//!   saber move's own push goes the same way;
//! - a jump presses up for a step (as JA's jump check does);
//! - a roll sets the legs rolling, held the animation's length
//!   (as JA's roll does); move_body then drives it (the roll's own command,
//!   the timer's speed set by the client-speed adjustment);
//! - a held force jump rises the faster the further it still is from its
//!   level's height (`force_rise`, from the take-off on); once at the
//!   height, or falling, its upward speed is capped at a plain jump's.
//!
//! Space in and out is KOTOR's (+Y forward, +X right); speeds are JA's
//! units per second, positions metres. move_body's space is JA's (+X forward,
//! +Y left, units).

use bevy::math::Vec3;
use collision::BoxWorld;
use sim::motion::{self, Anim, AnimTable, MotionState, Part};
use sim::{Command, buttons, math};

const UNIT: f32 = asset_ja::UNIT;
const STEP_MS: u32 = 8;
pub(crate) const STEP: f32 = STEP_MS as f32;

/// KOTOR's axes to JA's.
fn ja(v: Vec3) -> [f32; 3] {
    [v.y, -v.x, v.z]
}

/// JA's axes to KOTOR's.
fn kotor(v: [f32; 3]) -> Vec3 {
    Vec3::new(-v[1], v[0], v[2])
}

/// What the player asks of the ground from a moment on.
#[derive(Debug, Clone, Copy)]
pub enum Wish {
    /// The forward key toward this velocity (units/s), at its speed.
    Velocity(Vec3),
    /// The movement keys (−1, 0 or 1) as the view points them, at `speed`
    /// (units/s) running.
    Keys {
        forward: i8,
        right: i8,
        walking: bool,
        speed: f32,
    },
}

/// A leap off the ground.
#[derive(Debug, Clone, Copy)]
pub enum Leap {
    /// Up pressed: move_body's jump, 225 units/s.
    Jump,
    /// A force jump held to its level's height (units) and strength
    /// (units/s): level 1 is 96 and 420, level 2 192 and 590.
    Force { height: f32, strength: f32 },
    /// The velocity set outright (units/s), as the saber specials do: the
    /// lunge 150 forward, the jump attack 300 forward and 280 up, the flip
    /// slash 150 forward and 400 up. Airborne when it goes up; it applies
    /// in the air too.
    Launch(Vec3),
    /// Only the upward speed set (units/s), the flat velocity kept, as the
    /// dual and staff jump attacks jump.
    Rise(f32),
}

/// A leap taken whenever the player stands on the ground from `from` until
/// before `to` (ms): landing inside the stretch leaps again.
#[derive(Debug, Clone, Copy)]
pub struct Hop {
    pub from: f32,
    pub to: f32,
    pub leap: Leap,
}

#[derive(Debug, Clone, Default)]
pub struct Plan {
    /// (ms, wish), each holding until the next.
    pub wishes: Vec<(f32, Wish)>,
    /// (ms, leap).
    pub leaps: Vec<(f32, Leap)>,
    pub hops: Vec<Hop>,
    /// (ms, roll): the legs start rolling.
    pub rolls: Vec<(f32, Anim)>,
}

/// The origin's path, its velocity and whether it stands on the ground
/// after each step, and when each leap lands (ms). A jump's last step can
/// end nearer the floor than the collision epsilon without being pulled
/// back (Q-0020), so after one the origin rests 0.12 units (3 mm) under
/// where it started.
#[derive(Debug, Clone, Default)]
pub struct Ground {
    /// Units from the start, JA's axes.
    origins: Vec<[f32; 3]>,
    /// Units per second, JA's axes.
    velocities: Vec<[f32; 3]>,
    grounded: Vec<bool>,
    pub landings: Vec<f32>,
}

/// The upward speed (units/s) of a held force jump `risen` units up, on
/// its way to `height` at `strength`: the jump's own speed plus a tenth of
/// the strength, scaled by how much of the height is still to climb.
fn force_rise(risen: f32, height: f32, strength: f32) -> f32 {
    (height - risen) / height * strength / 10.0 + motion::JUMP_SPEED
}

impl Plan {
    /// Runs move_body for `length_ms`, the view's yaw (degrees, Quake's: left
    /// of the start) at each ms from `yaw`, the animation lengths from
    /// `anims`.
    pub fn run(&self, length_ms: f32, anims: &AnimTable, yaw: &dyn Fn(f32) -> f32) -> Ground {
        let floor = BoxWorld::floor(1000.0, 0.0, 1.0);
        // Standing where a landing leaves the box (`SimWorld::new`).
        let start = [0.0, 0.0, 0.125 - motion::SOLE_Z];
        let mut ps = MotionState::standing(start, 0.0);
        ps.ground = Some(motion::Ground::World);
        let steps = (length_ms / STEP).ceil() as usize + 1;
        let mut ground = Ground::default();
        let record = |ps: &MotionState, ground: &mut Ground| {
            ground
                .origins
                .push(std::array::from_fn(|i| ps.origin[i] - start[i]));
            ground.velocities.push(ps.velocity);
            ground.grounded.push(ps.ground.is_some());
        };
        record(&ps, &mut ground);
        let mut leaps = self.leaps.clone();
        leaps.sort_by(|a, b| a.0.total_cmp(&b.0));
        let mut rolls = self.rolls.clone();
        rolls.sort_by(|a, b| a.0.total_cmp(&b.0));
        let (mut next_leap, mut next_roll) = (0, 0);
        // A force jump under way: (height, strength, where it left from).
        let mut force: Option<(f32, f32, f32)> = None;
        for step in 0..steps {
            let now = step as f32 * STEP;
            let on_ground = ps.ground.is_some();
            let mut command = self.command(now, yaw(now), &mut ps);
            while next_roll < rolls.len() && rolls[next_roll].0 <= now {
                let anim = rolls[next_roll].1;
                let timer = anims.get(anim).length_ms();
                for part in [&mut ps.legs, &mut ps.torso] {
                    *part = Part {
                        anim,
                        timer,
                        flip: !part.flip,
                    };
                }
                next_roll += 1;
            }
            let mut leap = None;
            if next_leap < leaps.len()
                && leaps[next_leap].0 <= now
                && (on_ground || matches!(leaps[next_leap].1, Leap::Launch(_)))
            {
                leap = Some(leaps[next_leap].1);
                next_leap += 1;
            }
            if on_ground && leap.is_none() {
                leap = self
                    .hops
                    .iter()
                    .find(|hop| hop.from <= now && now < hop.to)
                    .map(|hop| hop.leap);
            }
            if let Some(leap) = leap {
                force = None;
                match leap {
                    Leap::Jump => command.up = 127,
                    Leap::Force { height, strength } => {
                        ps.velocity[2] = force_rise(0.0, height, strength);
                        force = Some((height, strength, ps.origin[2]));
                    }
                    Leap::Launch(v) => ps.velocity = ja(v),
                    Leap::Rise(z) => ps.velocity[2] = z,
                }
            } else if let Some((height, strength, from)) = force
                && !on_ground
            {
                let risen = ps.origin[2] - from;
                if risen < height && ps.velocity[2] > 0.0 {
                    ps.velocity[2] = force_rise(risen, height, strength);
                } else if ps.velocity[2] > motion::JUMP_SPEED {
                    ps.velocity[2] = motion::JUMP_SPEED;
                }
            }
            motion::move_body(
                &mut ps,
                &command,
                STEP_MS,
                u64::from(STEP_MS) * (step as u64 + 1),
                &floor,
                anims,
                None,
            );
            if !on_ground && ps.ground.is_some() {
                force = None;
                ground.landings.push(now + STEP);
            }
            record(&ps, &mut ground);
        }
        ground
    }

    /// The command at `now` ms from the latest wish made by then (the last
    /// one given, for the same moment), the view at `yaw`; sets the step's
    /// base speed on `ps`.
    fn command(&self, now: f32, yaw: f32, ps: &mut MotionState) -> Command {
        let wish = self
            .wishes
            .iter()
            .filter(|(at, _)| *at <= now)
            .max_by(|a, b| a.0.total_cmp(&b.0))
            .map(|w| w.1);
        let mut command = Command::default();
        let mut view = yaw;
        ps.run_speed = motion::BASE_SPEED;
        match wish {
            Some(Wish::Velocity(v)) if v.length() > 0.0 => {
                let [x, y, _] = ja(v);
                view = math::atan2_deg(y, x);
                command.forward = 127;
                ps.run_speed = v.length();
            }
            Some(Wish::Keys {
                forward,
                right,
                walking,
                speed,
            }) if forward != 0 || right != 0 => {
                // JA's client sends a walk as axes of 64.
                let axis = if walking { 64 } else { 127 };
                command.forward = forward.signum() * axis;
                command.right = right.signum() * axis;
                if walking {
                    command.buttons |= buttons::WALKING;
                }
                ps.run_speed = speed * 127.0 / f32::from(axis);
            }
            _ => {}
        }
        command.angles = [0, math::degrees_to_short(view), 0];
        command
    }
}

impl Ground {
    /// The origin at `t` seconds (held after the end), metres from the
    /// start in KOTOR's space.
    pub fn offset(&self, t: f32) -> Vec3 {
        let Some(last) = self.origins.last() else {
            return Vec3::ZERO;
        };
        let at = (t * 1000.0 / STEP).max(0.0);
        let i = at.floor() as usize;
        if i + 1 >= self.origins.len() {
            return kotor(*last) * UNIT;
        }
        let f = at - i as f32;
        kotor(self.origins[i]).lerp(kotor(self.origins[i + 1]), f) * UNIT
    }

    /// The velocity at `t` seconds, JA's (units/s, +X forward, +Y left):
    /// the step under way then.
    pub fn velocity(&self, t: f32) -> [f32; 3] {
        let i = (t * 1000.0 / STEP).max(0.0).floor() as usize + 1;
        let last = self.velocities.last().copied().unwrap_or([0.0; 3]);
        self.velocities.get(i).copied().unwrap_or(last)
    }

    /// Whether the origin stands on the ground at `t` seconds.
    pub fn on_ground(&self, t: f32) -> bool {
        let i = (t * 1000.0 / STEP).max(0.0).floor() as usize + 1;
        let last = self.grounded.last().copied().unwrap_or(true);
        self.grounded.get(i).copied().unwrap_or(last)
    }

    /// The origin every `every` seconds up to `length`, for tracking shots.
    pub fn samples(&self, length: f32, every: f32) -> Vec<(f32, Vec3)> {
        let count = (length / every).floor() as usize;
        (0..=count)
            .map(|i| {
                let t = i as f32 * every;
                (t, self.offset(t))
            })
            .collect()
    }
}
