//! A digest of the game data an install gives the runtime. Two machines with
//! the same fingerprint read the same bytes for every resource; the per-group
//! digests say where two installs differ.
//!
//! Covered: exactly the files the resolvers read (see `covered`). Player
//! data, configs and executables are not game data here and are left out.
//! Every covered file is hashed whole with BLAKE3. The per-file hash is
//! cached by (path, size, modification time), so a second run reads only
//! what changed.

use std::collections::BTreeMap;
use std::fs::File;
use std::io::{self, BufRead, BufReader, Write};
use std::path::Path;
use std::time::UNIX_EPOCH;

use crate::{DirIndex, Game};

/// Bump when the covered set or the canonical form changes: old cache files
/// and fingerprints then stop matching instead of silently agreeing.
pub const VERSION: &str = "kjk-fingerprint v1";

const KOTOR_FILES: &[&str] = &["chitin.key", "dialog.tlk", "patch.erf"];
const KOTOR_FOLDERS: &[&str] = &[
    "data/",
    "modules/",
    "override/",
    "lips/",
    "rims/",
    "texturepacks/",
    "streammusic/",
    "streamsounds/",
    "streamwaves/",
    "streamvoice/",
    "movies/",
];

/// Whether a lowercased path relative to the data root is game data that the
/// runtime reads.
pub fn covered(game: Game, key: &str) -> bool {
    match game {
        Game::Kotor1 | Game::Kotor2 => {
            let name = key.rsplit('/').next().unwrap_or(key);
            (KOTOR_FILES.contains(&key) || KOTOR_FOLDERS.iter().any(|dir| key.starts_with(dir)))
                && !crate::system_file(name)
        }
        Game::JediAcademy => key.strip_prefix("base/").is_some_and(crate::ja::game_data),
    }
}

/// The most groups a world names (`WorldData::groups`), as many as a join
/// handshake carries per game.
pub const MAX_WORLD_GROUPS: usize = 8;

/// The group a covered file is digested in: its first path component (the
/// file name, for files at the root). JA's loaded PK3s are groups of their
/// own (`base/assets0.pk3`), the first `MAX_WORLD_GROUPS - 1` in name
/// order, so a refusal names the archive that differs; the rest of `base/`
/// is `base`.
fn group_of<'a>(game: Game, key: &'a str, own: &[&str]) -> &'a str {
    if game == Game::JediAcademy && own.contains(&key) {
        return key;
    }
    key.split_once('/').map_or(key, |(first, _)| first)
}

/// The groups a game's worlds are built from (its tables, modules,
/// overrides; JA's `base/` with the animations): two machines that differ
/// there may build different worlds. Sounds, voices, movies and texture
/// packs are not among them.
pub fn world_group(game: Game, group: &str) -> bool {
    match game {
        Game::Kotor1 | Game::Kotor2 => matches!(
            group,
            "chitin.key" | "data" | "dialog.tlk" | "modules" | "override" | "patch.erf" | "rims"
        ),
        Game::JediAcademy => group == "base" || group.starts_with("base/"),
    }
}

#[derive(Debug)]
pub struct Fingerprint {
    pub game: Game,
    /// BLAKE3, hex.
    pub digest: String,
    pub files: usize,
    pub bytes: u64,
    /// First path component (or the file name, for files at the root) to
    /// (digest, file count).
    pub groups: BTreeMap<String, (String, usize)>,
    pub hashed: usize,
    pub cached: usize,
}

/// What two machines compare before they share a world: the digest of
/// the world's groups (`world_group`) and each one's digest.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorldData {
    pub digest: [u8; 32],
    pub groups: Vec<(String, [u8; 32])>,
}

impl Fingerprint {
    /// The part of the data a shared world is built from.
    pub fn world(&self) -> WorldData {
        let groups: Vec<(String, [u8; 32])> = self
            .groups
            .iter()
            .filter(|(group, _)| world_group(self.game, group))
            .map(|(group, (digest, _))| {
                let bytes = blake3::Hash::from_hex(digest).map_or([0; 32], |hash| *hash.as_bytes());
                (group.clone(), bytes)
            })
            .collect();
        let mut whole = blake3::Hasher::new();
        whole.update(format!("{VERSION}\nworld {}\n", self.game.key()).as_bytes());
        for (group, digest) in &groups {
            whole.update(group.as_bytes());
            whole.update(b"\n");
            whole.update(digest);
        }
        WorldData {
            digest: *whole.finalize().as_bytes(),
            groups,
        }
    }
}

struct CacheRow {
    size: u64,
    modified: u128,
    hash: String,
}

