//! Rooms cut from Jedi Academy levels, built as KOTOR rooms in memory
//! (`docs/LEVEL-EDIT.md`, "Jedi Academy rooms in a new map").
//!
//! A new map keeps a Jedi Academy room as its level and its name
//! (`level_edit::map::JaRoom`), never as its geometry. Before a run opens a
//! resolver of the map's game, it builds each such room from the player's
//! own Jedi Academy install and the cuts the project keeps
//! (`level_edit::ja_rooms`). The room's files go into the run's memory
//! (`vfs::made`), where every resolver opened after that finds them as it
//! finds a room of the install. No file of them is written. The room is
//! named as `level_edit::ja_rooms::room_resref` names it, and it has these
//! files:
//!
//! - Its model and the model's MDX (`fmt_odyssey::mdl::written`). The model
//!   holds the room's part of the level as the converter draws it
//!   (`asset_ja::level`, `Paint::Room`), with each surface's own shader and
//!   the level's light. Its origin is the middle of the room's floor, seen
//!   from above, at the floor's lowest point. A mesh whose shader does not
//!   stop a body (water, fog, a hanging banner) gets a transparency hint
//!   of 1, so kjk's collision leaves it out as it leaves out a KOTOR room's
//!   see-through meshes.
//! - Its walkmesh. The walkmesh is the level's floor
//!   (`asset_ja::level_collision`) cut to the room, all of it walkable
//!   stone. It is in the level's own coordinates, as a KOTOR room's
//!   walkmesh is in its own area's.
//! - Its textures as TGA pictures, each named `jt` and 14 letters and
//!   digits of a digest of its shader. A shader that adds its colour to
//!   what lies behind it gets a TXI that says so. A shader drawn from both
//!   sides has its triangles drawn twice, once each way, since a KOTOR
//!   room's meshes are drawn from the front only. An animated shader shows
//!   its first picture.
//! - Its light: the level's light atlas as one TGA (`jl` and a digest of
//!   its texels, so rooms with the same atlas share it), and each lightmap
//!   kept beside the level that the room uses (`jb`).
//!
//! The map's copy of the room, moved and turned to where the map places it,
//! is then made as a KOTOR room's copy is (`level_edit::map::room_copy`).
//! A run of the game makes it in memory too (`make_project`); the editor
//! makes it as it makes every copy of a map it edits.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::Path;

use asset_ja::level::Paint;
use asset_ja::level_rooms::{Cut, RoomCuts, Shape};
use asset_odyssey::model::ModelIr;
use asset_odyssey::texture::{Encoding, TextureIr};
use fmt_odyssey::ResType;
use fmt_odyssey::bwm::{WalkFace, Walkmesh};
use fmt_odyssey::mdl::WrittenMesh;
use level_edit::ja_rooms::JaRooms;
use level_edit::map::{JaRoom, NewMap};
use vfs::{Discovery, Game};

use crate::viewer_ja::{Files, blend_funcs, level_path, stack, textures};

/// The walk material of a built room's floor: stone (`surfacemat.2da`).
const FLOOR: u32 = 4;

/// The most vertices a mesh of a model may have: a face numbers its
/// vertices with 16 bits.
const MOST_VERTICES: usize = u16::MAX as usize;

/// A room built into memory.
#[derive(Debug, Clone)]
pub struct Built {
    pub ja: JaRoom,
    /// The name of the room's model and walkmesh.
    pub resref: String,
    /// Where the room's origin lies in the level, in metres. A map that
    /// places the room keeps this as the room's `from`.
    pub from: [f32; 3],
    /// The triangles the room's model draws.
    pub triangles: usize,
    /// The triangles of the room's walkmesh.
    pub floor: usize,
}

/// The cuts that `kept` holds, as the cutter takes them: the rest of the
/// level is room 0, and each cut room follows in the order it was made.
pub fn room_cuts(kept: &JaRooms) -> RoomCuts {
    RoomCuts {
        names: std::iter::once(kept.rest.clone())
            .chain(kept.rooms.iter().cloned())
            .collect(),
        cuts: (kept.cuts.iter())
            .map(|cut| Cut {
                room: cut.room + 1,
                shape: Shape {
                    points: cut.points.clone(),
                    up: cut.up,
                    down: cut.down,
                    margin: cut.margin,
                },
            })
            .collect(),
    }
}

/// 14 letters and digits of a digest of `bytes`, after `kind`: a name that
/// fits a resref's 16 characters.
fn named(kind: &str, bytes: &[u8]) -> String {
    format!("{kind}{}", &level_edit::ja_rooms::digest(bytes)[..14])
}

