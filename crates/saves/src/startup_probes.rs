//! The probes of what a start reads from the saves' folder (`kjk sim
//! probes`, group "startup files"): issue 279, a player whose game no
//! longer started after a quit to the desktop.

use std::path::Path;

use sim::probe::{Checks, check};

use crate::safety::backup_on_new_version;
use crate::{Meta, PLAYED, Played, list, read_played, write_encoded, write_played};

/// In a folder of its own under the system's temporary one:
/// - a note of what was played that was cut short reads as nothing, and is
///   kept aside as `played.json.bad`; the next start reads nothing and the
///   next write is read back;
/// - a note with a value of the wrong type (`"game": false`) is kept aside
///   the same way, as `played.json.bad-2` when the first name is taken;
/// - the note and the release stamp are written without leaving a
///   temporary file behind;
/// - a folder holding a cut meta, a cut story, a damaged release stamp and
///   a note kept aside still lists its good save, names the bad one, and
///   takes its backup for a new release.
pub fn probes() -> Checks {
    static RUNS: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let run = RUNS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let root = std::env::temp_dir().join(format!("kjk-startup-{}-{run}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let at = |name: &str| root.join(name);
    let checks = vec![
        ("played_cut_short_kept_aside", cut_short(&at("cut"))),
        ("played_wrong_type_kept_aside", wrong_type(&at("typed"))),
        (
            "played_and_stamp_written_whole",
            written_whole(&at("whole")),
        ),
        ("bad_files_do_not_stop_a_start", bad_files(&at("bad"))),
    ];
    let _ = std::fs::remove_dir_all(&root);
    checks
}

fn io(error: std::io::Error) -> String {
    error.to_string()
}

fn played() -> Played {
    Played {
        game: "k1".into(),
        module: "end_m01aa".into(),
        saved: 1_794_317_848,
        won: None,
    }
}

/// The names in `folder`, sorted.
fn names(folder: &Path) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(folder)
        .into_iter()
        .flatten()
        .flatten()
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    names
}

fn cut_short(root: &Path) -> Result<String, String> {
    write_played(root, &played())?;
    let path = root.join(PLAYED);
    let whole = std::fs::read(&path).map_err(io)?;
    std::fs::write(&path, &whole[..whole.len() / 2]).map_err(io)?;
    let first = read_played(root);
    let kept = std::fs::read(root.join("played.json.bad")).map_err(io)?;
    let second = read_played(root);
    write_played(root, &played())?;
    let third = read_played(root);
    check(
        first
            .as_ref()
            .is_err_and(|why| why.contains("played.json.bad"))
            && kept == whole[..whole.len() / 2]
            && second == Ok(None)
            && third == Ok(Some(played())),
        format!("{first:?}, then {second:?}, then {third:?}"),
    )
}

fn wrong_type(root: &Path) -> Result<String, String> {
    std::fs::create_dir_all(root).map_err(io)?;
    let path = root.join(PLAYED);
    std::fs::write(&path, r#"{"game": false}"#).map_err(io)?;
    let first = read_played(root);
    std::fs::write(&path, r#"{"game": false}"#).map_err(io)?;
    let second = read_played(root);
    let left = names(root);
    check(
        first
            .as_ref()
            .is_err_and(|why| why.contains("boolean `false`"))
            && second
                .as_ref()
                .is_err_and(|why| why.contains("played.json.bad-2"))
            && left == ["played.json.bad", "played.json.bad-2"],
        format!("{first:?}, then {second:?}, left {left:?}"),
    )
}

fn written_whole(root: &Path) -> Result<String, String> {
    write_played(root, &played())?;
    write_played(root, &played())?;
    backup_on_new_version(root, &root.join("backups"), "v0.1.0", "ab12cd34ef")?;
    let left = names(root);
    check(
        left == [".build", "played.json"] && read_played(root) == Ok(Some(played())),
        format!("left {left:?}"),
    )
}

fn bad_files(at: &Path) -> Result<String, String> {
    let root = at.join("saves");
    let meta = Meta {
        name: "kept".into(),
        module: "end_m01aa".into(),
        saved: 1_794_317_848,
        seats: 1,
        ..Meta::default()
    };
    let good = write_encoded(&root, None, b"world", &meta, None)?;
    let bad = write_encoded(&root, None, b"world", &meta, None)?;
    let cut = |path: &Path| -> Result<(), String> {
        let bytes = std::fs::read(path).map_err(io)?;
        std::fs::write(path, &bytes[..bytes.len() / 3]).map_err(io)
    };
    cut(&root.join(&bad).join(crate::META))?;
    std::fs::write(root.join(&good).join(crate::STORY), b"{\"game\": ").map_err(io)?;
    std::fs::write(root.join(crate::safety::STAMP), [0xff, 0xfe, 0x00]).map_err(io)?;
    std::fs::write(root.join("played.json.bad"), b"{").map_err(io)?;
    let listing = list(&root);
    let listed: Vec<&str> = listing.saves.iter().map(|(id, _)| id.as_str()).collect();
    let story = crate::read_story(&root, &good);
    let backup = backup_on_new_version(&root, &at.join("save-backups"), "v0.2.0", "ab12cd34ef");
    check(
        listed == [good.as_str()]
            && listing.unread.len() == 1
            && listing.unread[0].contains(&bad)
            && story.is_none()
            && read_played(&root) == Ok(None)
            && backup.as_ref().is_ok_and(Option::is_some),
        format!(
            "listed {listed:?}, unread {:?}, story {}, backup {backup:?}",
            listing.unread,
            story.is_some()
        ),
    )
}
