//! Keys, the mouse and gamepads turned into the screens' moves
//! (`ui::input`): arrows and Tab move, Enter activates, Escape goes back,
//! Page Up and Down scroll, typed text goes to edit boxes; a gamepad's
//! d-pad or left stick moves (repeating while held), A activates, B goes
//! back (on a screen: in play it is the pad's crouch), Menu opens the menu
//! in play and presses a screen's OK or Done (not with LB held: the quick
//! save), X does a double click, Y the screen's second button (Default),
//! the shoulders switch the menu bar's tabs, the triggers a page's own, the
//! right stick scrolls.

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
    pads: Option<(&Query<&Gamepad>, bool)>,
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
    input.held = mouse.is_some_and(|mouse| mouse.pressed(MouseButton::Left));
    // Each mouse button pressed by its name, for binding it (`key_mapping`;
    // a left click on a control stays the control's, `Stack`).
    input.raw.extend(
        mouse
            .into_iter()
            .flat_map(|mouse| mouse.get_just_pressed())
            .map(|button| crate::settings::Input::Mouse(*button).name()),
    );

    let (mut stick, mut scroll) = (Vec2::ZERO, 0.0f32);
    let (gamepads, open) = match pads {
        Some((gamepads, open)) => (Some(gamepads), open),
        None => (None, false),
    };
    for gamepad in gamepads.into_iter().flatten() {
        let shift = gamepad.pressed(crate::pad::SHIFT);
        for (button, key) in [
            (GamepadButton::DPadUp, Key::Up),
            (GamepadButton::DPadDown, Key::Down),
            (GamepadButton::DPadLeft, Key::Left),
            (GamepadButton::DPadRight, Key::Right),
            (GamepadButton::South, Key::Activate),
            (GamepadButton::East, Key::Back),
            (GamepadButton::West, Key::Primary),
            (GamepadButton::North, Key::Second),
            // Menu opens the menu in play, and accepts a screen open.
            (
                GamepadButton::Start,
                if open { Key::Accept } else { Key::Back },
            ),
            (GamepadButton::LeftTrigger, Key::PrevTab),
            (GamepadButton::RightTrigger, Key::NextTab),
            (GamepadButton::LeftTrigger2, Key::PrevPage),
            (GamepadButton::RightTrigger2, Key::NextPage),
        ] {
            // In play B crouches, and LB's Menu is the quick save.
            let play =
                !open && button == GamepadButton::East || shift && button == GamepadButton::Start;
            if gamepad.just_pressed(button) && !play {
                input.keys.push(key);
            }
        }
        // Each button pressed by its chord's name, for binding it
        // (`key_mapping`): LB alone is none, it is the chord's shift.
        input.raw.extend(
            gamepad
                .digital()
                .get_just_pressed()
                .filter(|button| **button != crate::pad::SHIFT)
                .map(|button| {
                    let chord = crate::pad::Chord {
                        shifted: shift,
                        button: *button,
                    };
                    format!("{}{}", ui::screens::key_mapping::PAD_RAW, chord.name())
                }),
        );
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
