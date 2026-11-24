//! The Bink decoder compared with `ffmpeg` on one movie (`kjk movie
//! compare`). Every picture must match `ffmpeg`'s exactly, plane by plane.
//! Every sound sample must be within `SAMPLE_TOLERANCE` of `ffmpeg`'s.

use std::io::Read;
use std::path::Path;
use std::process::{Command, Stdio};

use bink::{Bink, Sound, Video};

use crate::ffmpeg::program;

/// The largest difference allowed between a sample here and `ffmpeg`'s.
/// Both decode in 32-bit floats, so they differ only by rounding.
pub const SAMPLE_TOLERANCE: f32 = 0.001;

/// `ffmpeg` decoding `movie` to its standard output. It gets no standard
/// input, so it cannot take keys from the terminal or stop a run in the
/// background.
fn ffmpeg(movie: &Path, options: &[&str]) -> Command {
    let mut command = Command::new(program("ffmpeg"));
    command
        .args(["-nostdin", "-v", "error", "-i"])
        .arg(movie)
        .args(options)
        .arg("-")
        .stdin(Stdio::null());
    command
}

/// Decodes the first `frames` frames and compares each plane with
/// `ffmpeg`'s. `ffmpeg`'s frames are read one at a time, so a long movie
/// at a high resolution takes only a few megabytes. Gives how many frames
/// matched, or the first difference.
pub fn video(movie: &Path, bink: &Bink, frames: usize) -> Result<usize, String> {
    let frames = bink.frames().min(frames);
    let mut child = ffmpeg(
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
    )
    .stdout(Stdio::piped())
    .stderr(Stdio::piped())
    .spawn()
    .map_err(|error| format!("ffmpeg: {error}"))?;
    let compared = compare_frames(child.stdout.take().expect("piped"), bink, frames);
    if compared.is_err() {
        let _ = child.kill();
    }
    let output = child
        .wait_with_output()
        .map_err(|error| format!("ffmpeg: {error}"))?;
    if compared.is_ok() && !output.status.success() {
        return Err(format!(
            "ffmpeg: {}",
            String::from_utf8_lossy(&output.stderr)
        ));
    }
    compared
}

fn compare_frames(mut theirs: impl Read, bink: &Bink, frames: usize) -> Result<usize, String> {
    let mut video = Video::new(bink);
    let mut buffer = Vec::new();
    for frame in 0..frames {
        video
            .decode(bink.packets(frame)?.video)
            .map_err(|why| format!("frame {frame}: {why}"))?;
        for (plane, ours) in video.planes().iter().enumerate() {
            buffer.resize(ours.len(), 0);
            theirs
                .read_exact(&mut buffer)
                .map_err(|_| format!("ffmpeg gave fewer than {frames} frames"))?;
            if let Some(i) = ours.iter().zip(&buffer).position(|(a, b)| a != b) {
                let width = if plane == 0 {
                    bink.width
                } else {
                    bink.width.div_ceil(2)
                } as usize;
                let wrong = ours.iter().zip(&buffer).filter(|(a, b)| a != b).count();
                return Err(format!(
                    "frame {frame}, plane {plane}: the pixel at ({}, {}) is {} here and {} in ffmpeg, and {wrong} of {} pixels differ",
                    i % width,
                    i / width,
                    ours[i],
                    buffer[i],
                    ours.len()
                ));
            }
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
    let output = ffmpeg(
        movie,
        &["-vn", "-map", "0:a:0", "-f", "f32le", "-c:a", "pcm_f32le"],
    )
    .output()
    .map_err(|error| format!("ffmpeg: {error}"))?;
    if !output.status.success() {
        return Err(format!(
            "ffmpeg: {}",
            String::from_utf8_lossy(&output.stderr)
        ));
    }
    let theirs: Vec<f32> = output
        .stdout
        .as_chunks::<4>()
        .0
        .iter()
        .map(|&bytes| f32::from_le_bytes(bytes))
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
