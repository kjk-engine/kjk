//! The party has fallen (I30 P13): KOTOR's death box, on every player's
//! screen over the world. The host's Load loads its latest save for
//! everyone (`Intent::ReloadLatest`); a guest's waits for it. It closes when
//! the world is no longer lost (`launcher::ui_view::fall`).

use fmt_odyssey::gui::{Control, Gui};

use crate::intent::Intent;
use crate::paint::ControlState;
use crate::screen::{Screen, ScreenId};
use crate::screens::pages::description;
use crate::strings;
use crate::vm::ViewModel;

pub struct Lost;

fn hosting(vm: &ViewModel) -> bool {
    vm.lost.is_some_and(|lost| lost.host)
}

impl Screen for Lost {
    fn id(&self) -> ScreenId {
        ScreenId("lost")
    }

    fn layout(&self) -> &str {
        "confirm"
    }

    fn state(&self, control: &Control, vm: &ViewModel) -> ControlState {
        let host = hosting(vm);
        match control.tag.as_str() {
            "LB_MESSAGE" => description(&strings::bytes(if host {
                strings::PARTY_FALLEN
            } else {
                strings::WAIT_FOR_HOST
            })),
            "BTN_OK" if host => ControlState {
                text: Some(strings::bytes(strings::LOAD)),
                ..ControlState::default()
            },
            "BTN_OK" | "BTN_CANCEL" => ControlState {
                hidden: true,
                ..ControlState::default()
            },
            _ => ControlState::default(),
        }
    }

    fn press(&mut self, tag: &str, vm: &ViewModel, out: &mut Vec<Intent>) {
        if tag == "BTN_OK" && hosting(vm) {
            out.push(Intent::ReloadLatest);
        }
    }

    fn first_focus(&self) -> Option<&str> {
        Some("BTN_OK")
    }

    fn shape(&self, gui: &mut Gui) {
        super::message::grow(gui);
    }
}
