//! The corpus of real saves each release is checked against before its tag
//! (`kjk saves capture`, `kjk saves check`, `docs/RELEASE.md`; spec 2b of
//! `docs/superpowers/specs/2026-10-28-save-migrations-design.md`): slot
//! folders under `kjk-artifacts/save-corpus/<tag>/<name>/` holding the
//! save, its meta and its story, the verdict on each, and the table that
//! lists them. What loads them needs the games' files and is the launcher's;
//! what decides and prints needs nothing and is here.

use std::collections::BTreeMap;
use std::fmt::{self, Write as _};
use std::path::{Path, PathBuf};

use crate::{META, Meta, STORY, Story, WORLD};

/// The corpus's folder under the artifacts'.
pub const FOLDER: &str = "save-corpus";

/// A slot folder's files.
#[derive(Debug, Clone, PartialEq)]
pub struct Slot {
    /// The save, as `sim::Save::encode` makes it.
    pub world: Vec<u8>,
    pub meta: Option<Meta>,
    pub story: Option<Story>,
}

impl Slot {
    /// The slot in `folder`.
    pub fn open(folder: &Path) -> Result<Slot, String> {
        let files = [WORLD, META, STORY]
            .into_iter()
            .filter_map(|name| Some((name.to_string(), std::fs::read(folder.join(name)).ok()?)))
            .collect();
        Slot::from_files(files).map_err(|error| format!("{}: {error}", folder.display()))
    }

    /// The slot whose files are `files`, by name; refused without a world
    /// or when its meta's hash is not the world's. A story that does not
    /// read is none, as a load takes it.
    pub fn from_files(mut files: BTreeMap<String, Vec<u8>>) -> Result<Slot, String> {
        let world = files.remove(WORLD).ok_or(format!("no {WORLD}"))?;
        let meta = files
            .remove(META)
            .map(|bytes| {
                let text = String::from_utf8_lossy(&bytes);
                crate::parse_meta(&text).map(|(meta, _)| meta)
            })
            .transpose()?;
        let hash = meta.as_ref().and_then(|meta| meta.blake3.as_deref());
        if hash.is_some_and(|hash| hash != blake3::hash(&world).to_hex().as_str()) {
            return Err("the save is damaged: its world is not the one saved".into());
        }
        let story = files
            .remove(STORY)
            .and_then(|bytes| serde_json::from_slice(&bytes).ok());
        Ok(Slot { world, meta, story })
    }

    /// The slot written as `name` under `root`, a slot of that name there
    /// before replaced. Only that folder is ever removed: `name` is one
    /// plain folder name (`valid_name`).
    pub fn write(&self, root: &Path, name: &str) -> Result<(), String> {
        if !valid_name(name) {
            return Err(format!("{name:?}: not a name for a slot"));
        }
        let folder = root.join(name);
        if folder.exists() {
            std::fs::remove_dir_all(&folder).map_err(crate::failed(&folder))?;
        }
        std::fs::create_dir_all(&folder).map_err(crate::failed(&folder))?;
        let meta = self.meta.clone().unwrap_or_else(Meta::default);
        crate::fill(&folder, &self.world, &meta, self.story.as_ref(), None)
    }
}

/// Whether `name` is one plain folder name: not empty, not starting with a
/// dot (`.`, `..`, a hidden folder), with no separator in it.
pub fn valid_name(name: &str) -> bool {
    !name.is_empty() && !name.starts_with('.') && !name.contains(['/', '\\'])
}

/// The slots under `corpus`: `<release>/<name>` folders, or `<name>` ones
/// directly, each as (release, name, folder), in name order.
pub fn slots(corpus: &Path) -> Result<Vec<(String, String, PathBuf)>, String> {
    let names = |folder: &Path| -> Result<Vec<(String, PathBuf)>, String> {
        let entries = std::fs::read_dir(folder).map_err(crate::failed(folder))?;
        let mut found: Vec<(String, PathBuf)> = entries
            .flatten()
            .filter(|entry| entry.path().is_dir())
            .filter_map(|entry| Some((entry.file_name().to_str()?.to_string(), entry.path())))
            .filter(|(name, _)| !name.starts_with('.'))
            .collect();
        found.sort();
        Ok(found)
    };
    let mut slots = Vec::new();
    for (release, folder) in names(corpus)? {
        if folder.join(WORLD).exists() {
            slots.push((String::new(), release, folder));
            continue;
        }
        for (name, slot) in names(&folder)? {
            if slot.join(WORLD).exists() {
                slots.push((release.clone(), name, slot));
            }
        }
    }
    Ok(slots)
}

