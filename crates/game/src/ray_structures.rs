//! The scene's acceleration structures (`docs/LIGHTING-PLAN.md`, steps 6c
//! and 6d). A shader traces rays against them with a ray query. Under
//! Shadows at Ultra, each light's shadow is one ray toward the light
//! (`odyssey.wgsl`'s `ray_visibility`). The debug view `KJK_SHADOW_DEBUG=5`
//! also traces rays against them (`ray_debug`).
//!
//! The structures can be built on a card that offers ray queries
//! (`WgpuFeatures::EXPERIMENTAL_RAY_QUERY`). The Odyssey material's layout
//! and Bevy's buffers are then made for them when the plugins finish
//! (`available`). They are built and traced only while Shadows is at Ultra
//! and the anti-aliasing is TAA or DLSS (`RayQueries::traced`). Under MSAA,
//! Ultra draws as High does. `KJK_RAYS=on` makes a run use Ultra whatever
//! Shadows says. With `KJK_RAYS=off` the render device is made without ray
//! queries (`runtime`), so a run acts as a card that has none, and Ultra
//! draws as High does.
//!
//! A bottom-level structure holds triangles. There are four kinds.
//!
//! - A casting character's skinned mesh or shadow mesh gets one built from
//!   the vertices the skin-once pass places in the world (`skin_once`). It
//!   is built anew on each frame in which a character moved, because wgpu
//!   29 cannot refit a structure. The tile pass lists these meshes
//!   (`shadow_tiles::RayPiece`).
//! - A mesh with no skin that moves gets one of its own, built once from
//!   Bevy's own vertex buffer in the mesh's own frame. A character's weapon
//!   or helmet is such a mesh. So is a placeable's or a door's mesh while it
//!   moves or for `SETTLE_FRAMES` frames after it appeared or stopped. Their
//!   instances carry the mesh's place in the world, written each frame.
//! - The placeables' casting meshes that stand still share merged
//!   structures (`MergedProps`), and the doors' meshes that stand still
//!   share merged structures of their own. Each mesh is one geometry of a
//!   merged structure, placed in the world by its own matrix. A merged
//!   structure is built again only when a mesh in it goes, moves or changes
//!   its mesh, or when meshes that stood still for `SETTLE_FRAMES` frames
//!   are not in it yet. A door that opens makes its doors' structure be
//!   built twice, as it starts and after it stops. The placeables' structure
//!   is not built again then.
//! - An area's room meshes that write depth share merged structures too,
//!   with a geometry for each mesh (`RayRoom`, `MergedRooms`). Skies,
//!   backdrops and see-through meshes are left out (`scene`'s
//!   `Spawner::walled`). The merged structures are built again only once
//!   the list of room meshes has stayed the same for `STABLE_FRAMES`
//!   frames. The old ones are traced meanwhile. When no mesh of the old ones
//!   is listed any more, the area was left, and they are dropped at once.
//!
//! wgpu 29 walks every instance of the top-level structure each time a pass
//! sets a bind group that holds it, at the frame's submit. The Odyssey
//! material's bind group holds it, and a frame sets a few hundred of them.
//! So the count of instances is what the merged structures keep small.
//!
//! One top-level structure holds an instance of each, and is built each
//! frame. An instance's 24-bit number holds its owner's model number in
//! bits 0 to 9, the character's place among the casters (`shadows`'s
//! `ShadowList::casters`) in bits 10 to 15, bit 16 for a placeable or a
//! door, and bit 17 for a door. A merged structure of placeables or doors
//! has bit 19, and its low 16 bits tell where its geometries' numbers start
//! in the numbers buffer (`RayStructures::numbers`). Each geometry's number
//! there is what the mesh's own instance would carry. A merged structure of
//! rooms has bit 18. The characters, placeables and doors have the mask 1,
//! and the rooms the mask 2. A lamp's or the sun's ray traces mask 1 alone,
//! so it never meets a room. A fight's light traces mask 3, so a wall stops
//! it (step 6e). The Odyssey material binds the top-level structure at
//! binding 17 and the numbers buffer at binding 18 (`material`).

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use bevy::mesh::skinning::SkinnedMesh;
use bevy::pbr::MeshMaterial3d;
use bevy::platform::collections::{HashMap, HashSet};
use bevy::prelude::*;
use bevy::render::diagnostic::RecordDiagnostics;
use bevy::render::mesh::allocator::{MeshAllocator, MeshAllocatorSettings};
use bevy::render::mesh::{RenderMesh, RenderMeshBufferInfo};
use bevy::render::render_asset::RenderAssets;
use bevy::render::render_resource::{
    AccelerationStructureFlags, AccelerationStructureGeometryFlags,
    AccelerationStructureUpdateMode, Blas, BlasBuildEntry, BlasGeometries,
    BlasGeometrySizeDescriptors, BlasTriangleGeometry, BlasTriangleGeometrySizeDescriptor, Buffer,
    BufferDescriptor, BufferInitDescriptor, BufferUsages, CommandEncoderDescriptor,
    CreateBlasDescriptor, CreateTlasDescriptor, Tlas, TlasInstance, VertexFormat,
};
use bevy::render::renderer::{
    RenderContext, RenderDevice, RenderGraph, RenderGraphSystems, RenderQueue,
};
use bevy::render::settings::WgpuFeatures;
use bevy::render::{Extract, ExtractSchedule, MainWorld, RenderApp, RenderStartup};

use crate::material::OdysseyMaterial;
use crate::shadow_tiles::{ModelMeshes, RayPlaced, Ready};
use crate::skin_once::{self, SkinOnce};

/// The features a card without ray queries lacks. `KJK_RAYS=off` takes
/// them away from the render device as it is made.
pub const RAY_FEATURES: WgpuFeatures = WgpuFeatures::EXPERIMENTAL_RAY_QUERY
    .union(WgpuFeatures::EXPERIMENTAL_RAY_HIT_VERTEX_RETURN)
    .union(WgpuFeatures::EXTENDED_ACCELERATION_STRUCTURE_VERTEX_FORMATS)
    .union(WgpuFeatures::ACCELERATION_STRUCTURE_BINDING_ARRAY);

/// The most instances the top-level structure holds. A mesh past them is
/// left out, and the dump counts it (`dropped`). The structure is made
/// once at this size, because the material's bind groups hold it. The
/// rooms come last, so a room is left out before a character.
const INSTANCES: usize = 4096;

/// The most geometries one merged structure holds. A larger area's rooms
/// get more than one merged structure.
const GEOMETRIES: usize = 1024;

/// The numbers buffer's length. A placeable's or a door's mesh past it keeps
/// an instance of its own.
const NUMBERS: usize = 8192;

/// The doors' numbers start here in the numbers buffer. The placeables'
/// come before them.
const DOOR_NUMBERS_FROM: usize = 6144;

/// The bytes of a geometry's matrix: three rows of four floats.
const MATRIX_BYTES: u64 = 48;

/// The rooms' merged structures are built again only after the list of
/// room meshes has stayed the same for this many frames. An area's rooms
/// come a few at a time while it streams in.
const STABLE_FRAMES: u32 = 30;

/// A placeable's mesh joins the merged structures only after it has stood
/// still for this many frames. The merged structures are built again at
/// most once in this many frames to take in such meshes.
const SETTLE_FRAMES: u32 = 30;

/// A mesh with no skin keeps its structure this many frames after it was
/// last shown. A door or a placeable shown again within them is not built
/// again.
const KEEP_FRAMES: u32 = 600;

/// Bit 16 of an instance's number: a placeable or a door.
const PROP: u32 = 1 << 16;

