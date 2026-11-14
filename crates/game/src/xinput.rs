//! One controller for this copy of the game, read straight from XInput.
//!
//! Split screen tools such as Nucleus Co-op start several copies of the
//! game side by side, one per player. Each copy is started with `--pad N`
//! (or `KJK_PAD=N`), where N is the controller's XInput number from 1 to 4.
//! That copy then reads that one controller and no other.
//!
//! The game's usual controller backend (gilrs, through Windows.Gaming.Input)
//! only sends input to the window that has focus, so in split screen every
//! window but one would ignore its player. XInput sends input to every
//! program, focused or not. This plugin takes the place of Bevy's gilrs
//! plugin and sends the same input events, so the rest of the game reads
//! the controller as usual.
//!
//! Without `--pad` none of this runs, and the game reads every controller
//! through gilrs as before.

use bevy::input::InputSystems;
use bevy::input::gamepad::{
    GamepadAxis, GamepadButton, GamepadConnection, GamepadConnectionEvent, GamepadRumbleRequest,
    RawGamepadAxisChangedEvent, RawGamepadButtonChangedEvent, RawGamepadEvent,
};
use bevy::prelude::*;
use std::time::{Duration, Instant};

/// The environment variable `--pad` sets: the controller's XInput number,
/// from 1 to 4. A variable, so a copy that restarts into the menu keeps it.
pub const PAD: &str = "KJK_PAD";

/// How often an empty slot is asked whether a controller is there. XInput
/// is slow to answer for an empty slot, so it is not asked every frame.
const LOOK_AGAIN: Duration = Duration::from_secs(1);

/// The XInput slot (0 to 3) named by a `KJK_PAD` value (1 to 4).
fn slot_of(text: &str) -> Option<u32> {
    text.trim()
        .parse::<u32>()
        .ok()
        .filter(|number| (1..=4).contains(number))
        .map(|number| number - 1)
}

/// The plugin for the controller `KJK_PAD` names, if it names one and this
/// is Windows. On other systems the setting is ignored with a warning.
pub fn chosen() -> Option<SeatPlugin> {
    let text = std::env::var(PAD).ok()?;
    let Some(slot) = slot_of(&text) else {
        warn!(target: "pad", value = text, "{PAD} must be a number from 1 to 4; reading every controller");
        return None;
    };
    #[cfg(windows)]
    {
        info!(target: "pad", controller = slot + 1, "reading one XInput controller");
        Some(SeatPlugin {
            slot,
            device: Device::XINPUT,
        })
    }
    #[cfg(not(windows))]
    {
        warn!(target: "pad", controller = slot + 1, "{PAD} works on Windows only; reading every controller");
        None
    }
}

/// One XInput reading of a controller.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Reading {
    /// XInput's button bits (`XINPUT_GAMEPAD_*`).
    pub buttons: u16,
    pub left_trigger: u8,
    pub right_trigger: u8,
    pub left_stick: [i16; 2],
    pub right_stick: [i16; 2],
}

/// XInput's button bits and Bevy's names for them. Bevy calls the bumpers
/// triggers, and the triggers `LeftTrigger2` and `RightTrigger2`.
const BUTTONS: [(u16, GamepadButton); 14] = [
    (0x0001, GamepadButton::DPadUp),
    (0x0002, GamepadButton::DPadDown),
    (0x0004, GamepadButton::DPadLeft),
    (0x0008, GamepadButton::DPadRight),
    (0x0010, GamepadButton::Start),
    (0x0020, GamepadButton::Select),
    (0x0040, GamepadButton::LeftThumb),
    (0x0080, GamepadButton::RightThumb),
    (0x0100, GamepadButton::LeftTrigger),
    (0x0200, GamepadButton::RightTrigger),
    (0x1000, GamepadButton::South),
    (0x2000, GamepadButton::East),
    (0x4000, GamepadButton::West),
    (0x8000, GamepadButton::North),
];

/// A change between two readings, as the input event that reports it.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Change {
    Button(GamepadButton, f32),
    Axis(GamepadAxis, f32),
}

