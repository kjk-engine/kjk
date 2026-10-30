//! The movement step: one tick of a player's or creature's box through the
//! level, in JA's units, from its command.
//!
//! A step reads the command's keys and angles, lets the saber's command
//! steps have their say, settles the box's height (crouched, rolling or
//! standing), finds the ground, moves the box on the ground or through the
//! air (friction, acceleration toward the keys, then a slide along whatever
//! it meets, climbing low steps), finds the ground again, runs the weapon
//! step and picks the legs' gait. Everything is deterministic `f32` work on
//! [`MotionState`], which the snapshot carries whole.
//!
//! The slide lives in `slide.rs`, the Force jump in `force_jump.rs`; the
//! animations (`anim.rs`), the weapon step (`weapon.rs`) and the saber's
//! moves (`saber.rs`) add their own methods to the step's working state,
//! [`Move`].

pub(crate) mod anim;
#[cfg(feature = "probes")]
pub mod body_probes;
mod force_jump;
mod saber;
mod slide;
mod weapon;

use collision::{CollisionWorld, Contents, Extents, Shape};

use rules::movement::rolls;

use crate::input::{Command, buttons};
use crate::math::{self, Vec3, dot, length, mad, normalize, scale};
pub use anim::{Anim, AnimInfo, AnimTable, Part};
use anim::{Animator, Parts, play_flags};
pub(crate) use anim::{is_knockdown, is_leap, is_special_move, is_spin, roll_playing};
pub(crate) use saber::play_speed;
pub use weapon::{Arms, Other, shrugs_off_pain};

pub use collision::UNIT;
pub use collision::player::{
    BASE_SPEED, GRAVITY, HALF_WIDTH, SOLE_Z, STAND_HEIGHT, STEP_HEIGHT, WALKABLE_NORMAL_Z,
};

/// How far (units) down a steep slope a body must be able to slide for it
/// to count as a slope, not a crevice (`Move::crevice`).
const CREVICE_UNITS: f32 = rules::movement::fine::CREVICE_UNITS;
/// How near straight up a steep slope (the cosine) a body must head for the
/// crevice rule to stand it there (`Move::crevice`; a walker stepping
/// aside on its way up still heads up).
const CREVICE_HEADING: f32 = rules::movement::fine::CREVICE_HEADING;

/// How far (units) a body wedged into the level is looked out of, and by
/// what step (`free_spot`).
const UNBURY_REACH: f32 = rules::movement::fine::UNBURY_REACH;
const UNBURY_STEP: f32 = rules::movement::fine::UNBURY_STEP;

/// A jump's upward speed as it leaves the ground (units/s).
pub const JUMP_SPEED: f32 = rules::movement::air::JUMP_SPEED;
/// The box's top above the origin while crouched or rolling.
pub const CROUCH_HEIGHT: f32 = rules::movement::body::CROUCH_HEIGHT;
/// The eyes above the origin, standing or rolling, and crouched.
pub const EYE_HEIGHT: f32 = rules::movement::body::EYE_HEIGHT;
pub const CROUCH_EYE_HEIGHT: f32 = rules::movement::body::CROUCH_EYE_HEIGHT;
/// The share of its speed ground friction takes from a body each second.
pub const FRICTION: f32 = rules::movement::ground::FRICTION;
/// Below this speed friction acts as if the body moved this fast.
const STOP_SPEED: f32 = rules::movement::ground::STOP_SPEED;

/// The most entities one step records as touched.
const TOUCH_LIMIT: usize = 32;

/// How far (units) a body moving at `speed` (units/s) on the ground slides
/// once nothing drives it: ground friction takes `FRICTION` of its speed a
/// second, of `STOP_SPEED` below that.
pub fn stopping_distance(speed: f32) -> f32 {
    let slow = STOP_SPEED / (2.0 * FRICTION);
    if speed <= STOP_SPEED {
        speed * speed / (2.0 * STOP_SPEED * FRICTION)
    } else {
        (speed - STOP_SPEED) / FRICTION + slow
    }
}
/// The movement state's flag bits.
pub mod motion_flags {
    pub const DUCKED: u16 = 1;
    pub const JUMP_PRESSED: u16 = 2;
    pub const ROLLING: u16 = 4;
    pub const JUMPED_BACK: u16 = 8;
    pub const RUNNING_BACK: u16 = 16;
    /// `hold_ms` holds a landing: the velocity is kept through it.
    pub const LAND_HOLD: u16 = 32;
    /// Dead: no input, the view held, a corpse's friction on the ground,
    /// the death animation left alone.
    pub const DEAD: u16 = 64;
    /// `hold_ms` holds a hit's knockback: no ground friction, air control
    /// on the ground.
    pub const KNOCKBACK_HOLD: u16 = 128;
    /// Ours, not JA's: use held since it acted on something (used, talked
    /// to, refused by a lock: `world::interact`); it is not tried again
    /// until use is released, as KOTOR's click uses once.
    pub const USE_SPENT: u16 = 256;
    /// Ours: KOTOR's walker moved the body this step (`carry`), not the
    /// move's own physics; the next step that does not walk starts it still.
    pub const CARRIED: u16 = 512;
    /// Ours: the body moves at KOTOR's pace this step (`walk::paced`): it
    /// walks or runs as its move says whatever its keys, and gathers speed
    /// as at JA's stop speed at least, so a walk slower than that gets
    /// going (JA's ground friction holds back any slower wish, KOTOR's
    /// movers have none).
    pub const PACED: u16 = 1024;
}

/// What a step did, for sounds and effects.
pub mod events {
    pub const JUMP: u8 = 1;
    /// Landed hard enough to hurt.
    pub const FALL: u8 = 2;
    /// Landed softly (a footstep's sound).
    pub const LAND: u8 = 4;
    pub const ROLL: u8 = 8;
    /// Walked up a step.
    pub const STEP: u8 = 16;
    /// A saber swing started.
    pub const SABER_ATTACK: u8 = 32;
    /// The saber lit.
    pub const SABER_ON: u8 = 64;
    /// A special wanted more Force than the pool holds.
    pub const NO_POWER: u8 = 128;
}

/// What the player stands on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Ground {
    /// The level.
    World,
    /// An entity (a door, a placeable, a body).
    Entity(u32),
}

/// A player's movement state, in JA units: the snapshot carries it whole.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MotionState {
    /// The box's origin, 24 units above its soles.
    pub origin: Vec3,
    /// Units per second.
    pub velocity: Vec3,
    /// Where the player looks (pitch, yaw, roll; degrees).
    pub view: Vec3,
    /// Added to the command's angles: the view the sim sets survives the
    /// next command.
    pub view_offset: [i16; 3],
    pub ground: Option<Ground>,
    /// `motion_flags`.
    pub flags: u16,
    /// Milliseconds a landing or a knockback still holds.
    pub hold_ms: i32,
    pub legs: Part,
    pub torso: Part,
    /// Which way the keys push: 0 forward, counting 45° steps to the left.
    pub push_dir: u8,
    /// Went far enough off the ground to land with an animation.
    pub in_air_anim: bool,
    /// The footstep cycle (0 to 255).
    pub footstep_cycle: u8,
    /// The box's top above the origin (40 standing, 16 crouched).
    pub maxs_z: f32,
    pub eye_height: f32,
    /// The height the last jump left the ground at; 0.0 for none.
    pub takeoff_z: f32,
    /// When the slope stance may step next (sim milliseconds).
    pub slope_next_ms: u64,
    /// The run speed this body moves at, and the gravity it falls by.
    pub run_speed: f32,
    pub gravity: f32,
    /// What the last step did (`events`).
    pub events: u8,
    /// The last command's angles: a step without a command keeps them.
    pub command_angles: [i16; 3],
    /// The last command's crosshair line (`Command::aim`), kept likewise.
    pub command_aim: [i8; 3],
    /// Sim time before which holding use does nothing (JA tries a use at
    /// intervals while it is held).
    pub next_use_ms: u64,
}

