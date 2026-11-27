//! The editor's keys. Each action the keys do (a tool, Frame, Delete, Play,
//! the camera's flight) has one or more keys or combinations of keys, such
//! as `W` or `Ctrl+D`. The defaults follow Unreal Engine 5's level editor
//! where it has the same action, and keep the editor's own keys beside
//! them where Unreal Engine 5 has none.
//!
//! The keys are kept in a file the user can edit, `editor-keys.toml` in
//! kjk's own folder (`KJK_EDITOR_KEYS` names another file). The editor
//! writes the file with every action and its keys when there is none, and
//! reads it again when it changes. The Keys window (File, Keys...) and the
//! `edit keys` commands change it too. An action the file leaves out keeps
//! its default keys.

use std::collections::BTreeMap;
use std::path::PathBuf;

use bevy::input::ButtonInput;
use bevy::input::keyboard::KeyCode;
use bevy_egui::egui;

/// Where the keys are kept while the editor runs, and what it acts on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) enum Action {
    SelectTool,
    MoveTool,
    RotateTool,
    ScaleTool,
    Frame,
    Delete,
    Duplicate,
    DropToFloor,
    SelectAll,
    SelectNone,
    Undo,
    Redo,
    Save,
    Copy,
    CopyForAgent,
    PlacePicked,
    ShowCollision,
    ShowNav,
    ShowLights,
    Play,
    PlayHere,
    Walk,
    Audit,
    FlyForward,
    FlyBack,
    FlyLeft,
    FlyRight,
    FlyDown,
    FlyUp,
    FlyFaster,
    TalkNext,
    TalkBeside,
}

/// Where an action's keys work. Two actions may share a key only in
/// different places.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Place {
    /// The level's view, while no text field has the keys.
    View,
    /// The level's view while the right mouse button is held: the camera
    /// flies.
    Flying,
    /// The conversation window, while a line's text is typed.
    Conversation,
}

/// One action: its name in the file, what it does, where it works, and
/// its default keys.
pub(crate) struct About {
    pub(crate) action: Action,
    pub(crate) name: &'static str,
    pub(crate) does: &'static str,
    pub(crate) place: Place,
    pub(crate) defaults: &'static [&'static str],
}

