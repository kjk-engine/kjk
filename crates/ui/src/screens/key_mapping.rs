//! The key mapping (`optkeymapping`): every action and its binding, on the
//! keyboard's movement keys, its game keys, or a pad's buttons (the
//! layout's third filter, KOTOR's minigame keys, is the pad's here);
//! activating one asks for its new key or button (`optkeyentry`). It opens
//! on the tab of what the player plays with (`ViewModel::pad_playing`). Accept
//! keeps the changes, Cancel puts back what the page opened with (both
//! devices), Default restores the shown device's own.
//!
//! A binding another action has is not taken from it silently: the entry
//! says whose it is, and the same key again swaps the two (the other takes
//! this one's old binding), as the accessibility guidelines ask of a
//! remapping (XAG 107). Actions that share a binding by design (`shares`)
//! keep sharing.

use fmt_odyssey::gui::Control;

use crate::intent::Intent;
use crate::paint::{ControlState, ListView, Row};
use crate::screen::{Screen, ScreenId};
use crate::strings;
use crate::vm::{KeyRow, ViewModel};

/// What a binding is on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Device {
    Keys,
    Pad,
}

/// A pad button's name as the raw input carries it (`Pad:LB+South`).
pub const PAD_RAW: &str = "Pad:";
/// The pad button that cancels an entry (it opens the menu in play), and
/// the one held for a chord's second layer.
const PAD_CANCEL: &str = "Start";
const PAD_SHIFT: &str = "LeftTrigger";

