//! The run folder a child `kjk` run names. The tour, the sweeps and the
//! saber animation audit all read it, and the audit is built without the
//! `probes` feature too.

use std::path::PathBuf;

/// The run folder a child `kjk` run made, from the line it logs.
pub fn run_folder(log: &str) -> Option<PathBuf> {
    log.lines().find_map(|line| {
        let lower = line.to_ascii_lowercase();
        let at = lower.find("run folder: ")?;
        Some(PathBuf::from(line[at + "run folder: ".len()..].trim()))
    })
}
