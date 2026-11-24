//! Photo mode: the camera leaves the player's character and flies free, the
//! interface hidden, until the Photo mode key (F2 unless rebound), Escape or
//! a pad's B brings it back to the shot it left.
//!
//! The camera moves by the player's own movement keys and the left stick,
//! goes up by Jump and the right trigger, down by Crouch and the left
//! trigger, and looks by the mouse and the right stick. The controls that
//! only photo mode has are fixed (`CONTROLS`).
//!
//! Played alone, the world stops while photo mode is on, as it does for a
//! menu (`sim_host`), and the Photo time control runs it at 25% or at
//! normal speed instead; a replay also offers 10% and one tick at a time.
//! In co-op photo mode calls the game's own break as it begins, when none
//! holds, and resumes it as it ends, when it called it and the break still
//! holds by this seat. The simulation never knows of photo mode. While it
//! is on, play's keys reach nothing (`take_keys`) and the player's
//! character gets the empty command (`sim_host::PlayerInput`).

use bevy::ecs::system::SystemParam;
use bevy::input::gamepad::GamepadButton;
use bevy::input::mouse::{AccumulatedMouseMotion, MouseScrollUnit, MouseWheel};
use bevy::prelude::*;
use bevy::window::{CursorGrabMode, CursorOptions, PrimaryWindow};
use sim::action::Action as SimAction;
use sim::{Seat, Snapshot};

use crate::pad::{Actions, Device};
use crate::play_view::TalkShot;
use crate::settings::{Action, KeyMap};

/// How fast the camera moves (metres a second): as it is, with the fast
/// control held, and with the slow control held.
const SPEED: f32 = 3.0;
const FAST: f32 = 12.0;
const SLOW: f32 = 0.75;
/// How long the camera takes to reach its speed and to stop (seconds).
const EASE: f32 = 0.2;
/// The narrowest and the widest the camera sees (degrees, top to bottom).
const NARROWEST: f32 = 15.0;
const WIDEST: f32 = 100.0;
/// Degrees of field of view a zoom step (a notch of the wheel) takes.
const ZOOM_STEP: f32 = 2.5;
/// Zoom steps a second while the pad's d-pad holds up or down.
const PAD_ZOOM: f32 = 8.0;
/// The most the camera tilts to either side (degrees), and how fast it
/// tilts while its control is held (degrees a second).
const ROLL_MOST: f32 = 45.0;
const TILT_SPEED: f32 = 30.0;
/// How far from the player's character's soles the camera may go (metres).
const REACH: f32 = 40.0;
/// How far past the box around the area's rooms the camera may go (metres).
const AREA_MARGIN: f32 = 2.0;
/// The corner line fades after this long without a photo mode control
/// touched (seconds), over `LINE_FADE`.
const LINE_AFTER: f32 = 3.0;
const LINE_FADE: f32 = 0.5;
/// A wheel's pixels per line (a touchpad scrolls by pixels).
const WHEEL_PIXELS: f32 = 40.0;

/// How fast the world runs in photo mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Speed {
    #[default]
    Stopped,
    /// 10% of normal speed, in a replay only.
    Tenth,
    Quarter,
    Normal,
}

impl Speed {
    /// What the world's clock is scaled by while it runs at this speed. A
    /// stopped world takes no steps at all (`PhotoMode::holds_world`), so
    /// its clock is not scaled.
    pub fn scale(self) -> f32 {
        match self {
            Speed::Tenth => 0.1,
            Speed::Quarter => 0.25,
            Speed::Stopped | Speed::Normal => 1.0,
        }
    }

    /// The speed the Photo time control moves to next. A replay also
    /// offers `Tenth`.
    pub fn next(self, replay: bool) -> Speed {
        match self {
            Speed::Stopped if replay => Speed::Tenth,
            Speed::Stopped | Speed::Tenth => Speed::Quarter,
            Speed::Quarter => Speed::Normal,
            Speed::Normal => Speed::Stopped,
        }
    }