/// Bit 17 of an instance's number: a door. A door casts no shadow from a
/// lamp or the sun under Ultra, as it gets no box under the other settings.
/// A closed door stops a fight's light (step 6e).
const DOOR: u32 = 1 << 17;

/// Bit 18 of an instance's number: a merged structure of room meshes.
const ROOM: u32 = 1 << 18;

/// Bit 19 of an instance's number: a merged structure of placeables' or
/// doors' meshes. Its low 16 bits are where its geometries' numbers start
/// in the numbers buffer.
const MERGED: u32 = 1 << 19;

/// The mask of the characters', placeables' and doors' instances.
const MASK: u8 = 1;

/// The mask of the rooms' instances. Only a fight's light traces it.
const ROOM_MASK: u8 = 2;

/// What `KJK_RAYS` asks for: Some(true) for `on`, Some(false) for `off`,
/// and None when it is unset.
pub fn asked() -> Option<bool> {
    static ASKED: std::sync::OnceLock<Option<bool>> = std::sync::OnceLock::new();
    *ASKED.get_or_init(|| match std::env::var("KJK_RAYS").as_deref() {
        Ok("on") => Some(true),
        Ok("off") => Some(false),
        _ => None,
    })
}

/// Whether the structures can be built this run. It is set when the
/// plugins finish (`RayStructuresPlugin::finish`). The material's static
/// functions read it, as they cannot read a resource.
static AVAILABLE: AtomicBool = AtomicBool::new(false);

/// Whether the structures can be built this run (`RayQueries::available`).
/// The Odyssey material's layout then has the top-level structure, and a
/// build can read Bevy's buffers.
pub fn available() -> bool {
    AVAILABLE.load(Ordering::Relaxed)
}

/// Whether the card offers ray queries, whether the structures can be
/// built, and whether they are built and traced this frame. It is in the
/// main world and the render world. The first two are set when the plugins
/// finish, after Bevy has made the render device. `traced` is chosen each
/// frame in the main world (`choose`) and copied to the render world.
#[derive(Resource, Clone, Copy, Debug, Default)]
pub struct RayQueries {
    /// The render device has `WgpuFeatures::EXPERIMENTAL_RAY_QUERY`.
    pub offered: bool,
    /// The card offers ray queries and `KJK_RAYS` is not `off`. The options
    /// then offer Shadows at Ultra.
    pub available: bool,
    /// The structures are built and traced this frame. They are available,
    /// Shadows is at Ultra or `KJK_RAYS=on` asks for it, and the
    /// anti-aliasing is TAA or DLSS.
    pub traced: bool,
}

/// On a door's model root. Its meshes get structures as a placeable's do.
#[derive(Component, Clone, Copy, Debug)]
pub struct DoorModel;

/// One casting mesh of a placeable or a door: its mesh, where it is placed,
/// its instance's number, and whether it is opaque. A mesh cut by a
/// texture is not opaque.
#[derive(Clone, Copy, Debug)]
struct PropDraw {
    mesh: AssetId<Mesh>,
    placed: Mat4,
    number: u32,
    opaque: bool,
}

/// The placeables' and doors' meshes the main world lists each frame. The
/// render world swaps it for its own, so the list is never copied.
#[derive(Resource, Default)]
struct RayProps(Vec<PropDraw>);

/// On a room's mesh that is merged into the rooms' acceleration
/// structures, with the room's number (`scene`'s `Spawner::walled`). A wall,
/// a floor or a pillar of it then stops a fight's light under Ultra. Every
/// room mesh is merged alike, so the room's number is not read.
#[derive(Component, Clone, Copy, Debug)]
pub struct RayRoom(pub usize);

/// One room mesh: its mesh, and where it is placed.
#[derive(Clone, Copy, Debug)]
struct RoomDraw {
    mesh: AssetId<Mesh>,
    placed: Mat4,
}

/// The rooms' meshes, listed in the main world while the structures are
/// traced. The list is made again only when a room mesh appears, goes,
/// moves or changes its mesh, and `generation` then counts up. The render
/// world copies the list only when the generation changed.
#[derive(Resource, Default)]
struct RayRooms {
    generation: u64,
    list: Vec<RoomDraw>,
    /// Whether the list was made for a traced frame.
    traced: bool,
}

/// The counts the dump gives (`facts`). The render world writes them.
static CHARACTER_STRUCTURES: AtomicU64 = AtomicU64::new(0);
static STILL_STRUCTURES: AtomicU64 = AtomicU64::new(0);
static INSTANCES_USED: AtomicU64 = AtomicU64::new(0);
/// The bottom-level structures the top-level structure's instances use,
/// each counted once.
static DEPENDENCIES: AtomicU64 = AtomicU64::new(0);
static TRIANGLES: AtomicU64 = AtomicU64::new(0);
static CHARACTER_TRIANGLES: AtomicU64 = AtomicU64::new(0);
static DROPPED: AtomicU64 = AtomicU64::new(0);
static UNFIT: AtomicU64 = AtomicU64::new(0);
static PROPS: AtomicU64 = AtomicU64::new(0);
/// The placeables' meshes and the doors' meshes with an instance of their
/// own, and the characters' rigid meshes.
static PROPS_APART: AtomicU64 = AtomicU64::new(0);
static DOORS_APART: AtomicU64 = AtomicU64::new(0);
static RIGID_MESHES: AtomicU64 = AtomicU64::new(0);
/// The placeables' merged structures, their geometries, and how many times
/// they were built since the run started.
static PROP_STRUCTURES: AtomicU64 = AtomicU64::new(0);
static PROP_GEOMETRIES: AtomicU64 = AtomicU64::new(0);
static PROP_BUILDS: AtomicU64 = AtomicU64::new(0);
/// The same for the doors' merged structures.
static DOOR_STRUCTURES: AtomicU64 = AtomicU64::new(0);
static DOOR_GEOMETRIES: AtomicU64 = AtomicU64::new(0);
static DOOR_BUILDS: AtomicU64 = AtomicU64::new(0);
/// Frames since the last dump, and the frames among them that built the
/// characters' structures anew.
static FRAMES: AtomicU64 = AtomicU64::new(0);
static CHARACTER_BUILDS: AtomicU64 = AtomicU64::new(0);
/// The rooms' merged structures, their geometries and triangles, the room
/// meshes listed, and those that could not be merged.
static ROOM_STRUCTURES: AtomicU64 = AtomicU64::new(0);
static ROOM_GEOMETRIES: AtomicU64 = AtomicU64::new(0);
static ROOM_TRIANGLES: AtomicU64 = AtomicU64::new(0);
static ROOM_MESHES: AtomicU64 = AtomicU64::new(0);
static ROOM_UNFIT: AtomicU64 = AtomicU64::new(0);
/// How many times the rooms' merged structures were built since the run
/// started. An area's rooms are built once as it loads.
static ROOM_BUILDS: AtomicU64 = AtomicU64::new(0);

pub struct RayStructuresPlugin;

