//! The skeleton's static data: the bones on the chains the hand bolts and
//! the `Motion` bone hang from, their frames as the GLA stores them, the
//! hand tags and the sabers' blade tags. Built by the host from the JA install, like
//! `motion::AnimTable`, and checked once here so evaluating it cannot fail.

use std::fmt;

use super::matrix::Mat34;
use crate::math::Vec3;

/// The bones the look turns (`angles::player_angles`), in `Look`'s order.
pub const LOOK_BONES: [&str; 5] = [
    "lower_lumbar",
    "upper_lumbar",
    "thoracic",
    "cervical",
    "cranium",
];

/// The bone the torso's animation is set on; the root carries the legs'.
pub const TORSO_BONE: &str = "lower_lumbar";

/// The bone that follows the torso's animation outside the torso, whose
/// turn the look takes off the spine (`combat::saber::look`).
pub const MOTION_BONE: &str = "Motion";

/// At most this many bones: a pose is evaluated on the stack.
pub const MAX_BONES: usize = 64;

/// A bone: its parent (an earlier bone), and its base pose and inverse
/// (the frame a look angle turns in).
#[derive(Debug, Clone, PartialEq)]
pub struct Bone {
    pub name: String,
    pub parent: Option<u16>,
    pub base: Mat34,
    pub base_inverse: Mat34,
}

/// A tag vertex in the model's space, skinned by up to four bones. Weights
/// are the file's 10-bit values (`w / 1023`); the last bone takes what the
/// others leave, whatever its own value.
#[derive(Debug, Clone, PartialEq)]
pub struct TagVertex {
    pub position: Vec3,
    pub influences: Vec<(u16, u16)>,
}

/// A tag surface: the first three vertices of its first triangle's
/// surface, which the bolt is built from.
#[derive(Debug, Clone, PartialEq)]
pub struct Tag {
    pub vertices: [TagVertex; 3],
}

/// One blade of a saber: its `*blade<n>` tag's three vertices in the hilt's
/// space (the hilt has the one bone of the default skeleton) and its length
/// (`lengthMax`, units).
#[derive(Debug, Clone, PartialEq)]
pub struct SaberBlade {
    pub tag: [Vec3; 3],
    pub length: f32,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Saber {
    pub name: String,
    pub blades: Vec<SaberBlade>,
}

/// Everything `SkeletonData::new` checks and keeps.
#[derive(Debug, Clone, PartialEq)]
pub struct SkeletonParts {
    /// Parents before children; bone 0 is the root (`model_root`), the
    /// only one without a parent.
    pub bones: Vec<Bone>,
    pub frame_count: u32,
    /// `keys[frame * bones + bone]`: an index into `pool`.
    pub keys: Vec<u32>,
    /// Compressed bones as the GLA stores them: quaternion w, x, y, z,
    /// then the translation.
    pub pool: Vec<[u16; 7]>,
    /// `*r_hand`, `*l_hand`.
    pub hands: [Tag; 2],
    pub sabers: Vec<Saber>,
    /// `MOTION_BONE`, if the skeleton has it: among `bones`, with its
    /// parents.
    pub motion: Option<u16>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SkeletonError {
    NoBones,
    TooManyBones(usize),
    Root,
    Parent { bone: usize },
    NoTorso,
    NoFrames,
    Keys { expected: usize, found: usize },
    Key { index: usize },
    Influences { hand: usize, vertex: usize },
    Motion,
    NotFinite(String),
}

impl fmt::Display for SkeletonError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SkeletonError::NoBones => write!(f, "skeleton: no bones"),
            SkeletonError::TooManyBones(n) => {
                write!(f, "skeleton: {n} bones, at most {MAX_BONES}")
            }
            SkeletonError::Root => write!(f, "skeleton: bone 0 must be the only root"),
            SkeletonError::Parent { bone } => {
                write!(f, "skeleton: bone {bone}'s parent is not an earlier bone")
            }
            SkeletonError::NoTorso => write!(f, "skeleton: no {TORSO_BONE}"),
            SkeletonError::NoFrames => write!(f, "skeleton: no frames"),
            SkeletonError::Keys { expected, found } => {
                write!(f, "skeleton: {found} keys, expected {expected}")
            }
            SkeletonError::Key { index } => write!(f, "skeleton: key {index} past the pool"),
            SkeletonError::Influences { hand, vertex } => write!(
                f,
                "skeleton: hand {hand} vertex {vertex} needs 1 to 4 influences on its bones"
            ),
            SkeletonError::Motion => write!(f, "skeleton: no {MOTION_BONE} at its index"),
            SkeletonError::NotFinite(what) => write!(f, "skeleton: {what} is not finite"),
        }
    }
}

