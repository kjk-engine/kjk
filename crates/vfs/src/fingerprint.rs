//! A digest of the game data an install gives the runtime. Two machines with
//! the same fingerprint read the same bytes for every resource; the per-group
//! digests say where two installs differ.
//!
//! Covered: exactly the files the resolvers read (see `covered`), the
//! Steam Workshop items' with the game folder's (`covered_in_item`), and
//! the items' load order. Player data, configs and executables are not game
//! data here and are left out.
//! Every covered file is hashed whole with BLAKE3. The per-file hash is
//! cached by (path, size, modification time), so a second run reads only
//! what changed.

use std::collections::{BTreeMap, BTreeSet};
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
            if let Some(rest) = key.strip_prefix("override/") {
                return crate::resolver::read_from_override(game, rest);
            }
            let name = key.rsplit('/').next().unwrap_or(key);
            (KOTOR_FILES.contains(&key) || KOTOR_FOLDERS.iter().any(|dir| key.starts_with(dir)))
                && !crate::system_file(name)
        }
        Game::JediAcademy => key.strip_prefix("base/").is_some_and(crate::ja::game_data),
    }
}

/// Whether a lowercased path inside a Workshop item's folder is game data
/// the game reads from it: its `override/` top level (as the resolver
/// takes it), `dialog.tlk`, and its modules, lips, movies and stream
/// folders (`GetOverridePath`'s callers).
pub fn covered_in_item(key: &str) -> bool {
    if let Some(rest) = key.strip_prefix("override/") {
        return crate::resolver::read_from_item_override(rest);
    }
    let name = key.rsplit('/').next().unwrap_or(key);
    (key == "dialog.tlk" || ITEM_FOLDERS.iter().any(|dir| key.starts_with(dir)))
        && !crate::system_file(name)
}

const ITEM_FOLDERS: &[&str] = &[
    "modules/",
    "lips/",
    "movies/",
    "streammusic/",
    "streamsounds/",
    "streamvoice/",
    "streamwaves/",
];

/// The most groups a world names (`WorldData::groups`), as many as a join
/// handshake carries per game.
pub const MAX_WORLD_GROUPS: usize = 8;

/// The world group of the Workshop items past the ones that fit in
/// `MAX_WORLD_GROUPS` with the game folder's groups.
pub const LATER_WORKSHOP_ITEMS: &str = "workshop/later";

/// The group a covered file is digested in: its first path component (the
/// file name, for files at the root); a Workshop item's file, its first
/// path component in the item, after the item's label
/// (`workshop/485537937/override`). JA's loaded PK3s are groups of their
/// own (`base/assets0.pk3`), the first `MAX_WORLD_GROUPS - 1` in name
/// order, so a refusal names the archive that differs; the rest of `base/`
/// is `base`.
fn group_of<'a>(game: Game, key: &'a str, own: &[&str]) -> &'a str {
    if game == Game::JediAcademy && own.contains(&key) {
        return key;
    }
    let first = |key: &'a str| key.split_once('/').map_or(key, |(first, _)| first);
    match key
        .strip_prefix("workshop/")
        .and_then(|rest| rest.split_once('/'))
    {
        Some((id, rest)) => &key[..("workshop/".len() + id.len() + 1 + first(rest).len())],
        None => first(key),
    }
}

/// The groups a game's worlds are built from (its tables, modules,
/// overrides; JA's `base/` with the animations): two machines that differ
/// there may build different worlds. Sounds, voices, movies and texture
/// packs are not among them, nor `dialog.tlk`: its lines are what a player
/// reads, the host's scripts decide what they do, and the lines a level
/// reads are in its digest (`sim::script::Library::with_talk`). Aspyr's
/// Mac table differs from the Windows one in lines no level reads.
pub fn world_group(game: Game, group: &str) -> bool {
    match game {
        Game::Kotor1 | Game::Kotor2 => matches!(
            group,
            "chitin.key" | "data" | "modules" | "override" | "patch.erf" | "rims"
        ),
        Game::JediAcademy => group == "base" || group.starts_with("base/"),
    }
}

/// What a world file at `key` in a Workshop item vies for with another
/// item's copy, or `None` for a file outside the world groups. Which of two
/// items loads last decides only what they both hold: an override resource
/// (by its name, any type of it, as a texture's TGA and TPC stand for each
/// other) or a module (its `.mod`, `.rim`, `_s.rim` and `_dlg.erf` alike).
fn contest(game: Game, key: &str) -> Option<String> {
    let (part, rest) = key.split_once('/')?;
    if !world_group(game, part) {
        return None;
    }
    let stem = rest.rsplit_once('.').map_or(rest, |(stem, _)| stem);
    let stem = match part {
        "modules" => ["_s", "_dlg"]
            .iter()
            .find_map(|suffix| stem.strip_suffix(suffix))
            .unwrap_or(stem),
        _ => stem,
    };
    Some(format!("{part}/{stem}"))
}