/// Every action, in the order the file and the Keys window list them.
/// Unreal Engine 5 gives W, E and R to its move, rotate and scale tools,
/// Q to selecting, F to framing the selection, End to dropping it to the
/// floor, Ctrl+D to duplicating, Alt+P to playing, Alt+C to showing the
/// collision and P to showing the navigation mesh. With the right mouse
/// button held, W, A, S and D fly, Q goes down and E goes up.
pub(crate) const ACTIONS: &[About] = &[
    About {
        action: Action::SelectTool,
        name: "select_tool",
        does: "The Select tool",
        place: Place::View,
        defaults: &["Q"],
    },
    About {
        action: Action::MoveTool,
        name: "move_tool",
        does: "The Move tool",
        place: Place::View,
        defaults: &["W"],
    },
    About {
        action: Action::RotateTool,
        name: "rotate_tool",
        does: "The Rotate tool",
        place: Place::View,
        defaults: &["E"],
    },
    About {
        action: Action::ScaleTool,
        name: "scale_tool",
        does: "The Scale tool",
        place: Place::View,
        defaults: &["R"],
    },
    About {
        action: Action::Frame,
        name: "frame",
        does: "Move the camera to the selection",
        place: Place::View,
        defaults: &["F"],
    },
    About {
        action: Action::Delete,
        name: "delete",
        does: "Delete everything selected",
        place: Place::View,
        defaults: &["Delete"],
    },
    About {
        action: Action::Duplicate,
        name: "duplicate",
        does: "Copy everything selected, in the same place",
        place: Place::View,
        defaults: &["Ctrl+D"],
    },
    About {
        action: Action::DropToFloor,
        name: "drop_to_floor",
        does: "Put everything selected on the walkable floor under it",
        place: Place::View,
        defaults: &["End"],
    },
    About {
        action: Action::SelectAll,
        name: "select_all",
        does: "Select everything in the level",
        place: Place::View,
        defaults: &["Ctrl+A"],
    },
    About {
        action: Action::SelectNone,
        name: "select_none",
        does: "Select nothing",
        place: Place::View,
        defaults: &["Escape"],
    },
    About {
        action: Action::Undo,
        name: "undo",
        does: "Undo",
        place: Place::View,
        defaults: &["Ctrl+Z"],
    },
    About {
        action: Action::Redo,
        name: "redo",
        does: "Redo",
        place: Place::View,
        defaults: &["Ctrl+Y"],
    },
    About {
        action: Action::Save,
        name: "save",
        does: "Save the project",
        place: Place::View,
        defaults: &["Ctrl+S"],
    },
    About {
        action: Action::Copy,
        name: "copy",
        does: "Copy the selection to the clipboard",
        place: Place::View,
        defaults: &["Ctrl+C"],
    },
    About {
        action: Action::CopyForAgent,
        name: "copy_for_agent",
        does: "Copy the selection, or the level, as text for an agent",
        place: Place::View,
        defaults: &["Ctrl+Shift+C"],
    },
    About {
        action: Action::PlacePicked,
        name: "place_picked",
        does: "Place the asset picked in the picker where the view looks",
        place: Place::View,
        defaults: &["Enter"],
    },
    About {
        action: Action::ShowCollision,
        name: "show_collision",
        does: "Show or hide the collision",
        place: Place::View,
        defaults: &["Alt+C", "F2"],
    },
    About {
        action: Action::ShowNav,
        name: "show_nav",
        does: "Show or hide the navigation mesh",
        place: Place::View,
        defaults: &["P", "F3"],
    },
    About {
        action: Action::ShowLights,
        name: "show_lights",
        does: "Show or hide the lights",
        place: Place::View,
        defaults: &["F4"],
    },
    About {
        action: Action::Play,
        name: "play",
        does: "Save, then play the project in kjk",
        place: Place::View,
        defaults: &["Alt+P", "F5"],
    },
    About {
        action: Action::PlayHere,
        name: "play_here",
        does: "Save, then play from where the view looks",
        place: Place::View,
        defaults: &["Shift+F5"],
    },
    About {
        action: Action::Walk,
        name: "walk",
        does: "Save, then walk the level in walk mode",
        place: Place::View,
        defaults: &["F7"],
    },
    About {
        action: Action::Audit,
        name: "audit",
        does: "Save, then run the containment audit",
        place: Place::View,
        defaults: &["F6"],
    },
    About {
        action: Action::FlyForward,
        name: "fly_forward",
        does: "Fly forward",
        place: Place::Flying,
        defaults: &["W"],
    },
    About {
        action: Action::FlyBack,
        name: "fly_back",
        does: "Fly back",
        place: Place::Flying,
        defaults: &["S"],
    },
    About {
        action: Action::FlyLeft,
        name: "fly_left",
        does: "Fly left",
        place: Place::Flying,
        defaults: &["A"],
    },
    About {
        action: Action::FlyRight,
        name: "fly_right",
        does: "Fly right",
        place: Place::Flying,
        defaults: &["D"],
    },
    About {
        action: Action::FlyDown,
        name: "fly_down",
        does: "Fly down",
        place: Place::Flying,
        defaults: &["Q"],
    },
    About {
        action: Action::FlyUp,
        name: "fly_up",
        does: "Fly up",
        place: Place::Flying,
        defaults: &["E"],
    },
    About {
        action: Action::FlyFaster,
        name: "fly_faster",
        does: "Fly faster while held",
        place: Place::Flying,
        defaults: &["Shift"],
    },
    About {
        action: Action::TalkNext,
        name: "conversation_next",
        does: "Add the next line under the one being written, and write it",
        place: Place::Conversation,
        defaults: &["Ctrl+Enter"],
    },
    About {
        action: Action::TalkBeside,
        name: "conversation_beside",
        does: "Add another line beside the one being written, such as another reply, and write it",
        place: Place::Conversation,
        defaults: &["Ctrl+Shift+Enter"],
    },
];

/// What is known of an action.
pub(crate) fn about(action: Action) -> &'static About {
    ACTIONS
        .iter()
        .find(|about| about.action == action)
        .expect("every action is listed")
}

