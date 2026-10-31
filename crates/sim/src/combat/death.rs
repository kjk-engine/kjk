//! How a body takes a blow it does not shrug off: the death it falls in,
//! the odds that a hit it lives through makes it flinch, and the pain it
//! plays when it does.
//!
//! A killed body plays one of JA's nineteen deaths (`pick`), chosen by
//! where the killing blow landed (`location::locate`), how heavy it was on
//! JA's 100-point scale, whether the body was moving and whether the
//! killer held a saber; some of the choices are rolls on the stream the
//! caller passes. A hit it lives through makes it flinch with the odds
//! `pain_chance` gives in thousandths (the caller rolls against them), and
//! the flinch plays the pain `pain` picks.
//!
//! Some deaths KOTOR stages instead (`in_place`): a blow the story stages,
//! a script's death effect, or any blow that falls in a scene. The body
//! then keeps its place and plays KOTOR's own dying clip (`DIE`, started
//! by `dying`), which the views hold at its last frame.

use crate::combat::damage::Source;
use crate::combat::fighter::Location;
use crate::combat::idle::Acting;
use crate::motion::Anim;
use crate::rng::SimRng;

/// The dying clip a death in place plays (`Acting::animation`):
/// `animations.2da` row 80, `die`, as the views number the engine's clips
/// (10 000 and the row). The views hold its last frame: the body lies as
/// it ends.
pub const DIE: u16 = 10_080;

/// Whether a death by `source` is one KOTOR stages (`scene`: the blow
/// falls in a scene, `scene::Held::scene`).
pub fn in_place(source: &Source, scene: bool) -> bool {
    scene || matches!(source, Source::Script { staged: true, .. } | Source::Death)
}

/// The dying clip a death in place plays from `now` (ms): the views time
/// it by the clip and hold its end.
pub fn dying(now: u64) -> Acting {
    Acting {
        animation: DIE,
        since: now,
        ms: 0,
    }
}

/// How a body lying as a death left it gets up (a downed party member's
/// rise, `world::falls`): JA's plain get-up for the side it lies on, as
/// JA's knockdowns pick theirs (`bg_pmove`: knocked backwards, `GETUP4`'s
/// start on the back; forwards, `GETUP3`'s on the front), and how far the
/// views turn that get-up (degrees, anticlockwise from above) for its
/// lying start to meet where the death left the head. KOTOR gets a party
/// member up in the clip matched to its dying one (its supermodels'
/// `getupdead` after `die`, `getupdead1` after `die1`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Getup {
    pub anim: Anim,
    pub turn: f32,
}

/// Where each of JA's nineteen deaths leaves the body, measured on
/// `_humanoid` (`kjk rig a --moves BOTH_DEATHn --trace`, the last frame):
/// on its front or back (the chest's normal down or up), and the head's
/// direction from the feet (degrees from the model's right, its forward
/// +90).
const LYING: [(Anim, bool, f32); 19] = [
    (Anim::BOTH_DEATH1, false, -94.0),
    (Anim::BOTH_DEATH2, false, -80.0),
    (Anim::BOTH_DEATH3, false, 91.0),
    (Anim::BOTH_DEATH4, true, 78.0),
    (Anim::BOTH_DEATH5, false, 47.0),
    (Anim::BOTH_DEATH6, false, 47.0),
    (Anim::BOTH_DEATH7, false, 90.0),
    (Anim::BOTH_DEATH8, false, -89.0),
    (Anim::BOTH_DEATH9, false, 78.0),
    (Anim::BOTH_DEATH10, true, 101.0),
    (Anim::BOTH_DEATH11, false, 102.0),
    (Anim::BOTH_DEATH12, false, 134.0),
    (Anim::BOTH_DEATH13, false, -92.0),
    (Anim::BOTH_DEATH14, false, -106.0),
    (Anim::BOTH_DEATH15, true, 122.0),
    (Anim::BOTH_DEATH16, true, 119.0),
    (Anim::BOTH_DEATH17, true, 122.0),
    (Anim::BOTH_DEATH18, true, 89.0),
    (Anim::BOTH_DEATH19, true, 91.0),
];

/// The get-ups, on the back and on the front, and their first frames'
/// head direction (as `LYING`).
const BACK: (Anim, f32) = (Anim::BOTH_GETUP4, -90.0);
const FRONT: (Anim, f32) = (Anim::BOTH_GETUP3, 87.0);

/// The get-up for a body lying in `death` (its legs' animation); any other
/// (a death not JA's, a body that never fell) gets up as from the back.
pub fn getup(death: Anim) -> Getup {
    let (front, head) = LYING
        .iter()
        .find(|(anim, ..)| *anim == death)
        .map_or((false, BACK.1), |&(_, front, head)| (front, head));
    let (anim, start) = if front { FRONT } else { BACK };
    Getup {
        anim,
        turn: (head - start + 180.0).rem_euclid(360.0) - 180.0,
    }
}

