//! MDL/MDX: binary models. The MDL holds the node tree, headers, faces,
//! controllers and animations; the MDX holds interleaved per-vertex data.
//!
//! Offsets inside the MDL are relative to its data, which starts after a
//! 12-byte file header (0, MDL data size, MDX size). Layout, in order:
//! - geometry header (80 bytes): 2 words, name (32), root node offset, node
//!   count, 6 words, reference count, model type, 3 pad;
//! - model header (116 bytes): classification, subclassification, 1 byte,
//!   fog flag, child model count, animation offset array, supermodel
//!   reference, bounding box (6 floats), radius, animation scale, supermodel
//!   name (32), animation root offset, 1 word, MDX size, MDX offset, name
//!   offset array;
//! - node header (80 bytes): flags, number, name index, pad, root and parent
//!   offsets, position (3 floats), orientation (w, x, y, z), and arrays of
//!   child offsets, controller keys and controller data;
//! - then, by flag and in flag order, the light (92 bytes), emitter,
//!   reference, mesh (332 bytes; 340 in TSL), skin (100), animated mesh (56),
//!   dangly (28), AABB (4) and saber (20) headers.
//!
//! An "array" is (offset, count, count again).

use std::cell::Cell;

use crate::FormatError;

const FORMAT: &str = "MDL";
const DATA: usize = 12;
const NODE_HEADER: usize = 80;
const AABB_NODE: usize = 40;
/// How many times over the file's bytes its node and AABB headers may be
/// read. A tree reads each header once; a file whose nodes list a shared
/// child would otherwise read it once per path, which doubles per level.
const REVISITS: usize = 4;

pub const FLAG_HEADER: u16 = 0x0001;
pub const FLAG_LIGHT: u16 = 0x0002;
pub const FLAG_EMITTER: u16 = 0x0004;
pub const FLAG_CAMERA: u16 = 0x0008;
pub const FLAG_REFERENCE: u16 = 0x0010;
pub const FLAG_MESH: u16 = 0x0020;
pub const FLAG_SKIN: u16 = 0x0040;
pub const FLAG_ANIM: u16 = 0x0080;
pub const FLAG_DANGLY: u16 = 0x0100;
pub const FLAG_AABB: u16 = 0x0200;
pub const FLAG_SABER: u16 = 0x0800;
/// Flags neither reference knows; a node with any of them is refused.
const FLAGS_UNKNOWN: u16 = 0xf400;

/// Controller numbers (as reone numbers them). Any node's:
pub const CONTROLLER_POSITION: u32 = 8;
pub const CONTROLLER_ORIENTATION: u32 = 20;
pub const CONTROLLER_SCALE: u32 = 36;
/// A light's (they share numbers with an emitter's, by node type).
pub const CONTROLLER_LIGHT_COLOR: u32 = 76;
pub const CONTROLLER_LIGHT_RADIUS: u32 = 88;
pub const CONTROLLER_LIGHT_MULTIPLIER: u32 = 140;
/// A mesh's. xoreos names 128 as alpha; across the Endar Spire's rooms
/// every mesh node carries 132 (180 times, at 1.0) and 128 appears only on
/// other node types, so 132 it is.
pub const CONTROLLER_SELF_ILLUM: u32 = 100;
pub const CONTROLLER_ALPHA: u32 = 132;
const BEZIER: u8 = 16;

