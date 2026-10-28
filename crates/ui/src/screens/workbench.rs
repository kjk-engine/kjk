//! TSL's workbench (`workbench`, a script's `ShowUpgradeScreen` with item
//! creation; TSL's `component` layout) and lab station (`chemical`,
//! `ShowChemicalUpgradeScreen`; its `chemical` layout): one list at a time,
//! as KOTOR 2's. Making items, the table's items of the tab chosen (its
//! `group`: the workbench's armour, ranged, melee, lightsaber and other,
//! opened on other; the lab station's medical, stimulants, mines and
//! grenades, opened on medical), those the maker's skill does not reach
//! greyed; breaking down (View Breakdown Items), what the party carries
//! that the bench breaks down, at the workbench all or of a kind. The
//! chosen item's description, the skill it asks and the maker's total in
//! it, its price (or what it yields), the party's components (chemicals)
//! and how many it carries. Accept makes or breaks down the chosen item
//! (the host refuses with the game's line: the skill not reached, too few
//! components, upgrades still fitted); the bench told back shows the
//! result. Close (Back, where upgrading may be chosen) closes it.

use fmt_odyssey::gui::Control;

use crate::intent::{GameAction, Intent};
use crate::paint::{ControlState, ListView, Row};
use crate::screen::{Screen, ScreenId};
use crate::screens::filter::{self, Filter};
use crate::screens::pages::description;
use crate::screens::tab::{enabled, hidden, rows, shown, text, told};
use crate::vm::{BreakdownView, RecipeView, ViewModel, WorkbenchView};

const RECIPES: &str = "LB_SHOPITEMS";
const CARRIED: &str = "LB_INVITEMS";

/// The making tabs and the table's `group` each shows: the workbench's,
/// and the lab station's (its third button mines, its fourth grenades, as
/// TSL's lab screen binds them).
const GROUPS: [(&str, u8); 5] = [
    ("BTN_CREATE_ARMOR", 0),
    ("BTN_CREATE_RANGED", 1),
    ("BTN_CREATE_MELEE", 2),
    ("BTN_CREATE_LIGHTSABER", 3),
    ("BTN_CREATE_MISC", 4),
];
const LAB_GROUPS: [(&str, u8); 4] = [
    ("BTN_CREATE_HEALTH", 0),
    ("BTN_CREATE_STIMS", 1),
    ("BTN_CREATE_MINES", 2),
    ("BTN_CREATE_GRENADES", 3),
];

/// The close button's words: "Close", or "Back" where upgrading may be
/// chosen.
const CLOSE: u32 = 1582;
const BACK: u32 = 219;

pub struct Workbench {
    lab: bool,
    breaking: bool,
    group: u8,
    filter: Filter,
    chosen: Option<usize>,
}

impl Workbench {
    /// The workbench, on its other tab; the lab station (`lab`) on its
    /// medical one (TSL's screens open so).
    pub fn new(lab: bool) -> Workbench {
        Workbench {
            lab,
            breaking: false,
            group: if lab { 0 } else { 4 },
            filter: Filter::new(filter::BREAKDOWN),
            chosen: None,
        }
    }

    /// The tab button `tag` names: the group it shows.
    fn group_of(&self, tag: &str) -> Option<u8> {
        let groups: &[(&str, u8)] = if self.lab { &LAB_GROUPS } else { &GROUPS };
        groups
            .iter()
            .find(|(button, _)| *button == tag)
            .map(|(_, group)| *group)
    }

