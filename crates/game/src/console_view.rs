//! The console: its overlay, and its lines run against the table of
//! commands (`crates/commands`).
//!
//! A `--cmds` script hands every line its own parser does not know to
//! `console::LineRequests` (`console::Command::Line`), and so does the
//! overlay for each line the player enters. This plugin fills the table's
//! `Facts` from what the game shows, runs each line, and carries out what
//! each command asks for. An order goes to the simulation through
//! `loot::Outgoing`, the list the arena's keys and the Debug Mode character
//! page write to. A line whose command needs a loaded game waits while a
//! module loads, so a line that a script gives early runs once the world
//! is up.
//!
//! Every command and its result go to `kjk.log` under the log target
//! `console`, and to the console's log, which a dump shows as its `console`
//! fact and a bug report keeps.
//!
//! The overlay (`Overlay`, drawn by `ui::screens::console`) opens and
//! closes with the console's key (`settings::Action::Console`) once the
//! Console option is on. While it is open it takes every key, the mouse
//! wheel and the mouse buttons, so no key acts in the game and the
//! character stands still. The screens count it as open (`UiOpen`), so the
//! cursor is free and the host counts the player as busy. It is not a
//! screen on the stack: it opens over the HUD, a menu or a conversation and
//! leaves them as they are. A headless run drives it with `ui console …`
//! (`Overlay::script`).
//!
//! The host's game also keeps the simulation's cheat rule
//! (`sim::cheats::Rule`) as the player chose it (`send_rule`). A game played
//! alone with a command script, or with the Console option on, plays under
//! the rule Host only. Any other game played alone plays under the rule
//! Off. A hosted game plays under the lobby's Cheats row (the setting
//! `game.cheats`, Host only until the host changes it).
//!
//! The overlay opens at the main menu and in the co-op lobby too. There the
//! table's `warp`, `sandbox`, `load`, `host` and `join` start a game through
//! the menu's own session intents (`ui::intent::SessionIntent`). A game
//! that `warp` started at the main menu is marked as cheated once it is up
//! (`WarpMark`).

use std::collections::{BTreeMap, VecDeque};
use std::path::PathBuf;
use std::sync::Arc;

use bevy::input::ButtonState;
use bevy::input::keyboard::{Key, KeyboardInput};
use bevy::input::mouse::MouseWheel;
use bevy::prelude::*;
use bevy::window::PrimaryWindow;
use commands::{Effect, Facts};
use sim::action::{Action, ArenaOrder, CheatOrder};
use sim::cheats::Rule;
use ui::edit::EditBox;
use ui::intent::SessionIntent;

use crate::sim_host::{Loading, OwnSeat, PresentedSnapshot, SceneSwap};

#[cfg(feature = "probes")]
pub mod probes;

/// The most lines the console's log keeps.
const LOG_LINES: usize = 200;
/// The most lines the history keeps, and its file in kjk's user folder.
const HISTORY_LINES: usize = 200;
const HISTORY_FILE: &str = "console-history.txt";
/// The longest line the overlay takes, and the most lines a paste runs.
const LONGEST_LINE: usize = 256;
const MOST_PASTED: usize = 64;
/// How many log lines Page Up and Page Down scroll, and one wheel notch.
const PAGE: usize = 8;
const NOTCH: usize = 3;
/// How long the caret shows, then hides (seconds).
const BLINK: f32 = 0.5;

pub struct ConsoleViewPlugin;

impl Plugin for ConsoleViewPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<ConsoleLog>()
            .init_resource::<WarpMark>()
            .init_resource::<WarpLater>()
            .init_resource::<Scripted>()
            .init_resource::<Overlay>()
            .add_systems(
                PreUpdate,
                (capture, crate::reshade::drop_mouse.run_if(overlay_open))
                    .chain()
                    .after(bevy::input::InputSystems)
                    .before(crate::swallow::hold_up),
            )
            .add_systems(
                Update,
                (
                    send_rule,
                    dump_cheats,
                    warp_later,
                    (run_lines, show)
                        .chain()
                        .after(console::ConsoleQueue)
                        .before(crate::ui_view::Stepped),
                ),
            );
    }
}

/// A line of the console's log, and whether it tells of a mistake.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LogLine {
    pub text: String,
    pub error: bool,
}

/// The console's log: each command echoed with `> ` in front, then what it
/// printed, the last `LOG_LINES` lines.
#[derive(Resource, Default, Debug)]
pub struct ConsoleLog(pub VecDeque<LogLine>);

impl ConsoleLog {
    fn push(&mut self, text: String, error: bool) {
        if self.0.len() == LOG_LINES {
            self.0.pop_front();
        }
        self.0.push_back(LogLine { text, error });
    }

    /// The lines as text, oldest first.
    pub fn lines(&self) -> Vec<String> {
        self.0.iter().map(|line| line.text.clone()).collect()
    }

    /// What the simulation answered to this player's cheat orders, among
    /// the words it told this player (`loot::Told`). A refusal is a
    /// mistake.
    pub(crate) fn echo(&mut self, told: &[String]) {
        for text in told {
            if let Some(outcome) = crate::loot::cheat_outcome(text) {
                self.push(text.clone(), outcome != sim::cheats::Outcome::Done);
            }
        }
    }
}

/// The run has a command script (`--cmds`).
#[derive(Resource, Default, Debug, Clone, Copy)]
pub struct Scripted(pub bool);

/// The settings key of the console's switch (the Console option).
pub(crate) const CONSOLE: &str = ui::screens::options::CONSOLE;

/// How many frames the host waits for its rule to show in the snapshots
/// before it sends the rule again.
const RULE_RESEND_FRAMES: u32 = 120;

/// Whether the console may be used in this run: the Console option is on,
/// `KJK_CONSOLE` is 1, or the run has a command script.
fn enabled(scripted: bool, settings: Option<&crate::settings::Settings>) -> bool {
    scripted
        || settings.is_some_and(|settings| settings.on(CONSOLE))
        || std::env::var_os("KJK_CONSOLE").is_some_and(|value| value == "1")
}