/// An emitter node's controller numbers (as reone numbers them).
pub mod emitter_controller {
    pub const ALPHA_END: u32 = 80;
    pub const ALPHA_START: u32 = 84;
    pub const BIRTHRATE: u32 = 88;
    pub const BOUNCE_CO: u32 = 92;
    pub const DRAG: u32 = 100;
    pub const FPS: u32 = 104;
    pub const FRAME_END: u32 = 108;
    pub const FRAME_START: u32 = 112;
    pub const GRAV: u32 = 116;
    pub const LIFE_EXP: u32 = 120;
    pub const MASS: u32 = 124;
    pub const P2P_BEZIER2: u32 = 128;
    pub const P2P_BEZIER3: u32 = 132;
    pub const PARTICLE_ROT: u32 = 136;
    pub const RAND_VEL: u32 = 140;
    pub const SIZE_START: u32 = 144;
    pub const SIZE_END: u32 = 148;
    pub const SIZE_START_Y: u32 = 152;
    pub const SIZE_END_Y: u32 = 156;
    pub const SPREAD: u32 = 160;
    pub const VELOCITY: u32 = 168;
    pub const X_SIZE: u32 = 172;
    pub const Y_SIZE: u32 = 176;
    pub const BLUR_LENGTH: u32 = 180;
    pub const LIGHTNING_DELAY: u32 = 184;
    pub const LIGHTNING_RADIUS: u32 = 188;
    pub const LIGHTNING_SCALE: u32 = 192;
    pub const LIGHTNING_SUBDIV: u32 = 196;
    pub const ALPHA_MID: u32 = 216;
    pub const PERCENT_MID: u32 = 224;
    pub const SIZE_MID: u32 = 232;
    pub const SIZE_MID_Y: u32 = 236;
    pub const RANDOM_BIRTHRATE: u32 = 240;
    pub const COLOR_MID: u32 = 284;
    pub const COLOR_END: u32 = 380;
    pub const COLOR_START: u32 = 392;
}

/// Which game wrote the model; TSL mesh headers are 8 bytes longer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Variant {
    Kotor1,
    Kotor2,
}

/// The geometry header's first word as TSL's writer leaves it (PC, Xbox):
/// the file's only mark of the game that wrote it. Any other word is K1's
/// layout, as reone reads it.
const TSL_MARKS: [u32; 2] = [4_285_200, 4_285_872];

impl Variant {
    /// The variant `mdl` is written in, from its geometry header.
    fn of(mdl: &[u8]) -> Variant {
        let mark = mdl
            .get(DATA..DATA + 4)
            .map(|word| u32::from_le_bytes([word[0], word[1], word[2], word[3]]));
        match mark {
            Some(mark) if TSL_MARKS.contains(&mark) => Variant::Kotor2,
            _ => Variant::Kotor1,
        }
    }
}

#[derive(Debug, Clone)]
pub struct Model {
    pub name: String,
    pub classification: u8,
    pub subclassification: u8,
    pub affected_by_fog: bool,
    /// `None` when the file says `null`.
    pub supermodel: Option<String>,
    pub bounding_box: [f32; 6],
    pub radius: f32,
    pub animation_scale: f32,
    pub root: Node,
    pub animations: Vec<Animation>,
}

#[derive(Debug, Clone)]
pub struct Animation {
    pub name: String,
    pub length: f32,
    pub transition: f32,
    pub root_name: String,
    pub events: Vec<(f32, String)>,
    pub root: Node,
}

/// An animation's name, length (s) and events (time in s, name), without
/// its nodes (`Model::timings`).
#[derive(Debug, Clone, PartialEq)]
pub struct Timing {
    pub name: String,
    pub length: f32,
    pub events: Vec<(f32, String)>,
}

#[derive(Debug, Clone)]
pub struct Node {
    pub flags: u16,
    /// The header's second word (reone: node number; xoreos: supernode).
    pub number: u16,
    pub name: String,
    pub position: [f32; 3],
    /// (w, x, y, z).
    pub orientation: [f32; 4],
    pub controllers: Vec<Controller>,
    pub light: Option<Light>,
    pub reference: Option<Reference>,
    pub emitter: Option<Emitter>,
    pub mesh: Option<Mesh>,
    pub children: Vec<Node>,
    /// The node is the model header's animation root or below it. Only
    /// such nodes take a clip's keys (as reone reads them). In a
    /// head model the root sits below its copy of the body's spine, so the
    /// spine stays at rest and only the face moves.
    pub animated: bool,
}

#[derive(Debug, Clone)]
pub struct Controller {
    pub kind: u32,
    /// Values per row, without the Bézier flag. For orientation, 2 means
    /// one packed quaternion per row and 4 means (x, y, z, w).
    pub columns: u8,
    pub bezier: bool,
    pub times: Vec<f32>,
    /// Row by row: `columns` values (three times as many with Bézier
    /// handles), or one packed word per row for packed orientations.
    pub values: Vec<f32>,
}

