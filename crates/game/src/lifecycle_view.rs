//! What the view draws of module installs (render side only; it reads the
//! presented snapshots and `sim_host::Loading`):
//!
//! - the loading screen while a module is planned: the game's own
//!   (`ui::screens::loading`, KOTOR 2's layout: the module's loading image,
//!   the area's name, the bar) where the screens are drawn, else laid out
//!   as KOTOR's `loadscreen.gui` has it (640 × 480): the module's loading
//!   image over black, the progress bar (`PB_PROGRESS`, 238, 279, 164 ×
//!   22, filled with `bluefill` from the left); it holds a moment once the
//!   module is in (then this one fades), so the new scene appears settled;
//! - the doors, opening and closing as the sim has them: a door that
//!   changes state plays its `opening1`/`opening2` (or `closing`) clip
//!   (the first for a way its model has no clip for) from
//!   the snapshot it changed in, and holds the last frame; a door open when
//!   the scene is built stands at its opening's end.
//!
//! The loading screen is drawn by a camera of its own over the scene's
//! (ordered after it, not clearing, on a render layer nothing else is on),
//! so it needs no UI plugin and shows in headless captures too.

use std::time::Instant;

use bevy::camera::visibility::RenderLayers;
use bevy::camera::{OrthographicProjection, Projection, ScalingMode};
use bevy::prelude::*;
use sim::Kind;
use sim::module::ObjectKind;

use crate::material::{AreaLight, Lighting, OdysseyMaterial, Surface};
use crate::scene::{self, Animator, SceneContent, SceneModel};
use crate::sim_host::{Loading, PresentedSnapshot};
use crate::ui_view::Ui;

/// The loading screen's layer, and how long it holds and fades once the
/// module is in (seconds).
const LAYER: usize = 7;
const HOLD: f32 = 0.35;
const FADE: f32 = 0.35;
/// `PB_PROGRESS` in `loadscreen.gui` (left, top, width, height).
const BAR: [f32; 4] = [238.0, 279.0, 164.0, 22.0];

pub struct LifecycleViewPlugin;

impl Plugin for LifecycleViewPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Overlay>()
            .add_systems(
                Update,
                (
                    (loading_screen, game_screen).run_if(resource_exists::<Loading>),
                    count_assets,
                ),
            )
            .add_systems(
                PostUpdate,
                doors
                    .run_if(resource_exists::<PresentedSnapshot>)
                    .before(bevy::transform::TransformSystems::Propagate),
            );
    }
}

/// The loading screen's entities, while it is up.
#[derive(Resource, Default)]
struct Overlay {
    camera: Option<Entity>,
    /// Everything but the camera, and the fill bar.
    parts: Vec<Entity>,
    fill: Option<Entity>,
    /// The `Loading::images` the parts were built for.
    images: u32,
    materials: Vec<Handle<OdysseyMaterial>>,
}

/// A part of the loading screen.
#[derive(Component)]
struct OverlayPart;

/// The loading screen's camera: the views that place cameras leave it be.
#[derive(Component)]
pub struct OverlayCamera;

/// The scene's 3D cameras, not the loading screen's.
pub type SceneCamera = (With<Camera3d>, Without<OverlayCamera>);

/// Drawn as it is (the texture, or white, times `tint`), blended by the
/// tint's alpha.
pub(crate) fn unlit(tint: Vec4, texture: Option<Handle<Image>>) -> OdysseyMaterial {
    OdysseyMaterial::new(
        tint,
        texture,
        None,
        None,
        Surface::Blend,
        &Lighting {
            self_illum: [0.0; 3],
            ambient: [1.0; 3],
            diffuse: [1.0; 3],
            alpha: 1.0,
            is_static: false,
            is_room: true,
            fogged: false,
        },
        &AreaLight::ambient([1.0; 3]),
    )
}

