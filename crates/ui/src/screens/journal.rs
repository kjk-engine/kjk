//! The journal (`journal`), a page of the in-game menu: the quests by
//! name, the chosen one's entry beside them (the first, until another is
//! chosen), and the way to the messages.
//! Sorting and the other text wait for the journal's data (I19): greyed.

use fmt_odyssey::gui::Control;

use crate::intent::Intent;
use crate::paint::ControlState;
use crate::screens::pages::description;
use crate::screens::tab::{Page, disabled, rows};
use crate::vm::ViewModel;

#[derive(Default)]
pub struct Journal {
    selected: Option<usize>,
}

impl Page for Journal {
    const NAME: &'static str = "journal";
    const LAYOUT: &'static str = "journal";

    fn state(&self, control: &Control, vm: &ViewModel) -> ControlState {
        // It opens on the first quest (the latest updated) until another is
        // chosen.
        let selected = self
            .selected
            .or_else(|| (!vm.journal.is_empty()).then_some(0));
        let chosen = selected.and_then(|at| vm.journal.get(at));
        match control.tag.as_str() {
            "LB_ITEMS" => rows(&vm.journal, selected),
            "LBL_ITEM_DESCRIPTION" => {
                description(&chosen.map_or(Vec::new(), |quest| quest.description.clone()))
            }
            "BTN_SWAPTEXT"
            | "BTN_FILTER_TIME"
            | "BTN_FILTER_NAME"
            | "BTN_FILTER_PRIORITY"
            | "BTN_FILTER_PLANET" => disabled(),
            _ => ControlState::default(),
        }
    }

    fn press(&mut self, tag: &str, _: &ViewModel, out: &mut Vec<Intent>) {
        if tag == "BTN_MESSAGES" {
            out.push(Intent::Swap("messages"));
        }
    }

    fn row(&mut self, _: &str, index: usize, _: &ViewModel) {
        self.selected = Some(index);
    }

    fn first_focus(&self) -> Option<&str> {
        Some("LB_ITEMS")
    }
}