impl Plugin for RayStructuresPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<RayQueries>().add_systems(Last, facts);
    }

    /// Asks the render device whether it offers ray queries. With the
    /// structures available, has Bevy make its vertex and index buffers so
    /// that a structure can be built from them. Adds the systems that choose
    /// whether the structures are traced and that build them. Bevy makes its
    /// mesh allocator after this.
    fn finish(&self, app: &mut App) {
        let Some(render_app) = app.get_sub_app_mut(RenderApp) else {
            return;
        };
        let offered = render_app
            .world()
            .get_resource::<RenderDevice>()
            .is_some_and(|device| {
                device
                    .features()
                    .contains(WgpuFeatures::EXPERIMENTAL_RAY_QUERY)
            });
        let queries = RayQueries {
            offered,
            available: offered && asked() != Some(false),
            traced: false,
        };
        AVAILABLE.store(queries.available, Ordering::Relaxed);
        info!(
            offered = queries.offered,
            available = queries.available,
            asked = ?asked(),
            "ray queries"
        );
        render_app.insert_resource(queries);
        if queries.available {
            // The skin-once pass reads the vertex buffers, and the
            // structures are built from them and the index buffers.
            render_app
                .world_mut()
                .get_resource_or_init::<MeshAllocatorSettings>()
                .extra_buffer_usages |= BufferUsages::STORAGE | BufferUsages::BLAS_INPUT;
            render_app
                .add_systems(RenderStartup, init)
                .add_systems(ExtractSchedule, (extract_traced, extract))
                .add_systems(
                    RenderGraph,
                    build
                        .in_set(RenderGraphSystems::Render)
                        .after(crate::shadow_tiles::draw)
                        .before(bevy::core_pipeline::schedule::camera_driver),
                );
        }
        app.insert_resource(queries);
        if queries.available {
            app.init_resource::<RayProps>()
                .init_resource::<RayRooms>()
                .add_systems(
                    PostUpdate,
                    (
                        // Before the shadows' boxes and casters are written.
                        choose.before(bevy::transform::TransformSystems::Propagate),
                        (gather_props, gather_rooms)
                            .after(choose)
                            .after(bevy::transform::TransformSystems::Propagate),
                    ),
                );
        }
    }
}

/// Chooses whether the structures are built and traced this frame. They
/// are when Shadows is at Ultra, or `KJK_RAYS=on` asks for Ultra, and the
/// anti-aliasing blends the frames (TAA or DLSS). Each frame's ray goes to
/// another point of a light's source, and only that blend makes a soft
/// edge of them. Under MSAA, Ultra draws as High does.
fn choose(
    settings: Res<crate::shadows::ShadowSettings>,
    temporal: Option<Res<crate::temporal::Temporal>>,
    mut queries: ResMut<RayQueries>,
) {
    let ultra = *settings == crate::shadows::ShadowSettings::Ultra || asked() == Some(true);
    let blended = temporal.is_some_and(|temporal| temporal.used.temporal());
    let traced = queries.available && ultra && blended;
    if queries.traced != traced {
        info!(traced, "ray-traced shadows");
        queries.traced = traced;
    }
}

/// Copies whether the structures are traced this frame into the render
/// world.
fn extract_traced(main: Extract<Res<RayQueries>>, mut queries: ResMut<RayQueries>) {
    queries.traced = main.traced;
}

/// A mesh with no skin's structure, in the mesh's own frame. `size` is
/// what it was made for. `shown` is the last frame an instance used it.
struct Still {
    size: BlasTriangleGeometrySizeDescriptor,
    blas: Blas,
    built: bool,
    shown: u32,
}

/// A character's mesh's structure, built from the placed vertices. `key`
/// is the mesh and its first vertex among them.
struct Posed {
    key: (AssetId<Mesh>, u32),
    size: BlasTriangleGeometrySizeDescriptor,
    blas: Blas,
    built: bool,
}

/// One geometry of a merged structure. It holds where the mesh's triangles
/// lie in Bevy's buffers and the matrix that places them in the world.
struct Input {
    vertices: Buffer,
    first_vertex: u32,
    stride: u64,
    indices: Buffer,
    first_index: u32,
    size: BlasTriangleGeometrySizeDescriptor,
    placed: [f32; 12],
}

/// One merged structure. `transforms` holds a matrix for each of its
/// geometries. `base` is where its geometries' numbers start in the numbers
/// buffer, for a merged structure of placeables or doors.
struct Part {
    blas: Blas,
    transforms: Buffer,
    geometries: u32,
    triangles: u64,
    base: u32,
}

/// A placeable's mesh as it stands this frame: its mesh, where it is
/// placed, its number, and whether it is opaque. A mesh with the same key on
/// two frames stood still between them.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
struct PropKey {
    mesh: AssetId<Mesh>,
    placed: [u32; 16],
    number: u32,
    opaque: bool,
}

impl PropKey {
    fn of(prop: &PropDraw) -> Self {
        Self {
            mesh: prop.mesh,
            placed: prop.placed.to_cols_array().map(f32::to_bits),
            number: prop.number,
            opaque: prop.opaque,
        }
    }
}

/// The merged structures of one kind: the placeables' or the doors'.
#[derive(Default)]
struct MergedProps {
    parts: Vec<Part>,
    /// The meshes in them.
    members: HashSet<PropKey>,
    /// Each placeable's mesh listed this frame, with the first frame on
    /// which it stood where it stands, and the last frame it was listed.
    standing: HashMap<PropKey, (u32, u32)>,
    /// The frame they were last built on.
    built: u32,
}

/// The rooms' merged structures.
#[derive(Default)]
struct MergedRooms {
    parts: Vec<Part>,
    /// The meshes in them.
    meshes: HashSet<AssetId<Mesh>>,
    /// The generation of the room list they were built from, and how many
    /// of its meshes they hold.
    built: Option<(u64, usize)>,
    /// The same for the list as it stands now, and the frame from which it
    /// has stood so.
    wanted: Option<(u64, usize)>,
    since: u32,
    /// The generation of the room list last looked at.
    seen: u64,
    /// The listed room meshes that could not be merged.
    unfit: usize,
}

/// The structures the rays are traced against.
#[derive(Resource)]
pub struct RayStructures {
    tlas: Tlas,
    /// The number of each geometry of the placeables' and the doors' merged
    /// structures, as the mesh's own instance would carry it. The shader reads it at a hit on a
    /// merged structure (`odyssey.wgsl`'s `ray_number`).
    numbers: Buffer,
    posed: Vec<Posed>,
    still: HashMap<AssetId<Mesh>, Still>,
    /// This frame's placeables and doors (`RayProps`).
    props: Vec<PropDraw>,
    /// The placeables' merged structures, and the doors'.
    merged_props: [MergedProps; 2],
    /// The rooms' meshes (`RayRooms`), and the generation of the list they
    /// were copied from.
    rooms: Vec<RoomDraw>,
    rooms_generation: u64,
    merged_rooms: MergedRooms,
    /// How many instances the top-level structure held last frame.
    used: usize,
    frame: u32,
}

impl RayStructures {
    /// The top-level structure, which the Odyssey material binds.
    pub(crate) fn tlas(&self) -> &Tlas {
        &self.tlas
    }

    /// The numbers of the merged placeables' and doors' geometries, which
    /// the Odyssey material binds.
    pub(crate) fn numbers(&self) -> &Buffer {
        &self.numbers
    }
}

