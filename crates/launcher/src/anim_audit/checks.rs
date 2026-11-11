//! The audit's checks on the frames of a log: how far the drawn saber and
//! body stray, each frame, from JA's reference and from the sim.
//!
//! A frame's blades that are still growing or shrinking are skipped by every
//! check. Where a check compares the drawn body with JA's reference, it
//! measures both the same way and keeps what the drawn body adds (clamped
//! at 0). The reference's lengths, speeds and accelerations are first scaled
//! to the drawn body's size (`scale`, by the length of the arms).

use bevy::math::{Quat, Vec3};

use kjk_game::anim_audit_log::{CHEST, ja_bone};

use super::{Blade, Frame};

/// One check of the audit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Check {
    /// How much farther each blade's drawn hilt is from the hand gripping
    /// it than in the case's steadiest hold of that blade (`Holds`). The
    /// reference is not used. JA's hilt moves 0.04 to 0.22 m in JA's own
    /// hand bone, even under JA's rules.
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
    /// How fast the drawn hands move in a state that should stand still
    /// (`Still`). The state's first `SETTLE_MS` is not counted, as the body
    /// is still moving into the pose. In a state with `Still::Hands` the
    /// value is how much faster a hand moves than the reference's. In a
    /// held block (`Still::Chest`) the value is the speed of the hand that
    /// grips each blade in the drawn chest's own frame, which turns with the
    /// chest. The reference is not used. The design gives a held block a rule for its reference.
    /// The parry plays once and holds its last frame while the legs walk
    /// on, so the gripping hand does not move against the chest once the
    /// parry has played. Kjk's own parry timing could not be the
    /// reference, because a fault in it would show on both sides and
    /// cancel out.
    Stillness,
    /// The distance between the drawn blade and the sim's, at the hilt or
    /// the tip, while the saber is in contact.
    ContactGap,
    /// How far the drawn blade turns (degrees) in the `WINDOW_MS` after a
    /// swing begins (`swing_begins`).
    SwingStartTurn,
    /// How far the drawn tip moves (metres) in the `WINDOW_MS` after a swing
    /// begins. A turn of the whole body shows here, as it moves the tip
    /// without turning the blade.
    SwingStartTravel,
    /// How far the drawn blade turns (degrees) in the `WINDOW_MS` after the
    /// drawn body starts to play another sequence than the sim's.
    StanceEntryTurn,
    /// How far the drawn tip moves (metres) in the `WINDOW_MS` after the
    /// drawn body starts to play another sequence than the sim's.
    StanceEntryTravel,
    /// The distance between the drawn blade and the sim's, at the hilt or
    /// the tip, at the frame where the saber move starts to cut.
    CutStartGap,
}

impl Check {
    pub const ALL: [Check; 12] = [
        Check::Grip,
        Check::TwoHands,
        Check::SelfCut,
        Check::Pops,
        Check::Spin,
        Check::Stillness,
        Check::ContactGap,
        Check::SwingStartTurn,
        Check::SwingStartTravel,
        Check::StanceEntryTurn,
        Check::StanceEntryTravel,
        Check::CutStartGap,
    ];

