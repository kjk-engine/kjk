//! Ghoul2: GLM meshes (`2LGM`, version 6) and GLA skeletons with animation
//! (`2LGA`, version 6). Names are 64-byte fields.
//!
//! GLM header: ident, version, name, animation name, animation index, bone
//! count, LOD count and offset, surface count, surface hierarchy offset, end.
//! Right after the 164-byte header is a table of surface offsets, relative to
//! the table; each points at an entry: name, flags, shader name, shader
//! index, parent, child count, child indices. Each LOD is its
//! end offset, a table of surface offsets (relative to the table), then the
//! surfaces: ident, index, header offset, vertex count and offset, triangle
//! count and offset, bone reference count and offset, end. A vertex (32
//! bytes) is a normal, a position, a word packing the weight count (top 2
//! bits) and up to four 5-bit bone references plus the high 2 bits of each
//! weight, and four low weight bytes; texture coordinates follow the
//! vertices.
//!
//! GLA header: ident, version, name, scale, frame count and offset, bone
//! count, compressed bone pool offset, skeleton offset, end. Right after the
//! 100-byte header is a table of bone offsets, relative to the table; each
//! points at a bone: name, flags, parent, base pose and its
//! inverse (3×4 floats each), child count, children. Each frame holds a
//! 3-byte pool index per bone; a pool entry is 14 bytes (see
//! `unpack_bone`).

use std::fmt;

const MAX_QPATH: usize = 64;
const GLM_HEADER: usize = 164;
const GLA_HEADER: usize = 100;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Ghoul2Error {
    Signature(&'static str),
    Version(u32),
    Truncated { what: String },
    Bad(String),
}

impl fmt::Display for Ghoul2Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Ghoul2Error::Signature(what) => write!(f, "ghoul2: not a {what}"),
            Ghoul2Error::Version(version) => write!(f, "ghoul2: version {version}, expected 6"),
            Ghoul2Error::Truncated { what } => write!(f, "ghoul2: truncated {what}"),
            Ghoul2Error::Bad(what) => write!(f, "ghoul2: {what}"),
        }
    }
}

impl std::error::Error for Ghoul2Error {}

struct Bytes<'a>(&'a [u8]);

impl Bytes<'_> {
    fn slice(&self, at: usize, len: usize, what: &str) -> Result<&[u8], Ghoul2Error> {
        at.checked_add(len)
            .and_then(|end| self.0.get(at..end))
            .ok_or_else(|| Ghoul2Error::Truncated {
                what: what.to_string(),
            })
    }

    fn u32(&self, at: usize, what: &str) -> Result<u32, Ghoul2Error> {
        Ok(u32::from_le_bytes(
            self.slice(at, 4, what)?.try_into().expect("4 bytes"),
        ))
    }

    fn i32(&self, at: usize, what: &str) -> Result<i32, Ghoul2Error> {
        Ok(self.u32(at, what)? as i32)
    }

    fn f32(&self, at: usize, what: &str) -> Result<f32, Ghoul2Error> {
        Ok(f32::from_bits(self.u32(at, what)?))
    }

    fn name(&self, at: usize, what: &str) -> Result<String, Ghoul2Error> {
        let raw = self.slice(at, MAX_QPATH, what)?;
        let end = raw.iter().position(|&b| b == 0).unwrap_or(raw.len());
        Ok(String::from_utf8_lossy(&raw[..end]).into_owned())
    }

    /// A count read from the file, capped at how many `each`-byte records
    /// the file has room for: a capacity to reserve, not a promise.
    fn room(&self, count: usize, each: usize) -> usize {
        count.min(self.0.len() / each.max(1))
    }

    fn index(&self, value: i32, what: &str) -> Result<usize, Ghoul2Error> {
        usize::try_from(value).map_err(|_| Ghoul2Error::Bad(format!("negative {what} {value}")))
    }
}

#[derive(Debug, Clone)]
pub struct Glm {
    pub name: String,
    pub animation: String,
    pub bone_count: usize,
    pub surfaces: Vec<SurfaceInfo>,
    /// One entry per level of detail, largest first; each holds one surface
    /// per `surfaces` entry.
    pub lods: Vec<Vec<Surface>>,
}

