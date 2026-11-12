//! Shadow tiles. A tile is a small depth picture of one caster seen from
//! one light. The characters nearest the camera get a tile under each of
//! the lamps and the sun that light them most (`shadows::tile_lights`). The
//! placeables get kept tiles under the area's lamps and sun that light them
//! most (`shadows::kept_lights`). The surface shader reads a tile instead of
//! tracing a character's capsules or a placeable's box (`odyssey.wgsl`'s
//! `tile_visibility`). A tile is drawn from the caster's own meshes, so a
//! character's shadow has hands, a weapon and a robe, and a plant's has
//! leaves.
//!
//! All tiles sit in one atlas, a depth texture of two layers of
//! `ATLAS_SIZE` texels a side. Layer 0 holds the characters' tiles, each in
//! a slot of `TILE_SIZE` texels a side, and is drawn anew every frame,
//! before the cameras draw. Layer 1 holds the placeables' kept tiles, sized
//! by the placeable. A placeable and a lamp do not move, so layer 1 is drawn
//! only when its plan changes: when a placeable appears, moves or goes.
//! The pass is its own, not a Bevy view, so it adds no view's work on the
//! CPU.
//!
//! A mesh is drawn once for all its tiles, one instance a tile, and
//! meshes alike share that draw. The vertex shader moves each instance
//! into its tile's rectangle of the atlas, and the fragment shader throws
//! away what falls outside it. A surface whose texture's alpha is its
//! coverage (leaves, a grate, hair) casts only where its texture is mostly
//! opaque.
//!
//! The pass reads the meshes from Bevy's own buffers. A skinned mesh is
//! skinned in the pass's vertex shader. Its joint matrices are made here
//! from its bones, as Bevy makes them for the main pass, so the shadow
//! matches the body. Bevy makes them only for a mesh a camera sees, and a
//! character behind the camera still throws its shadow into view. A
//! cloak's sway (`dangly`) is left out: the tile draws the cloak at rest.
//!
//! A kept tile shows its placeable as it stood when the layer was drawn. A
//! placeable's own animation (a container's lid, a turning part) does not
//! draw it again. Its shadow keeps that pose until the layer is next drawn.
//! A skinned placeable's meshes are left out of its tiles.

use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Instant;

use bevy::asset::RenderAssetUsages;
use bevy::asset::{load_internal_asset, uuid_handle};
use bevy::camera::primitives::Aabb;
use bevy::image::{
    ImageAddressMode, ImageCompareFunction, ImageFilterMode, ImageSampler, ImageSamplerDescriptor,
};
use bevy::mesh::skinning::{SkinnedMesh, SkinnedMeshInverseBindposes};
use bevy::mesh::{MeshTag, MeshVertexBufferLayoutRef};
use bevy::pbr::MeshMaterial3d;
use bevy::platform::collections::HashMap;
use bevy::prelude::*;
use bevy::render::MainWorld;
use bevy::render::diagnostic::RecordDiagnostics;
use bevy::render::mesh::allocator::MeshAllocator;
use bevy::render::mesh::{RenderMesh, RenderMeshBufferInfo};
use bevy::render::render_asset::RenderAssets;
use bevy::render::render_resource::binding_types::{
    sampler, storage_buffer_read_only_sized, texture_2d,
};
use bevy::render::render_resource::{
    BindGroup, BindGroupEntries, BindGroupLayoutDescriptor, BindGroupLayoutEntries,
    CachedRenderPipelineId, CompareFunction, DepthStencilState, Extent3d, FragmentState, LoadOp,
    Operations, PipelineCache, PrimitiveState, RenderPassDepthStencilAttachment,
    RenderPassDescriptor, RenderPipelineDescriptor, SamplerBindingType, ShaderStages, ShaderType,
    StorageBuffer, StoreOp, TextureDimension, TextureFormat, TextureId, TextureSampleType,
    TextureUsages, TextureView, TextureViewDescriptor, TextureViewDimension, VertexState,
};
use bevy::render::renderer::{
    RenderContext, RenderDevice, RenderGraph, RenderGraphSystems, RenderQueue,
};
use bevy::render::texture::GpuImage;
use bevy::render::{ExtractSchedule, Render, RenderApp, RenderStartup, RenderSystems};

use crate::material::{OdysseyMaterial, Surface};
use crate::shadows::{KeptPlan, OWNER_TAG, PlannedTile, TilePlan};

const SHADER: Handle<Shader> = uuid_handle!("6d2f8a41-c3b7-4e95-a0d1-58e7b9f2c346");

/// The tiles' atlas, which every Odyssey material reads.
pub const ATLAS: Handle<Image> = uuid_handle!("e4a19c73-2b85-4f0d-9c6e-17d3a8b5f902");

/// How many texels a side each layer of the atlas has.
pub const ATLAS_SIZE: u32 = 2048;

/// How many texels a side a character's tile has.
pub const TILE_SIZE: u32 = 256;

/// The atlas's layer of the characters' tiles, drawn every frame.
pub const CHARACTERS_LAYER: u32 = 0;
/// The atlas's layer of the placeables' kept tiles.
pub const KEPT_LAYER: u32 = 1;

