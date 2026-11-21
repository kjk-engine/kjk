//! The recovery copy: work that is not saved, kept beside the project so a
//! crash or a power cut does not lose it.
//!
//! While the project has changes that are not saved, the editor writes a
//! copy of it every `EVERY` to `<kjk's folder>/recovery/<project>`: the
//! project as a save would write it, the edited level's kjk-only content
//! whole (`FULL_KJK`, with an installed level's lights and lamps placed by
//! hand, which a save keeps in the light data), and each script changed in
//! the viewer. A save removes the copy, and so does leaving the level by
//! choice. A copy that is there when the editor opens the project was left
//! by an editor that did not close: the window offers to take it
//! (`take`), as one edit that undo takes back, or to discard it.

use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use crate::state::EditorState;

/// How often the copy is written while there are changes not saved.
const EVERY: Duration = Duration::from_secs(60);

/// The file of the edited level's whole kjk-only content.
const FULL_KJK: &str = "recovery-kjk-area.toml";

/// The file that names the level the copy was made of.
const LEVEL: &str = "recovery-level.txt";

/// When the copy was last looked at, and what it was last written of.
pub(crate) struct Autosave {
    looked: Instant,
    /// The editor's count of changes and the drafts' texts (hashed) the
    /// copy holds.
    written: Option<(u64, u64)>,
}

impl Default for Autosave {
    fn default() -> Autosave {
        Autosave {
            looked: Instant::now(),
            written: None,
        }
    }
}

/// Where the recovery copy of the project `name` is kept.
fn folder_of(name: &str) -> Option<PathBuf> {
    store::kjk_folder(|var| std::env::var(var).ok())
        .map(|folder| folder.join("recovery").join(name))
}

/// The recovery copy of the project `name` an editor left, if there is one.
pub(crate) fn found(name: &str) -> Option<PathBuf> {
    folder_of(name).filter(|folder| level_edit::Project::exists(folder))
}

/// The drafts' texts as one number, to tell when one changed.
fn drafts_key(state: &EditorState) -> u64 {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    for (name, draft) in &state.drafts {
        if draft.changed() {
            name.hash(&mut hasher);
            draft.text.hash(&mut hasher);
        }
    }
    hasher.finish()
}

/// Writes the copy when it is due: every `EVERY`, while there are changes
/// not saved that the copy does not hold yet. A copy an editor left, not
/// taken or discarded yet, is not written over.
pub(crate) fn tick(state: &mut EditorState) {
    if state.autosave.looked.elapsed() < EVERY {
        return;
    }
    state.autosave.looked = Instant::now();
    if state.dragging || state.recovery.is_some() || !state.unsaved() {
        return;
    }
    let key = (state.editor.changes(), drafts_key(state));
    if state.autosave.written == Some(key) {
        return;
    }
    match write(state) {
        Ok(folder) => {
            state.autosave.written = Some(key);
            bevy::log::info!(target: "editor", "recovery copy written to {}", folder.display());
        }
        Err(why) => {
            bevy::log::warn!(target: "editor", "the recovery copy was not written: {why}");
        }
    }
}

/// Writes the copy now.
pub(crate) fn write(state: &EditorState) -> Result<PathBuf, String> {
    let project = &state.editor.project;
    let folder = folder_of(&project.name).ok_or("kjk's folder is not known")?;
    // What an earlier copy held and this one does not (a script no longer
    // changed) goes first.
    let _ = std::fs::remove_dir_all(&folder);
    let mut copy = project.clone();
    copy.folder = folder.clone();
    copy.save(&format!("kjk {} (recovery copy)", kjk_game::GIT_SHA))?;
    let file = |name: &str, bytes: &[u8]| {
        let path = folder.join(name);
        std::fs::write(&path, bytes).map_err(|error| format!("{}: {error}", path.display()))
    };
    file(FULL_KJK, project.kjk.write().as_bytes())?;
    file(LEVEL, project.module.as_bytes())?;
    for (name, draft) in &state.drafts {
        if draft.changed() {
            let mut held = draft.clone();
            crate::script_edit::write(&folder, name, &mut held)?;
        }
    }
    Ok(folder)
}

