//! The upgrade bench (a script's `ShowUpgradeScreen`), KOTOR 2's: the
//! bench (`upgradesel`) lists the party's upgradable items, all or of one
//! kind (ranged, lightsaber, melee, armour), with the chosen one's
//! description; Upgrade Items opens its slots (`upgrade`: as many as the
//! item has, on KOTOR 2's three, or its six for a lightsaber and an item
//! with more), where upgrades are fitted and taken out on a copy, made
//! only by Assemble (`Upgrade`).
//! Create Items opens TSL's workbench (`workbench`) where a script
//! opened the bench with item creation; else it is hidden. Back closes
//! each. Both hold their item by
//! its id: the list is the goods' as each frame has them, which another
//! seat's trade may reorder.

use std::collections::BTreeMap;

use fmt_odyssey::gui::Control;

use crate::intent::{Fitting, GameAction, Intent};
use crate::paint::ControlState;
use crate::screen::{Screen, ScreenId};
use crate::screens::pages::description;
use crate::screens::tab::{disabled, enabled, hidden, image, rows};
use crate::vm::{Entry, SceneView, UpgradeItem, UpgradeKind, ViewModel};

/// Each kind's tab.
const KINDS: [(&str, UpgradeKind); 4] = [
    ("BTN_RANGED", UpgradeKind::Ranged),
    ("BTN_LIGHTSABER", UpgradeKind::Lightsaber),
    ("BTN_MELEE", UpgradeKind::Melee),
    ("BTN_ARMOR", UpgradeKind::Armour),
];

const ITEMS: &str = "LB_UPGRADELIST";

fn back(tag: &str, out: &mut Vec<Intent>) -> bool {
    let back = tag == "BTN_BACK";
    if back {
        out.push(Intent::Close);
    }
    back
}

#[derive(Default)]
pub struct UpgradeSelect {
    /// The kind listed; none for all.
    kind: Option<UpgradeKind>,
    /// The chosen item's id.
    selected: Option<u32>,
}

impl UpgradeSelect {
    /// The player's items listed.
    fn listed<'a>(&self, vm: &'a ViewModel) -> Vec<&'a UpgradeItem> {
        vm.upgrades
            .iter()
            .filter(|item| self.kind.is_none_or(|kind| item.kind == kind))
            .collect()
    }

    /// The chosen item's row in the list, while it is listed.
    fn row_of(&self, listed: &[&UpgradeItem]) -> Option<usize> {
        listed
            .iter()
            .position(|item| Some(item.id) == self.selected)
    }

    /// The chosen item, while it is listed.
    fn chosen<'a>(&self, vm: &'a ViewModel) -> Option<&'a UpgradeItem> {
        let listed = self.listed(vm);
        self.row_of(&listed).map(|at| listed[at])
    }
}

impl Screen for UpgradeSelect {
    fn id(&self) -> ScreenId {
        ScreenId("upgrade_select")
    }

    fn layout(&self) -> &str {
        "upgradesel"
    }

    fn state(&self, control: &Control, vm: &ViewModel) -> ControlState {
        let tag = control.tag.as_str();
        if let Some((_, kind)) = KINDS.iter().find(|(button, _)| *button == tag) {
            return ControlState {
                selected: self.kind == Some(*kind),
                ..ControlState::default()
            };
        }
        let listed = self.listed(vm);
        let row = self.row_of(&listed);
        let chosen = row.map(|at| listed[at]);
        match tag {
            "BTN_ALL" => ControlState {
                selected: self.kind.is_none(),
                ..ControlState::default()
            },
            ITEMS => {
                let items: Vec<Entry> = listed.iter().map(|item| item.item.clone()).collect();
                rows(&items, row)
            }
            "LB_DESCRIPTION" => {
                description(&chosen.map_or(Vec::new(), |item| item.item.description.clone()))
            }
            "BTN_UPGRADEITEMS" if chosen.is_none() => disabled(),
            "BTN_CREATEITEMS" if vm.workbench.is_none() => hidden(),
            _ => ControlState::default(),
        }
    }

