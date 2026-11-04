//! A saved game opened for editing, changed by kjk's own rules, and
//! checked before it is written: what the save editor
//! (`docs/superpowers/specs/2026-11-04-save-editor-design.md`) stands on.
//! No game and no window, so the editor builds without Bevy's renderer.

mod data;
mod open;

pub use data::{Data, InstallSource};
pub use open::{Opened, Refusal, open, open_bytes};

#[cfg(feature = "probes")]
pub mod probes;