/// The keys a chord can end in: each key's name, as the file writes it,
/// with its codes for Bevy and for egui.
const KEYS: &[(&str, KeyCode, Option<egui::Key>)] = &[
    ("A", KeyCode::KeyA, Some(egui::Key::A)),
    ("B", KeyCode::KeyB, Some(egui::Key::B)),
    ("C", KeyCode::KeyC, Some(egui::Key::C)),
    ("D", KeyCode::KeyD, Some(egui::Key::D)),
    ("E", KeyCode::KeyE, Some(egui::Key::E)),
    ("F", KeyCode::KeyF, Some(egui::Key::F)),
    ("G", KeyCode::KeyG, Some(egui::Key::G)),
    ("H", KeyCode::KeyH, Some(egui::Key::H)),
    ("I", KeyCode::KeyI, Some(egui::Key::I)),
    ("J", KeyCode::KeyJ, Some(egui::Key::J)),
    ("K", KeyCode::KeyK, Some(egui::Key::K)),
    ("L", KeyCode::KeyL, Some(egui::Key::L)),
    ("M", KeyCode::KeyM, Some(egui::Key::M)),
    ("N", KeyCode::KeyN, Some(egui::Key::N)),
    ("O", KeyCode::KeyO, Some(egui::Key::O)),
    ("P", KeyCode::KeyP, Some(egui::Key::P)),
    ("Q", KeyCode::KeyQ, Some(egui::Key::Q)),
    ("R", KeyCode::KeyR, Some(egui::Key::R)),
    ("S", KeyCode::KeyS, Some(egui::Key::S)),
    ("T", KeyCode::KeyT, Some(egui::Key::T)),
    ("U", KeyCode::KeyU, Some(egui::Key::U)),
    ("V", KeyCode::KeyV, Some(egui::Key::V)),
    ("W", KeyCode::KeyW, Some(egui::Key::W)),
    ("X", KeyCode::KeyX, Some(egui::Key::X)),
    ("Y", KeyCode::KeyY, Some(egui::Key::Y)),
    ("Z", KeyCode::KeyZ, Some(egui::Key::Z)),
    ("0", KeyCode::Digit0, Some(egui::Key::Num0)),
    ("1", KeyCode::Digit1, Some(egui::Key::Num1)),
    ("2", KeyCode::Digit2, Some(egui::Key::Num2)),
    ("3", KeyCode::Digit3, Some(egui::Key::Num3)),
    ("4", KeyCode::Digit4, Some(egui::Key::Num4)),
    ("5", KeyCode::Digit5, Some(egui::Key::Num5)),
    ("6", KeyCode::Digit6, Some(egui::Key::Num6)),
    ("7", KeyCode::Digit7, Some(egui::Key::Num7)),
    ("8", KeyCode::Digit8, Some(egui::Key::Num8)),
    ("9", KeyCode::Digit9, Some(egui::Key::Num9)),
    ("F1", KeyCode::F1, Some(egui::Key::F1)),
    ("F2", KeyCode::F2, Some(egui::Key::F2)),
    ("F3", KeyCode::F3, Some(egui::Key::F3)),
    ("F4", KeyCode::F4, Some(egui::Key::F4)),
    ("F5", KeyCode::F5, Some(egui::Key::F5)),
    ("F6", KeyCode::F6, Some(egui::Key::F6)),
    ("F7", KeyCode::F7, Some(egui::Key::F7)),
    ("F8", KeyCode::F8, Some(egui::Key::F8)),
    ("F9", KeyCode::F9, Some(egui::Key::F9)),
    ("F10", KeyCode::F10, Some(egui::Key::F10)),
    ("F11", KeyCode::F11, Some(egui::Key::F11)),
    ("F12", KeyCode::F12, Some(egui::Key::F12)),
    ("Escape", KeyCode::Escape, Some(egui::Key::Escape)),
    ("Enter", KeyCode::Enter, Some(egui::Key::Enter)),
    ("Space", KeyCode::Space, Some(egui::Key::Space)),
    ("Tab", KeyCode::Tab, Some(egui::Key::Tab)),
    ("Backspace", KeyCode::Backspace, Some(egui::Key::Backspace)),
    ("Delete", KeyCode::Delete, Some(egui::Key::Delete)),
    ("Insert", KeyCode::Insert, Some(egui::Key::Insert)),
    ("Home", KeyCode::Home, Some(egui::Key::Home)),
    ("End", KeyCode::End, Some(egui::Key::End)),
    ("PageUp", KeyCode::PageUp, Some(egui::Key::PageUp)),
    ("PageDown", KeyCode::PageDown, Some(egui::Key::PageDown)),
    ("Up", KeyCode::ArrowUp, Some(egui::Key::ArrowUp)),
    ("Down", KeyCode::ArrowDown, Some(egui::Key::ArrowDown)),
    ("Left", KeyCode::ArrowLeft, Some(egui::Key::ArrowLeft)),
    ("Right", KeyCode::ArrowRight, Some(egui::Key::ArrowRight)),
    ("[", KeyCode::BracketLeft, Some(egui::Key::OpenBracket)),
    ("]", KeyCode::BracketRight, Some(egui::Key::CloseBracket)),
    (",", KeyCode::Comma, Some(egui::Key::Comma)),
    (".", KeyCode::Period, Some(egui::Key::Period)),
    ("/", KeyCode::Slash, Some(egui::Key::Slash)),
    (";", KeyCode::Semicolon, Some(egui::Key::Semicolon)),
    ("-", KeyCode::Minus, Some(egui::Key::Minus)),
    ("=", KeyCode::Equal, Some(egui::Key::Equals)),
    ("`", KeyCode::Backquote, Some(egui::Key::Backtick)),
    // The modifiers alone, for a key that is held: Shift to fly faster.
    ("Shift", KeyCode::ShiftLeft, None),
    ("Ctrl", KeyCode::ControlLeft, None),
    ("Alt", KeyCode::AltLeft, None),
];

