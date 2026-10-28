//! Where a JA player's body points each frame: the legs' yaw (the model's
//! own), the torso's aim, and the angles the spine and neck bones are
//! turned by, as multiplayer's client works them out.
//!
//! - **Legs.** The legs chase the view's yaw, turned toward the travel when
//!   running on the ground (up to 60° off the view, the other way when
//!   backing up diagonally). With `quirks` they are kept within 90° of the
//!   target and the backward diagonal is judged from the keys alone
//!   (QUIRKS.md Q-0012, Q-0013).
//! - **Torso.** Its pitch chases three quarters of the view's, resting
//!   within a tolerance while the player stands ready; its yaw is the
//!   view's.
//! - **Spine.** What the view is turned from the legs, shared between the
//!   lumbar bones and the thoracic one, less the `Motion` bolt's own turn
//!   while an animation may be corrected. The correction eases in and out
//!   over 100 ms; with `quirks` it switches at once (Q-0014).
//! - **Head.** It turns toward a look target (or with the view), eased
//!   while a target was seen within the last second, within its reach;
//!   the neck bones take shares of it, the thoracic one averaging its share
//!   with the spine's (Q-0011).
//! - **Lock breaks** hold every bone straight and change nothing.

use crate::SPECIAL_TRAILS;

/// Pitch, yaw, roll in degrees.
pub type Angles = [f32; 3];

const PITCH: usize = 0;
const YAW: usize = 1;
const ROLL: usize = 2;

/// What the player is doing at a moment.
#[derive(Debug, Clone, Copy, Default)]
pub struct Steer {
    pub view: Angles,
    /// Units per second.
    pub velocity: [f32; 3],
    /// The keys' direction: 0 forward, counting 45° steps to the left (2
    /// left, 4 back, 6 right), as the game's movement sets it.
    pub move_dir: u8,
    pub on_ground: bool,
    /// The direction of what the player looks at, if anything, as angles.
    pub look: Option<Angles>,
}

/// A moment as the look sees it: what the player does and what its model
/// plays.
#[derive(Debug, Clone, Copy)]
pub struct Moment<'a> {
    pub steer: Steer,
    pub legs: &'a str,
    pub torso: &'a str,
    /// The weapon's ready animation (`BOTH_STAND2` for the saber):
    /// standing on `BOTH_STAND1` with the torso ready lets the torso's
    /// pitch rest within its tolerance.
    pub ready: &'a str,
    pub dead: bool,
    /// Reproduce Q-0012, Q-0013 and Q-0014 (off by default: they jolt).
    pub quirks: bool,
    /// The `Motion` bolt's angles, its forward axis read as a direction
    /// (roll from its right axis), in the model's space.
    pub motion: Angles,
}

/// What carries over from frame to frame: the legs' and torso's chasing
/// angles, the head's last angles and how long it keeps looking.
#[derive(Debug, Clone, Copy, Default)]
pub struct State {
    legs_yaw: Swing,
    torso_pitch: Swing,
    last_head: Angles,
    look_until: f32,
    /// How much of the motion correction applies (0 to 1), and the last
    /// one applied in full.
    correcting: f32,
    correction: Angles,
}

impl State {
    /// Standing facing `yaw` for a while: the legs there, nothing swinging.
    pub fn facing(yaw: f32) -> State {
        State {
            legs_yaw: Swing {
                angle: wrap_360(yaw),
                chasing: false,
            },
            ..State::default()
        }
    }
}

