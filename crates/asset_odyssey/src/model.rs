//! A model as the renderer needs it: a flat node list with world
//! transforms, and drawable meshes with their own vertex and index buffers.

use fmt_odyssey::mdl::{self, Model, Node};
use mdl::{
    CONTROLLER_ALPHA as ALPHA, CONTROLLER_LIGHT_COLOR as LIGHT_COLOR,
    CONTROLLER_LIGHT_MULTIPLIER as LIGHT_MULTIPLIER, CONTROLLER_LIGHT_RADIUS as LIGHT_RADIUS,
    CONTROLLER_SELF_ILLUM as SELF_ILLUM,
};

/// Row-major 4×4 matrix.
pub type Matrix = [[f32; 4]; 4];

pub const IDENTITY: Matrix = [
    [1.0, 0.0, 0.0, 0.0],
    [0.0, 1.0, 0.0, 0.0],
    [0.0, 0.0, 1.0, 0.0],
    [0.0, 0.0, 0.0, 1.0],
];

#[derive(Debug, Clone)]
pub struct ModelIr {
    pub name: String,
    pub supermodel: Option<String>,
    pub nodes: Vec<NodeIr>,
    pub meshes: Vec<MeshIr>,
    pub lights: Vec<LightIr>,
    pub emitters: Vec<crate::emitter::EmitterIr>,
    /// The model's own clips (its supermodels hold more).
    pub animations: Vec<crate::anim::AnimationIr>,
    /// Scales position keys when this model plays a clip.
    pub animation_scale: f32,
    /// The model header's fog flag: the area's fog draws over it.
    pub affected_by_fog: bool,
    /// Mesh nodes that were not turned into a drawable mesh, and why.
    pub refusals: Vec<(String, Refusal)>,
}

/// A light node in its rest pose (`mdl::CONTROLLER_LIGHT_*`).
#[derive(Debug, Clone)]
pub struct LightIr {
    pub node: usize,
    pub color: [f32; 3],
    pub radius: f32,
    pub multiplier: f32,
    pub ambient_only: bool,
    /// 1 lights everything, 2 only dynamic objects (as reone reads it).
    pub dynamic_type: u32,
    /// The light's header marks it as one that casts shadows (reone's
    /// shadow light is the nearest such light).
    pub shadow: bool,
}

/// A controller's first row, the rest pose (reone reads values at time 0).
fn first_row(node: &Node, kind: u32) -> Option<&[f32]> {
    let controller = node.controllers.iter().find(|c| c.kind == kind)?;
    let columns = usize::from(controller.columns);
    controller.values.get(..columns)
}