/// Removes this editor's copy: after a save, or when the level is left by
/// choice. A copy another editor left, not taken or discarded yet, stays.
pub(crate) fn clear(state: &mut EditorState) {
    if state.recovery.is_some() {
        return;
    }
    if let Some(folder) = folder_of(&state.editor.project.name) {
        let _ = std::fs::remove_dir_all(folder);
    }
    state.autosave.written = None;
}

/// What the copy an editor left is of, for the window to say: the level,
/// and how long ago it was written.
pub(crate) fn describe(folder: &Path) -> String {
    let level = std::fs::read_to_string(folder.join(LEVEL)).unwrap_or_else(|_| String::new());
    let age = std::fs::metadata(folder.join(FULL_KJK))
        .and_then(|meta| meta.modified())
        .ok()
        .and_then(|written| written.elapsed().ok())
        .map(|age| match age.as_secs() {
            seconds if seconds < 120 => format!("{seconds} seconds ago"),
            seconds if seconds < 7200 => format!("{} minutes ago", seconds / 60),
            seconds if seconds < 172_800 => format!("{} hours ago", seconds / 3600),
            seconds => format!("{} days ago", seconds / 86_400),
        });
    match (level.trim(), age) {
        ("", Some(age)) => format!("written {age}"),
        (level, Some(age)) => format!("of the level {level}, written {age}"),
        (level, None) => format!("of the level {level}"),
    }
}

/// Takes the copy an editor left into the project being edited, as one
/// edit: the level's objects, kjk-only content, templates, conversations
/// and area settings, and the scripts that were being written. The
/// project's folder is not touched until the next save.
pub(crate) fn take(state: &mut EditorState) -> Result<(), String> {
    let folder = state
        .recovery
        .clone()
        .ok_or("there is no recovery copy to take")?;
    let mut copy = level_edit::Project::open(&folder)?;
    let module = state.editor.project.module.clone();
    copy.switch_to(&module, || {
        Err(format!("the recovery copy has no level {module}"))
    })?;
    // The whole kjk-only content, when the copy was made of this level.
    let made_of = std::fs::read_to_string(folder.join(LEVEL)).unwrap_or_else(|_| String::new());
    if made_of.trim() == module
        && let Ok(text) = std::fs::read_to_string(folder.join(FULL_KJK))
    {
        copy.kjk = level_edit::KjkArea::parse(&text)?;
    }
    state.editor.replace_content(copy);
    state.quiet = 0;
    // The scripts that were being written: each `NAME.nss` of the copy.
    let mut scripts = 0;
    for entry in std::fs::read_dir(&folder).into_iter().flatten().flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("nss") {
            continue;
        }
        let Some(name) = path.file_stem().and_then(|stem| stem.to_str()) else {
            continue;
        };
        let Ok(bytes) = std::fs::read(&path) else {
            continue;
        };
        let text: String = bytes.into_iter().map(char::from).collect();
        let saved = crate::state::script_text_on_disk(state, name).unwrap_or_else(String::new);
        state.drafts.insert(
            name.to_ascii_lowercase(),
            crate::script_edit::Draft { text, saved },
        );
        scripts += 1;
    }
    state.recovery = None;
    let _ = std::fs::remove_dir_all(&folder);
    state.say(format!(
        "took the recovery copy, with {scripts} scripts that were being written; undo gives the saved level back"
    ));
    Ok(())
}

/// Discards the copy an editor left.
pub(crate) fn discard(state: &mut EditorState) -> Result<(), String> {
    let folder = state
        .recovery
        .take()
        .ok_or("there is no recovery copy to discard")?;
    std::fs::remove_dir_all(&folder).map_err(|error| format!("{}: {error}", folder.display()))?;
    state.say("discarded the recovery copy");
    Ok(())
}