/// Builds the rooms of the level `level` that the project in `folder` cut
/// (only those named in `wanted`, when it names some), and puts their
/// files into memory for `game`. Gives the rooms built, and what went
/// wrong on the way that did not stop it.
pub fn build_level(
    discovery: &Discovery,
    game: Game,
    folder: &Path,
    level: &str,
    wanted: Option<&BTreeSet<String>>,
) -> Result<(Vec<Built>, Vec<String>), String> {
    let stack = stack(discovery)?;
    let files = Files::new(&stack);
    let path = level_path(level);
    let bytes = files
        .read(&path)
        .ok_or(format!("{path}: not in Jedi Academy's archives"))?;
    let bsp = fmt_ja::bsp::parse(&bytes).map_err(|error| format!("{path}: {error}"))?;
    let digest = level_edit::ja_rooms::digest(&bytes);
    let kept = JaRooms::read(folder, &path, &digest)?
        .ok_or(format!("the project keeps no rooms of {path}"))?;
    let cuts = room_cuts(&kept);
    let cutter = cuts.cutter();
    let md3 = |model: &str| fmt_ja::md3::parse(&files.read(model)?).ok();
    let floor = asset_ja::level_collision::collision(&bsp).floor;
    // Whether each shader stops a body. A shader the level names twice
    // stops one when either does.
    let mut stops: BTreeMap<String, bool> = BTreeMap::new();
    for shader in &bsp.shaders {
        *stops
            .entry(fmt_ja::game_path(&shader.name))
            .or_insert(false) |= asset_ja::level_collision::stops(shader.content_flags);
    }
    let mut notes = Vec::new();
    for name in wanted.into_iter().flatten() {
        if !cuts.names.contains(name) {
            notes.push(format!("{path}: the project has no room {name:?}"));
        }
    }

    // Each room's model and floor first, then the textures they all need.
    let mut rooms = Vec::new();
    for (index, name) in cuts.names.iter().enumerate() {
        if wanted.is_some_and(|wanted| !wanted.contains(name)) {
            continue;
        }
        let drawn = asset_ja::level::level(&path, &bsp, &md3, &Paint::Room(&cutter, index));
        let room_floor = cutter.room_part(&floor, index);
        rooms.push((name.clone(), drawn, room_floor));
    }
    let shaders: Vec<String> = (rooms.iter())
        .flat_map(|(_, drawn, _)| &drawn.model.meshes)
        .filter_map(|mesh| mesh.texture.clone())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect();
    let (decoded, missing) = textures(&files, &shaders);
    if !missing.is_empty() {
        notes.push(format!(
            "{path}: {} shaders have no picture: {}",
            missing.len(),
            missing.join(", ")
        ));
    }
    let additive: BTreeSet<String> = (blend_funcs(&files, &shaders).into_iter())
        .filter(|(_, (source, target))| source == "gl_one" && target == "gl_one")
        .map(|(shader, _)| shader)
        .collect();
    let mut texture_names = BTreeMap::new();
    for shader in &shaders {
        let resref = named("jt", shader.as_bytes());
        if let Some(texture) = decoded.get(shader)
            && !vfs::made::holds(game, &resref, ResType::TGA)
        {
            match tga(texture) {
                Some(bytes) => {
                    vfs::made::put(game, &resref, ResType::TGA, bytes);
                    if additive.contains(shader) {
                        let txi = b"blending additive\r\n".to_vec();
                        vfs::made::put(game, &resref, ResType::TXI, txi);
                    }
                }
                None => notes.push(format!("{shader}: its picture is not plain colour")),
            }
        }
        texture_names.insert(shader.clone(), resref);
    }
    let two_sided: BTreeSet<&String> = (decoded.iter())
        .filter(|(_, texture)| texture.facts.two_sided)
        .map(|(shader, _)| shader)
        .collect();

    let mut built = Vec::new();
    for (name, drawn, room_floor) in rooms {
        let resref = level_edit::ja_rooms::room_resref(&path, &name);
        // The lightmaps: the atlas, and the pages kept beside the level.
        let mut light_names = BTreeMap::new();
        let atlas = named(
            "jl",
            drawn.light.mips.first().map_or(&[][..], Vec::as_slice),
        );
        if !vfs::made::holds(game, &atlas, ResType::TGA) {
            match tga(&drawn.light) {
                Some(bytes) => vfs::made::put(game, &atlas, ResType::TGA, bytes),
                None => notes.push(format!("{path}: its light is not plain colour")),
            }
        }
        light_names.insert(asset_ja::level::light_name(&path), atlas);
        for page in &drawn.beside {
            let resref = named("jb", format!("{path}#{page}").as_bytes());
            let picture = asset_ja::level::beside_path(&path, *page);
            if !vfs::made::holds(game, &resref, ResType::TGA) {
                let (loaded, _) = textures(&files, std::iter::once(&picture));
                match loaded.get(&picture).and_then(tga) {
                    Some(bytes) => vfs::made::put(game, &resref, ResType::TGA, bytes),
                    None => notes.push(format!("{picture}: not found")),
                }
            }
            light_names.insert(asset_ja::level::beside_name(&path, *page), resref);
        }

        let from = origin(&drawn.model, &room_floor);
        let meshes = written_meshes(
            &resref,
            &drawn.model,
            from,
            (&texture_names, &light_names),
            (&two_sided, &stops),
        );
        let triangles = (meshes.iter()).map(|mesh| mesh.triangles.len()).sum();
        let (mdl, mdx) = fmt_odyssey::mdl::written(&resref, &meshes)
            .map_err(|error| format!("{path}, room {name}: {error}"))?;
        let walkmesh = walkmesh(&room_floor);
        vfs::made::put(game, &resref, ResType::MDL, mdl);
        vfs::made::put(game, &resref, ResType::MDX, mdx);
        vfs::made::put(game, &resref, ResType::WOK, walkmesh.write());
        if triangles == 0 {
            notes.push(format!("{path}: the room {name:?} draws nothing"));
        }
        built.push(Built {
            ja: JaRoom {
                level: path.clone(),
                room: name,
            },
            resref,
            from,
            triangles,
            floor: walkmesh.faces.len(),
        });
    }
    Ok((built, notes))
}