/// The format of the atlas. Each tile's depth range just holds its caster,
/// so 16 bits are enough.
const DEPTH: TextureFormat = TextureFormat::Depth16Unorm;

/// How many frames the kept layer waits for its meshes and pipelines to be
/// ready before it is drawn with what is ready. A pipeline is compiled a
/// few frames after it is first asked for.
const KEPT_WAIT: u32 = 120;

/// Where character tile `slot` lies in the atlas: x and y its corner, z and
/// w its size, in texels. The slots fill the layer row by row.
pub fn slot_rect(slot: usize) -> UVec4 {
    let across = (ATLAS_SIZE / TILE_SIZE) as usize;
    let (column, row) = ((slot % across) as u32, (slot / across) as u32);
    UVec4::new(column * TILE_SIZE, row * TILE_SIZE, TILE_SIZE, TILE_SIZE)
}

/// The CPU time one step of the tile pass took, added up since the dump
/// last read it, and how many times it ran. The render thread's steps
/// write theirs, and `gather` reads them all for the dump.
struct Clock {
    nanos: AtomicU64,
    runs: AtomicU64,
}

impl Clock {
    const fn new() -> Clock {
        Clock {
            nanos: AtomicU64::new(0),
            runs: AtomicU64::new(0),
        }
    }

    fn add(&self, since: Instant) {
        self.nanos
            .fetch_add(since.elapsed().as_nanos() as u64, Ordering::Relaxed);
        self.runs.fetch_add(1, Ordering::Relaxed);
    }

    /// The mean time a run took since the last read (microseconds).
    fn take(&self) -> Option<f64> {
        let nanos = self.nanos.swap(0, Ordering::Relaxed);
        let runs = self.runs.swap(0, Ordering::Relaxed);
        (runs > 0).then(|| nanos as f64 / runs as f64 / 1000.0)
    }
}

static GATHER: Clock = Clock::new();
static EXTRACT: Clock = Clock::new();
static PREPARE: Clock = Clock::new();
static DRAW: Clock = Clock::new();
/// The draw calls the characters' layer took since the dump last read
/// them, and the frames it was drawn in.
static CALLS: AtomicU64 = AtomicU64::new(0);
static CALL_FRAMES: AtomicU64 = AtomicU64::new(0);
/// How many times the kept layer was drawn since the game started.
static KEPT_DRAWN: AtomicU64 = AtomicU64::new(0);
/// The draw calls the kept layer took when last drawn.
static KEPT_CALLS: AtomicU64 = AtomicU64::new(0);

/// One mesh drawn into one tile.
#[derive(Clone, Copy, Debug)]
struct TileDraw {
    tile: u32,
    mesh: AssetId<Mesh>,
    /// Where the mesh is placed, for a mesh with no skin.
    world: Mat4,
    /// A skinned mesh's first joint matrix in `TileJobs::joints`.
    skin: Option<u32>,
    /// The texture whose alpha cuts the mesh's shadow (`cut_by`).
    cut: Option<AssetId<Image>>,
}

/// What the characters' layer draws this frame. Each tile is its rectangle
/// in the atlas (texels) and the light's view and projection. The main
/// world fills it, and the render world swaps it for its own each frame,
/// so its lists are never copied.
#[derive(Resource, Default)]
struct TileJobs {
    tiles: Vec<(UVec4, Mat4)>,
    draws: Vec<TileDraw>,
    /// The skinned meshes' joint matrices: each joint's place times its
    /// inverse bind pose.
    joints: Vec<Mat4>,
}

/// What the kept layer draws, made when its plan (`shadows::KeptPlan`)
/// changes. `generation` counts the plans, and the render world takes the
/// lists only when it moves on.
#[derive(Resource, Default)]
struct KeptJobs {
    generation: u64,
    tiles: Vec<(UVec4, Mat4)>,
    draws: Vec<TileDraw>,
    /// How many draws the plan has, and how many of them are cut by
    /// alpha, for the dump: the render world takes the draws.
    counted: (usize, usize),
}

/// The mesh entities of each model whose meshes carry it in their tag
/// (`shadows::OWNER_TAG`), kept as meshes appear, are tagged anew or go.
/// `gather` visits only the meshes of the models with tiles, not every
/// tagged mesh of the area.
#[derive(Resource, Default)]
pub struct ModelMeshes {
    of: HashMap<u32, Vec<Entity>>,
    model_of: HashMap<Entity, u32>,
}

impl ModelMeshes {
    /// The mesh entities of `model`.
    pub fn of(&self, model: usize) -> &[Entity] {
        self.of.get(&(model as u32)).map_or(&[], Vec::as_slice)
    }

    fn forget(&mut self, entity: Entity) {
        if let Some(model) = self.model_of.remove(&entity)
            && let Some(meshes) = self.of.get_mut(&model)
        {
            meshes.retain(|&mesh| mesh != entity);
            if meshes.is_empty() {
                self.of.remove(&model);
            }
        }
    }
}

/// Each character's meshes with no skin, by its model, as one sphere in
/// Bevy world space (xyz its centre, w its radius). Those are what it holds
/// and wears on its bones: a rifle, a hilt, a helmet. Its capsules hold its
/// body, but not these, so its tiles are fitted to both
/// (`shadows::upload`). A robe is skinned, and `HEM` holds its hem.
#[derive(Resource, Default)]
pub struct HeldReach(pub HashMap<usize, Vec4>);