/// The cheat rule a game plays under, `alone` or hosted (see the module
/// documentation).
fn wanted_rule(scripted: bool, settings: Option<&crate::settings::Settings>, alone: bool) -> Rule {
    match (alone, enabled(scripted, settings)) {
        (false, _) => coop_rule(settings),
        (true, true) => Rule::Host,
        (true, false) => Rule::Off,
    }
}

/// The settings values of the lobby's Cheats row
/// (`ui::screens::options::CHEAT_RULES`), by rule.
const RULE_KEYS: [(Rule, &str); 3] = [(Rule::Off, "off"), (Rule::Host, "host"), (Rule::All, "all")];

/// The rule a value of the Cheats row names. Any other value is Host only,
/// the row's default.
pub(crate) fn rule_named(key: &str) -> Rule {
    (RULE_KEYS.iter())
        .find(|(_, of)| *of == key)
        .map_or(Rule::Host, |(rule, _)| *rule)
}

/// The Cheats row's value for `rule`.
pub(crate) fn rule_key(rule: Rule) -> &'static str {
    (RULE_KEYS.iter())
        .find(|(of, _)| *of == rule)
        .map_or(ui::screens::options::CHEATS_DEFAULT, |(_, key)| key)
}

/// The rule a hosted game plays under: the lobby's Cheats row.
pub(crate) fn coop_rule(settings: Option<&crate::settings::Settings>) -> Rule {
    rule_named(
        settings
            .and_then(|settings| settings.value(ui::screens::options::CHEATS))
            .unwrap_or(ui::screens::options::CHEATS_DEFAULT),
    )
}

/// A game `warp` started at the main menu, not yet marked as cheated: once
/// the game is up and plays under its rule, the host's game sends the
/// order that marks it (`CheatOrder::Warped`).
#[derive(Resource, Default, Debug)]
pub struct WarpMark(pub bool);

/// The most frames a `warp` in a game waits for its mark to show before
/// its module loads anyway (an arena level is never marked).
const WARP_WAIT_FRAMES: u32 = 30;

/// A `warp` in a game: the module and arrival it loads, and the frames
/// left to wait. The order that marks the game goes first. An install that
/// starts in the same frame would drop it, so the module loads once the
/// mark shows, or after `WARP_WAIT_FRAMES` frames.
#[derive(Resource, Default, Debug)]
pub struct WarpLater(Option<(String, Option<String>, u32)>);

fn warp_later(
    mut later: ResMut<WarpLater>,
    presented: Option<Res<PresentedSnapshot>>,
    mut module: ResMut<console::ModuleRequest>,
) {
    let Some((_, _, frames)) = later.0.as_mut() else {
        return;
    };
    let marked = presented.is_some_and(|presented| presented.0.current().cheats.used);
    *frames = frames.saturating_sub(1);
    if (marked || *frames == 0)
        && let Some((name, arrival, _)) = later.0.take()
    {
        module.pending = Some((name, arrival));
    }
}

/// What `send_rule` reads of the session: the snapshot shown, this
/// player's seat, and whether the world runs here.
type Seated<'w> = (
    Option<Res<'w, PresentedSnapshot>>,
    Option<Res<'w, OwnSeat>>,
    Option<Res<'w, crate::sim_host::WorldHere>>,
);

/// The first seat's game sends `CheatOrder::Policy` whenever the
/// simulation's rule is not the one this run plays under: when a session
/// starts, after a save from another rule loads, and when the setting
/// changes. An order still on its way is not sent again for
/// `RULE_RESEND_FRAMES` frames. A guest's game never sends it.
fn send_rule(
    scripted: Res<Scripted>,
    settings: Option<Res<crate::settings::Settings>>,
    (presented, own, here): Seated,
    mut outgoing: Option<ResMut<crate::loot::Outgoing>>,
    mut mark: ResMut<WarpMark>,
    mut waiting: Local<u32>,
) {
    let (Some(presented), Some(own), Some(outgoing), Some(here)) =
        (presented, own, outgoing.as_mut(), here)
    else {
        return;
    };
    if own.0 != sim::Seat(0) {
        return;
    }
    let wanted = wanted_rule(scripted.0, settings.as_deref(), here.alone);
    if presented.0.current().cheats.rule == wanted {
        *waiting = 0;
        // The rule is in force, so the warp's mark is carried out.
        if std::mem::take(&mut mark.0) {
            outgoing.0.push(Action::Cheat(CheatOrder::Warped));
        }
        return;
    }
    if *waiting > 0 {
        *waiting -= 1;
        return;
    }
    info!(target: "console", "cheat rule: {wanted:?}");
    outgoing.0.push(Action::Cheat(CheatOrder::Policy(wanted)));
    *waiting = RULE_RESEND_FRAMES;
}

/// The shown game's cheat rule and mark, for a run's dumps (`cheats`).
fn dump_cheats(
    presented: Option<Res<PresentedSnapshot>>,
    mut dump: ResMut<console::DumpFacts>,
    mut shown: Local<Option<sim::cheats::Cheats>>,
) {
    let Some(presented) = presented else {
        return;
    };
    let cheats = presented.0.current().cheats;
    if *shown == Some(cheats) {
        return;
    }
    *shown = Some(cheats);
    dump.0.insert(
        "cheats".into(),
        serde_json::json!({ "rule": rule_key(cheats.rule), "used": cheats.used }),
    );
}

/// A key the overlay takes, already read from the keyboard.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Press {
    Text(String),
    Backspace,
    Delete,
    Left,
    Right,
    Home,
    End,
    Up,
    Down,
    /// Tab, and Tab with Shift held.
    Tab,
    BackTab,
    Enter,
    PageUp,
    PageDown,
    /// Escape: Tab's choices closed, or the console when none are shown.
    Escape,
    /// The console's key, which closes it.
    Close,
    /// Ctrl+V (Cmd+V on a Mac), with what the clipboard holds.
    Paste(String),
    /// Ctrl+C (Cmd+C): the line being typed copied.
    Copy,
    /// Ctrl+L: the log emptied.
    ClearLog,
}

