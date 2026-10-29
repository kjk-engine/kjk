//! The saves' safety net: autosaves in three slots, and the backup of the
//! whole saves' folder before a new release first touches it
//! (`docs/superpowers/specs/2026-10-28-safety-net-recorder-design.md`).

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::Meta;

/// The formats a save was written in (`sim::save::FORMAT`,
/// `sim::snapshot::FORMAT`).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Formats {
    pub save: u32,
    pub snapshot: u32,
}

/// The autosaves' slots, the newest first.
pub const AUTOS: [&str; 3] = ["auto", "auto-2", "auto-3"];

/// The folder beside the saves' that the backups go in (the one the
/// save-compat skill keeps its own in).
pub const BACKUPS: &str = "save-backups";
/// A backup made for a new release is named from it (`update-v0.1.0-…`).
pub(crate) const UPDATE: &str = "update-";
/// How many of those are kept.
pub(crate) const KEPT_BACKUPS: usize = 5;
/// The release and commit that last wrote the saves' folder.
pub(crate) const STAMP: &str = ".build";

/// `world` written as the newest autosave, the earlier two moved down a
/// slot (the oldest gone); its id.
pub fn write_auto(root: &Path, world: &[u8], meta: &Meta) -> Result<String, String> {
    std::fs::create_dir_all(root).map_err(crate::failed(root))?;
    for pair in (0..AUTOS.len() - 1).rev() {
        let (from, to) = (root.join(AUTOS[pair]), root.join(AUTOS[pair + 1]));
        if !from.exists() {
            continue;
        }
        if to.exists() {
            std::fs::remove_dir_all(&to).map_err(crate::failed(&to))?;
        }
        std::fs::rename(&from, &to).map_err(crate::failed(&from))?;
    }
    crate::write_encoded(root, Some(AUTOS[0]), world, meta)
}

/// Before `version` (`build`) first touches the saves in `root`: when the
/// saves were last written by another release, `root` copied whole into
/// `backups` (`update-<old release>-<old commit>-<date>`), the newest
/// `KEPT_BACKUPS` of those kept; then `root` stamped as this release's.
/// The backup made, if any. With no saves there is nothing to keep.
pub fn backup_on_new_version(
    root: &Path,
    backups: &Path,
    version: &str,
    build: &str,
) -> Result<Option<PathBuf>, String> {
    let at = crate::failed;
    let stamp = root.join(STAMP);
    let last = std::fs::read_to_string(&stamp).ok();
    let last = last.as_deref().map(str::trim);
    let ours = format!("{version} {build}");
    if last.and_then(|last| last.split(' ').next()) == Some(version) {
        return Ok(None);
    }
    let saved = crate::list(root);
    let made = if saved.saves.is_empty() && saved.unread.is_empty() {
        None
    } else {
        let (old, commit) = last
            .and_then(|last| last.split_once(' '))
            .unwrap_or(("unknown", "unknown"));
        let days = crate::now() / 86_400;
        let (year, month, day) = crate::civil_from_days(days as i64);
        let name = format!(
            "{UPDATE}{old}-{}-{year:04}{month:02}{day:02}",
            &commit[..8.min(commit.len())]
        );
        let mut to = backups.join(&name);
        for n in 2.. {
            if !to.exists() {
                break;
            }
            to = backups.join(format!("{name}-{n}"));
        }
        // Copied aside and renamed: a copy that fails (a full disk) leaves
        // no backup that looks whole.
        let partial = backups.join(format!("{name}.new"));
        let _ = std::fs::remove_dir_all(&partial);
        if let Err(error) = copy_tree(root, &partial) {
            let _ = std::fs::remove_dir_all(&partial);
            return Err(error);
        }
        std::fs::rename(&partial, &to).map_err(at(&to))?;
        prune(backups);
        Some(to)
    };
    std::fs::create_dir_all(root).map_err(at(root))?;
    std::fs::write(&stamp, ours).map_err(at(&stamp))?;
    Ok(made)
}

/// `from` copied into `to`, folders and all.
fn copy_tree(from: &Path, to: &Path) -> Result<(), String> {
    let at = crate::failed;
    std::fs::create_dir_all(to).map_err(at(to))?;
    for entry in std::fs::read_dir(from).map_err(at(from))?.flatten() {
        let (source, target) = (entry.path(), to.join(entry.file_name()));
        if source.is_dir() {
            copy_tree(&source, &target)?;
        } else {
            std::fs::copy(&source, &target).map_err(at(&source))?;
        }
    }
    Ok(())
}

/// The newest `KEPT_BACKUPS` of the release backups in `backups` kept, by
/// when they were made; the save-compat skill's own are not touched.
fn prune(backups: &Path) {
    let Ok(entries) = std::fs::read_dir(backups) else {
        return;
    };
    let mut updates: Vec<(std::time::SystemTime, PathBuf)> = entries
        .flatten()
        .filter(|entry| entry.file_name().to_string_lossy().starts_with(UPDATE))
        .filter_map(|entry| Some((entry.metadata().ok()?.modified().ok()?, entry.path())))
        .collect();
    updates.sort();
    let old = updates.len().saturating_sub(KEPT_BACKUPS);
    for (_, path) in &updates[..old] {
        let _ = std::fs::remove_dir_all(path);
    }
}

#[cfg(feature = "probes")]
mod probes;
#[cfg(feature = "probes")]
pub use probes::probes;
