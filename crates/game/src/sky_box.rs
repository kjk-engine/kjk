//! A Jedi Academy level's sky box (`asset_ja::sky`) kept round the camera.
//!
//! A model whose name ends in `asset_ja::sky::SUFFIX` is marked `SkyBox`
//! when it is spawned (`scene`). A marked model is moved to the camera
//! every frame, so its sides stay as far away whatever the camera does.
//! Its meshes are never left out for being past the camera's far plane,
//! which only culls: the box is larger than the level, and may be larger
//! than that plane's distance.
//!
//! A level without a sky box has no marked model, so the frame's step
//! looks at nothing there.

use bevy::camera::visibility::NoFrustumCulling;
use bevy::prelude::*;

use crate::scene::{SceneRoot, game_to_bevy};

pub struct SkyBoxPlugin;

/// Marks the root of a sky box's model. `scene` adds it to a model whose
/// name ends in `asset_ja::sky::SUFFIX`.
#[derive(Component)]
pub struct SkyBox;

/// Whether a model of this name is a sky box.
pub fn is_sky(model: &str) -> bool {
    model.ends_with(asset_ja::sky::SUFFIX)
}

impl Plugin for SkyBoxPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
            PostUpdate,
            follow_camera.before(bevy::transform::TransformSystems::Propagate),
        );
    }
}

/// The cameras that may draw the world.
type Cameras<'w, 's> =
    Query<'w, 's, (&'static Camera, &'static Transform), (With<Camera3d>, Without<SceneRoot>)>;

/// The roots of the sky boxes' models.
type Skies<'w, 's> =
    Query<'w, 's, (Entity, &'static mut Transform), (With<SkyBox>, Without<Camera3d>)>;

/// Moves each sky model to the camera that draws the world, and keeps its
/// meshes from being culled.
fn follow_camera(
    mut commands: Commands,
    cameras: Cameras,
    mut skies: Skies,
    children: Query<&Children>,
    meshes: Query<(), (With<Mesh3d>, Without<NoFrustumCulling>)>,
) {
    // The world's camera: the first one drawn that is active.
    let Some((_, eye)) = (cameras.iter())
        .filter(|(camera, _)| camera.is_active)
        .min_by_key(|(camera, _)| camera.order)
    else {
        return;
    };
    let in_game = game_to_bevy().inverse() * eye.translation;
    for (sky, mut transform) in &mut skies {
        if transform.translation != in_game {
            transform.translation = in_game;
        }
        for below in children.iter_descendants(sky) {
            if meshes.contains(below) {
                commands.entity(below).insert(NoFrustumCulling);
            }
        }
    }
}
