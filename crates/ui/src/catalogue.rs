//! The UI catalogue: every screen module's stories, and the sizes and
//! inputs each story is drawn at (docs/UI-GUIDELINES.md). A story is a named
//! state of a screen, reached by the steps `kjk gui screen --step` takes.
//! Each module of `screens` has a story file of its own name in
//! `data/ui/stories`, which the approved test `ui_catalogue` checks. A story
//! may start from a capture of play (`data/ui/captures`): the view model of
//! one moment of a real run, which the console's `ui capture NAME` writes.

use toml_edit::{DocumentMut, Item};

/// The folder of the story files, from the repository's root.
pub const STORIES: &str = "data/ui/stories";

/// The folder of the captures of play, from the repository's root.
pub const CAPTURES: &str = "data/ui/captures";

/// The screen a story names to show what is drawn over play: KOTOR's HUD,
/// its floating target block, the power wheel and the banner
/// (`crate::hud_layer`). They are drawn outside the screen stack.
pub const PLAY: &str = "play";

/// The story files' folder in the checkout this was built from: the
/// catalogue is a tool of the repository, not of a player's build.
pub fn folder() -> std::path::PathBuf {
    root().join(STORIES)
}

/// The captures' folder in the checkout this was built from.
pub fn captures() -> std::path::PathBuf {
    root().join(CAPTURES)
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
    /// The capture of play it starts from with the mouse (`Capture`), by
    /// its name in `data/ui/captures`. Without one, the story starts from
    /// stand-ins.
    pub capture: Option<String>,
    /// The capture it starts from with a controller: the same moment
    /// captured while a pad played. The game fills the action bar and the
    /// key hints for the input in use, so each input needs its own capture.
    pub pad_capture: Option<String>,
}

impl Story {
    /// The capture it starts from with `input`, by name, and whether it is
    /// drawn with that input at all: a story with a capture and no capture
    /// for the pad is drawn with the mouse alone.
    pub fn capture_for(&self, input: Input) -> Option<Option<&str>> {
        match (input, &self.capture, &self.pad_capture) {
            (Input::Mouse, capture, _) => Some(capture.as_deref()),
            (Input::Pad, _, Some(pad)) => Some(Some(pad)),
            (Input::Pad, Some(_), None) => None,
            (Input::Pad, None, None) => Some(None),
        }
    }
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
                capture: words(table.get("capture"), "capture")?,
                pad_capture: words(table.get("pad_capture"), "pad_capture")?,
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

/// One moment of play as the UI saw it: its view model, and the window it
/// was drawn in, in logical pixels. The view model places what follows a
/// point in the world, such as the floating target block, in shares of the
/// window, so a story draws it over the same spot at every size.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Capture {
    /// The game it was taken in, by its key (`k1`, `tsl`). Its words are
    /// lines of that game's talk table.
    pub game: String,
    pub window: [f32; 2],
    pub scale_factor: f32,
    pub view: crate::vm::ViewModel,
}

impl Capture {
    /// `view` of `game` as drawn in a window of `window` logical pixels, on a display
    /// of `scale_factor` physical pixels to each. The list of the saves on
    /// disk is left out: it names the player's own saves, and no screen
    /// over play shows it.
    pub fn of(
        view: &crate::vm::ViewModel,
        game: vfs::Game,
        window: [f32; 2],
        scale_factor: f32,
    ) -> Capture {
        Capture {
            game: game.key().to_string(),
            window,
            scale_factor,
            view: crate::vm::ViewModel {
                saves: Vec::new(),
                ..view.clone()
            },
        }
    }

    /// It as a capture file holds it: JSON, laid out for a reader. Each run
    /// of the game's words that `talk` finds in the game's talk table is
    /// written as its line there (`{"$talk": 123}`), so the file holds no
    /// text of the game's.
    pub fn text(&self, talk: &dyn Talk) -> Result<String, String> {
        let mut value = serde_json::to_value(self).map_err(|error| error.to_string())?;
        lines_for_words(&mut value, talk);
        serde_json::to_string_pretty(&value)
            .map(|text| text + "\n")
            .map_err(|error| error.to_string())
    }

    /// A capture file's text, read, each line of the talk table it names
    /// read from `talk`.
    pub fn read(text: &str, talk: &dyn Talk) -> Result<Capture, String> {
        let mut value: serde_json::Value =
            serde_json::from_str(text).map_err(|error| error.to_string())?;
        words_for_lines(&mut value, talk)?;
        serde_json::from_value(value).map_err(|error| error.to_string())
    }
}

/// The file of the capture named `name` in `folder`.
fn capture_file(folder: &std::path::Path, name: &str) -> std::path::PathBuf {
    folder.join(format!("{name}.json"))
}

