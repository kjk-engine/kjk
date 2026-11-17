//! What the renderer drew in the last frames, kept for a lost render device.
//!
//! A graphics driver that stops responding leaves no backtrace that says
//! why. So each frame the game notes a few numbers about what it asked the
//! GPU to draw: how many lights are listed, how many characters cast
//! shadows, their capsules, the shadow tiles, the placeables' boxes and
//! kept tiles, the pipelines still compiling, the samples a pixel takes,
//! the size the scene is drawn at, and the GPU memory wgpu has allocated.
//! The last `FRAMES` frames stay in memory. Nothing is written while the
//! game runs. When the render device is lost, `crash.txt` gets them all
//! (`crash::write_crash`'s callers), and so does the bug report that
//! carries it.
//!
//! The numbers are atomics that the systems which know them store into
//! (`note`). Once a frame, in `Last`, the main world copies them into the
//! ring. The render world's numbers (the pipelines compiling and the GPU
//! memory) are stored there and copied a frame later.

use std::sync::Mutex;
use std::sync::atomic::{AtomicU32, Ordering};

use bevy::prelude::*;

/// How many frames the ring keeps.
pub const FRAMES: usize = 8;

/// One number the ring keeps for each frame.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Fact {
    /// The lights in the light list (`material::LightList`).
    Lights,
    /// The characters that cast shadows (`shadows::ShadowList::count`).
    Casters,
    /// Their capsules.
    Capsules,
    /// The characters' shadow tiles drawn this frame.
    Tiles,
    /// The placeables' boxes traced near the camera.
    NearBoxes,
    /// The shown placeables' boxes (`shadows::BoxList::count`).
    Boxes,
    /// The placeables' kept tiles.
    KeptTiles,
    /// The Shadows setting: 0 Off, 1 Low, 2 High, 3 Ultra.
    Shadows,
    /// The samples a pixel of the scene camera takes (MSAA).
    Samples,
    /// The width and height the scene camera draws at, in pixels.
    Width,
    Height,
    /// The render pipelines still compiling.
    Pipelines,
    /// The GPU memory wgpu has allocated, in MiB (0 where wgpu does not
    /// report it).
    GpuMemory,
}

const ALL: [Fact; 13] = [
    Fact::Lights,
    Fact::Casters,
    Fact::Capsules,
    Fact::Tiles,
    Fact::NearBoxes,
    Fact::Boxes,
    Fact::KeptTiles,
    Fact::Shadows,
    Fact::Samples,
    Fact::Width,
    Fact::Height,
    Fact::Pipelines,
    Fact::GpuMemory,
];

impl Fact {
    fn name(self) -> &'static str {
        match self {
            Fact::Lights => "lights",
            Fact::Casters => "casters",
            Fact::Capsules => "capsules",
            Fact::Tiles => "tiles",
            Fact::NearBoxes => "near_boxes",
            Fact::Boxes => "boxes",
            Fact::KeptTiles => "kept_tiles",
            Fact::Shadows => "shadows",
            Fact::Samples => "msaa",
            Fact::Width => "width",
            Fact::Height => "height",
            Fact::Pipelines => "compiling",
            Fact::GpuMemory => "gpu_mib",
        }
    }
}

/// The numbers noted for the frame being made.
static NOW: [AtomicU32; ALL.len()] = [const { AtomicU32::new(0) }; ALL.len()];

/// One frame's numbers in the ring, with the frame's number.
type Frame = (u32, [u32; ALL.len()]);

/// The last `FRAMES` frames, oldest first once it is full. `next` is where
/// the next frame goes.
struct Ring {
    frames: [Frame; FRAMES],
    kept: usize,
    next: usize,
}

static RING: Mutex<Ring> = Mutex::new(Ring {
    frames: [(0, [0; ALL.len()]); FRAMES],
    kept: 0,
    next: 0,
});

/// Notes `value` for `fact` in the frame being made.
pub fn note(fact: Fact, value: u32) {
    NOW[fact as usize].store(value, Ordering::Relaxed);
}

/// Copies the numbers noted into the ring as frame `frame`.
pub fn push(frame: u32) {
    let values = std::array::from_fn(|index| NOW[index].load(Ordering::Relaxed));
    let mut ring = RING
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let next = ring.next;
    ring.frames[next] = (frame, values);
    ring.next = (next + 1) % FRAMES;
    ring.kept = (ring.kept + 1).min(FRAMES);
}

