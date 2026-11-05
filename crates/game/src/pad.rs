//! Gamepads in play, and the keys beside them: every frame the keys (as
//! the key map binds them) and the pads (as the pad map binds them) are
//! read once into `Actions`, which play, the screens' hotkeys and the HUD's
//! prompts all read. A pad's left stick moves as far as it is pushed (the
//! sim scales the step by the command's axes, walking below
//! `WALK_AXIS`), its right stick turns the view at a rate, faster the
//! longer it is held over; the triggers attack, the bumpers cast and shift.
//! The mouse's buttons are bindings as the keys are (`settings::Input`).
//!
//! A pad has two layers: its buttons alone, and its buttons with LB held
//! (`Chord::shifted`), which is no action of its own. While a screen, a
//! conversation or a movie has the pad, play sees none of it: the screens
//! read it (`ui_input`, `dialogue_view`), and what is still held when they
//! let it go is taken (`swallow`) so the A that closed a screen does not
//! jump.

use std::collections::BTreeMap;

use bevy::ecs::system::SystemParam;
use bevy::input::gamepad::{Gamepad, GamepadButton, GamepadRumbleIntensity, GamepadRumbleRequest};
use bevy::prelude::*;

use crate::settings::{Action, Input, KeyMap};

/// The button held for the second layer.
pub(crate) const SHIFT: GamepadButton = GamepadButton::LeftTrigger;

/// A pad binding: a button, alone or with `SHIFT` held.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Chord {
    pub shifted: bool,
    pub button: GamepadButton,
}

const fn alone(button: GamepadButton) -> Chord {
    Chord {
        shifted: false,
        button,
    }
}

const fn shifted(button: GamepadButton) -> Chord {
    Chord {
        shifted: true,
        button,
    }
}

/// Each action's default chord. Movement is the left stick's; Walk is how
/// far it is pushed. Two actions share a chord where their keys do (Ready
/// and Accept on Y, Follow and End on N) or where the screens take the
/// chord's other meaning (Leave on a conversation or a watched pazaak
/// table, Crouch in play). Start opens the menu and Select the inventory,
/// whose bumpers step through the other tabs, so the journal, map, party
/// and abilities have no chord of their own; the two hands, quick load,
/// the view, a break (the menu's too) and the bug report stay the
/// keyboard's. LB with the D-pad's left and right turns the target's
/// action (`CYCLE`), as KOTOR's pad does.
const PAD: [(Action, Chord); 26] = {
    use GamepadButton::*;
    [
        (Action::Attack, alone(RightTrigger2)),
        (Action::AltAttack, alone(LeftTrigger2)),
        (Action::Jump, alone(South)),
        (Action::Crouch, alone(East)),
        (Action::LeaveTalk, alone(East)),
        (Action::Use, alone(West)),
        (Action::Leader, alone(North)),
        (Action::Power, alone(RightTrigger)),
        (Action::Stealth, alone(LeftThumb)),
        (Action::SaberStyle, alone(RightThumb)),
        (Action::NextPower, alone(DPadUp)),
        (Action::PreviousPower, alone(DPadDown)),
        (Action::Medpac, alone(DPadLeft)),
        (Action::Grenade, alone(DPadRight)),
        (Action::Inventory, alone(Select)),
        (Action::Holster, shifted(South)),
        (Action::LayMine, shifted(East)),
        (Action::Security, shifted(West)),
        (Action::SwitchWeapons, shifted(North)),
        (Action::Form, shifted(LeftThumb)),
        (Action::NextFeat, shifted(RightTrigger)),
        (Action::Shield, shifted(DPadUp)),
        (Action::Stim, shifted(DPadDown)),
        (Action::QuickSave, shifted(Start)),
        (Action::Ready, shifted(Select)),
        (Action::Follow, shifted(RightThumb)),
    ]
};

/// The chords that answer a join request, beside `PAD`'s Ready and Follow
/// (an action has one chord there).
const SHARED: [(Action, Chord); 2] = [
    (Action::AcceptCall, shifted(GamepadButton::Select)),
    (Action::EndCall, shifted(GamepadButton::RightThumb)),
];

/// The target's action back and on (`target_view`, the mouse wheel's).
const CYCLE: [(Chord, i8); 2] = [
    (shifted(GamepadButton::DPadLeft), -1),
    (shifted(GamepadButton::DPadRight), 1),
];

/// A button's name as the settings file keeps it (`South`, `DPadUp`).
pub fn button_name(button: GamepadButton) -> String {
    format!("{button:?}")
}

/// The buttons a chord may name.
const BUTTONS: [GamepadButton; 17] = {
    use GamepadButton::*;
    [
        South,
        East,
        West,
        North,
        LeftTrigger,
        RightTrigger,
        LeftTrigger2,
        RightTrigger2,
        Select,
        Start,
        LeftThumb,
        RightThumb,
        DPadUp,
        DPadDown,
        DPadLeft,
        DPadRight,
        Mode,
    ]
};

