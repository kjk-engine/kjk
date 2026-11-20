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
//!
//! `Walkmesh::write` lays a walkmesh out as the games' files are: the
//! header (136 bytes), then the vertices, the faces' vertices, materials,
//! normals and plane distances, and for an area walkmesh the AABB tree,
//! the adjacency, the edges and the perimeters. `kjk formats` checks that
//! every copy in an install writes back to the same bytes. An empty table's
//! offset is where it would start, as in the games' files.

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
    /// The header's word after the AABB tree's offset.
    pub aabb_word: u32,
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
    /// The word between the face and the split plane.
    pub word: u32,
    pub plane: u32,
    pub children: [u32; 2],
}

/// The header's length, where the vertices start.
const HEADER: usize = 136;

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
            aabb_word: if area { reader.u32(108)? } else { 0 },
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
                    word: reader.u32(at + 28)?,
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

    /// The walkmesh as a file (above).
    pub fn write(&self) -> Vec<u8> {
        let mut out = vec![0; HEADER];
        let put = |out: &mut Vec<u8>, at: usize, word: u32| {
            out[at..at + 4].copy_from_slice(&word.to_le_bytes());
        };
        // The offset at `at` is where the next table starts.
        let mark = |out: &mut Vec<u8>, at: usize| {
            let here = out.len() as u32;
            put(out, at, here);
        };
        out[..8].copy_from_slice(b"BWM V1.0");
        put(&mut out, 8, u32::from(self.area));
        for (at, value) in self
            .use_positions
            .iter()
            .chain(std::iter::once(&self.position))
            .flatten()
            .enumerate()
        {
            put(&mut out, 12 + at * 4, value.to_bits());
        }
        put(&mut out, 72, self.vertices.len() as u32);
        let float =
            |out: &mut Vec<u8>, value: f32| out.extend_from_slice(&value.to_bits().to_le_bytes());
        let word = |out: &mut Vec<u8>, value: u32| out.extend_from_slice(&value.to_le_bytes());
        mark(&mut out, 76);
        for vertex in &self.vertices {
            vertex.iter().for_each(|&value| float(&mut out, value));
        }
        put(&mut out, 80, self.faces.len() as u32);
        mark(&mut out, 84);
        for face in &self.faces {
            face.vertices
                .iter()
                .for_each(|&value| word(&mut out, value));
        }
        mark(&mut out, 88);
        for face in &self.faces {
            word(&mut out, face.material);
        }
        mark(&mut out, 92);
        for face in &self.faces {
            face.normal.iter().for_each(|&value| float(&mut out, value));
        }
        mark(&mut out, 96);
        for face in &self.faces {
            float(&mut out, face.distance);
        }
        if !self.area {
            return out;
        }
        put(&mut out, 100, self.aabb.len() as u32);
        mark(&mut out, 104);
        put(&mut out, 108, self.aabb_word);
        for entry in &self.aabb {
            entry
                .bounds
                .iter()
                .for_each(|&value| float(&mut out, value));
            word(&mut out, entry.face as u32);
            word(&mut out, entry.word);
            word(&mut out, entry.plane);
            entry
                .children
                .iter()
                .for_each(|&value| word(&mut out, value));
        }
        put(&mut out, 112, self.adjacency.len() as u32);
        mark(&mut out, 116);
        for row in &self.adjacency {
            row.iter().for_each(|&value| word(&mut out, value as u32));
        }
        put(&mut out, 120, self.edges.len() as u32);
        mark(&mut out, 124);
        for (edge, transition) in &self.edges {
            word(&mut out, *edge as u32);
            word(&mut out, *transition as u32);
        }
        put(&mut out, 128, self.perimeters.len() as u32);
        mark(&mut out, 132);
        for perimeter in &self.perimeters {
            word(&mut out, *perimeter);
        }
        out
    }

    /// This walkmesh moved by `by`: its vertices, its faces' planes, its
    /// AABB tree's boxes and its absolute use positions. A room's walkmesh
    /// is in the area's coordinates, so a room placed elsewhere needs its
    /// walkmesh moved with it. The edges' transitions name rooms of the
    /// layout the walkmesh was made for, so they are cleared.
    pub fn moved(&self, by: [f32; 3]) -> Walkmesh {
        let shift = |point: [f32; 3]| std::array::from_fn(|axis| point[axis] + by[axis]);
        let mut moved = self.clone();
        moved.vertices = self.vertices.iter().map(|&vertex| shift(vertex)).collect();
        for face in &mut moved.faces {
            // A point p is on the plane when normal · p + distance = 0.
            let along: f32 = (0..3).map(|axis| face.normal[axis] * by[axis]).sum();
            face.distance -= along;
        }
        for entry in &mut moved.aabb {
            for (at, bound) in entry.bounds.iter_mut().enumerate() {
                *bound += by[at % 3];
            }
        }
        for position in &mut moved.use_positions[2..] {
            *position = shift(*position);
        }
        for (_, transition) in &mut moved.edges {
            *transition = -1;
        }
        moved
    }
}
