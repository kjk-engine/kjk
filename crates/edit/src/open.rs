//! A save opened for editing, or why it cannot be: every save kjk reads
//! (`sim::SAVE_OLDEST` to `sim::SAVE_FORMAT`) and nothing half-read.

use std::fmt;
use std::path::Path;

/// Why a save does not open.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Refusal {
    /// Made by a newer kjk than this one.
    Newer { found: u32, reads_up_to: u32 },
    /// Older than the oldest kjk still reads.
    Older { found: u32, reads_from: u32 },
    /// Its world does not read: cut short, changed, or not what its meta's
    /// hash says.
    Damaged(String),
    /// No save there: no `save.kjkv` or `meta.json`, or no such folder.
    NotASave(String),
}

impl fmt::Display for Refusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Refusal::Newer { found, reads_up_to } => write!(
                f,
                "made by a newer kjk (save format {found}); this one reads up to {reads_up_to}: use the editor that came with that kjk"
            ),
            Refusal::Older { found, reads_from } => write!(
                f,
                "older than any kjk still reads (save format {found}; the oldest read is {reads_from})"
            ),
            Refusal::Damaged(why) => write!(f, "damaged: {why}"),
            Refusal::NotASave(why) => write!(f, "not a save: {why}"),
        }
    }
}

/// A save as opened: its id in its folder, its world and its meta.
#[derive(Debug, Clone)]
pub struct Opened {
    pub id: String,
    pub save: sim::Save,
    pub meta: saves::Meta,
}

/// Save `id` in the saves' folder `root`, or why not.
pub fn open(root: &Path, id: &str) -> Result<Opened, Refusal> {
    let folder = root.join(id);
    for file in ["save.kjkv", "meta.json"] {
        if !folder.join(file).is_file() {
            return Err(Refusal::NotASave(format!(
                "no {file} in {}",
                folder.display()
            )));
        }
    }
    let (bytes, meta) = saves::read_bytes(root, id).map_err(Refusal::Damaged)?;
    Ok(Opened {
        id: id.to_string(),
        save: open_bytes(&bytes)?,
        meta,
    })
}

/// A save's world from its bytes, or why not.
pub fn open_bytes(bytes: &[u8]) -> Result<sim::Save, Refusal> {
    sim::Save::decode(bytes).map_err(|error| match error {
        sim::DecodeError::Version { found, .. } if found > sim::SAVE_FORMAT => Refusal::Newer {
            found,
            reads_up_to: sim::SAVE_FORMAT,
        },
        sim::DecodeError::Version { found, .. } if found < sim::SAVE_OLDEST => Refusal::Older {
            found,
            reads_from: sim::SAVE_OLDEST,
        },
        other => Refusal::Damaged(other.to_string()),
    })
}