impl Reading {
    /// Every button with its value from 0 to 1, the triggers included.
    fn buttons(&self) -> impl Iterator<Item = (GamepadButton, f32)> + '_ {
        let trigger = |value: u8| f32::from(value) / 255.0;
        BUTTONS
            .iter()
            .map(|&(bit, button)| (button, if self.buttons & bit == 0 { 0.0 } else { 1.0 }))
            .chain([
                (GamepadButton::LeftTrigger2, trigger(self.left_trigger)),
                (GamepadButton::RightTrigger2, trigger(self.right_trigger)),
            ])
    }

    /// Both sticks' axes from -1 to 1, with up as positive (as XInput has it).
    fn axes(&self) -> [(GamepadAxis, f32); 4] {
        let stick = |value: i16| (f32::from(value) / 32767.0).max(-1.0);
        [
            (GamepadAxis::LeftStickX, stick(self.left_stick[0])),
            (GamepadAxis::LeftStickY, stick(self.left_stick[1])),
            (GamepadAxis::RightStickX, stick(self.right_stick[0])),
            (GamepadAxis::RightStickY, stick(self.right_stick[1])),
        ]
    }

    /// What changed since `was`; everything when there was no reading.
    fn changes(&self, was: Option<&Reading>) -> Vec<Change> {
        let old_buttons: Vec<f32> = was.map_or_else(Vec::new, |was| {
            was.buttons().map(|(_, value)| value).collect()
        });
        let old_axes = was.map(Reading::axes);
        let buttons = self
            .buttons()
            .enumerate()
            .filter(|(index, (_, value))| old_buttons.get(*index) != Some(value))
            .map(|(_, (button, value))| Change::Button(button, value));
        let axes = self
            .axes()
            .into_iter()
            .enumerate()
            .filter(|(index, (_, value))| old_axes.map(|old| old[*index].1) != Some(*value))
            .map(|(_, (axis, value))| Change::Axis(axis, value));
        buttons.chain(axes).collect()
    }
}

/// Where readings come from and where rumble goes: XInput on Windows, a
/// stand-in in tests.
#[derive(Clone, Copy)]
pub struct Device {
    /// The slot's reading, or `None` when no controller is there.
    pub read: fn(u32) -> Option<Reading>,
    /// Sets the slot's strong (left) and weak (right) motor speeds.
    pub shake: fn(u32, u16, u16),
}

#[cfg(windows)]
impl Device {
    pub const XINPUT: Device = Device {
        read: windows::read,
        shake: windows::shake,
    };
}

/// Reads one XInput slot in place of Bevy's gilrs plugin (which the caller
/// turns off).
pub struct SeatPlugin {
    pub slot: u32,
    pub device: Device,
}

impl Plugin for SeatPlugin {
    fn build(&self, app: &mut App) {
        let pad = app.world_mut().spawn(Name::new("XInput controller")).id();
        app.insert_resource(Seat {
            slot: self.slot,
            device: self.device,
            pad,
            last: None,
            look_after: None,
            rumbles: Vec::new(),
            motors: (0, 0),
        })
        .add_systems(PreUpdate, poll.before(InputSystems))
        .add_systems(PostUpdate, rumble);
    }
}

/// The one controller this copy reads. Present only in a split screen copy.
#[derive(Resource)]
pub(crate) struct Seat {
    slot: u32,
    device: Device,
    /// The entity Bevy keeps the controller's state on, kept across
    /// disconnects as gilrs does.
    pad: Entity,
    /// The last reading, or `None` while no controller is connected.
    last: Option<Reading>,
    /// While no controller is connected, when to ask the slot again.
    look_after: Option<Instant>,
    /// Running rumbles: when each ends, and its strong and weak strength.
    rumbles: Vec<(Instant, f32, f32)>,
    /// The motor speeds last sent.
    motors: (u16, u16),
}

/// Reads the slot and sends the input events gilrs would have sent.
fn poll(
    mut seat: ResMut<Seat>,
    mut raw: MessageWriter<RawGamepadEvent>,
    mut connections: MessageWriter<GamepadConnectionEvent>,
    mut buttons: MessageWriter<RawGamepadButtonChangedEvent>,
    mut axes: MessageWriter<RawGamepadAxisChangedEvent>,
) {
    let now = Instant::now();
    if seat.last.is_none() && seat.look_after.is_some_and(|after| now < after) {
        return;
    }
    let pad = seat.pad;
    let mut connection = |connection| {
        let event = GamepadConnectionEvent::new(pad, connection);
        raw.write(event.clone().into());
        connections.write(event);
    };
    let Some(reading) = (seat.device.read)(seat.slot) else {
        if seat.last.take().is_some() {
            info!(target: "pad", controller = seat.slot + 1, "XInput controller disconnected");
            connection(GamepadConnection::Disconnected);
        }
        seat.look_after = Some(now + LOOK_AGAIN);
        return;
    };
    if seat.last.is_none() {
        info!(target: "pad", controller = seat.slot + 1, "XInput controller connected");
        connection(GamepadConnection::Connected {
            name: format!("XInput controller {}", seat.slot + 1),
            vendor_id: None,
            product_id: None,
        });
    }
    for change in reading.changes(seat.last.as_ref()) {
        match change {
            Change::Button(button, value) => {
                let event = RawGamepadButtonChangedEvent::new(pad, button, value);
                raw.write(event.into());
                buttons.write(event);
            }
            Change::Axis(axis, value) => {
                let event = RawGamepadAxisChangedEvent::new(pad, axis, value);
                raw.write(event.into());
                axes.write(event);
            }
        }
    }
    seat.last = Some(reading);
}

