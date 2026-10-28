//! The credits (`StartCreditSequence`, K1's ending): KOTOR's `credits`, a
//! block of names a `credits.2da` row, rise through their list on black,
//! then the main menu (the scripts end the game under them); Back goes
//! there at once.

use fmt_odyssey::gui::Control;

use crate::intent::{Intent, SessionIntent};
use crate::paint::{ControlState, ListView, Row};
use crate::screen::{Screen, ScreenId};
use crate::vm::ViewModel;

/// How fast the credits rise (layout pixels a second) [verify].
const SPEED: f32 = 40.0;

#[derive(Default)]
pub struct Credits {
    left: bool,
}

impl Credits {
    fn leave(&mut self, out: &mut Vec<Intent>) {
        if !std::mem::replace(&mut self.left, true) {
            out.push(Intent::Session(SessionIntent::ToMenu));
        }
    }
}

impl Screen for Credits {
    fn id(&self) -> ScreenId {
        ScreenId("credits")
    }

    fn layout(&self) -> &str {
        "credits"
    }

    fn state(&self, control: &Control, vm: &ViewModel) -> ControlState {
        if control.tag != "LB_CREDITS" {
            return ControlState::default();
        }
        ControlState {
            list: Some(ListView {
                rows: vm
                    .credits
                    .iter()
                    .map(|block| Row {
                        text: block.clone(),
                        ..Row::default()
                    })
                    .collect(),
                fit: true,
                roll: Some(SPEED),
                ..ListView::default()
            }),
            ..ControlState::default()
        }
    }

    fn press(&mut self, _: &str, _: &ViewModel, _: &mut Vec<Intent>) {}

    fn back(&mut self, out: &mut Vec<Intent>) {
        self.leave(out);
    }

    fn rolled(&mut self, _: &str, out: &mut Vec<Intent>) {
        self.leave(out);
    }
}
