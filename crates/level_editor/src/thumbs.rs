//! The asset picker's thumbnails: each placeable, creature and Jedi Academy
//! prop drawn small, once, on the CPU, and kept in kjk's store so later
//! runs read them at once.
//!
//! A thumbnail is drawn with a depth buffer, its textures sampled at the
//! nearest texel, and one light from above and in front with some ambient
//! light. It is a picture to find an asset by, not the game's look. Drawing
//! on the CPU needs no camera of its own in the scene, and works headless.

use std::collections::{BTreeMap, VecDeque};

use asset_odyssey::model::ModelIr;
use asset_odyssey::texture::TextureIr;
use bevy::math::{Mat4, Vec2, Vec3};
use fmt_odyssey::ResType;
use kjk_game::project_props::PropModel;
use kjk_game::viewer::Placed;

/// A thumbnail's side, pixels.
pub const SIDE: usize = 96;

/// Bump when what a thumbnail looks like changes: older ones are drawn
/// again.
const FORMAT: &str = "editor-thumbnail-v1";

/// What an asset is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum AssetKind {
    Placeable,
    Creature,
    Prop,
    /// A room of the install, by its model, for a new map.
    Room,
    /// A door template (`.utd`).
    Door,
    /// A lamp to place by hand (`lamps::LampAsset::key`).
    Lamp,
}

impl AssetKind {
    fn name(self) -> &'static str {
        match self {
            AssetKind::Placeable => "placeable",
            AssetKind::Creature => "creature",
            AssetKind::Prop => "prop",
            AssetKind::Room => "room",
            AssetKind::Door => "door",
            AssetKind::Lamp => "lamp",
        }
    }
}

/// An RGBA picture of `SIDE` by `SIDE`, rows from the top.
pub type Picture = Vec<u8>;

/// The thumbnails made so far, and those asked for.
#[derive(Default)]
pub struct Thumbnails {
    pub made: BTreeMap<(AssetKind, String), Option<Picture>>,
    wanted: VecDeque<(AssetKind, String)>,
}

impl Thumbnails {
    /// The thumbnail of an asset if it is made; asks for it otherwise.
    /// `None` inside means it could not be drawn.
    pub fn get(&mut self, kind: AssetKind, name: &str) -> Option<&Option<Picture>> {
        let key = (kind, name.to_string());
        if !self.made.contains_key(&key) && !self.wanted.contains(&key) {
            self.wanted.push_back(key.clone());
        }
        self.made.get(&key)
    }

    /// Forgets what was asked for and not made: the picker shows other
    /// tiles now.
    pub fn forget_wanted(&mut self) {
        self.wanted.clear();
    }

    /// The next asset asked for.
    pub fn next_wanted(&mut self) -> Option<(AssetKind, String)> {
        self.wanted.pop_front()
    }
}

/// A triangle to draw: corners, texture coordinates, which texture.
struct Face {
    corners: [Vec3; 3],
    uvs: [Vec2; 3],
    texture: Option<usize>,
}

/// A texture decoded to RGBA, its first mip.
struct Texels {
    width: usize,
    height: usize,
    rgba: Vec<u8>,
    alpha: bool,
}

impl Texels {
    fn of(texture: &TextureIr) -> Option<Texels> {
        let decoded = texture.decoded();
        let rgba = decoded.mips.into_iter().next()?;
        let (width, height) = (decoded.width as usize, decoded.height as usize);
        // A flip-book's frames lie down the texture: the first is its top.
        let height = match texture.facts.strip {
            Some(strip) if strip.frames > 0 => height / strip.frames as usize,
            _ => height,
        };
        (width > 0 && height > 0 && rgba.len() >= width * height * 4).then_some(Texels {
            width,
            height,
            rgba,
            alpha: texture.facts.alpha,
        })
    }

    fn sample(&self, uv: Vec2) -> [u8; 4] {
        let x = (uv.x.rem_euclid(1.0) * self.width as f32) as usize;
        let y = (uv.y.rem_euclid(1.0) * self.height as f32) as usize;
        let at = (y.min(self.height - 1) * self.width + x.min(self.width - 1)) * 4;
        [
            self.rgba[at],
            self.rgba[at + 1],
            self.rgba[at + 2],
            self.rgba[at + 3],
        ]
    }
}

/// The faces of placed models, each in the space the models are placed in.
/// A model hooked to another's node (a head on its body's `headhook`, a
/// weapon in a hand) stands where that node rests.
fn faces(models: &[Placed], texture_of: &mut dyn FnMut(&str) -> Option<usize>) -> Vec<Face> {
    let mut out = Vec::new();
    for placed in models {
        let frame = match placed.hook {
            Some((body, node)) => {
                let Some(body) = models.get(body) else {
                    continue;
                };
                let Some(node) = body.ir.nodes.get(node) else {
                    continue;
                };
                let local = placed
                    .hook_local
                    .as_ref()
                    .map_or(Mat4::IDENTITY, kjk_game::scene::to_mat4);
                kjk_game::scene::placement(body) * kjk_game::scene::to_mat4(&node.world) * local
            }
            None => kjk_game::scene::placement(placed),
        };
        add_model(&placed.ir, frame, texture_of, &mut out);
    }
    out
}

