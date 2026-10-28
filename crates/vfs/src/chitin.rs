//! `chitin.key` and the BIF tables it points at, cross-checked entry by
//! entry. Only the tables are read; resource data stays on disk.

use std::fs::File;
use std::io::{self, Read};
use std::path::PathBuf;

use fmt_odyssey::bif::{self, Bif};
use fmt_odyssey::key::Key;
use fmt_odyssey::{FormatError, ResType};

use crate::DirIndex;

#[derive(Debug)]
pub struct Chitin {
    pub bifs: Vec<ChitinBif>,
    pub entries: Vec<ChitinEntry>,
    pub problems: Vec<ChitinProblem>,
}

#[derive(Debug)]
pub struct ChitinBif {
    /// As `chitin.key` spells it.
    pub key_path: String,
    /// The file on disk, found without regard to case.
    pub path: Option<PathBuf>,
    pub len: u64,
}

#[derive(Debug, Clone)]
pub struct ChitinEntry {
    pub resref: String,
    pub restype: ResType,
    pub bif: usize,
    pub offset: u32,
    pub size: u32,
}

/// Every way a KEY entry can fail to land on data. Each one is counted and
/// reported; an entry with a problem is left out of `entries`.
#[derive(Debug, Clone)]
pub enum ChitinProblem {
    BifMissing {
        key_path: String,
    },
    BifUnreadable {
        key_path: String,
        error: String,
    },
    BifSizeMismatch {
        key_path: String,
        key_size: u32,
        file_len: u64,
    },
    BadBifIndex {
        resref: String,
        restype: ResType,
        bif: usize,
    },
    BadResourceIndex {
        resref: String,
        restype: ResType,
        index: usize,
        count: usize,
    },
    IdMismatch {
        resref: String,
        key_id: u32,
        bif_id: u32,
    },
    TypeMismatch {
        resref: String,
        key_type: ResType,
        bif_type: ResType,
    },
    DataOutOfBounds {
        resref: String,
        restype: ResType,
        end: u64,
        bif_len: u64,
    },
}

impl ChitinProblem {
    pub fn kind(&self) -> &'static str {
        match self {
            ChitinProblem::BifMissing { .. } => "bif-missing",
            ChitinProblem::BifUnreadable { .. } => "bif-unreadable",
            ChitinProblem::BifSizeMismatch { .. } => "bif-size-mismatch",
            ChitinProblem::BadBifIndex { .. } => "bad-bif-index",
            ChitinProblem::BadResourceIndex { .. } => "bad-resource-index",
            ChitinProblem::IdMismatch { .. } => "id-mismatch",
            ChitinProblem::TypeMismatch { .. } => "type-mismatch",
            ChitinProblem::DataOutOfBounds { .. } => "data-out-of-bounds",
        }
    }
}

#[derive(Debug)]
pub enum ChitinError {
    KeyMissing,
    Io(io::Error),
    Format(FormatError),
}

impl std::fmt::Display for ChitinError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ChitinError::KeyMissing => f.write_str("no chitin.key"),
            ChitinError::Io(error) => write!(f, "chitin.key: {error}"),
            ChitinError::Format(error) => write!(f, "{error}"),
        }
    }
}

impl Chitin {
    pub fn load(index: &DirIndex) -> Result<Chitin, ChitinError> {
        let key_path = index.get("chitin.key").ok_or(ChitinError::KeyMissing)?;
        let key_bytes = std::fs::read(key_path).map_err(ChitinError::Io)?;
        let key = Key::parse(&key_bytes).map_err(ChitinError::Format)?;

        let mut problems = Vec::new();
        let mut bifs = Vec::with_capacity(key.bifs.len());
        let mut tables: Vec<Option<Bif>> = Vec::with_capacity(key.bifs.len());
        for key_bif in &key.bifs {
            let path = index.get(&key_bif.path).map(PathBuf::from);
            let (len, table) = match &path {
                None => {
                    problems.push(ChitinProblem::BifMissing {
                        key_path: key_bif.path.clone(),
                    });
                    (0, None)
                }
                Some(path) => match read_table(path) {
                    Ok((len, table)) => (len, Some(table)),
                    Err(error) => {
                        problems.push(ChitinProblem::BifUnreadable {
                            key_path: key_bif.path.clone(),
                            error,
                        });
                        (0, None)
                    }
                },
            };
            if table.is_some() && u64::from(key_bif.size) != len {
                problems.push(ChitinProblem::BifSizeMismatch {
                    key_path: key_bif.path.clone(),
                    key_size: key_bif.size,
                    file_len: len,
                });
            }
            bifs.push(ChitinBif {
                key_path: key_bif.path.clone(),
                path,
                len,
            });
            tables.push(table);
        }

        let mut entries = Vec::with_capacity(key.entries.len());
        for entry in &key.entries {
            let bif = entry.bif_index();
            let Some(slot) = tables.get(bif) else {
                problems.push(ChitinProblem::BadBifIndex {
                    resref: entry.resref.clone(),
                    restype: entry.restype,
                    bif,
                });
                continue;
            };
            // A missing or unreadable BIF is already reported once above.
            let Some(table) = slot else { continue };
            let index = entry.resource_index();
            let Some(resource) = table.resources.get(index) else {
                problems.push(ChitinProblem::BadResourceIndex {
                    resref: entry.resref.clone(),
                    restype: entry.restype,
                    index,
                    count: table.resources.len(),
                });
                continue;
            };
            if resource.id != entry.id {
                problems.push(ChitinProblem::IdMismatch {
                    resref: entry.resref.clone(),
                    key_id: entry.id,
                    bif_id: resource.id,
                });
                continue;
            }
            if resource.restype != entry.restype {
                problems.push(ChitinProblem::TypeMismatch {
                    resref: entry.resref.clone(),
                    key_type: entry.restype,
                    bif_type: resource.restype,
                });
                continue;
            }
            let end = u64::from(resource.offset) + u64::from(resource.size);
            if end > bifs[bif].len {
                problems.push(ChitinProblem::DataOutOfBounds {
                    resref: entry.resref.clone(),
                    restype: entry.restype,
                    end,
                    bif_len: bifs[bif].len,
                });
                continue;
            }
            entries.push(ChitinEntry {
                resref: entry.resref.clone(),
                restype: entry.restype,
                bif,
                offset: resource.offset,
                size: resource.size,
            });
        }
        Ok(Chitin {
            bifs,
            entries,
            problems,
        })
    }
}

/// The BIF's length on disk and its parsed table, reading only the table.
fn read_table(path: &PathBuf) -> Result<(u64, Bif), String> {
    let mut file = File::open(path).map_err(|error| error.to_string())?;
    let len = file.metadata().map_err(|error| error.to_string())?.len();
    let mut prefix = vec![0; bif::HEADER_LEN];
    file.read_exact(&mut prefix)
        .map_err(|error| error.to_string())?;
    let end = Bif::table_end(&prefix).map_err(|error| error.to_string())?;
    if end as u64 > len {
        return Err(format!("table ends at {end}, file is {len} bytes"));
    }
    prefix.resize(end, 0);
    file.read_exact(&mut prefix[bif::HEADER_LEN..])
        .map_err(|error| error.to_string())?;
    let table = Bif::parse(&prefix).map_err(|error| error.to_string())?;
    Ok((len, table))
}