/// The formats of `bumps/BREAKING.md`'s entries (`- KJKV 72 (bump): why`):
/// each a format's tag and the version from which its older saves are
/// refused.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Breaking(Vec<(String, u32)>);

/// The names the decoders give a format in a refusal, and its tag.
const TAGS: [(&str, &str); 3] = [
    ("save", "KJKV"),
    ("snapshot", "KJKS"),
    ("templates", "KJKT"),
];

impl Breaking {
    /// The entries of `text`; any other line is not one.
    pub fn parse(text: &str) -> Breaking {
        Breaking(
            text.lines()
                .filter_map(|line| {
                    let mut words = line.trim().strip_prefix("- ")?.split_whitespace();
                    let (tag, version) = (words.next()?, words.next()?);
                    Some((tag.to_string(), version.parse().ok()?))
                })
                .collect(),
        )
    }

    /// Why `error` is the refusal of a release older than a `breaking`
    /// bump: a version below the newest entry of its format.
    pub fn refusal(&self, error: &sim::DecodeError) -> Option<String> {
        let sim::DecodeError::Version { format, found } = error else {
            return None;
        };
        let tag = TAGS.iter().find(|(name, _)| name == format)?.1;
        let (_, since) = self
            .0
            .iter()
            .filter(|(entry, since)| entry == tag && found < since)
            .max_by_key(|(_, since)| *since)?;
        Some(format!(
            "{tag} {found} is before the breaking bump {tag} {since}"
        ))
    }
}

/// What a save of the corpus came to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Verdict {
    /// Every step held.
    Pass,
    /// Refused because its release is before a `breaking` bump: the story
    /// restore is what such a save gets.
    Expected(String),
    /// A step did not hold.
    Fail(String),
}

impl Verdict {
    /// The verdict on a save that `decoded` (or was refused, with `error`)
    /// and met `problems` in the steps after. A refusal is expected when
    /// the save's release is before a `breaking` bump, and stays so only if
    /// what rescues it, the story's restore, held (no `problems`).
    pub fn of(
        decoded: Result<(), &sim::DecodeError>,
        breaking: &Breaking,
        problems: &[String],
    ) -> Verdict {
        match decoded {
            Err(error) => match breaking.refusal(error) {
                Some(why) if problems.is_empty() => Verdict::Expected(why),
                Some(why) => Verdict::Fail(format!("{why}, and {}", problems.join("; "))),
                None => Verdict::Fail(format!("refused: {error}")),
            },
            Ok(()) if problems.is_empty() => Verdict::Pass,
            Ok(()) => Verdict::Fail(problems.join("; ")),
        }
    }

    pub fn failed(&self) -> bool {
        matches!(self, Verdict::Fail(_))
    }
}

impl fmt::Display for Verdict {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Verdict::Pass => "PASS",
            Verdict::Expected(_) => "EXPECTED",
            Verdict::Fail(_) => "FAIL",
        })
    }
}

/// One line of the table: a save of the corpus and what each step found.
#[derive(Debug, Clone, PartialEq)]
pub struct Row {
    pub name: String,
    pub release: String,
    /// The formats migrated (`migrated`).
    pub formats: String,
    /// The stamps moved: its own level's, and the module left that was
    /// entered again.
    pub stamps: usize,
    /// What reconciling did (`counts`).
    pub reconcile: String,
    /// `OK`, or what the story restore lost.
    pub story: String,
    pub verdict: Verdict,
}

