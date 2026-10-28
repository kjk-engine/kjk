//! KOTOR 2's item filters (the inventory's and the store's buttons over
//! their lists): one chosen at a time, All first; a list shows the items of
//! the chosen button's kinds.

use crate::paint::ControlState;
use crate::vm::{Entry, ItemKind};

/// A filter's buttons: its tag and the kinds it shows (none: all).
pub type Buttons = &'static [(&'static str, &'static [ItemKind])];

pub const INVENTORY: Buttons = &[
    ("BTN_ALL", &[]),
    ("BTN_DATAPADS", &[ItemKind::Datapad]),
    ("BTN_WEAPONS", &[ItemKind::Weapon]),
    ("BTN_ARMOR", &[ItemKind::Armour]),
    ("BTN_USEABLE", &[ItemKind::Useable]),
    ("BTN_QUESTS", &[ItemKind::Quest]),
    ("BTN_MISC", &[ItemKind::Misc]),
];

pub const STORE: Buttons = &[
    ("BTN_ALL", &[]),
    ("BTN_WEAPONS", &[ItemKind::Weapon]),
    ("BTN_ARMOR", &[ItemKind::Armour]),
    (
        "BTN_MISC",
        &[
            ItemKind::Datapad,
            ItemKind::Useable,
            ItemKind::Quest,
            ItemKind::Misc,
        ],
    ),
];

/// The workbench's breakdown list (TSL's `component` layout).
pub const BREAKDOWN: Buttons = &[
    ("BTN_BREAK_ALL", &[]),
    ("BTN_BREAK_WEAPONS", &[ItemKind::Weapon]),
    ("BTN_BREAK_ARMOR", &[ItemKind::Armour]),
    ("BTN_BREAK_USEABLE", &[ItemKind::Useable]),
    (
        "BTN_BREAK_MISC",
        &[ItemKind::Datapad, ItemKind::Quest, ItemKind::Misc],
    ),
];

pub struct Filter {
    buttons: Buttons,
    chosen: usize,
}

impl Filter {
    pub fn new(buttons: Buttons) -> Filter {
        Filter { buttons, chosen: 0 }
    }

    /// A filter button's look (the chosen one selected); none for another
    /// control.
    pub fn state(&self, tag: &str) -> Option<ControlState> {
        let at = self.buttons.iter().position(|(button, _)| *button == tag)?;
        Some(ControlState {
            selected: at == self.chosen,
            ..ControlState::default()
        })
    }

    /// Chooses `tag`'s button; false for another control.
    pub fn press(&mut self, tag: &str) -> bool {
        let Some(at) = self.buttons.iter().position(|(button, _)| *button == tag) else {
            return false;
        };
        self.chosen = at;
        true
    }

    /// Whether `entry` shows (an entry of no kind counts as misc).
    pub fn keeps(&self, entry: &Entry) -> bool {
        let kinds = self.buttons[self.chosen].1;
        kinds.is_empty() || kinds.contains(&entry.kind.unwrap_or(ItemKind::Misc))
    }

    /// What of `items` shows, in order.
    pub fn shown<'a, T>(&self, items: &'a [T], entry: impl Fn(&T) -> &Entry) -> Vec<&'a T> {
        items
            .iter()
            .filter(|item| self.keeps(entry(item)))
            .collect()
    }
}