/// A line of the game's talk table, as a capture file names it in place of
/// its words. `unbraced` says that the words are the line's with what it
/// holds in braces taken out, as the games show it
/// (`fmt_odyssey::tlk::unbraced`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TalkLine {
    pub strref: u32,
    pub unbraced: bool,
}

/// The game's talk table, as captures are written and read with it.
pub trait Talk {
    /// The line whose words, as the game shows them, are `words`.
    fn line_of(&self, words: &[u8]) -> Option<TalkLine>;
    /// The words of `line`, as the game shows them.
    fn words_of(&self, line: TalkLine) -> Option<Vec<u8>>;
    /// The code page the words are written in, to read them as text (a
    /// save's area name is held as text).
    fn code_page(&self) -> fmt_odyssey::text::CodePage;
}

/// An install's talk table, with its lines looked up by their words.
pub struct TalkTable<'a> {
    tlk: &'a fmt_odyssey::tlk::Tlk,
    code_page: fmt_odyssey::text::CodePage,
    lines: std::collections::HashMap<Vec<u8>, TalkLine>,
}

impl TalkTable<'_> {
    /// `tlk`'s lines by their words. Where two lines have the same words,
    /// the first one is named. A line's own words are found before another
    /// line's words with braces taken out.
    pub fn new(tlk: &fmt_odyssey::tlk::Tlk) -> TalkTable<'_> {
        let entries = (tlk.entries.iter().enumerate())
            .filter_map(|(strref, entry)| Some((u32::try_from(strref).ok()?, entry)))
            .chain(tlk.lent.iter().map(|(strref, entry)| (*strref, entry)));
        let texts: Vec<(u32, &[u8])> = entries
            .filter(|(_, entry)| entry.text.len() >= MIN_WORDS)
            .map(|(strref, entry)| (strref, entry.text.as_slice()))
            .collect();
        let mut lines = std::collections::HashMap::new();
        for &(strref, text) in &texts {
            lines.entry(text.to_vec()).or_insert(TalkLine {
                strref,
                unbraced: false,
            });
        }
        for &(strref, text) in &texts {
            let shown = fmt_odyssey::tlk::unbraced(text);
            if shown.len() >= MIN_WORDS {
                lines.entry(shown).or_insert(TalkLine {
                    strref,
                    unbraced: true,
                });
            }
        }
        TalkTable {
            tlk,
            code_page: fmt_odyssey::text::CodePage::of(tlk),
            lines,
        }
    }
}

impl Talk for TalkTable<'_> {
    fn line_of(&self, words: &[u8]) -> Option<TalkLine> {
        self.lines.get(words).copied()
    }

    fn words_of(&self, line: TalkLine) -> Option<Vec<u8>> {
        let text = self.tlk.text(line.strref)?;
        Some(if line.unbraced {
            fmt_odyssey::tlk::unbraced(text)
        } else {
            text.to_vec()
        })
    }

    fn code_page(&self) -> fmt_odyssey::text::CodePage {
        self.code_page
    }
}

/// Stand-in words for each line (`talk 123`, `talk 123 unbraced`), for a
/// check that reads captures without an install. It finds the lines of the
/// words it gives back.
pub struct StandInTalk;

impl Talk for StandInTalk {
    fn line_of(&self, words: &[u8]) -> Option<TalkLine> {
        let words = std::str::from_utf8(words).ok()?.strip_prefix("talk ")?;
        let (strref, unbraced) = match words.strip_suffix(" unbraced") {
            Some(strref) => (strref, true),
            None => (words, false),
        };
        Some(TalkLine {
            strref: strref.parse().ok()?,
            unbraced,
        })
    }

    fn words_of(&self, line: TalkLine) -> Option<Vec<u8>> {
        let unbraced = if line.unbraced { " unbraced" } else { "" };
        Some(format!("talk {}{unbraced}", line.strref).into_bytes())
    }

    fn code_page(&self) -> fmt_odyssey::text::CodePage {
        fmt_odyssey::text::CodePage::Western
    }
}

/// The fewest bytes a run of words has for a capture to name its line.
/// Shorter runs, such as a slot's key "1", are left as they are.
const MIN_WORDS: usize = 3;

/// The fields that hold the game's words as text rather than as bytes: a
/// save's area name and its name. Other text, such as key names, is kjk's.
const TEXT_FIELDS: [&str; 2] = ["area", "name"];

/// The key a capture file names a line of the talk table by.
const TALK_KEY: &str = "$talk";

/// The view model's field written as it is: the game's settings, which
/// hold no words of the game's and stay readable.
const PLAIN: &str = "settings";

