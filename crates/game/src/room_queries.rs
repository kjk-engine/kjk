//! Occlusion queries on the rooms the culling would draw, so a room hidden
//! behind walls or a closed door is not drawn (issue 464).
//!
//! The culling (`scene::cull_rooms`) picks the rooms the VIS and the fog
//! show from the camera. Each frame, every one of them that is not exempt
//! is tested on the GPU. Its bounding box, grown by `MARGIN`, is drawn
//! against the depth the opaque pass left, without writing anything, inside
//! an occlusion query. The query answers whether any of the box's fragments
//! passed the depth test. The test runs after the opaque pass and before the
//! transparent pass (`main_transparent_pass_3d`), so glass, force fields
//! and other blended surfaces never hide what is behind them. Alpha-tested
//! surfaces write depth only where they are solid, so a grate hides only
//! its bars.
//!
//! wgpu has no conditional drawing, so the answers are read back on the CPU
//! (`map_answers`) and reach the culling one to three frames later. The
//! culling never waits for them. A room keeps its last answer until a newer
//! one arrives, and stays drawn `HOLD` frames after its last visible
//! answer. An answer older than `MAX_AGE` frames hides nothing, so a room
//! is drawn again when the answers stop coming. A room that just started
//! being asked about, and every room after a camera cut, is drawn until an
//! answer for a query made after that moment arrives. A room is exempt, and
//! always drawn, when the eye stands in it or inside its grown box, when it
//! is only seen (a sky or a backdrop), or when its model has no bounds. A
//! room past the first `MAX_QUERIES` asked about is drawn too.
//!
//! `KJK_ROOM_QUERIES=off` turns the queries off, so the culling draws every
//! room the VIS and the fog pick, as it did before.

use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::{Arc, Mutex};

use bevy::core_pipeline::Core3d;
use bevy::core_pipeline::core_3d::{main_opaque_pass_3d, main_transparent_pass_3d};
use bevy::prelude::*;
use bevy::render::diagnostic::RecordDiagnostics;
use bevy::render::extract_component::{ExtractComponent, ExtractComponentPlugin};
use bevy::render::extract_resource::{ExtractResource, ExtractResourcePlugin};
use bevy::render::render_resource::{
    Buffer, BufferDescriptor, BufferUsages, CompareFunction, DepthStencilState, LoadOp, MapMode,
    MultisampleState, Operations, PrimitiveState, PrimitiveTopology, RawRenderPipelineDescriptor,
    RawVertexBufferLayout, RawVertexState, RenderPassDepthStencilAttachment, RenderPassDescriptor,
    RenderPipeline, ShaderModule, ShaderModuleDescriptor, ShaderSource, StoreOp, VertexAttribute,
    VertexFormat, VertexStepMode,
};
use bevy::render::renderer::{RenderContext, RenderDevice, RenderQueue, ViewQuery};
use bevy::render::view::{ExtractedView, Msaa, ViewDepthTexture};
use bevy::render::{Render, RenderApp, RenderSystems};

/// How far each room's box is grown, in metres, so a room an opening
/// starts to show is asked about a little before it shows.
const MARGIN: f32 = 0.5;

/// How many frames a room stays drawn after its last answer that it is
/// visible, so a room seen through a gap that flickers shut is not popped.
const HOLD: u32 = 15;

/// How many frames old an answer can be and still hide a room. Answers
/// normally arrive two to four frames after their query. They stop coming
/// when a read back fails or the GPU falls behind, and the rooms they hid
/// are then drawn again.
const MAX_AGE: u32 = 30;

/// A camera that moves farther than this in one frame, in metres, has cut
/// to a new view. Every room is then drawn until fresh answers arrive.
const CUT_DISTANCE: f32 = 2.0;

/// A camera that turns farther than this in one frame, in radians, has cut.
const CUT_TURN: f32 = 0.5;

/// How many rooms a frame asks about at most. A room past it is drawn.
const MAX_QUERIES: u32 = 256;

/// How many frames of queries can be in flight at once. It also bounds the
/// answers waiting for the culling.
const SLOTS: usize = 3;

/// The 36 vertices of a box's 12 triangles, two for each of its 6 faces.
/// Each is an index into the box's 8 corners (`corners`).
const BOX_TRIANGLES: [usize; 36] = [
    0, 1, 3, 0, 3, 2, 4, 6, 7, 4, 7, 5, 0, 4, 5, 0, 5, 1, 2, 3, 7, 2, 7, 6, 0, 2, 6, 0, 6, 4, 1, 5,
    7, 1, 7, 3,
];

