//! The fingerprint of a bake: which game files a set of baked lightmaps was
//! made from, so that the game shows a bake only over those files.
//!
//! Players run the games unpatched, KOTOR 2 with TSLRCM, KOTOR 1 with the K1
//! Community Patch, or with other mods. TSLRCM changes the placeables of
//! about 11 areas and the rooms of 2. The K1 Community Patch changes the
//! rooms' geometry or the layout of their lightmaps in 18 areas, and the
//! placeables of about 37 modules. A bake made over one set of files would
//! be drawn wrongly over another.
//!
//! The fingerprint has three parts that every set has.
//!
//! - `rooms` is a digest of what the lightmaps depend on. That is the
//!   area's layout (each room's model and its place), each room model's
//!   MDL and MDX, and each stored lightmap that the rooms name, with its
//!   TXI. The game reads the stored lightmaps for the fill and for the
//!   levels of the fitted lights, so a changed stored lightmap changes the
//!   bake too.
//! - `placeables` is a digest of the placeables the bake lights at their
//!   vertices or lets block its rays. These are the placeables with a mesh
//!   that has no skin and does not dangle (`placeable_light::lit_meshes`),
//!   in the area's order. Each one counts with its number among the area's
//!   instances, its model's name, its place and its bearing as stored, and
//!   its model's MDL and MDX.
//! - `format` is `FORMAT`, a number that is raised by hand when the baker's
//!   files change their meaning.
//!
//! A set stamped by this kjk also has the exact lines (`Exact`). They judge
//! each baked lightmap and each lit placeable by itself, so that a mod which
//! changes one room, or adds one crate, does not turn the whole bake away.
//!
//! - Each lightmap the set holds a PNG of has a digest of the triangles that
//!   draw it (`lightmap_digests`). That is every room mesh that names the
//!   lightmap, has no skin, is not hung under a reference node, and has a
//!   lightmap coordinate for each vertex. Each corner counts with its place
//!   in the area and its lightmap coordinate. The digest leaves out the
//!   file bytes, the model and node names, the vertex order, how the meshes
//!   are split, the normals and the textures.
//! - The lightmaps the bake knew and left alone are the backdrops. The game
//!   draws them as it always has.
//! - Each lit placeable the bake lit is listed with its number among the
//!   area's instances, its model, its place and its direction as stored.
//!   Each lit placeable model has one digest of its lit meshes
//!   (`model_digest`).
//!
//! kjk v0.21.0 reads `area`, `format`, `rooms` and `placeables` and skips
//! every other line, so a set with the exact lines still reads there.
//!
//! Every file is the one the games would read, after the game folder,
//! `override/` and the Steam Workshop items have been ranked. Scripts,
//! dialogue, sounds and the diffuse textures are left out, because the bake
//! does not depend on them, or because a texture mod would then turn every
//! bake away. Nothing depends on where the files lie, on their times, or on
//! the machine. The exact digests use plain `f32` sums and products in a
//! fixed order, so macOS and Windows give the same digests.
//!
//! The baker writes the fingerprint into `FILE` in each set it writes, and
//! `kjk detail-bake <module> --stamp <folder>` writes it into a set baked
//! earlier. The game computes the fingerprint when an area loads under the
//! bake and compares the two (`verdict`, and `fit` for the exact lines).

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;
use std::path::Path;

use asset_odyssey::model::{Matrix, ModelIr};
use asset_odyssey::module::{AreaIr, Direction, Instance, Kind, RoomIr};
use fmt_odyssey::ResType;
use vfs::resolver::Resolver;

/// The file in a bake's set that holds its fingerprint.
pub const FILE: &str = "fingerprint.txt";

/// The meaning of the bake's files. Raise it by hand when the baker writes
/// something the game must read differently, so that every older bake is
/// turned away until it is baked again.
///
/// Format 2 (23 November 2026): `placeables.txt`, `probes.txt` and
/// `probes-sun.txt` hold their light without the fill from the stored
/// lightmaps, and name the stored texel the game reads that fill from
/// (`stored_fill`). They carry this number in a `format` line too, so that
/// a set without `fingerprint.txt` is checked as well.
pub const FORMAT: u32 = 2;

/// The version of the exact lines (`Exact`), written in their `exact` line.
/// A set whose exact lines are of another version is judged by `rooms` and
/// `placeables` alone.
pub const EXACT: u32 = 1;

/// The fingerprint of one area's files, or of the files a bake was made
/// from. The digests are hexadecimal.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Fingerprint {
    /// The area's name (its ARE), in lower case.
    pub area: String,
    pub format: u32,
    pub rooms: String,
    pub placeables: String,
    /// The exact lines. `None` for a set stamped by kjk v0.21.0 or earlier,
    /// which the game judges by `rooms` and `placeables` alone.
    pub exact: Option<Exact>,
}

impl Fingerprint {
    /// Whether `other` has the same area, format, `rooms` and `placeables`,
    /// whatever exact lines either has.
    pub fn same_digests(&self, other: &Fingerprint) -> bool {
        (self.area == other.area)
            && (self.format == other.format)
            && (self.rooms == other.rooms)
            && (self.placeables == other.placeables)
    }
}

