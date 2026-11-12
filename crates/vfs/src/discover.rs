use std::path::{Path, PathBuf};

use crate::Game;
use crate::ja::BASE;

#[derive(Debug, Clone)]
pub struct Install {
    pub game: Game,
    /// The folder the player installed to.
    pub root: PathBuf,
    /// The folder the game reads its data from: `root` for KOTOR, the folder
    /// holding `base/` for Jedi Academy, `steamassets/` for TSL's Linux
    /// port, or a folder inside the `.app` bundle for a macOS port.
    pub data_root: PathBuf,
    pub source: Source,
    pub store: Store,
    /// Files and folders that tell builds apart, as found in `root` or
    /// `data_root` (lowercase).
    pub markers: Vec<&'static str>,
    /// Whether the Steam Workshop items the game loads are played
    /// (`workshop_items`); `KJK_WORKSHOP=off` plays the game folder alone.
    pub workshop: bool,
    /// Jedi Academy's mod folder, as `KJK_JA_MOD` names it (`MD`): a folder
    /// beside `base/` whose archives load after `base/`'s (`ja_mod_folder`).
    /// `None` for the KOTOR games and when nothing names one (Movie Duels'
    /// folder is then found by itself).
    pub ja_mod: Option<String>,
}

/// The variable that turns the Workshop items off (`off`, `0`, `no` or
/// `false`).
pub const WORKSHOP_VAR: &str = "KJK_WORKSHOP";

/// The variable naming Jedi Academy's mod folder (`MD`), beside `base/`.
pub const JA_MOD_VAR: &str = "KJK_JA_MOD";

/// Movie Duels' folder beside `base/`, lowercased: used without a setting
/// when it holds Movie Duels' archives (`movie_duels_in`).
pub const MOVIE_DUELS: &str = "md";

/// Whether a file name, in any case, is one of Movie Duels' archives
/// (`MD_Assets.pk3`, `md_hilts.PK3`). Detection and the archives' loading
/// (`ja::JaStack::open`) both go by this name, wherever the archives lie.
pub fn movie_duels_archive(name: &str) -> bool {
    let name = name.to_ascii_lowercase();
    name.starts_with("md_") && name.ends_with(".pk3")
}

/// Whether `folder` holds Movie Duels' archives (`movie_duels_archive`) at
/// its root.
fn movie_duels_in(folder: &Path) -> bool {
    std::fs::read_dir(folder)
        .into_iter()
        .flatten()
        .flatten()
        .any(|entry| {
            movie_duels_archive(&entry.file_name().to_string_lossy()) && entry.path().is_file()
        })
}

/// The most subscribed items the game asks Steam for (`LoadSteamMods`).
pub const MAX_WORKSHOP_ITEMS: usize = 30;

/// A Steam Workshop item the game loads: its id and its folder, laid out
/// like the game's data folder (`override/`, `modules/`, `dialog.tlk`, …).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkshopItem {
    pub id: u64,
    pub folder: PathBuf,
}

/// The Workshop items found for an install, and where their order came
/// from.
#[derive(Debug, Clone, Default)]
pub struct WorkshopItems {
    /// In load order, lowest first: the last one wins (`DirIndex::workshop`).
    pub items: Vec<WorkshopItem>,
    /// Steam's subscription list the order is read from, or `None` when
    /// none was found (the items are then in id order).
    pub order_from: Option<PathBuf>,
}

impl Install {
    /// The Steam Workshop items the game loads, in its load order: TSL's
    /// (K1's executable reads no Workshop items), unless `workshop` is off.
    pub fn workshop_items(&self) -> WorkshopItems {
        if !self.workshop || !self.game.reads_workshop() {
            return WorkshopItems::default();
        }
        self.workshop_installed()
    }

