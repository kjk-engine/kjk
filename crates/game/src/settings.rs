//! The player's settings, kept in a file of their own in the user's
//! config folder (never the game's; `KJK_SETTINGS` names another): the mouse's feel, the keys, the
//! sound's volumes and the window (`display`). The
//! options screens change them (`Intent::Setting`); each change is saved
//! at once and shown back to the screens. The file also keeps the
//! install's host id (`Settings::host_id`), made once.
//!
//! The duel options (`DuelOptions`): the saber rules a session played here
//! plays by (`game.saber_rules`, JA's or a duel mode, `kjk play
//! --saber-rules` holding another for one run), and how the duel modes
//! show: the duel bars over saber users' heads, the red warning of an
//! unblockable swing, and a perfect block's slow motion (`duel_view`,
//! `sim_host`).

use std::collections::BTreeMap;
use std::path::PathBuf;

use bevy::prelude::*;
use sim::combat::saber::SaberRules;
use ui::intent::Intent;
use ui::vm::KeyRow;

use crate::camera_tune;
use crate::display::{Display, DisplayMode};
use crate::sound::Volumes;
use crate::ui_view::{Ui, UiIntents};
use audio::Bus;

/// What a key does in a game.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Action {
    Forward,
    Backward,
    Left,
    Right,
    Jump,
    Crouch,
    Walk,
    /// Fire or swing the weapon, and its other mode (a blaster's feat
    /// special, a throw, a kick).
    Attack,
    AltAttack,
    Use,
    Security,
    SaberStyle,
    /// The next feat attack of the weapon in hand (`feat_view`).
    NextFeat,
    /// The ten slots of the action bar, in order (`BAR`).
    Bar1,
    Bar2,
    Bar3,
    Bar4,
    Bar5,
    Bar6,
    Bar7,
    Bar8,
    Bar9,
    Bar10,
    /// Held: the power wheel is open.
    Wheel,
    /// The screen that sets up the action bar.
    BarSetup,
    Stealth,
    /// Hold the blade up to block, under the duel saber rules
    /// (`sim::combat::saber::duel`). With attack it aims the block.
    Block,
    /// Kick. Under the duel saber rules a character kicks with any weapon
    /// in hand. Under Jedi Academy's rules only a character fighting in the
    /// staff style kicks, as its alternate attack does.
    Kick,
    /// The alternate attack on a key of its own. By default Block shares
    /// the alternate attack's button and takes its press under the duel
    /// saber rules (`sim_host::share_button`), so this key throws the
    /// lightsaber there. Pressed with attack, it starts the kata.
    DuelAltAttack,
    /// Put the weapon in hand away (`sim::action::Action::Holster`).
    Holster,
    /// Exchange the weapon sets (`sim::action::Action::SwitchWeapons`).
    SwitchWeapons,
    /// TSL's next Form the character knows (`form_view`).
    Form,
    RightHand,
    LeftHand,
    Inventory,
    Journal,
    Map,
    Party,
    Abilities,
    QuickSave,
    QuickLoad,
    /// The next party member leads (KOTOR's `ChangeChar`, Tab).
    Leader,
    /// Solo mode on or off (both games' `PartyActive`, their V: here Next Form's).
    Solo,
    /// While the seat watches (I30 P3): follow the next party member.
    Follow,
    Break,
    Ready,
    /// Turn between the own eyes and JA's camera (`play_view::CameraMode`).
    View,
    /// The camera a step closer or farther (`camera_tune::zoomed`); past
    /// the nearest, the own eyes.
    ZoomIn,
    ZoomOut,
    /// Held: the mouse (a pad's right stick) turns the camera around the
    /// character, not the character (`camera_tune::Orbit`).
    Orbit,
    /// The camera's shoulder to the other side.
    SwapShoulder,
    /// Leave the conversation while it is unlocked (R6, `dialogue_view`).
    LeaveTalk,
    /// The host lets the first guest asking in join, or ends that
    /// conversation (R5, `dialogue_view`).
    AcceptCall,
    EndCall,
    /// Make a bug report (`report`).
    Report,
}

/// The action bar's slots, from the first to the tenth.
pub const BAR: [Action; sim::combat::bar::SLOTS] = [
    Action::Bar1,
    Action::Bar2,
    Action::Bar3,
    Action::Bar4,
    Action::Bar5,
    Action::Bar6,
    Action::Bar7,
    Action::Bar8,
    Action::Bar9,
    Action::Bar10,
];

/// How many of the bar's slots a controller can use, counted from the
/// first. Slots 5 to 10 have no buttons on a controller, and the HUD shows
/// only the first four slots while a controller is in use.
pub const PAD_BAR: usize = 4;

/// The value of a `key.<action>` or `pad.<action>` setting that leaves the
/// action with no key or chord.
pub(crate) const UNBOUND: &str = "none";

