//! The library `kjk original` loads into the original KOTOR 1 or KOTOR 2,
//! and what kjk needs to know to start a game with it (`launch`).
//!
//! The library takes the game from its main menu to a module without a
//! click. It does what the developers' hidden warp list does when a module
//! is picked in it. It fills the warp list, picks the module and calls the
//! list's click handler. The game then starts the module with its default
//! player, named "test".
//!
//! The library acts once a frame, on the game's own thread, from the game's
//! calls to OpenGL's `glClear`. On macOS, dyld's interposing hands it those
//! calls (`mac`). On Windows, the library rewrites the game's import table
//! in memory (`windows`). The game's files are never changed.
//!
//! The library finds the game's functions and fields in the game's own code
//! on each run (the `original_find` crate). When something is not found,
//! the library does nothing, and its log says what was missing.
//!
//! The library is built only with the `game` feature, and it only acts when
//! built for x86_64 macOS or 32-bit Windows (`build.rs`).

pub mod launch;

#[cfg(feature = "game")]
mod game;
#[cfg(all(feature = "game", mac_port))]
mod mac;
#[cfg(all(feature = "game", windows_port))]
mod windows;
