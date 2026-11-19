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
pub mod editor;
pub mod git;
pub mod project;

use std::path::PathBuf;

pub use area::{CollisionBox, KjkArea, Prop};
pub use editor::{Edit, Editor, Target};
pub use git::{Entry, Kind};
pub use project::{Problem, Project};

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