/// Each action: its settings name, its label, its default key or mouse
/// button (none for an action that has no key until the player binds one),
/// and whether it is a movement key.
pub(crate) const ACTIONS: [(Action, &str, &str, Option<Input>, bool); 55] = [
    (
        Action::Forward,
        "forward",
        "Forward",
        Some(Input::Key(KeyCode::KeyW)),
        true,
    ),
    (
        Action::Backward,
        "backward",
        "Backward",
        Some(Input::Key(KeyCode::KeyS)),
        true,
    ),
    (
        Action::Left,
        "left",
        "Strafe left",
        Some(Input::Key(KeyCode::KeyA)),
        true,
    ),
    (
        Action::Right,
        "right",
        "Strafe right",
        Some(Input::Key(KeyCode::KeyD)),
        true,
    ),
    (
        Action::Jump,
        "jump",
        "Jump",
        Some(Input::Key(KeyCode::Space)),
        true,
    ),
    (
        Action::Crouch,
        "crouch",
        "Crouch",
        Some(Input::Key(KeyCode::KeyC)),
        true,
    ),
    (
        Action::Walk,
        "walk",
        "Walk",
        Some(Input::Key(KeyCode::ShiftLeft)),
        true,
    ),
    (
        Action::Attack,
        "attack",
        "Attack",
        Some(Input::Mouse(MouseButton::Left)),
        false,
    ),
    (
        Action::AltAttack,
        "alt_attack",
        "Alt attack",
        Some(Input::Mouse(MouseButton::Right)),
        false,
    ),
    (
        Action::Use,
        "use",
        "Use",
        Some(Input::Key(KeyCode::KeyE)),
        false,
    ),
    (
        Action::Security,
        "security",
        "Security",
        Some(Input::Key(KeyCode::KeyU)),
        false,
    ),
    (
        Action::SaberStyle,
        "saber_style",
        "Saber style",
        Some(Input::Key(KeyCode::KeyL)),
        false,
    ),
    (
        Action::NextFeat,
        "next_feat",
        "Next feat attack",
        Some(Input::Key(KeyCode::KeyT)),
        false,
    ),
    (
        Action::Bar1,
        "bar_1",
        "Action bar slot 1",
        Some(Input::Key(KeyCode::Digit1)),
        false,
    ),
    (
        Action::Bar2,
        "bar_2",
        "Action bar slot 2",
        Some(Input::Key(KeyCode::Digit2)),
        false,
    ),
    (
        Action::Bar3,
        "bar_3",
        "Action bar slot 3",
        Some(Input::Key(KeyCode::Digit3)),
        false,
    ),
    (
        Action::Bar4,
        "bar_4",
        "Action bar slot 4",
        Some(Input::Key(KeyCode::Digit4)),
        false,
    ),
    (
        Action::Bar5,
        "bar_5",
        "Action bar slot 5",
        Some(Input::Key(KeyCode::Digit5)),
        false,
    ),
    (
        Action::Bar6,
        "bar_6",
        "Action bar slot 6",
        Some(Input::Key(KeyCode::Digit6)),
        false,
    ),
    (
        Action::Bar7,
        "bar_7",
        "Action bar slot 7",
        Some(Input::Key(KeyCode::Digit7)),
        false,
    ),
    (
        Action::Bar8,
        "bar_8",
        "Action bar slot 8",
        Some(Input::Key(KeyCode::Digit8)),
        false,
    ),
    (
        Action::Bar9,
        "bar_9",
        "Action bar slot 9",
        Some(Input::Key(KeyCode::Digit9)),
        false,
    ),
    (
        Action::Bar10,
        "bar_10",
        "Action bar slot 10",
        Some(Input::Key(KeyCode::Digit0)),
        false,
    ),
    (
        Action::Wheel,
        "wheel",
        "Power wheel",
        Some(Input::Key(KeyCode::KeyF)),
        false,
    ),
    (
        Action::BarSetup,
        "bar_setup",
        "Action bar setup",
        None,
        false,
    ),
    (
        Action::Stealth,
        "stealth",
        "Stealth mode",
        Some(Input::Key(KeyCode::KeyX)),
        false,
    ),
    (
        Action::Block,
        "block",
        "Block (Advanced rules)",
        Some(Input::Mouse(MouseButton::Right)),
        false,
    ),
    (
        Action::Kick,
        "kick",
        "Kick",
        Some(Input::Key(KeyCode::KeyG)),
        false,
    ),
    (
        Action::DuelAltAttack,
        "duel_alt_attack",
        "Saber throw and kata",
        Some(Input::Key(KeyCode::KeyR)),
        false,
    ),
    (
        Action::Holster,
        "holster",
        "Draw or holster weapon",
        Some(Input::Key(KeyCode::KeyH)),
        false,
    ),
    (
        Action::Form,
        "next_form",
        "Next Form",
        Some(Input::Key(KeyCode::KeyV)),
        false,
    ),
    (
        Action::SwitchWeapons,
        "switch_weapons",
        "Switch weapons",
        Some(Input::Key(KeyCode::KeyB)),
        false,
    ),
    (
        Action::RightHand,
        "right_hand",
        "Right hand",
        Some(Input::Key(KeyCode::BracketRight)),
        false,
    ),
    (
        Action::LeftHand,
        "left_hand",
        "Left hand",
        Some(Input::Key(KeyCode::BracketLeft)),
        false,
    ),
    (
        Action::Inventory,
        "inventory",
        "Inventory",
        Some(Input::Key(KeyCode::KeyI)),
        false,
    ),
    (
        Action::Journal,
        "journal",
        "Journal",
        Some(Input::Key(KeyCode::KeyJ)),
        false,
    ),
    (
        Action::Map,
        "map",
        "Map",
        Some(Input::Key(KeyCode::KeyM)),
        false,
    ),
    (
        Action::Party,
        "party",
        "Party",
        Some(Input::Key(KeyCode::KeyP)),
        false,
    ),
    (
        Action::Abilities,
        "abilities",
        "Abilities",
        Some(Input::Key(KeyCode::KeyK)),
        false,
    ),
    (
        Action::QuickSave,
        "quick_save",
        "Quick save",
        Some(Input::Key(KeyCode::F5)),
        false,
    ),
    (
        Action::QuickLoad,
        "quick_load",
        "Quick load",
        Some(Input::Key(KeyCode::F9)),
        false,
    ),
    (
        Action::Leader,
        "leader",
        "Switch party leader",
        Some(Input::Key(KeyCode::Tab)),
        false,
    ),
    (
        Action::Solo,
        "solo",
        "Solo mode",
        Some(Input::Key(KeyCode::KeyQ)),
        false,
    ),
    (
        Action::Follow,
        "follow",
        "Follow the next (watching)",
        Some(Input::Key(KeyCode::KeyN)),
        false,
    ),
    (
        Action::Break,
        "break",
        "Take a break",
        Some(Input::Key(KeyCode::KeyZ)),
        false,
    ),
    (
        Action::Ready,
        "ready",
        "Ready to go",
        Some(Input::Key(KeyCode::KeyY)),
        false,
    ),
    (
        Action::View,
        "view",
        "First person",
        Some(Input::Key(KeyCode::F1)),
        false,
    ),
    (
        Action::ZoomIn,
        "zoom_in",
        "Camera closer",
        Some(Input::Key(KeyCode::Equal)),
        false,
    ),
    (
        Action::ZoomOut,
        "zoom_out",
        "Camera farther",
        Some(Input::Key(KeyCode::Minus)),
        false,
    ),
    (
        Action::Orbit,
        "orbit",
        "Look around (hold)",
        Some(Input::Key(KeyCode::AltLeft)),
        false,
    ),
    (
        Action::SwapShoulder,
        "swap_shoulder",
        "Swap camera shoulder",
        Some(Input::Mouse(MouseButton::Middle)),
        false,
    ),
    (
        Action::LeaveTalk,
        "leave_talk",
        "Leave a conversation",
        Some(Input::Key(KeyCode::KeyL)),
        false,
    ),
    (
        Action::AcceptCall,
        "accept_call",
        "Accept a join request",
        Some(Input::Key(KeyCode::KeyY)),
        false,
    ),
    (
        Action::EndCall,
        "end_call",
        "End a join request",
        Some(Input::Key(KeyCode::KeyN)),
        false,
    ),
    (
        Action::Report,
        "report",
        "Report a bug",
        Some(Input::Key(KeyCode::F12)),
        false,
    ),
];

/// Keys a binding may name, by the name the screens and the file use.
const KEYS: [KeyCode; 101] = {
    use KeyCode::*;
    [
        KeyA,
        KeyB,
        KeyC,
        KeyD,
        KeyE,
        KeyF,
        KeyG,
        KeyH,
        KeyI,
        KeyJ,
        KeyK,
        KeyL,
        KeyM,
        KeyN,
        KeyO,
        KeyP,
        KeyQ,
        KeyR,
        KeyS,
        KeyT,
        KeyU,
        KeyV,
        KeyW,
        KeyX,
        KeyY,
        KeyZ,
        Digit0,
        Digit1,
        Digit2,
        Digit3,
        Digit4,
        Digit5,
        Digit6,
        Digit7,
        Digit8,
        Digit9,
        F1,
        F2,
        F3,
        F4,
        F5,
        F6,
        F7,
        F8,
        F9,
        F10,
        F11,
        F12,
        Space,
        Tab,
        Enter,
        Backspace,
        ShiftLeft,
        ShiftRight,
        ControlLeft,
        ControlRight,
        AltLeft,
        AltRight,
        CapsLock,
        BracketLeft,
        BracketRight,
        Comma,
        Period,
        Semicolon,
        Quote,
        Slash,
        Backslash,
        Minus,
        Equal,
        Backquote,
        ArrowUp,
        ArrowDown,
        ArrowLeft,
        ArrowRight,
        Insert,
        Delete,
        Home,
        End,
        PageUp,
        PageDown,
        Numpad0,
        Numpad1,
        Numpad2,
        Numpad3,
        Numpad4,
        Numpad5,
        Numpad6,
        Numpad7,
        Numpad8,
        Numpad9,
        NumpadAdd,
        NumpadSubtract,
        NumpadMultiply,
        NumpadDivide,
        NumpadDecimal,
        NumpadEnter,
        NumLock,
        ScrollLock,
        Pause,
        IntlBackslash,
        ContextMenu,
    ]
};

/// A key's name (`KeyW`).
pub fn key_name(key: KeyCode) -> String {
    format!("{key:?}")
}

/// The key a name names, among those a binding may use.
pub fn key_named(name: &str) -> Option<KeyCode> {
    KEYS.iter().copied().find(|key| key_name(*key) == name)
}

/// What a keyboard-and-mouse binding is on: a key or a mouse button (its
/// side buttons, `Back` and `Forward`, and any further one by number).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Input {
    Key(KeyCode),
    Mouse(MouseButton),
}

impl Input {
    /// As the settings file keeps it: `KeyW`, `MouseLeft`, `MouseBack`,
    /// `Mouse8`.
    pub fn name(self) -> String {
        match self {
            Input::Key(key) => key_name(key),
            Input::Mouse(MouseButton::Other(n)) => format!("Mouse{n}"),
            Input::Mouse(button) => format!("Mouse{button:?}"),
        }
    }

    /// The input a name names (`Input::name`), among those a binding may
    /// use.
    pub fn named(name: &str) -> Option<Input> {
        if let Some(key) = key_named(name) {
            return Some(Input::Key(key));
        }
        let button = match name.strip_prefix("Mouse")? {
            "Left" => MouseButton::Left,
            "Right" => MouseButton::Right,
            "Middle" => MouseButton::Middle,
            "Back" => MouseButton::Back,
            "Forward" => MouseButton::Forward,
            n => MouseButton::Other(n.parse().ok()?),
        };
        Some(Input::Mouse(button))
    }

    /// Whether the input is down, and whether it went down this frame. A
    /// mouse button reads as up without `mouse`.
    fn state(
        self,
        keys: Option<&ButtonInput<KeyCode>>,
        mouse: Option<&ButtonInput<MouseButton>>,
    ) -> (bool, bool) {
        match self {
            Input::Key(key) => keys.map_or((false, false), |keys| {
                (keys.pressed(key), keys.just_pressed(key))
            }),
            Input::Mouse(button) => mouse.map_or((false, false), |mouse| {
                (mouse.pressed(button), mouse.just_pressed(button))
            }),
        }
    }
}