/// A key with the modifiers held with it: `Ctrl+Shift+C`. A chord of a
/// modifier alone (`Shift`) holds no other modifier.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) struct Chord {
    pub(crate) ctrl: bool,
    pub(crate) shift: bool,
    pub(crate) alt: bool,
    /// The place of the key in `KEYS`.
    key: usize,
}

impl Chord {
    /// Reads `Ctrl+Shift+C`. The modifiers come in any order, and case
    /// does not matter.
    pub(crate) fn parse(text: &str) -> Result<Chord, String> {
        let parts: Vec<&str> = text.split('+').map(str::trim).collect();
        let (last, modifiers) = parts.split_last().ok_or("an empty key")?;
        let mut chord = Chord {
            ctrl: false,
            shift: false,
            alt: false,
            key: KEYS
                .iter()
                .position(|(name, _, _)| name.eq_ignore_ascii_case(last))
                .ok_or_else(|| format!("{text:?}: there is no key {last:?}"))?,
        };
        for modifier in modifiers {
            match modifier.to_ascii_lowercase().as_str() {
                "ctrl" | "control" => chord.ctrl = true,
                "shift" => chord.shift = true,
                "alt" => chord.alt = true,
                _ => return Err(format!("{text:?}: {modifier:?} is not Ctrl, Shift or Alt")),
            }
        }
        Ok(chord)
    }

    /// The key's Bevy code.
    fn code(self) -> KeyCode {
        KEYS[self.key].1
    }

    /// Whether the chord was pressed this frame: its key went down with
    /// its modifiers held and no other modifier.
    fn pressed(self, keys: &ButtonInput<KeyCode>) -> bool {
        keys.just_pressed(self.code()) && self.modifiers_held(keys)
    }

    fn modifiers_held(self, keys: &ButtonInput<KeyCode>) -> bool {
        let ctrl = keys.any_pressed([KeyCode::ControlLeft, KeyCode::ControlRight]);
        let shift = keys.any_pressed([KeyCode::ShiftLeft, KeyCode::ShiftRight]);
        let alt = keys.any_pressed([KeyCode::AltLeft, KeyCode::AltRight]);
        (ctrl, shift, alt) == (self.ctrl, self.shift, self.alt)
    }

    /// Takes the chord out of egui's events, if it was pressed this frame
    /// with exactly its modifiers: a text field then does not see it.
    pub(crate) fn take_from(self, ctx: &egui::Context) -> bool {
        let Some(key) = KEYS[self.key].2 else {
            return false;
        };
        ctx.input_mut(|input| {
            let at = input.events.iter().position(|event| {
                matches!(event, egui::Event::Key { key: pressed, pressed: true, modifiers, .. }
                    if *pressed == key
                        && modifiers.ctrl == self.ctrl
                        && modifiers.shift == self.shift
                        && modifiers.alt == self.alt)
            });
            at.map(|at| input.events.remove(at)).is_some()
        })
    }

    /// The chord as the file writes it.
    pub(crate) fn shown(self) -> String {
        let mut text = String::new();
        for (on, name) in [
            (self.ctrl, "Ctrl+"),
            (self.shift, "Shift+"),
            (self.alt, "Alt+"),
        ] {
            if on {
                text.push_str(name);
            }
        }
        text.push_str(KEYS[self.key].0);
        text
    }

