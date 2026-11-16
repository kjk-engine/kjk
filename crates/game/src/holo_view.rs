//! KOTOR 2's holograms: a creature whose template sets `Hologram`
//! (`CreatureView::hologram`; the recorded logs' officers on Peragus) is
//! drawn as TSL's executable draws one. Its meshes are hidden and each is
//! drawn in its place by the hologram pass, through the executable's
//! filter texture `holotex` (`glow_shell::Glow::hologram`). KOTOR 1's
//! templates have no such field.

use std::collections::BTreeMap;

use bevy::prelude::*;
use sim::module::ObjectKind;
use sim::{Kind, SimId};

use crate::cast::BodyRoots;
use crate::glow_shell::{Glow, GlowMaterial, SceneMeshes};
use crate::made::Made;
use crate::material::OdysseyMaterial;
use crate::scene::SceneContent;
use crate::sim_host::PresentedSnapshot;

/// The look on each hologram drawn, by character.
#[derive(Resource, Default)]
pub struct Holograms {
    by: BTreeMap<SimId, Glow>,
}

impl Holograms {
    /// How many characters it draws as holograms, and how many of their
    /// meshes it covers.
    pub fn counts(&self) -> (usize, usize) {
        (self.by.len(), self.by.values().map(Glow::shells).sum())
    }
}

/// The characters of `snapshot` drawn as holograms: those of the module's
/// creatures whose view (`views`) is a hologram's.
pub fn holograms(snapshot: &sim::Snapshot, views: &[crate::viewer::CreatureView]) -> Vec<SimId> {
    snapshot
        .entities
        .iter()
        .filter(|entity| match entity.kind {
            Kind::Object(ObjectKind::Creature, index) => views
                .iter()
                .any(|view| view.object == index && view.hologram),
            _ => false,
        })
        .map(|entity| entity.id)
        .collect()
}

/// Each frame: the hologram look on the holograms drawn, and on what they
/// put on since (a head hung later); none on those gone.
#[allow(clippy::too_many_arguments)]
pub fn draw(
    presented: Res<PresentedSnapshot>,
    scene: Option<Res<SceneContent>>,
    roots: Res<BodyRoots>,
    made: Res<Made>,
    (children, meshes): (Query<&Children>, SceneMeshes),
    (odyssey, mut materials): (Res<Assets<OdysseyMaterial>>, ResMut<Assets<GlowMaterial>>),
    mut drawn: ResMut<Holograms>,
    mut commands: Commands,
) {
    let views = scene
        .as_ref()
        .map_or(&[][..], |scene| &scene.0.creature_views[..]);
    let wanted = holograms(presented.0.current(), views);
    drawn.by.retain(|id, glow| {
        let kept = wanted.contains(id) && roots.0.contains_key(id);
        if !kept {
            glow.remove(&mut commands);
        }
        kept
    });
    let Some(filter) = &made.hologram_filter else {
        return;
    };
    for id in wanted {
        let Some(&root) = roots.0.get(&id) else {
            continue;
        };
        let glow = drawn
            .by
            .entry(id)
            .or_insert_with(|| Glow::hologram(filter.clone()));
        glow.cover(
            root,
            (&children, &meshes),
            (&odyssey, &mut materials),
            &mut commands,
        );
    }
}
