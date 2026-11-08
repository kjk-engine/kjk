//! A spot light's shadow map that holds only what never moves (a lamp
//! slot's rooms-only map, `shadows`) is drawn once and kept: while the
//! light stays where it is and sees the same meshes, the map in the shadow
//! texture is the one drawn before. Its view is left out of the frame's
//! render graph (no camera schedule of its own, no pass, no command
//! buffer), and the meshes it sees are left out of its draws, so nothing
//! is queued or batched for it either. Each of those views cost the render
//! thread about a millisecond a frame (2026-11-07, Ryzen 7 3700X: the Endar
//! Spire 143 to 207 fps without the lamps' rooms maps).
//!
//! The map is kept only when drawing it again would draw the same: the
//! light's place, reach and cone, the meshes it sees, and where its map lies
//! in Bevy's shadow texture (which follows from the shadow-casting lights
//! and the sun's cascades; a change of them makes the texture anew) are as
//! they were for `SETTLED` frames, with no render pipeline compiling
//! meanwhile (a mesh whose pipeline is not ready is left out of a map). The
//! frame anything of that changes, the map is drawn again, its meshes
//! queued again from the light's own list.

use std::hash::{DefaultHasher, Hash, Hasher};

use bevy::core_pipeline::Core3d;
use bevy::core_pipeline::schedule::RootNonCameraView;
use bevy::ecs::entity::{EntityHashMap, EntityHashSet};
use bevy::ecs::schedule::ScheduleLabel;
use bevy::pbr::{ExtractedDirectionalLight, ExtractedPointLight, LightEntity};
use bevy::prelude::*;
use bevy::render::extract_component::{ExtractComponent, ExtractComponentPlugin};
use bevy::render::render_resource::PipelineCache;
use bevy::render::view::RenderExtractedShadowMapVisibleEntities;
use bevy::render::{Render, RenderApp, RenderSystems};

/// On a spot light whose map holds only what never moves.
#[derive(Component, Clone, Copy, Default, ExtractComponent)]
pub struct StillShadowMap;

/// Frames a still map's inputs stay the same, every pipeline compiled,
/// before it is kept. The shadow texture is handed out again the same only
/// once the textures it replaced have left Bevy's texture cache (three
/// frames unused).
const SETTLED: u32 = 8;

pub struct StillMapsPlugin;

impl Plugin for StillMapsPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins(ExtractComponentPlugin::<StillShadowMap>::default());
        if let Some(render) = app.get_sub_app_mut(RenderApp) {
            render.init_resource::<StillMaps>().add_systems(
                Render,
                (
                    // Before the lights' visible meshes are collected into
                    // their draws.
                    settle
                        .in_set(RenderSystems::PrepareAssets)
                        .before(bevy::render::view::collect_visible_cpu_culled_entities),
                    // After the lights' views are made for the frame.
                    leave_out
                        .in_set(RenderSystems::CreateViews)
                        .after(bevy::pbr::prepare_lights),
                ),
            );
        }
    }
}

/// The still maps' lights: what each was last drawn from and for how many
/// frames, and those kept this frame.
#[derive(Resource, Default)]
struct StillMaps {
    settled: EntityHashMap<Settled>,
    kept: EntityHashSet,
}

#[derive(Default)]
struct Settled {
    key: u64,
    /// What its own list of meshes hashed to, and the list, kept while it
    /// is left empty for it.
    seen: u64,
    list: Option<RenderExtractedShadowMapVisibleEntities>,
    frames: u32,
}