/// The modifier keys a binding can ask to be held with its key or button.
/// Each is Ctrl, Shift or Alt, and the left and the right key of each count
/// the same.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Mods(u8);

/// The keys of each modifier, in the order of `key_mapping::MODIFIERS`.
static MODIFIER_KEYS: std::sync::LazyLock<Vec<[KeyCode; 2]>> = std::sync::LazyLock::new(|| {
    ui::screens::key_mapping::MODIFIERS
        .iter()
        .map(|(_, sides)| sides.map(|side| key_named(side).expect("a modifier key")))
        .collect()
});

impl Mods {
    /// The modifiers held now.
    pub fn held(keys: &ButtonInput<KeyCode>) -> Mods {
        Mods(
            MODIFIER_KEYS
                .iter()
                .enumerate()
                .filter(|(_, sides)| keys.any_pressed(**sides))
                .fold(0, |mods, (bit, _)| mods | 1 << bit),
        )
    }

    /// The names of these modifiers (`Ctrl`, `Shift`, `Alt`), in that order.
    pub fn names(self) -> impl Iterator<Item = &'static str> {
        ui::screens::key_mapping::MODIFIERS
            .iter()
            .enumerate()
            .filter(move |(bit, _)| self.0 & 1 << bit != 0)
            .map(|(_, (name, _))| *name)
    }

    /// Whether every modifier of `other` is among these.
    fn covers(self, other: Mods) -> bool {
        self.0 & other.0 == other.0
    }
}

/// A keyboard-and-mouse binding: a key or a mouse button, and the modifier
/// keys that must be held with it (none for most bindings).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Binding {
    pub mods: Mods,
    pub input: Input,
}

impl From<Input> for Binding {
    fn from(input: Input) -> Binding {
        Binding {
            mods: Mods::default(),
            input,
        }
    }
}

impl Binding {
    /// As the settings file keeps it: the modifiers first, each followed by
    /// a plus sign, then the input (`KeyW`, `Shift+MouseRight`,
    /// `Ctrl+Alt+KeyE`).
    pub fn name(self) -> String {
        let mods = self.mods.names().map(|name| format!("{name}+"));
        mods.chain([self.input.name()]).collect()
    }

    /// The binding a name names (`Binding::name`). Each modifier may be
    /// named once, in any order. A modifier key itself (Left Shift) takes no
    /// modifiers.
    pub fn named(name: &str) -> Option<Binding> {
        let mut parts: Vec<&str> = name.split('+').collect();
        let input = Input::named(parts.pop()?)?;
        let mut mods = Mods::default();
        for part in parts {
            let bit = ui::screens::key_mapping::MODIFIERS
                .iter()
                .position(|(name, _)| *name == part)?;
            if mods.0 & 1 << bit != 0 {
                return None;
            }
            mods.0 |= 1 << bit;
        }
        let modifier = |input: Input| {
            MODIFIER_KEYS
                .iter()
                .flatten()
                .any(|key| input == Input::Key(*key))
        };
        (mods == Mods::default() || !modifier(input)).then_some(Binding { mods, input })
    }
}

/// Which key or mouse button does what. An action with no key or mouse
/// button bound has no entry.
#[derive(Resource, Debug, Clone, PartialEq)]
pub struct KeyMap(BTreeMap<Action, Binding>);

impl Default for KeyMap {
    fn default() -> KeyMap {
        KeyMap(
            ACTIONS
                .iter()
                .filter_map(|(action, _, _, key, _)| Some((*action, Binding::from((*key)?))))
                .collect(),
        )
    }
}

impl KeyMap {
    /// The defaults, made once: for a system that reads a key with no
    /// settings in place (a headless or scripted run).
    pub fn defaults() -> &'static KeyMap {
        static DEFAULTS: std::sync::LazyLock<KeyMap> = std::sync::LazyLock::new(KeyMap::default);
        &DEFAULTS
    }

    /// The map with `action` on `key` alone (`bar_view`'s probes).
    #[cfg(feature = "probes")]
    pub(crate) fn with_key(mut self, action: Action, key: KeyCode) -> KeyMap {
        self.0.insert(action, Binding::from(Input::Key(key)));
        self
    }

    /// The binding of `action`, if it has one.
    pub fn binding(&self, action: Action) -> Option<Binding> {
        self.0.get(&action).copied()
    }

    /// The key or mouse button bound to `action`, without its modifiers.
    pub fn input(&self, action: Action) -> Option<Input> {
        self.binding(action).map(|binding| binding.input)
    }

    /// The key bound to `action`, if it is a key (not a mouse button).
    pub fn key(&self, action: Action) -> Option<KeyCode> {
        match self.input(action)? {
            Input::Key(key) => Some(key),
            Input::Mouse(_) => None,
        }
    }

    /// Whether `action`'s key went down this frame with its modifiers held.
    /// A binding on a mouse button, and an action with no binding, are never
    /// pressed here.
    pub fn key_pressed(&self, action: Action, keys: &ButtonInput<KeyCode>) -> bool {
        self.binding(action).is_some_and(|binding| {
            matches!(binding.input, Input::Key(key) if keys.just_pressed(key))
                && Mods::held(keys).covers(binding.mods)
        })
    }

    /// Each action's binding read from the keyboard and, with `mouse`, its
    /// buttons: whether it is held, and whether it went down this frame. A
    /// binding counts while its key or button and all its modifiers are
    /// held. It does not count while another action's binding on the same
    /// key or button, with more modifiers, counts too. So with Shift + Right
    /// mouse bound, Right mouse alone does not also fire.
    pub fn read(
        &self,
        keys: Option<&ButtonInput<KeyCode>>,
        mouse: Option<&ButtonInput<MouseButton>>,
    ) -> Vec<(Action, bool, bool)> {
        let held = keys.map_or(Mods::default(), Mods::held);
        let live = |binding: &Binding| {
            let (down, went) = binding.input.state(keys, mouse);
            let on = down && held.covers(binding.mods);
            (on, on && went)
        };
        self.0
            .iter()
            .map(|(action, binding)| {
                let outdone = self.0.values().any(|other| {
                    other.input == binding.input
                        && other.mods != binding.mods
                        && other.mods.covers(binding.mods)
                        && live(other).0
                });
                let (on, went) = if outdone {
                    (false, false)
                } else {
                    live(binding)
                };
                (*action, on, went)
            })
            .collect()
    }

    /// The binding of `action` as the screens name it (`W`, `1`, `F5`,
    /// `Mouse 5`, `Shift + Right mouse`, a dash for none).
    pub fn shown(&self, action: Action) -> Vec<u8> {
        let name = self.binding(action).map_or_else(String::new, Binding::name);
        ui::screens::key_mapping::key_name(&name).into_bytes()
    }
}

/// How the mouse turns the view.
#[derive(Resource, Debug, Clone, Copy, PartialEq)]
pub struct MouseFeel {
    /// Times the default turn.
    pub scale: f32,
    pub invert: bool,
}

impl Default for MouseFeel {
    fn default() -> MouseFeel {
        MouseFeel {
            scale: 1.0,
            invert: false,
        }
    }
}

/// The settings, by key, as kept in the file.
#[derive(Resource, Debug, Clone, PartialEq)]
pub struct Settings {
    values: BTreeMap<String, String>,
    path: Option<PathBuf>,
    /// The saber rules held for this run (`HELD_RULES`), until the options
    /// change them.
    held_rules: Option<SaberRules>,
}

