//! The owner-approved test scenarios (`tests/approved.rs`), and what they
//! share: the workspace's paths, the game installs `.env` names, and the
//! hash the workflows pin. Only a scenario the owner approved by name
//! lives here (AGENT.md); `docs/TESTS.md` says what each checks.
//!
//! Nothing here depends on Bevy, so `cargo test -p approved_tests` builds
//! the sim, the collision and their readers alone. The rig's timelines
//! need kjk's library: `--features rig`.

use std::collections::HashMap;
use std::path::PathBuf;

use asset_odyssey::module::Source;
use fmt_odyssey::ResType;
use vfs::resolver::{Resolver, TextureQuality};
use vfs::{DirIndex, Discovery, Game};

/// The workspace root.
pub fn workspace() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// `KEY=VALUE` lines of the workspace's `.env` (as kjk reads it); empty
/// when there is none.
fn dotenv() -> HashMap<String, String> {
    let Ok(text) = std::fs::read_to_string(workspace().join(".env")) else {
        return HashMap::new();
    };
    text.lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .filter_map(|line| line.split_once('='))
        .map(|(key, value)| {
            let value = value.trim();
            let value = value
                .strip_prefix('"')
                .and_then(|inner| inner.strip_suffix('"'))
                .unwrap_or(value);
            (key.trim().to_string(), value.to_string())
        })
        .collect()
}

/// The game installs, found as kjk finds them: the environment first,
/// then `.env`, then the usual places.
pub fn discovery() -> Discovery {
    let env_file = dotenv();
    vfs::discover(|name| {
        std::env::var(name)
            .ok()
            .or_else(|| env_file.get(name).cloned())
    })
}

/// The sim hash both workflows pin (`KJK_SIM_HASH:` in `ci.yml` and
/// `manual.yml`); an error when a file lacks it or they differ.
pub fn pinned_hash() -> Result<String, String> {
    let mut pins = Vec::new();
    for file in ["ci.yml", "manual.yml"] {
        let path = workspace().join(".github/workflows").join(file);
        let text = std::fs::read_to_string(&path)
            .map_err(|error| format!("{}: {error}", path.display()))?;
        let pin = text
            .lines()
            .find_map(|line| line.trim().strip_prefix("KJK_SIM_HASH:"))
            .map(|hash| hash.trim().trim_matches('"').to_string())
            .ok_or(format!("{file}: no KJK_SIM_HASH"))?;
        pins.push((file, pin));
    }
    if pins[0].1 != pins[1].1 {
        return Err(format!(
            "the workflows pin different hashes: {} {}, {} {}",
            pins[0].0, pins[0].1, pins[1].0, pins[1].1
        ));
    }
    Ok(pins.remove(0).1)
}

/// The branch's bump files that leave the hash's pin to the merge
/// (`bumps/README.md`): the sim moving on purpose, or the snapshot's
/// version.
pub fn hash_left_to_merge() -> Result<Vec<String>, String> {
    Ok(bumps::pending(&workspace())?
        .into_iter()
        .filter(bumps::Bump::moves_hash)
        .map(|bump| {
            let name = bump.path.file_name().unwrap_or(bump.path.as_os_str());
            format!("{}/{}", bumps::DIR, name.to_string_lossy())
        })
        .collect())
}

/// Whether a pending bump file moves a format saves hold (`save`,
/// `snapshot`, `pack`): the branch is then at main + 1, and its fixture is
/// written at the merge's pin (`sim::fixtures`).
pub fn stored_bumps_pending() -> bool {
    bumps::pending(&workspace())
        .expect("the bump files read")
        .iter()
        .flat_map(|bump| &bump.formats)
        .any(|(name, _)| matches!(*name, "save" | "snapshot" | "pack"))
}

/// A KOTOR resolver as a module reader's `Source`.
pub struct ResolverSource(pub Resolver);

impl Source for ResolverSource {
    fn read(&self, resref: &str, restype: ResType) -> Option<Vec<u8>> {
        self.0.find(resref, restype)?.location.read().ok()
    }

    fn exists(&self, resref: &str, restype: ResType) -> bool {
        self.0.find(resref, restype).is_some()
    }
    fn names(&self, restype: ResType) -> Vec<String> {
        self.0.names(restype)
    }

    fn own_names(&self, restype: ResType) -> Vec<String> {
        self.0.module_names(restype)
    }

    fn voice(&self, resref: &str) -> Option<Vec<u8>> {
        self.0.voice(resref)?.read().ok()
    }

    fn talk(&self) -> Option<std::sync::Arc<fmt_odyssey::tlk::Tlk>> {
        self.0.talk.get()
    }
}

/// `module` of `game` opened as kjk opens it, or why not (no install).
pub fn module_source(
    discovery: &Discovery,
    game: Game,
    module: &str,
) -> Result<ResolverSource, String> {
    let install = discovery.get(game).ok_or(format!(
        "{}: not found (set {} in .env)",
        game.key(),
        game.env_var()
    ))?;
    let index = DirIndex::build(&install.data_root).map_err(|error| error.to_string())?;
    Resolver::open(game, &index, Some(module), TextureQuality::High)
        .map(ResolverSource)
        .map_err(|error| error.to_string())
}
