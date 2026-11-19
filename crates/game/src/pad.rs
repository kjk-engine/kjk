//! Gamepads in play, and the keys beside them: every frame the keys (as
//! the key map binds them) and the pads (as the pad map binds them) are
//! read once into `Actions`, which play, the screens' hotkeys and the HUD's
//! prompts all read. A pad's left stick moves as far as it is pushed (the
//! sim scales the step by the command's axes, walking below
//! `WALK_AXIS`), its right stick turns the view at a rate, faster the
//! longer it is held over. The triggers attack. RB held opens the power
//! wheel, and LB held blocks.
//! The mouse's buttons are bindings as the keys are (`settings::Input`).
//!
//! A pad has two layers (`Layer`): its buttons alone, and its buttons with
//! LB held, which use the action bar and the actions the first layer has no
//! room for. LB is both the second layer's button and, alone, Block by
//! default. RB is no layer. While a screen, a conversation or a movie has
//! the pad, play sees none
//! of it. The screens read it instead (`ui_input`, `dialogue_view`). What
//! is still held when they let it go is taken (`swallow`), so the A that
//! closed a screen does not make the character jump.
//!
//! A stick axis whose first change since its pad connected lands off its
//! middle counts only once it has come back to the middle (`Settle`): a controller that is no gamepad
//! (a flight throttle, pedals) shows up as one, its parked axis read as a
//! stick, and would otherwise turn the view or walk on its own.

use std::collections::BTreeMap;

use bevy::ecs::system::SystemParam;
use bevy::input::gamepad::{
    Gamepad, GamepadAxis, GamepadAxisChangedEvent, GamepadButton, GamepadRumbleIntensity,
    GamepadRumbleRequest,
};
use bevy::prelude::*;

use sim::combat::bar::{Bar, Content};
use sim::combat::items::Shelf;

use crate::settings::{Action, BAR, Input, KeyMap, Mods};

/// LB, the button held for the second layer, which has the action bar. LB
/// alone may also be bound to one action, Block by default.
pub(crate) const SHIFT: GamepadButton = GamepadButton::LeftTrigger;

/// RB, the power wheel's button by default. It is an ordinary button and no
/// layer.
pub(crate) const RB: GamepadButton = GamepadButton::RightTrigger;

/// Whether LB is held with a chord's button: no (`Alone`) or yes (`Lb`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Layer {
    Alone,
    Lb,
}

impl Layer {
    /// The layer `pad` is on: LB's while LB is held, else the first.
    pub(crate) fn of(pad: &Gamepad) -> Layer {
        if pad.pressed(SHIFT) {
            Layer::Lb
        } else {
            Layer::Alone
        }
    }

    /// The bumper held for this layer, and its name in a chord's name
    /// (`LB+South`). The first layer has none.
    fn bumper(self) -> Option<(GamepadButton, &'static str)> {
        match self {
            Layer::Alone => None,
            Layer::Lb => Some((SHIFT, "LB")),
        }
    }
}

/// A pad binding: a button, alone or with LB held (`Layer`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Chord {
    pub layer: Layer,
    pub button: GamepadButton,
}

const fn alone(button: GamepadButton) -> Chord {
    Chord {
        layer: Layer::Alone,
        button,
    }
}

const fn shifted(button: GamepadButton) -> Chord {
    Chord {
        layer: Layer::Lb,
        button,
    }
}

/// Whether a chord's name (`RightTrigger`, `LB+RightTrigger`) has RB as its
/// button. Before layout 4 RB was free, and layout 4 held it for the action
/// bar's layer, so a file of a layout before 4 drops such chords
/// (`bar_layout`).
pub(crate) fn names_rb(chord: &str) -> bool {
    chord.rsplit('+').next() == Some(button_name(RB).as_str())
}

