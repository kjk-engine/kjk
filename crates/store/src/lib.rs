//! The store (`docs/IMPORT.md`, part 2): one folder per machine, outside
//! every repository, where the importer's products and other derived files
//! live, shared by every worktree, every branch and every process.
//!
//! - **Where:** `KJK_STORE`, else the user's cache folder
//!   (`~/Library/Caches/kjk`, `$XDG_CACHE_HOME/kjk` or
//!   `~/.cache/kjk`, `%LOCALAPPDATA%\kjk`).
//! - **What:** files by kind (a path such as `import/k1/items`) and key.
//!   A key is the hash of everything that made the file (`Key::of`: the
//!   maker's format word and its inputs), so two branches with different
//!   makers never write the same key, and a key found is always the bytes a
//!   key made would be.
//! - **How:** a file is written to a temporary name beside it and renamed
//!   into place, so a reader never sees half of one and two processes
//!   making the same key both end with the same file.
//! - **Use:** reading a file marks it used (its modified time), so
//!   `gc` can remove what nothing used for a while.
//! - **Formats:** a kind whose maker's format changes names it
//!   (`bake/k1/area-v3`), so `gc` drops the older ones whole once a newer
//!   one is there.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, SystemTime};

/// A file's key: the hex BLAKE3 of what made it.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Key(String);

impl Key {
    /// The key of the file `format` (the maker's name and version, bumped
    /// when its output changes) makes from `inputs`.
    pub fn of(format: &str, inputs: &[&[u8]]) -> Key {
        let mut hasher = blake3::Hasher::new();
        hasher.update(&(format.len() as u64).to_le_bytes());
        hasher.update(format.as_bytes());
        for input in inputs {
            hasher.update(&(input.len() as u64).to_le_bytes());
            hasher.update(input);
        }
        Key(hasher.finalize().to_hex().to_string())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// The store at a root folder.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Store {
    root: PathBuf,
}

/// What one kind of file takes: how many, how many bytes.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Usage {
    pub files: u64,
    pub bytes: u64,
}

/// One of our folders under the project's name before 2026-10-28
/// (`kotor-jk`) is moved to `folder`, its name now, the first time it is
/// looked for, so settings, saves and the store stay where they were.
pub fn carry_old_name(folder: &Path) {
    let (Some(parent), Some("kjk")) =
        (folder.parent(), folder.file_name().and_then(|n| n.to_str()))
    else {
        return;
    };
    let old = parent.join("kotor-jk");
    if !folder.exists() && old.is_dir() {
        let _ = fs::rename(&old, folder);
    }
}

impl Store {
    /// The machine's store: `KJK_STORE`, else the user's cache folder.
    /// `None` when neither can be told (no home folder).
    pub fn open() -> Option<Store> {
        Store::root_from(|name| std::env::var(name).ok())
            .inspect(|root| carry_old_name(root))
            .map(Store::at)
    }

    /// The store's root as `var` (the environment) gives it.
    pub fn root_from(var: impl Fn(&str) -> Option<String>) -> Option<PathBuf> {
        let set = |name: &str| {
            var(name)
                .filter(|value| !value.is_empty())
                .map(PathBuf::from)
        };
        if let Some(root) = set("KJK_STORE") {
            return Some(root);
        }
        let base = if cfg!(target_os = "macos") {
            set("HOME").map(|home| home.join("Library/Caches"))
        } else if cfg!(windows) {
            set("LOCALAPPDATA")
        } else {
            set("XDG_CACHE_HOME").or_else(|| set("HOME").map(|home| home.join(".cache")))
        };
        base.map(|base| base.join("kjk"))
    }