impl Device {
    /// The settings' prefix of its bindings (`key.jump`, `pad.jump`).
    fn prefix(self) -> &'static str {
        match self {
            Device::Keys => "key",
            Device::Pad => "pad",
        }
    }

    fn rows(self, vm: &ViewModel) -> &[KeyRow] {
        match self {
            Device::Keys => &vm.keys,
            Device::Pad => &vm.pad_keys,
        }
    }

    /// A binding as the page shows it.
    fn shown(self, key: &str, vm: &ViewModel) -> String {
        match self {
            Device::Keys => key_name(key),
            Device::Pad => chord_name(key, vm),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Tab {
    Movement,
    Game,
    Pad,
}

pub struct KeyMapping {
    tab: Tab,
    /// The tab shown when the lists were last asked (`lists_anew`).
    listed: Tab,
    /// Each binding's setting when the page opened (`key.jump` = `Space`).
    before: Vec<(String, String)>,
}

impl KeyMapping {
    pub fn new(vm: &ViewModel) -> KeyMapping {
        let before = [Device::Keys, Device::Pad]
            .into_iter()
            .flat_map(|device| {
                device
                    .rows(vm)
                    .iter()
                    .map(move |row| setting(device, &row.action, &row.key))
            })
            .collect();
        let tab = if vm.pad_playing {
            Tab::Pad
        } else {
            Tab::Movement
        };
        KeyMapping {
            tab,
            listed: tab,
            before,
        }
    }

    fn device(&self) -> Device {
        match self.tab {
            Tab::Pad => Device::Pad,
            Tab::Movement | Tab::Game => Device::Keys,
        }
    }

    /// The tab's rows; the pad's bound ones first, those it leaves to the
    /// keyboard (none bound) after them.
    fn shown<'a>(&self, vm: &'a ViewModel) -> impl Iterator<Item = &'a KeyRow> + use<'a> {
        let tab = self.tab;
        let rows = self.device().rows(vm);
        let shown = move |bound: bool| {
            rows.iter().filter(move |row| match tab {
                Tab::Movement => bound && row.movement,
                Tab::Game => bound && !row.movement,
                Tab::Pad => row.key.is_empty() != bound,
            })
        };
        shown(true).chain(shown(false))
    }
}

/// A row's height and its words' indent (layout pixels): a line of the
/// list's font with a binding's buttons beside it.
const ROW: i32 = 24;
const INDENT: i32 = 10;

/// A binding's setting: its key and value.
fn setting(device: Device, action: &str, key: &str) -> (String, String) {
    let value = match (device, key) {
        (Device::Pad, "") => "none",
        _ => key,
    };
    (format!("{}.{action}", device.prefix()), value.to_string())
}

fn filter(selected: bool) -> ControlState {
    ControlState {
        selected,
        ..ControlState::default()
    }
}

impl Screen for KeyMapping {
    fn tabs(&self) -> Vec<&'static str> {
        vec!["BTN_Filter_Move", "BTN_Filter_Game", "BTN_Filter_Mini"]
    }

    /// Back puts the bindings back as they were (KOTOR's Cancel).
    fn cancels(&self) -> bool {
        true
    }

    fn accept(&self) -> Option<&str> {
        Some("BTN_Accept")
    }

    fn id(&self) -> ScreenId {
        ScreenId("options_keymap")
    }

    fn layout(&self) -> &str {
        "optkeymapping"
    }

    fn state(&self, control: &Control, vm: &ViewModel) -> ControlState {
        let device = self.device();
        match control.tag.as_str() {
            "LST_EventList" => ControlState {
                list: Some(ListView {
                    rows: self
                        .shown(vm)
                        .map(|row| {
                            // The action at the row's left, its binding at
                            // its right: a pad's as its buttons (`icons`).
                            let drawn = device == Device::Pad && !row.key.is_empty();
                            Row {
                                text: strings::bytes(&row.label),
                                badge: drawn
                                    .then(|| crate::icons::pad_texture(&vm.pad_kind, &row.key)),
                                value: (!drawn)
                                    .then(|| strings::bytes(&device.shown(&row.key, vm))),
                                ..Row::default()
                            }
                        })
                        .collect(),
                    ..ListView::default()
                }),
                ..ControlState::default()
            },
            "LBL_Title" => ControlState {
                text: Some(strings::bytes(match device {
                    Device::Keys => strings::KEYS_TITLE,
                    Device::Pad => strings::PAD_TITLE,
                })),
                ..ControlState::default()
            },
            "BTN_Filter_Move" => filter(self.tab == Tab::Movement),
            "BTN_Filter_Game" => filter(self.tab == Tab::Game),
            "BTN_Filter_Mini" => ControlState {
                text: Some(strings::bytes(strings::CONTROLLER)),
                ..filter(self.tab == Tab::Pad)
            },
            _ => ControlState::default(),
        }
    }

    fn press(&mut self, tag: &str, _: &ViewModel, out: &mut Vec<Intent>) {
        match tag {
            "BTN_Filter_Move" => self.tab = Tab::Movement,
            "BTN_Filter_Game" => self.tab = Tab::Game,
            "BTN_Filter_Mini" => self.tab = Tab::Pad,
            "BTN_Accept" => out.push(Intent::Close),
            "BTN_Cancel" => self.back(out),
            "BTN_Default" => {
                let reset = match self.device() {
                    Device::Keys => "keys",
                    Device::Pad => "pad",
                };
                out.push(Intent::Setting("reset".into(), reset.into()));
            }
            _ => {}
        }
    }

    fn row(&mut self, _: &str, index: usize, vm: &ViewModel, out: &mut Vec<Intent>) {
        if let Some(row) = self.shown(vm).nth(index) {
            let (key, _) = setting(self.device(), &row.action, &row.key);
            out.push(Intent::OpenFor("key_entry", key));
        }
    }

    fn back(&mut self, out: &mut Vec<Intent>) {
        for (key, value) in &self.before {
            out.push(Intent::Setting(key.clone(), value.clone()));
        }
        out.push(Intent::Close);
    }

    fn first_focus(&self) -> Option<&str> {
        Some("LST_EventList")
    }

    /// Compact rows: a line each, the action at its left and its binding's
    /// buttons at its right, no framed box around each (KOTOR's are a
    /// box two lines tall).
    fn shape(&self, gui: &mut fmt_odyssey::gui::Gui) {
        use fmt_odyssey::gui::align;
        let list = gui
            .controls
            .iter_mut()
            .find(|control| control.tag == "LST_EventList");
        let Some(proto) = list.and_then(|list| list.proto.as_deref_mut()) else {
            return;
        };
        proto.extent.height = ROW;
        proto.extent.left += INDENT;
        proto.extent.width -= INDENT;
        proto.border = None;
        // The focused row a bar of the highlight's fill, edgeless.
        for lit in [
            &mut proto.hilight,
            &mut proto.selected,
            &mut proto.hilight_selected,
        ]
        .into_iter()
        .flatten()
        {
            lit.corner.clear();
            lit.edge.clear();
            lit.dimension = 0;
            lit.inner_offset = 0;
            lit.inner_offset_y = Some(0);
        }
        if let Some(text) = proto.text.as_mut() {
            text.alignment = align::LEFT | align::MIDDLE;
        }
    }

    /// Another tab's list starts at its top.
    fn lists_anew(&mut self) -> bool {
        std::mem::replace(&mut self.listed, self.tab) != self.tab
    }
}