impl MotionState {
    /// A body at rest at `origin`, facing `yaw` degrees, standing upright.
    pub fn standing(origin: Vec3, yaw: f32) -> MotionState {
        MotionState {
            origin,
            velocity: [0.0; 3],
            view: [0.0, yaw, 0.0],
            view_offset: [0; 3],
            ground: None,
            flags: 0,
            hold_ms: 0,
            legs: Part::default(),
            torso: Part::default(),
            push_dir: 0,
            in_air_anim: false,
            footstep_cycle: 0,
            maxs_z: STAND_HEIGHT,
            eye_height: EYE_HEIGHT,
            takeoff_z: 0.0,
            slope_next_ms: 0,
            run_speed: BASE_SPEED,
            gravity: GRAVITY,
            events: 0,
            command_angles: [0, math::degrees_to_short(yaw), 0],
            command_aim: [0; 3],
            next_use_ms: 0,
        }
    }

    /// Turns the view to `view` (degrees), whatever the commands' angles:
    /// the deltas take up the difference, so the next command keeps it.
    pub fn set_view(&mut self, view: Vec3) {
        for (axis, delta) in self.view_offset.iter_mut().enumerate() {
            *delta = math::degrees_to_short(view[axis]).wrapping_sub(self.command_angles[axis]);
        }
        self.view = view;
    }

    /// Turns to face `yaw` degrees (as a short angle reads it), keeping the
    /// pitch and roll.
    pub fn face(&mut self, yaw: f32) {
        let yaw = math::short_to_degrees(math::degrees_to_short(yaw));
        self.set_view([self.view[0], yaw, self.view[2]]);
    }

    pub fn ducked(&self) -> bool {
        self.flags & motion_flags::DUCKED != 0
    }
}

/// The view (degrees) a command's `angles` give with the state's `deltas`
/// added, the pitch held within the rulebook's tilt.
pub fn view_of(angles: [i16; 3], view_offset: [i16; 3]) -> [f32; 3] {
    let limit = rules::movement::body::PITCH_MAX;
    std::array::from_fn(|axis| {
        let mut short = angles[axis].wrapping_add(view_offset[axis]);
        if axis == 0 {
            short = short.clamp(-limit, limit);
        }
        math::short_to_degrees(short)
    })
}

/// Plays `anim` as a reaction from outside the move (a death, a pain, a
/// shot): on the torso only, or on both parts, over what plays, held for
/// its length.
pub fn react(ps: &mut MotionState, anims: &AnimTable, anim: Anim, torso_only: bool) {
    let animator = Animator {
        table: anims,
        sent_legs: ps.legs.anim,
        sent_torso: ps.torso.anim,
    };
    let parts = if torso_only {
        Parts::UPPER
    } else {
        Parts::WHOLE
    };
    animator.play(
        ps,
        parts,
        anim,
        play_flags::OVER_ANY | play_flags::HOLD_FULL,
    );
}

/// Plays a shot's `anim` on the torso: over whatever plays, started over
/// when it already plays or when the step began in it (`sent`), so each
/// shot of a held trigger kicks; its timer is left as it is. Not for the
/// dead.
pub fn begin_torso(ps: &mut MotionState, anims: &AnimTable, anim: Anim, sent: Anim) {
    if ps.flags & motion_flags::DEAD == 0 && !anims.get(anim).missing() {
        ps.begin_torso(anim, sent);
    }
}

/// Holds `anim` through the next step as JA holds a hand extend (renewed
/// every frame with a one-millisecond timer): on the torso, or on the
/// whole body, over what plays. Its timer outlasts the step's timer drop
/// by a millisecond, so the move's own choice waits; the step after, it
/// plays again or lets go.
pub fn hold(ps: &mut MotionState, anims: &AnimTable, anim: Anim, whole_body: bool, msec: u32) {
    if anims.get(anim).missing() {
        return;
    }
    let animator = Animator {
        table: anims,
        sent_legs: ps.legs.anim,
        sent_torso: ps.torso.anim,
    };
    let parts = if whole_body {
        Parts::WHOLE
    } else {
        Parts::UPPER
    };
    animator.play(
        ps,
        parts,
        anim,
        play_flags::OVER_ANY | play_flags::HOLD_FULL,
    );
    let timer = i32::try_from(msec).unwrap_or(i32::MAX - 1) + 1;
    ps.torso.timer = timer;
    if whole_body {
        ps.legs.timer = timer;
    }
}

/// The entities a step touched, in the order met.
pub type Touches = Vec<u32>;

/// A trace in units.
struct Hit {
    fraction: f32,
    end: Vec3,
    normal: Vec3,
    all_solid: bool,
    ground: Ground,
    contents: Contents,
}

/// The working state of one step: the player's state it changes, the
/// command as the step sees it, the box, and what the step has found so
/// far.
struct Move<'a, 'b> {
    ps: &'a mut MotionState,
    /// The step's command: the step changes it (a roll's keys, a spent
    /// jump, a dropped walk button).
    cmd: Command,
    /// The command as it came.
    raw: Command,
    arms: Option<&'a mut Arms<'b>>,
    animator: Animator<'a>,
    collision: &'a dyn CollisionWorld,
    time_ms: u64,
    /// The step's length, clamped, in milliseconds and in seconds.
    ms: i32,
    seconds: f32,
    mins: Vec3,
    maxs: Vec3,
    /// On walkable ground this step.
    on_walkable: bool,
    /// The plane of the ground below, walkable or steep.
    ground_plane: Option<Vec3>,
    /// The speed the keys ask for at full tilt (units/s).
    speed: f32,
    /// The view's forward and right.
    view_forward: Vec3,
    view_right: Vec3,
    touches: Touches,
    /// The state as the step found it, before anything moved it.
    origin_before: Vec3,
    velocity_before: Vec3,
}

/// One movement step of `msec` milliseconds ending at `time_ms`, against
/// `collision`, with the skeleton's animation lengths, and the weapon step
/// with `arms`; the entities it touched.
pub fn move_body(
    ps: &mut MotionState,
    command: &Command,
    msec: u32,
    time_ms: u64,
    collision: &dyn CollisionWorld,
    anims: &AnimTable,
    arms: Option<&mut Arms>,
) -> Touches {
    let mut step = Move::new(ps, command, msec, time_ms, (collision, anims), arms);
    step.run();
    step.touches
}

