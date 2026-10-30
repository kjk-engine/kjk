//! Ghoul2 playback on a JA player: which frame each bone shows at a
//! moment, how a new animation blends in over the old one, and the pose
//! that results, as multiplayer's client drives a player model.
//!
//! A player plays two parts. A set on the whole body lands on
//! `model_root`, the torso's on `lower_lumbar` (and the `Motion` bone with
//! it); every bone without an animation of its own follows its parent's,
//! so the legs and the arms move with the part above them. Speeds are in
//! frames per 50 ms, from the sequence's frame time (`playback`) and a
//! scale (a saber style's), negative for a sequence that runs backward.
//! Multiplayer blends each change: the bone holds the pose its old
//! animation stood at and eases into the new one over the blend.
//!
//! The game's faults are reproduced only with `Player::quirks`, each being
//! a visible jolt:
//! - a backward sequence starts a frame past its range, on the next
//!   sequence's frame (QUIRKS.md Q-0003), and its first wrap when looping
//!   starts a frame lower than the later ones (Q-0028);
//! - the pose a blend starts from weighs its two frames the wrong way round
//!   (Q-0004), and over a backward sequence is a whole frame (Q-0010);
//! - a blend set while another is still under way starts from the frame
//!   the bone's animation is on, dropping the pose on screen for a frame
//!   of the one barely begun (Q-0017); without quirks it starts from the
//!   pose shown;
//! - a legs restart while both parts play one animation picks up the
//!   torso's frame whenever the rate is not exactly a frame per 50 ms,
//!   while the torso starts over (Q-0029).
//!
//! Two frames, and a blend's two poses, are mixed along an arc rather than
//! by the game's lerped matrices, which shrink a bone and whip it round on
//! wide steps (see `mix_frames` and `mix_blend`), each joint kept on its
//! parent as the lerped matrix keeps it (`joint`).

use std::collections::BTreeMap;

use asset_odyssey::anim::{AnimationIr, NodeTrack};
use fmt_ja::animation_cfg::Sequence;
use fmt_ja::ghoul2::{Gla, unpack_bone};
use glam::{Mat3, Quat, Vec3};

use crate::UNIT;
use crate::angles::{self, Angles, Steer};

/// An animation on a bone of its own.
#[derive(Debug, Clone, Copy)]
struct BoneAnim {
    start_frame: i32,
    end_frame: i32,
    /// Frames per 50 ms; negative runs backward.
    speed: f32,
    start_ms: f32,
    looping: bool,
    /// The player's `quirks` when it was set: a backward loop's first wrap
    /// starts from the bound, as the reference's does.
    quirks: bool,
    blend: Option<Blend>,
}

#[derive(Debug, Clone, Copy)]
struct Blend {
    start_ms: f32,
    time_ms: f32,
    frame: f32,
    lerp_frame: f32,
    /// Starts from the pose shown when it began (`Player::shown`), not the
    /// frames.
    from_shown: bool,
    /// The pose it starts from turned so (`Player::adopt_blending_turned`).
    turned: Option<Quat>,
}

/// An animation timed elsewhere (`Player::adopt`): from `start_frame`
/// toward `end_frame` (one past the last frame shown that way) at `speed`
/// frames per 50 ms, from `start_ms`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Timed {
    pub start_frame: i32,
    pub end_frame: i32,
    pub speed: f32,
    pub start_ms: f32,
    pub looping: bool,
    pub blend: Option<BlendFrom>,
}

/// The pose a `Timed` animation blends from over `time_ms` from
/// `start_ms`: `frame` mixed toward `next` by its fraction, or, set while
/// another blend was under way, the pose that blend showed (`under`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BlendFrom {
    pub start_ms: f32,
    pub time_ms: f32,
    pub frame: f32,
    pub next: i32,
    pub under: Option<BlendUnder>,
}

/// The blend a `BlendFrom` began in: what it blended from (`frame` toward
/// `next`), where its animation stood when it began (current, next, how
/// far between), and how far through it was.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BlendUnder {
    pub frame: f32,
    pub next: i32,
    pub start: (i32, i32, f32),
    pub amount: f32,
}

/// A bone's timing and, mid-blend, its blend and how far through it is,
/// and the bone whose animation drives it.
type BoneState = (Timing, Option<(Blend, f32)>, usize);

/// Which frames a bone shows at a moment and how far between them.
#[derive(Debug, Clone, Copy)]
struct Timing {
    current: i32,
    new: i32,
    lerp: f32,
}

