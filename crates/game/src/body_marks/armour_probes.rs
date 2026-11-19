//! A bolt burn on armour, with a K1 install. The Sith trooper
//! (`g_sithtroop01`) wears armour made of meshes that hang rigidly from its
//! bones, with no skin. Its body is loaded the way the scene loads it, posed
//! at rest, and a pistol's bolt strikes its chest from the front.

use bevy::asset::RenderAssetUsages;
use bevy::prelude::*;

use super::{
    BOLT_DEPTH, BOLT_SIZE, Posed, UNIT, burn_place, cover, mark_axes, nearest_on_triangle,
};
use crate::bolt_hits::BurnSize;
use crate::probe_kit::{Checks, check, not_installed};
use crate::scene::{mesh_usage, odyssey_mesh, placement, to_mat4};
use crate::viewer::{CreatureLoad, Placed, resolver};

const TEMPLATE: &str = "g_sithtroop01";
/// The node the bolt aims at: the upper torso.
const CHEST: &str = "torsoupr_g";
/// How far the burn's triangles may lie from the armour (metres).
const NEAR_M: f32 = 0.05;

/// The checks, for `armour_burns` in the approved tests.
pub fn probes() -> Checks {
    vec![("trooper_chest_burn", trooper_chest_burn())]
}

/// A mesh of a placed model at rest: which model it belongs to, whether it
/// is drawn, and how the scene keeps it.
struct RestMesh {
    model: usize,
    name: String,
    drawn: bool,
    kept: bool,
}

/// The models' meshes as the scene makes them, posed at rest (game metres).
/// A mesh the scene keeps only where it is drawn has no copy to read, so it
/// is left out of the posed meshes, as `Bodies::posed` leaves it out.
fn rest_meshes(models: &[Placed], meshes: &mut Assets<Mesh>) -> (Vec<Posed>, Vec<RestMesh>) {
    let mut posed = Vec::new();
    let mut facts = Vec::new();
    for (model, placed) in models.iter().enumerate() {
        let root = placement(placed);
        for part in &placed.ir.meshes {
            let usage = mesh_usage(placed, part);
            let kept = usage.contains(RenderAssetUsages::MAIN_WORLD);
            let node = &placed.ir.nodes[part.node];
            facts.push(RestMesh {
                model,
                name: node.name.clone(),
                drawn: part.alpha > 0.0,
                kept,
            });
            if !kept {
                continue;
            }
            let world = root * to_mat4(&node.world);
            posed.push(Posed {
                entity: Entity::from_bits(facts.len() as u64),
                mesh: meshes.add(odyssey_mesh(part, usage, false, None)),
                skin: None,
                at: part
                    .positions
                    .iter()
                    .map(|&p| world.transform_point3(Vec3::from_array(p)))
                    .collect(),
                head_mesh: placed.attach.is_some(),
                head_vertex: vec![false; part.positions.len()],
            });
        }
    }
    (posed, facts)
}

