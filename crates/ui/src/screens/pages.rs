//! The in-game menu's pages on what the game has today: the player's own
//! character (its sheet), what it carries, the feedback log (the map's is
//! `map`). Using items waits for the world state, the party for its party
//! table: those controls show greyed. The pages are KOTOR 2's layouts (the
//! skin): the character's portrait, name, class, level and vitality are
//! the bar's.

use fmt_odyssey::gui::{Border, Control, Gui, Kind as ControlKind, Rect};

use crate::draw::{DrawList, Quad};
use crate::input::Key;
use crate::intent::{BarContent, BarEdit, GameAction, GuiSound, Intent};
use crate::paint::{ControlState, ListView, Painter, Row, bonus_colour, rect_of};
use crate::screen::Pointer;
use crate::screens::filter::{self, Filter};
use crate::screens::tab::{
    Page, disabled, enabled, hidden, image, rows, shown, signed, skin_line, text,
};
use crate::screens::{copy_beside, next_id};
use crate::skin::Source;
use crate::vm::{
    BarSetupView, BarSlot, CharacterView, Entry, Owned, Preview, SceneView, ViewModel, WheelEntry,
};

fn character(vm: &ViewModel) -> CharacterView {
    vm.character.clone().unwrap_or(CharacterView::default())
}

/// A description shown as the rows of a list fitted to it.
pub(crate) fn description(words: &[u8]) -> ControlState {
    ControlState {
        list: Some(ListView {
            rows: vec![Row {
                text: words.to_vec(),
                ..Row::default()
            }],
            fit: true,
            ..ListView::default()
        }),
        ..ControlState::default()
    }
}

/// An item's description box: its name, then `lead` (the equip page's
/// preview), then what it says (its stats and description,
/// `Entry::description`); empty for none.
fn described(entry: Option<&Entry>, lead: &[u8]) -> ControlState {
    description(&entry.map_or(Vec::new(), |entry| {
        let mut words = entry.name.clone();
        words.extend_from_slice(b"\n\n");
        words.extend_from_slice(lead);
        words.extend_from_slice(&entry.description);
        words
    }))
}

/// `described`, with `refused` (why the member may not wear it,
/// `Owned::refused`) over it in red: a remaster's line, the games show
/// only the red row.
fn described_refused(entry: Option<&Entry>, lead: &[u8], refused: Option<&[u8]>) -> ControlState {
    let mut state = described(entry, lead);
    if let (Some(list), Some(refused)) = (state.list.as_mut(), refused) {
        let mut line = refused.to_vec();
        line.extend_from_slice(b"\n");
        list.rows.insert(
            0,
            Row {
                text: line,
                refused: true,
                ..Row::default()
            },
        );
    }
    state
}

/// What putting an item on changes, as the equip page's numbers show it
/// (`Preview`): each that changes, "now -> then", a line each and a blank
/// line after; nothing when none does.
fn preview_lines(me: &CharacterView, then: &Preview) -> Vec<u8> {
    let mut lines = String::new();
    let mut change = |what: &str, now: String, then: String| {
        if now != then {
            lines.push_str(&format!("{what}: {now} -> {then}\n"));
        }
    };
    for (hand, side) in ["", "Off hand "].iter().enumerate() {
        change(
            &format!("{side}Attack"),
            signed(me.attack[hand]),
            signed(then.attack[hand]),
        );
        change(
            &format!("{side}Damage"),
            me.damage[hand].clone(),
            then.damage[hand].clone(),
        );
    }
    change("Defense", me.defence.to_string(), then.defence.to_string());
    if !lines.is_empty() {
        lines.push('\n');
    }
    lines.into_bytes()
}

/// Each weapon set's hands, main and off (`INVENTORY_SLOT_*`).
const HANDS: [[u8; 2]; 2] = [[4, 5], [18, 19]];

/// Why `owned` is red in `slot`'s list: why the member may not wear it
/// (`Owned::refused`), else, in an off hand's (K1's), that it does not go
/// with its main hand's weapon (`Owned::unpaired`), which the games show
/// only by the red row: a remaster's line, so it is not taken for a
/// proficiency the member has.
fn why_red(me: &CharacterView, slot: u8, owned: &Owned) -> Option<Vec<u8>> {
    if let Some(refused) = &owned.refused {
        return Some(refused.clone());
    }
    let set = HANDS.iter().position(|[_, off]| *off == slot)?;
    if !owned.unpaired[set] {
        return None;
    }
    let main = HANDS[set][0];
    let line: &[u8] = if me.worn.iter().any(|(at, _)| *at == main) {
        b"Off hand: only a one-handed weapon of the right hand's kind (melee or ranged)"
    } else {
        b"Off hand: equip a weapon in the right hand first"
    };
    Some(line.to_vec())
}

fn vitality(me: &CharacterView) -> String {
    format!("{}/{}", me.hp.0, me.hp.1)
}

/// The equip slots: their label, their button, the empty slot's icon,
/// the slot's name (a strref: the same in both games' talk tables; no
/// layout or table carries them, so the second set's hands are named as
/// the first's), its `INVENTORY_SLOT_*`.
const SLOTS: [(&str, &str, &str, u32, u8); 11] = [
    ("LBL_INV_IMPLANT", "BTN_INV_IMPLANT", "iimplant", 31388, 9),
    ("LBL_INV_HEAD", "BTN_INV_HEAD", "ihead", 31375, 0),
    ("LBL_INV_HANDS", "BTN_INV_HANDS", "ihands", 31383, 3),
    ("LBL_INV_ARM_L", "BTN_INV_ARM_L", "iforearm_l", 31376, 7),
    ("LBL_INV_BODY", "BTN_INV_BODY", "iarmor", 31380, 1),
    ("LBL_INV_ARM_R", "BTN_INV_ARM_R", "iforearm_r", 31377, 8),
    ("LBL_INV_WEAP_L", "BTN_INV_WEAP_L", "iweap_l", 31378, 5),
    ("LBL_INV_BELT", "BTN_INV_BELT", "ibelt", 31382, 10),
    ("LBL_INV_WEAP_R", "BTN_INV_WEAP_R", "iweap_r", 31379, 4),
    ("LBL_INV_WEAP_L2", "BTN_INV_WEAP_L2", "iweap_l", 31378, 19),
    ("LBL_INV_WEAP_R2", "BTN_INV_WEAP_R2", "iweap_r", 31379, 18),
];

/// The slot whose `equipableslots` bit a slot's items carry: the second
/// weapon set's hands (18, 19) the first's (4, 5), as TSL's equip screen
/// lists for them (`sim::inventory::slots::rules`).
fn rules(slot: u8) -> u8 {
    match slot {
        18 => 4,
        19 => 5,
        other => other,
    }
}

/// Whether `slot` is one the member's body has locked (`mask`,
/// `CharacterView::locked`; `sim::inventory::slots::locked`): a first-set
/// hand locks its second-set hand too.
fn locked(mask: u32, slot: u8) -> bool {
    let bit = |slot: u8| 1u32.checked_shl(u32::from(slot)).unwrap_or(0);
    mask & (bit(slot) | bit(rules(slot))) != 0
}

/// TSL's "You cannot equip or remove an item from this slot.", its equip
/// screen's answer to a locked slot chosen.
const LOCKED_SLOT: u32 = 125554;

/// Whether the equip page dresses the party member it shows: the
/// player's own, or one the bar marks (`MemberView::dresses`: the host's
/// companions no seat plays, I30 P14).
fn dresses(vm: &ViewModel) -> bool {
    vm.viewed == 0 || vm.party.get(vm.viewed).is_some_and(|member| member.dresses)
}