impl Press {
    /// A press by its name in `ui console key NAME`.
    fn named(name: &str) -> Option<Press> {
        Some(match name.to_ascii_lowercase().as_str() {
            "backspace" => Press::Backspace,
            "delete" => Press::Delete,
            "left" => Press::Left,
            "right" => Press::Right,
            "home" => Press::Home,
            "end" => Press::End,
            "up" => Press::Up,
            "down" => Press::Down,
            "tab" => Press::Tab,
            "backtab" => Press::BackTab,
            "enter" => Press::Enter,
            "pageup" => Press::PageUp,
            "pagedown" => Press::PageDown,
            "escape" => Press::Escape,
            "copy" => Press::Copy,
            "clear" => Press::ClearLog,
            // A script's typed words lose their trailing space.
            "space" => Press::Text(" ".into()),
            _ => return None,
        })
    }
}

/// What a press asks of the rest of the game.
#[derive(Debug, Default, PartialEq, Eq)]
pub(crate) struct Asked {
    /// Lines to run, in order.
    pub lines: Vec<String>,
    /// Text for the clipboard.
    pub copy: Option<String>,
    pub clear_log: bool,
}

/// The console's overlay: open or not, the line being typed, the history,
/// the log's scroll, and the suggestions for the word being typed.
///
/// The suggestions show under the line while the player types, up to
/// `ui::screens::console::CHOICES` at once, from the same lists Tab
/// completes from. Enter runs the line as typed. A suggestion is taken only
/// when the player picks it: Tab, Up or Down choose one, and Enter or a
/// click takes it.
#[derive(Resource, Debug)]
pub struct Overlay {
    open: bool,
    edit: EditBox,
    /// Earlier lines, oldest first, and the one Up and Down are on (none
    /// while the player is on the line being typed, kept in `draft`).
    history: Vec<String>,
    browsing: Option<usize>,
    draft: String,
    /// The history file was read.
    read: bool,
    /// How many log lines are scrolled away under the log's foot.
    scrolled: usize,
    /// The suggestions for the word before the caret, and the one the
    /// player picked, if any.
    candidates: Vec<String>,
    chosen: Option<usize>,
    /// The arguments still to type (`commands::complete::hint`), drawn
    /// after the line.
    hint: String,
    /// The line changed: `run_lines` makes the suggestions and the hint
    /// again, which need the table's facts. `listing` is false when the
    /// change came from the history, which shows no suggestions.
    refresh: bool,
    listing: bool,
    /// Tab was pressed before the suggestions were made again: once they
    /// are, a single one is taken and the first of several is picked.
    tab: bool,
    /// The choices are item templates, shown with the game's item names.
    items: bool,
    /// Where the pointer is, and whether the main button went down on it
    /// this frame (window pixels).
    pointer: Option<[f32; 2]>,
    clicked: bool,
    clock: f32,
    /// Modifier keys tracked from `KeyboardInput` events while the console is
    /// open (because `capture` calls `keys.reset_all()` each frame).
    ctrl: bool,
    shift: bool,
    alt: bool,
    super_key: bool,
}

impl Default for Overlay {
    fn default() -> Overlay {
        Overlay {
            open: false,
            edit: EditBox::new(LONGEST_LINE),
            history: Vec::new(),
            browsing: None,
            draft: String::new(),
            read: false,
            scrolled: 0,
            candidates: Vec::new(),
            chosen: None,
            hint: String::new(),
            refresh: false,
            listing: false,
            tab: false,
            items: false,
            pointer: None,
            clicked: false,
            clock: 0.0,
            ctrl: false,
            shift: false,
            alt: false,
            super_key: false,
        }
    }
}

impl Overlay {
    pub fn is_open(&self) -> bool {
        self.open
    }

    fn set_open(&mut self, open: bool) {
        if self.open != open {
            self.open = open;
            self.clock = 0.0;
            self.close_choices();
            if !open {
                self.ctrl = false;
                self.shift = false;
                self.alt = false;
                self.super_key = false;
            }
        }
    }

    fn close_choices(&mut self) {
        self.candidates.clear();
        self.chosen = None;
        self.tab = false;
    }

    /// Suggestions show and are up to date.
    fn choosing(&self) -> bool {
        !self.candidates.is_empty() && !self.refresh
    }

    /// The line changed: its suggestions and hint are made again, and no
    /// suggestion is picked.
    fn changed(&mut self, listing: bool) {
        self.refresh = true;
        self.listing = listing;
        self.chosen = None;
    }

    /// Whether `run_lines` has suggestions or a hint to make.
    pub(crate) fn wants_facts(&self) -> bool {
        self.refresh || self.tab
    }

    /// The next suggestion picked (`back`: the one before), round the list.
    /// With none picked, the first (`back`: the last).
    fn step_choice(&mut self, back: bool) {
        let count = self.candidates.len();
        self.chosen = Some(match (self.chosen, back) {
            (None, false) => 0,
            (None, true) => count - 1,
            (Some(at), false) => (at + 1) % count,
            (Some(at), true) => (at + count - 1) % count,
        });
    }

    /// One key pressed while the console is open, with `log` lines in the
    /// log; what it asks of the game.
    pub(crate) fn press(&mut self, press: Press, log: usize) -> Asked {
        let mut asked = Asked::default();
        let page = encoding();
        match press {
            Press::Tab if self.choosing() && self.candidates.len() == 1 => self.pick(0),
            Press::Tab | Press::BackTab | Press::Up | Press::Down if self.choosing() => {
                self.step_choice(matches!(press, Press::BackTab | Press::Up));
            }
            Press::Enter if self.chosen.is_some() => self.pick(self.chosen.unwrap_or(0)),
            Press::Escape if !self.candidates.is_empty() => self.close_choices(),
            Press::Tab | Press::BackTab => self.tab = true,
            Press::Escape | Press::Close => self.set_open(false),
            Press::Enter => {
                let line = self.edit.as_str();
                self.edit.clear();
                self.enter(&line, &mut asked);
            }
            Press::Up => self.browse(-1),
            Press::Down => self.browse(1),
            Press::PageUp => self.scrolled = (self.scrolled + PAGE).min(log),
            Press::PageDown => self.scrolled = self.scrolled.saturating_sub(PAGE),
            Press::Copy => asked.copy = Some(self.edit.as_str()),
            Press::ClearLog => {
                asked.clear_log = true;
                self.scrolled = 0;
            }
            Press::Paste(text) => {
                let text = text.replace("\r\n", "\n").replace('\r', "\n");
                let lines: Vec<&str> = text
                    .split('\n')
                    .filter(|line| !line.trim().is_empty())
                    .take(MOST_PASTED)
                    .collect();
                match lines.as_slice() {
                    [] => {}
                    [one] => {
                        self.edit.type_text(one, page);
                        self.changed(true);
                    }
                    many => {
                        for line in many {
                            self.edit.type_text(line, page);
                            let line = self.edit.as_str();
                            self.edit.clear();
                            self.enter(&line, &mut asked);
                        }
                    }
                }
            }
            edit => {
                match edit {
                    Press::Text(text) => self.edit.type_text(&text, page),
                    Press::Backspace => self.edit.backspace(),
                    Press::Delete => self.edit.delete(),
                    Press::Left => self.edit.step(-1),
                    Press::Right => self.edit.step(1),
                    Press::Home => self.edit.home(),
                    Press::End => self.edit.end(),
                    _ => {}
                }
                // A change to the line makes its suggestions again.
                self.browsing = None;
                self.changed(true);
            }
        }
        asked
    }

