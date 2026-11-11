//! The audit's checks on the frames of a log: how far the drawn saber and
//! body stray, each frame, from JA's reference and from the sim.
//!
//! A frame's blades that are still growing or shrinking are skipped by every
//! check. Where a check compares the drawn body with JA's reference, it
//! measures both the same way and keeps what the drawn body adds (clamped
//! at 0). The reference's lengths, speeds and accelerations are first scaled
//! to the drawn body's size (`scale`), as JA's skeleton is smaller than the
//! KOTOR body.

use bevy::math::Vec3;

use kjk_game::anim_audit_log::ja_bone;

use super::{Blade, Frame};

/// One check of the audit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Check {
    /// How much farther each blade's drawn hilt is from the hand gripping
    /// it than the reference's hilt is from the reference's hand.
    Grip,
    /// How much farther the off hand is from a staff's hilt line than the
    /// reference's off hand is from its own. Counted only while the
    /// reference's off hand holds the hilt (`OFF_HAND_HOLDS`).
    TwoHands,
    /// How much deeper the drawn blade cuts into the drawn body than the
    /// reference blade cuts into the reference body.
    SelfCut,
    /// How much sharper the drawn hands, head and feet jolt than the
    /// reference's.
    Pops,
    /// How much faster the drawn blade turns than the reference blade.
    Spin,
    /// How much faster the drawn hands move than the reference's, in a
    /// stance that should stand still. The state's first `SETTLE_MS` is
    /// not counted, as the body is still moving into the pose.
    Stillness,
    /// The distance between the drawn blade and the sim's, at the hilt or
    /// the tip, while the saber is in contact.
    ContactGap,
}

impl Check {
    pub const ALL: [Check; 7] = [
        Check::Grip,
        Check::TwoHands,
        Check::SelfCut,
        Check::Pops,
        Check::Spin,
        Check::Stillness,
        Check::ContactGap,
    ];

    /// The largest value the check allows. Grip, TwoHands, SelfCut and
    /// ContactGap are in metres, Pops in m/s², Spin in degrees per second
    /// and Stillness in m/s. These are the design limits. Under MDA's rules
    /// a value is also judged against JA's rules (`report::floor`). Grip's
    /// limit lies just above JA's own worst Grip, 0.104 m, as the KOTOR hilt
    /// sits about 6 cm off where JA's sits.
    pub fn limit(self) -> f32 {
        match self {
            Check::Grip => 0.11,
            Check::TwoHands => 0.08,
            Check::SelfCut => 0.03,
            Check::Pops => 400.0,
            Check::Spin => 720.0,
            Check::Stillness => 0.15,
            Check::ContactGap => 0.10,
        }
    }

    /// The check `name` names (`Check::name`).
    pub fn named(name: &str) -> Option<Check> {
        Check::ALL.into_iter().find(|check| check.name() == name)
    }

    pub fn name(self) -> &'static str {
        match self {
            Check::Grip => "grip",
            Check::TwoHands => "two-hands",
            Check::SelfCut => "self-cut",
            Check::Pops => "pops",
            Check::Spin => "spin",
            Check::Stillness => "stillness",
            Check::ContactGap => "contact-gap",
        }
    }
}

/// A check's worst value over the frames, and the moment of the frame where
/// it was found (sim ms).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Worst {
    pub check: Check,
    pub value: f32,
    pub ms: f64,
}

// The body parts the checks use, by the drawn body's KOTOR node. The
// reference's JA bone for each is the log's (`ja_bone`).
const PELVIS: &str = "pelvis_g";
const TORSO: &str = "torsoupr_g";
const LEFT_HAND: &str = "lhand_g";
const HANDS: [&str; 2] = [LEFT_HAND, "rhand_g"];
const FEET: [&str; 2] = ["lfoot_g", "rfoot_g"];
/// Each upper leg, from its hip to its knee.
const LEGS: [(&str, &str); 2] = [("lthigh_g", "lshin_g"), ("rthigh_g", "rshin_g")];
/// The reference's head is centred on the bone that stands for `head_g`. The drawn body has no `head_g`. Its head hangs on `headhook`,
/// and its centre is `HEAD_ABOVE_HOOK` above it.
const HEAD: &str = "head_g";
const HEAD_HOOK: &str = "headhook";
const HEAD_ABOVE_HOOK: f32 = 0.12;