    /// The store at `root` (made when first written to).
    pub fn at(root: impl Into<PathBuf>) -> Store {
        Store { root: root.into() }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Where `kind`'s file `key` is (it may not exist).
    pub fn path(&self, kind: &str, key: &Key) -> PathBuf {
        self.root.join(kind).join(&key.0[..2]).join(&key.0)
    }

    /// The bytes of `kind`'s file `key`, if it is there (marked used).
    pub fn get(&self, kind: &str, key: &Key) -> Option<Vec<u8>> {
        let path = self.path(kind, key);
        let bytes = fs::read(&path).ok()?;
        mark_used(&path);
        Some(bytes)
    }

    /// Puts `bytes` as `kind`'s file `key`, atomically; its path.
    pub fn put(&self, kind: &str, key: &Key, bytes: &[u8]) -> io::Result<PathBuf> {
        let path = self.path(kind, key);
        write_atomically(&path, bytes)?;
        Ok(path)
    }

    /// `kind`'s file `key`: the one there, else what `make` makes, put.
    /// A store that cannot be written still answers with what `make` made.
    pub fn get_or_make<E>(
        &self,
        kind: &str,
        key: &Key,
        make: impl FnOnce() -> Result<Vec<u8>, E>,
    ) -> Result<Vec<u8>, E> {
        if let Some(bytes) = self.get(kind, key) {
            return Ok(bytes);
        }
        let bytes = make()?;
        let _ = self.put(kind, key, &bytes);
        Ok(bytes)
    }

    /// A file of `kind` named `name` (not content-addressed: an index the
    /// maker keeps up to date itself, such as the install fingerprints).
    pub fn named(&self, kind: &str, name: &str) -> PathBuf {
        self.root.join(kind).join(name)
    }

    /// What each kind takes, by kind (the folder above each file's key
    /// folder, relative to the root), in order.
    pub fn usage(&self) -> io::Result<Vec<(String, Usage)>> {
        let mut usage: std::collections::BTreeMap<String, Usage> = Default::default();
        for (path, meta) in files(&self.root)? {
            let kind = path
                .strip_prefix(&self.root)
                .ok()
                .and_then(|relative| relative.parent())
                .map(|parent| {
                    // A content-addressed file sits in `<kind>/<xx>/<key>`.
                    let keyed = path.file_name().is_some_and(|n| n.len() == 64)
                        && parent.file_name().is_some_and(|n| n.len() == 2);
                    let kind = if keyed { parent.parent() } else { Some(parent) };
                    kind.map_or_else(String::new, |kind| kind.display().to_string())
                })
                .unwrap_or_else(String::new);
            usage.entry(kind).or_default().add(&meta);
        }
        Ok(usage.into_iter().collect())
    }

    /// Removes every kind a newer format supersedes (`superseded`), then
    /// every file not used for `unused` (by its modified time), or only
    /// counts them with `dry_run`; what it removed (or would).
    pub fn gc(&self, unused: Duration, dry_run: bool) -> io::Result<Collected> {
        let mut collected = Collected {
            folders: self.superseded()?,
            ..Collected::default()
        };
        for folder in &collected.folders {
            // File by file, so what is counted is what went.
            for (path, meta) in files(folder)? {
                if dry_run || fs::remove_file(&path).is_ok() {
                    collected.superseded.add(&meta);
                }
            }
            if !dry_run {
                let _ = fs::remove_dir_all(folder);
            }
        }
        let now = SystemTime::now();
        for (path, meta) in files(&self.root)? {
            let used = meta.modified().unwrap_or(now);
            if collected
                .folders
                .iter()
                .any(|folder| path.starts_with(folder))
                || now.duration_since(used).unwrap_or(Duration::ZERO) < unused
            {
                continue;
            }
            if dry_run || fs::remove_file(&path).is_ok() {
                collected.unused.add(&meta);
            }
        }
        Ok(collected)
    }

    /// The kind folders a newer format of theirs supersedes: `<name>-v<N>`
    /// beside a `<name>-v<M>` with M greater, and a plain `<name>` (kept
    /// before kinds named their format) beside any `<name>-v<M>`. A store
    /// a newer build wrote loses nothing to an older one's gc: only lower
    /// versions than the highest there go. Only kind folders count (their
    /// folders all key folders, `<xx>`), so a root shared with other
    /// programs' folders loses none of theirs.
    pub fn superseded(&self) -> io::Result<Vec<PathBuf>> {
        let mut found = Vec::new();
        let mut folders = vec![self.root.clone()];
        while let Some(folder) = folders.pop() {
            let entries = match fs::read_dir(&folder) {
                Ok(entries) => entries,
                Err(error) if error.kind() == io::ErrorKind::NotFound => continue,
                Err(error) => return Err(error),
            };
            let mut children = Vec::new();
            for entry in entries {
                let entry = entry?;
                if entry.file_type()?.is_dir() {
                    let name = entry.file_name().to_string_lossy().into_owned();
                    children.push((is_kind(&entry.path())?, name));
                }
            }
            let mut newest: std::collections::BTreeMap<&str, u32> = Default::default();
            for (name, version) in children
                .iter()
                .filter(|(kind, _)| *kind)
                .filter_map(|(_, child)| versioned(child))
            {
                let highest = newest.entry(name).or_default();
                *highest = (*highest).max(version);
            }
            for (kind, child) in &children {
                let old = *kind
                    && match versioned(child) {
                        Some((name, version)) => version < newest[name],
                        None => newest.contains_key(child.as_str()),
                    };
                let path = folder.join(child);
                if old {
                    found.push(path);
                } else {
                    folders.push(path);
                }
            }
        }
        found.sort();
        Ok(found)
    }
}

/// What a gc removed (or would): the kind folders a newer format
/// superseded and their files, and files no run used for a while.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Collected {
    pub folders: Vec<PathBuf>,
    pub superseded: Usage,
    pub unused: Usage,
}