/// For each Workshop item, the items loaded before it that hold some of
/// the same world data (`contest`), by id: the load order as far as it
/// changes what the resolver serves. Items holding nothing in common load
/// the same world in any order.
fn beneath(game: Game, index: &DirIndex) -> BTreeMap<u64, BTreeSet<u64>> {
    let claims: Vec<(u64, BTreeSet<String>)> = index
        .workshop()
        .iter()
        .map(|item| {
            let keys = item
                .files
                .iter()
                .filter(|(key, _)| covered_in_item(key))
                .filter_map(|(key, _)| contest(game, key))
                .collect();
            (item.id, keys)
        })
        .collect();
    claims
        .iter()
        .enumerate()
        .map(|(place, (id, mine))| {
            let under = claims[..place]
                .iter()
                .filter(|(_, theirs)| !mine.is_disjoint(theirs))
                .map(|(other, _)| *other)
                .collect();
            (*id, under)
        })
        .collect()
}

#[derive(Debug)]
pub struct Fingerprint {
    pub game: Game,
    /// The Workshop items covered, in load order (`DirIndex::workshop`).
    pub workshop: Vec<u64>,
    /// Each Workshop item's items loaded before it that hold some of the
    /// same world data (`beneath`).
    pub beneath: BTreeMap<u64, BTreeSet<u64>>,
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
    /// The part of the data a shared world is built from: the game
    /// folder's world groups, and each Workshop item's world groups as one
    /// group named by the item (`workshop/485537937`), in id order, whose
    /// digest has the items loaded before it that hold some of the same
    /// world data (`beneath`), so a refusal names an item that differs or
    /// moved past another it vies with. Two players who subscribed to the
    /// same items in another order (Steam's list is per account, the newest
    /// first) share a world unless that order changes what is served.
    /// Past `MAX_WORLD_GROUPS`, the later items share one group
    /// (`LATER_WORKSHOP_ITEMS`).
    pub fn world(&self) -> WorldData {
        let bytes =
            |digest: &str| blake3::Hash::from_hex(digest).map_or([0; 32], |hash| *hash.as_bytes());
        let mut groups: Vec<(String, [u8; 32])> = self
            .groups
            .iter()
            .filter(|(group, _)| !group.starts_with("workshop/") && world_group(self.game, group))
            .map(|(group, (digest, _))| (group.clone(), bytes(digest)))
            .collect();
        // Each item with world data: its label, and a digest of its world
        // groups and the items beneath it.
        let mut ids = self.workshop.clone();
        ids.sort_unstable();
        let items: Vec<(String, [u8; 32])> = ids
            .iter()
            .filter_map(|id| {
                let label = format!("workshop/{id}");
                let prefix = format!("{label}/");
                let parts: Vec<(&str, [u8; 32])> = self
                    .groups
                    .iter()
                    .filter_map(|(group, (digest, _))| {
                        let part = group.strip_prefix(&prefix)?;
                        world_group(self.game, part).then(|| (part, bytes(digest)))
                    })
                    .collect();
                (!parts.is_empty()).then_some((*id, label, parts))
            })
            .map(|(id, label, parts)| {
                let mut hasher = blake3::Hasher::new();
                hasher.update(format!("{VERSION}\n{label}\n").as_bytes());
                for under in self.beneath.get(&id).into_iter().flatten() {
                    hasher.update(format!("over {under}\n").as_bytes());
                }
                for (part, digest) in parts {
                    hasher.update(part.as_bytes());
                    hasher.update(b"\n");
                    hasher.update(&digest);
                }
                (label, *hasher.finalize().as_bytes())
            })
            .collect();
        let room = MAX_WORLD_GROUPS.saturating_sub(groups.len());
        let own = if items.len() > room {
            room.saturating_sub(1)
        } else {
            items.len()
        };
        let mut later = blake3::Hasher::new();
        for (place, (label, digest)) in items.iter().enumerate() {
            if place < own {
                groups.push((label.clone(), *digest));
            } else {
                later.update(digest);
            }
        }
        if items.len() > own {
            groups.push((
                LATER_WORKSHOP_ITEMS.to_string(),
                *later.finalize().as_bytes(),
            ));
        }
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
    compute(game, index, cache, Scope::All, &mut |_, _| {})
        .map(|print| print.expect("hashing never gives up"))
}

/// The fingerprint of the world groups alone (`world_group`), all a join
/// compares (`Fingerprint::world` gives the same as from `fingerprint`):
/// a few hundred files where the whole install has tens of thousands (the
/// voices, sounds and movies), so a player's first join does not hash
/// gigabytes it never compares. `progress` hears the bytes hashed so far
/// and the bytes to hash, as they go; the cache is kept as it goes too, so
/// a join given up half way picks up where it stopped.
pub fn world_fingerprint(
    game: Game,
    index: &DirIndex,
    cache: &Path,
    progress: &mut dyn FnMut(u64, u64),
) -> io::Result<Fingerprint> {
    compute(game, index, Some(cache), Scope::World, progress)
        .map(|print| print.expect("hashing never gives up"))
}

/// The fingerprint from the cache alone, or `None` when any covered file is
/// missing from it or has changed. Reads no file contents.
pub fn fingerprint_cached(
    game: Game,
    index: &DirIndex,
    cache: &Path,
) -> io::Result<Option<Fingerprint>> {
    compute(game, index, Some(cache), Scope::Cached, &mut |_, _| {})
}

/// Which files a fingerprint reads, and whether it hashes the ones the
/// cache does not hold.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Scope {
    /// Every covered file, hashed when the cache lacks it.
    All,
    /// Every covered file, from the cache alone.
    Cached,
    /// The world groups' files, hashed when the cache lacks them.
    World,
}

