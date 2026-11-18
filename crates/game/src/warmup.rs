//! The render pipelines a scene draws with, compiled while it comes up
//! (behind the loading screen, or in a run's first frames) rather than when
//! each is first drawn. Bevy compiles a pipeline the first frame something
//! that needs it is in view, and on macOS it does so on the render thread
//! (its asynchronous path is off there): tens of milliseconds each on a
//! cold shader cache, a hitch when a saber, an effect or a room first shows
//! mid-play.
//!
//! A mesh's pipeline follows from its material (an Odyssey one's surface),
//! its vertex layout and the view. So when a scene is spawned, every
//! surface is drawn once with every vertex layout the scene's meshes use
//! (and the distortion's and the grass's materials with their meshes'
//! layouts), by the scene's cameras: stand-ins on the scene's own meshes,
//! scaled to nothing so they cover no pixel. Every shell a character's
//! meshes can take (`glow_shell`: a Force power's glow, a mark, a
//! distortion, a hologram) is drawn over the same layouts. The screens'
//! camera (`ui_view`) draws the screens' material both blended and added.
//! Without that, the first HUD element drawn added would compile its
//! pipeline in the middle of play.
//!
//! The layouts drawn are the scene's, those a warm-up remembered (below)
//! and those a model's parts are made with (`MODEL_LAYOUTS`). A mesh made
//! for the render world only has no data left in the main world once it is
//! drawn, so its layout is noted when it is added (`Shapes`).
//!
//! The stand-ins go once the render world has compiled all it was given. A
//! pipeline compiled after that is logged (a warning, `pipelines`, with its
//! label and its key) and counted in the dumps' facts (`late_pipelines`,
//! which the bench also reports), to find what the warm-up misses.
//!
//! The other overlays (the conversation, the loading screen) draw on their
//! own layers with their own cameras and are not warmed: they draw from a
//! session's first frame.
//!
//! The layouts a warm-up drew, and those of meshes spawned after it
//! (`learn`), are remembered in the store. A run whose session loads behind
//! its loading screen (`kjk play`) draws those first, while it loads
//! (`prewarm`), through a camera made as the scene's will be: the scene's
//! own warm-up then finds them compiled.

use std::sync::Arc;
use std::sync::atomic::{AtomicU8, AtomicU64, AtomicUsize, Ordering};
use std::time::Instant;

use bevy::asset::RenderAssetUsages;
use bevy::camera::RenderTarget;
use bevy::camera::visibility::{NoFrustumCulling, RenderLayers};
use bevy::ecs::query::QueryFilter;
use bevy::ecs::system::SystemParam;
use bevy::mesh::skinning::{SkinnedMesh, SkinnedMeshInverseBindposes};
use bevy::mesh::{
    Indices, MeshVertexAttribute, MeshVertexAttributeId, PrimitiveTopology, VertexAttributeValues,
    VertexFormat,
};
use bevy::platform::collections::{HashMap, HashSet};
use bevy::prelude::*;
use bevy::render::render_resource::{CachedPipelineState, PipelineCache, PipelineDescriptor};
use bevy::render::{Render, RenderApp, RenderSystems};

use crate::glow_shell::GlowMaterial;
use crate::grass::{GrassMaterial, GrassParams};
use crate::material::{self, OdysseyMaterial, Surface};
use crate::scene_fx::RefractionMaterial;
use crate::ui_material::{UiMaterial, UiParams};

pub struct WarmupPlugin;

impl Plugin for WarmupPlugin {
    fn build(&self, app: &mut App) {
        // Nothing to warm without a renderer.
        let Some(render) = app.get_sub_app_mut(RenderApp) else {
            return;
        };
        let compiled = Compiled::default();
        render
            .insert_resource(compiled.clone())
            .add_systems(Render, watch.in_set(RenderSystems::Cleanup));
        app.insert_resource(compiled)
            .add_systems(Update, finish.run_if(resource_exists::<Warmup>))
            .init_resource::<Shapes>()
            .add_systems(PostUpdate, learn)
            .add_systems(Last, note_shapes)
            .add_systems(Last, facts);
    }
}

/// The log's message for a pipeline compiled after a warm-up, which the
/// bench looks for.
pub const LATE_PIPELINE: &str = "a pipeline compiled after the warm-up";

/// `Counts::stage`, after 0 (no warm-up yet): one under way, one done.
const WARMING: u8 = 1;
const WARM: u8 = 2;