/// How far a skinned robe's hem reaches past the capsules (metres).
pub const HEM: f32 = 0.15;

/// The meshes `gather` reads.
type TileMeshes<'w, 's> = Query<
    'w,
    's,
    (
        &'static Mesh3d,
        &'static GlobalTransform,
        &'static InheritedVisibility,
        Option<&'static SkinnedMesh>,
        &'static MeshMaterial3d<OdysseyMaterial>,
    ),
>;

/// Fills `HeldReach` for the characters that may cast this frame.
#[allow(clippy::type_complexity)]
fn held_reach(
    casters: Res<crate::shadows::Casters>,
    models: Res<ModelMeshes>,
    meshes: Query<
        (
            &Aabb,
            &GlobalTransform,
            &InheritedVisibility,
            &MeshMaterial3d<OdysseyMaterial>,
        ),
        Without<SkinnedMesh>,
    >,
    materials: Res<Assets<OdysseyMaterial>>,
    mut reach: ResMut<HeldReach>,
) {
    reach.0.clear();
    for caster in &casters.bodies {
        let mut sphere: Option<Vec4> = None;
        for &entity in models.of(caster.model) {
            let Ok((bounds, placed, shown, material)) = meshes.get(entity) else {
                continue;
            };
            if !shown.get() || materials.get(&material.0).and_then(casts).is_none() {
                continue;
            }
            let (scale, _, _) = placed.to_scale_rotation_translation();
            let centre = placed.transform_point(Vec3::from(bounds.center));
            let radius = (Vec3::from(bounds.half_extents) * scale.abs()).length();
            let one = centre.extend(radius);
            sphere = Some(sphere.map_or(one, |sphere| crate::shadows::spheres_joined(sphere, one)));
        }
        if let Some(sphere) = sphere {
            reach.0.insert(caster.model, sphere);
        }
    }
}

/// Keeps `ModelMeshes` up to date.
#[allow(clippy::type_complexity)]
fn track_meshes(
    tagged: Query<(Entity, &MeshTag), (Changed<MeshTag>, With<Mesh3d>)>,
    mut removed: RemovedComponents<MeshTag>,
    mut models: ResMut<ModelMeshes>,
) {
    for entity in removed.read() {
        models.forget(entity);
    }
    for (entity, tag) in &tagged {
        models.forget(entity);
        if tag.0 & OWNER_TAG != 0 {
            let model = tag.0 & !OWNER_TAG;
            models.of.entry(model).or_default().push(entity);
            models.model_of.insert(entity, model);
        }
    }
}

pub struct ShadowTilesPlugin;

impl Plugin for ShadowTilesPlugin {
    fn build(&self, app: &mut App) {
        load_internal_asset!(app, SHADER, "shadow_tiles.wgsl", Shader::from_wgsl);
        let _ = app
            .world_mut()
            .resource_mut::<Assets<Image>>()
            .insert(&ATLAS, atlas());
        app.init_resource::<TileJobs>()
            .init_resource::<KeptJobs>()
            .init_resource::<ModelMeshes>()
            .init_resource::<HeldReach>()
            .add_systems(
                PostUpdate,
                (
                    track_meshes.before(crate::shadows::Uploaded),
                    held_reach
                        .after(track_meshes)
                        .after(bevy::transform::TransformSystems::Propagate)
                        .after(crate::play_view::ShadowsCast)
                        .before(crate::shadows::Uploaded),
                    (gather_kept, gather)
                        .after(bevy::transform::TransformSystems::Propagate)
                        .after(crate::shadows::Uploaded),
                ),
            );
        let Some(render_app) = app.get_sub_app_mut(RenderApp) else {
            return;
        };
        render_app
            .init_resource::<TileJobs>()
            .add_systems(ExtractSchedule, extract)
            .add_systems(RenderStartup, init)
            .add_systems(Render, prepare.in_set(RenderSystems::PrepareBindGroups))
            .add_systems(
                RenderGraph,
                draw.in_set(RenderGraphSystems::Render)
                    .before(bevy::core_pipeline::schedule::camera_driver),
            );
    }
}

/// The atlas: a depth texture of two layers that the pass draws into and
/// the surface shader reads through a comparison sampler. A texel left
/// clear is at depth 1, where nothing blocks the light.
fn atlas() -> Image {
    let mut image = Image::new_uninit(
        Extent3d {
            width: ATLAS_SIZE,
            height: ATLAS_SIZE,
            depth_or_array_layers: 2,
        },
        TextureDimension::D2,
        DEPTH,
        RenderAssetUsages::default(),
    );
    image.texture_descriptor.usage =
        TextureUsages::TEXTURE_BINDING | TextureUsages::RENDER_ATTACHMENT | TextureUsages::COPY_DST;
    image.texture_view_descriptor = Some(TextureViewDescriptor {
        label: Some("shadow_tiles"),
        dimension: Some(TextureViewDimension::D2Array),
        ..default()
    });
    image.sampler = ImageSampler::Descriptor(ImageSamplerDescriptor {
        label: Some("shadow_tiles".into()),
        address_mode_u: ImageAddressMode::ClampToEdge,
        address_mode_v: ImageAddressMode::ClampToEdge,
        mag_filter: ImageFilterMode::Linear,
        min_filter: ImageFilterMode::Linear,
        compare: Some(ImageCompareFunction::LessEqual),
        ..default()
    });
    image
}