    /// The chord pressed this frame, if a key went down: the first key
    /// pressed that is not a modifier, with the modifiers held. A modifier
    /// pressed alone counts once it is let go with no other key pressed.
    fn caught(keys: &ButtonInput<KeyCode>) -> Option<Chord> {
        let ctrl = keys.any_pressed([KeyCode::ControlLeft, KeyCode::ControlRight]);
        let shift = keys.any_pressed([KeyCode::ShiftLeft, KeyCode::ShiftRight]);
        let alt = keys.any_pressed([KeyCode::AltLeft, KeyCode::AltRight]);
        if let Some(key) = KEYS
            .iter()
            .position(|(_, code, egui)| egui.is_some() && keys.just_pressed(*code))
        {
            return Some(Chord {
                ctrl,
                shift,
                alt,
                key,
            });
        }
        let released = |codes: [KeyCode; 2]| codes.iter().any(|code| keys.just_released(*code));
        let alone = |name: &str| Chord {
            ctrl: false,
            shift: false,
            alt: false,
            key: KEYS
                .iter()
                .position(|(n, _, _)| *n == name)
                .expect("a modifier"),
        };
        if released([KeyCode::ShiftLeft, KeyCode::ShiftRight]) && !ctrl && !alt {
            return Some(alone("Shift"));
        }
        if released([KeyCode::ControlLeft, KeyCode::ControlRight]) && !shift && !alt {
            return Some(alone("Ctrl"));
        }
        if released([KeyCode::AltLeft, KeyCode::AltRight]) && !ctrl && !shift {
            return Some(alone("Alt"));
        }
        None
    }
}

/// The keys of every action.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Keymap {
    keys: BTreeMap<Action, Vec<Chord>>,
}

impl Default for Keymap {
    fn default() -> Keymap {
        let keys = ACTIONS
            .iter()
            .map(|about| {
                let chords = about
                    .defaults
                    .iter()
                    .map(|text| Chord::parse(text).expect("the defaults parse"))
                    .collect();
                (about.action, chords)
            })
            .collect();
        Keymap { keys }
    }
}

impl Keymap {
    /// The keys of `action`.
    pub(crate) fn of(&self, action: Action) -> &[Chord] {
        self.keys.get(&action).map_or(&[], Vec::as_slice)
    }

    /// The keys of `action` as the panels show them: `Ctrl+D`, or
    /// `Alt+P or F5`. Empty when it has none.
    pub(crate) fn shown(&self, action: Action) -> String {
        let shown: Vec<String> = self.of(action).iter().map(|chord| chord.shown()).collect();
        shown.join(" or ")
    }

    /// Gives `action` the keys `chords`, none to leave it without keys.
    pub(crate) fn set(&mut self, action: Action, chords: Vec<Chord>) {
        self.keys.insert(action, chords);
    }

    /// The actions of `place` whose keys were pressed this frame.
    pub(crate) fn pressed(&self, place: Place, keys: &ButtonInput<KeyCode>) -> Vec<Action> {
        ACTIONS
            .iter()
            .filter(|about| about.place == place)
            .filter(|about| {
                self.of(about.action)
                    .iter()
                    .any(|chord| chord.pressed(keys))
            })
            .map(|about| about.action)
            .collect()
    }

    /// The keys of `action` that another action of the same place has too.
    pub(crate) fn clashes(&self, action: Action) -> Vec<(Chord, Action)> {
        let place = about(action).place;
        let mut found = Vec::new();
        for chord in self.of(action) {
            for other in ACTIONS
                .iter()
                .filter(|other| other.place == place && other.action != action)
            {
                if self.of(other.action).contains(chord) {
                    found.push((*chord, other.action));
                }
            }
        }
        found
    }

    /// Reads a keys file: the default keys, with each action the file
    /// names given the file's keys. What the file gets wrong is told, and
    /// that action keeps its default keys.
    pub(crate) fn read(text: &str) -> (Keymap, Vec<String>) {
        let mut map = Keymap::default();
        let mut problems = Vec::new();
        let document = match text.parse::<toml_edit::DocumentMut>() {
            Ok(document) => document,
            Err(why) => return (map, vec![format!("the keys file does not read: {why}")]),
        };
        let table = document
            .get("keys")
            .and_then(toml_edit::Item::as_table_like);
        let Some(table) = table else {
            return (map, problems);
        };
        for (name, item) in table.iter() {
            let Some(about) = ACTIONS.iter().find(|about| about.name == name) else {
                problems.push(format!("the keys file names no action {name:?}"));
                continue;
            };
            let texts: Option<Vec<&str>> = match item.as_value() {
                Some(toml_edit::Value::String(text)) => Some(vec![text.value().as_str()]),
                Some(toml_edit::Value::Array(array)) => {
                    array.iter().map(toml_edit::Value::as_str).collect()
                }
                _ => None,
            };
            let Some(texts) = texts else {
                problems.push(format!(
                    "{name}: give a key as text, such as \"Ctrl+D\", or a list of them"
                ));
                continue;
            };
            let chords: Result<Vec<Chord>, String> = texts
                .iter()
                .filter(|text| !text.trim().is_empty())
                .map(|text| Chord::parse(text))
                .collect();
            match chords {
                Ok(chords) => map.set(about.action, chords),
                Err(why) => problems.push(format!("{name}: {why}")),
            }
        }
        (map, problems)
    }