    /// `line` entered: kept in the history unless it repeats the last one,
    /// and asked to run.
    fn enter(&mut self, line: &str, asked: &mut Asked) {
        self.browsing = None;
        self.draft.clear();
        self.scrolled = 0;
        self.close_choices();
        if line.trim().is_empty() {
            return;
        }
        if self.history.last().map(String::as_str) != Some(line) {
            self.history.push(line.to_string());
            let extra = self.history.len().saturating_sub(HISTORY_LINES);
            self.history.drain(..extra);
        }
        asked.lines.push(line.to_string());
    }

    /// Up (`by` -1) or Down (1) through the history. Down past the newest
    /// line gives back the line that was being typed.
    fn browse(&mut self, by: isize) {
        if self.history.is_empty() {
            return;
        }
        let newest = self.history.len() - 1;
        let at = match (self.browsing, by < 0) {
            (None, true) => {
                self.draft = self.edit.as_str();
                Some(newest)
            }
            (None, false) => None,
            (Some(at), true) => Some(at.saturating_sub(1)),
            (Some(at), false) => (at < newest).then_some(at + 1),
        };
        self.browsing = at;
        let line = at.map_or_else(|| self.draft.clone(), |at| self.history[at].clone());
        self.edit.set(&line);
        self.close_choices();
        self.changed(false);
    }

    /// The word before the caret replaced by suggestion `index`, a space
    /// after it, and the next word's suggestions made.
    fn pick(&mut self, index: usize) {
        let Some(chosen) = self.candidates.get(index).cloned() else {
            return;
        };
        let caret = self.edit.caret().min(self.edit.text.len());
        let (before_bytes, after_bytes) = self.edit.text.split_at(caret);
        let before = fmt_odyssey::code_page(before_bytes);
        let after = fmt_odyssey::code_page(after_bytes);
        let start = before.rfind(' ').map_or(0, |at| at + 1);
        let after = after.trim_start_matches(' ');
        let picked = format!("{}{chosen} ", &before[..start]);
        let caret_pos = fmt_odyssey::code_page_bytes(&picked).len();
        self.edit.set(&format!("{picked}{after}"));
        self.edit.home();
        self.edit.step(caret_pos as isize);
        self.close_choices();
        self.changed(true);
    }

    /// The suggestions for the word before the caret and the hint, with
    /// `facts`. An empty line, and a line from the history, shows no
    /// suggestions unless Tab asked for them. After Tab a single suggestion
    /// is taken at once, and the first of several is picked.
    pub(crate) fn suggest(&mut self, facts: &Facts) {
        let tab = std::mem::take(&mut self.tab);
        self.refresh = false;
        let line = self.edit.as_str();
        let caret = self.edit.caret().min(self.edit.text.len());
        let before = fmt_odyssey::code_page(&self.edit.text[..caret]);
        let listed = tab || (self.listing && !before.trim().is_empty());
        let mut found = match listed {
            true => commands::complete::candidates(&before, facts),
            false => Vec::new(),
        };
        // A word typed in full needs no suggestion of itself alone.
        let typing = before.rsplit(' ').next().unwrap_or("");
        if !tab && found.len() == 1 && found[0].eq_ignore_ascii_case(typing) {
            found.clear();
        }
        self.items = (facts.items.as_ref())
            .zip(found.first())
            .is_some_and(|(items, first)| items.contains(first));
        self.candidates = found;
        self.chosen = None;
        let hint = commands::complete::hint(&line, facts);
        self.hint = match (hint.is_empty(), line.ends_with(' ')) {
            (true, _) => String::new(),
            (false, true) => hint,
            (false, false) => format!(" {hint}"),
        };
        if tab {
            match self.candidates.len() {
                0 => {}
                1 => self.pick(0),
                _ => self.chosen = Some(0),
            }
        }
    }

    /// The history read from `file`, once, before the first line.
    fn read_history(&mut self, file: Option<PathBuf>) {
        if std::mem::replace(&mut self.read, true) {
            return;
        }
        let Some(text) = file.and_then(|file| std::fs::read_to_string(file).ok()) else {
            return;
        };
        let lines: Vec<String> = text.lines().map(str::to_string).collect();
        let first = lines.len().saturating_sub(HISTORY_LINES);
        self.history = lines[first..].to_vec();
    }

    /// The history written to `file`.
    fn write_history(&self, file: Option<PathBuf>) {
        let Some(file) = file else {
            return;
        };
        let text: String = self
            .history
            .iter()
            .map(|line| format!("{line}\n"))
            .collect();
        if let Err(error) = std::fs::write(&file, text) {
            warn!(target: "console", "the console's history: {error}");
        }
    }

    /// A headless run's `ui console …` words: `open`, `close`, `type WORDS`,
    /// `paste WORDS` (`\n` between lines), or `key NAME` (`Press::named`).
    /// Whether the words were understood.
    pub(crate) fn script(&mut self, words: &[&str], log: usize) -> Option<Asked> {
        let press = match words {
            ["open"] => {
                self.set_open(true);
                return Some(Asked::default());
            }
            ["close"] => Press::Close,
            ["type", text @ ..] => Press::Text(text.join(" ")),
            ["paste", text @ ..] => Press::Paste(text.join(" ").replace("\\n", "\n")),
            ["key", name] => Press::named(name)?,
            _ => return None,
        };
        Some(self.press(press, log))
    }