fn load_cache(path: &Path) -> BTreeMap<String, CacheRow> {
    let Ok(file) = File::open(path) else {
        return BTreeMap::new();
    };
    let mut rows = BTreeMap::new();
    let mut lines = BufReader::new(file).lines().map_while(Result::ok);
    if lines.next().as_deref() != Some(VERSION) {
        return rows;
    }
    for line in lines {
        let fields: Vec<&str> = line.split('\t').collect();
        if let [key, size, modified, hash] = fields[..]
            && let (Ok(size), Ok(modified)) = (size.parse(), modified.parse())
        {
            rows.insert(
                key.to_string(),
                CacheRow {
                    size,
                    modified,
                    hash: hash.to_string(),
                },
            );
        }
    }
    rows
}

fn save_cache(path: &Path, rows: &BTreeMap<String, CacheRow>) -> io::Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let partial = path.with_extension("partial");
    let mut out = io::BufWriter::new(File::create(&partial)?);
    writeln!(out, "{VERSION}")?;
    for (key, row) in rows {
        writeln!(out, "{key}\t{}\t{}\t{}", row.size, row.modified, row.hash)?;
    }
    out.into_inner().map_err(|error| error.into_error())?;
    std::fs::rename(partial, path)
}

fn hash_file(path: &Path) -> io::Result<String> {
    let mut hasher = blake3::Hasher::new();
    hasher.update_reader(File::open(path)?)?;
    Ok(hasher.finalize().to_hex().to_string())
}

/// Fingerprints one install. `cache` is the cache file to read and rewrite,
/// or `None` to hash every file.
pub fn fingerprint(game: Game, index: &DirIndex, cache: Option<&Path>) -> io::Result<Fingerprint> {
    compute(game, index, cache, true).map(|print| print.expect("hashing never gives up"))
}

/// The fingerprint from the cache alone, or `None` when any covered file is
/// missing from it or has changed. Reads no file contents.
pub fn fingerprint_cached(
    game: Game,
    index: &DirIndex,
    cache: &Path,
) -> io::Result<Option<Fingerprint>> {
    compute(game, index, Some(cache), false)
}

fn compute(
    game: Game,
    index: &DirIndex,
    cache: Option<&Path>,
    hash_missing: bool,
) -> io::Result<Option<Fingerprint>> {
    let old = match cache {
        Some(path) => load_cache(path),
        None => BTreeMap::new(),
    };
    let mut new = BTreeMap::new();
    let (mut hashed, mut cached, mut bytes) = (0, 0, 0);
    let mut lines = Vec::new();
    for (key, path) in index.iter().filter(|(key, _)| covered(game, key)) {
        let meta = std::fs::metadata(path)?;
        let modified = meta
            .modified()?
            .duration_since(UNIX_EPOCH)
            .map_or(0, |since| since.as_nanos());
        let hash = match old.get(key) {
            Some(row) if row.size == meta.len() && row.modified == modified => {
                cached += 1;
                row.hash.clone()
            }
            _ if !hash_missing => return Ok(None),
            _ => {
                hashed += 1;
                hash_file(path)?
            }
        };
        bytes += meta.len();
        lines.push((key.to_string(), format!("{key}\t{}\t{hash}\n", meta.len())));
        new.insert(
            key.to_string(),
            CacheRow {
                size: meta.len(),
                modified,
                hash,
            },
        );
    }
    if let Some(cache) = cache
        && hashed > 0
    {
        save_cache(cache, &new)?;
    }

    let header = format!("{VERSION}\ngame {}\n", game.key());
    let mut whole = blake3::Hasher::new();
    whole.update(header.as_bytes());
    let own: Vec<&str> = lines
        .iter()
        .map(|(key, _)| key.as_str())
        .filter(|key| key.strip_prefix("base/").is_some_and(crate::ja::root_pk3))
        .take(MAX_WORLD_GROUPS - 1)
        .collect();
    let mut groups: BTreeMap<String, (blake3::Hasher, usize)> = BTreeMap::new();
    for (key, line) in &lines {
        whole.update(line.as_bytes());
        let group = group_of(game, key, &own);
        let slot = groups.entry(group.to_string()).or_insert_with(|| {
            let mut hasher = blake3::Hasher::new();
            hasher.update(header.as_bytes());
            (hasher, 0)
        });
        slot.0.update(line.as_bytes());
        slot.1 += 1;
    }
    Ok(Some(Fingerprint {
        game,
        digest: whole.finalize().to_hex().to_string(),
        files: lines.len(),
        bytes,
        groups: groups
            .into_iter()
            .map(|(group, (hasher, count))| {
                (group, (hasher.finalize().to_hex().to_string(), count))
            })
            .collect(),
        hashed,
        cached,
    }))
}