#[derive(Debug, Clone)]
pub struct Light {
    pub flare_radius: f32,
    pub priority: u32,
    pub ambient_only: bool,
    pub dynamic_type: u32,
    pub affect_dynamic: bool,
    pub shadow: bool,
    pub flare: bool,
    pub fading: bool,
}

#[derive(Debug, Clone)]
pub struct Reference {
    pub model: String,
    pub reattachable: bool,
}

/// An emitter's header (224 bytes, as reone and KotorBlender read it): the
/// particles' scalars (birth rate, colours, sizes...) are its controllers.
#[derive(Debug, Clone)]
pub struct Emitter {
    pub update: String,
    pub render: String,
    pub blend: String,
    pub texture: String,
    pub chunk: String,
    /// The texture's flipbook: columns and rows of frames.
    pub grid: [u32; 2],
    pub spawn_type: u32,
    pub two_sided: bool,
    /// The particles' lives start over (else an emitter gives one burst; a
    /// `Single` emitter's particle is born again when it dies).
    pub looped: bool,
    pub render_order: u16,
    /// p2p, p2p_sel, affected_by_wind, tinted, bounce, random, inherit,
    /// inherit_vel, inherit_local, splat, inherit_part, depth_texture
    /// (bit 0 up, as KotorBlender and reone number them: 0x40
    /// its particles go with it, 0x80 they take its velocity when born).
    pub flags: u32,
}

#[derive(Debug, Clone)]
pub struct Mesh {
    /// The mesh's own bounding box in node space: min (x, y, z), max.
    pub bounds: [f32; 6],
    /// Diffuse map, lightmap, and two more; empty or `null` when unused.
    pub textures: [String; 4],
    pub diffuse: [f32; 3],
    pub ambient: [f32; 3],
    pub transparency_hint: u32,
    pub render: bool,
    pub shadow: bool,
    pub lightmapped: bool,
    pub background: bool,
    pub beaming: bool,
    pub rotate_texture: bool,
    pub animate_uv: Option<[f32; 4]>,
    pub positions: Vec<[f32; 3]>,
    pub normals: Vec<[f32; 3]>,
    pub uv1: Vec<[f32; 2]>,
    pub uv2: Vec<[f32; 2]>,
    pub faces: Vec<Face>,
    /// The separate index array, when present; normally equal to the faces'
    /// vertex indices.
    pub indices: Vec<u16>,
    pub skin: Option<Skin>,
    pub dangly: Option<Dangly>,
    pub aabb: Option<AabbNode>,
    pub saber: bool,
}

#[derive(Debug, Clone, Copy)]
pub struct Face {
    pub normal: [f32; 3],
    pub distance: f32,
    /// Walk surface material (see `surfacemat.2da`).
    pub material: u32,
    pub adjacent: [u16; 3],
    pub vertices: [u16; 3],
}

#[derive(Debug, Clone)]
pub struct Skin {
    /// For each node of the model, in read order (depth first, a node
    /// before its children; how xoreos and reone
    /// index a skin's bones), its bone slot, or a negative/`0xffff`
    /// value when it is not a bone.
    pub bone_map: Vec<f32>,
    pub q_bones: Vec<[f32; 4]>,
    pub t_bones: Vec<[f32; 3]>,
    pub weights: Vec<[f32; 4]>,
    pub bones: Vec<[f32; 4]>,
}

#[derive(Debug, Clone)]
pub struct Dangly {
    pub displacement: f32,
    pub tightness: f32,
    pub period: f32,
    pub constraints: Vec<f32>,
}

#[derive(Debug, Clone)]
pub struct AabbNode {
    pub bounds: [f32; 6],
    /// `-1` for an inner node.
    pub face: i32,
    pub plane: u32,
    pub children: Option<Box<(AabbNode, AabbNode)>>,
}

struct Cursor<'a> {
    mdl: &'a [u8],
    mdx: &'a [u8],
    variant: Variant,
    names: Vec<String>,
    anim_root: u32,
    /// Header bytes still allowed to be read (`REVISITS`).
    budget: Cell<usize>,
}

