//! Jedi Academy props that a level editor project places in a KOTOR area
//! (`level_edit`, `docs/LEVEL-EDIT.md`), drawn in the area's scene. Play
//! draws the active project's (`add_active`); the editor draws the props
//! as it has them.
//!
//! A prop's model is an MD3 from Jedi Academy's `base/` archives. Its
//! surfaces become meshes of one node, in Jedi Academy's units turned to
//! metres (`collision::UNIT`) and scaled by the prop's scale. Its textures
//! are read through the surfaces' shaders, as the Jedi Academy viewer reads
//! them (`viewer_ja::textures`).

use std::collections::BTreeMap;

use asset_odyssey::model::{IDENTITY, MeshIr, ModelIr, NodeIr};
use asset_odyssey::texture::TextureIr;
use level_edit::Prop;

use crate::viewer::{LoadedScene, Placed};
use crate::viewer_ja::Files;

/// The light a prop gives itself, 0 to 1 of its texture's own colour, where
/// the room's light at it is not known (`room_glow`): an area whose rooms
/// have no lightmaps, or a prop over no room's floor. A KOTOR room is lit
/// by its lightmaps, which a Jedi Academy prop has none of, and the area's
/// ambient light alone leaves a prop nearly black.
const PROP_GLOW: f32 = 0.45;

/// How much stronger a baked lightmap shows than it is stored
/// (`BAKED_GAIN` in `odyssey.wgsl`).
const BAKED_GAIN: f32 = 2.0;

/// How far over the floor (metres) a prop's light is read: where the light
/// probes stand (`light_probes`).
const PROBE_RISE: f32 = 1.0;

/// How far (metres) under and over a prop its room's floor is looked for.
const FLOOR_BELOW: f32 = 3.0;
const FLOOR_ABOVE: f32 = 0.5;

/// The room's light at a prop standing at `at` (game space), as a glow:
/// the light from above that the area's light probes read off the floor's
/// lightmap round it (`light_probes`), in the room whose floor lies under
/// it, read where the probes are, a metre over that floor (`PROBE_RISE`).
/// A prop stacked on another stands higher than the probes reach, and
/// read at its own height it found none and glowed with `PROP_GLOW`, much
/// brighter than the props round it. The light is then taken times the
/// share of a room's stored light the view keeps (all of it with the
/// game's lighting, half with the live fitted lights, none with the lamps
/// alone). Under a bake the probes read the baked lightmaps, which are
/// stored at half their level, so their light is doubled (`BAKED_GAIN`).
/// `None` where no probe with light is near.
///
/// A prop with no floor that near, such as a rock on an outcrop with no
/// walkable top, reads the light of the floor straight under it, however
/// far down, and else that of the nearest floor round it (`floor_for`). It
/// used to glow with `PROP_GLOW` there, brighter than the rocks on the snow
/// beside it.
fn room_glow(scene: &LoadedScene, at: [f32; 3]) -> Option<[f32; 3]> {
    let (room, sample) = floor_for(scene, at)?;
    let light = scene
        .probes
        .light_at(&scene.rooms, Some(room), sample)
        .ok()?
        .0;
    use crate::fitted_lights::Mode;
    // A baked lightmap is stored at half its level (`BAKED_GAIN` in
    // `odyssey.wgsl`), and the probes read it as stored.
    let kept = match crate::fitted_lights::mode_of_area(scene.baked) {
        Mode::Off => 1.0,
        Mode::Baked => BAKED_GAIN,
        Mode::Live => 0.5,
        Mode::LampsOnly => 0.0,
    };
    Some(
        (light[crate::light_probes::FROM_ABOVE] * kept)
            .min(bevy::math::Vec3::ONE)
            .to_array(),
    )
}