    /// The speed as a run's commands name it (`ui photo speed …`).
    fn named(word: &str) -> Option<Speed> {
        Some(match word {
            "stopped" => Speed::Stopped,
            "tenth" => Speed::Tenth,
            "quarter" => Speed::Quarter,
            "normal" => Speed::Normal,
            _ => return None,
        })
    }

    /// The corner line's sentence for the speed, played alone.
    fn said(self) -> &'static str {
        match self {
            Speed::Stopped => "The world is stopped.",
            Speed::Tenth => "The world runs at 10% speed.",
            Speed::Quarter => "The world runs at 25% speed.",
            Speed::Normal => "The world runs at normal speed.",
        }
    }
}

/// The photo camera.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Camera {
    /// Where it stands (metres, game space).
    pub at: Vec3,
    /// How it is turned (degrees): the pitch (down above 0), the yaw and
    /// the roll, as a shot's angles are.
    pub angles: [f32; 3],
    /// How wide it sees (degrees, top to bottom).
    pub fov: f32,
    /// How fast it moves (metres a second, game space).
    pub velocity: Vec3,
}

impl Camera {
    /// A camera standing still at `shot`, its angles within half a turn
    /// either way (a shot's pitch can be a whole turn off).
    pub fn of((at, angles, fov): TalkShot) -> Camera {
        Camera {
            at: Vec3::from_array(at),
            angles: angles.map(|angle| (angle + 180.0).rem_euclid(360.0) - 180.0),
            fov,
            velocity: Vec3::ZERO,
        }
    }

    /// Its shot.
    pub fn shot(&self) -> TalkShot {
        (self.at.to_array(), self.angles, self.fov)
    }
}

/// A frame's controls of the camera.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Input {
    /// The move asked for, each from -1 to 1: forward along the view, to
    /// its right, and straight up.
    pub forward: f32,
    pub right: f32,
    pub up: f32,
    /// Degrees the view turns by: x to the right, y down (`sim_host::Look`).
    pub look: Vec2,
    pub fast: bool,
    pub slow: bool,
    /// The tilt asked for, from -1 (to the left) to 1 (to the right).
    pub tilt: f32,
    /// Zoom steps, in above 0.
    pub zoom: f32,
}

impl Input {
    fn touched(&self) -> bool {
        *self != Input::default()
    }
}

/// Where the camera may go: the box around the area's rooms (game space,
/// metres; none without rooms), and the player's character's soles.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Limits {
    pub area: Option<(Vec3, Vec3)>,
    pub soles: Vec3,
}

/// How fast the camera moves with `fast` or `slow` held (metres a second).
/// The fast control wins when both are held.
fn speed(fast: bool, slow: bool) -> f32 {
    match (fast, slow) {
        (true, _) => FAST,
        (false, true) => SLOW,
        (false, false) => SPEED,
    }
}

/// `camera` after `dt` seconds of `input`, kept within `limits`.
///
/// The camera turns by the look, slower the narrower it sees, so that a
/// zoomed camera does not swing wildly. Its pitch stops short of straight
/// up and down as the follow camera's does (`sim_host::pitch_limit`). It
/// flies along the view, and up and down. Its velocity closes on the one
/// asked for at the speed of the modifiers held divided by `EASE` each
/// second, so with the same modifiers held the camera reaches its speed and
/// stops in `EASE` seconds.
pub fn step(camera: Camera, input: &Input, limits: &Limits, dt: f32) -> Camera {
    let slowed = camera.fov / crate::play_view::FOV_DEGREES;
    let most = crate::sim_host::pitch_limit();
    let [pitch, yaw, roll] = camera.angles;
    let pitch = (pitch + input.look.y * slowed).clamp(-most, most);
    let yaw = yaw - input.look.x * slowed;
    let roll = (roll + input.tilt * TILT_SPEED * dt).clamp(-ROLL_MOST, ROLL_MOST);
    let fov = (camera.fov - input.zoom * ZOOM_STEP).clamp(NARROWEST, WIDEST);
    let speed = speed(input.fast, input.slow);
    let (forward, right) = crate::scene::fly_axes(yaw, -pitch);
    let wanted = (forward * input.forward + right * input.right + Vec3::Z * input.up)
        .clamp_length_max(1.0)
        * speed;
    let velocity = camera.velocity + (wanted - camera.velocity).clamp_length_max(speed / EASE * dt);
    Camera {
        at: clamp(camera.at + velocity * dt, limits),
        angles: [pitch, yaw, roll],
        fov,
        velocity,
    }
}

