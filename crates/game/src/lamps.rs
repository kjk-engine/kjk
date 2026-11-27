//! Lamps placed by hand (`level_edit::lamps`, `docs/LEVEL-EDIT.md`): models
//! of the game drawn where an area had no lamp, so the lighting of a level
//! can be reworked with new fittings and not only with invisible lights.
//!
//! A lamp is a whole model (a placeable's lamp) or one mesh of a model (a
//! light mesh, such as a ceiling panel of a room). Every scene of a module
//! draws its area's lamps (`place`, from `viewer::module_scene`). The fitted
//! lights then light a lamp's glowing meshes as they light a fitting of the
//! area's own (`fitted_lights`), by the kind its texture names.
//!
//! An area's lamps are, in this order of preference: the ones a program set
//! for it (`set_lamps`, the level editor's view before it saves), the ones
//! the active project's new map keeps (`level_edit::active::map_lamps`),
//! and the `[[lamp]]` tables of the light data's `fittings.toml`.
//!
//! A light mesh drops the lightmap of the room it was taken from, which was
//! baked for another place. It is lit at runtime as a placeable is. A light
//! mesh that does not glow in its model's files (a room's lamp that its
//! lightmap alone made bright) is set to glow fully, so it looks lit. No
//! lamp casts a shadow, so its own mesh never darkens its light. A lamp is
//! not solid.

use std::collections::BTreeMap;

use asset_odyssey::model::{IDENTITY, MeshIr, ModelIr, NodeIr};
use bevy::math::Vec3;
use level_edit::PlacedLamp;
use vfs::Game;
use vfs::resolver::Resolver;

use crate::scene::to_mat4;
use crate::viewer::Placed;

/// The lamps a program sets for an area in place of `fittings.toml`'s
/// (`set_lamps`), by the area's name in lower case.
static SET: std::sync::RwLock<BTreeMap<String, Vec<PlacedLamp>>> =
    std::sync::RwLock::new(BTreeMap::new());

/// Gives the area `area` these lamps in place of the ones `fittings.toml`
/// holds for it, or (`None`) the file's again. The level editor sets the
/// lamps it edits, so its view shows them before they are saved.
pub fn set_lamps(area: &str, lamps: Option<Vec<PlacedLamp>>) {
    let mut set = SET
        .write()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    match lamps {
        Some(lamps) => {
            set.insert(area.to_ascii_lowercase(), lamps);
        }
        None => {
            set.remove(&area.to_ascii_lowercase());
        }
    }
}

/// The lamps of `game`'s area `area` (its module, as the game names it).
pub fn of_area(game: Game, area: &str) -> Vec<PlacedLamp> {
    if let Some(set) = SET
        .read()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .get(&area.to_ascii_lowercase())
    {
        return set.clone();
    }
    if let Some(lamps) = level_edit::active::map_lamps(game.key(), area) {
        return lamps.to_vec();
    }
    // The light data's lamps belong to Enhanced lighting. Under Classic
    // the level is drawn as the original game draws it, without them. A
    // project's own lamps (above) are part of its level and stay.
    if crate::fitted_lights::classic() {
        return Vec::new();
    }
    let fittings = crate::fitted_lights::fittings();
    let lamps = (fittings.lamps.as_ref())
        .map_err(String::clone)
        .and_then(|tables| tables.of_area(game.key(), area));
    match lamps {
        Ok(lamps) => lamps,
        Err(why) => {
            bevy::log::warn!("the lamps of {area} do not read: {why}");
            Vec::new()
        }
    }
}

/// How a lamp's mesh is drawn: without the lightmap of the place it came
/// from, and casting no shadow. A light mesh (`glow`) that does not glow
/// in its files glows fully.
fn lamp_look(mesh: &mut MeshIr, glow: bool) {
    mesh.lightmap = None;
    mesh.uv2.clear();
    mesh.shadow = false;
    if glow && mesh.self_illum.iter().all(|&v| v <= 0.0) {
        mesh.self_illum = [1.0; 3];
    }
}

/// `lamp`'s model as it is drawn: the whole model, or its one mesh with
/// the mesh's middle at the model's origin.
pub fn model(resolver: &Resolver, lamp: &PlacedLamp) -> Result<ModelIr, String> {
    let mut ir = crate::viewer::model(resolver, &lamp.model)?
        .ok_or_else(|| format!("the install has no model {}", lamp.model))?;
    if lamp.mesh.is_empty() {
        for mesh in &mut ir.meshes {
            lamp_look(mesh, false);
        }
        return Ok(ir);
    }
    light_mesh(ir, &lamp.mesh)
}