    /// The keys file: every action with its keys, each under what it does.
    pub(crate) fn write(&self) -> String {
        let mut text = String::from(
            "# The level editor's keys. Each action takes one key or combination,\n\
             # such as \"Ctrl+D\", or a list of them, such as [\"Alt+P\", \"F5\"].\n\
             # An empty list leaves the action without keys. The modifiers are\n\
             # Ctrl, Shift and Alt. The other keys are the letters, the digits,\n\
             # F1 to F12, Escape, Enter, Space, Tab, Backspace, Delete, Insert,\n\
             # Home, End, PageUp, PageDown, Up, Down, Left, Right and [ ] , . / ; - = `.\n\
             # The fly keys work while the right mouse button is held. The\n\
             # conversation keys work while a line of a conversation is typed.\n\
             # The editor reads this file again when it changes.\n\n[keys]\n",
        );
        for about in ACTIONS {
            let chords: Vec<String> = self
                .of(about.action)
                .iter()
                .map(|chord| format!("\"{}\"", chord.shown()))
                .collect();
            let value = match chords.as_slice() {
                [one] => one.clone(),
                _ => format!("[{}]", chords.join(", ")),
            };
            text.push_str(&format!("# {}\n{} = {value}\n", about.does, about.name));
        }
        text
    }
}

/// The keys file: the one `KJK_EDITOR_KEYS` names, else
/// `editor-keys.toml` in kjk's own folder.
pub(crate) fn file() -> Option<PathBuf> {
    if let Some(path) = std::env::var_os("KJK_EDITOR_KEYS").filter(|path| !path.is_empty()) {
        return Some(PathBuf::from(path));
    }
    store::kjk_folder(|var| std::env::var(var).ok()).map(|folder| folder.join("editor-keys.toml"))
}

/// The keys as the editor holds them: the map, the file it was read from,
/// and when that file was last changed, to read it again after an edit.
pub(crate) struct Keys {
    pub(crate) map: Keymap,
    pub(crate) file: Option<PathBuf>,
    changed: Option<std::time::SystemTime>,
    looked: std::time::Instant,
}

impl Keys {
    /// The default keys, with no file.
    pub(crate) fn standard() -> Keys {
        Keys {
            map: Keymap::default(),
            file: None,
            changed: None,
            looked: std::time::Instant::now(),
        }
    }

    /// Reads the keys file, and writes it with the default keys when there
    /// is none. What it gets wrong goes to `say`.
    pub(crate) fn load(say: &mut dyn FnMut(String)) -> Keys {
        let file = file();
        let mut keys = Keys {
            map: Keymap::default(),
            file,
            changed: None,
            looked: std::time::Instant::now(),
        };
        match keys.file.as_ref().map(std::fs::read_to_string) {
            Some(Ok(text)) => {
                let (map, problems) = Keymap::read(&text);
                keys.map = map;
                for problem in problems {
                    say(problem);
                }
            }
            Some(Err(_)) => {
                if let Err(why) = keys.save() {
                    say(why);
                }
            }
            None => {}
        }
        keys.changed = keys.modified();
        keys
    }

    fn modified(&self) -> Option<std::time::SystemTime> {
        let file = self.file.as_ref()?;
        std::fs::metadata(file)
            .and_then(|meta| meta.modified())
            .ok()
    }

    /// Writes the keys to their file.
    pub(crate) fn save(&mut self) -> Result<(), String> {
        let Some(file) = &self.file else {
            return Err("the keys have no file: kjk's own folder is not known".into());
        };
        if let Some(folder) = file.parent() {
            let _ = std::fs::create_dir_all(folder);
        }
        std::fs::write(file, self.map.write())
            .map_err(|why| format!("{}: {why}", file.display()))?;
        self.changed = self.modified();
        Ok(())
    }