/// A step of a body KOTOR's walker moves (`walk::kotor_step`'s `stepped`):
/// it stands where the walker put it, on the floor, facing the step's way,
/// with the step's velocity and legs, the rest of `move_body`'s step run as
/// move_body runs it (the command's angles and saber moves, the timers, the
/// weapon and the torso's animations). Nothing is traced for the move, and
/// nothing is touched.
#[allow(clippy::too_many_arguments)]
pub fn carry(
    ps: &mut MotionState,
    command: &Command,
    msec: u32,
    time_ms: u64,
    collision: &dyn CollisionWorld,
    anims: &AnimTable,
    arms: Option<&mut Arms>,
    stepped: &crate::walk::Stepped,
) {
    Move::new(ps, command, msec, time_ms, (collision, anims), arms).carried(stepped);
}

/// The four rolls, whatever their timers.
fn is_roll_anim(anim: Anim) -> bool {
    matches!(
        anim,
        Anim::BOTH_ROLL_F | Anim::BOTH_ROLL_B | Anim::BOTH_ROLL_L | Anim::BOTH_ROLL_R
    )
}

/// Which way keys `forward` and `right` push, in 45° steps to the left of
/// forward; `last` when no key is held (a pure sideways push left over
/// turns a step toward forward).
fn push_direction(forward: i8, right: i8, last: u8) -> u8 {
    use std::cmp::Ordering::{Equal, Greater, Less};
    match (forward.cmp(&0), right.cmp(&0)) {
        (Greater, Equal) => 0,
        (Greater, Less) => 1,
        (Equal, Less) => 2,
        (Less, Less) => 3,
        (Less, Equal) => 4,
        (Less, Greater) => 5,
        (Equal, Greater) => 6,
        (Greater, Greater) => 7,
        (Equal, Equal) => match last {
            2 => 1,
            6 => 7,
            other => other,
        },
    }
}

impl<'a, 'b> Move<'a, 'b> {
    fn new(
        ps: &'a mut MotionState,
        command: &Command,
        msec: u32,
        time_ms: u64,
        (collision, anims): (&'a dyn CollisionWorld, &'a AnimTable),
        arms: Option<&'a mut Arms<'b>>,
    ) -> Self {
        let msec = msec.clamp(1, 200) as i32;
        let animator = Animator {
            table: anims,
            sent_legs: ps.legs.anim,
            sent_torso: ps.torso.anim,
        };
        let maxs = [HALF_WIDTH, HALF_WIDTH, ps.maxs_z];
        Move {
            cmd: *command,
            raw: *command,
            arms,
            animator,
            collision,
            time_ms,
            ms: msec,
            seconds: (f64::from(msec) * 0.001) as f32,
            mins: [-HALF_WIDTH, -HALF_WIDTH, SOLE_Z],
            maxs,
            on_walkable: false,
            ground_plane: None,
            speed: 0.0,
            view_forward: [0.0; 3],
            view_right: [0.0; 3],
            touches: Vec::new(),
            origin_before: ps.origin,
            velocity_before: ps.velocity,
            ps,
        }
    }
}

impl Move<'_, '_> {
    fn dead(&self) -> bool {
        self.ps.flags & motion_flags::DEAD != 0
    }

    /// The whole step, in its order.
    fn run(&mut self) {
        let dead = self.dead();
        self.ps.events = 0;
        if dead {
            // The dead keep only where they look.
            self.cmd = Command {
                angles: self.cmd.angles,
                aim: self.cmd.aim,
                ..Command::default()
            };
        }
        self.ps.command_angles = self.cmd.angles;
        self.ps.command_aim = self.cmd.aim;
        self.cmd.forward = self.cmd.forward.max(-127);
        self.cmd.right = self.cmd.right.max(-127);
        self.cmd.up = self.cmd.up.max(-127);

        let mut hold_view = !dead && self.weapon_keys();
        self.roll_keys();
        if !dead {
            hold_view |= self.special_move_keys();
        }
        self.set_step_speed();
        if hold_view {
            self.ps.set_view(self.ps.view);
        }
        let walk_axis = rules::movement::keys::WALK_AXIS;
        if self.ps.flags & motion_flags::PACED == 0
            && (self.cmd.forward.unsigned_abs() > walk_axis
                || self.cmd.right.unsigned_abs() > walk_axis)
        {
            self.cmd.buttons &= !buttons::WALKING;
        }

        self.origin_before = self.ps.origin;
        self.velocity_before = self.ps.velocity;
        if !dead {
            self.turn_view();
        }
        let (forward, right, _) = math::angle_vectors(self.ps.view);
        (self.view_forward, self.view_right) = (forward, right);

        if self.cmd.up < rules::movement::keys::JUMP_AXIS {
            self.ps.flags &= !motion_flags::JUMP_PRESSED;
        }
        if self.cmd.forward < 0 {
            self.ps.flags |= motion_flags::RUNNING_BACK;
        } else if self.cmd.forward > 0 || self.cmd.right != 0 {
            self.ps.flags &= !motion_flags::RUNNING_BACK;
        }

        self.settle_height();
        self.find_ground();
        if self.ps.ground.is_some() {
            self.ps.takeoff_z = 0.0;
        }
        self.run_down_timers();
        if dead {
            self.corpse_slide();
        }
        if self.on_walkable {
            self.ground_move();
        } else {
            self.air_move();
        }
        self.find_ground();

        if !dead {
            self.weapon_step();
            self.footsteps();
        }
        self.ps.velocity = self.ps.velocity.map(f32::round_ties_even);
        self.ps.maxs_z = self.maxs[2];
    }

    /// A roll steers itself: its own way at full tilt, nothing else held.
    fn roll_keys(&mut self) {
        let legs = self.ps.legs.anim;
        if !roll_playing(self.ps, legs) {
            return;
        }
        let (forward, right) = match legs {
            Anim::BOTH_ROLL_F => (127, 0),
            Anim::BOTH_ROLL_B => (-127, 0),
            Anim::BOTH_ROLL_R => (0, 127),
            Anim::BOTH_ROLL_L => (0, -127),
            _ => (self.cmd.forward, self.cmd.right),
        };
        (self.cmd.forward, self.cmd.right, self.cmd.up) = (forward, right, 0);
    }

    /// The step's speed: the body's run, slowed backward on the ground,
    /// as the saber allows, and a roll's own while one plays.
    fn set_step_speed(&mut self) {
        let mut speed = if self.floored() {
            0.0
        } else {
            self.ps.run_speed
        };
        if self.cmd.forward < 0
            && self.cmd.buttons & buttons::WALKING == 0
            && self.ps.ground.is_some()
        {
            speed *= rules::movement::ground::BACKWARD;
        }
        speed = self.saber_speed(speed);

        let legs = self.ps.legs;
        if roll_playing(self.ps, legs.anim) && speed > rolls::LEAST_SPEED {
            let left = f64::from(legs.timer);
            let divisors = if legs.anim == Anim::BOTH_ROLL_B {
                rolls::BACK_DIVISORS
            } else {
                rolls::FORWARD_DIVISORS
            };
            let divisor = if legs.timer > rolls::LATE_MS {
                divisors[0]
            } else {
                divisors[1]
            };
            speed = ((left / divisor) as f32).min(rolls::MOST_SPEED);
        }
        self.speed = speed;
    }