    /// The Workshop items installed for this game in the library the
    /// install is in (`steamapps/workshop/content/<app id>/<item id>/`),
    /// whether Steam found it or a variable named a folder inside one.
    ///
    /// TSL asks Steam for the subscribed items (`GetSubscribedItems`, at
    /// most 30) and loads each installed one in that order
    /// (`LoadSteamMods`). Steam keeps that list in
    /// `userdata/<account>/ugc/<app id>_subscriptions.vdf`, numbered from
    /// 0 (the newest subscription first); kjk takes it in that numbering,
    /// leaving out items disabled locally. Without the list, the
    /// installed items are taken in id order.
    fn workshop_installed(&self) -> WorkshopItems {
        let Some(library) = self.steam_library() else {
            return WorkshopItems::default();
        };
        let app = self.game.steam_app_id();
        let content = library
            .join("steamapps/workshop/content")
            .join(app.to_string());
        let installed = |id: u64| {
            let folder = content.join(id.to_string());
            std::fs::read_dir(&folder)
                .is_ok_and(|mut dir| dir.next().is_some())
                .then_some(WorkshopItem { id, folder })
        };
        let (ids, order_from) = match subscriptions(&library, app) {
            Some((list, subscribed)) => (subscribed, Some(list)),
            None => {
                let mut ids: Vec<u64> = std::fs::read_dir(&content)
                    .into_iter()
                    .flatten()
                    .flatten()
                    .filter_map(|entry| entry.file_name().to_str()?.parse().ok())
                    .collect();
                ids.sort();
                (ids, None)
            }
        };
        WorkshopItems {
            items: ids
                .into_iter()
                .take(MAX_WORKSHOP_ITEMS)
                .filter_map(installed)
                .collect(),
            order_from,
        }
    }

    /// The mod folder `ja_mod` names, found beside `base/` in any case: its
    /// lowercased name (the index's keys start with it) and its path; why
    /// not, when it names something else than a folder there. Named by
    /// nothing, Movie Duels' folder (`MOVIE_DUELS`) when it holds Movie
    /// Duels' archives. Without that folder, `base/` itself when a player
    /// copied Movie Duels' archives into it, as mods are often installed
    /// on Linux. Else `Ok(None)`. A mod folder that is not beside `base/`
    /// is looked for in OpenJK's own folder (`openjk_homes`) as well.
    pub fn ja_mod_folder(&self) -> Result<Option<(String, PathBuf)>, String> {
        self.ja_mod_folder_with(&openjk_homes())
    }

    /// `ja_mod_folder`, with `homes` as OpenJK's folders.
    pub fn ja_mod_folder_with(
        &self,
        homes: &[PathBuf],
    ) -> Result<Option<(String, PathBuf)>, String> {
        // The first folder `name` that `keep` takes: beside `base/`, else
        // in one of OpenJK's folders.
        let beside_or_home = |name: &str, keep: fn(&Path) -> bool| {
            std::iter::once(self.data_root.as_path())
                .chain(homes.iter().map(PathBuf::as_path))
                .find_map(|root| find_child(root, name).filter(|path| keep(path)))
        };
        let Some(name) = self.ja_mod.as_deref() else {
            if self.game != Game::JediAcademy {
                return Ok(None);
            }
            let in_base = || {
                find_child(&self.data_root, BASE)
                    .filter(|path| movie_duels_in(path))
                    .map(|path| (BASE.to_string(), path))
            };
            return Ok(beside_or_home(MOVIE_DUELS, movie_duels_in)
                .map(|path| (MOVIE_DUELS.to_string(), path))
                .or_else(in_base));
        };
        let plain = !name.is_empty()
            && !name.contains(['/', '\\', ':'])
            && name != "."
            && name != ".."
            && !name.eq_ignore_ascii_case("base");
        if !plain {
            return Err(format!(
                "{JA_MOD_VAR}={name:?}: not a folder name beside base/"
            ));
        }
        let lower = name.to_ascii_lowercase();
        match beside_or_home(&lower, Path::is_dir) {
            Some(path) => Ok(Some((lower, path))),
            None => Err(format!(
                "{JA_MOD_VAR}={name}: no such folder in {}",
                self.data_root.display()
            )),
        }
    }