impl Row {
    fn cells(&self) -> [String; 7] {
        [
            self.name.clone(),
            self.release.clone(),
            self.formats.clone(),
            self.stamps.to_string(),
            self.reconcile.clone(),
            self.story.clone(),
            self.verdict.to_string(),
        ]
    }
}

/// `step`'s row, or, if it panics, a row of `name` and `release` that
/// failed with the panic's words: one save's panic is that save's failure,
/// not the table's.
pub fn guarded(name: &str, release: &str, step: impl FnOnce() -> Row) -> Row {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(step)).unwrap_or_else(|panic| Row {
        name: name.to_string(),
        release: release.to_string(),
        formats: "-".into(),
        stamps: 0,
        reconcile: "-".into(),
        story: "-".into(),
        verdict: Verdict::Fail(format!("panicked: {}", panic_words(&*panic))),
    })
}

/// The words of a caught panic.
pub fn panic_words(panic: &(dyn std::any::Any + Send)) -> &str {
    panic
        .downcast_ref::<String>()
        .map(String::as_str)
        .or_else(|| panic.downcast_ref::<&str>().copied())
        .unwrap_or("a panic")
}

const HEADERS: [&str; 7] = [
    "save",
    "release",
    "formats migrated",
    "stamps",
    "reconcile m/d/a/s/l",
    "story",
    "verdict",
];

/// `rows` as a table, its columns as wide as their longest cell, and under
/// it the reason for each save that did not pass.
pub fn table(rows: &[Row]) -> String {
    let cells: Vec<[String; 7]> = rows.iter().map(Row::cells).collect();
    let widths: Vec<usize> = (0..HEADERS.len())
        .map(|column| {
            cells
                .iter()
                .map(|row| row[column].chars().count())
                .chain([HEADERS[column].chars().count()])
                .max()
                .unwrap_or(0)
        })
        .collect();
    let line = |cells: &[&str]| {
        let padded: Vec<String> = cells
            .iter()
            .zip(&widths)
            .map(|(cell, width)| format!("{cell:<width$}"))
            .collect();
        format!("| {} |\n", padded.join(" | "))
    };
    let mut out = line(&HEADERS);
    let rules: Vec<String> = widths.iter().map(|width| "-".repeat(*width)).collect();
    out += &line(&rules.iter().map(String::as_str).collect::<Vec<_>>());
    for row in &cells {
        out += &line(&row.iter().map(String::as_str).collect::<Vec<_>>());
    }
    for row in rows {
        if let Verdict::Expected(why) | Verdict::Fail(why) = &row.verdict {
            let _ = writeln!(out, "{}: {}: {why}", row.verdict, row.name);
        }
    }
    out
}

/// The formats `found` holds that this build reads by a migration, as
/// `KJKV 59→71`, else `none`.
pub fn migrated(found: sim::StoredVersions) -> String {
    let formats = [
        ("KJKV", found.save, sim::SAVE_FORMAT),
        ("KJKS", found.snapshot, sim::SNAPSHOT_FORMAT),
        ("KJKT", found.pack, sim::pack::VERSION),
    ];
    let moved: Vec<String> = formats
        .into_iter()
        .filter(|(_, was, now)| was != now)
        .map(|(tag, was, now)| format!("{tag} {was}→{now}"))
        .collect();
    if moved.is_empty() {
        "none".into()
    } else {
        moved.join(", ")
    }
}

/// What a reconcile did, as the table has it: moved, dropped, added,
/// snapped and left (creatures left where the game put them, off the
/// mesh), with `trusted` before them for a save without placements kept.
pub fn counts(report: &sim::reconcile::Report) -> String {
    let left = report.snaps.len().saturating_sub(report.snapped);
    let numbers = format!(
        "{}/{}/{}/{}/{left}",
        report.moved,
        report.dropped.len(),
        report.added.len(),
        report.snapped
    );
    if report.trusted {
        format!("trusted {numbers}")
    } else {
        numbers
    }
}

#[cfg(feature = "probes")]
mod probes;
#[cfg(feature = "probes")]
pub use probes::probes;