impl Chord {
    /// As the settings file keeps it: `South`, `LB+West`.
    pub fn name(self) -> String {
        let button = button_name(self.button);
        if self.shifted {
            format!("LB+{button}")
        } else {
            button
        }
    }

    /// The chord a name names (`Chord::name`), if it names one.
    pub fn named(name: &str) -> Option<Chord> {
        let (shifted, button) = match name.strip_prefix("LB+") {
            Some(rest) => (true, rest),
            None => (false, name),
        };
        let button = BUTTONS
            .iter()
            .copied()
            .find(|candidate| button_name(*candidate) == button)
            .filter(|button| *button != SHIFT)?;
        Some(Chord { shifted, button })
    }
}

/// Which chords do what.
#[derive(Resource, Debug, Clone, PartialEq)]
pub struct PadMap(BTreeMap<Action, Chord>);

impl Default for PadMap {
    fn default() -> PadMap {
        PadMap(PAD.iter().chain(&SHARED).copied().collect())
    }
}

impl PadMap {
    /// The defaults, made once: for a frame read with no settings in place
    /// (`kjk sim view`, a headless run).
    pub fn defaults() -> &'static PadMap {
        static DEFAULTS: std::sync::LazyLock<PadMap> = std::sync::LazyLock::new(PadMap::default);
        &DEFAULTS
    }

    /// The defaults with `bound` over them (`pad.<action>` in the settings,
    /// by the action's settings name); `None` there unbinds it.
    pub fn with(bound: impl IntoIterator<Item = (Action, Option<Chord>)>) -> PadMap {
        let mut map = PadMap::default();
        for (action, chord) in bound {
            match chord {
                Some(chord) => map.0.insert(action, chord),
                None => map.0.remove(&action),
            };
        }
        map
    }

    pub fn chord(&self, action: Action) -> Option<Chord> {
        self.0.get(&action).copied()
    }
}

/// How a pad turns the view and how far its sticks rest.
#[derive(Resource, Debug, Clone, Copy, PartialEq)]
pub struct PadFeel {
    /// Degrees a second the view turns across at the stick's edge.
    pub turn: f32,
    pub invert: bool,
    /// How far a stick is pushed before it counts (0 to 1).
    pub dead: f32,
    pub rumble: bool,
}

impl Default for PadFeel {
    fn default() -> PadFeel {
        PadFeel {
            turn: 200.0,
            invert: false,
            dead: 0.15,
            rumble: true,
        }
    }
}

/// The view turns up and down this much of its turn across.
const PITCH_SHARE: f32 = 0.65;

/// Held at the stick's edge, the turn grows by this share of itself over
/// `BOOST_SECONDS` (a long turn speeds up; aiming near stays fine).
const BOOST: f32 = 0.7;
const BOOST_SECONDS: f32 = 0.45;
const EDGE: f32 = 0.92;

/// The stick past this is all the way over (worn sticks never reach 1).
const OUTER: f32 = 0.95;

/// `stick` with the dead middle taken out, its length rescaled from there
/// to the edge (0 to 1), its direction kept.
pub(crate) fn rest(stick: Vec2, dead: f32) -> Vec2 {
    let length = stick.length();
    if length <= dead {
        return Vec2::ZERO;
    }
    let scaled = ((length - dead) / (OUTER - dead)).min(1.0);
    stick * (scaled / length)
}

/// The least share of a full step a stick pushed past its middle asks for:
/// the slowest walk, not a crawl in place.
const LEAST_STEP: f32 = 0.2;

/// How far below sideways the stick still strafes (the tangent of 30°):
/// the sim backpedals for any backward push, so a stick held sideways and a
/// little down would turn the run into a backpedal.
const STRAFE_SLOPE: f32 = 0.577;

/// The buttons whose moves the sim picks by the keys' directions (a roll,
/// a flip or cartwheel, a saber's lunge or kick): while one is held the
/// stick reads as the keys would, one of eight ways.
const DIRECTED: [Action; 4] = [
    Action::Jump,
    Action::Crouch,
    Action::Attack,
    Action::AltAttack,
];