#[derive(Debug, Clone)]
pub struct SurfaceInfo {
    pub name: String,
    pub flags: u32,
    pub shader: String,
    pub parent: i32,
    pub children: Vec<i32>,
}

pub const SURFACE_BOLT: u32 = 0x1;
pub const SURFACE_OFF: u32 = 0x2;

#[derive(Debug, Clone)]
pub struct Surface {
    pub positions: Vec<[f32; 3]>,
    pub normals: Vec<[f32; 3]>,
    pub uvs: Vec<[f32; 2]>,
    /// Up to four (bone reference, weight) pairs per vertex; the reference
    /// indexes `bone_references`.
    pub weights: Vec<Vec<(u8, f32)>>,
    pub triangles: Vec<[u32; 3]>,
    /// Skeleton bone for each reference.
    pub bone_references: Vec<i32>,
}

fn vertex_weights(word: u32, low: [u8; 4]) -> Vec<(u8, f32)> {
    let count = (word >> 30) as usize + 1;
    let mut total = 0.0;
    (0..count)
        .map(|i| {
            let bone = ((word >> (5 * i)) & 0x1f) as u8;
            let weight = if i == count - 1 {
                1.0 - total
            } else {
                let high = (word >> (12 + i * 2)) & 0x300;
                let value = (u32::from(low[i]) | high) as f32 / 1023.0;
                total += value;
                value
            };
            (bone, weight)
        })
        .collect()
}

impl Glm {
    pub fn parse(bytes: &[u8]) -> Result<Glm, Ghoul2Error> {
        let b = Bytes(bytes);
        if b.slice(0, 4, "header")? != b"2LGM" {
            return Err(Ghoul2Error::Signature("GLM"));
        }
        let version = b.u32(4, "header")?;
        if version != 6 {
            return Err(Ghoul2Error::Version(version));
        }
        let name = b.name(8, "name")?;
        let animation = b.name(72, "animation name")?;
        let bone_count = b.index(b.i32(140, "header")?, "bone count")?;
        let lod_count = b.index(b.i32(144, "header")?, "LOD count")?;
        let lods_at = b.index(b.i32(148, "header")?, "LOD offset")?;
        let surface_count = b.index(b.i32(152, "header")?, "surface count")?;

        let mut surfaces = Vec::with_capacity(b.room(surface_count, 4));
        for i in 0..surface_count {
            let at = GLM_HEADER
                + b.index(
                    b.i32(GLM_HEADER + i * 4, "hierarchy table")?,
                    "hierarchy entry",
                )?;
            let child_count = b.index(b.i32(at + 140, "hierarchy entry")?, "child count")?;
            let children = (0..child_count)
                .map(|c| b.i32(at + 144 + c * 4, "hierarchy children"))
                .collect::<Result<_, _>>()?;
            surfaces.push(SurfaceInfo {
                name: b.name(at, "surface name")?,
                flags: b.u32(at + 64, "hierarchy entry")?,
                shader: b.name(at + 68, "shader name")?,
                parent: b.i32(at + 136, "hierarchy entry")?,
                children,
            });
        }

        let mut lods = Vec::with_capacity(b.room(lod_count, 4));
        let mut lod_at = lods_at;
        for _ in 0..lod_count {
            let lod_end = lod_at + b.index(b.i32(lod_at, "LOD")?, "LOD end")?;
            let table = lod_at + 4;
            let mut lod = Vec::with_capacity(b.room(surface_count, 4));
            for s in 0..surface_count {
                let at = table
                    + b.index(b.i32(table + s * 4, "LOD surface table")?, "surface offset")?;
                let vertex_count = b.index(b.i32(at + 12, "surface")?, "vertex count")?;
                let vertices_at = at + b.index(b.i32(at + 16, "surface")?, "vertex offset")?;
                let triangle_count = b.index(b.i32(at + 20, "surface")?, "triangle count")?;
                let triangles_at = at + b.index(b.i32(at + 24, "surface")?, "triangle offset")?;
                let reference_count =
                    b.index(b.i32(at + 28, "surface")?, "bone reference count")?;
                let references_at =
                    at + b.index(b.i32(at + 32, "surface")?, "bone reference offset")?;
                let mut surface = Surface {
                    positions: Vec::with_capacity(b.room(vertex_count, 32)),
                    normals: Vec::with_capacity(b.room(vertex_count, 32)),
                    uvs: Vec::with_capacity(b.room(vertex_count, 32)),
                    weights: Vec::with_capacity(b.room(vertex_count, 32)),
                    triangles: Vec::with_capacity(b.room(triangle_count, 12)),
                    bone_references: Vec::with_capacity(b.room(reference_count, 4)),
                };
                for v in 0..vertex_count {
                    let vertex = vertices_at + v * 32;
                    let f3 = |o: usize| -> Result<[f32; 3], Ghoul2Error> {
                        Ok([
                            b.f32(vertex + o, "vertex")?,
                            b.f32(vertex + o + 4, "vertex")?,
                            b.f32(vertex + o + 8, "vertex")?,
                        ])
                    };
                    surface.normals.push(f3(0)?);
                    surface.positions.push(f3(12)?);
                    let word = b.u32(vertex + 24, "vertex")?;
                    let low: [u8; 4] = b
                        .slice(vertex + 28, 4, "vertex")?
                        .try_into()
                        .expect("4 bytes");
                    surface.weights.push(vertex_weights(word, low));
                }
                let uvs_at = vertices_at + vertex_count * 32;
                for v in 0..vertex_count {
                    surface.uvs.push([
                        b.f32(uvs_at + v * 8, "texture coordinates")?,
                        b.f32(uvs_at + v * 8 + 4, "texture coordinates")?,
                    ]);
                }
                for t in 0..triangle_count {
                    let at = triangles_at + t * 12;
                    let triangle = [
                        b.u32(at, "triangle")?,
                        b.u32(at + 4, "triangle")?,
                        b.u32(at + 8, "triangle")?,
                    ];
                    if triangle.iter().any(|&v| v as usize >= vertex_count) {
                        return Err(Ghoul2Error::Bad(format!(
                            "surface {s} triangle {t} indexes past {vertex_count} vertices"
                        )));
                    }
                    surface.triangles.push(triangle);
                }
                for r in 0..reference_count {
                    surface
                        .bone_references
                        .push(b.i32(references_at + r * 4, "bone references")?);
                }
                lod.push(surface);
            }
            lods.push(lod);
            lod_at = lod_end;
        }
        Ok(Glm {
            name,
            animation,
            bone_count,
            surfaces,
            lods,
        })
    }
}

