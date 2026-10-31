//! Posts progress to the Discord server's #progress channel through its
//! webhook: a caption and the screenshots, gifs or clips that show it.
//!
//! cargo xtask discord [--dry-run] <caption> <file>...
//!
//! The webhook's URL is the owner's and never tracked: it is read from
//! `KJK_DISCORD_PROGRESS_WEBHOOK`, else from `context/discord-progress-webhook`
//! (git-ignored). A server without boosts takes 20 MB a message, so a
//! larger clip or gif is re-encoded (ffmpeg, no sound) to fit and a larger
//! picture becomes a JPEG; the shrunk copies go to `target/discord/`. Files
//! that do not fit one message go on in more, the caption on the first.
//! `--dry-run` shrinks and groups but sends nothing.
//!
//! Release announcements are not posted from here: `.github/workflows/
//! discord-release.yml` posts each Release when the owner publishes it.

use std::path::{Path, PathBuf};
use std::process::Command;

use crate::repo_root;

/// What a server without boosts takes in one message.
const MESSAGE_BYTES: u64 = 20_000_000;

/// Room left for the request's own parts, and for the encoder overshooting.
const SHRINK_TARGET: f64 = 0.9 * MESSAGE_BYTES as f64;

/// Discord's caps: attachments and characters in one message.
const MESSAGE_FILES: usize = 10;
const CAPTION_CHARS: usize = 2000;

/// The widest a shrunk clip or picture is kept.
const SHRINK_WIDTH: u32 = 1280;

const VIDEO: &[&str] = &["gif", "mp4", "mov", "webm", "mkv"];
const PICTURE: &[&str] = &["png", "jpg", "jpeg", "webp"];

pub fn run(args: &[String]) -> Result<(), String> {
    let (dry_run, rest) = match args.first().map(String::as_str) {
        Some("--dry-run") => (true, &args[1..]),
        _ => (false, args),
    };
    let [caption, files @ ..] = rest else {
        return Err("discord: usage: cargo xtask discord [--dry-run] <caption> <file>...".into());
    };
    if files.is_empty() {
        return Err("discord: nothing to post (give at least one file)".into());
    }
    if caption.chars().count() > CAPTION_CHARS {
        return Err(format!(
            "discord: the caption is over {CAPTION_CHARS} characters"
        ));
    }

    let ready = files
        .iter()
        .map(|file| fit(Path::new(file)))
        .collect::<Result<Vec<_>, String>>()?;
    let messages = group(ready);

    let webhook = if dry_run { None } else { Some(webhook()?) };
    for (index, message) in messages.iter().enumerate() {
        let text = if index == 0 { caption.as_str() } else { "" };
        let total: u64 = message.iter().map(|(_, bytes)| bytes).sum();
        println!("message {} ({}):", index + 1, megabytes(total));
        for (path, bytes) in message {
            println!("  {} ({})", path.display(), megabytes(*bytes));
        }
        if let Some(url) = &webhook {
            send(url, text, message)?;
        }
    }
    if dry_run {
        println!("dry run: nothing sent");
    } else {
        println!("posted {} message(s) to #progress", messages.len());
    }
    Ok(())
}

/// The webhook's URL, from the environment or the git-ignored file.
fn webhook() -> Result<String, String> {
    if let Ok(url) = std::env::var("KJK_DISCORD_PROGRESS_WEBHOOK") {
        return Ok(url.trim().to_owned());
    }
    let file = repo_root().join("context/discord-progress-webhook");
    std::fs::read_to_string(&file)
        .map(|url| url.trim().to_owned())
        .map_err(|_| {
            format!(
                "discord: no webhook: set KJK_DISCORD_PROGRESS_WEBHOOK or put its URL in {}",
                file.display()
            )
        })
}