/// What the render world has compiled, shared with the main world.
#[derive(Resource, Clone, Default)]
pub(crate) struct Compiled(Arc<Counts>);

#[derive(Default)]
struct Counts {
    /// Render frames seen.
    frames: AtomicU64,
    /// Pipelines compiled so far.
    ready: AtomicUsize,
    /// Render frames in a row that compiled nothing and left nothing
    /// waiting.
    quiet: AtomicU64,
    stage: AtomicU8,
    /// Pipelines compiled after a warm-up finished.
    late: AtomicU64,
}

impl Compiled {
    /// How many render frames the render world has drawn.
    pub(crate) fn frames(&self) -> u64 {
        self.0.frames.load(Ordering::Relaxed)
    }

    /// Whether the render world has compiled every pipeline it was given:
    /// the last two render frames compiled nothing and left nothing
    /// waiting.
    pub(crate) fn quiet(&self) -> bool {
        self.0.quiet.load(Ordering::Relaxed) >= 2
    }
}

/// The stand-ins being drawn, and the render frame and compiled count when
/// they were spawned.
#[derive(Resource)]
pub(crate) struct Warmup {
    stand_ins: Vec<Entity>,
    started: Instant,
    frame: u64,
    ready: usize,
    /// The phase it is for the launch's times, and whether it is the
    /// scene's own (the pre-warm's is not).
    phase: &'static str,
    scene: bool,
}

impl Warmup {
    fn begin(
        compiled: &Compiled,
        stand_ins: Vec<Entity>,
        phase: &'static str,
        scene: bool,
    ) -> Warmup {
        let counts = &compiled.0;
        counts.stage.store(WARMING, Ordering::Relaxed);
        Warmup {
            stand_ins,
            started: Instant::now(),
            frame: counts.frames.load(Ordering::Relaxed),
            ready: counts.ready.load(Ordering::Relaxed),
            phase,
            scene,
        }
    }
}

/// A mesh entity's mesh, skin and layers.
type MeshData = (
    &'static Mesh3d,
    Option<&'static SkinnedMesh>,
    Option<&'static RenderLayers>,
);

/// The mesh entities drawn with `M`.
type Drawn<'w, 's, M> = Query<'w, 's, MeshData, With<MeshMaterial3d<M>>>;

/// The mesh entities drawn with `M` spawned since the system last ran.
type Spawned<'w, 's, M> = Query<'w, 's, MeshData, (With<MeshMaterial3d<M>>, Added<Mesh3d>)>;

/// A mesh and its skin, standing for every mesh of its vertex layout.
type Sample = (Handle<Mesh>, Option<SkinnedMesh>);

/// The materials a scene's meshes are warmed with, by the name a remembered
/// layout gives each; `Samples` holds a list for each, in this order.
const MATERIALS: [&str; 3] = ["odyssey", "refraction", "grass"];

/// Samples of the layouts drawn with each of `MATERIALS`.
#[derive(Default)]
struct Samples([Vec<Sample>; 3]);

impl Samples {
    /// Every sample with its material's name.
    fn named(&self) -> impl Iterator<Item = (&'static str, &Sample)> {
        MATERIALS
            .into_iter()
            .zip(&self.0)
            .flat_map(|(material, samples)| samples.iter().map(move |sample| (material, sample)))
    }
}

/// The assets the stand-ins' materials go into.
#[derive(SystemParam)]
pub(crate) struct WarmMaterials<'w> {
    odyssey: ResMut<'w, Assets<OdysseyMaterial>>,
    refraction: ResMut<'w, Assets<RefractionMaterial>>,
    grass: ResMut<'w, Assets<GrassMaterial>>,
    glow: ResMut<'w, Assets<GlowMaterial>>,
    /// None in a run without the game's screens (`kjk view`, `kjk module`).
    ui: Option<ResMut<'w, Assets<UiMaterial>>>,
}

