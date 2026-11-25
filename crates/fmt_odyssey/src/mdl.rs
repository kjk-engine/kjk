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
/// An AABB node's child pointer when it has no child.
const NO_CHILD: usize = u32::MAX as usize;
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
pub const FLAG_TRIGGER: u16 = 0x0400;
pub const FLAG_SABER: u16 = 0x0800;
/// The twelve flag words the games read a node by: a plain node, a light,
/// an emitter, a camera, a reference, a mesh, a skin, an animated mesh, a
/// dangly mesh, a walkmesh, a trigger and a lightsaber blade. The games
/// compare the whole word with each of these. A node with any other word
/// is kept as a plain node, and nothing of it is read past its header
/// (`Cursor::node`).
const NODE_TYPES: [u16; 12] = [
    FLAG_HEADER,
    FLAG_HEADER | FLAG_LIGHT,
    FLAG_HEADER | FLAG_EMITTER,
    FLAG_HEADER | FLAG_CAMERA,
    FLAG_HEADER | FLAG_REFERENCE,
    FLAG_HEADER | FLAG_MESH,
    FLAG_HEADER | FLAG_MESH | FLAG_SKIN,
    FLAG_HEADER | FLAG_MESH | FLAG_ANIM,
    FLAG_HEADER | FLAG_MESH | FLAG_DANGLY,
    FLAG_HEADER | FLAG_MESH | FLAG_AABB,
    FLAG_HEADER | FLAG_TRIGGER,
    FLAG_HEADER | FLAG_MESH | FLAG_SABER,
];

