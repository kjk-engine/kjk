//! Where a JA player's hands and saber blades are, as JA's server finds
//! them every frame from its own Ghoul2 skeleton to decide hits, blocks
//! and clashes (`docs/SKELETON-SIM.md`): the hilt hangs on the `*r_hand`
//! tag (a second saber on `*l_hand`), each blade starts at the hilt's
//! `*blade<n>` tag and runs along its negative Y for the blade's length.
//!
//! Only the bones on the hands' chains are evaluated, each as Ghoul2 does:
//! the part's two frames mixed matrix by matrix (not turned along an arc),
//! blended the same way into the pose the part blends from, times its
//! parent, and a look angle (`BONE_ANGLES_POSTMULT`) multiplied on. A tag's
//! bolt is built from its three vertices skinned by those bones. Floats go
//! through `+ - * /` and `sqrt` alone, and angles through `sim::math`, so
//! every platform places blades on the same bits.

mod data;
mod matrix;
mod playback;
mod turn;

pub use data::{
    Bone, LOOK_BONES, MAX_BONES, MOTION_BONE, Saber, SaberBlade, SkeletonData, SkeletonError,
    SkeletonParts, TORSO_BONE, Tag, TagVertex,
};
pub use matrix::Mat34;
pub use playback::{Blend, Frames, Playback, Sequence, Under};

use crate::math::{self, Vec3};
use matrix::{ROOT, decode, from_angles, matrix, multiply, transform};
use turn::{Local, mix_blend, mix_frames};

/// Pitch, yaw, roll in degrees.
pub type Angles = [f32; 3];

/// The look's angle overrides, one per `LOOK_BONES` entry, as the cgame
/// hands them to Ghoul2 (the caller computes them with `sim::math`).
pub type Look = [Angles; LOOK_BONES.len()];

/// What the skeleton plays: the legs' animation on the root, the torso's
/// on `TORSO_BONE` (and every bone under it), and the look, if set.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Pose {
    pub legs: Playback,
    pub torso: Playback,
    pub look: Option<Look>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Hand {
    Right = 0,
    Left = 1,
}

/// A blade of one of the data's sabers (`SkeletonData::sabers`), held in a
/// hand.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BladeId {
    pub hand: Hand,
    pub saber: usize,
    pub blade: usize,
}

/// A bolt: a hand's tag, or a blade's.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Bolt {
    Hand(Hand),
    Blade(BladeId),
}

/// An origin and three axes (X, Y, Z), units.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BoltMatrix {
    pub origin: Vec3,
    pub axes: [Vec3; 3],
}

impl BoltMatrix {
    fn of(m: &Mat34) -> BoltMatrix {
        BoltMatrix {
            origin: [m[0][3], m[1][3], m[2][3]],
            axes: std::array::from_fn(|c| [m[0][c], m[1][c], m[2][c]]),
        }
    }

    fn matrix(&self) -> Mat34 {
        std::array::from_fn(|r| {
            [
                self.axes[0][r],
                self.axes[1][r],
                self.axes[2][r],
                self.origin[r],
            ]
        })
    }

    /// Every value's bits, origin first: what a determinism check hashes.
    pub fn bits(&self) -> [u32; 12] {
        let mut bits = [0; 12];
        let values = self.origin.iter().chain(self.axes.iter().flatten());
        for (bit, value) in bits.iter_mut().zip(values) {
            *bit = value.to_bits();
        }
        bits
    }
}

/// A bolt in the model's space (Ghoul2's, the model facing +X), as the
/// skeleton holds it at `now_ms`; `None` for a saber or blade the data
/// lacks.
pub fn bolt(data: &SkeletonData, pose: &Pose, now_ms: i64, which: Bolt) -> Option<BoltMatrix> {
    let hand = match which {
        Bolt::Hand(hand) | Bolt::Blade(BladeId { hand, .. }) => hand,
    };
    let hand_bolt = tag_frame(&skin_hand(data, pose, now_ms, hand));
    match which {
        Bolt::Hand(_) => Some(BoltMatrix::of(&hand_bolt)),
        Bolt::Blade(id) => {
            let tag = &data.saber_blade(id)?.tag;
            // The hilt's one bone sits on the hand's bolt; its vertices are
            // summed from zero as any skinned vertex is.
            let points = tag.map(|p| transform(&hand_bolt, p).map(|v| 0.0 + v));
            Some(BoltMatrix::of(&tag_frame(&points)))
        }
    }
}

