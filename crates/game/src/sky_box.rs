//! A Jedi Academy level's sky box (`asset_ja::sky`) kept round the camera.
//!
//! A model whose name ends in `asset_ja::sky::SUFFIX` is moved to the
//! camera every frame, so its sides stay as far away whatever the camera
//! does. Its meshes are never left out for being past the camera's far
//! plane, which only culls: the box is larger than the level, and may be
//! larger than that plane's distance.

use bevy::camera::visibility::NoFrustumCulling;
use bevy::prelude::*;

use crate::scene::{SceneRoot, game_to_bevy};

pub struct SkyBoxPlugin;

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

/// Moves each sky model to the camera that draws the world, and keeps its
/// meshes from being culled.
fn follow_camera(
    mut commands: Commands,
    cameras: Cameras,
    mut skies: Query<(Entity, &Name, &mut Transform), Without<Camera3d>>,
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
    for (sky, name, mut transform) in &mut skies {
        if !name.as_str().ends_with(asset_ja::sky::SUFFIX) {
            continue;
        }
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
