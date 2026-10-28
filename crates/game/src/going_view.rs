//! Creatures and placeables a destroy took out of the world, drawn going
//! as KOTOR's client fades the model it still holds: the snapshot carries
//! each as it went while it departs (`Snapshot::departing`,
//! `sim::Departing`), drawn where it stood at what is left of its fade,
//! its meshes given blended materials of their own, until it is done. The
//! dead fade as they are taken away, still in the world until their fade
//! ends (`sim::Going::Fades`, `sim::remains`: their appearance's
//! `fadedelayondeath`, the engine's own where blank); what a script's
//! `DestroyObject` takes is drawn whole for its `fDelayUntilFade`, then
//! fades over the engine's fade, or goes at once for `bNoFade`. A
//! creature's remains stand once it has gone (`made`).
//!
//! The dead lie on, the owner's rule over KOTOR's (2026-10-11): the sim
//! takes a dead body away as KOTOR does, scripts seeing it gone then, but
//! the view draws it whole where it lies, its remains at its feet, for
//! `LIE_MS` more (`Fading::lay`), then fades it over `LIE_FADE_MS` and lets
//! it go (`Fading::done`). Past `MOST_LYING` the longest lying fade at once.
//! A new scene (another module, a load) starts with none lying: the
//! remains stand alone. A corpse row's body (the rancor's, whose remains
//! are its corpse) fades as KOTOR's does.
//!
//! A placeable the module placed is drawn while the snapshot draws it:
//! once one it had is gone (a script destroyed it, and it is done
//! departing), its model is hidden.

use std::collections::{BTreeMap, BTreeSet};

use bevy::prelude::*;
use sim::SimId;
use sim::module::ObjectKind;

use crate::cast::BodyRoots;
use crate::made::Made;
use crate::material::{OdysseyMaterial, Surface};
use crate::scene::{SceneContent, SceneModel};
use crate::sim_host::{PresentedSnapshot, SceneSwap};

/// How long a dead body lies drawn once the sim has taken it (sim ms), and
/// how long it then fades.
pub const LIE_MS: u64 = 120_000;
const LIE_FADE_MS: u64 = 1000;
/// The most bodies left lying at once.
pub const MOST_LYING: usize = 10;

/// The things fading, by entity: each mesh's own material and the alpha it
/// had; the module's placeables seen and hidden in this scene; and the dead
/// left lying, longest lying first.
#[derive(Resource, Default)]
pub struct Fading {
    meshes: BTreeMap<SimId, Vec<(Handle<OdysseyMaterial>, f32)>>,
    generation: u32,
    placed_seen: BTreeSet<u32>,
    placed_hidden: BTreeSet<u32>,
    lying: Vec<Lying>,
}

/// A dead body the sim took away, drawn lying: whose, its root, when it
/// starts fading (sim ms), and how it goes once faded.
pub struct Lying {
    pub id: SimId,
    pub root: Entity,
    fades_ms: u64,
    pub leaves: Leaves,
}

/// How a lying body goes: a module's creature's model hidden (its scene
/// model, its object), or a creature made in play despawned.
pub enum Leaves {
    Placed(usize, u32),
    Made(crate::made::Departure),
}

impl Fading {
    /// For scene swap `generation`: everything from another starts over
    /// (its bodies went with its scene).
    pub fn starts(&mut self, generation: u32) {
        if self.generation != generation {
            *self = Fading {
                generation,
                ..Fading::default()
            };
        }
    }

    /// `id`'s body (`root`) taken from the world at `now_ms`, left lying;
    /// past `MOST_LYING`, the longest lying fade from now.
    pub fn lay(&mut self, (id, root): (SimId, Entity), leaves: Leaves, now_ms: u64) {
        self.lying.push(Lying {
            id,
            root,
            fades_ms: now_ms + LIE_MS,
            leaves,
        });
        let over = self.lying.len().saturating_sub(MOST_LYING);
        for lying in &mut self.lying[..over] {
            lying.fades_ms = lying.fades_ms.min(now_ms);
        }
    }

    /// How many bodies lie (fading ones too).
    pub fn lying(&self) -> usize {
        self.lying.len()
    }

    /// The bodies faded out by `now_ms`, done lying.
    pub fn done(&mut self, now_ms: u64) -> Vec<Lying> {
        let (done, lying) = std::mem::take(&mut self.lying)
            .into_iter()
            .partition(|lying| now_ms >= lying.fades_ms + LIE_FADE_MS);
        self.lying = lying;
        done
    }

    /// The lying bodies fading at `now_ms`: whose, their roots, how opaque.
    fn lying_fades(&self, now_ms: u64) -> impl Iterator<Item = (SimId, Entity, f32)> + '_ {
        self.lying
            .iter()
            .filter(move |lying| now_ms > lying.fades_ms)
            .map(move |lying| {
                let faded = (now_ms - lying.fades_ms) as f32 / LIE_FADE_MS as f32;
                (lying.id, lying.root, 1.0 - faded.min(1.0))
            })
    }
}

/// Whether a dead creature (`facts`) of `level` lies on once taken: all but
/// a corpse row's, whose remains are its corpse.
pub fn lies_on(level: &sim::Level, facts: Option<&sim::module::ObjectData>) -> bool {
    let row = facts
        .and_then(|facts| facts.body)
        .map_or(0, |body| body.bag);
    !level
        .module()
        .and_then(|module| module.bags.get(usize::from(row)))
        .is_some_and(|bag| bag.corpse)
}

