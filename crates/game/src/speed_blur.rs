//! A minigame's speed blur (`SWMG_SetSpeedBlurEffect`, `Course::blur`):
//! the swoop's heartbeat turns it on past a speed of 150, its ratio
//! `(speed − 149) / 300`; Force Speed's (`script_fx::OwnScreen`), whole:
//! the executable sets the same blur to its most (0.75, the swoop's at a
//! ratio of 1). Drawn as the scene smeared toward the screen's
//! middle (`speed_blur.wgsl`), none there and whole toward the edges, as far
//! as the ratio says [verify by eye: KOTOR's own reach and fall-off]: a
//! quad over the whole screen, a child of the scene's camera, drawn first
//! of the transparent surfaces (`FIRST`): it smears the opaque scene
//! (Bevy's view transmission texture), and every transparent surface is
//! drawn crisp over it.

use bevy::asset::{load_internal_asset, uuid_handle};
use bevy::camera::visibility::NoFrustumCulling;
use bevy::mesh::MeshVertexBufferLayoutRef;
use bevy::pbr::{MaterialPipeline, MaterialPipelineKey};
use bevy::prelude::*;
use bevy::render::render_resource::{
    AsBindGroup, BlendState, RenderPipelineDescriptor, SpecializedMeshPipelineError,
};
use bevy::shader::ShaderRef;

use crate::lifecycle_view::SceneCamera;
use crate::sim_host::{OwnSeat, PresentedSnapshot};

const SHADER: Handle<Shader> = uuid_handle!("8f2b6c41-d3a7-4e95-b1c0-7a4e2d9f6b13");

/// Where the smear begins and is whole (a share of the screen's height from
/// its middle), and how far its samples reach at a ratio of 1 (a share of
/// the way to the middle).
const CLEAR: f32 = 0.18;
const WHOLE: f32 = 0.55;
const REACH: f32 = 0.35;
/// Added to the quad's distance for the transparent pass's order, which
/// draws the least first: the quad goes before every other transparent
/// surface, and they stay crisp over it (the swoop's HUD: the radar's
/// glass, the timer's digits; the bike's flames).
const FIRST: f32 = -1.0e9;

pub struct SpeedBlurPlugin;

impl Plugin for SpeedBlurPlugin {
    fn build(&self, app: &mut App) {
        load_internal_asset!(app, SHADER, "speed_blur.wgsl", Shader::from_wgsl);
        app.add_plugins(MaterialPlugin::<SpeedBlurMaterial>::default())
            .add_systems(
                PostUpdate,
                blur.run_if(resource_exists::<PresentedSnapshot>)
                    .before(bevy::transform::TransformSystems::Propagate),
            );
    }
}

#[derive(Asset, TypePath, AsBindGroup, Clone, Debug)]
pub struct SpeedBlurMaterial {
    /// x: the ratio, y and z: `CLEAR` and `WHOLE`, w: `REACH`.
    #[uniform(0)]
    strength: Vec4,
}

impl Material for SpeedBlurMaterial {
    fn vertex_shader() -> ShaderRef {
        ShaderRef::Handle(SHADER)
    }

    fn fragment_shader() -> ShaderRef {
        ShaderRef::Handle(SHADER)
    }

    fn reads_view_transmission_texture(&self) -> bool {
        true
    }

    fn alpha_mode(&self) -> AlphaMode {
        AlphaMode::Blend
    }

    fn depth_bias(&self) -> f32 {
        FIRST
    }

    fn enable_prepass() -> bool {
        false
    }

    fn enable_shadows() -> bool {
        false
    }

    fn specialize(
        _pipeline: &MaterialPipeline,
        descriptor: &mut RenderPipelineDescriptor,
        _layout: &MeshVertexBufferLayoutRef,
        _key: MaterialPipelineKey<Self>,
    ) -> Result<(), SpecializedMeshPipelineError> {
        descriptor.primitive.cull_mode = None;
        if let Some(fragment) = descriptor.fragment.as_mut() {
            for target in fragment.targets.iter_mut().flatten() {
                target.blend = Some(BlendState::ALPHA_BLENDING);
            }
        }
        if let Some(depth) = descriptor.depth_stencil.as_mut() {
            depth.depth_write_enabled = Some(false);
        }
        Ok(())
    }
}

/// The quad, once made: its entity and material.
#[derive(Component)]
struct Smear(Handle<SpeedBlurMaterial>);

/// Shows the smear at the ratio of the viewer's own run of the presented
/// course, or Force Speed's, under the scene's camera; hidden at none.
fn blur(
    presented: Res<PresentedSnapshot>,
    (own, screen): (
        Option<Res<OwnSeat>>,
        Option<Res<crate::script_fx::OwnScreen>>,
    ),
    cameras: Query<Entity, SceneCamera>,
    mut smears: Query<(Entity, &Smear, &ChildOf, &mut Visibility)>,
    mut materials: ResMut<Assets<SpeedBlurMaterial>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut commands: Commands,
) {
    let ratio = presented
        .0
        .current()
        .minigame
        .as_ref()
        .map_or(0.0, |course| {
            course.run_of(own.map_or(0, |own| own.0.0)).blur
        })
        .max(screen.map_or(0.0, |screen| screen.blur));
    let Some(camera) = cameras.iter().next() else {
        return;
    };
    let strength = Vec4::new(ratio, CLEAR, WHOLE, REACH);
    let mut found = false;
    for (entity, smear, parent, mut visible) in &mut smears {
        if parent.parent() != camera {
            commands.entity(entity).despawn();
            continue;
        }
        found = true;
        *visible = if ratio > 0.0 {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        };
        if let Some(mut material) = materials.get_mut(&smear.0)
            && material.strength != strength
        {
            material.strength = strength;
        }
    }
    if !found && ratio > 0.0 {
        let material = materials.add(SpeedBlurMaterial { strength });
        commands.entity(camera).with_child((
            Mesh3d(meshes.add(Rectangle::new(2.0, 2.0))),
            MeshMaterial3d(material.clone()),
            Smear(material),
            Transform::IDENTITY,
            Visibility::Inherited,
            NoFrustumCulling,
        ));
    }
}