/// Whether a mesh drawn with `material` casts into a tile, and if so, the
/// texture whose alpha cuts its shadow. Opaque surfaces cast. Blended and
/// added ones (a blade's glow, an effect's card) do not. A surface whose
/// texture's alpha is its coverage casts only where the alpha is half or
/// more (`OdysseyMaterial::alpha_is_coverage`).
fn casts(material: &OdysseyMaterial) -> Option<Option<AssetId<Image>>> {
    let solid = matches!(
        material.surface,
        Surface::Opaque | Surface::OpaqueAlpha | Surface::OpaqueEnvMasked
    ) && !material.motion_only
        && material.emitted.is_none()
        && material.particles.id() == crate::material::NO_PARTICLES.id();
    solid.then(|| {
        material
            .base
            .as_ref()
            .filter(|_| material.alpha_is_coverage())
            .map(Handle::id)
    })
}

/// Lists the meshes of each tile's caster for the characters' layer. A
/// caster's meshes carry its model in their tag (`shadows::OWNER_TAG`).
#[allow(clippy::too_many_arguments)]
fn gather(
    (plan, kept_plan, kept): (Res<TilePlan>, Res<KeptPlan>, Res<KeptJobs>),
    models: Res<ModelMeshes>,
    meshes: TileMeshes,
    bones: Query<&'static GlobalTransform>,
    binds: Res<Assets<SkinnedMeshInverseBindposes>>,
    materials: Res<Assets<OdysseyMaterial>>,
    mut jobs: ResMut<TileJobs>,
    mut facts: ResMut<console::DumpFacts>,
) {
    let started = Instant::now();
    jobs.tiles.clear();
    jobs.draws.clear();
    jobs.joints.clear();
    if !plan.0.is_empty() {
        let jobs = &mut *jobs;
        let mut joints = Joints {
            bones: &bones,
            binds: &binds,
            joints: &mut jobs.joints,
        };
        draws_of(
            &plan.0,
            (&models, &meshes, &materials),
            Some(&mut joints),
            (&mut jobs.tiles, &mut jobs.draws),
        );
    }
    GATHER.add(started);
    if facts.wanted() {
        let skinned = jobs.draws.iter().filter(|draw| draw.skin.is_some()).count();
        let cut = jobs.draws.iter().filter(|draw| draw.cut.is_some()).count();
        let calls = CALLS.swap(0, Ordering::Relaxed) as f64
            / CALL_FRAMES.swap(0, Ordering::Relaxed).max(1) as f64;
        facts.0.insert(
            "shadow_tiles".into(),
            serde_json::json!({
                "tiles": jobs.tiles.len(),
                "draws": jobs.draws.len(),
                "skinned_draws": skinned,
                "cut_draws": cut,
                "joints": jobs.joints.len(),
                // Draw calls a frame, over the frames since the last dump.
                "calls": calls,
                // Each step's mean CPU time since the last dump, in
                // microseconds: gather on the main thread, the rest on the
                // render thread.
                "cpu_us": {
                    "gather": GATHER.take(),
                    "extract": EXTRACT.take(),
                    "prepare": PREPARE.take(),
                    "draw": DRAW.take(),
                },
                // The placeables' kept tiles: how many, the meshes drawn
                // into them, the draw calls they took, how many plans were
                // made, and how many times the layer was drawn since the
                // game started. The last stays still while the placeables
                // do, whatever the camera does.
                "kept_tiles": kept_plan.0.len(),
                "kept_draws": kept.counted.0,
                "kept_cut_draws": kept.counted.1,
                "kept_calls": KEPT_CALLS.load(Ordering::Relaxed),
                "kept_plans": kept.generation,
                "kept_redraws": KEPT_DRAWN.load(Ordering::Relaxed),
            }),
        );
    }
}

/// Lists the kept tiles' meshes when their plan changes.
fn gather_kept(
    kept_plan: Res<KeptPlan>,
    models: Res<ModelMeshes>,
    meshes: TileMeshes,
    materials: Res<Assets<OdysseyMaterial>>,
    mut kept: ResMut<KeptJobs>,
) {
    if !kept_plan.is_changed() {
        return;
    }
    let kept = &mut *kept;
    kept.generation += 1;
    kept.tiles.clear();
    kept.draws.clear();
    draws_of(
        &kept_plan.0,
        (&models, &meshes, &materials),
        None,
        (&mut kept.tiles, &mut kept.draws),
    );
    let cut = kept.draws.iter().filter(|draw| draw.cut.is_some()).count();
    kept.counted = (kept.draws.len(), cut);
}

/// What makes a skinned mesh's joint matrices, and where they go.
struct Joints<'a, 'w, 's> {
    bones: &'a Query<'w, 's, &'static GlobalTransform>,
    binds: &'a Assets<SkinnedMeshInverseBindposes>,
    joints: &'a mut Vec<Mat4>,
}