/// With a K1 install, a pistol's bolt from 2 m in front of the trooper's
/// chest leaves a burn. At least one mark mesh is made. Each is made from a
/// drawn mesh of the body model (the mark's parent), not of the head. Every
/// triangle of the marks lies within 5 cm of the armour.
fn trooper_chest_burn() -> Result<String, String> {
    let discovery = crate::discover();
    let game = vfs::Game::Kotor1;
    if discovery.get(game).is_none() {
        return Ok(not_installed(game));
    }
    let resolver = resolver(&discovery, game, None)?;
    let mut models = Vec::new();
    CreatureLoad::default()
        .place(
            (&resolver, game),
            (1, TEMPLATE),
            ([0.0; 3], 0.0),
            &mut models,
        )?
        .ok_or(format!("{TEMPLATE} is not drawn"))?;
    let body = models
        .iter()
        .position(|placed| placed.attach.is_none() && placed.hook.is_none())
        .ok_or("no body model")?;
    let chest = models[body]
        .ir
        .nodes
        .iter()
        .find(|node| node.name.eq_ignore_ascii_case(CHEST))
        .map(|node| {
            (placement(&models[body]) * to_mat4(&node.world))
                .w_axis
                .truncate()
        })
        .ok_or(format!("no {CHEST} node"))?;
    let mut meshes = Assets::<Mesh>::default();
    let (posed, facts) = rest_meshes(&models, &mut meshes);
    let body_facts = facts.iter().filter(|fact| fact.model == body);
    let kept = body_facts.clone().filter(|fact| fact.kept).count();
    let unkept: Vec<&str> = body_facts
        .filter(|fact| !fact.kept)
        .map(|fact| fact.name.as_str())
        .collect();
    // Models face the game's +Y, so the bolt comes from +Y.
    let way = Vec3::NEG_Y;
    let from = chest - way * 2.0;
    let at = burn_place(&posed, &meshes, (from, way), chest).ok_or("the burn has no place")?;
    let size = BOLT_SIZE[BurnSize::Pistol as usize];
    let half = (size.0 + size.1) / 2.0 * UNIT;
    let axes = mark_axes(way, 0.0, (half, half));
    let index = |posed: &Posed| posed.entity.to_bits() as usize - 1;
    let skin: Vec<[Vec3; 3]> = posed
        .iter()
        .flat_map(|posed| {
            let corners = meshes
                .get(&posed.mesh)
                .and_then(super::index_list)
                .unwrap_or_else(Vec::new);
            corners
                .as_chunks::<3>()
                .0
                .iter()
                .map(|t| t.map(|v| posed.at[v]))
                .collect::<Vec<_>>()
        })
        .collect();
    let covered = cover(posed, &meshes, (at, way), axes, BOLT_DEPTH, true);
    let parents: Vec<&RestMesh> = covered
        .iter()
        .map(|(posed, _)| &facts[index(posed)])
        .collect();
    let on_body = parents.iter().all(|fact| fact.model == body && fact.drawn);
    let farthest_cm = covered
        .iter()
        .flat_map(|(posed, made)| {
            let corners = super::index_list(made).unwrap_or_else(Vec::new);
            // `gore` keeps the order of the vertices it keeps: the made
            // mesh's vertices are the posed mesh's, picked again.
            let order = meshes
                .get(&posed.mesh)
                .map_or_else(Vec::new, |mesh| picked_order(mesh, made));
            corners
                .as_chunks::<3>()
                .0
                .iter()
                .filter_map(|t| {
                    let corners = t.map(|v| order.get(v).and_then(|&v| posed.at.get(v)));
                    let [Some(a), Some(b), Some(c)] = corners else {
                        return None;
                    };
                    Some((*a + *b + *c) / 3.0)
                })
                .collect::<Vec<_>>()
        })
        .map(|middle| {
            skin.iter()
                .map(|&t| nearest_on_triangle(middle, t).distance(middle))
                .fold(f32::MAX, f32::min)
        })
        .fold(0.0, f32::max)
        * 100.0;
    let names: Vec<&str> = parents.iter().map(|fact| fact.name.as_str()).collect();
    check(
        !covered.is_empty() && on_body && farthest_cm <= NEAR_M * 100.0,
        format!(
            "{kept} body meshes keep a copy to read; not kept: {unkept:?}; the bolt meets the \
             body at {at}; the burn is made on {names:?}, each a drawn body mesh {on_body}; its \
             farthest triangle is {farthest_cm:.1} cm from the armour"
        ),
    )
}

/// For each vertex of `made` (a mark `gore` made from `mesh`), the vertex of
/// `mesh` it was copied from, found by its position.
fn picked_order(mesh: &Mesh, made: &Mesh) -> Vec<usize> {
    let positions = |mesh: &Mesh| match mesh.attribute(Mesh::ATTRIBUTE_POSITION) {
        Some(bevy::mesh::VertexAttributeValues::Float32x3(all)) => all.clone(),
        _ => Vec::new(),
    };
    let (all, picked) = (positions(mesh), positions(made));
    picked
        .iter()
        .map(|p| all.iter().position(|q| q == p).unwrap_or(0))
        .collect()
}