    fn recipes<'a>(&self, bench: &'a WorkbenchView) -> Vec<&'a RecipeView> {
        bench
            .recipes
            .iter()
            .filter(|recipe| recipe.group == self.group)
            .collect()
    }

    /// What may be broken down: at the lab station, all of it (its screen
    /// has no filters).
    fn carried<'a>(&self, bench: &'a WorkbenchView) -> Vec<&'a BreakdownView> {
        match self.lab {
            true => bench.breakdowns.iter().collect(),
            false => self.filter.shown(&bench.breakdowns, |item| &item.entry),
        }
    }

    /// What Accept does now.
    fn action(&self, bench: &WorkbenchView) -> Option<GameAction> {
        let chosen = self.chosen?;
        Some(if self.breaking {
            GameAction::Breakdown {
                bench: bench.bench.clone(),
                item: self.carried(bench).get(chosen)?.item,
            }
        } else {
            GameAction::Create {
                bench: bench.bench.clone(),
                row: self.recipes(bench).get(chosen)?.row,
            }
        })
    }

    fn recipe_rows(&self, bench: &WorkbenchView) -> ControlState {
        let rows = self
            .recipes(bench)
            .iter()
            .map(|recipe| Row {
                text: recipe.entry.name.clone(),
                icon: recipe.entry.icon.clone(),
                dimmed: !recipe.open,
                ..Row::default()
            })
            .collect();
        ControlState {
            list: Some(ListView {
                rows,
                selected: self.chosen,
                ..ListView::default()
            }),
            ..ControlState::default()
        }
    }

    fn state_in(&self, tag: &str, bench: &WorkbenchView) -> ControlState {
        if let Some(group) = self.group_of(tag) {
            return ControlState {
                hidden: self.breaking,
                selected: self.group == group,
                ..ControlState::default()
            };
        }
        if let Some(state) = self.filter.state(tag).filter(|_| !self.lab) {
            return ControlState {
                hidden: !self.breaking,
                ..state
            };
        }
        let recipe = (!self.breaking)
            .then(|| self.recipes(bench).get(self.chosen?).copied())
            .flatten();
        let carried = self
            .breaking
            .then(|| self.carried(bench).get(self.chosen?).copied())
            .flatten();
        let entry = recipe
            .map(|recipe| &recipe.entry)
            .or(carried.map(|item| &item.entry));
        match tag {
            RECIPES if self.breaking => hidden(),
            CARRIED if !self.breaking => hidden(),
            RECIPES => self.recipe_rows(bench),
            CARRIED => {
                let entries: Vec<_> = self
                    .carried(bench)
                    .iter()
                    .map(|item| item.entry.clone())
                    .collect();
                rows(&entries, self.chosen)
            }
            "LB_DESCRIPTION" | "LB_DESCRIPTIONINV" => {
                let own = (tag == "LB_DESCRIPTIONINV") == self.breaking;
                match own {
                    true => {
                        description(&entry.map_or(Vec::new(), |entry| entry.description.clone()))
                    }
                    false => hidden(),
                }
            }
            "LBL_RELATEDSKILL" => ControlState {
                text: Some(recipe.map_or(Vec::new(), |recipe| recipe.skill.clone())),
                ..ControlState::default()
            },
            "LBL_COST_VALUE" => match (recipe, carried) {
                (Some(recipe), _) => text(recipe.price),
                (_, Some(item)) => text(item.yields),
                _ => text(""),
            },
            "LBL_CREDITS_VALUE" => text(bench.stock),
            "LBL_STOCK_VALUE" => match (recipe, carried) {
                (Some(recipe), _) => text(recipe.held),
                (_, Some(item)) => text(item.entry.count.unwrap_or(1)),
                _ => text(""),
            },
            "BTN_Accept" => enabled(self.action(bench).is_some()),
            "BTN_Examine" => shown(true),
            "BTN_Cancel" if !self.lab => told(Some(if bench.upgrade { BACK } else { CLOSE })),
            _ => ControlState::default(),
        }
    }
}

impl Screen for Workbench {
    fn id(&self) -> ScreenId {
        ScreenId(if self.lab { "chemical" } else { "workbench" })
    }

    fn layout(&self) -> &str {
        if self.lab { "chemical" } else { "component" }
    }

    fn state(&self, control: &Control, vm: &ViewModel) -> ControlState {
        match &vm.workbench {
            Some(bench) => self.state_in(&control.tag, bench),
            None => ControlState::default(),
        }
    }

    fn press(&mut self, tag: &str, vm: &ViewModel, out: &mut Vec<Intent>) {
        match tag {
            "BTN_Cancel" | "BTN_BACK" => {
                out.push(Intent::Close);
                return;
            }
            "BTN_Examine" => {
                self.breaking = !self.breaking;
                self.chosen = None;
                return;
            }
            _ => {}
        }
        if let Some(group) = self.group_of(tag) {
            self.group = group;
            self.chosen = None;
        } else if !self.lab && self.filter.press(tag) {
            self.chosen = None;
        } else if tag == "BTN_Accept"
            && let Some(action) = vm.workbench.as_ref().and_then(|bench| self.action(bench))
        {
            out.push(Intent::Game(action));
        }
    }

    fn row(&mut self, list: &str, index: usize, _: &ViewModel, _: &mut Vec<Intent>) {
        if matches!(list, RECIPES | CARRIED) {
            self.chosen = Some(index);
        }
    }

    fn primary(&self, list: &str) -> Option<&'static str> {
        matches!(list, RECIPES | CARRIED).then_some("BTN_Accept")
    }

    fn back(&mut self, out: &mut Vec<Intent>) {
        out.push(Intent::Close);
    }

    fn first_focus(&self) -> Option<&str> {
        Some(if self.breaking { CARRIED } else { RECIPES })
    }
}