impl std::error::Error for SkeletonError {}

/// The checked data, and what evaluating it needs: which bones the torso
/// drives, which carry a look angle, each hand's chain and the `Motion`
/// bone's.
#[derive(Debug, Clone, PartialEq)]
pub struct SkeletonData {
    parts: SkeletonParts,
    pub(super) torso_driven: Vec<bool>,
    /// Each bone's place in `Look`, if the look turns it.
    pub(super) look: Vec<Option<u8>>,
    /// Each hand's bones, root first, parents before children.
    pub(super) chains: [Vec<u16>; 2],
    /// The `Motion` bone's, the same way.
    pub(super) motion_chain: Vec<u16>,
}

impl SkeletonData {
    pub fn new(parts: SkeletonParts) -> Result<SkeletonData, SkeletonError> {
        let bones = &parts.bones;
        if bones.is_empty() {
            return Err(SkeletonError::NoBones);
        }
        if bones.len() > MAX_BONES {
            return Err(SkeletonError::TooManyBones(bones.len()));
        }
        for (index, bone) in bones.iter().enumerate() {
            match (index, bone.parent) {
                (0, None) => {}
                (0, Some(_)) | (_, None) => return Err(SkeletonError::Root),
                (_, Some(parent)) if usize::from(parent) >= index => {
                    return Err(SkeletonError::Parent { bone: index });
                }
                _ => {}
            }
            let finite = bone.base.iter().chain(&bone.base_inverse).flatten();
            if !finite.into_iter().all(|v| v.is_finite()) {
                return Err(SkeletonError::NotFinite(format!(
                    "{}'s base pose",
                    bone.name
                )));
            }
        }
        let torso = bones
            .iter()
            .position(|bone| bone.name.eq_ignore_ascii_case(TORSO_BONE))
            .ok_or(SkeletonError::NoTorso)?;
        if parts.frame_count == 0 {
            return Err(SkeletonError::NoFrames);
        }
        if parts.motion.is_some_and(|motion| {
            bones
                .get(usize::from(motion))
                .is_none_or(|bone| !bone.name.eq_ignore_ascii_case(MOTION_BONE))
        }) {
            return Err(SkeletonError::Motion);
        }
        let expected = parts.frame_count as usize * bones.len();
        if parts.keys.len() != expected {
            return Err(SkeletonError::Keys {
                expected,
                found: parts.keys.len(),
            });
        }
        if let Some(index) = parts
            .keys
            .iter()
            .position(|&key| key as usize >= parts.pool.len())
        {
            return Err(SkeletonError::Key { index });
        }
        for (hand, tag) in parts.hands.iter().enumerate() {
            for (vertex, v) in tag.vertices.iter().enumerate() {
                let bad = v.influences.is_empty()
                    || v.influences.len() > 4
                    || v.influences
                        .iter()
                        .any(|&(b, _)| usize::from(b) >= bones.len());
                if bad {
                    return Err(SkeletonError::Influences { hand, vertex });
                }
                if !v.position.iter().all(|p| p.is_finite()) {
                    return Err(SkeletonError::NotFinite(format!("hand {hand}'s tag")));
                }
            }
        }
        for saber in &parts.sabers {
            let values = saber
                .blades
                .iter()
                .flat_map(|blade| blade.tag.iter().flatten().chain([&blade.length]));
            if !values.into_iter().all(|v| v.is_finite()) {
                return Err(SkeletonError::NotFinite(format!("saber {}", saber.name)));
            }
        }

        let mut torso_driven = vec![false; bones.len()];
        for (index, bone) in bones.iter().enumerate() {
            torso_driven[index] = index == torso
                || parts.motion == Some(index as u16)
                || bone
                    .parent
                    .is_some_and(|parent| torso_driven[usize::from(parent)]);
        }
        // The bones from the root to each of `ends`, in order.
        let chain = |ends: &mut dyn Iterator<Item = u16>| -> Vec<u16> {
            let mut needed = vec![false; bones.len()];
            for end in ends {
                let mut at = Some(end);
                while let Some(bone) = at {
                    needed[usize::from(bone)] = true;
                    at = bones[usize::from(bone)].parent;
                }
            }
            (0..bones.len() as u16)
                .filter(|&bone| needed[usize::from(bone)])
                .collect()
        };
        let chains = [0, 1].map(|hand: usize| {
            chain(
                &mut parts.hands[hand]
                    .vertices
                    .iter()
                    .flat_map(|vertex| vertex.influences.iter().map(|&(bone, _)| bone)),
            )
        });
        let motion_chain = chain(&mut parts.motion.into_iter());
        let look = bones
            .iter()
            .map(|bone| {
                LOOK_BONES
                    .iter()
                    .position(|name| bone.name.eq_ignore_ascii_case(name))
                    .map(|slot| slot as u8)
            })
            .collect();
        Ok(SkeletonData {
            look,
            torso_driven,
            chains,
            motion_chain,
            parts,
        })
    }