impl BoneAnim {
    /// The frames it shows at `now_ms` and how far between them.
    fn timing(&self, now_ms: f32) -> Timing {
        let forward = self.speed >= 0.0;
        let step = if forward { 1 } else { -1 };
        let last = self.end_frame - step;
        let hold = |frame: i32| Timing {
            current: frame,
            new: frame,
            lerp: 0.0,
        };
        if self.end_frame == self.start_frame {
            return hold(last.max(0));
        }
        let start = self.start_frame as f32;
        let bound = self.end_frame as f32;
        let place = start + ((now_ms - self.start_ms) / 50.0).max(0.0) * self.speed;
        let span = bound - start;
        // Back into the range by whole spans, kept on the far side of the
        // bound.
        let wrapped = |x: f32| bound + (x - bound) % span - span;
        let x = if forward {
            if self.looping && place >= bound {
                wrapped(place)
            } else if !self.looping && place > last as f32 {
                return hold(last);
            } else {
                place
            }
        } else {
            if self.quirks && self.looping && bound <= place && place < bound + 1.0 {
                // The first wrap, from the bound itself toward the start.
                return Timing {
                    current: self.end_frame,
                    new: self.start_frame,
                    lerp: (bound + 1.0) - place,
                };
            }
            if self.looping && place <= bound {
                wrapped(place)
            } else if !self.looping && place < last as f32 {
                return hold(last);
            } else {
                place
            }
        };
        let behind = if forward { x.trunc() } else { x.ceil() };
        let at_last = if forward {
            x >= last as f32
        } else {
            behind <= last as f32
        };
        let current = behind as i32;
        let new = if !at_last {
            current + step
        } else if self.looping {
            self.start_frame
        } else {
            last
        };
        Timing {
            current,
            new,
            lerp: (x - behind).abs(),
        }
    }

    /// The frame (with its fraction) it shows at `now_ms`.
    fn place(&self, now_ms: f32) -> f32 {
        let timing = self.timing(now_ms);
        timing.current as f32 + timing.lerp
    }
}

/// A bone's local transform (rotation, translation in metres).
pub type Local = (Quat, Vec3);

/// Two frames of one animation mixed as Ghoul2 lerps them: the joint at
/// `pivot` linearly (`joint`), rotation along the arc Ghoul2's lerped
/// matrix follows (the shorter one), at an even pace. Ghoul2's own matrix lerp rushes through
/// the middle of a wide step and shrinks the bone there; the dual and staff
/// sets have left-arm steps of up to 177° between frames, which that rush
/// turns into a whip. Both ends are fixed over the step, so the arc never
/// changes sides.
fn mix_frames(a: Local, b: Local, t: f32, pivot: Vec3) -> Local {
    let q = a.0.slerp(b.0, t).normalize();
    (q, joint(a, b, t, pivot, q))
}

/// The translation that puts the joint of a bone whose base place is
/// `pivot` where `a`'s and `b`'s, lerped, put it, once the bone is turned
/// by `q`. A key turns its bone about the model's origin, its translation
/// carrying the joint back (`pivot − turn · pivot` for a joint that stays
/// on its parent); a turn mixed apart from a translation lerped alone would
/// pull the joint off its parent by the chord between the two (the dual
/// set's left shoulder a metre from the chest, its upper arm 1.2 m long).
/// Ghoul2's lerped matrix keeps the joint in place.
fn joint(a: Local, b: Local, t: f32, pivot: Vec3, q: Quat) -> Vec3 {
    (a.1 + a.0 * pivot).lerp(b.1 + b.0 * pivot, t) - q * pivot
}

