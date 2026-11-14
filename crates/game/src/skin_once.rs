//! Skin once. A compute pass places each character's skinned vertices in
//! the world once a frame, and writes them to one buffer (`SkinOnce`'s
//! `skinned`). What draws the character afterwards reads that buffer, and
//! does not skin it again.
//!
//! Its first reader is the shadow tiles' pass (`shadow_tiles`). A character
//! has a tile under each of up to three lights, and its meshes were skinned
//! in the vertex shader once for each tile. Now they are skinned once, and
//! each tile draws the placed vertices. The main pass still skins a
//! character with Bevy's own shader. The casting characters' acceleration
//! structures are built from the same buffer (`ray_structures`): a vertex
//! is four floats, and its first three are its place in Bevy world space.
//!
//! A job is one mesh under one skin: a character's shadow mesh
//! (`shadow_tiles::bake`), or one of its skinned meshes drawn by itself.
//! The pass reads the mesh's vertices from Bevy's own vertex buffers, so
//! those are made with storage usage (`build`). It reads the joint matrices
//! from the buffer the tile pass already writes each frame. All jobs whose
//! meshes lie in one of Bevy's buffers run in one dispatch.
//!
//! The pass is off unless `KJK_SKIN_ONCE=on` is set, or the acceleration
//! structures are traced (`ray_structures::RayQueries::traced`, Shadows at
//! Ultra). Its pipeline is made when the structures are available, so it is
//! ready when Ultra is chosen. While the structures are traced, its jobs are
//! every casting character's meshes, and no tile is drawn. With the pass
//! off, the tile pass skins each mesh in its own vertex shader, once for
//! each tile. On an RTX 4070 Ti SUPER the pass costs more than it saves: the tile pass's steps
//! take 4 to 7 microseconds a frame more on the CPU, and 0.01 to 0.03 ms
//! more on the GPU (`docs/LIGHTING-PLAN.md`, step 4). The switch lets a
//! bench compare the two in one build.

use std::num::NonZero;
use std::sync::atomic::{AtomicU64, Ordering};

use bevy::asset::{load_internal_asset, uuid_handle};
use bevy::mesh::{MeshVertexAttribute, MeshVertexBufferLayoutRef};
use bevy::prelude::*;
use bevy::render::RenderApp;
use bevy::render::mesh::RenderMesh;
use bevy::render::mesh::allocator::{MeshAllocator, MeshAllocatorSettings};
use bevy::render::render_asset::RenderAssets;
use bevy::render::render_resource::binding_types::{
    storage_buffer_read_only_sized, storage_buffer_sized,
};
use bevy::render::render_resource::{
    BindGroup, BindGroupEntries, BindGroupLayoutDescriptor, BindGroupLayoutEntries,
    BindingResource, Buffer, BufferBinding, BufferDescriptor, BufferId, BufferUsages,
    CachedComputePipelineId, CachedPipelineState, CommandEncoder, ComputePassDescriptor,
    ComputePipelineDescriptor, PipelineCache, RawBufferVec, ShaderStages, VertexFormat,
};
use bevy::render::renderer::{RenderDevice, RenderQueue};

use crate::shadow_tiles::{PLACE_1, PLACE_2, PLACE_3};

const SHADER: Handle<Shader> = uuid_handle!("b7c41e59-8a2d-4d36-9f05-2e61c8a7d413");

/// How many vertices one workgroup places (`skin_once.wgsl`'s `GROUP`).
const GROUP: u32 = 64;

/// A placed vertex's size in `skinned`: four floats.
pub(crate) const PLACED: u64 = 16;

/// No such attribute, as the shader reads it.
const NONE: u32 = u32::MAX;

/// How many jobs the pass has, and how many vertices they place, for the
/// dump (`shadow_tiles::gather`).
pub(crate) static JOBS: AtomicU64 = AtomicU64::new(0);
pub(crate) static VERTICES: AtomicU64 = AtomicU64::new(0);

/// Whether `KJK_SKIN_ONCE=on` asks for the pass.
fn asked() -> bool {
    static ASKED: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ASKED.get_or_init(|| std::env::var("KJK_SKIN_ONCE").as_deref() == Ok("on"))
}

/// Whether the pass's pipeline is made: `KJK_SKIN_ONCE=on` asks for the
/// pass, or the acceleration structures are available. It holds once the
/// plugins have finished (`ray_structures::RayStructuresPlugin`).
pub(crate) fn wanted() -> bool {
    asked() || crate::ray_structures::available()
}