    /// Reads the file again when it was changed since: about once a
    /// second, so an edit in a text editor takes effect while the editor
    /// runs. Whether it was read again.
    pub(crate) fn follow_file(&mut self, say: &mut dyn FnMut(String)) -> bool {
        if self.looked.elapsed() < std::time::Duration::from_secs(1) {
            return false;
        }
        self.looked = std::time::Instant::now();
        let now = self.modified();
        if now.is_none() || now == self.changed {
            return false;
        }
        self.changed = now;
        let Some(Ok(text)) = self.file.as_ref().map(std::fs::read_to_string) else {
            return false;
        };
        let (map, problems) = Keymap::read(&text);
        self.map = map;
        for problem in problems {
            say(problem);
        }
        say("read the keys file again".into());
        true
    }
}

/// The Keys window's state: whether it is open, and the action whose new
/// key is being caught (with the place of the key it replaces, or `None`
/// for one more key).
#[derive(Default)]
pub(crate) struct Window {
    pub(crate) open: bool,
    catching: Option<(Action, Option<usize>)>,
}

impl Window {
    /// Whether the window waits for a key: the editor's keys do nothing
    /// meanwhile.
    pub(crate) fn catching(&self) -> bool {
        self.catching.is_some()
    }
}

/// The Keys window: every action with its keys. A key's button, clicked,
/// waits for the next key pressed and puts it in that key's place. Each
/// change is an `edit keys` command, which writes the file.
pub(crate) fn window(
    ctx: &egui::Context,
    state: &mut crate::state::EditorState,
    window: &mut Window,
    keys: Option<&ButtonInput<KeyCode>>,
) {
    if !window.open {
        window.catching = None;
        return;
    }
    let mut command: Option<String> = None;
    // A key pressed while one is caught takes its place.
    if let (Some((action, at)), Some(keys)) = (window.catching, keys)
        && let Some(chord) = Chord::caught(keys)
    {
        let mut chords: Vec<Chord> = state.keys.map.of(action).to_vec();
        match at {
            Some(at) if at < chords.len() => chords[at] = chord,
            _ if !chords.contains(&chord) => chords.push(chord),
            _ => {}
        }
        command = Some(set_command(action, &chords));
        window.catching = None;
    }
    let mut open = true;
    egui::Window::new("Keys")
        .open(&mut open)
        .default_width(640.0)
        .default_height(560.0)
        .show(ctx, |ui| {
            ui.label(
                "Click a key to change it, then press the new key or combination. The keys are \
                 kept in this file, which can also be edited by hand:",
            );
            if let Some(file) = &state.keys.file {
                ui.label(
                    egui::RichText::new(file.display().to_string())
                        .monospace()
                        .small(),
                );
            }
            ui.horizontal(|ui| {
                if ui
                    .button("Unreal Engine 5 defaults")
                    .on_hover_text("Gives every action its default keys again.")
                    .clicked()
                {
                    command = Some("keys reset".into());
                }
                if window.catching.is_some() && ui.button("Stop waiting for a key").clicked() {
                    window.catching = None;
                }
            });
            ui.separator();
            egui::ScrollArea::vertical().show(ui, |ui| {
                let mut place = None;
                egui::Grid::new("keys")
                    .num_columns(2)
                    .striped(true)
                    .show(ui, |ui| {
                        for about in ACTIONS {
                            if place != Some(about.place) {
                                place = Some(about.place);
                                ui.label(
                                    egui::RichText::new(match about.place {
                                        Place::View => "In the level's view",
                                        Place::Flying => "Flying, with the right mouse button held",
                                        Place::Conversation => "Writing a line of a conversation",
                                    })
                                    .strong(),
                                );
                                ui.end_row();
                            }
                            ui.label(about.does);
                            ui.horizontal(|ui| {
                                let chords = state.keys.map.of(about.action).to_vec();
                                let clashes = state.keys.map.clashes(about.action);
                                for (at, chord) in chords.iter().enumerate() {
                                    let waiting = window.catching == Some((about.action, Some(at)));
                                    let mut text = egui::RichText::new(if waiting {
                                        "press a key".to_string()
                                    } else {
                                        chord.shown()
                                    });
                                    let clash =
                                        clashes.iter().find(|(clashing, _)| clashing == chord);
                                    if clash.is_some() {
                                        text = text.color(egui::Color32::from_rgb(230, 150, 70));
                                    }
                                    let mut button = ui.selectable_label(waiting, text);
                                    if let Some((_, other)) = clash {
                                        button = button.on_hover_text(format!(
                                            "{} has this key too.",
                                            about_text(*other)
                                        ));
                                    }
                                    if button.clicked() {
                                        window.catching = Some((about.action, Some(at)));
                                    }
                                    if ui
                                        .small_button("x")
                                        .on_hover_text("Takes this key away.")
                                        .clicked()
                                    {
                                        let mut rest = chords.clone();
                                        rest.remove(at);
                                        command = Some(set_command(about.action, &rest));
                                    }
                                }
                                let waiting = window.catching == Some((about.action, None));
                                if ui
                                    .selectable_label(
                                        waiting,
                                        if waiting { "press a key" } else { "+" },
                                    )
                                    .on_hover_text("Adds one more key for this action.")
                                    .clicked()
                                {
                                    window.catching = Some((about.action, None));
                                }
                            });
                            ui.end_row();
                        }
                    });
            });
        });
    if !open {
        window.open = false;
        window.catching = None;
    }
    if let Some(command) = command {
        state.pending_command = Some(command);
    }
}