/// Each setting the screens change, and its default. KOTOR 2's "Use Small
/// Fonts" (`display.small_fonts`) is among them: KOTOR 2 sets it in its ini
/// file alone, and kjk's Interface page sets it as Text size. The menu's
/// width, whether pictures keep their shape, the text scale, the
/// notifications in the corner and the target block at the top centre are
/// kjk's own Interface settings. The text scale starts at 100%, which is
/// KOTOR 2's own size. Their defaults are what suits kjk's real-time play, where the
/// mouse turns the camera and is not free to click: menus up to 16:9 with
/// pictures kept in shape, notifications that slide in at the corner
/// without pausing the game, and the target block at the top centre with
/// the area name under it. Each can be set back to KOTOR 2's way on the
/// Interface page. The frame rate limit is kjk's own too, on the same page.
/// KOTOR 2 has none, and it starts off. KOTOR 2's feedback options and
/// their defaults are listed by the Feedback page (`ui::feedback::defaults`). A settings file that
/// already holds a value for one keeps it.
const DEFAULTS: [(&str, &str); 37] = [
    ("mouse.sensitivity", "0.50"),
    ("pad.sensitivity", "0.50"),
    ("pad.invert", "false"),
    ("pad.deadzone", "0.15"),
    ("pad.rumble", "true"),
    ("camera.invert", "false"),
    ("camera.view", "third"),
    ("sound.music", "1.00"),
    ("sound.voice", "1.00"),
    ("sound.effects", "1.00"),
    ("sound.movie", "1.00"),
    ("display.gamma", "0.50"),
    ("display.grass", "true"),
    ("display.mode", "window"),
    ("display.size", ""),
    ("display.vsync", "true"),
    (ui::screens::options::FRAME_LIMIT, "0"),
    ("display.msaa", "4"),
    ("display.shadows", "high"),
    ("display.lighting", "classic"),
    (EFFECTS, "high"),
    (AMBIENT_OCCLUSION, "off"),
    (ui::draw::SmallFonts::SETTING, "auto"),
    (ui::draw::TextScale::SETTING, ui::draw::TextScale::DEFAULT),
    (ui::draw::MenuWidth::SETTING, "16x9"),
    (ui::draw::IMAGE_ASPECT, "true"),
    (ui::draw::CORNER_NOTICES, "true"),
    (ui::draw::TARGET_TOP, "true"),
    (AUTOSAVE, "true"),
    (COLLISION, "limited"),
    (SABER_RULES, "ja"),
    (STANCE_SETS, "true"),
    (DUEL_BARS, "false"),
    (DUEL_WARNING, "false"),
    (DUEL_SLOW_MOTION, "true"),
    (HILT_SINGLE, ""),
    (HILT_DOUBLE, ""),
];

/// The hilts KOTOR's single- and double-bladed lightsabers are drawn with
/// (`weapons::Sabers::with_hilts`): KOTOR's own when empty.
const HILT_SINGLE: &str = "saber.hilt.single";
const HILT_DOUBLE: &str = "saber.hilt.double";
/// The camera's options page (`camera_tune`): its Default puts every camera
/// setting back.
pub(crate) const CAMERA_PAGE: &str = "options_camera";
/// The guns' camera page: its Default puts their camera back.
pub(crate) const GUN_CAMERA_PAGE: &str = "options_camera_guns";

/// The key of Effects Quality (`effects_quality`).
const EFFECTS: &str = "display.effects";

/// The key of Ambient Occlusion (`ambient_occlusion`).
const AMBIENT_OCCLUSION: &str = "display.ambient_occlusion";

/// The key of Autosave (saving on each module entered, `sim_host::autosave`).
pub(crate) const AUTOSAVE: &str = "game.autosave";

/// The key of Collision (`session::Collision`): how freely characters may
/// climb the world. A session keeps the collision it started with.
pub(crate) const COLLISION: &str = ui::screens::options::COLLISION;

/// The keys of the duel options (`DuelOptions`).
const SABER_RULES: &str = "game.saber_rules";
const DUEL_BARS: &str = "game.duel_bars";
const DUEL_WARNING: &str = "game.duel_warning";
const DUEL_SLOW_MOTION: &str = "game.duel_slow_motion";
/// Whether characters use Movie Duels' stances and guards
/// (`stance_sets::wanted`): `true` or `false`, true while unset.
pub(crate) const STANCE_SETS: &str = "game.stance_sets";

/// Whether a session played here chooses targeted blocks, by
/// `KJK_TARGETED_BLOCKS` alone (`Settings::targeted_blocks`). A headless
/// run, which has no settings, reads it here too.
pub(crate) fn targeted_blocks_from_env() -> bool {
    targeted_blocks_by(std::env::var_os("KJK_TARGETED_BLOCKS").as_deref())
}

/// `targeted_blocks_from_env` with `env` the value of
/// `KJK_TARGETED_BLOCKS`, if it is set. Only the value 1 turns targeted
/// blocks on.
fn targeted_blocks_by(env: Option<&std::ffi::OsStr>) -> bool {
    env.is_some_and(|value| value == "1")
}

/// The saber rules `kjk play --saber-rules` (or `kjk host`) holds for its
/// run over the setting, unsaved, until the options change it.
static HELD_RULES: std::sync::OnceLock<SaberRules> = std::sync::OnceLock::new();

/// Holds `rules` for this run over the setting (`HELD_RULES`); before the
/// settings are first read.
pub fn hold_saber_rules(rules: SaberRules) {
    let _ = HELD_RULES.set(rules);
}

/// Whether Movie Duels is installed beside Jedi Academy
/// (`vfs::Install::movie_duels`), looked for once a run. Its stances and
/// some of its hilts are offered only then. The Advanced rules need none of
/// its files.
pub fn movie_duels() -> bool {
    static FOUND: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *FOUND.get_or_init(|| {
        crate::discover()
            .get(vfs::Game::JediAcademy)
            .is_some_and(vfs::Install::movie_duels)
    })
}

/// The duel options as the settings have them: the rules a session played
/// here plays by, and how the duel modes show (`duel_view`, `sim_host`).
#[derive(Resource, Debug, Clone, Copy, PartialEq, Eq)]
pub struct DuelOptions {
    pub rules: SaberRules,
    /// Whether a session played here chooses targeted blocks
    /// (`SimWorld::targeted`).
    pub targeted: bool,
    /// The Stamina bar over saber users' heads.
    pub bars: bool,
    /// A character whose swing cannot be blocked drawn red.
    pub warning: bool,
    /// A perfect block slows the world for a moment (alone only).
    pub slow_motion: bool,
}

/// Fixes the collision of the session about to start (`session::collision`)
/// as the options have it now. A joined player takes the host's instead
/// (`net_join`).
pub fn fix_session_collision() {
    session::set_collision(Settings::read().collision());
}

/// Each part of the mix and the setting of its slider.
const VOLUMES: [(Bus, &str); 4] = [
    (Bus::Music, "sound.music"),
    (Bus::Voice, "sound.voice"),
    (Bus::Effects, "sound.effects"),
    (Bus::Movie, "sound.movie"),
];

/// The key of this install's host id (`Settings::host_id`).
pub(crate) const HOST_ID: &str = "host.id";

/// The key of the layout of the key bindings a file was saved with, and
/// the layout now: 2 moved the medpac, stim and shield to the number row,
/// H holstering; 3 made the attacks bindings of their own, the mouse's
/// reversed buttons (`mouse.reverse`) becoming theirs swapped; 4 put the
/// action bar's slots on the number row and the power wheel on F in place
/// of the item and power keys (`RETIRED`), which moved the hands and the
/// next feat attack. Layout 5 moved the controller's chords (`PAD_MOVED`)
/// and made LB the only layer button, with the action bar's slots on it.
/// Layout 6 added the action bar's slots 7 to 10 on the keys 7, 8, 9 and 0
/// (`ADDED_SLOTS`).
const KEY_LAYOUT: (&str, &str) = ("keys.layout", "6");

/// The settings' names of the action bar slots that layout 6 added.
const ADDED_SLOTS: [&str; 4] = ["bar_7", "bar_8", "bar_9", "bar_10"];

/// The chords layout 5 moved, each by its old default. LB alone blocks and
/// RB alone opens the power wheel. LB with A, B, X and Y uses the first four
/// bar slots, which were on RB. The D-pad's up alone is the next feat attack
/// and its down alone draws or holsters the weapon. LB with the D-pad's up
/// switches weapons, and LB with its down opens the security screen.
const PAD_MOVED: [(&str, &str); 10] = [
    ("pad.wheel", "DPadUp"),
    ("pad.block", "LB+RightTrigger2"),
    ("pad.bar_1", "RB+South"),
    ("pad.bar_2", "RB+East"),
    ("pad.bar_3", "RB+West"),
    ("pad.bar_4", "RB+North"),
    ("pad.holster", "LB+South"),
    ("pad.security", "LB+West"),
    ("pad.switch_weapons", "LB+North"),
    ("pad.next_feat", "LB+DPadUp"),
];

/// The bindings layout 4 moved, by their old default: a file of an older
/// layout still on one gets the new default.
const MOVED: [(&str, &str); 4] = [
    ("key.right_hand", "Digit1"),
    ("key.left_hand", "Digit2"),
    ("key.next_feat", "Digit6"),
    ("pad.next_feat", "LB+RightTrigger"),
];

/// The settings' names of the actions whose default layout 4 set: the
/// action bar's, which it added, and those it moved (`MOVED`).
const PLACED: [&str; 11] = [
    "bar_1",
    "bar_2",
    "bar_3",
    "bar_4",
    "bar_5",
    "bar_6",
    "wheel",
    "bar_setup",
    "right_hand",
    "left_hand",
    "next_feat",
];