const HEAD_RADIUS: f32 = 0.11;
const TORSO_RADIUS: f32 = 0.16;
const LEG_RADIUS: f32 = 0.08;
/// The length of blade from the hilt that sits in the hand, which SelfCut
/// does not count.
const IN_HAND: f32 = 0.15;
/// The reference's off hand holds a staff while it is this close to the
/// staff's hilt line (metres).
const OFF_HAND_HOLDS: f32 = 0.12;
/// The shortest time step (seconds) a speed is measured over, as in
/// `rig_timeline::measure`.
const SHORTEST_STEP: f32 = 1e-4;
/// How long (ms) the body takes to move into a still state's pose. The
/// block pose's entry moves the hands at up to 8 m/s for about 150 ms.
/// Stillness measures only the steps that start this long after the
/// state's first frame.
const SETTLE_MS: f64 = 300.0;

/// The drawn body or JA's reference.
#[derive(Clone, Copy)]
enum Side {
    Drawn,
    Reference,
}

impl Side {
    /// The place of the body part the KOTOR `node` names on this side.
    fn node(self, frame: &Frame, node: &str) -> Option<Vec3> {
        let found = match self {
            Side::Drawn => frame.nodes.get(node),
            Side::Reference => frame.ref_nodes.get(ja_bone(node)?),
        };
        found.copied().map(Vec3::from_array)
    }

    fn head(self, frame: &Frame) -> Option<Vec3> {
        match self {
            Side::Drawn => {
                let hook = frame.nodes.get(HEAD_HOOK).copied().map(Vec3::from_array);
                Some(hook? + Vec3::Z * HEAD_ABOVE_HOOK)
            }
            Side::Reference => self.node(frame, HEAD),
        }
    }

    /// The blade's hilt and tip on this side.
    fn blade(self, blade: &Blade) -> (Vec3, Vec3) {
        let [hilt, tip] = match self {
            Side::Drawn => blade.drawn,
            Side::Reference => blade.reference,
        };
        (Vec3::from_array(hilt), Vec3::from_array(tip))
    }

    /// The joints Pops follows that the frame has: the hands, the feet and
    /// the head.
    fn joints(self, frame: &Frame) -> [Option<Vec3>; 5] {
        let node = |part| self.node(frame, part);
        [
            node(HANDS[0]),
            node(HANDS[1]),
            node(FEET[0]),
            node(FEET[1]),
            self.head(frame),
        ]
    }

    /// The body's capsules the frame has, each an axis and a radius. The
    /// head is a sphere, an axis of no length.
    fn capsules(self, frame: &Frame) -> Vec<((Vec3, Vec3), f32)> {
        let node = |part| self.node(frame, part);
        let head = self.head(frame).map(|at| ((at, at), HEAD_RADIUS));
        let torso = node(PELVIS)
            .zip(node(TORSO))
            .map(|axis| (axis, TORSO_RADIUS));
        let legs = LEGS
            .iter()
            .map(|&(hip, knee)| Some(((node(hip)?, node(knee)?), LEG_RADIUS)));
        [head, torso].into_iter().chain(legs).flatten().collect()
    }
}

/// The size of the drawn body against the reference's: the median length
/// from pelvis to upper torso of the drawn body over the frames, divided by
/// the reference's. It is 1 when either is missing from every frame or has
/// no length.
pub fn scale(frames: &[Frame]) -> f32 {
    let length = |side: Side| {
        let lengths = frames
            .iter()
            .filter_map(|frame| Some(side.node(frame, TORSO)?.distance(side.node(frame, PELVIS)?)));
        median(lengths.collect())
    };
    match (length(Side::Drawn), length(Side::Reference)) {
        (Some(drawn), Some(reference)) if drawn > f32::EPSILON && reference > f32::EPSILON => {
            drawn / reference
        }
        _ => 1.0,
    }
}

fn median(mut values: Vec<f32>) -> Option<f32> {
    values.sort_by(f32::total_cmp);
    let middle = values.len() / 2;
    match values.len() {
        0 => None,
        n if n % 2 == 0 => Some((values[middle - 1] + values[middle]) / 2.0),
        _ => Some(values[middle]),
    }
}

/// The worst value of each check that applies to the frames. TwoHands
/// applies only to a `staff`, and Stillness only to a stance that should
/// stand `still`. A check that finds nothing has a worst of 0 at the first
/// frame.
pub fn measure(frames: &[Frame], staff: bool, still: bool) -> Vec<Worst> {
    let s = scale(frames);
    let first = frames.first().map_or(0.0, |frame| frame.ms);
    Check::ALL
        .into_iter()
        .filter(|check| match check {
            Check::TwoHands => staff,
            Check::Stillness => still,
            _ => true,
        })
        .map(|check| {
            let mut worst = Worst {
                check,
                value: 0.0,
                ms: first,
            };
            for (i, frame) in frames.iter().enumerate() {
                // A NaN becomes the worst and stays it, so a measure that
                // breaks is seen rather than hidden.
                if let Some(value) = value(check, frames, i, s)
                    && !worst.value.is_nan()
                    && (value.is_nan() || value > worst.value)
                {
                    worst.value = value;
                    worst.ms = frame.ms;
                }
            }
            worst
        })
        .collect()
}