/// `at` kept within `REACH` of the soles and within the area's box grown by
/// `AREA_MARGIN`. The part of a move past either limit is taken away and
/// the rest kept, so the camera slides along the limit.
pub fn clamp(at: Vec3, limits: &Limits) -> Vec3 {
    let at = limits.soles + (at - limits.soles).clamp_length_max(REACH);
    match limits.area {
        Some((low, high)) => at.clamp(low - AREA_MARGIN, high + AREA_MARGIN),
        None => at,
    }
}

/// The controls only photo mode has.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Control {
    TiltLeft,
    TiltRight,
    ZoomIn,
    ZoomOut,
    /// The tilt and the zoom back as photo mode began.
    Reset,
    /// The player's own character hidden or shown again.
    HideOwn,
    /// The world's next speed (`Speed::next`).
    Time,
    /// One tick of a replay.
    Step,
    Fast,
    Slow,
    Up,
    Down,
    Leave,
    Screenshot,
}

/// Each control's keys and its pad button. The camera's moves, Jump, Crouch
/// and the Screenshot key are the player's own bindings, and the mouse
/// wheel zooms.
pub const CONTROLS: [(Control, &[KeyCode], Option<GamepadButton>); 14] = [
    (
        Control::TiltLeft,
        &[KeyCode::KeyQ],
        Some(GamepadButton::DPadLeft),
    ),
    (
        Control::TiltRight,
        &[KeyCode::KeyE],
        Some(GamepadButton::DPadRight),
    ),
    (Control::ZoomIn, &[], Some(GamepadButton::DPadUp)),
    (Control::ZoomOut, &[], Some(GamepadButton::DPadDown)),
    (Control::Reset, &[KeyCode::KeyR], Some(GamepadButton::North)),
    (
        Control::HideOwn,
        &[KeyCode::KeyH],
        Some(GamepadButton::West),
    ),
    (Control::Time, &[KeyCode::KeyT], Some(GamepadButton::South)),
    (
        Control::Step,
        &[KeyCode::Period],
        Some(GamepadButton::RightTrigger),
    ),
    (
        Control::Fast,
        &[KeyCode::ShiftLeft, KeyCode::ShiftRight],
        Some(GamepadButton::LeftThumb),
    ),
    (
        Control::Slow,
        &[KeyCode::ControlLeft, KeyCode::ControlRight],
        Some(GamepadButton::LeftTrigger),
    ),
    (Control::Up, &[], Some(GamepadButton::RightTrigger2)),
    (Control::Down, &[], Some(GamepadButton::LeftTrigger2)),
    (
        Control::Leave,
        &[KeyCode::Escape],
        Some(GamepadButton::East),
    ),
    (Control::Screenshot, &[], Some(GamepadButton::Start)),
];

/// The keys and the pad button of `control`.
fn bound(control: Control) -> (&'static [KeyCode], Option<GamepadButton>) {
    CONTROLS
        .iter()
        .find(|(of, ..)| *of == control)
        .map_or((&[], None), |(_, keys, button)| (*keys, *button))
}

/// The screenshot photo mode takes: the corner line is hidden for the frame
/// before the picture, then the picture is taken (`report`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum Shot {
    #[default]
    None,
    /// Asked for this frame.
    Asked,
    /// The line is hidden: the picture is taken this frame.
    Due,
}

/// Photo mode while it is on.
#[derive(Debug, Clone, Copy)]
struct Photo {
    camera: Camera,
    /// The field of view photo mode began with, which the reset puts back.
    start_fov: f32,
    speed: Speed,
    hide_own: bool,
    /// Photo mode called this seat's break as it began (co-op).
    called_break: bool,
    /// Seconds since the player last touched a photo mode control.
    idle: f32,
    /// The scene photo mode began in (`SceneSwap::generation`).
    generation: u32,
}