    fn press(&mut self, tag: &str, vm: &ViewModel, out: &mut Vec<Intent>) {
        if back(tag, out) {
            return;
        }
        if let Some((_, kind)) = KINDS.iter().find(|(button, _)| *button == tag) {
            self.kind = Some(*kind);
            self.selected = None;
        } else if tag == "BTN_ALL" {
            self.kind = None;
            self.selected = None;
        } else if let ("BTN_UPGRADEITEMS", Some(item)) = (tag, self.chosen(vm)) {
            out.push(Intent::OpenFor("upgrade", item.id.to_string()));
        } else if tag == "BTN_CREATEITEMS" && vm.workbench.is_some() {
            out.push(Intent::Open("workbench"));
        }
    }

    fn row(&mut self, list: &str, index: usize, vm: &ViewModel, _: &mut Vec<Intent>) {
        if list == ITEMS {
            self.selected = self.listed(vm).get(index).map(|item| item.id);
        }
    }

    fn primary(&self, list: &str) -> Option<&'static str> {
        (list == ITEMS).then_some("BTN_UPGRADEITEMS")
    }

    fn first_focus(&self) -> Option<&str> {
        Some(ITEMS)
    }
}

/// The slots' buttons: three for most items; a lightsaber's, and an
/// item's with more than three, on KOTOR 2's six (the rest hidden).
const SLOTS: [&str; 3] = ["BTN_UPGRADE31", "BTN_UPGRADE32", "BTN_UPGRADE33"];
const WIDE_SLOTS: [&str; 6] = [
    "BTN_UPGRADE31_LS",
    "BTN_UPGRADE32_LS",
    "BTN_UPGRADE33_LS",
    "BTN_UPGRADE34_LS",
    "BTN_UPGRADE35_LS",
    "BTN_UPGRADE36_LS",
];

/// The fitting list.
const FITTING: &str = "LB_ITEMS";

/// "This upgrade is incompatible with this item.": an upgrade of a level
/// above the item's chosen.
const INCOMPATIBLE: &str = "116713";

/// What a row of a slot's list does: take the upgrade out, keep what is
/// there, or put in the slot's choice of that index.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Pick {
    Out,
    Keep,
    In(usize),
}

/// The item's slots, as TSL's bench works on a copy: a slot chosen lists
/// an empty row (taking out what is there), what is in it, and what the
/// party carries that fits it (one of a level above the item's greyed,
/// refused with the game's line); a row chosen changes the copy and shows
/// the slots again. Assemble makes the visit's changes together
/// (`GameAction::Assemble`) and closes it; Back throws them away and
/// closes it (from a slot's list: the slots again). The list and the
/// item's model with its slots share one panel (KOTOR 2's layout).
pub struct Upgrade {
    /// The item's id; an item gone shows nothing.
    item: u32,
    slot: Option<usize>,
    /// A slot's list newly shown: its cursor from the top.
    anew: bool,
    /// The visit's changes by slot: an upgrade the party carries put in
    /// (by its choice's object id), or (`None`) what is there taken out.
    changes: BTreeMap<usize, Option<u32>>,
}

impl Upgrade {
    pub fn new(item: &str) -> Option<Upgrade> {
        Some(Upgrade {
            item: item.parse().ok()?,
            slot: None,
            anew: false,
            changes: BTreeMap::new(),
        })
    }

