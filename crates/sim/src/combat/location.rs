//! Where on a body a hit lands (JA's hit locations): the head, the chest
//! and its sides, the back and its sides, the waist, or an arm, hand, leg
//! or foot on either side. The location scales the damage
//! (`Location::per_mille`) and picks the death a killing blow plays
//! (`death::pick`).
//!
//! The body is its box, turned to its view's yaw. A hit is judged by the
//! direction from the box's centre to the point struck, read along the
//! body's height, front and side. Each reading falls in one of five bands
//! by the `[location]` rules' edges; the bands find the limbs and the
//! head, and the readings themselves split what is left of the torso. The
//! front of the torso's middle is the chest, where JA finds no location
//! (QUIRKS.md Q-0030).

use crate::combat::fighter::Location;
use crate::math::{self, Vec3};
use rules::combat::location;

/// The band, 0 (lowest) to 4 (highest), a `reading` falls in by four
/// `edges`, highest first: one band lower for each edge, from the first,
/// that it is at or below, until the first it is above. A NaN is above
/// every edge.
fn band(reading: f32, edges: [f32; 4]) -> u8 {
    let under = edges.iter().take_while(|&&edge| reading <= edge).count();
    4 - under as u8
}

/// Where a hit at `point` lands on the body that fills the box from `min`
/// to `max`, facing `yaw` degrees. `None` for a hit at the box's very
/// centre, which has no direction.
pub fn locate(point: Vec3, min: Vec3, max: Vec3, yaw: f32) -> Location {
    use Location::*;
    let (forward, right, up) = math::angle_vectors([0.0, yaw, 0.0]);
    let centre = [
        (min[0] + max[0]) * 0.5,
        (min[1] + max[1]) * 0.5,
        (min[2] + max[2]) * 0.5,
    ];
    let mut direction = math::sub(point, centre);
    if math::normalize(&mut direction) == 0.0 {
        return None;
    }
    let height = math::dot(up, direction);
    let front = math::dot(forward, direction);
    let side = math::dot(right, direction);

    let v = band(height, location::VERTICAL_EDGES);
    let a = band(front, location::WIDE_EDGES);
    let s = band(side, location::WIDE_EDGES);
    // A limb is the right one only to the right of the body's middle.
    let limb = |left: Location, right: Location| if side > 0.0 { right } else { left };

    let foot = v == 0 && (a <= 1 || (a == 2 && s == 0));
    if foot {
        return limb(FootLeft, FootRight);
    }
    if v == 0 || v == 1 || (v == 2 && a == 0 && s == 0) {
        return limb(LegLeft, LegRight);
    }
    if v == 2 && s <= 1 && (2..=4).contains(&(a + s)) {
        return limb(HandLeft, HandRight);
    }
    if v == 3 && matches!(s, 2 | 3) && (4..=6).contains(&(a + s)) {
        return limb(ArmLeft, ArmRight);
    }
    if v == 4 && matches!(a, 1..=3) && matches!(s, 2..=4) {
        return Head;
    }
    if height < location::WAIST_BELOW {
        Waist
    } else if front < 0.0 {
        if side > location::BACK_SIDE {
            BackRight
        } else if side < -location::BACK_SIDE {
            BackLeft
        } else {
            Back
        }
    } else if side > location::CHEST_SIDE {
        ChestRight
    } else if side < -location::CHEST_SIDE {
        ChestLeft
    } else {
        Chest
    }
}
