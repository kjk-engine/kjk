//! The Bink decoder compared with `ffmpeg` on one movie (`kjk movie
//! compare`). Every picture must match `ffmpeg`'s exactly, plane by plane.
//! Every sound sample must be within `SAMPLE_TOLERANCE` of `ffmpeg`'s.

use std::path::Path;
use std::process::Command;

use bink::{Bink, Sound, Video};

use crate::ffmpeg::program;

/// The largest difference allowed between a sample here and `ffmpeg`'s.
/// Both decode in 32-bit floats, so they differ only by rounding.
pub const SAMPLE_TOLERANCE: f32 = 0.001;

fn ffmpeg(movie: &Path, options: &[&str]) -> Result<Vec<u8>, String> {
    let output = Command::new(program("ffmpeg"))
        .args(["-v", "error", "-i"])
        .arg(movie)
        .args(options)
        .arg("-")
        .output()
        .map_err(|error| format!("ffmpeg: {error}"))?;
    if !output.status.success() {
        return Err(format!(
            "ffmpeg: {}",
            String::from_utf8_lossy(&output.stderr)
        ));
    }
    Ok(output.stdout)
}

/// Decodes the first `frames` frames and compares each plane with
/// `ffmpeg`'s. Gives how many frames matched, or the first difference.
pub fn video(movie: &Path, bink: &Bink, frames: usize) -> Result<usize, String> {
    let frames = bink.frames().min(frames);
    let theirs = ffmpeg(
        movie,
        &[
            "-an",
            "-frames:v",
            &frames.to_string(),
            "-f",
            "rawvideo",
            "-pix_fmt",
            "yuv420p",
        ],
    )?;
    let mut video = Video::new(bink);
    let mut at = 0;
    for frame in 0..frames {
        video
            .decode(bink.packets(frame)?.video)
            .map_err(|why| format!("frame {frame}: {why}"))?;
        for (plane, ours) in video.planes().iter().enumerate() {
            let theirs = theirs
                .get(at..at + ours.len())
                .ok_or(format!("ffmpeg gave fewer than {frames} frames"))?;
            if let Some(i) = ours.iter().zip(theirs).position(|(a, b)| a != b) {
                let width = if plane == 0 {
                    bink.width
                } else {
                    bink.width.div_ceil(2)
                } as usize;
                let wrong = ours.iter().zip(theirs).filter(|(a, b)| a != b).count();
                return Err(format!(
                    "frame {frame}, plane {plane}: the pixel at ({}, {}) is {} here and {} in ffmpeg, and {wrong} of {} pixels differ",
                    i % width,
                    i / width,
                    ours[i],
                    theirs[i],
                    ours.len()
                ));
            }
            at += ours.len();
        }
    }
    Ok(frames)
}

/// Decodes the first audio track whole and compares it with `ffmpeg`'s.
/// Gives the largest difference, or `None` for a movie without sound.
pub fn audio(movie: &Path, bink: &Bink) -> Result<Option<f32>, String> {
    if bink.tracks.is_empty() {
        return Ok(None);
    }
    let ours = Sound::of(bink, 0)?.samples;
    let theirs: Vec<f32> = ffmpeg(
        movie,
        &["-vn", "-map", "0:a:0", "-f", "f32le", "-c:a", "pcm_f32le"],
    )?
    .chunks_exact(4)
    .map(|b| f32::from_le_bytes([b[0], b[1], b[2], b[3]]))
    .collect();
    if ours.len() != theirs.len() {
        return Err(format!(
            "{} samples here and {} in ffmpeg",
            ours.len(),
            theirs.len()
        ));
    }
    let largest = ours
        .iter()
        .zip(&theirs)
        .map(|(a, b)| (a - b).abs())
        .fold(0.0, f32::max);
    if largest > SAMPLE_TOLERANCE {
        return Err(format!("a sample differs from ffmpeg's by {largest}"));
    }
    Ok(Some(largest))
}
