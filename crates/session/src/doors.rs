//! Which ways each door opens (`sim::module::Ways`, QUIRKS.md Q-0025):
//! both when its two open walkmeshes (`<model>1.dwk`, `<model>2.dwk`, in
//! its own frame) stand on opposite sides of its closed one (`<model>0`),
//! each its middle at least `SIDE_M` off it, and its model has a second
//! opening to show it (`opening2`, its own or a supermodel's). K1's and
//! TSL's door models all slide or vanish in place, both openings the same:
//! every shipped door opens one way.

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
    for (placement, instance) in data.placements.iter_mut().zip(&area.instances) {
        let (Placement::Own(facts), Kind::Door, Some(model)) =
            (placement, instance.kind, &instance.model)
        else {
            continue;
        };
        facts.ways = *known.entry(model).or_insert_with(|| ways(source, model));
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