/// Each frame: the things going drawn as faded as their time left says
/// (the dead taken away whole, those done lying faded), and the module's
/// placeables gone hidden.
#[allow(clippy::too_many_arguments)]
pub fn fade(
    presented: Res<PresentedSnapshot>,
    swap: Res<SceneSwap>,
    scene: Res<SceneContent>,
    (roots, made): (Res<BodyRoots>, Option<Res<Made>>),
    scene_models: Query<(Entity, &SceneModel)>,
    children: Query<&Children>,
    (mut meshes, mut visibility): (
        Query<&mut MeshMaterial3d<OdysseyMaterial>>,
        Query<&mut Visibility>,
    ),
    mut materials: ResMut<Assets<OdysseyMaterial>>,
    mut fading: ResMut<Fading>,
) {
    let snapshot = presented.0.current();
    fading.starts(swap.generation);
    let objects = sim::Instances::in_snapshot(&swap.level, snapshot);
    // A dead body taken away is drawn whole: it lies on once gone.
    let lies = |entity: &sim::EntityState| {
        matches!(entity.kind, sim::Kind::Object(ObjectKind::Creature, _))
            && matches!(entity.going, Some(sim::Going::Fades(_)))
            && lies_on(&swap.level, objects.of_entity(entity))
    };
    let placed_root = |index: u32| {
        let model = *scene.0.instance_models.get(&index)?;
        scene_models
            .iter()
            .find(|(_, scene_model)| scene_model.index == model)
            .map(|(root, _)| root)
    };
    // Doors share the instance indices but are never hidden: a guest may
    // present the old module's snapshot over the new scene for a while.
    let placeable_root = |index: u32| {
        if scene.0.doors.iter().any(|door| door.object == index) {
            return None;
        }
        placed_root(index)
    };
    hide_placed_gone(snapshot, &mut fading, placeable_root, &mut visibility);
    // The dead fading in the world, then those departing, then those done
    // lying.
    let in_world = snapshot
        .entities
        .iter()
        .filter(|entity| !lies(entity))
        .filter_map(|entity| Some((entity, entity.going?.alpha(entity.life_ms?))));
    let departing = snapshot
        .departing
        .iter()
        .map(|departing| (&departing.entity, departing.alpha()));
    let lying: Vec<_> = fading.lying_fades(snapshot.time_ms).collect();
    let going: Vec<(SimId, Entity, f32)> = in_world
        .chain(departing)
        .filter_map(|(entity, alpha)| {
            if alpha >= 1.0 {
                return None;
            }
            let root = match entity.kind {
                sim::Kind::Object(ObjectKind::Creature, _) => roots.0.get(&entity.id).copied(),
                sim::Kind::Object(ObjectKind::Placeable, index) => made
                    .as_ref()
                    .and_then(|made| made.prop_root(index))
                    .or_else(|| placed_root(index)),
                _ => None,
            }?;
            Some((entity.id, root, alpha))
        })
        .chain(lying)
        .collect();
    fading
        .meshes
        .retain(|id, _| going.iter().any(|(going, ..)| going == id));
    for (id, root, alpha) in going {
        if let std::collections::btree_map::Entry::Vacant(vacant) = fading.meshes.entry(id) {
            let own = own_materials(root, &children, &mut meshes, &mut materials);
            // Its meshes not standing yet: tried again next frame.
            if own.is_empty() {
                continue;
            }
            vacant.insert(own);
        }
        for (handle, was) in fading.meshes.get(&id).into_iter().flatten() {
            if let Some(mut material) = materials.get_mut(handle) {
                material.params.tint.w = was * alpha;
            }
        }
    }
}

/// The module's placeables the snapshot drew and draws no longer, hidden
/// (each once).
fn hide_placed_gone(
    snapshot: &sim::Snapshot,
    fading: &mut Fading,
    placed_root: impl Fn(u32) -> Option<Entity>,
    visibility: &mut Query<&mut Visibility>,
) {
    let present: BTreeSet<u32> = snapshot
        .all_drawn()
        .filter_map(|entity| match entity.kind {
            sim::Kind::Object(ObjectKind::Placeable, index) => Some(index),
            _ => None,
        })
        .collect();
    fading.placed_seen.extend(&present);
    let gone: Vec<u32> = fading
        .placed_seen
        .difference(&present)
        .filter(|index| !fading.placed_hidden.contains(index))
        .copied()
        .collect();
    for index in gone {
        fading.placed_hidden.insert(index);
        if let Some(mut shown) = placed_root(index).and_then(|root| visibility.get_mut(root).ok()) {
            *shown = Visibility::Hidden;
        }
    }
}

/// `root`'s meshes each given a blended copy of its material: the copies,
/// with the alpha each had.
fn own_materials(
    root: Entity,
    children: &Query<&Children>,
    meshes: &mut Query<&mut MeshMaterial3d<OdysseyMaterial>>,
    materials: &mut Assets<OdysseyMaterial>,
) -> Vec<(Handle<OdysseyMaterial>, f32)> {
    let mut own = Vec::new();
    for entity in children.iter_descendants(root) {
        let Ok(mut handle) = meshes.get_mut(entity) else {
            continue;
        };
        let Some(mut faded) = materials.get(&handle.0).cloned() else {
            continue;
        };
        if matches!(
            faded.surface,
            Surface::Opaque | Surface::OpaqueAlpha | Surface::OpaqueEnvMasked
        ) {
            faded.surface = Surface::Blend;
        }
        let was = faded.params.tint.w;
        handle.0 = materials.add(faded);
        own.push((handle.0.clone(), was));
    }
    own
}
