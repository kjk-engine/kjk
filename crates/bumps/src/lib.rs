//! What a branch changes that only its merge may number: a format's version
//! and the sim hash's pin. A branch says so in a file of its own,
//! `bumps/<name>.toml`, so two branches never edit the same line; the pin
//! workflow (`cargo xtask pin`) numbers the bumps in the order they reached
//! `main`, writes the hash, and takes the files away (`bumps/README.md`).
//!
//! ```toml
//! sim = "why the sim's hash moves"
//!
//! [formats]
//! snapshot = "a fighter carries its blaster's bolt model (`Fighter::bolt`)."
//! ```
//!
//! Only that much TOML is read: `key = "text"` lines, `#` comments and the
//! `[formats]` table. No dependencies, as the sim's build script reads it.

use std::path::{Path, PathBuf};

/// The folder of the pending bumps, under the repository's root.
pub const DIR: &str = "bumps";

/// A versioned format: its constant reads `N + crate::pending::<NAME>`,
/// the bumps pending on the branch counted by the sim's build script.
pub struct Format {
    /// Its key in a bump's `[formats]`.
    pub name: &'static str,
    /// The magic its encoding opens with, as reports name it (`KJKS 70`).
    pub tag: &'static str,
    /// The source file holding its constant, from the root.
    pub file: &'static str,
    /// The constant's name there.
    pub constant: &'static str,
}

pub const FORMATS: &[Format] = &[
    Format {
        name: "snapshot",
        tag: "KJKS",
        file: "crates/sim/src/snapshot.rs",
        constant: "VERSION",
    },
    Format {
        name: "save",
        tag: "KJKV",
        file: "crates/sim/src/save.rs",
        constant: "VERSION",
    },
    Format {
        name: "replay",
        tag: "KJKR",
        file: "crates/sim/src/replay.rs",
        constant: "VERSION",
    },
    Format {
        name: "pack",
        tag: "KJKT",
        file: "crates/sim/src/pack.rs",
        constant: "VERSION",
    },
    Format {
        name: "gathered",
        tag: "KJKL",
        file: "crates/sim/src/pack.rs",
        constant: "GATHERED_VERSION",
    },
    Format {
        name: "install",
        tag: "KJKA",
        file: "crates/sim/src/install.rs",
        constant: "VERSION",
    },
];

pub fn format(name: &str) -> Option<&'static Format> {
    FORMATS.iter().find(|format| format.name == name)
}

/// One branch's bump file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Bump {
    pub path: PathBuf,
    /// Why the sim's hash moves on purpose, if it does beyond its formats.
    pub sim: Option<String>,
    /// Each format bumped, with the line its version's history gets.
    pub formats: Vec<(&'static str, String)>,
}

impl Bump {
    /// Whether the pinned hash is left to the merge: the sim moves on
    /// purpose, or the snapshot's version (part of what is hashed) does.
    pub fn moves_hash(&self) -> bool {
        self.sim.is_some() || self.formats.iter().any(|(name, _)| *name == "snapshot")
    }
}

/// The bump files under `root`, by name; an error names a bad one.
pub fn pending(root: &Path) -> Result<Vec<Bump>, String> {
    let dir = root.join(DIR);
    let mut paths = Vec::new();
    if dir.is_dir() {
        for entry in
            std::fs::read_dir(&dir).map_err(|error| format!("{}: {error}", dir.display()))?
        {
            let path = entry
                .map_err(|error| format!("{}: {error}", dir.display()))?
                .path();
            if path
                .extension()
                .is_some_and(|extension| extension == "toml")
            {
                paths.push(path);
            }
        }
    }
    paths.sort();
    paths
        .into_iter()
        .map(|path| {
            let text = std::fs::read_to_string(&path)
                .map_err(|error| format!("{}: {error}", path.display()))?;
            parse(&text)
                .map(|(sim, formats)| Bump {
                    path: path.clone(),
                    sim,
                    formats,
                })
                .map_err(|error| format!("{}: {error}", path.display()))
        })
        .collect()
}

/// How many of `bumps` bump `format`.
pub fn count(bumps: &[Bump], format: &str) -> u32 {
    bumps
        .iter()
        .flat_map(|bump| &bump.formats)
        .filter(|(name, _)| *name == format)
        .count() as u32
}

type Parsed = (Option<String>, Vec<(&'static str, String)>);

fn parse(text: &str) -> Result<Parsed, String> {
    let mut sim = None;
    let mut formats = Vec::new();
    let mut in_formats = false;
    for (index, line) in text.lines().enumerate() {
        let line = line.trim();
        let at = |what: String| format!("line {}: {what}", index + 1);
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if line.starts_with('[') {
            if line != "[formats]" {
                return Err(at(format!("unknown table {line}: only [formats]")));
            }
            in_formats = true;
            continue;
        }
        let (key, value) = line
            .split_once('=')
            .ok_or(at("not `key = \"text\"`".into()))?;
        let key = key.trim();
        let value = string(value.trim()).ok_or(at(format!("{key}: not a quoted string")))?;
        if value.is_empty() {
            return Err(at(format!("{key}: say why")));
        }
        if !in_formats {
            if key != "sim" {
                return Err(at(format!(
                    "unknown key {key}: `sim`, or a format under [formats]"
                )));
            }
            sim = Some(value);
        } else {
            let known = format(key).ok_or_else(|| {
                let names: Vec<_> = FORMATS.iter().map(|format| format.name).collect();
                at(format!("unknown format {key}: one of {}", names.join(", ")))
            })?;
            if formats.iter().any(|(name, _)| *name == known.name) {
                return Err(at(format!("{key} twice")));
            }
            formats.push((known.name, value));
        }
    }
    if sim.is_none() && formats.is_empty() {
        return Err("bumps nothing: `sim = \"…\"` or a format under [formats]".into());
    }
    Ok((sim, formats))
}

/// A basic TOML string: `"…"` with `\"` and `\\`, then nothing but a
/// comment.
fn string(text: &str) -> Option<String> {
    let mut chars = text.strip_prefix('"')?.chars();
    let mut out = String::new();
    loop {
        match chars.next()? {
            '"' => break,
            '\\' => match chars.next()? {
                escaped @ ('"' | '\\') => out.push(escaped),
                _ => return None,
            },
            other => out.push(other),
        }
    }
    let rest = chars.as_str().trim();
    (rest.is_empty() || rest.starts_with('#')).then_some(out)
}
