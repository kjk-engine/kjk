//! The window's fonts: the system's own, read where each OS keeps them. No
//! font ships with the editor (a font's licence is not kjk's), so none of
//! its licences is in the package.

use std::path::{Path, PathBuf};

/// A font file as read.
#[derive(Debug, Clone)]
pub struct Font {
    pub path: PathBuf,
    pub bytes: Vec<u8>,
}

/// The text's font, and a fixed-width one (the text's when there is none).
#[derive(Debug, Clone)]
pub struct Fonts {
    pub text: Font,
    pub mono: Font,
}

/// Where to look: files by their full paths, then folders searched below
/// by file name.
pub struct Places {
    pub files: Vec<PathBuf>,
    pub folders: Vec<PathBuf>,
}

/// The file names wanted, the best first.
pub struct Wanted<'a> {
    pub text: &'a [&'a str],
    pub mono: &'a [&'a str],
}

/// Whether `bytes` begin as a TrueType, OpenType or collection file.
fn is_font(bytes: &[u8]) -> bool {
    [&[0, 1, 0, 0][..], b"true", b"OTTO", b"ttcf"]
        .iter()
        .any(|magic| bytes.starts_with(magic))
}

/// The first of `names` (in their order) found at `places` that is a
/// font.
fn first(places: &Places, names: &[&str]) -> Option<Font> {
    names.iter().find_map(|name| {
        let files = places
            .files
            .iter()
            .filter(|path| path.file_name().is_some_and(|file| file == *name))
            .cloned();
        let below = places.folders.iter().flat_map(|folder| below(folder, name));
        files.chain(below).find_map(|path| {
            let bytes = std::fs::read(&path).ok().filter(|bytes| is_font(bytes))?;
            Some(Font { path, bytes })
        })
    })
}

/// The files called `name` anywhere below `folder` (a few levels down, as
/// font folders go).
fn below(folder: &Path, name: &str) -> Vec<PathBuf> {
    let mut found = Vec::new();
    let mut stack = vec![(folder.to_path_buf(), 0)];
    while let Some((folder, depth)) = stack.pop() {
        for entry in std::fs::read_dir(&folder).into_iter().flatten().flatten() {
            let path = entry.path();
            if path.is_dir() && depth < 4 {
                stack.push((path, depth + 1));
            } else if entry.file_name() == name {
                found.push(path);
            }
        }
    }
    found.sort();
    found
}

/// The fonts `wanted` at `places`; why not, naming what was looked for.
pub fn find(places: &Places, wanted: &Wanted) -> Result<Fonts, String> {
    let text = first(places, wanted.text).ok_or(format!(
        "no system font found: looked for {} in {}",
        wanted.text.join(", "),
        places
            .files
            .iter()
            .filter_map(|file| file.parent())
            .chain(places.folders.iter().map(PathBuf::as_path))
            .map(|path| path.display().to_string())
            .collect::<Vec<_>>()
            .join(", ")
    ))?;
    let mono = first(places, wanted.mono).unwrap_or_else(|| text.clone());
    Ok(Fonts { text, mono })
}

/// This system's fonts: macOS's Arial or Helvetica and Menlo; Windows'
/// Segoe UI or Arial and Consolas; on Linux and the Steam Deck, Noto Sans,
/// DejaVu Sans, Liberation Sans or Cantarell and their fixed-width kin from
/// the font folders.
pub fn system() -> Result<Fonts, String> {
    let home = std::env::var_os("HOME").map(PathBuf::from);
    let (places, wanted) = if cfg!(target_os = "macos") {
        let files = [
            "/System/Library/Fonts/Supplemental/Arial.ttf",
            "/Library/Fonts/Arial.ttf",
            "/System/Library/Fonts/Helvetica.ttc",
            "/System/Library/Fonts/Geneva.ttf",
            "/System/Library/Fonts/Menlo.ttc",
            "/System/Library/Fonts/Monaco.ttf",
        ];
        (
            Places {
                files: files.iter().map(PathBuf::from).collect(),
                folders: Vec::new(),
            },
            Wanted {
                text: &["Arial.ttf", "Helvetica.ttc", "Geneva.ttf"],
                mono: &["Menlo.ttc", "Monaco.ttf"],
            },
        )
    } else if cfg!(windows) {
        let windows =
            std::env::var_os("WINDIR").map_or_else(|| PathBuf::from(r"C:\Windows"), PathBuf::from);
        (
            Places {
                files: Vec::new(),
                folders: vec![windows.join("Fonts")],
            },
            Wanted {
                text: &["segoeui.ttf", "arial.ttf", "tahoma.ttf"],
                mono: &["consola.ttf", "cour.ttf"],
            },
        )
    } else {
        let data = std::env::var_os("XDG_DATA_HOME")
            .map(PathBuf::from)
            .or_else(|| home.as_ref().map(|home| home.join(".local/share")));
        let folders = ["/usr/share/fonts", "/usr/local/share/fonts"]
            .iter()
            .map(PathBuf::from)
            .chain(data.map(|data| data.join("fonts")))
            .chain(home.map(|home| home.join(".fonts")))
            .collect();
        (
            Places {
                files: Vec::new(),
                folders,
            },
            Wanted {
                text: &[
                    "NotoSans-Regular.ttf",
                    "DejaVuSans.ttf",
                    "LiberationSans-Regular.ttf",
                    "Cantarell-Regular.otf",
                    "FreeSans.ttf",
                ],
                mono: &[
                    "NotoSansMono-Regular.ttf",
                    "DejaVuSansMono.ttf",
                    "LiberationMono-Regular.ttf",
                    "FreeMono.ttf",
                ],
            },
        )
    };
    find(&places, &wanted)
}