/// Loads the pass's shader. With the pass asked for, has Bevy make its
/// vertex buffers with storage usage, so the pass can read them. This must
/// run before Bevy makes its mesh allocator, which is when the plugins
/// finish. With the structures available, their plugin adds the usage.
pub(crate) fn build(app: &mut App) {
    load_internal_asset!(app, SHADER, "skin_once.wgsl", Shader::from_wgsl);
    if !asked() {
        return;
    }
    if let Some(render_app) = app.get_sub_app_mut(RenderApp) {
        render_app
            .world_mut()
            .get_resource_or_init::<MeshAllocatorSettings>()
            .extra_buffer_usages |= BufferUsages::STORAGE;
    }
}

/// Where a job's vertex holds what the pass reads, in 32-bit words: the
/// vertex's size, then its position, its joints, its weights and a shadow
/// mesh's three other positions, each from the vertex's start.
#[derive(Clone, Copy, PartialEq, Eq)]
struct Reads {
    stride: u32,
    position: u32,
    indices: u32,
    weights: u32,
    places: [u32; 3],
}

/// What the pass reads of a vertex of `layout`. None when the mesh lacks
/// one of them, or holds it in another format than the shader reads.
fn reads_of(layout: &MeshVertexBufferLayoutRef, baked: bool) -> Option<Reads> {
    let its = layout.0.layout();
    let word = |attribute: &MeshVertexAttribute, format: VertexFormat| -> Option<u32> {
        let at = layout
            .0
            .attribute_ids()
            .iter()
            .position(|&id| id == attribute.id)?;
        let found = &its.attributes[at];
        (found.format == format && found.offset.is_multiple_of(4))
            .then_some((found.offset / 4) as u32)
    };
    if !its.array_stride.is_multiple_of(4) {
        return None;
    }
    let places = if baked {
        [
            word(&PLACE_1, VertexFormat::Float32x3)?,
            word(&PLACE_2, VertexFormat::Float32x3)?,
            word(&PLACE_3, VertexFormat::Float32x3)?,
        ]
    } else {
        [NONE; 3]
    };
    Some(Reads {
        stride: (its.array_stride / 4) as u32,
        position: word(&Mesh::ATTRIBUTE_POSITION, VertexFormat::Float32x3)?,
        indices: word(&Mesh::ATTRIBUTE_JOINT_INDEX, VertexFormat::Uint16x4)?,
        weights: word(&Mesh::ATTRIBUTE_JOINT_WEIGHT, VertexFormat::Float32x4)?,
        places,
    })
}

/// One mesh under one skin. `first` is its first matrix among the tile
/// pass's matrices, and `target` its first vertex in `skinned`. `layout`
/// and `count` are the mesh's as the job was made. `source` is where its
/// vertices lie this frame: which of Bevy's buffers, and its first vertex
/// there. None while the mesh is gone or is no longer as the job was made.
struct Job {
    mesh: AssetId<Mesh>,
    layout: MeshVertexBufferLayoutRef,
    reads: Reads,
    first: u32,
    count: u32,
    target: u32,
    source: Option<(BufferId, u32)>,
}

/// A job as the shader reads it (`skin_once.wgsl`'s `Job`).
type GpuJob = [u32; 12];

impl Job {
    fn gpu(&self) -> GpuJob {
        let Some((_, start)) = self.source else {
            return [0; 12];
        };
        let reads = self.reads;
        [
            start * reads.stride,
            reads.stride,
            self.count,
            self.target,
            self.first,
            reads.position,
            reads.indices,
            reads.weights,
            reads.places[0],
            reads.places[1],
            reads.places[2],
            0,
        ]
    }
}

/// The jobs whose meshes lie in one of Bevy's vertex buffers: that buffer
/// and their workgroups bound, and how many workgroups they are.
struct Run {
    bindings: BindGroup,
    groups: u32,
}

/// Whether the pass can take jobs now.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum State {
    /// It is turned off, or its pipeline failed.
    Off,
    /// Its pipeline is still compiling.
    Compiling,
    Ready,
}

