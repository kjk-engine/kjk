//! Saves and installs found, and a picked folder told in words (Task 2,
//! Review Focus 1 and 2).

use std::path::{Path, PathBuf};

use crate::model::locate::{self, Found};

/// A fresh folder for one probe.
pub(super) fn scratch(name: &str) -> PathBuf {
    let root = std::env::temp_dir().join(format!("kjk-save-editor-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let _ = std::fs::create_dir_all(&root);
    root
}

/// The kitchen sink written into `root` as a K1 save named `name`; its id.
pub(super) fn sink_save(root: &Path, name: &str) -> Result<String, String> {
    let meta = saves::Meta {
        name: name.into(),
        module: "end_m01aa".into(),
        game: Some("k1".into()),
        saved: 1_762_250_000,
        ..saves::Meta::default()
    };
    saves::write(root, None, &sim::fixtures::kitchen_sink().1, &meta)
}

fn io(error: std::io::Error) -> String {
    error.to_string()
}

/// An empty folder is no saves folder; two good saves and a cut one are a
/// saves folder of three, the cut one listed with its reason (Review Focus
/// 2); one made by a newer kjk is refused naming both formats.
pub fn saves_folder() -> Result<String, String> {
    let root = scratch("locate");
    let empty = locate::check_path(&root);
    let Found::Nothing(why) = &empty else {
        return Err(format!("an empty folder: {empty}"));
    };
    if !empty.to_string().contains("no save.kjkv inside") {
        return Err(format!("an empty folder says {why:?}"));
    }
    sink_save(&root, "first")?;
    sink_save(&root, "second")?;
    let cut = sink_save(&root, "cut")?;
    let world = root.join(&cut).join("save.kjkv");
    let bytes = std::fs::read(&world).map_err(io)?;
    std::fs::write(&world, &bytes[..bytes.len() / 2]).map_err(io)?;
    let found = locate::check_path(&root);
    if found
        != (Found::Saves {
            count: 3,
            unreadable: 1,
        })
    {
        return Err(format!("three saves, one cut: {found}"));
    }
    let rows = locate::list(&root);
    let damaged = rows
        .iter()
        .find(|row| row.id == cut)
        .ok_or("the cut save is not listed")?;
    if !matches!(damaged.readable, Err(edit::Refusal::Damaged(_))) || damaged.name != "cut" {
        return Err(format!(
            "the cut save: {:?} {:?}",
            damaged.name, damaged.readable
        ));
    }
    let good = rows.iter().filter(|row| row.readable.is_ok()).count();
    let newer = {
        let mut bytes = sim::fixtures::kitchen_sink().1.encode();
        bytes[4..8].copy_from_slice(&(sim::SAVE_FORMAT + 2).to_le_bytes());
        saves::write_encoded(&root, None, &bytes, &saves::Meta::default(), None)?
    };
    let rows = locate::list(&root);
    let told = rows
        .iter()
        .find(|row| row.id == newer)
        .and_then(|row| row.readable.as_ref().err())
        .map(ToString::to_string)
        .ok_or("the newer save is not refused")?;
    let _ = std::fs::remove_dir_all(&root);
    let both = told.contains(&(sim::SAVE_FORMAT + 2).to_string())
        && told.contains(&sim::SAVE_FORMAT.to_string());
    if good != 2 || !both || rows.len() != 4 {
        return Err(format!("{good} good of {}; newer: {told}", rows.len()));
    }
    Ok(format!(
        "{found}; {damaged_id}: cut; newer: {told}",
        damaged_id = cut
    ))
}

/// A folder laid out as a K1 install, its override holding two items.
fn fake_install(root: &Path) -> Result<(), String> {
    for folder in ["modules", "override"] {
        std::fs::create_dir_all(root.join(folder)).map_err(io)?;
    }
    for file in [
        "chitin.key",
        "dialog.tlk",
        "swkotor.exe",
        "override/a.uti",
        "override/b.UTI",
        "override/c.tga",
    ] {
        std::fs::write(root.join(file), b"").map_err(io)?;
    }
    Ok(())
}

/// A picked install folder says which game and how many items its override
/// holds; one found by kjk's settings is an install; with none found the
/// list is empty, and nothing panics (Review Focus 1).
pub fn installs() -> Result<String, String> {
    let root = scratch("install");
    let install = root.join("swkotor");
    fake_install(&install)?;
    let found = locate::check_path(&install);
    if found
        != (Found::Install {
            game: vfs::Game::Kotor1,
            override_items: 2,
        })
    {
        return Err(format!("the fake install: {found}"));
    }
    let settings = root.join("settings.json");
    let chosen = serde_json::json!({ "install.k1": install.display().to_string() });
    std::fs::write(&settings, chosen.to_string()).map_err(io)?;
    let by_settings = locate::discovery_with(|_| None, Some(&settings));
    let listed = locate::installs(&by_settings);
    let none = locate::installs(&vfs::Discovery::default());
    let nowhere = locate::check_path(&root.join("not-there"));
    let _ = std::fs::remove_dir_all(&root);
    if listed.first().map(|(game, path)| (*game, path.clone()))
        != Some((vfs::Game::Kotor1, install.clone()))
    {
        return Err(format!("by kjk's settings: {listed:?}"));
    }
    if !none.is_empty() || !matches!(nowhere, Found::Nothing(_)) {
        return Err(format!("none: {none:?}; a missing folder: {nowhere}"));
    }
    Ok(format!(
        "{found}; settings found {}; missing: {nowhere}",
        listed.len()
    ))
}