/// The settings' names of the actions layout 4 removed: the selected power
/// and the keys that chose it, and the item keys.
const RETIRED: [&str; 8] = [
    "power",
    "next_power",
    "previous_power",
    "medpac",
    "stim",
    "shield",
    "grenade",
    "lay_mine",
];

/// What a page's Default button puts back.
fn page_keys(page: &str) -> &'static [&'static str] {
    match page {
        "options_gameplay" => &["camera.invert", AUTOSAVE, COLLISION],
        "options_sabers" => &[
            SABER_RULES,
            DUEL_BARS,
            DUEL_WARNING,
            DUEL_SLOW_MOTION,
            HILT_SINGLE,
            HILT_DOUBLE,
            STANCE_SETS,
        ],
        "options_mouse" => &["mouse.sensitivity"],
        "options_controller" => &[
            "pad.sensitivity",
            "pad.invert",
            "pad.deadzone",
            "pad.rumble",
        ],
        "options_sound" => &["sound.music", "sound.voice", "sound.effects", "sound.movie"],
        "options_graphics" => &["display.gamma", "display.shadows", "display.grass"],
        "options_graphics_advanced" => &[
            "display.vsync",
            "display.msaa",
            "display.lighting",
            EFFECTS,
            AMBIENT_OCCLUSION,
        ],
        "options_interface" => &[
            ui::draw::MenuWidth::SETTING,
            ui::draw::IMAGE_ASPECT,
            ui::draw::SmallFonts::SETTING,
            ui::draw::TextScale::SETTING,
            ui::draw::CORNER_NOTICES,
            ui::draw::TARGET_TOP,
            ui::screens::options::FRAME_LIMIT,
        ],
        // The window's mode and size alone: a start that did not get going
        // with them (`display::recover`), `kjk --reset-display`.
        "display" => &["display.mode", "display.size"],
        _ => &[],
    }
}

/// The setting holding the folder the player chose for `game` (`setup`).
pub(crate) fn install_key(game: vfs::Game) -> String {
    format!("install.{}", game.key())
}

/// The setting naming Jedi Academy's mod folder (`MD`), the variable
/// `KJK_JA_MOD`'s stand-in (`chosen_installs`).
pub(crate) const JA_MOD: &str = "ja.mod";

/// The settings file's values alone, as kept (no defaults); nothing when
/// it does not read. What loading reads before any app (`discover`, the
/// view's options), saving nothing.
pub(crate) fn stored() -> BTreeMap<String, String> {
    file()
        .and_then(|path| std::fs::read_to_string(path).ok())
        .and_then(|text| serde_json::from_str::<BTreeMap<String, String>>(&text).ok())
        .unwrap_or_else(BTreeMap::new)
}

/// The folders the player chose for the games, by their variables' names
/// (`KJK_KOTOR1`, …), and Jedi Academy's mod folder (`ja.mod`, as
/// `KJK_JA_MOD`), read from the settings file alone: `discover` reads them
/// before any app, and saves nothing.
pub fn chosen_installs() -> std::collections::HashMap<String, String> {
    let values = stored();
    let installs = vfs::Game::ALL
        .into_iter()
        .map(|game| (install_key(game), game.env_var()));
    installs
        .chain([(JA_MOD.to_string(), vfs::JA_MOD_VAR)])
        .filter_map(|(key, var)| Some((var.to_string(), values.get(&key)?.clone())))
        .collect()
}

/// The folder for this program's settings: the platform's own place.
pub(crate) fn folder() -> Option<PathBuf> {
    store::kjk_folder(|name| std::env::var(name).ok())
}

/// The settings' file: the folder's `settings.json`, or the one
/// `KJK_SETTINGS` names (scripted runs, tests).
pub(crate) fn file() -> Option<PathBuf> {
    std::env::var_os("KJK_SETTINGS")
        .map(PathBuf::from)
        .or_else(|| folder().map(|folder| folder.join("settings.json")))
}

impl Settings {
    fn default_values() -> BTreeMap<String, String> {
        let mut values: BTreeMap<String, String> = DEFAULTS
            .iter()
            .copied()
            .chain(camera_tune::defaults())
            .chain(ui::feedback::defaults())
            .map(|(key, value)| (key.to_string(), value.to_string()))
            .collect();
        for (_, name, _, key, _) in ACTIONS {
            let key = key.map_or_else(|| UNBOUND.to_string(), Input::name);
            values.insert(format!("key.{name}"), key);
        }
        values.insert(KEY_LAYOUT.0.into(), KEY_LAYOUT.1.into());
        values
    }

    /// The defaults alone: no file, no path, nothing saved.
    pub fn defaults() -> Settings {
        Settings {
            values: Settings::default_values(),
            path: None,
            held_rules: HELD_RULES.get().copied(),
        }
    }

    /// The settings in the user's file, over the defaults. A file that does
    /// not read gives the defaults and a warning, and is kept aside as
    /// `settings.json.bad` (`store::set_aside`). A file with no host id gets
    /// one, made at random and saved.
    pub fn load() -> Settings {
        Settings::load_from(file())
    }

    /// `load` from the file at `path` (none: the defaults, kept nowhere).
    pub(crate) fn load_from(path: Option<PathBuf>) -> Settings {
        let mut settings = Settings::defaults();
        let mut changed = false;
        if let Some(path) = &path
            && let Ok(bytes) = std::fs::read(path)
        {
            match serde_json::from_slice::<BTreeMap<String, String>>(&bytes) {
                Ok(mut read) => {
                    changed = relayout(&mut read)
                        | retire_mda(&mut read, movie_duels())
                        | block_on_alt(&mut read);
                    changed |= one_shadow_setting(&mut read) | retire_duel_stances(&mut read);
                    settings.values.extend(read);
                }
                // The file is kept aside for the player, and the defaults
                // are saved in its place below (they have no host id).
                Err(error) => match store::set_aside(path) {
                    Ok(aside) => warn!(
                        "{}: {error}; the defaults stand, and the file was kept as {}",
                        path.display(),
                        aside.display()
                    ),
                    Err(why) => warn!(
                        "{}: {error}; the defaults stand, and the file could not be kept aside: {why}",
                        path.display()
                    ),
                },
            }
        }
        settings.path = path;
        if !settings.values.contains_key(HOST_ID) {
            let key = crate::solo::random_key();
            let id = key[..8].iter().map(|byte| format!("{byte:02x}")).collect();
            settings.values.insert(HOST_ID.into(), id);
            changed = true;
        }
        if changed {
            settings.save();
        }
        settings
    }

    /// The file the settings are kept in, if any.
    pub(crate) fn path(&self) -> Option<&std::path::Path> {
        self.path.as_deref()
    }

    /// Every value set, by key.
    pub fn values(&self) -> &BTreeMap<String, String> {
        &self.values
    }

    /// The value set for `key`, if any.
    pub fn value(&self, key: &str) -> Option<&str> {
        self.values.get(key).map(String::as_str)
    }

    /// This install's host id: its players' copies of its saves say it,
    /// telling them from another host's (`saves::copies::Received::host`).
    pub fn host_id(&self) -> &str {
        self.values.get(HOST_ID).map_or("", String::as_str)
    }

    fn save(&self) {
        let Some(path) = &self.path else {
            return;
        };
        // Written whole beside its place and renamed into it: a program
        // that stops while writing leaves the old settings, not half of them.
        let text = serde_json::to_string_pretty(&self.values).expect("strings serialize");
        if let Err(error) = store::write_atomically(path, text.as_bytes()) {
            warn!("{}: {error}", path.display());
        }
    }