    /// The largest value the check allows. Grip, TwoHands, SelfCut,
    /// ContactGap, CutStartGap and the two travel checks are in metres, Pops
    /// in m/s², Spin in degrees per second, Stillness in m/s and the two
    /// turn checks in degrees. These are the design limits. Under MDA's rules
    /// a value is also judged against JA's rules (`report::floor`).
    pub fn limit(self) -> f32 {
        match self {
            Check::Grip => 0.04,
            Check::TwoHands => 0.08,
            Check::SelfCut => 0.03,
            Check::Pops => 400.0,
            Check::Spin => 720.0,
            Check::Stillness => 0.15,
            Check::ContactGap => 0.10,
            Check::SwingStartTurn | Check::StanceEntryTurn => 68.0,
            Check::SwingStartTravel | Check::StanceEntryTravel => 1.25,
            Check::CutStartGap => 0.10,
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
            Check::SwingStartTurn => "swing-start-turn",
            Check::SwingStartTravel => "swing-start-travel",
            Check::StanceEntryTurn => "stance-entry-turn",
            Check::StanceEntryTravel => "stance-entry-travel",
            Check::CutStartGap => "cut-start-gap",
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
const TORSO: &str = CHEST;
const LEFT_HAND: &str = "lhand_g";
const HANDS: [&str; 2] = [LEFT_HAND, "rhand_g"];
const FEET: [&str; 2] = ["lfoot_g", "rfoot_g"];
/// Each upper leg, from its hip to its knee.
const LEGS: [(&str, &str); 2] = [("lthigh_g", "lshin_g"), ("rthigh_g", "rshin_g")];
/// Each arm, from its upper arm through its forearm to its hand, which
/// `scale` measures.
const ARMS: [[&str; 3]; 2] = [
    ["lbicep_g", "lforearm_g", LEFT_HAND],
    ["rbicep_g", "rforearm_g", "rhand_g"],
];
/// The drawn body has no `head_g`. Its head hangs on `headhook`. JA's
/// `cranium`, the reference's bone for `headhook`, sits at the same height
/// (1.16 m above the ankles in JA's stand, on both sides). Each head's
/// centre is `HEAD_ABOVE_HOOK` above its bone.
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
/// How long (ms) after a swing begins, or after the drawn body starts to play
/// another sequence than the sim's, the turn and the travel of the drawn blade
/// are added up.
const WINDOW_MS: f64 = 100.0;

/// How Stillness judges a state. A state that does not stand still has
/// `Still::No`, and Stillness is not measured there.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Still {
    No,
    /// The hands stay as still as the reference's.
    Hands,
    /// The hand that grips each blade stays still against the drawn chest.
    Chest,
}

/// What `value` needs besides the frames: the reference's scale (`scale`),
/// the case's steadiest holds and how the state is judged for Stillness.
#[derive(Clone, Copy)]
struct Context<'a> {
    scale: f32,
    holds: &'a Holds,
    still: Still,
}

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
        Some(self.node(frame, HEAD_HOOK)? + Vec3::Z * HEAD_ABOVE_HOOK)
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

/// The size of the drawn body against the reference's: the median, over
/// the frames and both arms, of the drawn arm's length over the
/// reference's. An arm's length runs from the upper arm through the forearm
/// to the hand (`ARMS`). The torso is not used, because KOTOR's `pelvis_g`
/// sits about 0.3 m above JA's pelvis bone, which would make the scale
/// about 0.3 for bodies whose limbs are 0.9 to 1.25 times JA's. An arm that
/// either side lacks, or whose reference has no length, is skipped. The
/// scale is 1 when no arm is left.
pub fn scale(frames: &[Frame]) -> f32 {
    let ratios = frames.iter().flat_map(|frame| {
        ARMS.iter().filter_map(move |&arm| {
            let length = |side: Side| {
                let [upper, fore, hand] = arm.map(|node| side.node(frame, node));
                Some(upper?.distance(fore?) + fore?.distance(hand?))
            };
            let (drawn, reference) = (length(Side::Drawn)?, length(Side::Reference)?);
            (reference > f32::EPSILON).then(|| drawn / reference)
        })
    });
    match median(ratios.collect()) {
        Some(ratio) if ratio > f32::EPSILON => ratio,
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

/// The steadiest hold of each blade in a case. For each blade slot it is
/// the smallest distance from the drawn hilt to the drawn hand that grips
/// it, over the case's frames where the blade is lit.
#[derive(Debug, Clone, Default)]
pub struct Holds(std::collections::BTreeMap<u8, f32>);

/// The steadiest hold of each blade over `frames`, a whole case's frames.
pub fn holds(frames: &[Frame]) -> Holds {
    let mut steadiest = std::collections::BTreeMap::new();
    for blade in frames.iter().flat_map(lit) {
        let held = hold(blade);
        steadiest
            .entry(blade.slot)
            .and_modify(|least: &mut f32| *least = least.min(held))
            .or_insert(held);
    }
    Holds(steadiest)
}

/// The distance from the drawn hilt of `blade` to the drawn hand that grips
/// it.
fn hold(blade: &Blade) -> f32 {
    Side::Drawn
        .blade(blade)
        .0
        .distance(Vec3::from_array(blade.hand))
}

/// The worst value of each check that applies to the frames. The case's
/// steadiest blade holds are `holds`. TwoHands applies only to a `staff`,
/// and Stillness only when `still` is not `Still::No`. A check that finds
/// nothing has a worst of 0 at the first frame.
pub fn measure(frames: &[Frame], holds: &Holds, staff: bool, still: Still) -> Vec<Worst> {
    let context = Context {
        scale: scale(frames),
        holds,
        still,
    };
    let first = frames.first().map_or(0.0, |frame| frame.ms);
    Check::ALL
        .into_iter()
        .filter(|check| match check {
            Check::TwoHands => staff,
            Check::Stillness => still != Still::No,
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
                if let Some(value) = value(check, frames, i, context)
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

/// The value of `check` at frame `i`, where it can be measured.
fn value(check: Check, frames: &[Frame], i: usize, context: Context) -> Option<f32> {
    let Context {
        scale: s,
        holds,
        still,
    } = context;
    let frame = &frames[i];
    let before = i.checked_sub(1).map(|j| &frames[j]);
    match check {
        Check::Grip => largest(lit(frame).map(|blade| grip(blade, holds))),
        Check::TwoHands => two_hands(frame, s),
        Check::SelfCut => largest(lit(frame).map(|blade| {
            let depth = |side: Side| depth(side.blade(blade), &side.capsules(frame));
            added(depth(Side::Drawn), s * depth(Side::Reference))
        })),
        Check::Pops => pops([before?, frame, frames.get(i + 1)?], s),
        Check::Spin => spin(before?, frame),
        Check::Stillness => {
            let settled = before.filter(|before| before.ms >= frames[0].ms + SETTLE_MS);
            match still {
                Still::Chest => chest_stillness(settled?, frame),
                _ => stillness(settled?, frame, s),
            }
        }
        Check::ContactGap if frame.contact => contact_gap(frame),
        Check::ContactGap => None,
        Check::CutStartGap => (frame.cuts && !before?.cuts)
            .then(|| contact_gap(frame))
            .flatten(),
        Check::SwingStartTurn
        | Check::SwingStartTravel
        | Check::StanceEntryTurn
        | Check::StanceEntryTravel => {
            let before = before?;
            let starts = match check {
                Check::SwingStartTurn | Check::SwingStartTravel => swing_begins(before, frame),
                _ => frame.swapped && !before.swapped,
            };
            let (turn, travel) = window(frames, i).filter(|_| starts)?;
            Some(match check {
                Check::SwingStartTurn | Check::StanceEntryTurn => turn,
                _ => travel,
            })
        }
    }
}

/// How far the drawn blade is from the sim's, at the hilt or the tip, for the
/// worst of the frame's lit blades.
fn contact_gap(frame: &Frame) -> Option<f32> {
    largest(lit(frame).map(|blade| {
        let (drawn, sim) = (Side::Drawn.blade(blade), ends(blade.sim));
        drawn.0.distance(sim.0).max(drawn.1.distance(sim.1))
    }))
}

/// Whether `name` is a sequence of the kind `letter` names with a digit and
/// then `rest`, such as `BOTH_S2_S1_T_` for `'S'` and `"_S"`.
fn sequence_of(name: &str, letter: char, rest: &str) -> bool {
    name.strip_prefix("BOTH_")
        .and_then(|name| name.strip_prefix(letter))
        .and_then(|name| name.strip_prefix(|c: char| c.is_ascii_digit()))
        .is_some_and(|name| name.starts_with(rest))
}

/// Whether a swing begins at `frame`. The sim's torso then plays a start
/// (`BOTH_S<digit>_S…`) or an attack (`BOTH_A<digit>_…`), and in the frame
/// `before` it plays none of a start, an attack and a transition
/// (`BOTH_T<digit>_…`). A return, or the plain stand, is none of them.
fn swing_begins(before: &Frame, frame: &Frame) -> bool {
    let swinging = |name: &str| sequence_of(name, 'S', "_S") || sequence_of(name, 'A', "_");
    swinging(&frame.sim[1]) && !(swinging(&before.sim[1]) || sequence_of(&before.sim[1], 'T', "_"))
}

/// The turn (degrees) and the tip's travel (metres) of the drawn blade from
/// `frames[i]` to the last frame not later than `WINDOW_MS` after it. Each is
/// summed over the pairs of neighbouring frames, for each blade slot of the
/// first frame, and the largest sum is taken. A pair in which the slot is
/// missing or growing adds nothing. A frame without a lit blade gives no
/// window.
fn window(frames: &[Frame], i: usize) -> Option<(f32, f32)> {
    let end = frames[i].ms + WINDOW_MS;
    let inside = frames[i..].iter().take_while(|frame| frame.ms <= end);
    let pairs: Vec<_> = inside.clone().zip(inside.skip(1)).collect();
    let sums = lit(&frames[i]).map(|first| {
        pairs.iter().fold((0.0, 0.0), |(turn, travel), &(a, b)| {
            match (lit_in_slot(a, first.slot), lit_in_slot(b, first.slot)) {
                (Some(from), Some(to)) => (
                    turn + angle(Side::Drawn, from, to),
                    travel + (Side::Drawn.blade(to).1 - Side::Drawn.blade(from).1).length(),
                ),
                _ => (turn, travel),
            }
        })
    });
    sums.reduce(|(turn, travel), (t, v)| (turn.max(t), travel.max(v)))
}

/// Grip of one blade: how much farther the drawn hilt is from the drawn
/// hand gripping it than in the case's steadiest hold of the same blade
/// slot. The hand node sits at the wrist, so the distance is not 0 in a
/// good grip. A blade slot without a hold in `holds` is its own hold, so it
/// counts 0.
fn grip(blade: &Blade, holds: &Holds) -> f32 {
    let held = hold(blade);
    added(held, holds.0.get(&blade.slot).copied().unwrap_or(held))
}

/// The frame's blades that are not growing or shrinking.
fn lit(frame: &Frame) -> impl Iterator<Item = &Blade> {
    frame.blades.iter().filter(|blade| !blade.growing)
}

/// The frame's blade in `slot`, if it is not growing or shrinking.
fn lit_in_slot(frame: &Frame, slot: u8) -> Option<&Blade> {
    lit(frame).find(|blade| blade.slot == slot)
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
    largest(lit(b).filter_map(|to| {
        let from = lit(a).find(|from| from.slot == to.slot)?;
        Some(added(
            angle(Side::Drawn, from, to) / dt,
            angle(Side::Reference, from, to) / dt,
        ))
    }))
}

/// The angle (degrees) that the direction of a blade turns from `from` to
/// `to` on `side`.
fn angle(side: Side, from: &Blade, to: &Blade) -> f32 {
    let direction = |blade| {
        let (hilt, tip) = side.blade(blade);
        (tip - hilt).normalize_or_zero()
    };
    // `angle_between` divides by the lengths, so a zero direction gives
    // NaN. The angle from the cross and the dot products is 0 there.
    let (from, to) = (direction(from), direction(to));
    from.cross(to).length().atan2(from.dot(to)).to_degrees()
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

/// Stillness of a held block from frame `a` to frame `b`: for each blade lit
/// in both, how fast the drawn hand that grips it moves in the drawn chest's
/// own frame, in m/s. The chest's frame is the place and the turn of its
/// node (`TORSO`, `Frame::chest`). So a hand carried round by a turning
/// chest counts 0. A frame without the chest's place or turn gives no value.
fn chest_stillness(a: &Frame, b: &Frame) -> Option<f32> {
    let dt = step(a, b)?;
    let against_chest = |frame: &Frame, blade: &Blade| {
        let turn = Quat::from_array(frame.chest?).normalize();
        let out = Vec3::from_array(blade.hand) - Side::Drawn.node(frame, TORSO)?;
        Some(turn.inverse() * out)
    };
    largest(lit(b).filter_map(|to| {
        let from = lit(a).find(|from| from.slot == to.slot)?;
        Some((against_chest(b, to)? - against_chest(a, from)?).length() / dt)
    }))
}
