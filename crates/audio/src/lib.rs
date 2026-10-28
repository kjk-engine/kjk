//! Sound for the view: clips decoded from the installs' files, placed as
//! Jedi Academy places a sound (`spatial`), beds (music, ambience) kept
//! coded and decoded as they play (`Stream`), mixed by kira, to the sound
//! device or captured on the game's clock (`Mixer`), so a headless run has
//! its mix as a WAV file (`wav`).
//!
//! The sim never hears anything: what sounds is what the view reads from
//! the presented snapshots.

mod clip;
mod decode;
mod mixer;
pub mod spatial;
mod stream;
pub mod wav;

pub use clip::Clip;
pub use mixer::{Bed, Bus, CAPTURE_RATE, Channel, Mixer, Output, Sound};
pub use spatial::{Emitter, Falloff, Listener};
pub use stream::Stream;
