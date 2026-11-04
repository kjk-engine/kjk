//! A saved game opened for editing, changed by kjk's own rules, and
//! checked before it is written: what the save editor
//! (`docs/superpowers/specs/2026-11-04-save-editor-design.md`) stands on.
//! No game and no window, so the editor builds without Bevy's renderer.
//! `docs/EDIT.md` tells what it offers, what is authority and what a view,
//! why party changes are queued, and which save formats it reads.

mod characters;
mod check;
mod data;
mod edits;
mod open;
mod party;
mod sheets;
mod warnings;

pub use characters::{Character, Who, characters, unsheeted};
pub use check::{Checked, load_and_step, round_trip};
pub use data::{Data, InstallSource};
pub use edits::Editor;
pub use open::{Opened, Refusal, open, open_bytes};
pub use party::{PartyChange, pending_party};
pub use warnings::{feat_warning, power_warning};

#[cfg(feature = "probes")]
pub mod probes;