    /// What `ui::screens::console` draws, the choices shown with the game's
    /// item names by `named`.
    fn view(&self, log: &ConsoleLog, named: &BTreeMap<String, Vec<u8>>) -> ui::vm::ConsoleView {
        let bytes = |text: &str| fmt_odyssey::code_page_bytes(text);
        ui::vm::ConsoleView {
            log: (log.0.iter())
                .map(|line| ui::vm::ConsoleLine {
                    text: bytes(&line.text),
                    error: line.error,
                })
                .collect(),
            scrolled: self.scrolled,
            input: self.edit.text.clone(),
            caret: self.edit.caret(),
            blink: self.clock % (2.0 * BLINK) < BLINK,
            candidates: (self.candidates.iter())
                .map(|word| ui::vm::ConsoleChoice {
                    word: bytes(word),
                    name: if self.items {
                        named.get(word).cloned().unwrap_or_else(Vec::new)
                    } else {
                        Vec::new()
                    },
                })
                .collect(),
            chosen: self.chosen,
            hint: bytes(&self.hint),
        }
    }
}

/// How the overlay's edit box types: lines are plain ASCII, so the
/// Western code page keeps every letter a command can have.
fn encoding() -> fmt_odyssey::CodePage {
    fmt_odyssey::CodePage::Western
}

/// The history's file: beside the settings' file, which is in kjk's user
/// folder unless a run names another (`KJK_SETTINGS`).
fn history_file() -> Option<PathBuf> {
    Some(crate::settings::file()?.parent()?.join(HISTORY_FILE))
}

fn overlay_open(overlay: Res<Overlay>) -> bool {
    overlay.open
}

/// A keyboard message as the overlay reads it, with Ctrl or Cmd
/// (`command`) and Shift held.
fn press_of(event: &KeyboardInput, command: bool, shift: bool) -> Option<Press> {
    Some(match &event.logical_key {
        Key::Enter => Press::Enter,
        Key::Tab if shift => Press::BackTab,
        Key::Tab => Press::Tab,
        Key::Backspace => Press::Backspace,
        Key::Delete => Press::Delete,
        Key::ArrowLeft => Press::Left,
        Key::ArrowRight => Press::Right,
        Key::ArrowUp => Press::Up,
        Key::ArrowDown => Press::Down,
        Key::Home => Press::Home,
        Key::End => Press::End,
        Key::PageUp => Press::PageUp,
        Key::PageDown => Press::PageDown,
        Key::Escape => Press::Escape,
        _ if command => match event.key_code {
            KeyCode::KeyV => Press::Paste(String::new()),
            KeyCode::KeyC => Press::Copy,
            KeyCode::KeyL => Press::ClearLog,
            _ => return None,
        },
        _ => {
            let text = event.text.as_ref()?;
            let text: String = text.chars().filter(|letter| !letter.is_control()).collect();
            return (!text.is_empty()).then_some(Press::Text(text));
        }
    })
}

/// What `capture` reads and changes of the frame's input.
type Inputs<'w> = (
    ResMut<'w, Messages<KeyboardInput>>,
    Option<ResMut<'w, ButtonInput<KeyCode>>>,
    Option<ResMut<'w, ButtonInput<MouseButton>>>,
    Option<Res<'w, Messages<MouseWheel>>>,
);

/// What decides whether the console may open now.
type Allowed<'w> = (
    Res<'w, Scripted>,
    Option<Res<'w, crate::settings::Settings>>,
    Option<Res<'w, crate::settings::KeyMap>>,
    Option<Res<'w, crate::movie_view::Showing>>,
);

/// Runs after Bevy reads the frame's input and before play does. The
/// console's key opens the console, when it may open, and closes it. While
/// it is open, every key press of the frame goes to the overlay and is then
/// taken from the frame, so nothing later sees it: not the screens under
/// the console, not play.
fn capture(
    (mut keyboard, mut keys, buttons, wheel): Inputs,
    (scripted, settings, map, showing): Allowed,
    (mut overlay, mut log, mut lines): (
        ResMut<Overlay>,
        ResMut<ConsoleLog>,
        Option<ResMut<console::LineRequests>>,
    ),
    mut swallowed: ResMut<crate::swallow::Swallowed>,
    mut clipboard: Option<ResMut<bevy::clipboard::Clipboard>>,
    windows: Query<&Window, With<PrimaryWindow>>,
    time: Res<Time>,
) {
    overlay.clock += time.delta_secs();
    let movie = showing.is_some_and(|showing| showing.0);
    let allowed = !movie && enabled(scripted.0, settings.as_deref());
    if !allowed {
        overlay.set_open(false);
        return;
    }
    let toggle = map
        .as_ref()
        .and_then(|map| map.binding(crate::settings::Action::Console));
    if !overlay.open
        && let Some(keys) = keys.as_deref()
    {
        overlay.ctrl = keys.any_pressed([KeyCode::ControlLeft, KeyCode::ControlRight]);
        overlay.shift = keys.any_pressed([KeyCode::ShiftLeft, KeyCode::ShiftRight]);
        overlay.alt = keys.any_pressed([KeyCode::AltLeft, KeyCode::AltRight]);
        overlay.super_key = keys.any_pressed([KeyCode::SuperLeft, KeyCode::SuperRight]);
    }
    let events: Vec<KeyboardInput> = keyboard.iter_current_update_messages().cloned().collect();
    let was_open = overlay.open;
    let mut taken = false;
    for event in &events {
        let down = event.state == ButtonState::Pressed;
        match event.key_code {
            KeyCode::ControlLeft | KeyCode::ControlRight => overlay.ctrl = down,
            KeyCode::ShiftLeft | KeyCode::ShiftRight => overlay.shift = down,
            KeyCode::AltLeft | KeyCode::AltRight => overlay.alt = down,
            KeyCode::SuperLeft | KeyCode::SuperRight => overlay.super_key = down,
            _ => {}
        }
        if !down {
            continue;
        }
        let held_mods = crate::settings::Mods::from_flags(overlay.ctrl, overlay.shift, overlay.alt);
        let console_key = toggle.is_some_and(|binding| {
            binding.input == crate::settings::Input::Key(event.key_code)
                && binding.mods == held_mods
        }) && !event.repeat;
        if !overlay.open {
            if console_key {
                overlay.set_open(true);
                taken = true;
                overlay.read_history(history_file());
            }
            continue;
        }
        taken = true;
        let command = overlay.ctrl || overlay.super_key;
        let shift = overlay.shift;
        let press = match console_key {
            true => Some(Press::Close),
            false => press_of(event, command, shift),
        };
        let press = match press {
            Some(Press::Paste(_)) => clipboard
                .as_mut()
                .and_then(|clipboard| clipboard.fetch_text().poll_result()?.ok())
                .map(Press::Paste),
            other => other,
        };
        if let Some(press) = press {
            let asked = overlay.press(press, log.0.len());
            carry_out(
                asked,
                &mut overlay,
                &mut log,
                lines.as_deref_mut(),
                clipboard.as_deref_mut(),
            );
        }
    }
    if !(overlay.open || was_open) {
        return;
    }
    // The wheel scrolls the log, the pointer chooses among Tab's choices.
    if overlay.open {
        let notches: f32 = wheel.as_ref().map_or(0.0, |wheel| {
            wheel
                .iter_current_update_messages()
                .map(|wheel| wheel.y.signum())
                .sum()
        });
        let log_len = log.0.len();
        let by = notches.abs() as usize * NOTCH;
        overlay.scrolled = match notches > 0.0 {
            true => (overlay.scrolled + by).min(log_len),
            false => overlay.scrolled.saturating_sub(by),
        };
        overlay.pointer = windows
            .iter()
            .next()
            .and_then(Window::cursor_position)
            .map(|at| [at.x, at.y]);
        overlay.clicked = buttons
            .as_ref()
            .is_some_and(|buttons| buttons.just_pressed(MouseButton::Left));
    }
    if taken {
        keyboard.clear();
        if let Some(keys) = keys.as_deref_mut() {
            // The console's key, still down as the console closes, reads
            // as up until it is let go.
            if let Some(crate::settings::Input::Key(toggle)) = toggle.map(|b| b.input) {
                swallowed.key(toggle, keys);
            }
            keys.reset_all();
        }
    }
}

