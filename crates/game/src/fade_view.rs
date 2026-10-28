//! The screen's fade as scripts set it (`SetGlobalFadeOut`,
//! `SetGlobalFadeIn`; render side only: it reads the presented snapshots'
//! `sim::Fade`): a full-screen quad of the fade's colour, its alpha the
//! fade's at the moment the views show (`sim_host::view_ms`).
//!
//! It is drawn by a camera of its own, as the loading screen is
//! (`lifecycle_view`): over the scene, under the loading screen, the HUD and
//! the conversation's bars (a faded cutscene still shows its lines), so it
//! shows in headless captures too. The camera comes with the first fade and
//! draws into what the scene's camera draws into.

use bevy::camera::visibility::RenderLayers;
use bevy::camera::{OrthographicProjection, Projection, ScalingMode};
use bevy::prelude::*;

use crate::lifecycle_view::{OverlayCamera, SceneCamera, unlit};
use crate::material::OdysseyMaterial;
use crate::sim_host::{PresentedSnapshot, view_ms};

/// The fade's layer; its camera's order is between the scene's (1) and the
/// loading screen's (10).
const LAYER: usize = 6;
const ORDER: isize = 5;

pub struct FadeViewPlugin;

impl Plugin for FadeViewPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, fade.run_if(resource_exists::<PresentedSnapshot>));
    }
}

/// The fade's camera and quad, once a fade has come; the scene camera its
/// target was copied from.
struct Layer {
    camera: Entity,
    quad: Entity,
    material: Handle<OdysseyMaterial>,
    copied: Option<Entity>,
}

/// How covered the screen is at the moment the views show (0 clear, 1 the
/// fade's colour).
pub(crate) fn covered(presented: &sim::Presented) -> f32 {
    presented
        .current()
        .fade
        .map_or(0.0, |fade| fade.alpha(view_ms(presented) as u64))
}

#[allow(clippy::too_many_arguments)]
fn fade(
    mut commands: Commands,
    presented: Res<PresentedSnapshot>,
    scene: Query<(Entity, Option<&bevy::camera::RenderTarget>), SceneCamera>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<OdysseyMaterial>>,
    mut shown: Query<&mut Visibility>,
    mut cameras: Query<&mut Camera>,
    mut facts: ResMut<console::DumpFacts>,
    mut layer: Local<Option<Layer>>,
) {
    let fade = presented.0.current().fade;
    let now_ms = view_ms(&presented.0) as u64;
    let alpha = covered(&presented.0);
    facts.0.insert(
        "fade".into(),
        serde_json::json!(fade.map(|fade| serde_json::json!({
            "out": fade.out,
            "colour": fade.colour,
            "start_ms": fade.start_ms,
            "wait_ms": fade.wait_ms,
            "length_ms": fade.length_ms,
            "conversation": fade.conversation,
            "now_ms": now_ms,
            "alpha": alpha,
        }))),
    );
    let Some(fade) = fade else {
        if let Some(layer) = layer.as_ref() {
            if let Ok(mut visibility) = shown.get_mut(layer.quad) {
                *visibility = Visibility::Hidden;
            }
            show_camera(&mut cameras, layer.camera, false);
        }
        return;
    };
    let layer = layer.get_or_insert_with(|| {
        let camera = commands
            .spawn((
                Camera3d::default(),
                crate::scene::over_world(ORDER),
                Projection::Orthographic(OrthographicProjection {
                    scaling_mode: ScalingMode::FixedVertical {
                        viewport_height: 480.0,
                    },
                    ..OrthographicProjection::default_3d()
                }),
                // Blended in the stored values, as the game's was: half
                // covered by black shows half as bright.
                crate::scene::encoded_view(),
                Transform::from_xyz(0.0, 0.0, 100.0),
                RenderLayers::layer(LAYER),
                OverlayCamera,
            ))
            .id();
        let material = materials.add(unlit(Vec4::ZERO, None));
        // To the screen's edges, whatever its shape.
        let quad = commands
            .spawn((
                Mesh3d(meshes.add(Rectangle::new(1.0, 1.0))),
                MeshMaterial3d(material.clone()),
                Transform::from_scale(Vec3::new(8000.0, 960.0, 1.0)),
                RenderLayers::layer(LAYER),
                Visibility::Hidden,
            ))
            .id();
        Layer {
            camera,
            quad,
            material,
            copied: None,
        }
    });
    if let Some((camera, target)) = scene.iter().next()
        && layer.copied != Some(camera)
    {
        layer.copied = Some(camera);
        if let Some(target) = target {
            commands.entity(layer.camera).insert(target.clone());
        }
    }
    // Changed only when it changes: a material touched is prepared again.
    let [r, g, b] = fade.colour;
    let tint = Vec4::new(r, g, b, alpha);
    if materials
        .get(&layer.material)
        .is_some_and(|material| material.params.tint != tint)
        && let Some(mut material) = materials.get_mut(&layer.material)
    {
        material.params.tint = tint;
    }
    if let Ok(mut visibility) = shown.get_mut(layer.quad) {
        *visibility = if alpha > 0.0 {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        };
    }
    show_camera(&mut cameras, layer.camera, alpha > 0.0);
}

/// The fade's camera renders only while the fade covers something: an
/// overlay camera with nothing to draw still runs the whole 3D pass.
fn show_camera(cameras: &mut Query<&mut Camera>, camera: Entity, active: bool) {
    if let Ok(mut camera) = cameras.get_mut(camera)
        && camera.is_active != active
    {
        camera.is_active = active;
    }
}
