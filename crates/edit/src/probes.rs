//! The editor's checks (the approved test `save_edits`): each a name and
//! what it found, or why it failed.

use std::path::{Path, PathBuf};

type Probe = (&'static str, Result<String, String>);

/// Every check, in order.
pub fn all() -> Vec<Probe> {
    vec![
        ("folder_platform", folder_platform()),
        ("folder_no_home", folder_no_home()),
        ("saves_folder", saves_folder()),
        ("open_refusals", open_refusals()),
        ("open_from_folder", open_from_folder()),
        ("data_k1", data_of(vfs::Game::Kotor1)),
        ("data_tsl", data_of(vfs::Game::Kotor2)),
    ]
}

/// The installs, as kjk finds them from the environment.
fn discovery() -> vfs::Discovery {
    vfs::discover(|name| std::env::var(name).ok())
}

/// `game`'s data for edits: every item the install holds, a lightsaber a
/// weapon among them, and classes that use the Force.
fn data_of(game: vfs::Game) -> Result<String, String> {
    let discovery = discovery();
    if discovery.get(game).is_none() {
        return Ok(format!("skipped: no {} install", game.key()));
    }
    let data = crate::Data::load(&discovery, game)?;
    let saber = data
        .catalog
        .get("g_w_lghtsbr01")
        .ok_or("no g_w_lghtsbr01 among the items")?;
    if saber.weapon.is_none() {
        return Err("the lightsaber is no weapon".into());
    }
    let force = data.classes.classes.iter().filter(|class| class.force_die > 0).count();
    if data.catalog.len() < 500 || force == 0 {
        return Err(format!("{} items, {force} Force classes", data.catalog.len()));
    }
    Ok(format!(
        "{} items ({} unread), {} classes ({force} with the Force)",
        data.catalog.len(),
        data.unreadable.len(),
        data.classes.classes.len()
    ))
}

/// The fixtures' save with every kind of state in it, encoded.
fn sink_bytes() -> Vec<u8> {
    sim::fixtures::kitchen_sink().1.encode()
}

/// `bytes` with their format version set to `version` (after `KJKV`).
fn versioned(mut bytes: Vec<u8>, version: u32) -> Vec<u8> {
    bytes[4..8].copy_from_slice(&version.to_le_bytes());
    bytes
}

/// What `open_bytes` makes of each kind of save, named.
fn open_refusals() -> Result<String, String> {
    use crate::Refusal;
    let (now, oldest) = (sim::SAVE_FORMAT, sim::SAVE_OLDEST);
    let bytes = sink_bytes();
    let half = bytes[..bytes.len() / 2].to_vec();
    type Case = (&'static str, Vec<u8>, fn(&Refusal) -> bool);
    let cases: [Case; 3] = [
        ("newer", versioned(bytes.clone(), now + 3), |r| {
            matches!(r, Refusal::Newer { found, reads_up_to } if *found == sim::SAVE_FORMAT + 3 && *reads_up_to == sim::SAVE_FORMAT)
        }),
        ("older", versioned(bytes.clone(), oldest - 1), |r| {
            matches!(r, Refusal::Older { found, reads_from } if *found == sim::SAVE_OLDEST - 1 && *reads_from == sim::SAVE_OLDEST)
        }),
        ("cut", half, |r| matches!(r, Refusal::Damaged(_))),
    ];
    crate::open_bytes(&bytes).map_err(|why| format!("the fixture refused: {why}"))?;
    let mut told = Vec::new();
    for (name, bytes, fits) in cases {
        match crate::open_bytes(&bytes) {
            Ok(_) => return Err(format!("{name}: opened")),
            Err(refusal) if fits(&refusal) => told.push(format!("{name}: {refusal}")),
            Err(refusal) => return Err(format!("{name}: {refusal:?}")),
        }
    }
    let newer = crate::open_bytes(&versioned(bytes, now + 3)).err().map(|r| r.to_string());
    let both = newer.is_some_and(|text| text.contains(&(now + 3).to_string()) && text.contains(&now.to_string()));
    if !both {
        return Err(format!("the newer refusal does not name both versions: {told:?}"));
    }
    Ok(told.join("; "))
}

/// A save written to a folder opens from it; an empty folder is no save.
fn open_from_folder() -> Result<String, String> {
    let root = std::env::temp_dir().join(format!("kjk-edit-open-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let save = sim::fixtures::kitchen_sink().1;
    let id = saves::write(&root, None, &save, &saves::Meta::default())?;
    let opened = crate::open(&root, &id).map_err(|why| why.to_string())?;
    if opened.save != save || opened.id != id {
        return Err("opened another save".into());
    }
    std::fs::create_dir_all(root.join("0099")).map_err(|e| e.to_string())?;
    let empty = crate::open(&root, "0099");
    let _ = std::fs::remove_dir_all(&root);
    match empty {
        Err(crate::Refusal::NotASave(why)) => Ok(format!("{id} opened; 0099: {why}")),
        other => Err(format!("an empty folder: {:?}", other.map(|o| o.id))),
    }
}

/// An environment of `pairs` alone.
fn env<'a>(pairs: &'a [(&'a str, &'a str)]) -> impl Fn(&str) -> Option<String> + 'a {
    move |name| {
        pairs
            .iter()
            .find(|(key, _)| *key == name)
            .map(|(_, value)| value.to_string())
    }
}

/// `want` as found, else what was.
fn expect<T: PartialEq + std::fmt::Debug>(found: T, want: T) -> Result<String, String> {
    if found == want {
        Ok(format!("{found:?}"))
    } else {
        Err(format!("{found:?}, not {want:?}"))
    }
}

const HOMES: [(&str, &str); 3] = [
    ("HOME", "/home/me"),
    ("APPDATA", "/appdata"),
    ("XDG_CONFIG_HOME", "/xdg"),
];

/// kjk's folder is the platform's own place for settings.
fn folder_platform() -> Result<String, String> {
    let base = if cfg!(target_os = "macos") {
        "/home/me/Library/Application Support"
    } else if cfg!(windows) {
        "/appdata"
    } else {
        "/xdg"
    };
    expect(store::kjk_folder(env(&HOMES)), Some(Path::new(base).join("kjk")))
}

/// Without a home there is no folder to tell.
fn folder_no_home() -> Result<String, String> {
    expect(store::kjk_folder(env(&[])), None)
}

/// `KJK_SAVES` wins, else `saves` beside the settings.
fn saves_folder() -> Result<String, String> {
    expect(
        saves::folder(env(&[("KJK_SAVES", "/mine")])),
        Some(PathBuf::from("/mine")),
    )?;
    expect(
        saves::folder(env(&HOMES)),
        store::kjk_folder(env(&HOMES)).map(|folder| folder.join("saves")),
    )
}