/// The command's forward and right axes: the movement keys' (`WALK_AXIS`
/// with `walk` held, as JA's client sends for a walk), or the stick's when
/// no key is held: from `LEAST_STEP` just past its middle to a full run at
/// its edge, the sim stepping as far as the axes ask. While a `DIRECTED`
/// button is held the stick snaps to the nearest of the keys' eight ways
/// (its push kept): the sim reads a move's direction from each axis' sign,
/// and a stick pushed left is rarely without some forward, which rolled
/// forward, or pushed forward without some side, which cartwheeled.
pub(crate) fn step(actions: &Actions, walk: bool) -> (i8, i8) {
    let speed = if walk {
        rules::movement::keys::WALK_AXIS as i8
    } else {
        127
    };
    let axis =
        |plus, minus| speed * i8::from(actions.held(plus)) - speed * i8::from(actions.held(minus));
    let keys = (
        axis(Action::Forward, Action::Backward),
        axis(Action::Right, Action::Left),
    );
    let mut stick = actions.stick;
    let length = stick.length();
    if keys != (0, 0) || length == 0.0 {
        return keys;
    }
    if !DIRECTED.iter().any(|action| actions.held(*action))
        && stick.y < 0.0
        && stick.y.abs() < stick.x.abs() * STRAFE_SLOPE
    {
        // Sideways and a little down strafes: no backpedal.
        stick = Vec2::new(stick.x.signum() * length, 0.0);
    }
    if DIRECTED.iter().any(|action| actions.held(*action)) {
        let eighth = std::f32::consts::FRAC_PI_4;
        let way = (stick.y.atan2(stick.x) / eighth).round() * eighth;
        let (y, x) = way.sin_cos();
        // As the keys hold both of a diagonal's axes full.
        let snap = |value: f32| {
            if value.abs() < 0.5 {
                0.0
            } else {
                value.signum() * length
            }
        };
        stick = Vec2::new(snap(x), snap(y));
    }
    let scale = (LEAST_STEP + (1.0 - LEAST_STEP) * length) / length;
    let axis = |value: f32| (value * scale * 127.0).round().clamp(-127.0, 127.0) as i8;
    (axis(stick.y), axis(stick.x))
}

/// Whether axes this short walk (the walking legs, not a slow run): the
/// sim runs any over `WALK_AXIS`, walk held or not.
pub(crate) fn walks(forward: i8, right: i8) -> bool {
    let most = forward.unsigned_abs().max(right.unsigned_abs());
    most > 0 && most <= rules::movement::keys::WALK_AXIS
}

/// The kind of pad, for its buttons' names.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PadKind {
    #[default]
    Xbox,
    PlayStation,
    Nintendo,
    Deck,
}

impl PadKind {
    /// By the pad's maker (a Steam Deck's own controls are Valve's, and
    /// Steam Input shows any pad as Valve's virtual one: on a Deck, the
    /// Deck's names).
    fn of(vendor: Option<u16>, product: Option<u16>) -> PadKind {
        match (vendor, product) {
            (Some(0x054c), _) => PadKind::PlayStation,
            (Some(0x057e), _) => PadKind::Nintendo,
            (Some(0x28de), Some(0x1205)) => PadKind::Deck,
            (Some(0x28de), _) if on_deck() => PadKind::Deck,
            _ => PadKind::Xbox,
        }
    }

    /// The kind `key` names (`PadKind::key`).
    pub fn named(key: &str) -> Option<PadKind> {
        [
            PadKind::Xbox,
            PadKind::PlayStation,
            PadKind::Nintendo,
            PadKind::Deck,
        ]
        .into_iter()
        .find(|kind| kind.key() == key)
    }

    /// Each button's name in the settings and as this kind names it, for
    /// the key mapping (`ui::vm::ViewModel::pad_names`).
    pub fn names(self) -> Vec<(String, String)> {
        BUTTONS
            .iter()
            .map(|button| (button_name(*button), self.name(*button).to_string()))
            .collect()
    }

    /// As the screens' icons name it (`ui::icons`).
    pub fn key(self) -> &'static str {
        match self {
            PadKind::Xbox => "xbox",
            PadKind::PlayStation => "playstation",
            PadKind::Nintendo => "nintendo",
            PadKind::Deck => "deck",
        }
    }

    /// A button's name on this kind of pad, as its prompts show it.
    pub fn name(self, button: GamepadButton) -> &'static str {
        use GamepadButton::*;
        use PadKind::*;
        match (self, button) {
            (PlayStation, South) => "Cross",
            (PlayStation, East) => "Circle",
            (PlayStation, West) => "Square",
            (PlayStation, North) => "Triangle",
            (PlayStation, Select) => "Create",
            (PlayStation, Start) => "Options",
            // Nintendo's face buttons are where Xbox's are, named across.
            (Nintendo, South) => "B",
            (Nintendo, East) => "A",
            (Nintendo, West) => "Y",
            (Nintendo, North) => "X",
            (Nintendo, LeftTrigger) => "L",
            (Nintendo, RightTrigger) => "R",
            (Nintendo, LeftTrigger2) => "ZL",
            (Nintendo, RightTrigger2) => "ZR",
            (Nintendo, Select) => "-",
            (Nintendo, Start) => "+",
            (Xbox, LeftTrigger) => "LB",
            (Xbox, RightTrigger) => "RB",
            (Xbox, LeftTrigger2) => "LT",
            (Xbox, RightTrigger2) => "RT",
            (Xbox, LeftThumb) => "LS",
            (Xbox, RightThumb) => "RS",
            (Xbox | Deck, Select) => "View",
            (Xbox | Deck, Start) => "Menu",
            (_, LeftTrigger) => "L1",
            (_, RightTrigger) => "R1",
            (_, LeftTrigger2) => "L2",
            (_, RightTrigger2) => "R2",
            (_, LeftThumb) => "L3",
            (_, RightThumb) => "R3",
            (_, South) => "A",
            (_, East) => "B",
            (_, West) => "X",
            (_, North) => "Y",
            (_, DPadUp) => "Up",
            (_, DPadDown) => "Down",
            (_, DPadLeft) => "Left",
            (_, DPadRight) => "Right",
            (_, Mode) => "Guide",
            _ => "?",
        }
    }

    /// A chord as its prompts show it: `X`, `LB+A`, `L1+Cross`.
    pub fn chord(self, chord: Chord) -> String {
        let button = self.name(chord.button);
        if chord.shifted {
            format!("{}+{button}", self.name(SHIFT))
        } else {
            button.to_string()
        }
    }
}