/// Where a prop standing at `at` reads its light: the room and the place a
/// metre over the floor (`PROBE_RISE`) that is, in this order, the highest
/// floor from `FLOOR_BELOW` under the prop to `FLOOR_ABOVE` over it; the
/// highest floor under it, however far down; or the floor whose middle is
/// nearest it seen from above, read at that middle. `None` in an area with
/// no floor.
fn floor_for(scene: &LoadedScene, at: [f32; 3]) -> Option<(usize, bevy::math::Vec3)> {
    // A floor's room and its height.
    let mut near: Option<(usize, f32)> = None;
    let mut under: Option<(usize, f32)> = None;
    let mut nearest: Option<(usize, f32, [f32; 3])> = None;
    for (index, room) in scene.rooms.iter().enumerate() {
        for [a, b, c] in &room.floor {
            if let Some(z) = crate::scene::height_at(*a, *b, *c, at[0], at[1])
                && z <= at[2] + FLOOR_ABOVE
            {
                if z >= at[2] - FLOOR_BELOW && near.is_none_or(|(_, higher)| z > higher) {
                    near = Some((index, z));
                }
                if under.is_none_or(|(_, higher)| z > higher) {
                    under = Some((index, z));
                }
            }
            let middle: [f32; 3] = std::array::from_fn(|axis| (a[axis] + b[axis] + c[axis]) / 3.0);
            let away = (middle[0] - at[0]).powi(2) + (middle[1] - at[1]).powi(2);
            if nearest.is_none_or(|(_, closer, _)| away < closer) {
                nearest = Some((index, away, middle));
            }
        }
    }
    let rise = |x: f32, y: f32, z: f32| bevy::math::Vec3::new(x, y, z + PROBE_RISE);
    match (near.or(under), nearest) {
        (Some((room, z)), _) => Some((room, rise(at[0], at[1], z))),
        (None, Some((room, _, [x, y, z]))) => Some((room, rise(x, y, z))),
        (None, None) => None,
    }
}

/// The side of a triangle's block in a prop's grid lightmap, in texels.
const GRID_BLOCK: usize = 2;

/// Lights the prop `made` (placed as `prop`) with the Jedi Academy light
/// `light` of the room it stands in, whose yaw is `room_yaw` (degrees), as
/// Jedi Academy lights a model: each corner takes the ambient light and
/// the directed light by how much its normal faces it
/// (`ModelLight::shade`). The light is stored as a lightmap of its own,
/// named `name`, a block of 2 by 2 texels for each triangle with its
/// corners' light, as the level's converter stores a model's light in a
/// room. Each mesh's corners are made its triangles' own, so each has its
/// place in the lightmap. The prop then glows not at all: its lightmap is
/// its light, as a room's is. Gives the lightmap.
fn lit_by_grid(
    made: &mut Placed,
    prop: &Prop,
    (light, room_yaw): (&asset_ja::light_grid::ModelLight, f32),
    name: &str,
) -> TextureIr {
    // A direction of the game turned into one of the level: back by the
    // room's yaw.
    let (sin, cos) = collision::trig::sin_cos_deg(-room_yaw);
    let to_level = |n: [f32; 3]| [cos * n[0] - sin * n[1], sin * n[0] + cos * n[1], n[2]];
    // Jedi Academy scales a model's normals with the model.
    let shade = |normal: [f32; 3], _: bevy::math::Vec3| {
        light.shade(to_level(normal).map(|part| part * prop.scale))
    };
    lit_per_corner(made, name, &shade)
}

/// A corner whose light from above is less than this share of the middle
/// value of its prop's corners read a pocket the rest of the prop is not in
/// (`lit_by_probes`).
const POCKET_SHARE: f32 = 0.35;

