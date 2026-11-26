//! What the view draws of module installs (render side only; it reads the
//! presented snapshots and `sim_host::Loading`):
//!
//! - the loading screen while a module is planned: the game's own
//!   (`ui::screens::loading`, the campaign's own layout, with the module's
//!   loading image, a hint and the bar) where the screens are drawn. Where
//!   they are not (a run with no game screens), it is laid out in the 640 ×
//!   480 of KOTOR 1's `loadscreen.gui` for either game: the module's loading
//!   image over black, the progress bar (`PB_PROGRESS`, 238, 279, 164 ×
//!   22, filled with `bluefill` from the left) and no hint. It holds a
//!   moment once the module is in, and then until the new scene can be
//!   drawn whole (`PictureWait`), then this one fades, so the new scene appears
//!   settled. Behind it, the scene's cameras draw nothing of the new area
//!   until its pipelines are compiled (`hold_scene`);
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
use crate::warmup::{Compiled, Warmup};

/// The loading screen's layer, and how long it holds and fades once the
/// module is in (seconds).
const LAYER: usize = 7;
const HOLD: f32 = 0.35;
const FADE: f32 = 0.35;
/// `PB_PROGRESS` in `loadscreen.gui` (left, top, width, height).
const BAR: [f32; 4] = [238.0, 279.0, 164.0, 22.0];
/// The longest a new picture is waited for (seconds). A pipeline that
/// never compiles must not hold the loading screen up for ever.
const LONGEST_WAIT: f32 = 30.0;
/// Render frames a new picture's meshes take to reach the render world and
/// ask for their pipelines.
const WAIT_FRAMES: u64 = 4;

/// Waits until the render world can draw a new picture whole. Bevy
/// compiles a render pipeline in the background the first time a mesh
/// needs it, and draws nothing with it until it is ready. On a cold shader
/// cache (the first run after an install or a driver update) that takes
/// seconds: a player saw a Taris area black but for a few doors and the
/// party for about 10 seconds after its loading screen had gone (162
/// pipelines compiled in 11.5 s). The picture is ready once a few render
/// frames have gone by, no pipeline warm-up (`warmup`) is under way and
/// every pipeline asked for is compiled, or once `LONGEST_WAIT` has passed.
pub(crate) struct PictureWait {
    since: Instant,
    frame: u64,
}

impl PictureWait {
    pub(crate) fn begin(compiled: Option<&Compiled>) -> PictureWait {
        PictureWait {
            since: Instant::now(),
            frame: compiled.map_or(0, Compiled::frames),
        }
    }

    /// Whether the picture can be shown. `warming`: a warm-up is under way.
    /// Without a renderer there is nothing to wait for.
    pub(crate) fn ready(&self, compiled: Option<&Compiled>, warming: bool) -> bool {
        let Some(compiled) = compiled else {
            return true;
        };
        let drawn = !warming && compiled.frames() >= self.frame + WAIT_FRAMES && compiled.quiet();
        drawn || self.timed_out()
    }

    /// Whether `LONGEST_WAIT` has passed since the wait began.
    pub(crate) fn timed_out(&self) -> bool {
        self.waited() >= LONGEST_WAIT
    }

    /// Seconds since the wait began.
    pub(crate) fn waited(&self) -> f32 {
        self.since.elapsed().as_secs_f32()
    }
}

/// Whether the world waits for the area a module's install put in to be
/// drawn whole. From the install's commit until the picture is ready
/// (`PictureWait`), the world's ticks are held (`sim_host::advance`, as
/// while a movie is up), so the area's scripts, conversations, fights and
/// timers start when the player can see them. A guest holds back telling
/// the host it has loaded meanwhile (`net_join`), and the host's world
/// waits for it as for any player still loading, at most its load wait.
/// Without a renderer nothing is waited for.
#[derive(Resource, Default)]
pub struct Picture {
    /// The commit the wait is for, the wait, and whether the picture was
    /// ready (it stays so for that commit).
    commit: Option<Instant>,
    wait: Option<PictureWait>,
    ready: bool,
}

