//! Keys, the mouse and gamepads turned into the screens' moves
//! (`ui::input`): arrows and Tab move, Enter activates, Escape goes back,
//! Page Up and Down scroll, typed text goes to edit boxes; a gamepad's
//! d-pad or left stick moves (repeating while held), A activates, B goes
//! back, the shoulders switch tabs, the right stick scrolls.

use bevy::input::ButtonState;
use bevy::input::gamepad::{Gamepad, GamepadButton};
use bevy::input::keyboard::KeyboardInput;
use bevy::input::mouse::{MouseScrollUnit, MouseWheel};
use bevy::prelude::*;
use ui::input::{Key, UiInput};

/// A stick past this counts as a press.
const STICK: f32 = 0.5;
/// A held direction repeats after this long, then this often (seconds).
const FIRST_REPEAT: f32 = 0.35;
const REPEAT: f32 = 0.12;

/// A held stick's direction and when it fires next.
#[derive(Default)]
pub(crate) struct Repeat {
    key: Option<Key>,
    next: f32,
}

impl Repeat {
    /// `key` while held: once at once, then again after a pause, then
    /// steadily.
    fn hold(&mut self, key: Option<Key>, dt: f32, out: &mut Vec<Key>) {
        match key {
            Some(key) if self.key == Some(key) => {
                self.next -= dt;
                if self.next <= 0.0 {
                    out.push(key);
                    self.next = REPEAT;
                }
            }
            Some(key) => {
                out.push(key);
                *self = Repeat {
                    key: Some(key),
                    next: FIRST_REPEAT,
                };
            }
            None => *self = Repeat::default(),
        }
    }
}

/// Both sticks' repeats.
#[derive(Default)]
pub(crate) struct Held {
    moves: Repeat,
    scroll: Repeat,
}

fn key_of(code: KeyCode, shift: bool) -> Option<Key> {
    Some(match code {
        KeyCode::ArrowUp => Key::Up,
        KeyCode::ArrowDown => Key::Down,
        KeyCode::ArrowLeft => Key::Left,
        KeyCode::ArrowRight => Key::Right,
        KeyCode::Tab if shift => Key::Up,
        KeyCode::Tab => Key::Down,
        KeyCode::Enter | KeyCode::NumpadEnter => Key::Activate,
        KeyCode::Escape => Key::Back,
        KeyCode::PageUp => Key::ScrollUp,
        KeyCode::PageDown => Key::ScrollDown,
        KeyCode::Backspace => Key::Backspace,
        _ => return None,
    })
}

fn stick_key(stick: Vec2) -> Option<Key> {
    if stick.length() < STICK {
        return None;
    }
    Some(if stick.x.abs() > stick.y.abs() {
        if stick.x > 0.0 { Key::Right } else { Key::Left }
    } else if stick.y > 0.0 {
        Key::Up
    } else {
        Key::Down
    })
}

/// This frame's input for the screens.
#[allow(clippy::too_many_arguments)]
pub(crate) fn gather(
    keys: Option<&ButtonInput<KeyCode>>,
    mouse: Option<&ButtonInput<MouseButton>>,
    typed: &mut MessageReader<KeyboardInput>,
    wheel: &mut MessageReader<MouseWheel>,
    gamepads: &Query<&Gamepad>,
    window: Option<&Window>,
    window_size: Vec2,
    dt: f32,
    held: &mut Held,
    last_pointer: &mut Option<Vec2>,
) -> UiInput {
    let shift =
        keys.is_some_and(|keys| keys.any_pressed([KeyCode::ShiftLeft, KeyCode::ShiftRight]));
    // Alt+Enter is the window's (`display`), not the screens'.
    let alt = keys.is_some_and(|keys| keys.any_pressed([KeyCode::AltLeft, KeyCode::AltRight]));
    let mut input = UiInput {
        window: (window_size.x, window_size.y),
        dt,
        ..UiInput::default()
    };
    for event in typed.read() {
        if event.state != ButtonState::Pressed {
            continue;
        }
        if !event.repeat {
            input.raw.push(format!("{:?}", event.key_code));
        }
        if alt && matches!(event.key_code, KeyCode::Enter | KeyCode::NumpadEnter) {
            continue;
        }
        if let Some(key) = key_of(event.key_code, shift) {
            input.keys.push(key);
            continue;
        }
        if let Some(text) = &event.text {
            input
                .text
                .extend(text.chars().filter(|character| !character.is_control()));
        }
    }
    for event in wheel.read() {
        input.wheel += match event.unit {
            MouseScrollUnit::Line => event.y,
            MouseScrollUnit::Pixel => event.y / 40.0,
        };
    }
    let pointer = window.and_then(Window::cursor_position);
    input.moved = pointer.is_some() && pointer != *last_pointer;
    *last_pointer = pointer;
    input.pointer = pointer.map(|at| [at.x, at.y]);
    input.click = mouse.is_some_and(|mouse| mouse.just_pressed(MouseButton::Left));

    let (mut stick, mut scroll) = (Vec2::ZERO, 0.0f32);
    for gamepad in gamepads {
        for (button, key) in [
            (GamepadButton::DPadUp, Key::Up),
            (GamepadButton::DPadDown, Key::Down),
            (GamepadButton::DPadLeft, Key::Left),
            (GamepadButton::DPadRight, Key::Right),
            (GamepadButton::South, Key::Activate),
            (GamepadButton::East, Key::Back),
            (GamepadButton::Start, Key::Back),
            (GamepadButton::LeftTrigger, Key::PrevTab),
            (GamepadButton::RightTrigger, Key::NextTab),
        ] {
            if gamepad.just_pressed(button) {
                input.keys.push(key);
            }
        }
        if stick.length() < gamepad.left_stick().length() {
            stick = gamepad.left_stick();
        }
        if scroll.abs() < gamepad.right_stick().y.abs() {
            scroll = gamepad.right_stick().y;
        }
    }
    held.moves.hold(stick_key(stick), dt, &mut input.keys);
    let scrolling = (scroll.abs() > STICK).then_some(if scroll > 0.0 {
        Key::ScrollUp
    } else {
        Key::ScrollDown
    });
    held.scroll.hold(scrolling, dt, &mut input.keys);
    input
}