/// Builds every room of every level the project in `folder` keeps rooms of
/// (`level_edit::ja_rooms::kept`) into memory for `game`. Gives the rooms
/// built, and what went wrong.
pub fn build_kept(discovery: &Discovery, game: Game, folder: &Path) -> (Vec<Built>, Vec<String>) {
    let mut built = Vec::new();
    let mut notes = Vec::new();
    for (level, _) in level_edit::ja_rooms::kept(folder) {
        match build_level(discovery, game, folder, &level, None) {
            Ok((rooms, more)) => {
                built.extend(rooms);
                notes.extend(more);
            }
            Err(why) => notes.push(why),
        }
    }
    (built, notes)
}

/// Puts into memory the copy of each Jedi Academy room of the map `map`
/// (of the module `module`): its model renamed and turned, and its
/// walkmesh moved to where the map places it (`level_edit::map::room_copy`).
/// The rooms themselves must be built first (`build_level`).
pub fn place_copies(game: Game, module: &str, map: &NewMap) -> Vec<String> {
    let read = |name: &str, restype: ResType| {
        vfs::made::get(game, name, restype).map(|bytes| bytes.to_vec())
    };
    let mut notes = Vec::new();
    for (index, room) in map.rooms.iter().enumerate() {
        if room.ja.is_none() {
            continue;
        }
        let name = level_edit::map::room_name(module, index);
        match level_edit::map::room_copy(room, &name, &read) {
            Ok(files) => {
                for (resref, restype, bytes) in files {
                    vfs::made::put(game, &resref, restype, bytes);
                }
            }
            Err(why) => notes.push(why),
        }
    }
    notes
}

/// Builds every Jedi Academy room that the maps of `project` place, and
/// the maps' copies of them, into memory, before a run of the game opens
/// any resolver. Gives what went wrong. A project whose maps place no
/// such room does nothing.
pub fn make_project(discovery: &Discovery, project: &level_edit::Project) -> Vec<String> {
    let Some(game) = Game::from_key(&project.game) else {
        return Vec::new();
    };
    let maps: Vec<(&str, &NewMap)> =
        std::iter::once((project.module.as_str(), project.map.as_ref()))
            .chain(
                (project.parked.iter())
                    .map(|(module, level)| (module.as_str(), level.map.as_ref())),
            )
            .filter_map(|(module, map)| Some((module, map?)))
            .collect();
    let mut wanted: BTreeMap<&str, BTreeSet<String>> = BTreeMap::new();
    for (_, map) in &maps {
        for ja in map.rooms.iter().filter_map(|room| room.ja.as_ref()) {
            (wanted.entry(ja.level.as_str()).or_default()).insert(ja.room.clone());
        }
    }
    let mut notes = Vec::new();
    for (level, rooms) in &wanted {
        match build_level(discovery, game, &project.folder, level, Some(rooms)) {
            Ok((_, more)) => notes.extend(more),
            Err(why) => notes.push(why),
        }
    }
    if !wanted.is_empty() {
        for (module, map) in maps {
            notes.extend(place_copies(game, module, map));
        }
    }
    notes
}