/// The bytes of one frame's boxes at most, 4 floats for each vertex.
const VERTEX_BYTES: u64 = MAX_QUERIES as u64 * BOX_TRIANGLES.len() as u64 * 16;

const SHADER: &str = "@vertex fn main(@location(0) clip: vec4<f32>) -> @builtin(position) vec4<f32> { return clip; }";

pub(crate) struct RoomQueriesPlugin;

impl Plugin for RoomQueriesPlugin {
    fn build(&self, app: &mut App) {
        let answers = Answers::default();
        app.insert_resource(RoomQueries::new(enabled()))
            .insert_resource(answers.clone())
            .init_resource::<QueryBoxes>()
            .add_plugins((
                ExtractResourcePlugin::<QueryBoxes>::default(),
                ExtractComponentPlugin::<QueryView>::default(),
            ))
            .add_observer(mark_view);
        let Some(render_app) = app.get_sub_app_mut(RenderApp) else {
            return;
        };
        render_app
            .insert_resource(answers)
            .init_resource::<QueryGpu>()
            .add_systems(Render, map_answers.in_set(RenderSystems::Cleanup))
            .add_systems(
                Core3d,
                query_rooms
                    .after(main_opaque_pass_3d)
                    .before(main_transparent_pass_3d),
            );
    }
}

fn enabled() -> bool {
    std::env::var("KJK_ROOM_QUERIES").as_deref() != Ok("off")
}

/// The box's 8 corners. Bit 0 of a corner's index picks the high x, bit 1
/// the high y and bit 2 the high z.
fn corners(low: Vec3, high: Vec3) -> [Vec3; 8] {
    std::array::from_fn(|bits| {
        Vec3::new(
            if bits & 1 == 0 { low.x } else { high.x },
            if bits & 2 == 0 { low.y } else { high.y },
            if bits & 4 == 0 { low.z } else { high.z },
        )
    })
}

/// Marks the scene camera, whose view the rooms are asked about from.
#[derive(Component, Clone, ExtractComponent)]
pub(crate) struct QueryView;

fn mark_view(add: On<Add, crate::scene::FlyCamera>, mut commands: Commands) {
    commands.entity(add.entity).insert(QueryView);
}

/// The rooms one frame asks about. Each has its index and its grown box in
/// Bevy's space. `scene` tells one scene's rooms from the next one's.
#[derive(Resource, Clone, Default, ExtractResource)]
pub(crate) struct QueryBoxes {
    scene: u32,
    frame: u32,
    boxes: Vec<(u16, Vec3, Vec3)>,
}

/// One frame's answers, as the GPU gave them.
struct Answer {
    scene: u32,
    frame: u32,
    rooms: Vec<u16>,
    visible: Vec<bool>,
}

/// The answers read back and waiting for the culling. The main world and
/// the render world share them.
#[derive(Resource, Clone, Default)]
pub(crate) struct Answers(Arc<Mutex<Vec<Answer>>>);

impl Answers {
    /// Keeps `answer`, dropping the oldest one when `SLOTS` already wait,
    /// so answers no culling takes do not pile up.
    fn push(&self, answer: Answer) {
        let Ok(mut waiting) = self.0.lock() else {
            return;
        };
        if waiting.len() >= SLOTS {
            waiting.remove(0);
        }
        waiting.push(answer);
    }
}

/// What the culling knows from the answers, room by room.
#[derive(Resource)]
pub(crate) struct RoomQueries {
    on: bool,
    scene: u32,
    frame: u32,
    /// Each room's box grown by `MARGIN`, in game space and in Bevy's
    /// space. `None` for a room without bounds.
    grown: Vec<Option<(Vec3, Vec3)>>,
    boxes: Vec<Option<(Vec3, Vec3)>>,
    /// The camera's place and rotation last frame, to tell a cut.
    camera: Option<(Vec3, Quat)>,
    /// For each room, the frame it last started being asked about, or the
    /// last cut. An answer to a query made before it cannot hide the room.
    fresh_from: Vec<u32>,
    /// For each room, the frame of the newest answer about it.
    answered: Vec<Option<u32>>,
    /// For each room, the last frame it is drawn for its last visible answer.
    seen_until: Vec<u32>,
    /// Which rooms were asked about last frame.
    asked: Vec<bool>,
    /// The frames between a query and its answer, last time.
    lag: Option<u32>,
}

impl RoomQueries {
    fn new(on: bool) -> RoomQueries {
        RoomQueries {
            on,
            scene: 0,
            frame: 0,
            grown: Vec::new(),
            boxes: Vec::new(),
            camera: None,
            fresh_from: Vec::new(),
            answered: Vec::new(),
            seen_until: Vec::new(),
            asked: Vec::new(),
            lag: None,
        }
    }

