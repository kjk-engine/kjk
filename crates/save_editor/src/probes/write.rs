//! Saving safely (Task 7, Review Focus 3): checked first, a backup before
//! the first write, nothing written when a check fails or kjk saved over
//! the file meanwhile.

use std::path::Path;

use super::locate::{scratch, sink_save};
use crate::model::session::{Change, Ground, Session};
use crate::model::write;

fn credits(root: &Path, id: &str) -> Result<i32, String> {
    Ok(edit::open(root, id)
        .map_err(|why| why.to_string())?
        .save
        .script
        .gold)
}

/// The kitchen sink saved in a fresh folder, opened in a session with its
/// credits changed.
fn edited(name: &str) -> Result<(std::path::PathBuf, String, Session), String> {
    let root = scratch(name);
    let id = sink_save(&root, "edited")?;
    let mut session = Session::open(&root, &id).map_err(|why| why.to_string())?;
    let items = sim::inventory::Catalog::new([]);
    session.apply(Change::Credits(777), &Ground::items(&items))?;
    Ok((root, id, session))
}

/// The backups beside the saves.
fn backups(root: &Path) -> Vec<String> {
    std::fs::read_dir(root)
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|entry| entry.file_name().to_str().map(str::to_string))
        .filter(|name| name.contains(".backup-"))
        .collect()
}

/// A good save: a backup of the save as it was, then the edited one
/// written, reading back as edited; with no install, checked by its round
/// trip alone; written twice, one backup.
pub fn good() -> Result<String, String> {
    let (root, id, mut session) = edited("write-good")?;
    let before = credits(&root, &id)?;
    let saved = write::save(&mut session, &root, None, false)?;
    let after = credits(&root, &id)?;
    // kjk lists no backup (its name is no save id): read its world itself.
    let backup = std::fs::read(saved.backup.join("save.kjkv"))
        .map_err(|error| error.to_string())
        .and_then(|bytes| edit::open_bytes(&bytes).map_err(|why| why.to_string()))
        .map(|save| save.script.gold);
    let listed = crate::model::locate::list(&root).len();
    let items = sim::inventory::Catalog::new([]);
    session.apply(Change::Credits(778), &Ground::items(&items))?;
    write::save(&mut session, &root, None, false)?;
    let again = credits(&root, &id)?;
    let kept = backups(&root);
    let _ = std::fs::remove_dir_all(&root);
    if after != 777 || again != 778 || saved.checked != edit::Checked::RoundTrip {
        return Err(format!(
            "written {after} then {again}, checked {:?}",
            saved.checked
        ));
    }
    if backup != Ok(before) || kept.len() != 1 || listed != 1 {
        return Err(format!(
            "backup holds {backup:?} (was {before}); backups {kept:?}; {listed} saves listed"
        ));
    }
    Ok(format!(
        "{before} → 777 → 778; one backup {kept:?}; {:?}",
        saved.checked
    ))
}

/// A check that fails writes nothing and backs nothing up.
pub fn failed_check() -> Result<String, String> {
    let (root, id, mut session) = edited("write-failed")?;
    let result = write::save_checked(&mut session, &root, None, false, |_| Err("injected".into()));
    let left = (credits(&root, &id)?, backups(&root));
    let _ = std::fs::remove_dir_all(&root);
    match result {
        Err(why) if why.contains("injected") && left.0 != 777 && left.1.is_empty() => {
            Ok(format!("refused: {why}"))
        }
        other => Err(format!(
            "{:?}; left {left:?}",
            other.map(|saved| saved.backup)
        )),
    }
}

/// kjk saved over the save after it was opened: refused, nothing written;
/// forced, written.
pub fn saved_over() -> Result<String, String> {
    let (root, id, mut session) = edited("write-over")?;
    let mut theirs = edit::open(&root, &id).map_err(|why| why.to_string())?;
    theirs.save.script.gold = 31;
    saves::write(&root, Some(&id), &theirs.save, &theirs.meta)?;
    let refused = write::save(&mut session, &root, None, false);
    let kept = credits(&root, &id)?;
    let forced = write::save(&mut session, &root, None, true).map(|_| ());
    let after = credits(&root, &id)?;
    let _ = std::fs::remove_dir_all(&root);
    match refused {
        Err(why)
            if why.contains("kjk saved over this save since it was opened")
                && kept == 31
                && forced.is_ok()
                && after == 777 =>
        {
            Ok(format!("refused: {why}; forced: written"))
        }
        other => Err(format!(
            "{other:?}; kept {kept}; forced {forced:?} → {after}"
        )),
    }
}

/// A save older than this kjk writes is told it will load only in this kjk
/// or newer; a current one is not.
pub fn older_note() -> Result<String, String> {
    let (root, _, mut session) = edited("write-older")?;
    let _ = std::fs::remove_dir_all(&root);
    let current = write::older_note(&session);
    session.format = sim::SAVE_FORMAT - 1;
    let older = write::older_note(&session).ok_or("an older save is not told")?;
    if current.is_some() || !older.contains(crate::VERSION) {
        return Err(format!("current: {current:?}; older: {older}"));
    }
    Ok(older)
}

/// With the save's game installed, the write is checked by loading the
/// save in its module: the kitchen sink (a K1 save by its meta, of no
/// module the install has) is refused, nothing written.
pub fn loaded_k1() -> Result<String, String> {
    let discovery = crate::model::locate::discovery();
    if discovery.get(vfs::Game::Kotor1).is_none() {
        return Ok("skipped: no k1 install".into());
    }
    let (root, id, mut session) = edited("write-loaded")?;
    let result = write::save(&mut session, &root, Some(&discovery), false);
    let left = (credits(&root, &id)?, backups(&root));
    let _ = std::fs::remove_dir_all(&root);
    match result {
        Err(why) if left.0 != 777 && left.1.is_empty() => Ok(format!("loaded, refused: {why}")),
        other => Err(format!(
            "{:?}; left {left:?}",
            other.map(|saved| saved.checked)
        )),
    }
}