/// Where the model stands: its origin and the yaw it faces (the legs').
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Placement {
    pub origin: Vec3,
    pub yaw: f32,
}

/// Most a model is pushed ahead of its origin (units).
pub const MAX_PUSH: f32 = 70.0;

impl Placement {
    /// The server's placement for its blades (rulebook 5.1): the origin
    /// pushed ahead along the velocity by the sum of the velocity's
    /// absolute components times `push_per_speed` (JA: 1.6 over the server
    /// frame rate), at most `MAX_PUSH`.
    pub fn ahead(origin: Vec3, velocity: Vec3, push_per_speed: f32, yaw: f32) -> Placement {
        let mut direction = velocity;
        math::normalize(&mut direction);
        let speed = velocity[0].abs() + velocity[1].abs() + velocity[2].abs();
        let push = (speed * push_per_speed).clamp(-MAX_PUSH, MAX_PUSH);
        Placement {
            origin: math::mad(origin, push, direction),
            yaw,
        }
    }
}

/// A model-space bolt placed in the world as JA multiplayer's bolt query
/// returns it: the rotation's rows made unit, turned to the yaw and moved
/// to the origin, then the axes turned a quarter (X becomes −Y, Y becomes
/// X), so the old −X is the new −Y.
pub fn world(bolt: &BoltMatrix, placement: &Placement) -> BoltMatrix {
    let mut m = bolt.matrix();
    for row in &mut m {
        let mut axis = [row[0], row[1], row[2]];
        math::normalize(&mut axis);
        row[..3].copy_from_slice(&axis);
    }
    let mut place = from_angles([0.0, placement.yaw, 0.0]);
    for (row, origin) in place.iter_mut().zip(placement.origin) {
        row[3] = origin;
    }
    let mut m = multiply(&place, &m);
    for row in &mut m {
        let x = row[0];
        row[0] = -row[1];
        row[1] = x;
    }
    BoltMatrix::of(&m)
}

/// A blade's segment: where it leaves the hilt, which way it runs, and its
/// tip `length` along.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Blade {
    pub muzzle: Vec3,
    pub direction: Vec3,
    pub tip: Vec3,
}

impl Blade {
    /// From a world bolt (`world`): the origin, the negative Y axis.
    pub fn of(bolt: &BoltMatrix, length: f32) -> Blade {
        let direction = bolt.axes[1].map(|v| -v);
        Blade {
            muzzle: bolt.origin,
            direction,
            tip: math::mad(bolt.origin, length, direction),
        }
    }
}

/// A saber's blade in the world at `now_ms`, at its full length; `None`
/// for a saber or blade the data lacks.
pub fn blade(
    data: &SkeletonData,
    pose: &Pose,
    now_ms: i64,
    placement: &Placement,
    id: BladeId,
) -> Option<Blade> {
    let length = data.saber_blade(id)?.length;
    let local = bolt(data, pose, now_ms, Bolt::Blade(id))?;
    Some(Blade::of(&world(&local, placement), length))
}

/// What a part shows at a moment: its frames, and the blend under way with
/// where the part's animation stood when it began.
struct Shown {
    frames: Frames,
    blend: Option<(Blend, f32, Frames)>,
}

impl Shown {
    fn of(playback: &Playback, now_ms: i64) -> Shown {
        Shown {
            frames: playback.frames(now_ms),
            blend: playback
                .blending(now_ms)
                .map(|(blend, amount)| (blend, amount, playback.frames(blend.start_ms))),
        }
    }

    /// A bone's local transform (`turn`): the two frames mixed, then
    /// blended into from the pose the part blends from (a frame mixed
    /// toward the next by its fraction, or what the blend it began in
    /// showed).
    fn local(&self, data: &SkeletonData, bone: usize) -> Mat34 {
        let key = |frame: i32| decode(data.key(frame, bone));
        let shown = |frames: Frames| -> Local {
            let current = key(frames.current);
            if frames.lerp == 0.0 {
                current
            } else {
                mix_frames(current, key(frames.next), frames.lerp)
            }
        };
        let toward = |frame: f32, next: i32| {
            let current = frame as i32;
            shown(Frames {
                current,
                next,
                lerp: frame - current as f32,
            })
        };
        let mut local = shown(self.frames);
        if let Some((blend, amount, start)) = self.blend {
            let from = toward(blend.frame, blend.next);
            let from = match blend.under {
                Some(under) => mix_blend(
                    toward(under.frame, under.next),
                    from,
                    shown(under.start).0,
                    under.amount,
                ),
                None => from,
            };
            local = mix_blend(from, local, shown(start).0, amount);
        }
        matrix(&local)
    }
}