impl WarmMaterials<'_> {
    /// A stand-in for each sample with each material of its kind: every
    /// Odyssey surface and every shell over it (`glow_shell`), the
    /// distortion and the grass (one pipeline each). The screens' material
    /// is drawn both blended and added on the screens' layer, by the
    /// screens' camera (`ui_view`).
    fn stand_ins(
        &mut self,
        commands: &mut Commands,
        samples: &Samples,
        meshes: &mut Assets<Mesh>,
    ) -> Vec<Entity> {
        let [odyssey, refracting, grassy] = &samples.0;
        let surfaces = surface_materials(&mut self.odyssey);
        let shells: Vec<Handle<GlowMaterial>> = (GlowMaterial::stand_ins().into_iter())
            .map(|shell| self.glow.add(shell))
            .collect();
        let refraction = self.refraction.add(RefractionMaterial::default());
        // The patches' and their motion twins' (`grass::GrassMotion`).
        let grass = [false, true].map(|motion| {
            self.grass.add(GrassMaterial {
                params: GrassParams::default(),
                art: None,
                lightmap: None,
                clock: crate::grass::CLOCK,
                motion,
            })
        });
        let mut spawned = stand_ins(commands, odyssey, &surfaces);
        spawned.extend(stand_ins(commands, odyssey, &shells));
        spawned.extend(stand_ins(commands, refracting, &[refraction]));
        spawned.extend(stand_ins(commands, grassy, &grass));
        let Some(ui) = self.ui.as_mut() else {
            return spawned;
        };
        let screens = [false, true].map(|additive| {
            ui.add(UiMaterial {
                params: UiParams::default(),
                texture: None,
                additive,
            })
        });
        let quad = (meshes.add(screens_mesh()), None);
        let layer = RenderLayers::layer(crate::ui_view::LAYER);
        for entity in stand_ins(commands, std::slice::from_ref(&quad), &screens) {
            commands.entity(entity).insert(layer.clone());
            spawned.push(entity);
        }
        spawned
    }
}

/// A triangle with the screens' vertex layout (`ui_view::run_mesh`).
fn screens_mesh() -> Mesh {
    Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::RENDER_WORLD,
    )
    .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, vec![[0.0f32; 3]; 3])
    .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, vec![[0.0f32; 2]; 3])
    .with_inserted_attribute(Mesh::ATTRIBUTE_COLOR, vec![[1.0f32; 4]; 3])
    .with_inserted_indices(Indices::U32(vec![0, 1, 2]))
}

/// A mesh's vertex layout: its attributes, their formats and its topology
/// (what a mesh brings to its pipeline).
type Shape = (
    Vec<(MeshVertexAttributeId, VertexFormat)>,
    PrimitiveTopology,
);

fn shape(data: &Mesh) -> Option<Shape> {
    let attributes = data.try_attributes().ok()?;
    Some((
        attributes
            .map(|(attribute, _)| (attribute.id, attribute.format))
            .collect(),
        data.primitive_topology(),
    ))
}

/// Each mesh's layout, noted when the mesh is added or changed, while its
/// data can still be read. A mesh made for the render world only
/// (`RenderAssetUsages::RENDER_WORLD`) has no data in the main world once
/// it is drawn, so a warm-up after that could not tell its layout.
#[derive(Resource, Default)]
pub(crate) struct Shapes(HashMap<AssetId<Mesh>, Shape>);

/// Notes the layout of each mesh added or changed this frame, and forgets
/// the meshes that are gone. It runs after the asset events are sent and
/// before the frame's meshes go to the render world.
fn note_shapes(
    mut events: MessageReader<AssetEvent<Mesh>>,
    meshes: Res<Assets<Mesh>>,
    mut shapes: ResMut<Shapes>,
) {
    for event in events.read() {
        match *event {
            AssetEvent::Added { id } | AssetEvent::Modified { id } => {
                // A mesh kept in the main world can be read when needed.
                let Some(data) = meshes.get(id) else {
                    continue;
                };
                if data.asset_usage.contains(RenderAssetUsages::MAIN_WORLD)
                    || shapes.0.contains_key(&id)
                {
                    continue;
                }
                if let Some(shape) = shape(data) {
                    shapes.0.insert(id, shape);
                }
            }
            AssetEvent::Removed { id } | AssetEvent::Unused { id } => {
                shapes.0.remove(&id);
            }
            AssetEvent::LoadedWithDependencies { .. } => {}
        }
    }
}

/// A mesh's layout, read from its data. A mesh whose data has gone to the
/// render world falls back to the layout noted in `Shapes`.
fn shape_of(meshes: &Assets<Mesh>, shapes: &Shapes, mesh: &Handle<Mesh>) -> Option<Shape> {
    (meshes.get(mesh).and_then(shape)).or_else(|| shapes.0.get(&mesh.id()).cloned())
}

