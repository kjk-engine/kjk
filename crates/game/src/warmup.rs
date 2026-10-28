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
//! layouts), by the scene's
//! cameras: stand-ins on the scene's own meshes, scaled to nothing so they
//! cover no pixel. They go once the render world has compiled all it was
//! given. A pipeline compiled after that is logged (debug, `pipelines`), to
//! find what the warm-up misses.
//!
//! The overlays (HUD, conversation, loading screen, the game's screens)
//! draw on their own layers with their own cameras and are not warmed:
//! they draw from a session's first frame.
//!
//! The layouts a warm-up drew are remembered in the store, and a run whose
//! session loads behind its loading screen (`kjk play`) draws those first,
//! while it loads (`prewarm`), through a camera made as the scene's will
//! be: the scene's own warm-up then finds them compiled.

use std::sync::Arc;
use std::sync::atomic::{AtomicU8, AtomicU64, AtomicUsize, Ordering};
use std::time::Instant;

use bevy::asset::RenderAssetUsages;
use bevy::camera::RenderTarget;
use bevy::camera::visibility::{NoFrustumCulling, RenderLayers};
use bevy::ecs::system::SystemParam;
use bevy::mesh::skinning::{SkinnedMesh, SkinnedMeshInverseBindposes};
use bevy::mesh::{Indices, MeshVertexAttribute, PrimitiveTopology, VertexAttributeValues};
use bevy::prelude::*;
use bevy::render::render_resource::{CachedPipelineState, PipelineCache, PipelineDescriptor};
use bevy::render::{Render, RenderApp, RenderSystems};

use crate::grass::{GrassMaterial, GrassParams};
use crate::material::{OdysseyMaterial, Surface};
use crate::scene_fx::RefractionMaterial;

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
            .add_systems(Update, finish.run_if(resource_exists::<Warmup>));
    }
}

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

/// The mesh entities drawn with `M`: their meshes, skins and layers.
type Drawn<'w, 's, M> = Query<
    'w,
    's,
    (
        &'static Mesh3d,
        Option<&'static SkinnedMesh>,
        Option<&'static RenderLayers>,
    ),
    With<MeshMaterial3d<M>>,
>;

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
}

impl WarmMaterials<'_> {
    /// A stand-in for each sample with each material of its kind: every
    /// Odyssey surface, the distortion and the grass (one pipeline each).
    fn stand_ins(&mut self, commands: &mut Commands, samples: &Samples) -> Vec<Entity> {
        let [odyssey, refracting, grassy] = &samples.0;
        let surfaces = surface_materials(&mut self.odyssey);
        let refraction = self.refraction.add(RefractionMaterial::default());
        let grass = self.grass.add(GrassMaterial {
            params: GrassParams::default(),
            art: None,
            lightmap: None,
        });
        let mut spawned = stand_ins(commands, odyssey, &surfaces);
        spawned.extend(stand_ins(commands, refracting, &[refraction]));
        spawned.extend(stand_ins(commands, grassy, &[grass]));
        spawned
    }
}