/// Lights the prop `made` under a bake that does not hold it, from the
/// light probes' six sides (`AreaProbes::light_at`, which holds the baked
/// lightmaps' light). Each corner reads the probes at its own place seen
/// from above, at the height of the floor under the prop (`light_near`;
/// `None` where no probe is near). So a large prop is lit along its length,
/// and two rocks side by side read the same probes where they meet. A
/// prop read at one place took one reading for all of it, and four rocks
/// within 5 metres read 0.81, 0.14, 0.82 and 0.27 from above (24 November
/// 2026). A corner with no reading, or one far darker than the prop's other
/// corners (`POCKET_SHARE`), takes the reading at the prop's own place
/// (`base`): it read a pocket, under an overhang or between rocks. Each
/// corner then takes the side it faces, weighed by how much it faces it, as
/// the shader weighs a probe (`probe_light` in `odyssey.wgsl`). The light is
/// stored as the bake stores its lightmaps, at half its level, and the
/// prop's material shows it as a room's baked lightmap
/// (`OdysseyMaterial::room_glow`). Gives the lightmap.
fn lit_by_probes(
    made: &mut Placed,
    base: &[bevy::math::Vec3; 6],
    light_near: &dyn Fn(bevy::math::Vec3) -> Option<[bevy::math::Vec3; 6]>,
    name: &str,
) -> TextureIr {
    use bevy::math::Vec3;
    let frame = crate::scene::placement(made);
    let luma = |light: Vec3| light.dot(Vec3::new(0.2126, 0.7152, 0.0722));
    // Each corner's reading, by its place in the game seen from above, to
    // the centimetre.
    let key = |at: Vec3| ((at.x * 100.0).round() as i64, (at.y * 100.0).round() as i64);
    let mut read: BTreeMap<(i64, i64), Option<[Vec3; 6]>> = BTreeMap::new();
    for mesh in &made.ir.meshes {
        for &at in &mesh.indices {
            let Some(point) = mesh.positions.get(at as usize) else {
                continue;
            };
            let at = frame.transform_point3(Vec3::from_array(*point));
            read.entry(key(at)).or_insert_with(|| light_near(at));
        }
    }
    let mut above: Vec<f32> = (read.values().flatten())
        .map(|light| luma(light[crate::light_probes::FROM_ABOVE]))
        .collect();
    above.sort_by(f32::total_cmp);
    let middle = above.get(above.len() / 2).copied().unwrap_or(0.0);
    let reading = |at: Vec3| -> [Vec3; 6] {
        match read.get(&key(at)).copied().flatten() {
            Some(light)
                if luma(light[crate::light_probes::FROM_ABOVE]) >= POCKET_SHARE * middle =>
            {
                light
            }
            _ => *base,
        }
    };
    let shade = |normal: [f32; 3], at: Vec3| {
        let light = reading(at);
        let [x, y, z] = normal;
        let length = (x * x + y * y + z * z).sqrt();
        if length <= 0.0 {
            return [0u8; 3];
        }
        let [x, y, z] = [x / length, y / length, z / length];
        // The sides in the probe's order: east, west, up, down, south,
        // north; the game's y is north.
        let east_west = if x > 0.0 { light[0] } else { light[1] };
        let up_down = if z > 0.0 { light[2] } else { light[3] };
        let north_south = if y > 0.0 { light[5] } else { light[4] };
        let lit = east_west * x * x + up_down * z * z + north_south * y * y;
        lit.to_array()
            .map(|part| (part * 255.0).round().clamp(0.0, 255.0) as u8)
    };
    lit_per_corner(made, name, &shade)
}

/// Gives the prop `made` a lightmap of its own named `name`, laid out as
/// the level's converter lays out a model's light in a room: a block of 2
/// by 2 texels for each triangle, in the order of the meshes and their
/// triangles, `across` blocks to a row. Each mesh's corners are made its
/// triangles' own, so each corner has its place in the lightmap: the
/// middle of its texel of the block (the first, second and third texel).
/// The prop then glows not at all. The bake lays a prop out the same way
/// and bakes into that lightmap (`kjk detail-bake`, `detail_bake.rs`).
/// Gives the lightmap's side in texels.
pub fn lay_out(made: &mut Placed, name: &str) -> usize {
    let triangles: usize = (made.ir.meshes.iter())
        .map(|mesh| mesh.indices.len() / 3)
        .sum();
    let across = (triangles as f32).sqrt().ceil().max(1.0) as usize;
    let side = across * GRID_BLOCK;
    let mut block = 0usize;
    for mesh in &mut made.ir.meshes {
        let mut positions = Vec::with_capacity(mesh.indices.len());
        let (mut normals, mut uv1, mut uv2) = (Vec::new(), Vec::new(), Vec::new());
        for triangle in mesh.indices.as_chunks::<3>().0 {
            let (left, top) = (block % across * GRID_BLOCK, block / across * GRID_BLOCK);
            for (corner, &at) in triangle.iter().enumerate() {
                let at = at as usize;
                positions.push(mesh.positions[at]);
                normals.push(mesh.normals.get(at).copied().unwrap_or([0.0, 0.0, 1.0]));
                uv1.push(mesh.uv1.get(at).copied().unwrap_or([0.0; 2]));
                let (column, row) = (corner % 2, corner / 2);
                uv2.push([
                    (left + column) as f32 / side as f32 + 0.5 / side as f32,
                    (top + row) as f32 / side as f32 + 0.5 / side as f32,
                ]);
            }
            block += 1;
        }
        mesh.indices = (0..positions.len() as u32).collect();
        mesh.positions = positions;
        mesh.normals = normals;
        mesh.uv1 = uv1;
        mesh.uv2 = uv2;
        mesh.lightmap = Some(name.to_string());
        mesh.self_illum = [0.0; 3];
    }
    side
}