    /// The view from the command, the pitch delta pulled back so the view
    /// never tilts past the limit.
    fn turn_view(&mut self) {
        let limit = rules::movement::body::PITCH_MAX;
        let pitch = self.cmd.angles[0].wrapping_add(self.ps.view_offset[0]);
        if pitch > limit {
            self.ps.view_offset[0] = limit.wrapping_sub(self.cmd.angles[0]);
        } else if pitch < -limit {
            self.ps.view_offset[0] = (-limit).wrapping_sub(self.cmd.angles[0]);
        }
        self.ps.view = view_of(self.cmd.angles, self.ps.view_offset);
    }

    /// The landing or knockback hold runs down, and the parts' timers.
    fn run_down_timers(&mut self) {
        if self.ps.hold_ms != 0 {
            if self.ms >= self.ps.hold_ms {
                self.ps.flags &= !(motion_flags::LAND_HOLD | motion_flags::KNOCKBACK_HOLD);
                self.ps.hold_ms = 0;
            } else {
                self.ps.hold_ms -= self.ms;
            }
        }
        for part in [&mut self.ps.legs, &mut self.ps.torso] {
            if part.timer > 0 {
                part.timer = (part.timer - self.ms).max(0);
            }
        }
    }

    /// The footstep cycle moves on at `pace` a millisecond.
    fn advance_footsteps(&mut self, pace: f32) {
        let cycle = (f32::from(self.ps.footstep_cycle) + pace * self.ms as f32) as i32;
        self.ps.footstep_cycle = (cycle & 255) as u8;
    }

    /// Records an entity the step met.
    fn touch(&mut self, ground: Ground) {
        if let Ground::Entity(entity) = ground
            && self.touches.len() < TOUCH_LIMIT
            && !self.touches.contains(&entity)
        {
            self.touches.push(entity);
        }
    }

    /// `carry`'s step: `run`'s without the move, the ground's trace and
    /// the footsteps, the legs playing the walker's animation.
    fn carried(&mut self, stepped: &crate::walk::Stepped) {
        self.ps.events = 0;
        self.ps.command_angles = self.cmd.angles;
        self.ps.command_aim = self.cmd.aim;
        self.cmd.forward = self.cmd.forward.max(-127);
        self.cmd.right = self.cmd.right.max(-127);
        self.cmd.up = self.cmd.up.max(-127);
        let hold_view = self.weapon_keys() | self.special_move_keys();
        self.set_step_speed();
        if hold_view {
            self.ps.set_view(self.ps.view);
        }
        self.turn_view();
        self.ps.face(stepped.yaw);
        let (forward, right, _) = math::angle_vectors(self.ps.view);
        (self.view_forward, self.view_right) = (forward, right);
        if self.cmd.up < rules::movement::keys::JUMP_AXIS {
            self.ps.flags &= !motion_flags::JUMP_PRESSED;
        }
        self.ps.flags &= !motion_flags::RUNNING_BACK;
        self.origin_before = self.ps.origin;
        self.velocity_before = self.ps.velocity;
        self.ps.origin = stepped.origin;
        self.ps.velocity = stepped.velocity;
        self.ps.ground = Some(Ground::World);
        self.ps.takeoff_z = 0.0;
        self.on_walkable = true;
        self.run_down_timers();
        self.weapon_step();
        self.animator
            .play(self.ps, Parts::LOWER, stepped.legs, play_flags::WHEN_FREE);
        match stepped.legs {
            Anim::BOTH_WALK1 => self.advance_footsteps(rules::movement::footsteps::CYCLE[0]),
            Anim::BOTH_RUN1 => self.advance_footsteps(rules::movement::footsteps::CYCLE[1]),
            _ => self.ps.footstep_cycle = 0,
        }
        // Whole units, as JA sends them; the weapon's hops carry it not.
        self.ps.velocity = stepped.velocity.map(f32::round_ties_even);
        self.ps.flags |= motion_flags::CARRIED;
        self.ps.maxs_z = self.maxs[2];
    }

    /// A box trace in units, converted at the collision boundary. The
    /// body's own box goes round across (`Shape::Upright`): KOTOR tests a
    /// creature's move by its personal space across its way, whichever way
    /// it goes, where JA's square box reaches √2 of its half-size into a
    /// diagonal wall (Khoonda's office joins its hall by a diagonal channel
    /// a box could not pass).
    fn trace_masked(&self, start: Vec3, end: Vec3, mins: Vec3, maxs: Vec3, mask: Contents) -> Hit {
        let metres = |v: Vec3| scale(v, UNIT);
        let extents = Extents {
            mins: metres(mins),
            maxs: metres(maxs),
        };
        let shape = if mins == self.mins && maxs == self.maxs {
            Shape::Upright(extents)
        } else {
            Shape::Box(extents)
        };
        let trace = self
            .collision
            .sweep(metres(start), metres(end), &shape, mask);
        let end = if trace.all_solid || trace.fraction <= 0.0 {
            start
        } else if trace.fraction >= 1.0 {
            end
        } else {
            std::array::from_fn(|i| start[i] + trace.fraction * (end[i] - start[i]))
        };
        Hit {
            fraction: trace.fraction,
            end,
            normal: trace.plane.normal,
            all_solid: trace.all_solid,
            ground: trace.entity.map_or(Ground::World, Ground::Entity),
            contents: trace.contents,
        }
    }

    fn trace(&self, start: Vec3, end: Vec3, mins: Vec3, maxs: Vec3) -> Hit {
        self.trace_masked(start, end, mins, maxs, Contents::PLAYER_SOLID)
    }

    // --- The box's height -------------------------------------------------

    /// Whether the box could stand up here: nine thin slabs over its top,
    /// each swept up to standing height, all get through.
    fn room_to_stand(&self) -> bool {
        let inset = HALF_WIDTH - 5.0;
        let cells = [-inset, 0.0, inset];
        let o = self.ps.origin;
        let (slab_mins, slab_maxs) = ([-5.0, -5.0, -2.5], [5.0, 5.0, 0.0]);
        cells.iter().all(|&x| {
            cells.iter().all(|&y| {
                let from = [x + o[0], y + o[1], self.maxs[2] + o[2]];
                let to = [x + o[0], y + o[1], STAND_HEIGHT + o[2]];
                let hit = self.trace(from, to, slab_mins, slab_maxs);
                !hit.all_solid && hit.fraction >= 1.0
            })
        })
    }