/// Makes the top-level structure and the numbers buffer. Builds the
/// top-level structure empty once, as a structure must be built before a
/// shader may read it.
fn init(mut commands: Commands, device: Res<RenderDevice>, queue: Res<RenderQueue>) {
    let tlas = device.wgpu_device().create_tlas(&CreateTlasDescriptor {
        label: Some("ray_structures"),
        flags: AccelerationStructureFlags::PREFER_FAST_TRACE,
        update_mode: AccelerationStructureUpdateMode::Build,
        max_instances: INSTANCES as u32,
    });
    let numbers = device.create_buffer(&BufferDescriptor {
        label: Some("ray_structures_numbers"),
        size: (NUMBERS * 4) as u64,
        usage: BufferUsages::STORAGE | BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    let mut encoder = device.create_command_encoder(&CommandEncoderDescriptor {
        label: Some("ray_structures_first"),
    });
    encoder.build_acceleration_structures(&[], [&tlas]);
    queue.submit([encoder.finish()]);
    commands.insert_resource(RayStructures {
        tlas,
        numbers,
        posed: Vec::new(),
        still: HashMap::new(),
        props: Vec::new(),
        merged_props: Default::default(),
        rooms: Vec::new(),
        rooms_generation: 0,
        merged_rooms: MergedRooms::default(),
        used: 0,
        frame: 0,
    });
}

/// Lists the casting meshes of the shown placeables and doors, each with
/// where it is placed, while the structures are traced. A placeable casts
/// when it has a shadow box (`shadows::ShadowBox`), which is what gets its
/// kept tiles. A skinned mesh is left out.
#[allow(clippy::type_complexity)]
fn gather_props(
    queries: Res<RayQueries>,
    roots: Query<
        (
            &crate::scene::SceneModel,
            &InheritedVisibility,
            Has<DoorModel>,
        ),
        Or<(With<crate::shadows::ShadowBox>, With<DoorModel>)>,
    >,
    models: Res<ModelMeshes>,
    meshes: Query<
        (
            &Mesh3d,
            &GlobalTransform,
            &InheritedVisibility,
            &MeshMaterial3d<OdysseyMaterial>,
        ),
        Without<SkinnedMesh>,
    >,
    materials: Res<Assets<OdysseyMaterial>>,
    mut props: ResMut<RayProps>,
) {
    props.0.clear();
    if !queries.traced {
        return;
    }
    for (model, shown, door) in &roots {
        if !shown.get() {
            continue;
        }
        let number = (model.index as u32 & 0x3ff) | PROP | if door { DOOR } else { 0 };
        for &entity in models.of(model.index) {
            let Ok((mesh, placed, shown, material)) = meshes.get(entity) else {
                continue;
            };
            if !shown.get() {
                continue;
            }
            // A mesh casts as it does into a kept tile.
            let Some(cut) = materials
                .get(&material.0)
                .and_then(crate::shadow_tiles::casts)
            else {
                continue;
            };
            props.0.push(PropDraw {
                mesh: mesh.id(),
                placed: placed.to_matrix(),
                number,
                opaque: cut.is_none(),
            });
        }
    }
}

/// Lists the rooms' meshes, each with where it is placed, while the
/// structures are traced. The list is made again only on a frame on which
/// a room mesh appeared, went, moved or changed its mesh, or the structures
/// began to be traced. Rooms are listed whether they are shown or not, as a
/// wall of a room the camera cannot see still stops a fight's light. While
/// nothing is traced, the list is empty.
#[allow(clippy::type_complexity)]
fn gather_rooms(
    queries: Res<RayQueries>,
    rooms: Query<(&Mesh3d, &GlobalTransform), With<RayRoom>>,
    changed: Query<
        (),
        (
            With<RayRoom>,
            Or<(Changed<GlobalTransform>, Changed<Mesh3d>)>,
        ),
    >,
    mut removed: RemovedComponents<RayRoom>,
    mut list: ResMut<RayRooms>,
) {
    // Read every frame, so that a room left while nothing was traced is not
    // counted later.
    let gone = removed.read().count() > 0;
    if !queries.traced {
        if list.traced || !list.list.is_empty() {
            list.list.clear();
            list.traced = false;
            list.generation += 1;
        }
        return;
    }
    if list.traced && !gone && changed.is_empty() {
        return;
    }
    let list = &mut *list;
    list.list.clear();
    list.list
        .extend(rooms.iter().map(|(mesh, placed)| RoomDraw {
            mesh: mesh.id(),
            placed: placed.to_matrix(),
        }));
    list.traced = true;
    list.generation += 1;
}

/// Takes this frame's placeables and doors into the render world, and the
/// rooms' meshes when their list was made again.
fn extract(mut main: ResMut<MainWorld>, structures: Option<ResMut<RayStructures>>) {
    let Some(mut structures) = structures else {
        return;
    };
    if let Some(mut theirs) = main.get_resource_mut::<RayProps>() {
        std::mem::swap(&mut structures.props, &mut theirs.0);
    }
    if let Some(theirs) = main.get_resource::<RayRooms>()
        && theirs.generation != structures.rooms_generation
    {
        structures.rooms.clone_from(&theirs.list);
        structures.rooms_generation = theirs.generation;
    }
}

/// `placed` as an instance's transform: three rows of four.
fn rows(placed: Mat4) -> [f32; 12] {
    let rows = placed.transpose();
    let mut out = [0.0; 12];
    out[0..4].copy_from_slice(&rows.x_axis.to_array());
    out[4..8].copy_from_slice(&rows.y_axis.to_array());
    out[8..12].copy_from_slice(&rows.z_axis.to_array());
    out
}

/// A placed vertex is already in the world.
const IDENTITY: [f32; 12] = [1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0];

/// The geometry flags of a mesh. A mesh cut by a texture is not opaque, so
/// a later part can reject a hit on it. Under Ultra every hit comes as a
/// candidate, and such a mesh casts whole (`odyssey.wgsl`'s
/// `ray_visibility`).
fn geometry_flags(opaque: bool) -> AccelerationStructureGeometryFlags {
    if opaque {
        AccelerationStructureGeometryFlags::OPAQUE
    } else {
        AccelerationStructureGeometryFlags::empty()
    }
}

/// Where a mesh's triangles lie in Bevy's buffers, for building its
/// structure from them. None when its position is not the first three
/// floats of its vertex, it has no indices, or a buffer was made without
/// `BLAS_INPUT`.
struct Source<'a> {
    vertices: Option<(&'a Buffer, u32, u64)>,
    indices: (&'a Buffer, u32),
    size: BlasTriangleGeometrySizeDescriptor,
}

/// The mesh `id` as a structure reads it from Bevy's buffers. With
/// `placed`, the vertices are the skin-once pass's instead: that buffer, the
/// mesh's first vertex in it, and how many it has.
fn source_of<'a>(
    id: AssetId<Mesh>,
    (meshes, allocator): (&RenderAssets<RenderMesh>, &'a MeshAllocator),
    placed: Option<(u32, u32)>,
    opaque: bool,
) -> Option<Source<'a>> {
    let mesh = meshes.get(id)?;
    let RenderMeshBufferInfo::Indexed {
        count: index_count,
        index_format,
    } = mesh.buffer_info
    else {
        return None;
    };
    let indices = allocator.mesh_index_slice(&id)?;
    if !indices.buffer.usage().contains(BufferUsages::BLAS_INPUT) {
        return None;
    }
    let (vertices, vertex_count) = match placed {
        Some((_, count)) => (None, count),
        None => {
            let layout = mesh.layout.0.layout();
            let at = mesh
                .layout
                .0
                .attribute_ids()
                .iter()
                .position(|&attribute| attribute == Mesh::ATTRIBUTE_POSITION.id)?;
            let position = &layout.attributes[at];
            if position.offset != 0 || position.format != VertexFormat::Float32x3 {
                return None;
            }
            let slice = allocator.mesh_vertex_slice(&id)?;
            if !slice.buffer.usage().contains(BufferUsages::BLAS_INPUT) {
                return None;
            }
            (
                Some((slice.buffer, slice.range.start, layout.array_stride)),
                mesh.vertex_count,
            )
        }
    };
    Some(Source {
        vertices,
        indices: (indices.buffer, indices.range.start),
        size: BlasTriangleGeometrySizeDescriptor {
            vertex_format: VertexFormat::Float32x3,
            vertex_count,
            index_format: Some(index_format),
            index_count: Some(index_count),
            flags: geometry_flags(opaque),
        },
    })
}

/// A new bottom-level structure for one mesh of `size`.
fn new_blas(device: &RenderDevice, size: &BlasTriangleGeometrySizeDescriptor, fast: bool) -> Blas {
    device.wgpu_device().create_blas(
        &CreateBlasDescriptor {
            label: Some("ray_structures_mesh"),
            flags: if fast {
                AccelerationStructureFlags::PREFER_FAST_BUILD
            } else {
                AccelerationStructureFlags::PREFER_FAST_TRACE
            },
            update_mode: AccelerationStructureUpdateMode::Build,
        },
        BlasGeometrySizeDescriptors::Triangles {
            descriptors: vec![size.clone()],
        },
    )
}

/// The mesh `id` as one geometry of a merged structure, placed by `placed`.
/// None when it cannot have a structure yet, or at all (`source_of`).
fn input_of(
    id: AssetId<Mesh>,
    buffers: (&RenderAssets<RenderMesh>, &MeshAllocator),
    opaque: bool,
    placed: [f32; 12],
) -> Option<Input> {
    let source = source_of(id, buffers, None, opaque)?;
    let (vertices, first_vertex, stride) = source.vertices?;
    Some(Input {
        vertices: vertices.clone(),
        first_vertex,
        stride,
        indices: source.indices.0.clone(),
        first_index: source.indices.1,
        size: source.size,
        placed,
    })
}

/// The most geometries one merged structure may hold on this device.
fn geometries_per(device: &RenderDevice) -> usize {
    GEOMETRIES
        .min(device.limits().max_blas_geometry_count as usize)
        .max(1)
}

/// New merged structures for `inputs`, with at most `per` geometries in
/// each. Each geometry's matrix is written to the structure's transform
/// buffer as the buffer is made. `part_entries` builds the structures.
fn merged_parts(
    device: &RenderDevice,
    inputs: &[Input],
    per: usize,
    label: &'static str,
) -> Vec<Part> {
    inputs
        .chunks(per)
        .enumerate()
        .map(|(at, chunk)| {
            let mut bytes = Vec::with_capacity(chunk.len() * MATRIX_BYTES as usize);
            for input in chunk {
                for value in input.placed {
                    bytes.extend_from_slice(&value.to_le_bytes());
                }
            }
            let transforms = device.create_buffer_with_data(&BufferInitDescriptor {
                label: Some(label),
                contents: &bytes,
                usage: BufferUsages::BLAS_INPUT,
            });
            let blas = device.wgpu_device().create_blas(
                &CreateBlasDescriptor {
                    label: Some(label),
                    flags: AccelerationStructureFlags::PREFER_FAST_TRACE
                        | AccelerationStructureFlags::USE_TRANSFORM,
                    update_mode: AccelerationStructureUpdateMode::Build,
                },
                BlasGeometrySizeDescriptors::Triangles {
                    descriptors: chunk.iter().map(|input| input.size.clone()).collect(),
                },
            );
            Part {
                blas,
                transforms,
                geometries: chunk.len() as u32,
                triangles: chunk
                    .iter()
                    .map(|input| u64::from(input.size.index_count.unwrap_or(0) / 3))
                    .sum(),
                base: (at * per) as u32,
            }
        })
        .collect()
}

/// The entries that build `parts` from `inputs`, as `merged_parts` made
/// them with at most `per` geometries in each.
fn part_entries<'a>(parts: &'a [Part], inputs: &'a [Input], per: usize) -> Vec<BlasBuildEntry<'a>> {
    parts
        .iter()
        .zip(inputs.chunks(per))
        .map(|(part, chunk)| BlasBuildEntry {
            blas: &part.blas,
            geometry: BlasGeometries::TriangleGeometries(
                chunk
                    .iter()
                    .enumerate()
                    .map(|(at, input)| BlasTriangleGeometry {
                        size: &input.size,
                        vertex_buffer: &input.vertices,
                        first_vertex: input.first_vertex,
                        vertex_stride: input.stride,
                        index_buffer: Some(&input.indices),
                        first_index: Some(input.first_index),
                        transform_buffer: Some(&part.transforms),
                        transform_buffer_offset: Some(at as u64 * MATRIX_BYTES),
                    })
                    .collect(),
            ),
        })
        .collect()
}

/// Which of the placeables' or the doors' merged structures a mesh of
/// `number` belongs in: 0 for a placeable's mesh, and 1 for a door's.
fn kind_of(number: u32) -> usize {
    usize::from(number & DOOR != 0)
}

/// Where the numbers of the placeables' (0) or the doors' (1) merged
/// geometries lie in the numbers buffer: the first and the count.
fn region(kind: usize) -> (usize, usize) {
    if kind == 0 {
        (0, DOOR_NUMBERS_FROM)
    } else {
        (DOOR_NUMBERS_FROM, NUMBERS - DOOR_NUMBERS_FROM)
    }
}

/// Keeps the placeables' (`kind` 0) or the doors' (`kind` 1) merged
/// structures up to date with this frame's `props`, and returns the
/// geometries to build when it makes them anew. They are made anew on a
/// frame on which a mesh in them is no longer listed where it stood. A door
/// that opens is such a mesh. They are also made anew when meshes that have
/// stood still for `SETTLE_FRAMES` frames are not in them yet, at most once
/// in `SETTLE_FRAMES` frames. The numbers of the merged geometries are
/// written to the kind's region of the numbers buffer.
fn settle_props(
    (merged, kind): (&mut MergedProps, usize),
    props: &[PropDraw],
    (frame, per): (u32, usize),
    buffers: (&RenderAssets<RenderMesh>, &MeshAllocator),
    (device, queue, numbers): (&RenderDevice, &RenderQueue, &Buffer),
) -> Option<Vec<Input>> {
    let (from, room) = region(kind);
    for prop in props.iter().filter(|prop| kind_of(prop.number) == kind) {
        let standing = merged
            .standing
            .entry(PropKey::of(prop))
            .or_insert((frame, frame));
        standing.1 = frame;
    }
    merged.standing.retain(|_, (_, last)| *last == frame);
    let settled = |first: u32| frame.wrapping_sub(first) >= SETTLE_FRAMES;
    let gone = merged
        .members
        .iter()
        .any(|key| !merged.standing.contains_key(key));
    let waiting = merged.members.len() < room
        && frame.wrapping_sub(merged.built) >= SETTLE_FRAMES
        && merged.standing.iter().any(|(key, &(first, _))| {
            settled(first)
                && !merged.members.contains(key)
                && source_of(key.mesh, buffers, None, key.opaque)
                    .is_some_and(|source| source.vertices.is_some())
        });
    if !gone && !waiting {
        return None;
    }
    let mut members = HashSet::new();
    let mut inputs = Vec::new();
    let mut written = Vec::new();
    for prop in props {
        if kind_of(prop.number) != kind || inputs.len() >= room {
            continue;
        }
        let key = PropKey::of(prop);
        if members.contains(&key)
            || !merged
                .standing
                .get(&key)
                .is_some_and(|&(first, _)| settled(first))
        {
            continue;
        }
        let Some(input) = input_of(prop.mesh, buffers, prop.opaque, rows(prop.placed)) else {
            continue;
        };
        members.insert(key);
        inputs.push(input);
        written.extend_from_slice(&prop.number.to_le_bytes());
    }
    let label = ["ray_structures_placeables", "ray_structures_doors"][kind];
    merged.parts = merged_parts(device, &inputs, per, label);
    for part in &mut merged.parts {
        part.base += from as u32;
    }
    merged.members = members;
    merged.built = frame;
    if !written.is_empty() {
        queue.write_buffer(numbers, (from * 4) as u64, &written);
        [&PROP_BUILDS, &DOOR_BUILDS][kind].fetch_add(1, Ordering::Relaxed);
    }
    Some(inputs)
}

/// Keeps the rooms' merged structures up to date with the room list
/// `rooms` of the generation `generation`, and returns the geometries to
/// build when it makes them anew. They are made anew once the list, and how
/// many of its meshes are on the GPU, have stayed the same for
/// `STABLE_FRAMES` frames. Until then the old ones are traced. When no mesh
/// of the old ones is listed any more, the area was left, and they are
/// dropped at once.
fn settle_rooms(
    merged: &mut MergedRooms,
    (rooms, generation): (&[RoomDraw], u64),
    (frame, per): (u32, usize),
    buffers: (&RenderAssets<RenderMesh>, &MeshAllocator),
    device: &RenderDevice,
) -> Option<Vec<Input>> {
    if merged.seen != generation {
        merged.seen = generation;
        if !merged.meshes.is_empty() && !rooms.iter().any(|room| merged.meshes.contains(&room.mesh))
        {
            merged.parts.clear();
            merged.meshes.clear();
            merged.built = None;
        }
    }
    let complete = merged.built == Some((generation, rooms.len()));
    // A list built while some of its meshes were not on the GPU yet is
    // looked at again once in `STABLE_FRAMES` frames, in case they came.
    let look = merged.built.map(|(built, _)| built) != Some(generation)
        || frame.is_multiple_of(STABLE_FRAMES);
    if complete || !look {
        return None;
    }
    let inputs: Vec<Input> = rooms
        .iter()
        .filter_map(|room| input_of(room.mesh, buffers, true, rows(room.placed)))
        .collect();
    let wanted = (generation, inputs.len());
    if merged.wanted != Some(wanted) {
        merged.wanted = Some(wanted);
        merged.since = frame;
    }
    merged.unfit = rooms.len() - inputs.len();
    if merged.built == Some(wanted) || frame.wrapping_sub(merged.since) < STABLE_FRAMES {
        return None;
    }
    merged.parts = merged_parts(device, &inputs, per, "ray_structures_rooms");
    merged.meshes = rooms.iter().map(|room| room.mesh).collect();
    merged.built = Some(wanted);
    if !inputs.is_empty() {
        ROOM_BUILDS.fetch_add(1, Ordering::Relaxed);
    }
    Some(inputs)
}

/// Builds the structures for this frame, after the tile pass has placed
/// the characters' vertices (`shadow_tiles::draw`). A character's
/// structure is built anew only on a frame in which the skin-once pass
/// placed the vertices again, which is a frame in which a character moved.
/// A mesh with an instance of its own is built the first time it is shown.
/// The merged structures are built when `settle_props` and `settle_rooms`
/// make them anew. The top-level structure is built every frame.
#[allow(clippy::too_many_arguments)]
pub(crate) fn build(
    structures: Option<ResMut<RayStructures>>,
    queries: Res<RayQueries>,
    ready: Option<Res<Ready>>,
    skin: Option<Res<SkinOnce>>,
    (meshes, allocator): (Res<RenderAssets<RenderMesh>>, Res<MeshAllocator>),
    device: Res<RenderDevice>,
    queue: Res<RenderQueue>,
    mut ctx: RenderContext,
) {
    let Some(mut structures) = structures else {
        return;
    };
    let structures = &mut *structures;
    if !queries.traced {
        forget(structures, &mut ctx);
        return;
    }
    structures.frame = structures.frame.wrapping_add(1);
    let frame = structures.frame;
    let per = geometries_per(&device);
    let buffers = (&*meshes, &*allocator);
    let skinned = skin
        .as_deref()
        .and_then(SkinOnce::skinned)
        .filter(|buffer| buffer.usage().contains(BufferUsages::BLAS_INPUT));
    let (pieces, matrices, moved) = match ready.as_deref() {
        Some(ready) => (ready.ray_pieces(), ready.matrices(), ready.moved()),
        None => (&[][..], &[][..], false),
    };
    // Each instance: which structure, where it is placed, and its number.
    enum Of {
        Posed(usize),
        Still(AssetId<Mesh>),
        Props(usize, usize),
        Rooms(usize),
    }
    let mut instances: Vec<(Of, [f32; 12], u32)> = Vec::new();
    let mut unfit = 0u64;
    let mut character_triangles = 0u64;
    let mut rigid = 0u64;
    // The characters' meshes the skin-once pass places.
    let mut posed: Vec<Posed> = Vec::new();
    // Each one's first index in Bevy's index buffer.
    let mut posed_from: Vec<u32> = Vec::new();
    for piece in pieces {
        match piece.placed {
            RayPlaced::Posed { first, count } => {
                let Some(source) = skinned.and_then(|_| {
                    source_of(piece.mesh, buffers, Some((first, count)), piece.opaque)
                }) else {
                    unfit += 1;
                    continue;
                };
                let key = (piece.mesh, first);
                let kept = structures
                    .posed
                    .iter()
                    .position(|held| held.key == key && held.size == source.size)
                    .map(|at| structures.posed.swap_remove(at));
                let held = kept.unwrap_or_else(|| Posed {
                    key,
                    blas: new_blas(&device, &source.size, true),
                    size: source.size,
                    built: false,
                });
                character_triangles += u64::from(held.size.index_count.unwrap_or(0) / 3);
                instances.push((Of::Posed(posed.len()), IDENTITY, piece.number));
                posed_from.push(source.indices.1);
                posed.push(held);
            }
            RayPlaced::Rigid(matrix) => {
                let Some(&placed) = matrices.get(matrix as usize) else {
                    continue;
                };
                if still_structure(
                    (&mut structures.still, frame),
                    piece.mesh,
                    buffers,
                    piece.opaque,
                    &device,
                ) {
                    instances.push((Of::Still(piece.mesh), rows(placed), piece.number));
                    rigid += 1;
                } else {
                    unfit += 1;
                }
            }
        }
    }
    structures.posed = posed;
    // The placeables and doors that stand still are merged, the placeables
    // apart from the doors. The rest get instances of their own.
    let props = std::mem::take(&mut structures.props);
    let prop_inputs = [0, 1].map(|kind| {
        settle_props(
            (&mut structures.merged_props[kind], kind),
            &props,
            (frame, per),
            buffers,
            (&device, &queue, &structures.numbers),
        )
    });
    let mut props_apart = 0u64;
    let mut doors = 0u64;
    for prop in &props {
        let door = prop.number & DOOR != 0;
        if structures.merged_props[kind_of(prop.number)]
            .members
            .contains(&PropKey::of(prop))
        {
            continue;
        }
        if still_structure(
            (&mut structures.still, frame),
            prop.mesh,
            buffers,
            prop.opaque,
            &device,
        ) {
            instances.push((Of::Still(prop.mesh), rows(prop.placed), prop.number));
            if door {
                doors += 1;
            } else {
                props_apart += 1;
            }
        } else {
            unfit += 1;
        }
    }
    for (kind, merged) in structures.merged_props.iter().enumerate() {
        for (at, part) in merged.parts.iter().enumerate() {
            instances.push((Of::Props(kind, at), IDENTITY, MERGED | part.base));
        }
    }
    let structures_props = props.len();
    structures.props = props;
    // The rooms come last.
    let rooms = std::mem::take(&mut structures.rooms);
    let room_inputs = settle_rooms(
        &mut structures.merged_rooms,
        (&rooms, structures.rooms_generation),
        (frame, per),
        buffers,
        &device,
    );
    for at in 0..structures.merged_rooms.parts.len() {
        instances.push((Of::Rooms(at), IDENTITY, ROOM));
    }
    let room_meshes = rooms.len();
    structures.rooms = rooms;
    let dropped = instances.len().saturating_sub(INSTANCES);
    instances.truncate(INSTANCES);
    // What the instances hold, and which structures they still use.
    let mut triangles = 0u64;
    for (of, ..) in &instances {
        match of {
            Of::Still(mesh) => {
                if let Some(still) = structures.still.get_mut(mesh) {
                    still.shown = frame;
                    triangles += u64::from(still.size.index_count.unwrap_or(0) / 3);
                }
            }
            Of::Props(kind, at) => {
                if let Some(part) = structures.merged_props[*kind].parts.get(*at) {
                    triangles += part.triangles;
                }
            }
            Of::Posed(_) | Of::Rooms(_) => {}
        }
    }
    triangles += character_triangles;
    structures
        .still
        .retain(|_, still| frame.wrapping_sub(still.shown) <= KEEP_FRAMES);
    // The structures to build this frame, each with its place in `posed`
    // or its mesh, so that only those built are marked so.
    let rebuild = moved || structures.posed.iter().any(|held| !held.built);
    let (character_built, character_entries): (Vec<usize>, Vec<BlasBuildEntry>) = match skinned {
        Some(skinned) if rebuild => structures
            .posed
            .iter()
            .zip(&posed_from)
            .enumerate()
            .filter_map(|(at, (held, &first_index))| {
                let indices = allocator.mesh_index_slice(&held.key.0)?.buffer;
                Some((
                    at,
                    BlasBuildEntry {
                        blas: &held.blas,
                        geometry: BlasGeometries::TriangleGeometries(vec![BlasTriangleGeometry {
                            size: &held.size,
                            vertex_buffer: skinned,
                            first_vertex: held.key.1,
                            vertex_stride: skin_once::PLACED,
                            index_buffer: Some(indices),
                            first_index: Some(first_index),
                            transform_buffer: None,
                            transform_buffer_offset: None,
                        }]),
                    },
                ))
            })
            .unzip(),
        _ => (Vec::new(), Vec::new()),
    };
    let (still_built, still_entries) = unbuilt(&structures.still, buffers);
    let prop_entries: Vec<BlasBuildEntry> = prop_inputs
        .iter()
        .zip(&structures.merged_props)
        .filter_map(|(inputs, merged)| Some(part_entries(&merged.parts, inputs.as_deref()?, per)))
        .flatten()
        .collect();
    let room_entries = room_inputs
        .as_deref()
        .map(|inputs| part_entries(&structures.merged_rooms.parts, inputs, per))
        .unwrap_or(Vec::new());
    let built_characters = !character_entries.is_empty();
    let diagnostics = ctx.diagnostic_recorder();
    let diagnostics = diagnostics.as_deref();
    let encoder = ctx.command_encoder();
    let span = diagnostics.time_span(encoder, "ray_structures");
    let characters = diagnostics.time_span(encoder, "characters");
    if built_characters {
        encoder.build_acceleration_structures(&character_entries, []);
    }
    characters.end(encoder);
    if !still_entries.is_empty() || !prop_entries.is_empty() {
        let placeables = diagnostics.time_span(encoder, "placeables");
        encoder.build_acceleration_structures(still_entries.iter().chain(&prop_entries), []);
        placeables.end(encoder);
    }
    if !room_entries.is_empty() {
        let rooms = diagnostics.time_span(encoder, "rooms");
        encoder.build_acceleration_structures(&room_entries, []);
        rooms.end(encoder);
    }
    drop(character_entries);
    drop(still_entries);
    drop(prop_entries);
    drop(room_entries);
    for at in character_built {
        if let Some(held) = structures.posed.get_mut(at) {
            held.built = true;
        }
    }
    for mesh in still_built {
        if let Some(still) = structures.still.get_mut(&mesh) {
            still.built = true;
        }
    }
    // An instance whose structure was never built is left out: a mesh
    // whose buffers moved before it was first built, or a character's
    // placed vertices when the pass has not run.
    let count = instances.len();
    let mut used = 0;
    let mut dependencies = 0u64;
    let mut used_stills: HashSet<AssetId<Mesh>> = HashSet::new();
    let RayStructures {
        tlas,
        posed,
        still,
        merged_props,
        merged_rooms,
        used: was_used,
        ..
    } = structures;
    if let Some(slots) = tlas.get_mut_slice(0..count.max(*was_used)) {
        for slot in slots.iter_mut() {
            *slot = None;
        }
        for (of, transform, number) in &instances {
            let mask = if number & ROOM != 0 { ROOM_MASK } else { MASK };
            let blas = match of {
                Of::Posed(at) => posed
                    .get(*at)
                    .filter(|held| held.built)
                    .map(|held| &held.blas),
                Of::Still(mesh) => still
                    .get(mesh)
                    .filter(|still| still.built)
                    .map(|still| &still.blas),
                Of::Props(kind, at) => merged_props[*kind].parts.get(*at).map(|part| &part.blas),
                Of::Rooms(at) => merged_rooms.parts.get(*at).map(|part| &part.blas),
            };
            if let Some(blas) = blas {
                slots[used] = Some(TlasInstance::new(blas, *transform, *number, mask));
                used += 1;
                // A mesh with an instance of its own may be shown twice
                // with one structure.
                if !matches!(of, Of::Still(mesh) if !used_stills.insert(*mesh)) {
                    dependencies += 1;
                }
            }
        }
    }
    *was_used = used;
    let top = diagnostics.time_span(encoder, "top_level");
    encoder.build_acceleration_structures(&[], [&*tlas]);
    top.end(encoder);
    span.end(encoder);
    let geometries = |parts: &[Part]| parts.iter().map(|part| u64::from(part.geometries)).sum();
    CHARACTER_STRUCTURES.store(posed.len() as u64, Ordering::Relaxed);
    STILL_STRUCTURES.store(still.len() as u64, Ordering::Relaxed);
    INSTANCES_USED.store(used as u64, Ordering::Relaxed);
    DEPENDENCIES.store(dependencies, Ordering::Relaxed);
    TRIANGLES.store(triangles, Ordering::Relaxed);
    CHARACTER_TRIANGLES.store(character_triangles, Ordering::Relaxed);
    DROPPED.store(dropped as u64, Ordering::Relaxed);
    UNFIT.store(unfit, Ordering::Relaxed);
    PROPS.store(structures_props as u64, Ordering::Relaxed);
    PROPS_APART.store(props_apart, Ordering::Relaxed);
    DOORS_APART.store(doors, Ordering::Relaxed);
    RIGID_MESHES.store(rigid, Ordering::Relaxed);
    PROP_STRUCTURES.store(merged_props[0].parts.len() as u64, Ordering::Relaxed);
    PROP_GEOMETRIES.store(geometries(&merged_props[0].parts), Ordering::Relaxed);
    DOOR_STRUCTURES.store(merged_props[1].parts.len() as u64, Ordering::Relaxed);
    DOOR_GEOMETRIES.store(geometries(&merged_props[1].parts), Ordering::Relaxed);
    ROOM_STRUCTURES.store(merged_rooms.parts.len() as u64, Ordering::Relaxed);
    ROOM_GEOMETRIES.store(geometries(&merged_rooms.parts), Ordering::Relaxed);
    ROOM_TRIANGLES.store(
        merged_rooms.parts.iter().map(|part| part.triangles).sum(),
        Ordering::Relaxed,
    );
    ROOM_MESHES.store(room_meshes as u64, Ordering::Relaxed);
    ROOM_UNFIT.store(merged_rooms.unfit as u64, Ordering::Relaxed);
    FRAMES.fetch_add(1, Ordering::Relaxed);
    if built_characters {
        CHARACTER_BUILDS.fetch_add(1, Ordering::Relaxed);
    }
}

/// Drops every structure while nothing is traced, so their memory is given
/// back. The top-level structure is built empty once. A frame that traces
/// again builds them all anew.
fn forget(structures: &mut RayStructures, ctx: &mut RenderContext) {
    if structures.used == 0
        && structures.posed.is_empty()
        && structures.still.is_empty()
        && structures
            .merged_props
            .iter()
            .all(|merged| merged.parts.is_empty() && merged.standing.is_empty())
        && structures.merged_rooms.parts.is_empty()
        && structures.merged_rooms.built.is_none()
    {
        return;
    }
    if let Some(slots) = structures.tlas.get_mut_slice(0..structures.used) {
        for slot in slots.iter_mut() {
            *slot = None;
        }
    }
    structures.used = 0;
    structures.posed.clear();
    structures.still.clear();
    structures.props.clear();
    structures.merged_props = Default::default();
    structures.merged_rooms = MergedRooms::default();
    ctx.command_encoder()
        .build_acceleration_structures(&[], [&structures.tlas]);
    for count in [
        &CHARACTER_STRUCTURES,
        &STILL_STRUCTURES,
        &INSTANCES_USED,
        &DEPENDENCIES,
        &TRIANGLES,
        &CHARACTER_TRIANGLES,
        &DROPPED,
        &UNFIT,
        &PROPS,
        &PROPS_APART,
        &DOORS_APART,
        &RIGID_MESHES,
        &PROP_STRUCTURES,
        &PROP_GEOMETRIES,
        &DOOR_STRUCTURES,
        &DOOR_GEOMETRIES,
        &ROOM_STRUCTURES,
        &ROOM_GEOMETRIES,
        &ROOM_TRIANGLES,
        &ROOM_MESHES,
        &ROOM_UNFIT,
    ] {
        count.store(0, Ordering::Relaxed);
    }
}

/// Makes the structure of the mesh `id` with no skin in `held` if there is
/// none yet, to be built this frame. False when the mesh cannot have one.
fn still_structure(
    (held, frame): (&mut HashMap<AssetId<Mesh>, Still>, u32),
    id: AssetId<Mesh>,
    buffers: (&RenderAssets<RenderMesh>, &MeshAllocator),
    opaque: bool,
    device: &RenderDevice,
) -> bool {
    let Some(source) = source_of(id, buffers, None, opaque) else {
        return false;
    };
    // A mesh written anew with another size gets a new structure.
    if let Some(kept) = held.get(&id)
        && kept.size.vertex_count == source.size.vertex_count
        && kept.size.index_count == source.size.index_count
    {
        return true;
    }
    held.insert(
        id,
        Still {
            blas: new_blas(device, &source.size, false),
            size: source.size,
            built: false,
            shown: frame,
        },
    );
    true
}

/// The structures in `held` not built yet, each with its mesh, as entries
/// to build from Bevy's own buffers.
fn unbuilt<'a>(
    held: &'a HashMap<AssetId<Mesh>, Still>,
    buffers: (&RenderAssets<RenderMesh>, &'a MeshAllocator),
) -> (Vec<AssetId<Mesh>>, Vec<BlasBuildEntry<'a>>) {
    held.iter()
        .filter(|(_, still)| !still.built)
        .filter_map(|(&mesh, still)| {
            let source = source_of(mesh, buffers, None, true)?;
            let (vertices, first_vertex, stride) = source.vertices?;
            Some((
                mesh,
                BlasBuildEntry {
                    blas: &still.blas,
                    geometry: BlasGeometries::TriangleGeometries(vec![BlasTriangleGeometry {
                        size: &still.size,
                        vertex_buffer: vertices,
                        first_vertex,
                        vertex_stride: stride,
                        index_buffer: Some(source.indices.0),
                        first_index: Some(source.indices.1),
                        transform_buffer: None,
                        transform_buffer_offset: None,
                    }]),
                },
            ))
        })
        .unzip()
}

/// Gives the dump the structures' counts. `instances` are the top-level
/// structure's instances. At the frame's submit, wgpu walks each of them
/// once for each bind group set that holds the structure.
/// `dependencies` are the bottom-level structures the instances use, each
/// counted once. `character_structures` are the characters' placed meshes'
/// structures, and `still_structures` the structures of meshes with an
/// instance of their own. `triangles` are what the instances hold, without
/// the rooms'. `character_builds` counts the frames since the last dump that
/// built the characters' structures, out of `frames`.
fn facts(queries: Res<RayQueries>, mut facts: ResMut<console::DumpFacts>) {
    if !facts.wanted() {
        return;
    }
    let load = |count: &AtomicU64| count.load(Ordering::Relaxed);
    facts.0.insert(
        "ray_structures".into(),
        serde_json::json!({
            "offered": queries.offered,
            "available": queries.available,
            "traced": queries.traced,
            "instances": load(&INSTANCES_USED),
            "dependencies": load(&DEPENDENCIES),
            "character_structures": load(&CHARACTER_STRUCTURES),
            "still_structures": load(&STILL_STRUCTURES),
            "triangles": load(&TRIANGLES),
            "character_triangles": load(&CHARACTER_TRIANGLES),
            // Instances past `INSTANCES`, and meshes that could not have a
            // structure (no indices, or the position not first in the
            // vertex).
            "dropped": load(&DROPPED),
            "unfit": load(&UNFIT),
            // The placeables' and doors' meshes listed this frame. Of them,
            // `props_apart` placeables' meshes and `doors_apart` doors'
            // meshes have instances of their own, as they moved or appeared
            // within the last `SETTLE_FRAMES` frames. `rigid_meshes` are the
            // characters' weapons and helmets, each with its own instance.
            "props": load(&PROPS),
            "props_apart": load(&PROPS_APART),
            "doors_apart": load(&DOORS_APART),
            "rigid_meshes": load(&RIGID_MESHES),
            // The placeables' merged structures, their geometries, and how
            // many times they were built since the run started.
            "prop_structures": load(&PROP_STRUCTURES),
            "prop_geometries": load(&PROP_GEOMETRIES),
            "prop_builds": load(&PROP_BUILDS),
            // The same for the doors' merged structures.
            "door_structures": load(&DOOR_STRUCTURES),
            "door_geometries": load(&DOOR_GEOMETRIES),
            "door_builds": load(&DOOR_BUILDS),
            "frames": FRAMES.swap(0, Ordering::Relaxed),
            "character_builds": CHARACTER_BUILDS.swap(0, Ordering::Relaxed),
            // The rooms' merged structures, their geometries and triangles,
            // which `triangles` leaves out. `room_meshes` are the room
            // meshes listed, and `room_unfit` those that could not be
            // merged. `room_builds` counts how many times the rooms' merged
            // structures were built since the run started.
            "room_structures": load(&ROOM_STRUCTURES),
            "room_geometries": load(&ROOM_GEOMETRIES),
            "room_triangles": load(&ROOM_TRIANGLES),
            "room_meshes": load(&ROOM_MESHES),
            "room_unfit": load(&ROOM_UNFIT),
            "room_builds": load(&ROOM_BUILDS),
        }),
    );
}