/// `text` with each `{action}` (a settings' name, as `{zoom_in}`) as what
/// the action is bound to now: the pad's button while a pad plays, else
/// the key. A brace that names no action stays as written.
pub fn with_bindings(text: &str, vm: &ViewModel) -> String {
    let device = if vm.pad_playing {
        Device::Pad
    } else {
        Device::Keys
    };
    device.rows(vm).iter().fold(text.to_string(), |text, row| {
        text.replace(&format!("{{{}}}", row.action), &device.shown(&row.key, vm))
    })
}

/// The modifier keys a binding can ask to be held with its key or button,
/// each by the name the settings file uses before a plus sign
/// (`Shift+MouseRight`), and its left and right keys.
pub const MODIFIERS: [(&str, [&str; 2]); 3] = [
    ("Ctrl", ["ControlLeft", "ControlRight"]),
    ("Shift", ["ShiftLeft", "ShiftRight"]),
    ("Alt", ["AltLeft", "AltRight"]),
];

/// Whether a key is one of the modifier keys (`ShiftLeft` is).
fn is_modifier(key: &str) -> bool {
    MODIFIERS.iter().any(|(_, sides)| sides.contains(&key))
}

/// A binding's name as shown: its modifiers first, each followed by a plus
/// sign, then its key or button (`Shift+MouseRight` as `Shift + Right
/// mouse`).
pub fn key_name(binding: &str) -> String {
    let (mods, key) = binding.rsplit_once('+').unwrap_or(("", binding));
    mods.split('+')
        .filter(|part| !part.is_empty())
        .map(str::to_string)
        .chain([input_name(key)])
        .collect::<Vec<_>>()
        .join(" + ")
}

/// A key's or mouse button's name as shown: `KeyW` as `W`, `Digit1` as
/// `1`, `ShiftLeft` as `Left Shift`, `Numpad5` as `Num 5`, `MouseBack` as
/// `Mouse 4`; one not known as its own name.
fn input_name(key: &str) -> String {
    let named = NAMES.iter().find(|(name, _)| *name == key);
    if let Some((_, shown)) = named {
        return shown.to_string();
    }
    if let Some(n) = key
        .strip_prefix("Mouse")
        .and_then(|n| n.parse::<u16>().ok())
    {
        return format!("Mouse {n}");
    }
    let shown = key
        .strip_prefix("Key")
        .or_else(|| key.strip_prefix("Digit"))
        .unwrap_or(key);
    match shown.strip_prefix("Numpad") {
        Some(rest) => format!("Num {rest}"),
        None => shown.to_string(),
    }
}

/// The names said otherwise than by their prefix's rule.
const NAMES: &[(&str, &str)] = &[
    ("MouseLeft", "Left mouse"),
    ("MouseRight", "Right mouse"),
    ("MouseMiddle", "Middle mouse"),
    ("MouseBack", "Mouse 4"),
    ("MouseForward", "Mouse 5"),
    ("ShiftLeft", "Left Shift"),
    ("ShiftRight", "Right Shift"),
    ("ControlLeft", "Left Ctrl"),
    ("ControlRight", "Right Ctrl"),
    ("AltLeft", "Left Alt"),
    ("AltRight", "Right Alt"),
    ("SuperLeft", "Left Win"),
    ("SuperRight", "Right Win"),
    ("CapsLock", "Caps Lock"),
    ("ArrowUp", "Up"),
    ("ArrowDown", "Down"),
    ("ArrowLeft", "Left"),
    ("ArrowRight", "Right"),
    ("PageUp", "Page Up"),
    ("PageDown", "Page Down"),
    ("BracketLeft", "["),
    ("BracketRight", "]"),
    ("Comma", ","),
    ("Period", "."),
    ("Semicolon", ";"),
    ("Quote", "'"),
    ("Slash", "/"),
    ("Backslash", "\\"),
    ("IntlBackslash", "\\"),
    ("Minus", "-"),
    ("Equal", "="),
    ("Backquote", "`"),
    ("NumpadAdd", "Num +"),
    ("NumpadSubtract", "Num -"),
    ("NumpadMultiply", "Num *"),
    ("NumpadDivide", "Num /"),
    ("NumpadDecimal", "Num ."),
    ("NumpadEnter", "Num Enter"),
    ("NumLock", "Num Lock"),
    ("ScrollLock", "Scroll Lock"),
    ("ContextMenu", "Menu key"),
];

