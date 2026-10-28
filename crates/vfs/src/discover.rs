use std::path::{Path, PathBuf};

use crate::Game;

#[derive(Debug, Clone)]
pub struct Install {
    pub game: Game,
    /// The folder the player installed to.
    pub root: PathBuf,
    /// The folder the game reads its data from: `root` for KOTOR, the folder
    /// holding `base/` for Jedi Academy, or a folder inside the `.app`
    /// bundle for a macOS port.
    pub data_root: PathBuf,
    pub source: Source,
    pub store: Store,
    /// Files and folders that tell builds apart, as found in `root` or
    /// `data_root` (lowercase).
    pub markers: Vec<&'static str>,
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
    discovery
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
/// `GameData/`, then the data folders of the `.app` bundle that `root` is or
/// directly contains.
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

/// Steam roots for this OS, then every library listed in their
/// `steamapps/libraryfolders.vdf`.
fn steam_libraries() -> Vec<PathBuf> {
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
        }
    }

    let mut libraries: Vec<PathBuf> = Vec::new();
    for root in roots.into_iter().filter(|root| root.is_dir()) {
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

/// The `"path" "…"` values of a `libraryfolders.vdf`, unescaped.
fn library_paths(vdf: &str) -> Vec<PathBuf> {
    vdf.lines()
        .filter_map(|line| {
            let mut quoted = line.split('"').skip(1).step_by(2);
            (quoted.next()? == "path").then(|| quoted.next())?
        })
        .map(|path| PathBuf::from(path.replace(r"\\", r"\")))
        .collect()
}

fn same_dir(a: &Path, b: &Path) -> bool {
    match (a.canonicalize(), b.canonicalize()) {
        (Ok(a), Ok(b)) => a == b,
        _ => a == b,
    }
}