/// Where a built room's origin lies in the level: the middle of its floor
/// seen from above, at the floor's lowest point. A room with no floor
/// takes its drawn part's middle and lowest point.
fn origin(model: &ModelIr, floor: &[[[f32; 3]; 3]]) -> [f32; 3] {
    let corners: Vec<[f32; 3]> = if floor.is_empty() {
        (model.meshes.iter())
            .flat_map(|mesh| mesh.positions.iter().copied())
            .collect()
    } else {
        floor.iter().flatten().copied().collect()
    };
    if corners.is_empty() {
        return [0.0; 3];
    }
    let (mut low, mut high) = ([f32::INFINITY; 3], [f32::NEG_INFINITY; 3]);
    for corner in &corners {
        for axis in 0..3 {
            low[axis] = low[axis].min(corner[axis]);
            high[axis] = high[axis].max(corner[axis]);
        }
    }
    [(low[0] + high[0]) / 2.0, (low[1] + high[1]) / 2.0, low[2]]
}

/// The meshes of a built room's model: each mesh of `model` moved so that
/// `from` is the origin, named after the room, with its texture and its
/// lightmap named as the room's files name them (`names`: by shader, and
/// by the converter's lightmap name). A mesh of more vertices than a mesh
/// may have is split. A mesh drawn from both sides (`two_sided`) has each
/// triangle twice, once each way. A mesh whose shader does not stop a
/// body (`stops`) has a transparency hint of 1.
fn written_meshes(
    resref: &str,
    model: &ModelIr,
    from: [f32; 3],
    (textures, lights): (&BTreeMap<String, String>, &BTreeMap<String, String>),
    (two_sided, stops): (&BTreeSet<&String>, &BTreeMap<String, bool>),
) -> Vec<WrittenMesh> {
    let mut written = Vec::new();
    for mesh in &model.meshes {
        let shader = mesh.texture.clone().unwrap_or_else(String::new);
        let mut triangles: Vec<[u32; 3]> = mesh.indices.as_chunks::<3>().0.to_vec();
        if two_sided.contains(&shader) {
            let back: Vec<[u32; 3]> = triangles.iter().map(|&[a, b, c]| [a, c, b]).collect();
            triangles.extend(back);
        }
        let template = WrittenMesh {
            name: String::new(),
            texture: textures.get(&shader).cloned().unwrap_or_else(String::new),
            lightmap: (mesh.lightmap.as_ref())
                .and_then(|light| lights.get(light))
                .cloned()
                .unwrap_or_else(String::new),
            positions: Vec::new(),
            normals: Vec::new(),
            uv1: Vec::new(),
            uv2: Vec::new(),
            triangles: Vec::new(),
            material: FLOOR,
            transparency_hint: u32::from(!stops.get(&shader).copied().unwrap_or(true)),
        };
        // The vertices of the piece being made, by their number in the
        // mesh.
        let mut numbers: HashMap<u32, u16> = HashMap::new();
        let mut piece = template.clone();
        for triangle in triangles {
            if piece.positions.len() + 3 > MOST_VERTICES {
                written.push(std::mem::replace(&mut piece, template.clone()));
                numbers.clear();
            }
            let mut corners = [0u16; 3];
            for (slot, &vertex) in corners.iter_mut().zip(&triangle) {
                *slot = *numbers.entry(vertex).or_insert_with(|| {
                    let at = vertex as usize;
                    let position = mesh.positions[at];
                    piece
                        .positions
                        .push(std::array::from_fn(|axis| position[axis] - from[axis]));
                    piece.normals.push(mesh.normals[at]);
                    piece.uv1.push(mesh.uv1[at]);
                    piece.uv2.push(mesh.uv2[at]);
                    (piece.positions.len() - 1) as u16
                });
            }
            piece.triangles.push(corners);
        }
        if !piece.triangles.is_empty() {
            written.push(piece);
        }
    }
    for (number, mesh) in written.iter_mut().enumerate() {
        mesh.name = format!("{resref}_{number}");
    }
    written
}