/// Each action's default chord. Movement is the left stick's; Walk is how
/// far it is pushed. Two actions share a chord where their keys do (Ready
/// and Accept on Y, Follow and End on N) or where the screens take the
/// chord's other meaning (Leave on a conversation or a watched pazaak
/// table, Crouch in play). Start opens the menu and Select the inventory,
/// whose bumpers step through the other tabs, so the journal, map, party
/// and abilities have no chord of their own. The two hands, quick load,
/// the view, a break (the menu's too) and the bug report stay the
/// keyboard's.
///
/// RB held alone keeps the power wheel open. LB held alone blocks. A player
/// may rebind both, as every other action. LB is also the layer's button,
/// so a chord with LB held is named `LB+` and another button. LB held with
/// A, B, X or Y uses the action bar's first four slots. LB held with the
/// right trigger attacks while the block is held (`UNDER_BLOCK`), which
/// aims the block. The D-pad's left and right turn the target's action
/// (`CYCLE`), as KOTOR's pad does. The right stick's click is both Saber
/// style and the camera's orbit. Held, it orbits, and its stick pushed up
/// or down zooms. Tapped, it changes the style as it is let go
/// (`TAP_ON_ORBIT`).
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
        (Action::Stealth, alone(LeftThumb)),
        (Action::SaberStyle, alone(RightThumb)),
        (Action::Orbit, alone(RightThumb)),
        (Action::Wheel, alone(RB)),
        (Action::Block, alone(SHIFT)),
        (Action::Inventory, alone(Select)),
        (Action::NextFeat, alone(DPadUp)),
        (Action::Holster, alone(DPadDown)),
        (Action::Bar1, shifted(South)),
        (Action::Bar2, shifted(East)),
        (Action::Bar3, shifted(West)),
        (Action::Bar4, shifted(North)),
        (Action::SwitchWeapons, shifted(DPadUp)),
        (Action::Security, shifted(DPadDown)),
        (Action::Solo, shifted(LeftTrigger2)),
        (Action::Form, shifted(LeftThumb)),
        (Action::Follow, shifted(RightThumb)),
        (Action::QuickSave, shifted(Start)),
        (Action::Ready, shifted(Select)),
    ]
};

/// The action whose first-layer chord also works with LB held, as long as
/// no LB chord uses the same button. By default it is the attack on the
/// right trigger. With LB held it is the attack made while blocking, which
/// the combat rules take as aiming the block.
const UNDER_BLOCK: Action = Action::Attack;

/// The action that shares the orbit's chord (`PAD`): it fires as the chord
/// is let go, unless the stick turned the camera meanwhile.
const TAP_ON_ORBIT: Action = Action::SaberStyle;

/// The chords that answer a join request, beside `PAD`'s Ready and Follow
/// (an action has one chord there).
const SHARED: [(Action, Chord); 2] = [
    (Action::AcceptCall, shifted(GamepadButton::Select)),
    (Action::EndCall, shifted(GamepadButton::RightThumb)),
];

/// The target's action back and on (`target_view`, the mouse wheel's). The
/// D-pad's left and right do not move it while they step through an
/// item's kinds (`PICK_STEPS`).
const CYCLE: [(Chord, i8); 2] = [
    (alone(GamepadButton::DPadLeft), -1),
    (alone(GamepadButton::DPadRight), 1),
];

/// The target's `CYCLE` chords as the target block shows them: back, then
/// on, drawn as the buttons of the pad that plays. None while the keys play
/// (the mouse wheel has no hint).
pub(crate) fn cycle_badges(actions: Option<&Actions>) -> Option<[String; 2]> {
    let Device::Pad(kind) = actions?.device else {
        return None;
    };
    Some(CYCLE.map(|(chord, _)| ui::icons::pad_texture(kind.key(), &chord.name())))
}

/// The bumper that steps while the power wheel is open: LB steps on by one
/// entry (`sample`, `wheel_view`). RB is the wheel's own button by default,
/// held to keep it open, so no bumper steps back.
const WHEEL_STEPS: [(GamepadButton, i8); 1] = [(SHIFT, 1)];