    /// `key` set to `value`; `reset` puts a page's settings (or `keys`, or
    /// the pad's chords, `pad`) back. Keys must name a key a binding may
    /// use, chords a chord (or `none`).
    pub fn apply(&mut self, key: &str, value: &str) {
        // A camera's preset sets its sliders (Custom, the cycle's last, and
        // the guns' Same as Melee move none); a slider moved names its
        // camera's preset again.
        if let Some(profile) = camera_tune::preset_of(key) {
            for (name, share) in profile.preset_values(value).into_iter().flatten() {
                self.values.insert(name.into(), share.into());
            }
            self.values.insert(key.into(), value.into());
            self.save();
            return;
        }
        if let Some(profile) = camera_tune::slider_of(key) {
            self.values.insert(key.into(), value.into());
            let preset = profile.preset_named(|name| self.value(name));
            self.values.insert(profile.preset.into(), preset.into());
            self.save();
            return;
        }
        let defaults = Settings::default_values();
        match (key, value) {
            ("reset", "keys") => {
                for (name, default) in defaults.iter().filter(|(name, _)| name.starts_with("key."))
                {
                    self.values.insert(name.clone(), default.clone());
                }
            }
            ("reset", "pad") => {
                for (_, name, ..) in ACTIONS {
                    self.values.remove(&format!("pad.{name}"));
                }
            }
            ("reset", page @ (CAMERA_PAGE | GUN_CAMERA_PAGE)) => {
                let reset: Vec<_> = if page == CAMERA_PAGE {
                    camera_tune::camera_defaults().collect()
                } else {
                    camera_tune::GUNS.defaults().collect()
                };
                for (name, default) in reset {
                    self.values.insert(name.into(), default.into());
                }
            }
            ("reset", ui::feedback::PAGE) => {
                for name in ui::feedback::reset_keys() {
                    self.values.insert(name.to_string(), defaults[name].clone());
                }
            }
            ("reset", page) => {
                if page_keys(page).contains(&SABER_RULES) {
                    self.held_rules = None;
                }
                for name in page_keys(page) {
                    self.values
                        .insert(name.to_string(), defaults[*name].clone());
                }
            }
            (name, key)
                if name.starts_with("key.") && key != UNBOUND && Binding::named(key).is_none() =>
            {
                warn!("{key}: not a key a binding may use");
                return;
            }
            (name, chord)
                if pad_action(name).is_some()
                    && chord != UNBOUND
                    && crate::pad::Chord::named(chord).is_none() =>
            {
                warn!("{chord}: not a chord a binding may use");
                return;
            }
            (name, value) => {
                if name == SABER_RULES {
                    self.held_rules = None;
                }
                self.values.insert(name.to_string(), value.to_string());
            }
        }
        self.save();
    }

    /// Whether setting `key` is `true`.
    pub fn on(&self, key: &str) -> bool {
        self.values.get(key).is_some_and(|value| value == "true")
    }

    /// The keys and mouse buttons: the defaults, with a file's
    /// `key.<action>` over them (a binding's name, `Binding::name`, or
    /// `none` to leave the action with no key).
    pub fn keys(&self) -> KeyMap {
        KeyMap(
            ACTIONS
                .iter()
                .filter_map(|(action, name, _, default, _)| {
                    let default = default.map(Binding::from);
                    let binding = match self.values.get(&format!("key.{name}")) {
                        Some(key) if key == UNBOUND => None,
                        Some(key) => Binding::named(key).or(default),
                        None => default,
                    };
                    Some((*action, binding?))
                })
                .collect(),
        )
    }

    pub fn mouse(&self) -> MouseFeel {
        let sensitivity = self
            .values
            .get("mouse.sensitivity")
            .and_then(|value| value.parse::<f32>().ok())
            .unwrap_or(0.5)
            .clamp(0.0, 1.0);
        MouseFeel {
            // A quarter to twice the default, the default halfway.
            scale: if sensitivity < 0.5 {
                0.25 + 1.5 * sensitivity
            } else {
                2.0 * sensitivity
            },
            invert: self
                .values
                .get("camera.invert")
                .is_some_and(|v| v == "true"),
        }
    }

    /// The pad's chords: the defaults, with a file's `pad.<action>` over
    /// them (a chord's name, `Chord::name`, or `none` to unbind it).
    pub fn pads(&self) -> crate::pad::PadMap {
        crate::pad::PadMap::with(ACTIONS.iter().filter_map(|(action, name, ..)| {
            let value = self.values.get(&format!("pad.{name}"))?;
            let chord = crate::pad::Chord::named(value);
            if chord.is_none() && value != UNBOUND {
                warn!("pad.{name}: {value} names no chord; the default stands");
                return None;
            }
            Some((*action, chord))
        }))
    }

    /// How the pad turns the view: 0.4 to 1.6 times the default rate, the
    /// default halfway; its sticks' dead zone as the options set it.
    pub fn pad(&self) -> crate::pad::PadFeel {
        let on = |key: &str| self.values.get(key).is_none_or(|value| value != "false");
        crate::pad::PadFeel {
            turn: 80.0 + 240.0 * self.fraction("pad.sensitivity", 0.5),
            invert: self.values.get("pad.invert").is_some_and(|v| v == "true"),
            dead: self
                .values
                .get("pad.deadzone")
                .and_then(|value| value.parse::<f32>().ok())
                .map_or(crate::pad::PadFeel::default().dead, |dead| {
                    dead.clamp(0.0, 0.6)
                }),
            rumble: on("pad.rumble"),
        }
    }

    /// KOTOR's "Tutorial Popups" option (on by default, as the game's): a
    /// tutorial window a script shows comes up (`script_screens`).
    pub fn tutorials(&self) -> bool {
        ui::feedback::on(&self.values, ui::feedback::TUTORIALS)
    }

    /// The saber rules a session played here plays by: those held for the
    /// run (`--saber-rules`), else the setting's: JA's when unset or not a
    /// name.
    pub fn saber_rules(&self) -> SaberRules {
        self.held_rules.unwrap_or_else(|| {
            self.values
                .get(SABER_RULES)
                .and_then(|name| SaberRules::named(name))
                .unwrap_or(SaberRules::Ja)
        })
    }

    /// Whether a session played here chooses targeted blocks. Targeted
    /// blocks are switched off, and the options page has no row for them.
    /// A value stored in the settings file is ignored. A developer turns
    /// them on by starting the game with `KJK_TARGETED_BLOCKS=1`. They
    /// apply only under the Advanced rules (`SimWorld::targeted`).
    pub fn targeted_blocks(&self) -> bool {
        targeted_blocks_from_env()
    }

    /// The defaults with the file's values over them, as `load` has them,
    /// saving nothing (`stored`).
    pub(crate) fn read() -> Settings {
        let mut settings = Settings::defaults();
        settings.values.extend(stored());
        settings
    }

    /// The duel options (`DuelOptions`).
    pub fn duel(&self) -> DuelOptions {
        DuelOptions {
            rules: self.saber_rules(),
            targeted: self.targeted_blocks(),
            bars: self.on(DUEL_BARS),
            warning: self.on(DUEL_WARNING),
            slow_motion: self.on(DUEL_SLOW_MOTION),
        }
    }

    /// The settings as the screens show them: the saber rules held for the
    /// run in the setting's place.
    pub fn values_shown(&self) -> BTreeMap<String, String> {
        let mut shown = self.values.clone();
        if let Some(rules) = self.held_rules {
            shown.insert(SABER_RULES.into(), rules.name().into());
        }
        shown
    }

    /// The player looks through its own eyes (`play_view::CameraMode`).
    pub fn first_person(&self) -> bool {
        self.values.get("camera.view").is_some_and(|v| v == "first")
    }

    /// The third-person camera the options ask for.
    pub fn camera(&self) -> camera_tune::CameraTune {
        camera_tune::CameraTune::from_settings(|key| self.value(key))
    }

    /// Keeps whether the player looks through its own eyes.
    pub fn keep_first_person(&mut self, first: bool) {
        self.apply("camera.view", if first { "first" } else { "third" });
    }

    /// A slider's setting, 0 to 1 (`default` when unset or not a number).
    fn fraction(&self, key: &str, default: f32) -> f32 {
        self.values
            .get(key)
            .and_then(|value| value.parse::<f32>().ok())
            .unwrap_or(default)
            .clamp(0.0, 1.0)
    }

    /// Each part of the mix's volume: the slider squared, so its middle is
    /// a quarter (-12 dB), as loud sounds half as loud.
    pub fn volumes(&self) -> Volumes {
        Volumes(
            VOLUMES
                .iter()
                .map(|(bus, key)| (*bus, self.fraction(key, 1.0).powi(2)))
                .collect(),
        )
    }

    /// How the window is shown.
    pub fn display(&self) -> Display {
        let get = |key: &str| self.values.get(key).map_or("", String::as_str);
        Display {
            mode: DisplayMode::named(get("display.mode")),
            size: crate::display::size_named(get("display.size")),
            vsync: get("display.vsync") != "false",
            frame_limit: ui::screens::options::frame_limit(get(ui::screens::options::FRAME_LIMIT)),
            antialias: crate::temporal::Antialias::named(get("display.msaa")),
            gamma: crate::display::gamma(self.fraction("display.gamma", 0.5)),
            grass: get("display.grass") != "false",
        }
    }

    /// What casts characters' shadows (the options' Shadows).
    pub fn shadows(&self) -> crate::shadows::ShadowSettings {
        let value = self.values.get("display.shadows");
        crate::shadows::ShadowSettings::named(value.map_or("high", String::as_str))
    }

