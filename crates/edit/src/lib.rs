//! A saved game opened for editing, changed by kjk's own rules, and
//! checked before it is written: what the save editor
//! (`docs/superpowers/specs/2026-11-04-save-editor-design.md`) stands on.
//! No game and no window, so the editor builds without Bevy's renderer.

mod characters;
mod data;
mod edits;
mod open;
mod sheets;
mod warnings;

pub use characters::{Character, Who, characters, unsheeted};
pub use data::{Data, InstallSource};
pub use edits::Editor;
pub use open::{Opened, Refusal, open, open_bytes};
pub use warnings::{feat_warning, power_warning};

#[cfg(feature = "probes")]
pub mod probes;
