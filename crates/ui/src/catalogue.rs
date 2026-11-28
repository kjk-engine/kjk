//! The UI catalogue: every screen module's stories, and the sizes and
//! inputs each story is drawn at (docs/UI-GUIDELINES.md). A story is a named
//! state of a screen, reached by the steps `kjk gui screen --step` takes.
//! Each module of `screens` has a story file of its own name in
//! `data/ui/stories`, which the approved test `ui_catalogue` checks. A story
//! may start from a capture of play (`data/ui/captures`): what the stories
//! draw of the view model at one moment of a real run, which the console's
//! `ui capture NAME [FOLDER]` writes.

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

/// The settings a capture keeps, by the start of their keys. The display's
/// and the feedback's options change what is drawn over play. The key
/// bindings, the camera, the sound and the machine's id do not.
const SETTINGS_KEPT: [&str; 2] = ["display.", "feedback."];

impl Capture {
    /// `view` of `game` as drawn in a window of `window` logical pixels, on
    /// a display of `scale_factor` physical pixels to each. It keeps only
    /// what the stories of captures draw: what stands over play, the
    /// galaxy map, the pad in use and the settings of `SETTINGS_KEPT`. The
    /// character's pages, the party, the key bindings, the logs and the
    /// saves on disk are left out. A story that needs another field adds
    /// it here.
    pub fn of(
        view: &crate::vm::ViewModel,
        game: vfs::Game,
        window: [f32; 2],
        scale_factor: f32,
    ) -> Capture {
        let settings = (view.settings.iter())
            .filter(|(key, _)| SETTINGS_KEPT.iter().any(|start| key.starts_with(start)))
            .map(|(key, value)| (key.clone(), value.clone()))
            .collect();
        Capture {
            game: game.key().to_string(),
            window,
            scale_factor,
            view: crate::vm::ViewModel {
                settings,
                session_rules: view.session_rules.clone(),
                pad_names: view.pad_names.clone(),
                pad_kind: view.pad_kind.clone(),
                pad_playing: view.pad_playing,
                hud: view.hud.clone(),
                camera_feed: view.camera_feed,
                target: view.target.clone(),
                transition: view.transition.clone(),
                crosshair: view.crosshair,
                reticle: view.reticle,
                melee_out: view.melee_out,
                hitmark: view.hitmark,
                block_reticle: view.block_reticle,
                perfect_mark: view.perfect_mark,
                bark: view.bark.clone(),
                floaties: view.floaties.clone(),
                overhead: view.overhead.clone(),
                arena: view.arena.clone(),
                arena_panel: view.arena_panel.clone(),
                wheel: view.wheel.clone(),
                summaries: view.summaries.clone(),
                dialog: view.dialog.clone(),
                requests: view.requests.clone(),
                calls: view.calls.clone(),
                galaxy: view.galaxy.clone(),
                pages: view.pages.clone(),
                pages_datapad: view.pages_datapad,
                ..crate::vm::ViewModel::default()
            },
        }
    }