    /// Whether the mod folder at `path` (`ja_mod_folder`) lies outside the
    /// data folder, in one of OpenJK's: the data folder's index then does
    /// not hold its files (`DirIndex::of_install`).
    pub fn outside_data_root(&self, path: &Path) -> bool {
        path.parent() != Some(self.data_root.as_path())
    }

    /// Whether Movie Duels is installed beside Jedi Academy and read: a mod
    /// folder is in use, and it or `base/` holds Movie Duels' archives. The
    /// archives are read exactly then (`ja::JaStack::open`). Its saber
    /// rules, hilts and stances are offered only then.
    pub fn movie_duels(&self) -> bool {
        matches!(self.ja_mod_folder(), Ok(Some((_, path)))
            if movie_duels_in(&path)
                || find_child(&self.data_root, BASE).is_some_and(|base| movie_duels_in(&base)))
    }

    /// What a player is told when a mod folder is named or Movie Duels is
    /// found: where it is read from and for what, or why it is not.
    pub fn ja_mod_note(&self) -> Option<String> {
        let found = if self.ja_mod.is_some() {
            "named"
        } else {
            "found"
        };
        match self.ja_mod_folder() {
            Ok(None) => None,
            Ok(Some((folder, path))) if folder == BASE => Some(format!(
                "Movie Duels' archives found in {}: their hilts and stances drawn, the world built from Jedi Academy's own archives alone",
                path.display()
            )),
            Ok(Some((_, path))) => Some(format!(
                "mod folder {} {found}{}: its hilts and stances drawn, the world built from base/ alone",
                path.display(),
                if self.movie_duels() {
                    " (Movie Duels)"
                } else {
                    ""
                }
            )),
            Err(why) => Some(format!("{why}; base/ alone is played")),
        }
    }

    /// The Steam library the install is in.
    fn steam_library(&self) -> Option<PathBuf> {
        match &self.source {
            Source::SteamLibrary(library) => Some(library.clone()),
            _ => self
                .root
                .parent()
                .filter(|common| common.file_name().is_some_and(|name| name == "common"))
                .and_then(Path::parent)
                .filter(|apps| apps.file_name().is_some_and(|name| name == "steamapps"))
                .and_then(Path::parent)
                .map(Path::to_path_buf),
        }
    }

    /// What a player is told when Workshop items are installed for this
    /// game: the items played, in load order (the last one wins), or why
    /// they are not.
    pub fn workshop_note(&self) -> Option<String> {
        let installed = self.workshop_installed();
        let ids: Vec<String> = installed
            .items
            .iter()
            .map(|item| item.id.to_string())
            .collect();
        if ids.is_empty() {
            return None;
        }
        let items = format!(
            "{} Steam Workshop item{}",
            ids.len(),
            if ids.len() == 1 { "" } else { "s" }
        );
        let ids = ids.join(", ");
        Some(if !self.game.reads_workshop() {
            format!(
                "{items} not loaded ({ids}): {} reads no Workshop items",
                self.game.key()
            )
        } else if !self.workshop {
            format!("{items} not loaded ({ids}): {WORKSHOP_VAR} is off")
        } else {
            let order = match installed.order_from {
                Some(_) => "in Steam's subscription order",
                None => "in id order (no Steam subscription list found)",
            };
            format!("{items} loaded, the last winning, {order}: {ids}")
        })
    }
}