/// A walkmesh of the triangles `floor` (metres, wound counter-clockwise
/// seen from above), every face walkable. Corners closer than a tenth of a
/// millimetre are one vertex. Each face's neighbours are the faces that
/// share an edge with it, and the outline is every edge that no other face
/// shares. The walkmesh has no search tree: kjk does not read one.
fn walkmesh(floor: &[[[f32; 3]; 3]]) -> Walkmesh {
    let mut vertices: Vec<[f32; 3]> = Vec::new();
    let mut by_place: HashMap<[i64; 3], u32> = HashMap::new();
    let mut faces = Vec::new();
    for corners in floor {
        let numbers = corners.map(|corner| {
            let key = corner.map(|part| (f64::from(part) * 10_000.0).round() as i64);
            *by_place.entry(key).or_insert_with(|| {
                vertices.push(corner);
                (vertices.len() - 1) as u32
            })
        });
        if numbers[0] == numbers[1] || numbers[1] == numbers[2] || numbers[0] == numbers[2] {
            continue;
        }
        let [a, b, c] = numbers.map(|number| vertices[number as usize].map(f64::from));
        let u: [f64; 3] = std::array::from_fn(|axis| b[axis] - a[axis]);
        let v: [f64; 3] = std::array::from_fn(|axis| c[axis] - a[axis]);
        let normal = [
            u[1] * v[2] - u[2] * v[1],
            u[2] * v[0] - u[0] * v[2],
            u[0] * v[1] - u[1] * v[0],
        ];
        let length = (normal.iter().map(|part| part * part).sum::<f64>()).sqrt();
        if length <= 1e-9 {
            continue;
        }
        let normal = normal.map(|part| part / length);
        let distance = -(normal[0] * a[0] + normal[1] * a[1] + normal[2] * a[2]);
        faces.push(WalkFace {
            vertices: numbers,
            material: FLOOR,
            normal: normal.map(|part| part as f32),
            distance: distance as f32,
        });
    }
    // Each edge's faces, by its two vertices in either order.
    let mut sides: HashMap<(u32, u32), Vec<usize>> = HashMap::new();
    for (face, walk) in faces.iter().enumerate() {
        for side in 0..3 {
            let (a, b) = (walk.vertices[side], walk.vertices[(side + 1) % 3]);
            sides
                .entry((a.min(b), a.max(b)))
                .or_default()
                .push(face * 3 + side);
        }
    }
    let mut adjacency = vec![[-1i32; 3]; faces.len()];
    let mut edges = Vec::new();
    for ((face, walk), neighbours) in faces.iter().enumerate().zip(&mut adjacency) {
        for (side, neighbour) in neighbours.iter_mut().enumerate() {
            let (a, b) = (walk.vertices[side], walk.vertices[(side + 1) % 3]);
            let other = sides[&(a.min(b), a.max(b))]
                .iter()
                .copied()
                .find(|&other| other / 3 != face);
            match other {
                Some(other) => *neighbour = other as i32,
                None => edges.push(((face * 3 + side) as i32, -1)),
            }
        }
    }
    let perimeters = if edges.is_empty() {
        Vec::new()
    } else {
        vec![edges.len() as u32]
    };
    Walkmesh {
        area: true,
        use_positions: [[0.0; 3]; 4],
        position: [0.0; 3],
        vertices,
        faces,
        aabb: Vec::new(),
        adjacency,
        edges,
        perimeters,
        aabb_word: 0,
    }
}

/// `texture`'s first picture as an uncompressed 32-bit TGA, its rows in
/// the order the texture keeps them (kjk reads a TGA's rows as they are
/// stored, `fmt_odyssey::texture`). A texture of frames stacked one under
/// another gives its first. `None` for a texture that is not plain colour,
/// or larger than a TGA holds.
fn tga(texture: &TextureIr) -> Option<Vec<u8>> {
    if texture.encoding != Encoding::Rgba8 {
        return None;
    }
    let frames = texture.facts.strip.map_or(1, |strip| strip.frames.max(1));
    let (width, height) = (texture.width, texture.height / frames);
    let (wide, high) = (u16::try_from(width).ok()?, u16::try_from(height).ok()?);
    let texels = texture.mips.first()?;
    let count = width as usize * height as usize;
    let texels = texels.get(..count * 4)?;
    let mut out = Vec::with_capacity(18 + count * 4);
    out.extend_from_slice(&[0, 0, 2, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
    out.extend_from_slice(&wide.to_le_bytes());
    out.extend_from_slice(&high.to_le_bytes());
    out.extend_from_slice(&[32, 8]);
    for &[red, green, blue, alpha] in texels.as_chunks::<4>().0 {
        out.extend_from_slice(&[blue, green, red, alpha]);
    }
    Some(out)
}
