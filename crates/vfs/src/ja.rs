//! Jedi Academy's `base/` folder and, when one is named, its mod folder
//! (`DirIndex::ja_mod`, `MD`): their PK3 archives in the engine's load
//! order and their loose files.
//!
//! The engine adds a folder itself first and then pushes every PK3 in
//! front of it, so any PK3 hides a loose file of the same name in its
//! folder, and a PK3 later in the sort hides an earlier one. The sort
//! compares names with letters folded to upper case and `\` or `:` read as
//! `/`. A mod folder is added after `base/`, the same way, so all of it
//! (its loose files too) is above all of `base/`.

use std::cmp::Ordering;
use std::collections::BTreeMap;
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::ops::Range;
use std::path::{Path, PathBuf};

use fmt_ja::pk3::{self, Pk3Entry};

use crate::DirIndex;

#[derive(Debug)]
pub struct Pk3File {
    pub path: PathBuf,
    /// The file name, as on disk.
    pub name: String,
    pub len: u64,
    pub entries: Vec<Pk3Entry>,
}

impl Pk3File {
    pub fn open(path: &Path) -> Result<Pk3File, String> {
        let mut file = File::open(path).map_err(|error| error.to_string())?;
        let len = file.metadata().map_err(|error| error.to_string())?.len();
        let tail_len = len.min(pk3::TAIL_LEN as u64);
        let mut tail = vec![0; tail_len as usize];
        file.seek(SeekFrom::Start(len - tail_len))
            .and_then(|_| file.read_exact(&mut tail))
            .map_err(|error| error.to_string())?;
        let directory = pk3::directory(&tail, len).map_err(|error| error.to_string())?;
        let mut central = vec![0; directory.size as usize];
        file.seek(SeekFrom::Start(directory.offset))
            .and_then(|_| file.read_exact(&mut central))
            .map_err(|error| error.to_string())?;
        let entries = pk3::entries(&central, directory).map_err(|error| error.to_string())?;
        Ok(Pk3File {
            path: path.to_path_buf(),
            name: file_name(path),
            len,
            entries,
        })
    }

    /// Reads, decompresses and CRC-checks one entry.
    pub fn read(&self, entry: &Pk3Entry) -> Result<Vec<u8>, String> {
        let mut file = File::open(&self.path).map_err(|error| error.to_string())?;
        let mut local = [0; pk3::LOCAL_HEADER_LEN];
        file.seek(SeekFrom::Start(u64::from(entry.local_header)))
            .and_then(|_| file.read_exact(&mut local))
            .map_err(|error| format!("{}: {error}", entry.name))?;
        let offset = pk3::data_offset(&local, entry).map_err(|error| error.to_string())?;
        let end = u64::from(entry.local_header) + offset + u64::from(entry.compressed);
        if end > self.len {
            return Err(format!(
                "{}: data ends at {end}, the archive is {} bytes",
                entry.name, self.len
            ));
        }
        let mut data = vec![0; entry.compressed as usize];
        file.seek(SeekFrom::Start(u64::from(entry.local_header) + offset))
            .and_then(|_| file.read_exact(&mut data))
            .map_err(|error| format!("{}: {error}", entry.name))?;
        pk3::decode(entry, &data).map_err(|error| error.to_string())
    }
}

/// A path's file name, or the whole path when it has none.
fn file_name(path: &Path) -> String {
    path.file_name().map_or_else(
        || path.display().to_string(),
        |name| name.to_string_lossy().into_owned(),
    )
}

/// The engine's PK3 sort: byte order after folding `a..z` to upper case and
/// reading `\` and `:` as `/`.
pub fn pak_order(a: &str, b: &str) -> Ordering {
    let fold = |c: u8| match c {
        b'a'..=b'z' => c - (b'a' - b'A'),
        b'\\' | b':' => b'/',
        _ => c,
    };
    a.bytes().map(fold).cmp(b.bytes().map(fold))
}