/// The power wheel's bumper badges as the wheel shows them beside its two
/// arrows: back, then on, drawn as the buttons of the pad that plays. No
/// bumper steps back, so the back badge is an empty name, which draws
/// nothing. None while the keys play (the mouse wheel has no hint).
pub(crate) fn wheel_badges(actions: Option<&Actions>) -> Option<[String; 2]> {
    let Device::Pad(kind) = actions?.device else {
        return None;
    };
    let badge = |step: i8| {
        WHEEL_STEPS
            .iter()
            .find(|(_, on)| *on == step)
            .map_or_else(String::new, |(button, _)| {
                ui::icons::pad_texture(kind.key(), &alone(*button).name())
            })
    };
    Some([badge(-1), badge(1)])
}

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
    /// As the settings file keeps it: `South`, `RightTrigger`, `LB+South`.
    pub fn name(self) -> String {
        let button = button_name(self.button);
        match self.layer.bumper() {
            Some((_, bumper)) => format!("{bumper}+{button}"),
            None => button,
        }
    }

    /// The chord a name names (`Chord::name`), if it names one. Every button
    /// may be bound alone, the bumpers too. LB is no button of its own
    /// layer, so `LB+LeftTrigger` names no chord. RB is no layer, so a
    /// name such as `RB+South`, which older files keep, names no chord.
    pub fn named(name: &str) -> Option<Chord> {
        let (layer, button) = Layer::Lb
            .bumper()
            .and_then(|(_, bumper)| name.strip_prefix(bumper)?.strip_prefix('+'))
            .map_or((Layer::Alone, name), |button| (Layer::Lb, button));
        let button = BUTTONS
            .iter()
            .copied()
            .find(|candidate| button_name(*candidate) == button)
            .filter(|button| layer == Layer::Alone || *button != SHIFT)?;
        Some(Chord { layer, button })
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

    /// Whether an action is bound to `button` with LB held. Such a button
    /// does not fall through to `UNDER_BLOCK` while LB is held.
    fn on_lb(&self, button: GamepadButton) -> bool {
        self.0
            .values()
            .any(|chord| chord.layer == Layer::Lb && chord.button == button)
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

/// Whether play takes a stick axis of a pad. A gamepad's sticks spring back
/// to the middle, so an axis whose first change since the pad connected
/// lands off its middle (by more than the dead middle) is parked (a
/// throttle, a pedal) and reads as the middle until a change brings it back
/// there; one whose first change lands in the middle, as a gamepad's does
/// when its stick starts to move, counts at once and from then on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum Settle {
    /// No change heard since the pad connected: its axes read as at rest.
    #[default]
    Unheard,
    Parked,
    Live,
}

impl Settle {
    /// Moves on for a change of the axis to `value`.
    pub(crate) fn hear(&mut self, value: f32, dead: f32) {
        let centred = value.abs() <= dead;
        *self = match *self {
            Settle::Unheard | Settle::Parked if !centred => Settle::Parked,
            _ => Settle::Live,
        };
    }

    /// What play takes of the axis's `reading`.
    pub(crate) fn take(self, reading: f32) -> f32 {
        if self == Settle::Live { reading } else { 0.0 }
    }
}

/// The sticks' axes in `Settle` order: the left stick's x and y, the right's.
const STICK_AXES: [GamepadAxis; 4] = [
    GamepadAxis::LeftStickX,
    GamepadAxis::LeftStickY,
    GamepadAxis::RightStickX,
    GamepadAxis::RightStickY,
];

/// A pad's sticks as play and the screens take them (`Settle`), read each
/// frame before either. Every pad has one (`SticksPlugin`).
#[derive(Component, Debug, Clone, Copy, Default)]
pub struct Sticks {
    settle: [Settle; 4],
    pub left: Vec2,
    pub right: Vec2,
}

/// The pads with their sticks as play takes them, for the screens.
pub(crate) type Pads<'w, 's> = Query<'w, 's, (&'static Gamepad, &'static Sticks)>;

/// Each pad's `Sticks` for this frame, from the axes' changes heard since
/// it connected; a parked axis in the run's log, to tell from a report
/// which device would turn the view on its own.
fn settle(
    mut pads: Query<(Entity, Ref<Gamepad>, &mut Sticks)>,
    mut changes: MessageReader<GamepadAxisChangedEvent>,
    feel: Option<Res<PadFeel>>,
) {
    let dead = feel.map_or(PadFeel::default().dead, |feel| feel.dead);
    // A pad (re)connected is heard afresh: its axes start at rest.
    for (_, pad, mut sticks) in &mut pads {
        if pad.is_added() {
            *sticks = Sticks::default();
        }
    }
    for change in changes.read() {
        let Some(i) = STICK_AXES.iter().position(|axis| *axis == change.axis) else {
            continue;
        };
        let Ok((entity, _, mut sticks)) = pads.get_mut(change.entity) else {
            continue;
        };
        let was = sticks.settle[i];
        sticks.settle[i].hear(change.value, dead);
        if was == Settle::Unheard && sticks.settle[i] == Settle::Parked {
            info!(target: "pad", ?entity, axis = ?change.axis, value = change.value,
                "axis off its middle, ignored until it comes back");
        }
    }
    for (_, pad, mut sticks) in &mut pads {
        let [lx, ly, rx, ry] =
            std::array::from_fn(|i| sticks.settle[i].take(pad.get(STICK_AXES[i]).unwrap_or(0.0)));
        sticks.left = Vec2::new(lx, ly);
        sticks.right = Vec2::new(rx, ry);
    }
}

/// Gives every pad its `Sticks` and reads them each frame, after bevy's
/// input: for play (`PadPlugin`) and the setup's screen.
pub struct SticksPlugin;

impl Plugin for SticksPlugin {
    fn build(&self, app: &mut App) {
        app.register_required_components::<Gamepad, Sticks>()
            .add_message::<GamepadAxisChangedEvent>()
            .add_systems(
                PreUpdate,
                settle.in_set(Settled).after(bevy::input::InputSystems),
            );
    }
}

/// Where `Sticks` are read.
#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub struct Settled;

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

    /// A chord as its prompts show it: `X`, `RB`, `LB+A`, `L1+Cross`.
    pub fn chord(self, chord: Chord) -> String {
        let button = self.name(chord.button);
        match chord.layer.bumper() {
            Some((bumper, _)) => format!("{}+{button}", self.name(bumper)),
            None => button.to_string(),
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

/// The pad a headless run's commands say is played with (`ui pad KIND`,
/// `ui pad off`). A headless run reads no device, so its frames show the
/// keys' prompts unless the commands name a pad.
#[derive(Resource, Debug, Clone, Copy, Default)]
pub struct ScriptedPad(pub Option<PadKind>);

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
    /// (`PICK_STEPS`, `pick_view`).
    pub pick_step: i8,
    /// The right stick, rested (`rest`), from 0 to 1 each way: x right, y
    /// up. The power wheel's pointer follows it (`wheel_view`).
    pub aim: Vec2,
    /// Steps LB takes while the power wheel is open, 1 for each press
    /// (`WHEEL_STEPS`, `wheel_view`).
    pub wheel_step: i8,
    pub device: Device,
    /// Block and the alternate attack are bound to one key or mouse button
    /// (the right mouse button by default, `sim_host::share_button`).
    pub block_shared: bool,
}

pub(crate) const fn bit(action: Action) -> u64 {
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
    /// binding of what it asks for: the executable writes
    /// the key `keymap.2da` binds to the row's action (25), and for the
    /// Xbox's buttons (24) the font's glyph of the button, so each is the
    /// binding of the action the button does there (A uses, Y switches
    /// weapons, Black changes the leader). The medical and mines buttons
    /// are the key of the bar slot that holds medpacs or mines on `bar`,
    /// the player's own (`token_action`). The rest stay as written.
    pub(crate) fn token(&self, token: &str, bar: Option<&Bar>) -> Option<String> {
        let action = token_action(token, bar)?;
        Some(self.shown(action).into_iter().map(char::from).collect())
    }

    /// Whether `action` was pressed this frame.
    pub(crate) fn pressed(&self, action: Action) -> bool {
        self.actions
            .as_ref()
            .is_some_and(|actions| actions.pressed(action))
    }

    /// `action`'s binding as `label` draws it. It is `None` while a pad
    /// plays and binds it to no button (the journal, by default), and while
    /// the keys play and bind it to no key (action bar setup, by default).
    /// A prompt then shows no binding at all rather than an empty one or a
    /// key the player is not using.
    pub(crate) fn bound_label(&self, action: Action) -> Option<ui::vm::KeyLabel> {
        let chord = self.pads.as_ref().and_then(|pads| pads.chord(action));
        let label = self.label(action);
        let shown = label.badge.is_some() || !label.words.is_empty();
        ((!self.pad() || chord.is_some()) && shown).then_some(label)
    }

    /// The key or mouse button bound to `action`, named as a screen hears a
    /// press of it (`ui::input::UiInput::raw`, `Input::name`). An action
    /// with no key has an empty name.
    pub(crate) fn raw_key(&self, action: Action) -> String {
        let keys = self.keys.as_deref().unwrap_or(KeyMap::defaults());
        keys.input(action).map_or(String::new(), Input::name)
    }

    /// Whether a controller is in use, rather than the keyboard and mouse.
    pub(crate) fn pad(&self) -> bool {
        self.actions
            .as_ref()
            .is_some_and(|actions| matches!(actions.device, Device::Pad(_)))
    }

    /// `action`'s binding over a HUD slot: while a pad plays, its buttons
    /// drawn (`ui::icons`, compact where words would overflow); else its
    /// key, a mouse button as `M1` (`M4` the side's first): short enough
    /// to fit a slot's top on one line. An action with no key has no words.
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
        let words = keys.binding(action).map_or_else(Vec::new, |binding| {
            let input = match binding.input {
                Input::Mouse(button) => format!("M{}", mouse_number(button)),
                Input::Key(_) => ui::screens::key_mapping::key_name(&binding.input.name()),
            };
            format!("{}{input}", short_mods(binding.mods)).into_bytes()
        });
        ui::vm::KeyLabel { words, badge }
    }
}

/// The controls' tokens of `stringtokens.2da` the games' text uses, each
/// with the action it asks for here (`Prompts::token`). `MGshoot` is the
/// minigames' shoot key, which shifts a swoop's gears. Attack does that
/// here.
const CONTROL_TOKENS: [(&str, Action); 20] = [
    ("abutton", Action::Use),
    ("MGshoot", Action::Attack),
    ("ybutton", Action::SwitchWeapons),
    ("blbutton", Action::Leader),
    ("ActionUp", Action::Forward),
    ("ActionDown", Action::Backward),
    ("ActionLeft", Action::Left),
    ("ActionRight", Action::Right),
    ("MGActionLeft", Action::Left),
    ("MGActionRight", Action::Right),
    ("ChangeChar", Action::Leader),
    ("PartyActive", Action::Solo),
    ("Equip", Action::Inventory),
    ("Inventory", Action::Inventory),
    ("Map", Action::Map),
    ("Quests", Action::Journal),
    ("Quicksave", Action::QuickSave),
    ("QUICKLOAD", Action::QuickLoad),
    ("STEALTH", Action::Stealth),
    ("PersonalPowerAct", Action::Wheel),
];

/// The tokens of KOTOR's medical and mines buttons, each with what the
/// button used. A bar slot holds that now (`token_action`).
const BAR_TOKENS: [(&str, Content); 2] = [
    ("PersonalMedicalAct", Content::Item(Shelf::Medpac)),
    ("PersonalMinesAct", Content::Mine),
];

/// The action a control's token asks for (`Prompts::token`). A token of
/// `BAR_TOKENS` asks for the bar slot on `bar` that holds its content. When
/// no slot holds it, the token asks for the power wheel, which lists it.
fn token_action(token: &str, bar: Option<&Bar>) -> Option<Action> {
    let fixed = CONTROL_TOKENS.iter().find(|(name, _)| *name == token);
    if let Some((_, action)) = fixed {
        return Some(*action);
    }
    let (_, content) = BAR_TOKENS.iter().find(|(name, _)| *name == token)?;
    let slot = bar.and_then(|bar| bar.slot_of(*content));
    Some(slot.map_or(Action::Wheel, |slot| BAR[slot]))
}

/// A binding's modifiers as a HUD slot shows them: the first letter of each,
/// then a plus sign (`S+` for Shift, `CA+` for Ctrl and Alt). No modifiers
/// show as nothing.
fn short_mods(mods: Mods) -> String {
    let letters: String = mods
        .names()
        .filter_map(|name| name.chars().next())
        .collect();
    if letters.is_empty() {
        letters
    } else {
        letters + "+"
    }
}

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

/// With an item's chord held and kinds to choose among, the buttons that step
/// back and on through them (`pick_view`): the D-pad's left and right. Until
/// it is let go, a button that stepped does nothing else.
const PICK_STEPS: [(GamepadButton, i8); 2] =
    [(GamepadButton::DPadLeft, -1), (GamepadButton::DPadRight, 1)];

/// Whether `button` steps through an item's kinds (`PICK_STEPS`).
fn steps(button: GamepadButton) -> bool {
    PICK_STEPS.iter().any(|(step, _)| *step == button)
}

/// How far the stick is pushed to flick.
const FLICK: f32 = 0.6;

/// What a pad keeps while the screens have it: their hotkeys (Select
/// closes the inventory it opened), and Leave, which a watched pazaak
/// table's screen takes (`pazaak_view::watch`).
const SCREEN_ACTIONS: [Action; 10] = [
    Action::Inventory,
    Action::Equipment,
    Action::Character,
    Action::Messages,
    Action::Journal,
    Action::Map,
    Action::Party,
    Action::Abilities,
    Action::BarSetup,
    Action::LeaveTalk,
];

/// What the keys keep while the screens have them, beside the screens'
/// own actions: the bug report, and quick save and quick load, which
/// their own screens refuse where they must (`report`, `save_view`).
const SCREEN_KEYS: [Action; 3] = [Action::Report, Action::QuickSave, Action::QuickLoad];

/// What a conversation with no screen over it leaves to the keys, beside
/// the screens' actions: answering a guest's join request, a break and a
/// ready check, and following the next party member while watching.
const TALK_KEYS: [Action; 5] = [
    Action::AcceptCall,
    Action::EndCall,
    Action::Break,
    Action::Ready,
    Action::Follow,
];

/// What has the keys this frame instead of play.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Taken {
    Play,
    /// A conversation, with no screen or movie over it.
    Talk,
    /// A screen or a movie.
    Screens,
}

