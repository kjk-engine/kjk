//! A character's look as its alignment makes it, as both executables dress
//! one. As a character falls to the dark side, its face (`heads.2da`), its
//! portrait (`portraits.2da`) and, without armour, its body
//! (`appearance.2da`) take darker textures in steps
//! (`rules::combat::sheet::DARK_LOOKS`, `rules::combat::sheet::DARK_BODY`;
//! `asset_odyssey::creature::align`).
//!
//! The executables apply it whenever a creature's appearance is set: when
//! it is made, and when it is dressed again. kjk builds every look with
//! it (`viewer::CreatureLoad`, `persona::wear`), and dresses a character
//! again when its step changes, as it does when its armour changes
//! (`made::guise` for any creature, `wardrobe` for a player). The HUD, the
//! character sheet and the party selection show the portrait of the step.

use asset_odyssey::creature::{self, Aligned};

/// How KOTOR's executable (`tsl` false) or KOTOR 2's dresses a character of
/// alignment `good_evil` (0 dark to 100 light).
pub fn aligned(tsl: bool, good_evil: u8) -> Aligned {
    Aligned {
        step: creature::dark_step(good_evil, rules::combat::sheet::DARK_LOOKS),
        evil_body: good_evil <= rules::combat::sheet::DARK_BODY[usize::from(tsl)],
        alt_texture: tsl,
    }
}

/// `aligned` for a creature whose alignment is not known: a new
/// character's.
pub fn neutral(tsl: bool) -> Aligned {
    aligned(tsl, sim::combat::sheet::NEUTRAL)
}

/// `aligned` in `game`.
pub fn in_game(game: vfs::Game, good_evil: u8) -> Aligned {
    aligned(game == vfs::Game::Kotor2, good_evil)
}

/// `aligned` in the game of `level`.
pub fn in_level(level: &sim::Level, good_evil: u8) -> Aligned {
    aligned(level.game() == sim::combat::passive::Game::Tsl, good_evil)
}

#[cfg(feature = "probes")]
pub mod probes;