    /// The collision a session started now builds its areas with (the
    /// options' Collision).
    pub fn collision(&self) -> session::Collision {
        self.values
            .get(COLLISION)
            .and_then(|name| session::Collision::named(name))
            .unwrap_or(session::Collision::Limited)
    }

    /// The original game's lighting, or kjk's own with glow (the options'
    /// Lighting).
    pub fn lighting(&self) -> crate::rolloff::LightingMode {
        let value = self.values.get("display.lighting");
        crate::rolloff::LightingMode::named(value.map_or("classic", String::as_str))
    }

    /// Whether particles are drawn at the frame's size or half of it (the
    /// options' Effects Quality).
    pub fn effects(&self) -> crate::effects_quality::EffectsQuality {
        let value = self.values.get(EFFECTS);
        crate::effects_quality::EffectsQuality::named(value.map_or("high", String::as_str))
    }

    /// Whether the room's light is darkened where something stands close to
    /// a surface (the options' Ambient Occlusion).
    pub fn ambient_occlusion(&self) -> crate::ambient_occlusion::AmbientOcclusion {
        let value = self.values.get(AMBIENT_OCCLUSION);
        crate::ambient_occlusion::AmbientOcclusion::named(value.map_or("off", String::as_str))
    }

    /// The key rows the key mapping shows, each with its binding's name
    /// (empty where none is bound).
    pub fn rows(&self) -> Vec<KeyRow> {
        let map = self.keys();
        ACTIONS
            .iter()
            .map(|(action, name, label, _, movement)| KeyRow {
                action: name.to_string(),
                label: label.to_string(),
                key: map.binding(*action).map_or_else(String::new, Binding::name),
                movement: *movement,
                shares: shares(*action),
            })
            .collect()
    }

    /// The pad's rows the key mapping's controller tab shows: every action
    /// but the stick's (moving, and walking by how far it is pushed), its
    /// chord's name (`LB+South`; empty where none is bound).
    pub fn pad_rows(&self) -> Vec<KeyRow> {
        let map = self.pads();
        ACTIONS
            .iter()
            .filter(|(action, ..)| !STICK.contains(action))
            .map(|(action, name, label, _, movement)| KeyRow {
                action: name.to_string(),
                label: label.to_string(),
                key: map
                    .chord(*action)
                    .map_or_else(String::new, crate::pad::Chord::name),
                movement: *movement,
                shares: shares(*action),
            })
            .collect()
    }
}

/// The actions the left stick does, which no pad chord binds.
const STICK: [Action; 5] = [
    Action::Forward,
    Action::Backward,
    Action::Left,
    Action::Right,
    Action::Walk,
];

/// The actions that may keep one binding, as the defaults have them: their
/// moments never meet (a ready check's Ready and a join request's Accept;
/// following while watching and ending a join request) or a screen takes
/// one (Leave, on a conversation or a watched pazaak table), or a tap is
/// one and a hold the other (a pad's R3: Saber style tapped, the camera's
/// orbit held, `pad::TAP_ON_ORBIT`), or what is in hand picks one (the
/// right mouse button blocks under the duel rules with a lit lightsaber,
/// and is the alternate attack otherwise, `sim_host::share_button`). A
/// rebinding swaps with any other action on the binding asked for.
pub(crate) const SHARES: [[Action; 2]; 6] = [
    [Action::AltAttack, Action::Block],
    [Action::SaberStyle, Action::LeaveTalk],
    [Action::SaberStyle, Action::Orbit],
    [Action::Crouch, Action::LeaveTalk],
    [Action::Ready, Action::AcceptCall],
    [Action::Follow, Action::EndCall],
];

/// The settings' names of the actions `action` may share a binding with.
fn shares(action: Action) -> Vec<String> {
    SHARES
        .iter()
        .filter(|pair| pair.contains(&action))
        .flat_map(|pair| pair.iter().filter(|other| **other != action))
        .filter_map(|other| {
            ACTIONS
                .iter()
                .find(|(action, ..)| action == other)
                .map(|(_, name, ..)| name.to_string())
        })
        .collect()
}

/// The action a `pad.<name>` setting binds, if it names one (not the pad's
/// feel: `pad.sensitivity`, …).
fn pad_action(setting: &str) -> Option<Action> {
    let name = setting.strip_prefix("pad.")?;
    ACTIONS
        .iter()
        .find(|(_, action, ..)| *action == name)
        .map(|(action, ..)| *action)
}

/// The key mapping's rows as the defaults have them: the keyboard's and
/// the mouse's, and the pad's (`kjk gui screen options_keymap`).
pub fn key_rows() -> (Vec<KeyRow>, Vec<KeyRow>) {
    let settings = Settings::defaults();
    (settings.rows(), settings.pad_rows())
}

/// Each action's label and its default key or button as the key mapping
/// shows it (`W`, `Space`, `Left mouse`), in the mapping's order: the
/// controls the rulebook's `[Action]` names (`kjk rules`).
pub fn key_bindings() -> Vec<(String, String)> {
    Settings::defaults()
        .rows()
        .into_iter()
        .map(|row| (row.label, ui::screens::key_mapping::key_name(&row.key)))
        .collect()
}

/// A file's Shadows and Soft Shadows check boxes brought to the one
/// Shadows setting. Shadows off is `off`. Shadows on with Soft Shadows off
/// is `low`, and with it on is `high`. Whether the file held the check
/// boxes.
fn one_shadow_setting(read: &mut BTreeMap<String, String>) -> bool {
    let soft = read.remove("display.soft_shadows");
    let now = match (
        read.get("display.shadows").map(String::as_str),
        soft.as_deref(),
    ) {
        (Some("false"), _) => "off",
        (Some("true") | None, Some("false")) => "low",
        (Some("true"), _) => "high",
        (_, soft) => return soft.is_some(),
    };
    read.insert("display.shadows".into(), now.into());
    true
}

/// Drops a saved `game.duel_stances`. The stances come with MDA, so the
/// setting that once turned them on no longer exists. True when it dropped
/// one.
pub(crate) fn retire_duel_stances(read: &mut BTreeMap<String, String>) -> bool {
    read.remove("game.duel_stances").is_some()
}

/// A saved `game.saber_rules` naming one of the Advanced rules' retired
/// names (`SaberRules::RETIRED_NAMES`) is rewritten to `advanced` when Movie
/// Duels is installed, which is what those names played, and to `ja`
/// otherwise, which is what they played without it. Whether it was.
fn retire_mda(read: &mut BTreeMap<String, String>, movie_duels: bool) -> bool {
    let Some(set) = read.get_mut(SABER_RULES) else {
        return false;
    };
    if !SaberRules::RETIRED_NAMES
        .iter()
        .any(|old| set.eq_ignore_ascii_case(old))
    {
        return false;
    }
    let now = match movie_duels {
        true => SaberRules::DuelAdvanced,
        false => SaberRules::Ja,
    };
    *set = now.name().into();
    true
}

/// A file with no binding for Block (one written before it existed) that
/// has the alternate attack on another button than the default puts Block
/// on that button too, so the two share it as the defaults do (a reversed
/// mouse's alternate attack is on the left button). Whether it did.
fn block_on_alt(read: &mut BTreeMap<String, String>) -> bool {
    // A file of the pre-release v0.17.1-md-rc.1 holds Block on R, its
    // default then. R is now the duel rules' alternate attack, which such
    // a file does not name, so its Block moves as an unnamed one does.
    let on_old_default = read.get("key.block").is_some_and(|key| key == "KeyR")
        && !read.contains_key("key.duel_alt_attack");
    if read.contains_key("key.block") && !on_old_default {
        return false;
    }
    let Some(alt) = read.get("key.alt_attack").cloned() else {
        return false;
    };
    read.insert("key.block".into(), alt);
    true
}

/// A file's settings brought to the bindings' layout now: those that moved
/// and it kept on their old default dropped (the new default stands), and
/// the layout set. Whether it was of an older layout.
fn relayout(read: &mut BTreeMap<String, String>) -> bool {
    let (key, layout) = KEY_LAYOUT;
    let saved: u32 = read
        .get(key)
        .and_then(|saved| saved.parse().ok())
        .unwrap_or(1);
    if read.get(key).is_some_and(|saved| saved == layout) {
        return false;
    }
    if saved < 3
        && read
            .remove("mouse.reverse")
            .is_some_and(|reversed| reversed == "true")
    {
        read.insert("key.attack".into(), "MouseRight".into());
        read.insert("key.alt_attack".into(), "MouseLeft".into());
    }
    if saved < 4 {
        bar_layout(read);
    }
    if saved < 5 {
        pad_layout(read);
    }
    // A slot that layout 6 added takes its default key, unless another
    // action of the file is on that key. Then the slot is left unbound.
    if saved < 6 {
        place_defaults(read, |name| ADDED_SLOTS.contains(&name), &["key."]);
    }
    read.insert(key.into(), layout.into());
    true
}

