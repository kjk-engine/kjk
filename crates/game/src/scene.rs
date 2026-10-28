//! What a run draws: nothing, one model, or a module's rooms. Owns the
//! camera (placed by the scene, by `camera` commands, and in a window by the
//! fly controls) and turns the scene's models into Bevy meshes.
//!
//! Everything stays in the game's space (Z up) under one root turned into
//! Bevy's (Y up).

use std::collections::BTreeMap;
use std::f32::consts::FRAC_PI_2;
use std::sync::atomic::{AtomicBool, Ordering};

use asset_odyssey::model::Matrix;
use asset_odyssey::texture::{Encoding, TextureIr};
use bevy::asset::RenderAssetUsages;
use bevy::camera::{CameraOutputMode, ClearColorConfig, CompositingSpace, RenderTarget};
use bevy::core_pipeline::tonemapping::{DebandDither, Tonemapping};
use bevy::image::{
    CompressedImageFormatSupport, CompressedImageFormats, ImageAddressMode, ImageFilterMode,
    ImageSampler, ImageSamplerDescriptor,
};
use bevy::input::mouse::AccumulatedMouseMotion;
use bevy::light::cluster::{ClusterConfig, GlobalClusterSettings};
use bevy::mesh::skinning::{SkinnedMesh, SkinnedMeshInverseBindposes};
use bevy::mesh::{Indices, PrimitiveTopology, VertexAttributeValues};
use bevy::prelude::*;
use bevy::render::render_resource::{
    BlendState, Extent3d, TextureDimension, TextureFormat, TextureUsages, TextureViewDescriptor,
    TextureViewDimension,
};
use bevy::render::storage::ShaderBuffer;
use bevy::render::view::NoIndirectDrawing;
use console::{AnimationTime, CameraPose, CameraRequest, CaptureTarget, DumpFacts};

use crate::material::{
    self, AreaLight, GpuLight, LightList, Lighting, MAX_LIGHTS, OdysseyMaterial,
    OdysseyMaterialPlugin, Surface,
};
use crate::viewer::{LoadedScene, Placed, RoomView};

/// The frame size of a headless run unless `--size` says otherwise.
pub const HEADLESS_SIZE: (u32, u32) = (1280, 720);
/// The viewer's vertical field of view until a `camera` command sets one:
/// the camera styles' first.
const FOV_DEGREES: f32 = crate::viewer::DEFAULT_VIEW_ANGLE;
/// Eye height above the entry point, metres.
const EYE_HEIGHT: f32 = crate::cast::HEAD_M;

#[derive(Resource)]
pub struct Headless(pub bool);

/// The headless frame size (width, height).
#[derive(Resource)]
pub struct FrameSize(pub u32, pub u32);

#[derive(Resource)]
pub struct SceneContent(pub Box<LoadedScene>);

/// The camera's direction in game space, kept so the fly controls and
/// `camera` commands agree on it.
#[derive(Component)]
pub(crate) struct FlyCamera {
    yaw: f32,
    pitch: f32,
}

/// A module room's root entity: index into `LoadedScene::rooms`.
#[derive(Component)]
struct RoomOf(usize);

/// A placed model's root: its index in `LoadedScene::models` and its node
/// entities (in the model's node order), for systems that pose it live.
#[derive(Component)]
pub struct SceneModel {
    pub index: usize,
    pub nodes: Vec<Entity>,
}

/// What the room culling knows: each room's bounds (game space), from the
/// scene's spawn, and which rooms show.
#[derive(Resource, Default)]
pub(crate) struct Culling {
    /// Each room's model's corners, lowest and highest; `None` without one.
    bounds: Vec<Option<(Vec3, Vec3)>>,
    shown: Vec<bool>,
}

impl Culling {
    /// Whether room `room` shows now; every room before the first check.
    pub(crate) fn shows(&self, room: usize) -> bool {
        self.shown.get(room).copied().unwrap_or(true)
    }

    /// Whether room `room` shows to an eye at `eye` (game space) standing
    /// over room `here`: `here` and the rooms linked to it in the VIS file
    /// either way round (K1 makes every link mutual), and besides them any
    /// room some part of which is nearer the eye than `fog_far`, where the
    /// fog would not yet hide it all (Q-0305: K1 draws only the VIS's, so a
    /// room its designers thought the bar hid shows the fog colour through
    /// an archway). Off every floor all rooms show.
    fn room_visible(
        &self,
        rooms: &[RoomView],
        here: Option<usize>,
        eye: Vec3,
        fog_far: f32,
        room: usize,
    ) -> bool {
        let lists = |from: usize, to: usize| {
            rooms[from]
                .visible
                .iter()
                .any(|name| name.eq_ignore_ascii_case(&rooms[to].name))
        };
        let near = || {
            self.bounds
                .get(room)
                .copied()
                .flatten()
                .is_none_or(|(low, high)| {
                    eye.clamp(low, high).distance_squared(eye) < fog_far * fog_far
                })
        };
        match here {
            Some(here) => room == here || lists(here, room) || lists(room, here) || near(),
            None => true,
        }
    }
}

/// The corners of a placed model's meshes, lowest and highest (game space).
fn model_bounds(placed: &crate::viewer::Placed) -> Option<(Vec3, Vec3)> {
    let frame = placement(placed);
    placed
        .ir
        .meshes
        .iter()
        .flat_map(|mesh| {
            let node = frame * to_mat4(&placed.ir.nodes[mesh.node].world);
            mesh.positions
                .iter()
                .map(move |p| node.transform_point3(Vec3::from_array(*p)))
        })
        .fold(None, |bounds, p| match bounds {
            None => Some((p, p)),
            Some((low, high)) => Some((p.min(low), p.max(high))),
        })
}

/// The room the player stands in (`room_at` under its feet), found each
/// frame; `None` off every floor.
#[derive(Resource, Default, Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) struct PlayerRoom(pub Option<usize>);

/// Where the player stands (game space, metres): its feet as the sim
/// presents them, else (the viewers, no sim) on the floor under the
/// camera (the eye height under it off every floor).
#[derive(Resource, Default, Clone, Copy, PartialEq, Debug)]
pub(crate) struct PlayerAt(pub Option<Vec3>);

/// What a scene spawned at its top level: despawned (with everything
/// under it) when the next scene replaces it.
#[derive(Component)]
pub(crate) struct SceneEntity;

/// The scene's root, game space (metres, +Z up) turned to Bevy's: models
/// placed in the world hang from it.
#[derive(Component)]
pub struct SceneRoot;

pub struct ScenePlugin;

/// Where the lights the materials read are picked for the camera.
#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub struct LightsPicked;

/// Where the blades are drawn: after the transforms, before the meshes'
/// changes are sent on.
#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub struct BladesDrawn;

/// Where the rooms the camera sees are picked (`Culling`).
#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub struct RoomsCulled;

impl Plugin for ScenePlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins(crate::warmup::WarmupPlugin)
            .add_observer(lean_camera);
        app.add_plugins((
            OdysseyMaterialPlugin,
            crate::shell::ShellPlugin,
            crate::scene_fx::EffectsPlugin,
            crate::heat_haze::HeatHazePlugin,
            crate::glow_shell::GlowShellPlugin,
            crate::speed_blur::SpeedBlurPlugin,
            crate::own_fx::OwnFxPlugin,
            crate::grass::GrassPlugin,
            crate::weather::WeatherPlugin,
            crate::shadows::ShadowsPlugin,
        ))
        .add_systems(Startup, spawn_camera)
        .add_systems(
            Update,
            // The warm-up reads the new meshes before their data goes
            // to the render world.
            (spawn_scene, clear_to_fog, crate::warmup::start)
                .chain()
                .run_if(resource_exists_and_changed::<SceneContent>)
                .before(apply_camera_request),
        )
        .init_resource::<Culling>()
        .init_resource::<PlayerRoom>()
        .init_resource::<PlayerAt>()
        .init_resource::<SceneLights>()
        .init_resource::<CarriedLights>()
        .init_resource::<crate::blaster_view::ShotLights>()
        .init_resource::<BodyLights>()
        .init_resource::<ScrollingMaterials>()
        .init_resource::<FlipBooks>()
        .init_resource::<LiveBlades>()
        .init_resource::<DrawnBlades>()
        .init_resource::<RetiredBlades>()
        .add_systems(
            Update,
            (
                apply_camera_request,
                fly,
                cull_rooms.in_set(RoomsCulled),
                find_player,
                pick_lights.in_set(LightsPicked),
                scroll_uvs,
                cycle_flipbooks,
                animate,
            )
                .chain(),
        )
        .add_systems(
            PostUpdate,
            // Before the meshes' changes are sent on: drawn after it,
            // a blade would show where its hilt was a frame earlier.
            (retire_blades, draw_blades)
                .chain()
                .in_set(BladesDrawn)
                .after(bevy::transform::TransformSystems::Propagate)
                .before(bevy::asset::AssetEventSystems),
        )
        .add_systems(
            Update,
            (show_during, follow_paths).before(bevy::transform::TransformSystems::Propagate),
        );
    }

    fn finish(&self, app: &mut App) {
        let blocks = app
            .world()
            .get_resource::<CompressedImageFormatSupport>()
            .is_some_and(|support| support.0.contains(CompressedImageFormats::BC));
        BLOCKS_ON_GPU.store(blocks, Ordering::Relaxed);
        if !blocks {
            info!("no BC texture formats on this GPU: DXT textures are decoded to RGBA8");
        }
        // Bevy's light clustering is off: no material reads the clusters
        // (the Odyssey material reads the lights' list by index, filled
        // either way), and clustering each camera's view every frame, on
        // the GPU, cost the render app 2.5 ms a frame. On the CPU, with no
        // clusters (`lean_camera`), it costs nothing.
        if let Some(mut settings) = app.world_mut().get_resource_mut::<GlobalClusterSettings>() {
            settings.gpu_clustering = None;
        }
    }
}

/// The game's Z-up space to Bevy's Y-up: game +Z becomes +Y and game +Y
/// becomes -Z (Bevy's forward).
pub(crate) fn game_to_bevy() -> Quat {
    Quat::from_rotation_x(-FRAC_PI_2)
}

pub(crate) fn to_mat4(m: &Matrix) -> Mat4 {
    Mat4::from_cols_array_2d(&[
        [m[0][0], m[1][0], m[2][0], m[3][0]],
        [m[0][1], m[1][1], m[2][1], m[3][1]],
        [m[0][2], m[1][2], m[2][2], m[3][2]],
        [m[0][3], m[1][3], m[2][3], m[3][3]],
    ])
}

/// A camera transform in Bevy space from a game-space pose.
fn camera_transform(pose: &CameraPose) -> Transform {
    let (yaw, pitch) = (pose.yaw.to_radians(), pose.pitch.to_radians());
    let direction = Vec3::new(
        yaw.cos() * pitch.cos(),
        yaw.sin() * pitch.cos(),
        pitch.sin(),
    );
    let rotation = game_to_bevy();
    Transform::from_translation(rotation * Vec3::from_array(pose.position))
        .looking_to(rotation * direction, Vec3::Y)
}