    /// The item, while the player's upgradable items hold it.
    fn item<'a>(&self, vm: &'a ViewModel) -> Option<&'a UpgradeItem> {
        vm.upgrades.iter().find(|item| item.id == self.item)
    }

    /// The slot buttons `item` uses, and the others.
    fn buttons(item: Option<&UpgradeItem>) -> (Vec<&'static str>, Vec<&'static str>) {
        let count = item.map_or(0, |item| item.slots.len());
        let saber = item.is_some_and(|item| item.kind == UpgradeKind::Lightsaber);
        let (set, other) = if saber || count > SLOTS.len() {
            (&WIDE_SLOTS[..], &SLOTS[..])
        } else {
            (&SLOTS[..], &WIDE_SLOTS[..])
        };
        let shown = set.iter().take(count).copied().collect();
        let unused = set.iter().skip(count).chain(other).copied().collect();
        (shown, unused)
    }

    /// What slot `at` of `item` holds on the copy.
    fn holds<'a>(&self, item: &'a UpgradeItem, at: usize) -> Option<&'a Entry> {
        let slot = item.slots.get(at)?;
        match self.changes.get(&at) {
            Some(Some(id)) => slot
                .choices
                .iter()
                .find(|choice| choice.id == *id)
                .map(|choice| &choice.entry),
            Some(None) => None,
            None => slot.fitted.as_ref().map(|(_, entry)| entry),
        }
    }

    /// The chosen slot's rows: the empty one (but for a colour crystal,
    /// which stays), what it holds, then the party's upgrades of its kind
    /// another slot of the visit does not take; how each shows and what
    /// choosing it does.
    fn rows(&self, item: &UpgradeItem) -> Vec<(Entry, bool, Pick)> {
        let Some((at, slot)) = self.slot.and_then(|at| Some((at, item.slots.get(at)?))) else {
            return Vec::new();
        };
        let stays = matches!(slot.fitted, Some((None, _)));
        let out = (!stays).then(|| (Entry::default(), true, Pick::Out));
        let kept = self
            .holds(item, at)
            .map(|entry| (entry.clone(), true, Pick::Keep));
        let taken = |id: u32| self.changes.values().any(|change| *change == Some(id));
        let choices = slot
            .choices
            .iter()
            .enumerate()
            .filter(|(_, choice)| !taken(choice.id))
            .map(|(index, choice)| (choice.entry.clone(), choice.fits, Pick::In(index)));
        out.into_iter().chain(kept).chain(choices).collect()
    }

    /// The row `index` of the chosen slot's list chosen: the copy changed,
    /// or the game's line for an upgrade the item does not take.
    fn pick(&mut self, item: &UpgradeItem, index: usize, out: &mut Vec<Intent>) {
        let Some(&(_, fits, pick)) = self.rows(item).get(index) else {
            return;
        };
        let (Some(at), true) = (self.slot, fits) else {
            out.push(Intent::OpenFor("message_box", INCOMPATIBLE.into()));
            return;
        };
        let fitted = item.slots[at].fitted.is_some();
        match pick {
            Pick::Keep => {}
            Pick::Out if fitted => {
                self.changes.insert(at, None);
            }
            Pick::Out => {
                self.changes.remove(&at);
            }
            Pick::In(choice) => {
                self.changes
                    .insert(at, Some(item.slots[at].choices[choice].id));
            }
        }
        self.slot = None;
    }

    /// The visit's changes as the host makes them, in slot order: an
    /// upgrade put in, what the slot holds taken out for it (TSL's by its
    /// slot, K1's by its row); or what it holds taken out.
    fn assembled(&self, item: &UpgradeItem) -> Vec<Fitting> {
        self.changes
            .iter()
            .filter_map(|(at, change)| {
                let slot = item.slots.get(*at)?;
                let row = slot.fitted.as_ref().and_then(|(row, _)| *row).or(slot.at);
                Some(match change {
                    Some(upgrade) => Fitting::In {
                        upgrade: *upgrade,
                        replace: row,
                    },
                    None => Fitting::Out(row?),
                })
            })
            .collect()
    }
}

impl Screen for Upgrade {
    fn id(&self) -> ScreenId {
        ScreenId("upgrade")
    }

    fn layout(&self) -> &str {
        "upgrade"
    }

