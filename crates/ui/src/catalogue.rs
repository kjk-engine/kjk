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
    /// The captures of play it starts from with the mouse (`Capture`), by
    /// game (`k1`, `tsl`) and by name in `data/ui/captures`. A story is one
    /// situation, such as a hostile target held, and each game has its own
    /// capture of it. Without captures, the story starts from stand-ins.
    pub capture: std::collections::BTreeMap<String, String>,
    /// The captures it starts from with a controller: the same moments
    /// captured while a pad played. The game fills the action bar and the
    /// key hints for the input in use, so each input needs its own capture.
    pub pad_capture: std::collections::BTreeMap<String, String>,
}

impl Story {
    /// The capture it starts from on `game` with `input`, by name, and
    /// whether it is drawn so at all. A story without captures starts from
    /// stand-ins everywhere. A story with captures is drawn only where it
    /// has one.
    pub fn capture_for(&self, game: &str, input: Input) -> Option<Option<&str>> {
        if self.capture.is_empty() && self.pad_capture.is_empty() {
            return Some(None);
        }
        let captures = match input {
            Input::Mouse => &self.capture,
            Input::Pad => &self.pad_capture,
        };
        captures.get(game).map(|name| Some(name.as_str()))
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
    let by_game = |item: Option<&Item>,
                   field: &str|
     -> Result<std::collections::BTreeMap<String, String>, String> {
        let Some(item) = item else {
            return Ok(Default::default());
        };
        let table = item.as_table_like().ok_or(why(&format!(
            "{field} is not a table of games, as {{ k1 = \"…\", tsl = \"…\" }}"
        )))?;
        table
            .iter()
            .map(|(game, name)| {
                let name = name
                    .as_str()
                    .ok_or(why(&format!("{field}.{game} is not text")))?;
                Ok((game.to_string(), name.to_string()))
            })
            .collect()
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
            if let Some(same) = stories
                .iter()
                .find(|story: &&Story| slug(&story.name) == slug(&name))
            {
                return Err(why(&format!(
                    "the stories {:?} and {name:?} have the same slug {:?}, so `--story` and the list could not tell them apart",
                    same.name,
                    slug(&name)
                )));
            }
            stories.push(Story {
                steps: list(table.get("steps"), "steps")?.unwrap_or_else(Vec::new),
                error: words(table.get("error"), "error")?,
                capture: by_game(table.get("capture"), "capture")?,
                pad_capture: by_game(table.get("pad_capture"), "pad_capture")?,
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

/// The settings that tie a capture to the machine it was taken on: the
/// install's host id, and the display's density and the distance the
/// owner looks from, which the catalogue keeps for the screen in use.
pub const PERSONAL_SETTINGS: [&str; 3] = [
    "host.id",
    "catalogue.display_density",
    "catalogue.distance_cm",
];

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
    /// text of the game's. It fails when text is left that holds a stretch
    /// of a line of the talk table. The settings that name the player's
    /// machine (`PERSONAL_SETTINGS`) are left out. A field of the view model that holds
    /// what an empty view model holds is left out.
    pub fn text(&self, talk: &dyn Talk) -> Result<String, String> {
        let mut value = serde_json::to_value(self).map_err(|error| error.to_string())?;
        if let Some(serde_json::Value::Object(settings)) = value.pointer_mut("/view/settings") {
            settings.retain(|key, _| !PERSONAL_SETTINGS.contains(&key.as_str()));
        }
        lines_for_words(&mut value, talk);
        if let Some(text) = quoted(&value, talk, "") {
            return Err(format!(
                "the capture holds the game's own words that no line of its talk table names, at {text}"
            ));
        }
        let empty = serde_json::to_value(crate::vm::ViewModel::default())
            .map_err(|error| error.to_string())?;
        if let (Some(serde_json::Value::Object(view)), serde_json::Value::Object(empty)) =
            (value.get_mut("view"), empty)
        {
            view.retain(|field, held| empty.get(field) != Some(held));
        }
        let mut text = String::new();
        write_value(&value, 0, &mut text);
        text.push('\n');
        Ok(text)
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
/// (`fmt_odyssey::tlk::unbraced`). `tail` says that the words are what the
/// line holds after its last " - ", as the area transition panel shows
/// "Taris - Upper City South" as "Upper City South".
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TalkLine {
    pub strref: u32,
    pub unbraced: bool,
    pub tail: bool,
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
    /// The line that holds a place for a number (`<CUSTOM0>`) whose words
    /// are `words` with a number in that place, and the number's digits.
    fn template_of(&self, _words: &[u8]) -> Option<(TalkLine, Vec<u8>)> {
        None
    }
    /// Whether `words` hold a stretch of a line of the game's own text,
    /// which a capture file must never keep as it is.
    fn quotes(&self, _words: &[u8]) -> bool {
        false
    }
}

/// What looking up an install's talk table by words needs, made once from
/// the table (`TalkTable::with` reuses it, so that a program that stages
/// many stories does not make it again for each).
#[derive(Clone)]
pub struct TalkIndex {
    code_page: fmt_odyssey::text::CodePage,
    lines: std::collections::HashMap<Vec<u8>, TalkLine>,
    templates: Vec<Template>,
    /// The first `QUOTED` bytes of each line that long or longer.
    long: std::collections::HashSet<Vec<u8>>,
}

/// A line with a place for one number (`<CUSTOM0>`): what comes before the
/// place and what comes after it.
#[derive(Clone)]
struct Template {
    strref: u32,
    before: Vec<u8>,
    after: Vec<u8>,
}

/// The place a line holds for a number that the game fills in.
const CUSTOM: &[u8] = b"<CUSTOM0>";

/// The most digits a number has in a line with a place for one.
const MOST_DIGITS: usize = 12;

/// How many bytes of a line's own text a capture may not hold as they are.
const QUOTED: usize = 24;

/// The separator of an area's place and its name, in the talk table's
/// names of areas (`Taris - Upper City South`).
const PLACE_AND_NAME: &[u8] = b" - ";

impl TalkIndex {
    /// `tlk`'s lines by their words. Where two lines have the same words,
    /// the first one is named. A line's own words are found before another
    /// line's words with braces taken out, and those before what a line
    /// holds after its last " - ".
    pub fn new(tlk: &fmt_odyssey::tlk::Tlk) -> TalkIndex {
        let entries = (tlk.entries.iter().enumerate())
            .filter_map(|(strref, entry)| Some((u32::try_from(strref).ok()?, entry)))
            .chain(tlk.lent.iter().map(|(strref, entry)| (*strref, entry)));
        let texts: Vec<(u32, &[u8])> = entries
            .filter(|(_, entry)| entry.text.len() >= MIN_WORDS)
            .map(|(strref, entry)| (strref, entry.text.as_slice()))
            .collect();
        let mut lines = std::collections::HashMap::new();
        let mut templates = Vec::new();
        let mut long = std::collections::HashSet::new();
        for &(strref, text) in &texts {
            lines.entry(text.to_vec()).or_insert(TalkLine {
                strref,
                unbraced: false,
                tail: false,
            });
            if text.len() >= QUOTED {
                long.insert(text[..QUOTED].to_vec());
            }
            if let Some(at) = find(text, CUSTOM)
                && find(&text[at + CUSTOM.len()..], b"<CUSTOM").is_none()
                && at >= MIN_WORDS
            {
                templates.push(Template {
                    strref,
                    before: text[..at].to_vec(),
                    after: text[at + CUSTOM.len()..].to_vec(),
                });
            }
        }
        for &(strref, text) in &texts {
            let shown = fmt_odyssey::tlk::unbraced(text);
            if shown.len() >= MIN_WORDS {
                lines.entry(shown).or_insert(TalkLine {
                    strref,
                    unbraced: true,
                    tail: false,
                });
            }
        }
        for &(strref, text) in &texts {
            let tail = (0..text.len())
                .rev()
                .find(|&at| text[at..].starts_with(PLACE_AND_NAME))
                .map(|at| &text[at + PLACE_AND_NAME.len()..]);
            if let Some(tail) = tail.filter(|tail| tail.len() >= MIN_WORDS) {
                lines.entry(tail.to_vec()).or_insert(TalkLine {
                    strref,
                    unbraced: false,
                    tail: true,
                });
            }
        }
        TalkIndex {
            code_page: fmt_odyssey::text::CodePage::of(tlk),
            lines,
            templates,
            long,
        }
    }
}

/// Where `needle` first is in `hay`.
fn find(hay: &[u8], needle: &[u8]) -> Option<usize> {
    hay.windows(needle.len())
        .position(|window| window == needle)
}

/// An install's talk table, with its lines looked up by their words.
pub struct TalkTable<'a> {
    tlk: &'a fmt_odyssey::tlk::Tlk,
    index: std::borrow::Cow<'a, TalkIndex>,
}

impl<'a> TalkTable<'a> {
    /// `tlk`'s lines by their words (`TalkIndex::new`).
    pub fn new(tlk: &'a fmt_odyssey::tlk::Tlk) -> TalkTable<'a> {
        TalkTable {
            tlk,
            index: std::borrow::Cow::Owned(TalkIndex::new(tlk)),
        }
    }

    /// `tlk` with `index` made from it earlier.
    pub fn with(tlk: &'a fmt_odyssey::tlk::Tlk, index: &'a TalkIndex) -> TalkTable<'a> {
        TalkTable {
            tlk,
            index: std::borrow::Cow::Borrowed(index),
        }
    }
}

impl Talk for TalkTable<'_> {
    fn line_of(&self, words: &[u8]) -> Option<TalkLine> {
        self.index.lines.get(words).copied()
    }

    fn words_of(&self, line: TalkLine) -> Option<Vec<u8>> {
        let text = self.tlk.text(line.strref)?;
        Some(if line.unbraced {
            fmt_odyssey::tlk::unbraced(text)
        } else if line.tail {
            let at = (0..text.len())
                .rev()
                .find(|&at| text[at..].starts_with(PLACE_AND_NAME))?;
            text[at + PLACE_AND_NAME.len()..].to_vec()
        } else {
            text.to_vec()
        })
    }

    fn code_page(&self) -> fmt_odyssey::text::CodePage {
        self.index.code_page
    }

    fn template_of(&self, words: &[u8]) -> Option<(TalkLine, Vec<u8>)> {
        self.index.templates.iter().find_map(|template| {
            let number = words
                .strip_prefix(template.before.as_slice())?
                .strip_suffix(template.after.as_slice())?;
            (!number.is_empty()
                && number.len() <= MOST_DIGITS
                && number.iter().all(u8::is_ascii_digit))
            .then(|| {
                let line = TalkLine {
                    strref: template.strref,
                    unbraced: false,
                    tail: false,
                };
                (line, number.to_vec())
            })
        })
    }

    fn quotes(&self, words: &[u8]) -> bool {
        words
            .windows(QUOTED)
            .any(|window| self.index.long.contains(window))
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
            tail: false,
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

/// The fewest bytes of a line that a longer text names (`$parts`), so that
/// kjk's own short words between them stay as they are.
const MIN_PART: usize = 4;

/// The fewest bytes of a line that a longer run of bytes names: a line of
/// the game's with a note of kjk's after it.
const MIN_LONG_PART: usize = 24;

/// The fields that hold the game's words as text rather than as bytes: a
/// save's area name and its name. Other text, such as key names, is kjk's.
const TEXT_FIELDS: [&str; 2] = ["area", "name"];

/// The fields that hold a list of texts that kjk composes from the game's
/// names and lines with its own words between them ("Trask: We've been
/// ambushed", "Jedi hit Sith Apprentice: 7 damage"): the messages page's
/// two logs.
const COMPOSED_FIELDS: [&str; 2] = ["dialog_log", "combat_log"];

/// The key a capture file names a line of the talk table by.
const TALK_KEY: &str = "$talk";

/// The key a capture file names a text made of lines of the talk table and
/// kjk's own words by: the list of them, in order.
const PARTS_KEY: &str = "$parts";

/// The view model's field written as it is: the game's settings, which
/// hold no words of the game's and stay readable.
const PLAIN: &str = "settings";

/// Each run of bytes in `value` (a list of numbers from 0 to 255) whose
/// words `talk` finds swapped for its line, and each text of a composed
/// field made of lines.
fn lines_for_words(value: &mut serde_json::Value, talk: &dyn Talk) {
    use serde_json::Value;
    match value {
        Value::Array(items) => {
            let bytes: Option<Vec<u8>> = (items.iter())
                .map(|item| item.as_u64().and_then(|byte| u8::try_from(byte).ok()))
                .collect();
            let named = bytes.filter(|bytes| words_like(bytes)).and_then(|bytes| {
                if let Some(line) = talk.line_of(&bytes) {
                    return Some(named(line, false));
                }
                if let Some((line, number)) = talk.template_of(&bytes) {
                    let mut named = named(line, false);
                    named["custom"] = Value::String(talk.code_page().readable(&number));
                    return Some(named);
                }
                let parts = parts_of(&bytes, talk, MIN_LONG_PART)?;
                let mut named = serde_json::Map::new();
                named.insert(PARTS_KEY.into(), Value::Array(parts));
                Some(Value::Object(named))
            });
            match named {
                Some(named) => *value = named,
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
                    Value::Array(texts) if COMPOSED_FIELDS.contains(&key.as_str()) => {
                        for text in texts {
                            if let Some(parts) = composed(text, talk) {
                                *text = parts;
                            }
                        }
                    }
                    _ => lines_for_words(field, talk),
                }
            }
        }
        _ => {}
    }
}

/// The text `value`, when it is a string that holds lines of the talk
/// table among other words, as the file names it.
fn composed(value: &serde_json::Value, talk: &dyn Talk) -> Option<serde_json::Value> {
    use serde_json::Value;
    let bytes: Vec<u8> = value
        .as_str()?
        .chars()
        .map(|ch| talk.code_page().byte(ch))
        .collect::<Option<_>>()?;
    let parts = parts_of(&bytes, talk, MIN_PART)?;
    let mut named = serde_json::Map::new();
    named.insert(PARTS_KEY.into(), Value::Array(parts));
    named.insert("text".into(), true.into());
    Some(Value::Object(named))
}

/// `bytes` as the lines of the talk table in it, each at the start of a
/// word and at the end of one, with the bytes between them. Nothing when
/// no line is in it.
fn parts_of(bytes: &[u8], talk: &dyn Talk, least: usize) -> Option<Vec<serde_json::Value>> {
    use serde_json::Value;
    let code_page = talk.code_page();
    let glue_of = |glue: &[u8]| {
        let readable = glue
            .iter()
            .all(|&byte| code_page.byte(code_page.char(byte)) == Some(byte));
        match readable {
            true => Value::String(code_page.readable(glue)),
            false => Value::Array(glue.iter().map(|&byte| byte.into()).collect()),
        }
    };
    let word = |at: usize| bytes.get(at).is_some_and(u8::is_ascii_alphanumeric);
    let mut parts = Vec::new();
    let mut glue = Vec::new();
    let mut named_any = false;
    let mut at = 0;
    while at < bytes.len() {
        let line = (at == 0 || !word(at - 1))
            .then(|| {
                (at + least..=bytes.len())
                    .rev()
                    .filter(|&end| !word(end))
                    .find_map(|end| Some((end, talk.line_of(&bytes[at..end])?)))
            })
            .flatten();
        match line {
            Some((end, line)) => {
                if !glue.is_empty() {
                    parts.push(glue_of(&std::mem::take(&mut glue)));
                }
                parts.push(named(line, false));
                named_any = true;
                at = end;
            }
            None => {
                glue.push(bytes[at]);
                at += 1;
            }
        }
    }
    if !glue.is_empty() {
        parts.push(glue_of(&glue));
    }
    named_any.then_some(parts)
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
    if line.tail {
        named.insert("tail".into(), true.into());
    }
    if text {
        named.insert("text".into(), true.into());
    }
    serde_json::Value::Object(named)
}

/// The words that the object `fields`, which names a line of the talk
/// table, stands for.
fn words_of_line(
    fields: &serde_json::Map<String, serde_json::Value>,
    talk: &dyn Talk,
) -> Result<Vec<u8>, String> {
    use serde_json::Value;
    let strref = fields
        .get(TALK_KEY)
        .and_then(Value::as_u64)
        .ok_or("not a line of the talk table")?;
    let flag = |name: &str| fields.get(name).and_then(Value::as_bool).unwrap_or(false);
    let line = TalkLine {
        strref: u32::try_from(strref).map_err(|_| format!("{strref}: not a line"))?,
        unbraced: flag("unbraced"),
        tail: flag("tail"),
    };
    let words = talk
        .words_of(line)
        .ok_or(format!("line {strref} is not in this install's talk table"))?;
    let Some(custom) = fields.get("custom").and_then(Value::as_str) else {
        return Ok(words);
    };
    let at = find(&words, CUSTOM).ok_or(format!("line {strref} has no place for a number"))?;
    let mut filled = words[..at].to_vec();
    filled.extend(custom.bytes());
    filled.extend_from_slice(&words[at + CUSTOM.len()..]);
    Ok(filled)
}

/// Each line of the talk table `value` names swapped back for its words.
fn words_for_lines(value: &mut serde_json::Value, talk: &dyn Talk) -> Result<(), String> {
    use serde_json::Value;
    match value {
        Value::Object(fields) => {
            let words = if let Some(parts) = fields.get(PARTS_KEY) {
                let parts = parts.as_array().ok_or("$parts is not a list")?;
                let mut words = Vec::new();
                for part in parts {
                    match part {
                        Value::String(text) => {
                            for ch in text.chars() {
                                let byte = talk.code_page().byte(ch);
                                words.push(byte.ok_or(format!("{ch:?}: not in the code page"))?);
                            }
                        }
                        Value::Array(bytes) => words.extend(
                            bytes
                                .iter()
                                .filter_map(|byte| u8::try_from(byte.as_u64()?).ok()),
                        ),
                        Value::Object(line) => words.extend(words_of_line(line, talk)?),
                        _ => return Err("a part of $parts is not text or a line".into()),
                    }
                }
                Some(words)
            } else if fields.contains_key(TALK_KEY) {
                Some(words_of_line(fields, talk)?)
            } else {
                None
            };
            match words {
                Some(words) => {
                    let text = fields.get("text").and_then(Value::as_bool).unwrap_or(false);
                    *value = if text {
                        Value::String(talk.code_page().readable(&words))
                    } else {
                        Value::Array(words.into_iter().map(Value::from).collect())
                    };
                    Ok(())
                }
                None => fields
                    .values_mut()
                    .try_for_each(|field| words_for_lines(field, talk)),
            }
        }
        Value::Array(items) => items
            .iter_mut()
            .try_for_each(|item| words_for_lines(item, talk)),
        _ => Ok(()),
    }
}

/// The first text in `value` that holds a stretch of the game's own text
/// that no line of the talk table names, as `where` says.
fn quoted(value: &serde_json::Value, talk: &dyn Talk, at: &str) -> Option<String> {
    use serde_json::Value;
    let shown = |bytes: &[u8]| talk.code_page().readable(&bytes[..bytes.len().min(60)]);
    match value {
        Value::String(text) => {
            let bytes: Vec<u8> = text
                .chars()
                .filter_map(|ch| talk.code_page().byte(ch))
                .collect();
            talk.quotes(&bytes)
                .then(|| format!("{at}: {}", shown(&bytes)))
        }
        Value::Array(items) => {
            let bytes: Option<Vec<u8>> = (items.iter())
                .map(|item| item.as_u64().and_then(|byte| u8::try_from(byte).ok()))
                .collect();
            match bytes {
                Some(bytes) if !bytes.is_empty() => talk
                    .quotes(&bytes)
                    .then(|| format!("{at}: {}", shown(&bytes))),
                _ => items
                    .iter()
                    .enumerate()
                    .find_map(|(n, item)| quoted(item, talk, &format!("{at}/{n}"))),
            }
        }
        Value::Object(fields) => fields
            .iter()
            .filter(|(key, _)| *key != PLAIN)
            .find_map(|(key, field)| quoted(field, talk, &format!("{at}/{key}"))),
        _ => None,
    }
}

/// `value` written for a reader: an object a line a field, and a list of
/// numbers or other plain values across as few lines as hold them.
fn write_value(value: &serde_json::Value, indent: usize, out: &mut String) {
    use serde_json::Value;
    const WIDTH: usize = 100;
    let pad = |depth: usize| "  ".repeat(depth);
    let plain = |value: &Value| !matches!(value, Value::Array(_) | Value::Object(_));
    match value {
        Value::Object(fields) if fields.is_empty() => out.push_str("{}"),
        Value::Object(fields) => {
            out.push_str("{\n");
            for (at, (key, field)) in fields.iter().enumerate() {
                out.push_str(&pad(indent + 1));
                out.push_str(&Value::String(key.clone()).to_string());
                out.push_str(": ");
                write_value(field, indent + 1, out);
                out.push_str(if at + 1 < fields.len() { ",\n" } else { "\n" });
            }
            out.push_str(&pad(indent));
            out.push('}');
        }
        Value::Array(items) if items.is_empty() => out.push_str("[]"),
        Value::Array(items) if items.iter().all(plain) => {
            let shown: Vec<String> = items.iter().map(Value::to_string).collect();
            let line = format!("[{}]", shown.join(", "));
            if indent * 2 + line.len() <= WIDTH {
                out.push_str(&line);
                return;
            }
            out.push_str("[\n");
            let mut row = pad(indent + 1);
            for (at, item) in shown.iter().enumerate() {
                let last = at + 1 == shown.len();
                let piece = format!("{item}{}", if last { "" } else { "," });
                if !row.trim().is_empty() && row.len() + piece.len() + 1 > WIDTH {
                    out.push_str(row.trim_end());
                    out.push('\n');
                    row = pad(indent + 1);
                }
                if !row.trim().is_empty() {
                    row.push(' ');
                }
                row.push_str(&piece);
            }
            out.push_str(&row);
            out.push('\n');
            out.push_str(&pad(indent));
            out.push(']');
        }
        Value::Array(items) => {
            out.push_str("[\n");
            for (at, item) in items.iter().enumerate() {
                out.push_str(&pad(indent + 1));
                write_value(item, indent + 1, out);
                out.push_str(if at + 1 < items.len() { ",\n" } else { "\n" });
            }
            out.push_str(&pad(indent));
            out.push(']');
        }
        other => out.push_str(&other.to_string()),
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
