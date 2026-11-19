//! The journal (`journal`), a page of the in-game menu, as KOTOR 2's: the
//! active quests or, after the swap button, the completed ones, sorted by
//! the time each was last updated (the latest first), by name, or by
//! planet; the chosen quest's entry beside them (the first, until another
//! is chosen), and the way to the messages.
//!
//! KOTOR 2's journal sets up only its Time, Name and Planet buttons: the
//! layout's Priority button is never shown. Its sort by planet puts the
//! higher `planetary.2da` rows first, and quests of one planet the latest
//! updated first.

use fmt_odyssey::gui::Control;

use crate::intent::Intent;
use crate::paint::ControlState;
use crate::screens::pages::description;
use crate::screens::tab::{Page, hidden, rows};
use crate::vm::{Entry, QuestView, ViewModel};

/// KOTOR 2's lines for the swap button and the sort buttons (its own talk
/// table's).
mod lines {
    pub const ACTIVE_BY: u32 = 49_145;
    pub const COMPLETED_BY: u32 = 49_146;
    pub const TIME: u32 = 50_676;
    pub const PLANET: u32 = 50_679;
}

/// How the quests are sorted.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
enum Sort {
    #[default]
    Time,
    Name,
    Planet,
}

impl Sort {
    fn of(tag: &str) -> Option<Sort> {
        match tag {
            "BTN_FILTER_TIME" => Some(Sort::Time),
            "BTN_FILTER_NAME" => Some(Sort::Name),
            "BTN_FILTER_PLANET" => Some(Sort::Planet),
            _ => None,
        }
    }
}

#[derive(Default)]
pub struct Journal {
    selected: Option<usize>,
    /// The completed quests shown, in place of the active ones.
    done: bool,
    sort: Sort,
}

impl Journal {
    /// The quests listed: the active or completed ones, in the sort's order.
    /// The view model has them the latest updated first, and the sorts keep
    /// that order among equals.
    fn listed<'a>(&self, vm: &'a ViewModel) -> Vec<&'a QuestView> {
        let mut listed: Vec<&QuestView> = vm
            .journal
            .iter()
            .filter(|quest| quest.done == self.done)
            .collect();
        match self.sort {
            Sort::Time => {}
            Sort::Name => listed.sort_by(|a, b| a.name.cmp(&b.name)),
            Sort::Planet => listed.sort_by_key(|quest| std::cmp::Reverse(quest.planet)),
        }
        listed
    }

    /// A sort button: its text (KOTOR 2's own where it sets one), lit when
    /// its sort is the one in use.
    fn sort_button(&self, sort: Sort, strref: Option<u32>) -> ControlState {
        ControlState {
            selected: self.sort == sort,
            strref,
            skin_strref: strref.is_some(),
            ..ControlState::default()
        }
    }
}

impl Page for Journal {
    const NAME: &'static str = "journal";
    const LAYOUT: &'static str = "journal";

    fn state(&self, control: &Control, vm: &ViewModel) -> ControlState {
        let listed = self.listed(vm);
        // It opens on the first quest listed until another is chosen.
        let selected = self.selected.or_else(|| (!listed.is_empty()).then_some(0));
        let chosen = selected.and_then(|at| listed.get(at));
        match control.tag.as_str() {
            "LB_ITEMS" => {
                let entries: Vec<Entry> = listed
                    .iter()
                    .map(|quest| Entry {
                        name: quest.name.clone(),
                        ..Entry::default()
                    })
                    .collect();
                rows(&entries, selected)
            }
            "LBL_ITEM_DESCRIPTION" => {
                description(&chosen.map_or(Vec::new(), |quest| quest.description.clone()))
            }
            "BTN_SWAPTEXT" => ControlState {
                strref: Some(if self.done {
                    lines::COMPLETED_BY
                } else {
                    lines::ACTIVE_BY
                }),
                skin_strref: true,
                ..ControlState::default()
            },
            "BTN_FILTER_TIME" => self.sort_button(Sort::Time, Some(lines::TIME)),
            "BTN_FILTER_NAME" => self.sort_button(Sort::Name, None),
            "BTN_FILTER_PLANET" => self.sort_button(Sort::Planet, Some(lines::PLANET)),
            "BTN_FILTER_PRIORITY" => hidden(),
            _ => ControlState::default(),
        }
    }

    fn press(&mut self, tag: &str, _: &ViewModel, out: &mut Vec<Intent>) {
        if tag == "BTN_MESSAGES" {
            out.push(Intent::Swap("messages"));
        } else if tag == "BTN_SWAPTEXT" {
            self.done = !self.done;
            self.selected = None;
        } else if let Some(sort) = Sort::of(tag) {
            self.sort = sort;
            self.selected = None;
        }
    }

    fn row(&mut self, list: &str, index: usize, vm: &ViewModel) {
        self.pointed(list, index, vm);
    }

    /// The row the pointer or the cursor comes to is chosen, as in KOTOR 2.
    fn pointed(&mut self, _: &str, index: usize, _: &ViewModel) {
        self.selected = Some(index);
    }

    fn first_focus(&self) -> Option<&str> {
        Some("LB_ITEMS")
    }
}