/// The death a killed body plays (JA's): by where the killing blow landed,
/// how heavy it was on the 100-point scale (`equivalent`: damage × 100 /
/// the most hit points; it may lie outside 0 to 100), whether the body was
/// `moving`, and whether the killer held a `saber`. Some choices roll on
/// `rng`; a blow with no location (a power's, a blast's) rolls among all
/// nineteen deaths.
pub fn pick(
    location: Location,
    equivalent: i32,
    moving: bool,
    saber: bool,
    rng: &mut SimRng,
) -> Anim {
    use Location::*;
    let quarter = quarter(equivalent);
    match location {
        FootLeft | FootRight if saber && rng.below(3) == 0 => Anim::BOTH_DEATH10,
        FootLeft | FootRight | LegLeft | LegRight => falling_forward(rng),
        Back if moving => falling_forward(rng),
        Back => Anim::BOTH_DEATH17,
        ChestRight | ArmRight | HandRight | BackRight => side_hit(RIGHT_SIDE, quarter, rng),
        ChestLeft | ArmLeft | HandLeft | BackLeft => side_hit(LEFT_SIDE, quarter, rng),
        Chest | Waist if quarter == 1 || !moving => {
            [Anim::BOTH_DEATH18, Anim::BOTH_DEATH19][rng.below(2) as usize]
        }
        Chest | Waist => match quarter {
            2 => Anim::BOTH_DEATH2,
            3 => [Anim::BOTH_DEATH1, Anim::BOTH_DEATH16][rng.below(2) as usize],
            _ => Anim::BOTH_DEATH10,
        },
        Head if quarter <= 2 => Anim::BOTH_DEATH17,
        Head => Anim::BOTH_DEATH13,
        None => {
            // The nineteen deaths stand in order in the animation list.
            let k = rng.below(19) as u16;
            Anim::from_index(Anim::BOTH_DEATH1.index() + k).unwrap_or(Anim::BOTH_DEATH1)
        }
    }
}

/// Which quarter of the 100-point scale a blow falls in, 1 to 4: up to 25
/// is the first, up to 50 the second, up to 75 the third, anything heavier
/// the fourth (a blow at or below 0 is in the first).
fn quarter(equivalent: i32) -> u8 {
    match equivalent {
        i32::MIN..=25 => 1,
        26..=50 => 2,
        51..=75 => 3,
        _ => 4,
    }
}

/// A fall forward: one time in three the first of them, else one of the
/// other two.
fn falling_forward(rng: &mut SimRng) -> Anim {
    if rng.below(3) == 0 {
        Anim::BOTH_DEATH4
    } else {
        [Anim::BOTH_DEATH5, Anim::BOTH_DEATH15][rng.below(2) as usize]
    }
}

/// The four deaths of a hit to one side: snapped round, thrown back, spun,
/// and thrown high.
struct Side {
    snap: Anim,
    back: Anim,
    spin: Anim,
    high: Anim,
}

const RIGHT_SIDE: Side = Side {
    snap: Anim::BOTH_DEATH9,
    back: Anim::BOTH_DEATH3,
    spin: Anim::BOTH_DEATH6,
    high: Anim::BOTH_DEATH8,
};

const LEFT_SIDE: Side = Side {
    snap: Anim::BOTH_DEATH11,
    back: Anim::BOTH_DEATH7,
    spin: Anim::BOTH_DEATH12,
    high: Anim::BOTH_DEATH14,
};

/// A death from a hit to one `side`: the heavier the blow the further it
/// throws, from the snap in the first quarter to the spin in the third; a
/// blow above three quarters is thrown high half the time, else falls in
/// any of the other three.
fn side_hit(side: Side, quarter: u8, rng: &mut SimRng) -> Anim {
    match quarter {
        1 => side.snap,
        2 => side.back,
        3 => side.spin,
        _ if rng.below(2) == 1 => side.high,
        _ => [side.snap, side.back, side.spin][rng.below(3) as usize],
    }
}

/// The odds, in thousandths, that a hit of `damage` leaving `hp` (both in
/// hundredths of a point, of `max_hp` whole points) makes a creature
/// flinch: always when it was caught unawares (`surprised`) or cut by a
/// `blade`, never when it is a `droid`, always from a blow of more than
/// half its most; otherwise by how hurt it already is and how heavy the
/// blow, divided by the flinch skill.
pub fn pain_chance(
    damage: i32,
    hp: i32,
    max_hp: u16,
    surprised: bool,
    blade: bool,
    droid: bool,
) -> i64 {
    let most = i64::from(max_hp.max(1)) * 100;
    let damage = i64::from(damage);
    let hp = i64::from(hp);
    if surprised || blade {
        return 1000;
    }
    if droid {
        return 0;
    }
    if damage * 2 > most {
        return 1000;
    }
    let lost = (most - hp).max(0) * 1000 / (2 * most);
    let blow = damage * 2000 / most;
    ((lost + blow) / rules::combat::flinch::SKILL_DIVISOR).clamp(0, 1000)
}

/// The pain a flinch plays: holding a saber pain 2 or 3 (JA's), else by
/// the hit's side, pain 2 for the left, 3 for the right, 1 for the rest
/// (the sim's `Anim` holds JA's first three pains).
pub fn pain(location: Location, saber: bool, rng: &mut SimRng) -> Anim {
    use Location::*;
    match location {
        _ if saber => [Anim::BOTH_PAIN2, Anim::BOTH_PAIN3][rng.below(2) as usize],
        ChestLeft | BackLeft | ArmLeft | HandLeft | LegLeft | FootLeft => Anim::BOTH_PAIN2,
        ChestRight | BackRight | ArmRight | HandRight | LegRight | FootRight => Anim::BOTH_PAIN3,
        _ => Anim::BOTH_PAIN1,
    }
}