    /// The box for this step: rolling, crouched or standing, and the eyes'
    /// height with it.
    fn settle_height(&mut self) {
        self.mins = [-HALF_WIDTH, -HALF_WIDTH, SOLE_Z];
        if roll_playing(self.ps, self.ps.legs.anim) {
            self.maxs[2] = CROUCH_HEIGHT;
            self.ps.eye_height = EYE_HEIGHT;
            self.ps.flags &= !motion_flags::DUCKED;
            self.ps.flags |= motion_flags::ROLLING;
            return;
        }
        if self.ps.flags & motion_flags::ROLLING != 0 {
            if self.room_to_stand() {
                self.maxs[2] = STAND_HEIGHT;
                self.ps.flags &= !motion_flags::ROLLING;
            }
        } else if self.cmd.up < 0 || self.floored() {
            self.ps.flags |= motion_flags::DUCKED;
        } else if self.ps.ducked() && self.room_to_stand() {
            self.maxs[2] = STAND_HEIGHT;
            self.ps.flags &= !motion_flags::DUCKED;
        }
        let (top, eyes) = if self.ps.ducked() {
            (CROUCH_HEIGHT, CROUCH_EYE_HEIGHT)
        } else if self.ps.flags & motion_flags::ROLLING != 0 {
            (CROUCH_HEIGHT, EYE_HEIGHT)
        } else {
            (STAND_HEIGHT, EYE_HEIGHT)
        };
        self.maxs[2] = top;
        self.ps.eye_height = eyes;
    }

    // --- The ground -------------------------------------------------------

    /// The box a quarter unit down from where it stands.
    fn probe_below(&self) -> Hit {
        let o = self.ps.origin;
        self.trace(o, [o[0], o[1], o[2] - 0.25], self.mins, self.maxs)
    }

    fn leave_ground(&mut self) {
        self.ps.ground = None;
        self.ground_plane = None;
        self.on_walkable = false;
    }

    /// Finds what the box stands on, landing it when it comes down.
    fn find_ground(&mut self) {
        let mut hit = self.probe_below();
        if hit.all_solid
            && let Some(spot) = self.room_nearby()
            && spot != self.ps.origin
        {
            self.ps.origin = spot;
            hit = self.probe_below();
        }

        if hit.all_solid && self.room_nearby().is_none() {
            if let Some(spot) = self.free_spot() {
                self.ps.origin = spot;
            }
            self.leave_ground();
            return;
        }
        if hit.fraction == 1.0 {
            self.ground_missed();
            self.ground_plane = None;
            self.on_walkable = false;
            return;
        }
        let velocity = self.ps.velocity;
        if velocity[2] > 0.0 && dot(velocity, hit.normal) > rules::movement::air::THROWN_OFF {
            if self.cmd.forward >= 0 {
                self.animator.legs_now(self.ps, Anim::BOTH_JUMP1);
                self.ps.flags &= !motion_flags::JUMPED_BACK;
            } else {
                self.animator.legs_now(self.ps, Anim::BOTH_JUMPBACK1);
                self.ps.flags |= motion_flags::JUMPED_BACK;
            }
            self.leave_ground();
            return;
        }
        if hit.normal[2] < WALKABLE_NORMAL_Z && !self.crevice(hit.normal) {
            self.ps.ground = None;
            self.on_walkable = false;
            self.ground_plane = Some(hit.normal);
            return;
        }

        self.ground_plane = Some(hit.normal);
        self.on_walkable = true;
        if self.ps.ground.is_none() {
            self.land();
            if self.velocity_before[2] < rules::movement::landing::HOLD_SPEED {
                self.ps.flags |= motion_flags::LAND_HOLD;
                self.ps.hold_ms = rules::movement::landing::HOLD_MS;
            }
        }
        self.ps.ground = Some(hit.ground);
        self.touch(hit.ground);
    }

    /// Nothing under the box: a fall starts, with its legs when it walked
    /// off a real drop.
    fn ground_missed(&mut self) {
        let stood = self.ps.ground.is_some();
        if stood || !self.ps.in_air_anim {
            let o = self.ps.origin;
            let drop = [o[0], o[1], o[2] - rules::movement::landing::FALL_DROP];
            if self.trace(o, drop, self.mins, self.maxs).fraction == 1.0 {
                if stood {
                    self.fall_legs();
                }
                self.ps.in_air_anim = true;
            }
        }
        if anim::roll_spent(self.ps, self.ps.legs.anim) {
            self.animator.play(
                self.ps,
                Parts::WHOLE,
                Anim::BOTH_INAIR1,
                play_flags::OVER_ANY | play_flags::HOLD_FULL,
            );
            self.ps.in_air_anim = true;
        }
        self.ps.ground = None;
    }

    /// The legs for leaving the ground without a jump.
    fn fall_legs(&mut self) {
        if self.ps.velocity[2] <= 0.0 && self.ps.flags & motion_flags::JUMP_PRESSED == 0 {
            self.animator.play(
                self.ps,
                Parts::LOWER,
                Anim::BOTH_INAIR1,
                play_flags::WHEN_FREE,
            );
            self.ps.flags &= !motion_flags::JUMPED_BACK;
        } else if self.cmd.forward >= 0 {
            self.animator.play(
                self.ps,
                Parts::LOWER,
                Anim::BOTH_JUMP1,
                play_flags::OVER_ANY,
            );
            self.ps.flags &= !motion_flags::JUMPED_BACK;
        } else {
            self.animator.play(
                self.ps,
                Parts::LOWER,
                Anim::BOTH_JUMPBACK1,
                play_flags::OVER_ANY,
            );
            self.ps.flags |= motion_flags::JUMPED_BACK;
        }
    }

    /// Touching down: how hard, the landing's legs, a roll out of it when
    /// crouched, and the event.
    fn land(&mut self) {
        use rules::movement::landing;

        // The speed at the instant of touchdown, from the fall this step.
        let rise = self.ps.origin[2] - self.origin_before[2];
        let v0 = self.velocity_before[2];
        let g = -self.ps.gravity;
        let half = g / 2.0;
        let disc = v0 * v0 - 4.0 * half * -rise;
        if disc < 0.0 {
            self.ps.in_air_anim = false;
            return;
        }
        let t = (-v0 - disc.sqrt()) / (2.0 * half);
        let impact = v0 + t * g;
        let mut severity = (f64::from(impact * impact) * 0.0001) as f32;
        if self.ps.ducked() {
            severity *= landing::CROUCHED;
        }

        let rolling = roll_playing(self.ps, self.ps.legs.anim);
        if !rolling && self.ps.in_air_anim {
            let anim = if self.ps.flags & motion_flags::JUMPED_BACK != 0 {
                Anim::BOTH_LANDBACK1
            } else {
                Anim::BOTH_LAND1
            };
            self.animator.legs_now(self.ps, anim);
            self.ps.legs.timer = landing::TIMER;
        }
        self.ps.in_air_anim = false;
        if severity < landing::SOFT {
            return;
        }

        let mut rolled = false;
        if self.ps.ducked() && severity >= landing::ROLL && !rolling {
            if anim::roll_spent(self.ps, self.ps.legs.anim) {
                self.ps.legs.timer = 0;
                self.ps.legs.anim = Anim::BOTH_STAND1;
                self.animator.play(
                    self.ps,
                    Parts::WHOLE,
                    Anim::BOTH_LAND1,
                    play_flags::OVER_ANY | play_flags::HOLD_FULL,
                );
                self.ps.legs.timer = landing::TIMER;
            } else if let Some(roll) = self.keyed_roll() {
                self.start_roll(roll);
                rolled = true;
            }
        }
        self.ps.events |= if rolled {
            events::ROLL
        } else if severity > landing::HURT {
            events::FALL
        } else {
            events::LAND
        };
        self.ps.velocity[2] = 0.0;
        self.ps.footstep_cycle = 0;
    }