#[derive(Debug, Clone)]
pub struct Gla {
    pub name: String,
    pub scale: f32,
    pub bones: Vec<Bone>,
    pub frame_count: usize,
    /// `frames[frame * bones + bone]`: an index into `pool`.
    pub frames: Vec<u32>,
    /// Compressed bone transforms, 7 values each; see `unpack_bone`.
    pub pool: Vec<[u16; 7]>,
}

#[derive(Debug, Clone)]
pub struct Bone {
    pub name: String,
    pub flags: u32,
    pub parent: i32,
    pub base_pose: [[f32; 4]; 3],
    pub base_pose_inverse: [[f32; 4]; 3],
    pub children: Vec<i32>,
}

impl Gla {
    pub fn parse(bytes: &[u8]) -> Result<Gla, Ghoul2Error> {
        let b = Bytes(bytes);
        if b.slice(0, 4, "header")? != b"2LGA" {
            return Err(Ghoul2Error::Signature("GLA"));
        }
        let version = b.u32(4, "header")?;
        if version != 6 {
            return Err(Ghoul2Error::Version(version));
        }
        let name = b.name(8, "name")?;
        let scale = b.f32(72, "header")?;
        let frame_count = b.index(b.i32(76, "header")?, "frame count")?;
        let frames_at = b.index(b.i32(80, "header")?, "frame offset")?;
        let bone_count = b.index(b.i32(84, "header")?, "bone count")?;
        let pool_at = b.index(b.i32(88, "header")?, "pool offset")?;
        let end = b.index(b.i32(96, "header")?, "end offset")?;

        let matrix = |at: usize| -> Result<[[f32; 4]; 3], Ghoul2Error> {
            let mut m = [[0.0; 4]; 3];
            for (r, row) in m.iter_mut().enumerate() {
                for (c, cell) in row.iter_mut().enumerate() {
                    *cell = b.f32(at + (r * 4 + c) * 4, "bone matrix")?;
                }
            }
            Ok(m)
        };
        let mut bones = Vec::with_capacity(b.room(bone_count, 4));
        for i in 0..bone_count {
            let at = GLA_HEADER
                + b.index(b.i32(GLA_HEADER + i * 4, "skeleton table")?, "bone offset")?;
            let child_count = b.index(b.i32(at + 168, "bone")?, "child count")?;
            bones.push(Bone {
                name: b.name(at, "bone name")?,
                flags: b.u32(at + 64, "bone")?,
                parent: b.i32(at + 68, "bone")?,
                base_pose: matrix(at + 72)?,
                base_pose_inverse: matrix(at + 120)?,
                children: (0..child_count)
                    .map(|c| b.i32(at + 172 + c * 4, "bone children"))
                    .collect::<Result<_, _>>()?,
            });
        }

        roots_reached(&bones)?;
        if frame_count == 0 {
            return Err(Ghoul2Error::Bad("no frames".into()));
        }

        let index_bytes = b.slice(frames_at, frame_count * bone_count * 3, "frames")?;
        let frames: Vec<u32> = index_bytes
            .as_chunks::<3>()
            .0
            .iter()
            .map(|c| u32::from(c[0]) | u32::from(c[1]) << 8 | u32::from(c[2]) << 16)
            .collect();
        let pool_len = end
            .checked_sub(pool_at)
            .ok_or_else(|| Ghoul2Error::Bad("bone pool starts after the end".into()))?;
        let pool_bytes = b.slice(pool_at, pool_len - pool_len % 14, "bone pool")?;
        let pool: Vec<[u16; 7]> = pool_bytes
            .as_chunks::<14>()
            .0
            .iter()
            .map(|c| std::array::from_fn(|i| u16::from_le_bytes([c[i * 2], c[i * 2 + 1]])))
            .collect();
        if let Some(bad) = frames.iter().find(|&&index| index as usize >= pool.len()) {
            return Err(Ghoul2Error::Bad(format!(
                "frame index {bad} past the pool of {}",
                pool.len()
            )));
        }
        Ok(Gla {
            name,
            scale,
            bones,
            frame_count,
            frames,
            pool,
        })
    }
}