/// What a press asked, carried out: the lines given to the table's runner,
/// the clipboard written, the log emptied. Each line entered is saved in
/// the history's file.
fn carry_out(
    asked: Asked,
    overlay: &mut Overlay,
    log: &mut ConsoleLog,
    lines: Option<&mut console::LineRequests>,
    clipboard: Option<&mut bevy::clipboard::Clipboard>,
) {
    if asked.clear_log {
        log.0.clear();
    }
    if let (Some(text), Some(clipboard)) = (asked.copy, clipboard)
        && let Err(error) = clipboard.set_text(text)
    {
        warn!(target: "console", "the clipboard: {error}");
    }
    if !asked.lines.is_empty() {
        overlay.write_history(history_file());
        if let Some(lines) = lines {
            lines.0.extend(asked.lines);
        }
    }
}

/// A headless run's `ui console …` words (`Overlay::script`), carried out.
pub(crate) fn script(
    words: &[&str],
    overlay: &mut Overlay,
    log: &mut ConsoleLog,
    lines: Option<&mut console::LineRequests>,
) -> bool {
    if words == ["open"] {
        overlay.read_history(history_file());
    }
    match overlay.script(words, log.0.len()) {
        Some(asked) => {
            carry_out(asked, overlay, log, lines, None);
            true
        }
        None => false,
    }
}

#[derive(Default)]
struct NamedItems {
    game: Option<vfs::Game>,
    names: BTreeMap<String, Vec<u8>>,
}

/// The overlay's view for the screens (`ui::vm::ViewModel::console`), while
/// it is open; the pointer's choice taken.
fn show(
    mut overlay: ResMut<Overlay>,
    log: Res<ConsoleLog>,
    ui: Option<ResMut<crate::ui_view::Ui>>,
    windows: Query<&Window, With<PrimaryWindow>>,
    mut named: Local<NamedItems>,
) {
    let Some(mut ui) = ui else {
        return;
    };
    let ui = &mut *ui;
    if !overlay.open {
        ui.vm.console = None;
        return;
    }
    // The game's names of the item templates the choices' box shows.
    if let Some(assets) = ui.assets.as_mut() {
        if named.game != Some(assets.game) {
            named.game = Some(assets.game);
            named.names.clear();
        }
        if overlay.items {
            let first = ui::screens::console::first_choice(overlay.chosen);
            let shown = overlay.candidates.iter().skip(first);
            for word in shown.take(ui::screens::console::CHOICES) {
                if !named.names.contains_key(word)
                    && let Some(look) = assets.item(word)
                {
                    named.names.insert(word.clone(), look.name);
                }
            }
        }
    }
    let mut view = overlay.view(&log, &named.names);
    let window = windows.iter().next();
    if let (Some(point), Some(window), Some(assets)) = (overlay.pointer, window, ui.assets.as_mut())
    {
        let window = (window.width(), window.height());
        let mut painter = ui::paint::Painter::new(assets, ui::paint::Palette::TSL);
        if let Some(row) = ui::screens::console::choice_at(&mut painter, &view, window, point)
            && std::mem::take(&mut overlay.clicked)
        {
            overlay.pick(row);
            view = overlay.view(&log, &named.names);
        }
    }
    overlay.clicked = false;
    ui.vm.console = Some(view);
}

/// What the line runner reads of play.
type Shown<'w> = (
    Option<Res<'w, SceneSwap>>,
    Option<Res<'w, Loading>>,
    Option<Res<'w, PresentedSnapshot>>,
    Option<Res<'w, OwnSeat>>,
    Option<Res<'w, crate::target_view::Targeted>>,
);

/// Where the effects of a command go.
type Sinks<'w> = (
    Option<ResMut<'w, crate::loot::Outgoing>>,
    ResMut<'w, console::UiRequests>,
    ResMut<'w, console::VfxRequests>,
    ResMut<'w, console::DumpFacts>,
);

/// Where a session command's effects go: the menu's intents, a warp in a
/// game, and the mark of a warp at the main menu.
type Starts<'w> = (
    Option<ResMut<'w, crate::ui_view::UiIntents>>,
    ResMut<'w, WarpLater>,
    ResMut<'w, WarpMark>,
);