impl Picture {
    /// Whether the world waits after the install `committed` (none: no
    /// install waits). A commit not yet looked at waits too, so no tick
    /// slips through before the first look.
    pub fn holds(&self, committed: Option<Instant>) -> bool {
        committed.is_some() && !(self.commit == committed && self.ready)
    }

    /// Looks at the picture after the install `committed`. With a
    /// renderer it is ready no sooner than the loading screen's first
    /// moment (`HOLD`), when the screen starts to lift (`settle`).
    fn observe(&mut self, committed: Option<Instant>, compiled: Option<&Compiled>, warming: bool) {
        if self.commit != committed {
            self.commit = committed;
            self.wait = committed.map(|_| PictureWait::begin(compiled));
            self.ready = false;
        }
        // The scene's cameras draw the area only once the warm-up is done
        // (`hold_scene`), so the render frames it needs are counted from
        // then.
        if warming && let Some(wait) = &mut self.wait {
            wait.frame = compiled.map_or(0, Compiled::frames);
        }
        if let (Some(wait), Some(committed)) = (&self.wait, committed)
            && !self.ready
        {
            let held = compiled.is_none() || committed.elapsed().as_secs_f32() >= HOLD;
            self.ready = held && wait.ready(compiled, warming);
        }
    }

    /// Whether the picture after the install `committed` is ready.
    fn ready(&self, committed: Instant) -> bool {
        self.commit == Some(committed) && self.ready
    }
}

/// What the picture waits for: the warm-up under way, and what the render
/// world has compiled.
type Drawing<'w> = (Option<Res<'w, Warmup>>, Option<Res<'w, Compiled>>);

/// Looks at the picture after the last install, every frame.
fn watch_picture(loading: Res<Loading>, mut picture: ResMut<Picture>, (warmup, compiled): Drawing) {
    picture.observe(loading.committed, compiled.as_deref(), warmup.is_some());
}

/// A scene camera that sees only the warm-up's stand-ins, and the layers it
/// saw before (none: the scene's own).
#[derive(Component)]
struct Held(Option<RenderLayers>);

/// While the loading screen waits for a new area and the area's pipeline
/// warm-up is under way (`warmup`), the scene's cameras see only the
/// warm-up's layer (`warmup::LAYER`). They draw only the stand-ins, so the
/// warm-up still compiles every pipeline the area needs, and they draw
/// nothing of the area itself. On an RTX 4070 Ti SUPER under Vulkan, with
/// an empty shader cache, the graphics device was lost in 5 of 30 loads
/// when the cameras drew the area while 200 to 340 pipelines were still
/// compiling. The cameras see the area again once the warm-up is done, or
/// once the loading screen has waited its longest (`LONGEST_WAIT`). The
/// resolve then forgets the frames before (`temporal::TemporalCut`). A
/// camera that does not see the scene's layer, such as the screens' or the
/// HUD's, is left as it is.
fn hold_scene(
    mut commands: Commands,
    (loading, picture, warmup): (Res<Loading>, Res<Picture>, Option<Res<Warmup>>),
    cameras: Query<(Entity, Option<&RenderLayers>, Has<Held>), SceneCamera>,
    held: Query<(Entity, &Held)>,
    cut: Option<ResMut<crate::temporal::TemporalCut>>,
) {
    let hold =
        warmup.is_some() && (loading.committed).is_some_and(|committed| !picture.ready(committed));
    if hold {
        let scene = RenderLayers::default();
        for (entity, layers, held) in &cameras {
            if held || layers.is_some_and(|layers| !layers.intersects(&scene)) {
                continue;
            }
            commands.entity(entity).insert((
                Held(layers.cloned()),
                RenderLayers::layer(crate::warmup::LAYER),
            ));
        }
        return;
    }
    if held.is_empty() {
        return;
    }
    for (entity, Held(layers)) in &held {
        let mut camera = commands.entity(entity);
        camera.remove::<Held>();
        match layers {
            Some(layers) => camera.insert(layers.clone()),
            None => camera.remove::<RenderLayers>(),
        };
    }
    if let Some(mut cut) = cut {
        cut.0 = true;
    }
}

