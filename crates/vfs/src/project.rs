//! A level editor project's folder read over an install (`level_edit`,
//! `docs/LEVEL-EDIT.md`).
//!
//! A run that plays or edits a project activates it once, before it opens
//! any install. Every resolver of the project's game opened after that
//! (`Resolver::base`) has the folder as its highest file layer: over
//! `override/` and over every Steam Workshop item's. So the area's changed
//! GIT and any template the project brings win, as a mod's file in
//! `override/` would. Only the folder's top level is read, and only files
//! named as a resource type other than TXT. The project's own
//! `project.toml` and `kjk-area.toml` are not resources and are not read.
//!
//! The folder is never written by a resolver. The co-op fingerprint does
//! not cover it: two machines must play the same project by hand.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use fmt_odyssey::ResType;

use crate::Game;
use crate::resolver::{Layer, Location, Rank};

/// The layer's name in `kjk ls --shadowed`.
pub const LAYER: &str = "project";

static ACTIVE: OnceLock<(Game, PathBuf)> = OnceLock::new();

/// Makes `folder` the project of every `game` resolver opened from now on.
/// Refused when a project is already active.
pub fn activate(game: Game, folder: &Path) -> Result<(), String> {
    ACTIVE
        .set((game, folder.to_path_buf()))
        .map_err(|_| "a project is already active".to_string())
}

/// The active project's game and folder.
pub fn active() -> Option<(Game, &'static Path)> {
    ACTIVE.get().map(|(game, folder)| (*game, folder.as_path()))
}

/// The active project's folder as a layer, when the project is `game`'s and
/// its folder holds any resource.
pub(crate) fn layer(game: Game) -> Option<Layer> {
    let (active, folder) = active()?;
    if active != game {
        return None;
    }
    let mut resources = BTreeMap::new();
    let mut names: Vec<(String, PathBuf)> = std::fs::read_dir(folder)
        .ok()?
        .filter_map(Result::ok)
        .filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_file()))
        .map(|entry| {
            (
                entry.file_name().to_string_lossy().to_ascii_lowercase(),
                entry.path(),
            )
        })
        .collect();
    names.sort();
    for (name, path) in names {
        let Some((resref, extension)) = name.rsplit_once('.') else {
            continue;
        };
        let Some(restype) = ResType::from_extension(extension) else {
            continue;
        };
        if restype == ResType::TXT || crate::system_file(&name) {
            continue;
        }
        let size = std::fs::metadata(&path).map_or(0, |meta| meta.len());
        resources.insert(
            (resref.to_string(), restype),
            Location::on_disk(path, 0, size),
        );
    }
    (!resources.is_empty()).then(|| Layer {
        name: LAYER.into(),
        rank: Rank::Directory,
        resources,
    })
}