/// A frame's result: the model's yaw (the legs'), the torso's aim, and the
/// angle overrides of the spine and neck bones, in the order Ghoul2 is
/// given them.
#[derive(Debug, Clone, Default)]
pub struct Pose {
    pub legs_yaw: f32,
    pub torso: Angles,
    pub bones: Vec<(&'static str, Angles)>,
}

/// One frame, `frame_ms` after the last, at `time_ms`.
pub fn player_angles(state: &mut State, moment: &Moment, time_ms: f32, frame_ms: f32) -> Pose {
    let steer = &moment.steer;
    let view = steer.view;
    let (legs, torso) = (moment.legs, moment.torso);
    if lock_break(legs) || lock_break(torso) {
        return Pose {
            legs_yaw: view[YAW],
            torso: [state.torso_pitch.angle, view[YAW], 0.0],
            bones: [
                "lower_lumbar",
                "upper_lumbar",
                "cranium",
                "thoracic",
                "cervical",
            ]
            .map(|bone| (bone, [0.0; 3]))
            .to_vec(),
        };
    }

    let standing =
        legs.eq_ignore_ascii_case("BOTH_STAND1") && torso.eq_ignore_ascii_case(moment.ready);
    if !standing {
        state.legs_yaw.chasing = true;
        state.torso_pitch.chasing = true;
    }
    let head_yaw = wrap_360(view[YAW]);

    state
        .torso_pitch
        .chase(wrap(view[PITCH]) * 0.75, frame_ms, TORSO_PITCH);

    let [vx, vy, _] = steer.velocity;
    let still = roll_or_getup(legs) || special_move(torso);
    let moving = !still && steer.on_ground && (vx != 0.0 || vy != 0.0);
    let keys = if moment.dead { 0 } else { steer.move_dir };
    let mut legs_target = head_yaw;
    if moving {
        legs_target += travel_turn(head_yaw, vx, vy, keys, moment.quirks);
    }
    let legs_swing = Chase {
        leash: moment.quirks.then_some(90.0),
        ..LEGS_YAW
    };
    state.legs_yaw.chase(legs_target, frame_ms, legs_swing);
    let legs_yaw = state.legs_yaw.angle;

    let mut spine = [view[PITCH] * 0.5, wrap_180(view[YAW] - legs_yaw), 0.0];
    let corrects = corrects(moment);
    if corrects {
        state.correction = moment.motion.map(wrap_180);
    }
    let ease = frame_ms / 100.0;
    state.correcting = match (moment.quirks, corrects) {
        (true, true) => 1.0,
        (true, false) => 0.0,
        (false, true) => (state.correcting + ease).min(1.0),
        (false, false) => (state.correcting - ease).max(0.0),
    };
    if state.correcting > 0.0 {
        for (angle, correction) in spine.iter_mut().zip(state.correction) {
            *angle = wrap_180(*angle - correction * state.correcting);
        }
    }

    let lower = [spine[PITCH] * 0.40, spine[YAW] * 0.45, spine[ROLL] * 0.45];
    let upper = [spine[PITCH] * 0.40, spine[YAW] * 0.35, spine[ROLL] * 0.35];
    let spine_thoracic = spine.map(|angle| angle * 0.20);

    let mut start = match steer.look {
        Some(target) => {
            state.look_until = time_ms + 1000.0;
            target
        }
        None => view,
    };
    start[PITCH] = 0.0;
    let mut look: Angles = std::array::from_fn(|i| diff(wrap_180(start[i]), wrap_180(view[i])));
    if state.look_until > time_ms {
        for (angle, (low, high)) in look.iter_mut().zip(LOOK_REACH) {
            *angle = angle.clamp(low, high);
        }
        let gap: Angles = std::array::from_fn(|i| wrap_180(look[i] - state.last_head[i]));
        if gap.iter().any(|&g| g != 0.0) {
            look = std::array::from_fn(|i| wrap_180(state.last_head[i] + gap[i] * 0.1 * 1.5));
        }
    }
    state.last_head = look;

    let head: Angles = std::array::from_fn(|i| look[i].clamp(HEAD_REACH[i].0, HEAD_REACH[i].1));
    let share = |pitch: f32, rest: f32| [head[PITCH] * pitch, head[YAW] * rest, head[ROLL] * rest];
    let neck_thoracic = share(0.4, 0.1);
    let thoracic: Angles = std::array::from_fn(|i| {
        if spine_thoracic[i] != 0.0 {
            (spine_thoracic[i] + neck_thoracic[i]) * 0.5
        } else {
            neck_thoracic[i]
        }
    });

    Pose {
        legs_yaw,
        torso: [state.torso_pitch.angle, head_yaw, 0.0],
        bones: vec![
            ("cranium", share(0.4, 0.6)),
            ("cervical", share(0.2, 0.3)),
            ("thoracic", thoracic),
            ("lower_lumbar", lower),
            ("upper_lumbar", upper),
        ],
    }
}

/// How far the eased look may turn per axis, (low, high).
const LOOK_REACH: [(f32, f32); 3] = [(-50.0, 50.0), (-70.0, 70.0), (-30.0, 30.0)];
/// How far the head itself turns per axis, (low, high).
const HEAD_REACH: [(f32, f32); 3] = [(-25.0, 50.0), (-55.0, 50.0), (-10.0, 10.0)];

/// The legs' yaw offset from the view toward where the player travels:
/// up to 60°, mirrored when backing up diagonally.
fn travel_turn(head_yaw: f32, vx: f32, vy: f32, keys: u8, quirks: bool) -> f32 {
    let turned = (head_yaw - yaw2(-vx, -vy)).rem_euclid(360.0);
    // Where the travel comes from, round to the view the short way.
    let from = if turned < 180.0 {
        turned
    } else {
        turned - 360.0
    };
    let off = from.abs();
    let behind = off < 90.0;
    let mut lean = if behind { off } else { 180.0 - off }.min(60.0);
    let backward_diagonal = if quirks {
        keys == 3 || keys == 5
    } else {
        (3..=5).contains(&keys) && behind
    };
    if backward_diagonal {
        lean = -lean;
    }
    if from < 0.0 { -lean } else { lean }
}

/// Whether the spine takes the motion bolt's correction this frame: not
/// while flipping, spinning, jumping specially, dying, attacking
/// specially or knocked down, rolling, in a special move, dead, or when
/// legs and torso play one animation.
fn corrects(moment: &Moment) -> bool {
    let (legs, torso) = (moment.legs, moment.torso);
    let either = |rule: fn(&str) -> bool| rule(legs) || rule(torso);
    !(either(flipping)
        || either(spinning)
        || either(special_jump)
        || either(crate::is_death)
        || either(special_attack)
        || either(knockdown)
        || roll(legs)
        || special_move(torso)
        || moment.dead
        || legs.eq_ignore_ascii_case(torso))
}

/// How an angle follows its target.
#[derive(Debug, Clone, Copy)]
struct Chase {
    /// Within this the angle may rest.
    tolerance: f32,
    /// Degrees per ms at the normal pace.
    speed: f32,
    /// Never further behind than this.
    leash: Option<f32>,
}

const TORSO_PITCH: Chase = Chase {
    tolerance: 15.0,
    speed: 0.1,
    leash: Some(30.0),
};
const LEGS_YAW: Chase = Chase {
    tolerance: 0.0,
    speed: 0.65,
    leash: None,
};

/// An angle on the 16-bit circle chasing a target, and whether it is.
#[derive(Debug, Clone, Copy, Default)]
struct Swing {
    angle: f32,
    chasing: bool,
}

impl Swing {
    /// One frame of `frame_ms` toward `target`.
    fn chase(&mut self, target: f32, frame_ms: f32, how: Chase) {
        let gap = diff(target, self.angle);
        if gap.abs() > how.tolerance {
            self.chasing = true;
        }
        if !self.chasing {
            return;
        }
        // Slower near the target, faster once well outside the tolerance.
        let pace = if gap.abs() < how.tolerance * 0.5 {
            0.5
        } else if gap.abs() < how.tolerance {
            1.0
        } else {
            2.0
        };
        let step = frame_ms * pace * how.speed;
        if step >= gap.abs() {
            self.angle = wrap_360(self.angle + gap);
            self.chasing = false;
        } else {
            self.angle = wrap_360(self.angle + step.copysign(gap));
        }
        if let Some(leash) = how.leash {
            let behind = diff(target, self.angle);
            if behind.abs() > leash {
                self.angle = wrap_360(target - (leash - 1.0).copysign(behind));
            }
        }
    }
}

/// `angle` on the game's 16-bit circle, in [0, 360).
pub fn wrap_360(angle: f32) -> f32 {
    const TO_SHORT: f32 = 65536.0_f32 / 360.0_f32;
    const TO_DEGREES: f32 = 360.0_f32 / 65536.0_f32;
    ((angle * TO_SHORT) as i32 & 65535) as f32 * TO_DEGREES
}

/// `angle` on the 16-bit circle, in (-180, 180].
pub fn wrap_180(angle: f32) -> f32 {
    wrap(wrap_360(angle))
}

/// An angle in [0, 360) moved to (-180, 180].
fn wrap(angle: f32) -> f32 {
    if angle > 180.0 { angle - 360.0 } else { angle }
}

/// `a - b` brought within [-180, 180] a turn at a time.
fn diff(a: f32, b: f32) -> f32 {
    let mut d = a - b;
    while d > 180.0 {
        d -= 360.0;
    }
    while d < -180.0 {
        d += 360.0;
    }
    d
}

/// The yaw of a flat direction, in [0, 360); 0 for none.
fn yaw2(x: f32, y: f32) -> f32 {
    if x == 0.0 && y == 0.0 {
        return 0.0;
    }
    let yaw = if x != 0.0 {
        y.atan2(x).to_degrees()
    } else if y > 0.0 {
        90.0
    } else {
        270.0
    };
    if yaw < 0.0 { yaw + 360.0 } else { yaw }
}

/// A direction as (pitch, yaw, 0): pitch negative looking up.
pub fn look_angles(v: [f32; 3]) -> Angles {
    let (pitch, yaw) = if v[0] == 0.0 && v[1] == 0.0 {
        (if v[2] > 0.0 { 90.0 } else { 270.0 }, 0.0)
    } else {
        let flat = (v[0] * v[0] + v[1] * v[1]).sqrt();
        let pitch = v[2].atan2(flat).to_degrees();
        (
            if pitch < 0.0 { pitch + 360.0 } else { pitch },
            yaw2(v[0], v[1]),
        )
    };
    [-pitch, yaw, 0.0]
}

fn upper(name: &str) -> String {
    name.to_ascii_uppercase()
}

/// The saber lock breaks one player can play alone.
fn lock_break(name: &str) -> bool {
    [
        "BOTH_BF1BREAK",
        "BOTH_BF2BREAK",
        "BOTH_CCWCIRCLEBREAK",
        "BOTH_CWCIRCLEBREAK",
    ]
    .contains(&upper(name).as_str())
}

fn flipping(name: &str) -> bool {
    let name = upper(name);
    crate::FLIPPING.contains(&name.as_str())
}

/// A saber transition that swings from one side across to the other, far
/// enough round for its style to count as a spin.
fn spinning(name: &str) -> bool {
    let name = upper(name);
    let Some(rest) = name.strip_prefix("BOTH_T") else {
        return false;
    };
    let (style, from, to) = match rest.as_bytes() {
        [style, b'_', f0, f1, b'_', t0, t1] => (*style, [*f0, *f1], [*t0, *t1]),
        _ => return false,
    };
    // (side, row) of a quadrant: left 0, right 1; top 0, middle 1, bottom 2.
    let quadrant = |code: [u8; 2]| {
        let side = match code[1] {
            b'L' => 0,
            b'R' => 1,
            _ => return None,
        };
        let row = match code[0] {
            b'T' => 0,
            b'_' => 1,
            b'B' => 2,
            _ => return None,
        };
        Some((side, row))
    };
    let (Some((from_side, from_row)), Some((to_side, to_row))) = (quadrant(from), quadrant(to))
    else {
        return false;
    };
    if from_side == to_side {
        return false;
    }
    let rows = from_row + to_row;
    match style {
        b'1' => rows >= 2,
        b'2' | b'3' => rows >= 3,
        b'4' => from == *b"BR" && to == *b"_L",
        _ => false,
    }
}

fn roll(name: &str) -> bool {
    ["BOTH_ROLL_F", "BOTH_ROLL_B", "BOTH_ROLL_R", "BOTH_ROLL_L"].contains(&upper(name).as_str())
}

fn roll_or_getup(name: &str) -> bool {
    let up = upper(name);
    roll(name) || up.starts_with("BOTH_GETUP_BROLL_") || up.starts_with("BOTH_GETUP_FROLL_")
}

fn knockdown(name: &str) -> bool {
    let up = upper(name);
    ["BOTH_KNOCKDOWN", "BOTH_GETUP", "BOTH_FORCE_GETUP_"]
        .iter()
        .any(|prefix| up.starts_with(prefix))
}

/// A special saber move or a kick (read off the torso).
fn special_move(name: &str) -> bool {
    let up = upper(name);
    SPECIAL_TRAILS.iter().any(|(special, _)| *special == up) || up.starts_with("BOTH_A7_KICK_")
}

/// The flips, wall runs, butterflies, arials, cartwheels and leaps that
/// leave the ground.
const SPECIAL_JUMPS: [&str; 37] = [
    "BOTH_A7_SOULCAL",
    "BOTH_ARIAL_F1",
    "BOTH_ARIAL_LEFT",
    "BOTH_ARIAL_RIGHT",
    "BOTH_BUTTERFLY_FL1",
    "BOTH_BUTTERFLY_FR1",
    "BOTH_BUTTERFLY_LEFT",
    "BOTH_BUTTERFLY_RIGHT",
    "BOTH_CARTWHEEL_LEFT",
    "BOTH_CARTWHEEL_RIGHT",
    "BOTH_FJSS_TL_BR",
    "BOTH_FJSS_TR_BL",
    "BOTH_FLIP_ATTACK7",
    "BOTH_FLIP_BACK1",
    "BOTH_FLIP_BACK2",
    "BOTH_FLIP_BACK3",
    "BOTH_FLIP_HOLD7",
    "BOTH_FLIP_LAND",
    "BOTH_FORCELEAP2_T__B_",
    "BOTH_FORCELONGLEAP_ATTACK",
    "BOTH_FORCELONGLEAP_START",
    "BOTH_FORCEWALLRUNFLIP_ALT",
    "BOTH_FORCEWALLRUNFLIP_END",
    "BOTH_FORCEWALLRUNFLIP_START",
    "BOTH_JUMPATTACK6",
    "BOTH_JUMPATTACK7",
    "BOTH_JUMPFLIPSLASHDOWN1",
    "BOTH_JUMPFLIPSTABDOWN",
    "BOTH_WALL_FLIP_BACK1",
    "BOTH_WALL_FLIP_LEFT",
    "BOTH_WALL_FLIP_RIGHT",
    "BOTH_WALL_RUN_LEFT",
    "BOTH_WALL_RUN_LEFT_FLIP",
    "BOTH_WALL_RUN_LEFT_STOP",
    "BOTH_WALL_RUN_RIGHT",
    "BOTH_WALL_RUN_RIGHT_FLIP",
    "BOTH_WALL_RUN_RIGHT_STOP",
];

fn special_jump(name: &str) -> bool {
    SPECIAL_JUMPS.contains(&upper(name).as_str())
}

/// The special saber moves but the arials and cartwheels, the kicks, and
/// the two flip slashes.
fn special_attack(name: &str) -> bool {
    const TUMBLES: [&str; 4] = [
        "BOTH_ARIAL_LEFT",
        "BOTH_ARIAL_RIGHT",
        "BOTH_CARTWHEEL_LEFT",
        "BOTH_CARTWHEEL_RIGHT",
    ];
    const KICKS: [&str; 11] = [
        "B", "BF", "F", "L", "R", "RL", "S", "B_AIR", "F_AIR", "L_AIR", "R_AIR",
    ];
    let up = upper(name);
    let special = SPECIAL_TRAILS
        .iter()
        .any(|(special, _)| *special == up && !TUMBLES.contains(special));
    let kick = up
        .strip_prefix("BOTH_A7_KICK_")
        .is_some_and(|kick| KICKS.contains(&kick));
    special || kick || up == "BOTH_FJSS_TL_BR" || up == "BOTH_FJSS_TR_BL"
}