    /// It as a capture file holds it: JSON, laid out for a reader, with
    /// every line of the game's talk table that `talk` finds in its words
    /// named in place of the words (`name_lines`). The view model's fields
    /// that are as a new one's are left out. It fails when the file
    /// would still hold the game's words, or would not read back the same.
    pub fn text(&self, talk: &dyn Talk) -> Result<String, String> {
        let mut value = serde_json::to_value(self).map_err(|error| error.to_string())?;
        let unset = serde_json::to_value(crate::vm::ViewModel::default())
            .map_err(|error| error.to_string())?;
        if let Some(view) = value.get_mut("view").and_then(serde_json::Value::as_object_mut) {
            view.retain(|field, value| unset.get(field) != Some(value));
        }
        name_lines(&mut value, false, talk);
        let text = serde_json::to_string_pretty(&value)
            .map(|text| text + "\n")
            .map_err(|error| error.to_string())?;
        let left = game_words(&text, talk)?;
        if !left.is_empty() {
            return Err(format!(
                "it would hold the game's words: {}",
                left.join("; ")
            ));
        }
        if Capture::read(&text, talk)? != *self {
            return Err("it would read otherwise once written".into());
        }
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

fn capture_file(folder: &std::path::Path, name: &str) -> std::path::PathBuf {
    folder.join(format!("{name}.json"))
}

/// The folder `ui capture` writes into: `folder` when one is given, else
/// the captures' folder of the checkout this was built from. A build
/// away from its checkout has no such folder, so it must be given one.
pub fn capture_folder(folder: Option<&std::path::Path>) -> Result<std::path::PathBuf, String> {
    if let Some(folder) = folder {
        return Ok(folder.to_path_buf());
    }
    let checkout = captures();
    if checkout.is_dir() {
        Ok(checkout)
    } else {
        Err(format!(
            "{} is not here, as in a build away from its checkout: name a folder, as `ui capture NAME FOLDER`",
            checkout.display()
        ))
    }
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
    /// The code page the words are written in, to read the words that the
    /// view model holds as text, such as a save's area name.
    fn code_page(&self) -> fmt_odyssey::text::CodePage;
    /// The line with tokens (`<CUSTOM0>`) that `words` start with, its
    /// tokens filled, as the game fills "Experience Points (XP) Received:
    /// <CUSTOM0>" with an amount: how many of `words` it takes, the line
    /// and what each token is filled with. The longest is found.
    fn filled_at(&self, _words: &[u8]) -> Option<(usize, TalkLine, Vec<Vec<u8>>)> {
        None
    }
    /// The words of `line`, its tokens filled with `fillings` in turn.
    fn filled_words(&self, line: TalkLine, fillings: &[Vec<u8>]) -> Option<Vec<u8>> {
        let words = self.words_of(line)?;
        let parts = token_parts(&words);
        (parts.len() == fillings.len() + 1).then(|| {
            let mut filled = parts[0].to_vec();
            for (filling, part) in fillings.iter().zip(&parts[1..]) {
                filled.extend(filling);
                filled.extend(*part);
            }
            filled
        })
    }
}

/// `words` cut at their tokens (`<CUSTOM0>`, `<FullName>`): the words
/// before the first token, between each two and after the last.
fn token_parts(words: &[u8]) -> Vec<&[u8]> {
    let mut parts = Vec::new();
    let mut from = 0;
    let mut at = 0;
    while at < words.len() {
        let token = (words[at] == b'<')
            .then(|| {
                let length = words[at + 1..]
                    .iter()
                    .position(|byte| !(byte.is_ascii_alphanumeric() || *byte == b'_'))?;
                (length > 0 && words.get(at + 1 + length) == Some(&b'>')).then_some(length + 2)
            })
            .flatten();
        match token {
            Some(length) => {
                parts.push(&words[from..at]);
                at += length;
                from = at;
            }
            None => at += 1,
        }
    }
    parts.push(&words[from..]);
    parts
}

/// An install's talk table, with its lines looked up by their words.
pub struct TalkTable<'a> {
    tlk: &'a fmt_odyssey::tlk::Tlk,
    code_page: fmt_odyssey::text::CodePage,
    lines: std::collections::HashMap<Vec<u8>, TalkLine>,
    /// The lines with tokens, by their words before the first token, with
    /// their words after each token.
    filled: std::collections::HashMap<Vec<u8>, Vec<Filled>>,
}

type Filled = (TalkLine, Vec<Vec<u8>>);

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
        // A line is found by its words before its first token, so a line
        // that starts with a token is not found.
        let mut filled: std::collections::HashMap<_, Vec<_>> = std::collections::HashMap::new();
        for &(strref, text) in &texts {
            let shown = fmt_odyssey::tlk::unbraced(text);
            for (words, unbraced) in [(text, false), (shown.as_slice(), true)] {
                let parts = token_parts(words);
                if parts.len() > 1 && words_like(parts[0]) && (!unbraced || shown != text) {
                    let after = parts[1..].iter().map(|part| part.to_vec()).collect();
                    filled
                        .entry(parts[0].to_vec())
                        .or_default()
                        .push((TalkLine { strref, unbraced }, after));
                }
            }
        }
        TalkTable {
            tlk,
            code_page: fmt_odyssey::text::CodePage::of(tlk),
            lines,
            filled,
        }
    }
}

