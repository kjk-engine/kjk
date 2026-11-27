//! The Lighting that the editor's Play, Play here and walk mode start kjk
//! with (`kjk --lighting`, `docs/LEVEL-EDIT.md`). kjk then lights that run
//! as chosen, whatever the game's options hold, and leaves the options as
//! they are. The choice is kept for the next time the editor opens, in
//! `editor-play.toml` in kjk's own folder.

use std::path::PathBuf;

/// One choice: the value kjk takes (`None` for the game's own options), the
/// name the editor shows, and what it does.
pub(crate) struct Choice {
    pub(crate) value: Option<&'static str>,
    pub(crate) name: &'static str,
    pub(crate) hint: &'static str,
}

/// Every choice, in the order the editor lists them.
pub(crate) const CHOICES: [Choice; 4] = [
    Choice {
        value: None,
        name: "Options",
        hint: "Play is lit as the game's options say",
    },
    Choice {
        value: Some("classic"),
        name: "Classic",
        hint: "Play is lit as the original games were lit",
    },
    Choice {
        value: Some("enhanced"),
        name: "Enhanced",
        hint: "Play is lit with kjk's own lighting, which a project's bake and its fitted lights are made for",
    },
    Choice {
        value: Some("baked"),
        name: "Enhanced Lightmaps",
        hint: "Play is lit with kjk's own lighting and the lighting download's baked lightmaps, where the download is installed",
    },
];

/// The choice whose value is `value`.
pub(crate) fn choice(value: Option<&str>) -> &'static Choice {
    (CHOICES.iter())
        .find(|choice| choice.value == value)
        .unwrap_or(&CHOICES[0])
}

/// The choice a command names: `options`, `classic`, `enhanced` or `baked`.
pub(crate) fn of_word(word: &str) -> Result<Option<&'static str>, String> {
    if word.eq_ignore_ascii_case("options") {
        return Ok(None);
    }
    (CHOICES.iter())
        .filter_map(|choice| choice.value)
        .find(|value| value.eq_ignore_ascii_case(word))
        .map(Some)
        .ok_or_else(|| "play lighting takes options, classic, enhanced or baked".to_string())
}

/// The file that keeps the choice: the one `KJK_EDITOR_PLAY` names, else
/// `editor-play.toml` in kjk's own folder.
fn file() -> Option<PathBuf> {
    if let Some(path) = std::env::var_os("KJK_EDITOR_PLAY").filter(|path| !path.is_empty()) {
        return Some(PathBuf::from(path));
    }
    store::kjk_folder(|var| std::env::var(var).ok()).map(|folder| folder.join("editor-play.toml"))
}

/// The choice kept from the last time, else the game's options.
pub(crate) fn load() -> Option<&'static str> {
    let text = file().and_then(|path| std::fs::read_to_string(path).ok())?;
    let value = text.lines().find_map(|line| {
        let (key, value) = line.split_once('=')?;
        (key.trim() == "lighting").then(|| value.trim().trim_matches('"').to_string())
    })?;
    of_word(&value).ok().flatten()
}

/// Keeps `value` for the next time the editor opens.
pub(crate) fn save(value: Option<&str>) -> Result<(), String> {
    let path = file().ok_or("kjk's own folder is not known, so the choice is not kept")?;
    let text = format!(
        "# The Lighting the level editor's Play starts kjk with: options, classic,\n\
         # enhanced or baked (Enhanced Lightmaps).\n\
         lighting = \"{}\"\n",
        value.unwrap_or("options")
    );
    std::fs::write(&path, text).map_err(|error| format!("{}: {error}", path.display()))
}