/// The file as posted with its size: itself when it fits a message, else a
/// shrunk copy.
fn fit(path: &Path) -> Result<(PathBuf, u64), String> {
    let bytes = size(path)?;
    if bytes <= MESSAGE_BYTES {
        return Ok((path.to_owned(), bytes));
    }
    let extension = path
        .extension()
        .and_then(|extension| extension.to_str())
        .map_or(String::new(), str::to_ascii_lowercase);
    let stem = path
        .file_stem()
        .and_then(|stem| stem.to_str())
        .unwrap_or("file");
    let out_dir = repo_root().join("target/discord");
    std::fs::create_dir_all(&out_dir).map_err(|error| format!("{}: {error}", out_dir.display()))?;
    let scale = format!("scale='min({SHRINK_WIDTH},iw)':-2");
    let out = if VIDEO.contains(&extension.as_str()) {
        let out = out_dir.join(format!("{stem}.mp4"));
        let kbits = SHRINK_TARGET * 8.0 / duration(path)? / 1000.0;
        let rate = format!("{}k", kbits as u64);
        let buffer = format!("{}k", 2 * kbits as u64);
        ffmpeg(
            path,
            &out,
            &[
                "-an",
                "-vf",
                &format!("{scale},format=yuv420p"),
                "-c:v",
                "libx264",
                "-preset",
                "slow",
                "-b:v",
                &rate,
                "-maxrate",
                &rate,
                "-bufsize",
                &buffer,
                "-movflags",
                "+faststart",
            ],
        )?;
        out
    } else if PICTURE.contains(&extension.as_str()) {
        let out = out_dir.join(format!("{stem}.jpg"));
        ffmpeg(path, &out, &["-vf", &scale, "-q:v", "3"])?;
        out
    } else {
        return Err(format!(
            "discord: {} is {} and not a picture or clip to shrink",
            path.display(),
            megabytes(bytes)
        ));
    };
    let shrunk = size(&out)?;
    if shrunk > MESSAGE_BYTES {
        return Err(format!(
            "discord: {} is still {} shrunk; cut it shorter",
            path.display(),
            megabytes(shrunk)
        ));
    }
    println!(
        "shrunk {} ({}) → {} ({})",
        path.display(),
        megabytes(bytes),
        out.display(),
        megabytes(shrunk)
    );
    Ok((out, shrunk))
}

/// The files in order, as few messages as fit Discord's caps.
fn group(files: Vec<(PathBuf, u64)>) -> Vec<Vec<(PathBuf, u64)>> {
    let mut messages: Vec<Vec<(PathBuf, u64)>> = Vec::new();
    for file in files {
        let fits = messages.last().is_some_and(|message| {
            message.len() < MESSAGE_FILES
                && message.iter().map(|(_, bytes)| bytes).sum::<u64>() + file.1 <= MESSAGE_BYTES
        });
        match messages.last_mut() {
            Some(message) if fits => message.push(file),
            _ => messages.push(vec![file]),
        }
    }
    messages
}

/// One message through the webhook: `curl`, as `git` is run elsewhere.
fn send(url: &str, text: &str, files: &[(PathBuf, u64)]) -> Result<(), String> {
    // No pings from a caption, whatever it says.
    let payload = serde_json::json!({ "content": text, "allowed_mentions": { "parse": [] } });
    let mut curl = Command::new("curl");
    curl.args([
        "--silent",
        "--show-error",
        "--fail-with-body",
        "--output",
        "/dev/null",
    ])
    .arg("--form")
    .arg(format!("payload_json={payload}"));
    for (index, (path, _)) in files.iter().enumerate() {
        curl.arg("--form")
            .arg(format!("files[{index}]=@{}", path.display()));
    }
    let output = curl
        .arg(url)
        .output()
        .map_err(|error| format!("curl: {error}"))?;
    if !output.status.success() {
        return Err(format!(
            "discord: the webhook refused the message: {}{}",
            String::from_utf8_lossy(&output.stderr).trim(),
            String::from_utf8_lossy(&output.stdout).trim()
        ));
    }
    Ok(())
}

fn ffmpeg(input: &Path, output: &Path, options: &[&str]) -> Result<(), String> {
    let status = Command::new("ffmpeg")
        .args(["-loglevel", "error", "-y", "-i"])
        .arg(input)
        .args(options)
        .arg(output)
        .status()
        .map_err(|error| format!("ffmpeg: {error} (is it installed?)"))?;
    if !status.success() {
        return Err(format!("ffmpeg could not shrink {}", input.display()));
    }
    Ok(())
}

/// A clip's length in seconds.
fn duration(path: &Path) -> Result<f64, String> {
    let output = Command::new("ffprobe")
        .args([
            "-v",
            "error",
            "-show_entries",
            "format=duration",
            "-of",
            "csv=p=0",
        ])
        .arg(path)
        .output()
        .map_err(|error| format!("ffprobe: {error}"))?;
    String::from_utf8_lossy(&output.stdout)
        .trim()
        .parse::<f64>()
        .ok()
        .filter(|seconds| *seconds > 0.0)
        .ok_or_else(|| format!("ffprobe could not read {}'s length", path.display()))
}

fn size(path: &Path) -> Result<u64, String> {
    std::fs::metadata(path)
        .map(|metadata| metadata.len())
        .map_err(|error| format!("{}: {error}", path.display()))
}

fn megabytes(bytes: u64) -> String {
    format!("{:.1} MB", bytes as f64 / 1_000_000.0)
}