/// What a set records of each baked lightmap and each lit placeable, so
/// that the game can judge them one by one (`fit`).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Exact {
    /// The lightmaps the set holds a PNG or KLM of, by name in lower case, each
    /// with the exact digest of the room triangles that draw it.
    pub lightmaps: BTreeMap<String, String>,
    /// Quantized geometry-tolerant digests of the lightmaps (`lightmap-approx`).
    pub lightmaps_approx: BTreeMap<String, String>,
    /// The lightmaps of the area's rooms that the bake knew and left alone,
    /// by name in lower case. The `stored.txt` lightmaps are among them.
    pub backdrops: BTreeSet<String>,
    /// The lit placeables the bake lit, in the order of the area's
    /// instances.
    pub placed: Vec<Placed>,
    /// The digest of each lit placeable model, by name in lower case.
    pub models: BTreeMap<String, String>,
    /// Quantized geometry-tolerant digests of each lit placeable model (`model-approx`).
    pub models_approx: BTreeMap<String, String>,
}

/// A lit placeable as the area places it. The numbers are kept as their
/// bits, with -0 written as 0, so that two places are the same only when
/// every number is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Placed {
    /// Its number among the area's instances, as `placeables.txt` names it.
    pub instance: usize,
    /// Its model's name, in lower case.
    pub model: String,
    pub position: [u32; 3],
    pub facing: Facing,
}

/// A placeable's direction as the area stores it, as bits (`Placed`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Facing {
    /// Radians about Z.
    Bearing(u32),
    /// The X and Y of the direction it faces.
    Vector([u32; 2]),
}

/// What the game may take from a bake whose fingerprint was compared with
/// the player's files.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Verdict {
    /// Everything matches. The game takes the whole bake.
    Whole,
    /// The rooms match, but the placeables do not. The game takes the
    /// lightmaps, and the placeables keep the light probes.
    LightmapsOnly,
    /// The rooms or the format differ. The game takes nothing from the
    /// bake. The text says why.
    Refused(String),
}

/// The bits of `number`, with -0 written as 0.
fn bits(number: f32) -> u32 {
    if number == 0.0 { 0 } else { number.to_bits() }
}

/// The number whose bits `bits` gives.
fn number(bits: u32) -> f32 {
    f32::from_bits(bits)
}

/// A digest fed with fields that each say how long they are, so that two
/// different lists of fields never give the same stream of bytes.
struct Digest(blake3::Hasher);

impl Digest {
    fn new(kind: &str) -> Digest {
        let mut digest = Digest(blake3::Hasher::new());
        digest.text(kind);
        digest
    }

    fn text(&mut self, text: &str) {
        self.0.update(&(text.len() as u64).to_le_bytes());
        self.0.update(text.as_bytes());
    }

    /// A file's bytes, or a mark that the file is missing.
    fn file(&mut self, bytes: Option<&[u8]>) {
        match bytes {
            Some(bytes) => {
                self.0.update(&[1]);
                self.0.update(&(bytes.len() as u64).to_le_bytes());
                self.0.update(bytes);
            }
            None => {
                self.0.update(&[0]);
            }
        }
    }

    /// Numbers by their bits as stored, as the `rooms` and `placeables`
    /// digests have always read them.
    fn numbers(&mut self, numbers: &[f32]) {
        for number in numbers {
            self.0.update(&number.to_bits().to_le_bytes());
        }
    }

    /// Bits from `bits`, as the exact digests read numbers.
    fn bits(&mut self, bits: &[u32]) {
        for bits in bits {
            self.0.update(&bits.to_le_bytes());
        }
    }

    /// Quantized integers from `ints`, for tolerant geometry digests.
    fn ints(&mut self, ints: &[i32]) {
        for value in ints {
            self.0.update(&value.to_le_bytes());
        }
    }

    fn count(&mut self, count: u64) {
        self.0.update(&count.to_le_bytes());
    }

    /// The first 128 bits, in hexadecimal.
    fn finish(self) -> String {
        self.0.finalize().to_hex()[..32].to_string()
    }
}

/// A model's MDL and MDX as the games would read them.
fn model_files(digest: &mut Digest, resolver: &Resolver, name: &str) {
    digest.text(name);
    digest.file(resolver.read(name, ResType::MDL).as_deref());
    digest.file(resolver.read(name, ResType::MDX).as_deref());
}

/// A model as the game reads it (`viewer::parsed`), or `None` when it is
/// missing or does not read.
fn model_ir(resolver: &Resolver, name: &str) -> Option<ModelIr> {
    crate::viewer::parsed(resolver, name)
        .ok()
        .flatten()
        .map(|parsed| ModelIr::from_model(&parsed))
}

/// The room models of `area`, by name in lower case, each read once. A
/// model that is missing or does not read is `None`.
type RoomModels = BTreeMap<String, Option<ModelIr>>;

fn room_models(resolver: &Resolver, area: &AreaIr) -> RoomModels {
    (area.rooms.iter())
        .map(|room| room.model.to_ascii_lowercase())
        .collect::<BTreeSet<String>>()
        .into_iter()
        .map(|name| {
            let ir = model_ir(resolver, &name);
            (name, ir)
        })
        .collect()
}

/// The stored lightmaps that any mesh of `models` names, in lower case.
fn named_lightmaps(models: &RoomModels) -> BTreeSet<String> {
    (models.values().flatten())
        .flat_map(|ir| ir.meshes.iter().filter_map(|mesh| mesh.lightmap.as_deref()))
        .map(str::to_ascii_lowercase)
        .collect()
}

/// The fingerprint of area `area` in the files that `resolver` reads, with
/// no exact lines.
pub fn of_area(resolver: &Resolver, area: &AreaIr) -> Fingerprint {
    let models = room_models(resolver, area);
    let (lit, lit_models) = lit_placeables(resolver, area);
    Fingerprint {
        area: area.resref.to_ascii_lowercase(),
        format: FORMAT,
        rooms: rooms(resolver, area, &models),
        placeables: placeables(resolver, &lit, &lit_models),
        exact: None,
    }
}