/// Plays the rumble the game asks of this controller. Rumbles running at
/// once add up, as Bevy's own rumble does.
fn rumble(mut seat: ResMut<Seat>, mut asked: MessageReader<GamepadRumbleRequest>) {
    let now = Instant::now();
    let pad = seat.pad;
    for request in asked.read() {
        match *request {
            GamepadRumbleRequest::Add {
                duration,
                intensity,
                gamepad,
            } if gamepad == pad => {
                seat.rumbles
                    .push((now + duration, intensity.strong_motor, intensity.weak_motor));
            }
            GamepadRumbleRequest::Stop { gamepad } if gamepad == pad => seat.rumbles.clear(),
            _ => {}
        }
    }
    seat.rumbles.retain(|(end, ..)| *end > now);
    let motors = motor_speeds(&seat.rumbles);
    if motors != seat.motors && seat.last.is_some() {
        (seat.device.shake)(seat.slot, motors.0, motors.1);
        seat.motors = motors;
    }
}

/// The summed strengths of the running rumbles, as XInput motor speeds.
fn motor_speeds(rumbles: &[(Instant, f32, f32)]) -> (u16, u16) {
    let speed = |sum: f32| (sum.clamp(0.0, 1.0) * f32::from(u16::MAX)).round() as u16;
    let (strong, weak) = rumbles
        .iter()
        .fold((0.0, 0.0), |(strong, weak), (_, s, w)| {
            (strong + s, weak + w)
        });
    (speed(strong), speed(weak))
}

#[cfg(windows)]
mod windows {
    use super::Reading;

    #[repr(C)]
    #[derive(Default)]
    struct XInputGamepad {
        buttons: u16,
        left_trigger: u8,
        right_trigger: u8,
        thumb_lx: i16,
        thumb_ly: i16,
        thumb_rx: i16,
        thumb_ry: i16,
    }

    #[repr(C)]
    #[derive(Default)]
    struct XInputState {
        packet_number: u32,
        gamepad: XInputGamepad,
    }

    #[repr(C)]
    struct XInputVibration {
        left_motor_speed: u16,
        right_motor_speed: u16,
    }

    // XInput 1.4 comes with Windows 8 and later. `raw-dylib` links it
    // without an import library.
    #[link(name = "xinput1_4", kind = "raw-dylib")]
    unsafe extern "system" {
        fn XInputGetState(user_index: u32, state: *mut XInputState) -> u32;
        fn XInputSetState(user_index: u32, vibration: *mut XInputVibration) -> u32;
    }

    const ERROR_SUCCESS: u32 = 0;

    pub(super) fn read(slot: u32) -> Option<Reading> {
        let mut state = XInputState::default();
        // SAFETY: `state` is a valid, writable XINPUT_STATE.
        let result = unsafe { XInputGetState(slot, &mut state) };
        (result == ERROR_SUCCESS).then(|| {
            let pad = state.gamepad;
            Reading {
                buttons: pad.buttons,
                left_trigger: pad.left_trigger,
                right_trigger: pad.right_trigger,
                left_stick: [pad.thumb_lx, pad.thumb_ly],
                right_stick: [pad.thumb_rx, pad.thumb_ry],
            }
        })
    }

    pub(super) fn shake(slot: u32, strong: u16, weak: u16) {
        let mut vibration = XInputVibration {
            left_motor_speed: strong,
            right_motor_speed: weak,
        };
        // SAFETY: `vibration` is a valid XINPUT_VIBRATION. A missing
        // controller makes the call fail, which is harmless.
        unsafe { XInputSetState(slot, &mut vibration) };
    }
}
