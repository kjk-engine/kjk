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

fn sub(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    std::array::from_fn(|i| a[i] - b[i])
}

fn cross(u: [f32; 3], v: [f32; 3]) -> [f32; 3] {
    [
        u[1] * v[2] - u[2] * v[1],
        u[2] * v[0] - u[0] * v[2],
        u[0] * v[1] - u[1] * v[0],
    ]
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
        let (triangles, _) = self.indices.as_chunks::<3>();
        // Edges as the triangles run them; an edge whose reverse is not
        // there is open. Vertices at one place are one corner (meshes split
        // vertices along uv seams).
        let key = |v: u32| self.positions[v as usize].map(f32::to_bits);
        let mut first: BTreeMap<[u32; 3], u32> = BTreeMap::new();
        let corner_of =
            |v: u32, first: &mut BTreeMap<[u32; 3], u32>| *first.entry(key(v)).or_insert(v);
        let mut runs = BTreeMap::new();
        for t in triangles {
            let t = t.map(|v| corner_of(v, &mut first));
            for (a, b) in [(t[0], t[1]), (t[1], t[2]), (t[2], t[0])] {
                if a != b {
                    runs.insert((a, b), ());
                }
            }
        }
        // The open edges out of each corner: more than one where two loops
        // pinch through it.
        let mut next: BTreeMap<u32, Vec<u32>> = BTreeMap::new();
        for &(a, b) in runs.keys().filter(|&&(a, b)| !runs.contains_key(&(b, a))) {
            next.entry(a).or_default().push(b);
        }
        // One open edge out of `at`, taken.
        fn take(next: &mut BTreeMap<u32, Vec<u32>>, at: u32) -> Option<u32> {
            let outs = next.get_mut(&at)?;
            let to = outs.pop();
            if outs.is_empty() {
                next.remove(&at);
            }
            to
        }
        // Walk the edges; coming back to a corner closes the loop since it.
        // A walk that leads nowhere is dropped.
        let mut laps = Vec::new();
        while let Some(start) = next.keys().next().copied() {
            let mut lap = vec![start];
            while let Some(to) = take(&mut next, *lap.last().unwrap_or(&start)) {
                if let Some(back) = lap.iter().position(|&v| v == to) {
                    laps.push(lap.split_off(back));
                    if lap.is_empty() {
                        break;
                    }
                }
                lap.push(to);
            }
        }
        let mut cap = SkinnedMesh::default();
        for lap in laps.into_iter().filter(|lap| lap.len() >= 3) {
            let corner = |v: u32| self.positions[v as usize];
            let middle = lap
                .iter()
                .fold([0.0f32; 3], |sum, &v| {
                    std::array::from_fn(|i| sum[i] + corner(v)[i])
                })
                .map(|c| c / lap.len() as f32);
            let off = sub(middle, wound.at);
            if (off[0] * off[0] + off[1] * off[1] + off[2] * off[2]).sqrt() > wound.within {
                continue;
            }
            self.fan(&mut cap, &lap, middle, wound.slot);
        }
        cap
    }

    /// One cap over a loop `lap` (as the mesh's triangles run its edges),
    /// fanned from `middle`.
    fn fan(&self, cap: &mut SkinnedMesh, lap: &[u32], middle: [f32; 3], slot: u16) {
        let corner = |v: u32| self.positions[v as usize];
        // The mesh runs the loop's edges one way; the cap runs them back,
        // so it faces out of the hole.
        let pairs: Vec<(u32, u32)> = lap
            .iter()
            .zip(lap.iter().cycle().skip(1))
            .map(|(&a, &b)| (b, a))
            .collect();
        let facing = pairs.iter().fold([0.0f32; 3], |sum, &(a, b)| {
            let n = cross(sub(corner(a), middle), sub(corner(b), middle));
            std::array::from_fn(|i| sum[i] + n[i])
        });
        let length = facing.iter().map(|c| c * c).sum::<f32>().sqrt().max(1e-9);
        let normal = facing.map(|c| c / length);
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
                    push(corner(v), [1.0, 0.0], self.joints[at], self.weights[at]),
                )
            })
            .collect();
        for (a, b) in pairs {
            cap.indices.extend([rim[&a], rim[&b], centre]);
        }
    }
}