/// Where a placed model's root sits in game space: its offset, turned by
/// its facing, then its attachment (a head's `headhook`).
fn placement(placed: &Placed) -> Mat4 {
    let frame = Mat4::from_rotation_translation(
        Quat::from_rotation_z(placed.facing),
        Vec3::from_array(placed.offset),
    );
    match &placed.attach {
        Some(attach) => frame * to_mat4(attach),
        None => frame,
    }
}

/// The pose that frames a lone model: from its front (models face the
/// game's +Y), a little to the side and above, far enough to fit it.
/// The game-space box `models`' meshes fill at rest; `None` for none.
pub(crate) fn bounds(models: &[Placed]) -> Option<(Vec3, Vec3)> {
    bounds_where(models, |_| true)
}

/// The same, of the meshes `keep` keeps.
pub(crate) fn bounds_where<'a>(
    models: impl IntoIterator<Item = &'a Placed>,
    keep: impl Fn(&asset_odyssey::model::MeshIr) -> bool,
) -> Option<(Vec3, Vec3)> {
    let (mut min, mut max) = (Vec3::splat(f32::MAX), Vec3::splat(f32::MIN));
    for placed in models {
        for mesh in placed.ir.meshes.iter().filter(|mesh| keep(mesh)) {
            let world = to_mat4(&placed.ir.nodes[mesh.node].world);
            for p in &mesh.positions {
                let v = placement(placed)
                    .transform_point3(world.transform_point3(Vec3::from_array(*p)));
                min = min.min(v);
                max = max.max(v);
            }
        }
    }
    (min.x <= max.x).then_some((min, max))
}

fn framing(scene: &LoadedScene) -> CameraPose {
    let (min, max) = bounds(&scene.models).unwrap_or((Vec3::splat(-1.0), Vec3::splat(1.0)));
    let centre = (min + max) / 2.0;
    let radius = ((max - min).length() / 2.0).max(0.1);
    let from = Vec3::new(0.55, 1.0, 0.35).normalize();
    let eye = centre + from * radius * 2.6;
    let look = (centre - eye).normalize();
    CameraPose {
        position: eye.to_array(),
        yaw: look.y.atan2(look.x).to_degrees(),
        pitch: look.z.asin().to_degrees(),
        fov: None,
    }
}

fn spawn_camera(
    mut commands: Commands,
    mut images: ResMut<Assets<Image>>,
    headless: Res<Headless>,
    size: Res<FrameSize>,
    scene: Option<Res<SceneContent>>,
) {
    let target = if headless.0 {
        let (width, height) = (size.0, size.1);
        let mut image =
            Image::new_target_texture(width, height, TextureFormat::Rgba8UnormSrgb, None);
        // A screenshot copies the texture back to the CPU.
        image.texture_descriptor.usage |= TextureUsages::COPY_SRC;
        let handle = images.add(image);
        commands.insert_resource(CaptureTarget::Image(handle.clone()));
        Some(RenderTarget::Image(handle.into()))
    } else {
        None
    };
    let Some(scene) = scene else {
        let mut camera = commands.spawn((Camera2d, CompositingSpace::Srgb));
        if let Some(target) = target {
            camera.insert(target);
        }
        return;
    };
    let mut camera = commands.spawn(scene_camera(&scene.0));
    if let Some(target) = target {
        camera.insert(target);
    }
}

/// The menu's camera given way to `scene`'s, drawing into what it drew
/// into (a session started from the menu).
pub(crate) fn replace_camera(world: &mut World, scene: &LoadedScene) {
    let old: Vec<(Entity, Option<RenderTarget>)> = world
        .query_filtered::<(Entity, Option<&RenderTarget>), With<Camera2d>>()
        .iter(world)
        .map(|(entity, target)| (entity, target.cloned()))
        .collect();
    let target = old.first().and_then(|(_, target)| target.clone());
    for (entity, _) in old {
        world.despawn(entity);
    }
    let mut camera = world.spawn(scene_camera(scene));
    if let Some(target) = target {
        camera.insert(target);
    }
}

/// A scene's camera, at its entry (or framing it all).
fn scene_camera(scene: &LoadedScene) -> impl Bundle {
    let pose = match scene.entry {
        Some((position, direction)) => CameraPose {
            position: [position[0], position[1], position[2] + EYE_HEIGHT],
            yaw: direction[1].atan2(direction[0]).to_degrees(),
            pitch: -5.0,
            fov: None,
        },
        None => framing(scene),
    };
    (
        camera_view(),
        camera_transform(&pose),
        FlyCamera {
            yaw: pose.yaw,
            pitch: pose.pitch,
        },
    )
}

/// What a scene's camera sees with, which every pipeline it draws with
/// depends on (`warmup::prewarm` makes one the same). It sees the shadow
/// maps' layers too, as Bevy draws a light's maps only for the cameras
/// that share its layers (`shadows::view_layers`).
pub(crate) fn camera_view() -> impl Bundle {
    (
        Camera3d::default(),
        crate::shadows::view_layers(),
        encoded_view(),
        Projection::Perspective(PerspectiveProjection {
            fov: FOV_DEGREES.to_radians(),
            near: 0.05,
            ..default()
        }),
    )
}

/// How every camera drawing onto the world's target draws: blending in
/// the stored (gamma-encoded) values, as the games' fixed-function
/// hardware blended in its framebuffer, with no tonemapping or dithering.
/// The main texture keeps encoded values (`Rgba8Unorm`) and the shaders
/// output them; Bevy decodes once on the way to the window or the frame's
/// sRGB image, which stores them back unchanged. The cameras on a target
/// share the world's main texture only while they agree on this.
pub(crate) fn encoded_view() -> impl Bundle {
    (
        Tonemapping::None,
        DebandDither::Disabled,
        CompositingSpace::Srgb,
    )
}

/// A camera drawing over the world on its target, at `order` (the fade,
/// the loading screen, the HUD, the screens, a movie): it does not clear,
/// and its frame replaces what the cameras before it left on the target.
/// They share the world's main texture (`encoded_view`), so each one's
/// frame already holds theirs; Bevy would blend it over their copy by the
/// world's alpha instead, and wherever the world wrote less than one (a
/// textured creature's alpha, a blended surface) the frame as it stood
/// before this camera drew (no bars, no fade) showed through.
pub(crate) fn over_world(order: isize) -> Camera {
    Camera {
        order,
        clear_color: ClearColorConfig::None,
        output_mode: CameraOutputMode::Write {
            blend_state: Some(BlendState::REPLACE),
            clear_color: ClearColorConfig::None,
        },
        ..default()
    }
}

/// What a `StandardMaterial` must be given to show `color` on the
/// world's encoded target: Bevy's shaders output linear values, which the
/// target stores as they are, so `color`'s encoded values go in as linear.
pub(crate) fn stored(color: Color) -> Color {
    let [red, green, blue, alpha] = color.to_srgba().to_f32_array();
    Color::linear_rgba(red, green, blue, alpha)
}

fn apply_camera_request(
    mut request: ResMut<CameraRequest>,
    mut cameras: Query<(&mut Transform, &mut FlyCamera, &mut Projection)>,
) {
    let Some(pose) = request.0 else { return };
    for (mut transform, mut fly, mut projection) in &mut cameras {
        *transform = camera_transform(&pose);
        fly.yaw = pose.yaw;
        fly.pitch = pose.pitch;
        if let (Some(fov), Projection::Perspective(perspective)) = (pose.fov, &mut *projection) {
            perspective.fov = fov.to_radians();
        }
    }
    if !cameras.is_empty() {
        request.0 = None;
    }
}

/// WASD to move, Q/E down and up, Shift faster, right mouse button to look.
fn fly(
    time: Res<Time>,
    keys: Option<Res<ButtonInput<KeyCode>>>,
    buttons: Option<Res<ButtonInput<MouseButton>>>,
    motion: Option<Res<AccumulatedMouseMotion>>,
    mut cameras: Query<(&mut Transform, &mut FlyCamera)>,
) {
    let (Some(keys), Some(buttons), Some(motion)) = (keys, buttons, motion) else {
        return;
    };
    for (mut transform, mut fly) in &mut cameras {
        if buttons.pressed(MouseButton::Right) {
            fly.yaw -= motion.delta.x * 0.15;
            let most = crate::sim_host::pitch_limit();
            fly.pitch = (fly.pitch - motion.delta.y * 0.15).clamp(-most, most);
        }
        let (yaw, pitch) = (fly.yaw.to_radians(), fly.pitch.to_radians());
        let forward = Vec3::new(
            yaw.cos() * pitch.cos(),
            yaw.sin() * pitch.cos(),
            pitch.sin(),
        );
        let right = Vec3::new(yaw.sin(), -yaw.cos(), 0.0);
        let mut step = Vec3::ZERO;
        for (key, direction) in [
            (KeyCode::KeyW, forward),
            (KeyCode::KeyS, -forward),
            (KeyCode::KeyD, right),
            (KeyCode::KeyA, -right),
            (KeyCode::KeyE, Vec3::Z),
            (KeyCode::KeyQ, -Vec3::Z),
        ] {
            if keys.pressed(key) {
                step += direction;
            }
        }
        let speed = if keys.pressed(KeyCode::ShiftLeft) {
            20.0
        } else {
            5.0
        };
        let rotation = game_to_bevy();
        let game_position = rotation.inverse() * transform.translation
            + step.normalize_or_zero() * speed * time.delta_secs();
        *transform = camera_transform(&CameraPose {
            position: game_position.to_array(),
            yaw: fly.yaw,
            pitch: fly.pitch,
            fov: None,
        });
    }
}

/// Whether the GPU samples BC formats, so DXT textures upload as their
/// blocks (set once the render device is up; until then, and without it,
/// they are decoded to RGBA8).
static BLOCKS_ON_GPU: AtomicBool = AtomicBool::new(false);

/// `texture` for any material: a DXT1 with holes is decoded (its holes
/// opaque black), as only the scene's material samples it as the games did.
pub(crate) fn image(texture: &TextureIr) -> Image {
    if texture.encoding == Encoding::Bc1Holes {
        return upload(&texture.decoded());
    }
    upload(texture)
}

/// `texture` for the scene's material (`OdysseyMaterial`), which takes a
/// texture without alpha as opaque (`opaque_texture`): DXT1 with holes
/// stays in blocks.
pub(crate) fn material_image(texture: &TextureIr) -> Image {
    upload(texture)
}

/// A flip-book texture's frames (`proceduretype cycle`), one image each,
/// and its rate; none for a texture of one frame.
pub(crate) fn flipbook_images(
    texture: &TextureIr,
    images: &mut Assets<Image>,
) -> Option<(Vec<Handle<Image>>, f32)> {
    let (_, fps) = texture.facts.cycle?;
    (texture.frame_mips.len() > 1 && fps > 0.0).then(|| {
        let handles = texture
            .frame_mips
            .iter()
            .map(|mips| {
                let frame = TextureIr {
                    mips: mips.clone(),
                    frame_mips: Vec::new(),
                    faces: Vec::new(),
                    facts: texture.facts.clone(),
                    ..*texture
                };
                images.add(material_image(&frame))
            })
            .collect();
        (handles, fps)
    })
}