/// The value of `check` at frame `i`, where it can be measured. `s` is the
/// reference's scale (`scale`).
fn value(check: Check, frames: &[Frame], i: usize, s: f32) -> Option<f32> {
    let frame = &frames[i];
    let before = i.checked_sub(1).map(|j| &frames[j]);
    match check {
        Check::Grip => largest(lit(frame).map(|blade| grip(frame, blade, s))),
        Check::TwoHands => two_hands(frame, s),
        Check::SelfCut => largest(lit(frame).map(|blade| {
            let depth = |side: Side| depth(side.blade(blade), &side.capsules(frame));
            added(depth(Side::Drawn), s * depth(Side::Reference))
        })),
        Check::Pops => pops([before?, frame, frames.get(i + 1)?], s),
        Check::Spin => spin(before?, frame),
        Check::Stillness => {
            let settled = before.filter(|before| before.ms >= frames[0].ms + SETTLE_MS);
            stillness(settled?, frame, s)
        }
        Check::ContactGap if frame.contact => largest(lit(frame).map(|blade| {
            let (drawn, sim) = (Side::Drawn.blade(blade), ends(blade.sim));
            drawn.0.distance(sim.0).max(drawn.1.distance(sim.1))
        })),
        Check::ContactGap => None,
    }
}

/// Grip of one blade: how much farther the drawn hilt is from the drawn
/// hand gripping it than the reference hilt is from the same hand's JA
/// bone, scaled. The hand node sits at the wrist, so neither distance is 0
/// in a good grip. The gripping hand is the drawn hand node nearest the
/// blade's `hand`. Without that hand on the reference, its distance is 0.
fn grip(frame: &Frame, blade: &Blade, s: f32) -> f32 {
    let hand = Vec3::from_array(blade.hand);
    let drawn = Side::Drawn.blade(blade).0.distance(hand);
    let gripping = HANDS
        .into_iter()
        .filter_map(|node| Some((node, Side::Drawn.node(frame, node)?.distance(hand))))
        .min_by(|a, b| a.1.total_cmp(&b.1));
    let reference = gripping.and_then(|(node, _)| {
        let bone = Side::Reference.node(frame, node)?;
        Some(Side::Reference.blade(blade).0.distance(bone))
    });
    added(drawn, s * reference.unwrap_or(0.0))
}

/// The frame's blades that are not growing or shrinking.
fn lit(frame: &Frame) -> impl Iterator<Item = &Blade> {
    frame.blades.iter().filter(|blade| !blade.growing)
}

fn ends([hilt, tip]: [[f32; 3]; 2]) -> (Vec3, Vec3) {
    (Vec3::from_array(hilt), Vec3::from_array(tip))
}

/// The largest of `values`, or NaN if any is NaN (`f32::max` would drop it).
fn largest(values: impl Iterator<Item = f32>) -> Option<f32> {
    values.reduce(|a, b| if a.is_nan() || a >= b { a } else { b })
}

/// What the drawn body adds over the scaled reference, never below 0. A NaN
/// stays NaN (`f32::max` would make it 0).
fn added(drawn: f32, reference: f32) -> f32 {
    let more = drawn - reference;
    if more < 0.0 { 0.0 } else { more }
}

/// The time from frame `a` to frame `b` in seconds, where it is long enough
/// to measure a speed over.
fn step(a: &Frame, b: &Frame) -> Option<f32> {
    let dt = ((b.ms - a.ms) / 1000.0) as f32;
    (dt > SHORTEST_STEP).then_some(dt)
}

/// TwoHands at one frame: the staff's first lit blade gives its hilt line,
/// the line through the blade's hilt and tip.
fn two_hands(frame: &Frame, s: f32) -> Option<f32> {
    let blade = lit(frame).min_by_key(|blade| blade.slot)?;
    let off_hand = |side: Side| {
        let (hilt, tip) = side.blade(blade);
        Some(from_line(side.node(frame, LEFT_HAND)?, hilt, tip))
    };
    let reference = off_hand(Side::Reference).filter(|d| *d <= OFF_HAND_HOLDS)?;
    Some(added(off_hand(Side::Drawn)?, s * reference))
}

/// The distance from `point` to the line through `a` and `b`.
fn from_line(point: Vec3, a: Vec3, b: Vec3) -> f32 {
    let along = (b - a).normalize_or_zero();
    let off = point - a;
    (off - along * off.dot(along)).length()
}