/// The equip page: a slot chosen lists what the party carries that fits
/// it; Equip wears the chosen one on the member shown, when this player
/// dresses it (what was there goes back to the party). A slot shows what
/// is worn in it, a hand without an item its weapon (a dev sheet's).
/// TSL's second weapon set has its two slots, and the swap button
/// exchanges the sets of the member shown, by the same rule; in both games
/// (KOTOR's own screen has one set: a remaster liberty there). The party's
/// arrows are hidden.
#[derive(Default)]
pub struct Equip {
    /// The slot looked at (its button and number), and the row chosen.
    slot: Option<(&'static str, u8)>,
    chosen: Option<usize>,
    /// The row the list's cursor is on: described while none is chosen.
    pointed: usize,
    /// The headgear eye has the focus: its tip shows.
    eye_focused: bool,
    /// The slot whose list the list's cursor last stood in: each slot's
    /// list (one list control) opens with its cursor at the top.
    cursor_in: Option<u8>,
    /// The controls under the description box (`under_desc_box`), told
    /// before the layout is stretched to the window, by tag and id: the
    /// menu bar's layout under the page has controls of the same tags (its
    /// own `LBL_BACK1`).
    under_desc: Vec<(String, i32)>,
}

/// The eye in the head slot's top right corner (a remaster's, kjk's own
/// button: KOTOR always draws what is worn on the head): it hides or shows
/// the member's headgear (`CharacterView::headgear_setting`), as drawn
/// only. Its icons are drawn here (`icons::eye_icon`), a fraction of the
/// slot wide; its tip shows while it has the focus.
const EYE: &str = "BTN_HEADGEAR";
const HEAD_SLOT: &str = "BTN_INV_HEAD";
const EYE_SHARE: i32 = 3;

impl Equip {
    /// Whether the member shown wears something on its head that the eye
    /// may hide, and whether it is hidden.
    fn eye(vm: &ViewModel) -> Option<(String, bool)> {
        let me = vm.character.as_ref()?;
        let setting = me.headgear_setting.clone()?;
        me.worn.iter().any(|(slot, _)| *slot == 0).then_some(())?;
        let hidden = vm
            .settings
            .get(&setting)
            .is_some_and(|value| value == "true");
        Some((setting, hidden))
    }
}

/// Where TSL's `equip_p` keeps its description box (`LB_DESC`) in its
/// 800 × 600 frame: over the slots, which it hides while a slot's list is
/// open.
const DESC_BOX: [i32; 4] = [410, 134, 313, 336];

/// Whether `control`'s centre lies within the description box (`DESC_BOX`),
/// in the layout as read (`Page::fit_read`, before the stretch).
fn under_desc_box(control: &Control) -> bool {
    let [left, top, width, height] = DESC_BOX;
    let extent = control.extent;
    let (x, y) = (
        extent.left + extent.width / 2,
        extent.top + extent.height / 2,
    );
    (left..left + width).contains(&x) && (top..top + height).contains(&y)
}

/// A row of a slot's list (`Equip::listed`).
#[derive(Clone, PartialEq)]
enum Listed {
    /// "None": takes off what the slot holds.
    Off,
    /// What the slot holds, "(Equipped)": choosing it changes nothing.
    Worn(Owned),
    /// What the party carries that may go in the slot.
    Carried(Owned),
}

/// What a worn row's name ends with.
const EQUIPPED: &[u8] = b"\n(Equipped)";

impl Equip {
    /// The slot looked at's list: when it holds an item, a "None" row (it
    /// takes it off; a hand's weapon without an item, a dev sheet's, cannot
    /// be) and that item marked equipped; then what the party carries that
    /// goes in it and is for the member's race (`Owned::unfit`), as both
    /// games' equip screens list (`CSWGuiInGameEquip`): those it may not
    /// wear among them (`refused`).
    ///
    /// With KOTOR 2's feedback option "Hide Unequippable" on, the items the
    /// member may not wear are left out instead of drawn red.
    fn listed(&self, me: &CharacterView, vm: &ViewModel) -> Vec<Listed> {
        let Some((_, slot)) = self.slot else {
            return Vec::new();
        };
        let hide = crate::feedback::on(&vm.settings, crate::feedback::HIDE_UNEQUIPPABLE);
        let worn = me.worn.iter().find(|(at, _)| *at == slot);
        let fitting = me.owned.iter().filter(|owned| {
            owned.slots & 1 << rules(slot) != 0
                && !owned.unfit
                && !(hide && why_red(me, slot, owned).is_some())
        });
        worn.into_iter()
            .flat_map(|(_, owned)| [Listed::Off, Listed::Worn(owned.clone())])
            .chain(fitting.cloned().map(Listed::Carried))
            .collect()
    }

    /// The chosen row, when choosing it changes something: `Some(None)` for
    /// the "None" row, else the item to wear.
    fn choice(&self, me: &CharacterView, vm: &ViewModel) -> Option<Option<Owned>> {
        match self.listed(me, vm).get(self.chosen?)? {
            Listed::Off => Some(None),
            Listed::Worn(_) => None,
            Listed::Carried(owned) => Some(Some(owned.clone())),
        }
    }
}

impl Page for Equip {
    const NAME: &'static str = "equip";
    const LAYOUT: &'static str = "equip";

    fn state(&self, control: &Control, vm: &ViewModel) -> ControlState {
        let me = character(vm);
        let tag = control.tag.as_str();
        if self.slot.is_some() {
            if tag == "LB_DESC" {
                // The "None" row tells what it would take off.
                let row = self.chosen.unwrap_or(self.pointed);
                let worn = |slot| me.worn.iter().find(|(at, _)| *at == slot);
                let listed = self.listed(&me, vm).into_iter().nth(row);
                let item = match &listed {
                    Some(Listed::Carried(owned) | Listed::Worn(owned)) => Some(owned.clone()),
                    Some(Listed::Off) => self
                        .slot
                        .and_then(|(_, slot)| worn(slot))
                        .map(|(_, owned)| owned.clone()),
                    None => None,
                };
                // What it would change: none for what is worn already.
                let lead = item
                    .as_ref()
                    .filter(|_| !matches!(listed, Some(Listed::Worn(_))))
                    .zip(self.slot)
                    .and_then(|(owned, (_, slot))| {
                        owned.previews.iter().find(|preview| preview.slot == slot)
                    })
                    .map_or(Vec::new(), |then| preview_lines(&me, then));
                // Why only for a row that would put it on.
                let refused = match (&listed, self.slot) {
                    (Some(Listed::Carried(owned)), Some((_, slot))) => why_red(&me, slot, owned),
                    _ => item.as_ref().and_then(|owned| owned.refused.clone()),
                };
                return described_refused(
                    item.as_ref().map(|owned| &owned.entry),
                    &lead,
                    refused.as_deref(),
                );
            }
            if self
                .under_desc
                .iter()
                .any(|(tag, id)| *tag == control.tag && *id == control.id)
            {
                return hidden();
            }
        }
        if let Some((.., empty, _, slot)) = SLOTS.iter().find(|(label, ..)| *label == tag) {
            let worn = me
                .worn
                .iter()
                .find(|(at, _)| at == slot)
                .map(|(_, owned)| &owned.entry);
            let hand = match slot {
                4 => me.weapons[0].as_ref(),
                5 => me.weapons[1].as_ref(),
                _ => None,
            };
            return image(Some(
                worn.or(hand)
                    .and_then(|entry| entry.icon.clone())
                    .unwrap_or_else(|| empty.to_string()),
            ));
        }
        // A locked slot's button in red, as TSL's equip screen draws it.
        if let Some((.., slot)) = SLOTS.iter().find(|(_, button, ..)| *button == tag) {
            return ControlState {
                refused: locked(me.locked, *slot),
                ..ControlState::default()
            };
        }
        match tag {
            EYE => match Equip::eye(vm).filter(|_| self.slot.is_none()) {
                Some((_, hidden)) => image(Some(crate::icons::eye_texture(!hidden))),
                None => hidden(),
            },
            "LBL_SLOTNAME" => ControlState {
                strref: self.slot.and_then(|(slot, _)| {
                    SLOTS
                        .iter()
                        .find(|(_, button, ..)| *button == slot)
                        .map(|(.., strref, _)| *strref)
                }),
                skin_strref: true,
                text: self.slot.is_none().then(Vec::new),
                ..ControlState::default()
            },
            "LB_ITEMS" if self.slot.is_some() => {
                let none = Entry {
                    name: b"None".to_vec(),
                    ..Entry::default()
                };
                let slot = self.slot.map_or(u8::MAX, |(_, slot)| slot);
                // In red, as both games' `CSWGuiInGameItemEntry::SetCanUse`
                // draws one the member may not wear, or (K1's) an off hand's
                // that does not go with its main hand's weapon.
                let refused = |owned: &Owned| why_red(&me, slot, owned).is_some();
                let (entries, red): (Vec<Entry>, Vec<bool>) = self
                    .listed(&me, vm)
                    .into_iter()
                    .map(|row| match row {
                        Listed::Carried(owned) => (owned.entry.clone(), refused(&owned)),
                        Listed::Worn(owned) => {
                            let mut entry = owned.entry.clone();
                            entry.name.extend_from_slice(EQUIPPED);
                            (entry, false)
                        }
                        Listed::Off => (none.clone(), false),
                    })
                    .unzip();
                let mut state = rows(&entries, self.chosen);
                if let Some(list) = state.list.as_mut() {
                    // The worn row's mark goes on a line of its own; the
                    // rows are still chosen.
                    list.fit = true;
                    list.pick = true;
                    for (row, red) in list.rows.iter_mut().zip(red) {
                        row.refused = red;
                    }
                }
                state
            }
            "LB_ITEMS" => {
                let entries: Vec<Entry> =
                    me.owned.iter().map(|owned| owned.entry.clone()).collect();
                rows(&entries, None)
            }
            "LB_DESC" | "LBL_CANTEQUIP" | "BTN_PREVNPC" | "BTN_NEXTNPC" => hidden(),
            "BTN_SWAPWEAPONS" => enabled(dresses(vm)),
            "LBL_DEF" => text(me.defence.to_string()),
            "LBL_ATKR" => text(&me.damage[0]),
            "LBL_ATKL" => text(&me.damage[1]),
            "LBL_TOHITR" => text(signed(me.attack[0])),
            "LBL_TOHITL" => text(signed(me.attack[1])),
            "BTN_EQUIP" => enabled(dresses(vm) && self.choice(&me, vm).is_some()),
            _ => ControlState::default(),
        }
    }