/// The frame a flip-book of `count` frames at `fps` shows `seconds` in.
pub(crate) fn flipbook_frame(count: usize, fps: f32, seconds: f32) -> usize {
    let frames = count as f32;
    let length = frames / fps;
    let at = seconds.rem_euclid(length);
    (((frames - 1.0) * (at / length)).round() as usize).min(count.saturating_sub(1))
}

fn upload(texture: &TextureIr) -> Image {
    if texture.encoding != Encoding::Rgba8 && !BLOCKS_ON_GPU.load(Ordering::Relaxed) {
        return upload(&texture.decoded());
    }
    let cube = texture.faces.len() == 6;
    let size = Extent3d {
        width: texture.width,
        height: texture.height,
        depth_or_array_layers: if cube { 6 } else { 1 },
    };
    // Sampled as stored; the shader multiplies before converting.
    let format = match texture.encoding {
        Encoding::Rgba8 => TextureFormat::Rgba8Unorm,
        Encoding::Bc1 | Encoding::Bc1Holes => TextureFormat::Bc1RgbaUnorm,
        Encoding::Bc3 => TextureFormat::Bc3RgbaUnorm,
    };
    let mut image = Image::new_uninit(
        size,
        TextureDimension::D2,
        format,
        RenderAssetUsages::RENDER_WORLD,
    );
    // Layer-major: each face's whole mip chain, then the next face.
    image.data = Some(if cube {
        texture
            .faces
            .iter()
            .flatten()
            .map(Vec::as_slice)
            .collect::<Vec<_>>()
            .concat()
    } else {
        texture.mips.concat()
    });
    image.texture_descriptor.mip_level_count = texture.mips.len() as u32;
    let address = if cube || texture.facts.clamp {
        ImageAddressMode::ClampToEdge
    } else {
        ImageAddressMode::Repeat
    };
    if cube {
        image.texture_view_descriptor = Some(TextureViewDescriptor {
            dimension: Some(TextureViewDimension::Cube),
            ..default()
        });
    }
    image.sampler = ImageSampler::Descriptor(ImageSamplerDescriptor {
        address_mode_u: address,
        address_mode_v: address,
        address_mode_w: address,
        mag_filter: ImageFilterMode::Linear,
        min_filter: ImageFilterMode::Linear,
        mipmap_filter: ImageFilterMode::Linear,
        ..default()
    });
    image
}

/// (diffuse, lightmap, surface, env map): one material each.
type MaterialKey<'a> = (
    Option<&'a str>,
    Option<&'a str>,
    Surface,
    Option<&'a str>,
    [u32; 11],
    Option<[u32; 2]>,
    bool,
);

/// Flip-book materials: each frame's image, and frames per second.
#[derive(Resource, Default)]
struct FlipBooks(Vec<(Handle<OdysseyMaterial>, Vec<Handle<Image>>, f32)>);

/// Materials whose diffuse UVs scroll, with their direction.
#[derive(Resource, Default)]
pub(crate) struct ScrollingMaterials(pub Vec<(Handle<OdysseyMaterial>, Vec2)>);

/// How fast an animated surface (water, screens) scrolls: its texture's
/// direction scaled by this each second. Our chosen speed.
const UV_ANIMATION_SPEED: f32 = 250.0;

/// Every light in the scene, and the ones the shader gets now (`None`
/// until the scene's first are picked).
#[derive(Resource, Default)]
struct SceneLights {
    all: Vec<GpuLight>,
    active: Option<Vec<usize>>,
}

/// Lights that move with what carries them (a worn item's light,
/// `script_fx`), in Bevy world space, as of this frame: candidates beside
/// the scene's.
#[derive(Resource, Default, PartialEq)]
pub(crate) struct CarriedLights(pub Vec<GpuLight>);

/// The lights creatures' bodies carry, where their nodes stand and as
/// their clips key them (`own_fx`), in Bevy world space, as of this frame:
/// candidates beside the scene's.
#[derive(Resource, Default, PartialEq)]
pub(crate) struct BodyLights(pub Vec<GpuLight>);

/// Extra reach added to every light's radius: a light is a candidate while
/// the camera is within its radius plus this, so it eases in before the
/// player reaches its edge. Our chosen margin.
const LIGHT_RADIUS_BIAS: f32 = 64.0;

/// One entity per drawable mesh, placed by its model's offset and its node's
/// rest-pose world transform. A scene that replaces another (a module
/// installed) despawns the old one first; its meshes, materials and
/// images go with their last handles.
/// The asset stores spawning a model writes to.
pub(crate) struct Stores<'a> {
    pub meshes: &'a mut Assets<Mesh>,
    pub materials: &'a mut Assets<OdysseyMaterial>,
    pub bindposes: &'a mut Assets<SkinnedMeshInverseBindposes>,
}

/// The asset stores as a system takes them.
#[derive(bevy::ecs::system::SystemParam)]
pub(crate) struct ModelAssets<'w> {
    meshes: ResMut<'w, Assets<Mesh>>,
    materials: ResMut<'w, Assets<OdysseyMaterial>>,
    images: ResMut<'w, Assets<Image>>,
    bindposes: ResMut<'w, Assets<SkinnedMeshInverseBindposes>>,
}

impl ModelAssets<'_> {
    /// The images (for a `Spawner`'s textures) and the stores its models
    /// go to.
    pub fn split(&mut self) -> (&mut Assets<Image>, Stores<'_>) {
        (
            &mut self.images,
            Stores {
                meshes: &mut self.meshes,
                materials: &mut self.materials,
                bindposes: &mut self.bindposes,
            },
        )
    }
}

/// What spawning models counted, for the scene's dump.
#[derive(Default)]
pub(crate) struct Tally {
    pub drawn: usize,
    lightmapped: usize,
    hidden: usize,
    skinned_meshes: usize,
    animated: usize,
    additive_without_hint: usize,
    unweighted_skins: Vec<String>,
    surfaces: BTreeMap<&'static str, usize>,
    envmaps: BTreeMap<&'static str, usize>,
}

/// Turns placed models into Bevy entities, meshes and materials: a scene's
/// all at once (`spawn_scene`), or weapons armed later (`armory`). It holds
/// the textures' images by name, the materials made so far (shared by
/// meshes alike), the flip-books and scrolling materials met, and counts.
pub(crate) struct Spawner<'t> {
    pub textures: BTreeMap<&'t str, Handle<Image>>,
    irs: &'t BTreeMap<String, TextureIr>,
    /// One image per frame for each flip-book diffuse, and its rate.
    frames: BTreeMap<&'t str, (Vec<Handle<Image>>, f32)>,
    area: AreaLight,
    cache: BTreeMap<MaterialKey<'t>, Handle<OdysseyMaterial>>,
    pub flipbooks: Vec<(Handle<OdysseyMaterial>, Vec<Handle<Image>>, f32)>,
    pub scrolling: Vec<(Handle<OdysseyMaterial>, Vec2)>,
    pub tally: Tally,
    /// The room models spawned now keep lights off what they hide
    /// (`shadows::ShadowRoom`); not a sky or a backdrop, which nothing
    /// stands in.
    pub rooms_block: bool,
}

