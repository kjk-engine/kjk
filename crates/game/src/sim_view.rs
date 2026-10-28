//! Draws the sim (`kjk sim view`) from `PresentedSnapshot` alone: a box per
//! entity over the arena floor, blended between the last two ticks. The
//! authority world is private to `sim_host`, so this module draws exactly
//! what a client that only receives snapshots could draw.

use std::collections::HashMap;

use bevy::camera::RenderTarget;
use bevy::prelude::*;
use console::CaptureTarget;
use sim::{Kind, SimId};

use crate::sim_host::PresentedSnapshot;

/// Half the arena's side, metres (the sim bounces entities off it).
const ARENA: f32 = 20.0;
const BOX: f32 = 0.8;

pub struct SimViewPlugin;

impl Plugin for SimViewPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, spawn_view)
            .add_systems(PostStartup, share_capture_target)
            .add_systems(Update, draw);
    }
}

#[derive(Component)]
struct SimCamera;

/// The box drawn for a sim entity.
#[derive(Component)]
struct Drawn(SimId);

#[derive(Resource)]
struct Look {
    cube: Handle<Mesh>,
    seats: Vec<Handle<StandardMaterial>>,
    wanderer: Handle<StandardMaterial>,
}

/// Game space (Z up) to Bevy's (Y up), as the scene does it.
fn to_bevy(position: [f32; 3]) -> Vec3 {
    Vec3::new(position[0], position[2], -position[1])
}

fn unlit(color: Color) -> StandardMaterial {
    StandardMaterial {
        base_color: crate::scene::stored(color),
        unlit: true,
        ..default()
    }
}

fn spawn_view(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    // Drawn over the scene's own (empty) camera.
    commands.spawn((
        SimCamera,
        Camera3d::default(),
        Camera {
            order: 1,
            ..default()
        },
        // Over the scene's camera, so as it draws.
        crate::scene::encoded_view(),
        Transform::from_translation(to_bevy([0.0, -42.0, 40.0])).looking_at(Vec3::ZERO, Vec3::Y),
    ));
    commands.spawn((
        Mesh3d(meshes.add(Plane3d::default().mesh().size(2.0 * ARENA, 2.0 * ARENA))),
        MeshMaterial3d(materials.add(unlit(Color::srgb(0.16, 0.17, 0.2)))),
    ));
    let seats = [
        Color::srgb(0.95, 0.35, 0.3),
        Color::srgb(0.3, 0.6, 0.95),
        Color::srgb(0.35, 0.85, 0.4),
        Color::srgb(0.95, 0.8, 0.3),
    ]
    .into_iter()
    .map(|color| materials.add(unlit(color)))
    .collect();
    commands.insert_resource(Look {
        cube: meshes.add(Cuboid::new(BOX, BOX, BOX)),
        seats,
        wanderer: materials.add(unlit(Color::srgb(0.75, 0.75, 0.78))),
    });
}

/// A headless run renders into an image the scene made at startup; this
/// camera draws into it too, so `screenshot` shows the sim.
fn share_capture_target(
    target: Option<Res<CaptureTarget>>,
    cameras: Query<Entity, With<SimCamera>>,
    mut commands: Commands,
) {
    let Some(CaptureTarget::Image(handle)) = target.as_deref() else {
        return;
    };
    for camera in &cameras {
        commands
            .entity(camera)
            .insert(RenderTarget::Image(handle.clone().into()));
    }
}

/// One box per entity in the presented snapshot: moved, added or removed.
fn draw(
    presented: Res<PresentedSnapshot>,
    look: Res<Look>,
    mut drawn: Query<(Entity, &Drawn, &mut Transform)>,
    mut commands: Commands,
) {
    let mut shown: HashMap<SimId, _> = presented
        .0
        .entities()
        .map(|entity| (entity.id, entity))
        .collect();
    let lift = Vec3::Y * BOX / 2.0;
    for (entity, id, mut transform) in &mut drawn {
        match shown.remove(&id.0) {
            Some(state) => transform.translation = to_bevy(state.position) + lift,
            None => commands.entity(entity).despawn(),
        }
    }
    for (id, state) in shown {
        let material = match state.kind {
            Kind::Avatar(seat) => look.seats[usize::from(seat.0) % look.seats.len()].clone(),
            Kind::Wanderer | Kind::Object(..) | Kind::Missile => look.wanderer.clone(),
        };
        commands.spawn((
            Drawn(id),
            Mesh3d(look.cube.clone()),
            MeshMaterial3d(material),
            Transform::from_translation(to_bevy(state.position) + lift),
        ));
    }
}