/// Lights the prop `made` by `shade`, the light of a corner whose normal
/// (in the game's space) and place (game space) are given, kept as a
/// lightmap of its own named `name` (`lay_out`): each triangle's block holds
/// its corners' light, and its fourth texel lies between the second and
/// third corners'. Gives the lightmap.
fn lit_per_corner(
    made: &mut Placed,
    name: &str,
    shade: &dyn Fn([f32; 3], bevy::math::Vec3) -> [u8; 3],
) -> TextureIr {
    use bevy::math::Vec3;
    let side = lay_out(made, name);
    let across = side / GRID_BLOCK;
    let mut rgba = vec![255u8; side * side * 4];
    // The prop's model turned and moved into the game.
    let frame = crate::scene::placement(made);
    let mut block = 0usize;
    for mesh in &made.ir.meshes {
        for (positions, normals) in
            (mesh.positions.as_chunks::<3>().0.iter()).zip(mesh.normals.as_chunks::<3>().0)
        {
            let (left, top) = (block % across * GRID_BLOCK, block / across * GRID_BLOCK);
            let colours: [[u8; 3]; 3] = std::array::from_fn(|corner| {
                let normal = frame.transform_vector3(Vec3::from_array(normals[corner]));
                let at = frame.transform_point3(Vec3::from_array(positions[corner]));
                shade(normal.to_array(), at)
            });
            let fourth: [u8; 3] = std::array::from_fn(|channel| {
                ((u16::from(colours[1][channel]) + u16::from(colours[2][channel])) / 2) as u8
            });
            for (texel, colour) in [colours[0], colours[1], colours[2], fourth]
                .iter()
                .enumerate()
            {
                let (x, y) = (left + texel % 2, top + texel / 2);
                let at = (y * side + x) * 4;
                rgba[at..at + 3].copy_from_slice(colour);
            }
            block += 1;
        }
    }
    lightmap_of(side as u32, side as u32, rgba)
}

/// A lightmap of `width` by `height` texels from its RGBA bytes.
fn lightmap_of(width: u32, height: u32, rgba: Vec<u8>) -> TextureIr {
    TextureIr {
        width,
        height,
        encoding: asset_odyssey::texture::Encoding::Rgba8,
        mips: vec![rgba],
        cube: false,
        frames: 1,
        facts: asset_odyssey::texture::TextureFacts::default(),
        faces: Vec::new(),
        frame_mips: Vec::new(),
    }
}

/// The name of the lightmap a bake gives `prop` (`kjk detail-bake`): its
/// name in lower case after `jprop_`, each character that is not a letter,
/// a digit, `-` or `_` made `_`.
pub fn baked_name(prop: &Prop) -> String {
    let name: String = (prop.name.chars())
        .map(|c| match c {
            'a'..='z' | 'A'..='Z' | '0'..='9' | '-' | '_' => c.to_ascii_lowercase(),
            _ => '_',
        })
        .collect();
    format!("jprop_{name}")
}

/// The file beside a bake's lightmaps that lists the props it baked
/// (`kjk detail-bake`), one line each: the lightmap's name, the prop's
/// place (three numbers), its yaw, its scale and its model.
pub const BAKED_PROPS: &str = "props.txt";