    fn press(&mut self, tag: &str, vm: &ViewModel, out: &mut Vec<Intent>) {
        if tag == EYE {
            if let Some((setting, hidden)) = Equip::eye(vm) {
                out.push(Intent::Setting(setting, (!hidden).to_string()));
            }
        } else if let Some((_, button, .., slot)) =
            SLOTS.iter().find(|(_, button, ..)| *button == tag)
        {
            // A locked slot does not open: TSL says why.
            if locked(character(vm).locked, *slot) {
                out.push(Intent::OpenFor("message_box", LOCKED_SLOT.to_string()));
                return;
            }
            self.slot = Some((button, *slot));
            self.chosen = None;
            self.pointed = 0;
        } else if tag == "BTN_BACK" && self.slot.is_some() {
            // Back from a slot's list to the slots, as TSL's.
            self.slot = None;
            self.chosen = None;
        } else if tag == "BTN_EQUIP" && dresses(vm) {
            let me = character(vm);
            if let (Some((_, slot)), Some(choice)) = (self.slot, self.choice(&me, vm)) {
                // K1's equip screen: an item worn, `Inventory_Select`; one
                // taken off, `Inventory_Drop`.
                out.push(Intent::Sound(if choice.is_some() {
                    GuiSound::InventorySelect
                } else {
                    GuiSound::InventoryDrop
                }));
                out.push(Intent::Game(match choice {
                    Some(owned) => GameAction::Equip {
                        who: me.id,
                        item: owned.item,
                        slot,
                    },
                    None => GameAction::Unequip { who: me.id, slot },
                }));
                self.slot = None;
                self.chosen = None;
            }
        } else if tag == "BTN_SWAPWEAPONS" && dresses(vm) {
            out.push(Intent::Game(GameAction::SwitchWeapons {
                who: character(vm).id,
            }));
        } else if tag == "BTN_BACK" {
            out.push(Intent::CloseAll);
        }
    }

    fn row(&mut self, _: &str, index: usize, _: &ViewModel) {
        if self.slot.is_some() {
            self.chosen = Some(index);
        }
    }

    /// As in both games, the row the pointer or the cursor comes to is the
    /// chosen one: only it is lit (as the inventory's, `Inventory`).
    fn pointed(&mut self, list: &str, index: usize, _: &ViewModel) {
        if list == "LB_ITEMS" {
            self.pointed = index;
            if self.slot.is_some() {
                self.chosen = Some(index);
            }
        }
    }

    /// A double click on a slot takes off what is worn in it; on an item of
    /// its list, wears it (`primary`).
    fn double(&mut self, tag: &str, vm: &ViewModel, out: &mut Vec<Intent>) {
        let me = character(vm);
        if SLOTS.iter().any(|(_, button, ..)| *button == tag)
            && self.listed(&me, vm).first() == Some(&Listed::Off)
        {
            self.chosen = Some(0);
            self.press("BTN_EQUIP", vm, out);
        }
    }

    fn primary(&self, list: &str) -> Option<&'static str> {
        (list == "LB_ITEMS").then_some("BTN_EQUIP")
    }

    fn lists_anew(&mut self) -> bool {
        let slot = self.slot.map(|(_, slot)| slot);
        std::mem::replace(&mut self.cursor_in, slot) != slot
    }

    /// The slots, else, while a slot's list is open over them, the list.
    fn first_focus(&self) -> Option<&str> {
        Some(if self.slot.is_some() {
            "LB_ITEMS"
        } else {
            "BTN_INV_BODY"
        })
    }

    fn focused(&mut self, tag: &str, _: &ViewModel) {
        self.eye_focused = tag == EYE;
    }

    fn left(&mut self, tag: &str) {
        if tag == EYE {
            self.eye_focused = false;
        }
    }

    /// The eye's tip, over its top right corner while it has the focus.
    /// KOTOR 2's feedback option "Enable Tool Tips" off shows no tip.
    fn quads(&self, gui: &Gui, vm: &ViewModel, painter: &mut Painter) -> Vec<Quad> {
        let tips = crate::feedback::on(&vm.settings, crate::feedback::TOOLTIPS);
        let (Some((_, hidden)), Some(eye), true) =
            (Equip::eye(vm), gui.find(EYE), self.eye_focused && tips)
        else {
            return Vec::new();
        };
        let words: &[u8] = if hidden {
            b"Show headgear"
        } else {
            b"Hide headgear"
        };
        let [l, t, w, _] = rect_of(eye);
        let mut out = DrawList::default();
        painter.tooltip(words, [l + w, t], &mut out);
        out.quads
    }

    /// The eye, a square a share of the head slot wide in its top right
    /// corner, last so the pointer finds it over the slot.
    fn shape(&self, gui: &mut Gui) {
        let Some(slot) = gui.find(HEAD_SLOT).cloned() else {
            return;
        };
        let side = slot.extent.width.min(slot.extent.height) / EYE_SHARE;
        let id = gui
            .controls
            .iter()
            .map(|control| control.id)
            .max()
            .unwrap_or(0)
            + 1;
        let mut eye = Control {
            id,
            tag: EYE.into(),
            border: None,
            hilight: None,
            selected: None,
            hilight_selected: None,
            text: None,
            moveto: None,
            children: Vec::new(),
            ..slot
        };
        eye.extent.left += eye.extent.width - side;
        eye.extent.width = side;
        eye.extent.height = side;
        gui.controls.push(eye);
    }

    fn fit_read(&mut self, gui: &Gui) {
        self.under_desc = gui
            .walk()
            .filter(|control| under_desc_box(control))
            .map(|control| (control.tag.clone(), control.id))
            .collect();
    }
}

/// The inventory page: the party's items, of the kind its filter shows;
/// Use Item uses the chosen one on the member shown (KOTOR's: the items
/// used on oneself, a medpac or a stim), when this player may.
///
/// As in both games, the row the pointer or the cursor comes to is the
/// chosen one (KOTOR 2's lists select the row under the mouse): only it is
/// lit, and its description shows. It stays chosen when the pointer leaves
/// the list.
pub struct Inventory {
    filter: Filter,
    selected: Option<usize>,
    /// The row the list's cursor is on: described while none is chosen.
    pointed: usize,
    /// The item it opened on (`at_datapad`), chosen until the player
    /// chooses another row or filter.
    wanted: Option<u32>,
    /// The list's cursor was put on the wanted item (`cursor_to`).
    placed: bool,
}

impl Default for Inventory {
    fn default() -> Inventory {
        Inventory {
            filter: Filter::new(filter::INVENTORY),
            selected: None,
            pointed: 0,
            wanted: None,
            placed: false,
        }
    }
}

impl Inventory {
    /// The page opened on datapad `item` (a picked-up datapad's box read
    /// with the journal key): the Datapads filter chosen, the datapad
    /// chosen in its list, its words in the description.
    pub fn at_datapad(item: u32) -> Inventory {
        let mut page = Inventory {
            wanted: Some(item),
            ..Inventory::default()
        };
        page.filter.press("BTN_DATAPADS");
        page
    }

    /// The chosen row: the player's, else the item it opened on.
    fn chosen(&self, me: &CharacterView) -> Option<usize> {
        self.selected.or_else(|| {
            let wanted = self.wanted?;
            self.filter
                .shown(&me.owned, |owned| &owned.entry)
                .iter()
                .position(|owned| owned.item == wanted)
        })
    }

    /// The chosen item, when this player may use it on the member shown.
    fn usable(&self, vm: &ViewModel, me: &CharacterView) -> Option<u32> {
        let listed = self.filter.shown(&me.owned, |owned| &owned.entry);
        let owned = listed.get(self.chosen(me)?)?;
        (owned.usable && dresses(vm)).then_some(owned.item)
    }
}

impl Page for Inventory {
    fn tabs(&self) -> Vec<&'static str> {
        self.filter.tags()
    }

    const NAME: &'static str = "inventory";
    const LAYOUT: &'static str = "inventory";

    fn state(&self, control: &Control, vm: &ViewModel) -> ControlState {
        let me = character(vm);
        let tag = control.tag.as_str();
        if let Some(state) = self.filter.state(tag) {
            return state;
        }
        let items: Vec<Entry> = self
            .filter
            .shown(&me.owned, |owned| &owned.entry)
            .into_iter()
            .map(|owned| owned.entry.clone())
            .collect();
        match tag {
            "LB_ITEMS" => rows(&items, self.chosen(&me)),
            "LB_DESCRIPTION" => described(items.get(self.chosen(&me).unwrap_or(self.pointed)), &[]),
            "LBL_CREDITS_VALUE" => text(me.credits),
            "BTN_USEITEM" => enabled(self.usable(vm, &me).is_some()),
            _ => ControlState::default(),
        }
    }

    fn press(&mut self, tag: &str, vm: &ViewModel, out: &mut Vec<Intent>) {
        if self.filter.press(tag) {
            self.selected = None;
            self.wanted = None;
            self.pointed = 0;
        } else if tag == "BTN_USEITEM" {
            let me = character(vm);
            if let Some(item) = self.usable(vm, &me) {
                out.push(Intent::Game(GameAction::UseItem { who: me.id, item }));
            }
        }
    }

    fn row(&mut self, _: &str, index: usize, _: &ViewModel) {
        self.selected = Some(index);
        self.wanted = None;
    }

    fn pointed(&mut self, list: &str, index: usize, _: &ViewModel) {
        if list == "LB_ITEMS" {
            self.pointed = index;
            self.selected = Some(index);
            self.wanted = None;
        }
    }

    fn cursor_to(&mut self, vm: &ViewModel) -> Option<(&'static str, usize)> {
        if self.placed || self.wanted.is_none() {
            return None;
        }
        let row = self.chosen(&character(vm))?;
        self.placed = true;
        Some(("LB_ITEMS", row))
    }

    fn primary(&self, list: &str) -> Option<&'static str> {
        (list == "LB_ITEMS").then_some("BTN_USEITEM")
    }

    fn first_focus(&self) -> Option<&str> {
        Some("LB_ITEMS")
    }
}