/// The skin-once pass: its jobs and what they read and write on the GPU.
#[derive(Resource)]
pub(crate) struct SkinOnce {
    shared_layout: BindGroupLayoutDescriptor,
    source_layout: BindGroupLayoutDescriptor,
    /// None with the pass turned off.
    pipeline: Option<CachedComputePipelineId>,
    /// Whether the pass is used this frame (`set_used`). With the pipeline
    /// made but the pass unused, the tile pass skins in its own shader.
    used: bool,
    jobs: Vec<Job>,
    /// How many vertices the jobs place.
    vertices: u32,
    /// The jobs were made anew since the GPU's lists were written.
    planned: bool,
    /// Bevy wrote or dropped a mesh since the jobs' vertices were last
    /// looked for (`stir`), so they may have moved.
    stirred: bool,
    /// Every job had its vertices when they were last looked for.
    whole: bool,
    /// The largest buffer one binding may hold, in bytes.
    largest: u64,
    /// A binding must start at a multiple of this many workgroups in
    /// `groups`.
    step: usize,
    gpu_jobs: RawBufferVec<GpuJob>,
    /// Each workgroup's job and which of the job's workgroups it is. Each
    /// run's workgroups follow one another.
    groups: RawBufferVec<u32>,
    /// The placed vertices, four floats each.
    skinned: Option<Buffer>,
    /// The matrices, the jobs and `skinned` bound, and the buffers the
    /// bindings were made of.
    shared: Option<(BindGroup, [BufferId; 3])>,
    runs: Vec<Run>,
}

/// Makes the pass's bindings and queues its pipeline.
pub(crate) fn init(mut commands: Commands, cache: Res<PipelineCache>, device: Res<RenderDevice>) {
    let limits = device.limits();
    let shared_layout = BindGroupLayoutDescriptor::new(
        "skin_once",
        &BindGroupLayoutEntries::sequential(
            ShaderStages::COMPUTE,
            (
                storage_buffer_read_only_sized(false, NonZero::new(size_of::<Mat4>() as u64)),
                storage_buffer_read_only_sized(false, NonZero::new(size_of::<GpuJob>() as u64)),
                storage_buffer_sized(false, NonZero::new(PLACED)),
            ),
        ),
    );
    let source_layout = BindGroupLayoutDescriptor::new(
        "skin_once_source",
        &BindGroupLayoutEntries::sequential(
            ShaderStages::COMPUTE,
            (
                storage_buffer_read_only_sized(false, NonZero::new(4)),
                storage_buffer_read_only_sized(false, NonZero::new(4)),
            ),
        ),
    );
    let pipeline = wanted().then(|| {
        cache.queue_compute_pipeline(ComputePipelineDescriptor {
            label: Some("skin_once".into()),
            layout: vec![shared_layout.clone(), source_layout.clone()],
            immediate_size: 0,
            shader: SHADER,
            shader_defs: Vec::new(),
            entry_point: Some("skin".into()),
            zero_initialize_workgroup_memory: false,
        })
    });
    commands.insert_resource(SkinOnce {
        shared_layout,
        source_layout,
        pipeline,
        used: asked(),
        jobs: Vec::new(),
        vertices: 0,
        planned: false,
        stirred: false,
        whole: true,
        largest: limits.max_storage_buffer_binding_size,
        step: (limits.min_storage_buffer_offset_alignment as usize / 4).max(1),
        gpu_jobs: RawBufferVec::new(BufferUsages::STORAGE),
        groups: RawBufferVec::new(BufferUsages::STORAGE),
        skinned: None,
        shared: None,
        runs: Vec::new(),
    });
}

/// Whether the pass can read `buffer`: it was made with storage usage, and
/// is no larger than `largest`, the most one binding may hold.
fn readable(buffer: &Buffer, largest: u64) -> bool {
    buffer.usage().contains(BufferUsages::STORAGE) && buffer.size() <= largest
}

impl SkinOnce {
    /// Uses the pass this frame when `KJK_SKIN_ONCE=on` asks for it, or
    /// when the acceleration structures are `traced`.
    pub(crate) fn set_used(&mut self, traced: bool) {
        self.used = asked() || traced;
    }

    pub(crate) fn state(&self, cache: &PipelineCache) -> State {
        let Some(id) = self.pipeline.filter(|_| self.used) else {
            return State::Off;
        };
        match cache.get_compute_pipeline_state(id) {
            CachedPipelineState::Ok(_) => State::Ready,
            CachedPipelineState::Err(_) => State::Off,
            _ => State::Compiling,
        }
    }

    /// Forgets the jobs. The tile pass then names the ones it draws (`job`).
    pub(crate) fn clear(&mut self) {
        self.jobs.clear();
        self.vertices = 0;
        self.planned = true;
    }

    /// Notes that Bevy wrote or dropped a mesh this frame. The jobs'
    /// vertices are then looked for again before they are next placed.
    pub(crate) fn stir(&mut self) {
        self.stirred = true;
    }