/// The mesh of `ir`'s node `node` alone, as a model of one node: its
/// vertices in the model's space, then moved so the middle of its bounds is
/// at the origin.
pub fn light_mesh(ir: ModelIr, node: &str) -> Result<ModelIr, String> {
    let at = (ir.nodes.iter())
        .position(|found| found.name.eq_ignore_ascii_case(node))
        .ok_or_else(|| format!("the model {} has no node {node}", ir.name))?;
    let world = to_mat4(&ir.nodes[at].world);
    let mut meshes: Vec<MeshIr> = (ir.meshes.into_iter())
        .filter(|mesh| mesh.node == at && mesh.skin.is_none())
        .collect();
    if meshes.is_empty() {
        return Err(format!("the node {node} of {} has no mesh", ir.name));
    }
    let (mut low, mut high) = (Vec3::splat(f32::INFINITY), Vec3::splat(f32::NEG_INFINITY));
    for mesh in &mut meshes {
        for point in &mut mesh.positions {
            let moved = world.transform_point3(Vec3::from_array(*point));
            (low, high) = (low.min(moved), high.max(moved));
            *point = moved.to_array();
        }
        for normal in &mut mesh.normals {
            *normal = (world.transform_vector3(Vec3::from_array(*normal)))
                .normalize_or_zero()
                .to_array();
        }
    }
    let middle = if low.is_finite() && high.is_finite() {
        (low + high) * 0.5
    } else {
        Vec3::ZERO
    };
    for mesh in &mut meshes {
        for point in &mut mesh.positions {
            *point = (Vec3::from_array(*point) - middle).to_array();
        }
        mesh.node = 0;
        mesh.dangly = None;
        lamp_look(mesh, true);
    }
    let name = format!("{}:{node}", ir.name);
    Ok(ModelIr {
        name: name.clone(),
        supermodel: None,
        nodes: vec![NodeIr {
            name,
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
        affected_by_fog: ir.affected_by_fog,
        refusals: Vec::new(),
        folded: Vec::new(),
    })
}

/// A lamp's pitch and roll as the turn its model takes under its yaw
/// (`Placed::attach`, which `scene::placement` applies after the facing):
/// its roll about X first, then its pitch about Y. `None` for a lamp that
/// is neither tipped nor rolled.
pub fn tilt(lamp: &PlacedLamp) -> Option<asset_odyssey::model::Matrix> {
    if lamp.pitch == 0.0 && lamp.roll == 0.0 {
        return None;
    }
    let turn = bevy::math::Quat::from_rotation_y(lamp.pitch.to_radians())
        * bevy::math::Quat::from_rotation_x(lamp.roll.to_radians());
    // `Matrix` is kept by rows (`scene::to_mat4`).
    Some(
        bevy::math::Mat4::from_quat(turn)
            .transpose()
            .to_cols_array_2d(),
    )
}

/// Adds `game`'s area `module`'s lamps to `models` (a module's scene
/// before its textures are read). Returns each lamp's model index (`None`
/// for one whose model does not read) and whether its glowing texture is a
/// placeable's kind, in the order the area lists them
/// (`LoadedScene::lamps`).
pub fn place(
    resolver: &Resolver,
    game: Game,
    module: &str,
    models: &mut Vec<Placed>,
) -> Vec<(Option<usize>, bool)> {
    let mut read: BTreeMap<(String, String), Option<ModelIr>> = BTreeMap::new();
    let mut placed = Vec::new();
    for lamp in of_area(game, module) {
        let key = (
            lamp.model.to_ascii_lowercase(),
            lamp.mesh.to_ascii_lowercase(),
        );
        let ir = read.entry(key).or_insert_with(|| {
            model(resolver, &lamp)
                .map_err(|why| bevy::log::warn!("lamp {} is not drawn: {why}", lamp.label()))
                .ok()
        });
        let index = ir.as_ref().map(|ir| {
            models.push(Placed {
                facing: lamp.yaw.to_radians(),
                attach: tilt(&lamp),
                ..Placed::at(ir.clone(), lamp.at)
            });
            models.len() - 1
        });
        placed.push((index, lamp.on_placeable));
    }
    placed
}