/// The digest of the layout, the room models and their stored lightmaps.
fn rooms(resolver: &Resolver, area: &AreaIr, models: &RoomModels) -> String {
    let mut digest = Digest::new("rooms");
    digest.count(area.rooms.len() as u64);
    for room in &area.rooms {
        digest.text(&room.model.to_ascii_lowercase());
        digest.numbers(&room.position);
    }
    for name in models.keys() {
        model_files(&mut digest, resolver, name);
    }
    // Each stored lightmap as the game picks it: a TPC or a TGA, whichever
    // the highest layer holds, with its TXI.
    for name in &named_lightmaps(models) {
        digest.text(name);
        match resolver.find_any(name, &[ResType::TPC, ResType::TGA]) {
            Some((restype, found)) => {
                digest.text(if restype == ResType::TPC {
                    "tpc"
                } else {
                    "tga"
                });
                digest.file(found.location.read().ok().as_deref());
            }
            None => digest.text("missing"),
        }
        digest.file(resolver.read(name, ResType::TXI).as_deref());
    }
    digest.finish()
}

/// The mod that a layer of the resolver belongs to, named as
/// `lighting_bakes::place` reads it: `workshop/<id>` for a Steam Workshop
/// item's files, `override` for the game's override folder and `modules`
/// for a `.mod` in the game's modules folder. `None` for the game's own
/// files.
pub fn mod_place(layer: &str) -> Option<String> {
    if let Some(rest) = layer.strip_prefix("workshop/") {
        let id = rest.split('/').next().filter(|id| !id.is_empty())?;
        Some(format!("workshop/{id}"))
    } else if layer == "override" || layer.starts_with("override/") {
        Some("override".to_string())
    } else if layer.starts_with("modules/") && layer.ends_with(".mod") {
        Some("modules".to_string())
    } else {
        None
    }
}

/// The mods whose files replace the game's own among the files that the
/// `rooms` digest of `area` reads: the room models' MDL and MDX, and the
/// stored lightmaps with their TXI (`mod_place`).
pub fn room_mods(resolver: &Resolver, area: &AreaIr) -> BTreeSet<String> {
    let models = room_models(resolver, area);
    let lightmaps = named_lightmaps(&models);
    let models = (models.keys())
        .flat_map(|name| [ResType::MDL, ResType::MDX].map(|restype| resolver.find(name, restype)));
    let lightmaps = lightmaps.iter().flat_map(|name| {
        [
            (resolver.find_any(name, &[ResType::TPC, ResType::TGA])).map(|(_, found)| found),
            resolver.find(name, ResType::TXI),
        ]
    });
    (models.chain(lightmaps))
        .flatten()
        .filter_map(|found| mod_place(&found.layer.name))
        .collect()
}

/// A lit placeable: its number among the area's instances, its model's
/// name in lower case and the instance.
type Lit<'a> = (usize, String, &'a Instance);

/// Each lit placeable model, by name in lower case.
type LitModels = BTreeMap<String, ModelIr>;

/// The placeables the bake lights or lets block its rays, each with its
/// number among the area's instances and its model's name in lower case,
/// in the order of the area's instances, as `still_placeables` of the baker
/// and `module_scene` of the game find them. Beside them, each of their
/// models, read once.
fn lit_placeables<'a>(resolver: &Resolver, area: &'a AreaIr) -> (Vec<Lit<'a>>, LitModels) {
    let mut read: BTreeMap<String, Option<ModelIr>> = BTreeMap::new();
    let mut lit = Vec::new();
    for (object, instance) in area.instances.iter().enumerate() {
        if instance.kind != Kind::Placeable {
            continue;
        }
        let Some(name) = instance.model.as_deref().map(str::to_ascii_lowercase) else {
            continue;
        };
        let ir = read.entry(name.clone()).or_insert_with(|| {
            model_ir(resolver, &name).filter(|ir| {
                crate::placeable_light::lit_meshes(ir)
                    .iter()
                    .any(|lit| *lit)
            })
        });
        if ir.is_some() {
            lit.push((object, name, instance));
        }
    }
    let models = (read.into_iter())
        .filter_map(|(name, ir)| Some((name, ir?)))
        .collect();
    (lit, models)
}

/// The digest of the lit placeables `lit` and their models `models`.
fn placeables(resolver: &Resolver, lit: &[Lit], models: &LitModels) -> String {
    let mut digest = Digest::new("placeables");
    for (object, name, instance) in lit {
        digest.count(*object as u64);
        digest.text(name);
        digest.numbers(&instance.position);
        match instance.direction {
            Direction::Bearing(bearing) => {
                digest.text("bearing");
                digest.numbers(&[bearing]);
            }
            Direction::Vector(vector) => {
                digest.text("vector");
                digest.numbers(&vector);
            }
        }
    }
    for name in models.keys() {
        model_files(&mut digest, resolver, name);
    }
    digest.finish()
}

/// A lit placeable as `Placed` keeps it.
fn placed(object: usize, name: &str, instance: &Instance) -> Placed {
    Placed {
        instance: object,
        model: name.to_string(),
        position: instance.position.map(bits),
        facing: match instance.direction {
            Direction::Bearing(bearing) => Facing::Bearing(bits(bearing)),
            Direction::Vector(vector) => Facing::Vector(vector.map(bits)),
        },
    }
}

/// `m` times the point `p`, in plain `f32`, each row summed from left to
/// right.
fn apply(m: &Matrix, p: [f32; 3]) -> [f32; 3] {
    std::array::from_fn(|row| m[row][0] * p[0] + m[row][1] * p[1] + m[row][2] * p[2] + m[row][3])
}