impl Usage {
    fn add(&mut self, meta: &fs::Metadata) {
        self.files += 1;
        self.bytes += meta.len();
    }
}

/// Whether `folder` holds keyed files only: its entries all key folders
/// (two hex digits), and at least one.
fn is_kind(folder: &Path) -> io::Result<bool> {
    let mut any = false;
    for entry in fs::read_dir(folder)? {
        let entry = entry?;
        let name = entry.file_name();
        let keyed = name.len() == 2
            && name
                .to_string_lossy()
                .bytes()
                .all(|b| b.is_ascii_hexdigit());
        if !keyed || !entry.file_type()?.is_dir() {
            return Ok(false);
        }
        any = true;
    }
    Ok(any)
}

/// A kind folder's name and format version: `area-v3` is `("area", 3)`.
fn versioned(folder: &str) -> Option<(&str, u32)> {
    let (name, version) = folder.rsplit_once("-v")?;
    if name.is_empty() || !version.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    Some((name, version.parse().ok()?))
}

/// Every file under `root` (none when it does not exist yet), skipping
/// temporary ones being written.
fn files(root: &Path) -> io::Result<Vec<(PathBuf, fs::Metadata)>> {
    let mut found = Vec::new();
    let mut folders = vec![root.to_path_buf()];
    while let Some(folder) = folders.pop() {
        let entries = match fs::read_dir(&folder) {
            Ok(entries) => entries,
            Err(error) if error.kind() == io::ErrorKind::NotFound => continue,
            Err(error) => return Err(error),
        };
        for entry in entries {
            let entry = entry?;
            let meta = entry.metadata()?;
            let path = entry.path();
            if meta.is_dir() {
                folders.push(path);
            } else if !is_temporary(&path) {
                found.push((path, meta));
            }
        }
    }
    found.sort_by(|a, b| a.0.cmp(&b.0));
    Ok(found)
}

const TEMPORARY: &str = ".kjk-tmp";

fn is_temporary(path: &Path) -> bool {
    path.extension().is_some_and(|ext| ext == &TEMPORARY[1..])
}

/// Writes `bytes` to `path` through a temporary file beside it renamed into
/// place: readers see the old file or the whole new one.
pub fn write_atomically(path: &Path, bytes: &[u8]) -> io::Result<()> {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let name = path
        .file_name()
        .map_or_else(String::new, |name| name.to_string_lossy().into_owned());
    let temporary = path.with_file_name(format!(
        "{name}.{}-{}{TEMPORARY}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    fs::write(&temporary, bytes)?;
    fs::rename(&temporary, path).inspect_err(|_| {
        let _ = fs::remove_file(&temporary);
    })
}

/// Marks `path` used now (its modified time), for `gc`; failing is fine.
fn mark_used(path: &Path) {
    if let Ok(file) = fs::File::options().append(true).open(path) {
        let _ = file.set_modified(SystemTime::now());
    }
}
