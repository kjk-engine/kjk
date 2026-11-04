//! kjk's save editor (spec: `docs/superpowers/specs/2026-11-04-save-editor-design.md`):
//! the model under its window. Finding saves and installs, the game's names
//! and icons, the editing session with its changes and undo, and the write
//! after its checks, all without a window; the window is the binary's
//! (`src/ui`, the `app` feature).

pub mod model;

#[cfg(feature = "probes")]
pub mod probes;

/// The release this editor is from (the latest `v*` tag), as kjk's own.
pub const VERSION: &str = env!("KJK_VERSION");