/// How deep the blade from `hilt` to `tip` reaches into the deepest of
/// `capsules`, leaving out its first `IN_HAND` from the hilt.
fn depth((hilt, tip): (Vec3, Vec3), capsules: &[((Vec3, Vec3), f32)]) -> f32 {
    let length = hilt.distance(tip);
    if length <= IN_HAND {
        return 0.0;
    }
    let start = hilt + (tip - hilt) * (IN_HAND / length);
    let depths = capsules
        .iter()
        .map(|&((a, b), radius)| radius - segments_apart((start, tip), (a, b)));
    largest(depths).map_or(0.0, |deepest| added(deepest, 0.0))
}

/// The shortest distance between segments `p` and `q` (Ericson, Real-Time
/// Collision Detection, 5.1.9). Either may be of no length.
fn segments_apart((p1, p2): (Vec3, Vec3), (q1, q2): (Vec3, Vec3)) -> f32 {
    let (d1, d2, r) = (p2 - p1, q2 - q1, p1 - q1);
    let (a, e, f) = (d1.dot(d1), d2.dot(d2), d2.dot(r));
    let (s, t) = if a <= f32::EPSILON && e <= f32::EPSILON {
        (0.0, 0.0)
    } else if a <= f32::EPSILON {
        (0.0, (f / e).clamp(0.0, 1.0))
    } else {
        let c = d1.dot(r);
        if e <= f32::EPSILON {
            ((-c / a).clamp(0.0, 1.0), 0.0)
        } else {
            let b = d1.dot(d2);
            let denom = a * e - b * b;
            let s = if denom > f32::EPSILON {
                ((b * f - c * e) / denom).clamp(0.0, 1.0)
            } else {
                0.0
            };
            let t = (b * s + f) / e;
            if t < 0.0 {
                ((-c / a).clamp(0.0, 1.0), 0.0)
            } else if t > 1.0 {
                (((b - c) / a).clamp(0.0, 1.0), 1.0)
            } else {
                (s, t)
            }
        }
    };
    (p1 + d1 * s).distance(q1 + d2 * t)
}

/// Pops at the middle of three frames: the largest acceleration of the
/// drawn joints, less the reference's largest, scaled. Each acceleration is
/// the change of the two speeds over the mean step, as in
/// `rig_timeline::measure`.
fn pops([a, b, c]: [&Frame; 3], s: f32) -> Option<f32> {
    let (d0, d1) = (step(a, b)?, step(b, c)?);
    let sharpest = |side: Side| {
        let joints = side
            .joints(a)
            .into_iter()
            .zip(side.joints(b))
            .zip(side.joints(c));
        largest(joints.filter_map(|((p0, p1), p2)| {
            let (v0, v1) = ((p1? - p0?) / d0, (p2? - p1?) / d1);
            Some(((v1 - v0) / ((d0 + d1) / 2.0)).length())
        }))
    };
    let reference = sharpest(Side::Reference).unwrap_or(0.0);
    Some(added(sharpest(Side::Drawn)?, s * reference))
}

/// Spin from frame `a` to frame `b`: for each blade lit in both, how much
/// faster the drawn blade's direction turns than the reference's, in degrees
/// per second. An angle rate is not scaled.
fn spin(a: &Frame, b: &Frame) -> Option<f32> {
    let dt = step(a, b)?;
    let turn = |side: Side, from: &Blade, to: &Blade| {
        let direction = |blade| {
            let (hilt, tip) = side.blade(blade);
            (tip - hilt).normalize_or_zero()
        };
        // `angle_between` divides by the lengths, so a zero direction gives
        // NaN. The angle from the cross and the dot products is 0 there.
        let (from, to) = (direction(from), direction(to));
        from.cross(to).length().atan2(from.dot(to)).to_degrees() / dt
    };
    largest(lit(b).filter_map(|to| {
        let from = lit(a).find(|from| from.slot == to.slot)?;
        Some(added(
            turn(Side::Drawn, from, to),
            turn(Side::Reference, from, to),
        ))
    }))
}

/// Stillness from frame `a` to frame `b`: how much faster a drawn hand
/// moves than the reference's same hand, scaled.
fn stillness(a: &Frame, b: &Frame, s: f32) -> Option<f32> {
    let dt = step(a, b)?;
    let speed =
        |side: Side, hand: &str| Some(side.node(b, hand)?.distance(side.node(a, hand)?) / dt);
    largest(HANDS.into_iter().filter_map(|hand| {
        let reference = speed(Side::Reference, hand).unwrap_or(0.0);
        Some(added(speed(Side::Drawn, hand)?, s * reference))
    }))
}
