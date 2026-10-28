//! A computer's conversation (`computer`): the line in the upper screen,
//! the replies numbered below it, the player's Computer Use and Repair
//! with the spikes and parts the scripts count. A reply is taken by its
//! row (pointer, Up and Down, Enter) or by its number; with none offered,
//! Enter or a click ends the line. While the conversation may be left (R6), a Leave
//! row stands under the replies. The game opens and closes it with the
//! conversation; Back does nothing.

use fmt_odyssey::gui::Control;

use crate::input::Key;
use crate::intent::Intent;
use crate::paint::{ControlState, ListView, Row};
use crate::screen::{Screen, ScreenId};
use crate::screens::pages::description;
use crate::screens::tab::text;
use crate::strings;
use crate::vm::{ComputerView, ViewModel};

const REPLIES: &str = "LB_REPLIES";

static NONE: ComputerView = ComputerView {
    message: Vec::new(),
    replies: Vec::new(),
    computer_use: 0,
    spikes: 0,
    repair: 0,
    parts: 0,
    leave: None,
    leave_key: Vec::new(),
};

pub struct Computer;

fn view(vm: &ViewModel) -> &ComputerView {
    vm.computer.as_ref().unwrap_or(&NONE)
}

/// The replies as the list shows them: "1. …", each row as tall as its
/// words; the Leave row under them while the conversation may be left
/// (faded while it is locked, R6).
fn replies(computer: &ComputerView) -> ControlState {
    let leave = computer.leave.map(|offered| Row {
        text: strings::leave(&computer.leave_key),
        dimmed: !offered,
        ..Row::default()
    });
    ControlState {
        list: Some(ListView {
            rows: computer
                .replies
                .iter()
                .enumerate()
                .map(|(nth, reply)| Row {
                    text: strings::reply(nth, reply),
                    ..Row::default()
                })
                .chain(leave)
                .collect(),
            fit: true,
            pick: true,
            ..ListView::default()
        }),
        ..ControlState::default()
    }
}

impl Screen for Computer {
    fn id(&self) -> ScreenId {
        ScreenId("computer")
    }

    fn layout(&self) -> &str {
        "computer"
    }

    fn state(&self, control: &Control, vm: &ViewModel) -> ControlState {
        let computer = view(vm);
        match control.tag.as_str() {
            "LB_MESSAGE" => description(&computer.message),
            REPLIES => replies(computer),
            "LBL_COMP_SKILL_VAL" => text(computer.computer_use),
            "LBL_COMP_SPIKES_VAL" => text(computer.spikes),
            "LBL_REP_SKILL_VAL" => text(computer.repair),
            "LBL_REP_UNITS_VAL" => text(computer.parts),
            _ => ControlState::default(),
        }
    }

    fn press(&mut self, _: &str, _: &ViewModel, _: &mut Vec<Intent>) {}

    fn row(&mut self, list: &str, index: usize, vm: &ViewModel, out: &mut Vec<Intent>) {
        if list != REPLIES {
            return;
        }
        let computer = view(vm);
        if index == computer.replies.len() {
            if computer.leave == Some(true) {
                out.push(Intent::Leave);
            }
        } else if let Ok(nth) = u8::try_from(index) {
            out.push(Intent::Answer(Some(nth)));
        }
    }

    fn back(&mut self, _: &mut Vec<Intent>) {}

    /// A click skips a line with no reply to take, as Enter does.
    fn clicked(&mut self, vm: &ViewModel, out: &mut Vec<Intent>) -> bool {
        let skip = view(vm).replies.is_empty();
        if skip {
            out.push(Intent::Answer(None));
        }
        skip
    }

    fn key(&mut self, key: Key, vm: &ViewModel, out: &mut Vec<Intent>) -> bool {
        let skip = key == Key::Activate && view(vm).replies.is_empty();
        if skip {
            out.push(Intent::Answer(None));
        }
        skip
    }

    fn raw(&mut self, key: &str, vm: &ViewModel, out: &mut Vec<Intent>) -> bool {
        let offered = view(vm).replies.len();
        let nth = key
            .strip_prefix("Digit")
            .or_else(|| key.strip_prefix("Numpad"))
            .and_then(|n| n.parse::<u8>().ok())
            .and_then(|n| n.checked_sub(1))
            .filter(|nth| *nth < 9 && usize::from(*nth) < offered);
        if let Some(nth) = nth {
            out.push(Intent::Answer(Some(nth)));
        }
        nth.is_some()
    }

    fn first_focus(&self) -> Option<&str> {
        Some(REPLIES)
    }
}
