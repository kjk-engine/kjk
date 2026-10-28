//! A Bink (or any movie `ffmpeg` reads) made into our own (`Movie`):
//! `ffprobe` tells its size and frame rate, `ffmpeg` writes its frames as
//! JPEGs and its sound as an MP3 (FLAC where it has no MP3 encoder). Both
//! are found on the `PATH`; without them there are no movies.

use std::path::Path;
use std::process::{Command, Stdio};

use crate::{Coding, Movie};

/// A JPEG's quality as `ffmpeg`'s `-q:v` (2 best to 31 worst): 6 keeps
/// KOTOR's 640-wide frames clean at about 370 KB a second.
const QUALITY: &str = "6";
/// An MP3's quality as `-q:a` (LAME's VBR, 0 best to 9 worst).
const MP3_QUALITY: &str = "2";
/// What the settings above make, for store keys: bump it when they change.
pub const FORMAT: &str = "kjk-movie-v1-q6";

/// The tools, and whether `ffmpeg` encodes MP3.
#[derive(Debug, Clone)]
pub struct Ffmpeg {
    mp3: bool,
}

impl Ffmpeg {
    /// `ffmpeg` and `ffprobe` as the `PATH` has them; why not, if not.
    pub fn find() -> Result<Ffmpeg, String> {
        let encoders = run(Command::new("ffmpeg").args(["-hide_banner", "-encoders"]))
            .map_err(|error| format!("no ffmpeg ({error})"))?;
        run(Command::new("ffprobe").arg("-version"))
            .map_err(|error| format!("no ffprobe ({error})"))?;
        Ok(Ffmpeg {
            mp3: String::from_utf8_lossy(&encoders).contains("libmp3lame"),
        })
    }

    /// The movie at `input`, made ours.
    pub fn transcode(&self, input: &Path) -> Result<Vec<u8>, String> {
        let streams = probe(input)?;
        let video = streams
            .iter()
            .find(|s| s.get("codec_type") == Some("video"))
            .ok_or("no picture")?;
        let size = (number(video, "width")?, number(video, "height")?);
        let rate = video
            .get("r_frame_rate")
            .and_then(|rate| rate.split_once('/'))
            .and_then(|(n, d)| Some((n.parse().ok()?, d.parse().ok()?)))
            .filter(|(n, d): &(u32, u32)| *n > 0 && *d > 0)
            .ok_or("no frame rate")?;
        let pictures = run(Command::new("ffmpeg")
            .args(["-v", "error", "-i"])
            .arg(input)
            .args(["-map", "0:v:0", "-an", "-f", "image2pipe", "-c:v", "mjpeg"])
            .args(["-q:v", QUALITY, "-"]))?;
        let frames = crate::split_jpegs(&pictures)?;
        let has_sound = streams.iter().any(|s| s.get("codec_type") == Some("audio"));
        let sound = if has_sound {
            let (coding, codec, format): (Coding, &[&str], &str) = if self.mp3 {
                (Coding::Mp3, &["libmp3lame", "-q:a", MP3_QUALITY], "mp3")
            } else {
                (Coding::Flac, &["flac"], "flac")
            };
            let bytes = run(Command::new("ffmpeg")
                .args(["-v", "error", "-i"])
                .arg(input)
                .args(["-map", "0:a:0", "-vn", "-c:a"])
                .args(codec)
                .args(["-f", format, "-"]))?;
            Some((coding, bytes))
        } else {
            None
        };
        Ok(Movie::encode(
            size,
            rate,
            &frames,
            sound
                .as_ref()
                .map(|(coding, bytes)| (*coding, bytes.as_slice())),
        ))
    }
}

/// What a stream's `key=value` lines say.
struct Stream(Vec<(String, String)>);

impl Stream {
    fn get(&self, key: &str) -> Option<&str> {
        self.0
            .iter()
            .find(|(k, _)| k == key)
            .map(|(_, value)| value.as_str())
    }
}

fn number(stream: &Stream, key: &str) -> Result<u32, String> {
    stream
        .get(key)
        .and_then(|value| value.parse().ok())
        .ok_or(format!("no {key}"))
}

/// Each stream's type, size and frame rate, as `ffprobe` reads them.
fn probe(input: &Path) -> Result<Vec<Stream>, String> {
    let out = run(Command::new("ffprobe")
        .args(["-v", "error", "-show_entries"])
        .arg("stream=codec_type,width,height,r_frame_rate")
        .arg(input))?;
    let mut streams = Vec::new();
    for line in String::from_utf8_lossy(&out).lines() {
        match line.trim() {
            "[STREAM]" => streams.push(Stream(Vec::new())),
            line => {
                if let (Some(stream), Some((key, value))) =
                    (streams.last_mut(), line.split_once('='))
                {
                    stream.0.push((key.to_string(), value.to_string()));
                }
            }
        }
    }
    Ok(streams)
}

/// What `command` writes, or why it failed.
fn run(command: &mut Command) -> Result<Vec<u8>, String> {
    let out = command
        .stdin(Stdio::null())
        .output()
        .map_err(|error| error.to_string())?;
    if !out.status.success() {
        let said = String::from_utf8_lossy(&out.stderr);
        return Err(format!(
            "{:?} failed: {}",
            command.get_program(),
            said.lines().next().unwrap_or("(nothing said)")
        ));
    }
    Ok(out.stdout)
}