/// Whether this runs on a Steam Deck: Steam sets `SteamDeck` for its own
/// games, and the machine names itself Valve's Jupiter (LCD) or Galileo
/// (OLED) to all (a non-Steam game gets no `SteamDeck`).
pub(crate) fn on_deck() -> bool {
    static DECK: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *DECK.get_or_init(|| {
        let read = |name: &str| std::fs::read_to_string(format!("/sys/class/dmi/id/{name}"));
        let valve = read("board_vendor").is_ok_and(|vendor| vendor.trim() == "Valve");
        let model =
            read("product_name").is_ok_and(|name| ["Jupiter", "Galileo"].contains(&name.trim()));
        std::env::var("SteamDeck").is_ok_and(|value| value == "1") || valve && model
    })
}

/// What the player last played with.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Device {
    #[default]
    Keys,
    Pad(PadKind),
}

/// This frame's actions, from the keys and the pads alike.
#[derive(Resource, Debug, Clone, Copy, Default)]
pub struct Actions {
    held: u64,
    pressed: u64,
    /// The left stick, rested (`rest`): x right, y forward.
    pub stick: Vec2,
    /// Degrees the right stick turns the view this frame (x to the right,
    /// y down, as `Look::turn` takes them).
    pub turn: Vec2,
    /// The target's action moved on (1) or back (-1) this frame (`CYCLE`).
    pub cycle: i8,
    /// Steps the item key held takes through its kinds this frame
    /// (`PICK_STEP`, `pick_view`).
    pub pick_step: i8,
    pub device: Device,
}

const fn bit(action: Action) -> u64 {
    1 << action as u32
}

impl Actions {
    pub fn held(&self, action: Action) -> bool {
        self.held & bit(action) != 0
    }

    /// Pressed this frame.
    pub fn pressed(&self, action: Action) -> bool {
        self.pressed & bit(action) != 0
    }

    /// Pressed this frame as `pick_view` has it (a run's commands).
    pub(crate) fn set_pressed(&mut self, action: Action) {
        self.pressed |= bit(action);
    }

    /// Held or not as `pick_view` has it, whatever the device says.
    pub(crate) fn set_held(&mut self, action: Action, on: bool) {
        self.held = if on {
            self.held | bit(action)
        } else {
            self.held & !bit(action)
        };
    }
}

/// The bindings as the prompts show them (the HUD's slots, the request
/// card, a conversation's Leave): the pad's while one plays.
#[derive(SystemParam)]
pub(crate) struct Prompts<'w> {
    keys: Option<Res<'w, KeyMap>>,
    pads: Option<Res<'w, PadMap>>,
    actions: Option<Res<'w, Actions>>,
}

