//! A container's contents (`container`), live: what is left in it as the
//! host has it, alike items one stack with its count on the icon, an item
//! asked for shown faded until the host confirms it. Activating an item
//! takes the whole stack, OK (or A) takes everything and closes it, Close or Back
//! stops holding it open. Giving waits for the world state's inventory.

use fmt_odyssey::gui::Control;

use crate::intent::{GameAction, Intent};
use crate::paint::{ControlState, ListView, Row};
use crate::screen::{Screen, ScreenId};
use crate::strings;
use crate::vm::ViewModel;

pub struct Container;

/// The key that takes everything (`KeyA`).
const TAKE_ALL_KEY: &str = "KeyA";

impl Container {
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
            "LBL_MESSAGE" => ControlState {
                text: vm.loot.as_ref().map(|loot| loot.title.clone()),
                ..ControlState::default()
            },
            "LB_ITEMS" => ControlState {
                list: Some(ListView {
                    rows: vm
                        .loot
                        .iter()
                        .flat_map(|loot| &loot.rows)
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
            "BTN_OK" => text(strings::TAKE_ALL),
            "BTN_CANCEL" => text(strings::CLOSE),
            "BTN_GIVEITEMS" => ControlState {
                hidden: true,
                ..ControlState::default()
            },
            _ => ControlState::default(),
        }
    }

    fn press(&mut self, tag: &str, vm: &ViewModel, out: &mut Vec<Intent>) {
        match tag {
            "BTN_OK" => Container::take_all(vm, out),
            "BTN_CANCEL" => Container::close(vm, out),
            _ => {}
        }
    }

    fn raw(&mut self, key: &str, vm: &ViewModel, out: &mut Vec<Intent>) -> bool {
        let take = key == TAKE_ALL_KEY;
        if take {
            Container::take_all(vm, out);
        }
        take
    }

    fn row(&mut self, _: &str, index: usize, vm: &ViewModel, out: &mut Vec<Intent>) {
        if let Some(loot) = &vm.loot
            && let Some(row) = loot.rows.get(index).filter(|row| !row.pending)
        {
            out.extend(row.items.iter().map(|&item| {
                Intent::Game(GameAction::Take {
                    container: loot.container,
                    item,
                })
            }));
        }
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
}
