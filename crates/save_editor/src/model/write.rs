//! An edited save written back safely: never over a file kjk changed since
//! it was opened (unless forced), only once it reads back as it is and,
//! with the game's install, loads and steps; a backup of the save as it was
//! before the session's first write; and written as kjk writes saves
//! (`saves::write_encoded`: a fresh folder, then a rename).

use std::path::{Path, PathBuf};

use crate::model::session::Session;

/// Ticks an edited save is stepped before it is written.
pub const TICKS: u64 = 60;

/// What a write did.
#[derive(Debug, Clone, PartialEq)]
pub struct Saved {
    /// The save as it was before this session first wrote it.
    pub backup: PathBuf,
    pub checked: edit::Checked,
}

/// Save `id`'s world file as it is on disk.
pub fn world(root: &Path, id: &str) -> Option<Vec<u8>> {
    std::fs::read(root.join(id).join("save.kjkv")).ok()
}

/// A world file's stamp: its blake3, hex.
pub fn stamp(bytes: &[u8]) -> String {
    blake3::hash(bytes).to_hex().to_string()
}

/// What the save screen says of a save older than this kjk writes: once
/// written, it loads only in this kjk or newer.
pub fn older_note(session: &Session) -> Option<String> {
    (session.format < sim::SAVE_FORMAT).then(|| {
        format!(
            "this save was made by an older kjk (save format {}); once written it will only load in kjk {} or newer",
            session.format,
            crate::VERSION
        )
    })
}

/// `session`'s save written over save `opened.id` in `root`, checked with
/// `discovery`'s install when it has the save's game; why not, with nothing
/// written. Refused when kjk saved over the file since it was opened,
/// unless `force`.
pub fn save(
    session: &mut Session,
    root: &Path,
    discovery: Option<&vfs::Discovery>,
    force: bool,
) -> Result<Saved, String> {
    let install = discovery.filter(|discovery| {
        crate::model::locate::game_of(&session.opened)
            .is_some_and(|game| discovery.get(game).is_some())
    });
    save_checked(session, root, discovery, force, |save| match install {
        Some(discovery) => edit::load_and_step(save, discovery, TICKS),
        None => edit::round_trip(save),
    })
}

/// The same with the check `check` (`edit::load_and_step` or
/// `edit::round_trip`; another in the probes).
pub fn save_checked(
    session: &mut Session,
    root: &Path,
    _discovery: Option<&vfs::Discovery>,
    force: bool,
    check: impl FnOnce(&sim::Save) -> Result<edit::Checked, String>,
) -> Result<Saved, String> {
    let id = session.opened.id.clone();
    let now = world(root, &id).map(|bytes| stamp(&bytes));
    if !force && session.stamp.is_some() && now != session.stamp {
        return Err(
            "kjk saved over this save since it was opened: open it again, or save anyway".into(),
        );
    }
    let checked = check(&session.edited).map_err(|why| format!("not written: {why}"))?;
    let backup = match &session.backup {
        Some(backup) => backup.clone(),
        None => back_up(root, &id)?,
    };
    session.backup = Some(backup.clone());
    let story = match &session.level {
        Some(level) => Some(saves::Story::of(&session.edited, level)),
        None => saves::read_story(root, &id),
    };
    let meta = saves::Meta {
        saved: saves::now(),
        build: Some(env!("KJK_GIT_SHA").into()),
        version: Some(crate::VERSION.into()),
        formats: Some(saves::Formats {
            save: sim::SAVE_FORMAT,
            snapshot: sim::SNAPSHOT_FORMAT,
            story: saves::STORY_SCHEMA,
        }),
        ..session.opened.meta.clone()
    };
    let bytes = session.edited.encode();
    saves::write_encoded(root, Some(&id), &bytes, &meta, story.as_ref())?;
    // What is on disk now is the session's new start.
    session.opened.save = session.edited.clone();
    session.opened.meta = meta;
    session.changes.clear();
    session.stamp = Some(stamp(&bytes));
    session.format = sim::SAVE_FORMAT;
    Ok(Saved { backup, checked })
}

/// Save `id`'s folder copied to `<id>.backup-<YYYYMMDD-HHMMSS>` beside it
/// (a name kjk does not list); the copy.
fn back_up(root: &Path, id: &str) -> Result<PathBuf, String> {
    let now = saves::now();
    let (days, seconds) = (now / 86_400, now % 86_400);
    let (year, month, day) = saves::civil_from_days(days as i64);
    let base = format!(
        "{id}.backup-{year:04}{month:02}{day:02}-{:02}{:02}{:02}",
        seconds / 3600,
        seconds / 60 % 60,
        seconds % 60
    );
    let free = (0..100)
        .map(|n| {
            if n == 0 {
                base.clone()
            } else {
                format!("{base}-{n}")
            }
        })
        .map(|name| root.join(name))
        .find(|path| !path.exists())
        .ok_or(format!("no free backup name for save {id}"))?;
    copy(&root.join(id), &free).map_err(|error| format!("backing up save {id}: {error}"))?;
    Ok(free)
}

fn copy(from: &Path, to: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(to)?;
    for entry in std::fs::read_dir(from)? {
        let entry = entry?;
        let target = to.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy(&entry.path(), &target)?;
        } else {
            std::fs::copy(entry.path(), target)?;
        }
    }
    Ok(())
}