impl Prompts<'_> {
    /// `action`'s binding: its chord while a pad plays (its key when it
    /// has none), else its key.
    pub(crate) fn shown(&self, action: Action) -> Vec<u8> {
        let keys = self.keys.as_deref().unwrap_or(KeyMap::defaults());
        let device = self
            .actions
            .as_ref()
            .map_or(Device::Keys, |actions| actions.device);
        let chord = self.pads.as_ref().and_then(|pads| pads.chord(action));
        match (device, chord) {
            (Device::Pad(kind), Some(chord)) => kind.chord(chord).into_bytes(),
            _ => keys.shown(action),
        }
    }

    /// A control's token in the games' text (`<abutton>`, `<ActionUp>`;
    /// `stringtokens.2da`, action codes 24 and 25) as this player's
    /// binding of what it asks for: the executable's `GetTokenValue` writes
    /// the key `keymap.2da` binds to the row's action (25), and for the
    /// Xbox's buttons (24) the font's glyph of the button, so each is the
    /// binding of the action the button does there (A uses, Y switches
    /// weapons, Black changes the leader). The rest stay as written.
    pub(crate) fn token(&self, token: &str) -> Option<String> {
        let action = CONTROL_TOKENS
            .iter()
            .find(|(name, _)| *name == token)
            .map(|(_, action)| *action)?;
        Some(self.shown(action).into_iter().map(char::from).collect())
    }

    /// Whether `action` was pressed this frame.
    pub(crate) fn pressed(&self, action: Action) -> bool {
        self.actions
            .as_ref()
            .is_some_and(|actions| actions.pressed(action))
    }

    /// `action`'s binding over a HUD slot: while a pad plays, its buttons
    /// drawn (`ui::icons`, compact where words would overflow); else its
    /// key, a mouse button as `M1` (`M4` the side's first): short enough
    /// to fit a slot's top on one line.
    pub(crate) fn label(&self, action: Action) -> ui::vm::KeyLabel {
        let device = self
            .actions
            .as_ref()
            .map_or(Device::Keys, |actions| actions.device);
        let chord = self.pads.as_ref().and_then(|pads| pads.chord(action));
        let badge = match (device, chord) {
            (Device::Pad(kind), Some(chord)) => {
                Some(ui::icons::pad_texture(kind.key(), &chord.name()))
            }
            _ => None,
        };
        let keys = self.keys.as_deref().unwrap_or(KeyMap::defaults());
        let words = match keys.input(action) {
            Input::Mouse(button) => format!("M{}", mouse_number(button)).into_bytes(),
            Input::Key(_) => keys.shown(action),
        };
        ui::vm::KeyLabel { words, badge }
    }
}

/// The controls' tokens of `stringtokens.2da` the games' text uses, each
/// with the action it asks for here (`Prompts::token`).
const CONTROL_TOKENS: [(&str, Action); 20] = [
    ("abutton", Action::Use),
    ("ybutton", Action::SwitchWeapons),
    ("blbutton", Action::Leader),
    ("ActionUp", Action::Forward),
    ("ActionDown", Action::Backward),
    ("ActionLeft", Action::Left),
    ("ActionRight", Action::Right),
    ("MGActionLeft", Action::Left),
    ("MGActionRight", Action::Right),
    ("ChangeChar", Action::Leader),
    ("Equip", Action::Inventory),
    ("Inventory", Action::Inventory),
    ("Map", Action::Map),
    ("Quests", Action::Journal),
    ("Quicksave", Action::QuickSave),
    ("QUICKLOAD", Action::QuickLoad),
    ("STEALTH", Action::Stealth),
    ("PersonalPowerAct", Action::Power),
    ("PersonalMedicalAct", Action::Medpac),
    ("PersonalMinesAct", Action::LayMine),
];

/// A mouse button's number, as players count them (the side buttons 4 and
/// 5).
fn mouse_number(button: MouseButton) -> u16 {
    match button {
        MouseButton::Left => 1,
        MouseButton::Right => 2,
        MouseButton::Middle => 3,
        MouseButton::Back => 4,
        MouseButton::Forward => 5,
        MouseButton::Other(n) => n,
    }
}

/// The pad presses the screens read (`dialogue_view`, `movie_view`): the
/// first layer's, this frame, from any pad, and the left stick flicked
/// (each axis -1 or 1 the frame it is pushed past `FLICK`, up and right).
#[derive(Resource, Debug, Clone, Default)]
pub struct PadPresses {
    pressed: Vec<GamepadButton>,
    pub flick: IVec2,
}

impl PadPresses {
    pub fn any(&self, buttons: &[GamepadButton]) -> bool {
        buttons.iter().any(|button| self.pressed.contains(button))
    }
}

/// With an item's chord held and kinds to choose among, the button that steps
/// through them and, until it is let go, does nothing else (`pick_view`): RB.
/// LB is the second layer, so it cannot step back.
const PICK_STEP: GamepadButton = GamepadButton::RightTrigger;

/// How far the stick is pushed to flick.
const FLICK: f32 = 0.6;

/// What a pad keeps while the screens have it: their hotkeys (Select
/// closes the inventory it opened), and Leave, which a watched pazaak
/// table's screen takes (`pazaak_view::watch`).
const SCREEN_ACTIONS: [Action; 6] = [
    Action::Inventory,
    Action::Journal,
    Action::Map,
    Action::Party,
    Action::Abilities,
    Action::LeaveTalk,
];

/// The keyboard and the mouse, and whether the mouse holds the cursor.
#[derive(SystemParam)]
pub(crate) struct Desk<'w, 's> {
    keys: Option<Res<'w, ButtonInput<KeyCode>>>,
    mouse: Option<Res<'w, ButtonInput<MouseButton>>>,
    cursors: Query<'w, 's, &'static bevy::window::CursorOptions, With<bevy::window::PrimaryWindow>>,
}