/// The `Motion` bone's bolt in the model's space at `now_ms`, as the look
/// reads it (`G2API_GetBoltMatrix_NoRecNoRot`): where the bone has its base
/// pose; `None` for a skeleton without the bone.
pub fn motion(data: &SkeletonData, pose: &Pose, now_ms: i64) -> Option<BoltMatrix> {
    let bone = usize::from(data.motion()?);
    let mut finals = [[[0.0f32; 4]; 3]; MAX_BONES];
    place_bones(data, pose, now_ms, &data.motion_chain, &mut finals);
    Some(BoltMatrix::of(&multiply(
        &finals[bone],
        &data.bones()[bone].base,
    )))
}

/// The bones of `chain` (parents first) where `pose` has them at `now_ms`,
/// into `finals`: each its parent's times its own, then its look turn.
fn place_bones(
    data: &SkeletonData,
    pose: &Pose,
    now_ms: i64,
    chain: &[u16],
    finals: &mut [Mat34; MAX_BONES],
) {
    let legs = Shown::of(&pose.legs, now_ms);
    let torso = Shown::of(&pose.torso, now_ms);
    for &bone in chain {
        let bone = usize::from(bone);
        let shown = if data.torso_driven[bone] {
            &torso
        } else {
            &legs
        };
        let local = shown.local(data, bone);
        let info = &data.bones()[bone];
        let parent = info.parent.map_or(&ROOT, |p| &finals[usize::from(p)]);
        let mut m = multiply(parent, &local);
        let look = pose.look.zip(data.look[bone]);
        if let Some((look, slot)) = look {
            let [pitch, yaw, roll] = look[usize::from(slot)];
            // Yaw about the bone's own X, pitch about its Y, roll about its
            // Z, in its base pose's frame.
            let turn = from_angles([pitch, roll, yaw]);
            let turn = multiply(&info.base, &multiply(&turn, &info.base_inverse));
            m = multiply(&m, &turn);
        }
        finals[bone] = m;
    }
}

/// The hand's tag vertices skinned by its chain at `now_ms`.
fn skin_hand(data: &SkeletonData, pose: &Pose, now_ms: i64, hand: Hand) -> [Vec3; 3] {
    let mut finals = [[[0.0f32; 4]; 3]; MAX_BONES];
    place_bones(data, pose, now_ms, data.chain(hand), &mut finals);
    data.hands()[hand as usize]
        .vertices
        .each_ref()
        .map(|vertex| {
            let mut point = [0.0f32; 3];
            let mut total = 0.0f32;
            let count = vertex.influences.len();
            for (k, &(bone, raw)) in vertex.influences.iter().enumerate() {
                let weight = if k + 1 == count {
                    1.0 - total
                } else {
                    let weight = f32::from(raw) * (1.0 / 1023.0);
                    total += weight;
                    weight
                };
                let placed = transform(&finals[usize::from(bone)], vertex.position);
                for (sum, v) in point.iter_mut().zip(placed) {
                    *sum += weight * v;
                }
            }
            point
        })
}

/// A tag's bolt from its three placed vertices: the long side (0 to 1) and
/// the short side (2 to 0) as unit axes, the long made square to the short,
/// their cross product; columns (short, long, −cross) at vertex 2.
fn tag_frame(p: &[Vec3; 3]) -> Mat34 {
    let sides: [Vec3; 3] = std::array::from_fn(|j| math::sub(p[(j + 1) % 3], p[j]));
    let unit = |v: Vec3| {
        let mut v = v;
        math::normalize(&mut v);
        v
    };
    let short = unit(sides[2]);
    let long = unit(sides[0]);
    let long = unit(math::mad(long, -math::dot(long, short), short));
    let normal = unit(math::cross(sides[0], sides[2]));
    std::array::from_fn(|r| [short[r], long[r], -normal[r], p[2][r]])
}