impl Taken {
    pub(crate) fn of((ui, replies, showing): crate::leader_view::Busy) -> Taken {
        if ui.is_some_and(|ui| ui.stack.is_open()) || showing.is_some_and(|showing| showing.0) {
            Taken::Screens
        } else if replies.is_some_and(|replies| replies.active) {
            Taken::Talk
        } else {
            Taken::Play
        }
    }
}

/// The bits of `actions` in `Actions`.
fn bits<'a>(actions: impl IntoIterator<Item = &'a Action>) -> u64 {
    actions
        .into_iter()
        .fold(0, |bits, action| bits | bit(*action))
}

/// The actions the keys keep while `taken` has them, or `None` in play,
/// where every action counts. A screen or a movie keeps `SCREEN_ACTIONS`
/// and `SCREEN_KEYS`. A conversation keeps `TALK_KEYS` too, except one
/// bound to a key the conversation reads itself (`dialogue_view::reads`:
/// a reply's number, or a key that moves through the replies or the
/// lines). The conversation's use of that key wins.
pub(crate) fn open_keys(taken: Taken, keys: &KeyMap) -> Option<u64> {
    let screens = bits(SCREEN_ACTIONS.iter().chain(&SCREEN_KEYS));
    let talk = TALK_KEYS.iter().filter(|action| {
        keys.key(**action)
            .is_none_or(|key| !crate::dialogue_view::reads(key))
    });
    match taken {
        Taken::Play => None,
        Taken::Screens => Some(screens),
        Taken::Talk => Some(screens | bits(talk)),
    }
}

