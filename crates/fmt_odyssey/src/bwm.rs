//! BWM V1.0 walkmeshes: `.wok` for area rooms, `.pwk` for placeables and
//! `.dwk` for doors.
//!
//! Header: `BWM V1.0`, type (1 for an area walkmesh, 0 otherwise), two
//! relative and two absolute use positions (3 floats each), the position,
//! then vertex count and offset, face count, and offsets of the face
//! indices, materials, normals and plane distances. Area walkmeshes add
//! counts and offsets of the AABB tree, one word, the adjacency, edges and
//! perimeters. Faces index vertices with u32s. An AABB node (44 bytes) is a
//! box, a face (−1 for an inner node), a word, the split plane and two child
//! indices.

use crate::FormatError;
use crate::reader::Reader;

const FORMAT: &str = "BWM";

#[derive(Debug, Clone, PartialEq)]
pub struct Walkmesh {
    /// True for an area room's walkmesh (`.wok`).
    pub area: bool,
    pub use_positions: [[f32; 3]; 4],
    pub position: [f32; 3],
    pub vertices: Vec<[f32; 3]>,
    pub faces: Vec<WalkFace>,
    pub aabb: Vec<AabbEntry>,
    /// Per walkable face, three edge neighbours (face × 3 + edge), or −1.
    pub adjacency: Vec<[i32; 3]>,
    /// (edge, transition) pairs on the walkmesh's outline.
    pub edges: Vec<(i32, i32)>,
    pub perimeters: Vec<u32>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct WalkFace {
    pub vertices: [u32; 3],
    /// Surface material, a row of `surfacemat.2da`.
    pub material: u32,
    pub normal: [f32; 3],
    pub distance: f32,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AabbEntry {
    pub bounds: [f32; 6],
    pub face: i32,
    pub plane: u32,
    pub children: [u32; 2],
}

impl Walkmesh {
    pub fn parse(bytes: &[u8]) -> Result<Walkmesh, FormatError> {
        let reader = Reader::new(FORMAT, bytes);
        reader.expect_tag(b"BWM V1.0")?;
        let f = |at: usize| -> Result<f32, FormatError> { Ok(f32::from_bits(reader.u32(at)?)) };
        let v3 =
            |at: usize| -> Result<[f32; 3], FormatError> { Ok([f(at)?, f(at + 4)?, f(at + 8)?]) };
        let area = reader.u32(8)? == 1;
        let use_positions = [v3(12)?, v3(24)?, v3(36)?, v3(48)?];
        let position = v3(60)?;
        let vertex_count = reader.u32(72)? as usize;
        let mut walkmesh = Walkmesh {
            area,
            use_positions,
            position,
            vertices: Vec::new(),
            faces: Vec::new(),
            aabb: Vec::new(),
            adjacency: Vec::new(),
            edges: Vec::new(),
            perimeters: Vec::new(),
        };
        if vertex_count == 0 {
            return Ok(walkmesh);
        }
        let vertices_at = reader.u32(76)? as usize;
        let face_count = reader.u32(80)? as usize;
        let indices_at = reader.u32(84)? as usize;
        let materials_at = reader.u32(88)? as usize;
        let normals_at = reader.u32(92)? as usize;
        let distances_at = reader.u32(96)? as usize;
        reader.table(vertices_at, vertex_count, 12)?;
        reader.table(indices_at, face_count, 12)?;
        walkmesh.vertices = (0..vertex_count)
            .map(|i| v3(vertices_at + i * 12))
            .collect::<Result<_, _>>()?;
        for i in 0..face_count {
            let vertices = [
                reader.u32(indices_at + i * 12)?,
                reader.u32(indices_at + i * 12 + 4)?,
                reader.u32(indices_at + i * 12 + 8)?,
            ];
            if vertices.iter().any(|&v| v as usize >= vertex_count) {
                return Err(FormatError::Unsupported {
                    format: FORMAT,
                    what: format!("face {i} indexes vertex {vertices:?} of {vertex_count}"),
                });
            }
            walkmesh.faces.push(WalkFace {
                vertices,
                material: reader.u32(materials_at + i * 4)?,
                normal: v3(normals_at + i * 12)?,
                distance: f(distances_at + i * 4)?,
            });
        }
        if area {
            let (aabb_count, aabb_at) = (reader.u32(100)? as usize, reader.u32(104)? as usize);
            let (adjacency_count, adjacency_at) =
                (reader.u32(112)? as usize, reader.u32(116)? as usize);
            let (edge_count, edges_at) = (reader.u32(120)? as usize, reader.u32(124)? as usize);
            let (perimeter_count, perimeters_at) =
                (reader.u32(128)? as usize, reader.u32(132)? as usize);
            for i in 0..aabb_count {
                let at = aabb_at + i * 44;
                walkmesh.aabb.push(AabbEntry {
                    bounds: [
                        f(at)?,
                        f(at + 4)?,
                        f(at + 8)?,
                        f(at + 12)?,
                        f(at + 16)?,
                        f(at + 20)?,
                    ],
                    face: reader.u32(at + 24)? as i32,
                    plane: reader.u32(at + 32)?,
                    children: [reader.u32(at + 36)?, reader.u32(at + 40)?],
                });
            }
            for i in 0..adjacency_count {
                let at = adjacency_at + i * 12;
                walkmesh.adjacency.push([
                    reader.u32(at)? as i32,
                    reader.u32(at + 4)? as i32,
                    reader.u32(at + 8)? as i32,
                ]);
            }
            for i in 0..edge_count {
                let at = edges_at + i * 8;
                walkmesh
                    .edges
                    .push((reader.u32(at)? as i32, reader.u32(at + 4)? as i32));
            }
            walkmesh.perimeters = (0..perimeter_count)
                .map(|i| reader.u32(perimeters_at + i * 4))
                .collect::<Result<_, _>>()?;
        }
        Ok(walkmesh)
    }
}
