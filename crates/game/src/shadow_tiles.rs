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
//! A character's model has many meshes, and each would be a draw call. So
//! each model of a character gets shadow meshes when it is spawned
//! (`bake`): its meshes that cast, joined into one mesh for each texture
//! that cuts them. A shadow mesh is drawn in the meshes' place while they
//! are all shown and as they were spawned. When one of them is hidden, cut
//! or swapped, the model's meshes are drawn one by one again, so the shadow
//! is the same either way.
//!
//! The draw calls are kept from frame to frame. The same characters under
//! the same lights make the same draw calls, so the render world makes them
//! anew only when the list of draws changes (`TileJobs::generation`). Every
//! frame it writes only the matrices, in one buffer: each tile's view, and
//! what places each mesh. A frame whose matrices are the same as the frame
//! before draws nothing, and the layer keeps its picture. That is so while
//! the game is paused.
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

use std::num::NonZero;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::Instant;

use asset_odyssey::model::ModelIr;

use bevy::asset::RenderAssetUsages;
use bevy::asset::{load_internal_asset, uuid_handle};
use bevy::camera::primitives::Aabb;
use bevy::image::{
    ImageAddressMode, ImageCompareFunction, ImageFilterMode, ImageSampler, ImageSamplerDescriptor,
};
use bevy::mesh::skinning::{SkinnedMesh, SkinnedMeshInverseBindposes};
use bevy::mesh::{
    Indices, MeshTag, MeshVertexAttribute, MeshVertexBufferLayoutRef, PrimitiveTopology,
    VertexAttributeValues,
};
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
    BindGroup, BindGroupEntries, BindGroupLayoutDescriptor, BindGroupLayoutEntries, BufferId,
    BufferUsages, CachedRenderPipelineId, CompareFunction, DepthStencilState, Extent3d,
    FragmentState, LoadOp, Operations, PipelineCache, PrimitiveState, RawBufferVec,
    RenderPassDepthStencilAttachment, RenderPassDescriptor, RenderPipelineDescriptor,
    SamplerBindingType, ShaderStages, StoreOp, TextureDimension, TextureFormat, TextureId,
    TextureSampleType, TextureUsages, TextureView, TextureViewDescriptor, TextureViewDimension,
    VertexFormat, VertexState,
};
use bevy::render::renderer::{
    RenderContext, RenderDevice, RenderGraph, RenderGraphSystems, RenderQueue,
};
use bevy::render::texture::GpuImage;
use bevy::render::{ExtractSchedule, Render, RenderApp, RenderStartup, RenderSystems};

use crate::material::{OdysseyMaterial, Surface};
use crate::scene::to_mat4;
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
/// How many times the characters' draw calls were made anew since the dump
/// last read it. It stays at 0 while the same characters stand under the
/// same lights.
static REMADE: AtomicU64 = AtomicU64::new(0);
/// How many frames drew no character tile because no matrix had changed,
/// since the dump last read it.
static STILL: AtomicU64 = AtomicU64::new(0);
/// Set when a kept draw call met a mesh whose vertex layout had changed.
/// The draw calls are then made anew.
static STALE: AtomicBool = AtomicBool::new(false);
/// How many times the kept layer was drawn since the game started.
static KEPT_DRAWN: AtomicU64 = AtomicU64::new(0);
/// The draw calls the kept layer took when last drawn.
static KEPT_CALLS: AtomicU64 = AtomicU64::new(0);

/// How a draw's vertices are placed.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum Kind {
    /// By one matrix: where the mesh is placed.
    Rigid,
    /// By its skin's joint matrices, as the main pass skins it.
    Skinned,
    /// A shadow mesh (`bake`), by where its model's nodes are placed.
    Baked,
}

/// One mesh drawn into one tile.
#[derive(Clone, Copy, Debug, PartialEq)]
struct TileDraw {
    tile: u32,
    mesh: AssetId<Mesh>,
    kind: Kind,
    /// Its first matrix in the jobs' `matrices`. A mesh with no skin has
    /// one, where it is placed. A skinned mesh has one for each joint. A
    /// shadow mesh has one for each node of its model that places a vertex.
    first: u32,
    /// The texture whose alpha cuts the mesh's shadow (`casts`).
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
    /// Every matrix the pass's shaders read. First come two for each tile:
    /// the light's view and projection, and where the tile lies in the
    /// atlas (`tile_fit`). Then come the matrices the draws are placed by
    /// (`TileDraw::first`).
    matrices: Vec<Mat4>,
    /// Counts how many times `draws` differed from the frame before. While
    /// it stays, the render world keeps the draw calls it made.
    generation: u64,
}

