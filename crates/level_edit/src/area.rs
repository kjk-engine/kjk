//! The content of an area that only kjk understands (`kjk-area.toml`):
//! Jedi Academy props and collision boxes. The original games never read
//! this file.
//!
//! ```toml
//! [[prop]]
//! name = "crate-1"
//! model = "models/map_objects/imperial/crate_h.md3"
//! position = [10.5, 2.0, 0.0]
//! yaw = 90.0
//! scale = 1.0
//! solid = true
//!
//! [[box]]
//! name = "ledge-1"
//! centre = [12.0, 4.0, 2.5]
//! size = [2.0, 1.0, 0.25]
//! yaw = 0.0
//! ```
//!
//! Positions and sizes are metres in KOTOR's space (Z up). `yaw` is degrees
//! from +X, turning left. A prop's model is a path inside Jedi Academy's
//! archives: the project names it and never holds a copy of it. A prop is
//! drawn at `scale` times its size in Jedi Academy, whose unit is
//! `collision::UNIT` (an inch). A solid prop's triangles are solid for every
//! body, as a room's are.

use collision::trig;

/// The area's kjk-only content.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct KjkArea {
    pub props: Vec<Prop>,
    pub boxes: Vec<CollisionBox>,
}

/// A Jedi Academy model placed in the area.
#[derive(Debug, Clone, PartialEq)]
pub struct Prop {
    pub name: String,
    /// The model's path in Jedi Academy's archives (an MD3), lowercase,
    /// with `/` between folders.
    pub model: String,
    pub position: [f32; 3],
    pub yaw: f32,
    pub scale: f32,
    pub solid: bool,
}

/// A box of invisible collision: solid for every body and every shot.
#[derive(Debug, Clone, PartialEq)]
pub struct CollisionBox {
    pub name: String,
    pub centre: [f32; 3],
    /// Its whole length along its own X, Y and Z, each above 0.
    pub size: [f32; 3],
    pub yaw: f32,
}

/// The smallest side a box may have, in metres: thinner than a triangle's
/// slab (1 cm) it would trace as nothing.
pub const SMALLEST_SIDE: f32 = 0.02;

/// The smallest and the largest scale a prop may be drawn at.
pub const SCALES: (f32, f32) = (0.05, 20.0);

impl CollisionBox {
    /// Its eight corners: bit 0 of the index picks the box's +X side, bit
    /// 1 its +Y side, bit 2 its +Z side.
    pub fn corners(&self) -> [[f32; 3]; 8] {
        let (sin, cos) = trig::sin_cos_deg(self.yaw);
        let half = self.size.map(|side| side * 0.5);
        std::array::from_fn(|at| {
            let local = [0, 1, 2].map(|axis| {
                if at & (1 << axis) != 0 {
                    half[axis]
                } else {
                    -half[axis]
                }
            });
            [
                self.centre[0] + local[0] * cos - local[1] * sin,
                self.centre[1] + local[0] * sin + local[1] * cos,
                self.centre[2] + local[2],
            ]
        })
    }

    /// Its corners, the normals of its six faces and its twelve edges, for
    /// a convex brush (`collision::LevelBuilder::convex`).
    pub fn hull(&self) -> collision::clip_file::Hull {
        let (sin, cos) = trig::sin_cos_deg(self.yaw);
        let x = [cos, sin, 0.0];
        let y = [-sin, cos, 0.0];
        let faces = vec![
            x,
            x.map(|c| -c),
            y,
            y.map(|c| -c),
            [0.0, 0.0, 1.0],
            [0.0, 0.0, -1.0],
        ];
        let mut edges = Vec::with_capacity(12);
        for a in 0..8usize {
            for bit in [1, 2, 4] {
                if a & bit == 0 {
                    edges.push((a, a | bit));
                }
            }
        }
        collision::clip_file::Hull {
            points: self.corners().to_vec(),
            faces,
            edges,
        }
    }

    /// Its twelve triangles, each wound to face out, for drawing.
    pub fn triangles(&self) -> Vec<[[f32; 3]; 3]> {
        let c = self.corners();
        // Each face by its four corners, counter-clockwise seen from
        // outside.
        let quads = [
            [1, 3, 7, 5],
            [0, 4, 6, 2],
            [2, 6, 7, 3],
            [0, 1, 5, 4],
            [4, 5, 7, 6],
            [0, 2, 3, 1],
        ];
        quads
            .iter()
            .flat_map(|[a, b, d, e]| [[c[*a], c[*b], c[*d]], [c[*a], c[*d], c[*e]]])
            .collect()
    }
}

impl Prop {
    /// A point of the model (Jedi Academy units, its own space) where the
    /// prop stands in the area: scaled, turned by its yaw, moved to its
    /// position.
    pub fn place(&self, point: [f32; 3]) -> [f32; 3] {
        let (sin, cos) = trig::sin_cos_deg(self.yaw);
        let scale = collision::UNIT * self.scale;
        let [x, y, z] = point.map(|value| value * scale);
        [
            self.position[0] + x * cos - y * sin,
            self.position[1] + x * sin + y * cos,
            self.position[2] + z,
        ]
    }