/// `kjk sim probes --only "loading hold"`: when the world waits for the
/// picture after an install (`Picture::holds`), the rule `sim_host` holds
/// the world's ticks by.
#[cfg(feature = "probes")]
pub fn picture_probes() -> sim::probe::Checks {
    // An install a second ago, past the loading screen's first moment.
    let now = Instant::now();
    let committed = Some(
        now.checked_sub(std::time::Duration::from_secs(1))
            .unwrap_or(now),
    );
    let mut picture = Picture::default();
    let unseen = picture.holds(committed);
    let none = !picture.holds(None);
    // No renderer: ready at the first look.
    picture.observe(committed, None, false);
    let headless = !picture.holds(committed);
    let again = committed.map(|at| at + std::time::Duration::from_millis(1));
    let next = picture.holds(again);
    // A renderer that has drawn nothing yet: the world waits.
    let compiled = Compiled::default();
    let mut drawing = Picture::default();
    drawing.observe(committed, Some(&compiled), false);
    let waits = drawing.holds(committed);
    // Never drawn, it waits its longest and no more.
    drawing.wait = drawing.wait.take().map(|wait| PictureWait {
        since: wait.since - std::time::Duration::from_secs_f32(LONGEST_WAIT),
        ..wait
    });
    drawing.observe(committed, Some(&compiled), true);
    let capped = !drawing.holds(committed);
    vec![
        (
            "picture_holds_the_world",
            sim::probe::check(
                unseen && none && waits && next,
                format!(
                    "a commit not looked at holds {unseen}, no commit goes on {none}, a picture not drawn holds {waits}, the next commit holds {next}"
                ),
            ),
        ),
        (
            "picture_waits_within_bounds",
            sim::probe::check(
                headless && capped,
                format!(
                    "no renderer goes on at once {headless}, the longest wait goes on {capped}"
                ),
            ),
        ),
    ]
}

pub struct LifecycleViewPlugin;