/// What the kept layer draws, made when its plan (`shadows::KeptPlan`)
/// changes. `generation` counts the plans, and the render world takes the
/// lists only when it moves on.
#[derive(Resource, Default)]
struct KeptJobs {
    generation: u64,
    tiles: Vec<(UVec4, Mat4)>,
    draws: Vec<TileDraw>,
    matrices: Vec<Mat4>,
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
        Option<&'static Baked>,
    ),
>;

/// A shadow mesh's vertex as its second, third and fourth joints place it.
/// `Mesh::ATTRIBUTE_POSITION` holds it as its first joint places it.
const PLACE_1: MeshVertexAttribute =
    MeshVertexAttribute::new("ShadowPlace1", 988_540_931, VertexFormat::Float32x3);
const PLACE_2: MeshVertexAttribute =
    MeshVertexAttribute::new("ShadowPlace2", 988_540_932, VertexFormat::Float32x3);
const PLACE_3: MeshVertexAttribute =
    MeshVertexAttribute::new("ShadowPlace3", 988_540_933, VertexFormat::Float32x3);

/// The fewest meshes a shadow mesh is made of. One mesh alone is one draw
/// call either way.
const LEAST_BAKED: usize = 2;

/// On a character's model's root: its shadow meshes (`bake`).
#[derive(Component)]
pub struct ShadowMesh {
    /// The model's nodes that place the shadow meshes' vertices, in the
    /// order the vertices name them.
    nodes: Vec<Entity>,
    groups: Vec<ShadowGroup>,
}

/// One shadow mesh: the model's meshes that the texture `cut` cuts, or its
/// meshes that nothing cuts, joined. `parts` is how many meshes it holds.
struct ShadowGroup {
    mesh: Handle<Mesh>,
    cut: Option<AssetId<Image>>,
    parts: u32,
}

/// On a mesh that a shadow mesh of its model holds: the model's root and
/// which of its shadow meshes. The rest is the mesh as it was spawned: its
/// mesh, how many joints its skin has (0 with no skin) and the texture that
/// cuts it. The shadow mesh stands for the mesh only while these still
/// hold.
#[derive(Component)]
pub struct Baked {
    root: Entity,
    group: u32,
    mesh: AssetId<Mesh>,
    joints: u32,
    cut: Option<AssetId<Image>>,
}

/// One mesh of a model as the scene spawned it: its place in the model's
/// meshes, its entity, its mesh, its material and whether it got a skin.
pub(crate) struct SpawnedPart {
    pub part: usize,
    pub entity: Entity,
    pub mesh: AssetId<Mesh>,
    pub material: Handle<OdysseyMaterial>,
    pub skinned: bool,
}

/// Makes the shadow meshes of the model `ir` spawned at `root`, whose
/// nodes are `nodes` and whose meshes are `parts`. The meshes that cast are
/// grouped by the texture that cuts them, and each group of `LEAST_BAKED`
/// or more becomes one shadow mesh. A danglymesh is left out, because its
/// vertices move. So is a cut mesh with no texture coordinates.
pub(crate) fn bake(
    commands: &mut Commands,
    (meshes, materials): (&mut Assets<Mesh>, &Assets<OdysseyMaterial>),
    (ir, nodes, root): (&ModelIr, &[Entity], Entity),
    parts: &[SpawnedPart],
) {
    let mut groups: Vec<(Option<AssetId<Image>>, Vec<&SpawnedPart>)> = Vec::new();
    for part in parts {
        let Some(cut) = materials.get(&part.material).and_then(casts) else {
            continue;
        };
        let source = &ir.meshes[part.part];
        if source.dangly.is_some() || (cut.is_some() && source.uv1.len() != source.positions.len())
        {
            continue;
        }
        match groups.iter_mut().find(|(of, _)| *of == cut) {
            Some((_, group)) => group.push(part),
            None => groups.push((cut, vec![part])),
        }
    }
    let mut palette: Vec<usize> = Vec::new();
    let mut baked = Vec::new();
    for (cut, group) in groups {
        if group.len() < LEAST_BAKED {
            continue;
        }
        let Some(mesh) = shadow_mesh(ir, &group, cut.is_some(), &mut palette) else {
            continue;
        };
        for part in &group {
            let joints = ir.meshes[part.part]
                .skin
                .as_ref()
                .filter(|_| part.skinned)
                .map_or(0, |skin| skin.joints.len());
            commands.entity(part.entity).insert(Baked {
                root,
                group: baked.len() as u32,
                mesh: part.mesh,
                joints: joints as u32,
                cut,
            });
        }
        baked.push(ShadowGroup {
            mesh: meshes.add(mesh),
            cut,
            parts: group.len() as u32,
        });
    }
    if !baked.is_empty() {
        commands.entity(root).insert(ShadowMesh {
            nodes: palette.iter().map(|&node| nodes[node]).collect(),
            groups: baked,
        });
    }
}