#[derive(Debug, Clone)]
pub struct NodeIr {
    pub name: String,
    pub parent: Option<usize>,
    /// Translation (x, y, z) and rotation (x, y, z, w) relative to the
    /// parent, in the rest pose.
    pub translation: [f32; 3],
    pub rotation: [f32; 4],
    pub world: Matrix,
    /// At or below the model's animation root: only these take clip keys.
    pub animated: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Refusal {
    /// The mesh is marked not to render (walkmeshes, shadow volumes).
    NotRendered,
    /// A lightsaber's blade (the node's saber flag), built from its own
    /// vertex data (not read), drawn with its texture (reone draws a saber
    /// mesh with its own); a hilt's flat planes share that texture. Its box
    /// in the node's space (min x, y, z, max), as its header has it.
    SaberBlade {
        texture: Option<String>,
        bounds: [f32; 6],
    },
    NoVertices,
    NoFaces,
    /// An AABB node: the room's walkmesh, which is collision, not drawn
    /// (reone skips these by node type).
    Walkmesh,
    /// A face names a vertex the mesh does not have.
    FaceOutsideVertices,
}

#[derive(Debug, Clone)]
pub struct MeshIr {
    pub node: usize,
    pub positions: Vec<[f32; 3]>,
    pub normals: Vec<[f32; 3]>,
    pub uv1: Vec<[f32; 2]>,
    pub uv2: Vec<[f32; 2]>,
    pub indices: Vec<u32>,
    /// Diffuse texture resref, lowercase; `None` when the model names none.
    pub texture: Option<String>,
    pub lightmap: Option<String>,
    pub transparency_hint: u32,
    pub skinned: bool,
    /// Rest-pose self-illumination, (0, 0, 0) when the node has none.
    pub self_illum: [f32; 3],
    /// Rest-pose alpha controller, 1 when absent.
    pub alpha: f32,
    /// The mesh header's ambient and diffuse colours.
    pub ambient: [f32; 3],
    pub diffuse: [f32; 3],
    /// `animateuv`: the scroll direction (reone reads only the direction).
    pub uv_scroll: Option<[f32; 2]>,
    pub skin: Option<SkinIr>,
}

/// A skinned mesh's bones. `joints[slot]` is the node driving bone slot
/// `slot`; `inverse_binds[slot]` takes the mesh's rest-pose vertices into
/// that bone's space. Per vertex, four slots and their weights.
#[derive(Debug, Clone)]
pub struct SkinIr {
    pub joints: Vec<usize>,
    /// From the file's `qbones`/`tbones` (reone: translate × rotate).
    pub inverse_binds: Vec<Matrix>,
    /// The same from the rest pose: bone⁻¹ × mesh. Kept to measure the two.
    pub rest_inverse_binds: Vec<Matrix>,
    pub vertex_joints: Vec<[u16; 4]>,
    pub vertex_weights: Vec<[f32; 4]>,
}

/// Rotation (w, x, y, z) as in the file; returned as (x, y, z, w).
fn quaternion(file: [f32; 4]) -> [f32; 4] {
    [file[1], file[2], file[3], file[0]]
}

/// The matrix of a place `t` and a turn `q` (x, y, z, w).
pub fn matrix(t: [f32; 3], q: [f32; 4]) -> Matrix {
    let [x, y, z, w] = q;
    [
        [
            1.0 - 2.0 * (y * y + z * z),
            2.0 * (x * y - z * w),
            2.0 * (x * z + y * w),
            t[0],
        ],
        [
            2.0 * (x * y + z * w),
            1.0 - 2.0 * (x * x + z * z),
            2.0 * (y * z - x * w),
            t[1],
        ],
        [
            2.0 * (x * z - y * w),
            2.0 * (y * z + x * w),
            1.0 - 2.0 * (x * x + y * y),
            t[2],
        ],
        [0.0, 0.0, 0.0, 1.0],
    ]
}

pub fn multiply(a: &Matrix, b: &Matrix) -> Matrix {
    let mut out = [[0.0; 4]; 4];
    for (r, row) in out.iter_mut().enumerate() {
        for (c, cell) in row.iter_mut().enumerate() {
            *cell = (0..4).map(|k| a[r][k] * b[k][c]).sum();
        }
    }
    out
}

/// The inverse of a rotation-and-translation matrix.
pub fn invert_rigid(m: &Matrix) -> Matrix {
    let mut out = IDENTITY;
    for r in 0..3 {
        for c in 0..3 {
            out[r][c] = m[c][r];
        }
    }
    for row in out.iter_mut().take(3) {
        row[3] = -(0..3).map(|k| row[k] * m[k][3]).sum::<f32>();
    }
    out
}

/// Builds a mesh's skin once every node's rest transform is known. A slot
/// is a `bone_map` value that is neither negative nor `0xffff`; the vertex
/// slots refer to those. A vertex slot that names no bone gets weight 0.
fn skin(nodes: &[NodeIr], mesh_node: usize, skin: &mdl::Skin) -> Option<SkinIr> {
    let mut joints: Vec<Option<usize>> = Vec::new();
    for (node, &slot) in skin.bone_map.iter().enumerate() {
        if slot < 0.0 || slot >= 65535.0 || node >= nodes.len() {
            continue;
        }
        let slot = slot as usize;
        if joints.len() <= slot {
            joints.resize(slot + 1, None);
        }
        joints[slot] = Some(node);
    }
    if joints.is_empty() {
        return None;
    }
    let fallback = mesh_node;
    let joints: Vec<usize> = joints.into_iter().map(|j| j.unwrap_or(fallback)).collect();
    let mesh_world = &nodes[mesh_node].world;
    let mut inverse_binds = Vec::with_capacity(joints.len());
    let mut rest_inverse_binds = Vec::with_capacity(joints.len());
    for &node in &joints {
        // reone: translate(tbone) × rotate(qbone), the bone's own entry by
        // node index.
        let file = match (skin.q_bones.get(node), skin.t_bones.get(node)) {
            (Some(q), Some(t)) => matrix(*t, quaternion(*q)),
            _ => IDENTITY,
        };
        inverse_binds.push(file);
        rest_inverse_binds.push(multiply(&invert_rigid(&nodes[node].world), mesh_world));
    }
    let count = joints.len();
    let (vertex_joints, vertex_weights) = skin
        .bones
        .iter()
        .zip(&skin.weights)
        .map(|(slots, weights)| {
            let mut j = [0u16; 4];
            let mut w = [0f32; 4];
            for k in 0..4 {
                let slot = slots[k];
                if slot >= 0.0 && (slot as usize) < count {
                    j[k] = slot as u16;
                    w[k] = weights[k];
                }
            }
            (j, w)
        })
        .unzip();
    Some(SkinIr {
        joints,
        inverse_binds,
        rest_inverse_binds,
        vertex_joints,
        vertex_weights,
    })
}

fn texture_name(name: &str) -> Option<String> {
    let name = name.trim().to_ascii_lowercase();
    (!name.is_empty() && name != "null").then_some(name)
}

impl ModelIr {
    /// The index of the node named `name`: names compare without case, as
    /// both references look nodes up.
    pub fn node_index(&self, name: &str) -> Option<usize> {
        self.nodes
            .iter()
            .position(|node| node.name.eq_ignore_ascii_case(name))
    }

