//! A character aiming a blaster raises it to eye level, the way a soldier
//! looks down the sights.
//!
//! - **The raise:** the body holds the last frame of its gun's shot
//!   (`cast::Figure::raise`), where JA's arms leave the gun after firing.
//!   The arms are then turned further by the wield's corrections
//!   (`corrections`) until the gun stands before the eyes. One pistol is
//!   held out in front of the face. A pistol in each hand is held up the
//!   same way, side by side. A rifle, a repeating blaster or any other gun
//!   that is not a pistol is held with both hands at the shoulder.
//! - **How fast:** a player's body follows the sim's raise level
//!   (`Shooting::aim_share`), eased (`eased`). The gun goes up over 200 ms
//!   and down over 150 ms, the sim's own times, and it never jumps.
//! - **Other characters:** the view does not ask the sim how far they
//!   aim. Each of their shots raises their guns, and they keep them up
//!   until `OTHERS_HOLD_MS` after their last shot.
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
/// that the gun stands before the eyes. Each is a bone of JA's skeleton
/// and its turn at full raise, in degrees (pitch, yaw, roll about the
/// bone's own axes, as `asset_ja::g2::Player::turn` takes them).
pub(crate) fn corrections(wield: Wield) -> &'static [(&'static str, [f32; 3])] {
    match wield {
        Wield::Pistol => PISTOL,
        Wield::Pair => PAIR,
        Wield::Rifle => RIFLE,
    }
}

// The turns were fitted on JA's skeleton so that each gun hand ends where
// a soldier holds it (`aim_pose_probes`), then checked on film. Only the
// shoulders and the chest turn. The elbows keep the bend of the shot's
// last frame, so they never bend sideways. A bone's second angle turns it
// about its own length.

/// One pistol: the right arm comes up and in until the hand is 2 cm under
/// the eyes and 40 cm in front of them, and the chest turns toward the gun.
const PISTOL: &[(&str, [f32; 3])] = &[
    ("rhumerus", [34.0, -17.0, 11.0]),
    ("thoracic", [0.0, 20.0, 0.0]),
];
/// A pistol in each hand: both arms come up, the hands 4 to 5 cm under the
/// eyes and 37 cm in front of them, 10 cm to either side.
const PAIR: &[(&str, [f32; 3])] = &[
    ("rhumerus", [38.0, -24.0, -17.0]),
    ("lhumerus", [19.0, -60.0, -76.0]),
    ("thoracic", [0.0, -20.0, 0.0]),
];
/// A rifle: the right hand comes up to 2 cm under the eyes and 13 cm in
/// front of them, the elbow out to the side, and the left arm reaches
/// forward under the barrel.
const RIFLE: &[(&str, [f32; 3])] = &[
    ("rhumerus", [69.0, 39.0, 59.0]),
    ("lhumerus", [30.0, -45.0, -14.0]),
    ("thoracic", [0.0, -20.0, 0.0]),
];

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
    /// Held up as far as this share (0 to 1, eased), the sim's raise.
    Level(f32),
}

/// How far a body's guns are held up at `now_ms`. A body a seat acts
/// through (`seat`) follows the sim: its raise while the gun is raised
/// at all, else a timed raise while it charges a Power Blast or steadies
/// a Sniper Shot. Any other body holds them up from each of its shots
/// until `OTHERS_HOLD_MS` after its last one (`last_shot_ms`).
pub(crate) fn lift(
    seat: bool,
    shooting: &Shooting,
    last_shot_ms: Option<f64>,
    now_ms: f64,
) -> Lift {
    if !seat {
        let held =
            last_shot_ms.is_some_and(|shot| (0.0..OTHERS_HOLD_MS).contains(&(now_ms - shot)));
        return if held { Lift::Timed } else { Lift::Down };
    }
    if shooting.aim > 0 {
        Lift::Level(eased(shooting.aim_share()))
    } else if shooting.charging_from.is_some() {
        Lift::Timed
    } else {
        Lift::Down
    }
}