    /// Forgets the jobs when no tile is drawn, so the dump does not count
    /// the last ones.
    pub(crate) fn rest(&mut self) {
        if !self.jobs.is_empty() {
            self.clear();
            self.runs.clear();
            JOBS.store(0, Ordering::Relaxed);
            VERTICES.store(0, Ordering::Relaxed);
        }
    }

    /// The job that places the mesh `id` by the matrices from `first` on,
    /// made if there is none yet: its first vertex in `skinned`. `baked`
    /// says the mesh is a shadow mesh. None when the pass cannot read the
    /// mesh. The caller then skins it in its own shader.
    pub(crate) fn job(
        &mut self,
        (id, first, baked): (AssetId<Mesh>, u32, bool),
        (meshes, allocator): (&RenderAssets<RenderMesh>, &MeshAllocator),
    ) -> Option<u32> {
        let mesh = meshes.get(id)?;
        let reads = reads_of(&mesh.layout, baked)?;
        if let Some(job) = self
            .jobs
            .iter()
            .find(|job| (job.mesh, job.first, job.reads) == (id, first, reads))
        {
            return Some(job.target);
        }
        let slice = allocator.mesh_vertex_slice(&id)?;
        // A workgroup names its job and its place in the job in 16 bits
        // each.
        if !readable(slice.buffer, self.largest)
            || self.jobs.len() >= 1 << 16
            || mesh.vertex_count.div_ceil(GROUP) > 1 << 16
        {
            return None;
        }
        let target = self.vertices;
        self.jobs.push(Job {
            mesh: id,
            layout: mesh.layout.clone(),
            reads,
            first,
            count: mesh.vertex_count,
            target,
            source: None,
        });
        self.vertices += mesh.vertex_count;
        self.planned = true;
        Some(target)
    }

    /// The placed vertices.
    pub(crate) fn skinned(&self) -> Option<&Buffer> {
        self.skinned.as_ref()
    }

    /// Readies the jobs for this frame. When the jobs are new, or Bevy has
    /// written a mesh since they were last looked at (`stir`), finds where
    /// each job's vertices lie, and writes the jobs for the shader when
    /// any of them has moved. Bevy moves a mesh's vertices only when it
    /// writes a mesh. `matrices` is the tile pass's buffer of matrices.
    /// Returns false when a job's mesh is gone or is no longer as the job
    /// was made. That job is left out, and the caller makes its jobs anew.
    pub(crate) fn refresh(
        &mut self,
        (meshes, allocator): (&RenderAssets<RenderMesh>, &MeshAllocator),
        matrices: Option<&Buffer>,
        (cache, device, queue): (&PipelineCache, &RenderDevice, &RenderQueue),
    ) -> bool {
        let mut moved = std::mem::take(&mut self.planned);
        // A mesh's vertices move only when Bevy writes a mesh.
        if moved || std::mem::take(&mut self.stirred) {
            self.whole = true;
            for job in &mut self.jobs {
                let source = meshes
                    .get(job.mesh)
                    .filter(|mesh| mesh.layout == job.layout && mesh.vertex_count == job.count)
                    .and_then(|_| allocator.mesh_vertex_slice(&job.mesh))
                    .filter(|slice| readable(slice.buffer, self.largest))
                    .map(|slice| (slice.buffer.id(), slice.range.start));
                self.whole &= source.is_some();
                moved |= source != job.source;
                job.source = source;
            }
        }
        let whole = self.whole;
        JOBS.store(self.jobs.len() as u64, Ordering::Relaxed);
        VERTICES.store(u64::from(self.vertices), Ordering::Relaxed);
        if moved {
            self.write(allocator, (cache, device, queue));
        }
        // The tile pass makes its buffer of matrices again when it grows.
        let (Some(matrices), Some(jobs), Some(skinned)) =
            (matrices, self.gpu_jobs.buffer(), self.skinned.as_ref())
        else {
            self.shared = None;
            return whole;
        };
        let made_of = [matrices.id(), jobs.id(), skinned.id()];
        if !self.shared.as_ref().is_some_and(|(_, of)| *of == made_of) {
            let bindings = device.create_bind_group(
                "skin_once",
                &cache.get_bind_group_layout(&self.shared_layout),
                &BindGroupEntries::sequential((
                    matrices.as_entire_binding(),
                    jobs.as_entire_binding(),
                    skinned.as_entire_binding(),
                )),
            );
            self.shared = Some((bindings, made_of));
        }
        whole
    }

