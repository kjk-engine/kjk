//! Which ways each door opens (`sim::module::Ways`, QUIRKS.md Q-0025):
//! both when its two open walkmeshes (`<model>1.dwk`, `<model>2.dwk`, in
//! its own frame) stand on opposite sides of its closed one (`<model>0`),
//! each its middle at least `SIDE_M` off it, and its model has a second
//! opening to show it (`opening2`, its own or a supermodel's). K1's and
//! TSL's door models all slide or vanish in place, both openings the same:
//! every shipped door opens one way.
//!
//! And each placeable's and door's action points (`ObjectData::use_points`):
//! the two use positions its walkmesh keeps (`<model>.pwk`, a door's closed
//! `<model>0.dwk`).

use std::collections::BTreeMap;

use asset_odyssey::module::{AreaIr, Kind, Source};
use fmt_odyssey::ResType;
use fmt_odyssey::bwm::Walkmesh;
use fmt_odyssey::mdl::Model;
use sim::module::{ModuleData, Placement, Ways};

/// How far off its closed doorway an open walkmesh's middle must stand to
/// open to a side (metres): a swung leaf's is half its width out.
const SIDE_M: f32 = 0.25;

/// How deep a model's supermodels are looked through for its clip.
const SUPERMODELS: usize = 4;

/// Sets the ways of `data`'s doors, placed as `area` places them (the
/// placements in its instances' order).
pub(crate) fn fill(source: &dyn Source, data: &mut ModuleData, area: &AreaIr) {
    let mut known: BTreeMap<&str, Ways> = BTreeMap::new();
    let mut points: BTreeMap<&str, Vec<[f32; 3]>> = BTreeMap::new();
    let mut door_points: BTreeMap<&str, Vec<[f32; 3]>> = BTreeMap::new();
    for (placement, instance) in data.placements.iter_mut().zip(&area.instances) {
        let (Placement::Own(facts), Some(model)) = (placement, &instance.model) else {
            continue;
        };
        match instance.kind {
            Kind::Door => {
                facts.ways = *known.entry(model).or_insert_with(|| ways(source, model));
                facts.use_points = door_points
                    .entry(model)
                    .or_insert_with(|| door_use_points(source, model))
                    .clone();
            }
            Kind::Placeable => {
                facts.use_points = points
                    .entry(model)
                    .or_insert_with(|| use_points(source, model))
                    .clone();
            }
            _ => {}
        }
    }
}

/// A placeable model's action points: its walkmesh's two relative use
/// positions (the header's first two), the second only when it is not the
/// origin, none when the first is (TSL's
/// `CSWSPlaceable::GetNearestActionPoint` then takes the placeable's
/// position) [verify in K1's exe].
pub(crate) fn use_points(source: &dyn Source, model: &str) -> Vec<[f32; 3]> {
    walkmesh_use_points(source, &model.to_ascii_lowercase(), ResType::PWK)
}

/// A door model's action points: its closed walkmesh's (`<model>0.dwk`),
/// as above; the exes' `CSWSDoor::GetActionPoint` reads the use positions
/// of the walkmesh of the state asked for, and a use asks the closed one
/// (`CSWSCreature::GetUseRange`).
fn door_use_points(source: &dyn Source, model: &str) -> Vec<[f32; 3]> {
    walkmesh_use_points(
        source,
        &format!("{}0", model.to_ascii_lowercase()),
        ResType::DWK,
    )
}

fn walkmesh_use_points(source: &dyn Source, name: &str, kind: ResType) -> Vec<[f32; 3]> {
    let Some(walkmesh) = source
        .read(name, kind)
        .and_then(|bytes| Walkmesh::parse(&bytes).ok())
    else {
        return Vec::new();
    };
    let [first, second, ..] = walkmesh.use_positions;
    match (first == [0.0; 3], second == [0.0; 3]) {
        (true, _) => Vec::new(),
        (false, true) => vec![first],
        (false, false) => vec![first, second],
    }
}

fn ways(source: &dyn Source, model: &str) -> Ways {
    let walkmesh = |state: u8| {
        let bytes = source.read(&format!("{model}{state}"), ResType::DWK)?;
        Walkmesh::parse(&bytes).ok()
    };
    let middle = |walkmesh: &Walkmesh| {
        let count = walkmesh.vertices.len().max(1) as f32;
        walkmesh.vertices.iter().map(|v| v[1]).sum::<f32>() / count
    };
    let (Some(first), Some(second)) = (walkmesh(1), walkmesh(2)) else {
        return Ways::One;
    };
    let doorway = walkmesh(0).map_or(0.0, |closed| middle(&closed));
    let (first, second) = (middle(&first) - doorway, middle(&second) - doorway);
    let sides = first.abs() >= SIDE_M && second.abs() >= SIDE_M && (first > 0.0) != (second > 0.0);
    match sides && has_clip(source, model, "opening2") {
        false => Ways::One,
        true if first > 0.0 => Ways::FirstToFront,
        true => Ways::FirstToBack,
    }
}

/// Whether `model` or one of its supermodels has the clip `name`.
fn has_clip(source: &dyn Source, model: &str, name: &str) -> bool {
    let mut next = Some(model.to_ascii_lowercase());
    for _ in 0..=SUPERMODELS {
        let Some(at) = next.take() else {
            return false;
        };
        let Some(mdl) = source.read(&at, ResType::MDL) else {
            return false;
        };
        let mdx = source.read(&at, ResType::MDX).unwrap_or_else(Vec::new);
        let Ok(parsed) = Model::parse(&mdl, &mdx) else {
            return false;
        };
        if parsed.animations.iter().any(|clip| clip.name == name) {
            return true;
        }
        next = parsed
            .supermodel
            .map(|name| name.trim().to_ascii_lowercase())
            .filter(|name| !name.is_empty() && name != "null" && *name != at);
    }
    false
}