/// The character page: the sheet, and kjk's own Stance under the member
/// (`STANCE`), with its tip while it has the focus.
#[derive(Default)]
pub struct Character {
    stance_focused: bool,
}

/// The character page's Stance: the stance set the member stands in or
/// waits for (`CharacterView::stance`). A press steps the member's saved
/// pick (`CharacterView::stance_setting`) through its row's set and every
/// skeleton offered (`ViewModel::stances`), and the game sends the pick to
/// the host. It is greyed, a press doing nothing, unless the session plays
/// by MDA and its level has stance sets.
const STANCE: &str = "BTN_STANCE";

/// The gap between Close and Stance, and how far Stance reaches: to the
/// member's view's right edge.
const STANCE_GAP: i32 = 10;

impl Character {
    /// Whether stances may be picked: the session plays by MDA, and its
    /// level offers stance sets.
    fn stances_on(vm: &ViewModel) -> bool {
        vm.session_rules.as_deref() == Some(crate::vm::DUEL_RULES) && !vm.stances.is_empty()
    }

    /// The member's pick among `""` (its row's) and the skeletons offered:
    /// where it is, and the choices.
    fn picks<'v>(vm: &'v ViewModel, key: &str) -> (usize, Vec<&'v str>) {
        let choices: Vec<&str> = std::iter::once("")
            .chain(vm.stances.iter().map(|(folder, _)| folder.as_str()))
            .collect();
        let set = vm.settings.get(key).map_or("", String::as_str);
        let at = choices
            .iter()
            .position(|folder| *folder == set)
            .unwrap_or(0);
        (at, choices)
    }

    /// Stance's words: the name of the set the member stands in or waits
    /// for, or `STANCE_OWN` while it stands in none.
    fn stance(me: &CharacterView) -> String {
        let name = me.stance.as_deref().unwrap_or(crate::strings::STANCE_OWN);
        format!("{}: {name}", crate::strings::STANCE)
    }
}

/// The skills page's row names, which KOTOR 2's code sets on its layout's
/// labels: "Skill Rank", "Bonus" and "Total Rank".
const SKILL_RANK: u32 = 1579;
const SKILL_BONUS: u32 = 32129;
const SKILL_TOTAL: u32 = 41904;

/// The six scores' labels, in the sheet's order.
const SCORES: [(&str, &str); 6] = [
    ("LBL_STR", "LBL_STR_MOD"),
    ("LBL_DEX", "LBL_DEX_MOD"),
    ("LBL_CON", "LBL_CON_MOD"),
    ("LBL_INT", "LBL_INT_MOD"),
    ("LBL_WIS", "LBL_WIS_MOD"),
    ("LBL_CHA", "LBL_CHA_MOD"),
];

impl Page for Character {
    const NAME: &'static str = "character";
    const LAYOUT: &'static str = "character";

    fn state(&self, control: &Control, vm: &ViewModel) -> ControlState {
        let me = character(vm);
        let tag = control.tag.as_str();
        // Each score as items and effects leave it, and its modifier, both
        // coloured by how far they moved the character's own score, as
        // KOTOR 2's character screen does (`bonus_colour`).
        for (at, (score, modifier)) in SCORES.iter().enumerate() {
            let value = i32::from(me.abilities[at]);
            let shown = if tag == *score {
                value.to_string()
            } else if tag == *modifier {
                signed((value - 10).div_euclid(2))
            } else {
                continue;
            };
            return ControlState {
                tint: Some(bonus_colour(value - i32::from(me.own_abilities[at]))),
                ..text(shown)
            };
        }
        match tag {
            "SLD_ALIGN" => ControlState {
                value: Some(f32::from(me.alignment.min(100)) / 100.0),
                ..ControlState::default()
            },
            "BTN_LEVELUP" if me.may_level => ControlState::default(),
            "BTN_AUTO" if me.auto_level.is_some() => ControlState::default(),
            "LBL_NEEDED_XP" | "LBL_NEXT_LEVEL" => match me.next_xp {
                Some(xp) if tag == "LBL_NEEDED_XP" => text(xp.to_string()),
                Some(_) => ControlState::default(),
                None => hidden(),
            },
            "BTN_LEVELUP" | "BTN_AUTO" | "LBL_FORCEMASTERY" | "BTN_3DCHAR" => hidden(),
            "LBL_3DCHAR" => ControlState {
                scene: Some(SceneView::Member {
                    nth: vm.viewed,
                    alignment: me.alignment,
                }),
                ..ControlState::default()
            },
            STANCE => match &me.stance_setting {
                Some(_) => ControlState {
                    greyed: !Character::stances_on(vm),
                    ..text(Character::stance(&me))
                },
                None => hidden(),
            },
            "LBL_VITALITY_STAT" => text(vitality(&me)),
            "LBL_DEFENSE_STAT" => text(me.defence.to_string()),
            "LBL_FORCE_STAT" => text(format!("{}/{}", me.fp.0, me.fp.1)),
            "LBL_FORTITUDE_STAT" => text(signed(me.saves[0])),
            "LBL_REFLEX_STAT" => text(signed(me.saves[1])),
            "LBL_WILL_STAT" => text(signed(me.saves[2])),
            "LBL_EXPERIENCE_STAT" => text(me.xp.to_string()),
            _ => ControlState::default(),
        }
    }

    fn press(&mut self, tag: &str, vm: &ViewModel, out: &mut Vec<Intent>) {
        if tag == "BTN_LEVELUP" && character(vm).may_level {
            out.push(Intent::Open("levelup"));
        }
        if tag == STANCE
            && Character::stances_on(vm)
            && let Some(key) = character(vm).stance_setting
        {
            let (at, choices) = Character::picks(vm, &key);
            let next = choices[(at + 1) % choices.len()];
            out.push(Intent::Setting(key, next.to_string()));
        }
        if tag == "BTN_AUTO"
            && let me = character(vm)
            && let Some(auto) = me.auto_level
        {
            out.push(Intent::Game(GameAction::LevelUp {
                who: me.id,
                class: auto.class,
                ability: auto.ability,
                skills: auto.skills,
                feats: auto.feats,
                powers: auto.powers,
            }));
        }
    }

    fn focused(&mut self, tag: &str, _: &ViewModel) {
        self.stance_focused = tag == STANCE;
    }

    fn left(&mut self, tag: &str) {
        if tag == STANCE {
            self.stance_focused = false;
        }
    }

    /// Stance's tip over its top right corner while it has the focus: what
    /// it is, or what it needs.
    fn quads(&self, gui: &Gui, vm: &ViewModel, painter: &mut Painter) -> Vec<Quad> {
        let (Some(stance), true, Some(_)) = (
            gui.find(STANCE),
            self.stance_focused,
            character(vm).stance_setting,
        ) else {
            return Vec::new();
        };
        let words = if Character::stances_on(vm) {
            crate::strings::STANCE_HELP
        } else {
            crate::strings::STANCE_NEEDS
        };
        let [l, t, w, _] = rect_of(stance);
        let mut out = DrawList::default();
        painter.tooltip(words.as_bytes(), [l + w, t], &mut out);
        out.quads
    }

    /// Stance: a copy of Close beside it, reaching to the member's view's
    /// right edge, the focus going between them.
    fn shape(&self, gui: &mut Gui) {
        let Some(view) = gui.find("LBL_3DCHAR") else {
            return;
        };
        let right = view.extent.left + view.extent.width;
        if let Some(stance) = copy_beside(gui, "BTN_EXIT", STANCE, STANCE_GAP) {
            stance.extent.width = right - stance.extent.left;
        }
    }
}

/// What the abilities page lists.
#[derive(Default, Clone, Copy, PartialEq, Eq)]
enum Kind {
    #[default]
    Skills,
    Powers,
    Feats,
}

#[derive(Default)]
pub struct Abilities {
    kind: Kind,
    selected: Option<usize>,
}

impl Abilities {
    fn entries(&self, me: &CharacterView) -> Vec<Entry> {
        match self.kind {
            Kind::Skills => me.skills.clone(),
            Kind::Powers => me.powers.clone(),
            Kind::Feats => me.feats.clone(),
        }
    }
}