    /// The model's triangles where the prop stands, from its MD3's bytes:
    /// what a solid prop adds to the collision. A surface named as JA's
    /// tags are (`tag_…`) holds no geometry and is left out. An MD3 winds
    /// its triangles clockwise seen from outside; each is given here
    /// counter-clockwise, so its face's normal points out and the 1 cm slab
    /// the level makes of it lies inside the prop.
    pub fn triangles(&self, md3: &fmt_ja::md3::Md3) -> Vec<[[f32; 3]; 3]> {
        let mut out = Vec::new();
        for surface in &md3.surfaces {
            if surface.name.to_ascii_lowercase().starts_with("tag_") {
                continue;
            }
            for triangle in &surface.triangles {
                let corner = |at: u32| {
                    surface
                        .positions
                        .get(at as usize)
                        .map(|point| self.place(*point))
                };
                if let (Some(a), Some(b), Some(c)) = (
                    corner(triangle[0]),
                    corner(triangle[1]),
                    corner(triangle[2]),
                ) {
                    out.push([a, c, b]);
                }
            }
        }
        out
    }
}

/// Why `kjk-area.toml` does not read.
fn bad(what: impl std::fmt::Display) -> String {
    format!("kjk-area.toml: {what}")
}

fn number(item: Option<&toml_edit::Item>, what: &str) -> Result<f32, String> {
    let item = item.ok_or_else(|| bad(format!("{what} is missing")))?;
    item.as_float()
        .map(|value| value as f32)
        .or_else(|| item.as_integer().map(|value| value as f32))
        .filter(|value| value.is_finite())
        .ok_or_else(|| bad(format!("{what} is not a number")))
}

fn triple(item: Option<&toml_edit::Item>, what: &str) -> Result<[f32; 3], String> {
    let array = item
        .and_then(toml_edit::Item::as_array)
        .ok_or_else(|| bad(format!("{what} is not a list of three numbers")))?;
    let values: Vec<f32> = array
        .iter()
        .filter_map(|value| {
            value
                .as_float()
                .map(|value| value as f32)
                .or_else(|| value.as_integer().map(|value| value as f32))
        })
        .filter(|value| value.is_finite())
        .collect();
    <[f32; 3]>::try_from(values).map_err(|_| bad(format!("{what} is not a list of three numbers")))
}

fn text(item: Option<&toml_edit::Item>, what: &str) -> Result<String, String> {
    item.and_then(toml_edit::Item::as_str)
        .map(str::to_string)
        .ok_or_else(|| bad(format!("{what} is missing")))
}

impl KjkArea {
    /// Reads the file's text. Every entry must have all its fields.
    pub fn parse(source: &str) -> Result<KjkArea, String> {
        let document: toml_edit::DocumentMut = source.parse().map_err(bad)?;
        let mut area = KjkArea::default();
        let tables = |key: &str| {
            document
                .get(key)
                .and_then(toml_edit::Item::as_array_of_tables)
                .map(|tables| tables.iter().collect::<Vec<_>>())
                .unwrap_or_else(Vec::new)
        };
        for table in tables("prop") {
            area.props.push(Prop {
                name: text(table.get("name"), "a prop's name")?,
                model: text(table.get("model"), "a prop's model")?.to_ascii_lowercase(),
                position: triple(table.get("position"), "a prop's position")?,
                yaw: number(table.get("yaw"), "a prop's yaw")?,
                scale: number(table.get("scale"), "a prop's scale")?,
                solid: table
                    .get("solid")
                    .and_then(toml_edit::Item::as_bool)
                    .ok_or_else(|| bad("a prop's solid is not true or false"))?,
            });
        }
        for table in tables("box") {
            area.boxes.push(CollisionBox {
                name: text(table.get("name"), "a box's name")?,
                centre: triple(table.get("centre"), "a box's centre")?,
                size: triple(table.get("size"), "a box's size")?,
                yaw: number(table.get("yaw"), "a box's yaw")?,
            });
        }
        Ok(area)
    }

    /// The file's text. Numbers are written so they read back to the same
    /// bits.
    pub fn write(&self) -> String {
        let mut out = String::from(
            "# What only kjk understands in this area: Jedi Academy props and\n\
             # collision boxes. Written by kjk's level editor (docs/LEVEL-EDIT.md).\n",
        );
        let quote = |text: &str| format!("{text:?}");
        let triple = |v: [f32; 3]| format!("[{:?}, {:?}, {:?}]", v[0], v[1], v[2]);
        for prop in &self.props {
            out.push_str(&format!(
                "\n[[prop]]\nname = {}\nmodel = {}\nposition = {}\nyaw = {:?}\nscale = {:?}\nsolid = {}\n",
                quote(&prop.name),
                quote(&prop.model),
                triple(prop.position),
                prop.yaw,
                prop.scale,
                prop.solid,
            ));
        }
        for shape in &self.boxes {
            out.push_str(&format!(
                "\n[[box]]\nname = {}\ncentre = {}\nsize = {}\nyaw = {:?}\n",
                quote(&shape.name),
                triple(shape.centre),
                triple(shape.size),
                shape.yaw,
            ));
        }
        out
    }

    /// A name no prop or box has yet: `stem-N` for the smallest N from 1.
    pub fn free_name(&self, stem: &str) -> String {
        (1..)
            .map(|n| format!("{stem}-{n}"))
            .find(|name| {
                !self.props.iter().any(|prop| &prop.name == name)
                    && !self.boxes.iter().any(|shape| &shape.name == name)
            })
            .expect("some number is free")
    }
}