impl<'t> Spawner<'t> {
    /// Images for `textures` (each flip-book's frames too).
    pub fn new(
        textures: &'t BTreeMap<String, TextureIr>,
        area: AreaLight,
        images: &mut Assets<Image>,
    ) -> Spawner<'t> {
        let handles = textures
            .iter()
            .map(|(name, texture)| (name.as_str(), images.add(material_image(texture))))
            .collect();
        let frames = textures
            .iter()
            .filter_map(|(name, texture)| Some((name.as_str(), flipbook_images(texture, images)?)))
            .collect();
        Spawner {
            textures: handles,
            irs: textures,
            frames,
            area,
            cache: BTreeMap::new(),
            flipbooks: Vec::new(),
            scrolling: Vec::new(),
            tally: Tally::default(),
            rooms_block: true,
        }
    }

    pub fn materials_made(&self) -> usize {
        self.cache.len()
    }

    /// One model (`index` in its scene; `is_room` lit as a room) under
    /// `parent` at `transform`: its root, carrying `SceneModel`, and one
    /// entity per node in the model's hierarchy, so bones can move.
    pub fn model(
        &mut self,
        commands: &mut Commands,
        stores: &mut Stores,
        (placed, index, is_room): (&Placed, usize, bool),
        (parent, transform): (Entity, Transform),
    ) -> (Entity, Vec<Entity>) {
        let model_root = commands
            .spawn((
                transform,
                Visibility::default(),
                Name::new(placed.ir.name.clone()),
            ))
            .id();
        commands.entity(parent).add_child(model_root);
        // Spawned first and parented after: a GLA may list a bone before
        // its parent.
        let nodes: Vec<Entity> = placed
            .ir
            .nodes
            .iter()
            .map(|node| {
                let [x, y, z, w] = node.rotation;
                commands
                    .spawn((
                        Transform::from_translation(Vec3::from_array(node.translation))
                            .with_rotation(Quat::from_xyzw(x, y, z, w)),
                        Visibility::default(),
                    ))
                    .id()
            })
            .collect();
        for (node, &entity) in placed.ir.nodes.iter().zip(&nodes) {
            let parent = node
                .parent
                .and_then(|parent| nodes.get(parent).copied())
                .unwrap_or(model_root);
            commands.entity(parent).add_child(entity);
        }
        commands.entity(model_root).insert(SceneModel {
            index,
            nodes: nodes.clone(),
        });
        if let Some(clip) = &placed.clip
            && let Some(animator) = animator(placed, clip, &nodes)
        {
            commands.entity(model_root).insert(animator);
            self.tally.animated += 1;
        }
        for part in &placed.ir.meshes {
            self.mesh(commands, stores, (placed, part, is_room), &nodes);
        }
        (model_root, nodes)
    }

    /// A scene's model (`index`, `is_room` lit as a room) where it is
    /// placed: a hooked one (a head, a weapon) hangs under its body's node,
    /// so it follows the body's animation (`spawned`: the node entities of
    /// the scene's models from `first` on, spawned so far); anything else
    /// sits under the scene's `root`.
    pub fn placed(
        &mut self,
        commands: &mut Commands,
        stores: &mut Stores,
        (placed, index, is_room): (&Placed, usize, bool),
        (root, spawned, first): (Entity, &[Vec<Entity>], usize),
    ) -> (Entity, Vec<Entity>) {
        let hooked = placed
            .hook
            .and_then(|(body, node)| spawned.get(body.checked_sub(first)?)?.get(node).copied());
        let transform = match hooked {
            Some(_) => placed
                .hook_local
                .as_ref()
                .map_or(Transform::IDENTITY, |local| {
                    Transform::from_matrix(to_mat4(local))
                }),
            None => Transform::from_matrix(placement(placed)),
        };
        self.model(
            commands,
            stores,
            (placed, index, is_room),
            (hooked.unwrap_or(root), transform),
        )
    }

    /// Lights `specs` as live blades (`LiveBlades`, from its end on), each
    /// from its hilt (`hilt` of its model's index): the first one's index.
    pub fn live_blades(
        &self,
        commands: &mut Commands,
        stores: &mut Stores,
        lives: &mut LiveBlades,
        (specs, hilt): (&[crate::viewer::BladeSpec], impl Fn(usize) -> Entity),
    ) -> usize {
        let first = lives.0.len();
        for (n, spec) in specs.iter().enumerate() {
            lives.0.push(LiveBlade::default());
            self.blade(commands, stores, (first + n, spec), hilt(spec.model));
        }
        first
    }

    /// One mesh of a model, hung from its node.
    fn mesh(
        &mut self,
        commands: &mut Commands,
        stores: &mut Stores,
        (placed, part, is_room): (&Placed, &asset_odyssey::model::MeshIr, bool),
        nodes: &[Entity],
    ) {
        let tally = &mut self.tally;
        let textures = &self.textures;
        // A skinned mesh stays in main memory too: a cut reads it
        // (`severing`), and so do a body's marks (`body_marks`), a head's
        // too.
        let usage = if part.skin.is_some() || placed.attach.is_some() {
            RenderAssetUsages::default()
        } else {
            RenderAssetUsages::RENDER_WORLD
        };
        let mut mesh = Mesh::new(PrimitiveTopology::TriangleList, usage);
        mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, part.positions.clone());
        if part.normals.len() == part.positions.len() {
            mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, part.normals.clone());
        }
        if part.uv1.len() == part.positions.len() {
            mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, part.uv1.clone());
        }
        let texture = part
            .texture
            .as_deref()
            .and_then(|name| textures.get_key_value(name).map(|(name, _)| *name));
        let lightmap = part
            .lightmap
            .as_deref()
            .and_then(|name| textures.get_key_value(name).map(|(name, _)| *name))
            .filter(|_| part.uv2.len() == part.positions.len());
        if lightmap.is_some() {
            tally.lightmapped += 1;
            mesh.insert_attribute(Mesh::ATTRIBUTE_UV_1, part.uv2.clone());
        }
        // As xoreos loads textures: a mesh none of whose textures
        // load is not rendered.
        let lightmap_loaded = part
            .lightmap
            .as_deref()
            .is_some_and(|name| textures.contains_key(name));
        if texture.is_none() && !lightmap_loaded {
            tally.hidden += 1;
            return;
        }
        mesh.insert_indices(Indices::U32(part.indices.clone()));
        let facts: Vec<_> = [part.texture.as_deref(), part.lightmap.as_deref()]
            .into_iter()
            .flatten()
            .filter_map(|name| self.irs.get(name).map(|texture| &texture.facts))
            .collect();
        let surface = material::surface(part.transparency_hint, part.alpha, &facts);
        // Its back faces are culled, as reone and xoreos draw every KOTOR
        // mesh; a decal's are drawn (a textured window or grate seen from
        // both sides, a JA shader's `cull none`), and a mesh whose textures
        // say nothing is drawn from both sides.
        let culled = !facts.iter().all(|facts| facts.decal || facts.two_sided);
        let lighting = Lighting {
            self_illum: part.self_illum,
            ambient: part.ambient,
            diffuse: part.diffuse,
            alpha: part.alpha,
            is_static: placed.is_static,
            is_room,
            fogged: placed.ir.affected_by_fog,
        };
        // The env map: a cube map named by any of the mesh's textures
        // (xoreos takes the last one that names one).
        let envmap_name = facts.iter().rev().find_map(|facts| facts.envmap.as_deref());
        let envmap = envmap_name
            .and_then(|name| self.irs.get_key_value(name))
            .filter(|(_, texture)| texture.faces.len() == 6)
            .map(|(name, _)| name.as_str());
        match (envmap_name, envmap) {
            (Some(_), Some(_)) => *tally.envmaps.entry("cube").or_insert(0) += 1,
            (Some(name), None) if self.irs.contains_key(name) => {
                *tally.envmaps.entry("not a cube (not drawn)").or_insert(0) += 1
            }
            (Some(_), None) => *tally.envmaps.entry("missing").or_insert(0) += 1,
            _ => {}
        }
        *tally.surfaces.entry(surface.name()).or_insert(0) += 1;
        if surface == Surface::Additive && part.transparency_hint == 0 {
            tally.additive_without_hint += 1;
        }
        let area = self.area;
        let material = self
            .cache
            .entry((
                texture,
                lightmap,
                surface,
                envmap,
                lighting.bits(),
                part.uv_scroll.map(|dir| dir.map(f32::to_bits)),
                culled,
            ))
            .or_insert_with(|| {
                let handle = |name: Option<&str>| name.and_then(|name| textures.get(name).cloned());
                // A missing diffuse leaves its texture stage empty, so the
                // lightmap alone shows (xoreos).
                let opaque = texture
                    .and_then(|name| self.irs.get(name))
                    .is_some_and(|texture| !texture.facts.alpha);
                stores.materials.add(
                    OdysseyMaterial::new(
                        Vec4::ONE,
                        handle(texture),
                        handle(lightmap),
                        handle(envmap),
                        surface,
                        &lighting,
                        &area,
                    )
                    .opaque_texture(opaque)
                    .culled(culled),
                )
            })
            .clone();
        if let Some((handles, fps)) = texture.and_then(|name| self.frames.get(name))
            && !self
                .flipbooks
                .iter()
                .any(|(handle, _, _)| *handle == material)
        {
            self.flipbooks
                .push((material.clone(), handles.clone(), *fps));
        }
        if let Some(dir) = part.uv_scroll
            && !self.scrolling.iter().any(|(handle, _)| *handle == material)
        {
            self.scrolling
                .push((material.clone(), Vec2::from_array(dir)));
        }
        let skinned = part.skin.as_ref().filter(|skin| {
            skin.vertex_joints.len() == part.positions.len()
                && skin.vertex_weights.len() == part.positions.len()
        });
        if let Some(skin) = skinned {
            mesh.insert_attribute(
                Mesh::ATTRIBUTE_JOINT_INDEX,
                VertexAttributeValues::Uint16x4(skin.vertex_joints.clone()),
            );
            mesh.insert_attribute(Mesh::ATTRIBUTE_JOINT_WEIGHT, skin.vertex_weights.clone());
        }
        let child = commands
            .spawn((
                Mesh3d(stores.meshes.add(mesh)),
                MeshMaterial3d(material),
                Transform::IDENTITY,
            ))
            .id();
        if let Some(skin) = skinned {
            let binds: Vec<Mat4> = skin.inverse_binds.iter().map(to_mat4).collect();
            // The bones can carry the mesh far from its rest bounds (a
            // character walking off), so it is never frustum-culled.
            commands.entity(child).insert((
                SkinnedMesh {
                    inverse_bindposes: stores
                        .bindposes
                        .add(SkinnedMeshInverseBindposes::from(binds)),
                    joints: skin.joints.iter().map(|&node| nodes[node]).collect(),
                },
                bevy::camera::visibility::NoFrustumCulling,
            ));
            tally.skinned_meshes += 1;
            let unweighted = skin
                .vertex_weights
                .iter()
                .filter(|w| w.iter().sum::<f32>() < 0.5)
                .count();
            if unweighted > 0 {
                tally.unweighted_skins.push(format!(
                    "{}/{}: {unweighted} of {}",
                    placed.ir.name,
                    placed.ir.nodes[part.node].name,
                    skin.vertex_weights.len()
                ));
            }
        }
        // In a module's world, a character's, door's or placeable's solid
        // surfaces cast, and a room's block the lights (`shadows`);
        // see-through or glowing surfaces do neither.
        if self.area.shadowed
            && matches!(
                surface,
                Surface::Opaque | Surface::OpaqueAlpha | Surface::OpaqueEnvMasked
            )
        {
            if is_room {
                if self.rooms_block {
                    commands.entity(child).insert(crate::shadows::ShadowRoom);
                }
            } else {
                commands.entity(child).insert(crate::shadows::ShadowCaster);
            }
        }
        commands.entity(nodes[part.node]).add_child(child);
        tally.drawn += 1;
    }

    /// A saber blade (`index` in `LoadedScene::blades` and `LiveBlades`)
    /// grown from `hilt`: its glow, core and trail meshes hang under its
    /// `Blade` entity, so retiring it (`RetiredBlades`) takes them all.
    pub fn blade(
        &self,
        commands: &mut Commands,
        stores: &mut Stores,
        (index, spec): (usize, &crate::viewer::BladeSpec),
        hilt: Entity,
    ) {
        let material = |name: &str, materials: &mut Assets<OdysseyMaterial>| {
            materials.add(OdysseyMaterial::new(
                Vec4::ONE,
                self.textures.get(name).cloned(),
                None,
                None,
                Surface::AddOne,
                &Lighting {
                    self_illum: [0.0; 3],
                    ambient: [1.0; 3],
                    diffuse: [1.0; 3],
                    alpha: 1.0,
                    is_static: false,
                    is_room: true,
                    fogged: false,
                },
                &self.area,
            ))
        };
        let glow_mesh = stores.meshes.add(blade_mesh(Vec::new(), Vec::new()));
        let core_mesh = stores.meshes.add(blade_mesh(Vec::new(), Vec::new()));
        // Built with its colour attribute from the start: a mesh must keep
        // one vertex layout.
        let trail_mesh = stores
            .meshes
            .add(coloured_quads(Vec::new(), Vec::new(), Vec::new()));
        let mut parts = vec![
            (glow_mesh.clone(), material(&spec.glow, stores.materials)),
            (core_mesh.clone(), material(&spec.core, stores.materials)),
        ];
        if !spec.trail.is_empty() || spec.live {
            parts.push((
                trail_mesh.clone(),
                material(&spec.trail_texture, stores.materials),
            ));
        }
        commands
            .spawn((
                SceneEntity,
                Transform::IDENTITY,
                Visibility::default(),
                Blade {
                    spec: index,
                    live: spec.live.then(Live::default),
                    hilt,
                    origin: Vec3::from_array(spec.origin),
                    dir: Vec3::from_array(spec.dir),
                    length: spec.length,
                    radius: spec.radius,
                    glow: glow_mesh,
                    core: core_mesh,
                    extend: spec
                        .extend
                        .iter()
                        .map(|&(t, l)| (t, [l, 0.0, 0.0]))
                        .collect(),
                    period: spec.period,
                    trail: spec.trail.clone(),
                    trail_mesh,
                    trail_color: spec.trail_color,
                    shown: spec.shown.clone(),
                    glow_empty: true,
                    trail_empty: true,
                },
            ))
            .with_children(|blade| {
                for (mesh, material) in parts {
                    blade.spawn((
                        Mesh3d(mesh),
                        MeshMaterial3d(material),
                        Transform::IDENTITY,
                        bevy::camera::visibility::NoFrustumCulling,
                    ));
                }
            });
    }
}

/// Blades whose weapons were put away (`armory`), by their index in
/// `LiveBlades`: despawned with their meshes before the blades are drawn.
#[derive(Resource, Default)]
pub struct RetiredBlades(pub Vec<usize>);

