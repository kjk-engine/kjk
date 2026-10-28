//! The key mapping (`optkeymapping`): every action and its key, movement
//! or the game's; activating one asks for its new key (`optkeyentry`, the
//! next key pressed). Accept keeps the changes, Cancel puts back the keys
//! the page opened with, Default restores the game's own.

use fmt_odyssey::gui::Control;

use crate::intent::Intent;
use crate::paint::{ControlState, ListView, Row};
use crate::screen::{Screen, ScreenId};
use crate::strings;
use crate::vm::{KeyRow, ViewModel};

pub struct KeyMapping {
    /// The movement keys shown, else the game's.
    movement: bool,
    /// The keys when the page opened.
    before: Vec<KeyRow>,
}

impl KeyMapping {
    pub fn new(vm: &ViewModel) -> KeyMapping {
        KeyMapping {
            movement: true,
            before: vm.keys.clone(),
        }
    }

    fn shown<'a>(&self, vm: &'a ViewModel) -> impl Iterator<Item = &'a KeyRow> + use<'a> {
        let movement = self.movement;
        vm.keys.iter().filter(move |row| row.movement == movement)
    }
}

impl Screen for KeyMapping {
    fn id(&self) -> ScreenId {
        ScreenId("options_keymap")
    }

    fn layout(&self) -> &str {
        "optkeymapping"
    }

    fn state(&self, control: &Control, vm: &ViewModel) -> ControlState {
        match control.tag.as_str() {
            "LST_EventList" => ControlState {
                list: Some(ListView {
                    rows: self
                        .shown(vm)
                        .map(|row| Row {
                            text: strings::bytes(&format!("{}: {}", row.label, key_name(&row.key))),
                            ..Row::default()
                        })
                        .collect(),
                    ..ListView::default()
                }),
                ..ControlState::default()
            },
            "BTN_Filter_Move" => ControlState {
                selected: self.movement,
                ..ControlState::default()
            },
            "BTN_Filter_Game" => ControlState {
                selected: !self.movement,
                ..ControlState::default()
            },
            "BTN_Filter_Mini" => ControlState {
                hidden: true,
                ..ControlState::default()
            },
            _ => ControlState::default(),
        }
    }

    fn press(&mut self, tag: &str, vm: &ViewModel, out: &mut Vec<Intent>) {
        match tag {
            "BTN_Filter_Move" => self.movement = true,
            "BTN_Filter_Game" => self.movement = false,
            "BTN_Accept" => out.push(Intent::Close),
            "BTN_Cancel" => self.back(out),
            "BTN_Default" => out.push(Intent::Setting("reset".into(), "keys".into())),
            _ => {}
        }
        let _ = vm;
    }

    fn row(&mut self, _: &str, index: usize, vm: &ViewModel, out: &mut Vec<Intent>) {
        if let Some(row) = self.shown(vm).nth(index) {
            out.push(Intent::OpenFor("key_entry", row.action.clone()));
        }
    }

    fn back(&mut self, out: &mut Vec<Intent>) {
        for row in &self.before {
            out.push(Intent::Setting(
                format!("key.{}", row.action),
                row.key.clone(),
            ));
        }
        out.push(Intent::Close);
    }

    fn first_focus(&self) -> Option<&str> {
        Some("LST_EventList")
    }
}

/// A key's name as shown: `KeyW` as `W`, `Digit1` as `1`.
pub fn key_name(key: &str) -> &str {
    key.strip_prefix("Key")
        .or_else(|| key.strip_prefix("Digit"))
        .unwrap_or(key)
}

/// Asking for an action's new key (`optkeyentry`): the next key pressed
/// is it; Escape or Cancel keeps the old one.
pub struct KeyEntry {
    action: String,
    label: String,
}

impl KeyEntry {
    pub fn new(action: &str, vm: &ViewModel) -> KeyEntry {
        KeyEntry {
            action: action.to_string(),
            label: vm
                .keys
                .iter()
                .find(|row| row.action == action)
                .map_or(action.to_string(), |row| row.label.clone()),
        }
    }
}

impl Screen for KeyEntry {
    fn id(&self) -> ScreenId {
        ScreenId("key_entry")
    }

    fn layout(&self) -> &str {
        "optkeyentry"
    }

    fn state(&self, control: &Control, _: &ViewModel) -> ControlState {
        match control.tag.as_str() {
            "LBL_EventName" => ControlState {
                text: Some(strings::bytes(&self.label)),
                ..ControlState::default()
            },
            _ => ControlState::default(),
        }
    }

    fn press(&mut self, tag: &str, _: &ViewModel, out: &mut Vec<Intent>) {
        if tag == "BTN_Cancel" {
            out.push(Intent::Close);
        }
    }

    fn raw(&mut self, key: &str, _: &ViewModel, out: &mut Vec<Intent>) -> bool {
        if key != "Escape" {
            out.push(Intent::Setting(
                format!("key.{}", self.action),
                key.to_string(),
            ));
        }
        out.push(Intent::Close);
        true
    }
}