impl Page for Abilities {
    fn tabs(&self) -> Vec<&'static str> {
        vec!["BTN_SKILLS", "BTN_POWERS", "BTN_FEATS"]
    }

    const NAME: &'static str = "abilities";
    const LAYOUT: &'static str = "abilities";

    fn state(&self, control: &Control, vm: &ViewModel) -> ControlState {
        let me = character(vm);
        let entries = self.entries(&me);
        let chosen = self.selected.and_then(|at| entries.get(at));
        // The chosen skill's own ranks and its total (the skills list is in
        // `skills.2da` order), as KOTOR 2's skills page shows them with the
        // bonus between (`CSWGuiInGameAbilities::OnEnterSkill`).
        let rank = self
            .selected
            .filter(|_| self.kind == Kind::Skills && chosen.is_some())
            .and_then(|at| Some((*me.own_skills.get(at)?, *me.skill_totals.get(at)?)));
        let feats = self.kind == Kind::Feats;
        let told = || description(&chosen.map_or(Vec::new(), |entry| entry.description.clone()));
        let pick = |kind: Kind| ControlState {
            selected: self.kind == kind,
            ..ControlState::default()
        };
        match control.tag.as_str() {
            "LB_ABILITY" => rows(&entries, self.selected),
            "LBL_NAME" => ControlState {
                text: Some(chosen.map_or(Vec::new(), |entry| entry.name.clone())),
                ..ControlState::default()
            },
            // Feats' descriptions have the taller box; skills' the ranks under.
            "LB_DESC" if !feats => told(),
            "LB_DESC_FEATS" if feats => told(),
            "LB_DESC" | "LB_DESC_FEATS" => hidden(),
            "LBL_INFOBG" if rank.is_none() => hidden(),
            "LBL_RANKVAL" | "LBL_BONUSVAL" | "LBL_TOTALVAL" => match rank {
                Some((rank, _)) if control.tag == "LBL_RANKVAL" => text(rank.to_string()),
                Some((rank, total)) if control.tag == "LBL_BONUSVAL" => {
                    let bonus = total - i32::from(rank);
                    ControlState {
                        tint: Some(bonus_colour(bonus)),
                        ..text(signed(bonus))
                    }
                }
                Some((_, total)) => text(total.to_string()),
                None => hidden(),
            },
            "LBL_SKILLRANK" | "LBL_BONUS" | "LBL_TOTAL" if rank.is_none() => hidden(),
            // KOTOR 2's layout has its powers' words here ("Base Cost",
            // "Dark Side"). Its code names the skills' rows itself.
            "LBL_SKILLRANK" => skin_line(SKILL_RANK),
            "LBL_BONUS" => skin_line(SKILL_BONUS),
            "LBL_TOTAL" => skin_line(SKILL_TOTAL),
            "BTN_SKILLS" => pick(Kind::Skills),
            "BTN_POWERS" => pick(Kind::Powers),
            "BTN_FEATS" => pick(Kind::Feats),
            BAR_SETUP => text(BAR_SETUP_WORDS),
            _ => ControlState::default(),
        }
    }

    fn press(&mut self, tag: &str, _: &ViewModel, out: &mut Vec<Intent>) {
        let kind = match tag {
            "BTN_SKILLS" => Kind::Skills,
            "BTN_POWERS" => Kind::Powers,
            "BTN_FEATS" => Kind::Feats,
            BAR_SETUP => {
                out.push(Intent::Swap(<ActionBar as Page>::NAME));
                return;
            }
            _ => return,
        };
        if kind != self.kind {
            self.kind = kind;
            self.selected = None;
        }
    }

    fn row(&mut self, list: &str, index: usize, vm: &ViewModel) {
        self.pointed(list, index, vm);
    }

    /// The row the pointer or the cursor comes to is chosen, as in KOTOR 2.
    fn pointed(&mut self, list: &str, index: usize, _: &ViewModel) {
        if list == "LB_ABILITY" {
            self.selected = Some(index);
        }
    }

    fn first_focus(&self) -> Option<&str> {
        Some("LB_ABILITY")
    }

    /// The button that opens the action bar setup page, beside Close.
    fn shape(&self, gui: &mut Gui) {
        copy_beside(gui, "BTN_EXIT", BAR_SETUP, BAR_SETUP_GAP);
    }
}

/// The abilities page's button that opens the action bar setup page, its
/// words, and its gap from Close beside it (layout pixels). The button shows
/// no key, so a player who left the setup page's key unbound sees no empty
/// key box.
const BAR_SETUP: &str = "BTN_ACTIONBAR";
const BAR_SETUP_WORDS: &str = "Action bar";
const BAR_SETUP_GAP: i32 = 10;

/// The setup page's groups of icons, in the order they stand on the page,
/// each with its label. Group 0 is the Force powers, group 1 the feat
/// attacks and group 2 the items, mines and stealth mode.
const GROUPS: [&str; 3] = ["Force powers", "Feat attacks", "Items"];

/// The setup page's slot frames and the icons in them. Slot `n` is
/// `BTN_BARSLOTn` and its icon `LBL_BARSLOTn`, from `n` = 0.
const SLOT: &str = "BTN_BARSLOT";
const SLOT_ICON: &str = "LBL_BARSLOT";
/// The grid's icons. Icon `i` of group `g` is the frame `BTN_PICKg_i` with
/// the icon `LBL_PICKg_i` in it, from 0.
const PICK: &str = "BTN_PICK";
const PICK_ICON: &str = "LBL_PICK";
/// Group `g`'s label is `LBL_GROUPg`.
const GROUP_LABEL: &str = "LBL_GROUP";
/// How many slots the setup page shows. It shows all six on a controller
/// too, although a controller uses only the first four.
const SETUP_SLOTS: i32 = 6;
/// KOTOR 2's HUD action slot (`BTN_ACTION0` of its HUD layout), which the
/// setup page's slots copy. The first picture is the frame, and the second
/// is the frame while it has the focus. The size is the frame's width and
/// height. The icon is the icon's left, top and side inside the frame. The
/// sizes are in the HUD layout's pixels, and the setup page scales them.
const HUD_SLOT: (&str, &str) = ("lbl_mibox00", "lbl_mibox00a");
const HUD_SLOT_SIZE: [i32; 2] = [36, 60];
const HUD_SLOT_ICON: [i32; 3] = [3, 15, 30];
/// A setup slot's width, and the gap between two slots (layout pixels).
/// The six slots and their gaps are as wide as `PICK_COLUMNS` grid icons,
/// so the slot row and the grid share both edges.
const SLOT_WIDE: i32 = 38;
const SLOT_GAP: i32 = 12;
/// KOTOR 2's square item frame (the equip page's `BTN_INV_HEAD`): its
/// corner and edge pictures, its thickness, and its colour at rest and
/// with the focus. Each icon of the grid stands in one.
const PICK_FRAME: (&str, &str, i32) = ("uibit_brdr_16wc", "uibit_brdr_16we", 16);
const PICK_COLOR: [[f32; 3]; 2] = [
    [0.050_980_393, 0.349_019_62, 0.270_588_25],
    [0.698_039_23, 0.698_039_23, 0.6],
];
/// A grid frame's side and the icon's inset in its frame (layout pixels).
/// A frame is about the size of a HUD slot's icon. The frames of a group
/// touch, with no gap between them.
const PICK_SIDE: i32 = 32;
const PICK_INSET: i32 = 2;
/// How many icons stand in one row of the grid.
const PICK_COLUMNS: i32 = 9;
/// A group label's height, and the gap above each group (layout pixels).
const LABEL_TALL: i32 = 16;
const GROUP_GAP: i32 = 6;
/// The fill style that stretches a frame's picture over the whole frame,
/// as KOTOR 2's HUD layout gives its action slots.
const STRETCHED: i32 = 2;

/// The setup page's words. No line of KOTOR 2 fits, so these are kjk's own.
const SETUP_TITLE: &str = "Action bar";
const SETUP_HELP: &str =
    "Drag a power, feat attack or item onto a slot to put it on the action bar.";
const SETUP_NOTHING: &str = "This character has nothing to put on the action bar.";
const SETUP_OTHERS: &str = "Another player sets up this character's action bar.";
const HIDDEN_FROM_WHEEL: &str = "This power is hidden from the wheel.";

/// The abilities page's controls that the setup page does not use. It
/// hides them.
const UNUSED: [&str; 14] = [
    "BTN_SKILLS",
    "BTN_POWERS",
    "BTN_FEATS",
    "LBL_FILTER",
    "LBL_BAR6",
    "LB_ABILITY",
    "LB_DESC_FEATS",
    "LBL_INFOBG",
    "LBL_SKILLRANK",
    "LBL_RANKVAL",
    "LBL_BONUS",
    "LBL_BONUSVAL",
    "LBL_TOTAL",
    "LBL_TOTALVAL",
];

/// What has the focus on the setup page: an icon of the grid by its group
/// and its place, or a slot by its place.
#[derive(Clone, Copy, PartialEq, Eq)]
enum SetupFocus {
    Pick(usize, usize),
    Slot(usize),
}

impl SetupFocus {
    fn of(tag: &str) -> Option<SetupFocus> {
        if let Some((group, at)) = pick_of(tag, PICK) {
            return Some(SetupFocus::Pick(group, at));
        }
        slot_of(tag, SLOT).map(SetupFocus::Slot)
    }
}

/// One icon of the grid: what it puts on the bar, how it shows, and
/// whether it is a power hidden from the power wheel.
type Choice<'v> = (BarContent, &'v WheelEntry, bool);

/// What the pointer drags: an icon of the grid, or what a slot holds.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Dragged {
    Pick(BarContent),
    Slot(usize, BarContent),
}

/// A drag under way: what is dragged, its icon, and where the pointer is
/// (layout pixels).
struct Drag {
    what: Dragged,
    icon: String,
    at: [f32; 2],
}