    /// Writes the jobs and their workgroups for the shader, makes `skinned`
    /// large enough, and binds each run's vertex buffer and workgroups.
    fn write(
        &mut self,
        allocator: &MeshAllocator,
        (cache, device, queue): (&PipelineCache, &RenderDevice, &RenderQueue),
    ) {
        self.runs.clear();
        // The buffers the jobs read, each with one of its meshes.
        let mut sources: Vec<(BufferId, AssetId<Mesh>)> = Vec::new();
        for job in &self.jobs {
            if let Some((buffer, _)) = job.source
                && !sources.iter().any(|&(held, _)| held == buffer)
            {
                sources.push((buffer, job.mesh));
            }
        }
        // A run's workgroups are bound from their first on, and a binding
        // must start at a multiple of this many of them.
        let step = self.step;
        let groups = self.groups.values_mut();
        groups.clear();
        let mut ranges = Vec::with_capacity(sources.len());
        for &(buffer, _) in &sources {
            groups.resize(groups.len().next_multiple_of(step), 0);
            let start = groups.len();
            for (index, job) in self.jobs.iter().enumerate() {
                if job.source.is_some_and(|(held, _)| held == buffer) {
                    let its = job.count.div_ceil(GROUP);
                    groups.extend((0..its).map(|group| index as u32 | group << 16));
                }
            }
            ranges.push(start..groups.len());
        }
        *self.gpu_jobs.values_mut() = self.jobs.iter().map(Job::gpu).collect();
        if self.gpu_jobs.is_empty() || self.groups.is_empty() {
            return;
        }
        // Each buffer is made with room to spare, so it is not made again
        // each time its list grows by one.
        let room = self.gpu_jobs.len().next_power_of_two();
        self.gpu_jobs.reserve(room, device);
        self.gpu_jobs.write_buffer(device, queue);
        let room = self.groups.len().next_power_of_two();
        self.groups.reserve(room, device);
        self.groups.write_buffer(device, queue);
        let needed = u64::from(self.vertices.max(1)) * PLACED;
        if self
            .skinned
            .as_ref()
            .is_none_or(|held| held.size() < needed)
        {
            self.skinned = Some(device.create_buffer(&BufferDescriptor {
                label: Some("skin_once"),
                size: needed.next_power_of_two(),
                // The characters' acceleration structures are built from
                // it.
                usage: if crate::ray_structures::available() {
                    BufferUsages::STORAGE | BufferUsages::VERTEX | BufferUsages::BLAS_INPUT
                } else {
                    BufferUsages::STORAGE | BufferUsages::VERTEX
                },
                mapped_at_creation: false,
            }));
        }
        let Some(groups) = self.groups.buffer() else {
            return;
        };
        let layout = cache.get_bind_group_layout(&self.source_layout);
        for (&(_, mesh), range) in sources.iter().zip(ranges) {
            let (Some(slice), Some(size)) = (
                allocator.mesh_vertex_slice(&mesh),
                NonZero::new(4 * range.len() as u64),
            ) else {
                continue;
            };
            let bindings = device.create_bind_group(
                "skin_once_source",
                &layout,
                &BindGroupEntries::sequential((
                    slice.buffer.as_entire_binding(),
                    BindingResource::Buffer(BufferBinding {
                        buffer: groups,
                        offset: 4 * range.start as u64,
                        size: Some(size),
                    }),
                )),
            );
            self.runs.push(Run {
                bindings,
                groups: range.len() as u32,
            });
        }
    }

    /// Places every job's vertices: one dispatch for each of Bevy's vertex
    /// buffers the jobs read.
    pub(crate) fn run(&self, encoder: &mut CommandEncoder, cache: &PipelineCache) {
        let (Some(id), Some((shared, _))) = (self.pipeline, &self.shared) else {
            return;
        };
        let Some(pipeline) = cache.get_compute_pipeline(id) else {
            return;
        };
        if self.runs.is_empty() {
            return;
        }
        let mut pass = encoder.begin_compute_pass(&ComputePassDescriptor {
            label: Some("skin_once"),
            timestamp_writes: None,
        });
        pass.set_pipeline(pipeline);
        pass.set_bind_group(0, &**shared, &[]);
        for run in &self.runs {
            pass.set_bind_group(1, &*run.bindings, &[]);
            pass.dispatch_workgroups(run.groups, 1, 1);
        }
    }
}
