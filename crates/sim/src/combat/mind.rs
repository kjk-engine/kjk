//! A mind's hands (`docs/COMBAT.md`: a mind turns an
//! intent into commands, as a player's hands would; `combat::ai` and
//! `world::minds` decide the intent): turned to its target, it holds the
//! trigger while its aim is on; with a saber or melee weapon it walks up
//! to it and, within reach, holds attack with a movement key that picks
//! the swing; with its fists it walks closer, within its punch, and holds
//! attack. Minds run on the authority only; prediction repeats each
//! creature's last command.
//!
//! Its aim is off by a few degrees, a different few every so often
//! (`AIM_EVERY_MS`), and its swing's direction changes now and then
//! (`SWING_EVERY_MS`), from a hash of the time and its id rather than a
//! random stream: no state to keep, nothing drawn, and the same on every
//! machine.

use crate::input::{Command, buttons};
use crate::math::{self, Vec3};
use crate::motion::MotionState;
use crate::snapshot::SimId;
use rules::ai::hands;

/// How far a creature sees (units) where its template gives no
/// `PerceptionRange` (`module::Senses`).
pub const SIGHT: f32 = rules::minds::looking::SIGHT;
/// Its aim wanders up to this many degrees in yaw (less in pitch,
/// `ai.hands.pitch_share`), changing every `AIM_EVERY_MS`.
pub const AIM_ERROR: f32 = hands::AIM_ERROR;
pub const AIM_EVERY_MS: u64 = hands::AIM_EVERY_MS;
/// It fires while its view is within this many degrees of its aim.
pub const FIRE_CONE: f32 = hands::FIRE_CONE;
/// A melee creature closes to this distance (units).
pub const REACH: f32 = hands::REACH;
/// A scene's fighter (one a conversation films) comes to a stop this far
/// (units) from its target, centre to centre: `appearance.2da`'s
/// `prefatckdist` for K1's creatures (`ai.hands.scene_reach_m`), not the
/// 2.1 m between personal-space edges.
pub const SCENE_REACH: f32 = hands::SCENE_REACH_M / collision::UNIT;
/// It swings within this distance (units): about a blade's length past
/// its reach.
pub const STRIKE: f32 = hands::STRIKE;
/// Its swing's direction holds this long.
pub const SWING_EVERY_MS: u64 = hands::SWING_EVERY_MS;
/// The movement keys a swing is picked by (forward, right): top to
/// bottom, the four diagonals, the two sides (rulebook 2.5; back alone and
/// the specials' keys are left to players).
const SWINGS: [(i8, i8); 7] = [
    (127, 0),
    (127, 127),
    (0, 127),
    (127, -127),
    (0, -127),
    (-127, 127),
    (-127, -127),
];

/// Where a character's eye is (units).
pub fn eye(player: &MotionState) -> Vec3 {
    let [x, y, z] = player.origin;
    [x, y, z + player.eye_height]
}

/// How far above the eye a player's crosshair line runs: the third-person
/// camera looks along the view at a point this high over the eye (JA's
/// `cg_thirdPersonVertOffset`), so the screen's middle is on that line.
pub const CROSSHAIR_ABOVE_EYE: f32 = hands::CROSSHAIR_ABOVE_EYE;

/// `Command::aim`'s step (units): 1.27 cm, the line ±63 units (1.6 m) off
/// the third-person one at most, past where the eyes are ever drawn.
pub const AIM_STEP: f32 = 0.5;

/// Where a player's crosshair line passes over it (units): the line through
/// the middle of its screen, along its view; its shots go to what it meets.
/// JA's third-person camera looks along the view at a point
/// `CROSSHAIR_ABOVE_EYE` over the eye; a camera elsewhere (the player's own
/// eyes) moves the line by its command's `aim` (`aim_for`).
pub fn crosshair(player: &MotionState) -> Vec3 {
    let [x, y, z] = eye(player);
    let [dx, dy, dz] = player.command_aim.map(|step| f32::from(step) * AIM_STEP);
    [x + dx, y + dy, z + CROSSHAIR_ABOVE_EYE + dz]
}

/// The `Command::aim` of a camera at `camera` (units) looking along
/// `player`'s view: how far its line runs off the third-person one, across
/// the view (a way along it moves the line not), in `AIM_STEP`s.
pub fn aim_for(player: &MotionState, camera: Vec3) -> [i8; 3] {
    let [x, y, z] = eye(player);
    let off = math::sub(camera, [x, y, z + CROSSHAIR_ABOVE_EYE]);
    let (forward, ..) = math::angle_vectors(player.view);
    let across = math::mad(off, -math::dot(off, forward), forward);
    across.map(|units| (units / AIM_STEP).round().clamp(-127.0, 127.0) as i8)
}

/// Its chest: what others aim at (units).
pub fn chest(player: &MotionState) -> Vec3 {
    let [x, y, z] = player.origin;
    [x, y, z + player.maxs_z * 0.5]
}