/// The slot that the control `tag` of the slots named `prefix` is.
fn slot_of(tag: &str, prefix: &str) -> Option<usize> {
    tag.strip_prefix(prefix)?.parse().ok()
}

/// The group and the place of the grid icon `tag`, of the controls named
/// `prefix`.
fn pick_of(tag: &str, prefix: &str) -> Option<(usize, usize)> {
    let (group, at) = tag.strip_prefix(prefix)?.split_once('_')?;
    Some((group.parse().ok()?, at.parse().ok()?))
}

/// The icons of each group, in the order `GROUPS` names them.
fn picks(view: &BarSetupView) -> [Vec<Choice<'_>>; 3] {
    fn plain((content, entry): &(BarContent, WheelEntry)) -> Choice<'_> {
        (*content, entry, false)
    }
    [
        view.powers
            .iter()
            .map(|(content, entry, hidden)| (*content, entry, *hidden))
            .collect(),
        view.feats.iter().map(plain).collect(),
        view.items.iter().map(plain).collect(),
    ]
}

/// The action bar setup page, on the abilities page's KOTOR 2 layout. The
/// member's six bar slots stand in a row at the top. Under them, everything
/// the member can put on the bar stands as small icons in a grid, grouped
/// under the labels "Force powers", "Feat attacks" and "Items". One line
/// above the slots names the icon or slot under the pointer, and holds the
/// help when the pointer is over neither.
///
/// - With the mouse, the player drags an icon onto a slot to put it there.
///   A filled slot dragged onto another slot moves there, and the two slots
///   swap when the other was filled. A filled slot dragged anywhere else is
///   cleared. A right click on a power hides it from the power wheel or
///   shows it again. A hidden power's icon is drawn in the disabled look.
/// - On the keyboard, a slot's key puts the icon under the pointer (or with
///   the focus) in that slot. With a slot focused, a slot's key or Delete
///   clears it. H hides the focused power or shows it again.
/// - On a controller, A on an icon chooses it, and A on a slot then puts it
///   there. B lets go of the chosen icon. X on a slot clears it. Y hides
///   the focused power or shows it again.
///
/// The grid is laid out for the icons the member has. When their number
/// changes, the page shapes its layout again (`Screen::reshapes`). The
/// member shown follows the menu bar's portraits. When the menu shows
/// another member, the page forgets the chosen icon and any drag. Only a
/// player who dresses the member (`dresses`) changes its bar.
///
/// Back (Escape, or B with no icon chosen) returns to the abilities page,
/// as does the menu bar's abilities tab.
#[derive(Default)]
pub struct ActionBar {
    /// How many icons each group had when the grid was last shaped, and
    /// how many it has now.
    shaped: [usize; 3],
    counts: [usize; 3],
    /// The first icon of the grid, which has the focus when the page opens.
    first: String,
    focus: Option<SetupFocus>,
    /// What the name line names. The pointer sets it to what is under it,
    /// and the keys and a controller set it to what they focus.
    named: Option<SetupFocus>,
    /// The keys or a controller moved the focus since the pointer last
    /// moved.
    keyed: bool,
    /// The icon chosen with A or Enter, waiting for a slot.
    held: Option<BarContent>,
    drag: Option<Drag>,
    /// The member whose bar the page showed last (`BarSetupView::who`).
    who: Option<u32>,
}

impl ActionBar {
    /// The page for the view `vm` holds when it opens, its grid shaped for
    /// that view's icons.
    pub fn new(vm: &ViewModel) -> ActionBar {
        let mut page = ActionBar::default();
        page.cursor_to(vm);
        page.shaped = page.counts;
        page
    }

    /// The grid icon at `group` and `at`.
    fn pick<'v>(view: &'v BarSetupView, (group, at): (usize, usize)) -> Option<Choice<'v>> {
        picks(view)[group].get(at).copied()
    }

    /// The focused grid icon.
    fn chosen<'v>(&self, view: &'v BarSetupView) -> Option<Choice<'v>> {
        match self.focus? {
            SetupFocus::Pick(group, at) => ActionBar::pick(view, (group, at)),
            SetupFocus::Slot(_) => None,
        }
    }

    /// Sends `edit` for the member shown, when this player dresses it.
    fn send(vm: &ViewModel, edit: BarEdit, out: &mut Vec<Intent>) {
        if let Some(view) = vm.bar_setup.as_ref().filter(|_| dresses(vm)) {
            out.push(Intent::Game(GameAction::Bar {
                who: view.who,
                edit,
            }));
        }
    }

    /// Puts the focused grid icon in `slot`.
    fn put(&self, slot: usize, vm: &ViewModel, out: &mut Vec<Intent>) {
        let chosen = vm.bar_setup.as_ref().and_then(|view| self.chosen(view));
        if let Some((content, ..)) = chosen {
            let slot = slot as u8;
            ActionBar::send(vm, BarEdit::Put { slot, content }, out);
        }
    }

    /// Empties the focused slot.
    fn clear(&self, vm: &ViewModel, out: &mut Vec<Intent>) {
        if let Some(SetupFocus::Slot(slot)) = self.focus {
            let slot = slot as u8;
            ActionBar::send(vm, BarEdit::Clear { slot }, out);
        }
    }

    /// Hides the focused power from the power wheel, or shows it again.
    fn hide(&self, vm: &ViewModel, out: &mut Vec<Intent>) {
        let chosen = vm.bar_setup.as_ref().and_then(|view| self.chosen(view));
        if let Some((BarContent::Power(row), _, hidden)) = chosen {
            ActionBar::send(vm, BarEdit::Hide { row, hide: !hidden }, out);
        }
    }

    /// What a drag of `what` does when it is let go over the control
    /// `under`. A grid icon let go over a slot goes in it. A slot's content
    /// let go over another slot moves there, and the game swaps the two
    /// slots when the other was filled (`BarEdit::Put`). A slot's content
    /// let go anywhere but a slot is cleared from its slot.
    fn drop(what: Dragged, under: Option<&str>, vm: &ViewModel, out: &mut Vec<Intent>) {
        let target = under.and_then(|tag| slot_of(tag, SLOT));
        let edit = match (what, target) {
            (Dragged::Pick(content), Some(slot)) | (Dragged::Slot(_, content), Some(slot)) => {
                BarEdit::Put {
                    slot: slot as u8,
                    content,
                }
            }
            (Dragged::Slot(slot, _), None) => BarEdit::Clear { slot: slot as u8 },
            (Dragged::Pick(_), None) => return,
        };
        if !matches!((what, edit), (Dragged::Slot(from, _), BarEdit::Put { slot, .. }) if from == usize::from(slot))
        {
            ActionBar::send(vm, edit, out);
        }
    }

    /// The grid icon of `content`.
    fn choice_of(view: &BarSetupView, content: BarContent) -> Option<Choice<'_>> {
        picks(view)
            .into_iter()
            .flatten()
            .find(|(held, ..)| *held == content)
    }

    /// What the info box tells of: what is dragged while a drag goes on,
    /// else the grid icon or the slot that the name follows (`named`). A
    /// slot tells of the grid icon of what it holds.
    fn told<'v>(&self, view: &'v BarSetupView) -> Option<Choice<'v>> {
        if let Some(drag) = &self.drag {
            let (Dragged::Pick(content) | Dragged::Slot(_, content)) = drag.what;
            return ActionBar::choice_of(view, content);
        }
        match self.named? {
            SetupFocus::Pick(group, at) => ActionBar::pick(view, (group, at)),
            SetupFocus::Slot(slot) => ActionBar::choice_of(view, (*view.contents.get(slot)?)?),
        }
    }

    /// The info box's body for `choice`: the game's description, then one
    /// sentence if the power is hidden from the wheel. Without either the
    /// body is empty, and the box shows only the name.
    fn body((_, entry, hidden_from_wheel): Choice) -> Vec<u8> {
        let hidden = hidden_from_wheel.then_some(HIDDEN_FROM_WHEEL);
        let parts = [entry.description.as_slice()]
            .into_iter()
            .chain(hidden.map(str::as_bytes))
            .filter(|part| !part.is_empty());
        parts.collect::<Vec<_>>().join(&b"\n\n"[..])
    }

    /// The help in the info box: how to use the page, or why the page
    /// offers nothing.
    fn help(vm: &ViewModel, view: &BarSetupView) -> &'static str {
        if !dresses(vm) {
            SETUP_OTHERS
        } else if view.powers.is_empty() && view.feats.is_empty() && view.items.is_empty() {
            SETUP_NOTHING
        } else {
            SETUP_HELP
        }
    }
}

impl Page for ActionBar {
    const NAME: &'static str = "action_bar";
    const LAYOUT: &'static str = "abilities";

