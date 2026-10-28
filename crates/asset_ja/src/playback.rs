//! A sequence's frames and frame time, and sequences played one after
//! another.

use fmt_ja::animation_cfg::Sequence;
use fmt_ja::ghoul2::Gla;

use crate::g2::{Cue, Part};

/// The GLA frames `sequence` shows, in order, and the time each is shown
/// (ms): `ceil(1000 / fps)`, `floor` for a negative rate (a rate of 0
/// counts as 1). A backward sequence lists the game's frames, from one past
/// its range down to one above its first (QUIRKS.md Q-0003). `None` when
/// the range is negative or runs past the skeleton's `gla_frames`.
pub fn playback(sequence: &Sequence, gla_frames: usize) -> Option<(Vec<usize>, f32)> {
    let first = usize::try_from(sequence.first_frame).ok()?;
    let count = usize::try_from(sequence.frame_count).ok()?.max(1);
    let fps = if sequence.frames_per_second == 0 {
        1.0
    } else {
        sequence.frames_per_second as f32
    };
    let frame_ms = if fps < 0.0 {
        (1000.0 / fps).floor()
    } else {
        (1000.0 / fps).ceil()
    };
    let frames: Vec<usize> = if frame_ms < 0.0 {
        (first + 1..=first + count).rev().collect()
    } else {
        (first..first + count).collect()
    };
    frames
        .iter()
        .all(|&frame| frame < gla_frames)
        .then(|| (frames, frame_ms.abs()))
}

/// Sequences played one after another on the whole body, each blending in
/// over its own time as the one before it is held to its end (a looping one
/// for all its frames, others until their last frame shows): the cues and
/// the total length (ms). `None` if any sequence does not fit the skeleton.
pub fn chain<'s>(gla: &Gla, moves: &[(&'s Sequence, f32)]) -> Option<(Vec<Cue<'s>>, f32)> {
    let mut at_ms = 0.0;
    let mut cues = Vec::with_capacity(moves.len());
    for &(sequence, blend_ms) in moves {
        let (frames, frame_ms) = playback(sequence, gla.frame_count)?;
        cues.push(Cue {
            at_ms,
            part: Part::Both,
            sequence,
            blend_ms,
            speed_scale: 1.0,
        });
        let held = if sequence.loop_frame != -1 {
            frames.len()
        } else {
            frames.len().saturating_sub(1).max(1)
        };
        at_ms += held as f32 * frame_ms;
    }
    Some((cues, at_ms))
}