/// What a run's commands ask of photo mode (`ui photo …`).
#[derive(Debug, Clone, Copy, PartialEq)]
enum Ask {
    /// On, at the shot given, else at the shot the player sees.
    On(Option<TalkShot>),
    Off,
    Speed(Speed),
    HideOwn,
    /// One tick of a replay, as the step control asks.
    Step,
}

/// Photo mode's state.
#[derive(Resource, Default)]
pub struct PhotoMode {
    on: Option<Photo>,
    /// The frame's actions as the keys and pads made them, kept before play
    /// is given none while photo mode is on (`take_keys`).
    keys: Actions,
    shot: Shot,
    asks: Vec<Ask>,
    /// What a run's commands hold (`ui photo fly`): the move, and the turn
    /// in degrees a second.
    flying: Input,
}

impl PhotoMode {
    pub fn is_on(&self) -> bool {
        self.on.is_some()
    }

    /// The photo camera's shot while photo mode is on.
    pub fn shot(&self) -> Option<TalkShot> {
        self.on.map(|photo| photo.camera.shot())
    }

    /// Whether the player's own character is hidden.
    pub(crate) fn hides_own(&self) -> bool {
        self.on.is_some_and(|photo| photo.hide_own)
    }

    /// The frame's actions while photo mode is on: play has none then.
    pub(crate) fn keys(&self) -> Option<&Actions> {
        self.on.map(|_| &self.keys)
    }

    /// Whether photo mode stops a world played alone (`sim_host`).
    pub(crate) fn holds_world(&self) -> bool {
        self.on.is_some_and(|photo| photo.speed == Speed::Stopped)
    }

    /// What photo mode scales the clock of a world played alone by.
    pub(crate) fn scale(&self) -> f32 {
        self.on.map_or(1.0, |photo| photo.speed.scale())
    }

    /// Whether `report` takes the screenshot now: a screenshot `asked` for
    /// while photo mode is on waits a frame, with the corner line hidden.
    pub(crate) fn shoots(&mut self, asked: bool) -> bool {
        if self.on.is_none() {
            return asked;
        }
        if self.shot == Shot::Due {
            self.shot = Shot::None;
            return true;
        }
        if asked {
            self.shot = Shot::Asked;
        }
        false
    }

    /// A run's request (`ui photo …`): `on`, `on X Y Z PITCH YAW [FOV]`
    /// (metres in game space, degrees), `off`, `speed
    /// stopped|tenth|quarter|normal`, `hide` (the own character hidden or
    /// shown), `step` (one tick of a stopped replay), and `fly FORWARD RIGHT UP [TURN_RIGHT TURN_DOWN]` (the move
    /// held, each from -1 to 1, and the turn in degrees a second; `fly 0 0
    /// 0` lets go). False for anything else.
    pub(crate) fn script(&mut self, words: &[&str]) -> bool {
        let numbers: Option<Vec<f32>> = words
            .iter()
            .skip(1)
            .map(|word| word.parse::<f32>().ok().filter(|n| n.is_finite()))
            .collect();
        let ask = match (words, numbers.as_deref()) {
            (["on"], _) => Ask::On(None),
            (["on", ..], Some(&[x, y, z, pitch, yaw])) => Ask::On(Some((
                [x, y, z],
                [pitch, yaw, 0.0],
                crate::play_view::FOV_DEGREES,
            ))),
            (["on", ..], Some(&[x, y, z, pitch, yaw, fov])) => {
                Ask::On(Some(([x, y, z], [pitch, yaw, 0.0], fov)))
            }
            (["off"], _) => Ask::Off,
            (["hide"], _) => Ask::HideOwn,
            (["step"], _) => Ask::Step,
            (["speed", word], _) => match Speed::named(word) {
                Some(speed) => Ask::Speed(speed),
                None => return false,
            },
            (["fly", ..], Some(numbers)) if matches!(numbers.len(), 3 | 5) => {
                let turn = numbers.get(3..5).map_or(Vec2::ZERO, Vec2::from_slice);
                self.flying = Input {
                    forward: numbers[0],
                    right: numbers[1],
                    up: numbers[2],
                    look: turn,
                    ..Input::default()
                };
                return true;
            }
            _ => return false,
        };
        self.asks.push(ask);
        true
    }
}

