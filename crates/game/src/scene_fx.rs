//! The scene's effects (`viewer::Effects`), rebuilt every frame around the
//! camera as JA's renderer draws them: sprites and puffs face the view,
//! lightning is laid out from the eye (`asset_ja::fx::bolt`), and a push's
//! distortion shows a copy of the screen (below). One mesh per shader,
//! blended as the shader's first stage says.
//!
//! Each frame shows the last client frame's effects, so a held shot
//! repeats, and the game's per-frame random numbers come from the frame
//! and the effect's place in the list.
//!
//! A push or a pull shows as its refraction model (`testboom.md3`) at the
//! hand, facing away from the eye and scaled, its texture coordinates a
//! copy of a square of the screen round the hand (larger the nearer the
//! hand), turning and rippling as the model's shader moves them, and
//! fading (`distort`). JA's renderer reads the turn's sines from a table
//! (`table_sin`), and takes the view's left for its right, so the square
//! it copies is mirrored; here the corner is mirrored back.

use std::collections::BTreeMap;

use asset_ja::fx::{self, Kind, Rng};
use bevy::asset::{RenderAssetUsages, load_internal_asset, uuid_handle};
use bevy::camera::visibility::NoFrustumCulling;
use bevy::mesh::{Indices, MeshVertexBufferLayoutRef, PrimitiveTopology};
use bevy::pbr::{MaterialPipeline, MaterialPipelineKey};
use bevy::prelude::*;
use bevy::render::render_resource::{
    AsBindGroup, BlendComponent, BlendFactor, BlendOperation, BlendState, RenderPipelineDescriptor,
    SpecializedMeshPipelineError,
};
use bevy::shader::ShaderRef;
use console::AnimationTime;

use asset_odyssey::texture::{FrameStrip, TextureIr};

use crate::material::{AreaLight, Lighting, OdysseyMaterial, Surface};
use crate::scene::SceneEntity;
use crate::viewer::{Effect, EffectBlend, Effects};

const SHADER: Handle<Shader> = uuid_handle!("8a2f6c1e-47b3-4f0e-b2d9-6c5e1a7d3f90");
const UNIT: f32 = asset_ja::UNIT;

pub struct EffectsPlugin;

impl Plugin for EffectsPlugin {
    fn build(&self, app: &mut App) {
        load_internal_asset!(app, SHADER, "refraction.wgsl", Shader::from_wgsl);
        app.add_plugins(MaterialPlugin::<RefractionMaterial>::default())
            .add_systems(Startup, keep_transmission)
            .add_systems(
                PostUpdate,
                // Before the meshes' changes are sent on: after it, the
                // effects would show a frame late (as the blades did).
                draw_effects
                    .after(bevy::transform::TransformSystems::Propagate)
                    .before(bevy::asset::AssetEventSystems),
            );
    }
}

/// An empty refraction batch kept for good: Bevy copies the scene behind
/// (the view transmission texture) only while its transmissive phase has
/// an item, and the surfaces that bend it from the transparent phase
/// (`glow_shell`'s distortion, `heat_haze`, `speed_blur`) put none there,
/// so without JA's refraction (`force_view`) they would show black.
fn keep_transmission(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut refractions: ResMut<Assets<RefractionMaterial>>,
) {
    commands.spawn((
        Mesh3d(meshes.add(Batch::default().into_mesh())),
        MeshMaterial3d(refractions.add(RefractionMaterial::default())),
        Transform::IDENTITY,
        NoFrustumCulling,
    ));
}

/// The screen copy a distortion shows (see the module docs).
#[derive(Asset, TypePath, AsBindGroup, Clone, Debug, Default)]
pub struct RefractionMaterial {}

impl Material for RefractionMaterial {
    fn fragment_shader() -> ShaderRef {
        ShaderRef::Handle(SHADER)
    }