/// A sample of each vertex layout among `drawn` on the scene's layers.
fn layouts<F: QueryFilter>(
    (meshes, shapes): (&Assets<Mesh>, &Shapes),
    drawn: &Query<MeshData, F>,
) -> Vec<Sample> {
    let scene = RenderLayers::default();
    let mut seen = Vec::new();
    let mut samples = Vec::new();
    for (mesh, skin, layers) in drawn {
        if layers.is_some_and(|layers| !layers.intersects(&scene)) {
            continue;
        }
        let Some(layout) = shape_of(meshes, shapes, &mesh.0) else {
            continue;
        };
        if !seen.contains(&layout) {
            seen.push(layout);
            samples.push((mesh.0.clone(), skin.cloned()));
        }
    }
    samples
}

/// A stand-in per material and layout: the layout's sample shrunk to a
/// point.
fn stand_ins<M: Material>(
    commands: &mut Commands,
    samples: &[Sample],
    materials: &[Handle<M>],
) -> Vec<Entity> {
    let mut spawned = Vec::new();
    for (mesh, skin) in samples {
        for material in materials {
            let mut stand_in = commands.spawn((
                Mesh3d(mesh.clone()),
                MeshMaterial3d(material.clone()),
                Transform::from_scale(Vec3::ZERO),
                NoFrustumCulling,
            ));
            // Every joint is the stand-in itself, so a skinned one shrinks
            // to the point too.
            if let Some(skin) = skin {
                let entity = stand_in.id();
                stand_in.insert(SkinnedMesh {
                    inverse_bindposes: skin.inverse_bindposes.clone(),
                    joints: vec![entity; skin.joints.len()],
                });
            }
            spawned.push(stand_in.id());
        }
    }
    spawned
}

/// Only a material's surface, culling, whether it reads a list, whether its
/// list holds particles and whether it discards alpha 0 reach its pipeline.
/// It makes one material for every surface with every combination of the
/// rest, and one for each pipeline that differs only in its alpha mode.
fn surface_materials(materials: &mut Assets<OdysseyMaterial>) -> Vec<Handle<OdysseyMaterial>> {
    let plain = crate::lifecycle_view::unlit(Vec4::ZERO, None);
    let lists = [&material::NO_PARTICLES, &material::STAND_IN_PARTICLES];
    let mut made = Vec::new();
    for &surface in &Surface::ALL {
        for culled in [false, true] {
            for particles in lists {
                for cut in [false, true] {
                    made.push(OdysseyMaterial {
                        culled,
                        particles: particles.clone(),
                        ..plain.clone().as_surface(surface).cutting(cut)
                    });
                }
            }
        }
    }
    made.extend(
        [Surface::AddOne, Surface::BlendVertex, Surface::Modulate].map(|surface| OdysseyMaterial {
            particles: material::STAND_IN_PARTICLES,
            listed_quads: true,
            ..plain.clone().as_surface(surface)
        }),
    );
    // Surfaces that agree on the key and the alpha mode share a pipeline.
    let mut kept: Vec<(material::OdysseyKey, String)> = Vec::new();
    made.retain(|made| {
        let pipeline = (
            material::OdysseyKey::from(made),
            format!("{:?}", made.alpha_mode()),
        );
        let new = !kept.contains(&pipeline);
        if new {
            kept.push(pipeline);
        }
        new
    });
    made.into_iter().map(|made| materials.add(made)).collect()
}

/// Draws every surface with every vertex layout the scene's meshes use, a
/// warm-up remembered or a model's parts are made with, and the distortion
/// and the grass with their meshes' layouts. It runs once a scene is
/// spawned. A mesh spawned later is not there yet. The remembered layouts
/// cover it once a run has seen it (`learn`).
#[allow(clippy::too_many_arguments)]
pub(crate) fn start(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut bindposes: ResMut<Assets<SkinnedMeshInverseBindposes>>,
    mut materials: WarmMaterials,
    odyssey: Drawn<OdysseyMaterial>,
    refracting: Drawn<RefractionMaterial>,
    grassy: Drawn<GrassMaterial>,
    previous: Option<Res<Warmup>>,
    compiled: Option<Res<Compiled>>,
    shapes: Option<Res<Shapes>>,
) {
    let (Some(compiled), Some(shapes)) = (compiled, shapes) else {
        return;
    };
    if let Some(previous) = previous {
        for &entity in &previous.stand_ins {
            commands.entity(entity).despawn();
        }
    }
    let read = (&*meshes, &*shapes);
    let mut samples = Samples([
        layouts(read, &odyssey),
        layouts(read, &refracting),
        layouts(read, &grassy),
    ]);
    let lines = lines(read, &samples);
    let remembered = with_models(remember(&lines));
    for line in remembered.iter().filter(|line| !lines.contains(line)) {
        if let Some((material, sample)) = made_layout(line, &mut meshes, &mut bindposes) {
            samples.0[material].push(sample);
        }
    }
    let spawned = materials.stand_ins(&mut commands, &samples, &mut meshes);
    commands.insert_resource(Warmup::begin(&compiled, spawned, "pipeline warm-up", true));
}