/// The calls photo mode makes as it begins in co-op: a break, when none
/// holds. In combat the simulation refuses it and tells the player so.
pub(crate) fn break_on_enter(snapshot: &Snapshot) -> Option<SimAction> {
    snapshot.paused_by.is_none().then_some(SimAction::Break)
}

/// The call photo mode makes as it ends in co-op: a resume of the break it
/// `called`, while that break still holds by `seat`.
pub(crate) fn break_on_leave(called: bool, snapshot: &Snapshot, seat: Seat) -> Option<SimAction> {
    (called && snapshot.paused_by == Some(seat)).then_some(SimAction::Resume)
}

/// Play's actions taken away while photo mode is on, and kept for photo
/// mode itself: play is left only the device.
pub(crate) fn take_keys(mut actions: ResMut<Actions>, photo: Option<ResMut<PhotoMode>>) {
    let Some(mut photo) = photo else {
        return;
    };
    photo.keys = *actions;
    if photo.is_on() {
        *actions = blank(&actions);
    }
}

/// `actions` with nothing held or pressed, on the same device.
pub(crate) fn blank(actions: &Actions) -> Actions {
    let mut blank = Actions::default();
    blank.device = actions.device;
    blank
}

/// Whether photo mode is off: the run condition of what reads play's keys
/// outside `Actions`.
pub fn off(photo: Option<Res<PhotoMode>>) -> bool {
    photo.is_none_or(|photo| !photo.is_on())
}

/// The corner line (`ui_view` draws it): its words, and how much of it
/// shows, from 0 to 1.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Line {
    pub(crate) words: Vec<u8>,
    pub(crate) alpha: f32,
}

/// One replay tick asked for by the step control, and whether a replay
/// plays (`sim_host` says so each frame).
#[derive(Resource, Default)]
pub(crate) struct Replay {
    pub(crate) playing: bool,
    pub(crate) step: bool,
}

/// The input photo mode reads.
#[derive(SystemParam)]
pub(crate) struct Controls<'w, 's> {
    keys: Option<Res<'w, ButtonInput<KeyCode>>>,
    map: Option<Res<'w, KeyMap>>,
    wheel: MessageReader<'w, 's, MouseWheel>,
    motion: Option<Res<'w, AccumulatedMouseMotion>>,
    feel: Option<Res<'w, crate::settings::MouseFeel>>,
    cursors: Query<'w, 's, &'static CursorOptions, With<PrimaryWindow>>,
    pads: crate::pad::Pads<'w, 's>,
    time: Res<'w, Time<Real>>,
}

impl Controls<'_, '_> {
    fn held(&self, control: Control) -> bool {
        let (keys, button) = bound(control);
        self.keys
            .as_ref()
            .is_some_and(|held| held.any_pressed(keys.iter().copied()))
            || button.is_some_and(|button| self.pads.iter().any(|(pad, _)| pad.pressed(button)))
    }

    fn pressed(&self, control: Control) -> bool {
        let (keys, button) = bound(control);
        self.keys
            .as_ref()
            .is_some_and(|held| held.any_just_pressed(keys.iter().copied()))
            || button
                .is_some_and(|button| self.pads.iter().any(|(pad, _)| pad.just_pressed(button)))
    }