fn retire_blades(
    mut retired: ResMut<RetiredBlades>,
    blades: Query<(Entity, &Blade)>,
    mut lives: ResMut<LiveBlades>,
    mut commands: Commands,
) {
    if retired.0.is_empty() {
        return;
    }
    for (entity, blade) in &blades {
        if retired.0.contains(&blade.spec) {
            commands.entity(entity).despawn();
            if let Some(live) = lives.0.get_mut(blade.spec) {
                *live = LiveBlade::default();
            }
        }
    }
    retired.0.clear();
}

/// The world is cleared to its area's fog colour, black where it has none,
/// as KOTOR sets its clear colour with the fog's: what no room draws (a
/// room the VIS leaves out beyond an open door, a gap in a wall) shows as
/// the far fog does.
fn clear_to_fog(scene: Res<SceneContent>, mut commands: Commands) {
    let [red, green, blue] = scene.0.fog.as_ref().map_or([0.0; 3], |fog| fog.color);
    commands.insert_resource(ClearColor(Color::srgb(red, green, blue)));
}

pub(crate) fn spawn_scene(
    mut commands: Commands,
    scene: Option<Res<SceneContent>>,
    old: Query<Entity, With<SceneEntity>>,
    mut culling: ResMut<Culling>,
    mut assets: ModelAssets,
    mut refractions: ResMut<Assets<crate::scene_fx::RefractionMaterial>>,
    mut facts: ResMut<DumpFacts>,
) {
    let Some(scene) = scene else { return };
    let started = std::time::Instant::now();
    for entity in &old {
        commands.entity(entity).despawn();
    }
    let loaded = &scene.0;
    *culling = Culling {
        bounds: (loaded.rooms.iter())
            .map(|room| {
                room.model
                    .and_then(|model| model_bounds(&loaded.models[model]))
            })
            .collect(),
        shown: Vec::new(),
    };
    let (images, mut stores) = assets.split();
    let mut spawner = Spawner::new(&loaded.textures, loaded.area_light(), images);
    // Lights in Bevy world space: model offset and bearing, then the
    // light node's rest-pose position.
    let mut lights = Vec::new();
    let mut casting = Vec::new();
    let mut surfaces = vec![Vec::new(); loaded.rooms.len()];
    let mut dynamic_types: BTreeMap<String, usize> = BTreeMap::new();
    for (index, placed) in loaded.models.iter().enumerate() {
        let frame = placement(placed);
        let room = loaded
            .rooms
            .iter()
            .position(|room| room.model == Some(index));
        // A room's surfaces, for the shadows' rays; not a sky's or a
        // backdrop's (a room with no floor), which shelters nothing.
        if let Some(room) = room
            && loaded.lit
            && (!loaded.rooms[room].floor.is_empty() || loaded.rooms[room].floor_error.is_some())
        {
            surfaces[room] = crate::shadows::surfaces(placed, game_to_bevy(), frame);
        }
        // A creature's body carries its lights (`own_fx::BodyLights`).
        let carried = (loaded.own_fx.iter())
            .any(|own| own.model == index && matches!(own.plays, crate::viewer::Plays::Body));
        for light in placed.ir.lights.iter().filter(|_| !carried) {
            let local = to_mat4(&placed.ir.nodes[light.node].world).transform_point3(Vec3::ZERO);
            let position = game_to_bevy() * frame.transform_point3(local);
            let key = format!(
                "type {}{}",
                light.dynamic_type,
                if light.ambient_only {
                    " ambient-only"
                } else {
                    ""
                }
            );
            *dynamic_types.entry(key).or_insert(0) += 1;
            let gpu = GpuLight {
                position_radius: position.extend(light.radius),
                color_multiplier: Vec3::from_array(light.color).extend(light.multiplier),
                kind: UVec4::new(u32::from(light.ambient_only), light.dynamic_type, 0, 0),
            };
            if !light.ambient_only && loaded.lit {
                casting.push(crate::shadows::CastingLight {
                    id: casting.len() as u32,
                    light: gpu,
                    flagged: light.shadow,
                    room,
                });
            }
            lights.push(gpu);
        }
    }
    let root = commands
        .spawn((
            Transform::from_rotation(game_to_bevy()),
            Visibility::default(),
            SceneEntity,
            SceneRoot,
        ))
        .id();
    let mut node_entities: Vec<Vec<Entity>> = Vec::with_capacity(loaded.models.len());
    let mut model_roots: Vec<Entity> = Vec::with_capacity(loaded.models.len());
    for (index, placed) in loaded.models.iter().enumerate() {
        let room = loaded
            .rooms
            .iter()
            .position(|room| room.model == Some(index));
        let is_room = !loaded.lit || room.is_some();
        // A room with no floor is only seen: a sky, a backdrop.
        spawner.rooms_block = room.is_none_or(|room| {
            let room = &loaded.rooms[room];
            !room.floor.is_empty() || room.floor_error.is_some()
        });
        let (model_root, nodes) = spawner.placed(
            &mut commands,
            &mut stores,
            (placed, index, is_room),
            (root, &node_entities, 0),
        );
        if let Some(room) = room {
            commands.entity(model_root).insert(RoomOf(room));
        }
        model_roots.push(model_root);
        node_entities.push(nodes);
    }
    let tally = &spawner.tally;
    let refused: Vec<String> = loaded
        .models
        .iter()
        .flat_map(|placed| {
            placed
                .ir
                .refusals
                .iter()
                .map(move |(node, why)| format!("{}/{node}: {why:?}", placed.ir.name))
        })
        .collect();
    let untextured: Vec<String> = loaded
        .models
        .iter()
        .flat_map(|placed| {
            placed
                .ir
                .meshes
                .iter()
                .filter(|mesh| mesh.texture.is_none())
                .map(move |mesh| {
                    let node = &placed.ir.nodes[mesh.node].name;
                    format!("{}/{node} (lightmap {:?})", placed.ir.name, mesh.lightmap)
                })
        })
        .collect();
    // Every room's own floor should be found as that room (a room found
    // instead has floor above it there).
    let floor_mismatches: Vec<String> = loaded
        .rooms
        .iter()
        .enumerate()
        .filter_map(|(index, room)| {
            let [a, b, c] = room.floor.first()?;
            let (x, y) = ((a[0] + b[0] + c[0]) / 3.0, (a[1] + b[1] + c[1]) / 3.0);
            let found = room_at(&loaded.rooms, x, y);
            (found != Some(index)).then(|| {
                let found = found.map(|found| loaded.rooms[found].name.as_str());
                format!("{} -> {found:?}", room.name)
            })
        })
        .collect();
    let seen_only: Vec<&str> = loaded
        .rooms
        .iter()
        .filter(|room| room.floor.is_empty() && room.floor_error.is_none())
        .map(|room| room.name.as_str())
        .collect();
    let unread_floors: Vec<&str> = loaded
        .rooms
        .iter()
        .filter_map(|room| room.floor_error.as_deref())
        .collect();
    facts.0.insert(
        "scene".into(),
        serde_json::json!({
            "name": loaded.name,
            "models": loaded.models.len(),
            "instances": loaded.instances,
            "creatures": loaded.creatures,
            "creature_refusals": loaded.creature_refusals,
            "meshes": tally.drawn,
            "lightmapped": tally.lightmapped,
            "surfaces": tally.surfaces,
            "hidden_no_texture_loaded": tally.hidden,
            "skinned_meshes": tally.skinned_meshes,
            "animated": tally.animated,
            "missing_clips": loaded.missing_clips,
            "creature_notes": loaded.creature_notes,
            "weapons": loaded.weapons,
            "skin_vertices_weight_below_half": tally.unweighted_skins,
            "lights": lights.len(),
            "shadow_flagged": casting.iter().filter(|casting| casting.flagged).count(),
            "light_types": dynamic_types,
            "ambient": loaded.ambient,
            "envmaps": tally.envmaps,
            "additive_without_hint": tally.additive_without_hint,
            "odd_blending": loaded
                .textures
                .iter()
                .filter_map(|(name, texture)| {
                    texture.facts.odd_blending.as_ref().map(|value| format!("{name}: {value}"))
                })
                .collect::<Vec<_>>(),
            "materials": spawner.materials_made(),
            "refused": refused,
            "untextured": untextured,
            "entry": loaded.entry,
            "floor_mismatches": floor_mismatches,
            "seen_only_rooms": seen_only,
            "unread_floors": unread_floors,
            "fog": loaded.fog.map(|fog| serde_json::json!({
                "color": fog.color, "near": fog.near, "far": fog.far,
            })),
            "vis": loaded.rooms.iter().map(|room| (room.name.as_str(), &room.visible)).collect::<BTreeMap<_, _>>(),
            "textures": loaded.textures.len(),
            "missing_textures": loaded.missing,
            "missing_texture_stand_ins": loaded
                .missing
                .iter()
                .map(|name| (name.as_str(), crate::viewer::stand_in(loaded, name)))
                .collect::<BTreeMap<_, _>>(),
            "missing_models": loaded.missing_models,
        }),
    );
    let tally_drawn = tally.drawn;
    facts.0.insert(
        "uv_scrolling".into(),
        serde_json::json!(spawner.scrolling.len()),
    );
    facts.0.insert(
        "flipbooks".into(),
        serde_json::json!(spawner.flipbooks.len()),
    );
    // Saber blades: two meshes each (glow sprites, core), rebuilt every
    // frame around the hilt and the camera.
    commands.insert_resource(LiveBlades(vec![LiveBlade::default(); loaded.blades.len()]));
    for (index, spec) in loaded.blades.iter().enumerate() {
        if let Some(&hilt) = model_roots.get(spec.model) {
            spawner.blade(&mut commands, &mut stores, (index, spec), hilt);
        }
    }
    for shown in &loaded.shown {
        if let Some(&entity) = model_roots.get(shown.model) {
            commands.entity(entity).insert(ShownDuring {
                ranges: shown.ranges.clone(),
                period: shown.period,
            });
            if !shown.path.is_empty() {
                let keys = shown
                    .path
                    .iter()
                    .map(|(t, m)| {
                        let (_, rotation, translation) = to_mat4(m).to_scale_rotation_translation();
                        (*t, translation, rotation)
                    })
                    .collect();
                commands.entity(entity).insert(FollowPath {
                    keys,
                    period: shown.period,
                });
            }
        }
    }
    facts
        .0
        .insert("blades".into(), serde_json::json!(loaded.blades.len()));
    crate::scene_fx::spawn(
        &mut commands,
        &loaded.effects,
        (&spawner.textures, &loaded.textures),
        stores.meshes,
        stores.materials,
        &mut refractions,
        loaded.ambient,
    );
    commands.insert_resource(ScrollingMaterials(spawner.scrolling));
    commands.insert_resource(FlipBooks(spawner.flipbooks));
    facts.0.insert(
        "effects".into(),
        serde_json::json!(loaded.effects.items.len()),
    );
    commands.insert_resource(SceneLights {
        all: lights,
        active: None,
    });
    commands.insert_resource(crate::shadows::ShadowLights(casting));
    commands.insert_resource(crate::shadows::RoomSurfaces(surfaces));
    crate::launch_times::record(
        "scene spawn (images, meshes, materials)",
        started.elapsed().as_secs_f64() * 1000.0,
    );
    crate::launch_times::mark("scene spawned");
    info!(
        "scene {}: {} models, {tally_drawn} meshes, {} textures ({} as DXT blocks), {} missing, {} refused",
        loaded.name,
        loaded.models.len(),
        loaded.textures.len(),
        loaded
            .textures
            .values()
            .filter(|texture| texture.encoding != Encoding::Rgba8)
            .count(),
        loaded.missing.len(),
        refused.len()
    );
}