/// A triangle's corner as the exact digest reads it: its place in the area
/// and its lightmap coordinate, as bits.
type Corner = [u32; 5];

/// The triangle `corners` turned so that its smallest corner comes first.
/// The order of its corners round the triangle stays.
fn turned(corners: [Corner; 3]) -> [Corner; 3] {
    let first = (0..3).min_by_key(|&i| corners[i]).unwrap_or(0);
    std::array::from_fn(|i| corners[(first + i) % 3])
}

/// The digest of the triangles `triangles` of one lightmap, in any order.
fn triangles_digest(mut triangles: Vec<[Corner; 3]>) -> String {
    triangles.sort_unstable();
    let mut digest = Digest::new("lightmap");
    digest.count(triangles.len() as u64);
    for triangle in &triangles {
        for corner in triangle {
            digest.bits(corner);
        }
    }
    digest.finish()
}

/// The digest of a lightmap that no room mesh draws.
pub fn undrawn_digest() -> String {
    triangles_digest(Vec::new())
}

/// The triangles of each lightmap that the rooms of `area` draw, in the
/// area, and the room models that draw each one. A mesh counts when it is
/// not hung under a reference node, has no skin, names a lightmap and has a
/// lightmap coordinate for each vertex, as the renderer and the baker take
/// it. Each corner is placed by its node's rest place, which holds the
/// ancestors' scale, and then by its room's place.
fn lightmap_triangles(
    rooms: &[RoomIr],
    models: &RoomModels,
) -> BTreeMap<String, (Vec<[Corner; 3]>, BTreeSet<String>)> {
    let mut found: BTreeMap<String, (Vec<[Corner; 3]>, BTreeSet<String>)> = BTreeMap::new();
    for room in rooms {
        let model = room.model.to_ascii_lowercase();
        let Some(Some(ir)) = models.get(&model) else {
            continue;
        };
        for mesh in &ir.meshes {
            let Some(lightmap) = mesh.lightmap.as_deref() else {
                continue;
            };
            if mesh.referenced || mesh.skin.is_some() || mesh.uv2.len() != mesh.positions.len() {
                continue;
            }
            let Some(node) = ir.nodes.get(mesh.node) else {
                continue;
            };
            let corner = |index: u32| -> Option<Corner> {
                let index = index as usize;
                let at = apply(&node.world, *mesh.positions.get(index)?);
                let uv = mesh.uv2.get(index)?;
                Some([
                    bits(at[0] + room.position[0]),
                    bits(at[1] + room.position[1]),
                    bits(at[2] + room.position[2]),
                    bits(uv[0]),
                    bits(uv[1]),
                ])
            };
            let (triangles, rooms) = found.entry(lightmap.to_ascii_lowercase()).or_default();
            rooms.insert(model.clone());
            triangles.extend(mesh.indices.as_chunks::<3>().0.iter().filter_map(|face| {
                Some(turned([
                    corner(face[0])?,
                    corner(face[1])?,
                    corner(face[2])?,
                ]))
            }));
        }
    }
    found
}

/// The digest of each lightmap that the room meshes of `rooms` draw
/// (`lightmap_triangles`), and the room models that draw each one.
type LightmapDigests = (BTreeMap<String, String>, BTreeMap<String, BTreeSet<String>>);

fn lightmap_digests(rooms: &[RoomIr], models: &RoomModels) -> LightmapDigests {
    let (mut lightmaps, mut rooms_of) = (BTreeMap::new(), BTreeMap::new());
    for (name, (triangles, drawn_by)) in lightmap_triangles(rooms, models) {
        lightmaps.insert(name.clone(), triangles_digest(triangles));
        rooms_of.insert(name, drawn_by);
    }
    (lightmaps, rooms_of)
}

const POS_QUANT: f32 = 1000.0;
const UV_QUANT: f32 = 2048.0;
const PLACE_POS_TOL: f32 = 0.005;
const PLACE_ANGLE_TOL: f32 = 0.01;

fn quant(v: f32, scale: f32) -> i32 {
    if v.is_finite() {
        (v * scale)
            .clamp(i32::MIN as f32 / 2.0, i32::MAX as f32 / 2.0)
            .round() as i32
    } else {
        0
    }
}

fn turned_approx<T: Ord + Copy>(corners: [T; 3]) -> [T; 3] {
    let first = (0..3).min_by_key(|&i| corners[i]).unwrap_or(0);
    std::array::from_fn(|i| corners[(first + i) % 3])
}

type ApproxCorner = [i32; 5];

fn approx_triangles_digest(mut triangles: Vec<[ApproxCorner; 3]>) -> String {
    triangles.sort_unstable();
    let mut digest = Digest::new("lightmap-approx");
    digest.count(triangles.len() as u64);
    for triangle in &triangles {
        for corner in triangle {
            digest.ints(corner);
        }
    }
    digest.finish()
}

pub fn undrawn_approx_digest() -> String {
    approx_triangles_digest(Vec::new())
}