    pub fn bones(&self) -> &[Bone] {
        &self.parts.bones
    }

    pub fn frame_count(&self) -> u32 {
        self.parts.frame_count
    }

    pub fn pool_len(&self) -> usize {
        self.parts.pool.len()
    }

    pub fn hands(&self) -> &[Tag; 2] {
        &self.parts.hands
    }

    pub fn sabers(&self) -> &[Saber] {
        &self.parts.sabers
    }

    pub fn saber_blade(&self, id: super::BladeId) -> Option<&SaberBlade> {
        self.parts.sabers.get(id.saber)?.blades.get(id.blade)
    }

    /// The bones a hand's bolt is evaluated from, root first.
    pub fn chain(&self, hand: super::Hand) -> &[u16] {
        &self.chains[hand as usize]
    }

    /// The `Motion` bone, if the skeleton has it.
    pub fn motion(&self) -> Option<u16> {
        self.parts.motion
    }

    /// A bone's compressed transform in a frame; frames outside the data
    /// read frame 0, as Ghoul2 does.
    pub(super) fn key(&self, frame: i32, bone: usize) -> &[u16; 7] {
        let frame = u32::try_from(frame)
            .ok()
            .filter(|&frame| frame < self.parts.frame_count)
            .unwrap_or(0) as usize;
        &self.parts.pool[self.parts.keys[frame * self.parts.bones.len() + bone] as usize]
    }

    /// BLAKE3 of everything, as bits: two machines with the same digest
    /// place blades the same.
    pub fn digest(&self) -> [u8; 32] {
        let mut h = collision::Digester::new();
        let floats = |h: &mut collision::Digester, values: &mut dyn Iterator<Item = &f32>| {
            for v in values {
                h.update(&v.to_bits().to_le_bytes());
            }
        };
        let text = |h: &mut collision::Digester, s: &str| {
            h.update(&(s.len() as u32).to_le_bytes());
            h.update(s.as_bytes());
        };
        h.update(b"KJK skeleton 2");
        let parts = &self.parts;
        h.update(&(parts.bones.len() as u32).to_le_bytes());
        for bone in &parts.bones {
            text(&mut h, &bone.name);
            h.update(&bone.parent.unwrap_or(u16::MAX).to_le_bytes());
            floats(
                &mut h,
                &mut bone.base.iter().chain(&bone.base_inverse).flatten(),
            );
        }
        h.update(&parts.frame_count.to_le_bytes());
        for key in &parts.keys {
            h.update(&key.to_le_bytes());
        }
        h.update(&(parts.pool.len() as u32).to_le_bytes());
        for entry in &parts.pool {
            for value in entry {
                h.update(&value.to_le_bytes());
            }
        }
        for tag in &parts.hands {
            for vertex in &tag.vertices {
                floats(&mut h, &mut vertex.position.iter());
                h.update(&(vertex.influences.len() as u32).to_le_bytes());
                for (bone, weight) in &vertex.influences {
                    h.update(&bone.to_le_bytes());
                    h.update(&weight.to_le_bytes());
                }
            }
        }
        h.update(&parts.motion.unwrap_or(u16::MAX).to_le_bytes());
        h.update(&(parts.sabers.len() as u32).to_le_bytes());
        for saber in &parts.sabers {
            text(&mut h, &saber.name);
            h.update(&(saber.blades.len() as u32).to_le_bytes());
            for blade in &saber.blades {
                floats(
                    &mut h,
                    &mut blade.tag.iter().flatten().chain([&blade.length]),
                );
            }
        }
        *h.finalize().as_bytes()
    }
}