/// The meshes `parts` of the model `ir` joined into one shadow mesh. Each
/// vertex names up to four of the model's nodes by their places in
/// `palette`, which grows as nodes are first named, and has a weight for
/// each. It also has its place as each of the four nodes sees it: a
/// skinned vertex's place taken through that joint's inverse bind pose,
/// and a vertex with no skin as it is, under its mesh's node. The pass's
/// shader places a vertex by each node, and sums the results by their
/// weights. That is the sum the main pass makes for a skinned mesh, so the
/// shadow matches the body. None when a mesh's skin cannot be joined so: an
/// inverse bind pose that is not a plain placement, or a weighted joint the
/// skin does not have.
fn shadow_mesh(
    ir: &ModelIr,
    parts: &[&SpawnedPart],
    cut: bool,
    palette: &mut Vec<usize>,
) -> Option<Mesh> {
    let mut slot = |node: usize| -> u16 {
        let at = palette
            .iter()
            .position(|&held| held == node)
            .unwrap_or_else(|| {
                palette.push(node);
                palette.len() - 1
            });
        at as u16
    };
    let mut places: [Vec<[f32; 3]>; 4] = default();
    let mut slots: Vec<[u16; 4]> = Vec::new();
    let mut weights: Vec<[f32; 4]> = Vec::new();
    let mut uvs: Vec<[f32; 2]> = Vec::new();
    let mut indices: Vec<u32> = Vec::new();
    for part in parts {
        let source = &ir.meshes[part.part];
        let first = places[0].len() as u32;
        let count = source.positions.len() as u32;
        match source.skin.as_ref().filter(|_| part.skinned) {
            Some(skin) => {
                let binds: Vec<Mat4> = skin.inverse_binds.iter().map(to_mat4).collect();
                // The main pass gives a skin a matrix for each joint that
                // has an inverse bind pose.
                let bones = skin.joints.len().min(binds.len());
                if binds[..bones].iter().any(|bind| bind.row(3) != Vec4::W) {
                    return None;
                }
                let named: Vec<u16> = skin.joints[..bones]
                    .iter()
                    .map(|&node| slot(node))
                    .collect();
                let vertices = source
                    .positions
                    .iter()
                    .zip(&skin.vertex_joints)
                    .zip(&skin.vertex_weights);
                for ((position, joints), weight) in vertices {
                    let position = Vec3::from_array(*position);
                    let mut its = [0u16; 4];
                    for k in 0..4 {
                        let bone = joints[k] as usize;
                        if bone < bones {
                            places[k].push(binds[bone].transform_point3(position).to_array());
                            its[k] = named[bone];
                        } else if weight[k] == 0.0 {
                            places[k].push(position.to_array());
                        } else {
                            return None;
                        }
                    }
                    slots.push(its);
                    weights.push(*weight);
                }
            }
            None => {
                let its = slot(source.node);
                for position in &source.positions {
                    for place in &mut places {
                        place.push(*position);
                    }
                    slots.push([its; 4]);
                    weights.push([1.0, 0.0, 0.0, 0.0]);
                }
            }
        }
        if cut {
            uvs.extend_from_slice(&source.uv1);
        }
        for corners in source.indices.as_chunks::<3>().0 {
            if corners.iter().all(|&corner| corner < count) {
                indices.extend(corners.iter().map(|&corner| first + corner));
            }
        }
    }
    if indices.is_empty() {
        return None;
    }
    let [first, second, third, fourth] = places;
    let mut mesh = Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::RENDER_WORLD,
    );
    mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, first);
    mesh.insert_attribute(PLACE_1, second);
    mesh.insert_attribute(PLACE_2, third);
    mesh.insert_attribute(PLACE_3, fourth);
    mesh.insert_attribute(
        Mesh::ATTRIBUTE_JOINT_INDEX,
        VertexAttributeValues::Uint16x4(slots),
    );
    mesh.insert_attribute(Mesh::ATTRIBUTE_JOINT_WEIGHT, weights);
    if cut {
        mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, uvs);
    }
    mesh.insert_indices(Indices::U32(indices));
    Some(mesh)
}

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