fn add_model(
    ir: &ModelIr,
    frame: Mat4,
    texture_of: &mut dyn FnMut(&str) -> Option<usize>,
    out: &mut Vec<Face>,
) {
    for mesh in &ir.meshes {
        let Some(node) = ir.nodes.get(mesh.node) else {
            continue;
        };
        // A mesh drawn see-through by its alpha controller at rest, or
        // hidden at scale 0, adds nothing.
        if mesh.alpha <= 0.01 {
            continue;
        }
        let matrix = frame * kjk_game::scene::to_mat4(&node.world);
        let texture = mesh.texture.as_deref().and_then(&mut *texture_of);
        for triangle in mesh.indices.as_chunks::<3>().0 {
            let at = |i: u32| mesh.positions.get(i as usize).copied();
            let (Some(a), Some(b), Some(c)) = (at(triangle[0]), at(triangle[1]), at(triangle[2]))
            else {
                continue;
            };
            let uv = |i: u32| {
                mesh.uv1
                    .get(i as usize)
                    .map_or(Vec2::ZERO, |uv| Vec2::from_array(*uv))
            };
            out.push(Face {
                corners: [a, b, c].map(|p| matrix.transform_point3(Vec3::from_array(p))),
                uvs: [uv(triangle[0]), uv(triangle[1]), uv(triangle[2])],
                texture,
            });
        }
    }
}

/// `faces` drawn into a picture, framed to fit, seen from in front, to one
/// side and a little above. The models' space is Z up.
fn draw(faces: &[Face], textures: &[Texels]) -> Option<Picture> {
    draw_from(faces, textures, Vec3::new(1.0, -1.2, 0.8))
}

/// `draw` seen from `eye`'s way (from the model's middle).
fn draw_from(faces: &[Face], textures: &[Texels], eye: Vec3) -> Option<Picture> {
    let (low, high) = faces.iter().flat_map(|face| face.corners).fold(
        None,
        |span: Option<(Vec3, Vec3)>, p| match span {
            None => Some((p, p)),
            Some((low, high)) => Some((low.min(p), high.max(p))),
        },
    )?;
    let centre = (low + high) * 0.5;
    let radius = ((high - low).length() * 0.5).max(0.01);
    // Looking from `eye`'s side, toward the centre.
    let eye_direction = eye.normalize();
    let view = Mat4::look_at_rh(centre + eye_direction * radius * 4.0, centre, Vec3::Z);
    let light = Vec3::new(0.4, -0.7, 1.0).normalize();
    let mut colour = vec![0u8; SIDE * SIDE * 4];
    let mut depth = vec![f32::INFINITY; SIDE * SIDE];
    let scale = SIDE as f32 * 0.5 / (radius * 1.05);
    let to_screen = |p: Vec3| {
        let v = view.transform_point3(p);
        Vec3::new(
            SIDE as f32 * 0.5 + v.x * scale,
            SIDE as f32 * 0.5 - v.y * scale,
            -v.z,
        )
    };
    for face in faces {
        let normal = (face.corners[1] - face.corners[0])
            .cross(face.corners[2] - face.corners[0])
            .normalize_or_zero();
        // Both sides are drawn: a model's winding is not known here.
        let lit = 0.45 + 0.55 * normal.dot(light).abs();
        let s = face.corners.map(to_screen);
        let area = (s[1].x - s[0].x) * (s[2].y - s[0].y) - (s[2].x - s[0].x) * (s[1].y - s[0].y);
        if area.abs() < 1e-6 {
            continue;
        }
        let min_x = s
            .iter()
            .map(|p| p.x)
            .fold(f32::INFINITY, f32::min)
            .floor()
            .max(0.0) as usize;
        let max_x = s
            .iter()
            .map(|p| p.x)
            .fold(f32::NEG_INFINITY, f32::max)
            .ceil()
            .min(SIDE as f32 - 1.0);
        let min_y = s
            .iter()
            .map(|p| p.y)
            .fold(f32::INFINITY, f32::min)
            .floor()
            .max(0.0) as usize;
        let max_y = s
            .iter()
            .map(|p| p.y)
            .fold(f32::NEG_INFINITY, f32::max)
            .ceil()
            .min(SIDE as f32 - 1.0);
        if max_x < 0.0 || max_y < 0.0 {
            continue;
        }
        for y in min_y..=max_y as usize {
            for x in min_x..=max_x as usize {
                let (px, py) = (x as f32 + 0.5, y as f32 + 0.5);
                let w0 = ((s[1].x - px) * (s[2].y - py) - (s[2].x - px) * (s[1].y - py)) / area;
                let w1 = ((s[2].x - px) * (s[0].y - py) - (s[0].x - px) * (s[2].y - py)) / area;
                let w2 = 1.0 - w0 - w1;
                if w0 < 0.0 || w1 < 0.0 || w2 < 0.0 {
                    continue;
                }
                let z = w0 * s[0].z + w1 * s[1].z + w2 * s[2].z;
                let at = y * SIDE + x;
                if z >= depth[at] {
                    continue;
                }
                let texel = match face.texture.and_then(|t| textures.get(t)) {
                    Some(texels) => {
                        let uv = face.uvs[0] * w0 + face.uvs[1] * w1 + face.uvs[2] * w2;
                        let texel = texels.sample(uv);
                        if texels.alpha && texel[3] < 128 {
                            continue;
                        }
                        texel
                    }
                    None => [180, 180, 180, 255],
                };
                depth[at] = z;
                for channel in 0..3 {
                    colour[at * 4 + channel] = (f32::from(texel[channel]) * lit).min(255.0) as u8;
                }
                colour[at * 4 + 3] = 255;
            }
        }
    }
    Some(colour)
}