/// A chord's name as shown, in the names of the pad played with last
/// (`LB+South` as `LB+A`); an unbound one as a dash.
pub fn chord_name(chord: &str, vm: &ViewModel) -> String {
    let button = |name: &str| {
        vm.pad_names
            .iter()
            .find(|(button, _)| button == name)
            .map_or_else(|| name.to_string(), |(_, shown)| shown.clone())
    };
    match chord.strip_prefix("LB+") {
        _ if chord.is_empty() || chord == "none" => strings::UNBOUND.to_string(),
        Some(rest) => format!("{}+{}", button(PAD_SHIFT), button(rest)),
        None => button(chord),
    }
}

/// Layout pixels the entry's message box grows by.
const ENTRY_GROW: i32 = 5 * 18;

/// The words of a message box, a paragraph a row, each wrapped to it.
fn paragraphs(words: &[&str]) -> ControlState {
    ControlState {
        list: Some(ListView {
            rows: words
                .iter()
                .map(|words| Row {
                    text: strings::bytes(words),
                    ..Row::default()
                })
                .collect(),
            fit: true,
            ..ListView::default()
        }),
        ..ControlState::default()
    }
}

/// The raw input's name of a key let go (`Released:ShiftLeft`), which only
/// the key entry reads.
pub const RELEASED_RAW: &str = "Released:";

/// Asking for an action's new key or button (KOTOR's message box): the next one
/// pressed is it; Escape, a pad's Menu or Cancel keeps the old one. One
/// another action has is named first, and taken (swapped) on a second
/// press.
///
/// On the keyboard and mouse, a modifier key (Ctrl, Shift or Alt) waits.
/// If another key or a mouse button is pressed while it is held, the binding
/// is the two together (Shift + Right mouse). If the modifier is let go
/// first, the binding is the modifier key alone (Left Shift).
pub struct KeyEntry {
    device: Device,
    action: String,
    label: String,
    current: String,
    /// The binding asked for that another action has, and whose it is.
    clash: Option<(String, KeyRow)>,
    /// The modifier keys held down since the entry opened, in the order
    /// they were pressed.
    held: Vec<String>,
}

impl KeyEntry {
    /// For a binding's setting (`key.jump`, `pad.jump`).
    pub fn new(setting: &str, vm: &ViewModel) -> KeyEntry {
        let (device, action) = match setting.split_once('.') {
            Some(("pad", action)) => (Device::Pad, action),
            Some((_, action)) => (Device::Keys, action),
            None => (Device::Keys, setting),
        };
        let row = device.rows(vm).iter().find(|row| row.action == action);
        KeyEntry {
            device,
            action: action.to_string(),
            label: row.map_or(action.to_string(), |row| row.label.clone()),
            current: row.map_or(String::new(), |row| row.key.clone()),
            clash: None,
            held: Vec::new(),
        }
    }

    /// The binding a key or button pressed now makes, with the modifiers
    /// held (`Ctrl+Shift+KeyE`), in the order of `MODIFIERS`. None while
    /// the key is a modifier, which waits (`KeyEntry`).
    fn chord(&mut self, key: &str) -> Option<String> {
        if let Some(released) = key.strip_prefix(RELEASED_RAW) {
            let first = self.held.first().is_some_and(|held| held == released);
            self.held.retain(|held| held != released);
            return first.then(|| released.to_string());
        }
        if is_modifier(key) {
            if !self.held.iter().any(|held| held == key) {
                self.held.push(key.to_string());
            }
            return None;
        }
        let mods = MODIFIERS
            .iter()
            .filter(|(_, sides)| self.held.iter().any(|held| sides.contains(&held.as_str())));
        Some(
            mods.map(|(name, _)| format!("{name}+"))
                .chain([key.to_string()])
                .collect(),
        )
    }

