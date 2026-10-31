//! A container's contents (`container`), live: what is left in it as the
//! host has it, alike items one stack with its count on the icon, an item
//! asked for shown faded until the host confirms it. Activating an item
//! takes the whole stack, OK (or A) takes everything and closes it, Close or Back
//! stops holding it open.
//!
//! Its give mode is KOTOR's (`CSWGuiContainer::SetupGiveMode`, issue 47):
//! `BTN_GIVEITEMS` ("Give Items", 38543) lists what the party carries but
//! its plot items under "Items Available to Place in Container" (392), and
//! activating one puts its whole stack in (`GiveItem`: `PerformGiveItem`
//! with the entry's count); the same button, now "Get Items" (38542), goes
//! back to the contents. OK takes nothing there, as KOTOR's.

use fmt_odyssey::gui::Control;

use crate::intent::{GameAction, Intent};
use crate::paint::{ControlState, ListView, Row};
use crate::screen::{Screen, ScreenId};
use crate::screens::tab::told;
use crate::strings;
use crate::vm::{LootRow, ViewModel};

/// KOTOR's words for the give mode: its list's title, the button into it
/// and the one back.
const GIVE_TITLE: u32 = 392;
const GIVE_ITEMS: u32 = 38543;
const GET_ITEMS: u32 = 38542;

#[derive(Default)]
pub struct Container {
    /// In give mode: the party's items listed, to put in.
    giving: bool,
    /// The mode changed since the list was last asked: its cursor starts
    /// at the top (`lists_anew`).
    switched: bool,
}

/// The key that takes everything (`KeyA`).
const TAKE_ALL_KEY: &str = "KeyA";

impl Container {
    /// The rows the list shows: the container's, or in give mode the
    /// party's.
    fn rows<'a>(&self, vm: &'a ViewModel) -> &'a [LootRow] {
        vm.loot.as_ref().map_or(
            &[],
            |loot| {
                if self.giving { &loot.give } else { &loot.rows }
            },
        )
    }

    fn take_all(vm: &ViewModel, out: &mut Vec<Intent>) {
        if let Some(loot) = &vm.loot {
            out.push(Intent::Game(GameAction::TakeAll(loot.container)));
        }
        Container::close(vm, out);
    }

    fn close(vm: &ViewModel, out: &mut Vec<Intent>) {
        if let Some(loot) = &vm.loot {
            out.push(Intent::Game(GameAction::Close(loot.container)));
        }
        out.push(Intent::Close);
    }
}

impl Screen for Container {
    fn id(&self) -> ScreenId {
        ScreenId("container")
    }

    fn layout(&self) -> &str {
        "container"
    }

    /// KOTOR 2's box sits at the frame's corner: centred, as KOTOR 1's is.
    fn shape(&self, gui: &mut fmt_odyssey::gui::Gui) {
        let (width, height) = gui.frame();
        if gui.extent.left == 0 && gui.extent.top == 0 && gui.extent.width < width {
            gui.extent.left = (width - gui.extent.width) / 2;
            gui.extent.top = (height - gui.extent.height) / 2;
        }
    }

    fn state(&self, control: &Control, vm: &ViewModel) -> ControlState {
        let text = |words: &str| ControlState {
            text: Some(strings::bytes(words)),
            ..ControlState::default()
        };
        match control.tag.as_str() {
            "LBL_MESSAGE" if self.giving => told(Some(GIVE_TITLE)),
            "LBL_MESSAGE" => ControlState {
                text: vm.loot.as_ref().map(|loot| loot.title.clone()),
                ..ControlState::default()
            },
            "LB_ITEMS" => ControlState {
                list: Some(ListView {
                    rows: self
                        .rows(vm)
                        .iter()
                        .map(|row| Row {
                            text: row.name.clone(),
                            icon: row.icon.clone(),
                            count: Some(row.count),
                            dimmed: row.pending,
                            ..Row::default()
                        })
                        .collect(),
                    ..ListView::default()
                }),
                ..ControlState::default()
            },
            "BTN_OK" if self.giving => ControlState {
                disabled: true,
                ..told(Some(GIVE_ITEMS))
            },
            "BTN_OK" => text(strings::TAKE_ALL),
            "BTN_CANCEL" => text(strings::CLOSE),
            "BTN_GIVEITEMS" if self.giving => told(Some(GET_ITEMS)),
            "BTN_GIVEITEMS" => told(Some(GIVE_ITEMS)),
            _ => ControlState::default(),
        }
    }

    fn press(&mut self, tag: &str, vm: &ViewModel, out: &mut Vec<Intent>) {
        match tag {
            "BTN_OK" if !self.giving => Container::take_all(vm, out),
            "BTN_CANCEL" => Container::close(vm, out),
            "BTN_GIVEITEMS" => {
                self.giving = !self.giving;
                self.switched = true;
            }
            _ => {}
        }
    }

    fn raw(&mut self, key: &str, vm: &ViewModel, out: &mut Vec<Intent>) -> bool {
        let take = key == TAKE_ALL_KEY && !self.giving;
        if take {
            Container::take_all(vm, out);
        }
        take
    }

    fn row(&mut self, _: &str, index: usize, vm: &ViewModel, out: &mut Vec<Intent>) {
        let (Some(loot), Some(row)) = (&vm.loot, self.rows(vm).get(index)) else {
            return;
        };
        if row.pending {
            return;
        }
        let container = loot.container;
        out.extend(row.items.iter().map(|&item| {
            Intent::Game(if self.giving {
                GameAction::Put { container, item }
            } else {
                GameAction::Take { container, item }
            })
        }));
    }

    fn back(&mut self, _out: &mut Vec<Intent>) {}

    fn key(&mut self, key: crate::input::Key, vm: &ViewModel, out: &mut Vec<Intent>) -> bool {
        if key == crate::input::Key::Back {
            Container::close(vm, out);
            return true;
        }
        false
    }

    fn first_focus(&self) -> Option<&str> {
        Some("LB_ITEMS")
    }

    fn lists_anew(&mut self) -> bool {
        std::mem::take(&mut self.switched)
    }
}