/// A blend from the captured pose `a` into the moving new animation `b`,
/// which stood at `start` when the blend began, along the arc Ghoul2's
/// lerped matrix turns on.
/// - The side is the one that was the shorter at the start, and stays: a
///   shortest arc chosen afresh would flip over as the new pose swings past
///   half a turn from the old one (the medium style's `BOTH_T2_BR_TR`
///   blend).
/// - The pace is the lerped matrix's own (its polar part), which points a
///   bone where Ghoul2's shrunken bone points, while the two are within a
///   quarter turn. Nearer half a turn that pace hangs back and then rushes
///   through the middle, where Ghoul2's bone has shrunk to nothing and ours
///   would whip at full length (the strong style's `BOTH_S3_S1_TL` swings
///   the left upper arm 160° in 10 ms), so it eases into an even pace by
///   half a turn.
fn mix_blend(a: Local, b: Local, start: Quat, t: f32, pivot: Vec3) -> Local {
    use std::f32::consts::{FRAC_PI_2, PI};
    let side = if a.0.dot(start) < 0.0 { -1.0 } else { 1.0 };
    let follow = if b.0.dot(start) < 0.0 { -b.0 } else { b.0 };
    let target = follow * side;
    let theta = 2.0 * a.0.dot(target).clamp(-1.0, 1.0).acos();
    let even = t * theta;
    let turned = if theta >= PI {
        even
    } else {
        let polar = (t * theta.sin()).atan2(1.0 - t + t * theta.cos());
        let w = ((theta - FRAC_PI_2) / FRAC_PI_2).clamp(0.0, 1.0).powi(2);
        (1.0 - w) * polar + w * even
    };
    let f = if theta > 1e-6 { turned / theta } else { t };
    let q = arc(a.0, target, f);
    (q, joint(a, b, t, pivot, q))
}

/// From `a` to `b` along their great circle at an even pace, the long way
/// round when their dot product is negative.
fn arc(a: Quat, b: Quat, t: f32) -> Quat {
    let d = a.dot(b).clamp(-1.0, 1.0);
    if d > 0.9999 {
        return a.lerp(b, t).normalize();
    }
    if d < -0.9999 {
        // A full turn: the same rotation at both ends.
        return a;
    }
    let theta = d.acos();
    let sin = theta.sin();
    (a * (((1.0 - t) * theta).sin() / sin) + b * ((t * theta).sin() / sin)).normalize()
}

/// A player model's animation state: what each bone that has an
/// animation of its own is playing.
pub struct Player<'a> {
    gla: &'a Gla,
    order: Vec<usize>,
    anims: BTreeMap<usize, BoneAnim>,
    /// Reproduce the game's playback faults, off by default because each
    /// makes a visible jolt:
    /// - backward sequences start one frame past their range (QUIRKS.md
    ///   Q-0003): that frame belongs to the next sequence, and running from
    ///   it into the sequence's own last frame can swing a bone half a turn
    ///   in one frame (the medium style's `BOTH_T2_BR_TR` whips the right
    ///   thigh 150° in 34 ms). Off, a backward sequence shows its own
    ///   frames, last to first;
    /// - the pose a blend starts from weighs its two frames the wrong way
    ///   round (Q-0004): caught exactly on a frame it takes the next one, a
    ///   whole frame ahead of where the body was; and a backward sequence's
    ///   is snapped to a whole frame (Q-0010). Off, it is the pose the bone
    ///   was showing;
    /// - a backward loop's first wrap starts a frame lower than its later
    ///   ones (Q-0028).
    ///
    /// Also Q-0017 (a change mid-blend pops, see the module docs).
    pub quirks: bool,
    /// The pose shown when a blend began mid-blend, by the bone that
    /// drives it.
    shown: BTreeMap<usize, Vec<Local>>,
}