    /// The action other than this one on `binding` it may not share with.
    fn other(&self, binding: &str, vm: &ViewModel) -> Option<KeyRow> {
        let mine = self
            .device
            .rows(vm)
            .iter()
            .find(|row| row.action == self.action);
        self.device
            .rows(vm)
            .iter()
            .find(|row| {
                row.key == binding
                    && row.action != self.action
                    && !row.shares.contains(&self.action)
                    && !mine.is_some_and(|mine| mine.shares.contains(&row.action))
            })
            .cloned()
    }
}

impl Screen for KeyEntry {
    fn id(&self) -> ScreenId {
        ScreenId("key_entry")
    }

    /// KOTOR's message box (its own `optkeyentry` has no panel under it in
    /// TSL's art): what is bound, how, and what it is on now or whose the
    /// binding asked for is.
    fn layout(&self) -> &str {
        "confirm"
    }

    fn state(&self, control: &Control, vm: &ViewModel) -> ControlState {
        let pad = self.device == Device::Pad;
        match control.tag.as_str() {
            "LB_MESSAGE" => {
                let how = if pad {
                    strings::PRESS_BUTTON
                        .replace("{shift}", &chord_name(PAD_SHIFT, vm))
                        .replace("{menu}", &chord_name(PAD_CANCEL, vm))
                } else {
                    strings::PRESS_KEY.to_string()
                };
                let now = match &self.clash {
                    Some((binding, other)) => {
                        let shown = self.device.shown(binding, vm);
                        format!(
                            "{shown} is {}'s. Press {shown} again to swap them.",
                            other.label
                        )
                    }
                    None => format!(
                        "{} {}",
                        strings::NOW_ON,
                        self.device.shown(&self.current, vm)
                    ),
                };
                paragraphs(&[&self.label, &how, &now])
            }
            "BTN_OK" => ControlState {
                hidden: true,
                ..ControlState::default()
            },
            "BTN_CANCEL" => ControlState {
                text: Some(strings::bytes(strings::CANCEL)),
                ..ControlState::default()
            },
            _ => ControlState::default(),
        }
    }

    fn press(&mut self, tag: &str, _: &ViewModel, out: &mut Vec<Intent>) {
        if tag == "BTN_CANCEL" {
            out.push(Intent::Close);
        }
    }

    fn first_focus(&self) -> Option<&str> {
        Some("BTN_CANCEL")
    }

    /// Room for its three paragraphs, the instructions two lines.
    fn shape(&self, gui: &mut fmt_odyssey::gui::Gui) {
        super::message::taller(gui, ENTRY_GROW);
    }

    /// Every key or button pressed is the entry's (none presses the page's
    /// own buttons); one of the other device is let be. A modifier key
    /// pressed, or any key let go, is noted and passed on.
    fn raw(&mut self, key: &str, vm: &ViewModel, out: &mut Vec<Intent>) -> bool {
        let pad = key.strip_prefix(PAD_RAW);
        if key == "Escape" || pad == Some(PAD_CANCEL) {
            out.push(Intent::Close);
            return true;
        }
        let made;
        let binding = match (self.device, pad) {
            (Device::Keys, None) => match self.chord(key) {
                Some(chord) => {
                    made = chord;
                    made.as_str()
                }
                // A modifier waiting, or a key let go: the frame's other
                // presses still come here (a click with Shift pressed in the
                // same frame).
                None => return false,
            },
            (Device::Pad, Some(chord)) => chord,
            _ => return true,
        };
        let prefix = self.device.prefix();
        if binding == self.current {
            out.push(Intent::Close);
            return true;
        }
        match self.other(binding, vm) {
            Some(other) if self.clash.as_ref().map(|(b, _)| b.as_str()) != Some(binding) => {
                self.clash = Some((binding.to_string(), other));
                return true;
            }
            Some(other) => {
                let (key, value) = setting(self.device, &other.action, &self.current);
                out.push(Intent::Setting(key, value));
            }
            None => {}
        }
        out.push(Intent::Setting(
            format!("{prefix}.{}", self.action),
            binding.to_string(),
        ));
        out.push(Intent::Close);
        true
    }
}