/// The movement keys' bits: each action that `settings::ACTIONS` marks as
/// a movement key.
const MOVEMENT: u64 = {
    let (mut bits, mut at) = (0, 0);
    while at < crate::settings::ACTIONS.len() {
        if crate::settings::ACTIONS[at].4 {
            bits |= bit(crate::settings::ACTIONS[at].0);
        }
        at += 1;
    }
    bits
};

/// The actions held (`held`) and pressed (`down`) as play takes them.
/// While something else has the keys, only the actions in `open`
/// (`open_keys`) come through, so a number key that picks a reply uses no
/// action bar slot. Every other action held meanwhile (`kept`) then stays
/// out of play until its key is let go, as a pad's buttons do (`sample`).
/// The key that picked the last reply is often still down as play resumes.
/// The movement keys are never kept. A player who holds W as a screen
/// closes goes on moving.
pub(crate) fn gate_keys(open: Option<u64>, kept: &mut u64, held: u64, down: u64) -> (u64, u64) {
    let held_kept = held & !MOVEMENT;
    *kept = if open.is_some() {
        held_kept
    } else {
        *kept & held_kept
    };
    let open = open.unwrap_or(!*kept);
    (held & open, down & open)
}

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

/// What `sample` keeps from frame to frame: the stick's boost, whether the
/// screens had the pad, the last flick, a pick's step under way, and
/// whether each pad's stick turned the camera while the orbit's chord was
/// held, and the layer each pad's held buttons went down on (`Pressed`).
/// The last is whether the right stick is still pushed from the time the
/// power wheel was open.
type Kept<'s> = (
    Local<'s, f32>,
    Local<'s, bool>,
    Local<'s, IVec2>,
    Local<'s, bool>,
    Local<'s, BTreeMap<Entity, bool>>,
    Local<'s, Pressed>,
    Local<'s, bool>,
);