    /// The frame's camera input: the movement keys as the player bound
    /// them, the stick, the mouse while it holds the cursor, the wheel and
    /// the fixed controls (`CONTROLS`). `keys` are the frame's actions.
    fn input(&mut self, keys: &Actions) -> Input {
        let map = self.map.as_deref().unwrap_or(KeyMap::defaults());
        let read = map.read(self.keys.as_deref(), None);
        let bound_held = |action: Action| read.iter().any(|(of, on, _)| *of == action && *on);
        let axis = |plus: bool, minus: bool| f32::from(i8::from(plus) - i8::from(minus));
        let either = |key: f32, other: f32| if key != 0.0 { key } else { other };
        let wheel: f32 = self
            .wheel
            .read()
            .map(|event| match event.unit {
                MouseScrollUnit::Line => event.y,
                MouseScrollUnit::Pixel => event.y / WHEEL_PIXELS,
            })
            .sum();
        let grabbed = self
            .cursors
            .iter()
            .any(|cursor| cursor.grab_mode != CursorGrabMode::None);
        let feel = self.feel.as_deref().copied().unwrap_or_default();
        let mouse = self
            .motion
            .as_ref()
            .filter(|_| grabbed)
            .map_or(Vec2::ZERO, |motion| {
                let up = if feel.invert { -1.0 } else { 1.0 };
                Vec2::new(motion.delta.x, up * motion.delta.y)
                    * crate::sim_host::MOUSE_DEGREES
                    * feel.scale
            });
        let dt = self.time.delta_secs();
        Input {
            forward: either(
                axis(bound_held(Action::Forward), bound_held(Action::Backward)),
                keys.stick.y,
            ),
            right: either(
                axis(bound_held(Action::Right), bound_held(Action::Left)),
                keys.stick.x,
            ),
            up: axis(
                bound_held(Action::Jump) || self.held(Control::Up),
                bound_held(Action::Crouch) || self.held(Control::Down),
            ),
            look: mouse + keys.turn,
            fast: self.held(Control::Fast),
            slow: self.held(Control::Slow),
            tilt: axis(self.held(Control::TiltRight), self.held(Control::TiltLeft)),
            zoom: wheel
                + axis(self.held(Control::ZoomIn), self.held(Control::ZoomOut)) * PAD_ZOOM * dt,
        }
    }
}

/// What photo mode reads of play.
#[derive(SystemParam)]
pub(crate) struct Play<'w> {
    presented: Option<Res<'w, crate::sim_host::PresentedSnapshot>>,
    own: Option<Res<'w, crate::sim_host::OwnSeat>>,
    swap: Option<Res<'w, crate::sim_host::SceneSwap>>,
    framed: Option<Res<'w, crate::play_view::ShotTaken>>,
    world_tick: Option<Res<'w, console::WorldTick>>,
    screens: Option<Res<'w, crate::ui_view::UiOpen>>,
    showing: Option<Res<'w, crate::movie_view::Showing>>,
    loading: Option<Res<'w, crate::sim_host::Loading>>,
    picture: Option<Res<'w, crate::lifecycle_view::Picture>>,
    culling: Option<Res<'w, crate::scene::Culling>>,
    scene: Option<Res<'w, crate::scene::SceneContent>>,
}

impl Play<'_> {
    /// Whether nothing of the world can be filmed: no world, a screen open
    /// (a menu, the map), a movie up or an area loading.
    fn blocked(&self) -> bool {
        let loading = self.loading.as_ref().is_some_and(|loading| {
            loading.active
                || self
                    .picture
                    .as_ref()
                    .is_some_and(|picture| picture.holds(loading.committed))
        });
        self.presented.is_none()
            || self.screens.as_ref().is_some_and(|open| open.0)
            || self.showing.as_ref().is_some_and(|showing| showing.0)
            || loading
    }

    /// Whether the world is played alone (no co-op): only then does it
    /// stop and change speed for photo mode.
    fn alone(&self) -> bool {
        self.world_tick
            .as_ref()
            .is_some_and(|tick| tick.0.is_some())
    }

    fn generation(&self) -> u32 {
        self.swap.as_ref().map_or(0, |swap| swap.generation)
    }

    fn limits(&self) -> Limits {
        let area = self
            .culling
            .as_ref()
            .zip(self.scene.as_ref())
            .and_then(|(culling, scene)| culling.area(&scene.0.rooms));
        Limits {
            area,
            soles: self
                .framed
                .as_ref()
                .map_or(Vec3::ZERO, |framed| framed.soles),
        }
    }
}

