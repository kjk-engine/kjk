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

/// An install's shared data (no module) as a module reader's `Source`.
pub struct InstallSource(pub Resolver);

impl InstallSource {
    /// `game`'s install, or why not.
    pub fn open(discovery: &Discovery, game: Game) -> Result<InstallSource, String> {
        let install = discovery.get(game).ok_or(format!(
            "{}: no install found (set {})",
            game.key(),
            game.env_var()
        ))?;
        let index = DirIndex::build(&install.data_root).map_err(|error| error.to_string())?;
        Resolver::open(game, &index, None, TextureQuality::High)
            .map(InstallSource)
            .map_err(|error| error.to_string())
    }
}

impl Source for InstallSource {
    fn read(&self, resref: &str, restype: ResType) -> Option<Vec<u8>> {
        self.0.find(resref, restype)?.location.read().ok()
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