/// A quad in the screen's 640 × 480 layout (GUI pixels, top left origin),
/// `depth` over the ones below.
pub(crate) fn gui_quad(rect: [f32; 4], depth: f32) -> Transform {
    let [left, top, width, height] = rect;
    Transform::from_xyz(
        left + width / 2.0 - 320.0,
        240.0 - (top + height / 2.0),
        depth,
    )
    .with_scale(Vec3::new(width, height, 1.0))
}

/// The progress bar filled `done` (0 to 1) from the left.
fn bar_fill(done: f32) -> Transform {
    let [left, top, width, height] = BAR;
    gui_quad(
        [left, top, (width * done.clamp(0.0, 1.0)).max(0.01), height],
        2.0,
    )
}

/// Whether the loading screen shows: it comes up with its image, at most a
/// moment after the install starts (the paused scene shows until then, not
/// black).
fn up(loading: &Loading) -> bool {
    loading.active
        && (loading.screen
            || loading
                .started
                .is_none_or(|started| started.elapsed().as_secs_f32() >= 0.3))
}

/// The game's own loading screen while a module installs, where the
/// screens are drawn (a session's start shows its own already).
fn game_screen(loading: Res<Loading>, ui: Option<ResMut<Ui>>, mut ours: Local<bool>) {
    let Some(mut ui) = ui.filter(|ui| ui.assets.is_some()) else {
        return;
    };
    let ui = &mut *ui;
    if !up(&loading) {
        if std::mem::take(&mut *ours) {
            ui.stack.close("loading");
        }
        return;
    }
    if !*ours && ui.stack.has("loading") {
        return;
    }
    ui.vm.session = ui::vm::SessionView {
        status: loading.title.clone().unwrap_or(String::new()),
        image: loading.image_name.clone(),
        progress: Some(loading.progress),
        error: None,
    };
    // A guest's galaxy map goes with the world it was shown in (the host
    // travelled).
    ui.stack.close("galaxymap");
    ui.stack.sync("loading", true, &ui.vm);
    *ours = true;
}

/// Once the module is in: a moment, then the fade; the screen's alpha.
fn settle(loading: &mut Loading) -> f32 {
    let Some(committed) = loading.committed else {
        return 1.0;
    };
    let since = Instant::now().duration_since(committed).as_secs_f32();
    if since >= HOLD + FADE {
        loading.active = false;
        loading.committed = None;
    }
    (1.0 - (since - HOLD) / FADE).clamp(0.0, 1.0)
}