fn lightmap_approx_digests(rooms: &[RoomIr], models: &RoomModels) -> BTreeMap<String, String> {
    let mut found: BTreeMap<String, Vec<[ApproxCorner; 3]>> = BTreeMap::new();
    for room in rooms {
        let model = room.model.to_ascii_lowercase();
        let Some(Some(ir)) = models.get(&model) else {
            continue;
        };
        for mesh in &ir.meshes {
            let Some(lightmap) = mesh.lightmap.as_deref() else {
                continue;
            };
            if mesh.referenced || mesh.skin.is_some() || mesh.uv2.len() != mesh.positions.len() {
                continue;
            }
            let Some(node) = ir.nodes.get(mesh.node) else {
                continue;
            };
            let corner = |index: u32| -> Option<ApproxCorner> {
                let index = index as usize;
                let at = apply(&node.world, *mesh.positions.get(index)?);
                let uv = mesh.uv2.get(index)?;
                Some([
                    quant(at[0] + room.position[0], POS_QUANT),
                    quant(at[1] + room.position[1], POS_QUANT),
                    quant(at[2] + room.position[2], POS_QUANT),
                    quant(uv[0], UV_QUANT),
                    quant(uv[1], UV_QUANT),
                ])
            };
            let triangles = found.entry(lightmap.to_ascii_lowercase()).or_default();
            triangles.extend(mesh.indices.as_chunks::<3>().0.iter().filter_map(|face| {
                let tri = [corner(face[0])?, corner(face[1])?, corner(face[2])?];
                if tri[0] == tri[1] || tri[1] == tri[2] || tri[0] == tri[2] {
                    return None;
                }
                Some(turned_approx(tri))
            }));
        }
    }
    found
        .into_iter()
        .map(|(name, triangles)| (name, approx_triangles_digest(triangles)))
        .collect()
}

/// Quantized world-space triangle digest of a lit placeable model (`model-approx`),
/// tolerant of vertex reordering, mesh splits, node hierarchy changes, and
/// sub-millimetre re-save drift.
pub fn model_approx_digest(ir: &ModelIr) -> String {
    let lit = crate::placeable_light::lit_meshes(ir);
    let mut triangles: Vec<[[i32; 3]; 3]> = Vec::new();
    for (number, mesh) in ir.meshes.iter().enumerate() {
        if !lit.get(number).copied().unwrap_or(false) {
            continue;
        }
        let rest = ir
            .nodes
            .get(mesh.node)
            .map_or([[0.0; 4]; 4], |node| node.world);
        let corner = |index: u32| -> Option<[i32; 3]> {
            let at = apply(&rest, *mesh.positions.get(index as usize)?);
            Some([
                quant(at[0], POS_QUANT),
                quant(at[1], POS_QUANT),
                quant(at[2], POS_QUANT),
            ])
        };
        triangles.extend(mesh.indices.as_chunks::<3>().0.iter().filter_map(|face| {
            let tri = [corner(face[0])?, corner(face[1])?, corner(face[2])?];
            if tri[0] == tri[1] || tri[1] == tri[2] || tri[0] == tri[2] {
                return None;
            }
            Some(turned_approx(tri))
        }));
    }
    triangles.sort_unstable();
    let mut digest = Digest::new("placeable-model-approx");
    digest.count(triangles.len() as u64);
    for triangle in &triangles {
        for corner in triangle {
            digest.ints(corner);
        }
    }
    digest.finish()
}

/// The digest of a lit placeable model. It covers each lit mesh's number
/// among the model's meshes, its node's rest place, which holds the
/// ancestors' scale, its vertices in order and its triangles' corners.
pub fn model_digest(ir: &ModelIr) -> String {
    let mut digest = Digest::new("placeable model");
    let lit = crate::placeable_light::lit_meshes(ir);
    for (number, mesh) in ir.meshes.iter().enumerate() {
        if !lit.get(number).copied().unwrap_or(false) {
            continue;
        }
        digest.count(number as u64);
        let rest = ir
            .nodes
            .get(mesh.node)
            .map_or([[0.0; 4]; 4], |node| node.world);
        digest.bits(
            &rest
                .as_flattened()
                .iter()
                .copied()
                .map(bits)
                .collect::<Vec<u32>>(),
        );
        digest.count(mesh.positions.len() as u64);
        digest.bits(
            &mesh
                .positions
                .as_flattened()
                .iter()
                .copied()
                .map(bits)
                .collect::<Vec<u32>>(),
        );
        digest.count(mesh.indices.len() as u64);
        digest.bits(&mesh.indices);
    }
    digest.finish()
}

/// The player's files as the exact lines judge them, beside their
/// fingerprint.
#[derive(Debug, Clone)]
pub struct Current {
    /// The fingerprint, with no exact lines.
    pub print: Fingerprint,
    /// The digest of each lightmap that a room mesh draws (`lightmap_digests`).
    pub lightmaps: BTreeMap<String, String>,
    /// Quantized geometry-tolerant digest of each lightmap (`lightmap_approx_digests`).
    pub lightmaps_approx: BTreeMap<String, String>,
    /// The room models that draw each lightmap of `lightmaps`.
    pub rooms_of: BTreeMap<String, BTreeSet<String>>,
    /// Every lightmap that a mesh of a room model names.
    pub named: BTreeSet<String>,
    /// The lit placeables, in the order of the area's instances.
    pub placed: Vec<Placed>,
    /// The digest of each lit placeable model.
    pub models: BTreeMap<String, String>,
    /// Quantized geometry-tolerant digest of each lit placeable model.
    pub models_approx: BTreeMap<String, String>,
}

impl Current {
    /// The digest of lightmap `name` in these files. A lightmap that no room
    /// mesh draws has the digest of no triangles.
    pub fn lightmap(&self, name: &str) -> String {
        self.lightmaps
            .get(name)
            .cloned()
            .unwrap_or_else(undrawn_digest)
    }