/// The corner line's words: "Photo mode", the world's speed (whether the
/// break holds, in co-op), and the controls that leave and take a
/// screenshot, as the device in use names them.
fn line_words(speed: Speed, alone: Option<bool>, device: Device, map: &KeyMap) -> Vec<u8> {
    let state = match alone {
        None => speed.said(),
        Some(true) => "Paused for everyone.",
        Some(false) => "The game keeps running.",
    };
    let (leave, shot) = match device {
        Device::Pad(kind) => (
            kind.name(GamepadButton::East).to_string(),
            kind.name(GamepadButton::Start).to_string(),
        ),
        Device::Keys => (
            "Escape".to_string(),
            String::from_utf8_lossy(&map.shown(Action::Screenshot)).into_owned(),
        ),
    };
    format!("Photo mode. {state} Press {leave} to leave and {shot} to take a screenshot.")
        .into_bytes()
}

/// Photo mode turned on and off, and its camera flown, each frame. In
/// co-op it calls and resumes the break (`party_calls::Asked`). It writes
/// the corner line for the screens (`Ui::photo`), which hide the rest of
/// the interface while photo mode is on.
pub(crate) fn photo_keys(
    mut controls: Controls,
    play: Play,
    (mut photo, mut ui, mut asked, mut replay, mut facts): (
        ResMut<PhotoMode>,
        ResMut<crate::ui_view::Ui>,
        ResMut<crate::party_calls::Asked>,
        ResMut<Replay>,
        ResMut<console::DumpFacts>,
    ),
) {
    let photo = &mut *photo;
    let keys = photo.keys;
    let asks = std::mem::take(&mut photo.asks);
    let toggled = keys.pressed(Action::PhotoMode);
    let snapshot = play
        .presented
        .as_ref()
        .map(|presented| presented.0.current());
    let seat = play.own.as_ref().map_or(Seat(0), |own| own.0);
    let generation = play.generation();
    let alone = play.alone();
    match photo.on {
        None => {
            let asked_on = asks.iter().find_map(|ask| match ask {
                Ask::On(at) => Some(*at),
                _ => None,
            });
            let framed = play.framed.as_ref().and_then(|framed| framed.shot);
            let at = match asked_on {
                Some(Some(at)) => Some(at),
                Some(None) => framed,
                None => framed.filter(|_| toggled),
            };
            if let (Some(at), false) = (at, play.blocked()) {
                let call = snapshot.filter(|_| !alone).and_then(break_on_enter);
                let called_break = call.is_some();
                asked.0.extend(call);
                photo.on = Some(Photo {
                    camera: Camera::of(at),
                    start_fov: at.2,
                    speed: Speed::Stopped,
                    hide_own: false,
                    called_break,
                    idle: 0.0,
                    generation,
                });
            }
        }
        Some(on) => {
            let leaving = toggled
                || controls.pressed(Control::Leave)
                || asks.contains(&Ask::Off)
                || play.blocked()
                || on.generation != generation;
            if leaving {
                if let Some(snapshot) = snapshot.filter(|_| !alone) {
                    asked
                        .0
                        .extend(break_on_leave(on.called_break, snapshot, seat));
                }
                photo.on = None;
                photo.shot = Shot::None;
                photo.flying = Input::default();
            }
        }
    }
    let limits = play.limits();
    if facts.wanted() {
        facts.0.insert(
            "photo".into(),
            serde_json::json!({
                "on": photo.on.is_some(),
                "shot": photo.shot(),
                "speed": photo.on.map(|on| format!("{:?}", on.speed)),
                "hide_own": photo.hides_own(),
                "called_break": photo.on.is_some_and(|on| on.called_break),
                "soles": limits.soles.to_array(),
                "area": limits.area.map(|(low, high)| [low.to_array(), high.to_array()]),
            }),
        );
    }
    let Some(on) = photo.on.as_mut() else {
        if ui.photo.is_some() {
            ui.photo = None;
        }
        return;
    };
    let dt = controls.time.delta_secs();
    let mut input = controls.input(&keys);
    let flown = photo.flying;
    input.forward += flown.forward;
    input.right += flown.right;
    input.up += flown.up;
    input.look += flown.look * dt;
    let pressed = |control| controls.pressed(control);
    let mut touched = input.touched();
    for ask in &asks {
        match ask {
            Ask::Speed(speed) if alone => on.speed = *speed,
            Ask::HideOwn => on.hide_own = !on.hide_own,
            _ => {}
        }
    }
    if pressed(Control::Time) && alone {
        on.speed = on.speed.next(replay.playing);
    }
    let stepped = pressed(Control::Step) || asks.contains(&Ask::Step);
    if stepped && replay.playing && on.speed == Speed::Stopped {
        replay.step = true;
    }
    if pressed(Control::Reset) {
        on.camera.angles[2] = 0.0;
        on.camera.fov = on.start_fov;
    }
    if pressed(Control::HideOwn) {
        on.hide_own = !on.hide_own;
    }
    if pressed(Control::Screenshot) {
        photo.shot = Shot::Asked;
    }
    touched |= CONTROLS.iter().any(|(control, ..)| pressed(*control));
    on.camera = step(on.camera, &input, &limits, dt);
    on.idle = if touched { 0.0 } else { on.idle + dt };
    // The frame after a screenshot is asked for draws no line, and the
    // picture is taken of it.
    if photo.shot == Shot::Asked {
        photo.shot = Shot::Due;
    }
    let paused = snapshot.is_some_and(|snapshot| snapshot.paused_by.is_some());
    let map = controls.map.as_deref().unwrap_or(KeyMap::defaults());
    let fade = ((on.idle - LINE_AFTER) / LINE_FADE).clamp(0.0, 1.0);
    ui.photo = Some(Line {
        words: line_words(on.speed, (!alone).then_some(paused), keys.device, map),
        alpha: if photo.shot == Shot::None {
            1.0 - fade
        } else {
            0.0
        },
    });
}