/// Where a creature looks at `target` (units): a gunner at its chest, where
/// its shots go (they fly along its view); anyone else into its eyes, as
/// JA's saber and melee minds face their enemy (`Jedi_FaceEnemy`, head to
/// head), so fighters of a height stand level, not bent over the other's
/// chest.
pub fn look_at(target: &MotionState, attack: Attack) -> Vec3 {
    match attack {
        Attack::Ranged => chest(target),
        Attack::Swing | Attack::Blow(_) => eye(target),
    }
}

/// The pitch and yaw (degrees; pitch positive looking down) that look from
/// `from` at `to`.
pub fn aim(from: Vec3, to: Vec3) -> (f32, f32) {
    let d = math::sub(to, from);
    let flat = math::flat_length(d);
    (math::atan2_deg(-d[2], flat), math::atan2_deg(d[1], d[0]))
}

/// `a` − `b` in degrees, in (−180, 180] (by whole turns: the sim's
/// floats keep to + − × ÷).
pub fn turn(a: f32, b: f32) -> f32 {
    let mut d = a - b;
    while d > 180.0 {
        d -= 360.0;
    }
    while d <= -180.0 {
        d += 360.0;
    }
    d
}

/// A number in [−1, 1] from `time_ms`'s window and `id`: the aim's wander.
fn wander(time_ms: u64, id: SimId, salt: u64) -> f32 {
    hash(time_ms / AIM_EVERY_MS, id, salt)
}

/// A number in [0, 1) from a window's number, `id` and `salt`: a mind's
/// dice.
pub fn dice(window: u64, id: SimId, salt: u64) -> f32 {
    ((hash(window, id, salt) + 1.0) * 0.5).min(0.999_999)
}

/// The pitch and yaw (degrees) a view at `player`'s turns to in `msec` on
/// its way to `pitch`, `yaw`, at `rate` degrees a second (JA's NPCs turn
/// toward their goal angles; pitch at yaw's rate).
pub fn turned(player: &MotionState, (pitch, yaw): (f32, f32), rate: f32, msec: u32) -> (f32, f32) {
    let most = rate * msec as f32 / 1000.0;
    let toward = |from: f32, to: f32| from + turn(to, from).clamp(-most, most);
    (toward(player.view[0], pitch), toward(player.view[1], yaw))
}

/// A number in [−1, 1] from a window's number, `id` and `salt`
/// (`rng::hashed`).
fn hash(window: u64, id: SimId, salt: u64) -> f32 {
    crate::rng::hashed(window, id.0, salt) * 2.0 - 1.0
}

/// The command that turns `player`'s view to `pitch`, `yaw` (the view is
/// the command's angles plus the deltas the sim keeps).
pub fn looking(player: &MotionState, pitch: f32, yaw: f32) -> Command {
    let short = |degrees: f32, axis: usize| {
        math::degrees_to_short(degrees).wrapping_sub(player.view_offset[axis])
    };
    Command {
        angles: [short(pitch, 0), short(yaw, 1), player.command_angles[2]],
        ..Command::default()
    }
}

/// How a creature attacks its target.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Attack {
    /// Shots: it fires while its aim is on.
    Ranged,
    /// A saber or melee weapon: it walks within `REACH` and swings within
    /// `STRIKE`, its movement keys picking the swing.
    Swing,
    /// Blows that land within this reach (units, centre to centre; its
    /// fists): it walks within three quarters of it and holds attack there.
    Blow(f32),
}

/// What a creature does this tick about `target` (looked at `at`, units:
/// `look_at`), if it has one it can see: turn toward it, aiming from `from`
/// (its eye; a player's crosshair, `crosshair`) a little off, or squarely
/// when `steady` (KOTOR's fighters don't wander), and fire while on
/// (ranged), or walk up to it and swing or strike within reach.
pub fn engage(
    me: SimId,
    player: &MotionState,
    from: Vec3,
    at: Vec3,
    attack: Attack,
    time_ms: u64,
    steady: bool,
) -> Command {
    let (pitch, yaw) = aim(from, at);
    let off = if steady { 0.0 } else { AIM_ERROR };
    let pitch = pitch + wander(time_ms, me, 1) * off * hands::PITCH_SHARE;
    let yaw = yaw + wander(time_ms, me, 2) * off;
    let mut command = looking(player, pitch, yaw);
    if turn(player.view[1], yaw).abs() >= FIRE_CONE {
        return command;
    }
    let flat = math::flat_distance(at, player.origin);
    match attack {
        Attack::Ranged => command.buttons |= buttons::ATTACK,
        Attack::Swing => {
            if flat > REACH {
                command.forward = 127;
            }
            if flat <= STRIKE {
                let pick = (hash(time_ms / SWING_EVERY_MS, me, 3) + 1.0) * 0.5;
                let (forward, right) =
                    SWINGS[((pick * SWINGS.len() as f32) as usize).min(SWINGS.len() - 1)];
                command.buttons |= buttons::ATTACK;
                command.forward = forward;
                command.right = right;
            }
        }
        Attack::Blow(reach) => {
            if flat > reach * hands::BLOW_SHARE {
                command.forward = 127;
            } else {
                command.buttons |= buttons::ATTACK;
            }
        }
    }
    command
}