    /// The geometry-tolerant digest of lightmap `name` in these files.
    pub fn lightmap_approx(&self, name: &str) -> String {
        self.lightmaps_approx
            .get(name)
            .cloned()
            .unwrap_or_else(undrawn_approx_digest)
    }
}

/// Area `area` in the files that `resolver` reads: its fingerprint and the
/// numbers the exact lines compare. Each model is read once.
pub fn current(resolver: &Resolver, area: &AreaIr) -> Current {
    let models = room_models(resolver, area);
    let (lit, lit_models) = lit_placeables(resolver, area);
    let print = Fingerprint {
        area: area.resref.to_ascii_lowercase(),
        format: FORMAT,
        rooms: rooms(resolver, area, &models),
        placeables: placeables(resolver, &lit, &lit_models),
        exact: None,
    };
    let (lightmaps, rooms_of) = lightmap_digests(&area.rooms, &models);
    let lightmaps_approx = lightmap_approx_digests(&area.rooms, &models);
    Current {
        print,
        lightmaps,
        lightmaps_approx,
        rooms_of,
        named: named_lightmaps(&models),
        placed: (lit.iter())
            .map(|(object, name, instance)| placed(*object, name, instance))
            .collect(),
        models: (lit_models.iter())
            .map(|(name, ir)| (name.clone(), model_digest(ir)))
            .collect(),
        models_approx: (lit_models.iter())
            .map(|(name, ir)| (name.clone(), model_approx_digest(ir)))
            .collect(),
    }
}

/// The lightmaps of a set that the game shows from a PNG: each PNG in the
/// folder `set` by its name in lower case, without the lightmaps that its
/// `stored.txt` names.
pub fn set_lightmaps(set: &Path) -> BTreeSet<String> {
    let stored = crate::stored_fill::stored_names(set);
    let Ok(entries) = std::fs::read_dir(set) else {
        return BTreeSet::new();
    };
    (entries.flatten())
        .filter_map(|entry| entry.file_name().to_str().map(str::to_ascii_lowercase))
        .filter_map(|name| {
            if name.ends_with(".dir.png") {
                return None;
            }
            name.strip_suffix(".klm")
                .or_else(|| name.strip_suffix(".png"))
                .map(str::to_string)
        })
        .filter(|name| !stored.contains(name))
        .collect()
}

/// The fingerprint of a set baked over the files `current`, whose PNGs are
/// the lightmaps `pngs`, with its exact lines. Beside it, the lightmaps of
/// `pngs` that no room mesh draws, such as a lightmap baked only for the
/// grass under a walkmesh. Those are stamped with the digest of no
/// triangles, so they match as long as no room mesh draws them.
pub fn stamp(current: &Current, pngs: &BTreeSet<String>) -> (Fingerprint, Vec<String>) {
    let undrawn = (pngs.iter())
        .filter(|name| !current.lightmaps.contains_key(*name))
        .cloned()
        .collect();
    let exact = Exact {
        lightmaps: (pngs.iter())
            .map(|name| (name.clone(), current.lightmap(name)))
            .collect(),
        lightmaps_approx: (pngs.iter())
            .filter_map(|name| Some((name.clone(), current.lightmaps_approx.get(name)?.clone())))
            .collect(),
        backdrops: current.named.difference(pngs).cloned().collect(),
        placed: current.placed.clone(),
        models: current.models.clone(),
        models_approx: current.models_approx.clone(),
    };
    let print = Fingerprint {
        exact: Some(exact),
        ..current.print.clone()
    };
    (print, undrawn)
}

/// What the game may take from a bake stamped `stamp`, over files whose
/// fingerprint is `current`.
pub fn verdict(stamp: &Fingerprint, current: &Fingerprint) -> Verdict {
    if stamp.format != current.format {
        Verdict::Refused(format_differs(stamp.format, current.format))
    } else if stamp.rooms != current.rooms {
        Verdict::Refused(
            "the area's rooms or stored lightmaps in the game's files are not the ones the bake was made from"
                .to_string(),
        )
    } else if stamp.placeables != current.placeables {
        Verdict::LightmapsOnly
    } else {
        Verdict::Whole
    }
}

/// Why a bake of format `stamped` is refused by a game that reads `reads`.
fn format_differs(stamped: u32, reads: u32) -> String {
    format!("the bake's format is {stamped}, and this game reads format {reads}")
}

/// What the game may take from a bake with exact lines, lightmap by
/// lightmap and placeable by placeable (`fit`).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Fit {
    /// The baked lightmaps whose triangles are the ones the bake was made
    /// over. The game shows their PNGs.
    pub matching: BTreeSet<String>,
    /// The baked lightmaps whose triangles differ, each with the room models
    /// of the player's files that draw it (none when no room draws it any
    /// more). The game draws them at their stored light.
    pub differing: BTreeMap<String, BTreeSet<String>>,
    /// The lightmaps the bake knew and left alone. The game draws them as it
    /// always has.
    pub backdrops: BTreeSet<String>,
    /// For each lit placeable of the player's files that matches a baked
    /// one, its number among the area's instances and the baked one's
    /// number, under which `placeables.txt` holds its light.
    pub placeables: BTreeMap<usize, usize>,
    /// Whether any lit placeable of the player's files, or any baked one,
    /// has no match.
    pub placeables_differ: bool,
}

impl Fit {
    /// Whether the game shows any of the bake. An area where no baked
    /// lightmap matches is drawn with the game's own lighting.
    pub fn baked(&self) -> bool {
        !self.matching.is_empty()
    }
}