fn run_lines(
    (mut requests, mut overlay): (ResMut<console::LineRequests>, ResMut<Overlay>),
    (swap, loading, presented, own, targeted): Shown,
    mut ui: Option<ResMut<crate::ui_view::Ui>>,
    (mut outgoing, mut screens, mut visuals, mut dump): Sinks,
    (mut intents, mut later, mut mark): Starts,
    (mut log, mut mods): (ResMut<ConsoleLog>, Local<ModCommands>),
) {
    if requests.0.is_empty() && !overlay.wants_facts() {
        return;
    }
    let loaded = swap.is_some() && !loading.as_ref().is_some_and(|loading| loading.active);
    // The lobby is open, or a game is starting behind the loading screen.
    let lobby = ui.as_ref().is_some_and(|ui| ui.vm.lobby.is_some());
    let starting = ui.as_ref().is_some_and(|ui| ui.stack.has("loading"));
    let assets = ui.as_mut().and_then(|ui| ui.assets.as_mut());
    let module = swap.as_ref().map(|swap| swap.level.name().to_string());
    let added = (assets.as_deref())
        .map(|assets| mods.of(assets, module))
        .unwrap_or_else(|| Arc::new(commands::mods::Table::default()));
    let waiting = |line: &str| {
        !loaded
            && (commands::words::split(line).iter())
                .any(|words| commands::needs_game_with(words, &added))
    };
    let ready = requests.0.iter().take_while(|line| !waiting(line)).count();
    let mut facts = facts(
        swap.as_deref().filter(|_| loaded),
        (presented.as_deref(), own.as_deref()),
        targeted.as_deref().and_then(|targeted| targeted.object),
        assets,
    );
    facts.lobby = lobby;
    facts.menu = swap.is_none() && own.is_none() && !lobby && !starting;
    facts.mods = added;
    if overlay.wants_facts() {
        overlay.suggest(&facts);
    }
    if ready == 0 {
        return;
    }
    let lines: Vec<String> = requests.0.drain(..ready).collect();
    for line in lines {
        for (text, ran) in commands::run_line(&line, &facts) {
            log.push(format!("> {text}"), false);
            info!(target: "console", "> {text}");
            let effects = match ran {
                Ok(effects) => effects,
                Err(refusal) => {
                    warn!(target: "console", "{refusal}");
                    log.push(refusal.0, true);
                    continue;
                }
            };
            'effects: for effect in effects {
                match effect {
                    Effect::Order(action) => {
                        if let Some(outgoing) = outgoing.as_mut() {
                            outgoing.0.push(action);
                        }
                    }
                    Effect::Restore { who } => {
                        let (Some(swap), Some(outgoing)) = (swap.as_ref(), outgoing.as_mut())
                        else {
                            continue;
                        };
                        outgoing.0.push(Action::Arena(ArenaOrder::Restore {
                            who,
                            sheet: Box::new(crate::arena::sheet_for(&swap.level)),
                        }));
                    }
                    Effect::Print(text) => {
                        info!(target: "console", "{text}");
                        log.push(text, false);
                    }
                    Effect::ClearLog => log.0.clear(),
                    Effect::Ui(words) => screens.0.push(words),
                    Effect::Vfx(words) => visuals.0.push(words),
                    Effect::Saves => {
                        for text in saves_listed() {
                            info!(target: "console", "{text}");
                            log.push(text, false);
                        }
                    }
                    Effect::Mods => {
                        for text in mods_listed() {
                            info!(target: "console", "{text}");
                            log.push(text, false);
                        }
                    }
                    Effect::Start(start) => {
                        let at_menu = facts.menu;
                        let session = match start {
                            commands::Start::Warp {
                                module: name,
                                arrival,
                            } if !at_menu => {
                                later.0 = Some((name, arrival, WARP_WAIT_FRAMES));
                                if let Some(outgoing) = outgoing.as_mut() {
                                    outgoing.0.push(Action::Cheat(CheatOrder::Warped));
                                }
                                continue;
                            }
                            commands::Start::Warp { module, arrival } => {
                                mark.0 = true;
                                SessionIntent::Warp { module, arrival }
                            }
                            commands::Start::Sandbox => SessionIntent::Sandbox,
                            commands::Start::Load(id) => {
                                if !save_exists(&id) {
                                    let text = format!(
                                        "Save \"{id}\" not found. Type \"saves\" to list available saves."
                                    );
                                    warn!(target: "console", "{text}");
                                    log.push(text, true);
                                    break 'effects;
                                }
                                SessionIntent::Solo {
                                    save: Some(id),
                                    character: None,
                                    campaign: None,
                                }
                            }
                            commands::Start::Host(save) => {
                                if let Some(id) = &save
                                    && !save_exists(id)
                                {
                                    let text = format!(
                                        "Save \"{id}\" not found. Type \"saves\" to list available saves."
                                    );
                                    warn!(target: "console", "{text}");
                                    log.push(text, true);
                                    break 'effects;
                                }
                                SessionIntent::Host {
                                    save,
                                    seats: ui::screens::main_menu::SEATS,
                                    port: ui::intent::DEFAULT_PORT,
                                    character: None,
                                    campaign: None,
                                }
                            }
                            commands::Start::Join { address, name } => SessionIntent::Join {
                                address,
                                name: name.unwrap_or_else(|| ui::strings::DEFAULT_NAME.into()),
                            },
                        };
                        if let Some(intents) = intents.as_mut() {
                            intents.0.push(ui::intent::Intent::Session(session));
                        }
                    }
                }
            }
        }
    }
    dump.0
        .insert("console".into(), serde_json::Value::from(log.lines()));
}

/// The commands that mods add (`kjkconsole.2da`), read again when the game
/// or the module changes.
#[derive(Default)]
struct ModCommands {
    /// The game and the module the table was read for.
    read_for: Option<(vfs::Game, Option<String>)>,
    table: Arc<commands::mods::Table>,
}

