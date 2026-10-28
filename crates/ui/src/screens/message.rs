//! A message and OK (`confirm`): why a session could not start, a
//! script's message box (`DisplayMessageBox`, its strref), and the like,
//! wrapped to the box. OK or Back closes it.

use fmt_odyssey::gui::{Control, Gui};

use crate::intent::Intent;
use crate::paint::ControlState;
use crate::screen::{Screen, ScreenId};
use crate::screens::pages::description;
use crate::screens::tab::told;
use crate::strings;
use crate::vm::ViewModel;

enum Words {
    Text(String),
    Strref(u32),
    /// In the game's code page.
    Said(Vec<u8>),
}

pub struct Message {
    words: Words,
}

impl Message {
    pub fn new(text: &str) -> Message {
        Message {
            words: Words::Text(text.to_string()),
        }
    }

    /// `words` in the game's code page.
    pub fn said(words: Vec<u8>) -> Message {
        Message {
            words: Words::Said(words),
        }
    }

    /// The game's text `strref`.
    pub fn told(strref: &str) -> Option<Message> {
        Some(Message {
            words: Words::Strref(strref.parse().ok()?),
        })
    }
}

/// A message box of pages a script showed (`ViewModel::pages`): KOTOR's
/// tutorial window (its `tutorial.2da` row's `Message_PC0` to `2`, a page
/// each) and TSL's datapad (its item's words). OK turns the page, and on
/// the last closes it; there is no cancel. The tutorial's icon is not drawn
/// [verify where KOTOR puts it].
#[derive(Default)]
pub struct Pages {
    at: usize,
}

impl Screen for Pages {
    fn id(&self) -> ScreenId {
        ScreenId("pages")
    }

    fn layout(&self) -> &str {
        "confirm"
    }

    fn state(&self, control: &Control, vm: &ViewModel) -> ControlState {
        match control.tag.as_str() {
            "LB_MESSAGE" => description(vm.pages.get(self.at).map_or(&[][..], Vec::as_slice)),
            "BTN_OK" => ControlState {
                text: Some(strings::bytes(strings::OK)),
                ..ControlState::default()
            },
            "BTN_CANCEL" => ControlState {
                hidden: true,
                ..ControlState::default()
            },
            _ => ControlState::default(),
        }
    }

    fn press(&mut self, tag: &str, vm: &ViewModel, out: &mut Vec<Intent>) {
        if tag == "BTN_OK" {
            self.at += 1;
            if self.at >= vm.pages.len() {
                out.push(Intent::Close);
            }
        }
    }

    fn first_focus(&self) -> Option<&str> {
        Some("BTN_OK")
    }

    fn shape(&self, gui: &mut Gui) {
        grow(gui);
    }
}

/// Lines of room the box gains for its message.
const GROW: i32 = 3 * 18;

impl Screen for Message {
    fn id(&self) -> ScreenId {
        ScreenId("message")
    }

    fn layout(&self) -> &str {
        "confirm"
    }

    fn state(&self, control: &Control, _: &ViewModel) -> ControlState {
        match control.tag.as_str() {
            "LB_MESSAGE" => match &self.words {
                Words::Text(text) => description(&strings::bytes(text)),
                Words::Strref(strref) => told(Some(*strref)),
                Words::Said(words) => description(words),
            },
            "BTN_OK" => ControlState {
                text: Some(strings::bytes(strings::OK)),
                ..ControlState::default()
            },
            "BTN_CANCEL" => ControlState {
                hidden: true,
                ..ControlState::default()
            },
            _ => ControlState::default(),
        }
    }

    fn press(&mut self, tag: &str, _: &ViewModel, out: &mut Vec<Intent>) {
        if tag == "BTN_OK" {
            out.push(Intent::Close);
        }
    }

    fn first_focus(&self) -> Option<&str> {
        Some("BTN_OK")
    }

    fn shape(&self, gui: &mut Gui) {
        grow(gui);
    }
}

/// `confirm` grown by `GROW` for its message, the buttons below moved down.
pub(super) fn grow(gui: &mut Gui) {
    gui.extent.height += GROW;
    gui.extent.top -= GROW / 2;
    let list_bottom = gui
        .find("LB_MESSAGE")
        .map(|list| list.extent.top + list.extent.height);
    for control in &mut gui.controls {
        if control.tag == "LB_MESSAGE" {
            control.extent.height += GROW;
        } else if list_bottom.is_some_and(|bottom| control.extent.top >= bottom) {
            control.extent.top += GROW;
        }
    }
}
