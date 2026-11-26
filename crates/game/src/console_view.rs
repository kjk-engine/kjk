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
//! (`sim::cheats::Rule`) as the player chose it (`send_rule`). A run with a
//! command script, or with the Console option on, plays under the rule
//! Host only. Any other run plays under the rule Off.

use std::collections::{BTreeMap, VecDeque};
use std::path::PathBuf;

use bevy::input::ButtonState;
use bevy::input::keyboard::{Key, KeyboardInput};
use bevy::input::mouse::MouseWheel;
use bevy::prelude::*;
use bevy::window::PrimaryWindow;
use commands::{Effect, Facts};
use sim::action::{Action, ArenaOrder, CheatOrder};
use sim::cheats::Rule;
use ui::edit::EditBox;

use crate::sim_host::{Loading, OwnSeat, PresentedSnapshot, SceneSwap};

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

/// The cheat rule this run plays under (see the module documentation).
fn wanted_rule(scripted: bool, settings: Option<&crate::settings::Settings>) -> Rule {
    match enabled(scripted, settings) {
        true => Rule::Host,
        false => Rule::Off,
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
    mut waiting: Local<u32>,
) {
    let (Some(presented), Some(own), Some(outgoing)) = (presented, own, outgoing.as_mut()) else {
        return;
    };
    if here.is_none() || own.0 != sim::Seat(0) {
        return;
    }
    let wanted = wanted_rule(scripted.0, settings.as_deref());
    if presented.0.current().cheats.rule == wanted {
        *waiting = 0;
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
/// the log's scroll, and Tab's choices.
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
    /// Tab's choices for the word before the caret, and the chosen one.
    candidates: Vec<String>,
    chosen: Option<usize>,
    /// Tab was pressed: `run_lines` fills the choices, which need the
    /// table's facts.
    asking: bool,
    /// The choices are item templates, shown with the game's item names.
    items: bool,
    /// Where the pointer is, and whether the main button went down on it
    /// this frame (window pixels).
    pointer: Option<[f32; 2]>,
    clicked: bool,
    clock: f32,
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
            asking: false,
            items: false,
            pointer: None,
            clicked: false,
            clock: 0.0,
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
        }
    }

    fn close_choices(&mut self) {
        self.candidates.clear();
        self.chosen = None;
        self.asking = false;
    }

    fn choosing(&self) -> bool {
        !self.candidates.is_empty()
    }

    /// One key pressed while the console is open, with `log` lines in the
    /// log; what it asks of the game.
    pub(crate) fn press(&mut self, press: Press, log: usize) -> Asked {
        let mut asked = Asked::default();
        let page = encoding();
        match press {
            Press::Tab | Press::BackTab | Press::Up | Press::Down if self.choosing() => {
                let back = matches!(press, Press::BackTab | Press::Up);
                let count = self.candidates.len();
                let at = self.chosen.unwrap_or(0);
                self.chosen = Some(match back {
                    true => (at + count - 1) % count,
                    false => (at + 1) % count,
                });
            }
            Press::Enter if self.choosing() => self.pick(self.chosen.unwrap_or(0)),
            Press::Escape if self.choosing() => self.close_choices(),
            Press::Tab | Press::BackTab => self.asking = true,
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
                let lines: Vec<&str> = text.split('\n').take(MOST_PASTED).collect();
                if let [one] = lines.as_slice() {
                    self.edit.type_text(one, page);
                } else {
                    for line in lines.iter().filter(|line| !line.trim().is_empty()) {
                        self.edit.type_text(line, page);
                        let line = self.edit.as_str();
                        self.edit.clear();
                        self.enter(&line, &mut asked);
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
                // A change to the line closes Tab's choices.
                self.browsing = None;
                self.close_choices();
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
    }

    /// The word before the caret replaced by choice `index`, a space after
    /// it, and the choices closed.
    fn pick(&mut self, index: usize) {
        let Some(chosen) = self.candidates.get(index).cloned() else {
            return;
        };
        let line = self.edit.as_str();
        let caret = self.edit.caret().min(line.len());
        let (before, after) = line.split_at(caret);
        let start = before.rfind(' ').map_or(0, |at| at + 1);
        let after = after.trim_start_matches(' ');
        let picked = format!("{}{chosen} ", &before[..start]);
        self.edit.set(&format!("{picked}{after}"));
        self.edit.home();
        self.edit.step(picked.len() as isize);
        self.close_choices();
    }

    /// Tab's choices for the word before the caret, with `facts`. A single
    /// choice is taken at once.
    pub(crate) fn complete(&mut self, facts: &Facts) {
        self.asking = false;
        let line = self.edit.as_str();
        let before = &line[..self.edit.caret().min(line.len())];
        let found = commands::complete::candidates(before, facts);
        self.items = (facts.items.as_ref())
            .zip(found.first())
            .is_some_and(|(items, first)| items.contains(first));
        self.candidates = found;
        self.chosen = None;
        match self.candidates.len() {
            0 => {}
            1 => self.pick(0),
            _ => self.chosen = Some(0),
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
                    name: named.get(word).cloned().unwrap_or_else(Vec::new),
                })
                .collect(),
            chosen: self.chosen,
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
        .and_then(|map| map.key(crate::settings::Action::Console));
    let held = |codes: [KeyCode; 2]| {
        keys.as_ref()
            .is_some_and(|keys| codes.iter().any(|code| keys.pressed(*code)))
    };
    let command = held([KeyCode::ControlLeft, KeyCode::ControlRight])
        || held([KeyCode::SuperLeft, KeyCode::SuperRight]);
    let shift = held([KeyCode::ShiftLeft, KeyCode::ShiftRight]);
    let pressed: Vec<KeyboardInput> = (keyboard.iter_current_update_messages())
        .filter(|event| event.state == ButtonState::Pressed)
        .cloned()
        .collect();
    let was_open = overlay.open;
    let mut taken = false;
    for event in &pressed {
        let console_key = Some(event.key_code) == toggle && !event.repeat;
        if !overlay.open {
            if console_key {
                overlay.set_open(true);
                taken = true;
                overlay.read_history(history_file());
            }
            continue;
        }
        taken = true;
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
            if let Some(toggle) = toggle {
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

/// The overlay's view for the screens (`ui::vm::ViewModel::console`), while
/// it is open; the pointer's choice taken.
fn show(
    mut overlay: ResMut<Overlay>,
    log: Res<ConsoleLog>,
    ui: Option<ResMut<crate::ui_view::Ui>>,
    windows: Query<&Window, With<PrimaryWindow>>,
    mut named: Local<BTreeMap<String, Vec<u8>>>,
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
    if overlay.items
        && let Some(assets) = ui.assets.as_mut()
    {
        let first = ui::screens::console::first_choice(overlay.chosen);
        let shown = overlay.candidates.iter().skip(first);
        for word in shown.take(ui::screens::console::CHOICES) {
            if !named.contains_key(word)
                && let Some(look) = assets.item(word)
            {
                named.insert(word.clone(), look.name);
            }
        }
    }
    let mut view = overlay.view(&log, &named);
    let window = windows.iter().next();
    if let (Some(point), Some(window), Some(assets)) = (overlay.pointer, window, ui.assets.as_mut())
    {
        let window = (window.width(), window.height());
        let mut painter = ui::paint::Painter::new(assets, ui::paint::Palette::TSL);
        if let Some(row) = ui::screens::console::choice_at(&mut painter, &view, window, point) {
            overlay.chosen = Some(row);
            if std::mem::take(&mut overlay.clicked) {
                overlay.pick(row);
            }
            view = overlay.view(&log, &named);
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

fn run_lines(
    (mut requests, mut overlay): (ResMut<console::LineRequests>, ResMut<Overlay>),
    (swap, loading, presented, own, targeted): Shown,
    mut ui: Option<ResMut<crate::ui_view::Ui>>,
    (mut outgoing, mut screens, mut visuals, mut dump): Sinks,
    mut log: ResMut<ConsoleLog>,
) {
    if requests.0.is_empty() && !overlay.asking {
        return;
    }
    let loaded = swap.is_some() && !loading.as_ref().is_some_and(|loading| loading.active);
    let waiting = |line: &str| {
        !loaded && (commands::words::split(line).iter()).any(|words| commands::needs_game(words))
    };
    let ready = requests.0.iter().take_while(|line| !waiting(line)).count();
    let assets = ui.as_mut().and_then(|ui| ui.assets.as_mut());
    let facts = facts(
        swap.as_deref().filter(|_| loaded),
        (presented.as_deref(), own.as_deref()),
        targeted.as_deref().and_then(|targeted| targeted.object),
        assets,
    );
    if overlay.asking {
        overlay.complete(&facts);
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
            for effect in effects {
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
                }
            }
        }
    }
    dump.0
        .insert("console".into(), serde_json::Value::from(log.lines()));
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
        facts.credits = Some(snapshot.goods.gold);
        facts.xp = Some(snapshot.progress.xp);
        facts.target = target
            .and_then(|index| sim::target::entity(snapshot, index))
            .map(|entity| entity.id);
        facts.me = crate::cast::me(snapshot, own.0);
        let level = (facts.me)
            .and_then(|me| snapshot.get(me))
            .and_then(|entity| entity.sheet)
            .map(|sheet| sheet.level);
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
