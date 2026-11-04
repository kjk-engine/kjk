//! The window's font, the system's own (none is shipped): found by the
//! names each OS keeps, in its font folders; told in words when none is.

use std::path::Path;

use super::locate::scratch;
use crate::model::fonts;

fn put(path: &Path, bytes: &[u8]) -> Result<(), String> {
    std::fs::create_dir_all(path.parent().ok_or("no parent")?)
        .map_err(|error| error.to_string())?;
    std::fs::write(path, bytes).map_err(|error| error.to_string())
}

/// A font folder searched as Linux's are: the first wanted name that is a
/// font wins, wherever it sits below; a file that is no font is passed
/// over; with none, the refusal names what was looked for.
pub fn found() -> Result<String, String> {
    let root = scratch("fonts");
    let truetype = [0, 1, 0, 0, 0, 12];
    put(
        &root.join("share/fonts/noto/NotoSans-Regular.ttf"),
        b"not a font",
    )?;
    put(
        &root.join("share/fonts/truetype/dejavu/DejaVuSans.ttf"),
        &truetype,
    )?;
    put(
        &root.join("share/fonts/truetype/dejavu/DejaVuSansMono.ttf"),
        b"ttcf\0\x01",
    )?;
    let places = fonts::Places {
        files: Vec::new(),
        folders: vec![root.join("share/fonts")],
    };
    let wanted = fonts::Wanted {
        text: &["NotoSans-Regular.ttf", "DejaVuSans.ttf"],
        mono: &["DejaVuSansMono.ttf"],
    };
    let got = fonts::find(&places, &wanted);
    let empty = fonts::find(
        &fonts::Places {
            files: Vec::new(),
            folders: vec![root.join("none")],
        },
        &wanted,
    );
    let _ = std::fs::remove_dir_all(&root);
    let got = got?;
    let names = (
        got.text
            .path
            .file_name()
            .and_then(|name| name.to_str())
            .map(str::to_string),
        got.mono
            .path
            .file_name()
            .and_then(|name| name.to_str())
            .map(str::to_string),
    );
    if names
        != (
            Some("DejaVuSans.ttf".into()),
            Some("DejaVuSansMono.ttf".into()),
        )
        || got.text.bytes != truetype
    {
        return Err(format!("found {names:?}"));
    }
    match empty {
        Err(why) if why.contains("DejaVuSans.ttf") => Ok(format!("found {names:?}; none: {why}")),
        other => Err(format!(
            "an empty folder: {:?}",
            other.map(|fonts| fonts.text.path)
        )),
    }
}

/// This machine's own font, as the editor loads it at start.
pub fn this_system() -> Result<String, String> {
    let fonts = fonts::system()?;
    Ok(format!(
        "{} ({} bytes), mono {}",
        fonts.text.path.display(),
        fonts.text.bytes.len(),
        fonts.mono.path.display()
    ))
}
