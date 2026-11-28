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
/// The pad button that cancels an entry (it opens the menu in play).
const PAD_CANCEL: &str = "Start";
/// LB, the button held for a chord's second layer. LB alone may also be
/// bound (Block is, by default).
const PAD_SHIFT: &str = "LeftTrigger";
/// The second layer's prefix in a chord's name (`LB+South`).
const PAD_LAYER: &str = "LB+";

/// The entry's instructions while a pad binds. `{shift}` and `{menu}` are
/// the pad's names for LB and Start.
const PRESS_PAD: &str = "Press a button. Hold {shift} and press another button for a chord with {shift}. Press {shift} alone and let it go to bind {shift} alone. Press {menu} to cancel.";

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

/// A binding's setting: its key and value (`none` for no binding).
fn setting(device: Device, action: &str, key: &str) -> (String, String) {
    let value = if key.is_empty() { "none" } else { key };
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
/// the action is bound to now: the pad's button while a pad plays, falling
/// back to the keyboard binding for keyboard-only actions (such as
/// `{console}`). A brace that names no action stays as written.
pub fn with_bindings(text: &str, vm: &ViewModel) -> String {
    let substitute = |text: String, device: Device| {
        device.rows(vm).iter().fold(text, |text, row| {
            text.replace(&format!("{{{}}}", row.action), &device.shown(&row.key, vm))
        })
    };
    let text = substitute(
        text.to_string(),
        if vm.pad_playing {
            Device::Pad
        } else {
            Device::Keys
        },
    );
    if vm.pad_playing && text.contains('{') {
        substitute(text, Device::Keys)
    } else {
        text
    }
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
/// mouse`); no binding (empty, or `none`) as a dash.
pub fn key_name(binding: &str) -> String {
    if binding.is_empty() || binding == "none" {
        return strings::UNBOUND.to_string();
    }
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
    ("PrintScreen", "Print Screen"),
    ("ContextMenu", "Menu key"),
];

/// A chord's name as shown, in the names of the pad played with last
/// (`LB+South` as `LB+A`, `RightTrigger` as `RB`). An unbound one shows as
/// a dash.
pub fn chord_name(chord: &str, vm: &ViewModel) -> String {
    let button = |name: &str| {
        vm.pad_names
            .iter()
            .find(|(button, _)| button == name)
            .map_or_else(|| name.to_string(), |(_, shown)| shown.clone())
    };
    match chord.strip_prefix(PAD_LAYER) {
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
    /// The binding asked for that other actions have, and whose it is.
    clash: Option<(String, Vec<KeyRow>)>,
    /// The modifier keys held down since the entry opened, in the order
    /// they were pressed.
    held: Vec<String>,
    /// Whether LB went down on a pad and no other button has been pressed
    /// since. LB let go then binds LB alone (`pad_chord`).
    shift_waits: bool,
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
            shift_waits: false,
        }
    }

    /// The pad binding a raw pad input makes (`Pad:LB+South`, or a button let
    /// go as `Released:Pad:LeftTrigger`). LB pressed waits, because it is
    /// also the layer's button. If another button is pressed while LB is
    /// held, the binding is the chord of the two (`LB+South`). If LB is let
    /// go first, the binding is LB alone. None while LB waits, and for a
    /// button let go that binds nothing.
    fn pad_chord(&mut self, key: &str) -> Option<String> {
        let shift =
            |name: &str| name == PAD_SHIFT || name.strip_prefix(PAD_LAYER) == Some(PAD_SHIFT);
        if let Some(released) = key
            .strip_prefix(RELEASED_RAW)
            .and_then(|key| key.strip_prefix(PAD_RAW))
        {
            let alone = shift(released) && std::mem::take(&mut self.shift_waits);
            return alone.then(|| PAD_SHIFT.to_string());
        }
        let pressed = key.strip_prefix(PAD_RAW)?;
        self.shift_waits = shift(pressed);
        (!self.shift_waits).then(|| pressed.to_string())
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

    /// The actions other than this one on `binding` that it may not share
    /// with. Several actions may share one binding (Block, the alternate
    /// attack and Aim share the right mouse button), so a swap moves them
    /// all.
    fn others(&self, binding: &str, vm: &ViewModel) -> Vec<KeyRow> {
        let mine = self
            .device
            .rows(vm)
            .iter()
            .find(|row| row.action == self.action);
        self.device
            .rows(vm)
            .iter()
            .filter(|row| {
                row.key == binding
                    && row.action != self.action
                    && !row.shares.contains(&self.action)
                    && !mine.is_some_and(|mine| mine.shares.contains(&row.action))
            })
            .cloned()
            .collect()
    }

    /// Whether an action that stays on the binding this one leaves
    /// (`current`) may not share it with `moved`. Such a moved action is
    /// left unbound instead. For example, Aim bound onto E would move Use to
    /// the right mouse button, where Block and the alternate attack stay.
    fn refused_at_current(&self, moved: &KeyRow, vm: &ViewModel) -> bool {
        !self.current.is_empty()
            && self.device.rows(vm).iter().any(|row| {
                row.key == self.current
                    && row.action != self.action
                    && row.action != moved.action
                    && !row.shares.contains(&moved.action)
                    && !moved.shares.contains(&row.action)
            })
    }

    /// What the entry says when `binding` is `others`'. When every one of
    /// them can take the old key, the next press swaps them. When some
    /// cannot (`refused_at_current`), it says which actions move to the old
    /// key and which are left without a key.
    fn clash_message(&self, binding: &str, others: &[KeyRow], vm: &ViewModel) -> String {
        let shown = self.device.shown(binding, vm);
        let (left, moved): (Vec<&KeyRow>, Vec<&KeyRow>) = others
            .iter()
            .partition(|other| self.refused_at_current(other, vm));
        let all: Vec<&KeyRow> = others.iter().collect();
        let named = format!("{shown} is {}.", owners(&all));
        if left.is_empty() {
            return format!("{named} Press {shown} again to swap them.");
        }
        let mut message = format!("{named} Press {shown} again to bind {} to it.", self.label);
        if !moved.is_empty() {
            let old = self.device.shown(&self.current, vm);
            message += &format!(" {} will move to {old}.", names(&moved));
        }
        message + &format!(" {} will be left without a key.", names(&left))
    }
}

/// The actions whose binding is asked for, as the clash message names
/// them: "Use's", or "Block's, Alt attack's and Aim's".
fn owners(rows: &[&KeyRow]) -> String {
    listed(rows.iter().map(|row| format!("{}'s", row.label)).collect())
}

/// The actions of `rows` by name, as the clash message lists them: "Use",
/// or "Block, Alt attack and Aim".
fn names(rows: &[&KeyRow]) -> String {
    listed(rows.iter().map(|row| row.label.clone()).collect())
}

/// `names` joined as a sentence lists them: "a", "a and b", "a, b and c".
fn listed(names: Vec<String>) -> String {
    match names.split_last() {
        Some((last, rest)) if !rest.is_empty() => format!("{} and {last}", rest.join(", ")),
        _ => names.concat(),
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
                    PRESS_PAD
                        .replace("{shift}", &chord_name(PAD_SHIFT, vm))
                        .replace("{menu}", &chord_name(PAD_CANCEL, vm))
                } else {
                    strings::PRESS_KEY.to_string()
                };
                let now = match &self.clash {
                    Some((binding, others)) => self.clash_message(binding, others, vm),
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
            (Device::Pad, _) if pad.is_some() || key.starts_with(RELEASED_RAW) => {
                match self.pad_chord(key) {
                    Some(chord) => {
                        made = chord;
                        made.as_str()
                    }
                    // LB waiting, or a button let go that binds nothing.
                    None => return pad.is_some(),
                }
            }
            _ => return true,
        };
        let prefix = self.device.prefix();
        if binding == self.current {
            out.push(Intent::Close);
            return true;
        }
        let others = self.others(binding, vm);
        if !others.is_empty() && self.clash.as_ref().map(|(b, _)| b.as_str()) != Some(binding) {
            self.clash = Some((binding.to_string(), others));
            return true;
        }
        for other in &others {
            let to = if self.refused_at_current(other, vm) {
                ""
            } else {
                self.current.as_str()
            };
            let (key, value) = setting(self.device, &other.action, to);
            out.push(Intent::Setting(key, value));
        }
        out.push(Intent::Setting(
            format!("{prefix}.{}", self.action),
            binding.to_string(),
        ));
        out.push(Intent::Close);
        true
    }
}
