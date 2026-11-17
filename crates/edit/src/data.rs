//! What an install gives edits: every item it holds, as scripts know them,
//! and its classes, read without the game.

use std::sync::Arc;

use asset_odyssey::module::Source;
use fmt_odyssey::ResType;
use sim::inventory::Catalog;
use sim::levels::Classes;
use vfs::resolver::{Resolver, TextureQuality};
use vfs::{DirIndex, Discovery, Game};

/// An install's items and classes.
pub struct Data {
    /// Every UTI it lists (override's among them) that reads.
    pub catalog: Catalog,
    pub classes: Classes,
    /// The UTIs it lists that did not read, by name.
    pub unreadable: Vec<String>,
    /// The install's files, for what else a tool reads by name.
    pub source: InstallSource,
}

impl Data {
    /// `game`'s install as `discovery` found it, read; why not when it is
    /// not there or does not open.
    pub fn load(discovery: &Discovery, game: Game) -> Result<Data, String> {
        let source = InstallSource::open(discovery, game)?;
        let (catalog, unreadable) = session::item_catalog(&source);
        let classes = session::classes::classes(&source);
        Ok(Data {
            catalog,
            classes,
            unreadable,
            source,
        })
    }
}

/// An install's data (its shared data, a module's archives on it if one)
/// as a module reader's `Source`.
pub struct InstallSource(pub Resolver);

impl InstallSource {
    /// `game`'s install, or why not.
    pub fn open(discovery: &Discovery, game: Game) -> Result<InstallSource, String> {
        InstallSource::with_module(discovery, game, None)
    }

    /// `game`'s install with `module`'s archives on it (none: its shared
    /// data alone), or why not, naming the module.
    pub fn with_module(
        discovery: &Discovery,
        game: Game,
        module: Option<&str>,
    ) -> Result<InstallSource, String> {
        let install = discovery.get(game).ok_or(format!(
            "{}: no install found (set {})",
            game.key(),
            game.env_var()
        ))?;
        let index = DirIndex::of_install(install).map_err(|error| error.to_string())?;
        let mut base = Resolver::base(game, &index, TextureQuality::High)
            .map_err(|error| error.to_string())?;
        // As the game opens KOTOR 1: KOTOR 2's powers lent where this machine
        // has KOTOR 2, so a character's lent powers read as in the game.
        if game == Game::Kotor1
            && let Some(tsl) = discovery.get(Game::Kotor2)
            && let Ok(tsl) = DirIndex::of_install(tsl)
                .map_err(|error| error.to_string())
                .and_then(|index| {
                    Resolver::base(Game::Kotor2, &index, TextureQuality::High)
                        .map_err(|error| error.to_string())
                })
        {
            let _ = base.lend(&tsl);
        }
        match module {
            Some(module) => base
                .with_module(&index, module)
                .map(InstallSource)
                .map_err(|error| format!("{module}: {error}")),
            None => Ok(InstallSource(base)),
        }
    }
}

impl Source for InstallSource {
    fn read(&self, resref: &str, restype: ResType) -> Option<Vec<u8>> {
        self.0.find(resref, restype)?.location.read().ok()
    }

    fn lent_script(&self, resref: &str) -> bool {
        vfs::lent::lent_script(&self.0, resref)
    }

    fn exists(&self, resref: &str, restype: ResType) -> bool {
        self.0.find(resref, restype).is_some()
    }

    fn names(&self, restype: ResType) -> Vec<String> {
        self.0.names(restype)
    }

    fn talk(&self) -> Option<Arc<fmt_odyssey::tlk::Tlk>> {
        self.0.talk.get()
    }
}