impl Plugin for LifecycleViewPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Overlay>()
            .init_resource::<GameScreen>()
            .init_resource::<Picture>()
            .add_systems(
                Update,
                (
                    (
                        watch_picture,
                        loading_screen.before(console::ConsoleQueue),
                        game_screen,
                    )
                        .chain()
                        .run_if(resource_exists::<Loading>),
                    count_assets,
                    hold_scene
                        .after(watch_picture)
                        .after(crate::warmup::start)
                        .run_if(resource_exists::<Loading>),
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
    /// The commit the fade is for, and when it began.
    fading: Option<(Instant, Instant)>,
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
/// tint's alpha, over whatever its camera's target already shows
/// (`OdysseyMaterial::on_top`).
pub(crate) fn unlit(tint: Vec4, texture: Option<Handle<Image>>) -> OdysseyMaterial {
    let mut material = OdysseyMaterial::new(
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
            room_flat: false,
            fogged: false,
        },
        &AreaLight::ambient([1.0; 3]),
    );
    material.on_top = true;
    material
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

/// Whether the game's loading screen is this view's to close.
#[derive(Resource, Default)]
struct GameScreen(bool);

/// A session just installed behind its own loading screen (`session_flow`):
/// the screen stays up, with the session's image and words, and is closed
/// as a module's is, once the area can be drawn whole (`settle`). Closed at
/// once, it showed the area black for seconds on a cold shader cache
/// (issue 410).
pub(crate) fn hand_over(world: &mut World) {
    if !world.contains_resource::<Loading>() {
        return;
    }
    let (image, status) = {
        let mut ui = world.resource_mut::<Ui>();
        let ui = &mut *ui;
        ui.stack.open("loading", &ui.vm);
        (ui.vm.session.image.clone(), ui.vm.session.status.clone())
    };
    let now = Instant::now();
    let mut loading = world.resource_mut::<Loading>();
    loading.active = true;
    loading.screen = true;
    loading.started = Some(now);
    loading.committed = Some(now);
    loading.image_name = image;
    loading.title = (!status.is_empty()).then_some(status);
    loading.progress = 1.0;
    world.insert_resource(GameScreen(true));
}

/// The game's own loading screen while a module installs, where the
/// screens are drawn (a session's start shows its own already).
fn game_screen(loading: Res<Loading>, ui: Option<ResMut<Ui>>, mut ours: ResMut<GameScreen>) {
    let ours = &mut ours.0;
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
        version: std::mem::take(&mut ui.vm.session.version),
        campaign: ui.vm.session.campaign,
    };
    // A guest's galaxy map goes with the world it was shown in (the host
    // travelled).
    ui.stack.close("galaxymap");
    ui.stack.sync("loading", true, &ui.vm);
    *ours = true;
}

/// Once the module is in: a moment, and then until the new scene can be
/// drawn whole (`Picture`), then the fade; the screen's alpha. A scene
/// already ready fades after the moment, as it always did. The world's
/// first tick in the new area comes as the fade begins.
fn settle(loading: &mut Loading, overlay: &mut Overlay, picture: &Picture) -> f32 {
    let Some(committed) = loading.committed else {
        overlay.fading = None;
        return 1.0;
    };
    let fading = match overlay.fading.filter(|(of, _)| *of == committed) {
        Some((_, fading)) => fading,
        None => {
            let held = committed.elapsed().as_secs_f32() >= HOLD;
            if !held || !picture.ready(committed) {
                return 1.0;
            }
            let ms = (committed.elapsed().as_secs_f32() * 1000.0).round();
            info!(target: "lifecycle", ms, "the loading screen lifts, the area drawn");
            let now = Instant::now();
            overlay.fading = Some((committed, now));
            now
        }
    };
    let since = fading.elapsed().as_secs_f32();
    if since >= FADE {
        loading.active = false;
        loading.committed = None;
        overlay.fading = None;
    }
    (1.0 - since / FADE).clamp(0.0, 1.0)
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
    picture: Res<Picture>,
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
        settle(&mut loading, overlay, &picture);
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
            crate::reshade::overlay_camera(),
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
    if facts.wanted() {
        facts.0.insert(
            "loading_screen".into(),
            serde_json::json!({
                "module": loading.module,
                "title": loading.title,
                "image": loading.image.as_ref().map(|image| [image.width, image.height]),
                "progress": loading.progress,
            }),
        );
    }
    // The bar fills from the left.
    if let Some(Ok(mut transform)) = overlay.fill.map(|fill| transforms.get_mut(fill)) {
        *transform = bar_fill(loading.progress);
    }
    if loading.committed.is_some() {
        let alpha = settle(&mut loading, overlay, &picture);
        for handle in &overlay.materials {
            if let Some(mut material) = materials.get_mut(handle) {
                material.params.tint.w = alpha;
            }
        }
    }
}

/// How many meshes, images and materials are loaded: a module left must
/// take its own with it (`dump` shows them).
/// The other materials and the shaders are counted too: one that grows with
/// every load or area change is kept by something the session holds on to
/// (issue 445).
fn count_assets(
    meshes: Res<Assets<Mesh>>,
    images: Res<Assets<Image>>,
    materials: Res<Assets<OdysseyMaterial>>,
    others: OtherAssets,
    mut facts: ResMut<console::DumpFacts>,
) {
    if !facts.wanted() {
        return;
    }
    let (glow, grass, haze, refraction, shell, blur, ui, standard, shaders) = others;
    let len = |assets: Option<usize>| assets.unwrap_or(0);
    facts.0.insert(
        "assets".into(),
        serde_json::json!({
            "meshes": meshes.len(),
            "images": images.len(),
            "materials": materials.len(),
            "glow_materials": len(glow.map(|assets| assets.len())),
            "grass_materials": len(grass.map(|assets| assets.len())),
            "heat_haze_materials": len(haze.map(|assets| assets.len())),
            "refraction_materials": len(refraction.map(|assets| assets.len())),
            "shell_materials": len(shell.map(|assets| assets.len())),
            "speed_blur_materials": len(blur.map(|assets| assets.len())),
            "ui_materials": len(ui.map(|assets| assets.len())),
            "standard_materials": len(standard.map(|assets| assets.len())),
            "shaders": len(shaders.map(|assets| assets.len())),
        }),
    );
}

/// The assets `count_assets` counts besides meshes, images and the
/// Odyssey materials, where their plugins run.
type OtherAssets<'w> = (
    Option<Res<'w, Assets<crate::glow_shell::GlowMaterial>>>,
    Option<Res<'w, Assets<crate::grass::GrassMaterial>>>,
    Option<Res<'w, Assets<crate::heat_haze::HeatHazeMaterial>>>,
    Option<Res<'w, Assets<crate::scene_fx::RefractionMaterial>>>,
    Option<Res<'w, Assets<crate::shell::ShellMaterial>>>,
    Option<Res<'w, Assets<crate::speed_blur::SpeedBlurMaterial>>>,
    Option<Res<'w, Assets<crate::ui_material::UiMaterial>>>,
    Option<Res<'w, Assets<StandardMaterial>>>,
    Option<Res<'w, Assets<Shader>>>,
);

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
    /// The scene's model, what its clips key on its meshes, and those
    /// meshes' materials, once it has played (a force field fades by
    /// alpha as it opens: Onderon's palace, `DOR_OND05`).
    model: usize,
    fx: Option<std::sync::Arc<crate::model_fx::ModelFx>>,
    lit: Option<Vec<crate::model_fx::Lit>>,
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