    /// Whether the box on steep ground of `normal` has no way down it (a
    /// sharp crevice, its other side in the way): it stands there rather
    /// than hang in the air for good, a jump never landing (a backflip into
    /// a cleft of the Sand People's enclave on Tatooine; up the Ebon Hawk's
    /// ramp, steeper than walkable ground, from its foot). Only a body
    /// standing still or heading up the slope: one going across or down it
    /// slides off and lands (HK-47 crossing a bevel in the Leviathan's
    /// medical bay, else stood on it for good).
    fn crevice(&self, normal: Vec3) -> bool {
        // Down the slope: gravity less its part into the plane.
        let down = [
            normal[0] * normal[2],
            normal[1] * normal[2],
            normal[2] * normal[2] - 1.0,
        ];
        let slope = length(down);
        if slope < 1e-3 {
            return false;
        }
        let (f, r) = (f32::from(self.cmd.forward), f32::from(self.cmd.right));
        let wish = [
            self.view_forward[0] * f + self.view_right[0] * r,
            self.view_forward[1] * f + self.view_right[1] * r,
        ];
        // Up the slope (within `CREVICE_HEADING` of straight up it), or
        // standing still.
        let wished = math::flat_length([wish[0], wish[1], 0.0]);
        let level = math::flat_length([down[0], down[1], 0.0]);
        if wished > 0.0
            && level > 1e-3
            && -(wish[0] * down[0] + wish[1] * down[1]) < CREVICE_HEADING * wished * level
        {
            return false;
        }
        let o = self.ps.origin;
        let end = mad(o, CREVICE_UNITS / slope, down);
        self.trace(o, end, self.mins, self.maxs).fraction < rules::movement::fine::CREVICE_FREE
    }

    /// The first of the 27 spots within a unit of the origin on each axis
    /// (the origin among them) where the box fits (JA only checks for one:
    /// a body with room so near is left where it is).
    fn room_nearby(&self) -> Option<Vec3> {
        const NUDGES: [f32; 3] = [-1.0, 0.0, 1.0];
        let o = self.ps.origin;
        NUDGES.iter().find_map(|&dx| {
            NUDGES.iter().find_map(|&dy| {
                NUDGES.iter().find_map(|&dz| {
                    let spot = [o[0] + dx, o[1] + dy, o[2] + dz];
                    (!self.trace(spot, spot, self.mins, self.maxs).all_solid).then_some(spot)
                })
            })
        })
    }

    /// The nearest spot the box fits at, out from the origin in rings of
    /// `UNBURY_STEP` units to `UNBURY_REACH`, sixteen ways round and a step
    /// up at most: where a body wedged into the level (a blow on the Star
    /// Forge's second deck) is put.
    fn free_spot(&self) -> Option<Vec3> {
        let o = self.ps.origin;
        let rings = (UNBURY_REACH / UNBURY_STEP) as usize;
        (1..=rings).find_map(|ring| {
            let r = ring as f32 * UNBURY_STEP;
            (0..16).find_map(|k| {
                let (sin, cos) = math::sin_cos_deg(k as f32 * 22.5);
                [0.0, STEP_HEIGHT / 2.0, STEP_HEIGHT]
                    .into_iter()
                    .find_map(|dz| {
                        let spot = [o[0] + r * cos, o[1] + r * sin, o[2] + dz];
                        (!self.trace(spot, spot, self.mins, self.maxs).all_solid).then_some(spot)
                    })
            })
        })
    }

    // --- Moving -----------------------------------------------------------

    /// A corpse on the ground slows by a fixed amount each step.
    fn corpse_slide(&mut self) {
        if !self.on_walkable {
            return;
        }
        let mut way = self.ps.velocity;
        let speed = normalize(&mut way) - rules::movement::ground::DEAD_SLOW;
        self.ps.velocity = if speed <= 0.0 {
            [0.0; 3]
        } else {
            scale(way, speed)
        };
    }

    fn friction(&mut self) {
        use rules::movement::ground;
        let mut measured = self.ps.velocity;
        if self.on_walkable {
            measured[2] = 0.0;
        }
        let speed = length(measured);
        if speed < ground::STILL_BELOW {
            self.ps.velocity[0] = 0.0;
            self.ps.velocity[1] = 0.0;
            return;
        }
        let drop = if self.on_walkable && self.ps.flags & motion_flags::KNOCKBACK_HOLD == 0 {
            let control = if speed < STOP_SPEED {
                STOP_SPEED
            } else {
                speed
            };
            control * FRICTION * self.seconds
        } else {
            0.0
        };
        let factor = (speed - drop).max(0.0) / speed;
        self.ps.velocity = scale(self.ps.velocity, factor);
    }

    /// Gathers speed toward `way` up to `wanted` at `rate`.
    fn accelerate(&mut self, way: Vec3, wanted: f32, rate: f32) {
        let current = dot(self.ps.velocity, way);
        let add = wanted - current;
        if add <= 0.0 {
            return;
        }
        let amount = (rate * self.seconds * wanted).min(add);
        self.ps.velocity = mad(self.ps.velocity, amount, way);
    }

    /// How much of the step's speed the keys ask for, per unit of the key
    /// vector.
    fn key_scale(&self) -> f32 {
        let f = i32::from(self.cmd.forward);
        let r = i32::from(self.cmd.right);
        let most = f.abs().max(r.abs());
        if most == 0 {
            return 0.0;
        }
        let total = f64::from((f * f + r * r) as f32).sqrt() as f32;
        (f64::from(self.speed * most as f32) / (127.0 * f64::from(total))) as f32
    }

    /// The wanted direction and speed for the keys along `forward` and
    /// `right`, flattened when `flat`.
    fn wanted(&self, forward: Vec3, right: Vec3, flat: bool) -> (Vec3, f32) {
        let (f, r) = (f32::from(self.cmd.forward), f32::from(self.cmd.right));
        let mut keys: Vec3 = std::array::from_fn(|i| forward[i] * f + right[i] * r);
        if flat {
            keys[2] = 0.0;
        }
        let length = normalize(&mut keys);
        (keys, length * self.key_scale())
    }

    fn note_push_direction(&mut self) {
        self.ps.push_dir = push_direction(self.cmd.forward, self.cmd.right, self.ps.push_dir);
    }

