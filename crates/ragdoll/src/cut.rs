//! Cutting a skinned mesh in two at a joint, and closing the wounds. A
//! triangle goes with the side its skin leans to; each side is skinned
//! again so nothing stretches across the cut (the stump's weights on the
//! limb's bones move to the joint the cut is at, the limb's weights on the
//! body's bones to the limb's root). A wound is any open loop of edges at
//! the joint: one the cut left, or one the model always had there (a
//! sleeve's end, an armhole, where it meets another mesh). Each is closed
//! by a fan from its middle: its rim skinned as the mesh is there, its
//! middle wholly to that side's bone.

use std::collections::BTreeMap;

use crate::loops;

/// A skinned mesh: its vertices (mesh space), their skin (four bone slots
/// and weights each), and its triangles.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct SkinnedMesh {
    pub positions: Vec<[f32; 3]>,
    pub normals: Vec<[f32; 3]>,
    pub uvs: Vec<[f32; 2]>,
    pub joints: Vec<[u16; 4]>,
    pub weights: Vec<[f32; 4]>,
    pub indices: Vec<u32>,
}

/// Where a mesh is cut: which of its bone slots are the limb's, the slot
/// of the joint the limb hangs on (the stump's weights on the limb move
/// there), and the slot of the limb's root (the limb's weights on the body
/// move there).
pub struct Cut<'a> {
    pub limb: &'a dyn Fn(u16) -> bool,
    pub stump: u16,
    pub root: u16,
}

/// A wound to close: the joint (mesh space), how far from it a loop's
/// middle may be, and the bone slot the cap's middle hangs on.
pub struct Wound {
    pub at: [f32; 3],
    pub within: f32,
    pub slot: u16,
}

impl SkinnedMesh {
    /// How much of vertex `v`'s skin is the limb's (0 to 1).
    fn limb_share(&self, v: usize, limb: &dyn Fn(u16) -> bool) -> f32 {
        let (joints, weights) = (self.joints[v], self.weights[v]);
        let total: f32 = weights.iter().sum();
        let on_limb: f32 = (0..4)
            .filter(|&i| limb(joints[i]))
            .map(|i| weights[i])
            .sum();
        if total > 0.0 { on_limb / total } else { 0.0 }
    }

    /// The body's slot (not the limb's) the limb's skin blends into most:
    /// where the mesh hangs on the body at the cut, when no node above the
    /// cut has a slot of its own (a neck skinned beside the chest, not under
    /// it). None if no vertex of the limb blends into the body.
    pub fn blended(&self, limb: &dyn Fn(u16) -> bool) -> Option<u16> {
        let mut shares: BTreeMap<u16, f32> = BTreeMap::new();
        for (joints, weights) in self.joints.iter().zip(&self.weights) {
            let pairs = || joints.iter().zip(weights).filter(|&(_, &w)| w > 0.0);
            if pairs().any(|(&j, _)| limb(j)) {
                for (&j, &w) in pairs().filter(|&(&j, _)| !limb(j)) {
                    *shares.entry(j).or_default() += w;
                }
            }
        }
        shares
            .into_iter()
            .max_by(|a, b| a.1.total_cmp(&b.1))
            .map(|(slot, _)| slot)
    }

    /// Cuts the mesh: the part that stays, and the part that goes with the
    /// limb (either may be empty).
    pub fn cut(&self, cut: &Cut) -> (SkinnedMesh, SkinnedMesh) {
        let (triangles, _) = self.indices.as_chunks::<3>();
        let (mut stay, mut go) = (Vec::new(), Vec::new());
        for t in triangles {
            let share: f32 = t
                .iter()
                .map(|&v| self.limb_share(v as usize, cut.limb))
                .sum();
            if share > 1.5 { &mut go } else { &mut stay }.push(*t);
        }
        let stays = self.part(
            &stay,
            |slot| {
                if (cut.limb)(slot) { cut.stump } else { slot }
            },
        );
        let goes = self.part(&go, |slot| if (cut.limb)(slot) { slot } else { cut.root });
        (stays, goes)
    }

    /// `triangles` as a mesh of their own, skinned again by `rebind`.
    fn part(&self, triangles: &[[u32; 3]], rebind: impl Fn(u16) -> u16) -> SkinnedMesh {
        let mut mesh = SkinnedMesh::default();
        let mut index = BTreeMap::new();
        for t in triangles {
            for &v in t {
                let local = *index.entry(v).or_insert_with(|| {
                    let at = v as usize;
                    mesh.positions.push(self.positions[at]);
                    if let Some(&n) = self.normals.get(at) {
                        mesh.normals.push(n);
                    }
                    if let Some(&uv) = self.uvs.get(at) {
                        mesh.uvs.push(uv);
                    }
                    mesh.joints.push(self.joints[at].map(&rebind));
                    mesh.weights.push(self.weights[at]);
                    (mesh.positions.len() - 1) as u32
                });
                mesh.indices.push(local);
            }
        }
        mesh
    }

    /// The caps closing the mesh's wounds at `wound`: each open loop of
    /// edges (edges of one triangle only) whose middle lies within reach of
    /// the joint, fanned from its middle and turned to face out of the
    /// mesh. Vertex uv x is 0 at a cap's middle and 1 at its rim.
    pub fn caps(&self, wound: &Wound) -> SkinnedMesh {
        let mut cap = SkinnedMesh::default();
        for lap in loops::open_loops(&self.positions, &self.indices) {
            let middle = loops::middle(&self.positions, &lap);
            if loops::length(loops::sub(middle, wound.at)) > wound.within {
                continue;
            }
            self.fan(&mut cap, &lap, middle, wound.slot);
        }
        cap
    }

    /// One cap over a loop `lap` (as the mesh's triangles run its edges),
    /// fanned from `middle`.
    fn fan(&self, cap: &mut SkinnedMesh, lap: &[u32], middle: [f32; 3], slot: u16) {
        let (pairs, normal) = loops::fan(&self.positions, lap, middle);
        let mut push = |p: [f32; 3], uv: [f32; 2], joints: [u16; 4], weights: [f32; 4]| {
            cap.positions.push(p);
            cap.normals.push(normal);
            cap.uvs.push(uv);
            cap.joints.push(joints);
            cap.weights.push(weights);
            (cap.positions.len() - 1) as u32
        };
        let centre = push(middle, [0.0, 0.0], [slot, 0, 0, 0], [1.0, 0.0, 0.0, 0.0]);
        let rim: BTreeMap<u32, u32> = lap
            .iter()
            .map(|&v| {
                let at = v as usize;
                (
                    v,
                    push(
                        self.positions[at],
                        [1.0, 0.0],
                        self.joints[at],
                        self.weights[at],
                    ),
                )
            })
            .collect();
        for (a, b) in pairs {
            cap.indices.extend([rim[&a], rim[&b], centre]);
        }
    }
}