/// What the game may take from a bake stamped `stamp` over the files
/// `current`, judged lightmap by lightmap. `None` when the stamp has no
/// exact lines of version `EXACT`, and the caller then takes `verdict`. The
/// reason when the format differs.
///
/// A baked lightmap matches when its digest is the one of the player's
/// files. A lit placeable of the player's files matches a baked one with
/// the same model, place and direction, when its model's digest is also the
/// baked one's. Each baked placeable matches at most one placeable.
fn pos_within(a: [u32; 3], b: [u32; 3], tol: f32) -> bool {
    let mut d2 = 0.0;
    for i in 0..3 {
        let diff = number(a[i]) - number(b[i]);
        if !diff.is_finite() {
            return false;
        }
        d2 += diff * diff;
    }
    d2 <= tol * tol
}

fn facing_dir(facing: Facing) -> Option<[f32; 2]> {
    match facing {
        Facing::Bearing(b) => {
            let angle = number(b);
            angle.is_finite().then(|| [angle.cos(), angle.sin()])
        }
        Facing::Vector([x, y]) => {
            let (fx, fy) = (number(x), number(y));
            let len = (fx * fx + fy * fy).sqrt();
            (len.is_finite() && len > 1e-6).then(|| [fx / len, fy / len])
        }
    }
}

fn facing_within(a: Facing, b: Facing, tol: f32) -> bool {
    if a == b {
        return true;
    }
    let (Some(va), Some(vb)) = (facing_dir(a), facing_dir(b)) else {
        return false;
    };
    (va[0] * vb[0] + va[1] * vb[1]) >= tol.cos()
}

pub fn fit(stamp: &Fingerprint, current: &Current) -> Option<Result<Fit, String>> {
    let exact = stamp.exact.as_ref()?;
    if stamp.format != current.print.format {
        return Some(Err(format_differs(stamp.format, current.print.format)));
    }
    let mut fit = Fit {
        backdrops: exact.backdrops.clone(),
        ..Fit::default()
    };
    for (name, digest) in &exact.lightmaps {
        let matched = *digest == current.lightmap(name)
            || exact
                .lightmaps_approx
                .get(name)
                .is_some_and(|approx| current.lightmaps_approx.get(name) == Some(approx));
        if matched {
            fit.matching.insert(name.clone());
        } else {
            let rooms = current
                .rooms_of
                .get(name)
                .cloned()
                .unwrap_or(BTreeSet::new());
            fit.differing.insert(name.clone(), rooms);
        }
    }
    let exact_model = |model: &str| {
        exact.models.contains_key(model) && exact.models.get(model) == current.models.get(model)
    };
    let approx_model = |model: &str| {
        exact_model(model)
            || (exact.models_approx.contains_key(model)
                && exact.models_approx.get(model) == current.models_approx.get(model))
    };
    let mut taken = vec![false; exact.placed.len()];
    let mut unmatched: Vec<&Placed> = Vec::new();
    for now in &current.placed {
        let found = (exact.placed.iter().enumerate()).find(|(index, baked)| {
            !taken[*index]
                && baked.model == now.model
                && baked.position == now.position
                && baked.facing == now.facing
                && exact_model(&now.model)
        });
        match found {
            Some((index, baked)) => {
                taken[index] = true;
                fit.placeables.insert(now.instance, baked.instance);
            }
            None => unmatched.push(now),
        }
    }
    for now in unmatched {
        let found = (exact.placed.iter().enumerate()).find(|(index, baked)| {
            !taken[*index]
                && baked.model == now.model
                && pos_within(baked.position, now.position, PLACE_POS_TOL)
                && facing_within(baked.facing, now.facing, PLACE_ANGLE_TOL)
                && approx_model(&now.model)
        });
        match found {
            Some((index, baked)) => {
                taken[index] = true;
                fit.placeables.insert(now.instance, baked.instance);
            }
            None => fit.placeables_differ = true,
        }
    }
    fit.placeables_differ |= taken.iter().any(|taken| !taken);
    Some(Ok(fit))
}

/// A number as the exact lines write it. Rust writes the shortest text that
/// reads back to the same bits.
fn written(bits: u32) -> String {
    number(bits).to_string()
}

/// The number in `word`, as bits with -0 written as 0.
fn read_bits(word: &str) -> Option<u32> {
    word.parse::<f32>().ok().map(bits)
}

impl Exact {
    /// The exact lines, after the `exact` line that gives their version:
    ///
    /// - `lightmap <name> <digest>` for each lightmap with a PNG,
    /// - `backdrop <name>` for each lightmap the bake left alone,
    /// - `placeable <instance> <model> <x> <y> <z> bearing <radians>`, or
    ///   `... vector <x> <y>`, for each lit placeable,
    /// - `model <name> <digest>` for each lit placeable model.
    pub fn lines(&self) -> String {
        let mut text = String::new();
        let _ = writeln!(text, "exact {EXACT}");
        for (name, digest) in &self.lightmaps {
            let _ = writeln!(text, "lightmap {name} {digest}");
        }
        for (name, digest) in &self.lightmaps_approx {
            let _ = writeln!(text, "lightmap-approx {name} {digest}");
        }
        for name in &self.backdrops {
            let _ = writeln!(text, "backdrop {name}");
        }
        for placed in &self.placed {
            let [x, y, z] = placed.position.map(written);
            let facing = match placed.facing {
                Facing::Bearing(bearing) => format!("bearing {}", written(bearing)),
                Facing::Vector([x, y]) => format!("vector {} {}", written(x), written(y)),
            };
            let _ = writeln!(
                text,
                "placeable {} {} {x} {y} {z} {facing}",
                placed.instance, placed.model
            );
        }
        for (name, digest) in &self.models {
            let _ = writeln!(text, "model {name} {digest}");
        }
        for (name, digest) in &self.models_approx {
            let _ = writeln!(text, "model-approx {name} {digest}");
        }
        text
    }