    fn state(&self, control: &Control, vm: &ViewModel) -> ControlState {
        let tag = control.tag.as_str();
        let view = vm.bar_setup.clone().unwrap_or(BarSetupView::default());
        if let Some(slot) = slot_of(tag, SLOT) {
            return shown(slot < view.slots.len());
        }
        if let Some(slot) = slot_of(tag, SLOT_ICON) {
            return match view.slots.get(slot) {
                // The setup page draws nothing in the disabled look for
                // being unusable now. That look means hidden from the wheel
                // here, and only a grid icon shows it.
                Some(BarSlot {
                    icon: Some(icon), ..
                }) => image(Some(icon.clone())),
                _ => hidden(),
            };
        }
        if let Some(place) = pick_of(tag, PICK) {
            return match ActionBar::pick(&view, place) {
                // The icon chosen with A waits for a slot in the lit look.
                Some((content, ..)) => ControlState {
                    selected: self.held == Some(content),
                    ..ControlState::default()
                },
                None => hidden(),
            };
        }
        if let Some(place) = pick_of(tag, PICK_ICON) {
            return match ActionBar::pick(&view, place) {
                Some((_, entry, hidden_from_wheel)) => ControlState {
                    fill: Some(entry.icon.clone()),
                    dimmed: hidden_from_wheel,
                    ..ControlState::default()
                },
                None => hidden(),
            };
        }
        if let Some(group) = slot_of(tag, GROUP_LABEL) {
            let shows = picks(&view)
                .get(group)
                .is_some_and(|icons| !icons.is_empty());
            return if shows { text(GROUPS[group]) } else { hidden() };
        }
        if UNUSED.contains(&tag) {
            return hidden();
        }
        match tag {
            "LBL_ABILITIES" => text(SETUP_TITLE),
            "LBL_NAME" => ControlState {
                text: Some(
                    self.told(&view)
                        .map_or(Vec::new(), |(_, entry, _)| entry.name.clone()),
                ),
                ..ControlState::default()
            },
            "LB_DESC" => match self.told(&view) {
                Some(choice) => description(&ActionBar::body(choice)),
                None => description(ActionBar::help(vm, &view).as_bytes()),
            },
            _ => ControlState::default(),
        }
    }

    /// A grid icon activated with A or Enter is chosen, to be put in the
    /// slot activated next. A click that starts a drag chooses nothing.
    fn press(&mut self, tag: &str, vm: &ViewModel, out: &mut Vec<Intent>) {
        if self.drag.is_some() {
            return;
        }
        let view = vm.bar_setup.as_ref();
        match SetupFocus::of(tag) {
            Some(SetupFocus::Pick(group, at)) => {
                self.held = view
                    .and_then(|view| ActionBar::pick(view, (group, at)))
                    .map(|(content, ..)| content);
            }
            Some(SetupFocus::Slot(slot)) => {
                if let Some(content) = self.held.take() {
                    let slot = slot as u8;
                    ActionBar::send(vm, BarEdit::Put { slot, content }, out);
                }
            }
            None => {}
        }
    }

    fn first_focus(&self) -> Option<&str> {
        Some(self.first.as_str())
    }

    fn reshapes(&mut self) -> bool {
        let changed = self.shaped != self.counts;
        self.shaped = self.counts;
        changed
    }

    /// The stack asks this each frame, before the frame's input. The page
    /// uses it to count the icons of each group, and to notice that the
    /// menu shows another member. It then forgets the chosen icon and any
    /// drag. It never moves the cursor itself.
    fn cursor_to(&mut self, vm: &ViewModel) -> Option<(&'static str, usize)> {
        let view = vm.bar_setup.as_ref();
        self.counts = view.map_or([0; 3], |view| picks(view).map(|icons| icons.len()));
        // With no icons yet, the first slot, which every shape has.
        self.first = (self.counts.iter().position(|count| *count > 0))
            .map_or(format!("{SLOT}0"), |group| format!("{PICK}{group}_0"));
        let who = view.map(|view| view.who);
        if self.who.is_some() && self.who != who {
            self.held = None;
            self.drag = None;
        }
        self.who = who;
        None
    }

    fn focused(&mut self, tag: &str, _: &ViewModel) {
        self.focus = SetupFocus::of(tag);
        if self.keyed {
            self.named = self.focus;
        }
    }

    /// A press starts a drag of the grid icon or the filled slot under the
    /// pointer. The drag follows the pointer while the button is held, and
    /// letting go drops it (`drop`).
    fn pointer(&mut self, pointer: Pointer<'_>, vm: &ViewModel, out: &mut Vec<Intent>) {
        if pointer.at.is_some() {
            self.keyed = false;
            self.named = pointer.under.and_then(SetupFocus::of);
        }
        let view = vm.bar_setup.as_ref();
        if pointer.press
            && let (Some(view), Some(at), Some(under)) = (view, pointer.at, pointer.under)
        {
            let what = match SetupFocus::of(under) {
                Some(SetupFocus::Pick(group, place)) => ActionBar::pick(view, (group, place))
                    .map(|(content, entry, _)| (Dragged::Pick(content), entry.icon.clone())),
                Some(SetupFocus::Slot(slot)) => {
                    let content = view.contents.get(slot).copied().flatten();
                    let icon = view.slots.get(slot).and_then(|slot| slot.icon.clone());
                    content
                        .zip(icon)
                        .map(|(content, icon)| (Dragged::Slot(slot, content), icon))
                }
                None => None,
            };
            self.drag = what.map(|(what, icon)| Drag { what, icon, at });
        }
        if let (Some(drag), Some(at)) = (self.drag.as_mut(), pointer.at) {
            drag.at = at;
        }
        if !pointer.held
            && let Some(drag) = self.drag.take()
        {
            ActionBar::drop(drag.what, pointer.under, vm, out);
        }
    }

    /// A controller's X, Y and B on the page, and the moves that make the
    /// name line follow the focus.
    fn key(&mut self, key: Key, vm: &ViewModel, out: &mut Vec<Intent>) -> bool {
        match (key, self.focus) {
            (Key::Primary, Some(SetupFocus::Slot(_))) => self.clear(vm, out),
            (Key::Second, Some(SetupFocus::Pick(..))) => self.hide(vm, out),
            (Key::Back, _) if self.held.is_some() => self.held = None,
            (Key::Back, _) => out.push(Intent::Swap(<Abilities as Page>::NAME)),
            (Key::Up | Key::Down | Key::Left | Key::Right, _) => {
                self.keyed = true;
                return false;
            }
            _ => return false,
        }
        true
    }

    /// The keyboard's slot keys, Delete, H and the right mouse button on the
    /// page.
    fn raw(&mut self, key: &str, vm: &ViewModel, out: &mut Vec<Intent>) -> bool {
        let keys = vm.bar_setup.as_ref().map_or(&[][..], |view| &view.keys[..]);
        let slot = keys
            .iter()
            .position(|bound| !bound.is_empty() && bound == key);
        match (slot, key, self.focus) {
            (Some(_), _, Some(SetupFocus::Slot(_)))
            | (None, "Delete", Some(SetupFocus::Slot(_))) => {
                self.clear(vm, out);
            }
            (Some(slot), _, Some(SetupFocus::Pick(..))) => self.put(slot, vm, out),
            (None, "KeyH" | "MouseRight", Some(SetupFocus::Pick(..))) => self.hide(vm, out),
            _ => return false,
        }
        true
    }

    /// Each slot's key at its top and the uses left at its bottom right, as
    /// the HUD draws them, each item's uses left on its grid icon, and the
    /// dragged icon under the pointer.
    fn quads(&self, gui: &Gui, vm: &ViewModel, painter: &mut Painter) -> Vec<Quad> {
        let Some(view) = vm.bar_setup.as_ref() else {
            return Vec::new();
        };
        let frames = view.slots.iter().enumerate().filter_map(|(at, slot)| {
            let frame = gui.find(&format!("{SLOT}{at}"))?;
            Some((slot, rect_of(frame)))
        });
        let mut out = DrawList::default();
        crate::screens::hud::slot_marks(frames, painter, &mut out);
        let counted: Vec<(BarSlot, [f32; 4])> = picks(view)
            .iter()
            .enumerate()
            .flat_map(|(group, icons)| {
                icons
                    .iter()
                    .enumerate()
                    .map(move |(at, icon)| (group, at, icon))
            })
            .filter_map(|(group, at, (_, entry, _))| {
                let frame = gui.find(&format!("{PICK}{group}_{at}"))?;
                let count = entry.count?;
                let slot = BarSlot {
                    count: Some(count),
                    ..BarSlot::default()
                };
                Some((slot, rect_of(frame)))
            })
            .collect();
        crate::screens::hud::slot_marks(
            counted.iter().map(|(slot, rect)| (slot, *rect)),
            painter,
            &mut out,
        );
        if let Some(drag) = &self.drag {
            // As large as a slot's icon, where it is about to go.
            let side = (HUD_SLOT_ICON[2] * SLOT_WIDE / HUD_SLOT_SIZE[0]) as f32;
            let rect = [drag.at[0] - side / 2.0, drag.at[1] - side / 2.0, side, side];
            painter.image(Source::Campaign, &drag.icon, rect, [1.0; 4], &mut out);
        }
        out.quads
    }