#[allow(clippy::too_many_arguments)]
fn loading_screen(
    mut commands: Commands,
    mut loading: ResMut<Loading>,
    ui: Option<Res<Ui>>,
    mut overlay: ResMut<Overlay>,
    cameras: Query<(Entity, Option<&bevy::camera::RenderTarget>), SceneCamera>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<OdysseyMaterial>>,
    mut images: ResMut<Assets<Image>>,
    mut transforms: Query<&mut Transform, With<OverlayPart>>,
    (mut facts, mut states): (ResMut<console::DumpFacts>, ResMut<console::States>),
) {
    let overlay = &mut *overlay;
    let game = ui.is_some_and(|ui| ui.assets.is_some());
    // `wait loading_screen`: up, with the module's image.
    let drawn = if game {
        up(&loading)
    } else {
        overlay.images == loading.images
    };
    let shown = loading.active && drawn && loading.image.is_some();
    states.0.insert("loading_screen".into(), shown);
    if game {
        settle(&mut loading);
    }
    if !loading.active || game {
        if overlay.camera.is_some() {
            for entity in overlay.parts.drain(..).chain(overlay.camera.take()) {
                commands.entity(entity).despawn();
            }
            overlay.fill = None;
            overlay.materials.clear();
        }
        return;
    }
    if overlay.camera.is_none() && !up(&loading) {
        return;
    }
    if overlay.camera.is_none() {
        let target = cameras
            .iter()
            .next()
            .and_then(|(_, target)| target.cloned());
        let mut camera = commands.spawn((
            Camera3d::default(),
            scene::over_world(10),
            Projection::Orthographic(OrthographicProjection {
                scaling_mode: ScalingMode::FixedVertical {
                    viewport_height: 480.0,
                },
                ..OrthographicProjection::default_3d()
            }),
            crate::scene::encoded_view(),
            Transform::from_xyz(0.0, 0.0, 100.0),
            RenderLayers::layer(LAYER),
            OverlayCamera,
        ));
        if let Some(target) = target {
            camera.insert(target);
        }
        overlay.camera = Some(camera.id());
        overlay.images = u32::MAX;
    }
    if overlay.images != loading.images {
        for entity in overlay.parts.drain(..) {
            commands.entity(entity).despawn();
        }
        overlay.materials.clear();
        let quad = meshes.add(Rectangle::new(1.0, 1.0));
        let mut part = |commands: &mut Commands,
                        material: OdysseyMaterial,
                        transform: Transform,
                        overlay: &mut Overlay| {
            let handle = materials.add(material);
            overlay.materials.push(handle.clone());
            let entity = commands
                .spawn((
                    Mesh3d(quad.clone()),
                    MeshMaterial3d(handle),
                    transform,
                    RenderLayers::layer(LAYER),
                    OverlayPart,
                ))
                .id();
            overlay.parts.push(entity);
            entity
        };
        // Black to the screen's edges, whatever its shape.
        let backdrop = Transform::from_xyz(0.0, 0.0, 0.0).with_scale(Vec3::new(8000.0, 960.0, 1.0));
        part(
            &mut commands,
            unlit(Vec4::new(0.0, 0.0, 0.0, 1.0), None),
            backdrop,
            overlay,
        );
        if let Some(texture) = &loading.image {
            let image = images.add(scene::image(texture));
            // Its rows are stored bottom to top (TPC): turned over.
            let mut place = gui_quad([0.0, 0.0, 640.0, 480.0], 1.0);
            place.scale.y = -place.scale.y;
            part(&mut commands, unlit(Vec4::ONE, Some(image)), place, overlay);
        }
        let bar = loading
            .bar
            .as_ref()
            .map(|texture| images.add(scene::image(texture)));
        let fill_color = if bar.is_some() {
            Vec4::ONE
        } else {
            Vec4::new(0.0, 0.66, 0.98, 1.0)
        };
        let fill = part(
            &mut commands,
            unlit(fill_color, bar),
            bar_fill(loading.progress),
            overlay,
        );
        overlay.fill = Some(fill);
        overlay.images = loading.images;
    }
    facts.0.insert(
        "loading_screen".into(),
        serde_json::json!({
            "module": loading.module,
            "title": loading.title,
            "image": loading.image.as_ref().map(|image| [image.width, image.height]),
            "progress": loading.progress,
        }),
    );
    // The bar fills from the left.
    if let Some(Ok(mut transform)) = overlay.fill.map(|fill| transforms.get_mut(fill)) {
        *transform = bar_fill(loading.progress);
    }
    if loading.committed.is_some() {
        let alpha = settle(&mut loading);
        for handle in &overlay.materials {
            if let Some(mut material) = materials.get_mut(handle) {
                material.params.tint.w = alpha;
            }
        }
    }
}

/// How many meshes, images and materials are loaded: a module left must
/// take its own with it (`dump` shows them).
fn count_assets(
    meshes: Res<Assets<Mesh>>,
    images: Res<Assets<Image>>,
    materials: Res<Assets<OdysseyMaterial>>,
    mut facts: ResMut<console::DumpFacts>,
) {
    facts.0.insert(
        "assets".into(),
        serde_json::json!({
            "meshes": meshes.len(),
            "images": images.len(),
            "materials": materials.len(),
        }),
    );
}

/// A door of the scene, animated as the sim has it.
struct DoorAnim {
    object: u32,
    /// Its opening (1, 2) and closing (1, 2) clips on its nodes.
    opening: [Option<Animator>; 2],
    closing: [Option<Animator>; 2],
    /// Played in place of opening once blows break it.
    busted: Option<Animator>,
    broken: bool,
    state: u8,
    /// The state before the last change, and when it changed (sim ms);
    /// `None` for a door that has not moved since the scene was built.
    from: u8,
    since: Option<f64>,
}

