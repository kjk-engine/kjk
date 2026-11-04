//! Where the saves and the games' installs are, found as kjk finds them,
//! and what a folder someone picks is, told in plain words.

use std::collections::BTreeMap;
use std::fmt;
use std::path::{Path, PathBuf};

use vfs::Game;

/// A save as the list shows it, and whether it opens.
#[derive(Debug, Clone)]
pub struct SaveRow {
    pub id: String,
    pub name: String,
    pub module: String,
    /// When it was saved, as kjk's screens show it (UTC).
    pub saved: String,
    /// Its game, when its meta names one.
    pub game: Option<Game>,
    /// Why it does not open, if it does not.
    pub readable: Result<(), edit::Refusal>,
}

/// What a folder is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Found {
    Saves { count: usize, unreadable: usize },
    Install { game: Game, override_items: usize },
    Nothing(String),
}

/// A game's short name, as players say it.
pub fn game_name(game: Game) -> &'static str {
    match game {
        Game::Kotor1 => "K1",
        Game::Kotor2 => "TSL",
        Game::JediAcademy => "Jedi Academy",
    }
}

impl fmt::Display for Found {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Found::Saves { count, unreadable } => {
                let plural = if *count == 1 { "" } else { "s" };
                write!(f, "kjk saves folder, {count} save{plural}")?;
                if *unreadable > 0 {
                    write!(f, " ({unreadable} unreadable)")?;
                }
                Ok(())
            }
            Found::Install {
                game,
                override_items,
            } => write!(
                f,
                "{} install, override has {override_items} item{}",
                game_name(*game),
                if *override_items == 1 { "" } else { "s" }
            ),
            Found::Nothing(why) => f.write_str(why),
        }
    }
}

/// kjk's saves folder, as kjk finds it (`KJK_SAVES`, else `saves` in kjk's
/// folder).
pub fn default_saves() -> Option<PathBuf> {
    saves::folder(env)
}

fn env(name: &str) -> Option<String> {
    std::env::var(name).ok()
}

/// The installs as kjk finds them: from the environment, else the folders
/// chosen in kjk's settings, else the Steam and GOG folders.
pub fn discovery() -> vfs::Discovery {
    let settings = std::env::var_os("KJK_SETTINGS")
        .map(PathBuf::from)
        .or_else(|| store::kjk_folder(env).map(|folder| folder.join("settings.json")));
    discovery_with(env, settings.as_deref())
}

/// The installs `var` names, else those chosen in the settings file
/// `settings` (`install.k1`, …), else the Steam and GOG folders.
pub fn discovery_with(
    var: impl Fn(&str) -> Option<String>,
    settings: Option<&Path>,
) -> vfs::Discovery {
    let values: BTreeMap<String, String> = settings
        .and_then(|path| std::fs::read_to_string(path).ok())
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_else(BTreeMap::new);
    vfs::discover(|name| {
        var(name).or_else(|| {
            let game = Game::ALL.into_iter().find(|game| game.env_var() == name)?;
            values.get(&format!("install.{}", game.key())).cloned()
        })
    })
}

/// The KOTOR installs found, K1 first: the ones the editor reads.
pub fn installs(discovery: &vfs::Discovery) -> Vec<(Game, PathBuf)> {
    [Game::Kotor1, Game::Kotor2]
        .into_iter()
        .filter_map(|game| Some((game, discovery.get(game)?.root.clone())))
        .collect()
}

/// The save ids in `root`: the folders kjk would list.
fn ids(root: &Path) -> Vec<String> {
    std::fs::read_dir(root)
        .into_iter()
        .flatten()
        .flatten()
        .filter(|entry| entry.path().is_dir())
        .filter_map(|entry| entry.file_name().to_str().map(str::to_string))
        .filter(|id| saves::valid_id(id))
        .collect()
}

/// What `path` is: a saves folder (how many saves, how many unreadable), a
/// KOTOR install (which, and how many items its override holds), or why it
/// is neither.
pub fn check_path(path: &Path) -> Found {
    if !path.is_dir() {
        return Found::Nothing(format!("no such folder: {}", path.display()));
    }
    for game in [Game::Kotor1, Game::Kotor2] {
        if let Ok(install) = vfs::install_at(game, path) {
            return Found::Install {
                game,
                override_items: override_items(&install.data_root),
            };
        }
    }
    if path.join("save.kjkv").is_file() {
        return Found::Nothing(
            "this is one save: pick the folder that holds it (kjk's saves folder)".into(),
        );
    }
    let rows = list(path);
    if rows.is_empty() {
        return Found::Nothing("not a saves folder: no save.kjkv inside".into());
    }
    Found::Saves {
        count: rows.len(),
        unreadable: rows.iter().filter(|row| row.readable.is_err()).count(),
    }
}

/// The items (UTIs) in an install's `override` folder.
fn override_items(data_root: &Path) -> usize {
    let folder = std::fs::read_dir(data_root)
        .into_iter()
        .flatten()
        .flatten()
        .find(|entry| entry.file_name().eq_ignore_ascii_case("override"));
    folder
        .and_then(|folder| std::fs::read_dir(folder.path()).ok())
        .into_iter()
        .flatten()
        .flatten()
        .filter(|entry| {
            Path::new(&entry.file_name())
                .extension()
                .is_some_and(|extension| extension.eq_ignore_ascii_case("uti"))
        })
        .count()
}

/// Every save in `root`, the latest first, each opened to tell whether it
/// reads (a save that does not is listed with why, never half-opened).
pub fn list(root: &Path) -> Vec<SaveRow> {
    let mut rows: Vec<(u64, SaveRow)> = ids(root)
        .into_iter()
        .filter(|id| {
            let folder = root.join(id);
            folder.join("save.kjkv").is_file() || folder.join("meta.json").is_file()
        })
        .map(|id| {
            let (meta, readable) = match edit::open(root, &id) {
                Ok(opened) => (opened.meta, Ok(())),
                Err(refusal) => (meta_of(&root.join(&id)), Err(refusal)),
            };
            let row = SaveRow {
                name: if meta.name.is_empty() {
                    format!("save {id}")
                } else {
                    meta.name.clone()
                },
                module: meta.module.clone(),
                saved: when(meta.saved),
                game: meta.game.as_deref().and_then(Game::from_key),
                readable,
                id,
            };
            (meta.saved, row)
        })
        .collect();
    rows.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.id.cmp(&b.1.id)));
    rows.into_iter().map(|(_, row)| row).collect()
}

/// A save folder's meta as far as it reads (none: empty).
fn meta_of(folder: &Path) -> saves::Meta {
    std::fs::read_to_string(folder.join("meta.json"))
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_else(saves::Meta::default)
}

/// When `saved` (Unix seconds) was, as kjk's screens show it (UTC).
pub fn when(saved: u64) -> String {
    let (days, seconds) = (saved / 86_400, saved % 86_400);
    let (year, month, day) = saves::civil_from_days(days as i64);
    format!(
        "{year:04}-{month:02}-{day:02} {:02}:{:02}",
        seconds / 3600,
        seconds / 60 % 60
    )
}