/// Lists `plan`'s tiles and the meshes of each one's caster. A skinned
/// mesh is drawn only when `joints` is given.
fn draws_of(
    plan: &[PlannedTile],
    (models, meshes, materials): (&ModelMeshes, &TileMeshes, &Assets<OdysseyMaterial>),
    mut joints: Option<&mut Joints>,
    (tiles, draws): (&mut Vec<(UVec4, Mat4)>, &mut Vec<TileDraw>),
) {
    let mut tiles_of: HashMap<usize, Vec<u32>> = HashMap::new();
    for (index, tile) in plan.iter().enumerate() {
        tiles_of.entry(tile.model).or_default().push(index as u32);
        tiles.push((tile.rect, tile.view_proj));
    }
    for (&model, its_tiles) in &tiles_of {
        for &entity in models.of(model) {
            let Ok((mesh, placed, shown, skin, material)) = meshes.get(entity) else {
                continue;
            };
            if !shown.get() {
                continue;
            }
            let Some(cut) = materials.get(&material.0).and_then(casts) else {
                continue;
            };
            let skin = match (skin, joints.as_deref_mut()) {
                (None, _) => None,
                (Some(_), None) => continue,
                (Some(skin), Some(joints)) => {
                    let Some(inverse) = joints.binds.get(&skin.inverse_bindposes) else {
                        continue;
                    };
                    let first = joints.joints.len() as u32;
                    for (&joint, inverse) in skin.joints.iter().zip(inverse.iter()) {
                        let placed = joints
                            .bones
                            .get(joint)
                            .map_or(Mat4::IDENTITY, GlobalTransform::to_matrix);
                        joints.joints.push(placed * *inverse);
                    }
                    Some(first)
                }
            };
            for &tile in its_tiles {
                draws.push(TileDraw {
                    tile,
                    mesh: mesh.id(),
                    world: placed.to_matrix(),
                    skin,
                    cut,
                });
            }
        }
    }
}

/// Takes this frame's character jobs into the render world, and gives the
/// main world last frame's lists to fill again. Takes the kept jobs when
/// their plan has moved on.
fn extract(
    mut main: ResMut<MainWorld>,
    mut jobs: ResMut<TileJobs>,
    mut kept: Option<ResMut<Kept>>,
) {
    let started = Instant::now();
    if let Some(mut theirs) = main.get_resource_mut::<TileJobs>() {
        std::mem::swap(&mut *jobs, &mut *theirs);
    }
    if let Some(kept) = kept.as_deref_mut()
        && let Some(mut theirs) = main.get_resource_mut::<KeptJobs>()
        && theirs.generation != kept.generation
    {
        kept.generation = theirs.generation;
        kept.waiting = Some((
            std::mem::take(&mut theirs.tiles),
            std::mem::take(&mut theirs.draws),
        ));
        kept.waited = 0;
    }
    EXTRACT.add(started);
}

/// A mesh drawn into one tile, as the shaders read it.
#[derive(Clone, Copy, Default, ShaderType)]
struct GpuDraw {
    world: Mat4,
    view_proj: Mat4,
    /// Moves the tile's clip space into the atlas's: xy scales it, and zw
    /// times w is added.
    place: Vec4,
    /// The tile's rectangle in the atlas's texels: xy its first corner, zw
    /// its far one.
    bounds: Vec4,
    /// Its first joint matrix in `Ready::joints`.
    skin: u32,
}

/// The move from a tile's clip space into the atlas's for the tile at
/// `rect` (texels): the tile's square of clip space becomes its
/// rectangle's.
fn place_of(rect: UVec4) -> Vec4 {
    let atlas = ATLAS_SIZE as f32;
    let rect = rect.as_vec4();
    Vec4::new(
        rect.z / atlas,
        rect.w / atlas,
        (2.0 * rect.x + rect.z) / atlas - 1.0,
        1.0 - (2.0 * rect.y + rect.w) / atlas,
    )
}

/// One draw call: a mesh drawn by a pipeline into the tiles of
/// `instances`, which are its draws' places in the draws' buffer. `cut` is
/// the texture whose alpha cuts it.
struct Batch {
    pipeline: CachedRenderPipelineId,
    cut: Option<AssetId<Image>>,
    mesh: AssetId<Mesh>,
    instances: std::ops::Range<u32>,
}

/// The pass's bindings and its pipelines, one for each vertex layout with
/// and without a skin and an alpha cut, and the bindings of each texture
/// that cuts.
#[derive(Resource)]
struct Pipelines {
    layout: BindGroupLayoutDescriptor,
    cut_layout: BindGroupLayoutDescriptor,
    ids: HashMap<(MeshVertexBufferLayoutRef, bool, bool), Option<CachedRenderPipelineId>>,
    cuts: HashMap<AssetId<Image>, BindGroup>,
}

/// This frame's draw calls of the characters' layer, ready for the pass.
#[derive(Resource, Default)]
struct Ready {
    batches: Vec<Batch>,
    buffer: StorageBuffer<Vec<GpuDraw>>,
    joints: StorageBuffer<Vec<Mat4>>,
}

/// A new kept plan's tiles and draws, waiting to be drawn.
type KeptWaiting = (Vec<(UVec4, Mat4)>, Vec<TileDraw>);