/// The stand-ins go once the render world has drawn them and gone quiet:
/// a few frames on, two in a row that compiled nothing and left nothing
/// waiting (a material or shader not ready yet holds its pipeline back a
/// frame or more).
fn finish(mut commands: Commands, warmup: Res<Warmup>, compiled: Res<Compiled>) {
    let counts = &compiled.0;
    let frames = counts.frames.load(Ordering::Relaxed);
    if frames < warmup.frame + 4 || counts.quiet.load(Ordering::Relaxed) < 2 {
        return;
    }
    for &entity in &warmup.stand_ins {
        commands.entity(entity).despawn();
    }
    commands.remove_resource::<Warmup>();
    counts.stage.store(WARM, Ordering::Relaxed);
    crate::launch_times::record(
        warmup.phase,
        warmup.started.elapsed().as_secs_f64() * 1000.0,
    );
    if warmup.scene {
        crate::launch_times::mark("pipelines warmed");
    }
    info!(
        target: "pipelines",
        "{}: {} pipelines in {} ms ({} stand-ins)",
        warmup.phase,
        counts.ready.load(Ordering::Relaxed) - warmup.ready,
        warmup.started.elapsed().as_millis(),
        warmup.stand_ins.len()
    );
}

/// Seconds with nothing compiled after which a pipeline still compiling
/// counts as stuck.
const STUCK_COMPILING: f32 = 10.0;
/// The same for a pipeline waiting for its shader to load, which Bevy
/// tries again every frame for ever when the shader never loads.
const STUCK_UNLOADED: f32 = 2.0;

/// Since when nothing was compiled while pipelines waited (none: some were
/// compiled, or none waited), and the pipelines found stuck (by their
/// place in the cache), which no longer count as waiting.
#[derive(Default)]
struct Stall {
    since: Option<Instant>,
    stuck: HashSet<usize>,
}

/// Counts what the pipeline cache has compiled, for the main world, and
/// logs a pipeline compiled after a warm-up. A frame is quiet when nothing
/// was compiled and nothing waits but stuck pipelines: those still
/// compiling after `STUCK_COMPILING` seconds with nothing compiled, or
/// waiting for a shader after `STUCK_UNLOADED` seconds. A stuck pipeline
/// is logged once and no longer counts until it compiles, so a picture
/// waiting on it (`lifecycle_view::PictureWait`) is not held up for the
/// whole of its longest wait.
fn watch(
    cache: Res<PipelineCache>,
    compiled: Res<Compiled>,
    mut ready: Local<Vec<bool>>,
    mut stall: Local<Stall>,
) {
    let counts = &compiled.0;
    let warm = counts.stage.load(Ordering::Relaxed) == WARM;
    let stall = &mut *stall;
    let waiting: Vec<usize> = (cache.waiting_pipelines())
        .filter(|n| !stall.stuck.contains(n))
        .collect();
    let (mut compiling, mut unloaded) = (false, false);
    let mut new = 0;
    for (n, pipeline) in cache.pipelines().enumerate() {
        if waiting.contains(&n) {
            match pipeline.state {
                CachedPipelineState::Creating(_) => compiling = true,
                CachedPipelineState::Ok(_) => {}
                _ => unloaded = true,
            }
        }
        if ready.len() <= n {
            ready.push(false);
        }
        if ready[n] || !matches!(pipeline.state, CachedPipelineState::Ok(_)) {
            continue;
        }
        ready[n] = true;
        new += 1;
        stall.stuck.remove(&n);
        if warm {
            counts.late.fetch_add(1, Ordering::Relaxed);
            warn!(
                target: "pipelines",
                label = label(pipeline),
                key = key(pipeline),
                "{LATE_PIPELINE}"
            );
        }
    }
    if new > 0 || waiting.is_empty() {
        stall.since = None;
    }
    let stalled = stall
        .since
        .get_or_insert_with(Instant::now)
        .elapsed()
        .as_secs_f32();
    let busy = (compiling && stalled < STUCK_COMPILING) || (unloaded && stalled < STUCK_UNLOADED);
    if !waiting.is_empty() && !busy {
        let stuck: Vec<String> = (cache.pipelines().enumerate())
            .filter(|(n, _)| waiting.contains(n))
            .map(|(_, pipeline)| label(pipeline))
            .collect();
        warn!(
            target: "pipelines",
            ?stuck,
            "pipelines stuck, nothing compiled for {stalled:.1} s: no picture waits for them"
        );
        stall.stuck.extend(waiting);
        stall.since = None;
    }
    counts.frames.fetch_add(1, Ordering::Relaxed);
    counts.ready.fetch_add(new, Ordering::Relaxed);
    if new == 0 && !busy {
        counts.quiet.fetch_add(1, Ordering::Relaxed);
    } else {
        counts.quiet.store(0, Ordering::Relaxed);
    }
}

