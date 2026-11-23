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
/// alone). `None` where no probe with light is near.
fn room_glow(scene: &LoadedScene, at: [f32; 3]) -> Option<[f32; 3]> {
    let mut under: Option<(usize, f32)> = None;
    for (index, room) in scene.rooms.iter().enumerate() {
        for [a, b, c] in &room.floor {
            let Some(z) = crate::scene::height_at(*a, *b, *c, at[0], at[1]) else {
                continue;
            };
            if z <= at[2] + FLOOR_ABOVE
                && z >= at[2] - FLOOR_BELOW
                && under.is_none_or(|(_, higher)| z > higher)
            {
                under = Some((index, z));
            }
        }
    }
    let height = under.map_or(at[2], |(_, floor)| floor);
    let sample = bevy::math::Vec3::new(at[0], at[1], height + PROBE_RISE);
    let light = scene
        .probes
        .light_at(&scene.rooms, under.map(|(room, _)| room), sample)
        .ok()?;
    use crate::fitted_lights::Mode;
    let kept = match crate::fitted_lights::mode() {
        Mode::Off | Mode::Baked => 1.0,
        Mode::Live => 0.5,
        Mode::LampsOnly => 0.0,
    };
    Some(
        (light[crate::light_probes::FROM_ABOVE] * kept)
            .min(bevy::math::Vec3::ONE)
            .to_array(),
    )
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
                let glow = room_glow(scene, prop.position).unwrap_or([PROP_GLOW; 3]);
                let mut made = placed(model, prop);
                for mesh in &mut made.ir.meshes {
                    mesh.self_illum = glow;
                }
                made.room_glow = true;
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