/// The ring as text for `crash.txt`: a heading, then one line for each
/// frame kept, oldest first. Empty when no frame was kept, or when another
/// thread holds the ring (a panic while it was being written).
pub fn text() -> String {
    let Ok(ring) = RING.try_lock() else {
        return String::new();
    };
    if ring.kept == 0 {
        return String::new();
    }
    let first = (ring.next + FRAMES - ring.kept) % FRAMES;
    let mut text = format!(
        "render facts, a line for each of the last {} frames:\n",
        ring.kept
    );
    for step in 0..ring.kept {
        let (frame, values) = ring.frames[(first + step) % FRAMES];
        text.push_str(&format!("frame {frame}:"));
        for (fact, value) in ALL.iter().zip(values) {
            text.push_str(&format!(" {} {value}", fact.name()));
        }
        text.push('\n');
    }
    text
}

/// Keeps the ring: the main world's numbers and the push in `Last`, the
/// render world's numbers in its `Render` schedule.
pub struct RenderFactsPlugin;

impl Plugin for RenderFactsPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Last, keep_frame);
    }

    fn finish(&self, app: &mut App) {
        if let Some(render_app) = app.get_sub_app_mut(bevy::render::RenderApp) {
            render_app.add_systems(
                bevy::render::Render,
                note_render_world.in_set(bevy::render::RenderSystems::Cleanup),
            );
        }
    }
}

/// Notes the scene camera's samples and size, and pushes the frame.
fn keep_frame(
    cameras: Query<(&Camera, Option<&Msaa>), With<crate::scene::FlyCamera>>,
    mut frame: Local<u32>,
) {
    if let Some((camera, msaa)) = cameras.iter().find(|(camera, _)| camera.is_active) {
        let size = camera.physical_target_size().unwrap_or(UVec2::ZERO);
        note(Fact::Samples, msaa.map_or(1, Msaa::samples));
        note(Fact::Width, size.x);
        note(Fact::Height, size.y);
    }
    *frame = frame.wrapping_add(1);
    push(*frame);
}

/// How many render frames apart the GPU memory is asked. wgpu walks every
/// allocation to answer, so it is not asked every frame.
const MEMORY_EVERY: u32 = 120;

/// Notes the pipelines still compiling, and now and then the GPU memory.
fn note_render_world(
    cache: Res<bevy::render::render_resource::PipelineCache>,
    device: Res<bevy::render::renderer::RenderDevice>,
    mut frame: Local<u32>,
) {
    note(Fact::Pipelines, cache.waiting_pipelines().count() as u32);
    if (*frame).is_multiple_of(MEMORY_EVERY) && crate::crash::device_lost().is_none() {
        let mib = device
            .wgpu_device()
            .generate_allocator_report()
            .map_or(0, |report| report.total_allocated_bytes / (1024 * 1024));
        note(Fact::GpuMemory, u32::try_from(mib).unwrap_or(u32::MAX));
    }
    *frame = frame.wrapping_add(1);
}

/// The probes of the ring (`kjk sim probes`, group "render facts"). It
/// keeps the last `FRAMES` frames and lists them oldest first, with every
/// number noted.
#[cfg(feature = "probes")]
pub fn probes() -> sim::probe::Checks {
    use sim::probe::check;
    for frame in 1..=(FRAMES as u32 + 3) {
        note(Fact::Casters, frame * 10);
        note(Fact::Samples, 2);
        push(frame);
    }
    let text = text();
    let lines: Vec<&str> = text.lines().collect();
    let oldest = FRAMES as u32 + 3 - FRAMES as u32 + 1;
    let wanted_first = format!("frame {oldest}: lights");
    let wanted_last = format!("frame {}: lights", FRAMES as u32 + 3);
    vec![
        (
            "render facts keep the last frames, oldest first",
            check(
                lines.len() == FRAMES + 1
                    && lines[1].starts_with(&wanted_first)
                    && lines[FRAMES].starts_with(&wanted_last),
                text.clone(),
            ),
        ),
        (
            "render facts hold every number",
            check(
                ALL.iter()
                    .all(|fact| lines[FRAMES].contains(&format!(" {} ", fact.name())))
                    && lines[FRAMES].contains(&format!("casters {}", (FRAMES as u32 + 3) * 10))
                    && lines[FRAMES].contains("msaa 2"),
                lines[FRAMES].to_string(),
            ),
        ),
    ]
}