fn truncated(needed: usize, len: usize) -> FormatError {
    FormatError::Truncated {
        format: FORMAT,
        needed,
        len,
    }
}

fn unsupported(what: String) -> FormatError {
    FormatError::Unsupported {
        format: FORMAT,
        what,
    }
}

fn text(raw: &[u8]) -> String {
    let end = raw.iter().position(|&b| b == 0).unwrap_or(raw.len());
    String::from_utf8_lossy(&raw[..end]).to_ascii_lowercase()
}

impl Cursor<'_> {
    fn bytes(&self, at: usize, len: usize) -> Result<&[u8], FormatError> {
        let start = DATA + at;
        self.mdl
            .get(start..start + len)
            .ok_or(truncated(start + len, self.mdl.len()))
    }

    fn u32(&self, at: usize) -> Result<u32, FormatError> {
        Ok(u32::from_le_bytes(
            self.bytes(at, 4)?.try_into().expect("4 bytes"),
        ))
    }

    fn u16(&self, at: usize) -> Result<u16, FormatError> {
        Ok(u16::from_le_bytes(
            self.bytes(at, 2)?.try_into().expect("2 bytes"),
        ))
    }

    fn f32(&self, at: usize) -> Result<f32, FormatError> {
        Ok(f32::from_bits(self.u32(at)?))
    }

    fn floats<const N: usize>(&self, at: usize) -> Result<[f32; N], FormatError> {
        let mut out = [0.0; N];
        for (i, value) in out.iter_mut().enumerate() {
            *value = self.f32(at + i * 4)?;
        }
        Ok(out)
    }

    fn text(&self, at: usize, len: usize) -> Result<String, FormatError> {
        Ok(text(self.bytes(at, len)?))
    }

    /// (offset, count) of an array definition.
    fn array(&self, at: usize) -> Result<(usize, usize), FormatError> {
        Ok((self.u32(at)? as usize, self.u32(at + 4)? as usize))
    }

    fn u32s(&self, (offset, count): (usize, usize)) -> Result<Vec<u32>, FormatError> {
        (0..count).map(|i| self.u32(offset + i * 4)).collect()
    }

    fn f32s(&self, offset: usize, count: usize) -> Result<Vec<f32>, FormatError> {
        (0..count).map(|i| self.f32(offset + i * 4)).collect()
    }

    /// A count read from the file, capped at how many `each`-byte records
    /// the file has room for: a capacity to reserve, not a promise.
    fn room(&self, count: usize, each: usize) -> usize {
        count.min(self.mdl.len() / each)
    }

    /// Takes `len` header bytes from the budget.
    fn spend(&self, len: usize) -> Result<(), FormatError> {
        let left = self.budget.get().checked_sub(len).ok_or_else(|| {
            unsupported(format!(
                "nodes read more than {REVISITS} times the file (shared subtrees?)"
            ))
        })?;
        self.budget.set(left);
        Ok(())
    }

    fn node(&self, at: usize, depth: usize, inherited: bool) -> Result<Node, FormatError> {
        let animated = inherited || at as u32 == self.anim_root;
        if depth > 128 {
            return Err(unsupported("node tree deeper than 128 (a cycle?)".into()));
        }
        self.spend(NODE_HEADER)?;
        let flags = self.u16(at)?;
        if flags & FLAGS_UNKNOWN != 0 {
            return Err(unsupported(format!("node flags {flags:#06x}")));
        }
        let number = self.u16(at + 2)?;
        let name_index = usize::from(self.u16(at + 4)?);
        let name = self.names.get(name_index).cloned().ok_or_else(|| {
            unsupported(format!("name index {name_index} of {}", self.names.len()))
        })?;
        let position = self.floats::<3>(at + 16)?;
        let orientation = self.floats::<4>(at + 28)?;
        let children_at = self.array(at + 44)?;
        let keys_at = self.array(at + 56)?;
        let (data_offset, data_count) = self.array(at + 68)?;
        let data = self.f32s(data_offset, data_count)?;

        let mut controllers = Vec::with_capacity(self.room(keys_at.1, 16));
        for k in 0..keys_at.1 {
            let key = keys_at.0 + k * 16;
            let kind = self.u32(key)?;
            let rows = usize::from(self.u16(key + 6)?);
            let time = usize::from(self.u16(key + 8)?);
            let value = usize::from(self.u16(key + 10)?);
            let raw_columns = self.bytes(key + 12, 1)?[0];
            let bezier = raw_columns & BEZIER != 0;
            let columns = raw_columns & !BEZIER;
            let per_row = if kind == CONTROLLER_ORIENTATION && columns == 2 {
                1
            } else {
                usize::from(columns) * if bezier { 3 } else { 1 }
            };
            let times = data
                .get(time..time + rows)
                .ok_or_else(|| unsupported(format!("controller {kind} times outside its data")))?
                .to_vec();
            let values = data
                .get(value..value + rows * per_row)
                .ok_or_else(|| unsupported(format!("controller {kind} values outside its data")))?
                .to_vec();
            controllers.push(Controller {
                kind,
                columns,
                bezier,
                times,
                values,
            });
        }

        let mut sub = at + NODE_HEADER;
        let mut light = None;
        let mut emitter = None;
        let mut reference = None;
        let mut mesh = None;
        if flags & FLAG_LIGHT != 0 {
            light = Some(Light {
                flare_radius: self.f32(sub)?,
                priority: self.u32(sub + 64)?,
                ambient_only: self.u32(sub + 68)? != 0,
                dynamic_type: self.u32(sub + 72)?,
                affect_dynamic: self.u32(sub + 76)? != 0,
                shadow: self.u32(sub + 80)? != 0,
                flare: self.u32(sub + 84)? != 0,
                fading: self.u32(sub + 88)? != 0,
            });
            sub += 92;
        }
        if flags & FLAG_EMITTER != 0 {
            emitter = Some(Emitter {
                update: self.text(sub + 32, 32)?,
                render: self.text(sub + 64, 32)?,
                blend: self.text(sub + 96, 32)?,
                texture: self.text(sub + 128, 32)?,
                chunk: self.text(sub + 160, 16)?,
                grid: [self.u32(sub + 20)?, self.u32(sub + 24)?],
                spawn_type: self.u32(sub + 28)?,
                two_sided: self.u32(sub + 176)? != 0,
                looped: self.u32(sub + 180)? != 0,
                render_order: self.u16(sub + 184)?,
                flags: self.u32(sub + 220)?,
            });
            sub += 224;
        }
        if flags & FLAG_REFERENCE != 0 {
            reference = Some(Reference {
                model: self.text(sub, 32)?,
                reattachable: self.u32(sub + 32)? != 0,
            });
            sub += 36;
        }
        if flags & FLAG_MESH != 0 {
            mesh = Some(self.mesh(sub, flags)?);
        }

        let child_offsets = self.u32s(children_at)?;
        let children = child_offsets
            .into_iter()
            .map(|child| self.node(child as usize, depth + 1, animated))
            .collect::<Result<_, _>>()?;
        Ok(Node {
            flags,
            number,
            name,
            position,
            orientation,
            controllers,
            light,
            reference,
            emitter,
            mesh,
            children,
            animated,
        })
    }

    fn mesh(&self, at: usize, flags: u16) -> Result<Mesh, FormatError> {
        let faces_at = self.array(at + 8)?;
        let bounds = self.floats::<6>(at + 20)?;
        let diffuse = self.floats::<3>(at + 60)?;
        let ambient = self.floats::<3>(at + 72)?;
        let transparency_hint = self.u32(at + 84)?;
        let textures = [
            self.text(at + 88, 32)?,
            self.text(at + 120, 32)?,
            self.text(at + 152, 12)?,
            self.text(at + 164, 12)?,
        ];
        let index_offsets = self.array(at + 188)?;
        let animate = self.u32(at + 232)? != 0;
        let uv = self.floats::<4>(at + 236)?;
        let stride = self.u32(at + 252)? as usize;
        let offset = |i: usize| -> Result<Option<usize>, FormatError> {
            let value = self.u32(at + 260 + i * 4)?;
            Ok((value != u32::MAX).then_some(value as usize))
        };
        let (off_position, off_normal, off_uv1, off_uv2) =
            (offset(0)?, offset(1)?, offset(3)?, offset(4)?);
        let vertex_count = usize::from(self.u16(at + 304)?);
        let bytes = self.bytes(at + 308, 6)?;
        let (lightmapped, rotate_texture, background, shadow, beaming, render) = (
            bytes[0] != 0,
            bytes[1] != 0,
            bytes[2] != 0,
            bytes[3] != 0,
            bytes[4] != 0,
            bytes[5] != 0,
        );
        let tail = at
            + 324
            + if self.variant == Variant::Kotor2 {
                8
            } else {
                0
            };
        let mdx_start = self.u32(tail)? as usize;
        let mdl_vertices = self.u32(tail + 4)? as usize;
        let mut sub = tail + 8;

        let mut skin = None;
        let mut dangly = None;
        let mut aabb = None;
        let mut skin_offsets = None;
        if flags & FLAG_SKIN != 0 {
            let weights = self.u32(sub + 12)? as usize;
            let bone_indices = self.u32(sub + 16)? as usize;
            let bone_map_at = self.u32(sub + 20)? as usize;
            let bone_count = self.u32(sub + 24)? as usize;
            let q_at = self.u32(sub + 28)? as usize;
            let t_at = self.u32(sub + 40)? as usize;
            let bone_map = self.f32s(bone_map_at, bone_count)?;
            let q_bones = (0..bone_count)
                .map(|i| self.floats::<4>(q_at + i * 16))
                .collect::<Result<_, _>>()?;
            let t_bones = (0..bone_count)
                .map(|i| self.floats::<3>(t_at + i * 12))
                .collect::<Result<_, _>>()?;
            skin_offsets = Some((weights, bone_indices));
            skin = Some(Skin {
                bone_map,
                q_bones,
                t_bones,
                weights: Vec::new(),
                bones: Vec::new(),
            });
            sub += 100;
        }
        if flags & FLAG_ANIM != 0 {
            sub += 56;
        }
        if flags & FLAG_DANGLY != 0 {
            let constraints = self.array(sub)?;
            dangly = Some(Dangly {
                displacement: self.f32(sub + 12)?,
                tightness: self.f32(sub + 16)?,
                period: self.f32(sub + 20)?,
                constraints: self.f32s(constraints.0, constraints.1)?,
            });
            sub += 28;
        }
        if flags & FLAG_AABB != 0 {
            aabb = Some(self.aabb(self.u32(sub)? as usize, 0)?);
        }
        let saber = flags & FLAG_SABER != 0;

        let mut mesh = Mesh {
            bounds,
            textures,
            diffuse,
            ambient,
            transparency_hint,
            render,
            shadow,
            lightmapped,
            background,
            beaming,
            rotate_texture,
            animate_uv: animate.then_some(uv),
            positions: Vec::new(),
            normals: Vec::new(),
            uv1: Vec::new(),
            uv2: Vec::new(),
            faces: Vec::new(),
            indices: Vec::new(),
            skin,
            dangly,
            aabb,
            saber,
        };

        if !saber {
            if stride > 0 && vertex_count > 0 {
                let vertex = |i: usize, off: usize, n: usize| -> Result<Vec<f32>, FormatError> {
                    let start = mdx_start + i * stride + off;
                    (0..n)
                        .map(|k| {
                            let at = start + k * 4;
                            self.mdx
                                .get(at..at + 4)
                                .map(|b| f32::from_le_bytes(b.try_into().expect("4 bytes")))
                                .ok_or(truncated(at + 4, self.mdx.len()))
                        })
                        .collect()
                };
                for i in 0..vertex_count {
                    if let Some(off) = off_position {
                        let v = vertex(i, off, 3)?;
                        mesh.positions.push([v[0], v[1], v[2]]);
                    }
                    if let Some(off) = off_normal {
                        let v = vertex(i, off, 3)?;
                        mesh.normals.push([v[0], v[1], v[2]]);
                    }
                    if let Some(off) = off_uv1 {
                        let v = vertex(i, off, 2)?;
                        mesh.uv1.push([v[0], v[1]]);
                    }
                    if let Some(off) = off_uv2 {
                        let v = vertex(i, off, 2)?;
                        mesh.uv2.push([v[0], v[1]]);
                    }
                    if let (Some(skin), Some((weights, bones))) = (mesh.skin.as_mut(), skin_offsets)
                    {
                        let w = vertex(i, weights, 4)?;
                        let b = vertex(i, bones, 4)?;
                        skin.weights.push([w[0], w[1], w[2], w[3]]);
                        skin.bones.push([b[0], b[1], b[2], b[3]]);
                    }
                }
            } else if vertex_count > 0 {
                // No MDX data: positions only, from the MDL.
                for i in 0..vertex_count {
                    mesh.positions
                        .push(self.floats::<3>(mdl_vertices + i * 12)?);
                }
            }
            for f in 0..faces_at.1 {
                let face = faces_at.0 + f * 32;
                mesh.faces.push(Face {
                    normal: self.floats::<3>(face)?,
                    distance: self.f32(face + 12)?,
                    material: self.u32(face + 16)?,
                    adjacent: [
                        self.u16(face + 20)?,
                        self.u16(face + 22)?,
                        self.u16(face + 24)?,
                    ],
                    vertices: [
                        self.u16(face + 26)?,
                        self.u16(face + 28)?,
                        self.u16(face + 30)?,
                    ],
                });
            }
            if index_offsets.1 > 0 && faces_at.1 > 0 {
                let indices_at = self.u32(index_offsets.0)? as usize;
                mesh.indices = (0..faces_at.1 * 3)
                    .map(|i| self.u16(indices_at + i * 2))
                    .collect::<Result<_, _>>()?;
            }
        }
        Ok(mesh)
    }

    /// The header's name table: each node's name, by number.
    fn name_table(&self) -> Result<Vec<String>, FormatError> {
        let names_at = self.array(80 + 104)?;
        self.u32s(names_at)?
            .into_iter()
            .map(|offset| {
                let start = DATA + offset as usize;
                let raw = self
                    .mdl
                    .get(start..)
                    .ok_or(truncated(start, self.mdl.len()))?;
                Ok(text(&raw[..raw.len().min(64)]))
            })
            .collect()
    }

    /// The animation header at `at`: its name, length and events.
    fn timing(&self, at: usize) -> Result<Timing, FormatError> {
        let events_at = self.array(at + 80 + 40)?;
        let events = (0..events_at.1)
            .map(|i| {
                let event = events_at.0 + i * 36;
                Ok((self.f32(event)?, self.text(event + 4, 32)?))
            })
            .collect::<Result<_, FormatError>>()?;
        Ok(Timing {
            name: self.text(at + 8, 32)?,
            length: self.f32(at + 80)?,
            events,
        })
    }

    fn aabb(&self, at: usize, depth: usize) -> Result<AabbNode, FormatError> {
        if depth > 64 {
            return Err(unsupported("AABB tree deeper than 64".into()));
        }
        self.spend(AABB_NODE)?;
        let bounds = self.floats::<6>(at)?;
        let left = self.u32(at + 24)? as usize;
        let right = self.u32(at + 28)? as usize;
        let face = self.u32(at + 32)? as i32;
        let plane = self.u32(at + 36)?;
        let children = if face == -1 {
            Some(Box::new((
                self.aabb(left, depth + 1)?,
                self.aabb(right, depth + 1)?,
            )))
        } else {
            None
        };
        Ok(AabbNode {
            bounds,
            face,
            plane,
            children,
        })
    }
}