/// Checks that every bone's parent is a bone of the skeleton or negative (a
/// root), and that following parents from any bone reaches a root: readers
/// walk the chain.
fn roots_reached(bones: &[Bone]) -> Result<(), Ghoul2Error> {
    const UNSEEN: u8 = 0;
    const ON_PATH: u8 = 1;
    const ROOTED: u8 = 2;
    let mut state = vec![UNSEEN; bones.len()];
    let mut path = Vec::new();
    for start in 0..bones.len() {
        let mut bone = start;
        while state[bone] == UNSEEN {
            state[bone] = ON_PATH;
            path.push(bone);
            let Ok(parent) = usize::try_from(bones[bone].parent) else {
                break;
            };
            if parent >= bones.len() {
                return Err(Ghoul2Error::Bad(format!(
                    "bone {bone}'s parent {parent} of {}",
                    bones.len()
                )));
            }
            bone = parent;
        }
        if state[bone] == ON_PATH && usize::try_from(bones[bone].parent).is_ok() {
            return Err(Ghoul2Error::Bad(format!("bone {bone} is its own ancestor")));
        }
        for &placed in &path {
            state[placed] = ROOTED;
        }
        path.clear();
    }
    Ok(())
}

/// A pool entry as (rotation x, y, z, w) and translation: each rotation
/// value is `v / 16383 − 2`, stored w first; each translation value is
/// `v / 64 − 512`. This is how the engine's decompression reads them.
pub fn unpack_bone(entry: &[u16; 7]) -> ([f32; 4], [f32; 3]) {
    let r = |v: u16| f32::from(v) / 16383.0 - 2.0;
    let t = |v: u16| f32::from(v) / 64.0 - 512.0;
    (
        [r(entry[1]), r(entry[2]), r(entry[3]), r(entry[0])],
        [t(entry[4]), t(entry[5]), t(entry[6])],
    )
}