/// The bindings and the pad's feel, as the settings have them, and
/// whether the run has no window (a headless run reads no device: it
/// plays only what its script says).
type Bound<'w> = (
    Option<Res<'w, KeyMap>>,
    Option<Res<'w, PadMap>>,
    Option<Res<'w, PadFeel>>,
    Res<'w, crate::scene::Headless>,
);

/// Reads this frame's `Actions` (after bevy's input, before play). The
/// screens have the pad while one of `Busy` is up (a screen, a
/// conversation, a movie).
#[allow(clippy::too_many_arguments)]
pub(crate) fn sample(
    desk: Desk,
    (key_map, pad_map, feel, headless): Bound,
    mut gamepads: Query<&mut Gamepad>,
    taken: crate::leader_view::Busy,
    time: Res<Time>,
    mut actions: ResMut<Actions>,
    mut presses: ResMut<PadPresses>,
    (mut boost, mut was_busy, mut flicked, mut stepping): (
        Local<f32>,
        Local<bool>,
        Local<IVec2>,
        Local<bool>,
    ),
    picking: Option<Res<crate::pick_view::Picking>>,
) {
    if headless.0 {
        *actions = Actions::default();
        *presses = PadPresses::default();
        return;
    }
    let key_map = key_map.as_deref().unwrap_or(KeyMap::defaults());
    let pad_map = pad_map.as_deref().unwrap_or(PadMap::defaults());
    let feel = feel.as_deref().copied().unwrap_or_else(PadFeel::default);
    let menus = crate::leader_view::busy(taken);
    // Let go by the screens: what is still held reads as up until released
    // (LB stays: it is no action, and a held shift carries on).
    if *was_busy && !menus {
        for mut pad in &mut gamepads {
            let held: Vec<GamepadButton> = pad
                .digital()
                .get_pressed()
                .copied()
                .filter(|button| *button != SHIFT)
                .collect();
            for button in held {
                pad.digital_mut().reset(button);
            }
        }
    }
    *was_busy = menus;

    let mut next = Actions {
        device: actions.device,
        ..Actions::default()
    };
    // The mouse's buttons count while it holds the cursor: the click that
    // takes it (`swallow`) and one on a free pointer are the pointer's.
    let grabbed = desk
        .cursors
        .iter()
        .any(|cursor| cursor.grab_mode != bevy::window::CursorGrabMode::None);
    let mouse = desk.mouse.as_deref().filter(|_| grabbed);
    for (action, _, _, _, _) in crate::settings::ACTIONS {
        let (held, pressed) = match key_map.input(action) {
            Input::Key(key) => desk.keys.as_deref().map_or((false, false), |keys| {
                (keys.pressed(key), keys.just_pressed(key))
            }),
            Input::Mouse(button) => mouse.map_or((false, false), |mouse| {
                (mouse.pressed(button), mouse.just_pressed(button))
            }),
        };
        next.held |= u64::from(held) * bit(action);
        next.pressed |= u64::from(pressed) * bit(action);
    }
    if let Some(keys) = desk.keys.as_deref() {
        // The right Shift walks and the left Ctrl crouches too.
        next.held |= u64::from(keys.pressed(KeyCode::ShiftRight)) * bit(Action::Walk);
        next.held |= u64::from(keys.pressed(KeyCode::ControlLeft)) * bit(Action::Crouch);
    }
    let typed = desk
        .keys
        .as_deref()
        .is_some_and(|keys| keys.get_just_pressed().next().is_some());
    let clicked = desk
        .mouse
        .as_deref()
        .is_some_and(|mouse| mouse.get_just_pressed().next().is_some());
    if typed || clicked {
        next.device = Device::Keys;
    }

    presses.pressed.clear();
    let mut pushed = IVec2::ZERO;
    let mut look = Vec2::ZERO;
    for pad in &gamepads {
        let used = pad.digital().get_just_pressed().next().is_some()
            || rest(pad.left_stick(), feel.dead) != Vec2::ZERO
            || rest(pad.right_stick(), feel.dead) != Vec2::ZERO;
        if used {
            next.device = Device::Pad(PadKind::of(pad.vendor_id(), pad.product_id()));
        }
        let shift = pad.pressed(SHIFT);
        presses
            .pressed
            .extend(pad.digital().get_just_pressed().filter(|_| !shift).copied());
        let axis = |value: f32| i32::from(value > FLICK) - i32::from(value < -FLICK);
        let stick = pad.left_stick();
        if pushed == IVec2::ZERO {
            pushed = IVec2::new(axis(stick.x), axis(stick.y));
        }
        let choosing = picking
            .as_ref()
            .filter(|_| !menus)
            .and_then(|picking| picking.choosing());
        if choosing.is_some() && pad.just_pressed(PICK_STEP) {
            next.pick_step += 1;
            *stepping = true;
        }
        for (action, chord) in &pad_map.0 {
            let layer = chord.shifted == shift || choosing == Some(*action);
            if menus && !SCREEN_ACTIONS.contains(action) || !layer {
                continue;
            }
            if *stepping && chord.button == PICK_STEP {
                continue;
            }
            next.held |= u64::from(pad.pressed(chord.button)) * bit(*action);
            next.pressed |= u64::from(pad.just_pressed(chord.button)) * bit(*action);
        }
        if menus {
            continue;
        }
        for (chord, step) in CYCLE.iter().filter(|(chord, _)| chord.shifted == shift) {
            next.cycle += step * i8::from(pad.just_pressed(chord.button));
        }
        let stick = rest(pad.left_stick(), feel.dead);
        if stick.length() > next.stick.length() {
            next.stick = stick;
        }
        let right = rest(pad.right_stick(), feel.dead);
        if right.length() > look.length() {
            look = right;
        }
    }

    *stepping = *stepping && gamepads.iter().any(|pad| pad.pressed(PICK_STEP));
    presses.flick = IVec2::select(pushed.cmpne(*flicked), pushed, IVec2::ZERO);
    *flicked = pushed;

    // The turn: the stick's push squared (fine near the middle), growing
    // while it is held at the edge.
    let seconds = time.delta_secs();
    *boost = if look.length() >= EDGE {
        (*boost + seconds / BOOST_SECONDS).min(1.0)
    } else {
        0.0
    };
    let rate = feel.turn * (1.0 + BOOST * *boost);
    let down = if feel.invert { 1.0 } else { -1.0 };
    next.turn = Vec2::new(
        look.x * look.length() * rate * seconds,
        down * look.y * look.length() * rate * PITCH_SHARE * seconds,
    );
    *actions = next;
}