    /// Moving on walkable ground.
    fn ground_move(&mut self) {
        use rules::movement::ground;

        if self.jump_check() {
            self.air_move();
            return;
        }
        self.friction();
        self.note_push_direction();

        let plane = self.ground_plane.unwrap_or([0.0, 0.0, 1.0]);
        let along = |v: Vec3| {
            let mut v = self.along_plane([v[0], v[1], 0.0], plane);
            normalize(&mut v);
            v
        };
        let (forward, right) = (along(self.view_forward), along(self.view_right));
        let (way, mut wanted) = self.wanted(forward, right, false);

        let slow = self.speed * ground::DUCK_SCALE;
        let rising_from_roll =
            self.ps.flags & motion_flags::ROLLING != 0 && !is_roll_anim(self.ps.legs.anim);
        if (self.ps.ducked() || rising_from_roll) && wanted > slow {
            wanted = slow;
        }

        let knocked_back = self.ps.flags & motion_flags::KNOCKBACK_HOLD != 0;
        let rate = if knocked_back {
            rules::movement::air::AIR_ACCELERATE
        } else if self.ps.flags & motion_flags::PACED != 0 && wanted > 0.0 {
            ground::ACCELERATE * (STOP_SPEED / wanted).max(1.0)
        } else {
            ground::ACCELERATE
        };
        self.accelerate(way, wanted, rate);
        if knocked_back {
            self.ps.velocity[2] -= self.ps.gravity * self.seconds;
        }

        // Keep the speed, turned along the ground.
        let speed = length(self.ps.velocity);
        let mut velocity = self.along_plane(self.ps.velocity, plane);
        normalize(&mut velocity);
        self.ps.velocity = scale(velocity, speed);

        if self.ps.velocity[0] == 0.0 && self.ps.velocity[1] == 0.0 {
            return;
        }
        self.climbing_slide(false);
    }

    /// Moving through the air (or down steep ground).
    fn air_move(&mut self) {
        self.jump_check();
        self.friction();
        self.note_push_direction();

        let flat = |v: Vec3| {
            let mut v = [v[0], v[1], 0.0];
            normalize(&mut v);
            v
        };
        let (forward, right) = (flat(self.view_forward), flat(self.view_right));
        let (way, wanted) = self.wanted(forward, right, true);
        self.accelerate(way, wanted, rules::movement::air::AIR_ACCELERATE);
        if let Some(plane) = self.ground_plane {
            self.ps.velocity = self.along_plane(self.ps.velocity, plane);
        }
        self.climbing_slide(true);
    }

    // --- Jumping ----------------------------------------------------------

    /// Starts a jump when the keys ask for one; true when one started.
    fn jump_check(&mut self) -> bool {
        if self.floored() || self.floor_anim_held() || roll_playing(self.ps, self.ps.legs.anim) {
            return false;
        }
        if self.force_jump() {
            return false;
        }
        let jump_axis = rules::movement::keys::JUMP_AXIS;
        if self.cmd.up < jump_axis && self.ps.ground.is_some() {
            return false;
        }
        if self.ps.flags & motion_flags::JUMP_PRESSED != 0 {
            self.cmd.up = 0;
            return false;
        }
        if self.ps.ground.is_none() {
            return false;
        }
        if self.cmd.up > 0 {
            self.ps.velocity[2] = JUMP_SPEED;
        }
        self.ground_plane = None;
        self.on_walkable = false;
        self.ps.flags |= motion_flags::JUMP_PRESSED;
        self.ps.ground = None;
        self.set_jump_start(self.ps.origin[2]);
        self.ps.events |= events::JUMP;
        self.jump_legs();
        true
    }

    /// Notes the height a jump leaves from (never exactly 0.0, which reads
    /// as no jump).
    pub(super) fn set_jump_start(&mut self, z: f32) {
        self.ps.takeoff_z = z;
        if self.ps.takeoff_z == 0.0 {
            self.ps.takeoff_z = z - 0.1;
        }
    }

    fn jump_legs(&mut self) {
        let (forward, right) = (self.cmd.forward, self.cmd.right);
        let anim = if forward > 0 {
            Anim::BOTH_JUMP1
        } else if forward < 0 {
            Anim::BOTH_JUMPBACK1
        } else if right > 0 {
            Anim::BOTH_JUMPRIGHT1
        } else if right < 0 {
            Anim::BOTH_JUMPLEFT1
        } else {
            Anim::BOTH_JUMP1
        };
        if forward < 0 {
            self.ps.flags |= motion_flags::JUMPED_BACK;
        } else {
            self.ps.flags &= !motion_flags::JUMPED_BACK;
        }
        self.animator
            .play(self.ps, Parts::LOWER, anim, play_flags::OVER_ANY);
    }

    // --- Rolls ------------------------------------------------------------

    /// The roll the keys ask for, if there is room to roll that way.
    fn keyed_roll(&self) -> Option<Anim> {
        let room = rolls::ROOM;
        let (forward, right, _) = math::angle_vectors([0.0, self.ps.view[1], 0.0]);
        let (roll, way, reach) = if self.cmd.forward != 0 {
            if self.ps.flags & motion_flags::RUNNING_BACK != 0 {
                (Anim::BOTH_ROLL_B, forward, -room)
            } else {
                (Anim::BOTH_ROLL_F, forward, room)
            }
        } else if self.cmd.right > 0 {
            (Anim::BOTH_ROLL_R, right, room)
        } else if self.cmd.right < 0 {
            (Anim::BOTH_ROLL_L, right, -room)
        } else {
            return None;
        };
        let mins = [self.mins[0], self.mins[1], self.mins[2] + STEP_HEIGHT];
        let maxs = [self.maxs[0], self.maxs[1], CROUCH_HEIGHT];
        let o = self.ps.origin;
        let hit = self.trace_masked(o, mad(o, reach, way), mins, maxs, Contents::SOLID);
        (hit.fraction >= 1.0).then_some(roll)
    }

    fn start_roll(&mut self, roll: Anim) {
        self.ps.legs.timer = 0;
        self.ps.legs.anim = Anim::BOTH_STAND1;
        self.animator.play(
            self.ps,
            Parts::WHOLE,
            roll,
            play_flags::OVER_ANY | play_flags::HOLD_FULL,
        );
    }

    // --- The legs' gait ---------------------------------------------------

    /// The legs' animation for how the body moves, and the footstep cycle.
    fn footsteps(&mut self) {
        use rules::movement::footsteps::CYCLE;

        let legs = self.ps.legs.anim;
        let keyed = self.cmd.forward != 0 || self.cmd.right != 0;
        let walk_held = self.cmd.buttons & buttons::WALKING != 0;
        let rolling = self.ps.flags & motion_flags::ROLLING != 0;
        let running = keyed && !walk_held && !self.ps.ducked() && !rolling;
        let set = if weapon::gait_overrides(legs, running) {
            play_flags::WHEN_FREE | play_flags::OVER_ANY
        } else {
            play_flags::WHEN_FREE
        };
        let v = self.ps.velocity;
        let flat_speed = f64::from(v[0] * v[0] + v[1] * v[1]).sqrt() as f32;

        if self.spinning_attack() {
            self.animator.legs_next(self.ps, self.ps.torso.anim);
            self.advance_footsteps(CYCLE[0]);
            return;
        }
        if self.ps.ground.is_none() {
            return;
        }
        if !keyed {
            if flat_speed < rules::movement::footsteps::STILL_BELOW {
                self.ps.footstep_cycle = 0;
                if self.ps.ducked() || rolling {
                    self.crouch_in(Anim::BOTH_CROUCH1IDLE, set);
                } else if !self.slope_stance() {
                    let stand = self.saber_stand().unwrap_or(Anim::BOTH_STAND1);
                    let stand = self.slope_return(stand);
                    self.animator.legs_next(self.ps, stand);
                }
            }
            return;
        }

        let pace = if self.ps.ducked() {
            if !self.roll_from_crouch(legs, v) {
                self.crouched_walk(set);
            }
            CYCLE[2]
        } else if rolling && !is_roll_anim(legs) {
            self.crouched_walk(set);
            CYCLE[2]
        } else {
            let backward = self.ps.flags & motion_flags::RUNNING_BACK != 0;
            let gait =
                self.saber_gait(walk_held, backward)
                    .unwrap_or(match (walk_held, backward) {
                        (false, false) => Anim::BOTH_RUN1,
                        (false, true) => Anim::BOTH_RUNBACK1,
                        (true, false) => Anim::BOTH_WALK1,
                        (true, true) => Anim::BOTH_WALKBACK1,
                    });
            let chosen = self.slope_return(gait);
            if self.ps.legs.anim != gait && chosen == gait {
                self.animator.play(self.ps, Parts::LOWER, gait, set);
            } else {
                self.animator.legs_next(self.ps, chosen);
            }
            if walk_held { CYCLE[0] } else { CYCLE[1] }
        };
        self.advance_footsteps(pace);
    }

