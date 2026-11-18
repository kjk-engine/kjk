//! A character aiming a blaster raises it: a pistol held out at cheek
//! level, a rifle with its stock at the shoulder.
//!
//! - **The raise:** the body holds the last frame of its gun's shot
//!   (`cast::Figure::raise`), where JA's arms leave the gun after firing.
//!   The arms are then turned further by the wield's corrections
//!   (`corrections`). One pistol is held out at arm's length, its sights
//!   level with the cheek. A pistol in each hand is held up the same way,
//!   side by side. A rifle, a repeating blaster or any other gun
//!   that is not a pistol is held with both hands at the shoulder.
//! - **The head:** with one pistol or a rifle, the head leans toward the
//!   gun side as the gun comes up (`head_tilt_deg`). It does not turn where
//!   the character looks, and the body the player looks through in first
//!   person keeps its head upright.
//! - **How fast:** a player's body follows the sim's raise level
//!   (`Shooting::aim_share`), eased (`eased`). The gun goes up over 200 ms
//!   and down over 150 ms, the sim's own times, and it never jumps.
//! - **Other characters:** the view does not ask the sim how far they
//!   aim. Each of their shots raises their guns, and they keep them up
//!   until `OTHERS_HOLD_MS` after their last shot. They also raise them
//!   while they charge a Power Blast or steady a Sniper Shot.
//! - **The legs** are not touched, so a character that walks while aiming
//!   keeps walking.
//!
//! The view alone: the sim's skeleton holds JA's pose.

use sim::combat::specials::Shooting;

/// How a character holds the guns it raises.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Wield {
    Pistol,
    Pair,
    Rifle,
}

/// The wield of a body by its KOTOR wield number (`stance::wield`): 5 one
/// pistol, 6 a pistol in each hand. Every other gun is held as a rifle.
pub(crate) fn wield_of(wield: u8) -> Wield {
    match wield {
        5 => Wield::Pistol,
        6 => Wield::Pair,
        _ => Wield::Rifle,
    }
}

/// The turns added to JA's bones over the held last frame of the shot, so
/// that the gun is held up to aim. Each is a bone of JA's skeleton
/// and its turn at full raise, in degrees (pitch, yaw, roll about the
/// bone's own axes, as `asset_ja::g2::Player::turn` takes them).
pub(crate) fn corrections(wield: Wield) -> &'static [(&'static str, [f32; 3])] {
    match wield {
        Wield::Pistol => PISTOL,
        Wield::Pair => PAIR,
        Wield::Rifle => RIFLE,
    }
}

// The turns were fitted so that each gun stands where a soldier holds it on
// the KOTOR body the player sees: JA's pose retargeted to Carth's body
// (`p_carthbb`), as the firing motion's probes pose it. Then they were
// checked in still pictures. The shoulders, the elbows and the chest turn.
// A bone's second angle turns it about its own length.

/// One pistol: the right arm is held out nearly straight ahead of the right
/// shoulder, bent about 20 degrees at the elbow, with the wrist 2 cm below
/// the shoulder's height and 54 cm in front of it. The pistol's sights are
/// then level with the cheek, a little below the eyes, and the gun is about
/// 15 cm to the right of the face's middle and 43 cm in front of it. The
/// chest turns toward the gun.
const PISTOL: &[(&str, [f32; 3])] = &[
    ("rhumerus", [18.5, -23.5, 11.0]),
    ("rradius", [-24.0, 0.5, 0.0]),
    ("thoracic", [0.0, 20.0, 0.0]),
];
/// A pistol in each hand: the chest turns square to the aim, and each arm
/// is held out nearly straight ahead of its own shoulder, bent about 20
/// degrees at the elbow. The sights are level with the cheeks, and the two
/// guns stand about 28 cm apart.
const PAIR: &[(&str, [f32; 3])] = &[
    ("rhumerus", [20.0, 3.5, 17.0]),
    ("rradius", [-33.5, 0.0, -20.5]),
    ("lhumerus", [64.0, -69.0, -93.0]),
    ("lradius", [-60.0, -25.0, 28.0]),
    ("thoracic", [0.0, 18.0, 0.0]),
];
/// A rifle: the stock sits by the right shoulder and the collarbone, its
/// rear end straight ahead of the shoulder joint, 11 cm in front of it and
/// 6 cm below it. Every part of the rifle stays at least 10 cm from the
/// shoulder joint and the upper arm's bone, so it does not pass into the
/// shoulder. The right elbow is bent and hangs down. The chest turns the
/// right side back, so the rifle lies along the body, and the upright head
/// stays 9 cm clear of it. The left hand reaches under the barrel on its
/// own (`asset_ja::rig::Aim::support`), so its arm takes no turn here.
const RIFLE: &[(&str, [f32; 3])] = &[
    ("rhumerus", [17.5, 38.0, 1.5]),
    ("rradius", [31.5, 0.5, -53.0]),
    ("thoracic", [0.0, -44.5, 0.0]),
];

/// How far (degrees) a character's head leans toward the gun side at full
/// raise, as someone lining up a sight does. A character with one pistol
/// or a rifle leans its head 7 degrees. A character with a pistol in each
/// hand looks straight between them and does not lean its head.
pub(crate) fn head_tilt_deg(wield: Wield) -> f32 {
    match wield {
        Wield::Pistol | Wield::Rifle => 7.0,
        Wield::Pair => 0.0,
    }
}

/// The raise share (0 to 1) eased in and out: Perlin's smootherstep, which
/// starts and ends with no speed and no change of speed.
pub(crate) fn eased(share: f32) -> f32 {
    let s = share.clamp(0.0, 1.0);
    s * s * s * (s * (s * 6.0 - 15.0) + 10.0)
}

/// How long another character keeps its guns up after its last shot (ms).
pub(crate) const OTHERS_HOLD_MS: f64 = 600.0;

/// How far a body's guns are held up.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum Lift {
    /// Not held up.
    Down,
    /// Held up, rising over a fixed time from when they went up.
    Timed,
    /// Another character's, held up after its shots. It rises over the
    /// same fixed time, from when the shot that raised it began.
    AfterShot,
    /// Held up as far as this share (0 to 1, eased), the sim's raise.
    Level(f32),
}

/// How far a body's guns are held up at `now_ms`. A body a seat acts
/// through (`seat`) follows the sim's raise while the gun is raised at
/// all. Any body has a timed raise while it charges a Power Blast or
/// steadies a Sniper Shot. Any other body also holds its guns up from each
/// of its shots until `OTHERS_HOLD_MS` after its last one (`last_shot_ms`).
pub(crate) fn lift(
    seat: bool,
    shooting: &Shooting,
    last_shot_ms: Option<f64>,
    now_ms: f64,
) -> Lift {
    let after_shot =
        !seat && last_shot_ms.is_some_and(|shot| (0.0..OTHERS_HOLD_MS).contains(&(now_ms - shot)));
    if seat && shooting.aim > 0 {
        Lift::Level(eased(shooting.aim_share()))
    } else if shooting.charging_from.is_some() {
        Lift::Timed
    } else if after_shot {
        Lift::AfterShot
    } else {
        Lift::Down
    }
}