/// The subscribed items of `app` in Steam's numbering, without the ones
/// disabled locally, and the list they were read from: the account's that
/// `appworkshop_<app>.acf` says subscribed them, else the newest list.
/// Steam keeps the lists in its own folder (`userdata/`): the library
/// itself when it has one, else one of this OS's Steam folders (the
/// library is a second one).
fn subscriptions(library: &Path, app: u32) -> Option<(PathBuf, Vec<u64>)> {
    let acf = library.join(format!("steamapps/workshop/appworkshop_{app}.acf"));
    let subscriber = std::fs::read_to_string(acf).ok().and_then(|text| {
        let tree = keyvalues(&text);
        let workshop = get(&tree, "appworkshop")?.block()?;
        let (_, item) = get(workshop, "workshopitemdetails")?.block()?.first()?;
        Some(get(item.block()?, "subscribedby")?.text()?.to_string())
    });
    let roots = if library.join("userdata").is_dir() {
        vec![library.to_path_buf()]
    } else {
        steam_roots()
    };
    let mut lists: Vec<(PathBuf, String)> = roots
        .iter()
        .flat_map(|root| {
            std::fs::read_dir(root.join("userdata"))
                .into_iter()
                .flatten()
        })
        .flatten()
        .map(|account| {
            let list = account.path().join(format!("ugc/{app}_subscriptions.vdf"));
            (list, account.file_name().to_string_lossy().into_owned())
        })
        .filter(|(list, _)| list.is_file())
        .collect();
    lists.sort_by_key(|(list, account)| {
        let modified = std::fs::metadata(list)
            .and_then(|meta| meta.modified())
            .ok();
        (Some(account) == subscriber.as_ref(), modified)
    });
    let (list, _) = lists.pop()?;
    let tree = keyvalues(&std::fs::read_to_string(&list).ok()?);
    let mut numbered: Vec<(u64, u64)> = get(&tree, "subscribedfiles")?
        .block()?
        .iter()
        .filter_map(|(number, entry)| {
            let entry = entry.block()?;
            let field = |name: &str| get(entry, name).and_then(Value::text);
            let id = field("publishedfileid")?.parse().ok()?;
            (field("disabled_locally") != Some("1")).then_some((number.parse().ok()?, id))
        })
        .collect();
    numbered.sort();
    Some((list, numbered.into_iter().map(|(_, id)| id).collect()))
}

/// A Valve KeyValues value: a quoted text, or a block of named values.
#[derive(Debug)]
enum Value {
    Text(String),
    Block(Vec<(String, Value)>),
}

impl Value {
    fn text(&self) -> Option<&str> {
        match self {
            Value::Text(text) => Some(text),
            Value::Block(_) => None,
        }
    }

    fn block(&self) -> Option<&[(String, Value)]> {
        match self {
            Value::Block(entries) => Some(entries),
            Value::Text(_) => None,
        }
    }
}

/// The first value named `key` (in any case) in `block`.
fn get<'a>(block: &'a [(String, Value)], key: &str) -> Option<&'a Value> {
    block
        .iter()
        .find(|(name, _)| name.eq_ignore_ascii_case(key))
        .map(|(_, value)| value)
}