    /// Starts a new scene whose rooms have the `bounds` (game space). Every
    /// earlier answer is dropped.
    pub(crate) fn reset(&mut self, bounds: &[Option<(Vec3, Vec3)>]) {
        let rooms = bounds.len();
        let margin = Vec3::splat(MARGIN);
        let rotate = crate::scene::game_to_bevy();
        self.scene = self.scene.wrapping_add(1);
        self.grown = (bounds.iter())
            .map(|bounds| bounds.map(|(low, high)| (low - margin, high + margin)))
            .collect();
        self.boxes = (self.grown.iter())
            .map(|grown| {
                grown.map(|(low, high)| {
                    (corners(low, high).iter()).fold((Vec3::MAX, Vec3::MIN), |(low, high), c| {
                        let c = rotate * *c;
                        (low.min(c), high.max(c))
                    })
                })
            })
            .collect();
        self.camera = None;
        self.fresh_from = vec![self.frame; rooms];
        self.answered = vec![None; rooms];
        self.seen_until = vec![0; rooms];
        self.asked = vec![false; rooms];
        self.lag = None;
    }

    /// Whether room `room`'s newest answer hides it this frame.
    fn hides(&self, room: usize) -> bool {
        self.answered[room].is_some_and(|answered| {
            answered >= self.fresh_from[room]
                && self.frame.wrapping_sub(answered) <= MAX_AGE
                && self.seen_until[room] < answered
        })
    }

    fn learn(&mut self, answer: &Answer) {
        for (&room, &seen) in answer.rooms.iter().zip(&answer.visible) {
            let room = usize::from(room);
            if room >= self.answered.len() {
                continue;
            }
            if self.answered[room].is_none_or(|old| answer.frame > old) {
                self.answered[room] = Some(answer.frame);
                if seen {
                    self.seen_until[room] = answer.frame + HOLD;
                }
            }
        }
        self.lag = Some(self.frame.wrapping_sub(answer.frame));
    }
}

/// The culling's use of the occlusion queries (`scene::cull_rooms`).
#[derive(bevy::ecs::system::SystemParam)]
pub(crate) struct Occlusion<'w> {
    queries: ResMut<'w, RoomQueries>,
    answers: Res<'w, Answers>,
    boxes: ResMut<'w, QueryBoxes>,
}

impl Occlusion<'_> {
    pub(crate) fn on(&self) -> bool {
        self.queries.on
    }

    /// The frames between a query and its answer, last time.
    pub(crate) fn lag(&self) -> Option<u32> {
        self.queries.lag
    }

    /// Stops asking about rooms while the culling has no scene or camera.
    pub(crate) fn stop(&mut self) {
        if !self.boxes.boxes.is_empty() {
            self.boxes.boxes.clear();
        }
    }

    /// Which of the `picked` rooms to draw this frame, and asks about the
    /// rest for the frames to come. `exempt` rooms are always drawn. The
    /// eye at `eye` (game space) draws every room whose grown box holds it.
    /// `camera` is the camera's place and rotation, to tell a cut.
    pub(crate) fn decide(
        &mut self,
        picked: &[bool],
        exempt: &[bool],
        eye: Vec3,
        camera: (Vec3, Quat),
    ) -> Vec<bool> {
        let queries = &mut *self.queries;
        queries.frame = queries.frame.wrapping_add(1);
        let frame = queries.frame;
        self.boxes.scene = queries.scene;
        self.boxes.frame = frame;
        self.boxes.boxes.clear();
        if queries.asked.len() != picked.len() {
            return picked.to_vec();
        }
        if let Ok(mut waiting) = self.answers.0.lock() {
            for answer in waiting.drain(..) {
                if answer.scene == queries.scene {
                    queries.learn(&answer);
                }
            }
        }
        let cut = queries.camera.is_some_and(|(place, turn)| {
            place.distance(camera.0) > CUT_DISTANCE || turn.angle_between(camera.1) > CUT_TURN
        });
        queries.camera = Some(camera);
        (0..picked.len())
            .map(|room| {
                let asked = picked[room]
                    && queries.on
                    && !exempt[room]
                    && self.boxes.boxes.len() < MAX_QUERIES as usize
                    && (queries.grown[room]).is_some_and(|(low, high)| {
                        !(eye.cmpge(low).all() && eye.cmple(high).all())
                    });
                if cut || (asked && !queries.asked[room]) {
                    queries.fresh_from[room] = frame;
                }
                queries.asked[room] = asked;
                if !asked {
                    return picked[room];
                }
                if let Some((low, high)) = queries.boxes[room] {
                    self.boxes.boxes.push((room as u16, low, high));
                }
                !queries.hides(room)
            })
            .collect()
    }
}