/// Bytes hashed between two writes of the cache.
const CACHE_EVERY: u64 = 256 << 20;

/// Whether the file at `key` (`workshop/<id>/…` for an item's) is in one of
/// the world groups.
fn in_world(game: Game, key: &str) -> bool {
    let in_item = key
        .strip_prefix("workshop/")
        .and_then(|rest| rest.split_once('/'))
        .map(|(_, rest)| rest);
    let key = in_item.unwrap_or(key);
    world_group(game, key.split_once('/').map_or(key, |(first, _)| first))
}

fn compute(
    game: Game,
    index: &DirIndex,
    cache: Option<&Path>,
    scope: Scope,
    progress: &mut dyn FnMut(u64, u64),
) -> io::Result<Option<Fingerprint>> {
    let old = match cache {
        Some(path) => load_cache(path),
        None => BTreeMap::new(),
    };
    let items = index.workshop().iter().flat_map(|item| {
        let label = item.label();
        item.files
            .iter()
            .filter(|(key, _)| covered_in_item(key))
            .map(move |(key, path)| (format!("{label}/{key}"), path))
    });
    let mut files = Vec::new();
    for (key, path) in index
        .iter()
        .filter(|(key, _)| covered(game, key))
        .map(|(key, path)| (key.to_string(), path))
        .chain(items)
        .filter(|(key, _)| scope != Scope::World || in_world(game, key))
    {
        let meta = std::fs::metadata(path)?;
        let modified = meta
            .modified()?
            .duration_since(UNIX_EPOCH)
            .map_or(0, |since| since.as_nanos());
        let hash = old
            .get(&key)
            .filter(|row| row.size == meta.len() && row.modified == modified)
            .map(|row| row.hash.clone());
        if hash.is_none() && scope == Scope::Cached {
            return Ok(None);
        }
        files.push((key, path, meta.len(), modified, hash));
    }
    let to_hash: u64 = files
        .iter()
        .filter(|file| file.4.is_none())
        .map(|file| file.2)
        .sum();
    // The cache keeps the rows of the files not read (yet); a whole
    // fingerprint, once done, drops those of files gone.
    let mut rows = old;
    let (mut hashed, mut cached, mut bytes) = (0, 0, 0);
    let (mut done, mut unsaved) = (0, 0);
    let mut lines = Vec::new();
    for (key, path, size, modified, hash) in files {
        let hash = match hash {
            Some(hash) => {
                cached += 1;
                hash
            }
            None => {
                hashed += 1;
                let hash = hash_file(path)?;
                done += size;
                unsaved += size;
                progress(done, to_hash);
                hash
            }
        };
        bytes += size;
        lines.push((key.clone(), format!("{key}\t{size}\t{hash}\n")));
        rows.insert(
            key,
            CacheRow {
                size,
                modified,
                hash,
            },
        );
        if let Some(cache) = cache
            && unsaved >= CACHE_EVERY
        {
            unsaved = 0;
            save_cache(cache, &rows)?;
        }
    }
    if scope == Scope::All {
        let read: BTreeSet<&str> = lines.iter().map(|(key, _)| key.as_str()).collect();
        rows.retain(|key, _| read.contains(key.as_str()));
    }
    if let Some(cache) = cache
        && hashed > 0
    {
        save_cache(cache, &rows)?;
    }

    let header = format!("{VERSION}\ngame {}\n", game.key());
    let mut whole = blake3::Hasher::new();
    whole.update(header.as_bytes());
    // The items' order is the whole digest's; a group's is its files'.
    let workshop: Vec<u64> = index.workshop().iter().map(|item| item.id).collect();
    if !workshop.is_empty() {
        let ids: Vec<String> = workshop.iter().map(u64::to_string).collect();
        whole.update(format!("workshop {}\n", ids.join(" ")).as_bytes());
    }
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
        workshop,
        beneath: beneath(game, index),
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