/// The kept layer. `waiting` holds a new plan's tiles and draws until its
/// meshes and pipelines are ready (`KEPT_WAIT`). `redraw` is set when the
/// layer must be drawn again: for a new plan, or a new atlas.
#[derive(Resource, Default)]
struct Kept {
    generation: u64,
    waiting: Option<KeptWaiting>,
    waited: u32,
    batches: Vec<Batch>,
    buffer: StorageBuffer<Vec<GpuDraw>>,
    /// Stands for the joints: no kept draw is skinned.
    no_joints: StorageBuffer<Vec<Mat4>>,
    redraw: bool,
}

/// A view of each of the atlas's layers, made once for its texture.
#[derive(Resource, Default)]
struct LayerViews {
    texture: Option<TextureId>,
    layers: Vec<TextureView>,
}

fn init(mut commands: Commands) {
    let layout = BindGroupLayoutDescriptor::new(
        "shadow_tiles",
        &BindGroupLayoutEntries::sequential(
            ShaderStages::VERTEX,
            (
                storage_buffer_read_only_sized(false, None),
                storage_buffer_read_only_sized(false, None),
            ),
        ),
    );
    let cut_layout = BindGroupLayoutDescriptor::new(
        "shadow_tiles_cut",
        &BindGroupLayoutEntries::sequential(
            ShaderStages::FRAGMENT,
            (
                texture_2d(TextureSampleType::Float { filterable: true }),
                sampler(SamplerBindingType::Filtering),
            ),
        ),
    );
    commands.insert_resource(Pipelines {
        layout,
        cut_layout,
        ids: HashMap::new(),
        cuts: HashMap::new(),
    });
    commands.init_resource::<Ready>();
    commands.init_resource::<Kept>();
    commands.init_resource::<LayerViews>();
}

/// The pipeline drawing a mesh of `layout` into the atlas. It writes depth
/// alone, from both faces. Its fragment shader keeps each instance in its
/// tile, and with `cut` throws away where the texture's alpha is under
/// half. None when the mesh lacks what the pipeline reads.
fn pipeline(
    layout: &MeshVertexBufferLayoutRef,
    (skinned, cut): (bool, bool),
    pipelines: &Pipelines,
) -> Option<RenderPipelineDescriptor> {
    let mut attributes = vec![Mesh::ATTRIBUTE_POSITION.at_shader_location(0)];
    let mut shader_defs = Vec::new();
    if skinned {
        attributes.push(Mesh::ATTRIBUTE_JOINT_INDEX.at_shader_location(1));
        attributes.push(Mesh::ATTRIBUTE_JOINT_WEIGHT.at_shader_location(2));
        shader_defs.push("SKINNED".into());
    }
    let mut bindings = vec![pipelines.layout.clone()];
    if cut {
        attributes.push(Mesh::ATTRIBUTE_UV_0.at_shader_location(3));
        shader_defs.push("CUT".into());
        bindings.push(pipelines.cut_layout.clone());
    }
    let buffer = layout.0.get_layout(&attributes).ok()?;
    Some(RenderPipelineDescriptor {
        label: Some("shadow_tiles".into()),
        layout: bindings,
        vertex: VertexState {
            shader: SHADER,
            shader_defs: shader_defs.clone(),
            buffers: vec![buffer],
            ..default()
        },
        fragment: Some(FragmentState {
            shader: SHADER,
            shader_defs,
            entry_point: None,
            targets: Vec::new(),
        }),
        primitive: PrimitiveState {
            cull_mode: None,
            ..default()
        },
        depth_stencil: Some(DepthStencilState {
            format: DEPTH,
            depth_write_enabled: Some(true),
            depth_compare: Some(CompareFunction::Less),
            stencil: default(),
            bias: default(),
        }),
        ..default()
    })
}

/// What finding the draws' meshes, pipelines and textures reads.
struct Finder<'a> {
    meshes: &'a RenderAssets<RenderMesh>,
    images: &'a RenderAssets<GpuImage>,
    cache: &'a PipelineCache,
    device: &'a RenderDevice,
}