/// The line of `BAKED_PROPS` for `prop`, whose lightmap is `name`.
pub fn baked_line(prop: &Prop, name: &str) -> String {
    let [x, y, z] = prop.position;
    format!(
        "{name} {x:.3} {y:.3} {z:.3} {:.2} {:.3} {}",
        prop.yaw, prop.scale, prop.model
    )
}

/// The lightmap a bake gives `prop`, when a folder of baked lightmaps
/// holds one for it as the prop stands now: its line of `BAKED_PROPS`
/// matches the prop's line (`baked_line`), and its picture reads. A prop
/// moved, turned, scaled or given another model since the bake takes none,
/// and is lit by the probes (`lit_by_probes`).
fn baked_lightmap(prop: &Prop) -> Option<TextureIr> {
    let name = baked_name(prop);
    let line = baked_line(prop, &name);
    // The last folder first, as the rooms' lightmaps are taken
    // (`viewer::detail_baked`): a project's bake wins over an earlier one.
    for folder in crate::viewer::bake_folders().into_iter().rev() {
        let Ok(text) = std::fs::read_to_string(folder.join(BAKED_PROPS)) else {
            continue;
        };
        if !text.lines().any(|listed| listed.trim() == line) {
            continue;
        }
        let path = folder.join(format!("{name}.png"));
        match image::open(&path) {
            Ok(picture) => {
                let picture = picture.into_rgba8();
                return Some(lightmap_of(
                    picture.width(),
                    picture.height(),
                    picture.into_raw(),
                ));
            }
            Err(error) => {
                bevy::log::warn!("baked lightmap {} does not read: {error}", path.display());
            }
        }
    }
    None
}

/// A prop's model read once: its meshes at scale 1 (metres) and the
/// textures they name.
#[derive(Clone)]
pub struct PropModel {
    pub ir: ModelIr,
    pub textures: BTreeMap<String, TextureIr>,
}

/// Each vertex's normal: the sum of the normals of the faces that use it,
/// each as long as its face is large, made one long. `triangles` wind
/// counter-clockwise seen from outside (`wound`).
fn normals(positions: &[[f32; 3]], triangles: &[[u32; 3]]) -> Vec<[f32; 3]> {
    let mut sums = vec![[0.0f32; 3]; positions.len()];
    for triangle in triangles {
        let [a, b, c] = triangle.map(|at| positions.get(at as usize).copied());
        let (Some(a), Some(b), Some(c)) = (a, b, c) else {
            continue;
        };
        let u = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
        let v = [c[0] - a[0], c[1] - a[1], c[2] - a[2]];
        let n = [
            u[1] * v[2] - u[2] * v[1],
            u[2] * v[0] - u[0] * v[2],
            u[0] * v[1] - u[1] * v[0],
        ];
        for at in triangle {
            if let Some(sum) = sums.get_mut(*at as usize) {
                for axis in 0..3 {
                    sum[axis] += n[axis];
                }
            }
        }
    }
    sums.into_iter()
        .map(|sum| {
            let length = (sum[0] * sum[0] + sum[1] * sum[1] + sum[2] * sum[2]).sqrt();
            if length > 0.0 {
                sum.map(|value| value / length)
            } else {
                [0.0, 0.0, 1.0]
            }
        })
        .collect()
}

/// An MD3 surface's triangles as the scene draws them. An MD3's triangles
/// wind clockwise seen from outside, as Quake 3's do. The scene takes a
/// triangle that winds counter-clockwise toward the camera as its front and
/// leaves out its back, so as stored, every face toward the camera was left
/// out and the model's inside showed.
fn wound(triangles: &[[u32; 3]]) -> Vec<[u32; 3]> {
    triangles.iter().map(|&[a, b, c]| [a, c, b]).collect()
}