/// The layer each held button of each pad went down on. A button keeps that
/// layer until it is let go, whatever LB does meanwhile. So a player who
/// lets go of LB a moment before A still has the first bar slot held, and
/// the character does not jump. A layer of `None` means the button went
/// down on no layer: it is LB pressed while the power wheel was open, which
/// only steps the wheel and does nothing else until it is let go.
///
/// It also keeps the pads whose held LB has made a chord (`chorded`).
#[derive(Default)]
pub(crate) struct Pressed {
    layers: std::collections::HashMap<(Entity, GamepadButton), Option<Layer>>,
    /// The pads on which another button was pressed with LB held, and that
    /// chord used an action (a bar slot, Solo, Form, Switch weapons and the
    /// rest). On such a pad, the action bound to LB alone (Block by
    /// default) is not held until LB is let go. A player who readies a
    /// grenade with LB and X and keeps LB held after letting go of X so
    /// does not block, which would take the lightsaber back out. The right
    /// trigger's attack with LB held (`UNDER_BLOCK`) is no LB chord, so it
    /// keeps the block.
    chorded: std::collections::HashSet<Entity>,
}

/// The layer a press of `button` on `pad` counts on now. LB is no button of
/// its own layer, so its own press is on the first layer, where LB alone
/// may be bound (Block by default). While the power wheel is open
/// (`wheeling`), LB steps the wheel and is no layer. Every other button
/// then counts as pressed alone, and LB pressed then counts on no layer at
/// all.
fn layer_now(pad: &Gamepad, button: GamepadButton, wheeling: bool) -> Option<Layer> {
    match (wheeling, button == SHIFT) {
        (true, true) => None,
        (true, false) | (false, true) => Some(Layer::Alone),
        (false, false) => Some(Layer::of(pad)),
    }
}

impl Pressed {
    /// Records the layer of the buttons `pad` pressed this frame
    /// (`layer_now`), and whether one of them made an LB chord of
    /// `pad_map` (`chorded`).
    fn press(&mut self, entity: Entity, pad: &Gamepad, wheeling: bool, pad_map: &PadMap) {
        for button in pad.digital().get_just_pressed() {
            let layer = layer_now(pad, *button, wheeling);
            self.layers.insert((entity, *button), layer);
            if layer == Some(Layer::Lb) && pad_map.on_lb(*button) {
                self.chorded.insert(entity);
            }
        }
    }

    /// The layer `button` of `pad` went down on, while it is held or on the
    /// frame it is let go. For any other button, it is the layer a press
    /// would count on now (`layer_now`).
    fn layer(
        &self,
        entity: Entity,
        pad: &Gamepad,
        button: GamepadButton,
        wheeling: bool,
    ) -> Option<Layer> {
        self.layers
            .get(&(entity, button))
            .copied()
            .unwrap_or_else(|| layer_now(pad, button, wheeling))
    }

