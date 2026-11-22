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
    /// What the player is told about the install's Steam Workshop items and
    /// Jedi Academy's mod folder (`vfs::Install::workshop_note`), logged
    /// with the base resolver (`log_streams`): discovery runs before the
    /// log is set up, so a line logged there is lost.
    notes: Vec<String>,
    fingerprint: OnceLock<Fingerprint>,
    base: OnceLock<(Result<Resolver, String>, f64)>,
}

type Opening = Arc<Mutex<Option<Arc<Install>>>>;

/// Every install opened (or being opened), by its data folder.
static OPENED: Mutex<BTreeMap<PathBuf, Opening>> = Mutex::new(BTreeMap::new());

/// `game`'s install, opened once a process (a caller asking while another
/// thread opens it waits for that one; one that failed is tried again).
/// A game not found says why its folder was refused, when one was.
pub fn open(discovery: &Discovery, game: Game) -> Result<Arc<Install>, String> {
    let found = discovery
        .get(game)
        .ok_or_else(|| match discovery.refusal(game) {
            Some(refusal) => format!("{}: {}", refusal.path.display(), refusal.reason),
            None => format!("{}: not found (set {})", game.key(), game.env_var()),
        })?;
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
    let index = DirIndex::of_install(found).map_err(|error| error.to_string())?;
    let install = Arc::new(Install {
        game: found.game,
        index: Arc::new(index),
        index_ms: started.elapsed().as_secs_f64() * 1000.0,
        notes: [found.workshop_note(), found.ja_mod_note()]
            .into_iter()
            .flatten()
            .collect(),
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

/// Logs the install's notes (its Workshop items in load order, those past
/// the limit, a mod folder) and how many files each stream folder of a
/// KOTOR install holds (music, sounds, voices and movies). It warns when
/// the install has no music files, since every area's music and ambience
/// is then silent.
fn log_streams(game: Game, notes: &[String], base: &Resolver) {
    for note in notes {
        bevy::log::warn!(target: "installs", game = game.key(), "{note}");
    }
    log_texture_packs(game, base);
    log_story_files(game, base);
    for layer in &base.streams {
        bevy::log::info!(
            target: "installs",
            game = game.key(),
            "stream folder {}/ holds {} files",
            layer.name,
            layer.resources.len()
        );
    }
    let music = base
        .streams
        .iter()
        .find(|layer| layer.name == "streammusic")
        .map_or(0, |layer| layer.resources.len());
    if music == 0 {
        bevy::log::warn!(
            target: "installs",
            game = game.key(),
            "no music: the install's streammusic folder is missing or holds no sound files"
        );
    }
}

/// Logs how many files each texture pack the base resolver reads holds, and
/// warns when one is missing. The game's own textures for the menus, the
/// areas and the characters are in these two archives, so without them the
/// game draws untextured meshes and particles as plain squares. It also
/// warns of every archive of the install that was there but did not open.
fn log_texture_packs(game: Game, base: &Resolver) {
    for pack in ["swpc_tex_gui.erf", TextureQuality::High.pack()] {
        let name = format!("texturepacks/{pack}");
        match base.layers.iter().find(|layer| layer.name == name) {
            Some(layer) => bevy::log::info!(
                target: "installs",
                game = game.key(),
                "texture pack {name} holds {} files",
                layer.resources.len()
            ),
            None => bevy::log::warn!(
                target: "installs",
                game = game.key(),
                "texture pack {name} is missing or does not open, so the textures in it are drawn as missing"
            ),
        }
    }
    let archive = |source: &str| {
        [".erf", ".rim", ".mod", ".key"]
            .iter()
            .any(|ext| source.to_ascii_lowercase().ends_with(ext))
    };
    for problem in base.problems.iter().filter(|p| archive(&p.source)) {
        bevy::log::warn!(
            target: "installs",
            game = game.key(),
            "{} was not read: {}",
            problem.source,
            problem.problem
        );
    }
}

/// Logs where the story's shared files come from and whether they read:
/// the journal (`global.jrl`), the global catalogue (`globalcat.2da`) and
/// the talk table (`dialog.tlk`). A mod's copy of one of them decides what
/// the journal shows and which globals the scripts may set, so a report
/// names the copy that won (the game's own, the override folder's or a
/// Workshop item's) and the copies it hides.
fn log_story_files(game: Game, base: &Resolver) {
    use fmt_odyssey::ResType;
    /// Whether a file's bytes read, else why not.
    type Reads = fn(&[u8]) -> Result<(), String>;
    let checks: [(&str, &str, ResType, Reads); 2] = [
        ("global.jrl", "global", ResType::JRL, |bytes| {
            fmt_odyssey::gff::Gff::parse(bytes)
                .map(drop)
                .map_err(|error| error.to_string())
        }),
        ("globalcat.2da", "globalcat", ResType::TWO_DA, |bytes| {
            fmt_odyssey::twoda::TwoDa::parse(bytes)
                .map(drop)
                .map_err(|error| error.to_string())
        }),
    ];
    for (file, name, restype, parse) in checks {
        let Some(found) = base.find(name, restype) else {
            bevy::log::warn!(target: "installs", game = game.key(), "{file}: the install has none");
            continue;
        };
        let hidden: Vec<&str> = found
            .shadowed
            .iter()
            .map(|(layer, _)| layer.name.as_str())
            .collect();
        let read = found
            .location
            .read()
            .map_err(|error| error.to_string())
            .and_then(|bytes| parse(&bytes));
        let source = &found.layer.name;
        match read {
            Ok(()) => bevy::log::info!(
                target: "installs",
                game = game.key(),
                "{file} is read from {source}; it hides {hidden:?}"
            ),
            Err(error) => bevy::log::warn!(
                target: "installs",
                game = game.key(),
                "{file} from {source} does not read: {error}; it hides {hidden:?}"
            ),
        }
    }
    let talk = base.talk.path().map(|path| path.display().to_string());
    match (talk, base.talk.get()) {
        (Some(path), Some(_)) => {
            bevy::log::info!(target: "installs", game = game.key(), "dialog.tlk is read from {path}");
        }
        (Some(path), None) => {
            bevy::log::warn!(target: "installs", game = game.key(), "dialog.tlk from {path} does not read");
        }
        (None, _) => {
            bevy::log::warn!(target: "installs", game = game.key(), "dialog.tlk: the install has none");
        }
    }
}

/// KOTOR 2's Force powers and feats lent to KOTOR 1's `base`
/// (`Resolver::lend`), from KOTOR 2's install, which kjk requires. It logs
/// which rows were added, and why a table got none.
fn lend_kotor2_powers(base: &mut Resolver) {
    let tsl = open(&crate::discover(), Game::Kotor2).and_then(|install| install.resolver(None));
    let lent = tsl.and_then(|tsl| base.lend(&tsl));
    match lent {
        Ok(lent) => {
            bevy::log::info!(
                target: "installs",
                game = "k1",
                "KOTOR 2's Force powers and feats added to KOTOR 1: spells.2da rows {:?}, feat.2da rows {:?}",
                lent.rows(),
                lent.feat_rows()
            );
            for why in &lent.refused {
                bevy::log::warn!(target: "installs", game = "k1", "{why}");
            }
        }
        Err(why) => bevy::log::warn!(
            target: "installs",
            game = "k1",
            "KOTOR 2's Force powers and feats are not added to KOTOR 1: {why}"
        ),
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

    /// The world data two machines compare (`Fingerprint::world`), with
    /// what KOTOR 2 lends KOTOR 1's data in it (`WorldData::with_lent`) and
    /// what the mods for kjk, or a level editor project, change beyond the
    /// games' own files (`WorldData::with_kjk`).
    pub fn world(&self, print: &Fingerprint) -> vfs::fingerprint::WorldData {
        let lent = self.base().ok().and_then(Resolver::lent_digest);
        let kjk = level_edit::active::world_digest(self.game.key());
        print.world().with_lent(lent).with_kjk(kjk)
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
            let mut base = Resolver::base(self.game, &self.index, TextureQuality::High)
                .map_err(|error| error.to_string());
            if let Ok(base) = &mut base {
                log_streams(self.game, &self.notes, base);
                if self.game == Game::Kotor1 {
                    lend_kotor2_powers(base);
                }
            }
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
