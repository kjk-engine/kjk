//! Counters in the Perfetto trace (`KJK_PERF=1`, D16): values sampled as
//! the run goes, on the trace's own clock, written into `trace.json` as
//! counter tracks when the run ends (`tracing-chrome` writes spans and
//! events only). Off, a sample costs one check.

use std::io::{Read as _, Seek as _, SeekFrom, Write as _};
use std::path::Path;
use std::sync::{Mutex, OnceLock};
use std::time::Instant;

/// When the trace started (its timestamps count from here).
static ORIGIN: OnceLock<Instant> = OnceLock::new();
static SAMPLES: Mutex<Vec<serde_json::Value>> = Mutex::new(Vec::new());

/// The trace starts now: counters are kept from here.
pub fn start() {
    let _ = ORIGIN.set(Instant::now());
}

/// A sample of `track`'s counters (each its own line in Perfetto,
/// "<track> <name>").
pub fn counter(track: &str, values: &[(&str, f64)]) {
    let Some(origin) = ORIGIN.get() else {
        return;
    };
    let args: serde_json::Map<String, serde_json::Value> = values
        .iter()
        .map(|(name, value)| (name.to_string(), serde_json::json!(value)))
        .collect();
    let sample = serde_json::json!({
        "ph": "C",
        "name": track,
        "ts": origin.elapsed().as_nanos() as f64 / 1000.0,
        "pid": 1,
        "args": args,
    });
    if let Ok(mut samples) = SAMPLES.lock() {
        samples.push(sample);
    }
}

/// Adds the counters to the finished trace at `path` (its writer
/// flushed and closed): before its closing `]`, without reading the rest
/// (a trace runs to gigabytes).
pub fn finish(path: &Path) {
    let samples = match SAMPLES.lock() {
        Ok(mut samples) => std::mem::take(&mut *samples),
        Err(_) => return,
    };
    if samples.is_empty() {
        return;
    }
    match append(path, &samples) {
        Ok(()) => eprintln!("trace: {} counter samples added", samples.len()),
        Err(error) => eprintln!("{}: counters not added: {error}", path.display()),
    }
}

fn append(path: &Path, samples: &[serde_json::Value]) -> std::io::Result<()> {
    let mut file = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(path)?;
    let length = file.seek(SeekFrom::End(0))?;
    let tail = length.min(64);
    file.seek(SeekFrom::End(-(tail as i64)))?;
    let mut end = Vec::new();
    file.read_to_end(&mut end)?;
    let close = end
        .iter()
        .rposition(|&byte| byte == b']')
        .ok_or_else(|| std::io::Error::other("no closing ] at the end"))?;
    file.set_len(length - tail + close as u64)?;
    file.seek(SeekFrom::End(0))?;
    let mut out = std::io::BufWriter::new(file);
    for sample in samples {
        out.write_all(b",\n")?;
        serde_json::to_writer(&mut out, sample)?;
    }
    out.write_all(b"\n]")?;
    out.flush()
}
