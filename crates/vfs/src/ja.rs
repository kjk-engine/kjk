//! Jedi Academy's `base/` folder: its PK3 archives in the engine's load order
//! and its loose files.
//!
//! The engine adds the folder itself first and then pushes every PK3 in
//! front of it, so any PK3 hides a loose file of the same name, and a PK3
//! later in the sort hides an earlier one. The sort compares names with
//! letters folded to upper case and `\` or `:` read as `/`.

use std::cmp::Ordering;
use std::collections::BTreeMap;
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JaSource {
    /// A loose file under `base/`.
    Loose,
    /// An entry of `JaStack::pk3s[pk3]`.
    Pk3 { pk3: usize, entry: usize },
}

#[derive(Debug)]
pub struct JaStack {
    /// In load order: later entries win.
    pub pk3s: Vec<Pk3File>,
    /// Loose files under `base/`, by lowercased relative path.
    pub loose: BTreeMap<String, PathBuf>,
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

impl JaStack {
    /// `index` is the JA data root (the folder holding `base/`).
    pub fn open(index: &DirIndex) -> JaStack {
        let mut stack = JaStack {
            pk3s: Vec::new(),
            loose: BTreeMap::new(),
            problems: Vec::new(),
        };
        let mut paks = Vec::new();
        for (name, path) in index.files_in("base") {
            if name.ends_with(".pk3") {
                paks.push(path);
            }
        }
        for (key, path) in index.iter() {
            if let Some(rest) = key.strip_prefix("base/")
                && !(rest.ends_with(".pk3") && !rest.contains('/'))
            {
                stack.loose.insert(rest.to_string(), path.to_path_buf());
            }
        }
        paks.sort_by(|a, b| pak_order(&file_name(a), &file_name(b)));
        for path in paks {
            match Pk3File::open(path) {
                Ok(pak) => stack.pk3s.push(pak),
                Err(error) => stack.problems.push(format!("{}: {error}", path.display())),
            }
        }
        stack
    }

    pub fn source_name(&self, source: JaSource) -> String {
        match source {
            JaSource::Loose => "base/ (loose)".to_string(),
            JaSource::Pk3 { pk3, .. } => self.pk3s[pk3].name.clone(),
        }
    }

    /// Every name, resolved. Folder entries are left out.
    pub fn all(&self) -> BTreeMap<&str, JaResolved> {
        let mut all: BTreeMap<&str, JaResolved> = BTreeMap::new();
        for (pk3_index, pak) in self.pk3s.iter().enumerate().rev() {
            // Within one archive the later entry wins.
            for (entry_index, entry) in pak.entries.iter().enumerate().rev() {
                if entry.is_folder() {
                    continue;
                }
                let source = JaSource::Pk3 {
                    pk3: pk3_index,
                    entry: entry_index,
                };
                all.entry(&entry.name)
                    .and_modify(|resolved| resolved.shadowed.push(source))
                    .or_insert(JaResolved {
                        source,
                        size: u64::from(entry.size),
                        shadowed: Vec::new(),
                    });
            }
        }
        for (name, path) in &self.loose {
            let size = std::fs::metadata(path).map_or(0, |meta| meta.len());
            all.entry(name)
                .and_modify(|resolved| resolved.shadowed.push(JaSource::Loose))
                .or_insert(JaResolved {
                    source: JaSource::Loose,
                    size,
                    shadowed: Vec::new(),
                });
        }
        all
    }

    pub fn read(&self, name: &str, source: JaSource) -> Result<Vec<u8>, String> {
        match source {
            JaSource::Loose => {
                let path = self.loose.get(name).ok_or(format!("{name}: not loose"))?;
                std::fs::read(path).map_err(|error| error.to_string())
            }
            JaSource::Pk3 { pk3, entry } => {
                let pak = &self.pk3s[pk3];
                pak.read(&pak.entries[entry])
            }
        }
    }
}