/// The room whose walkable floor is topmost at (x, y), as reone's elevation
/// test finds it (a ray cast down from high above; the nearest hit wins).
fn room_at(rooms: &[RoomView], x: f32, y: f32) -> Option<usize> {
    floor_at(rooms, x, y).map(|(index, _)| index)
}

/// That room and its floor's height there.
fn floor_at(rooms: &[RoomView], x: f32, y: f32) -> Option<(usize, f32)> {
    let mut best: Option<(usize, f32)> = None;
    for (index, room) in rooms.iter().enumerate() {
        for [a, b, c] in &room.floor {
            let Some(z) = height_at(*a, *b, *c, x, y) else {
                continue;
            };
            if best.is_none_or(|(_, top)| z > top) {
                best = Some((index, z));
            }
        }
    }
    best
}

/// Where the player stands and the room it stands in: its body's feet in
/// the presented snapshot (the own seat's, seat 0's without one), else the
/// floor under the camera.
fn find_player(
    scene: Option<Res<SceneContent>>,
    presented: Option<Res<crate::sim_host::PresentedSnapshot>>,
    seat: Option<Res<crate::sim_host::OwnSeat>>,
    cameras: Query<&Transform, crate::lifecycle_view::SceneCamera>,
    (mut at, mut room): (ResMut<PlayerAt>, ResMut<PlayerRoom>),
) {
    let Some(scene) = scene else { return };
    let rooms = &scene.0.rooms;
    let feet = presented.as_ref().and_then(|presented| {
        let current = presented.0.current();
        let seat = seat.as_ref().map_or(sim::Seat(0), |seat| seat.0);
        crate::script_fx::feet(current, current.body(seat)?.id)
    });
    let under_eye = || {
        let eye = game_to_bevy().inverse() * cameras.iter().next()?.translation;
        let floor = floor_at(rooms, eye.x, eye.y).map_or(eye.z - EYE_HEIGHT, |(_, z)| z);
        Some(eye.with_z(floor))
    };
    let found = feet.or_else(under_eye);
    if at.set_if_neq(PlayerAt(found)) || scene.is_changed() {
        room.set_if_neq(PlayerRoom(found.and_then(|at| room_at(rooms, at.x, at.y))));
    }
}

/// The triangle's height at (x, y), when (x, y) lies over it.
fn height_at(a: [f32; 3], b: [f32; 3], c: [f32; 3], x: f32, y: f32) -> Option<f32> {
    let det = (b[1] - c[1]) * (a[0] - c[0]) + (c[0] - b[0]) * (a[1] - c[1]);
    if det.abs() < f32::EPSILON {
        return None;
    }
    let u = ((b[1] - c[1]) * (x - c[0]) + (c[0] - b[0]) * (y - c[1])) / det;
    let v = ((c[1] - a[1]) * (x - c[0]) + (a[0] - c[0]) * (y - c[1])) / det;
    let w = 1.0 - u - v;
    (u >= 0.0 && v >= 0.0 && w >= 0.0).then(|| u * a[2] + v * b[2] + w * c[2])
}

/// What every camera leaves out of Bevy's renderer: light clusters (see
/// `ScenePlugin::finish`), and GPU culling with its indirect draws
/// (`NoIndirectDrawing`; the meshes are still culled on the CPU, and still
/// built on the GPU). The culling's per-view compute, bind groups and
/// indirect buffers cost the render app 4 to 9 ms a frame (more the more
/// views: each overlay camera, each shadow map) for nothing a frame here
/// draws less of; the frames are the same.
fn lean_camera(add: On<Add, Camera>, mut commands: Commands) {
    commands
        .entity(add.entity)
        .insert((ClusterConfig::None, NoIndirectDrawing));
}

/// Shows the rooms `Culling::room_visible` shows from the camera: its
/// room's VIS, and the rooms near enough that the fog leaves something of
/// them (all of them in an area without fog; the frustum culling of each
/// mesh then draws only those in view).
fn cull_rooms(
    scene: Option<Res<SceneContent>>,
    cameras: Query<Ref<Transform>, With<FlyCamera>>,
    mut rooms: Query<(&RoomOf, &mut Visibility)>,
    mut culling: ResMut<Culling>,
    mut facts: ResMut<DumpFacts>,
) {
    let Some(scene) = scene else { return };
    let all = &scene.0.rooms;
    if all.is_empty() {
        return;
    }
    let Ok(camera) = cameras.single() else { return };
    if !camera.is_changed() && culling.shown.len() == all.len() {
        return;
    }
    let eye = game_to_bevy().inverse() * camera.translation;
    let current = room_at(all, eye.x, eye.y);
    let fog_far = scene.0.fog.as_ref().map_or(f32::INFINITY, |fog| fog.far);
    let shown: Vec<bool> = (0..all.len())
        .map(|index| culling.room_visible(all, current, eye, fog_far, index))
        .collect();
    if shown == culling.shown {
        return;
    }
    for (room, mut visibility) in &mut rooms {
        visibility.set_if_neq(if shown[room.0] {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        });
    }
    culling.shown = shown;
    let visible: Vec<&str> = (0..all.len())
        .filter(|&index| culling.shown[index])
        .map(|index| all[index].name.as_str())
        .collect();
    let name = current.map(|index| all[index].name.as_str());
    info!(
        "room {}: {} of {} rooms visible",
        name.unwrap_or("(none)"),
        visible.len(),
        all.len()
    );
    facts.0.insert(
        "culling".into(),
        serde_json::json!({ "room": name, "visible": visible, "rooms": all.len() }),
    );
}

/// Chooses the lights as reone does: up to `MAX_LIGHTS` lights nearest
/// the camera, each within its radius plus `LIGHT_RADIUS_BIAS`. When the
/// set changes the materials' shared list (`AREA_LIGHTS`) is written anew.
fn pick_lights(
    cameras: Query<Ref<Transform>, With<FlyCamera>>,
    lights: Option<ResMut<SceneLights>>,
    (carried, bodies, shots): (
        Res<CarriedLights>,
        Res<BodyLights>,
        Res<crate::blaster_view::ShotLights>,
    ),
    (mut buffers, mut area): (ResMut<Assets<ShaderBuffer>>, ResMut<material::AreaLights>),
    mut facts: ResMut<DumpFacts>,
) {
    let Some(mut lights) = lights else { return };
    let Ok(camera) = cameras.single() else { return };
    let moved = carried.is_changed() || bodies.is_changed() || shots.is_changed();
    if !camera.is_changed() && !moved && lights.active.is_some() {
        return;
    }
    let eye = camera.translation;
    // The scene's lights, then the carried ones, by one index.
    let all: Vec<GpuLight> = (lights.all.iter())
        .chain(&carried.0)
        .chain(&bodies.0)
        .chain(&shots.0)
        .copied()
        .collect();
    let mut near: Vec<(usize, f32)> = all
        .iter()
        .enumerate()
        .filter_map(|(index, light)| {
            let distance2 = light.position_radius.truncate().distance_squared(eye);
            let reach = light.position_radius.w + LIGHT_RADIUS_BIAS;
            (distance2 < reach * reach).then_some((index, distance2))
        })
        .collect();
    near.sort_by(|a, b| a.1.total_cmp(&b.1));
    let active: Vec<usize> = near
        .iter()
        .take(MAX_LIGHTS)
        .map(|(index, _)| *index)
        .collect();
    if lights.active.as_ref() == Some(&active) && !moved {
        return;
    }
    // The flash stays as the lightning left it.
    let flash = (area.0.flash_at, area.0.flash_colour);
    area.0 = LightList::of(active.iter().map(|index| all[*index]));
    (area.0.flash_at, area.0.flash_colour) = flash;
    area.upload(&mut buffers);
    facts.0.insert(
        "lighting".into(),
        serde_json::json!({
            "candidates": near.len(),
            "active": active.len(),
            "carried": carried.0.len() + bodies.0.len() + shots.0.len(),
        }),
    );
    lights.active = Some(active);
}

/// The time animations read: the clock, or what `anim <seconds>` holds.
pub(crate) fn animation_seconds(time: &Time, held: &AnimationTime) -> f32 {
    held.0.unwrap_or_else(|| time.elapsed_secs())
}

/// A surface's scrolling texture, as reone has it: the offset moves by
/// `250 × direction` per second and wraps to [0, 1). Computed from the
/// animation time rather than stepped, so a held time gives the same frame.
fn scroll_uvs(
    time: Res<Time>,
    held: Res<AnimationTime>,
    scrolling: Res<ScrollingMaterials>,
    mut materials: ResMut<Assets<OdysseyMaterial>>,
) {
    let seconds = animation_seconds(&time, &held);
    for (handle, dir) in &scrolling.0 {
        let offset = UV_ANIMATION_SPEED * *dir * seconds;
        let offset = offset - offset.floor();
        if materials
            .get(handle)
            .is_some_and(|material| material.params.uv_offset != offset)
            && let Some(mut material) = materials.get_mut(handle)
        {
            material.params.uv_offset = offset;
        }
    }
}

/// Diffuse flip-books. Neither reference animates a diffuse flip-book;
/// this uses the timing of reone's bump-map cycle: the
/// cycle lasts `frames / fps`, and the frame is
/// `round((frames - 1) × time / length)`.
fn cycle_flipbooks(
    time: Res<Time>,
    held: Res<AnimationTime>,
    flipbooks: Res<FlipBooks>,
    mut materials: ResMut<Assets<OdysseyMaterial>>,
) {
    for (handle, frames, fps) in &flipbooks.0 {
        let frame = flipbook_frame(frames.len(), *fps, animation_seconds(&time, &held));
        let wanted = &frames[frame];
        if materials
            .get(handle)
            .is_some_and(|material| material.base.as_ref() != Some(wanted))
            && let Some(mut material) = materials.get_mut(handle)
        {
            material.base = Some(wanted.clone());
        }
    }
}

/// A clip looping on a model: each track bound to its node's entity and
/// rest pose.
#[derive(Component)]
pub struct Animator {
    clip: std::sync::Arc<asset_odyssey::anim::AnimationIr>,
    scale: f32,
    /// (node entity, rest translation, rest rotation, track index).
    bindings: Vec<(Entity, Vec3, Quat, usize)>,
}