    /// KOTOR 2's stretch to a wide window keeps each picture's shape but
    /// moves the controls apart, which opens gaps in the grid. This closes
    /// them at the size the page is drawn. The grid's icons touch again, and
    /// the slots spread so that the slot row is exactly as wide as the grid.
    fn fit_drawn(&mut self, gui: &mut Gui) {
        let extent = |gui: &Gui, tag: &str| gui.find(tag).map(|control| control.extent);
        let Some(first) = extent(gui, &format!("{SLOT}0")) else {
            return;
        };
        let Some(square) = gui
            .controls
            .iter()
            .find(|control| pick_of(&control.tag, PICK).is_some())
            .map(|control| control.extent)
        else {
            return;
        };
        let (side, tall) = (square.width, square.height);
        let wide = PICK_COLUMNS * side;
        // Where each control goes: its left, and its top when it moves down.
        let place = |tag: &str| -> Option<(i32, Option<i32>)> {
            if let Some(at) = slot_of(tag, SLOT).or(slot_of(tag, SLOT_ICON)) {
                let spread = (wide - first.width) * at as i32 / (SETUP_SLOTS - 1);
                return Some((first.left + spread, None));
            }
            let (group, at) = pick_of(tag, PICK).or(pick_of(tag, PICK_ICON))?;
            let top = extent(gui, &format!("{PICK}{group}_0"))?.top;
            let at = at as i32;
            Some((
                first.left + at % PICK_COLUMNS * side,
                Some(top + at / PICK_COLUMNS * tall),
            ))
        };
        let moves: Vec<(usize, i32, Option<i32>)> = gui
            .controls
            .iter()
            .enumerate()
            .filter_map(|(at, control)| {
                let (left, top) = place(&control.tag)?;
                // A frame and its icon move together, by the frame's move.
                let frame = control.tag.replacen("LBL_", "BTN_", 1);
                let from = extent(gui, &frame)?;
                Some((at, left - from.left, top.map(|top| top - from.top)))
            })
            .collect();
        for (at, dx, dy) in moves {
            let extent = &mut gui.controls[at].extent;
            extent.left += dx;
            extent.top += dy.unwrap_or(0);
        }
        for control in &mut gui.controls {
            if slot_of(&control.tag, GROUP_LABEL).is_some() {
                control.extent.left = first.left;
                control.extent.width = wide;
            }
        }
    }

    /// The six slots in a row at the top of the list's place, in the HUD
    /// action slot's shape, and under them each group's label and icons in
    /// a grid of `PICK_COLUMNS` icons across. The grid starts at the slot
    /// row's left edge, and its icons touch. A group with no icons takes no
    /// room. The info box (the name and the description) stays at the right,
    /// where the abilities page has it.
    fn shape(&self, gui: &mut Gui) {
        let (Some(list), Some(band)) = (
            gui.find("LB_ABILITY").cloned(),
            gui.find("BTN_SKILLS").cloned(),
        ) else {
            return;
        };
        let parent = list.parent;
        let (left, width) = (list.extent.left, list.extent.width);
        let mut id = next_id(gui);
        let mut push = |gui: &mut Gui, control: Control| {
            gui.controls.push(Control {
                id,
                parent,
                ..control
            });
            id += 1;
        };
        use fmt_odyssey::gui::align;
        let label = |tag: String, extent: Rect| Control {
            kind: ControlKind::Label,
            tag,
            extent,
            text: band.text.clone().map(|text| fmt_odyssey::gui::Text {
                alignment: align::LEFT | align::MIDDLE,
                ..text
            }),
            ..Control::default()
        };
        // The slots, centred in the list's place.
        let [hud_wide, hud_tall] = HUD_SLOT_SIZE;
        let tall = SLOT_WIDE * hud_tall / hud_wide;
        let [icon_left, icon_top, icon_side] =
            HUD_SLOT_ICON.map(|size| size * SLOT_WIDE / hud_wide);
        let row = SETUP_SLOTS * SLOT_WIDE + (SETUP_SLOTS - 1) * SLOT_GAP;
        let slots_left = left + (width - row) / 2;
        let frame = |fill: &str| Border {
            fill: fill.into(),
            fill_style: STRETCHED,
            ..Border::default()
        };
        let framed = |tag: String, extent: Rect, border: Border, hilight: Border| Control {
            kind: ControlKind::Button,
            tag,
            extent,
            border: Some(border),
            hilight: Some(hilight),
            ..Control::default()
        };
        let icon = |tag: String, extent: Rect| Control {
            kind: ControlKind::Label,
            tag,
            extent,
            ..Control::default()
        };
        let top = list.extent.top;
        for at in 0..SETUP_SLOTS {
            let slot_left = slots_left + at * (SLOT_WIDE + SLOT_GAP);
            let extent = Rect {
                left: slot_left,
                top,
                width: SLOT_WIDE,
                height: tall,
            };
            push(
                gui,
                framed(
                    format!("{SLOT}{at}"),
                    extent,
                    frame(HUD_SLOT.0),
                    frame(HUD_SLOT.1),
                ),
            );
            let inside = Rect {
                left: slot_left + icon_left,
                top: top + icon_top,
                width: icon_side,
                height: icon_side,
            };
            push(gui, icon(format!("{SLOT_ICON}{at}"), inside));
        }
        // The grid, from the slot row's left edge.
        let (pitch, columns) = (PICK_SIDE, PICK_COLUMNS);
        let grid_wide = columns * pitch;
        let grid_left = slots_left;
        let square = |color: [f32; 3]| Border {
            corner: PICK_FRAME.0.into(),
            edge: PICK_FRAME.1.into(),
            fill_style: STRETCHED,
            dimension: PICK_FRAME.2,
            color: Some(color),
            ..Border::default()
        };
        let mut y = top + tall + GROUP_GAP;
        for (group, count) in self
            .shaped
            .iter()
            .enumerate()
            .filter(|(_, count)| **count > 0)
        {
            let extent = Rect {
                left: grid_left,
                top: y,
                width: grid_wide,
                height: LABEL_TALL,
            };
            push(gui, label(format!("{GROUP_LABEL}{group}"), extent));
            y += LABEL_TALL;
            for at in 0..*count as i32 {
                let (column, line) = (at % columns, at / columns);
                let extent = Rect {
                    left: grid_left + column * pitch,
                    top: y + line * pitch,
                    width: PICK_SIDE,
                    height: PICK_SIDE,
                };
                let tag = format!("{PICK}{group}_{at}");
                push(
                    gui,
                    framed(tag, extent, square(PICK_COLOR[0]), square(PICK_COLOR[1])),
                );
                let inside = Rect {
                    left: extent.left + PICK_INSET,
                    top: extent.top + PICK_INSET,
                    width: PICK_SIDE - 2 * PICK_INSET,
                    height: PICK_SIDE - 2 * PICK_INSET,
                };
                push(gui, icon(format!("{PICK_ICON}{group}_{at}"), inside));
            }
            let lines = (*count as i32 + columns - 1) / columns;
            y += lines * pitch + GROUP_GAP;
        }
    }
}

/// What the messages page lists.
#[derive(Default, Clone, Copy, PartialEq, Eq)]
enum Log {
    #[default]
    Feedback,
    Dialog,
    Combat,
}

/// Each tab of the messages page: its button, its list, what it lists.
const LOGS: [(&str, &str, Log); 3] = [
    ("BTN_FEEDBACK", "LB_MESSAGES", Log::Feedback),
    ("BTN_DIALOG", "LB_DIALOG", Log::Dialog),
    ("BTN_COMBAT", "LB_COMBAT", Log::Combat),
];

/// The feedback, dialogue and combat logs, one tab each; each shows its
/// newest lines until scrolled. The effects tab waits for the party's
/// effects.
#[derive(Default)]
pub struct Messages {
    log: Log,
}

impl Page for Messages {
    fn tabs(&self) -> Vec<&'static str> {
        LOGS.iter().map(|(tag, ..)| *tag).collect()
    }

    const NAME: &'static str = "messages";
    const LAYOUT: &'static str = "messages";

    fn state(&self, control: &Control, vm: &ViewModel) -> ControlState {
        let tag = control.tag.as_str();
        if let Some((_, _, log)) = LOGS.iter().find(|(button, ..)| *button == tag) {
            return ControlState {
                selected: *log == self.log,
                ..ControlState::default()
            };
        }
        if let Some((_, _, log)) = LOGS.iter().find(|(_, list, _)| *list == tag) {
            if *log != self.log {
                return hidden();
            }
            let lines = match log {
                Log::Feedback => &vm.feedback,
                Log::Dialog => &vm.dialog_log,
                Log::Combat => &vm.combat_log,
            };
            return ControlState {
                list: Some(ListView {
                    rows: lines
                        .iter()
                        .map(|line| Row {
                            text: crate::strings::bytes(line),
                            ..Row::default()
                        })
                        .collect(),
                    fit: true,
                    pick: true,
                    follow: true,
                    ..ListView::default()
                }),
                ..ControlState::default()
            };
        }
        match tag {
            "LB_EFFECTS_GOOD" | "LB_EFFECTS_BAD" | "LBL_EFFECTS_GOOD" | "LBL_EFFECTS_BAD" => {
                hidden()
            }
            "BTN_EFFECTS" => disabled(),
            _ => ControlState::default(),
        }
    }

    fn press(&mut self, tag: &str, _: &ViewModel, _: &mut Vec<Intent>) {
        if let Some((_, _, log)) = LOGS.iter().find(|(button, ..)| *button == tag) {
            self.log = *log;
        }
    }

    fn first_focus(&self) -> Option<&str> {
        Some("LB_MESSAGES")
    }
}