impl Model {
    /// Parses a binary model of either game (`Variant::of` tells which).
    pub fn parse(mdl: &[u8], mdx: &[u8]) -> Result<Model, FormatError> {
        if mdl.len() < DATA + 196 {
            return Err(truncated(DATA + 196, mdl.len()));
        }
        if mdl[..4] != [0, 0, 0, 0] {
            return Err(unsupported(
                "file header does not start with 0 (an ASCII model?)".into(),
            ));
        }
        let mut cursor = Cursor {
            mdl,
            mdx,
            variant: Variant::of(mdl),
            names: Vec::new(),
            anim_root: 0,
            budget: Cell::new(mdl.len() * REVISITS),
        };
        let name = cursor.text(8, 32)?;
        let root_at = cursor.u32(40)? as usize;
        let header = 80;
        let classification = cursor.bytes(header, 1)?[0];
        let subclassification = cursor.bytes(header + 1, 1)?[0];
        let affected_by_fog = cursor.bytes(header + 3, 1)?[0] != 0;
        let animations_at = cursor.array(header + 8)?;
        let bounding_box = cursor.floats::<6>(header + 24)?;
        let radius = cursor.f32(header + 48)?;
        let animation_scale = cursor.f32(header + 52)?;
        let supermodel = cursor.text(header + 56, 32)?;
        cursor.anim_root = cursor.u32(header + 88)?;
        cursor.names = cursor.name_table()?;

        let root = cursor.node(root_at, 0, false)?;
        let animations = cursor
            .u32s(animations_at)?
            .into_iter()
            .map(|offset| {
                let at = offset as usize;
                let root_at = cursor.u32(at + 40)? as usize;
                let Timing {
                    name,
                    length,
                    events,
                } = cursor.timing(at)?;
                Ok(Animation {
                    name,
                    length,
                    transition: cursor.f32(at + 84)?,
                    root_name: cursor.text(at + 88, 32)?,
                    events,
                    root: cursor.node(root_at, 0, false)?,
                })
            })
            .collect::<Result<_, FormatError>>()?;

        Ok(Model {
            name,
            classification,
            subclassification,
            affected_by_fog,
            supermodel: (!supermodel.is_empty() && supermodel != "null").then_some(supermodel),
            bounding_box,
            radius,
            animation_scale,
            root,
            animations,
        })
    }

