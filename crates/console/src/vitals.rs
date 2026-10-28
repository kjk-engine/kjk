//! The run's vitals in every `dump`: the process's memory, the frames'
//! times since the last dump, and the assets alive, so a long session's
//! growth and stalls show in its dumps.

use std::collections::VecDeque;

use bevy::prelude::*;
use serde_json::json;

use crate::{DumpDue, DumpFacts};

/// Frame times kept: the last 10 s at 60 Hz (or every frame since the last
/// dump, whichever is fewer).
const FRAMES_KEPT: usize = 600;
/// A frame this long or longer is a hitch.
const HITCH_MS: f32 = 33.4;

/// The frames' times (ms) since the last dump.
#[derive(Resource, Default)]
pub(crate) struct FrameTimes(VecDeque<f32>);

pub(crate) fn time_frames(time: Res<Time<Real>>, mut frames: ResMut<FrameTimes>) {
    if frames.0.len() == FRAMES_KEPT {
        frames.0.pop_front();
    }
    frames.0.push_back(time.delta_secs() * 1000.0);
}

/// When a dump is due: the vitals into its facts, and the frames' times
/// start again.
pub(crate) fn publish(
    due: Res<DumpDue>,
    mut facts: ResMut<DumpFacts>,
    mut frames: ResMut<FrameTimes>,
    images: Option<Res<Assets<Image>>>,
    meshes: Option<Res<Assets<Mesh>>>,
) {
    if !due.0 {
        return;
    }
    let mut times: Vec<f32> = frames.0.drain(..).collect();
    times.sort_by(f32::total_cmp);
    let at = |q: f32| {
        times
            .get(((times.len() as f32 - 1.0) * q).round() as usize)
            .copied()
    };
    let image_bytes: usize = images.as_ref().map_or(0, |images| {
        images
            .iter()
            .filter_map(|(_, image)| image.data.as_ref().map(Vec::len))
            .sum()
    });
    let memory = memory();
    facts.0.insert(
        "vitals".into(),
        json!({
            "footprint_mb": memory.map(|m| m.footprint as f64 / MB),
            "resident_mb": memory.map(|m| m.resident as f64 / MB),
            "frames": times.len(),
            "frame_ms": {
                "p50": at(0.5), "p95": at(0.95), "p99": at(0.99), "max": times.last(),
                "hitches": times.iter().filter(|&&ms| ms >= HITCH_MS).count(),
            },
            "images": images.as_ref().map_or(0, |images| images.len()),
            "image_cpu_mb": image_bytes as f64 / MB,
            "meshes": meshes.as_ref().map_or(0, |meshes| meshes.len()),
        }),
    );
}

const MB: f64 = 1024.0 * 1024.0;

/// The process's memory, in bytes.
#[derive(Clone, Copy)]
pub struct Memory {
    /// What the system charges the process (macOS: the physical
    /// footprint, GPU allocations included; elsewhere the resident set).
    pub footprint: u64,
    pub resident: u64,
}

/// This process's memory now, where the platform says.
#[cfg(target_os = "macos")]
pub fn memory() -> Option<Memory> {
    let mut info = std::mem::MaybeUninit::<libc::rusage_info_v2>::zeroed();
    // SAFETY: `info` is a rusage_info_v2, the flavour asked for, and lives
    // through the call.
    let ok = unsafe {
        libc::proc_pid_rusage(
            std::process::id() as i32,
            libc::RUSAGE_INFO_V2,
            info.as_mut_ptr().cast(),
        )
    } == 0;
    // SAFETY: the call filled it.
    let info = ok.then(|| unsafe { info.assume_init() })?;
    Some(Memory {
        footprint: info.ri_phys_footprint,
        resident: info.ri_resident_size,
    })
}

/// This process's memory now, where the platform says.
#[cfg(target_os = "linux")]
pub fn memory() -> Option<Memory> {
    // In kilobytes, whatever the page size.
    let status = std::fs::read_to_string("/proc/self/status").ok()?;
    let line = status
        .lines()
        .find_map(|line| line.strip_prefix("VmRSS:"))?;
    let resident = line.split_whitespace().next()?.parse::<u64>().ok()? * 1024;
    Some(Memory {
        footprint: resident,
        resident,
    })
}

/// This process's memory now, where the platform says.
#[cfg(not(any(target_os = "macos", target_os = "linux")))]
pub fn memory() -> Option<Memory> {
    None
}