/// An MD3 as a model of one node, in metres. A surface named as JA's tags
/// are (`tag_…`) or with no triangles is left out.
pub fn model(name: &str, md3: &fmt_ja::md3::Md3) -> ModelIr {
    let meshes = md3
        .surfaces
        .iter()
        .filter(|surface| !surface.name.to_ascii_lowercase().starts_with("tag_"))
        .filter(|surface| !surface.triangles.is_empty())
        .map(|surface| (surface, wound(&surface.triangles)))
        .map(|(surface, triangles)| MeshIr {
            node: 0,
            positions: surface
                .positions
                .iter()
                .map(|point| point.map(|value| value * collision::UNIT))
                .collect(),
            normals: normals(&surface.positions, &triangles),
            uv1: surface.uvs.clone(),
            uv2: Vec::new(),
            indices: triangles.iter().flatten().copied().collect(),
            texture: surface
                .shaders
                .first()
                .map(|shader| fmt_ja::game_path(shader))
                .filter(|shader| !shader.is_empty()),
            lightmap: None,
            transparency_hint: 0,
            skinned: false,
            shadow: true,
            self_illum: [PROP_GLOW; 3],
            alpha: 1.0,
            ambient: [1.0; 3],
            diffuse: [1.0; 3],
            uv_scroll: None,
            skin: None,
            dangly: None,
            referenced: false,
        })
        .collect();
    ModelIr {
        name: name.to_string(),
        supermodel: None,
        nodes: vec![NodeIr {
            name: name.to_string(),
            parent: None,
            translation: [0.0; 3],
            rotation: [0.0, 0.0, 0.0, 1.0],
            world: IDENTITY,
            animated: false,
        }],
        meshes,
        lights: Vec::new(),
        emitters: Vec::new(),
        animations: Vec::new(),
        animation_scale: 1.0,
        affected_by_fog: true,
        refusals: Vec::new(),
        folded: Vec::new(),
    }
}

/// The model at `path` in Jedi Academy's archives, with its textures.
pub fn read(files: &Files, path: &str) -> Result<PropModel, String> {
    let bytes = files
        .read(path)
        .ok_or(format!("{path}: not in Jedi Academy's archives"))?;
    let md3 = fmt_ja::md3::parse(&bytes).map_err(|error| format!("{path}: {error}"))?;
    let ir = model(path, &md3);
    let names: Vec<String> = ir.meshes.iter().filter_map(|m| m.texture.clone()).collect();
    let (textures, _) = crate::viewer_ja::textures(files, &names);
    Ok(PropModel { ir, textures })
}

/// `prop` placed: its model scaled, at its position, turned by its yaw.
pub fn placed(model: &PropModel, prop: &Prop) -> Placed {
    let mut ir = model.ir.clone();
    for mesh in &mut ir.meshes {
        for point in &mut mesh.positions {
            *point = point.map(|value| value * prop.scale);
        }
    }
    Placed {
        facing: prop.yaw.to_radians(),
        ..Placed::at(ir, prop.position)
    }
}

