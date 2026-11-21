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
//! role = "walkable"
//! ```
//!
//! A box's `role` says what it does (`BoxRole`); a box without one is
//! solid.
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
    /// The lights placed by hand (`lights`). A new map keeps them here as
    /// `[[light]]` tables; an installed level's are saved to
    /// `data/lights/fittings.toml` instead (`Project::save`).
    pub lights: Vec<crate::lights::PlacedLight>,
    /// The lamps placed by hand (`lamps`), kept as the lights are: a new
    /// map's here as `[[lamp]]` tables, an installed level's in
    /// `data/lights/fittings.toml`.
    pub lamps: Vec<crate::lamps::PlacedLamp>,
    /// The links between floors that walkers jump or drop across
    /// (`collision::nav::Link`), as `[[link]]` tables.
    pub links: Vec<NavLink>,
}

/// A way between two floors the navigation mesh does not join: a jump
/// (crossed either way) or a drop (from `from` down to `to` only), from
/// where a walker leaves to where it lands (metres, the feet).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct NavLink {
    pub kind: collision::nav::LinkKind,
    pub from: [f32; 3],
    pub to: [f32; 3],
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

/// A box the area adds, invisible in play: by its role, solid collision,
/// or a change to where the game's characters find their way.
#[derive(Debug, Clone, PartialEq)]
pub struct CollisionBox {
    pub name: String,
    pub centre: [f32; 3],
    /// Its whole length along its own X, Y and Z, each above 0.
    pub size: [f32; 3],
    pub yaw: f32,
    pub role: BoxRole,
}

/// What a box does. The navigation mesh is the floor the game's
/// characters find their way over (`docs/NAV.md`); a player walks on the
/// collision whatever the mesh holds.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum BoxRole {
    /// Solid for every body and every shot.
    #[default]
    Solid,
    /// Solid, and its top joins the navigation mesh, so characters walk
    /// over it: a platform, a step, a bridge.
    Walkable,
    /// Not solid. The navigation mesh under it is cut away, so characters
    /// go round it.
    NavBlock,
    /// Not solid. Its top joins the navigation mesh: floor for the
    /// characters where the walkmeshes leave a gap, such as between two
    /// rooms that do not meet edge to edge.
    NavFloor,
}

impl BoxRole {
    pub const ALL: [BoxRole; 4] = [
        BoxRole::Solid,
        BoxRole::Walkable,
        BoxRole::NavBlock,
        BoxRole::NavFloor,
    ];

    /// Its name in `kjk-area.toml` and in `edit role`.
    pub fn word(self) -> &'static str {
        match self {
            BoxRole::Solid => "solid",
            BoxRole::Walkable => "walkable",
            BoxRole::NavBlock => "block",
            BoxRole::NavFloor => "floor",
        }
    }

    pub fn from_word(word: &str) -> Option<BoxRole> {
        BoxRole::ALL.into_iter().find(|role| role.word() == word)
    }

    /// Whether bodies and shots meet it.
    pub fn solid(self) -> bool {
        matches!(self, BoxRole::Solid | BoxRole::Walkable)
    }
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

    /// Its top's two triangles, wound up.
    pub fn top(&self) -> [[[f32; 3]; 3]; 2] {
        let c = self.corners();
        [[c[4], c[5], c[7]], [c[4], c[7], c[6]]]
    }

    /// Whether `point` lies inside it.
    pub fn holds(&self, point: [f32; 3]) -> bool {
        let (sin, cos) = trig::sin_cos_deg(self.yaw);
        let d = [0, 1, 2].map(|axis| point[axis] - self.centre[axis]);
        let local = [d[0] * cos + d[1] * sin, -d[0] * sin + d[1] * cos, d[2]];
        (0..3).all(|axis| local[axis].abs() <= self.size[axis] * 0.5)
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
                role: match table.get("role") {
                    None => BoxRole::Solid,
                    Some(item) => item.as_str().and_then(BoxRole::from_word).ok_or_else(|| {
                        bad("a box's role is not solid, walkable, block or floor")
                    })?,
                },
            });
        }
        for table in tables("light") {
            area.lights
                .push(crate::lights::PlacedLight::read(table).map_err(bad)?);
        }
        for table in tables("lamp") {
            area.lamps
                .push(crate::lamps::PlacedLamp::read(table).map_err(bad)?);
        }
        for table in tables("link") {
            let kind = match text(table.get("kind"), "a link's kind")?.as_str() {
                "jump" => collision::nav::LinkKind::Jump,
                "drop" => collision::nav::LinkKind::Drop,
                _ => return Err(bad("a link's kind is not jump or drop")),
            };
            area.links.push(NavLink {
                kind,
                from: triple(table.get("from"), "a link's from")?,
                to: triple(table.get("to"), "a link's to")?,
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
            // A solid box is written as before boxes had roles.
            if shape.role != BoxRole::Solid {
                out.push_str(&format!("role = {}\n", quote(shape.role.word())));
            }
        }
        for light in &self.lights {
            out.push_str(&format!("\n[[light]]\n{}", light.table(&[])));
        }
        for lamp in &self.lamps {
            out.push_str(&format!("\n[[lamp]]\n{}", lamp.table(&[])));
        }
        for link in &self.links {
            let kind = match link.kind {
                collision::nav::LinkKind::Jump => "jump",
                collision::nav::LinkKind::Drop => "drop",
            };
            out.push_str(&format!(
                "\n[[link]]\nkind = {}\nfrom = {}\nto = {}\n",
                quote(kind),
                triple(link.from),
                triple(link.to),
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