/// A file of a layout before 4 brought to it. The removed actions' bindings
/// are dropped, and so are the moved ones it kept on their old default.
/// A chord on RB, which layout 4 held for the action bar's layer, is
/// dropped too. Last, an action whose default this layout set (`PLACED`)
/// and that the file does not bind takes its default key
/// (`place_defaults`). The chords are placed by `pad_layout`.
fn bar_layout(read: &mut BTreeMap<String, String>) {
    read.retain(|setting, _| {
        let action = setting
            .strip_prefix("key.")
            .or_else(|| setting.strip_prefix("pad."));
        !action.is_some_and(|action| RETIRED.contains(&action))
    });
    for (name, old) in MOVED {
        if read.get(name).is_some_and(|saved| saved == old) {
            read.remove(name);
        }
    }
    read.retain(|setting, chord| !(pad_action(setting).is_some() && crate::pad::names_rb(chord)));
    place_defaults(read, |name| PLACED.contains(&name), &["key."]);
}

/// A file of layout 4 brought to 5, which moved the controller's chords
/// (`PAD_MOVED`). The file's chords that no longer name a chord are
/// dropped. Those are the chords with RB held, because RB is no layer now.
/// A moved action's chord that the file kept on its old default is dropped
/// too. So is any other chord of the file that is now a moved action's new
/// default, such as an old LB chord on LB with A, which is now the first
/// bar slot's. Last, every action the file does not bind takes its default
/// chord, unless another action of the file is on it (`place_defaults`).
/// Then it is left unbound.
fn pad_layout(read: &mut BTreeMap<String, String>) {
    let pads = crate::pad::PadMap::defaults();
    let moved_to: Vec<(&str, String)> = PAD_MOVED
        .iter()
        .filter_map(|(setting, _)| {
            let chord = pads.chord(pad_action(setting)?)?;
            Some((*setting, chord.name()))
        })
        .collect();
    let dropped: Vec<String> = read
        .iter()
        .filter(|(setting, value)| {
            let Some(action) = pad_action(setting) else {
                return false;
            };
            let sharing = shares(action);
            let on_moved = moved_to.iter().any(|(other, chord)| {
                *other != setting.as_str()
                    && chord == *value
                    && !sharing
                        .iter()
                        .any(|name| other.strip_prefix("pad.") == Some(name.as_str()))
            });
            value.as_str() != UNBOUND
                && (crate::pad::Chord::named(value).is_none()
                    || PAD_MOVED.contains(&(setting.as_str(), value.as_str()))
                    || on_moved)
        })
        .map(|(setting, _)| setting.clone())
        .collect();
    for setting in &dropped {
        read.remove(setting);
    }
    place_defaults(read, |_| true, &["pad."]);
}

/// Gives each action that `placed` names, and that the file does not bind
/// on a device of `prefixes` (`key.`, `pad.`), its default key or chord, but
/// only if no other action of the file is on that binding. Otherwise the
/// action is left unbound, so that no two actions end on one binding unless
/// `SHARES` lets them share it.
fn place_defaults(
    read: &mut BTreeMap<String, String>,
    placed: impl Fn(&str) -> bool,
    prefixes: &[&str],
) {
    let pads = crate::pad::PadMap::defaults();
    for &(action, name, _, key, _) in ACTIONS.iter().filter(|(_, name, ..)| placed(name)) {
        let defaults = [
            ("key.", key.map(Input::name)),
            ("pad.", pads.chord(action).map(crate::pad::Chord::name)),
        ];
        for (prefix, default) in defaults {
            let setting = format!("{prefix}{name}");
            let Some(default) =
                default.filter(|_| prefixes.contains(&prefix) && !read.contains_key(&setting))
            else {
                continue;
            };
            let sharing = shares(action);
            let taken = read.iter().any(|(other, value)| {
                *value == default
                    && other
                        .strip_prefix(prefix)
                        .is_some_and(|other| !sharing.iter().any(|name| name == other))
            });
            if taken {
                read.insert(setting, UNBOUND.into());
            }
        }
    }
}

pub struct SettingsPlugin;

impl Plugin for SettingsPlugin {
    fn build(&self, app: &mut App) {
        let mut settings = Settings::load();
        // A window's run whose last start did not get going with the
        // display it tried opens in a window, and says so.
        let windowed = !app
            .world()
            .get_resource::<crate::scene::Headless>()
            .is_some_and(|headless| headless.0);
        if windowed && let Some(reset) = crate::display::recover(&mut settings) {
            app.insert_resource(reset);
        }
        app.insert_resource(settings.keys())
            .insert_resource(settings.mouse())
            .insert_resource(settings.pads())
            .insert_resource(settings.pad())
            .insert_resource(settings.volumes())
            .insert_resource(settings.display())
            .insert_resource(settings.shadows())
            .insert_resource(settings.duel())
            .insert_resource(settings.lighting())
            .insert_resource(settings.effects())
            .insert_resource(settings.ambient_occlusion())
            .insert_resource(settings.camera())
            .insert_resource(settings)
            .add_systems(Startup, show)
            .add_systems(Update, apply.after(crate::ui_view::Stepped));
    }
}

/// The settings as the screens show them.
fn show(settings: Res<Settings>, ui: Option<ResMut<Ui>>) {
    if let Some(mut ui) = ui {
        settings.shown(&mut ui);
    }
}

impl Settings {
    /// `ui`'s copy of the settings brought up to date. `ui` counts each
    /// time a setting in its copy changes (`Ui::settings_revision`).
    pub(crate) fn shown(&self, ui: &mut Ui) {
        let shown = self.values_shown();
        if ui.vm.settings != shown {
            ui.vm.settings = shown;
            ui.settings_revision = ui.settings_revision.wrapping_add(1);
        }
        ui.vm.movie_duels = movie_duels();
        ui.vm.keys = self.rows();
        ui.vm.pad_keys = self.pad_rows();
    }
}

/// The screens' setting changes, applied, saved and shown back.
fn apply(
    mut intents: ResMut<UiIntents>,
    mut settings: ResMut<Settings>,
    (mut keys, mut mouse): (ResMut<KeyMap>, ResMut<MouseFeel>),
    (mut pads, mut pad): (ResMut<crate::pad::PadMap>, ResMut<crate::pad::PadFeel>),
    (mut volumes, mut display, mut camera): (
        ResMut<Volumes>,
        ResMut<Display>,
        ResMut<camera_tune::CameraTune>,
    ),
    (mut shadows, mut lighting, mut effects, mut duel): (
        ResMut<crate::shadows::ShadowSettings>,
        ResMut<crate::rolloff::LightingMode>,
        ResMut<crate::effects_quality::EffectsQuality>,
        ResMut<DuelOptions>,
    ),
    (mut occlusion, ui): (
        ResMut<crate::ambient_occlusion::AmbientOcclusion>,
        Option<ResMut<Ui>>,
    ),
) {
    let mut changed = false;
    intents.0.retain(|intent| match intent {
        Intent::Setting(key, value) => {
            settings.apply(key, value);
            changed = true;
            false
        }
        _ => true,
    });
    if changed {
        *keys = settings.keys();
        *mouse = settings.mouse();
        pads.set_if_neq(settings.pads());
        pad.set_if_neq(settings.pad());
        volumes.set_if_neq(settings.volumes());
        display.set_if_neq(settings.display());
        shadows.set_if_neq(settings.shadows());
        duel.set_if_neq(settings.duel());
        lighting.set_if_neq(settings.lighting());
        effects.set_if_neq(settings.effects());
        occlusion.set_if_neq(settings.ambient_occlusion());
        camera.set_if_neq(settings.camera());
        if let Some(mut ui) = ui {
            settings.shown(&mut ui);
        }
    }
}

#[cfg(feature = "probes")]
pub fn binding_probes() -> sim::probe::Checks {
    probes::checks()
}

/// The settings' file as a start reads it (issue 279): one that does not
/// read is kept aside and the defaults are saved in its place.
#[cfg(feature = "probes")]
pub fn file_probes() -> sim::probe::Checks {
    probes::file_checks()
}

#[cfg(feature = "probes")]
#[path = "settings_probes.rs"]
mod probes;