/// Adds `props` to `scene`, reading each model once into `cache`. Returns
/// each prop's model index in the scene (`None` for one that does not
/// read) and why each that does not read.
pub fn add(
    scene: &mut LoadedScene,
    files: Option<&Files>,
    cache: &mut BTreeMap<String, Result<PropModel, String>>,
    props: &[Prop],
) -> (Vec<Option<usize>>, Vec<String>) {
    let mut indices = Vec::new();
    let mut notes = Vec::new();
    // Whether the area is drawn under a bake: a project's own, or the
    // lighting download's under the options' Enhanced Lightmaps
    // (`fitted_lights::mode_of_area`). `fitted_lights::baked` alone missed
    // the download's, and a prop there kept its level's light grid.
    let under_bake =
        crate::fitted_lights::mode_of_area(scene.baked) == crate::fitted_lights::Mode::Baked;
    for prop in props {
        if !cache.contains_key(&prop.model) {
            let read = match files {
                Some(files) => read(files, &prop.model),
                None => Err("Jedi Academy is not installed".to_string()),
            };
            cache.insert(prop.model.clone(), read);
        }
        match &cache[&prop.model] {
            Ok(model) => {
                for (name, texture) in &model.textures {
                    scene
                        .textures
                        .entry(name.clone())
                        .or_insert_with(|| texture.clone());
                }
                indices.push(Some(scene.models.len()));
                // Its glow is the room's light where it stands; the light
                // probes give it that light instead under Enhanced lighting
                // (`Placed::room_glow`).
                let mut made = placed(model, prop);
                // A prop in a room cut from a Jedi Academy level is lit by
                // that level's light grid, as Jedi Academy lights it, while
                // the rooms show their stored light. A bake replaces that
                // light, so under a bake the prop takes the probes' light,
                // which the bake gives them.
                let grid = (!under_bake)
                    .then(|| floor_for(scene, prop.position))
                    .flatten()
                    .and_then(|(room, _)| scene.rooms.get(room))
                    .and_then(|room| crate::ja_made::grid_light(&room.name, prop.position));
                // Under a bake that holds the prop, its own baked lightmap.
                let from_bake = under_bake.then(|| baked_lightmap(prop)).flatten();
                // Under a bake that does not, the probes' light from each
                // side, which holds the bake's light: at the prop's own
                // place, and at each corner's (`lit_by_probes`).
                let floor = (under_bake && from_bake.is_none())
                    .then(|| floor_for(scene, prop.position))
                    .flatten();
                let baked = floor.and_then(|(room, sample)| {
                    scene
                        .probes
                        .light_at(&scene.rooms, Some(room), sample)
                        .ok()
                        .map(|(light, _)| light)
                });
                if let Some(lightmap) = from_bake {
                    let name = baked_name(prop);
                    lay_out(&mut made, &name);
                    scene.textures.insert(name, lightmap);
                    // Shown as a room's baked lightmap is
                    // (`OdysseyMaterial::room_glow`).
                    made.room_glow = true;
                    scene.models.push(made);
                    continue;
                }
                match (grid, baked) {
                    (Some((light, room_yaw)), _) => {
                        let name = format!("{}#grid", prop.name);
                        let lightmap = lit_by_grid(&mut made, prop, (&light, room_yaw), &name);
                        scene.textures.insert(name, lightmap);
                        // Its lightmap stands in for the room's light, as a
                        // room's does (`OdysseyMaterial::room_glow`).
                        made.room_glow = true;
                    }
                    (None, Some(light)) => {
                        let name = format!("{}#baked", prop.name);
                        let light_near = |at: bevy::math::Vec3| {
                            let (room, sample) = floor?;
                            let at = bevy::math::Vec3::new(at.x, at.y, sample.z);
                            scene
                                .probes
                                .light_at(&scene.rooms, Some(room), at)
                                .ok()
                                .map(|(light, _)| light)
                        };
                        let lightmap = lit_by_probes(&mut made, &light, &light_near, &name);
                        scene.textures.insert(name, lightmap);
                        // Shown as a room's baked lightmap is
                        // (`OdysseyMaterial::room_glow`).
                        made.room_glow = true;
                    }
                    (None, None) => {
                        let glow = room_glow(scene, prop.position).unwrap_or([PROP_GLOW; 3]);
                        for mesh in &mut made.ir.meshes {
                            mesh.self_illum = glow;
                        }
                        made.room_glow = true;
                    }
                }
                scene.models.push(made);
            }
            Err(why) => {
                notes.push(format!("prop {}: {why}", prop.name));
                indices.push(None);
            }
        }
    }
    (indices, notes)
}

/// Adds the props of the active project (`level_edit::active`) for
/// `game`'s `module` to its scene: what play draws.
pub fn add_active(scene: &mut LoadedScene, game: vfs::Game, module: &str) {
    let props = level_edit::active::props(game.key(), module);
    if props.is_empty() {
        return;
    }
    let stack = match crate::viewer_ja::stack(&crate::discover()) {
        Ok(stack) => stack,
        Err(why) => {
            bevy::log::warn!("the project's props are not drawn: {why}");
            return;
        }
    };
    let files = Files::base(&stack);
    let mut cache = BTreeMap::new();
    let (_, notes) = add(scene, Some(&files), &mut cache, props);
    for note in notes {
        bevy::log::warn!("{note}");
    }
}