/// The pad's buttons as the key mapping names them (`South` as `A`,
/// `Cross`), for the pad played with last (an Xbox pad's until one plays),
/// and whether a pad is what plays (the key mapping opens on its tab).
fn pad_names(
    actions: Res<Actions>,
    ui: Option<ResMut<crate::ui_view::Ui>>,
    mut named: Local<Option<(PadKind, bool)>>,
) {
    let kind = match actions.device {
        Device::Pad(kind) => kind,
        Device::Keys => named.map_or(PadKind::Xbox, |(kind, _)| kind),
    };
    let now = (kind, matches!(actions.device, Device::Pad(_)));
    let Some(mut ui) = ui.filter(|_| *named != Some(now)) else {
        return;
    };
    *named = Some(now);
    ui.vm.pad_kind = kind.key().to_string();
    ui.vm.pad_names = kind.names();
    ui.vm.pad_playing = now.1;
}

/// A shake of the pad the player plays with, if it shakes.
#[derive(Message, Debug, Clone, Copy)]
pub(crate) struct Rumble {
    /// 0 to 1.
    pub strength: f32,
    pub seconds: f32,
}

/// A hit taken shakes the pad; one dealt ticks it.
const TAKEN: Rumble = Rumble {
    strength: 0.55,
    seconds: 0.2,
};
const DEALT: Rumble = Rumble {
    strength: 0.18,
    seconds: 0.06,
};

/// The player's own character's hits, as the damage numbers follow them
/// (`floaty_view`): each fighter's events not yet seen, those that landed
/// on the player or that the player dealt.
fn felt(
    presented: Option<Res<crate::sim_host::PresentedSnapshot>>,
    seat: Option<Res<crate::sim_host::OwnSeat>>,
    mut seen: Local<BTreeMap<sim::SimId, u16>>,
    mut shakes: MessageWriter<Rumble>,
) {
    let Some(presented) = presented else {
        return;
    };
    let snapshot = presented.0.current();
    let Some(me) = crate::cast::me(snapshot, seat.map_or(sim::Seat(0), |seat| seat.0)) else {
        return;
    };
    let landed = |event: &&sim::combat::Event| {
        event.damage > 0
            && matches!(
                event.result,
                sim::combat::Outcome::Hit
                    | sim::combat::Outcome::Critical
                    | sim::combat::Outcome::Automatic
            )
    };
    for entity in &snapshot.entities {
        let Some(fighter) = entity.fighter.as_ref() else {
            continue;
        };
        let Some(last) = seen.insert(entity.id, fighter.next_event) else {
            continue;
        };
        let fresh = fighter.next_event.wrapping_sub(last);
        if fresh > u16::MAX / 2 {
            continue;
        }
        for event in fighter.newest().take(usize::from(fresh)).filter(landed) {
            if entity.id == me {
                shakes.write(TAKEN);
            } else if event.by == Some(me) {
                shakes.write(DEALT);
            }
        }
    }
}