fn about_text(action: Action) -> &'static str {
    about(action).does
}

/// The command that gives `action` the keys `chords`.
fn set_command(action: Action, chords: &[Chord]) -> String {
    let chords: Vec<String> = chords.iter().map(|chord| chord.shown()).collect();
    if chords.is_empty() {
        format!("keys clear {}", about(action).name)
    } else {
        format!("keys set {} {}", about(action).name, chords.join(" "))
    }
}

/// `edit keys`: lists every action with its keys (`edit keys`), gives an
/// action keys (`set ACTION KEY...`), takes them all away (`clear
/// ACTION`), or gives every action its default keys (`reset`), or one
/// action (`reset ACTION`). A change writes the keys file.
pub(crate) fn command(state: &mut crate::state::EditorState, words: &[&str]) -> Result<(), String> {
    let action_of = |name: &str| {
        ACTIONS
            .iter()
            .find(|about| about.name == name)
            .map(|about| about.action)
            .ok_or_else(|| format!("there is no action {name:?} (edit keys lists them)"))
    };
    match words {
        [] => {
            let lines: Vec<String> = ACTIONS
                .iter()
                .map(|about| format!("{} = {}", about.name, state.keys.map.shown(about.action)))
                .collect();
            state.answer = Some(lines.join("; "));
            return Ok(());
        }
        ["set", name, chords @ ..] if !chords.is_empty() => {
            let action = action_of(name)?;
            let chords = chords
                .iter()
                .map(|text| Chord::parse(text))
                .collect::<Result<Vec<Chord>, String>>()?;
            state.keys.map.set(action, chords);
        }
        ["clear", name] => {
            let action = action_of(name)?;
            state.keys.map.set(action, Vec::new());
        }
        ["reset"] => state.keys.map = Keymap::default(),
        ["reset", name] => {
            let action = action_of(name)?;
            let defaults = Keymap::default().of(action).to_vec();
            state.keys.map.set(action, defaults);
        }
        _ => {
            return Err(
                "edit keys takes set ACTION KEY..., clear ACTION, or reset [ACTION]".into(),
            );
        }
    }
    state.keys.save()?;
    let saved =
        (state.keys.file.as_ref()).map_or_else(String::new, |file| file.display().to_string());
    state.say(format!("keys saved to {saved}"));
    Ok(())
}

/// The keys of every action as a dump shows them.
pub(crate) fn facts(map: &Keymap) -> serde_json::Value {
    ACTIONS
        .iter()
        .map(|about| {
            let chords: Vec<String> = map
                .of(about.action)
                .iter()
                .map(|chord| chord.shown())
                .collect();
            (about.name.to_string(), serde_json::json!(chords))
        })
        .collect::<serde_json::Map<String, serde_json::Value>>()
        .into()
}

/// The fly controls' keys (`FlyControls::keys`) as the map gives them.
pub(crate) fn fly_keys(map: &Keymap) -> kjk_game::scene::FlyKeys {
    let codes = |action| map.of(action).iter().map(|chord| chord.code()).collect();
    kjk_game::scene::FlyKeys {
        forward: codes(Action::FlyForward),
        back: codes(Action::FlyBack),
        left: codes(Action::FlyLeft),
        right: codes(Action::FlyRight),
        down: codes(Action::FlyDown),
        up: codes(Action::FlyUp),
        faster: map
            .of(Action::FlyFaster)
            .iter()
            .flat_map(|chord| match chord.code() {
                KeyCode::ShiftLeft => vec![KeyCode::ShiftLeft, KeyCode::ShiftRight],
                KeyCode::ControlLeft => vec![KeyCode::ControlLeft, KeyCode::ControlRight],
                KeyCode::AltLeft => vec![KeyCode::AltLeft, KeyCode::AltRight],
                code => vec![code],
            })
            .collect(),
    }
}
