//! The bug report's box, in KOTOR's box for a save's name (`savename`):
//! what went wrong, typed; OK makes the report (`Intent::Report`), Cancel
//! goes back.

use fmt_odyssey::gui::Control;

use crate::edit::EditBox;
use crate::intent::Intent;
use crate::paint::ControlState;
use crate::screen::{Screen, ScreenId};
use crate::strings;
use crate::vm::ViewModel;

/// How long a note may be.
const NOTE_LONGEST: usize = 500;

pub struct Report {
    note: EditBox,
}

impl Default for Report {
    fn default() -> Self {
        Report {
            note: EditBox::new(NOTE_LONGEST),
        }
    }
}

fn text(words: &str) -> ControlState {
    ControlState {
        text: Some(strings::bytes(words)),
        ..ControlState::default()
    }
}

impl Screen for Report {
    fn id(&self) -> ScreenId {
        ScreenId("report")
    }

    fn layout(&self) -> &str {
        "savename"
    }

    fn state(&self, control: &Control, _: &ViewModel) -> ControlState {
        match control.tag.as_str() {
            "LBL_TITLE" => {
                text("What went wrong? (The report holds your save, character names included.)")
            }
            "BTN_OK" => text("Make report"),
            "BTN_CANCEL" => text(strings::CANCEL),
            _ => ControlState::default(),
        }
    }

    fn press(&mut self, tag: &str, _: &ViewModel, out: &mut Vec<Intent>) {
        match tag {
            "BTN_OK" => out.push(Intent::Report(self.note.as_str())),
            "BTN_CANCEL" => out.push(Intent::Close),
            _ => {}
        }
    }

    fn edit(&mut self, tag: &str) -> Option<&mut EditBox> {
        (tag == "EDITBOX").then_some(&mut self.note)
    }

    fn typing(&self) -> Option<&str> {
        Some("EDITBOX")
    }

    fn first_focus(&self) -> Option<&str> {
        Some("BTN_OK")
    }
}