/// Each pad as it connects, in the run's log: its name and maker, to tell
/// from a report which device a player's buttons came from.
fn connected(pads: Query<(Entity, &Gamepad, Option<&Name>), Added<Gamepad>>) {
    for (entity, pad, name) in &pads {
        let kind = PadKind::of(pad.vendor_id(), pad.product_id());
        info!(
            target: "pad",
            ?entity,
            name = name.map_or("?", |name| name.as_str()),
            vendor = ?pad.vendor_id().map(|id| format!("{id:04x}")),
            product = ?pad.product_id().map(|id| format!("{id:04x}")),
            ?kind,
            "pad connected"
        );
    }
}

/// How many presses a run's log keeps (`heard`).
const PRESSES_LOGGED: u32 = 40;

/// The first presses (a pad's buttons and the keys) and each pad's first
/// push of either stick, in the run's log: what a player's pad sends, by
/// the names it arrives as.
fn heard(
    pads: Query<(Entity, &Gamepad)>,
    keys: Option<Res<ButtonInput<KeyCode>>>,
    mut logged: Local<(u32, Vec<(Entity, bool)>)>,
) {
    let (count, moved) = &mut *logged;
    // Keys too: Steam can turn a pad's buttons into keys (a keyboard layout).
    for key in keys.iter().flat_map(|keys| keys.get_just_pressed()) {
        if *count < PRESSES_LOGGED {
            *count += 1;
            info!(target: "pad", ?key, "key pressed");
        }
    }
    for (entity, pad) in &pads {
        for button in pad.digital().get_just_pressed() {
            if *count < PRESSES_LOGGED {
                *count += 1;
                info!(target: "pad", ?entity, ?button, "pressed");
            }
        }
        for (right, stick) in [(false, pad.left_stick()), (true, pad.right_stick())] {
            if stick.length() > 0.5 && !moved.contains(&(entity, right)) {
                moved.push((entity, right));
                let side = if right { "right" } else { "left" };
                info!(target: "pad", ?entity, side, x = stick.x, y = stick.y, "stick moved");
            }
        }
    }
}

fn rumble(
    mut asked: MessageReader<Rumble>,
    feel: Option<Res<PadFeel>>,
    actions: Res<Actions>,
    gamepads: Query<Entity, With<Gamepad>>,
    mut requests: MessageWriter<GamepadRumbleRequest>,
) {
    let on = feel.is_none_or(|feel| feel.rumble) && matches!(actions.device, Device::Pad(_));
    for shake in asked.read().filter(|_| on) {
        let strength = shake.strength.clamp(0.0, 1.0);
        for gamepad in &gamepads {
            requests.write(GamepadRumbleRequest::Add {
                gamepad,
                duration: std::time::Duration::from_secs_f32(shake.seconds),
                intensity: GamepadRumbleIntensity {
                    strong_motor: strength,
                    weak_motor: strength,
                },
            });
        }
    }
}

/// Where `Actions` is read: play's systems run after it.
#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) struct Sampled;

/// A pad's player coming to an edit box (a save's name, the character's,
/// an address) gets Steam's keyboard on the screen, where Steam shows one
/// (a Steam Deck, Big Picture); elsewhere STEAM + X or a keyboard types.
#[cfg(feature = "steam")]
fn on_screen_keyboard(
    ui: Option<ResMut<crate::ui_view::Ui>>,
    actions: Res<Actions>,
    windows: Query<&Window, With<bevy::window::PrimaryWindow>>,
    mut shown: Local<bool>,
) {
    let typing = ui.is_some_and(|mut ui| ui.bypass_change_detection().stack.typing());
    if typing && !*shown && matches!(actions.device, Device::Pad(_)) {
        // The keyboard keeps clear of the top third, where the screens'
        // edit boxes sit.
        let field = windows.iter().next().map_or([0, 0, 1280, 240], |window| {
            let (width, height) = (window.physical_width(), window.physical_height());
            [0, 0, width as i32, height as i32 / 3]
        });
        if !net::io::steam::floating_keyboard(field) {
            debug!(target: "pad", "no keyboard on the screen here");
        }
    }
    *shown = typing;
}

pub struct PadPlugin;

impl Plugin for PadPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Actions>()
            .init_resource::<PadPresses>()
            .add_message::<Rumble>()
            // Bevy's, which a run without its gamepad plugin (headless)
            // lacks.
            .add_message::<GamepadRumbleRequest>()
            .add_systems(
                PreUpdate,
                sample
                    .in_set(Sampled)
                    .after(bevy::input::InputSystems)
                    .after(crate::swallow::hold_up),
            )
            .add_systems(Update, ((felt, rumble).chain(), pad_names, connected, heard));
        #[cfg(feature = "steam")]
        app.add_systems(Update, on_screen_keyboard.after(crate::ui_view::Stepped));
    }
}

pub mod linux;

#[cfg(feature = "probes")]
pub fn probes() -> sim::probe::Checks {
    let mut checks = probes::checks();
    checks.extend(linux::probes());
    checks
}

#[cfg(feature = "probes")]
mod probes;