/// Each run of bytes in `value` (a list of numbers from 0 to 255) whose
/// words `talk` finds swapped for its line.
fn lines_for_words(value: &mut serde_json::Value, talk: &dyn Talk) {
    use serde_json::Value;
    match value {
        Value::Array(items) => {
            let bytes: Option<Vec<u8>> = (items.iter())
                .map(|item| item.as_u64().and_then(|byte| u8::try_from(byte).ok()))
                .collect();
            let line = bytes
                .filter(|bytes| words_like(bytes))
                .and_then(|bytes| talk.line_of(&bytes));
            match line {
                Some(line) => *value = named(line, false),
                None => items
                    .iter_mut()
                    .for_each(|item| lines_for_words(item, talk)),
            }
        }
        Value::Object(fields) => {
            for (key, field) in fields.iter_mut().filter(|(key, _)| *key != PLAIN) {
                match field {
                    Value::String(text) if TEXT_FIELDS.contains(&key.as_str()) => {
                        let bytes: Option<Vec<u8>> =
                            text.chars().map(|ch| talk.code_page().byte(ch)).collect();
                        let line = bytes
                            .filter(|bytes| words_like(bytes))
                            .and_then(|bytes| talk.line_of(&bytes));
                        if let Some(line) = line {
                            *field = named(line, true);
                        }
                    }
                    _ => lines_for_words(field, talk),
                }
            }
        }
        _ => {}
    }
}

/// Whether `bytes` may be words of the game's: at least `MIN_WORDS` of them
/// and a letter among them. A pair of numbers, such as a character's
/// health and its most, is not.
fn words_like(bytes: &[u8]) -> bool {
    bytes.len() >= MIN_WORDS && bytes.iter().any(u8::is_ascii_alphabetic)
}

/// `line` as a capture file names it, standing for bytes or, with `text`,
/// for text.
fn named(line: TalkLine, text: bool) -> serde_json::Value {
    let mut named = serde_json::Map::new();
    named.insert(TALK_KEY.into(), line.strref.into());
    if line.unbraced {
        named.insert("unbraced".into(), true.into());
    }
    if text {
        named.insert("text".into(), true.into());
    }
    serde_json::Value::Object(named)
}

/// Each line of the talk table `value` names swapped back for its words.
fn words_for_lines(value: &mut serde_json::Value, talk: &dyn Talk) -> Result<(), String> {
    use serde_json::Value;
    match value {
        Value::Object(fields) => match fields.get(TALK_KEY).and_then(Value::as_u64) {
            Some(strref) => {
                let line = TalkLine {
                    strref: u32::try_from(strref).map_err(|_| format!("{strref}: not a line"))?,
                    unbraced: fields
                        .get("unbraced")
                        .and_then(Value::as_bool)
                        .unwrap_or(false),
                };
                let words = talk
                    .words_of(line)
                    .ok_or(format!("line {strref} is not in this install's talk table"))?;
                *value = if fields.get("text").and_then(Value::as_bool).unwrap_or(false) {
                    Value::String(talk.code_page().readable(&words))
                } else {
                    Value::Array(words.into_iter().map(Value::from).collect())
                };
                Ok(())
            }
            None => fields
                .values_mut()
                .try_for_each(|field| words_for_lines(field, talk)),
        },
        Value::Array(items) => items
            .iter_mut()
            .try_for_each(|item| words_for_lines(item, talk)),
        _ => Ok(()),
    }
}

/// The capture named `name` in `folder`, read with `talk`.
pub fn read_capture(
    folder: &std::path::Path,
    name: &str,
    talk: &dyn Talk,
) -> Result<Capture, String> {
    let path = capture_file(folder, name);
    let text =
        std::fs::read_to_string(&path).map_err(|error| format!("{}: {error}", path.display()))?;
    Capture::read(&text, talk).map_err(|error| format!("{}: {error}", path.display()))
}

/// `capture` written into `folder` as `name` with `talk`, and the file's
/// path.
pub fn write_capture(
    folder: &std::path::Path,
    name: &str,
    capture: &Capture,
    talk: &dyn Talk,
) -> Result<std::path::PathBuf, String> {
    if name.is_empty()
        || !name
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || ch == '-' || ch == '_')
    {
        return Err(format!(
            "{name:?}: a capture's name is letters, digits, - and _"
        ));
    }
    std::fs::create_dir_all(folder).map_err(|error| format!("{}: {error}", folder.display()))?;
    let path = capture_file(folder, name);
    std::fs::write(&path, capture.text(talk)?)
        .map_err(|error| format!("{}: {error}", path.display()))?;
    Ok(path)
}