/// A pipeline's label, for the log.
fn label(pipeline: &bevy::render::render_resource::CachedPipeline) -> String {
    let label = match &pipeline.descriptor {
        PipelineDescriptor::RenderPipelineDescriptor(descriptor) => &descriptor.label,
        PipelineDescriptor::ComputePipelineDescriptor(descriptor) => &descriptor.label,
    };
    label.as_deref().unwrap_or("(no label)").to_string()
}

/// What a pipeline was specialized for, for the log. It names the shaders
/// and their definitions. For a render pipeline it also names the vertex
/// layouts, the topology, the culled face, the sample count, the depth
/// format and test, and each target's format and blend.
fn key(pipeline: &bevy::render::render_resource::CachedPipeline) -> String {
    let shader = |shader: &Handle<bevy::shader::Shader>| {
        shader
            .path()
            .map_or_else(|| format!("{:?}", shader.id()), ToString::to_string)
    };
    let defs = |defs: &[bevy::shader::ShaderDefVal]| {
        defs.iter()
            .map(|def| match def {
                bevy::shader::ShaderDefVal::Bool(name, true) => name.clone(),
                bevy::shader::ShaderDefVal::Bool(name, false) => format!("{name}=false"),
                bevy::shader::ShaderDefVal::Int(name, value) => format!("{name}={value}"),
                bevy::shader::ShaderDefVal::UInt(name, value) => format!("{name}={value}"),
            })
            .collect::<Vec<_>>()
            .join(" ")
    };
    match &pipeline.descriptor {
        PipelineDescriptor::RenderPipelineDescriptor(descriptor) => {
            let vertex = &descriptor.vertex;
            let layouts: Vec<Vec<String>> = (vertex.buffers.iter())
                .map(|buffer| {
                    (buffer.attributes.iter())
                        .map(|attribute| {
                            format!("{}:{:?}", attribute.shader_location, attribute.format)
                        })
                        .collect()
                })
                .collect();
            let fragment = descriptor.fragment.as_ref().map(|fragment| {
                let targets: Vec<String> = (fragment.targets.iter())
                    .map(|target| {
                        target.as_ref().map_or("none".to_string(), |target| {
                            format!("{:?} blend {:?}", target.format, target.blend)
                        })
                    })
                    .collect();
                format!(
                    "fragment {} [{}] targets {targets:?}",
                    shader(&fragment.shader),
                    defs(&fragment.shader_defs)
                )
            });
            format!(
                "vertex {} [{}] layouts {layouts:?}; {:?}; cull {:?}; {} samples; depth {:?}; {}",
                shader(&vertex.shader),
                defs(&vertex.shader_defs),
                descriptor.primitive.topology,
                descriptor.primitive.cull_mode,
                descriptor.multisample.count,
                (descriptor.depth_stencil.as_ref()).map(|depth| (
                    depth.format,
                    depth.depth_write_enabled,
                    depth.depth_compare
                )),
                fragment.unwrap_or("no fragment".to_string()),
            )
        }
        PipelineDescriptor::ComputePipelineDescriptor(descriptor) => format!(
            "compute {} [{}]",
            shader(&descriptor.shader),
            defs(&descriptor.shader_defs)
        ),
    }
}

/// The pipelines compiled after a warm-up so far, in the dumps' facts.
fn facts(compiled: Res<Compiled>, mut facts: ResMut<console::DumpFacts>) {
    if !facts.wanted() {
        return;
    }
    facts.0.insert(
        "late_pipelines".into(),
        serde_json::json!(compiled.0.late.load(Ordering::Relaxed)),
    );
}

