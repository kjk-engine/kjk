//! The skeleton probes' measures without a window: each built-in timeline
//! (`kjk rig c --moves <timeline>`, the hybrid) loaded as the command
//! loads it, and the notes its load writes, foot slide and jolts among
//! them (`rig_timeline::measure`). The approved test
//! `timeline_jolt_ceilings` holds every timeline's jolts under ceilings.

use vfs::Discovery;

use crate::viewer_rig;

/// The built-in timelines.
pub fn timelines() -> &'static [&'static str] {
    crate::rig_timeline::TIMELINES
}

/// `timeline`'s notes, loaded as `kjk rig c --moves <timeline>` loads it
/// with every other option at its default.
pub fn notes(discovery: &Discovery, timeline: &str) -> Result<Vec<String>, String> {
    let options = viewer_rig::Options {
        probe: "c",
        body: "p_carthbb",
        head: "p_carthh",
        moves: timeline,
        saber: "single_1",
        kotor_clip: "pause1",
        trace: None,
        kotor_hilt: true,
        colors: vec!["blue"],
        quirks: false,
    };
    viewer_rig::load(discovery, &options).map(|scene| scene.creature_notes)
}

/// The sharpest jolt of each node the notes measure: (node, m/s², at
/// seconds), from their `jolt <node> (m/s²): <a> at <t>, …` lines.
pub fn jolts(notes: &[String]) -> Vec<(String, f32, f32)> {
    notes
        .iter()
        .filter_map(|note| {
            let rest = note.strip_prefix("jolt ")?;
            let (node, peaks) = rest.split_once(" (m/s²): ")?;
            let (peak, at) = peaks.split(", ").next()?.split_once(" at ")?;
            Some((node.to_string(), peak.parse().ok()?, at.parse().ok()?))
        })
        .collect()
}