    /// A crouched body running fast enough (or rolling out of the
    /// double-bladed kata) rolls the keys' way when there is room; true
    /// when a roll started.
    fn roll_from_crouch(&mut self, legs: Anim, velocity: Vec3) -> bool {
        let [kata_from, kata_to] = rules::weapons_in_hand::moves::SOULCAL_ROLL;
        let timer = self.ps.legs.timer;
        let fast = anim::is_run(legs) && dot(velocity, velocity) >= rolls::FROM_SPEED_SQUARED;
        let kata = legs == Anim::BOTH_A7_SOULCAL && timer > kata_from && timer < kata_to;
        if roll_playing(self.ps, legs) || !(fast || kata) {
            return false;
        }
        let Some(roll) = self.keyed_roll() else {
            return false;
        };
        self.start_roll(roll);
        self.ps.events |= events::ROLL;
        self.maxs[2] = CROUCH_HEIGHT;
        self.ps.eye_height = EYE_HEIGHT;
        self.ps.flags &= !motion_flags::DUCKED;
        self.ps.flags |= motion_flags::ROLLING;
        true
    }

    /// Plays `anim` on the legs, or carries on with it when it plays.
    fn crouch_in(&mut self, anim: Anim, set: u8) {
        if self.ps.legs.anim == anim {
            self.animator.legs_next(self.ps, anim);
        } else {
            self.animator.play(self.ps, Parts::LOWER, anim, set);
        }
    }

    fn crouched_walk(&mut self, set: u8) {
        let anim = if self.ps.flags & motion_flags::RUNNING_BACK != 0 {
            Anim::BOTH_CROUCH1WALKBACK
        } else {
            Anim::BOTH_CROUCH1WALK
        };
        self.crouch_in(anim, set);
    }

    // --- Slope stances ----------------------------------------------------

    /// The slope stance's family (0 left foot up, 1 right, 2 and 3 the same
    /// for the first saber style) and level (1 to 5) of `anim`, if it is one.
    fn slope_level(anim: Anim) -> Option<(u16, u16)> {
        if !anim.between(Anim::LEGS_LEFTUP1, Anim::LEGS_S1_RUP5) {
            return None;
        }
        let place = anim.index() - Anim::LEGS_LEFTUP1.index();
        Some((place / 5, place % 5 + 1))
    }

    /// The stance of `family` at `level`.
    fn slope_anim(family: u16, level: u16) -> Anim {
        Anim::LEGS_LEFTUP1.offset(i32::from(family * 5 + level - 1))
    }

    /// Leaving a slope stance a level at a time: while the legs stand high
    /// on a slope, the body holds still and steps down toward `wanted`.
    fn slope_return(&mut self, wanted: Anim) -> Anim {
        let legs = self.ps.legs.anim;
        match Self::slope_level(legs) {
            Some((_, level)) if level >= 2 => {
                let next = if self.ps.slope_next_ms < self.time_ms {
                    self.ps.slope_next_ms =
                        self.time_ms + rules::movement::footsteps::SLOPE_BACK_MS;
                    legs.offset(-1)
                } else {
                    legs
                };
                self.ps.velocity = [0.0; 3];
                next
            }
            _ => wanted,
        }
    }

    /// Standing still on uneven ground, one foot higher than the other: the
    /// legs take the stance for the difference; true when they do.
    fn slope_stance(&mut self) -> bool {
        let slope_step = rules::movement::footsteps::SLOPE_STEP;
        let slope_ms = rules::movement::footsteps::SLOPE_MS;

        let legs = self.ps.legs.anim;
        let Some(feet) = self.animator.table.get(legs).feet else {
            return false;
        };
        let (sin, cos) = math::sin_cos_deg(self.ps.view[1]);
        let o = self.ps.origin;
        let bottom = o[2] + self.mins[2] + 1.0;
        let sole_height = |[x, y]: [f32; 2]| {
            let at = [o[0] + x * cos - y * sin, o[1] + x * sin + y * cos];
            let from = [at[0], at[1], bottom];
            let to = [at[0], at[1], bottom - slope_step * 10.0];
            self.trace(from, to, [-3.0, -3.0, 0.0], [3.0, 3.0, 1.0]).end[2]
        };
        let diff = sole_height(feet[0]) - sole_height(feet[1]);

        let level_up = (1..=5u16)
            .rev()
            .find(|&n| diff >= slope_step * f32::from(n));
        let (mut family, level) = match level_up {
            Some(n) => (0, n),
            None => match (1..=5u16)
                .rev()
                .find(|&n| diff <= slope_step * -f32::from(n))
            {
                Some(n) => (1, n),
                None => return false,
            },
        };
        let legs_stance = Self::slope_level(legs);
        if legs == Anim::BOTH_STAND1 || matches!(legs_stance, Some((2 | 3, _))) {
            family += 2;
        } else if !(matches!(legs, Anim::BOTH_STAND2 | Anim::BOTH_CROUCH1IDLE)
            || matches!(legs_stance, Some((0 | 1, _))))
        {
            return false;
        }
        let mut target = Self::slope_anim(family, level);

        if legs_stance.is_some() {
            if target != legs && self.ps.slope_next_ms < self.time_ms {
                target = legs.offset(if target > legs { 1 } else { -1 });
                self.ps.slope_next_ms = self.time_ms + slope_ms;
            }
        } else {
            let side = family % 2;
            let family = if legs == Anim::BOTH_STAND1 {
                side + 2
            } else {
                side
            };
            target = Self::slope_anim(family, 1);
            self.ps.slope_next_ms = self.time_ms + slope_ms;
        }
        self.animator.legs_next(self.ps, target);
        true
    }

    /// The torso takes the legs' animation once nothing holds it (a plain
    /// stand for a slope stance).
    pub(super) fn torso_follows_legs(&mut self) {
        if self.ps.torso.timer > 0 {
            return;
        }
        let legs = self.ps.legs.anim;
        if anim::is_slope_stance(legs) {
            self.animator.play(
                self.ps,
                Parts::UPPER,
                Anim::BOTH_STAND1,
                play_flags::OVER_ANY,
            );
        } else if self.ps.torso.anim != legs {
            self.animator
                .play(self.ps, Parts::UPPER, legs, play_flags::OVER_ANY);
        }
    }
}
