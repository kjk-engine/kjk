//! The in-game menu's pages on what the game has today: the player's own
//! character (its sheet), what it carries, the feedback log (the map's is
//! `map`). Using items waits for the world state, the party for its party
//! table: those controls show greyed. The pages are KOTOR 2's layouts (the
//! skin): the character's portrait, name, class, level and vitality are
//! the bar's.

use fmt_odyssey::gui::{Control, Gui};

use crate::draw::{DrawList, Quad};
use crate::intent::{GameAction, GuiSound, Intent};
use crate::paint::{ControlState, ListView, Painter, Row, rect_of};
use crate::screens::filter::{self, Filter};
use crate::screens::tab::{Page, disabled, enabled, hidden, image, rows, signed, text};
use crate::vm::{CharacterView, Entry, Owned, Preview, SceneView, ViewModel};

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

/// Where TSL's `equip_p` keeps its description box (`LB_DESC`): over the
/// slots, which it hides while a slot's list is open.
const DESC_BOX: [i32; 4] = [410, 134, 313, 336];

/// Whether `control`'s centre lies within the description box (`DESC_BOX`).
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
    fn listed(&self, me: &CharacterView) -> Vec<Listed> {
        let Some((_, slot)) = self.slot else {
            return Vec::new();
        };
        let worn = me.worn.iter().find(|(at, _)| *at == slot);
        let fitting = me
            .owned
            .iter()
            .filter(|owned| owned.slots & 1 << rules(slot) != 0 && !owned.unfit);
        worn.into_iter()
            .flat_map(|(_, owned)| [Listed::Off, Listed::Worn(owned.clone())])
            .chain(fitting.cloned().map(Listed::Carried))
            .collect()
    }

    /// The chosen row, when choosing it changes something: `Some(None)` for
    /// the "None" row, else the item to wear.
    fn choice(&self, me: &CharacterView) -> Option<Option<Owned>> {
        match self.listed(me).get(self.chosen?)? {
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
                let listed = self.listed(&me).into_iter().nth(row);
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
                let refused = item.as_ref().and_then(|owned| owned.refused.as_deref());
                return described_refused(item.as_ref().map(|owned| &owned.entry), &lead, refused);
            }
            if under_desc_box(control) {
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
                text: self.slot.is_none().then(Vec::new),
                ..ControlState::default()
            },
            "LB_ITEMS" if self.slot.is_some() => {
                let none = Entry {
                    name: b"None".to_vec(),
                    ..Entry::default()
                };
                let slot = self.slot.map(|(_, slot)| slot);
                let off_hand = [5, 19].iter().position(|left| Some(*left) == slot);
                // In red, as both games' `CSWGuiInGameItemEntry::SetCanUse`
                // draws one the member may not wear, or (K1's) an off hand's
                // that does not go with its main hand's weapon.
                let refused = |owned: &Owned| {
                    owned.refused.is_some() || off_hand.is_some_and(|set| owned.unpaired[set])
                };
                let (entries, red): (Vec<Entry>, Vec<bool>) = self
                    .listed(&me)
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
            "BTN_EQUIP" => enabled(dresses(vm) && self.choice(&me).is_some()),
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
            self.slot = Some((button, *slot));
            self.chosen = None;
            self.pointed = 0;
        } else if tag == "BTN_BACK" && self.slot.is_some() {
            // Back from a slot's list to the slots, as TSL's.
            self.slot = None;
            self.chosen = None;
        } else if tag == "BTN_EQUIP" && dresses(vm) {
            let me = character(vm);
            if let (Some((_, slot)), Some(choice)) = (self.slot, self.choice(&me)) {
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

    fn pointed(&mut self, list: &str, index: usize, _: &ViewModel) {
        if list == "LB_ITEMS" {
            self.pointed = index;
        }
    }

    /// A double click on a slot takes off what is worn in it; on an item of
    /// its list, wears it (`primary`).
    fn double(&mut self, tag: &str, vm: &ViewModel, out: &mut Vec<Intent>) {
        let me = character(vm);
        if SLOTS.iter().any(|(_, button, ..)| *button == tag)
            && self.listed(&me).first() == Some(&Listed::Off)
        {
            self.chosen = Some(0);
            self.press("BTN_EQUIP", vm, out);
        }
    }

    fn primary(&self, list: &str) -> Option<&'static str> {
        (list == "LB_ITEMS").then_some("BTN_EQUIP")
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
    fn quads(&self, gui: &Gui, vm: &ViewModel, painter: &mut Painter) -> Vec<Quad> {
        let (Some((_, hidden)), Some(eye), true) =
            (Equip::eye(vm), gui.find(EYE), self.eye_focused)
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
}

/// The inventory page: the party's items, of the kind its filter shows;
/// Use Item uses the chosen one on the member shown (KOTOR's: the items
/// used on oneself, a medpac or a stim), when this player may.
pub struct Inventory {
    filter: Filter,
    selected: Option<usize>,
    /// The row the list's cursor is on: described while none is chosen.
    pointed: usize,
}

impl Default for Inventory {
    fn default() -> Inventory {
        Inventory {
            filter: Filter::new(filter::INVENTORY),
            selected: None,
            pointed: 0,
        }
    }
}

impl Inventory {
    /// The chosen item, when this player may use it on the member shown.
    fn usable(&self, vm: &ViewModel, me: &CharacterView) -> Option<u32> {
        let listed = self.filter.shown(&me.owned, |owned| &owned.entry);
        let owned = listed.get(self.selected?)?;
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
            "LB_ITEMS" => rows(&items, self.selected),
            "LB_DESCRIPTION" => described(items.get(self.selected.unwrap_or(self.pointed)), &[]),
            "LBL_CREDITS_VALUE" => text(me.credits),
            "BTN_USEITEM" => enabled(self.usable(vm, &me).is_some()),
            _ => ControlState::default(),
        }
    }

    fn press(&mut self, tag: &str, vm: &ViewModel, out: &mut Vec<Intent>) {
        if self.filter.press(tag) {
            self.selected = None;
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
    }

    fn pointed(&mut self, list: &str, index: usize, _: &ViewModel) {
        if list == "LB_ITEMS" {
            self.pointed = index;
        }
    }

    fn primary(&self, list: &str) -> Option<&'static str> {
        (list == "LB_ITEMS").then_some("BTN_USEITEM")
    }

    fn first_focus(&self) -> Option<&str> {
        Some("LB_ITEMS")
    }
}

#[derive(Default)]
pub struct Character;

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
        for (at, (score, modifier)) in SCORES.iter().enumerate() {
            let value = i32::from(me.abilities[at]);
            if tag == *score {
                return text(value.to_string());
            }
            if tag == *modifier {
                return text(signed((value - 10).div_euclid(2)));
            }
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
        let rank = chosen
            .filter(|_| self.kind == Kind::Skills)
            .and_then(|entry| entry.count)
            .map(|rank| rank.to_string());
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
            "LBL_RANKVAL" | "LBL_TOTALVAL" => match &rank {
                Some(rank) => text(rank),
                None => hidden(),
            },
            "LBL_BONUSVAL" => match &rank {
                Some(_) => text("0"),
                None => hidden(),
            },
            "LBL_SKILLRANK" | "LBL_BONUS" | "LBL_TOTAL" if rank.is_none() => hidden(),
            "BTN_SKILLS" => pick(Kind::Skills),
            "BTN_POWERS" => pick(Kind::Powers),
            "BTN_FEATS" => pick(Kind::Feats),
            _ => ControlState::default(),
        }
    }

    fn press(&mut self, tag: &str, _: &ViewModel, _: &mut Vec<Intent>) {
        let kind = match tag {
            "BTN_SKILLS" => Kind::Skills,
            "BTN_POWERS" => Kind::Powers,
            "BTN_FEATS" => Kind::Feats,
            _ => return,
        };
        if kind != self.kind {
            self.kind = kind;
            self.selected = None;
        }
    }

    fn row(&mut self, _: &str, index: usize, _: &ViewModel) {
        self.selected = Some(index);
    }

    fn first_focus(&self) -> Option<&str> {
        Some("LB_ABILITY")
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