/// Binds a clip's tracks to a model's nodes (by name, animated nodes at
/// or under the clip's root, as reone).
pub(crate) fn animator(
    placed: &Placed,
    clip: &std::sync::Arc<asset_odyssey::anim::AnimationIr>,
    nodes: &[Entity],
) -> Option<Animator> {
    let ir = &placed.ir;
    let bindings: Vec<(Entity, Vec3, Quat, usize)> = clip
        .bind(ir)
        .into_iter()
        .enumerate()
        .filter_map(|(index, track)| {
            let node = &ir.nodes[index];
            let [x, y, z, w] = node.rotation;
            Some((
                *nodes.get(index)?,
                Vec3::from_array(node.translation),
                Quat::from_xyzw(x, y, z, w),
                track?,
            ))
        })
        .collect();
    (!bindings.is_empty() && clip.length > 0.0).then(|| Animator {
        clip: clip.clone(),
        scale: ir.animation_scale,
        bindings,
    })
}

/// A clip's pose, as reone computes it: position = rest + scale × key, the
/// orientation the key's; nodes without a key keep their rest pose.
fn animate(
    time: Res<Time>,
    held: Res<AnimationTime>,
    animators: Query<&Animator>,
    mut transforms: Query<&mut Transform>,
) {
    let seconds = animation_seconds(&time, &held);
    for animator in &animators {
        animator.pose(seconds % animator.clip.length, &mut transforms);
    }
}

impl Animator {
    /// This animator with its position keys times `scale` instead of the
    /// model's animation scale (reone plays a cutscene's stunt clips at 1).
    pub(crate) fn at_scale(self, scale: f32) -> Animator {
        Animator { scale, ..self }
    }

    /// The clip's name.
    pub(crate) fn name(&self) -> &str {
        &self.clip.name
    }

    /// The clip's length, seconds.
    pub(crate) fn length(&self) -> f32 {
        self.clip.length
    }

    /// The clip it plays.
    pub(crate) fn clip(&self) -> &std::sync::Arc<asset_odyssey::anim::AnimationIr> {
        &self.clip
    }

    /// How long a clip takes over from the one before it (seconds): its
    /// own transition time, else a quarter of a second (our
    /// chosen blend time for a clip that names none).
    pub(crate) fn transition(&self) -> f32 {
        if self.clip.transition > 0.0 {
            self.clip.transition
        } else {
            0.25
        }
    }

    /// A bound node's place and turn `at` seconds in.
    fn local(&self, at: f32, (rest_t, rest_r, track): (Vec3, Quat, usize)) -> (Vec3, Quat) {
        let (place, [x, y, z, w]) = self.clip.local(
            Some(track),
            (rest_t.to_array(), rest_r.to_array()),
            self.scale,
            at,
        );
        (Vec3::from_array(place), Quat::from_xyzw(x, y, z, w))
    }

    /// Poses the bound nodes as the clip has them `at` seconds in.
    pub(crate) fn pose(&self, at: f32, transforms: &mut Query<&mut Transform>) {
        for &(entity, rest_t, rest_r, track) in &self.bindings {
            let (translation, rotation) = self.local(at, (rest_t, rest_r, track));
            if let Ok(mut transform) = transforms.get_mut(entity) {
                transform.translation = translation;
                transform.rotation = rotation;
            }
        }
    }

    /// Poses the bound nodes of this clip and of `from` (another clip of
    /// the model, at its own time) `toward` (0 to 1) of the way from
    /// `from`'s pose to this one's: places lerped, turns slerped; a node
    /// one clip keys and the other does not mixes with its rest pose.
    pub(crate) fn pose_from(
        &self,
        at: f32,
        (from, from_at): (&Animator, f32),
        toward: f32,
        transforms: &mut Query<&mut Transform>,
    ) {
        let mut mixed: std::collections::BTreeMap<Entity, [(Vec3, Quat); 2]> =
            std::collections::BTreeMap::new();
        for (k, (animator, time)) in [(from, from_at), (self, at)].into_iter().enumerate() {
            for &(entity, rest_t, rest_r, track) in &animator.bindings {
                mixed.entry(entity).or_insert([(rest_t, rest_r); 2])[k] =
                    animator.local(time, (rest_t, rest_r, track));
            }
        }
        for (entity, [(ta, ra), (tb, rb)]) in mixed {
            if let Ok(mut transform) = transforms.get_mut(entity) {
                transform.translation = ta.lerp(tb, toward);
                transform.rotation = ra.slerp(rb, toward);
            }
        }
    }

    /// Mixes the clip `at` seconds in over the bound nodes' pose as it
    /// stands, `weight` (0 to 1) of the way: places lerped, turns slerped.
    pub(crate) fn pose_over(&self, at: f32, weight: f32, transforms: &mut Query<&mut Transform>) {
        for &(entity, rest_t, rest_r, track) in &self.bindings {
            let (translation, rotation) = self.local(at, (rest_t, rest_r, track));
            if let Ok(mut transform) = transforms.get_mut(entity) {
                transform.translation = transform.translation.lerp(translation, weight);
                transform.rotation = transform.rotation.slerp(rotation, weight);
            }
        }
    }

    /// Adds the clip's motion `at` seconds in to the pose as it stands,
    /// `weight` (0 to 1) of it: each bound node of `turned` turns on by as
    /// much as the clip turns it from its first frame (in the node's own
    /// frame). Only nodes posed afresh this frame take it, or it would
    /// build up.
    pub(crate) fn add_over(
        &self,
        at: f32,
        weight: f32,
        turned: &[Entity],
        transforms: &mut Query<&mut Transform>,
    ) {
        for &(entity, rest_t, rest_r, track) in &self.bindings {
            if !turned.contains(&entity) {
                continue;
            }
            let (_, from) = self.local(0.0, (rest_t, rest_r, track));
            let (_, to) = self.local(at, (rest_t, rest_r, track));
            if let Ok(mut transform) = transforms.get_mut(entity) {
                let by = Quat::IDENTITY.slerp(from.inverse() * to, weight);
                transform.rotation = (transform.rotation * by).normalize();
            }
        }
    }

    /// Puts the bound nodes back in their rest pose.
    pub(crate) fn rest(&self, transforms: &mut Query<&mut Transform>) {
        for &(entity, rest_t, rest_r, _) in &self.bindings {
            if let Ok(mut transform) = transforms.get_mut(entity) {
                transform.translation = rest_t;
                transform.rotation = rest_r;
            }
        }
    }
}

/// What a live blade (`BladeSpec::live`) should be this frame, by its
/// index in `LoadedScene::blades`; one past the end is out.
#[derive(Resource, Default)]
pub struct LiveBlades(pub Vec<LiveBlade>);

impl LiveBlades {
    pub fn get(&self, spec: usize) -> LiveBlade {
        const OUT: LiveBlade = LiveBlade {
            lit: false,
            trail_ms: 0.0,
        };
        self.0.get(spec).copied().unwrap_or(OUT)
    }
}

/// Where each live blade was drawn this frame, by its index in
/// `LoadedScene::blades`: its hilt and tip in game space (metres), `None`
/// while it is out.
#[derive(Resource, Default)]
pub struct DrawnBlades(pub Vec<Option<[Vec3; 2]>>);

#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct LiveBlade {
    /// Lit: it grows to full length, as JA's cgame grows it
    /// (`asset_ja::Ignition`); out, it shrinks away.
    pub lit: bool,
    /// How long the trail segment it leaves this frame lasts (ms; 0: none).
    pub trail_ms: f32,
}

/// A live blade's state from frame to frame: its length, and where it
/// was the last frames, for its trail.
#[derive(Default)]
struct Live {
    ignition: asset_ja::Ignition,
    recent: std::collections::VecDeque<crate::viewer::TrailKey>,
}

/// The longest a trail segment lives (ms; a special's 2000 / 5).
const LONGEST_TRAIL_MS: f32 = 400.0;

impl Live {
    /// The blade's length (units) this frame, at `seconds`, toward `full`
    /// lit or 0 out.
    fn length(&mut self, lit: bool, seconds: f32, full: f32) -> f32 {
        let target = if lit { full } else { 0.0 };
        self.ignition.frame(seconds * 1000.0, target, full);
        self.ignition.length
    }

    /// Adds the blade's place this frame to the trail (one key per frame,
    /// as the cgame adds one) and drops the segments gone by.
    fn trail(&mut self, key: crate::viewer::TrailKey) {
        if self.recent.back().is_some_and(|last| last.time >= key.time) {
            return;
        }
        self.recent.push_back(key);
        let gone = key.time - LONGEST_TRAIL_MS / 1000.0;
        while self.recent.len() > 2 && self.recent[1].time < gone {
            self.recent.pop_front();
        }
    }
}

/// A saber blade grown from its hilt each frame.
#[derive(Component)]
struct Blade {
    /// Its index in `LoadedScene::blades`.
    spec: usize,
    /// For a live blade, its state.
    live: Option<Live>,
    hilt: Entity,
    /// In the hilt's space (metres).
    origin: Vec3,
    dir: Vec3,
    length: f32,
    radius: f32,
    glow: Handle<Mesh>,
    core: Handle<Mesh>,
    /// (seconds, [Quake units, 0, 0]) keys, as `sample_position` reads
    /// them; empty for always `length`.
    extend: Vec<(f32, [f32; 3])>,
    period: f32,
    trail: Vec<crate::viewer::TrailKey>,
    trail_mesh: Handle<Mesh>,
    trail_color: [f32; 3],
    /// Seconds of the period the blade is drawn in; empty for always.
    shown: Vec<(f32, f32)>,
    /// Whether the glow and core meshes, and the trail's, hold nothing now
    /// (so a blade that stays hidden is not rebuilt every frame).
    glow_empty: bool,
    trail_empty: bool,
}

/// A model moving on its own: its transform at times (seconds), within a
/// period.
#[derive(Component)]
struct FollowPath {
    keys: Vec<(f32, Vec3, Quat)>,
    period: f32,
}

/// A model shown only within these (from, to) seconds of each period.
#[derive(Component)]
struct ShownDuring {
    ranges: Vec<(f32, f32)>,
    period: f32,
}

/// Seconds into a repeating period (the time itself when there is none).
pub(crate) fn within(seconds: f32, period: f32) -> f32 {
    if period > 0.0 {
        seconds % period
    } else {
        seconds
    }
}

fn show_during(
    time: Res<Time>,
    held: Res<AnimationTime>,
    mut models: Query<(&ShownDuring, &mut Visibility)>,
) {
    let seconds = animation_seconds(&time, &held);
    for (shown, mut visibility) in &mut models {
        let at = within(seconds, shown.period);
        let on = shown.ranges.iter().any(|&(from, to)| at >= from && at < to);
        *visibility = if on {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        };
    }
}

/// Moves models along their paths: positions lerped, turns slerped between
/// keys, held before the first and after the last.
fn follow_paths(
    time: Res<Time>,
    held: Res<AnimationTime>,
    mut models: Query<(&FollowPath, &mut Transform)>,
) {
    let seconds = animation_seconds(&time, &held);
    for (path, mut transform) in &mut models {
        let at = within(seconds, path.period);
        let next = path.keys.partition_point(|key| key.0 <= at);
        let (position, rotation) = match (next.checked_sub(1), path.keys.get(next)) {
            (Some(i), Some(b)) => {
                let a = &path.keys[i];
                let f = if b.0 > a.0 {
                    (at - a.0) / (b.0 - a.0)
                } else {
                    0.0
                };
                (a.1.lerp(b.1, f), a.2.slerp(b.2, f))
            }
            (Some(i), None) => (path.keys[i].1, path.keys[i].2),
            (None, Some(b)) => (b.1, b.2),
            (None, None) => continue,
        };
        transform.translation = position;
        transform.rotation = rotation;
    }
}