impl<'a> Player<'a> {
    pub fn new(gla: &'a Gla) -> Player<'a> {
        let mut order = Vec::with_capacity(gla.bones.len());
        let mut placed = vec![false; gla.bones.len()];
        while order.len() < gla.bones.len() {
            let before = order.len();
            for (bone, info) in gla.bones.iter().enumerate() {
                let ready = usize::try_from(info.parent)
                    .map_or(true, |parent| parent >= placed.len() || placed[parent]);
                if !placed[bone] && ready {
                    placed[bone] = true;
                    order.push(bone);
                }
            }
            if order.len() == before {
                order.extend((0..placed.len()).filter(|&bone| !placed[bone]));
                break;
            }
        }
        Player {
            gla,
            order,
            anims: BTreeMap::new(),
            quirks: false,
            shown: BTreeMap::new(),
        }
    }

    pub fn bone(&self, name: &str) -> Option<usize> {
        self.gla
            .bones
            .iter()
            .position(|bone| bone.name.eq_ignore_ascii_case(name))
    }

    /// Plays `sequence` on `bone` from `now_ms`, blending over `blend_ms`
    /// from what it played, at `speed_scale` times its own rate.
    pub fn set(
        &mut self,
        bone: usize,
        sequence: &Sequence,
        now_ms: f32,
        blend_ms: f32,
        speed_scale: f32,
    ) {
        self.set_from(bone, sequence, now_ms, blend_ms, speed_scale, None);
    }

    /// `set`, started as if it had been running long enough to stand at
    /// frame `begin` now.
    fn set_from(
        &mut self,
        bone: usize,
        sequence: &Sequence,
        now_ms: f32,
        blend_ms: f32,
        speed_scale: f32,
        begin: Option<f32>,
    ) {
        let Some((_, frame_ms)) = crate::playback(sequence, self.gla.frame_count) else {
            return;
        };
        let signed_ms = if sequence.frames_per_second < 0 {
            -frame_ms
        } else {
            frame_ms
        };
        let speed = 50.0 / signed_ms * speed_scale;
        let (first, count) = (sequence.first_frame, sequence.frame_count);
        let (start_frame, end_frame) = if speed < 0.0 && self.quirks {
            (first + count, first)
        } else if speed < 0.0 {
            (first + count - 1, first - 1)
        } else {
            (first, first + count)
        };
        let blend = self.blend_from_now(bone, now_ms, blend_ms);
        let start_ms = match begin {
            Some(begin) if speed != 0.0 => {
                (now_ms - (begin - start_frame as f32) * 50.0 / speed).trunc()
            }
            _ => now_ms,
        };
        self.anims.insert(
            bone,
            BoneAnim {
                start_frame,
                end_frame,
                speed,
                start_ms,
                looping: sequence.loop_frame != -1,
                quirks: self.quirks,
                blend,
            },
        );
    }

    /// The blend a new animation on `bone` starts with at `now_ms`: from
    /// where its old animation stands (or, mid-blend and without quirks,
    /// from the pose on screen, which is recorded here).
    fn blend_from_now(&mut self, bone: usize, now_ms: f32, blend_ms: f32) -> Option<Blend> {
        let old = self.anims.get(&bone).copied();
        let mid_blend = !self.quirks
            && blend_ms > 0.0
            && old.and_then(|anim| anim.blend).is_some_and(|blend| {
                let into = now_ms - blend.start_ms;
                0.0 < into && into < blend.time_ms
            });
        if mid_blend {
            let shown = self.pose(now_ms);
            self.shown.insert(bone, shown);
        }
        if blend_ms.is_nan() || blend_ms <= 0.0 {
            return None;
        }
        let old = old?;
        if let Some(blend) = old.blend.filter(|blend| blend.start_ms == now_ms) {
            // Set again at the same moment: the same start, the new length.
            return Some(Blend {
                time_ms: blend_ms,
                ..blend
            });
        }
        let timing = old.timing(now_ms);
        let at = timing.current as f32 + timing.lerp;
        let past = |x: f32| {
            if x < old.end_frame as f32 {
                x
            } else if old.looping {
                old.start_frame as f32
            } else {
                old.end_frame as f32 - 1.0
            }
        };
        let (frame, lerp_frame) = if old.speed < 0.0 && self.quirks {
            (at.floor(), at.floor())
        } else if old.speed < 0.0 {
            (at, timing.new as f32)
        } else {
            (past(at), past(at + 1.0))
        };
        Some(Blend {
            start_ms: now_ms,
            time_ms: blend_ms,
            frame,
            lerp_frame,
            from_shown: mid_blend,
            turned: None,
        })
    }

    /// Where `other`'s own animation stands at `now_ms`, if `sequence`
    /// runs forward and that frame lies within it: the frame a set of
    /// `sequence` on the other part takes up.
    fn pick_up(&self, other: Option<usize>, sequence: &Sequence, now_ms: f32) -> Option<f32> {
        if sequence.frames_per_second < 0 {
            return None;
        }
        let at = self.anims.get(&other?)?.place(now_ms);
        let first = sequence.first_frame as f32;
        (first <= at && at <= first + sequence.frame_count as f32).then_some(at)
    }

    /// Whether the game would take a restart of `sequence` at
    /// `speed_scale` for a change of speed (Q-0029): any rate but one frame
    /// per 50 ms.
    fn restart_picks_up(&self, sequence: &Sequence, speed_scale: f32) -> bool {
        crate::playback(sequence, self.gla.frame_count)
            .is_some_and(|(_, frame_ms)| 50.0 / frame_ms * speed_scale != 1.0)
    }

    /// Plays an animation timed elsewhere on `bone` (a recorded one): its
    /// range, speed and start, and the blend it was in, whose starting pose
    /// is rebuilt when that blend began during another.
    pub fn adopt(&mut self, bone: usize, timed: &Timed) {
        if let Some(from) = timed.blend
            && let Some(under) = from.under
        {
            let under_pose = (0..self.gla.bones.len())
                .map(|b| {
                    let pivot = self.base(b).1;
                    let show = |current: i32, new: i32, lerp: f32| {
                        if lerp == 0.0 {
                            self.frame(b, current)
                        } else {
                            mix_frames(self.frame(b, current), self.frame(b, new), lerp, pivot)
                        }
                    };
                    let toward =
                        |at: f32, next: i32| show(at as i32, next, at - (at as i32) as f32);
                    let (current, new, lerp) = under.start;
                    mix_blend(
                        toward(under.frame, under.next),
                        toward(from.frame, from.next),
                        show(current, new, lerp).0,
                        under.amount,
                        pivot,
                    )
                })
                .collect();
            self.shown.insert(bone, under_pose);
        }
        let blend = timed.blend.map(|from| Blend {
            start_ms: from.start_ms,
            time_ms: from.time_ms,
            frame: from.frame,
            lerp_frame: from.next as f32,
            from_shown: from.under.is_some(),
            turned: None,
        });
        self.anims.insert(bone, self.timed_anim(timed, blend));
    }

    fn timed_anim(&self, timed: &Timed, blend: Option<Blend>) -> BoneAnim {
        BoneAnim {
            start_frame: timed.start_frame,
            end_frame: timed.end_frame,
            speed: timed.speed,
            start_ms: timed.start_ms,
            looping: timed.looping,
            quirks: self.quirks,
            blend,
        }
    }

    /// The frame (with its fraction) `bone`'s own animation stands at.
    pub fn frame_at(&self, bone: usize, now_ms: f32) -> Option<f32> {
        Some(self.anims.get(&bone)?.place(now_ms))
    }

    /// One animation set on a part, as multiplayer's client sets the legs
    /// and the torso: when both end up on one animation, a part that
    /// changes to it takes up the other's frame, so they stay in step.
    pub fn set_part(&mut self, set: &PartSet, bones: Bones) {
        let sequence = set.sequence;
        let name = sequence.name.as_str();
        let at = set.at_ms;
        let (legs_before, torso_before) = set.before;
        let (legs_after, torso_after) = set.after;
        let together = legs_after == torso_after;
        let scale = set.speed_scale;
        if matches!(set.part, Part::Both | Part::Legs) {
            let begin = if !together {
                None
            } else if legs_before != name || (self.quirks && self.restart_picks_up(sequence, scale))
            {
                self.pick_up(bones.torso, sequence, at)
            } else {
                None
            };
            let motion = bones.motion.filter(|_| torso_after == name);
            for bone in [bones.root, motion].into_iter().flatten() {
                self.set_from(bone, sequence, at, set.blends.0, scale, begin);
            }
        }
        if matches!(set.part, Part::Both | Part::Torso) {
            let begin = if together && torso_before != name {
                self.pick_up(bones.root, sequence, at)
            } else {
                None
            };
            for bone in [bones.torso, bones.motion].into_iter().flatten() {
                self.set_from(bone, sequence, at, set.blends.1, scale, begin);
            }
        }
    }

    /// `adopt` without the recorded blend: the animation timed elsewhere
    /// blends in from now over `blend_ms`, as a set would.
    pub fn adopt_blending(&mut self, bone: usize, timed: &Timed, now_ms: f32, blend_ms: f32) {
        let blend = self.blend_from_now(bone, now_ms, blend_ms);
        self.anims.insert(bone, self.timed_anim(timed, blend));
    }

    /// `adopt_blending` for a root (model space), the pose blended from
    /// turned by `turn` first: a caller that turns the whole result back by
    /// `turn`'s inverse blends a turned animation in from the pose shown
    /// without swinging the body round (`game::cast`'s get-ups).
    pub fn adopt_blending_turned(
        &mut self,
        bone: usize,
        timed: &Timed,
        now_ms: f32,
        blend_ms: f32,
        turn: Quat,
    ) {
        let blend = self
            .blend_from_now(bone, now_ms, blend_ms)
            .map(|blend| Blend {
                turned: Some(turn),
                ..blend
            });
        self.anims.insert(bone, self.timed_anim(timed, blend));
    }

    fn frame(&self, bone: usize, frame: i32) -> Local {
        let bones = self.gla.bones.len();
        let frame = usize::try_from(frame)
            .unwrap_or(0)
            .min(self.gla.frame_count.saturating_sub(1));
        let index = self.gla.frames[frame * bones + bone] as usize;
        let (q, t) = unpack_bone(&self.gla.pool[index]);
        (
            Quat::from_xyzw(q[0], q[1], q[2], q[3]).normalize(),
            Vec3::from_array(t) * UNIT,
        )
    }

    /// Every bone's local transform at `now_ms`. A bone with an animation
    /// of its own plays it (blending in from the pose it held while its
    /// blend lasts); the others follow their parent's animation on their
    /// own frames; a bone with neither stays at the identity.
    pub fn pose(&self, now_ms: f32) -> Vec<Local> {
        let count = self.gla.bones.len();
        let mut pose = vec![(Quat::IDENTITY, Vec3::ZERO); count];
        let mut states: Vec<Option<BoneState>> = vec![None; count];
        for &bone in &self.order {
            let state = match self.anims.get(&bone) {
                Some(anim) => {
                    let blend = anim.blend.and_then(|blend| {
                        let into = now_ms - blend.start_ms;
                        (0.0 <= into && into < blend.time_ms).then(|| (blend, into / blend.time_ms))
                    });
                    Some((anim.timing(now_ms), blend, bone))
                }
                None => usize::try_from(self.gla.bones[bone].parent)
                    .ok()
                    .filter(|&parent| parent < count)
                    .and_then(|parent| states[parent]),
            };
            let Some((timing, blend, driver)) = state else {
                continue;
            };
            states[bone] = state;
            let pivot = self.base(bone).1;
            let show = |timing: Timing| {
                let current = self.frame(bone, timing.current);
                if timing.lerp == 0.0 {
                    current
                } else {
                    mix_frames(current, self.frame(bone, timing.new), timing.lerp, pivot)
                }
            };
            let mut local = show(timing);
            if let Some((blend, amount)) = blend {
                let back = blend.frame - blend.frame.trunc();
                let held = self.frame(bone, blend.frame as i32);
                let next = self.frame(bone, blend.lerp_frame as i32);
                let recorded = blend
                    .from_shown
                    .then(|| self.shown.get(&driver).and_then(|shown| shown.get(bone)))
                    .flatten();
                let captured = match recorded {
                    Some(&shown) => shown,
                    // Q-0004: the fraction on the wrong frame.
                    None if self.quirks => mix_frames(next, held, back, pivot),
                    None => mix_frames(held, next, back, pivot),
                };
                let captured = match blend.turned {
                    Some(turn) => (turn * captured.0, turn * captured.1),
                    None => captured,
                };
                let reference = show(self.anims[&driver].timing(blend.start_ms)).0;
                local = mix_blend(captured, local, reference, amount, pivot);
            }
            pose[bone] = local;
        }
        pose
    }

    /// Every bone's final (model-space) transform for a pose.
    fn finals(&self, pose: &[Local]) -> Vec<Local> {
        let mut finals = vec![(Quat::IDENTITY, Vec3::ZERO); pose.len()];
        for &bone in &self.order {
            let (q, t) = pose[bone];
            let parent = usize::try_from(self.gla.bones[bone].parent)
                .ok()
                .filter(|&parent| parent < finals.len());
            finals[bone] = match parent {
                Some(parent) => {
                    let (pq, pt) = finals[parent];
                    (pq * q, pt + pq * t)
                }
                None => (q, t),
            };
        }
        finals
    }

    /// A bone's base pose: its rotation (the GLA's is scaled) and place.
    fn base(&self, bone: usize) -> Local {
        let m = &self.gla.bones[bone].base_pose;
        let column = |c: usize| Vec3::new(m[0][c], m[1][c], m[2][c]).normalize_or_zero();
        (
            Quat::from_mat3(&Mat3::from_cols(column(0), column(1), column(2))).normalize(),
            Vec3::new(m[0][3], m[1][3], m[2][3]) * UNIT,
        )
    }

    /// What takes a point of the model at its base pose to where `bone`
    /// has it in `pose` (model space, metres).
    pub fn bone_final(&self, pose: &[Local], bone: usize) -> Local {
        self.finals(pose)[bone]
    }

    /// Where a bone is in a pose, in model space (metres).
    pub fn bone_point(&self, pose: &[Local], bone: usize) -> Vec3 {
        let (q, t) = self.finals(pose)[bone];
        t + q * self.base(bone).1
    }

    /// A bone's direction in a pose as the game's angles (degrees, the
    /// game's axes): pitch and yaw of its forward axis (the bone's −Y), roll
    /// from its right axis (−X).
    pub fn bolt_angles(&self, pose: &[Local], bone: usize) -> Angles {
        let q = self.finals(pose)[bone].0 * self.base(bone).0;
        // Ours to the game's: our −Y forward is its +X.
        let game = |v: Vec3| [-v.y, v.x, v.z];
        let mut out = angles::look_angles(game(q * -Vec3::Y));
        out[2] = -(angles::look_angles(game(q * -Vec3::X))[0]);
        out
    }

    /// Turns `bone` in `pose` by angle overrides (pitch, yaw, roll in
    /// degrees, about the bone's own axes at its base pose: yaw about X,
    /// then pitch about Y, then roll about Z), pivoting on the bone's base
    /// place.
    pub fn turn(&self, pose: &mut [Local], bone: usize, angles: Angles) {
        let [pitch, yaw, roll] = angles.map(f32::to_radians);
        let m =
            Quat::from_rotation_z(roll) * Quat::from_rotation_y(pitch) * Quat::from_rotation_x(yaw);
        let (base, at) = self.base(bone);
        let t = (base * m * base.inverse()).normalize();
        let (q, tr) = pose[bone];
        pose[bone] = ((q * t).normalize(), tr + q * (at - t * at));
    }
}

/// Which bones a set plays on: `Both` the whole body
/// (`model_root`) and the torso (`lower_lumbar`, and the `Motion` bone with
/// it, as the game sets a torso animation); `Legs` the body alone; `Torso` the torso.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Part {
    Both,
    Legs,
    Torso,
}