#[derive(Default)]
struct Doors(Vec<DoorAnim>);

/// Open state `state`'s clip of a door's two (opening or closing): the
/// first for a way the model has no clip for.
fn way(clips: &[Option<Animator>; 2], state: u8) -> Option<&Animator> {
    clips[usize::from(state - 1).min(1)]
        .as_ref()
        .or(clips[0].as_ref())
}

/// Door `object`'s state in `snapshot`, and whether blows broke it
/// (`sim::objects`).
fn door_state(snapshot: &sim::Snapshot, instances: &sim::Instances, object: u32) -> (u8, bool) {
    let Some(entity) = snapshot
        .entities
        .iter()
        .find(|entity| entity.kind == Kind::Object(ObjectKind::Door, object))
    else {
        return (0, false);
    };
    let state = entity.object.unwrap_or(sim::ObjectState::default());
    let broken = instances
        .of_entity(entity)
        .is_some_and(|facts| state.broken(facts));
    (state.state, broken)
}

fn doors(
    scene: Option<Res<SceneContent>>,
    swap: Option<Res<crate::sim_host::SceneSwap>>,
    presented: Res<PresentedSnapshot>,
    models: Query<&SceneModel>,
    mut transforms: Query<&mut Transform>,
    mut doors: Local<Doors>,
) {
    let (Some(scene), Some(swap)) = (scene, swap) else {
        return;
    };
    let current = presented.0.current();
    let instances = sim::Instances::in_snapshot(&swap.level, current);
    if scene.is_changed() {
        doors.0.clear();
        let nodes: Vec<(usize, &Vec<Entity>)> = models
            .iter()
            .map(|model| (model.index, &model.nodes))
            .collect();
        for door in &scene.0.doors {
            let Some((_, nodes)) = nodes.iter().find(|(index, _)| *index == door.model) else {
                continue;
            };
            let placed = &scene.0.models[door.model];
            let bind = |clip: &Option<std::sync::Arc<asset_odyssey::anim::AnimationIr>>| {
                clip.as_ref()
                    .and_then(|clip| scene::animator(placed, clip, nodes))
            };
            let (state, broken) = door_state(current, &instances, door.object);
            doors.0.push(DoorAnim {
                object: door.object,
                opening: [bind(&door.opening[0]), bind(&door.opening[1])],
                closing: [bind(&door.closing[0]), bind(&door.closing[1])],
                busted: bind(&door.busted),
                broken,
                state,
                from: 0,
                since: None,
            });
        }
    }
    let (previous, alpha) = (presented.0.previous(), presented.0.alpha());
    let now = previous.time_ms as f64
        + (current.time_ms as f64 - previous.time_ms as f64) * f64::from(alpha);
    for door in &mut doors.0 {
        let (state, broken) = door_state(current, &instances, door.object);
        // Broken where it stood open: its `busted` from now.
        if broken != door.broken || state != door.state {
            door.since = Some(current.time_ms as f64);
        }
        door.broken = broken;
        if state != door.state {
            door.from = door.state;
            door.state = state;
        }
        let seconds = door
            .since
            .map_or(f32::MAX, |since| ((now - since) / 1000.0) as f32);
        let clip = match door.state {
            0 if door.from > 0 => door.since.and(way(&door.closing, door.from)),
            0 => None,
            _ if door.broken && door.busted.is_some() => door.busted.as_ref(),
            state => way(&door.opening, state),
        };
        match clip {
            Some(animator) => animator.pose(seconds.clamp(0.0, animator.length()), &mut transforms),
            None if door.state == 0 => {
                for animator in door.opening.iter().flatten() {
                    animator.rest(&mut transforms);
                }
            }
            None => {}
        }
    }
}
