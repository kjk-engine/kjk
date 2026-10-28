//! Jedi Academy file formats. Pure parsers over `&[u8]`: this crate opens no
//! files.

pub mod animation_cfg;
pub mod efx;
pub mod ghoul2;
pub mod md3;
pub mod pk3;
pub mod script;
pub mod shader;

/// A path as the games look it up: either slash, any case (Quake's file
/// system, and the Odyssey installs' folders, are case-blind): `/` and
/// lower case.
pub fn game_path(path: &str) -> String {
    path.replace('\\', "/").to_ascii_lowercase()
}