    fn reads_view_transmission_texture(&self) -> bool {
        true
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
        // The game draws the model with its entity alpha forced, which
        // overrides the shader's `blendFunc GL_SRC_ALPHA GL_SRC_COLOR` with
        // source alpha, one minus source alpha.
        let blend = BlendState {
            color: BlendComponent {
                src_factor: BlendFactor::SrcAlpha,
                dst_factor: BlendFactor::OneMinusSrcAlpha,
                operation: BlendOperation::Add,
            },
            alpha: BlendComponent {
                src_factor: BlendFactor::Zero,
                dst_factor: BlendFactor::One,
                operation: BlendOperation::Add,
            },
        };
        if let Some(fragment) = descriptor.fragment.as_mut() {
            for target in fragment.targets.iter_mut().flatten() {
                target.blend = Some(blend);
            }
        }
        if let Some(depth) = descriptor.depth_stencil.as_mut() {
            depth.depth_write_enabled = Some(false);
        }
        Ok(())
    }
}

/// How an effect shader draws: its blend, and its texture's frames when it
/// is an animated map.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Look {
    pub blend: EffectBlend,
    pub strip: Option<FrameStrip>,
}

/// One mesh per effect shader (lower-case name), with its look, and the
/// meshes that hold nothing now (left alone while they stay empty).
pub(crate) struct ShaderMeshes {
    shaders: BTreeMap<String, (Handle<Mesh>, Look)>,
    empty: std::collections::BTreeSet<String>,
}

impl ShaderMeshes {
    fn new(shaders: BTreeMap<String, (Handle<Mesh>, Look)>) -> ShaderMeshes {
        ShaderMeshes {
            empty: shaders.keys().cloned().collect(),
            shaders,
        }
    }

    /// A mesh and an unlit material per shader of `blends`, with its
    /// texture from `textures` (by name); they outlive the scenes.
    pub(crate) fn spawn(
        commands: &mut Commands,
        (blends, textures): (&BTreeMap<String, EffectBlend>, &BTreeMap<String, TextureIr>),
        images: &mut Assets<Image>,
        meshes: &mut Assets<Mesh>,
        materials: &mut Assets<OdysseyMaterial>,
    ) -> ShaderMeshes {
        let mut shaders = BTreeMap::new();
        for (name, &blend) in blends {
            let texture = textures.get(name);
            let strip = texture.and_then(|texture| texture.facts.strip);
            let texture = texture.map(|texture| images.add(crate::scene::image(texture)));
            let material = material(blend, texture, [1.0; 3], materials);
            let mesh = meshes.add(Batch::default().into_mesh());
            commands.spawn((
                Mesh3d(mesh.clone()),
                MeshMaterial3d(material),
                Transform::IDENTITY,
                NoFrustumCulling,
            ));
            shaders.insert(name.clone(), (mesh, Look { blend, strip }));
        }
        ShaderMeshes::new(shaders)
    }

    /// The blend of a shader (lower-case), if it has a mesh.
    pub(crate) fn blend(&self, shader: &str) -> Option<EffectBlend> {
        self.look(shader).map(|look| look.blend)
    }

    /// The look of a shader (lower-case), if it has a mesh.
    pub(crate) fn look(&self, shader: &str) -> Option<Look> {
        self.shaders.get(shader).map(|&(_, look)| look)
    }

    /// Each shader's batch into its mesh (an empty one for a shader with
    /// none).
    pub(crate) fn fill(&mut self, mut batches: BTreeMap<String, Batch>, meshes: &mut Assets<Mesh>) {
        for (name, (mesh, _)) in &self.shaders {
            let batch = batches.remove(name).unwrap_or_else(Batch::default);
            let empty = batch.is_empty();
            if empty && self.empty.contains(name) {
                continue;
            }
            if let Some(mut handle) = meshes.get_mut(mesh) {
                *handle = batch.into_mesh();
            }
            if empty {
                self.empty.insert(name.clone());
            } else {
                self.empty.remove(name);
            }
        }
    }
}

/// The effects to draw and the meshes they are drawn into.
#[derive(Resource)]
pub struct EffectMeshes {
    effects: Effects,
    shaders: ShaderMeshes,
    refraction: Option<Handle<Mesh>>,
    /// The distortion's mesh holds nothing now.
    refraction_empty: bool,
}

