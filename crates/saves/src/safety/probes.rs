//! The probes of the saves' safety net (`kjk sim probes`, group "save
//! safety").

use super::*;
use crate::{Meta, list, read, write_encoded};
use sim::probe::{Checks, check};

/// In a folder of its own under the system's temporary one:
/// - the meta's new fields round-trip, and an older meta without them reads;
/// - a save whose world is not what its meta's hash says is listed as
///   damaged and does not load; one without a hash is trusted;
/// - autosaves rotate through their three slots;
/// - the backup on a new release: none without saves or for the same
///   release, a whole copy for a new one, five kept;
/// - a write that fails leaves no half-written save;
/// - a save that does not decode names the build that made it.
pub fn probes() -> Checks {
    let root = std::env::temp_dir().join(format!("kjk-save-safety-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let at = |name: &str| root.join(name);
    let mut checks = vec![
        ("meta_new_fields_round_trip", new_fields()),
        ("meta_old_fields_read", old_fields()),
        ("save_damaged_listed", damaged(&at("damaged"))),
        ("save_without_hash_trusted", without_hash(&at("unhashed"))),
        ("autos_rotate", autos(&at("autos"))),
        ("failed_write_leaves_nothing", failed_write(&at("failed"))),
        ("refusal_names_build", names_build(&at("refused"))),
    ];
    checks.extend(backups(&at("backups")));
    let _ = std::fs::remove_dir_all(&root);
    checks
}

fn meta(name: &str) -> Meta {
    Meta {
        name: name.into(),
        module: "end_m01aa".into(),
        area: "Command Module".into(),
        saved: crate::now(),
        seats: 1,
        ..Meta::default()
    }
}

fn stamped(name: &str) -> Meta {
    Meta {
        build: Some("ab12cd34ef".into()),
        version: Some("v0.1.0".into()),
        game: Some("k1".into()),
        formats: Some(Formats {
            save: 59,
            snapshot: 72,
        }),
        ..meta(name)
    }
}

fn io(error: std::io::Error) -> String {
    error.to_string()
}

fn new_fields() -> Result<String, String> {
    let meta = Meta {
        blake3: Some("00ff".into()),
        ..stamped("fields")
    };
    let text = serde_json::to_string(&meta).map_err(|error| error.to_string())?;
    let back: Meta = serde_json::from_str(&text).map_err(|error| error.to_string())?;
    check(back == meta, text)
}

fn old_fields() -> Result<String, String> {
    let text = r#"{"name":"old","module":"end_m01aa","area":"Command Module","saved":1,"seats":1}"#;
    let (meta, _) = crate::parse_meta(text)?;
    check(
        meta.blake3.is_none() && meta.build.is_none() && meta.formats.is_none(),
        format!("{meta:?}"),
    )
}

fn damaged(root: &Path) -> Result<String, String> {
    let id = write_encoded(root, None, b"KJKV world", &meta("damaged"))?;
    let world = root.join(&id).join(crate::WORLD);
    let mut bytes = std::fs::read(&world).map_err(io)?;
    bytes[0] ^= 1;
    std::fs::write(&world, bytes).map_err(io)?;
    let listing = list(root);
    let refused = read(root, &id).err().unwrap_or_else(String::new);
    check(
        listing.damaged == [id.clone()] && refused.contains("damaged"),
        format!("damaged {:?}, read: {refused}", listing.damaged),
    )
}

fn without_hash(root: &Path) -> Result<String, String> {
    let id = write_encoded(root, None, b"KJKV world", &meta("unhashed"))?;
    let path = root.join(&id).join(crate::META);
    let text = std::fs::read_to_string(&path).map_err(io)?;
    let mut json: serde_json::Value =
        serde_json::from_str(&text).map_err(|error| error.to_string())?;
    json.as_object_mut().map(|fields| fields.remove("blake3"));
    std::fs::write(&path, json.to_string()).map_err(io)?;
    let listing = list(root);
    check(
        listing.damaged.is_empty() && listing.saves.len() == 1,
        format!("damaged {:?}", listing.damaged),
    )
}

fn autos(root: &Path) -> Result<String, String> {
    for n in 1..=4 {
        write_auto(
            root,
            format!("world {n}").as_bytes(),
            &meta(&format!("auto {n}")),
        )?;
    }
    let names: Vec<String> = AUTOS
        .iter()
        .map(|id| crate::read_meta(&root.join(id)).map(|meta| meta.name))
        .collect::<Result<_, _>>()?;
    check(
        names == ["auto 4", "auto 3", "auto 2"],
        format!("{names:?}"),
    )
}

fn failed_write(root: &Path) -> Result<String, String> {
    // A file where the saves' folder should be: nothing can be written in it.
    std::fs::create_dir_all(root.parent().unwrap_or(root)).map_err(io)?;
    std::fs::write(root, b"not a folder").map_err(io)?;
    let written = write_encoded(root, Some("0001"), b"world", &meta("failed"));
    let left = std::fs::metadata(root).is_ok_and(|meta| meta.is_file());
    let _ = std::fs::remove_file(root);
    check(written.is_err() && left, format!("{written:?}"))
}

fn names_build(root: &Path) -> Result<String, String> {
    let id = write_encoded(root, None, b"KJKV not a world", &stamped("refused"))?;
    let refused = read(root, &id).err().unwrap_or_else(String::new);
    check(
        refused.contains("made with v0.1.0 (ab12cd34)") && refused.contains(BACKUPS),
        refused,
    )
}

/// The backup's cases, in order on one folder.
fn backups(at: &Path) -> Checks {
    let (root, kept) = (at.join("saves"), at.join("save-backups"));
    let backup = |version: &str| backup_on_new_version(&root, &kept, version, "ab12cd34ef");
    let updates = || {
        std::fs::read_dir(&kept).map_or(0, |entries| {
            entries
                .flatten()
                .filter(|entry| entry.file_name().to_string_lossy().starts_with(UPDATE))
                .count()
        })
    };
    let first = backup("v0.1.0").map(|made| {
        check(
            made.is_none() && root.join(STAMP).exists() && updates() == 0,
            format!("{made:?}, {} backups", updates()),
        )
    });
    let same = backup("v0.1.0").map(|made| check(made.is_none(), format!("{made:?}")));
    let new = write_encoded(&root, Some(crate::QUICK), b"world", &meta("kept"))
        .and_then(|_| backup("v0.2.0"))
        .map(|made| {
            let copied = made
                .as_ref()
                .is_some_and(|made| made.join(crate::QUICK).join(crate::WORLD).exists());
            let named = made.as_ref().is_some_and(|made| {
                made.file_name().is_some_and(|name| {
                    name.to_string_lossy()
                        .starts_with("update-v0.1.0-ab12cd34-")
                })
            });
            check(copied && named, format!("{made:?}"))
        });
    let five = (3..=9)
        .try_for_each(|minor| backup(&format!("v0.{minor}.0")).map(|_| ()))
        .map(|()| check(updates() == KEPT_BACKUPS, format!("{} backups", updates())));
    [
        ("backup_first_none", first),
        ("backup_same_none", same),
        ("backup_new_copies", new),
        ("backup_keeps_five", five),
    ]
    .into_iter()
    .map(|(name, result)| (name, result.and_then(|checked| checked)))
    .collect()
}
