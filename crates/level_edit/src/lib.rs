//! The level editor's data, with no window and no game install: a project
//! (a game, a module, the module's area as the user changed it, and the
//! content only kjk understands), the edits made to it with undo and redo,
//! and the checks made before it is saved. `docs/LEVEL-EDIT.md` describes
//! the editor that stands on it.
//!
//! A project is a folder of its own in kjk's folder (`projects_folder`).
//! It holds `project.toml` (the game, the module and the area), the area's
//! changed `.git` file, and `kjk-area.toml` (the Jedi Academy props and the
//! collision boxes, which no KOTOR file can hold). Nothing here writes into
//! a game install.
//!
//! The project a run plays (`kjk play --project NAME`) is opened once at
//! the start (`active::activate`). Its folder is then read over the
//! install's `override/` (`vfs::project`), and its props and boxes join the
//! area's collision (`active::added`).

pub mod active;
pub mod area;
pub mod blocks;
pub mod checks;
pub mod clip;
pub mod dialog;
pub mod editor;
pub mod effects;
pub mod fields;
pub mod git;
pub mod item_props;
pub mod items;
pub mod ja_rooms;
pub mod journal;
pub mod kinds;
pub mod lamps;
pub mod levels;
pub mod lighting;
pub mod lights;
pub mod links;
pub mod map;
pub mod mods;
pub mod nav;
pub mod project;
pub mod scripts;
pub mod settings;
pub mod template;

use std::path::PathBuf;

pub use area::{BoxRole, CollisionBox, KjkArea, Prop};
pub use editor::{Edit, Editor, Target};
pub use git::{Entry, Kind};
pub use lamps::PlacedLamp;
pub use lights::PlacedLight;
pub use map::{NewMap, Room};
pub use project::{Problem, Project};
pub use template::{Change, Template};

/// The folder that holds the projects, as `var` (the environment) gives it.
/// `KJK_PROJECTS` wins. Otherwise it is `projects` in kjk's own folder,
/// beside the saves (`store::kjk_folder`). `None` without a home folder to
/// tell it by.
pub fn projects_folder(var: impl Fn(&str) -> Option<String>) -> Option<PathBuf> {
    var("KJK_PROJECTS")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .or_else(|| store::kjk_folder(&var).map(|folder| folder.join("projects")))
}

/// Whether `name` can name a project: 1 to 64 characters, each a letter, a
/// digit, `-` or `_`. The name is a folder's name, so it holds no
/// separator and no dot.
pub fn valid_name(name: &str) -> bool {
    (1..=64).contains(&name.len())
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

/// What is wrong with `name` as the name of a level or an area that a
/// project or a mod keeps files of, if anything is. Such a name is 1 to 32
/// characters, each a lowercase letter, a digit, `_` or `-`. Every level
/// and area of both games is named so.
///
/// The name becomes a part of a path: `NAME.git` in the project's folder,
/// and a folder of its own for a level's bake. So it holds no separator, no
/// dot and no drive, and a path made with it stays in the folder it is
/// joined to. A project's file is read from disk and may be another
/// person's, so its names are checked before any is used
/// (`Project::open`), and again where a file or a folder is deleted.
pub fn file_name_problem(name: &str) -> Option<String> {
    let fits = (1..=32).contains(&name.len())
        && name
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_' || c == '-');
    (!fits).then(|| {
        format!("{name:?} cannot name a file: use 1 to 32 lowercase letters, digits, - and _")
    })
}

/// A text a person typed, as the bytes a game file keeps it in. The games
/// write text in Windows-1252, a byte a character
/// (`fmt_odyssey::CodePage::Western`), and their fonts draw a glyph for
/// each byte. A character that code page does not have is written as `?`.
///
/// Every text of a project that the games show goes through this and
/// `game_text`: a conversation's line, an item's name and description, a
/// journal entry, a template's name and a new map's title. They used to be
/// written three ways. A line and a name were written as UTF-8, which the
/// games show as two wrong letters for each accented one. An item's and a
/// journal's were written as Latin-1, which has no curly quote, long dash
/// or ellipsis.
///
/// A Russian release writes Windows-1251. The editor does not ask which
/// code page the install has, so Cyrillic letters are written as `?`.
pub fn game_bytes(text: &str) -> Vec<u8> {
    text.chars()
        .map(|character| (fmt_odyssey::CodePage::Western.byte(character)).unwrap_or(b'?'))
        .collect()
}

/// A game file's text (`game_bytes`) as the letters it stands for, to show
/// and to edit. Every byte reads as one character, and `game_bytes` writes
/// that character as the same byte again.
pub fn game_text(bytes: &[u8]) -> String {
    fmt_odyssey::CodePage::Western.readable(bytes)
}

/// `text` as a TOML string: in double quotes, with `"`, `\` and the
/// characters that print nothing escaped as TOML escapes them. Every text
/// kjk writes into one of its TOML files goes through this.
///
/// Rust's own quoting (`{:?}`) was used for it. That writes a non-breaking
/// space as `\u{a0}`, a zero-width space as `\u{200b}` and a zero byte as
/// `\0`, none of which TOML reads. A title pasted with such a character
/// was saved into a file that did not open again.
pub fn quoted(text: &str) -> String {
    let mut out = String::with_capacity(text.len() + 2);
    out.push('"');
    for c in text.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\t' => out.push_str("\\t"),
            '\r' => out.push_str("\\r"),
            '\u{8}' => out.push_str("\\b"),
            '\u{c}' => out.push_str("\\f"),
            c if c.is_control() => out.push_str(&format!("\\u{:04X}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// The folder of the project `name` (`projects_folder`), refused when
/// `name` cannot name a project.
pub fn folder_of(name: &str, var: impl Fn(&str) -> Option<String>) -> Result<PathBuf, String> {
    if !valid_name(name) {
        return Err(format!(
            "{name:?} cannot name a project: use 1 to 64 letters, digits, - and _"
        ));
    }
    projects_folder(var)
        .map(|projects| projects.join(name))
        .ok_or("no folder for projects: set KJK_PROJECTS or KJK_HOME".into())
}
