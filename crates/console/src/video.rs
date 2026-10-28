//! `record`'s and `clip`'s videos: their screenshots and the run's sound
//! made into an MP4 by `ffmpeg`. Without `ffmpeg` the frames stay, with a
//! notice.

use std::path::Path;
use std::process::{Command, Stdio};

use crate::script::{Frames, Video};

/// The run's sound, on the game's clock from 0 (`audio.wav` in the run
/// folder), when the run makes one.
pub const AUDIO: &str = "audio.wav";
/// A clip's frame rate: each screenshot shown until the next one's
/// moment.
const CLIP_FPS: &str = "30";

/// Writes `<name>.mp4` in `folder`; `Ok(false)` when there is no `ffmpeg`.
pub fn make(folder: &Path, video: &Video) -> Result<bool, String> {
    let name = &video.name;
    let mut ffmpeg = Command::new("ffmpeg");
    ffmpeg
        .current_dir(folder)
        .args(["-y", "-loglevel", "error"]);
    let (from, seconds) = match &video.frames {
        Frames::Even { from, fps, count } => {
            ffmpeg
                .args(["-framerate", &fps.to_string(), "-i"])
                .arg(format!("{name}-%04d.png"));
            (f64::from(*from), f64::from(*count) / f64::from(*fps))
        }
        Frames::Clipped(times) => {
            let (Some(first), Some(last)) = (times.first(), times.last()) else {
                return Err(format!("{name}: no frames were clipped"));
            };
            let list = folder.join(format!("{name}.ffconcat"));
            let mut text = String::from("ffconcat version 1.0\n");
            for (frame, pair) in times.windows(2).enumerate() {
                text += &format!(
                    "file {name}-{frame:04}.png\nduration {}\n",
                    (pair[1] - pair[0]) / 1000.0
                );
            }
            // The last frame is listed twice so that it shows at all.
            let last_file = format!("{name}-{:04}.png", times.len() - 1);
            text += &format!("file {last_file}\nduration 0.1\nfile {last_file}\n");
            std::fs::write(&list, text).map_err(|error| format!("{}: {error}", list.display()))?;
            ffmpeg
                .args(["-f", "concat", "-i"])
                .arg(list.file_name().expect("named above"));
            (first / 1000.0, (last - first) / 1000.0 + 0.1)
        }
    };
    if folder.join(AUDIO).is_file() {
        ffmpeg
            .args(["-ss", &from.to_string(), "-t", &seconds.to_string()])
            .args(["-i", AUDIO, "-c:a", "aac", "-b:a", "160k"]);
    }
    if matches!(video.frames, Frames::Clipped(_)) {
        ffmpeg.args(["-fps_mode", "cfr", "-r", CLIP_FPS]);
    }
    ffmpeg
        .args(["-c:v", "libx264", "-pix_fmt", "yuv420p"])
        // x264 wants even sizes.
        .args(["-vf", "pad=ceil(iw/2)*2:ceil(ih/2)*2"])
        .arg(format!("{name}.mp4"))
        .stdin(Stdio::null());
    match ffmpeg.output() {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(format!("ffmpeg: {error}")),
        Ok(output) if output.status.success() => Ok(true),
        Ok(output) => Err(format!(
            "ffmpeg: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        )),
    }
}
