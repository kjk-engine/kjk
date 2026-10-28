//! The in-game menu's pages on what the game has today: the player's own
//! character (its sheet), what it carries, the feedback log (the map's is
//! `map`). Using items waits for the world state, the party for its party
//! table: those controls show greyed. The pages are KOTOR 2's layouts (the
//! skin): the character's portrait, name, class, level and vitality are
//! the bar's.

use fmt_odyssey::gui::Control;

use crate::intent::{GameAction, GuiSound, Intent};
use crate::paint::{ControlState, ListView, Row};
use crate::screens::filter::{self, Filter};
use crate::screens::tab::{Page, disabled, enabled, hidden, image, rows, signed, text};
use crate::vm::{CharacterView, Entry, Owned, SceneView, ViewModel};

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
}

impl Equip {
    /// The slot looked at's list: a "None" row first when it holds an item
    /// (it takes it off; a hand's weapon without an item, a dev sheet's,
    /// cannot be), then what the party carries that fits it (an off hand:
    /// what pairs with its main hand's weapon, `Owned::unpaired`).
    fn listed(&self, me: &CharacterView) -> Vec<Option<Owned>> {
        let Some((_, slot)) = self.slot else {
            return Vec::new();
        };
        let off_hand = [5, 19].iter().position(|left| *left == slot);
        let wearing = me.worn.iter().any(|(at, _)| *at == slot);
        let fitting = me.owned.iter().filter(|owned| {
            owned.slots & 1 << rules(slot) != 0 && off_hand.is_none_or(|set| !owned.unpaired[set])
        });
        wearing
            .then_some(None)
            .into_iter()
            .chain(fitting.cloned().map(Some))
            .collect()
    }

    /// The chosen row: `None` for nothing, `Some(None)` for the "None" row,
    /// else the item.
    fn choice(&self, me: &CharacterView) -> Option<Option<Owned>> {
        self.listed(me).get(self.chosen?).cloned()
    }
}

impl Page for Equip {
    const NAME: &'static str = "equip";
    const LAYOUT: &'static str = "equip";

    fn state(&self, control: &Control, vm: &ViewModel) -> ControlState {
        let me = character(vm);
        let tag = control.tag.as_str();
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
                let entries: Vec<Entry> = self
                    .listed(&me)
                    .into_iter()
                    .map(|row| row.map_or_else(|| none.clone(), |owned| owned.entry))
                    .collect();
                rows(&entries, self.chosen)
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
        if let Some((_, button, .., slot)) = SLOTS.iter().find(|(_, button, ..)| *button == tag) {
            self.slot = Some((button, *slot));
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

    /// A double click on a slot takes off what is worn in it; on an item of
    /// its list, wears it (`primary`).
    fn double(&mut self, tag: &str, vm: &ViewModel, out: &mut Vec<Intent>) {
        let me = character(vm);
        if SLOTS.iter().any(|(_, button, ..)| *button == tag)
            && self.listed(&me).first() == Some(&None)
        {
            self.chosen = Some(0);
            self.press("BTN_EQUIP", vm, out);
        }
    }

    fn primary(&self, list: &str) -> Option<&'static str> {
        (list == "LB_ITEMS").then_some("BTN_EQUIP")
    }

    fn first_focus(&self) -> Option<&str> {
        Some("BTN_INV_BODY")
    }
}

/// The inventory page: the party's items, of the kind its filter shows;
/// Use Item uses the chosen one on the member shown (KOTOR's: the items
/// used on oneself, a medpac or a stim), when this player may.
pub struct Inventory {
    filter: Filter,
    selected: Option<usize>,
}

impl Default for Inventory {
    fn default() -> Inventory {
        Inventory {
            filter: Filter::new(filter::INVENTORY),
            selected: None,
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
            "LB_DESCRIPTION" => description(
                &self
                    .selected
                    .and_then(|at| items.get(at))
                    .map_or(Vec::new(), |entry| entry.description.clone()),
            ),
            "LBL_CREDITS_VALUE" => text(me.credits),
            "BTN_USEITEM" => enabled(self.usable(vm, &me).is_some()),
            _ => ControlState::default(),
        }
    }

    fn press(&mut self, tag: &str, vm: &ViewModel, out: &mut Vec<Intent>) {
        if self.filter.press(tag) {
            self.selected = None;
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