/// What a door's keyed alpha is shown through: its meshes' materials, and
/// the animated materials' slots where the material plugin runs.
type DoorMaterials<'w, 's> = (
    Query<'w, 's, &'static Children>,
    Query<'w, 's, &'static mut MeshMaterial3d<crate::material::OdysseyMaterial>>,
    ResMut<'w, Assets<crate::material::OdysseyMaterial>>,
    Option<ResMut<'w, crate::material::Animated>>,
    Option<ResMut<'w, crate::scene::AnimatedMaterials>>,
);

fn doors(
    scene: Option<Res<SceneContent>>,
    swap: Option<Res<crate::sim_host::SceneSwap>>,
    presented: Res<PresentedSnapshot>,
    models: Query<&SceneModel>,
    mut transforms: Query<&mut Transform>,
    (children, mut mesh_materials, mut materials, mut animated, mut moving): DoorMaterials,
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
                model: door.model,
                fx: door.fx.clone(),
                lit: None,
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
        let shown = clip.map(|animator| {
            let at = seconds.clamp(0.0, animator.length());
            (animator.clip_name(), at)
        });
        match clip {
            Some(animator) => animator.pose(seconds.clamp(0.0, animator.length()), &mut transforms),
            None if door.state == 0 => {
                for animator in door.opening.iter().flatten() {
                    animator.rest(&mut transforms);
                }
            }
            None => {}
        }
        // Its meshes' alpha as the clip keys it, once it has moved or stands
        // open; a door never moved draws as its model is.
        let Some(fx) = door.fx.clone().filter(|fx| !fx.surfaces.is_empty()) else {
            continue;
        };
        let placed = &scene.0.models[door.model];
        if door.lit.is_none() && shown.is_some() {
            let nodes = models.iter().find(|model| model.index == door.model);
            door.lit = nodes.and_then(|model| {
                crate::model_fx::own_materials(
                    (&fx, &placed.ir, &model.nodes),
                    &crate::model_fx::glows(&placed.ir, &scene.0.textures),
                    &children,
                    (&mut mesh_materials, &mut materials, moving.as_deref_mut()),
                )
            });
        }
        for lit in door.lit.iter_mut().flatten() {
            match &shown {
                Some((name, at)) => lit.key((&fx, &placed.ir), name, *at),
                None => lit.reset(&placed.ir),
            }
            lit.show(&mut materials, animated.as_deref_mut());
        }
    }
}