    /// The node named `name` (`node_index`).
    pub fn node(&self, name: &str) -> Option<&NodeIr> {
        self.node_index(name).map(|index| &self.nodes[index])
    }

    /// The model's own clip named `name`, without case.
    pub fn animation(&self, name: &str) -> Option<&crate::anim::AnimationIr> {
        self.animations
            .iter()
            .find(|clip| clip.name.eq_ignore_ascii_case(name))
    }

    /// The textures of the model's lightsaber blades (`Refusal::SaberBlade`).
    pub fn saber_textures(&self) -> impl Iterator<Item = &str> {
        self.refusals
            .iter()
            .filter_map(|(_, refusal)| match refusal {
                Refusal::SaberBlade { texture, .. } => texture.as_deref(),
                _ => None,
            })
    }

    /// Whether node `index` is the node named `ancestor` or under it.
    pub fn under(&self, mut index: usize, ancestor: &str) -> bool {
        loop {
            let Some(node) = self.nodes.get(index) else {
                return false;
            };
            if node.name.eq_ignore_ascii_case(ancestor) {
                return true;
            }
            match node.parent {
                Some(parent) => index = parent,
                None => return false,
            }
        }
    }

    /// The box round every drawn mesh with each node at `worlds` (by node
    /// index, in the model's space; the nodes' own `world` is the rest
    /// pose), in the model's space (min x, y, z, max): a rigid mesh's
    /// vertices by its node's place, a skinned one's by its bones' as the
    /// renderer skins them. reone bounds a model for picking by its meshes
    /// in their nodes' places. `None` for a model that draws nothing.
    pub fn bounds(&self, worlds: &[Matrix]) -> Option<[f32; 6]> {
        let apply = |m: &Matrix, p: [f32; 3]| -> [f32; 3] {
            std::array::from_fn(|row| {
                m[row][0] * p[0] + m[row][1] * p[1] + m[row][2] * p[2] + m[row][3]
            })
        };
        let mut bounds: Option<[f32; 6]> = None;
        let mut take = |at: [f32; 3]| {
            let b = bounds.get_or_insert([at[0], at[1], at[2], at[0], at[1], at[2]]);
            for axis in 0..3 {
                b[axis] = b[axis].min(at[axis]);
                b[axis + 3] = b[axis + 3].max(at[axis]);
            }
        };
        for mesh in &self.meshes {
            match &mesh.skin {
                Some(skin) => {
                    let bones: Vec<Matrix> = (skin.joints.iter().zip(&skin.inverse_binds))
                        .map(|(&joint, bind)| multiply(&worlds[joint], bind))
                        .collect();
                    for (v, p) in mesh.positions.iter().enumerate() {
                        let mut at = [0.0; 3];
                        for (slot, weight) in
                            skin.vertex_joints[v].iter().zip(skin.vertex_weights[v])
                        {
                            let Some(bone) = bones.get(usize::from(*slot)).filter(|_| weight > 0.0)
                            else {
                                continue;
                            };
                            let moved = apply(bone, *p);
                            (0..3).for_each(|axis| at[axis] += weight * moved[axis]);
                        }
                        take(at);
                    }
                }
                None => mesh
                    .positions
                    .iter()
                    .for_each(|p| take(apply(&worlds[mesh.node], *p))),
            }
        }
        bounds
    }

    /// `bounds` in the rest pose.
    pub fn rest_bounds(&self) -> Option<[f32; 6]> {
        let rest: Vec<Matrix> = self.nodes.iter().map(|node| node.world).collect();
        self.bounds(&rest)
    }

