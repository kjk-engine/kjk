//! The installs, each opened once a process and shared: its files' index,
//! its fingerprint (from the cache alone) and its base resolver (`chitin.key`
//! and the BIFs' tables, the global archives, `dialog.tlk` when first
//! asked for). The installers, the game's screens, the HUD, the dialogue
//! view and the play's bodies all stack on the same layers. `warm` opens
//! them on threads of their own at launch, so the window comes up while
//! they read.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Instant;

use fmt_odyssey::tlk::Tlk;
use vfs::fingerprint::Fingerprint;
use vfs::resolver::{Resolver, TextureQuality};
use vfs::{DirIndex, Discovery, Game};

/// One install, opened.
pub struct Install {
    pub game: Game,
    pub index: Arc<DirIndex>,
    /// Milliseconds: walking the install's files for the index.
    pub index_ms: f64,
    fingerprint: OnceLock<Fingerprint>,
    base: OnceLock<(Result<Resolver, String>, f64)>,
}

type Opening = Arc<Mutex<Option<Arc<Install>>>>;

/// Every install opened (or being opened), by its data folder.
static OPENED: Mutex<BTreeMap<PathBuf, Opening>> = Mutex::new(BTreeMap::new());

/// `game`'s install, opened once a process (a caller asking while another
/// thread opens it waits for that one; one that failed is tried again).
pub fn open(discovery: &Discovery, game: Game) -> Result<Arc<Install>, String> {
    let found = discovery.get(game).ok_or(format!(
        "{}: not found (set {})",
        game.key(),
        game.env_var()
    ))?;
    opened(found)
}

/// The campaigns this machine may begin a new game of: the KOTOR games
/// found, K1's first.
pub fn campaigns(discovery: &Discovery) -> Vec<Game> {
    [Game::Kotor1, Game::Kotor2]
        .into_iter()
        .filter(|game| discovery.get(*game).is_some())
        .collect()
}

/// The KOTOR game whose install has module `module` (`modules/` holds
/// its `.rim` or `.mod`): K1 if it has one, else TSL if it does, else K1
/// (whose installer then says the module is missing). No module name is
/// in both games.
pub fn game_of_module(discovery: &Discovery, module: &str) -> Game {
    let has = |game| {
        open(discovery, game).is_ok_and(|install| {
            Resolver::modules(&install.index)
                .iter()
                .any(|name| name.eq_ignore_ascii_case(module))
        })
    };
    [Game::Kotor1, Game::Kotor2]
        .into_iter()
        .find(|game| has(*game))
        .unwrap_or(Game::Kotor1)
}

fn opened(found: &vfs::Install) -> Result<Arc<Install>, String> {
    let opening = OPENED
        .lock()
        .map_err(|_| "the installs' lock".to_string())?
        .entry(found.data_root.clone())
        .or_default()
        .clone();
    let mut opening = opening
        .lock()
        .map_err(|_| "an install's lock".to_string())?;
    if let Some(install) = &*opening {
        return Ok(install.clone());
    }
    let started = Instant::now();
    let index = DirIndex::build(&found.data_root).map_err(|error| error.to_string())?;
    let install = Arc::new(Install {
        game: found.game,
        index: Arc::new(index),
        index_ms: started.elapsed().as_secs_f64() * 1000.0,
        fingerprint: OnceLock::new(),
        base: OnceLock::new(),
    });
    *opening = Some(install.clone());
    Ok(install)
}

/// Opens every found install on a thread of its own: its index and its
/// fingerprint (the run's manifest lists them), and KOTOR 1's base
/// resolver (the game played), ready when first asked for.
pub fn warm(discovery: &Discovery) {
    for found in &discovery.installs {
        let found = found.clone();
        std::thread::spawn(move || {
            if let Ok(install) = opened(&found) {
                install.fingerprint();
                if install.game == Game::Kotor1 {
                    let _ = install.base();
                }
            }
        });
    }
}

impl Install {
    /// Its fingerprint from the cache alone (no file is hashed): `None`
    /// when a file changed since it was last hashed, asked again next time
    /// (a host or a joining player hashes it before it plans).
    pub fn fingerprint(&self) -> Option<&Fingerprint> {
        if let Some(print) = self.fingerprint.get() {
            return Some(print);
        }
        let cache = crate::fingerprint_cache(self.game);
        let print = vfs::fingerprint::fingerprint_cached(self.game, &self.index, &cache).ok()??;
        Some(self.fingerprint.get_or_init(|| print))
    }

    /// The resolver without a module, opened once.
    pub fn base(&self) -> Result<&Resolver, String> {
        self.opened_base().0.as_ref().map_err(Clone::clone)
    }

    /// Milliseconds the base resolver took to open, once it has.
    pub fn base_ms(&self) -> Option<f64> {
        self.base.get().map(|(_, ms)| *ms)
    }

    fn opened_base(&self) -> &(Result<Resolver, String>, f64) {
        self.base.get_or_init(|| {
            let started = Instant::now();
            let base = Resolver::base(self.game, &self.index, TextureQuality::High)
                .map_err(|error| error.to_string());
            (base, started.elapsed().as_secs_f64() * 1000.0)
        })
    }

    /// The base resolver with `module`'s archives on top (the base itself
    /// without one).
    pub fn resolver(&self, module: Option<&str>) -> Result<Resolver, String> {
        let base = self.base()?;
        match module {
            Some(module) => base
                .with_module(&self.index, module)
                .map_err(|error| error.to_string()),
            None => Ok(base.clone()),
        }
    }

    /// `dialog.tlk`, read once for every resolver of the install.
    pub fn talk(&self) -> Option<Arc<Tlk>> {
        self.base().ok()?.talk.get()
    }
}