/// The skeleton's part bones: the whole body's (`model_root`), the
/// torso's (`lower_lumbar`) and the motion bone (`Motion`).
#[derive(Debug, Clone, Copy)]
pub struct Bones {
    pub root: Option<usize>,
    pub torso: Option<usize>,
    pub motion: Option<usize>,
}

impl Bones {
    pub fn of(player: &Player) -> Bones {
        Bones {
            root: player.bone("model_root"),
            torso: player.bone("lower_lumbar"),
            motion: player.bone("Motion"),
        }
    }
}

/// One animation set on a part (`Player::set_part`).
#[derive(Debug, Clone, Copy)]
pub struct PartSet<'a> {
    pub part: Part,
    pub sequence: &'a Sequence,
    pub at_ms: f32,
    /// The legs' and the torso's blends (ms).
    pub blends: (f32, f32),
    pub speed_scale: f32,
    /// What the legs and the torso played before this set ("" for
    /// nothing), and play after it.
    pub before: (&'a str, &'a str),
    pub after: (&'a str, &'a str),
}

/// One animation set during a timeline.
#[derive(Debug, Clone)]
pub struct Cue<'s> {
    pub at_ms: f32,
    pub part: Part,
    pub sequence: &'s Sequence,
    pub blend_ms: f32,
    pub speed_scale: f32,
}