/// The name a model hung under a reference node gives its `default` clip
/// in the model it hangs in, before its own name (`attach_references`):
/// `animloop` first, so a room loops it.
pub const HUNG_LOOP: &str = "animloop~";

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
    /// Each vertex's rest place, one per constraint: where the spring pulls
    /// it back to.
    pub positions: Vec<[f32; 3]>,
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

    /// `len` floats of a controller from `start` in its node's controller
    /// data (read from `offset`, `data`). The games load a model whose
    /// controller rows run past that data's count (a player's TSL refused
    /// `c_drdprot` here, and with it the Harbinger's crew quarters): they
    /// index the data from its start without checking the count, so the
    /// rows read on into the floats the file holds after it, as here.
    /// `None` only when they run past the file's end.
    fn controller_floats(
        &self,
        data: &[f32],
        offset: usize,
        start: usize,
        len: usize,
    ) -> Option<Vec<f32>> {
        match data.get(start..start + len) {
            Some(within) => Some(within.to_vec()),
            None => self.f32s(offset + start * 4, len).ok(),
        }
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
        let number = self.u16(at + 2)?;
        let name_index = usize::from(self.u16(at + 4)?);
        let position = self.floats::<3>(at + 16)?;
        let orientation = self.floats::<4>(at + 28)?;
        if !NODE_TYPES.contains(&flags) {
            // The games read a node's children, its controllers and its
            // type's own headers only when its flag word is one of the
            // twelve they know. They keep any other node as a plain node
            // and never look past its header, so the model still loads. A
            // player's `l_drdprot` has a node with the word 0x6310. kjk
            // refused that model, and with it two areas of the droid
            // planet M4-78. Such a node draws nothing and has no children
            // here either.
            return Ok(Node {
                flags,
                number,
                // A name number outside the name table names nothing.
                name: self.names.get(name_index).cloned().unwrap_or(String::new()),
                position,
                orientation,
                controllers: Vec::new(),
                light: None,
                reference: None,
                emitter: None,
                mesh: None,
                children: Vec::new(),
                animated,
            });
        }
        let name = self.names.get(name_index).cloned().ok_or_else(|| {
            unsupported(format!("name index {name_index} of {}", self.names.len()))
        })?;
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
            let (Some(times), Some(values)) = (
                self.controller_floats(&data, data_offset, time, rows),
                self.controller_floats(&data, data_offset, value, rows * per_row),
            ) else {
                // Rows past the end of the file: the games would read
                // whatever memory follows; this controller animates nothing.
                continue;
            };
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
            let positions_at = self.u32(sub + 24)? as usize;
            dangly = Some(Dangly {
                displacement: self.f32(sub + 12)?,
                tightness: self.f32(sub + 16)?,
                period: self.f32(sub + 20)?,
                constraints: self.f32s(constraints.0, constraints.1)?,
                positions: (0..constraints.1)
                    .map(|i| self.floats::<3>(positions_at + i * 12))
                    .collect::<Result<_, _>>()?,
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
        // A child pointer of -1 means there is no child. Some modded room
        // models (Echoes of the Republic's 772qgm_01a) have an empty tree:
        // a root that is an inner node with both pointers -1.
        let children = if face == -1 && left != NO_CHILD && right != NO_CHILD {
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

impl Node {
    /// Whether the node's flag word is one the games read a node by. Any
    /// other node is a plain node without children, controllers or parts.
    pub fn of_known_type(&self) -> bool {
        NODE_TYPES.contains(&self.flags)
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

    /// The names of the model's nodes, read from its header's name table
    /// alone (no nodes, no MDX).
    pub fn node_names(mdl: &[u8]) -> Result<Vec<String>, FormatError> {
        if mdl.len() < DATA + 196 {
            return Err(truncated(DATA + 196, mdl.len()));
        }
        Cursor {
            mdl,
            mdx: &[],
            variant: Variant::of(mdl),
            names: Vec::new(),
            anim_root: 0,
            budget: Cell::new(mdl.len() * REVISITS),
        }
        .name_table()
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

    /// Hangs the model each reference node names under that node, as the
    /// games draw a reference node: the other model's whole tree rides on
    /// it. `load` gives the named model, its own references already hung,
    /// or `None` when it is not there. Mods use reference nodes to share
    /// one hilt between many lightsaber models (Lightsaber Hilt Variety's
    /// `ref_hilt`). Returns the names `load` could not give.
    ///
    /// A hung model's `default` clip comes with it, renamed `HUNG_LOOP`
    /// and its name, its root the hung model's root node. A room loops
    /// every clip whose name begins `animloop`, so a room plays it, as the
    /// games do: Dantooine's brith flaps its wings as its room flies it
    /// about (`m14aa_01f`, issue 364), and the Star Forge's sky moves the
    /// fleets it hangs (`m45aa_01z`'s `m45aa_srf` and `m45aa_ssf`). A
    /// placeable plays
    /// only `animloopNN`, so it does not. Clips the hung model brought
    /// from its own references come too.
    pub fn attach_references(
        &mut self,
        load: &mut dyn FnMut(&str) -> Option<Model>,
    ) -> Vec<String> {
        fn walk(
            node: &mut Node,
            load: &mut dyn FnMut(&str) -> Option<Model>,
            (missing, clips): (&mut Vec<String>, &mut Vec<Animation>),
        ) {
            for child in &mut node.children {
                walk(child, load, (missing, clips));
            }
            let Some(name) = node
                .reference
                .as_ref()
                .map(|reference| reference.model.trim())
            else {
                return;
            };
            if name.is_empty() || name.eq_ignore_ascii_case("null") {
                return;
            }
            match load(name) {
                Some(model) => {
                    let root = model.root.name.clone();
                    for clip in model.animations {
                        if clip.name.eq_ignore_ascii_case("default") {
                            clips.push(Animation {
                                name: format!("{HUNG_LOOP}{}", model.name.to_ascii_lowercase()),
                                root_name: root.clone(),
                                ..clip
                            });
                        } else if clip.name.starts_with(HUNG_LOOP) {
                            clips.push(clip);
                        }
                    }
                    node.children.push(model.root);
                }
                None => missing.push(name.to_string()),
            }
        }
        let mut missing = Vec::new();
        let mut clips = Vec::new();
        walk(&mut self.root, load, (&mut missing, &mut clips));
        self.animations.extend(clips);
        missing
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

/// A copy of the model file `mdl` under another name: the name in its
/// header (32 bytes) is `name`, and nothing else changes. The level editor
/// copies a room's model this way when it places the room somewhere new,
/// since the room's walkmesh goes by the model's name. Refused when `name`
/// does not fit the header or the file is too short to have one.
pub fn renamed(mdl: &[u8], name: &str) -> Result<Vec<u8>, FormatError> {
    if mdl.len() < DATA + 196 {
        return Err(truncated(DATA + 196, mdl.len()));
    }
    if name.len() >= 32 || !name.is_ascii() {
        return Err(unsupported(format!("{name:?} does not fit a model's name")));
    }
    let mut copy = mdl.to_vec();
    let field = &mut copy[DATA + 8..DATA + 40];
    field.fill(0);
    field[..name.len()].copy_from_slice(name.as_bytes());
    Ok(copy)
}

/// A copy of the model file `mdl` whose meshes name other lightmaps:
/// each mesh's lightmap that `rename` gives a new name for takes that
/// name. A room placed twice in one map gets lightmaps of its own in its
/// second copy, so a bake lights each copy where it stands. Gives the copy
/// and every lightmap name the model had. Every node under the root is
/// visited once. Refused when a node or a name lies outside the file, or
/// a new name does not fit.
pub fn relit(
    mdl: &[u8],
    rename: &dyn Fn(&str) -> Option<String>,
) -> Result<(Vec<u8>, Vec<String>), FormatError> {
    const LIGHTMAP: usize = NODE_HEADER + 120;
    let mut copy = mdl.to_vec();
    let get = |copy: &[u8], at: usize| -> Result<u32, FormatError> {
        copy.get(DATA + at..DATA + at + 4)
            .map(|bytes| u32::from_le_bytes(bytes.try_into().expect("4 bytes")))
            .ok_or(truncated(DATA + at + 4, copy.len()))
    };
    let mut names = Vec::new();
    let mut seen = std::collections::HashSet::new();
    let mut nodes = vec![get(&copy, 40)? as usize];
    while let Some(at) = nodes.pop() {
        if !seen.insert(at) {
            continue;
        }
        let flags = (copy.get(DATA + at..DATA + at + 2))
            .map(|bytes| u16::from_le_bytes([bytes[0], bytes[1]]))
            .ok_or(truncated(DATA + at + 2, copy.len()))?;
        if flags & FLAG_MESH != 0 {
            let field = DATA + at + LIGHTMAP;
            let len = copy.len();
            let bytes = copy
                .get_mut(field..field + 32)
                .ok_or(truncated(field + 32, len))?;
            let end = bytes.iter().position(|&byte| byte == 0).unwrap_or(32);
            let name = String::from_utf8_lossy(&bytes[..end]).to_ascii_lowercase();
            if !name.is_empty() && !fmt_is_null(&name) {
                if let Some(new) = rename(&name) {
                    if new.len() >= 32 || !new.is_ascii() {
                        return Err(unsupported(format!(
                            "{new:?} does not fit a lightmap's name"
                        )));
                    }
                    bytes.fill(0);
                    bytes[..new.len()].copy_from_slice(new.as_bytes());
                }
                if !names.contains(&name) {
                    names.push(name);
                }
            }
        }
        let (children, count) = (get(&copy, at + 44)? as usize, get(&copy, at + 48)? as usize);
        for child in 0..count {
            nodes.push(get(&copy, children + child * 4)? as usize);
        }
    }
    Ok((copy, names))
}

/// Whether a texture name is the games' word for none.
fn fmt_is_null(name: &str) -> bool {
    name.eq_ignore_ascii_case("null")
}

/// A copy of the model file `mdl` moved by `by`: each child of the root
/// node is moved, its place in its node header and in its position
/// controllers, and so are the same nodes in each animation. Everything
/// under them moves with them, so no vertex changes and the MDX stays as
/// it is. The model's bounding box moves too. A room cut from a Jedi
/// Academy level is built in the level's own coordinates, and its copy in
/// a map is moved so that the map's place for it is its origin
/// (`level_edit::map::room_copy`). Refused when a node or a controller
/// lies outside the file.
pub fn shifted(mdl: &[u8], by: [f32; 3]) -> Result<Vec<u8>, FormatError> {
    if mdl.len() < DATA + 196 {
        return Err(truncated(DATA + 196, mdl.len()));
    }
    let mut copy = mdl.to_vec();
    let get = |copy: &[u8], at: usize| -> Result<u32, FormatError> {
        copy.get(DATA + at..DATA + at + 4)
            .map(|bytes| u32::from_le_bytes(bytes.try_into().expect("4 bytes")))
            .ok_or(truncated(DATA + at + 4, copy.len()))
    };
    let float = |copy: &[u8], at: usize| get(copy, at).map(f32::from_bits);
    let add = |copy: &mut [u8], at: usize, value: f32| -> Result<(), FormatError> {
        let len = copy.len();
        let now = float(copy, at)?;
        copy.get_mut(DATA + at..DATA + at + 4)
            .ok_or(truncated(DATA + at + 4, len))?
            .copy_from_slice(&(now + value).to_bits().to_le_bytes());
        Ok(())
    };
    // One node: its header's place (x, y, z at 16), then its position
    // controller's rows.
    let move_node = |copy: &mut Vec<u8>, at: usize| -> Result<(), FormatError> {
        for (axis, part) in by.iter().enumerate() {
            add(copy, at + 16 + axis * 4, *part)?;
        }
        let (keys_at, keys) = (get(copy, at + 56)? as usize, get(copy, at + 60)? as usize);
        let data_at = get(copy, at + 68)? as usize;
        for k in 0..keys.min(256) {
            let key = keys_at + k * 16;
            let kind = get(copy, key)?;
            let rows = (get(copy, key + 4)? >> 16) as usize;
            let values = (get(copy, key + 8)? >> 16) as usize;
            let raw = copy
                .get(DATA + key + 12)
                .copied()
                .ok_or(truncated(DATA + key + 13, copy.len()))?;
            let (columns, bezier) = (raw & !BEZIER, raw & BEZIER != 0);
            if (kind, columns) != (CONTROLLER_POSITION, 3) {
                continue;
            }
            // How a Bézier row's control points move with it is not known
            // here, so such a model is not moved.
            if bezier {
                return Err(unsupported(
                    "a node's position is a Bézier curve, which is not moved".to_string(),
                ));
            }
            let start = data_at + values * 4;
            for row in 0..rows {
                let at = start + row * 12;
                for (axis, part) in by.iter().enumerate() {
                    add(copy, at + axis * 4, *part)?;
                }
            }
        }
        Ok(())
    };
    let children = |copy: &[u8], root: usize| -> Result<Vec<usize>, FormatError> {
        let (at, count) = (
            get(copy, root + 44)? as usize,
            get(copy, root + 48)? as usize,
        );
        (0..count.min(4096))
            .map(|i| get(copy, at + i * 4).map(|offset| offset as usize))
            .collect()
    };
    let root = get(&copy, 40)? as usize;
    for child in children(&copy, root)? {
        move_node(&mut copy, child)?;
    }
    let header = 80;
    let (animations_at, animations) = (
        get(&copy, header + 8)? as usize,
        get(&copy, header + 12)? as usize,
    );
    for i in 0..animations.min(1024) {
        let animation = get(&copy, animations_at + i * 4)? as usize;
        let animation_root = get(&copy, animation + 40)? as usize;
        for child in children(&copy, animation_root)? {
            move_node(&mut copy, child)?;
        }
    }
    // The bounding box.
    for corner in 0..2 {
        for (axis, part) in by.iter().enumerate() {
            add(&mut copy, header + 24 + corner * 12 + axis * 4, *part)?;
        }
    }
    Ok(copy)
}

/// A packed orientation word (`unpack_quaternion`) of a unit quaternion
/// (x, y, z, w). The word keeps no sign for w, which it reads as negative
/// or zero, so a quaternion with w above zero is packed as its negative,
/// which turns the same way.
pub fn pack_quaternion(q: [f32; 4]) -> u32 {
    let q = if q[3] > 0.0 { q.map(|v| -v) } else { q };
    let bits = |value: f32, most: f32| ((1.0 - value.clamp(-1.0, 1.0)) * most).round() as u32;
    bits(q[0], 1023.0) | bits(q[1], 1023.0) << 11 | bits(q[2], 511.0) << 22
}

/// A copy of the model file `mdl` turned about Z (anticlockwise seen from
/// above) round its origin by the angle whose sine and cosine are `turn`,
/// as a room of a new map is turned.
/// Each child of the model's root node is turned: its place and its
/// orientation, in its node header and in its position and orientation
/// controllers, and so are the same nodes in each animation. Everything
/// under them turns with them, so no vertex changes and the MDX stays as
/// it is. The model's bounding box is made round the turned box. Refused
/// when a node or a controller lies outside the file.
pub fn turned(mdl: &[u8], (sin, cos): (f32, f32)) -> Result<Vec<u8>, FormatError> {
    if mdl.len() < DATA + 196 {
        return Err(truncated(DATA + 196, mdl.len()));
    }
    let mut copy = mdl.to_vec();
    // The half angle's, for the quaternions: cos² (a/2) = (1 + cos a) / 2.
    let half_cos = ((1.0 + cos) * 0.5).max(0.0).sqrt();
    let half_sin = ((1.0 - cos) * 0.5).max(0.0).sqrt().copysign(sin);
    let turn = |x: f32, y: f32| (cos * x - sin * y, sin * x + cos * y);
    // Rz ⊗ q, with q as (x, y, z, w).
    let spin = |q: [f32; 4]| {
        let (s, c) = (half_sin, half_cos);
        [
            c * q[0] - s * q[1],
            c * q[1] + s * q[0],
            c * q[2] + s * q[3],
            c * q[3] - s * q[2],
        ]
    };
    let get = |copy: &[u8], at: usize| -> Result<u32, FormatError> {
        copy.get(DATA + at..DATA + at + 4)
            .map(|bytes| u32::from_le_bytes(bytes.try_into().expect("4 bytes")))
            .ok_or(truncated(DATA + at + 4, copy.len()))
    };
    let float = |copy: &[u8], at: usize| get(copy, at).map(f32::from_bits);
    let put = |copy: &mut [u8], at: usize, word: u32| -> Result<(), FormatError> {
        let len = copy.len();
        copy.get_mut(DATA + at..DATA + at + 4)
            .ok_or(truncated(DATA + at + 4, len))?
            .copy_from_slice(&word.to_le_bytes());
        Ok(())
    };
    let set = |copy: &mut [u8], at: usize, value: f32| put(copy, at, value.to_bits());

    // One node: its header's place (x, y, z at 16) and orientation
    // (w, x, y, z at 28), then its controllers' rows.
    let turn_node = |copy: &mut Vec<u8>, at: usize| -> Result<(), FormatError> {
        let (x, y) = turn(float(copy, at + 16)?, float(copy, at + 20)?);
        set(copy, at + 16, x)?;
        set(copy, at + 20, y)?;
        let wxyz = [
            float(copy, at + 28)?,
            float(copy, at + 32)?,
            float(copy, at + 36)?,
            float(copy, at + 40)?,
        ];
        let q = spin([wxyz[1], wxyz[2], wxyz[3], wxyz[0]]);
        for (offset, value) in [(28, q[3]), (32, q[0]), (36, q[1]), (40, q[2])] {
            set(copy, at + offset, value)?;
        }
        let (keys_at, keys) = (get(copy, at + 56)? as usize, get(copy, at + 60)? as usize);
        let data_at = get(copy, at + 68)? as usize;
        for k in 0..keys.min(256) {
            let key = keys_at + k * 16;
            let kind = get(copy, key)?;
            let head = get(copy, key + 4)?;
            let rows = (head >> 16) as usize;
            let values = (get(copy, key + 8)? >> 16) as usize;
            let raw = copy
                .get(DATA + key + 12)
                .copied()
                .ok_or(truncated(DATA + key + 13, copy.len()))?;
            let (columns, bezier) = (raw & !BEZIER, raw & BEZIER != 0);
            let start = data_at + values * 4;
            match (kind, columns) {
                (CONTROLLER_POSITION, 3) => {
                    let triples = rows * if bezier { 3 } else { 1 };
                    for row in 0..triples {
                        let at = start + row * 12;
                        let (x, y) = turn(float(copy, at)?, float(copy, at + 4)?);
                        set(copy, at, x)?;
                        set(copy, at + 4, y)?;
                    }
                }
                (CONTROLLER_ORIENTATION, 4) => {
                    for row in 0..rows {
                        let at = start + row * 16;
                        let q = [
                            float(copy, at)?,
                            float(copy, at + 4)?,
                            float(copy, at + 8)?,
                            float(copy, at + 12)?,
                        ];
                        for (i, value) in spin(q).into_iter().enumerate() {
                            set(copy, at + i * 4, value)?;
                        }
                    }
                }
                (CONTROLLER_ORIENTATION, 2) => {
                    for row in 0..rows {
                        let at = start + row * 4;
                        let q = unpack_quaternion(get(copy, at)?);
                        put(copy, at, pack_quaternion(spin(q)))?;
                    }
                }
                _ => {}
            }
        }
        Ok(())
    };
    // The children of a root node at `root`.
    let children = |copy: &[u8], root: usize| -> Result<Vec<usize>, FormatError> {
        let (at, count) = (
            get(copy, root + 44)? as usize,
            get(copy, root + 48)? as usize,
        );
        (0..count.min(4096))
            .map(|i| get(copy, at + i * 4).map(|offset| offset as usize))
            .collect()
    };
    let root = get(&copy, 40)? as usize;
    for child in children(&copy, root)? {
        turn_node(&mut copy, child)?;
    }
    let header = 80;
    let (animations_at, animations) = (
        get(&copy, header + 8)? as usize,
        get(&copy, header + 12)? as usize,
    );
    for i in 0..animations.min(1024) {
        let animation = get(&copy, animations_at + i * 4)? as usize;
        let animation_root = get(&copy, animation + 40)? as usize;
        for child in children(&copy, animation_root)? {
            turn_node(&mut copy, child)?;
        }
    }
    // The bounding box round the turned box's corners.
    let box_at = header + 24;
    let low = [
        float(&copy, box_at)?,
        float(&copy, box_at + 4)?,
        float(&copy, box_at + 8)?,
    ];
    let high = [
        float(&copy, box_at + 12)?,
        float(&copy, box_at + 16)?,
        float(&copy, box_at + 20)?,
    ];
    let (mut min, mut max) = ([f32::INFINITY; 2], [f32::NEG_INFINITY; 2]);
    for x in [low[0], high[0]] {
        for y in [low[1], high[1]] {
            let (tx, ty) = turn(x, y);
            min = [min[0].min(tx), min[1].min(ty)];
            max = [max[0].max(tx), max[1].max(ty)];
        }
    }
    if min.iter().chain(&max).all(|v| v.is_finite()) {
        set(&mut copy, box_at, min[0])?;
        set(&mut copy, box_at + 4, min[1])?;
        set(&mut copy, box_at + 12, max[0])?;
        set(&mut copy, box_at + 16, max[1])?;
    }
    Ok(copy)
}

/// One mesh of a model `written` lays out: its name, its texture and its
/// lightmap (16 characters at most each: they name resources), and its
/// vertices and triangles in the model's space.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct WrittenMesh {
    pub name: String,
    pub texture: String,
    pub lightmap: String,
    pub positions: Vec<[f32; 3]>,
    pub normals: Vec<[f32; 3]>,
    pub uv1: Vec<[f32; 2]>,
    pub uv2: Vec<[f32; 2]>,
    /// Counter-clockwise seen from the front.
    pub triangles: Vec<[u16; 3]>,
    /// Walk surface material of every face (`surfacemat.2da`).
    pub material: u32,
    /// The mesh's transparency hint. kjk counts a room's mesh with any hint
    /// as see-through, so it is not solid.
    pub transparency_hint: u32,
}

/// A K1 model file and its MDX of `meshes`, each a mesh node under one root
/// node named `name`, at the model's origin, with no controller and no
/// animation. Only kjk reads such a model: it is laid out as `Model::parse`
/// reads one, and `renamed` and `turned` change it as they change a room's.
/// It is what kjk builds a Jedi Academy room as (`asset_ja`), in memory.
///
/// Layout, offsets after the file header: the geometry and model headers,
/// then the name table and its names, then each node (the root first): its
/// header, a mesh's header after it, then the mesh's faces, and last the
/// root's list of children. A mesh's vertices are in the MDX, one after
/// another: place, normal, texture place and lightmap place (40 bytes).
///
/// Refused when a mesh has more than 65,535 vertices (a face numbers them
/// with 16 bits), when its arrays differ in length, or when a name does
/// not fit.
pub fn written(name: &str, meshes: &[WrittenMesh]) -> Result<(Vec<u8>, Vec<u8>), FormatError> {
    const GEOMETRY: usize = 80;
    const MODEL: usize = 116;
    const MESH: usize = 332;
    const STRIDE: usize = 40;
    if name.len() >= 32 || !name.is_ascii() {
        return Err(unsupported(format!("{name:?} does not fit a model's name")));
    }
    for mesh in meshes {
        let count = mesh.positions.len();
        if count > usize::from(u16::MAX) {
            return Err(unsupported(format!(
                "{}: {count} vertices, more than a face can number",
                mesh.name
            )));
        }
        if [mesh.normals.len(), mesh.uv1.len(), mesh.uv2.len()]
            .iter()
            .any(|len| *len != count)
        {
            return Err(unsupported(format!(
                "{}: its arrays differ in length",
                mesh.name
            )));
        }
        if mesh.texture.len() > 16 || mesh.lightmap.len() > 16 || mesh.name.len() >= 32 {
            return Err(unsupported(format!("{}: a name is too long", mesh.name)));
        }
        if (mesh.triangles.iter().flatten()).any(|&at| usize::from(at) >= count) {
            return Err(unsupported(format!(
                "{}: a face names no vertex",
                mesh.name
            )));
        }
    }
    fn put_u32(data: &mut [u8], at: usize, value: u32) {
        data[at..at + 4].copy_from_slice(&value.to_le_bytes());
    }
    fn put_u16(data: &mut [u8], at: usize, value: u16) {
        data[at..at + 2].copy_from_slice(&value.to_le_bytes());
    }
    fn put_f32(data: &mut [u8], at: usize, value: f32) {
        put_u32(data, at, value.to_bits());
    }
    fn put_text(data: &mut [u8], at: usize, len: usize, text: &str) {
        let bytes = &text.as_bytes()[..text.len().min(len - 1)];
        data[at..at + bytes.len()].copy_from_slice(bytes);
    }
    fn bounds<'a>(points: impl Iterator<Item = &'a [f32; 3]>) -> ([f32; 3], [f32; 3]) {
        let (mut low, mut high) = ([f32::INFINITY; 3], [f32::NEG_INFINITY; 3]);
        for point in points {
            for axis in 0..3 {
                low[axis] = low[axis].min(point[axis]);
                high[axis] = high[axis].max(point[axis]);
            }
        }
        if low[0].is_finite() {
            (low, high)
        } else {
            ([0.0; 3], [0.0; 3])
        }
    }

    let mut data: Vec<u8> = vec![0; GEOMETRY + MODEL];
    // The name table: the root's name, then each mesh's.
    let names: Vec<&str> = std::iter::once(name)
        .chain(meshes.iter().map(|mesh| mesh.name.as_str()))
        .collect();
    let table_at = data.len();
    data.resize(table_at + names.len() * 4, 0);
    for (index, text) in names.iter().enumerate() {
        let at = data.len();
        data.extend_from_slice(text.as_bytes());
        data.push(0);
        put_u32(&mut data, table_at + index * 4, at as u32);
    }
    while !data.len().is_multiple_of(4) {
        data.push(0);
    }
    let (low, high) = bounds(meshes.iter().flat_map(|mesh| &mesh.positions));
    let radius = (0..3)
        .map(|axis| (high[axis] - low[axis]).powi(2))
        .sum::<f32>()
        .sqrt()
        / 2.0;

    // A node's header: its flags, its number and name, the root and its
    // parent, its place at the origin, facing ahead.
    let node_header = |data: &mut Vec<u8>, flags: u16, number: u16, parent: u32| -> usize {
        let at = data.len();
        data.resize(at + NODE_HEADER, 0);
        put_u16(data, at, flags);
        put_u16(data, at + 2, number);
        put_u16(data, at + 4, number);
        put_u32(data, at + 8, GEOMETRY as u32);
        put_u32(data, at + 12, parent);
        // Orientation (w, x, y, z): none.
        put_f32(data, at + 28, 1.0);
        at
    };
    let root = node_header(&mut data, FLAG_HEADER, 0, 0);
    let mut mdx: Vec<u8> = Vec::new();
    let mut children = Vec::with_capacity(meshes.len());
    for (index, mesh) in meshes.iter().enumerate() {
        let number = (index + 1) as u16;
        let at = node_header(&mut data, FLAG_HEADER | FLAG_MESH, number, root as u32);
        children.push(at as u32);
        let header = data.len();
        data.resize(header + MESH, 0);
        let (low, high) = bounds(mesh.positions.iter());
        for axis in 0..3 {
            put_f32(&mut data, header + 20 + axis * 4, low[axis]);
            put_f32(&mut data, header + 32 + axis * 4, high[axis]);
            put_f32(&mut data, header + 60 + axis * 4, 1.0);
            put_f32(&mut data, header + 72 + axis * 4, 1.0);
        }
        put_u32(&mut data, header + 84, mesh.transparency_hint);
        put_text(&mut data, header + 88, 32, &mesh.texture);
        put_text(&mut data, header + 120, 32, &mesh.lightmap);
        put_u32(&mut data, header + 252, STRIDE as u32);
        // Where each kind of vertex data lies in a vertex: place, normal,
        // (no colour), texture place and lightmap place; none of the rest.
        for slot in 0..11 {
            let offset = match slot {
                0 => 0,
                1 => 12,
                3 => 24,
                4 => 32,
                _ => u32::MAX,
            };
            put_u32(&mut data, header + 260 + slot * 4, offset);
        }
        put_u16(&mut data, header + 304, mesh.positions.len() as u16);
        put_u16(&mut data, header + 306, 2);
        // Lightmapped, not rotating its texture, not a background, casting a
        // shadow, not beaming, drawn.
        data[header + 308..header + 314].copy_from_slice(&[1, 0, 0, 1, 0, 1]);
        put_u32(&mut data, header + 324, mdx.len() as u32);
        for at in 0..mesh.positions.len() {
            for value in (mesh.positions[at].iter())
                .chain(&mesh.normals[at])
                .chain(&mesh.uv1[at])
                .chain(&mesh.uv2[at])
            {
                mdx.extend_from_slice(&value.to_bits().to_le_bytes());
            }
        }
        // The faces: each its plane, its material, no neighbours, and its
        // vertices.
        let faces_at = data.len();
        put_u32(&mut data, header + 8, faces_at as u32);
        put_u32(&mut data, header + 12, mesh.triangles.len() as u32);
        put_u32(&mut data, header + 16, mesh.triangles.len() as u32);
        for triangle in &mesh.triangles {
            let [a, b, c] = triangle.map(|at| mesh.positions[usize::from(at)]);
            let u = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
            let v = [c[0] - a[0], c[1] - a[1], c[2] - a[2]];
            let mut normal = [
                u[1] * v[2] - u[2] * v[1],
                u[2] * v[0] - u[0] * v[2],
                u[0] * v[1] - u[1] * v[0],
            ];
            let length = normal.iter().map(|part| part * part).sum::<f32>().sqrt();
            if length > 0.0 {
                normal = normal.map(|part| part / length);
            }
            let distance = -(normal[0] * a[0] + normal[1] * a[1] + normal[2] * a[2]);
            let face = data.len();
            data.resize(face + 32, 0);
            for (axis, part) in normal.iter().enumerate() {
                put_f32(&mut data, face + axis * 4, *part);
            }
            put_f32(&mut data, face + 12, distance);
            put_u32(&mut data, face + 16, mesh.material);
            for (side, corner) in triangle.iter().enumerate() {
                put_u16(&mut data, face + 20 + side * 2, u16::MAX);
                put_u16(&mut data, face + 26 + side * 2, *corner);
            }
        }
    }
    // The root's children.
    let children_at = data.len();
    for child in &children {
        data.extend_from_slice(&child.to_le_bytes());
    }
    put_u32(&mut data, root + 44, children_at as u32);
    put_u32(&mut data, root + 48, children.len() as u32);
    put_u32(&mut data, root + 52, children.len() as u32);

    // The geometry header: K1's mark, the name, the root and the count of
    // nodes, and the model type; then the model header.
    put_u32(&mut data, 0, 4_273_776);
    put_text(&mut data, 8, 32, name);
    put_u32(&mut data, 40, root as u32);
    put_u32(&mut data, 44, names.len() as u32);
    data[76] = 2;
    let model = GEOMETRY;
    data[model + 3] = 1;
    for axis in 0..3 {
        put_f32(&mut data, model + 24 + axis * 4, low[axis]);
        put_f32(&mut data, model + 36 + axis * 4, high[axis]);
    }
    put_f32(&mut data, model + 48, radius);
    put_f32(&mut data, model + 52, 1.0);
    put_text(&mut data, model + 56, 32, "null");
    put_u32(&mut data, model + 96, mdx.len() as u32);
    put_u32(&mut data, model + 104, table_at as u32);
    put_u32(&mut data, model + 108, names.len() as u32);
    put_u32(&mut data, model + 112, names.len() as u32);

    let mut file = Vec::with_capacity(DATA + data.len());
    file.extend_from_slice(&0u32.to_le_bytes());
    file.extend_from_slice(&(data.len() as u32).to_le_bytes());
    file.extend_from_slice(&(mdx.len() as u32).to_le_bytes());
    file.extend_from_slice(&data);
    Ok((file, mdx))
}
