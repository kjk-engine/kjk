//! The UI catalogue: every screen module's stories, and the sizes and
//! inputs each story is drawn at (docs/UI-GUIDELINES.md). A story is a named
//! state of a screen, reached by the steps `kjk gui screen --step` takes.
//! Each module of `screens` has a story file of its own name in
//! `data/ui/stories`, which the approved test `ui_catalogue` checks.

use toml_edit::{DocumentMut, Item};

/// The folder of the story files, from the repository's root.
pub const STORIES: &str = "data/ui/stories";

/// The story files' folder in the checkout this was built from: the
/// catalogue is a tool of the repository, not of a player's build.
pub fn folder() -> std::path::PathBuf {
    root().join(STORIES)
}

/// The root of the checkout this was built from.
pub fn root() -> std::path::PathBuf {
    let crate_folder = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    crate_folder
        .parent()
        .and_then(std::path::Path::parent)
        .unwrap_or(crate_folder)
        .to_path_buf()
}

/// A screen size the catalogue draws: its picture's size in the display's
/// physical pixels, and the display's physical pixels to each logical one.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Size {
    pub name: &'static str,
    pub pixels: (u32, u32),
    pub factor: f32,
}

impl Size {
    /// The window the screens are stepped in, in logical pixels.
    pub fn window(self) -> (f32, f32) {
        (
            self.pixels.0 as f32 / self.factor,
            self.pixels.1 as f32 / self.factor,
        )
    }

    pub fn named(name: &str) -> Option<Size> {
        SIZES.into_iter().find(|size| size.name == name)
    }
}

/// The six sizes of docs/UI-GUIDELINES.md.
pub const SIZES: [Size; 6] = [
    Size {
        name: "deck",
        pixels: (1280, 800),
        factor: 1.0,
    },
    Size {
        name: "1080p",
        pixels: (1920, 1080),
        factor: 1.0,
    },
    Size {
        name: "4k-tv",
        pixels: (3840, 2160),
        factor: 1.0,
    },
    Size {
        name: "4k-monitor",
        pixels: (3840, 2160),
        factor: 2.0,
    },
    Size {
        name: "4x3",
        pixels: (1440, 1080),
        factor: 1.0,
    },
    Size {
        name: "ultrawide",
        pixels: (3440, 1440),
        factor: 1.0,
    },
];

/// What a story is played with.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Input {
    /// A controller, its hints shown.
    Pad,
    /// The mouse and the keyboard.
    Mouse,
}

impl Input {
    pub const ALL: [Input; 2] = [Input::Pad, Input::Mouse];

    pub fn name(self) -> &'static str {
        match self {
            Input::Pad => "pad",
            Input::Mouse => "mouse",
        }
    }

    pub fn named(name: &str) -> Option<Input> {
        Input::ALL.into_iter().find(|input| input.name() == name)
    }

    /// The steps that set the input up before a story's own, at `size`: a
    /// controller's glyphs are the Steam Deck's on the Deck, else an Xbox
    /// pad's.
    pub fn steps(self, size: Size) -> Vec<String> {
        match self {
            Input::Pad if size.name == "deck" => vec!["pad deck".into()],
            Input::Pad => vec!["pad xbox".into()],
            Input::Mouse => Vec::new(),
        }
    }
}

/// Where a screen stands in the audit.
pub const STATUSES: [&str; 3] = ["not started", "needs work", "audited"];

#[derive(Debug, Clone, PartialEq)]
pub struct Story {
    pub name: String,
    /// The screen as `screens::open` names it.
    pub screen: String,
    pub steps: Vec<String>,
    /// The games it is drawn on, `k1` and `tsl`.
    pub games: Vec<String>,
    /// Why a session did not start, for the message screen.
    pub error: Option<String>,
}

/// A module's story file.
#[derive(Debug, Clone, PartialEq)]
pub struct Stories {
    pub module: String,
    pub status: String,
    pub notes: String,
    pub stories: Vec<Story>,
}

/// `text`, the story file of `module`, read.
pub fn parse(module: &str, text: &str) -> Result<Stories, String> {
    let why = |what: &str| format!("{module}: {what}");
    let document: DocumentMut = text.parse().map_err(|error| why(&format!("{error}")))?;
    let words = |item: Option<&Item>, field: &str| -> Result<Option<String>, String> {
        match item {
            None => Ok(None),
            Some(item) => (item.as_str().map(|text| Some(text.to_string())))
                .ok_or(why(&format!("{field} is not text"))),
        }
    };
    let list = |item: Option<&Item>, field: &str| -> Result<Option<Vec<String>>, String> {
        let Some(item) = item else {
            return Ok(None);
        };
        let array = item
            .as_array()
            .ok_or(why(&format!("{field} is not a list")))?;
        (array.iter())
            .map(|value| {
                (value.as_str().map(str::to_string)).ok_or(why(&format!("{field} holds no text")))
            })
            .collect::<Result<Vec<String>, String>>()
            .map(Some)
    };
    let status = words(document.get("status"), "status")?.ok_or(why("no status"))?;
    if !STATUSES.contains(&status.as_str()) {
        return Err(why(&format!(
            "status {status:?} is not one of {STATUSES:?}"
        )));
    }
    let notes = words(document.get("notes"), "notes")?.unwrap_or_else(String::new);
    let mut stories = Vec::new();
    if let Some(tables) = document.get("story") {
        let tables = tables
            .as_array_of_tables()
            .ok_or(why("story is not [[story]]"))?;
        for table in tables {
            let name = words(table.get("name"), "name")?.ok_or(why("a story has no name"))?;
            let screen =
                words(table.get("screen"), "screen")?.ok_or(why(&format!("{name}: no screen")))?;
            let games = list(table.get("games"), "games")?
                .unwrap_or_else(|| vec!["k1".into(), "tsl".into()]);
            if let Some(game) = games
                .iter()
                .find(|game| !matches!(game.as_str(), "k1" | "tsl"))
            {
                return Err(why(&format!("{name}: game {game} is not k1 or tsl")));
            }
            if stories.iter().any(|story: &Story| story.name == name) {
                return Err(why(&format!("two stories named {name}")));
            }
            stories.push(Story {
                steps: list(table.get("steps"), "steps")?.unwrap_or_else(Vec::new),
                error: words(table.get("error"), "error")?,
                name,
                screen,
                games,
            });
        }
    }
    Ok(Stories {
        module: module.to_string(),
        status,
        notes,
        stories,
    })
}

/// Every story file in `folder`, by module name.
pub fn read_all(folder: &std::path::Path) -> Result<Vec<Stories>, String> {
    let mut files: Vec<_> = std::fs::read_dir(folder)
        .map_err(|error| format!("{}: {error}", folder.display()))?
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| {
            path.extension()
                .is_some_and(|extension| extension == "toml")
        })
        .collect();
    files.sort();
    (files.iter())
        .map(|path| {
            let module = path
                .file_stem()
                .map(|stem| stem.to_string_lossy().into_owned());
            let text = std::fs::read_to_string(path)
                .map_err(|error| format!("{}: {error}", path.display()))?;
            parse(&module.unwrap_or_else(String::new), &text)
        })
        .collect()
}

/// A story's name as a file name: lower case, a dash for each run of
/// anything but letters and digits.
pub fn slug(name: &str) -> String {
    let mut out = String::new();
    for ch in name.to_lowercase().chars() {
        if ch.is_ascii_alphanumeric() {
            out.push(ch);
        } else if !out.ends_with('-') {
            out.push('-');
        }
    }
    out.trim_matches('-').to_string()
}