    /// Takes one exact line, split into words. `None` when the line is not
    /// an exact line, and an error when it is one that does not read.
    pub fn read_line(&mut self, words: &[&str]) -> Option<Result<(), ()>> {
        let read = match words {
            ["lightmap", name, digest] => {
                self.lightmaps
                    .insert(name.to_ascii_lowercase(), (*digest).to_string());
                Ok(())
            }
            ["lightmap-approx", name, digest] => {
                self.lightmaps_approx
                    .insert(name.to_ascii_lowercase(), (*digest).to_string());
                Ok(())
            }
            ["backdrop", name] => {
                self.backdrops.insert(name.to_ascii_lowercase());
                Ok(())
            }
            ["placeable", instance, model, x, y, z, facing @ ..] => {
                let facing = match facing {
                    ["bearing", bearing] => read_bits(bearing).map(Facing::Bearing),
                    ["vector", x, y] => {
                        (read_bits(x).zip(read_bits(y))).map(|(x, y)| Facing::Vector([x, y]))
                    }
                    _ => None,
                };
                let position =
                    (read_bits(x).zip(read_bits(y)).zip(read_bits(z))).map(|((x, y), z)| [x, y, z]);
                match (instance.parse().ok(), position, facing) {
                    (Some(instance), Some(position), Some(facing)) => {
                        self.placed.push(Placed {
                            instance,
                            model: model.to_ascii_lowercase(),
                            position,
                            facing,
                        });
                        Ok(())
                    }
                    _ => Err(()),
                }
            }
            ["model", name, digest] => {
                self.models
                    .insert(name.to_ascii_lowercase(), (*digest).to_string());
                Ok(())
            }
            ["model-approx", name, digest] => {
                self.models_approx
                    .insert(name.to_ascii_lowercase(), (*digest).to_string());
                Ok(())
            }
            _ => return None,
        };
        Some(read)
    }
}

/// The text of `FILE` for `print`.
pub fn text(print: &Fingerprint) -> String {
    let mut text = String::from(
        "# The game files this set of lightmaps was baked from (crates/game/src/bake_fingerprint.rs).\n\
         # format is the meaning of the bake's files. rooms is a digest of the area's layout, its room\n\
         # models and their stored lightmaps. placeables is a digest of the placeables the bake lights\n\
         # or lets block its rays. The game shows the lightmaps only when format and rooms match the\n\
         # player's files, and the placeables' light only when placeables matches too.\n",
    );
    let _ = writeln!(text, "area {}", print.area);
    let _ = writeln!(text, "format {}", print.format);
    let _ = writeln!(text, "rooms {}", print.rooms);
    let _ = writeln!(text, "placeables {}", print.placeables);
    if let Some(exact) = &print.exact {
        text.push_str(
            "# The exact lines. Each lightmap with a PNG has a digest of the room triangles that draw it,\n\
             # and the game shows the PNG where that digest matches. A backdrop is a room lightmap the\n\
             # bake left alone. Each lit placeable is listed with its place, and each lit placeable\n\
             # model has a digest of its lit meshes. A game that reads these lines uses them in place\n\
             # of rooms and placeables.\n",
        );
        text.push_str(&exact.lines());
    }
    text
}

/// Writes `print` into `FILE` of the set `folder`. The text goes into a
/// file beside it first, which is then renamed into place, so that a
/// fingerprint is never left half written.
pub fn write(folder: &Path, print: &Fingerprint) -> Result<(), String> {
    let path = folder.join(FILE);
    let partial = folder.join(format!("{FILE}.partial"));
    let failed = |error: std::io::Error| format!("{}: {error}", path.display());
    std::fs::write(&partial, text(print)).map_err(failed)?;
    std::fs::rename(&partial, &path).map_err(failed)
}

/// The fingerprint in `FILE` of the set `folder`. `None` when the set has
/// no such file, and the reason when the file does not read.
pub fn read(folder: &Path) -> Option<Result<Fingerprint, String>> {
    let path = folder.join(FILE);
    let text = std::fs::read_to_string(&path).ok()?;
    Some(parse(&text).ok_or_else(|| format!("{} does not read", path.display())))
}

/// The fingerprint in the text of a `FILE`. Exact lines of another version
/// than `EXACT` are left out, and so are exact lines that do not read.
pub fn parse(text: &str) -> Option<Fingerprint> {
    let (mut area, mut format, mut rooms, mut placeables) = (None, None, None, None);
    let (mut version, mut exact, mut broken) = (None, Exact::default(), false);
    for line in text.lines().map(str::trim) {
        if line.starts_with('#') {
            continue;
        }
        let Some((key, value)) = line.split_once(' ') else {
            continue;
        };
        let value = value.trim().to_string();
        match key {
            "area" => area = Some(value.to_ascii_lowercase()),
            "format" => format = value.parse().ok(),
            "rooms" => rooms = Some(value),
            "placeables" => placeables = Some(value),
            "exact" => version = value.parse::<u32>().ok(),
            _ => {
                let words: Vec<&str> = line.split_whitespace().collect();
                broken |= exact.read_line(&words) == Some(Err(()));
            }
        }
    }
    Some(Fingerprint {
        area: area?,
        format: format?,
        rooms: rooms?,
        placeables: placeables?,
        exact: (version == Some(EXACT) && !broken).then_some(exact),
    })
}

#[cfg(feature = "probes")]
pub mod probes;