/// A timeline of cues sampled every `step_ms` for `length_ms` as a clip:
/// one track per GLA bone in bone order, local keys (as `crate::combo`).
pub fn bake(gla: &Gla, cues: &[Cue], length_ms: f32, step_ms: f32, name: &str) -> AnimationIr {
    bake_with(Player::new(gla), cues, length_ms, step_ms, name)
}

/// `bake` on a player set up by the caller (e.g. with `quirks` on).
pub fn bake_with(
    player: Player,
    cues: &[Cue],
    length_ms: f32,
    step_ms: f32,
    name: &str,
) -> AnimationIr {
    sample(player, cues, length_ms, step_ms, name, None).clip
}

/// How a baked player is steered: where it looks and how it moves each
/// moment (ms), and its weapon's ready animation (`angles::Moment::ready`).
pub struct Look<'a> {
    pub steer: &'a dyn Fn(f32) -> Steer,
    pub ready: &'a str,
    /// Reproduce the jolting reference quirks: the look's
    /// (`angles::Moment::quirks`), the player's (`Player::quirks`), and
    /// deaths set without a blend (`crate::blend_ms`, QUIRKS.md Q-0009).
    pub quirks: bool,
}

/// A steered bake: the clip, and each sample's angles (ms, pose), which
/// effects aim with.
pub struct Steered {
    pub clip: AnimationIr,
    pub angles: Vec<(f32, angles::Pose)>,
}