/// A picture of placed KOTOR models, their textures read from `resolver`.
pub fn of_models(resolver: &vfs::resolver::Resolver, models: &[Placed]) -> Option<Picture> {
    let mut textures: Vec<Texels> = Vec::new();
    let mut named: BTreeMap<String, Option<usize>> = BTreeMap::new();
    let mut texture_of = |name: &str| {
        *named.entry(name.to_string()).or_insert_with(|| {
            let texels = kjk_game::viewer::texture(resolver, name).and_then(|t| Texels::of(&t))?;
            textures.push(texels);
            Some(textures.len() - 1)
        })
    };
    let faces = faces(models, &mut texture_of);
    draw(&faces, &textures)
}

/// A picture of a room, seen from high above with the faces that look down
/// (its ceiling) left out, so its floor shows.
pub fn of_room(resolver: &vfs::resolver::Resolver, room: &Placed) -> Option<Picture> {
    let mut textures: Vec<Texels> = Vec::new();
    let mut named: BTreeMap<String, Option<usize>> = BTreeMap::new();
    let mut texture_of = |name: &str| {
        *named.entry(name.to_string()).or_insert_with(|| {
            let texels = kjk_game::viewer::texture(resolver, name).and_then(|t| Texels::of(&t))?;
            textures.push(texels);
            Some(textures.len() - 1)
        })
    };
    let faces: Vec<Face> = faces(std::slice::from_ref(room), &mut texture_of)
        .into_iter()
        .filter(|face| {
            let normal = (face.corners[1] - face.corners[0])
                .cross(face.corners[2] - face.corners[0])
                .normalize_or_zero();
            normal.z > -0.3
        })
        .collect();
    draw_from(&faces, &textures, Vec3::new(0.15, -0.25, 1.0))
}

/// A picture of a Jedi Academy prop.
pub fn of_prop(model: &PropModel) -> Option<Picture> {
    let mut textures: Vec<Texels> = Vec::new();
    let mut named: BTreeMap<String, Option<usize>> = BTreeMap::new();
    let mut texture_of = |name: &str| {
        *named.entry(name.to_string()).or_insert_with(|| {
            let texels = Texels::of(model.textures.get(name)?)?;
            textures.push(texels);
            Some(textures.len() - 1)
        })
    };
    let placed = [Placed::at(model.ir.clone(), [0.0; 3])];
    let faces = faces(&placed, &mut texture_of);
    draw(&faces, &textures)
}

/// The model a placeable template is drawn with: its `Appearance` row's
/// `modelname` in `placeables.2da`.
pub fn placeable_model(resolver: &vfs::resolver::Resolver, template: &str) -> Option<String> {
    let bytes = resolver.read(template, ResType::UTP)?;
    let gff = fmt_odyssey::gff::Gff::parse(&bytes).ok()?;
    let row = gff.root.uint("Appearance")?;
    let table = resolver.two_da("placeables")?;
    table.resref(row as usize, "modelname")
}

/// The model a door template is drawn with: its `GenericType` row's
/// `modelname` in `genericdoors.2da` (KOTOR's doors have appearance 0),
/// else its `Appearance` row's in `doortypes.2da`, as the module reader
/// finds it.
pub fn door_model(resolver: &vfs::resolver::Resolver, template: &str) -> Option<String> {
    let bytes = resolver.read(template, ResType::UTD)?;
    let gff = fmt_odyssey::gff::Gff::parse(&bytes).ok()?;
    let (table, row) = match gff.root.uint("Appearance").unwrap_or(0) {
        0 => (
            resolver.two_da("genericdoors")?,
            gff.root.uint("GenericType")?,
        ),
        appearance => (resolver.two_da("doortypes")?, appearance),
    };
    table
        .resref(row as usize, "modelname")
        .or_else(|| table.resref(row as usize, "model"))
}

/// The store's key for an asset's thumbnail: what it is, and the bytes it
/// is made from (its template, or a prop's model), so a changed file is
/// drawn again.
pub fn key(game: &str, kind: AssetKind, name: &str, source: &[u8]) -> store::Key {
    store::Key::of(
        FORMAT,
        &[
            game.as_bytes(),
            kind.name().as_bytes(),
            name.as_bytes(),
            source,
        ],
    )
}

/// Where thumbnails are kept in the store.
pub const STORE_KIND: &str = "editor/thumbnails";