    pub fn from_model(model: &Model) -> ModelIr {
        let mut ir = ModelIr {
            name: model.name.clone(),
            supermodel: model.supermodel.clone(),
            nodes: Vec::new(),
            meshes: Vec::new(),
            lights: Vec::new(),
            emitters: Vec::new(),
            animations: model
                .animations
                .iter()
                .map(|animation| crate::anim::AnimationIr::from_animation(animation, &model.name))
                .collect(),
            animation_scale: model.animation_scale,
            affected_by_fog: model.affected_by_fog,
            refusals: Vec::new(),
        };
        let mut skins = Vec::new();
        ir.add(&model.root, None, &IDENTITY, &mut skins);
        for (mesh, source) in skins {
            let node = ir.meshes[mesh].node;
            ir.meshes[mesh].skin = skin(&ir.nodes, node, source);
        }
        ir
    }

    fn add<'m>(
        &mut self,
        node: &'m Node,
        parent: Option<usize>,
        parent_world: &Matrix,
        skins: &mut Vec<(usize, &'m mdl::Skin)>,
    ) {
        let rotation = quaternion(node.orientation);
        let world = multiply(parent_world, &matrix(node.position, rotation));
        let index = self.nodes.len();
        self.nodes.push(NodeIr {
            name: node.name.clone(),
            parent,
            translation: node.position,
            rotation,
            world,
            animated: node.animated,
        });
        if let Some(light) = &node.light {
            let three = |kind| {
                first_row(node, kind)
                    .filter(|row| row.len() >= 3)
                    .map(|row| [row[0], row[1], row[2]])
            };
            let one = |kind| first_row(node, kind).and_then(|row| row.first().copied());
            self.lights.push(LightIr {
                node: index,
                color: three(LIGHT_COLOR).unwrap_or([1.0; 3]),
                radius: one(LIGHT_RADIUS).unwrap_or(0.0),
                multiplier: one(LIGHT_MULTIPLIER).unwrap_or(1.0),
                ambient_only: light.ambient_only,
                dynamic_type: light.dynamic_type,
                shadow: light.shadow,
            });
        }
        if let Some(emitter) = &node.emitter {
            self.emitters
                .push(crate::emitter::EmitterIr::from_node(index, node, emitter));
        }
        if let Some(mesh) = &node.mesh {
            match Self::mesh(index, node, mesh) {
                Ok(drawable) => {
                    if let Some(source) = &mesh.skin {
                        skins.push((self.meshes.len(), source));
                    }
                    self.meshes.push(drawable);
                }
                Err(refusal) => self.refusals.push((node.name.clone(), refusal)),
            }
        }
        for child in &node.children {
            self.add(child, Some(index), &world, skins);
        }
    }

    fn mesh(node: usize, source: &Node, mesh: &mdl::Mesh) -> Result<MeshIr, Refusal> {
        if mesh.saber {
            return Err(Refusal::SaberBlade {
                texture: texture_name(&mesh.textures[0]),
                bounds: mesh.bounds,
            });
        }
        if !mesh.render {
            return Err(Refusal::NotRendered);
        }
        if mesh.positions.is_empty() {
            return Err(Refusal::NoVertices);
        }
        if mesh.faces.is_empty() {
            return Err(Refusal::NoFaces);
        }
        if mesh.aabb.is_some() {
            return Err(Refusal::Walkmesh);
        }
        let vertices = mesh.positions.len();
        if mesh
            .faces
            .iter()
            .any(|face| face.vertices.iter().any(|&v| usize::from(v) >= vertices))
        {
            return Err(Refusal::FaceOutsideVertices);
        }
        // A mesh naming no texture is kept: a creature's body texture can
        // supply one (both references apply it to such meshes), and the
        // scene hides any mesh whose textures all fail to load.
        let (texture, lightmap) = (
            texture_name(&mesh.textures[0]),
            texture_name(&mesh.textures[1]),
        );
        Ok(MeshIr {
            node,
            positions: mesh.positions.clone(),
            normals: mesh.normals.clone(),
            uv1: mesh.uv1.clone(),
            uv2: mesh.uv2.clone(),
            indices: mesh
                .faces
                .iter()
                .flat_map(|face| face.vertices.map(u32::from))
                .collect(),
            texture,
            lightmap,
            transparency_hint: mesh.transparency_hint,
            skinned: mesh.skin.is_some(),
            skin: None,
            self_illum: first_row(source, SELF_ILLUM)
                .filter(|row| row.len() >= 3)
                .map_or([0.0; 3], |row| [row[0], row[1], row[2]]),
            alpha: first_row(source, ALPHA)
                .and_then(|row| row.first().copied())
                .unwrap_or(1.0),
            ambient: mesh.ambient,
            diffuse: mesh.diffuse,
            uv_scroll: mesh
                .animate_uv
                .map(|uv| [uv[0], uv[1]])
                .filter(|dir| dir[0] != 0.0 || dir[1] != 0.0),
        })
    }
}