/// A slot's state. It is free, or its queries were recorded this frame, or
/// its answers are being read back.
const FREE: u8 = 0;
const RECORDED: u8 = 1;
const READING: u8 = 2;

/// One frame's queries and the buffers their boxes and answers go through.
struct Slot {
    set: wgpu::QuerySet,
    vertices: Buffer,
    resolved: Buffer,
    read: Buffer,
    state: Arc<AtomicU8>,
    scene: u32,
    frame: u32,
    rooms: Vec<u16>,
}

/// The render world's queries. It holds the box pipeline for each sample
/// count and depth format, and the slots in flight.
#[derive(Resource, Default)]
struct QueryGpu {
    shader: Option<ShaderModule>,
    pipelines: Vec<(u32, wgpu::TextureFormat, RenderPipeline)>,
    slots: Vec<Slot>,
    /// The boxes' vertices of the frame being recorded, kept between frames.
    bytes: Vec<u8>,
}

impl QueryGpu {
    fn pipeline(
        &mut self,
        device: &RenderDevice,
        samples: u32,
        format: wgpu::TextureFormat,
    ) -> &RenderPipeline {
        if let Some(index) = (self.pipelines.iter())
            .position(|(count, depth, _)| *count == samples && *depth == format)
        {
            return &self.pipelines[index].2;
        }
        let shader = self.shader.get_or_insert_with(|| {
            device.create_and_validate_shader_module(ShaderModuleDescriptor {
                label: Some("room_queries"),
                source: ShaderSource::Wgsl(SHADER.into()),
            })
        });
        let pipeline = device.create_render_pipeline(&RawRenderPipelineDescriptor {
            label: Some("room_queries"),
            layout: None,
            vertex: RawVertexState {
                module: shader,
                entry_point: Some("main"),
                compilation_options: Default::default(),
                buffers: &[RawVertexBufferLayout {
                    array_stride: 16,
                    step_mode: VertexStepMode::Vertex,
                    attributes: &[VertexAttribute {
                        format: VertexFormat::Float32x4,
                        offset: 0,
                        shader_location: 0,
                    }],
                }],
            },
            primitive: PrimitiveState {
                topology: PrimitiveTopology::TriangleList,
                cull_mode: None,
                ..default()
            },
            depth_stencil: Some(DepthStencilState {
                format,
                depth_write_enabled: Some(false),
                depth_compare: Some(CompareFunction::GreaterEqual),
                stencil: default(),
                bias: default(),
            }),
            multisample: MultisampleState {
                count: samples,
                ..default()
            },
            fragment: None,
            multiview_mask: None,
            cache: None,
        });
        self.pipelines.push((samples, format, pipeline));
        &self.pipelines.last().expect("just pushed").2
    }

    /// A free slot, made when fewer than `SLOTS` exist.
    fn free_slot(&mut self, device: &RenderDevice) -> Option<usize> {
        if let Some(index) =
            (self.slots.iter()).position(|slot| slot.state.load(Ordering::Acquire) == FREE)
        {
            return Some(index);
        }
        if self.slots.len() >= SLOTS {
            return None;
        }
        let bytes = u64::from(MAX_QUERIES) * 8;
        self.slots.push(Slot {
            set: device
                .wgpu_device()
                .create_query_set(&wgpu::QuerySetDescriptor {
                    label: Some("room_queries"),
                    ty: wgpu::QueryType::Occlusion,
                    count: MAX_QUERIES,
                }),
            vertices: device.create_buffer(&BufferDescriptor {
                label: Some("room_queries_boxes"),
                size: VERTEX_BYTES,
                usage: BufferUsages::VERTEX | BufferUsages::COPY_DST,
                mapped_at_creation: false,
            }),
            resolved: device.create_buffer(&BufferDescriptor {
                label: Some("room_queries_resolved"),
                size: bytes,
                usage: BufferUsages::QUERY_RESOLVE | BufferUsages::COPY_SRC,
                mapped_at_creation: false,
            }),
            read: device.create_buffer(&BufferDescriptor {
                label: Some("room_queries_read"),
                size: bytes,
                usage: BufferUsages::MAP_READ | BufferUsages::COPY_DST,
                mapped_at_creation: false,
            }),
            state: Arc::new(AtomicU8::new(FREE)),
            scene: 0,
            frame: 0,
            rooms: Vec::with_capacity(MAX_QUERIES as usize),
        });
        Some(self.slots.len() - 1)
    }
}

