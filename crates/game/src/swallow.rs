//! Presses something took: a movie skipped, a line skipped or a reply
//! picked, the click that takes the cursor. Play never sees them held: the
//! key or button reads as up until it is let go, so the click that ends the
//! opening crawl does not swing once the world resumes, nor its Space jump.

use std::collections::HashSet;

use bevy::input::ButtonState;
use bevy::input::keyboard::{KeyboardFocusLost, KeyboardInput};
use bevy::input::mouse::MouseButtonInput;
use bevy::prelude::*;

/// The keys and buttons taken and still down.
#[derive(Resource, Debug, Default)]
pub(crate) struct Swallowed {
    keys: HashSet<KeyCode>,
    buttons: HashSet<MouseButton>,
}

impl Swallowed {
    /// `key` taken, if it is still down: a tap pressed and let go within
    /// one frame was already let go (its release read before it was
    /// taken), and held up it would eat the next press.
    pub(crate) fn key(&mut self, key: KeyCode, keys: &ButtonInput<KeyCode>) {
        if keys.pressed(key) {
            self.keys.insert(key);
        }
    }

    /// `button` taken, if it is still down (as `key`).
    pub(crate) fn button(&mut self, button: MouseButton, buttons: &ButtonInput<MouseButton>) {
        if buttons.pressed(button) {
            self.buttons.insert(button);
        }
    }

    /// Every button pressed this frame, taken (`buttons`' presses cleared,
    /// so nothing later in the frame sees them); whether there was one.
    pub(crate) fn pressed_buttons(&mut self, buttons: &mut ButtonInput<MouseButton>) -> bool {
        let pressed: Vec<MouseButton> = buttons.get_just_pressed().copied().collect();
        for button in &pressed {
            buttons.clear_just_pressed(*button);
            self.button(*button, buttons);
        }
        !pressed.is_empty()
    }
}

/// Runs after bevy reads the frame's input and before play does: what was
/// let go is forgotten, the rest reads as up (a key's repeat presses it
/// again, so this holds it up every frame, not once).
pub(crate) fn hold_up(
    mut swallowed: ResMut<Swallowed>,
    mut keys: Option<ResMut<ButtonInput<KeyCode>>>,
    mut buttons: Option<ResMut<ButtonInput<MouseButton>>>,
    mut key_events: MessageReader<KeyboardInput>,
    mut button_events: MessageReader<MouseButtonInput>,
    mut focus_lost: MessageReader<KeyboardFocusLost>,
) {
    let released = |state: &ButtonState| *state == ButtonState::Released;
    for event in key_events.read().filter(|event| released(&event.state)) {
        swallowed.keys.remove(&event.key_code);
    }
    for event in button_events.read().filter(|event| released(&event.state)) {
        swallowed.buttons.remove(&event.button);
    }
    // Bevy lets every key go when the window loses the keyboard, and the
    // release may never be heard.
    if focus_lost.read().count() > 0 {
        swallowed.keys.clear();
        swallowed.buttons.clear();
    }
    if swallowed.keys.is_empty() && swallowed.buttons.is_empty() {
        return;
    }
    if let Some(keys) = keys.as_deref_mut() {
        for key in &swallowed.keys {
            keys.reset(*key);
        }
    }
    if let Some(buttons) = buttons.as_deref_mut() {
        for button in &swallowed.buttons {
            buttons.reset(*button);
        }
    }
}