/// Finds each draw's mesh, pipeline and cutting texture, and makes one
/// draw call of each mesh's draws. Also how many draws are not ready yet:
/// a mesh not on the GPU, a pipeline still compiling or a texture still
/// loading.
fn batches_of(
    tiles: &[(UVec4, Mat4)],
    draws: &[TileDraw],
    pipelines: &mut Pipelines,
    finder: &Finder,
) -> (Vec<Batch>, Vec<GpuDraw>, usize) {
    let places: Vec<(Vec4, Vec4)> = tiles
        .iter()
        .map(|&(rect, _)| {
            let far = (rect.xy() + rect.zw()).as_vec2();
            (
                place_of(rect),
                rect.xy().as_vec2().extend(far.x).extend(far.y),
            )
        })
        .collect();
    let mut waiting = 0;
    let mut sorted = Vec::with_capacity(draws.len());
    for draw in draws {
        let Some(mesh) = finder.meshes.get(draw.mesh) else {
            waiting += 1;
            continue;
        };
        let skinned = draw.skin.is_some();
        // A cut needs the texture on the GPU and the mesh's UVs. A mesh
        // with none casts whole.
        let mut cut = draw.cut;
        let mut id = None;
        for with_cut in [cut.is_some(), false] {
            let key = (mesh.layout.clone(), skinned, with_cut);
            let found = match pipelines.ids.get(&key) {
                Some(&found) => found,
                None => {
                    let found = pipeline(&mesh.layout, (skinned, with_cut), pipelines)
                        .map(|descriptor| finder.cache.queue_render_pipeline(descriptor));
                    pipelines.ids.insert(key, found);
                    found
                }
            };
            if found.is_some() {
                id = found;
                if !with_cut {
                    cut = None;
                }
                break;
            }
        }
        let Some(id) = id else {
            continue;
        };
        if let Some(image) = cut
            && !pipelines.cuts.contains_key(&image)
        {
            let Some(gpu) = finder.images.get(image) else {
                waiting += 1;
                continue;
            };
            let bindings = finder.device.create_bind_group(
                "shadow_tiles_cut",
                &finder.cache.get_bind_group_layout(&pipelines.cut_layout),
                &BindGroupEntries::sequential((&gpu.texture_view, &gpu.sampler)),
            );
            pipelines.cuts.insert(image, bindings);
        }
        if finder.cache.get_render_pipeline(id).is_none() {
            waiting += 1;
        }
        let (place, bounds) = places[draw.tile as usize];
        sorted.push((
            id,
            cut,
            draw.mesh,
            GpuDraw {
                world: draw.world,
                view_proj: tiles[draw.tile as usize].1,
                place,
                bounds,
                skin: draw.skin.unwrap_or(0),
            },
        ));
    }
    // A mesh's draws follow one another, so one call draws them all.
    sorted.sort_unstable_by_key(|&(id, cut, mesh, _)| (id, cut, mesh));
    let mut batches: Vec<Batch> = Vec::new();
    let mut gpu = Vec::with_capacity(sorted.len());
    for (index, (id, cut, mesh, draw)) in sorted.into_iter().enumerate() {
        let index = index as u32;
        match batches.last_mut() {
            Some(batch) if batch.pipeline == id && batch.cut == cut && batch.mesh == mesh => {
                batch.instances.end = index + 1;
            }
            _ => batches.push(Batch {
                pipeline: id,
                cut,
                mesh,
                instances: index..index + 1,
            }),
        }
        gpu.push(draw);
    }
    (batches, gpu, waiting)
}

/// Writes the characters' draws for the shaders, and the kept layer's when
/// a new plan is ready.
#[allow(clippy::too_many_arguments)]
fn prepare(
    mut jobs: ResMut<TileJobs>,
    (meshes, images): (Res<RenderAssets<RenderMesh>>, Res<RenderAssets<GpuImage>>),
    cache: Res<PipelineCache>,
    mut pipelines: ResMut<Pipelines>,
    (mut ready, mut kept): (ResMut<Ready>, ResMut<Kept>),
    device: Res<RenderDevice>,
    queue: Res<RenderQueue>,
) {
    let started = Instant::now();
    let finder = Finder {
        meshes: &meshes,
        images: &images,
        cache: &cache,
        device: &device,
    };
    let ready = &mut *ready;
    ready.batches.clear();
    if !jobs.tiles.is_empty() {
        let (batches, gpu, _) = batches_of(&jobs.tiles, &jobs.draws, &mut pipelines, &finder);
        if !gpu.is_empty() {
            ready.batches = batches;
            ready.buffer.set(gpu);
            ready.buffer.write_buffer(&device, &queue);
            // A storage buffer may not be empty.
            let mut joints = std::mem::take(&mut jobs.joints);
            if joints.is_empty() {
                joints.push(Mat4::IDENTITY);
            }
            ready.joints.set(joints);
            ready.joints.write_buffer(&device, &queue);
        }
    }
    let kept = &mut *kept;
    if let Some((tiles, draws)) = &kept.waiting {
        let (batches, gpu, waiting) = batches_of(tiles, draws, &mut pipelines, &finder);
        if waiting == 0 || kept.waited >= KEPT_WAIT {
            kept.batches = batches;
            if !gpu.is_empty() {
                kept.buffer.set(gpu);
                kept.buffer.write_buffer(&device, &queue);
            }
            if kept.no_joints.get().is_empty() {
                kept.no_joints.set(vec![Mat4::IDENTITY]);
                kept.no_joints.write_buffer(&device, &queue);
            }
            kept.waiting = None;
            kept.redraw = true;
        } else {
            kept.waited += 1;
        }
    }
    PREPARE.add(started);
}

/// What drawing a layer's batches reads.
struct Drawer<'a> {
    pipelines: &'a Pipelines,
    cache: &'a PipelineCache,
    meshes: &'a RenderAssets<RenderMesh>,
    allocator: &'a MeshAllocator,
    device: &'a RenderDevice,
}