/// Draws each asked room's box against the opaque depth inside its own
/// occlusion query, then copies the answers where the CPU can read them.
fn query_rooms(
    view: ViewQuery<(&ExtractedView, &ViewDepthTexture, &Msaa), With<QueryView>>,
    boxes: Res<QueryBoxes>,
    mut gpu: ResMut<QueryGpu>,
    (device, queue): (Res<RenderDevice>, Res<RenderQueue>),
    mut ctx: RenderContext,
) {
    if boxes.boxes.is_empty() {
        return;
    }
    let (extracted, depth, msaa) = view.into_inner();
    let Some(index) = gpu.free_slot(&device) else {
        return;
    };
    let clip_from_world = extracted.clip_from_world.unwrap_or_else(|| {
        extracted.clip_from_view * extracted.world_from_view.to_matrix().inverse()
    });
    let count = boxes.boxes.len().min(MAX_QUERIES as usize) as u32;
    let gpu = &mut *gpu;
    gpu.bytes.clear();
    for &(_, low, high) in &boxes.boxes[..count as usize] {
        let clip = corners(low, high).map(|corner| clip_from_world * corner.extend(1.0));
        for &corner in &BOX_TRIANGLES {
            for value in clip[corner].to_array() {
                gpu.bytes.extend_from_slice(&value.to_ne_bytes());
            }
        }
    }
    let format = depth.texture.format();
    let pipeline = gpu.pipeline(&device, msaa.samples(), format).clone();
    let slot = &mut gpu.slots[index];
    queue.write_buffer(&slot.vertices, 0, &gpu.bytes);
    let diagnostics = ctx.diagnostic_recorder();
    let diagnostics = diagnostics.as_deref();
    let time_span = diagnostics.time_span(ctx.command_encoder(), "room_queries");
    let encoder = ctx.command_encoder();
    {
        let mut pass = encoder.begin_render_pass(&RenderPassDescriptor {
            label: Some("room_queries"),
            color_attachments: &[],
            depth_stencil_attachment: Some(RenderPassDepthStencilAttachment {
                view: depth.view(),
                depth_ops: Some(Operations {
                    load: LoadOp::Load,
                    store: StoreOp::Store,
                }),
                stencil_ops: None,
            }),
            timestamp_writes: None,
            occlusion_query_set: Some(&slot.set),
            multiview_mask: None,
        });
        pass.set_pipeline(&pipeline);
        pass.set_vertex_buffer(0, *slot.vertices.slice(..));
        let per_box = BOX_TRIANGLES.len() as u32;
        for query in 0..count {
            pass.begin_occlusion_query(query);
            pass.draw(query * per_box..(query + 1) * per_box, 0..1);
            pass.end_occlusion_query();
        }
    }
    encoder.resolve_query_set(&slot.set, 0..count, &slot.resolved, 0);
    encoder.copy_buffer_to_buffer(&slot.resolved, 0, &slot.read, 0, u64::from(count) * 8);
    time_span.end(encoder);
    slot.scene = boxes.scene;
    slot.frame = boxes.frame;
    slot.rooms.clear();
    (slot.rooms).extend(boxes.boxes[..count as usize].iter().map(|(room, ..)| *room));
    slot.state.store(RECORDED, Ordering::Release);
}

/// Once the frame is submitted, reads back each slot recorded in it. Any
/// answer that is not zero means visible, because Vulkan's queries are not
/// precise.
fn map_answers(gpu: Res<QueryGpu>, answers: Res<Answers>) {
    for slot in &gpu.slots {
        if slot
            .state
            .compare_exchange(RECORDED, READING, Ordering::AcqRel, Ordering::Acquire)
            .is_err()
        {
            continue;
        }
        let bytes = slot.rooms.len() as u64 * 8;
        let read = slot.read.clone();
        let state = slot.state.clone();
        let answers = answers.clone();
        let (scene, frame, rooms) = (slot.scene, slot.frame, slot.rooms.clone());
        slot.read
            .slice(..bytes)
            .map_async(MapMode::Read, move |result| {
                if result.is_ok() {
                    let visible = {
                        let data = read.slice(..bytes).get_mapped_range();
                        (data.as_chunks::<8>().0.iter())
                            .map(|answer| answer.iter().any(|byte| *byte != 0))
                            .collect()
                    };
                    read.unmap();
                    answers.push(Answer {
                        scene,
                        frame,
                        rooms,
                        visible,
                    });
                }
                state.store(FREE, Ordering::Release);
            });
    }
}