/// A mesh of quads. An empty one is given a single degenerate quad: Bevy's
/// mesh allocator logs use-after-free errors for meshes that go empty.
fn blade_mesh(mut positions: Vec<[f32; 3]>, mut uvs: Vec<[f32; 2]>) -> Mesh {
    if positions.is_empty() {
        positions = vec![[0.0; 3]; 4];
        uvs = vec![[0.0; 2]; 4];
    }
    let quads = positions.len() / 4;
    let indices: Vec<u32> = (0..quads as u32)
        .flat_map(|q| [0, 1, 2, 0, 2, 3].map(|i| q * 4 + i))
        .collect();
    let normals = vec![[0.0, 0.0, 1.0]; positions.len()];
    let mut mesh = Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::default(),
    );
    mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, positions);
    mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, normals);
    mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, uvs);
    mesh.insert_indices(Indices::U32(indices));
    mesh
}

/// A blade's trail at `at` (seconds) from its keys, one per client frame
/// it was drawn: every two keys in a row are a segment, a quad from the
/// newer key's edge back to the older one's, showing for the newer key's
/// life and fading out over it, its texture sliding along as it ages.
/// Coloured `color`; none when no segment shows.
fn trail_mesh(keys: &[crate::viewer::TrailKey], at: f32, color: [f32; 3]) -> Option<Mesh> {
    let game = game_to_bevy();
    let mut positions = Vec::new();
    let mut uvs = Vec::new();
    let mut colours = Vec::new();
    for pair in keys.windows(2) {
        let (old, new) = (&pair[0], &pair[1]);
        let life = new.life_ms / 1000.0;
        // Skipped when it has no life, is not made yet or is gone (a NaN
        // anywhere skips nothing).
        if life <= 0.0 || at < new.time || at >= new.time + life {
            continue;
        }
        let age = (at - new.time) / life;
        // How far back the older edge's texture starts: one less what is
        // left of that edge's alpha.
        let back = 1.0 - (1.0 - (new.time - old.time) / life);
        let colour = [color[0], color[1], color[2], 1.0 - age];
        let corners = [
            (new.base, [age, 1.0]),
            (new.tip, [age, 0.0]),
            (old.tip, [back + age, 0.0]),
            (old.base, [back + age, 1.0]),
        ];
        for (point, uv) in corners {
            positions.push((game * Vec3::from_array(point)).to_array());
            uvs.push(uv);
            colours.push(colour);
        }
    }
    (!positions.is_empty()).then(|| coloured_quads(positions, uvs, colours))
}

/// `blade_mesh` with vertex colours; never empty: an invisible quad keeps
/// the mesh allocated.
pub(crate) fn coloured_quads(
    mut positions: Vec<[f32; 3]>,
    mut uvs: Vec<[f32; 2]>,
    mut colours: Vec<[f32; 4]>,
) -> Mesh {
    if positions.is_empty() {
        positions = vec![[0.0; 3]; 4];
        uvs = vec![[0.0; 2]; 4];
        colours = vec![[0.0; 4]; 4];
    }
    let mut mesh = blade_mesh(positions, uvs);
    mesh.insert_attribute(Mesh::ATTRIBUTE_COLOR, colours);
    mesh
}

/// Triangles by `indices` with texture coordinates and colours (an
/// emitter's chunks); a degenerate quad when there are none, as
/// `coloured_quads`.
pub(crate) fn coloured_triangles(
    positions: Vec<[f32; 3]>,
    uvs: Vec<[f32; 2]>,
    colours: Vec<[f32; 4]>,
    indices: Vec<u32>,
) -> Mesh {
    if indices.is_empty() {
        return coloured_quads(Vec::new(), Vec::new(), Vec::new());
    }
    let normals = vec![[0.0, 0.0, 1.0]; positions.len()];
    let mut mesh = Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::default(),
    );
    mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, positions);
    mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, normals);
    mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, uvs);
    mesh.insert_attribute(Mesh::ATTRIBUTE_COLOR, colours);
    mesh.insert_indices(Indices::U32(indices));
    mesh
}

/// A repeatable stand-in for the game's per-frame random number in
/// [-1, 1], from the animation time and a counter, so held shots repeat.
fn flicker(seconds: f32, salt: u32) -> f32 {
    let x = crate::view_rng::mix32_salted((seconds * 1000.0) as u32, salt);
    (x as f32 / u32::MAX as f32) * 2.0 - 1.0
}

/// Draws every saber blade this frame as JA's client draws one: a live
/// blade grows and shrinks with its ignition and leaves a trail key each
/// frame, a recorded one follows its clip's length and shown ranges. A lit
/// blade is a row of camera-facing glow sprites from the tip back to the
/// hilt, each a little larger than the last, a larger sprite at the hilt,
/// and a core quad from the tip to a unit behind the hilt turned to face
/// the eye; the radii jitter a little each frame, and a blade still growing
/// or shrinking is drawn thicker. Fills `DrawnBlades` and the `blade_ends`
/// dump fact. Nothing happens without exactly one scene camera.
#[allow(clippy::too_many_arguments)]
fn draw_blades(
    (time, held): (Res<Time>, Res<AnimationTime>),
    mut blades: Query<&mut Blade>,
    lives: Res<LiveBlades>,
    mut drawn: ResMut<DrawnBlades>,
    transforms: Query<&GlobalTransform>,
    cameras: Query<&GlobalTransform, With<FlyCamera>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut facts: ResMut<DumpFacts>,
) {
    let Ok(camera) = cameras.single() else {
        return;
    };
    drawn.0.clear();
    drawn.0.resize(lives.0.len(), None);
    let seconds = animation_seconds(&time, &held);
    let eye = camera.translation();
    let right = camera.right().as_vec3();
    let up = camera.up().as_vec3();
    let to_game = game_to_bevy().inverse();
    let unit = asset_ja::UNIT;
    let rounded = |v: Vec3| {
        (to_game * v)
            .to_array()
            .map(|x| (x * 1000.0).round() / 1000.0)
    };
    let mut ends = Vec::new();

    for (place, mut blade) in blades.iter_mut().enumerate() {
        let salt = place as u32 * 16;
        let Ok(hilt) = transforms.get(blade.hilt) else {
            continue;
        };
        let blade = &mut *blade;
        let origin = hilt.transform_point(blade.origin);
        let dir = hilt
            .affine()
            .transform_vector3(blade.dir)
            .normalize_or_zero();
        let at = within(seconds, blade.period);

        // How long it is now (units), and its trail, if it draws one.
        let (current, trail) = match &mut blade.live {
            Some(live) => {
                let wanted = lives.get(blade.spec);
                let current = live.length(wanted.lit, seconds, blade.length);
                live.trail(crate::viewer::TrailKey {
                    time: seconds,
                    base: (to_game * origin).to_array(),
                    // Three units past the blade's tip.
                    tip: (to_game * (origin + dir * (current + 3.0) * unit)).to_array(),
                    life_ms: if current >= 0.5 { wanted.trail_ms } else { 0.0 },
                });
                let trail = trail_mesh(live.recent.make_contiguous(), seconds, blade.trail_color);
                (current, Some(trail))
            }
            None => {
                let hidden = !blade.shown.is_empty()
                    && !blade.shown.iter().any(|&(from, to)| from <= at && at < to);
                let length = if hidden {
                    0.0
                } else {
                    asset_odyssey::anim::sample_position(&blade.extend, at)
                        .map_or(blade.length, |extent| extent[0])
                };
                let trail = (!blade.trail.is_empty())
                    .then(|| trail_mesh(&blade.trail, at, blade.trail_color));
                (length, trail)
            }
        };
        if let Some(trail) = trail {
            let empty = trail.is_none();
            if !(empty && blade.trail_empty)
                && let Some(mut asset) = meshes.get_mut(&blade.trail_mesh)
            {
                *asset =
                    trail.unwrap_or_else(|| coloured_quads(Vec::new(), Vec::new(), Vec::new()));
            }
            blade.trail_empty = empty;
        }

        if current < 0.5 {
            if !blade.glow_empty {
                for handle in [&blade.glow, &blade.core] {
                    if let Some(mut asset) = meshes.get_mut(handle) {
                        *asset = blade_mesh(Vec::new(), Vec::new());
                    }
                }
                blade.glow_empty = true;
            }
            continue;
        }
        blade.glow_empty = false;
        // Growing or shrinking, it is drawn thicker.
        let fatter = if current < blade.length {
            1.0 + 2.0 / current
        } else {
            1.0
        };
        // The radii jitter by up to 7.5 %.
        let range = blade.radius * 0.075;
        let length = current * unit;

        // The glow: sprites from the tip back to the hilt, then one at it.
        let mut positions = Vec::new();
        let mut uvs = Vec::new();
        let mut sprite = |centre: Vec3, r: f32| {
            let corners = [
                (-right - up, [0.0, 1.0]),
                (right - up, [1.0, 1.0]),
                (right + up, [1.0, 0.0]),
                (-right + up, [0.0, 0.0]),
            ];
            for (corner, uv) in corners {
                positions.push((centre + corner * r).to_array());
                uvs.push(uv);
            }
        };
        let mut along = length;
        let mut r = (blade.radius - range + flicker(seconds, salt) * range) * fatter * unit;
        while along > 0.0 {
            sprite(origin + dir * along, r);
            along -= r * 0.65;
            r += 0.017 * unit;
        }
        sprite(
            origin,
            (5.5 + (flicker(seconds, salt + 1) * 0.5 + 0.5) * 0.25) * unit,
        );
        if let Some(mut asset) = meshes.get_mut(&blade.glow) {
            *asset = blade_mesh(positions, uvs);
        }

        let start = origin + dir * length;
        let end = origin - dir * unit;
        ends.push(serde_json::json!({ "hilt": rounded(origin), "tip": rounded(start) }));
        if blade.live.is_some()
            && let Some(slot) = drawn.0.get_mut(blade.spec)
        {
            *slot = Some([to_game * origin, to_game * start]);
        }

        // The core: one quad along the blade, turned to face the eye.
        let side = (start - eye).cross(end - eye).normalize_or_zero();
        let w = (blade.radius / 3.0 + flicker(seconds, salt + 2) * range) * fatter * unit;
        let core = vec![
            (start + side * w).to_array(),
            (start - side * w).to_array(),
            (end - side * w).to_array(),
            (end + side * w).to_array(),
        ];
        let core_uvs = vec![[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]];
        if let Some(mut asset) = meshes.get_mut(&blade.core) {
            *asset = blade_mesh(core, core_uvs);
        }
    }
    facts
        .0
        .insert("blade_ends".into(), serde_json::Value::Array(ends));
}