/// Quads (fan order) with texture coordinates and colours; never empty,
/// since Bevy's mesh allocator logs errors for meshes that go empty.
fn quads(mut positions: Vec<[f32; 3]>, mut uvs: Vec<[f32; 2]>, mut colours: Vec<[f32; 4]>) -> Mesh {
    if positions.is_empty() {
        positions = vec![[0.0; 3]; 4];
        uvs = vec![[0.0; 2]; 4];
        colours = vec![[0.0; 4]; 4];
    }
    let indices: Vec<u32> = (0..(positions.len() / 4) as u32)
        .flat_map(|q| [0, 1, 2, 0, 2, 3].map(|i| q * 4 + i))
        .collect();
    triangles(positions, uvs, colours, indices)
}

fn triangles(
    positions: Vec<[f32; 3]>,
    uvs: Vec<[f32; 2]>,
    colours: Vec<[f32; 4]>,
    indices: Vec<u32>,
) -> Mesh {
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

/// Spawns a mesh per effect shader and the distortion's, and hands the
/// effects to `draw_effects`. `textures` are the scene's by name. The
/// meshes belong to the scene (`SceneEntity`): the next scene despawns them,
/// and a scene without effects leaves none to draw.
pub fn spawn(
    commands: &mut Commands,
    effects: &Effects,
    (textures, irs): (&BTreeMap<&str, Handle<Image>>, &BTreeMap<String, TextureIr>),
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<OdysseyMaterial>,
    refractions: &mut Assets<RefractionMaterial>,
    ambient: [f32; 3],
) {
    if effects.items.is_empty() {
        commands.remove_resource::<EffectMeshes>();
        return;
    }
    let mut shaders = BTreeMap::new();
    for (name, &blend) in &effects.blends {
        let material = material(
            blend,
            textures.get(name.as_str()).cloned(),
            ambient,
            materials,
        );
        let mesh = meshes.add(quads(Vec::new(), Vec::new(), Vec::new()));
        commands.spawn((
            Mesh3d(mesh.clone()),
            MeshMaterial3d(material),
            Transform::IDENTITY,
            NoFrustumCulling,
            SceneEntity,
        ));
        let strip = irs.get(name).and_then(|texture| texture.facts.strip);
        shaders.insert(name.clone(), (mesh, Look { blend, strip }));
    }
    let refraction = effects.refraction_model.as_ref().map(|_| {
        let mesh = meshes.add(quads(Vec::new(), Vec::new(), Vec::new()));
        commands.spawn((
            Mesh3d(mesh.clone()),
            MeshMaterial3d(refractions.add(RefractionMaterial::default())),
            Transform::IDENTITY,
            NoFrustumCulling,
            SceneEntity,
        ));
        mesh
    });
    commands.insert_resource(EffectMeshes {
        effects: effects.clone(),
        shaders: ShaderMeshes::new(shaders),
        refraction,
        refraction_empty: true,
    });
}

/// An effect shader's material: its texture, unlit, blended as its first
/// stage says.
pub(crate) fn material(
    blend: EffectBlend,
    texture: Option<Handle<Image>>,
    ambient: [f32; 3],
    materials: &mut Assets<OdysseyMaterial>,
) -> Handle<OdysseyMaterial> {
    let surface = match blend {
        EffectBlend::Add => Surface::AddOne,
        EffectBlend::Alpha => Surface::BlendVertex,
        EffectBlend::Modulate => Surface::Modulate,
    };
    materials.add(OdysseyMaterial::new(
        Vec4::ONE,
        texture,
        None,
        None,
        surface,
        &Lighting {
            self_illum: [0.0; 3],
            ambient: [1.0; 3],
            diffuse: [1.0; 3],
            alpha: 1.0,
            is_static: false,
            is_room: true,
            fogged: false,
        },
        &AreaLight::ambient(ambient),
    ))
}

/// Quads being gathered for one shader's mesh.
#[derive(Default)]
pub(crate) struct Batch {
    positions: Vec<[f32; 3]>,
    uvs: Vec<[f32; 2]>,
    colours: Vec<[f32; 4]>,
}

impl Batch {
    pub(crate) fn quad(
        &mut self,
        corners: [(Vec3, [f32; 2]); 4],
        colour: [f32; 4],
        to_bevy: impl Fn(Vec3) -> Vec3,
    ) {
        for (p, uv) in corners {
            self.positions.push(to_bevy(p).to_array());
            self.uvs.push(uv);
            self.colours.push(colour);
        }
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.positions.is_empty()
    }

    /// The texture coordinates from vertex `from` on moved to frame
    /// `frame` of a strip of `frames` (`FrameStrip`).
    fn show_frame(&mut self, from: usize, frame: u32, frames: u32) {
        let (frame, frames) = (frame as f32, frames.max(1) as f32);
        for uv in &mut self.uvs[from..] {
            uv[1] = (frame + uv[1].clamp(0.0, 1.0)) / frames;
        }
    }

    /// The quads as one mesh (a degenerate quad when there are none).
    pub(crate) fn into_mesh(self) -> Mesh {
        quads(self.positions, self.uvs, self.colours)
    }

    /// The vertices as triangles by `indices` (a degenerate quad when there
    /// are none).
    pub(crate) fn into_triangles(self, indices: Vec<u32>) -> Mesh {
        if indices.is_empty() {
            return quads(Vec::new(), Vec::new(), Vec::new());
        }
        triangles(self.positions, self.uvs, self.colours, indices)
    }

    /// A line's quad (`fx::Strip`): its two triangles share its start's far
    /// corner and its end's near one.
    fn strip(&mut self, [a, b, c, d]: fx::Strip, colour: [f32; 4], to_bevy: impl Fn(Vec3) -> Vec3) {
        self.quad([b, d, c, a], colour, to_bevy);
    }
}

/// Where effects are seen from, in game-space units: the eye and the
/// view's left and up.
pub(crate) struct Eye {
    pub eye: Vec3,
    pub left: Vec3,
    pub up: Vec3,
}

/// A primitive at `now` (client frames `frame` ms apart) into its shader's
/// batch, drawn as the renderer draws its kind: a sprite facing the view, a
/// line or lightning square to the eye's line of sight. An animated map
/// shows its frame of the moment, timed from the primitive's start when
/// it says so (`setShaderTime`), else from the clock's.
pub(crate) fn primitive(
    batch: &mut Batch,
    spawned: &fx::Spawned,
    look: Look,
    (now, frame): (f32, f32),
    rng: &mut Rng,
    eye: &Eye,
    to_bevy: impl Fn(Vec3) -> Vec3 + Copy,
) {
    let from = batch.uvs.len();
    draw_primitive(batch, spawned, look.blend, (now, frame), rng, eye, to_bevy);
    if let Some(strip) = look.strip {
        let since = if spawned.flags.contains(fx::Flags::SET_SHADER_TIME) {
            now - spawned.start_ms
        } else {
            now
        };
        batch.show_frame(from, strip.frame(since / 1000.0), strip.frames);
    }
}

fn draw_primitive(
    batch: &mut Batch,
    spawned: &fx::Spawned,
    blend: EffectBlend,
    (now, frame): (f32, f32),
    rng: &mut Rng,
    eye: &Eye,
    to_bevy: impl Fn(Vec3) -> Vec3 + Copy,
) {
    let shown = spawned.shown(now, frame, rng);
    let colour = vertex_colour(shown.rgba, blend);
    match spawned.kind {
        Kind::Particle => batch.quad(
            fx::sprite(shown.origin, shown.radius, shown.rotation, eye.left, eye.up),
            colour,
            to_bevy,
        ),
        Kind::Line => batch.strip(fx::line(spawned, shown.radius, eye.eye), colour, to_bevy),
        Kind::Electricity => {
            for strip in fx::bolt(spawned, shown.radius, now, eye.eye, rng) {
                batch.strip(strip, colour, to_bevy);
            }
        }
        Kind::OrientedParticle => batch.quad(fx::oriented(spawned, &shown), colour, to_bevy),
        Kind::Tail => batch.strip(
            fx::tail(spawned, &shown, (now, frame), eye.eye),
            colour,
            to_bevy,
        ),
        Kind::Cylinder => {
            for quad in fx::cylinder(spawned, &shown, eye.eye) {
                batch.quad(quad, colour, to_bevy);
            }
        }
        // Heard or played by their caller (`combat_fx`), or laid as marks.
        Kind::Sound | Kind::Runner | Kind::Decal => {}
    }
}

/// The view as the effects need it, in game-space units.
pub(crate) struct View {
    eye: Vec3,
    forward: Vec3,
    left: Vec3,
    up: Vec3,
    /// Horizontal and vertical fields of view (degrees) and the target's
    /// size in pixels.
    fov: (f32, f32),
    size: (f32, f32),
}

impl View {
    /// The view of a scene camera.
    pub(crate) fn of(camera: &GlobalTransform, projection: &Projection, info: &Camera) -> View {
        let to_game = crate::scene::game_to_bevy().inverse();
        let (fov_y, aspect) = match projection {
            Projection::Perspective(p) => (p.fov, p.aspect_ratio),
            _ => (60f32.to_radians(), 1.0),
        };
        let size = info
            .physical_viewport_size()
            .map_or((640.0, 480.0), |s| (s.x as f32, s.y as f32));
        let Eye { eye, left, up } = eye_of(camera);
        View {
            eye,
            forward: to_game * camera.forward().as_vec3(),
            left,
            up,
            fov: (
                (2.0 * ((fov_y / 2.0).tan() * aspect).atan()).to_degrees(),
                fov_y.to_degrees(),
            ),
            size,
        }
    }

    pub(crate) fn eye(&self) -> Eye {
        Eye {
            eye: self.eye,
            left: self.left,
            up: self.up,
        }
    }
}

/// A puff: a sprite facing the view that drifts along the view's left at
/// `speed` (units a second) from `origin` (units), turned by `rotation`
/// (degrees), growing from 8 units to `radius` + 8 and darkening to
/// nothing between `start_ms` and `end_ms`.
pub(crate) struct Puff {
    pub start_ms: f32,
    pub end_ms: f32,
    pub origin: [f32; 3],
    pub speed: f32,
    pub radius: f32,
    pub rotation: f32,
    pub colour: [u8; 3],
}

impl Puff {
    /// Whether it shows at `now` (its first and last moments included).
    pub(crate) fn alive(&self, now: f32) -> bool {
        self.start_ms <= now && now <= self.end_ms
    }

    /// Its quad at `now` into `batch`, coloured for `blend`.
    pub(crate) fn draw(
        &self,
        batch: &mut Batch,
        (blend, now): (EffectBlend, f32),
        view: &View,
        to_bevy: impl Fn(Vec3) -> Vec3,
    ) {
        // The share of its life still to come.
        let share = (self.end_ms - now) / (self.end_ms - self.start_ms).max(1.0);
        let [r, g, b] = self.colour.map(|byte| (f32::from(byte) * share) as u8);
        let drift = self.speed * (now - self.start_ms) / 1000.0;
        let place = Vec3::from_array(self.origin) + view.left * drift;
        let size = self.radius * (1.0 - share) + 8.0;
        batch.quad(
            fx::sprite(place, size, self.rotation, view.left, view.up),
            vertex_colour([r, g, b, 0], blend),
            to_bevy,
        );
    }
}

/// The eye of a camera placed in Bevy's space.
pub(crate) fn eye_of(camera: &GlobalTransform) -> Eye {
    let to_game = crate::scene::game_to_bevy().inverse();
    Eye {
        eye: to_game * camera.translation() / UNIT,
        left: to_game * -camera.right().as_vec3(),
        up: to_game * camera.up().as_vec3(),
    }
}

#[allow(clippy::too_many_arguments)]
fn draw_effects(
    time: Res<Time>,
    held: Res<AnimationTime>,
    drawn: Option<ResMut<EffectMeshes>>,
    cameras: Query<(&GlobalTransform, &Projection, &Camera), crate::lifecycle_view::SceneCamera>,
    mut meshes: ResMut<Assets<Mesh>>,
) {
    let Some(mut drawn) = drawn else { return };
    let Ok((camera, projection, camera_info)) = cameras.single() else {
        return;
    };
    let drawn = &mut *drawn;
    let effects = &drawn.effects;
    let seconds = crate::scene::animation_seconds(&time, &held);
    let at = crate::scene::within(seconds, effects.period_ms / 1000.0) * 1000.0;
    // The client frame showing: the last one by now.
    let frame = effects.frame_ms.max(1.0);
    let now = effects.frame_origin_ms + ((at - effects.frame_origin_ms) / frame).floor() * frame;
    let to_bevy = |p: Vec3| crate::scene::game_to_bevy() * (p * UNIT);
    let view = View::of(camera, projection, camera_info);
    let mut batches: BTreeMap<String, Batch> = BTreeMap::new();
    let mut refraction = Batch::default();
    let mut refraction_indices: Vec<u32> = Vec::new();
    let frame_index = (now / frame).round() as i32;
    for (index, item) in effects.items.iter().enumerate() {
        // The game's generator for this frame and effect.
        let mut rng = Rng(frame_index.wrapping_mul(7919) ^ (index as i32).wrapping_mul(104_729));
        match item {
            Effect::Fx(spawned) if spawned.alive(now) => {
                let Some(look) = drawn.shaders.look(&spawned.shader.to_ascii_lowercase()) else {
                    continue;
                };
                let batch = batches
                    .entry(spawned.shader.to_ascii_lowercase())
                    .or_default();
                primitive(
                    batch,
                    spawned,
                    look,
                    (now, frame),
                    &mut rng,
                    &view.eye(),
                    to_bevy,
                );
            }
            Effect::Puff {
                start_ms,
                end_ms,
                origin,
                speed,
                radius,
                rotation,
                colour,
                shader,
            } if now >= *start_ms && now <= *end_ms => {
                let Some(blend) = drawn.shaders.blend(&shader.to_ascii_lowercase()) else {
                    continue;
                };
                let puff = Puff {
                    start_ms: *start_ms,
                    end_ms: *end_ms,
                    origin: *origin,
                    speed: *speed,
                    radius: *radius,
                    rotation: *rotation,
                    colour: *colour,
                };
                let batch = batches.entry(shader.to_ascii_lowercase()).or_default();
                puff.draw(batch, (blend, now), &view, to_bevy);
            }
            Effect::Refraction {
                start_ms,
                end_ms,
                pull,
                path,
            } if now >= *start_ms && now < *end_ms => {
                if let Some(model) = &effects.refraction_model {
                    distort(
                        model,
                        (*start_ms, *pull, path),
                        now,
                        &view,
                        &mut refraction,
                        &mut refraction_indices,
                        to_bevy,
                    );
                }
            }
            _ => {}
        }
    }
    drawn.shaders.fill(batches, &mut meshes);
    let empty = refraction_indices.is_empty();
    if let Some(mesh) = &drawn.refraction
        && !(empty && drawn.refraction_empty)
    {
        if let Some(mut handle) = meshes.get_mut(mesh) {
            *handle = refraction.into_triangles(refraction_indices);
        }
        drawn.refraction_empty = empty;
    }
}

/// A shader's vertex colour from the effect's colour bytes: additive art
/// ignores alpha (it fades by darkening), alpha-blended art fades by it.
pub(crate) fn vertex_colour(rgba: [u8; 4], blend: EffectBlend) -> [f32; 4] {
    let [r, g, b, a] = rgba.map(|v| f32::from(v) / 255.0);
    match blend {
        EffectBlend::Add => [r, g, b, 1.0],
        EffectBlend::Alpha => [r, g, b, a],
        EffectBlend::Modulate => [1.0; 4],
    }
}

/// The sine of `turns` (a fraction of a turn) as JA's renderer reads it:
/// from a table of 1024 entries a turn, the fraction cut down to its entry
/// and wrapped into the table by a bit mask (NaN reads the first entry).
fn table_sin(turns: f32) -> f32 {
    let index = ((turns * 1024.0) as i32) & 1023;
    (index as f32 * std::f32::consts::TAU / 1024.0).sin()
}

/// One frame at `now` of a push's or pull's distortion cast at `start_ms`:
/// the refraction `model` at the hand (the latest place of `path` by now),
/// facing away from the eye, scaled (a pull grows, a push shrinks, held at
/// full size for its first third) and fading, showing through its texture
/// coordinates a square of the screen round the hand, those coordinates
/// turning 15° a second backward and rippling. Its vertices go to `batch`,
/// its triangles to `indices`.
pub(crate) fn distort(
    model: &crate::viewer::EffectModel,
    (start_ms, pull, path): (f32, bool, &[(f32, [f32; 3])]),
    now: f32,
    view: &View,
    batch: &mut Batch,
    indices: &mut Vec<u32>,
    to_bevy: impl Fn(Vec3) -> Vec3,
) {
    let left = start_ms + 500.0 - now;
    let grown = if pull {
        (500.0 - left) * 0.003
    } else {
        left * 0.003
    };
    let scale = grown.clamp(0.2, 1.0);
    let alpha = (left * 0.488).clamp(10.0, 244.0).trunc() / 255.0;

    let Some(&(_, origin)) = path.iter().rfind(|(ms, _)| *ms <= now).or(path.first()) else {
        return;
    };
    let origin = Vec3::from_array(origin);
    let away = origin - view.eye;
    let distance = away.length();
    if distance <= 0.1 {
        return;
    }
    // Facing away from the eye, rolled half a turn.
    let forward = away / distance;
    let yaw = forward.y.atan2(forward.x);
    let flat_left = Vec3::new(-yaw.sin(), yaw.cos(), 0.0);
    let pitch_up = forward.cross(flat_left).normalize_or(Vec3::Z);
    let axes = [forward * scale, -flat_left * scale, -pitch_up * scale];

    // Where the hand shows on the screen. JA's renderer takes the view's
    // left for its right, so its screen comes out mirrored; the corner is
    // mirrored back.
    let x = away.dot(view.left);
    let y = away.dot(view.up);
    let z = away.dot(view.forward);
    if z < 0.01 {
        return;
    }
    let (width, height) = view.size;
    let (cx, cy) = (width / 2.0, height / 2.0);
    let sx = cx + cx / z * (90.0 / view.fov.0) * x;
    let sy = cy - cy / z * (90.0 / view.fov.1) * y;
    let square = if distance < 128.0 {
        256.0
    } else if distance < 256.0 {
        128.0
    } else if distance < 512.0 {
        64.0
    } else {
        32.0
    };
    let corner = |size: f32, at: f32| {
        (size - at.trunc() - square / 2.0).clamp(0.0, (size - square).max(0.0))
    };
    let (corner_x, corner_y) = (corner(width, sx), corner(height, sy));

    // The texture's turn and ripple, timed from the cast.
    let seconds = (now - start_ms) / 1000.0;
    let index = (-15.0 * seconds * (1024.0 / 360.0)) as i32;
    let sin = table_sin(index as f32 / 1024.0);
    let cos = table_sin(index.wrapping_add(256) as f32 / 1024.0);
    let wave = seconds * 2.0;

    let (positions, uvs, faces) = model;
    let base = batch.positions.len() as u32;
    for (p, &[s, t]) in positions.iter().zip(uvs) {
        let s1 = s * cos + t * -sin + (0.5 - 0.5 * cos + 0.5 * sin);
        let t1 = s * sin + t * cos + (0.5 - 0.5 * sin - 0.5 * cos);
        let s2 = s1 + table_sin((p[0] + p[2]) / 128.0 * 0.125 + wave) * 0.03;
        let t2 = t1 + table_sin(p[1] / 128.0 * 0.125 + wave) * 0.03;
        // On the screen's copy, which GL lays bottom-up.
        let u = (corner_x + s2 * square) / width;
        let v = 1.0 - (corner_y + t2 * square) / height;
        let world = origin + axes[0] * p[0] + axes[1] * p[1] + axes[2] * p[2];
        batch.positions.push(to_bevy(world).to_array());
        batch.uvs.push([u, v]);
        batch.colours.push([1.0, 1.0, 1.0, alpha]);
    }
    for face in faces {
        indices.extend(face.map(|i| i + base));
    }
}
