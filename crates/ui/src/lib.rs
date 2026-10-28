//! KOTOR's screens without an engine or a sim: a layout and a view-model
//! in, a draw list and intents out. `probes` holds the checks the approved
//! test `gui_layouts` runs.

pub mod assets;
pub mod draw;
pub mod edit;
pub mod input;
pub mod intent;
pub mod paint;
#[cfg(feature = "probes")]
pub mod probes;
pub mod raster;
pub mod screen;
pub mod screens;
pub mod skin;
pub mod strings;
#[cfg(feature = "probes")]
pub mod sweep;
pub mod vm;