/// The vertex attributes a remembered layout can name: those the scene's
/// meshes are made of.
const ATTRIBUTES: [(&str, MeshVertexAttribute); 10] = [
    ("position", Mesh::ATTRIBUTE_POSITION),
    ("normal", Mesh::ATTRIBUTE_NORMAL),
    ("uv0", Mesh::ATTRIBUTE_UV_0),
    ("uv1", Mesh::ATTRIBUTE_UV_1),
    ("colour", Mesh::ATTRIBUTE_COLOR),
    ("joints", Mesh::ATTRIBUTE_JOINT_INDEX),
    ("weights", Mesh::ATTRIBUTE_JOINT_WEIGHT),
    ("sway", crate::grass::SWAY),
    ("base", crate::grass::BASE),
    ("dangly", crate::dangly::SLOT),
];

const TOPOLOGIES: [(&str, PrimitiveTopology); 5] = [
    ("triangles", PrimitiveTopology::TriangleList),
    ("strip", PrimitiveTopology::TriangleStrip),
    ("lines", PrimitiveTopology::LineList),
    ("line-strip", PrimitiveTopology::LineStrip),
    ("points", PrimitiveTopology::PointList),
];

const LAYOUTS: &str = "warmup/layouts";

fn layouts_key() -> store::Key {
    store::Key::of("warmup-layouts-v2", &[])
}

/// A sample's layout as a line: its material (one of `MATERIALS`),
/// topology, whether it is skinned and its attributes; `None` when it has
/// one a warm-up cannot make again.
fn layout_line(
    (meshes, shapes): (&Assets<Mesh>, &Shapes),
    material: &str,
    (mesh, skin): &Sample,
) -> Option<String> {
    let (attributes, topology) = shape_of(meshes, shapes, mesh)?;
    let topology = TOPOLOGIES.iter().find(|(_, known)| *known == topology)?.0;
    let names = attributes
        .iter()
        .map(|(id, _)| {
            ATTRIBUTES
                .iter()
                .find(|(_, known)| known.id == *id)
                .map(|(name, _)| *name)
        })
        .collect::<Option<Vec<_>>>()?;
    let skinned = if skin.is_some() { "skinned" } else { "rigid" };
    Some(format!(
        "{material} {topology} {skinned} {}",
        names.join(",")
    ))
}

/// The samples' layouts as lines (`layout_line`).
fn lines(read: (&Assets<Mesh>, &Shapes), samples: &Samples) -> Vec<String> {
    samples
        .named()
        .filter_map(|(material, sample)| layout_line(read, material, sample))
        .collect()
}

/// The layouts a model's parts are made with (`scene::odyssey_mesh`): a
/// position and a normal, with or without either set of UVs, and a skinned
/// body's. A model first shown in the middle of play (an effect, a thrown
/// item) is drawn with one of them, so every warm-up draws them all, even
/// on a machine that has remembered none yet.
const MODEL_LAYOUTS: [&str; 5] = [
    "odyssey triangles rigid position,normal",
    "odyssey triangles rigid position,normal,uv0",
    "odyssey triangles rigid position,normal,uv1",
    "odyssey triangles rigid position,normal,uv0,uv1",
    "odyssey triangles skinned position,normal,uv0,weights,joints",
];

/// The layouts remembered.
fn remembered(store: &store::Store) -> Vec<String> {
    store
        .get(LAYOUTS, &layouts_key())
        .and_then(|bytes| String::from_utf8(bytes).ok())
        .map(|text| text.lines().map(str::to_string).collect())
        .unwrap_or_else(Vec::new)
}

/// `remembered` and the models' layouts (`MODEL_LAYOUTS`), each once.
fn with_models(mut lines: Vec<String>) -> Vec<String> {
    for line in MODEL_LAYOUTS {
        if !lines.iter().any(|known| known == line) {
            lines.push(line.to_string());
        }
    }
    lines
}

/// Adds `new` to the layouts remembered in the store. Returns every
/// remembered layout, or none when the store cannot be opened.
fn remember(new: &[String]) -> Vec<String> {
    let Some(store) = store::Store::open() else {
        return Vec::new();
    };
    let mut lines = remembered(&store);
    let before = lines.len();
    for line in new {
        if !lines.contains(line) {
            lines.push(line.clone());
        }
    }
    if lines.len() > before {
        let _ = store.put(LAYOUTS, &layouts_key(), lines.join("\n").as_bytes());
    }
    lines
}