/// `bake`, the player turned each sample as the game's client turns it before
/// Ghoul2 builds the skeleton (`angles::player_angles`, one frame a
/// sample): the model to the legs' yaw about its origin, the spine and neck
/// by their angle overrides (`Player::turn`). The `Motion` bolt is read
/// before the overrides, which do not reach it.
pub fn bake_steered(
    gla: &Gla,
    cues: &[Cue],
    length_ms: f32,
    step_ms: f32,
    name: &str,
    look: &Look,
) -> Steered {
    let mut player = Player::new(gla);
    player.quirks = look.quirks;
    sample(player, cues, length_ms, step_ms, name, Some(look))
}

fn sample(
    mut player: Player,
    cues: &[Cue],
    length_ms: f32,
    step_ms: f32,
    name: &str,
    look: Option<&Look>,
) -> Steered {
    let gla = player.gla;
    let parts = Bones::of(&player);
    let (root, motion) = (parts.root, parts.motion);
    let mut cues: Vec<&Cue> = cues.iter().collect();
    cues.sort_by(|a, b| a.at_ms.total_cmp(&b.at_ms));
    let mut next = 0;
    let bones = gla.bones.len();
    let mut tracks: Vec<NodeTrack> = gla
        .bones
        .iter()
        .map(|bone| NodeTrack {
            node: bone.name.clone(),
            positions: Vec::new(),
            orientations: Vec::new(),
        })
        .collect();
    let mut state = look.map_or(angles::State::default(), |look| {
        angles::State::facing((look.steer)(0.0).view[1])
    });
    // What the legs and the torso play, for the angles.
    let (mut legs, mut body) = ("", "");
    let mut turns = Vec::new();
    let steps = (length_ms / step_ms).ceil().max(1.0) as usize;
    let mut last = 0.0;
    for step in 0..=steps {
        let now = (step as f32 * step_ms).min(length_ms);
        while next < cues.len() && cues[next].at_ms <= now {
            let cue = cues[next];
            let snaps = look.is_some_and(|look| look.quirks);
            // A part's blend, dropped into or out of a death with quirks.
            let blend = |old: &str| {
                let old = (!old.is_empty()).then_some(old);
                if snaps && crate::blend_ms(old, &cue.sequence.name, true) == 0.0 {
                    0.0
                } else {
                    cue.blend_ms
                }
            };
            let name = cue.sequence.name.as_str();
            let after = match cue.part {
                Part::Both => (name, name),
                Part::Legs => (name, body),
                Part::Torso => (legs, name),
            };
            player.set_part(
                &PartSet {
                    part: cue.part,
                    sequence: cue.sequence,
                    at_ms: cue.at_ms,
                    blends: (blend(legs), blend(body)),
                    speed_scale: cue.speed_scale,
                    before: (legs, body),
                    after,
                },
                parts,
            );
            (legs, body) = after;
            next += 1;
        }
        let mut pose = player.pose(now);
        if let Some(look) = look {
            let moment = angles::Moment {
                steer: (look.steer)(now),
                legs,
                torso: body,
                ready: look.ready,
                dead: false,
                quirks: look.quirks,
                motion: motion.map_or([0.0; 3], |bone| player.bolt_angles(&pose, bone)),
            };
            let turned = angles::player_angles(&mut state, &moment, now, now - last);
            for &(bone, angles) in &turned.bones {
                if let Some(bone) = player.bone(bone) {
                    player.turn(&mut pose, bone, angles);
                }
            }
            if let Some(root) = root {
                let yaw = Quat::from_rotation_z(turned.legs_yaw.to_radians());
                pose[root] = (yaw * pose[root].0, yaw * pose[root].1);
            }
            turns.push((now, turned));
        }
        last = now;
        let time = now / 1000.0;
        for (bone, (q, t)) in pose.into_iter().enumerate().take(bones) {
            tracks[bone].positions.push((time, t.to_array()));
            tracks[bone].orientations.push((time, [q.x, q.y, q.z, q.w]));
        }
    }
    Steered {
        clip: AnimationIr {
            name: name.to_ascii_lowercase(),
            length: length_ms / 1000.0,
            transition: 0.0,
            root: None,
            tracks,
            settings: Vec::new(),
            events: Vec::new(),
        },
        angles: turns,
    }
}