    /// Whether `entity`'s held LB has made a chord since it went down
    /// (`chorded`).
    fn chorded(&self, entity: Entity) -> bool {
        self.chorded.contains(&entity)
    }

    /// Forgets the buttons no pad holds any longer, and the chords of the
    /// pads that have let go of LB.
    fn release(&mut self, held: impl Fn(Entity, GamepadButton) -> bool) {
        self.layers
            .retain(|(entity, button), _| held(*entity, *button));
        self.chorded.retain(|entity| held(*entity, SHIFT));
    }
}

/// Reads this frame's `Actions` (after bevy's input, before play). The
/// screens have the pad while one of `Busy` is up (a screen, a
/// conversation, a movie).
#[allow(clippy::too_many_arguments)]
pub(crate) fn sample(
    desk: Desk,
    (key_map, pad_map, feel, headless): Bound,
    mut gamepads: Query<(Entity, &mut Gamepad, &Sticks)>,
    taken: crate::leader_view::Busy,
    time: Res<Time>,
    mut actions: ResMut<Actions>,
    mut presses: ResMut<PadPresses>,
    (mut boost, mut was_busy, mut flicked, mut stepping, mut orbited, mut pressed, mut wheeled): Kept,
    (picking, wheel): (
        Option<Res<crate::pick_view::Picking>>,
        Option<Res<crate::wheel_view::Wheel>>,
    ),
    scripted: Option<Res<ScriptedPad>>,
    mut kept_keys: Local<u64>,
) {
    if headless.0 {
        let pad = scripted.and_then(|scripted| scripted.0);
        *actions = Actions {
            device: pad.map_or(Device::Keys, Device::Pad),
            ..Actions::default()
        };
        *presses = PadPresses::default();
        return;
    }
    let key_map = key_map.as_deref().unwrap_or(KeyMap::defaults());
    let pad_map = pad_map.as_deref().unwrap_or(PadMap::defaults());
    let feel = feel.as_deref().copied().unwrap_or_else(PadFeel::default);
    let taken = Taken::of(taken);
    let menus = taken != Taken::Play;
    // The wheel's state is the frame before's: `wheel_view::steer` runs
    // after this.
    let wheeling = !menus && wheel.is_some_and(|wheel| wheel.open);
    // Let go by the screens: what is still held reads as up until released.
    // The bumpers too, because RB opens the power wheel and LB blocks, and
    // the screens use both to step through their tabs.
    if *was_busy && !menus {
        for (_, mut pad, _) in &mut gamepads {
            let held: Vec<GamepadButton> = pad.digital().get_pressed().copied().collect();
            for button in held {
                pad.digital_mut().reset(button);
            }
        }
    }
    *was_busy = menus;

    let mut next = Actions {
        device: actions.device,
        block_shared: key_map.input(Action::Block).is_some()
            && key_map.input(Action::Block) == key_map.input(Action::AltAttack),
        ..Actions::default()
    };
    // The mouse's buttons count while it holds the cursor: the click that
    // takes it (`swallow`) and one on a free pointer are the pointer's.
    let grabbed = desk
        .cursors
        .iter()
        .any(|cursor| cursor.grab_mode != bevy::window::CursorGrabMode::None);
    let mouse = desk.mouse.as_deref().filter(|_| grabbed);
    let (mut held, mut down) = (0, 0);
    for (action, on, pressed) in key_map.read(desk.keys.as_deref(), mouse) {
        held |= u64::from(on) * bit(action);
        down |= u64::from(pressed) * bit(action);
    }
    if let Some(keys) = desk.keys.as_deref() {
        // The right Shift walks and the left Ctrl crouches too.
        held |= u64::from(keys.pressed(KeyCode::ShiftRight)) * bit(Action::Walk);
        held |= u64::from(keys.pressed(KeyCode::ControlLeft)) * bit(Action::Crouch);
    }
    let open = open_keys(taken, key_map);
    (next.held, next.pressed) = gate_keys(open, &mut kept_keys, held, down);
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
    // The action sharing the orbit's chord, if one does (`TAP_ON_ORBIT`).
    let tap = pad_map
        .chord(Action::Orbit)
        .filter(|orbit| pad_map.chord(TAP_ON_ORBIT) == Some(*orbit));
    for (entity, pad, sticks) in &gamepads {
        let (left, right) = (sticks.left, sticks.right);
        let used = pad.digital().get_just_pressed().next().is_some()
            || rest(left, feel.dead) != Vec2::ZERO
            || rest(right, feel.dead) != Vec2::ZERO;
        if used {
            next.device = Device::Pad(PadKind::of(pad.vendor_id(), pad.product_id()));
        }
        let first_layer = Layer::of(pad) == Layer::Alone;
        presses.pressed.extend(
            pad.digital()
                .get_just_pressed()
                .filter(|_| first_layer)
                .copied(),
        );
        let axis = |value: f32| i32::from(value > FLICK) - i32::from(value < -FLICK);
        if pushed == IVec2::ZERO {
            pushed = IVec2::new(axis(left.x), axis(left.y));
        }
        let choosing = picking
            .as_ref()
            .filter(|_| !menus)
            .and_then(|picking| picking.choosing());
        for (button, step) in PICK_STEPS {
            if choosing.is_some() && pad.just_pressed(button) {
                next.pick_step += step;
                *stepping = true;
            }
        }
        for (button, step) in WHEEL_STEPS {
            if wheeling && pad.just_pressed(button) {
                next.wheel_step += step;
            }
        }
        pressed.press(entity, pad, wheeling, pad_map);
        let layer = |button| pressed.layer(entity, pad, button, wheeling);
        let chorded = pressed.chorded(entity);
        for (action, chord) in &pad_map.0 {
            // LB alone's action (Block by default) waits for LB to be let
            // go once LB has made a chord (`Pressed::chorded`).
            if chorded && *chord == alone(SHIFT) {
                continue;
            }
            let on = layer(chord.button);
            // The attack's first-layer chord also attacks with LB held,
            // unless an LB chord uses its button.
            let under_block = *action == UNDER_BLOCK
                && chord.layer == Layer::Alone
                && on == Some(Layer::Lb)
                && !pad_map.on_lb(chord.button);
            let live = on == Some(chord.layer) || under_block || choosing == Some(*action);
            if menus && !SCREEN_ACTIONS.contains(action) || !live {
                continue;
            }
            if *stepping && steps(chord.button) {
                continue;
            }
            // The tap on the orbit's chord fires as it is let go (below).
            let on_release = *action == TAP_ON_ORBIT && tap == Some(*chord);
            next.held |= u64::from(pad.pressed(chord.button)) * bit(*action);
            next.pressed |= u64::from(pad.just_pressed(chord.button) && !on_release) * bit(*action);
        }
        if menus {
            continue;
        }
        if let Some(orbit) = tap {
            // Each pad's own hold: whether its stick turned the camera.
            let turned = orbited.entry(entity).or_default();
            if pad.just_pressed(orbit.button) {
                *turned = false;
            }
            *turned |= pad.pressed(orbit.button) && rest(right, feel.dead) != Vec2::ZERO;
            if Some(orbit.layer) == layer(orbit.button)
                && pad.just_released(orbit.button)
                && !*turned
            {
                next.pressed |= bit(TAP_ON_ORBIT);
            }
        }
        // The D-pad's left and right step through an item's kinds instead
        // while a pick is under way (`PICK_STEPS`).
        let cycle = CYCLE
            .iter()
            .filter(|(chord, _)| !*stepping && Some(chord.layer) == layer(chord.button));
        for (chord, step) in cycle {
            next.cycle += step * i8::from(pad.just_pressed(chord.button));
        }
        let stick = rest(left, feel.dead);
        if stick.length() > next.stick.length() {
            next.stick = stick;
        }
        let right = rest(right, feel.dead);
        if right.length() > look.length() {
            look = right;
        }
    }

    pressed.release(|entity, button| {
        gamepads
            .get(entity)
            .is_ok_and(|(_, pad, _)| pad.pressed(button))
    });
    *stepping = *stepping
        && gamepads
            .iter()
            .any(|(_, pad, _)| PICK_STEPS.iter().any(|(button, _)| pad.pressed(*button)));
    presses.flick = IVec2::select(pushed.cmpne(*flicked), pushed, IVec2::ZERO);
    *flicked = pushed;

    // The right stick moved the power wheel's pointer while the wheel was
    // open. After the wheel closes, the stick turns nothing until it has
    // been back in its middle, so the camera does not jump as the player
    // lets go of the wheel's key.
    *wheeled = wheeling || (*wheeled && look != Vec2::ZERO);
    if *wheeled && !wheeling {
        look = Vec2::ZERO;
    }
    // The turn: the stick's push squared (fine near the middle), growing
    // while it is held at the edge.
    let seconds = time.delta_secs();
    *boost = if look.length() >= EDGE {
        (*boost + seconds / BOOST_SECONDS).min(1.0)
    } else {
        0.0
    };
    next.aim = look;
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
        app.add_plugins(SticksPlugin)
            .init_resource::<Actions>()
            .init_resource::<ScriptedPad>()
            .init_resource::<PadPresses>()
            .add_message::<Rumble>()
            // Bevy's, which a run without its gamepad plugin (headless)
            // lacks.
            .add_message::<GamepadRumbleRequest>()
            .add_systems(
                PreUpdate,
                sample
                    .in_set(Sampled)
                    .after(Settled)
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
pub(crate) mod probes;