/// A sample of each vertex layout among `drawn` on the scene's layers (what
/// a mesh brings to its pipeline: its attributes, their formats and its
/// topology). Meshes whose data already went to the render world (built in
/// an earlier frame) are passed over.
fn layouts<M: Material>(meshes: &Assets<Mesh>, drawn: &Drawn<M>) -> Vec<Sample> {
    let scene = RenderLayers::default();
    let mut seen = Vec::new();
    let mut samples = Vec::new();
    for (mesh, skin, layers) in drawn {
        if layers.is_some_and(|layers| !layers.intersects(&scene)) {
            continue;
        }
        let Some(data) = meshes.get(&mesh.0) else {
            continue;
        };
        let Ok(attributes) = data.try_attributes() else {
            continue;
        };
        let layout = (
            attributes
                .map(|(attribute, _)| (attribute.id, attribute.format))
                .collect::<Vec<_>>(),
            data.primitive_topology(),
        );
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

/// A material of each surface, culled and not: only a material's surface
/// and culling reach its pipeline.
fn surface_materials(materials: &mut Assets<OdysseyMaterial>) -> Vec<Handle<OdysseyMaterial>> {
    Surface::ALL
        .iter()
        .flat_map(|&surface| [false, true].map(|culled| (surface, culled)))
        .map(|(surface, culled)| {
            materials.add(OdysseyMaterial {
                surface,
                culled,
                ..crate::lifecycle_view::unlit(Vec4::ZERO, None)
            })
        })
        .collect()
}

/// Draws every surface with every vertex layout the scene's meshes use, and
/// the distortion and the grass with their meshes' layouts; run once a
/// scene is spawned.
#[allow(clippy::too_many_arguments)]
pub(crate) fn start(
    mut commands: Commands,
    meshes: Res<Assets<Mesh>>,
    mut materials: WarmMaterials,
    odyssey: Drawn<OdysseyMaterial>,
    refracting: Drawn<RefractionMaterial>,
    grassy: Drawn<GrassMaterial>,
    previous: Option<Res<Warmup>>,
    compiled: Option<Res<Compiled>>,
) {
    let Some(compiled) = compiled else {
        return;
    };
    if let Some(previous) = previous {
        for &entity in &previous.stand_ins {
            commands.entity(entity).despawn();
        }
    }
    let samples = Samples([
        layouts(&meshes, &odyssey),
        layouts(&meshes, &refracting),
        layouts(&meshes, &grassy),
    ]);
    remember(&meshes, &samples);
    let spawned = materials.stand_ins(&mut commands, &samples);
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

/// Counts what the pipeline cache has compiled, for the main world, and
/// logs a pipeline compiled after a warm-up.
fn watch(cache: Res<PipelineCache>, compiled: Res<Compiled>, mut ready: Local<Vec<bool>>) {
    let counts = &compiled.0;
    let warm = counts.stage.load(Ordering::Relaxed) == WARM;
    let mut new = 0;
    for (n, pipeline) in cache.pipelines().enumerate() {
        if ready.len() <= n {
            ready.push(false);
        }
        if ready[n] || !matches!(pipeline.state, CachedPipelineState::Ok(_)) {
            continue;
        }
        ready[n] = true;
        new += 1;
        if warm {
            let label = match &pipeline.descriptor {
                PipelineDescriptor::RenderPipelineDescriptor(descriptor) => &descriptor.label,
                PipelineDescriptor::ComputePipelineDescriptor(descriptor) => &descriptor.label,
            };
            debug!(target: "pipelines", ?label, "a pipeline compiled after the warm-up");
        }
    }
    let waiting = cache.waiting_pipelines().next().is_some();
    counts.frames.fetch_add(1, Ordering::Relaxed);
    counts.ready.fetch_add(new, Ordering::Relaxed);
    if new == 0 && !waiting {
        counts.quiet.fetch_add(1, Ordering::Relaxed);
    } else {
        counts.quiet.store(0, Ordering::Relaxed);
    }
}

/// The vertex attributes a remembered layout can name: those the scene's
/// meshes are made of.
const ATTRIBUTES: [(&str, MeshVertexAttribute); 9] = [
    ("position", Mesh::ATTRIBUTE_POSITION),
    ("normal", Mesh::ATTRIBUTE_NORMAL),
    ("uv0", Mesh::ATTRIBUTE_UV_0),
    ("uv1", Mesh::ATTRIBUTE_UV_1),
    ("colour", Mesh::ATTRIBUTE_COLOR),
    ("joints", Mesh::ATTRIBUTE_JOINT_INDEX),
    ("weights", Mesh::ATTRIBUTE_JOINT_WEIGHT),
    ("sway", crate::grass::SWAY),
    ("base", crate::grass::BASE),
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
    store::Key::of("warmup-layouts-v1", &[])
}

/// A sample's layout as a line: its material (one of `MATERIALS`),
/// topology, whether it is skinned and its attributes; `None` when it has
/// one a warm-up cannot make again.
fn layout_line(meshes: &Assets<Mesh>, material: &str, (mesh, skin): &Sample) -> Option<String> {
    let data = meshes.get(mesh)?;
    let topology = TOPOLOGIES
        .iter()
        .find(|(_, topology)| *topology == data.primitive_topology())?
        .0;
    let names = data
        .try_attributes()
        .ok()?
        .map(|(attribute, _)| {
            ATTRIBUTES
                .iter()
                .find(|(_, known)| known.id == attribute.id)
                .map(|(name, _)| *name)
        })
        .collect::<Option<Vec<_>>>()?;
    let skinned = if skin.is_some() { "skinned" } else { "rigid" };
    Some(format!(
        "{material} {topology} {skinned} {}",
        names.join(",")
    ))
}

/// Adds the scene's layouts to those remembered.
fn remember(meshes: &Assets<Mesh>, samples: &Samples) {
    let Some(store) = store::Store::open() else {
        return;
    };
    let key = layouts_key();
    let mut lines: Vec<String> = store
        .get(LAYOUTS, &key)
        .and_then(|bytes| String::from_utf8(bytes).ok())
        .map(|text| text.lines().map(str::to_string).collect())
        .unwrap_or_else(Vec::new);
    let before = lines.len();
    let new = samples
        .named()
        .filter_map(|(material, sample)| layout_line(meshes, material, sample));
    for line in new {
        if !lines.contains(&line) {
            lines.push(line);
        }
    }
    if lines.len() > before {
        let _ = store.put(LAYOUTS, &key, lines.join("\n").as_bytes());
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
    let Some(lines) = store::Store::open()
        .and_then(|store| store.get(LAYOUTS, &layouts_key()))
        .and_then(|bytes| String::from_utf8(bytes).ok())
    else {
        return;
    };
    let mut samples = Samples::default();
    for (material, sample) in lines
        .lines()
        .filter_map(|line| made_layout(line, &mut meshes, &mut bindposes))
    {
        samples.0[material].push(sample);
    }
    let mut spawned = materials.stand_ins(&mut commands, &samples);
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