    /// The model's supermodel and its animations' names, lengths and
    /// events, read from the headers alone (no nodes, no MDX, either game):
    /// when an animation's events fall (a creature's attack's `hit`).
    pub fn timings(mdl: &[u8]) -> Result<(Option<String>, Vec<Timing>), FormatError> {
        if mdl.len() < DATA + 196 {
            return Err(truncated(DATA + 196, mdl.len()));
        }
        let cursor = Cursor {
            mdl,
            mdx: &[],
            variant: Variant::of(mdl),
            names: Vec::new(),
            anim_root: 0,
            budget: Cell::new(mdl.len() * REVISITS),
        };
        let header = 80;
        let supermodel = cursor.text(header + 56, 32)?;
        let timings = cursor
            .u32s(cursor.array(header + 8)?)?
            .into_iter()
            .map(|offset| cursor.timing(offset as usize))
            .collect::<Result<_, FormatError>>()?;
        Ok((
            (!supermodel.is_empty() && supermodel != "null").then_some(supermodel),
            timings,
        ))
    }

    /// Every node of the model's own tree, depth first.
    pub fn nodes(&self) -> Vec<&Node> {
        fn walk<'a>(node: &'a Node, out: &mut Vec<&'a Node>) {
            out.push(node);
            node.children.iter().for_each(|child| walk(child, out));
        }
        let mut out = Vec::new();
        walk(&self.root, &mut out);
        out
    }
}

/// A packed orientation word as a quaternion (x, y, z, w): 11 bits each for
/// x and y and 10 for z, mapped to [-1, 1]; w is the negative root of the
/// remainder. Both references decode it this way.
pub fn unpack_quaternion(word: u32) -> [f32; 4] {
    let mut x = 1.0 - (word & 0x7ff) as f32 / 1023.0;
    let mut y = 1.0 - ((word >> 11) & 0x7ff) as f32 / 1023.0;
    let mut z = 1.0 - (word >> 22) as f32 / 511.0;
    let dot = x * x + y * y + z * z;
    let w = if dot < 1.0 {
        -(1.0 - dot).sqrt()
    } else {
        let len = dot.sqrt();
        x /= len;
        y /= len;
        z /= len;
        0.0
    };
    [x, y, z, w]
}