/// Reads a text KeyValues file (`.vdf`, `.acf`): quoted names and texts,
/// `\\`-escaped, and `{ … }` blocks. What does not parse ends the block.
fn keyvalues(text: &str) -> Vec<(String, Value)> {
    let mut tokens = Vec::new();
    let mut chars = text.chars();
    while let Some(c) = chars.next() {
        match c {
            '{' | '}' => tokens.push(c.to_string()),
            '"' => {
                let mut token = String::from('"');
                while let Some(c) = chars.next() {
                    match c {
                        '"' => break,
                        '\\' => token.extend(chars.next()),
                        c => token.push(c),
                    }
                }
                tokens.push(token);
            }
            _ => {}
        }
    }
    fn block(tokens: &mut std::slice::Iter<String>) -> Vec<(String, Value)> {
        let mut entries = Vec::new();
        while let Some(name) = tokens.next().and_then(|token| token.strip_prefix('"')) {
            let value = match tokens.next().map(String::as_str) {
                Some("{") => Value::Block(block(tokens)),
                Some(text) if text.starts_with('"') => Value::Text(text[1..].to_string()),
                _ => break,
            };
            entries.push((name.to_string(), value));
        }
        entries
    }
    block(&mut tokens.iter())
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Source {
    /// Named by an environment or `.env` variable.
    Env(&'static str),
    /// Found in a Steam library.
    SteamLibrary(PathBuf),
    /// Found at a GOG default location.
    GogDefault,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Store {
    Steam,
    Gog,
    Unknown,
}

/// A folder that was named or looked like an install, and why it is not one.
#[derive(Debug, Clone)]
pub struct Refusal {
    pub game: Game,
    pub path: PathBuf,
    pub source: Source,
    pub reason: String,
}

#[derive(Debug, Default)]
pub struct Discovery {
    pub installs: Vec<Install>,
    pub refusals: Vec<Refusal>,
}

impl Discovery {
    pub fn get(&self, game: Game) -> Option<&Install> {
        self.installs.iter().find(|install| install.game == game)
    }
}

/// Finds one install per game. A variable (`KJK_KOTOR1`, …) wins; a variable
/// that names something other than that game is a refusal, with no fallback,
/// so a wrong path is never silently replaced by another install. Without a
/// variable, Steam libraries are searched, then GOG's default folders.
/// `KJK_WORKSHOP=off` leaves the installs' Workshop items out;
/// `KJK_JA_MOD` names Jedi Academy's mod folder.
pub fn discover(var: impl Fn(&str) -> Option<String>) -> Discovery {
    let mut discovery = Discovery::default();
    let steam_libraries = steam_libraries();
    for game in Game::ALL {
        if let Some(path) = var(game.env_var()).filter(|path| !path.trim().is_empty()) {
            let source = Source::Env(game.env_var());
            match probe(game, Path::new(path.trim()), source.clone()) {
                Ok(install) => discovery.installs.push(install),
                Err(reason) => discovery.refusals.push(Refusal {
                    game,
                    path: PathBuf::from(path.trim()),
                    source,
                    reason,
                }),
            }
            continue;
        }
        let steam = steam_libraries.iter().map(|library| {
            (
                library
                    .join("steamapps")
                    .join("common")
                    .join(steam_folder(game)),
                Source::SteamLibrary(library.clone()),
            )
        });
        let gog = gog_defaults(game)
            .into_iter()
            .map(|path| (path, Source::GogDefault));
        for (path, source) in steam.chain(gog) {
            if !path.is_dir() {
                continue;
            }
            match probe(game, &path, source.clone()) {
                Ok(install) => {
                    discovery.installs.push(install);
                    break;
                }
                Err(reason) => discovery.refusals.push(Refusal {
                    game,
                    path,
                    source,
                    reason,
                }),
            }
        }
    }
    let workshop = var(WORKSHOP_VAR).is_none_or(|value| {
        !matches!(
            value.trim().to_ascii_lowercase().as_str(),
            "off" | "0" | "no" | "false"
        )
    });
    let ja_mod = var(JA_MOD_VAR)
        .map(|name| name.trim().to_string())
        .filter(|name| !name.is_empty());
    for install in &mut discovery.installs {
        install.workshop = workshop;
        if install.game == Game::JediAcademy {
            install.ja_mod = ja_mod.clone();
        }
    }
    discovery
}

/// `path` as an install of `game`, or why it is not one: the check
/// `discover` makes of every folder, for a folder a person picks.
pub fn install_at(game: Game, path: &Path) -> Result<Install, String> {
    probe(game, path, Source::Env(game.env_var()))
}

/// Names directly inside `dir`, lowercased; folders get a trailing `/`.
fn listing(dir: &Path) -> Result<Vec<String>, String> {
    let entries = std::fs::read_dir(dir).map_err(|error| format!("cannot read folder: {error}"))?;
    let mut names = Vec::new();
    for entry in entries.flatten() {
        let mut name = entry.file_name().to_string_lossy().to_ascii_lowercase();
        if entry.file_type().is_ok_and(|kind| kind.is_dir()) {
            name.push('/');
        }
        names.push(name);
    }
    Ok(names)
}

fn find_child(dir: &Path, lowercase_name: &str) -> Option<PathBuf> {
    std::fs::read_dir(dir).ok()?.flatten().find_map(|entry| {
        entry
            .file_name()
            .to_string_lossy()
            .eq_ignore_ascii_case(lowercase_name)
            .then(|| entry.path())
    })
}

/// Where the macOS ports keep their data inside the `.app` bundle: Aspyr's
/// KOTOR in `Contents/Assets`, its TSL in `Contents/GameData`, and Jedi
/// Academy's `base/` directly in `Contents`.
const BUNDLE_DATA: &[&str] = &["Contents/Assets", "Contents/GameData", "Contents"];

/// The folders that may hold the game's data, in order: `root`, its
/// `GameData/`, its `steamassets/`, then the data folders of the `.app`
/// bundle that `root` is or directly contains. The native Linux port of TSL
/// on Steam keeps its data in `steamassets/`.
fn data_candidates(root: &Path) -> Vec<PathBuf> {
    let is_bundle = |path: &Path| {
        path.extension()
            .is_some_and(|extension| extension.eq_ignore_ascii_case("app"))
    };
    let mut bundles: Vec<PathBuf> = if is_bundle(root) {
        vec![root.to_path_buf()]
    } else {
        std::fs::read_dir(root)
            .into_iter()
            .flatten()
            .flatten()
            .map(|entry| entry.path())
            .filter(|path| is_bundle(path) && path.is_dir())
            .collect()
    };
    bundles.sort();
    std::iter::once(root.to_path_buf())
        .chain(find_child(root, "gamedata"))
        .chain(find_child(root, "steamassets"))
        .chain(
            bundles
                .iter()
                .flat_map(|bundle| BUNDLE_DATA.iter().map(|data| bundle.join(data))),
        )
        .filter(|path| path.is_dir())
        .collect()
}

fn probe(game: Game, root: &Path, source: Source) -> Result<Install, String> {
    let names = listing(root)?;
    let has = |name: &str| names.iter().any(|found| found == name);
    let candidates = data_candidates(root);

    let (data_root, data_names) = match game {
        Game::Kotor1 | Game::Kotor2 => {
            let required = ["chitin.key", "dialog.tlk", "modules/"];
            let Some((data_root, data_names)) = candidates.into_iter().find_map(|dir| {
                let data_names = listing(&dir).ok()?;
                required
                    .iter()
                    .all(|name| data_names.iter().any(|found| found == name))
                    .then_some((dir, data_names))
            }) else {
                let missing: Vec<&str> = required.into_iter().filter(|name| !has(name)).collect();
                return Err(format!(
                    "not a KOTOR install: missing {}",
                    missing.join(", ")
                ));
            };
            let data_has = |name: &str| data_names.iter().any(|found| found == name);
            let is_tsl = if has("swkotor2.exe") {
                true
            } else if has("swkotor.exe") {
                false
            } else {
                data_has("streamvoice/")
            };
            let found = if is_tsl { Game::Kotor2 } else { Game::Kotor1 };
            if found != game {
                return Err(format!(
                    "this is a {} install, not {}",
                    found.key(),
                    game.key()
                ));
            }
            (data_root, data_names)
        }
        Game::JediAcademy => {
            let data_root = candidates
                .into_iter()
                .find(|dir| find_child(dir, "base").is_some())
                .ok_or("not a Jedi Academy install: no base/, GameData/base/ or app bundle")?;
            let base = find_child(&data_root, "base").expect("checked above");
            if find_child(&base, "assets0.pk3").is_none() {
                return Err("not a Jedi Academy install: base/ has no assets0.pk3".into());
            }
            let data_names = listing(&data_root)?;
            (data_root, data_names)
        }
    };

    let store = if names.iter().any(|name| name.starts_with("goggame-")) {
        Store::Gog
    } else if has("steam_api.dll")
        || names.iter().any(|name| name.ends_with("_install.vdf"))
        || matches!(source, Source::SteamLibrary(_))
    {
        Store::Steam
    } else {
        Store::Unknown
    };

    let markers = MARKERS
        .iter()
        .copied()
        .filter(|marker| has(marker) || data_names.iter().any(|found| found == *marker))
        .collect();

    Ok(Install {
        game,
        root: root.to_path_buf(),
        data_root,
        source,
        store,
        markers,
        workshop: true,
        ja_mod: None,
    })
}

/// Top-level names worth reporting because they differ between builds. On
/// this machine: TSL's Aspyr-updated Steam build has `steam_api.dll` and
/// `cloudsaves/`; K1 has `patch.erf` and `rims/`.
const MARKERS: &[&str] = &[
    "swkotor.exe",
    "swkotor2.exe",
    "jediacademy.exe",
    "steam_api.dll",
    "cloudsaves/",
    "patch.erf",
    "rims/",
    "texturepacks/",
    "streamvoice/",
    "streamwaves/",
];

fn steam_folder(game: Game) -> &'static str {
    match game {
        Game::Kotor1 => "swkotor",
        Game::Kotor2 => "Knights of the Old Republic II",
        Game::JediAcademy => "Jedi Academy",
    }
}

/// GOG's default install folders. Not verified on a GOG install yet.
fn gog_defaults(game: Game) -> Vec<PathBuf> {
    let folder = match game {
        Game::Kotor1 => "Star Wars - KotOR",
        Game::Kotor2 => "Star Wars - KotOR2",
        Game::JediAcademy => "Star Wars Jedi Knight - Jedi Academy",
    };
    if cfg!(windows) {
        vec![
            Path::new(r"C:\GOG Games").join(folder),
            Path::new(r"C:\Program Files (x86)\GOG Galaxy\Games").join(folder),
        ]
    } else {
        Vec::new()
    }
}

/// OpenJK's folder for a player's own files and mods on Linux and the
/// BSDs: `$XDG_DATA_HOME/openjk`, else `~/.local/share/openjk`. A player
/// who plays Movie Duels through OpenJK may have installed it there rather
/// than beside `base/`. There is none on macOS and Windows.
fn openjk_homes() -> Vec<PathBuf> {
    if cfg!(any(windows, target_os = "macos")) {
        return Vec::new();
    }
    let data = std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
        .or_else(|| std::env::home_dir().map(|home| home.join(".local/share")));
    data.map(|data| data.join("openjk"))
        .into_iter()
        .filter(|home| home.is_dir())
        .collect()
}

/// This OS's Steam folders that exist.
fn steam_roots() -> Vec<PathBuf> {
    let mut roots = Vec::new();
    if cfg!(windows) {
        roots.push(PathBuf::from(r"C:\Program Files (x86)\Steam"));
        roots.push(PathBuf::from(r"C:\Program Files\Steam"));
    } else if let Some(home) = std::env::home_dir() {
        if cfg!(target_os = "macos") {
            roots.push(home.join("Library/Application Support/Steam"));
        } else {
            roots.push(home.join(".steam/steam"));
            roots.push(home.join(".local/share/Steam"));
            roots.push(home.join(".var/app/com.valvesoftware.Steam/.local/share/Steam"));
            // Ubuntu's Snap package of Steam.
            roots.push(home.join("snap/steam/common/.local/share/Steam"));
        }
    }
    roots.retain(|root| root.is_dir());
    roots
}

/// Steam roots for this OS, then every library listed in their
/// `steamapps/libraryfolders.vdf`.
fn steam_libraries() -> Vec<PathBuf> {
    let mut libraries: Vec<PathBuf> = Vec::new();
    for root in steam_roots() {
        let vdf = root.join("steamapps").join("libraryfolders.vdf");
        let listed = std::fs::read_to_string(vdf)
            .map(|text| library_paths(&text))
            .unwrap_or_else(|_| Vec::new());
        for library in std::iter::once(root).chain(listed) {
            if !libraries.iter().any(|known| same_dir(known, &library)) {
                libraries.push(library);
            }
        }
    }
    libraries
}

/// The libraries' `"path"` values of a `libraryfolders.vdf`.
fn library_paths(vdf: &str) -> Vec<PathBuf> {
    let tree = keyvalues(vdf);
    let Some(Value::Block(libraries)) = get(&tree, "libraryfolders") else {
        return Vec::new();
    };
    libraries
        .iter()
        .filter_map(|(_, library)| get(library.block()?, "path")?.text())
        .map(PathBuf::from)
        .collect()
}

fn same_dir(a: &Path, b: &Path) -> bool {
    match (a.canonicalize(), b.canonicalize()) {
        (Ok(a), Ok(b)) => a == b,
        _ => a == b,
    }
}
