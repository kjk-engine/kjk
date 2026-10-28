//! Game installs on disk: finding them, telling them apart, and looking files
//! up without caring about case. Installs are only ever read.

pub mod archive;
pub mod chitin;
mod dir_index;
mod discover;
pub mod fingerprint;
pub mod ja;
pub mod resolver;

pub use dir_index::{CaseCollision, DirIndex};
pub use discover::{Discovery, Install, Refusal, Source, Store, discover};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Game {
    Kotor1,
    Kotor2,
    JediAcademy,
}

impl Game {
    pub const ALL: [Game; 3] = [Game::Kotor1, Game::Kotor2, Game::JediAcademy];

    /// The `.env` / environment variable that points at this game's install.
    pub fn env_var(self) -> &'static str {
        match self {
            Game::Kotor1 => "KJK_KOTOR1",
            Game::Kotor2 => "KJK_KOTOR2",
            Game::JediAcademy => "KJK_JA",
        }
    }

    /// Short name used on the command line.
    pub fn key(self) -> &'static str {
        match self {
            Game::Kotor1 => "k1",
            Game::Kotor2 => "tsl",
            Game::JediAcademy => "ja",
        }
    }

    pub fn from_key(key: &str) -> Option<Game> {
        Game::ALL
            .into_iter()
            .find(|game| game.key().eq_ignore_ascii_case(key))
    }
}