impl ModCommands {
    /// The mod commands of the campaign `assets` reads, with `module`
    /// loaded. A row that was skipped goes to the log with its reason.
    fn of(
        &mut self,
        assets: &ui::assets::GuiAssets,
        module: Option<String>,
    ) -> Arc<commands::mods::Table> {
        let key = (assets.game, module);
        if self.read_for.as_ref() == Some(&key) {
            return self.table.clone();
        }
        let bytes = assets.table_bytes(commands::mods::TABLE);
        let parameters = assets.game == vfs::Game::Kotor2;
        let table = commands::mods::read(bytes, parameters, &mut |strref| {
            assets.campaign_text(strref)
        });
        if let Some(problem) = &table.problem {
            warn!(target: "console", "{}.2da could not be read: {problem}", commands::mods::TABLE);
        }
        for skipped in &table.skipped {
            warn!(target: "console", "{}", commands::mods::skipped_line(skipped));
        }
        if table.found {
            info!(target: "console", "mods add {} console commands", table.commands.len());
        }
        self.read_for = Some(key);
        self.table = Arc::new(table);
        self.table.clone()
    }
}

/// The saves in the saves' folder, the latest first, with their metas.
fn saves_found() -> Vec<(String, saves::Meta)> {
    crate::save_view::folder().map_or_else(Vec::new, |root| saves::list(&root).saves)
}

/// Whether a save has the number `id`.
fn save_exists(id: &str) -> bool {
    saves_found().iter().any(|(of, _)| of == id)
}

/// The `saves` command's lines: one for each save, the latest first, with
/// its number, its name, its area, when it was saved and whether cheats
/// were used.
fn saves_listed() -> Vec<String> {
    let found = saves_found();
    if found.is_empty() {
        return vec!["No saved games found.".into()];
    }
    found
        .iter()
        .map(|(id, meta)| {
            let cheats = if meta.cheats { ", Cheats used" } else { "" };
            format!(
                "{id}  {}  {}  {}{cheats}",
                meta.name,
                meta.area,
                crate::save_view::date(meta.saved)
            )
        })
        .collect()
}

/// The `mods` command's lines: for each KOTOR game installed, its
/// override folder and how many files it holds, then the Steam Workshop
/// items the game reads, in the order they apply (the last one wins).
fn mods_listed() -> Vec<String> {
    let discovery = crate::discover();
    let mut lines = Vec::new();
    for game in [vfs::Game::Kotor1, vfs::Game::Kotor2] {
        let Some(install) = discovery.get(game) else {
            continue;
        };
        let name = match game {
            vfs::Game::Kotor2 => "KOTOR 2",
            _ => "KOTOR",
        };
        let folder = install.data_root.join("override");
        let files = std::fs::read_dir(&folder).map_or(0, |entries| entries.flatten().count());
        lines.push(format!(
            "{name} override: {} ({files} files)",
            folder.display()
        ));
        let items = install.workshop_items().items;
        if items.is_empty() {
            lines.push(format!("{name} Workshop: none"));
        }
        for (at, item) in items.iter().enumerate() {
            lines.push(format!(
                "{name} Workshop ({}/{}): {}",
                at + 1,
                items.len(),
                item.id
            ));
        }
    }
    if lines.is_empty() {
        lines.push("No KOTOR installations found.".into());
    }
    lines
}

/// The table's facts of this moment: the level when one is up, what the
/// presented snapshot shows of this seat's party, the object the crosshair
/// is on (`target`, an object number), and the classes the game's tables
/// have.
fn facts(
    swap: Option<&SceneSwap>,
    (presented, own): (Option<&PresentedSnapshot>, Option<&OwnSeat>),
    target: Option<u32>,
    assets: Option<&mut ui::assets::GuiAssets>,
) -> Facts {
    let mut facts = Facts {
        version: concat!(env!("KJK_VERSION"), " (", env!("KJK_GIT_SHA"), ")").into(),
        ..Facts::default()
    };
    let Some(swap) = swap else {
        return facts;
    };
    facts.loaded = true;
    facts.arena = swap.level.arena_rules().is_some();
    facts.module = Some(swap.level.name().to_string());
    let library = swap.level.scripts();
    facts.creatures = library.map(|library| {
        library
            .creatures()
            .keys()
            .map(|key| key.to_ascii_lowercase())
            .collect()
    });
    facts.items = library.map(|library| {
        (library.items().iter())
            .map(|item| item.template.to_ascii_lowercase())
            .collect()
    });
    facts.scripts = library.map(|library| library.names().map(str::to_ascii_lowercase).collect());
    if let Some(assets) = assets {
        let classes = (0..assets.rows("classes"))
            .filter_map(|row| {
                let label = assets.cell("classes", row, "label")?;
                Some((u8::try_from(row).ok()?, label))
            })
            .collect::<Vec<_>>();
        facts.classes = (!classes.is_empty()).then_some(classes);
    }
    if let (Some(presented), Some(own)) = (presented, own) {
        let snapshot = presented.0.current();
        facts.host = own.0 == sim::Seat(0);
        facts.cheats = snapshot.cheats.rule;
        facts.xp = Some(snapshot.progress.xp);
        facts.target = target
            .and_then(|index| sim::target::entity(snapshot, index))
            .map(|entity| entity.id);
        facts.me = crate::cast::me(snapshot, own.0);
        let sheet = (facts.me)
            .and_then(|me| snapshot.get(me))
            .and_then(|entity| entity.sheet);
        facts.god = sheet.is_some_and(|s| s.flags & sim::combat::sheet::flags::IMMORTAL != 0);
        let level = sheet.map(|sheet| sheet.level);
        facts.next_level_xp =
            (library.zip(level)).and_then(|(library, level)| library.classes().next(level));
        facts.position = (facts.me)
            .and_then(|me| snapshot.get(me))
            .map(|entity| entity.position);
        facts.party = (crate::cast::party_of(snapshot, own.0).iter())
            .map(|member| member.id)
            .collect();
    }
    facts
}

/// The console's part of a bug report: whether the game shown was changed
/// by a console command (`cheated`, unknown outside a game), then the
/// console's log.
pub(crate) fn report_text(cheated: Option<bool>, log: &ConsoleLog) -> String {
    let cheated = cheated.map_or("no game", |used| if used { "yes" } else { "no" });
    let mut text = format!("cheats used: {cheated}\n");
    for line in &log.0 {
        text.push_str(&line.text);
        text.push('\n');
    }
    text
}