    fn state(&self, control: &Control, vm: &ViewModel) -> ControlState {
        let item = self.item(vm);
        let saber = item.is_some_and(|item| item.kind == UpgradeKind::Lightsaber);
        let tag = control.tag.as_str();
        let listing = self.slot.is_some();
        let (shown, unused) = Upgrade::buttons(item);
        let slot_control = |buttons: &[&str]| {
            buttons
                .iter()
                .any(|button| tag == *button || tag == button.replace("BTN_", "LBL_"))
        };
        let panel = matches!(
            tag,
            "3D_MODEL" | "3D_MODEL_LS" | "LBL_SLOTNAME" | "LBL_PROPERTY"
        );
        if slot_control(&unused)
            || (listing && (panel || slot_control(&shown)))
            || (!listing && tag == FITTING)
        {
            return hidden();
        }
        if let Some(at) = shown.iter().position(|button| *button == tag) {
            let holds = item.and_then(|item| self.holds(item, at));
            return ControlState {
                selected: self.slot == Some(at),
                ..image(holds.and_then(|entry| entry.icon.clone()))
            };
        }
        match tag {
            "LB_DESC" => {
                description(&item.map_or(Vec::new(), |item| item.item.description.clone()))
            }
            "LBL_TITLE" => ControlState {
                text: Some(item.map_or(Vec::new(), |item| item.item.name.clone())),
                ..ControlState::default()
            },
            "3D_MODEL" if saber => hidden(),
            "3D_MODEL_LS" if !saber => hidden(),
            // Armour has no model of its own to show.
            "3D_MODEL" | "3D_MODEL_LS" => ControlState {
                scene: item
                    .filter(|item| item.kind != UpgradeKind::Armour)
                    .map(|item| SceneView::Item(item.template.clone())),
                ..ControlState::default()
            },
            FITTING => {
                let listed = item.map_or(Vec::new(), |item| self.rows(item));
                let entries: Vec<Entry> = listed.iter().map(|(entry, ..)| entry.clone()).collect();
                let mut state = rows(&entries, None);
                if let Some(list) = state.list.as_mut() {
                    for (row, (_, fits, _)) in list.rows.iter_mut().zip(&listed) {
                        row.dimmed = !fits;
                    }
                }
                state
            }
            "BTN_OK" => hidden(),
            "BTN_ASSEMBLE" => enabled(!listing && !self.changes.is_empty()),
            _ => ControlState::default(),
        }
    }

    fn press(&mut self, tag: &str, vm: &ViewModel, out: &mut Vec<Intent>) {
        if tag == "BTN_BACK" {
            self.back(out);
            return;
        }
        let item = self.item(vm);
        let (shown, _) = Upgrade::buttons(item);
        if let Some(at) = shown.iter().position(|button| *button == tag) {
            self.slot = Some(at);
            self.anew = true;
        } else if let ("BTN_ASSEMBLE", Some(item)) = (tag, item)
            && self.slot.is_none()
            && !self.changes.is_empty()
        {
            out.push(Intent::Game(GameAction::Assemble {
                item: item.id,
                changes: self.assembled(item),
            }));
            out.push(Intent::Close);
        }
    }

    fn back(&mut self, out: &mut Vec<Intent>) {
        if self.slot.take().is_none() {
            out.push(Intent::Close);
        }
    }

    fn row(&mut self, list: &str, index: usize, vm: &ViewModel, out: &mut Vec<Intent>) {
        if list == FITTING
            && let Some(item) = self.item(vm).cloned()
        {
            self.pick(&item, index, out);
        }
    }

    fn lists_anew(&mut self) -> bool {
        std::mem::take(&mut self.anew)
    }

    /// A slot's list, while it shows; else Back.
    fn first_focus(&self) -> Option<&str> {
        Some(if self.slot.is_some() {
            FITTING
        } else {
            "BTN_BACK"
        })
    }
}