impl Talk for TalkTable<'_> {
    fn line_of(&self, words: &[u8]) -> Option<TalkLine> {
        self.lines.get(words).copied()
    }

    fn filled_at(&self, words: &[u8]) -> Option<(usize, TalkLine, Vec<Vec<u8>>)> {
        let mut found: Option<(usize, TalkLine, Vec<Vec<u8>>)> = None;
        for before in MIN_WORDS..=words.len() {
            for (line, after) in self.filled.get(&words[..before]).into_iter().flatten() {
                if let Some((end, fillings)) = fill(words, before, after)
                    && found.as_ref().is_none_or(|(longest, ..)| end > *longest)
                {
                    found = Some((end, *line, fillings));
                }
            }
        }
        found
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

/// Where a line's words after each token, `after`, stand in `words` from
/// `at`: the end of the last, and what fills each token. A token is filled
/// up to the first place its next words stand, or, for a token that ends
/// the line, up to the end of the word. A token is never filled with
/// nothing.
fn fill(words: &[u8], mut at: usize, after: &[Vec<u8>]) -> Option<(usize, Vec<Vec<u8>>)> {
    let mut fillings = Vec::new();
    for next in after {
        let rest = &words[at..];
        let length = if next.is_empty() {
            rest.iter()
                .position(u8::is_ascii_whitespace)
                .unwrap_or(rest.len())
        } else {
            (1..rest.len()).find(|&length| rest[length..].starts_with(next))?
        };
        if length == 0 {
            return None;
        }
        fillings.push(rest[..length].to_vec());
        at += length + next.len();
    }
    Some((at, fillings))
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

    fn filled_words(&self, line: TalkLine, fillings: &[Vec<u8>]) -> Option<Vec<u8>> {
        let mut words = self.words_of(line)?;
        for filling in fillings {
            words.extend(b" with ");
            words.extend(filling);
        }
        Some(words)
    }

    fn code_page(&self) -> fmt_odyssey::text::CodePage {
        fmt_odyssey::text::CodePage::Western
    }
}

/// The fewest bytes a run of words has for a capture to name its line.
/// Shorter runs, such as a slot's key "1", are left as they are.
const MIN_WORDS: usize = 3;

/// The key a capture file names a line of the talk table by.
const TALK_KEY: &str = "$talk";

/// The key of what fills each token of a line (`<CUSTOM0>`), as text.
const FILLED_KEY: &str = "filled";

/// The key of the parts of words joined from lines of the talk table and
/// kjk's own words, such as a line of the conversation log ("Trask: ..."),
/// a reply ("1. ...") or a status summary's row ("... Received: 200").
const PARTS_KEY: &str = "$parts";

/// The fields that hold the game's words as text rather than as bytes: a
/// save's area name and its name, and the lines of the logs. Other text,
/// such as a key's name or what a key does, is kjk's.
const TEXT_FIELDS: [&str; 5] = ["area", "name", "dialog_log", "combat_log", "feedback"];

/// The view model's field written as it is: the game's settings, kjk's
/// own keys and values.
const PLAIN: &str = "settings";

/// A piece of a run of words: a line of the talk table, or words that are
/// in no line.
#[derive(Debug, Clone, PartialEq)]
enum Piece {
    /// A line, and what fills each of its tokens.
    Line(TalkLine, Vec<Vec<u8>>),
    Words(Vec<u8>),
}

/// `words` as the lines of `talk` it holds and the words between them. A
/// line is found where it stands between the ends of words, longest
/// first from each place, so "hit" is not found inside "white".
fn pieces(words: &[u8], talk: &dyn Talk) -> Vec<Piece> {
    if let Some(line) = Some(words)
        .filter(|words| words_like(words))
        .and_then(|words| talk.line_of(words))
    {
        return vec![Piece::Line(line, Vec::new())];
    }
    let word = |at: usize| words.get(at).is_some_and(u8::is_ascii_alphanumeric);
    let starts = |at: usize| at == 0 || !word(at - 1);
    let ends = |at: usize| at == words.len() || !word(at);
    let mut pieces = Vec::new();
    let mut between = Vec::new();
    let mut at = 0;
    while at < words.len() {
        let whole = starts(at)
            .then(|| {
                ((at + MIN_WORDS..=words.len()).rev())
                    .filter(|&end| ends(end) && words_like(&words[at..end]))
                    .find_map(|end| Some((end, talk.line_of(&words[at..end])?, Vec::new())))
            })
            .flatten();
        let filled = starts(at)
            .then(|| talk.filled_at(&words[at..]))
            .flatten()
            .map(|(length, line, fillings)| (at + length, line, fillings));
        let found = match (whole, filled) {
            (Some(whole), Some(filled)) => Some(if filled.0 > whole.0 { filled } else { whole }),
            (whole, filled) => whole.or(filled),
        };
        match found {
            Some((end, line, fillings)) => {
                if !between.is_empty() {
                    pieces.push(Piece::Words(std::mem::take(&mut between)));
                }
                pieces.push(Piece::Line(line, fillings));
                at = end;
            }
            None => {
                between.push(words[at]);
                at += 1;
            }
        }
    }
    if !between.is_empty() {
        pieces.push(Piece::Words(between));
    }
    pieces
}

/// Whether `bytes` may be words of the game's: at least `MIN_WORDS` of them
/// and a letter among them. A pair of numbers, such as a character's
/// health and its most, is not.
fn words_like(bytes: &[u8]) -> bool {
    bytes.len() >= MIN_WORDS && bytes.iter().any(u8::is_ascii_alphabetic)
}

/// The words `value` holds, and whether it holds them as text: a list of
/// numbers from 0 to 255 in the game's code page, or, in a field of
/// `TEXT_FIELDS` (`text_field`), text. A list of other numbers, or text
/// with a letter outside the code page, holds none.
fn words_in(
    value: &serde_json::Value,
    text_field: bool,
    code_page: fmt_odyssey::text::CodePage,
) -> Option<(Vec<u8>, bool)> {
    use serde_json::Value;
    match value {
        Value::Array(items) => (items.iter())
            .map(|item| item.as_u64().and_then(|byte| u8::try_from(byte).ok()))
            .collect::<Option<Vec<u8>>>()
            .map(|bytes| (bytes, false)),
        Value::String(text) if text_field => (text.chars())
            .map(|ch| code_page.byte(ch))
            .collect::<Option<Vec<u8>>>()
            .map(|bytes| (bytes, true)),
        _ => None,
    }
    .filter(|(bytes, _)| words_like(bytes))
}

/// Each run of words in `value` that holds lines of `talk` written as
/// those lines and the words between them.
fn name_lines(value: &mut serde_json::Value, text_field: bool, talk: &dyn Talk) {
    use serde_json::Value;
    if let Some((words, text)) = words_in(value, text_field, talk.code_page()) {
        let pieces = pieces(&words, talk);
        if pieces.iter().any(|piece| matches!(piece, Piece::Line(..))) {
            *value = named(&pieces, text, talk);
            return;
        }
    }
    match value {
        Value::Array(items) => {
            (items.iter_mut()).for_each(|item| name_lines(item, text_field, talk))
        }
        Value::Object(fields) => (fields.iter_mut())
            .filter(|(key, _)| *key != PLAIN)
            .for_each(|(key, field)| name_lines(field, TEXT_FIELDS.contains(&key.as_str()), talk)),
        _ => {}
    }
}

/// `pieces` as a capture file names them: one line as `{"$talk": 123}`,
/// several pieces as `{"$parts": [...]}` with the words between lines as
/// text. `text` says that they stand for text rather than for bytes.
fn named(pieces: &[Piece], text: bool, talk: &dyn Talk) -> serde_json::Value {
    let line_named = |line: &TalkLine, fillings: &[Vec<u8>]| {
        let mut named = serde_json::Map::new();
        named.insert(TALK_KEY.into(), line.strref.into());
        if line.unbraced {
            named.insert("unbraced".into(), true.into());
        }
        if !fillings.is_empty() {
            let fillings = (fillings.iter())
                .map(|filling| talk.code_page().readable(filling).into())
                .collect();
            named.insert(FILLED_KEY.into(), serde_json::Value::Array(fillings));
        }
        named
    };
    let mut named = match pieces {
        [Piece::Line(line, fillings)] => line_named(line, fillings),
        _ => {
            let parts = (pieces.iter())
                .map(|piece| match piece {
                    Piece::Line(line, fillings) => {
                        serde_json::Value::Object(line_named(line, fillings))
                    }
                    Piece::Words(words) => talk.code_page().readable(words).into(),
                })
                .collect();
            serde_json::Map::from_iter([(PARTS_KEY.to_string(), serde_json::Value::Array(parts))])
        }
    };
    if text {
        named.insert("text".into(), true.into());
    }
    serde_json::Value::Object(named)
}

/// The words of a line `named` names, read from `talk`.
fn line_words(
    named: &serde_json::Map<String, serde_json::Value>,
    talk: &dyn Talk,
) -> Result<Vec<u8>, String> {
    use serde_json::Value;
    let strref = named
        .get(TALK_KEY)
        .and_then(Value::as_u64)
        .ok_or(format!("{named:?}: not a line"))?;
    let line = TalkLine {
        strref: u32::try_from(strref).map_err(|_| format!("{strref}: not a line"))?,
        unbraced: named
            .get("unbraced")
            .and_then(Value::as_bool)
            .unwrap_or(false),
    };
    let missing =
        || format!("line {strref} is not in this install's talk table, or has other tokens");
    match named.get(FILLED_KEY) {
        None => talk.words_of(line).ok_or_else(missing),
        Some(fillings) => {
            let fillings = (fillings.as_array().into_iter().flatten())
                .map(|filling| bytes_of(filling, talk))
                .collect::<Result<Vec<_>, _>>()?;
            talk.filled_words(line, &fillings).ok_or_else(missing)
        }
    }
}

/// The words of `text`, written as text in the game's code page.
fn bytes_of(text: &serde_json::Value, talk: &dyn Talk) -> Result<Vec<u8>, String> {
    let text = text.as_str().ok_or(format!("{text}: not text"))?;
    (text.chars())
        .map(|ch| talk.code_page().byte(ch))
        .collect::<Option<Vec<u8>>>()
        .ok_or(format!("{text:?} is not in the game's code page"))
}

/// Each line of the talk table `value` names, alone or among parts,
/// swapped back for its words.
fn words_for_lines(value: &mut serde_json::Value, talk: &dyn Talk) -> Result<(), String> {
    use serde_json::Value;
    match value {
        Value::Object(fields)
            if fields.contains_key(TALK_KEY) || fields.contains_key(PARTS_KEY) =>
        {
            let words = match fields.get(PARTS_KEY) {
                None => line_words(fields, talk)?,
                Some(parts) => {
                    let parts = parts
                        .as_array()
                        .ok_or(format!("{PARTS_KEY} is not a list"))?;
                    let mut words = Vec::new();
                    for part in parts {
                        match part {
                            Value::Object(line) => words.extend(line_words(line, talk)?),
                            Value::String(_) => words.extend(bytes_of(part, talk)?),
                            _ => return Err(format!("{part}: not a line or words")),
                        }
                    }
                    words
                }
            };
            *value = if fields.get("text").and_then(Value::as_bool).unwrap_or(false) {
                Value::String(talk.code_page().readable(&words))
            } else {
                Value::Array(words.into_iter().map(Value::from).collect())
            };
            Ok(())
        }
        Value::Object(fields) => fields
            .values_mut()
            .try_for_each(|field| words_for_lines(field, talk)),
        Value::Array(items) => items
            .iter_mut()
            .try_for_each(|item| words_for_lines(item, talk)),
        _ => Ok(()),
    }
}

/// Where the text of a capture file, `text`, holds words of `talk`'s
/// lines rather than naming those lines, each as the field's path and its
/// words. A capture holds none: this is the check that it does, against
/// the talk table of each game installed.
pub fn game_words(text: &str, talk: &dyn Talk) -> Result<Vec<String>, String> {
    fn walk(
        value: &serde_json::Value,
        (path, text_field): (&str, bool),
        talk: &dyn Talk,
        found: &mut Vec<String>,
    ) {
        use serde_json::Value;
        let check = |value: &Value, path: &str, text_field: bool| {
            if let Some((words, _)) = words_in(value, text_field, talk.code_page())
                && (pieces(&words, talk).iter()).any(|piece| matches!(piece, Piece::Line(..)))
            {
                Some(format!("{path}: {:?}", talk.code_page().readable(&words)))
            } else {
                None
            }
        };
        match value {
            Value::Object(fields) if fields.contains_key(TALK_KEY) => {
                let fillings = fields.get(FILLED_KEY).and_then(Value::as_array);
                for (at, filling) in fillings.into_iter().flatten().enumerate() {
                    found.extend(check(filling, &format!("{path}.{FILLED_KEY}[{at}]"), true));
                }
            }
            Value::Object(fields) if fields.contains_key(PARTS_KEY) => {
                for (at, part) in (fields[PARTS_KEY].as_array().into_iter().flatten()).enumerate() {
                    let path = format!("{path}.{PARTS_KEY}[{at}]");
                    match part {
                        Value::String(_) => {
                            found.extend(check(part, &path, true));
                        }
                        _ => walk(part, (&path, true), talk, found),
                    }
                }
            }
            Value::Object(fields) => {
                for (key, field) in fields.iter().filter(|(key, _)| *key != PLAIN) {
                    let text_field = TEXT_FIELDS.contains(&key.as_str());
                    walk(field, (&format!("{path}.{key}"), text_field), talk, found);
                }
            }
            Value::Array(items) => {
                if let Some(words) = check(value, path, text_field) {
                    found.push(words);
                } else {
                    for (at, item) in items.iter().enumerate() {
                        walk(item, (&format!("{path}[{at}]"), text_field), talk, found);
                    }
                }
            }
            Value::String(_) => {
                found.extend(check(value, path, text_field));
            }
            _ => {}
        }
    }
    let value: serde_json::Value = serde_json::from_str(text).map_err(|error| error.to_string())?;
    let mut found = Vec::new();
    walk(&value, ("", false), talk, &mut found);
    Ok(found)
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
    let text = capture.text(talk)?;
    std::fs::create_dir_all(folder).map_err(|error| format!("{}: {error}", folder.display()))?;
    let path = capture_file(folder, name);
    std::fs::write(&path, text).map_err(|error| format!("{}: {error}", path.display()))?;
    Ok(path)
}