/// What the game, the store or the system writes into `base/` (or a mod
/// folder): saves,
/// screenshots, demos, settings and logs, the game's own libraries, and
/// folder metadata (`crate::system_file`). None is an asset. Settings and
/// logs only at the folder's root: a model's `animation.cfg` is data.
const NOT_DATA_FOLDERS: &[&str] = &["saves/", "screenshots/", "demos/"];
const NOT_DATA_ROOT_EXTENSIONS: &[&str] = &[".cfg", ".log"];
const NOT_DATA_EXTENSIONS: &[&str] = &[".dll", ".so", ".dylib", ".exe"];

/// Whether a lowercased path under `base/` (or a mod folder) is game
/// data: a PK3, or a loose
/// file the engine could read in place of an archive's.
pub fn game_data(rest: &str) -> bool {
    let name = rest.rsplit('/').next().unwrap_or(rest);
    let at_root = !rest.contains('/');
    !(NOT_DATA_FOLDERS
        .iter()
        .any(|folder| rest.starts_with(folder))
        || (at_root
            && NOT_DATA_ROOT_EXTENSIONS
                .iter()
                .any(|ext| name.ends_with(ext)))
        || NOT_DATA_EXTENSIONS.iter().any(|ext| name.ends_with(ext))
        || crate::system_file(name)
        || rest.contains(".bundle/"))
}