/// Remembers the layouts of meshes spawned after a scene's warm-up. A
/// run's later warm-ups and pre-warms then draw them. Each layout is looked
/// up in the store once a run.
fn learn(
    meshes: Res<Assets<Mesh>>,
    shapes: Res<Shapes>,
    odyssey: Spawned<OdysseyMaterial>,
    refracting: Spawned<RefractionMaterial>,
    grassy: Spawned<GrassMaterial>,
    mut known: Local<HashSet<String>>,
) {
    if odyssey.is_empty() && refracting.is_empty() && grassy.is_empty() {
        return;
    }
    let read = (&*meshes, &*shapes);
    let samples = Samples([
        layouts(read, &odyssey),
        layouts(read, &refracting),
        layouts(read, &grassy),
    ]);
    let new: Vec<String> = (lines(read, &samples).into_iter())
        .filter(|line| known.insert(line.clone()))
        .collect();
    if !new.is_empty() {
        remember(&new);
    }
}

/// A mesh of one triangle with a remembered layout's attributes, and its
/// skin when it is skinned, with its material's place in `MATERIALS`;
/// `None` for a line that does not read.
fn made_layout(
    line: &str,
    meshes: &mut Assets<Mesh>,
    bindposes: &mut Assets<SkinnedMeshInverseBindposes>,
) -> Option<(usize, Sample)> {
    let mut words = line.split(' ');
    let material = words.next()?;
    let material = MATERIALS.iter().position(|known| *known == material)?;
    let topology = words.next()?;
    let topology = TOPOLOGIES.iter().find(|(name, _)| *name == topology)?.1;
    let skinned = words.next()? == "skinned";
    let mut mesh = Mesh::new(topology, RenderAssetUsages::RENDER_WORLD);
    for name in words.next()?.split(',') {
        let attribute = ATTRIBUTES.iter().find(|(known, _)| *known == name)?.1;
        let values = match name {
            "uv0" | "uv1" => VertexAttributeValues::Float32x2(vec![[0.0; 2]; 3]),
            "colour" | "weights" => VertexAttributeValues::Float32x4(vec![[1.0, 0.0, 0.0, 0.0]; 3]),
            "joints" => VertexAttributeValues::Uint16x4(vec![[0; 4]; 3]),
            "sway" => VertexAttributeValues::Float32(vec![0.0; 3]),
            "dangly" => VertexAttributeValues::Uint32(vec![0; 3]),
            _ => VertexAttributeValues::Float32x3(vec![[0.0; 3]; 3]),
        };
        mesh.insert_attribute(attribute, values);
    }
    mesh.insert_indices(Indices::U32(vec![0, 1, 2]));
    let skin = skinned.then(|| SkinnedMesh {
        inverse_bindposes: bindposes.add(SkinnedMeshInverseBindposes::from(vec![Mat4::IDENTITY])),
        joints: vec![Entity::PLACEHOLDER],
    });
    Some((material, (meshes.add(mesh), skin)))
}

/// While a session loads behind the loading screen: every surface (and the
/// distortion and the grass) drawn with every layout remembered for it, by
/// a camera made as the scene's (its view is part of each pipeline), into
/// what the screen's camera draws into, before it.
pub(crate) fn prewarm(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut bindposes: ResMut<Assets<SkinnedMeshInverseBindposes>>,
    mut materials: WarmMaterials,
    screen: Query<Option<&RenderTarget>, With<Camera2d>>,
    compiled: Option<Res<Compiled>>,
) {
    let Some(compiled) = compiled else {
        return;
    };
    let lines = with_models(store::Store::open().map_or_else(Vec::new, |store| remembered(&store)));
    let mut samples = Samples::default();
    for (material, sample) in lines
        .iter()
        .filter_map(|line| made_layout(line, &mut meshes, &mut bindposes))
    {
        samples.0[material].push(sample);
    }
    let mut spawned = materials.stand_ins(&mut commands, &samples, &mut meshes);
    let mut camera = commands.spawn((
        crate::scene::camera_view(),
        Camera {
            order: -1,
            ..default()
        },
    ));
    if let Some(target) = screen.iter().flatten().next() {
        camera.insert(target.clone());
    }
    spawned.push(camera.id());
    commands.insert_resource(Warmup::begin(
        &compiled,
        spawned,
        "pipeline pre-warm (loading)",
        false,
    ));
}
