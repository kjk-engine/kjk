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
//!
//! [migrations]
//! snapshot = "old saves' fighters read no bolt model: the default one"
//! ```
//!
//! A format saves hold (`Format::stored`) bumped needs its `[migrations]`
//! line, how a save from before it is read, or the file a top-level
//! `breaking = "why"`: then saves from before it are refused.
//!
//! Only that much TOML is read: `key = "text"` lines, `#` comments and the
//! `[formats]` and `[migrations]` tables. No dependencies, as the sim's
//! build script reads it.

use std::path::{Path, PathBuf};

/// The folder of the pending bumps, under the repository's root.
pub const DIR: &str = "bumps";

/// The pinned bumps' named versions, under the repository's root.
pub const SINCE: &str = "crates/sim/src/since.rs";

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
    /// Saves hold it, so a bump says how old saves are read.
    pub stored: bool,
}

pub const FORMATS: &[Format] = &[
    Format {
        name: "snapshot",
        tag: "KJKS",
        file: "crates/sim/src/snapshot.rs",
        constant: "VERSION",
        stored: true,
    },
    Format {
        name: "save",
        tag: "KJKV",
        file: "crates/sim/src/save.rs",
        constant: "VERSION",
        stored: true,
    },
    Format {
        name: "replay",
        tag: "KJKR",
        file: "crates/sim/src/replay.rs",
        constant: "VERSION",
        stored: false,
    },
    Format {
        name: "pack",
        tag: "KJKT",
        file: "crates/sim/src/pack.rs",
        constant: "VERSION",
        stored: true,
    },
    Format {
        name: "gathered",
        tag: "KJKL",
        file: "crates/sim/src/pack.rs",
        constant: "GATHERED_VERSION",
        stored: false,
    },
    Format {
        name: "install",
        tag: "KJKA",
        file: "crates/sim/src/install.rs",
        constant: "VERSION",
        stored: false,
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
    /// How saves from before each stored format's bump are read.
    pub migrations: Vec<(&'static str, String)>,
    /// Why saves from before it are refused, instead.
    pub breaking: Option<String>,
}

impl Bump {
    /// Whether the pinned hash is left to the merge: the sim moves on
    /// purpose, or the snapshot's version (part of what is hashed) does.
    pub fn moves_hash(&self) -> bool {
        self.sim.is_some() || self.formats.iter().any(|(name, _)| *name == "snapshot")
    }

    /// The name its versions go by in `sim::since`: the file's stem in
    /// capitals, `-` and `.` as `_` (`saber-duel-fixes.toml`,
    /// `SABER_DUEL_FIXES`).
    pub fn name(&self) -> String {
        self.path
            .file_stem()
            .unwrap_or(self.path.as_os_str())
            .to_string_lossy()
            .to_uppercase()
            .replace(['-', '.'], "_")
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
            parse(path, &text)
        })
        .collect()
}

/// The bump file `name` holding `text`; an error names it.
pub fn parse_file(name: &str, text: &str) -> Result<Bump, String> {
    parse(PathBuf::from(name), text)
}

/// The version `format`'s constant names before the bumps pending:
/// the `N` of `const <constant>: u32 = N + crate::pending::<NAME>;` in its
/// file under `root`.
pub fn base(root: &Path, format: &Format) -> Result<u32, String> {
    let path = root.join(format.file);
    let source =
        std::fs::read_to_string(&path).map_err(|error| format!("{}: {error}", path.display()))?;
    source
        .lines()
        .find_map(|line| declared(line, format))
        .ok_or(format!(
            "{}: no `const {}: u32 = N + crate::pending::{};`",
            format.file,
            format.constant,
            format.name.to_uppercase()
        ))
}

/// The `N` of `line` if it declares `format`'s constant as
/// `[pub ]const <constant>: u32 = N + crate::pending::<NAME>;`.
pub fn declared(line: &str, format: &Format) -> Option<u32> {
    line.strip_prefix("pub ")
        .unwrap_or(line)
        .strip_prefix(&format!("const {}: u32 = ", format.constant))?
        .trim_end()
        .strip_suffix(&format!(
            " + crate::pending::{};",
            format.name.to_uppercase()
        ))?
        .parse()
        .ok()
}

/// `format`'s variant of `sim::Stored` (`Save`, `Snapshot`, `Pack`).
pub fn stored_variant(format: &Format) -> String {
    let mut name = format.name.to_string();
    name[..1].make_ascii_uppercase();
    name
}

/// An error naming the file of any of `bumps` whose name another has, or
/// a pinned bump in `since` (the source of `SINCE`) has: its versions would
/// be another's.
pub fn clashes(bumps: &[Bump], since: &str) -> Result<(), String> {
    for (at, bump) in bumps.iter().enumerate() {
        let name = bump.name();
        if since.contains(&format!(" {name}: ")) {
            return Err(format!(
                "{}: {name} is a pinned bump's name (`{SINCE}`): name the file anew",
                bump.path.display()
            ));
        }
        if let Some(other) = bumps[..at].iter().find(|other| other.name() == name) {
            return Err(format!(
                "{}: {name} is {}'s name too: name the file anew",
                bump.path.display(),
                other.path.display()
            ));
        }
    }
    Ok(())
}

/// The version from which `format`'s older saves are refused by a pending
/// `breaking` bump (its base and its place among `bumps`), if one is.
pub fn breaking_since(bumps: &[Bump], format: &str, base: u32) -> Option<u32> {
    let mut place = 0;
    let mut since = None;
    for bump in bumps {
        if bump.formats.iter().any(|(name, _)| *name == format) {
            place += 1;
            let migrated = bump.migrations.iter().any(|(name, _)| *name == format);
            if bump.breaking.is_some() && !migrated {
                since = Some(base + place);
            }
        }
    }
    since
}

/// Each of `bumps` that bumps `format`, by its name, with its place among
/// them from 1: its version is the format's base and that.
pub fn pending_since(bumps: &[Bump], format: &str) -> Vec<(String, u32)> {
    bumps
        .iter()
        .filter(|bump| bump.formats.iter().any(|(name, _)| *name == format))
        .enumerate()
        .map(|(at, bump)| (bump.name(), at as u32 + 1))
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

#[derive(Clone, Copy, PartialEq)]
enum Table {
    Top,
    Formats,
    Migrations,
}

fn parse(path: PathBuf, text: &str) -> Result<Bump, String> {
    let named = |what: String| format!("{}: {what}", path.display());
    let mut bump = Bump {
        path: path.clone(),
        sim: None,
        formats: Vec::new(),
        migrations: Vec::new(),
        breaking: None,
    };
    let mut table = Table::Top;
    for (index, line) in text.lines().enumerate() {
        let line = line.trim();
        let at = |what: String| named(format!("line {}: {what}", index + 1));
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if line.starts_with('[') {
            table = match line {
                "[formats]" => Table::Formats,
                "[migrations]" => Table::Migrations,
                _ => {
                    return Err(at(format!(
                        "unknown table {line}: only [formats] and [migrations]"
                    )));
                }
            };
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
        let known = || {
            format(key).ok_or_else(|| {
                let names: Vec<_> = FORMATS.iter().map(|format| format.name).collect();
                at(format!("unknown format {key}: one of {}", names.join(", ")))
            })
        };
        match table {
            Table::Top => match key {
                "sim" => bump.sim = Some(value),
                "breaking" => bump.breaking = Some(value),
                _ => {
                    return Err(at(format!(
                        "unknown key {key}: `sim`, `breaking`, or a format under [formats]"
                    )));
                }
            },
            Table::Formats | Table::Migrations => {
                let known = known()?;
                let list = if table == Table::Formats {
                    &mut bump.formats
                } else {
                    if !known.stored {
                        return Err(at(format!(
                            "{key} is not stored in saves: it needs no migration"
                        )));
                    }
                    &mut bump.migrations
                };
                if list.iter().any(|(name, _)| *name == known.name) {
                    return Err(at(format!("{key} twice")));
                }
                list.push((known.name, value));
            }
        }
    }
    let name = bump.name();
    let valid = name.starts_with(|first: char| first.is_ascii_uppercase())
        && name
            .chars()
            .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == '_');
    if !valid {
        return Err(named(format!(
            "{name} is not a name its versions can go by: start the file's name with a letter, then letters, digits, `-` or `_`"
        )));
    }
    if bump.sim.is_none() && bump.formats.is_empty() {
        return Err(named(
            "bumps nothing: `sim = \"…\"` or a format under [formats]".into(),
        ));
    }
    for (name, _) in &bump.migrations {
        if !bump.formats.iter().any(|(bumped, _)| bumped == name) {
            return Err(named(format!(
                "[migrations] {name}: {name} is not bumped under [formats]"
            )));
        }
    }
    if bump.breaking.is_none() {
        for (name, _) in &bump.formats {
            let stored = format(name).is_some_and(|format| format.stored);
            if stored && !bump.migrations.iter().any(|(migrated, _)| migrated == name) {
                return Err(named(format!(
                    "{name}: say how old saves are read under [migrations], or why they can't (`breaking = \"…\"`)"
                )));
            }
        }
    }
    Ok(bump)
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