/// Whether a lowercased path under `base/` (or a mod folder) is one of the
/// PK3s the engine loads (those at the folder's root).
pub fn root_pk3(rest: &str) -> bool {
    rest.ends_with(".pk3") && !rest.contains('/')
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JaSource {
    /// A loose file of `JaStack::folders[folder]`.
    Loose { folder: usize },
    /// An entry of `JaStack::pk3s[pk3]`.
    Pk3 { pk3: usize, entry: usize },
}

/// A folder the engine reads: `base/`, then the mod's.
#[derive(Debug)]
pub struct JaFolder {
    /// Lowercased, as the index has it (`base`, `md`).
    pub name: String,
    /// Its PK3s' places in `JaStack::pk3s`.
    pub pk3s: Range<usize>,
    /// Its loose files, by lowercased path under it.
    pub loose: BTreeMap<String, PathBuf>,
}

#[derive(Debug)]
pub struct JaStack {
    /// In load order: later entries win.
    pub pk3s: Vec<Pk3File>,
    /// `base/`, then the mod folder when there is one: lowest first.
    pub folders: Vec<JaFolder>,
    pub problems: Vec<String>,
}

/// One name after precedence.
#[derive(Debug)]
pub struct JaResolved {
    pub source: JaSource,
    pub size: u64,
    /// Lower sources holding the same name, highest first.
    pub shadowed: Vec<JaSource>,
}

/// The folder every install has.
const BASE: &str = "base";

impl JaStack {
    /// `index` is the JA data root (the folder holding `base/`); its mod
    /// folder (`DirIndex::ja_mod`), if any, loads after `base/`.
    pub fn open(index: &DirIndex) -> JaStack {
        let mut stack = JaStack {
            pk3s: Vec::new(),
            folders: Vec::new(),
            problems: Vec::new(),
        };
        for folder in std::iter::once(BASE).chain(index.ja_mod()) {
            stack.add_folder(index, folder);
        }
        stack
    }

    /// `folder`'s root PK3s in the engine's sort, over every folder added
    /// before, and its loose files.
    fn add_folder(&mut self, index: &DirIndex, folder: &str) {
        let mut paks: Vec<&Path> = index
            .files_in(folder)
            .into_iter()
            .filter(|(name, _)| name.ends_with(".pk3"))
            .map(|(_, path)| path)
            .collect();
        let prefix = format!("{folder}/");
        let loose = index
            .iter()
            .filter_map(|(key, path)| {
                let rest = key.strip_prefix(&prefix)?;
                (!root_pk3(rest) && game_data(rest)).then(|| (rest.to_string(), path.to_path_buf()))
            })
            .collect();
        paks.sort_by(|a, b| pak_order(&file_name(a), &file_name(b)));
        let first = self.pk3s.len();
        for path in paks {
            match Pk3File::open(path) {
                Ok(pak) => self.pk3s.push(pak),
                Err(error) => self.problems.push(format!("{}: {error}", path.display())),
            }
        }
        self.folders.push(JaFolder {
            name: folder.to_string(),
            pk3s: first..self.pk3s.len(),
            loose,
        });
    }

    /// The folder `pk3` is in.
    pub fn folder_of(&self, pk3: usize) -> &JaFolder {
        self.folders
            .iter()
            .find(|folder| folder.pk3s.contains(&pk3))
            .unwrap_or(&self.folders[0])
    }

    /// Every folder's loose files.
    pub fn loose_files(&self) -> usize {
        self.folders.iter().map(|folder| folder.loose.len()).sum()
    }

    /// Where loose file `name` of `folder` is on disk.
    pub fn loose_path(&self, name: &str, folder: usize) -> Option<&Path> {
        let folder = self.folders.get(folder)?;
        folder.loose.get(name).map(PathBuf::as_path)
    }

    /// An archive's file name, after its folder's for a mod folder's
    /// (`md/zz_md.pk3`).
    pub fn pk3_name(&self, pk3: usize) -> String {
        match self.folder_of(pk3).name.as_str() {
            BASE => self.pk3s[pk3].name.clone(),
            folder => format!("{folder}/{}", self.pk3s[pk3].name),
        }
    }

    /// A source's name: its archive's (`pk3_name`), or its folder's loose
    /// files.
    pub fn source_name(&self, source: JaSource) -> String {
        match source {
            JaSource::Loose { folder } => format!("{}/ (loose)", self.folders[folder].name),
            JaSource::Pk3 { pk3, .. } => self.pk3_name(pk3),
        }
    }

    /// Every name, resolved. Folder entries are left out.
    pub fn all(&self) -> BTreeMap<&str, JaResolved> {
        self.all_in(self.folders.len())
    }

    /// Every name of `base/` alone, resolved: what JA itself reads, a mod
    /// folder (`DirIndex::ja_mod`) left out.
    pub fn base(&self) -> BTreeMap<&str, JaResolved> {
        self.all_in(1)
    }

    /// Every name of the first `folders` folders, resolved.
    fn all_in(&self, folders: usize) -> BTreeMap<&str, JaResolved> {
        let mut all: BTreeMap<&str, JaResolved> = BTreeMap::new();
        let mut add = |name, source, size| {
            all.entry(name)
                .and_modify(|resolved: &mut JaResolved| resolved.shadowed.push(source))
                .or_insert(JaResolved {
                    source,
                    size,
                    shadowed: Vec::new(),
                });
        };
        // Highest first: the last folder's archives, last first, then its
        // loose files.
        for (folder_index, folder) in self.folders.iter().enumerate().take(folders).rev() {
            for pk3_index in folder.pk3s.clone().rev() {
                // Within one archive the later entry wins.
                let pak = &self.pk3s[pk3_index];
                for (entry_index, entry) in pak.entries.iter().enumerate().rev() {
                    if entry.is_folder() {
                        continue;
                    }
                    let source = JaSource::Pk3 {
                        pk3: pk3_index,
                        entry: entry_index,
                    };
                    add(entry.name.as_str(), source, u64::from(entry.size));
                }
            }
            for (name, path) in &folder.loose {
                let size = std::fs::metadata(path).map_or(0, |meta| meta.len());
                let source = JaSource::Loose {
                    folder: folder_index,
                };
                add(name.as_str(), source, size);
            }
        }
        all
    }

    pub fn read(&self, name: &str, source: JaSource) -> Result<Vec<u8>, String> {
        match source {
            JaSource::Loose { folder } => {
                let path = self
                    .loose_path(name, folder)
                    .ok_or(format!("{name}: not loose"))?;
                std::fs::read(path).map_err(|error| error.to_string())
            }
            JaSource::Pk3 { pk3, entry } => {
                let pak = &self.pk3s[pk3];
                pak.read(&pak.entries[entry])
            }
        }
    }
}