/// What listing a plan's draws reads.
#[derive(bevy::ecs::system::SystemParam)]
struct Sources<'w, 's> {
    models: Res<'w, ModelMeshes>,
    meshes: TileMeshes<'w, 's>,
    materials: Res<'w, Assets<OdysseyMaterial>>,
    bones: Query<'w, 's, &'static GlobalTransform>,
    binds: Res<'w, Assets<SkinnedMeshInverseBindposes>>,
    shadows: Query<'w, 's, &'static ShadowMesh>,
}

/// Lists the meshes of each tile's caster for the characters' layer. A
/// caster's meshes carry its model in their tag (`shadows::OWNER_TAG`).
fn gather(
    (plan, kept_plan, kept): (Res<TilePlan>, Res<KeptPlan>, Res<KeptJobs>),
    sources: Sources,
    mut jobs: ResMut<TileJobs>,
    (mut last, mut generation): (Local<Vec<TileDraw>>, Local<u64>),
    mut facts: ResMut<console::DumpFacts>,
) {
    let started = Instant::now();
    let jobs = &mut *jobs;
    jobs.tiles.clear();
    jobs.draws.clear();
    jobs.matrices.clear();
    if !plan.0.is_empty() {
        draws_of(
            &plan.0,
            &sources,
            true,
            (&mut jobs.tiles, &mut jobs.draws, &mut jobs.matrices),
        );
    }
    // The same draws as the frame before keep the render world's draw
    // calls.
    if *last != jobs.draws {
        last.clone_from(&jobs.draws);
        *generation += 1;
    }
    jobs.generation = *generation;
    GATHER.add(started);
    if facts.wanted() {
        let of = |kind: Kind| jobs.draws.iter().filter(|draw| draw.kind == kind).count();
        let cut = jobs.draws.iter().filter(|draw| draw.cut.is_some()).count();
        let calls = CALLS.swap(0, Ordering::Relaxed) as f64
            / CALL_FRAMES.swap(0, Ordering::Relaxed).max(1) as f64;
        facts.0.insert(
            "shadow_tiles".into(),
            serde_json::json!({
                "tiles": jobs.tiles.len(),
                "draws": jobs.draws.len(),
                "skinned_draws": of(Kind::Skinned),
                // The draws of shadow meshes, each in the place of two or
                // more meshes.
                "baked_draws": of(Kind::Baked),
                "cut_draws": cut,
                // The matrices that place the meshes, past the two of
                // each tile.
                "joints": jobs.matrices.len() - 2 * jobs.tiles.len(),
                // Draw calls a frame, over the frames since the last dump.
                "calls": calls,
                // How many times the draw calls were made anew since the
                // last dump, and how many frames drew nothing because no
                // matrix had changed.
                "remade": REMADE.swap(0, Ordering::Relaxed),
                "still_frames": STILL.swap(0, Ordering::Relaxed),
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
fn gather_kept(kept_plan: Res<KeptPlan>, sources: Sources, mut kept: ResMut<KeptJobs>) {
    if !kept_plan.is_changed() {
        return;
    }
    let kept = &mut *kept;
    kept.generation += 1;
    kept.tiles.clear();
    kept.draws.clear();
    kept.matrices.clear();
    draws_of(
        &kept_plan.0,
        &sources,
        false,
        (&mut kept.tiles, &mut kept.draws, &mut kept.matrices),
    );
    let cut = kept.draws.iter().filter(|draw| draw.cut.is_some()).count();
    kept.counted = (kept.draws.len(), cut);
}

/// One mesh of a model that casts into its tiles now: its mesh, where it
/// is placed, its skin, the texture that cuts it, and the shadow mesh that
/// may stand for it (the model's root and which of its shadow meshes).
struct Found<'a> {
    mesh: AssetId<Mesh>,
    placed: &'a GlobalTransform,
    skin: Option<&'a SkinnedMesh>,
    cut: Option<AssetId<Image>>,
    held: Option<(Entity, u32)>,
}

/// Lists `plan`'s tiles, the meshes of each one's caster and the matrices
/// the shaders read, into empty lists. The matrices are two for each tile,
/// then the ones that place the meshes. With `characters`, a shadow mesh is
/// drawn in the place of its meshes when every one of them casts now, as it
/// was spawned (`Baked`). Without, each mesh is drawn itself and a skinned
/// one is left out, as the kept tiles have it.
fn draws_of(
    plan: &[PlannedTile],
    sources: &Sources,
    characters: bool,
    (tiles, draws, joints): (&mut Vec<(UVec4, Mat4)>, &mut Vec<TileDraw>, &mut Vec<Mat4>),
) {
    tiles.extend(plan.iter().map(|tile| (tile.rect, tile.view_proj)));
    for tile in plan {
        joints.push(tile.view_proj);
        joints.push(tile_fit(tile.rect));
    }
    let mut found: Vec<Found> = Vec::new();
    // Each shadow mesh met, and how many of its meshes cast now. The count
    // is set to 0 when the shadow mesh cannot stand for them.
    let mut met: Vec<(Entity, u32, u32)> = Vec::new();
    // Each model root's first matrix in `joints`.
    let mut placed_at: Vec<(Entity, u32)> = Vec::new();
    let mut next_tile = 0u32;
    // A model's tiles follow one another in a plan.
    for run in plan.chunk_by(|a, b| a.model == b.model) {
        let its_tiles = next_tile..next_tile + run.len() as u32;
        next_tile = its_tiles.end;
        found.clear();
        met.clear();
        placed_at.clear();
        for &entity in sources.models.of(run[0].model) {
            let Ok((mesh, placed, shown, skin, material, baked)) = sources.meshes.get(entity)
            else {
                continue;
            };
            if !shown.get() {
                continue;
            }
            let Some(cut) = sources.materials.get(&material.0).and_then(casts) else {
                continue;
            };
            let held = baked
                .filter(|baked| {
                    characters
                        && baked.mesh == mesh.id()
                        && baked.cut == cut
                        && baked.joints == skin.map_or(0, |skin| skin.joints.len() as u32)
                })
                .map(|baked| (baked.root, baked.group));
            if let Some((root, group)) = held {
                match met.iter_mut().find(|met| (met.0, met.1) == (root, group)) {
                    Some(met) => met.2 += 1,
                    None => met.push((root, group, 1)),
                }
            }
            found.push(Found {
                mesh: mesh.id(),
                placed,
                skin,
                cut,
                held,
            });
        }
        for met in &mut met {
            let (root, group, count) = *met;
            let whole = sources.shadows.get(root).ok().and_then(|shadow| {
                let its = shadow.groups.get(group as usize)?;
                (its.parts == count).then_some((shadow, its))
            });
            let Some((shadow, its)) = whole else {
                met.2 = 0;
                continue;
            };
            let first = match placed_at.iter().find(|placed| placed.0 == root) {
                Some(&(_, first)) => first,
                None => {
                    let first = joints.len() as u32;
                    joints.extend(shadow.nodes.iter().map(|&node| {
                        sources
                            .bones
                            .get(node)
                            .map_or(Mat4::IDENTITY, GlobalTransform::to_matrix)
                    }));
                    placed_at.push((root, first));
                    first
                }
            };
            draws.extend(its_tiles.clone().map(|tile| TileDraw {
                tile,
                mesh: its.mesh.id(),
                kind: Kind::Baked,
                first,
                cut: its.cut,
            }));
        }
        for mesh in &found {
            let stood_for = mesh.held.is_some_and(|(root, group)| {
                met.iter()
                    .any(|&(of, its, count)| (of, its) == (root, group) && count > 0)
            });
            if stood_for {
                continue;
            }
            let first = joints.len() as u32;
            let kind = match mesh.skin {
                None => {
                    joints.push(mesh.placed.to_matrix());
                    Kind::Rigid
                }
                Some(_) if !characters => continue,
                Some(skin) => {
                    let Some(inverse) = sources.binds.get(&skin.inverse_bindposes) else {
                        continue;
                    };
                    for (&joint, inverse) in skin.joints.iter().zip(inverse.iter()) {
                        let placed = sources
                            .bones
                            .get(joint)
                            .map_or(Mat4::IDENTITY, GlobalTransform::to_matrix);
                        joints.push(placed * *inverse);
                    }
                    Kind::Skinned
                }
            };
            draws.extend(its_tiles.clone().map(|tile| TileDraw {
                tile,
                mesh: mesh.mesh,
                kind,
                first,
                cut: mesh.cut,
            }));
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
        kept.waiting = Some(KeptWaiting {
            draws: std::mem::take(&mut theirs.draws),
            matrices: std::mem::take(&mut theirs.matrices),
        });
        kept.waited = 0;
    }
    EXTRACT.add(started);
}

/// A draw as the shaders read it: its tile, and its first matrix among the
/// matrices.
type GpuDraw = [u32; 2];

/// Where the tile at `rect` (texels) lies in the atlas, as the pass's
/// shaders read it. The first column moves the tile's clip space into the
/// atlas's: xy scales it, and zw times w is added. The second is the
/// tile's rectangle in the atlas's texels: xy its first corner, zw its far
/// one. The other two are not read.
fn tile_fit(rect: UVec4) -> Mat4 {
    let far = (rect.xy() + rect.zw()).as_vec2();
    Mat4::from_cols(
        place_of(rect),
        rect.xy().as_vec2().extend(far.x).extend(far.y),
        Vec4::ZERO,
        Vec4::ZERO,
    )
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
/// the texture whose alpha cuts it. `layout` is the mesh's vertex layout
/// when the call was made, which the pipeline was made for.
struct Batch {
    pipeline: CachedRenderPipelineId,
    cut: Option<AssetId<Image>>,
    mesh: AssetId<Mesh>,
    layout: MeshVertexBufferLayoutRef,
    instances: std::ops::Range<u32>,
}

/// The pass's bindings and its pipelines, one for each vertex layout, each
/// way of placing its vertices (`Kind`), with and without an alpha cut, and
/// the bindings of each texture that cuts.
#[derive(Resource)]
struct Pipelines {
    layout: BindGroupLayoutDescriptor,
    cut_layout: BindGroupLayoutDescriptor,
    ids: HashMap<(MeshVertexBufferLayoutRef, Kind, bool), Option<CachedRenderPipelineId>>,
    cuts: HashMap<AssetId<Image>, BindGroup>,
}

/// One layer's draw calls, and what its shaders read on the GPU: the
/// draws and the matrices. `bindings` binds the two buffers, and is made
/// again only when one of them is made again.
struct Layer {
    batches: Vec<Batch>,
    draws: RawBufferVec<GpuDraw>,
    matrices: RawBufferVec<Mat4>,
    bindings: Option<(BindGroup, [BufferId; 2])>,
}

impl Default for Layer {
    fn default() -> Layer {
        Layer {
            batches: Vec::new(),
            draws: RawBufferVec::new(BufferUsages::STORAGE),
            matrices: RawBufferVec::new(BufferUsages::STORAGE),
            bindings: None,
        }
    }
}

impl Layer {
    /// Takes new draw calls and their draws.
    fn take(&mut self, batches: Vec<Batch>, draws: Vec<GpuDraw>) {
        self.batches = batches;
        *self.draws.values_mut() = draws;
    }

    /// Takes `matrices`, and leaves its own list in their place, to be
    /// filled again.
    fn pose(&mut self, matrices: &mut Vec<Mat4>) {
        std::mem::swap(self.matrices.values_mut(), matrices);
        // A storage buffer may not be empty.
        if self.matrices.is_empty() {
            self.matrices.push(Mat4::IDENTITY);
        }
    }

    /// Writes the matrices to the GPU, and the draws with `draws`. Each
    /// buffer is made with room to spare, so it is not made again each time
    /// its list grows by one. Then binds them.
    fn write(
        &mut self,
        draws: bool,
        pipelines: &Pipelines,
        (cache, device, queue): (&PipelineCache, &RenderDevice, &RenderQueue),
    ) {
        if draws && !self.draws.is_empty() {
            let room = self.draws.len().next_power_of_two();
            self.draws.reserve(room, device);
            self.draws.write_buffer(device, queue);
        }
        let room = self.matrices.len().next_power_of_two();
        self.matrices.reserve(room, device);
        self.matrices.write_buffer(device, queue);
        let (Some(matrices), Some(draws)) = (self.matrices.buffer(), self.draws.buffer()) else {
            self.bindings = None;
            return;
        };
        let made_of = [matrices.id(), draws.id()];
        if self.bindings.as_ref().is_some_and(|(_, of)| *of == made_of) {
            return;
        }
        let bindings = device.create_bind_group(
            "shadow_tiles",
            &cache.get_bind_group_layout(&pipelines.layout),
            &BindGroupEntries::sequential((
                matrices.as_entire_binding(),
                draws.as_entire_binding(),
            )),
        );
        self.bindings = Some((bindings, made_of));
    }
}

/// The characters' layer, ready for the pass. `made_of` is the generation
/// of the jobs its draw calls were made of (`TileJobs::generation`), once
/// every mesh, pipeline and texture of theirs was ready. While the jobs
/// stay at it, the draw calls are kept. `live` is whether this frame has
/// anything to draw. `still` is whether the frame's draws and matrices are
/// the same as the frame's before: the layer then holds its picture
/// already, and is not drawn again.
#[derive(Resource, Default)]
struct Ready {
    layer: Layer,
    made_of: Option<u64>,
    live: bool,
    still: bool,
}

/// A new kept plan's draws and matrices, waiting to be drawn.
struct KeptWaiting {
    draws: Vec<TileDraw>,
    matrices: Vec<Mat4>,
}

/// The kept layer. `waiting` holds a new plan until its meshes and
/// pipelines are ready (`KEPT_WAIT`). `redraw` is set when the layer must
/// be drawn again: for a new plan, or a new atlas.
#[derive(Resource, Default)]
struct Kept {
    generation: u64,
    waiting: Option<KeptWaiting>,
    waited: u32,
    layer: Layer,
    redraw: bool,
}

/// A view of each of the atlas's layers, made once for its texture.
#[derive(Resource, Default)]
struct LayerViews {
    texture: Option<TextureId>,
    layers: Vec<TextureView>,
}

fn init(mut commands: Commands) {
    // Each buffer's least size is one of what it lists: a matrix and a
    // draw.
    let layout = BindGroupLayoutDescriptor::new(
        "shadow_tiles",
        &BindGroupLayoutEntries::sequential(
            ShaderStages::VERTEX,
            (
                storage_buffer_read_only_sized(false, NonZero::new(size_of::<Mat4>() as u64)),
                storage_buffer_read_only_sized(false, NonZero::new(size_of::<GpuDraw>() as u64)),
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
    (kind, cut): (Kind, bool),
    pipelines: &Pipelines,
) -> Option<RenderPipelineDescriptor> {
    let mut attributes = vec![Mesh::ATTRIBUTE_POSITION.at_shader_location(0)];
    let mut shader_defs = Vec::new();
    if kind != Kind::Rigid {
        attributes.push(Mesh::ATTRIBUTE_JOINT_INDEX.at_shader_location(1));
        attributes.push(Mesh::ATTRIBUTE_JOINT_WEIGHT.at_shader_location(2));
        shader_defs.push("SKINNED".into());
    }
    if kind == Kind::Baked {
        attributes.push(PLACE_1.at_shader_location(4));
        attributes.push(PLACE_2.at_shader_location(5));
        attributes.push(PLACE_3.at_shader_location(6));
        shader_defs.push("BAKED".into());
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
    draws: &[TileDraw],
    pipelines: &mut Pipelines,
    finder: &Finder,
) -> (Vec<Batch>, Vec<GpuDraw>, usize) {
    let mut waiting = 0;
    let mut sorted = Vec::with_capacity(draws.len());
    for draw in draws {
        let Some(mesh) = finder.meshes.get(draw.mesh) else {
            waiting += 1;
            continue;
        };
        // A cut needs the texture on the GPU and the mesh's UVs. A mesh
        // with none casts whole.
        let mut cut = draw.cut;
        let mut id = None;
        for with_cut in [cut.is_some(), false] {
            let key = (mesh.layout.clone(), draw.kind, with_cut);
            let found = match pipelines.ids.get(&key) {
                Some(&found) => found,
                None => {
                    let found = pipeline(&mesh.layout, (draw.kind, with_cut), pipelines)
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
        sorted.push((id, cut, draw.mesh, [draw.tile, draw.first]));
    }
    // A mesh's draws follow one another, so one call draws them all.
    sorted.sort_unstable_by_key(|&(id, cut, mesh, _)| (id, cut, mesh));
    let mut batches: Vec<Batch> = Vec::new();
    let mut gpu = Vec::with_capacity(sorted.len());
    for (id, cut, mesh, draw) in sorted {
        let index = gpu.len() as u32;
        match batches.last_mut() {
            Some(batch) if batch.pipeline == id && batch.cut == cut && batch.mesh == mesh => {
                batch.instances.end = index + 1;
            }
            _ => {
                // Its mesh was on the GPU a moment ago.
                let Some(layout) = finder.meshes.get(mesh).map(|mesh| mesh.layout.clone()) else {
                    continue;
                };
                batches.push(Batch {
                    pipeline: id,
                    cut,
                    mesh,
                    layout,
                    instances: index..index + 1,
                });
            }
        }
        gpu.push(draw);
    }
    (batches, gpu, waiting)
}

/// Makes the characters' draw calls when their draws have changed, and
/// writes the matrices for the shaders when any of them has. Makes and
/// writes the kept layer's when a new plan is ready.
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
    let gpu = (&*cache, &*device, &*queue);
    let ready = &mut *ready;
    let jobs = &mut *jobs;
    if STALE.swap(false, Ordering::Relaxed) {
        ready.made_of = None;
    }
    ready.live = false;
    ready.still = false;
    if !jobs.draws.is_empty() {
        let remade = ready.made_of != Some(jobs.generation);
        if remade {
            let (batches, draws, waiting) = batches_of(&jobs.draws, &mut pipelines, &finder);
            ready.layer.take(batches, draws);
            ready.made_of = (waiting == 0).then_some(jobs.generation);
            REMADE.fetch_add(1, Ordering::Relaxed);
        }
        if !ready.layer.batches.is_empty() {
            ready.live = true;
            // The same draws and matrices as the frame before: the GPU has
            // them, and the layer has their picture.
            ready.still = !remade && *ready.layer.matrices.values() == jobs.matrices;
            if ready.still {
                STILL.fetch_add(1, Ordering::Relaxed);
            } else {
                ready.layer.pose(&mut jobs.matrices);
                ready.layer.write(remade, &pipelines, gpu);
            }
        }
    }
    let kept = &mut *kept;
    if let Some(waiting) = &mut kept.waiting {
        let (batches, draws, unready) = batches_of(&waiting.draws, &mut pipelines, &finder);
        if unready == 0 || kept.waited >= KEPT_WAIT {
            kept.layer.take(batches, draws);
            kept.layer.pose(&mut waiting.matrices);
            kept.layer.write(true, &pipelines, gpu);
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
}

/// Draws `layer`'s batches into the atlas's layer `view`, cleared first.
/// Returns the draw calls made.
fn draw_layer(ctx: &mut RenderContext, view: &TextureView, layer: &Layer, drawer: &Drawer) -> u64 {
    // The tracked pass skips a pipeline, binding or buffer already set.
    // Most meshes share a few of the allocator's buffers.
    let mut pass = ctx.begin_tracked_render_pass(RenderPassDescriptor {
        label: Some("shadow_tiles"),
        color_attachments: &[],
        depth_stencil_attachment: Some(RenderPassDepthStencilAttachment {
            view,
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
    let Some((bindings, _)) = &layer.bindings else {
        return 0;
    };
    if layer.batches.is_empty() {
        return 0;
    }
    pass.set_bind_group(0, bindings, &[]);
    let mut calls = 0u64;
    for batch in &layer.batches {
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
        // The pipeline was made for the layout the mesh had. A mesh
        // written anew with another layout is left out this frame, and the
        // draw calls are made anew for the next.
        if mesh.layout != batch.layout {
            STALE.store(true, Ordering::Relaxed);
            continue;
        }
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
    mut ctx: RenderContext,
) {
    let started = Instant::now();
    let Some(atlas) = images.get(&ATLAS) else {
        DRAW.add(started);
        return;
    };
    // A new atlas holds no kept tiles, and no picture of the characters.
    let new_atlas = views.texture != Some(atlas.texture.id());
    if new_atlas {
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
    let characters = ready.live && (!ready.still || new_atlas);
    if !characters && !kept.redraw {
        DRAW.add(started);
        return;
    }
    let drawer = Drawer {
        pipelines: &pipelines,
        cache: &cache,
        meshes: &meshes,
        allocator: &allocator,
    };
    let diagnostics = ctx.diagnostic_recorder();
    let diagnostics = diagnostics.as_deref();
    let time_span = diagnostics.time_span(ctx.command_encoder(), "shadow_tiles");
    if characters {
        let calls = draw_layer(
            &mut ctx,
            &views.layers[CHARACTERS_LAYER as usize],
            &ready.layer,
            &drawer,
        );
        CALLS.fetch_add(calls, Ordering::Relaxed);
        CALL_FRAMES.fetch_add(1, Ordering::Relaxed);
    }
    if kept.redraw {
        let calls = draw_layer(
            &mut ctx,
            &views.layers[KEPT_LAYER as usize],
            &kept.layer,
            &drawer,
        );
        KEPT_CALLS.store(calls, Ordering::Relaxed);
        KEPT_DRAWN.fetch_add(1, Ordering::Relaxed);
        kept.redraw = false;
    }
    time_span.end(ctx.command_encoder());
    DRAW.add(started);
}