/// Where the maps lie in the shadow texture: Bevy lays out the sun's
/// cascades, then the spot lights that cast, in its own order.
fn layout<'a>(
    lights: impl Iterator<Item = (Entity, &'a ExtractedPointLight)>,
    suns: impl Iterator<Item = (Entity, &'a ExtractedDirectionalLight)>,
) -> u64 {
    let mut layout = DefaultHasher::new();
    let mut casting: Vec<(Entity, bool)> = lights
        .filter(|(_, light)| light.shadow_maps_enabled)
        .map(|(entity, light)| (entity, light.spot_light_angles.is_some()))
        .collect();
    casting.sort_unstable();
    casting.hash(&mut layout);
    let mut cascades: Vec<(Entity, usize)> = suns
        .filter(|(_, sun)| sun.shadow_maps_enabled)
        .map(|(entity, sun)| (entity, sun.cascade_shadow_config.bounds.len()))
        .collect();
    cascades.sort_unstable();
    cascades.hash(&mut layout);
    layout.finish()
}

/// The meshes a light sees, by subview and class, in whatever order its
/// lists come (each list is sorted).
fn seen(visible: &RenderExtractedShadowMapVisibleEntities) -> u64 {
    let mut seen = 0u64;
    for (retained, entities) in &visible.subviews {
        for (class, listed) in &entities.classes {
            let mut one = DefaultHasher::new();
            (retained, class, &listed.entities).hash(&mut one);
            seen = seen.wrapping_add(one.finish());
        }
    }
    seen
}

/// Which still maps are kept this frame; a kept one's list of meshes left
/// empty, so its draws are taken out of its bins (and put back from the
/// light's own list the frame it is drawn again).
#[allow(clippy::type_complexity)]
fn settle(
    mut maps: ResMut<StillMaps>,
    pipelines: Res<PipelineCache>,
    mut lights: Query<(
        Entity,
        &ExtractedPointLight,
        Option<&mut RenderExtractedShadowMapVisibleEntities>,
        Has<StillShadowMap>,
    )>,
    suns: Query<(Entity, &ExtractedDirectionalLight)>,
) {
    let maps = &mut *maps;
    let compiling = pipelines.waiting_pipelines().next().is_some();
    let layout = layout(
        lights.iter().map(|(entity, light, ..)| (entity, light)),
        suns.iter(),
    );
    maps.kept.clear();
    let mut still = Vec::new();
    for (entity, light, visible, marked) in &mut lights {
        if !marked || !light.shadow_maps_enabled || light.spot_light_angles.is_none() {
            continue;
        }
        still.push(entity);
        let state = maps.settled.entry(entity).or_default();
        let kept_before = state.frames > SETTLED;
        // A list left empty last frame and not extracted since is the one
        // hashed then.
        let seen_now = match &visible {
            Some(visible) if visible.is_changed() || !kept_before => seen(visible),
            Some(_) => state.seen,
            None => 0,
        };
        let mut key = DefaultHasher::new();
        layout.hash(&mut key);
        seen_now.hash(&mut key);
        light
            .transform
            .to_matrix()
            .to_cols_array()
            .map(f32::to_bits)
            .hash(&mut key);
        light.range.to_bits().hash(&mut key);
        light.shadow_map_near_z.to_bits().hash(&mut key);
        light
            .spot_light_angles
            .map(|(inner, outer)| (inner.to_bits(), outer.to_bits()))
            .hash(&mut key);
        let key = key.finish();
        if key != state.key || compiling {
            state.key = key;
            state.frames = 0;
        } else {
            state.frames = state.frames.saturating_add(1);
        }
        state.seen = seen_now;
        let Some(mut visible) = visible else {
            continue;
        };
        let fresh = visible.is_changed();
        if state.frames > SETTLED {
            maps.kept.insert(entity);
            if fresh || state.list.is_none() {
                state.list = Some(RenderExtractedShadowMapVisibleEntities {
                    subviews: visible.subviews.clone(),
                });
            }
            for entities in visible.subviews.values_mut() {
                for listed in entities.classes.values_mut() {
                    listed.entities.clear();
                }
            }
        } else if let Some(list) = state.list.take()
            && kept_before
            && !fresh
        {
            // Drawn again from a list left empty and not extracted since:
            // the light's own list back.
            visible.subviews = list.subviews;
        }
    }
    maps.settled.retain(|entity, _| still.contains(entity));
}

/// A kept map's view leaves the render graph; one drawn again comes back.
fn leave_out(
    mut commands: Commands,
    maps: Res<StillMaps>,
    views: Query<(Entity, &LightEntity, Has<RootNonCameraView>)>,
) {
    for (view, light, rooted) in &views {
        let LightEntity::Spot { light_entity } = *light else {
            continue;
        };
        let kept = maps.kept.contains(&light_entity);
        if kept && rooted {
            commands.entity(view).remove::<RootNonCameraView>();
        } else if !kept && !rooted {
            commands
                .entity(view)
                .insert(RootNonCameraView(Core3d.intern()));
        }
    }
}
