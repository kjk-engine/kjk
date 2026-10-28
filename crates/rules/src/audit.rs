//! What keeps the book whole (the approved test `rulebook`, `cargo xtask
//! rulebook`): every rule read by the code, and no number the sim plays by
//! kept outside the book. A literal constant in `crates/sim` is either a
//! rule's alias or listed in `data/rules/not-rules.txt` with why it is no
//! rule (a table's row, a bit flag, a format's version).

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use crate::{BOOK, Kind};

/// Where the sim's sources are, below the workspace root.
pub const SIM: &str = "crates/sim/src";
/// The constants that are no rules, below the workspace root.
pub const NOT_RULES: &str = "data/rules/not-rules.txt";

/// A literal constant: its file (from the workspace root) and its name.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct Literal {
    pub file: String,
    pub name: String,
}

impl Literal {
    /// Its line in `not-rules.txt` (without the why).
    pub fn key(&self) -> String {
        format!("{} {}", self.file, self.name)
    }
}

/// Every `.rs` file under `dir`, sorted.
fn sources(dir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
            } else if path.extension().is_some_and(|ext| ext == "rs") {
                out.push(path);
            }
        }
    }
    out.sort();
    out
}

/// A probe's file: checks, not the game, never holding a rule.
fn is_probe(file: &str) -> bool {
    file.contains("probes")
}

/// The constants `crates/sim` writes as a literal number (or an array or
/// tuple of them), its probes left out.
pub fn literal_constants(root: &Path) -> Vec<Literal> {
    let mut out = Vec::new();
    for path in sources(&root.join(SIM)) {
        let file = path
            .strip_prefix(root)
            .unwrap_or(&path)
            .to_string_lossy()
            .replace('\\', "/");
        if is_probe(&file) {
            continue;
        }
        let Ok(text) = std::fs::read_to_string(&path) else {
            continue;
        };
        let lines: Vec<&str> = text.lines().collect();
        for (at, line) in lines.iter().enumerate() {
            let Some((name, value)) = constant(line) else {
                continue;
            };
            // A value on the lines after the `=`.
            let value = match value.trim() {
                "" => lines[at + 1..]
                    .iter()
                    .map(|line| line.trim())
                    .find(|line| !line.is_empty())
                    .unwrap_or(""),
                value => value,
            };
            if value.starts_with(|c: char| c.is_ascii_digit() || "-[(".contains(c)) {
                out.push(Literal {
                    file: file.clone(),
                    name: name.to_string(),
                });
            }
        }
    }
    out.sort();
    out.dedup();
    out
}

/// A line's constant: its name and what follows its `=`.
fn constant(line: &str) -> Option<(&str, &str)> {
    let mut rest = line.trim_start();
    for visibility in ["pub(crate) ", "pub(super) ", "pub(in crate) ", "pub "] {
        if let Some(after) = rest.strip_prefix(visibility) {
            rest = after;
            break;
        }
    }
    let rest = rest.strip_prefix("const ")?;
    let (name, rest) = rest.split_once(':')?;
    let name = name.trim();
    let named = name.starts_with(|c: char| c.is_ascii_uppercase())
        && name
            .chars()
            .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == '_');
    if !named {
        return None;
    }
    let (_, value) = rest.split_once('=')?;
    Some((name, value))
}

/// `not-rules.txt`'s entries (`file NAME`, the why after a `#` dropped).
pub fn not_rules(root: &Path) -> BTreeSet<String> {
    std::fs::read_to_string(root.join(NOT_RULES))
        .unwrap_or_else(|_| String::new())
        .lines()
        .map(|line| line.split('#').next().unwrap_or("").trim())
        .filter(|line| !line.is_empty())
        .map(|line| line.split_whitespace().collect::<Vec<_>>().join(" "))
        .collect()
}

/// The rules no code reads: each must be named by its whole path
/// (`rules::combat::damage::PACE`), or by its module after a `use` of it
/// (`use rules::combat::damage;` … `damage::PACE`), in code (comments
/// aside) of the workspace's crates other than this one.
pub fn unread_rules(root: &Path) -> Vec<&'static str> {
    let texts: Vec<String> = std::fs::read_dir(root.join("crates"))
        .into_iter()
        .flatten()
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.file_name().is_some_and(|name| name != "rules"))
        .flat_map(|crate_dir| sources(&crate_dir))
        .filter_map(|path| std::fs::read_to_string(path).ok())
        .map(|text| uncommented(&text))
        .collect();
    BOOK.iter()
        .filter_map(|entry| match entry.kind {
            Kind::Rule { constant, .. } => Some(constant),
            Kind::Section => None,
        })
        .filter(|constant| !texts.iter().any(|text| reads(text, constant)))
        .collect()
}

/// `text` without its `//` comments (a `//` in a string literal cuts its
/// line short too, which only ever hides a read, never invents one).
fn uncommented(text: &str) -> String {
    text.lines()
        .map(|line| line.split("//").next().unwrap_or(""))
        .collect::<Vec<_>>()
        .join("\n")
}

/// Whether `name` stands in `text` as a whole word (no identifier's letter
/// right after it: `grip::LIFT` is not `grip::LIFT_EVERY_MS`).
fn names(text: &str, name: &str) -> bool {
    text.match_indices(name).any(|(at, _)| {
        text[at + name.len()..]
            .chars()
            .next()
            .is_none_or(|next| !(next.is_alphanumeric() || next == '_'))
    })
}

/// Whether `text` names `constant` (`rules::a::b::NAME`) as `unread_rules`
/// asks.
fn reads(text: &str, constant: &str) -> bool {
    if names(text, constant) {
        return true;
    }
    let Some((module, name)) = constant.rsplit_once("::") else {
        return false;
    };
    let last = module.rsplit("::").next().unwrap_or(module);
    text.contains(&format!("use {module};")) && names(text, &format!("{last}::{name}"))
}

/// Everything wrong with the book against the code at `root`: rules no
/// code reads, literal constants neither rules nor listed, listed ones
/// gone. Empty when whole.
pub fn problems(root: &Path) -> Vec<String> {
    let mut out: Vec<String> = unread_rules(root)
        .into_iter()
        .map(|constant| format!("{constant}: no code reads this rule"))
        .collect();
    let listed = not_rules(root);
    let found = literal_constants(root);
    let keys: BTreeSet<String> = found.iter().map(Literal::key).collect();
    for literal in &found {
        if !listed.contains(&literal.key()) {
            out.push(format!(
                "{}: a number outside the rulebook (make it a rule in data/rules/, or list it in {NOT_RULES} with why it is none)",
                literal.key()
            ));
        }
    }
    for key in listed.difference(&keys) {
        out.push(format!(
            "{NOT_RULES}: `{key}` names no literal constant now"
        ));
    }
    out
}