/// The player's own character hidden while photo mode asks (H), with what
/// hangs on its body, and shown again after.
pub(crate) fn hide_own(
    photo: Res<PhotoMode>,
    framed: Res<crate::play_view::ShotTaken>,
    roots: Res<crate::cast::BodyRoots>,
    mut shown: Query<&mut Visibility>,
    mut hidden: Local<Option<Entity>>,
) {
    let wanted = framed
        .me
        .filter(|_| photo.hides_own())
        .and_then(|me| roots.0.get(&me).copied());
    if *hidden != wanted
        && let Some(was) = hidden.take()
        && let Ok(mut visibility) = shown.get_mut(was)
    {
        visibility.set_if_neq(Visibility::Inherited);
    }
    if let Some(root) = wanted
        && let Ok(mut visibility) = shown.get_mut(root)
    {
        visibility.set_if_neq(Visibility::Hidden);
    }
    *hidden = wanted;
}

/// Where photo mode reads its controls: after the screens took theirs, and
/// before the screenshot key is read.
#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) struct PhotoKeys;

pub struct PhotoModePlugin;

impl Plugin for PhotoModePlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<PhotoMode>()
            .init_resource::<Replay>()
            .add_systems(
                Update,
                photo_keys
                    .in_set(PhotoKeys)
                    .after(crate::ui_view::Stepped)
                    .run_if(resource_exists::<crate::ui_view::Ui>),
            )
            .add_systems(
                PostUpdate,
                hide_own
                    .run_if(resource_exists::<crate::play_view::ShotTaken>)
                    .run_if(resource_exists::<crate::cast::BodyRoots>)
                    .after(crate::play_view::Drawn)
                    .before(bevy::camera::visibility::VisibilitySystems::VisibilityPropagate),
            );
    }
}

#[cfg(feature = "probes")]
#[path = "photo_mode_probes.rs"]
mod probes;

#[cfg(feature = "probes")]
pub use probes::{camera_probes, probes};
