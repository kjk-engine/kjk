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
    ]
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