/// Draws `batches` into `layer`, cleared first. Returns the draw calls
/// made.
fn draw_layer(
    ctx: &mut RenderContext,
    layer: &TextureView,
    (batches, draws, joints): (
        &[Batch],
        &StorageBuffer<Vec<GpuDraw>>,
        &StorageBuffer<Vec<Mat4>>,
    ),
    drawer: &Drawer,
) -> u64 {
    let bindings = match (draws.binding(), joints.binding()) {
        (Some(draws), Some(joints)) if !batches.is_empty() => {
            Some(drawer.device.create_bind_group(
                "shadow_tiles",
                &drawer.cache.get_bind_group_layout(&drawer.pipelines.layout),
                &BindGroupEntries::sequential((joints, draws)),
            ))
        }
        _ => None,
    };
    // The tracked pass skips a pipeline, binding or buffer already set.
    // Most meshes share a few of the allocator's buffers.
    let mut pass = ctx.begin_tracked_render_pass(RenderPassDescriptor {
        label: Some("shadow_tiles"),
        color_attachments: &[],
        depth_stencil_attachment: Some(RenderPassDepthStencilAttachment {
            view: layer,
            depth_ops: Some(Operations {
                load: LoadOp::Clear(1.0),
                store: StoreOp::Store,
            }),
            stencil_ops: None,
        }),
        timestamp_writes: None,
        occlusion_query_set: None,
        multiview_mask: None,
    });
    let Some(bindings) = &bindings else {
        return 0;
    };
    pass.set_bind_group(0, bindings, &[]);
    let mut calls = 0u64;
    for batch in batches {
        let mesh_id = batch.mesh;
        let Some(render) = drawer.cache.get_render_pipeline(batch.pipeline) else {
            continue;
        };
        let (Some(mesh), Some(vertices)) = (
            drawer.meshes.get(mesh_id),
            drawer.allocator.mesh_vertex_slice(&mesh_id),
        ) else {
            continue;
        };
        if let Some(image) = batch.cut {
            let Some(cut) = drawer.pipelines.cuts.get(&image) else {
                continue;
            };
            pass.set_bind_group(1, cut, &[]);
        }
        pass.set_render_pipeline(render);
        pass.set_vertex_buffer(0, vertices.buffer.slice(..));
        let instances = batch.instances.clone();
        match &mesh.buffer_info {
            RenderMeshBufferInfo::Indexed {
                index_format,
                count,
            } => {
                let Some(indices) = drawer.allocator.mesh_index_slice(&mesh_id) else {
                    continue;
                };
                pass.set_index_buffer(indices.buffer.slice(..), *index_format);
                pass.draw_indexed(
                    indices.range.start..indices.range.start + count,
                    vertices.range.start as i32,
                    instances,
                );
            }
            RenderMeshBufferInfo::NonIndexed => {
                pass.draw(vertices.range.clone(), instances);
            }
        }
        calls += 1;
    }
    calls
}

/// Draws the characters' tiles into layer 0, and the kept tiles into layer
/// 1 when they must be drawn again.
#[allow(clippy::too_many_arguments)]
fn draw(
    ready: Res<Ready>,
    mut kept: ResMut<Kept>,
    mut views: ResMut<LayerViews>,
    pipelines: Res<Pipelines>,
    cache: Res<PipelineCache>,
    images: Res<RenderAssets<GpuImage>>,
    (meshes, allocator): (Res<RenderAssets<RenderMesh>>, Res<MeshAllocator>),
    device: Res<RenderDevice>,
    mut ctx: RenderContext,
) {
    let started = Instant::now();
    let Some(atlas) = images.get(&ATLAS) else {
        DRAW.add(started);
        return;
    };
    // A new atlas holds no kept tiles.
    if views.texture != Some(atlas.texture.id()) {
        views.texture = Some(atlas.texture.id());
        views.layers = (0..2)
            .map(|layer| {
                atlas.texture.create_view(&TextureViewDescriptor {
                    label: Some("shadow_tiles_layer"),
                    dimension: Some(TextureViewDimension::D2),
                    base_array_layer: layer,
                    array_layer_count: Some(1),
                    ..default()
                })
            })
            .collect();
        kept.redraw = true;
    }
    if ready.batches.is_empty() && !kept.redraw {
        DRAW.add(started);
        return;
    }
    let drawer = Drawer {
        pipelines: &pipelines,
        cache: &cache,
        meshes: &meshes,
        allocator: &allocator,
        device: &device,
    };
    let diagnostics = ctx.diagnostic_recorder();
    let diagnostics = diagnostics.as_deref();
    let time_span = diagnostics.time_span(ctx.command_encoder(), "shadow_tiles");
    if !ready.batches.is_empty() {
        let calls = draw_layer(
            &mut ctx,
            &views.layers[CHARACTERS_LAYER as usize],
            (&ready.batches, &ready.buffer, &ready.joints),
            &drawer,
        );
        CALLS.fetch_add(calls, Ordering::Relaxed);
        CALL_FRAMES.fetch_add(1, Ordering::Relaxed);
    }
    if kept.redraw {
        let calls = draw_layer(
            &mut ctx,
            &views.layers[KEPT_LAYER as usize],
            (&kept.batches, &kept.buffer, &kept.no_joints),
            &drawer,
        );
        KEPT_CALLS.store(calls, Ordering::Relaxed);
        KEPT_DRAWN.fetch_add(1, Ordering::Relaxed);
        kept.redraw = false;
    }
    time_span.end(ctx.command_encoder());
    DRAW.add(started);
}
