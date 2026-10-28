//! Items (I19): what each item template is (`ItemFacts`, read from its UTI
//! and `baseitems.2da` by the session), the game's templates by resref
//! (`Catalog`, on the authority with the scripts), the items a module's
//! objects start with (`Held`), the items in play (`Item`, script objects
//! kept with the scripts' state), what players see of them (`Goods`), a
//! store's terms and trades (`Terms`, `Trade`, `price`), and what the
//! equipped ones make of a character's sheet (`dress`).
//!
//! KOTOR's items are objects scripts make, move, equip and destroy; they
//! never stand in the world (a corpse's loot is a placeable's inventory).
//! The party carries one inventory, the leader's; every other character
//! and container its own. A character's sheet is what its class, level and
//! scores give (`undress`) and what its equipped items add: a weapon in
//! each hand, the body's armour defence and how much DEX may add to it,
//! and every equipped item's guards by damage type; its inventory the
//! items it carries to use in a fight (`combat::items`).

use std::collections::{BTreeMap, BTreeSet};

use crate::combat::Sheet;
use crate::combat::gear::Gear;
use crate::combat::items::{self, Carried};
use crate::combat::sheet::{DAMAGE_TYPES, Guards, Weapon};
use crate::wire::{DecodeError, Reader, Writer};

#[cfg(feature = "probes")]
pub mod probes;
pub mod properties;
pub mod workshop;

pub use properties::{Limit, Property, Upgrade};

/// `ItemFacts::slot_kinds`' mark for a lightsaber's colour crystal slot.
pub const COLOUR_SLOT: u8 = u8::MAX;

/// TSL's lightsaber crystal slots' kind (`upgradetypes.2da` row 0): the
/// one kind its bench fits whatever the item's upgrade level.
pub const CRYSTAL: u8 = 0;

/// An `itempropdef.2da` row set (`ItemFacts::rows`) in a digest.
fn write_rows(rows: u128, out: &mut Writer) {
    out.u64(rows as u64);
    out.u64((rows >> 64) as u64);
}

/// `INVENTORY_SLOT_*`: where an equipped item sits (an `Equip_ItemList`
/// struct id is `1 << slot`).
pub mod slots {
    pub const HEAD: u8 = 0;
    pub const BODY: u8 = 1;
    pub const HANDS: u8 = 3;
    pub const RIGHT_WEAPON: u8 = 4;
    pub const LEFT_WEAPON: u8 = 5;
    pub const LEFT_ARM: u8 = 7;
    pub const RIGHT_ARM: u8 = 8;
    pub const IMPLANT: u8 = 9;
    pub const BELT: u8 = 10;
    /// A creature's own weapons and hide (claws, bites, a droid's plating).
    pub const CREATURE_LEFT: u8 = 14;
    pub const CREATURE_RIGHT: u8 = 15;
    pub const CREATURE_BITE: u8 = 16;
    pub const CREATURE_ARMOUR: u8 = 17;
    /// TSL's second weapon set (`INVENTORY_SLOT_RIGHTWEAPON2`, `…LEFTWEAPON2`),
    /// in both games (a remaster liberty in KOTOR, which has one set): held
    /// ready, not in hand, until a swap (`script::items::State::switch_weapons`)
    /// exchanges it with the first.
    pub const RIGHT_WEAPON_2: u8 = 18;
    pub const LEFT_WEAPON_2: u8 = 19;
    /// How many slot numbers there are (`GetItemInSlot`'s range).
    pub const COUNT: u8 = 20;

    /// The weapon sets, each its right and left hand.
    pub const SETS: [[u8; 2]; 2] = [[RIGHT_WEAPON, LEFT_WEAPON], [RIGHT_WEAPON_2, LEFT_WEAPON_2]];

    /// The slot whose equip rules `slot` takes: the second set's hands take
    /// the first's (TSL's `CanEquipItem` reads masks `0x40000`, `0x80000` as
    /// `0x40010`, `0x80020`; `baseitems.2da` `equipableslots` names only the
    /// first set's), every other slot its own.
    pub fn rules(slot: u8) -> u8 {
        match slot {
            RIGHT_WEAPON_2 => RIGHT_WEAPON,
            LEFT_WEAPON_2 => LEFT_WEAPON,
            other => other,
        }
    }

    /// Whether an item equipped in `slot` is worn, its properties in force:
    /// all but the second set's (TSL's `EquipItem` applies no item
    /// properties for slots `0x40000`, `0x80000`).
    pub fn worn(slot: u8) -> bool {
        !SETS[1].contains(&slot)
    }
}

/// An armour's part of a sheet: `baseitems.2da` `baseac`, and `dexbonus`
/// (the most DEX may add to defence; `None` uncapped).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Armour {
    pub defence: i8,
    pub dex_cap: Option<u8>,
}

impl Armour {
    /// No armour: nothing to defence, DEX uncapped.
    pub const NONE: Armour = Armour {
        defence: 0,
        dex_cap: None,
    };
}

/// What an item template is, as the sim uses it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ItemFacts {
    /// Lowercase resrefs.
    pub template: String,
    pub tag: String,
    /// The tag as its data writes it, when that has capitals; empty
    /// otherwise: `GetTag` answers with it, as with an object's
    /// (`module::ObjectData::cased_tag`).
    pub cased_tag: String,
    /// `LocalizedName`'s strref (`u32::MAX`: none).
    pub name: u32,
    /// `baseitems.2da` row.
    pub base_item: u16,
    /// The template's stack, and the most one stack holds (`stacking`).
    pub stack: u16,
    pub stacking: u16,
    pub charges: u8,
    pub cost: u32,
    pub plot: bool,
    /// `INVENTORY_SLOT_*` bits it may be equipped in (`equipableslots`).
    pub slots: u32,
    /// As a weapon in hand, if combat knows it as one: its base item's
    /// (`bare`) with its properties in force.
    pub weapon: Option<Weapon>,
    pub bare: Option<Weapon>,
    /// As worn on the body, if armour.
    pub armour: Option<Armour>,
    /// Its own guards by damage type (immunity and vulnerability percent,
    /// resistance points).
    pub guards: Guards,
    /// Its properties in force that apply when it is worn
    /// (`properties::make`), which make its weapon, guards, `gear` and
    /// `limits`; and what it gives its wearer, who may wear it.
    pub properties: Vec<Property>,
    pub gear: Gear,
    pub limits: Vec<Limit>,
    /// The `itempropdef.2da` rows it has in force, as bits
    /// (`GetItemHasItemProperty`).
    pub rows: u128,
    /// What it casts when used (its Cast Spell property), if anything.
    pub spell: Option<ItemSpell>,
    /// Who may use it (`baseitems.2da` `droidorhuman`): droids only
    /// (`Some(true)`), all but droids (`Some(false)`), or anyone.
    pub droids: Option<bool>,
    /// The upgrades it takes (its properties with an `upgrade.2da` row the
    /// template has not fitted), by row.
    pub upgrades: Vec<Upgrade>,
    /// As an upgrade: its `upgrade.2da` row and kind (`upgradetype`).
    pub upgrade: Option<(u16, u8)>,
    /// TSL's upgrades: its `UpgradeLevel` (as a host, the most an upgrade
    /// fitted may have; as an upgrade, what it asks), and as a host its
    /// slots' kinds (`upgradetypes.2da` rows, `COLOUR_SLOT` for a
    /// lightsaber's colour crystal), by its base item's `itemtype`.
    pub upgrade_level: u8,
    pub slot_kinds: Vec<u8>,
    /// TSL's upgrade items its template has in its slots (`UpgradeSlotN`),
    /// by template, empty for none: an item made of it starts with them
    /// (`Item::fitted`).
    pub parts: Vec<String>,
    /// A colour crystal: the lightsabers of its colour, one for each kind
    /// of hilt (`upcrystals.2da`: short, long, double), by template.
    pub recolours: Vec<String>,
    /// Its Security bonus (a security spike's `ThievesTools` property);
    /// 0 for none.
    pub tools: u8,
    /// The feats one must have to equip it (`baseitems.2da` `reqfeat0` to
    /// `reqfeat4`: proficiencies, implant levels, Jedi Defense for robes).
    pub needs: Vec<u16>,
    /// The `traps.2da` row it lays (property 46 `Trap`'s subtype), if a
    /// trap kit.
    pub trap: Option<u8>,
    /// A stealth field generator (`baseitems.2da` `itemtype` 44: base item
    /// 88 `Stealth_Unit`): worn, stealth mode may be turned on.
    pub stealth_unit: bool,
    /// An energy shield (`baseitems.2da` `itemtype` 20: base item 46
    /// `Forearm_Bands`, the shields' row): the one item TSL's duels may
    /// allow (`FORFEIT_NO_ITEM_BUT_SHIELD`, `script::party::forfeit::using`).
    pub shield: bool,
}

impl ItemFacts {
    fn digest(&self, out: &mut Writer) {
        out.string(&self.template);
        out.string(&self.tag);
        out.u32(self.name);
        out.u16(self.base_item);
        out.u16(self.stack);
        out.u16(self.stacking);
        out.u8(self.charges);
        out.u32(self.cost);
        out.u8(u8::from(self.plot));
        out.u32(self.slots);
        crate::wire::write_fixed(
            out,
            self.weapon.as_ref(),
            &Weapon::UNARMED,
            crate::combat::sheet::encode_weapon,
        );
        let armour = self.armour.unwrap_or(Armour::NONE);
        out.u8(u8::from(self.armour.is_some()));
        out.i8(armour.defence);
        out.u8(armour.dex_cap.unwrap_or(u8::MAX));
        for immunity in self.guards.immunity {
            out.i8(immunity);
        }
        out.bytes(&self.guards.resist);
        out.u16(self.spell.map_or(u16::MAX, |spell| spell.row));
        // Only an item that casts lists its cost, so a catalog without
        // keeps its digest.
        if let Some(spell) = self.spell {
            out.string("cost");
            out.u8(spell.cost);
        }
        if let Some(droids) = self.droids {
            out.string("droids");
            out.u8(u8::from(droids));
        }
        // Only upgradable gear, upgrades and colour crystals list these.
        if !self.upgrades.is_empty() {
            out.string("upgrades");
            out.u32(self.upgrades.len() as u32);
            for upgrade in &self.upgrades {
                upgrade.encode(out);
            }
        }
        if let Some((row, kind)) = self.upgrade {
            out.string("upgrade");
            out.u16(row);
            out.u8(kind);
        }
        if self.upgrade_level != 0 || !self.slot_kinds.is_empty() {
            out.string("level");
            out.u8(self.upgrade_level);
            out.section(&self.slot_kinds);
            for part in &self.parts {
                out.string(part);
            }
        }
        if !self.recolours.is_empty() {
            out.string("recolours");
            for template in &self.recolours {
                out.string(template);
            }
        }
        // Only a tool lists it, so a catalog without keeps its digest.
        if self.tools != 0 {
            out.string("tools");
            out.u8(self.tools);
        }
        // Likewise its other properties, a kit's trap, a stealth unit and a
        // shield.
        if !self.properties.is_empty() || self.rows != 0 {
            out.string("properties");
            out.u32(self.properties.len() as u32);
            for property in &self.properties {
                property.encode(out);
            }
            write_rows(self.rows, out);
            if let Some(bare) = &self.bare {
                crate::combat::sheet::encode_weapon(bare, out);
            }
        }
        if let Some(row) = self.trap {
            out.string("trap");
            out.u8(row);
        }
        if self.stealth_unit {
            out.string("stealth");
        }
        if self.shield {
            out.string("shield");
        }
        if !self.cased_tag.is_empty() {
            out.string("cased");
            out.string(&self.cased_tag);
        }
        if !self.needs.is_empty() {
            out.string("needs");
            out.u32(self.needs.len() as u32);
            for feat in &self.needs {
                out.u16(*feat);
            }
        }
    }

    /// Its tag as `GetTag` gives it: as its data writes it.
    pub fn written_tag(&self) -> &str {
        crate::module::written(&self.tag, &self.cased_tag)
    }

    /// How many uses one of it with `stack` and `charges` has left
    /// (`ItemSpell::cost`): its stack, its charges over what a use takes,
    /// or no end (`u16::MAX`); none without a spell.
    pub fn uses_left(&self, stack: u16, charges: u8) -> u16 {
        match self.spell.map(ItemSpell::cost) {
            Some(Cost::One) => stack,
            Some(Cost::Charges(each)) => u16::from(charges / each.max(1)),
            Some(Cost::Free) => u16::MAX,
            None => 0,
        }
    }

    /// Whether one held so (worn, or carried: `worn`) may be used: one that
    /// is worn in a slot only while worn (a droid's shield).
    pub fn ready(&self, worn: bool) -> bool {
        worn || self.slots == 0
    }

    /// It with `more` properties in force besides its own: its weapon,
    /// guards, gear and limits made of them all (`properties::make`), and
    /// their rows.
    pub fn with(&self, more: &[Property], rows: u128) -> ItemFacts {
        let mut facts = self.clone();
        let all: Vec<Property> = self.properties.iter().chain(more).cloned().collect();
        let made = properties::make(self.bare, &all);
        facts.weapon = made.weapon;
        facts.guards = made.guards;
        facts.gear = made.gear;
        facts.limits = made.limits;
        facts.properties = all;
        facts.rows |= rows;
        facts
    }

    /// It made of its own properties in force (the session's reading).
    pub fn made(self) -> ItemFacts {
        self.with(&[], 0)
    }

    /// As an item's upgrades leave it: K1's `upgrades` fitted (rows as
    /// bits, `Item::upgrades`: its own properties of those rows) and TSL's
    /// `fitted` (`Item::fitted`: the upgrade items' properties, from
    /// `catalog`).
    pub fn fitted(&self, upgrades: u32, fitted: &[String], catalog: &Catalog) -> ItemFacts {
        let k1 = self
            .upgrades
            .iter()
            .filter(|upgrade| has_upgrade(upgrades, upgrade.row));
        let tsl = fitted.iter().filter_map(|template| catalog.get(template));
        let mut more = Vec::new();
        let mut rows = 0;
        for (properties, of) in k1
            .map(|upgrade| (&upgrade.properties, upgrade.rows))
            .chain(tsl.map(|part| (&part.properties, part.rows)))
        {
            more.extend(properties.iter().cloned());
            rows |= of;
        }
        if more.is_empty() && rows == 0 {
            return self.clone();
        }
        self.with(&more, rows)
    }

    /// Whether it has `itempropdef.2da` row `row` in force.
    pub fn has_row(&self, row: u8) -> bool {
        row < 128 && self.rows & 1 << row != 0
    }

    /// The upgrade of `upgrade.2da` row `row` it takes, if it takes it.
    pub fn takes(&self, row: u8) -> Option<&Upgrade> {
        self.upgrades.iter().find(|upgrade| upgrade.row == row)
    }

    /// Whether a character of race `race` may use it.
    pub fn fits(&self, race: u8) -> bool {
        self.droids
            .is_none_or(|droids| droids == (race == crate::combat::sheet::races::DROID))
    }

    /// The `spells.2da` row it casts that an item button takes (a grenade,
    /// a medpac, a shield, a stim: `combat::items::use_of`), if one.
    pub fn combat_row(&self) -> Option<u16> {
        self.spell
            .map(|spell| spell.row)
            .filter(|row| items::use_of(*row).is_some())
    }
}

/// An item's Cast Spell property (`itempropdef.2da` row 10): the
/// `spells.2da` row it casts and what a use costs (`cost`, its
/// `iprp_chargecost.2da` row).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ItemSpell {
    pub row: u16,
    pub cost: u8,
}

/// What one use of an item takes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Cost {
    /// One of its stack.
    One,
    /// Charges; the last spent, the item is gone.
    Charges(u8),
    /// Nothing (no charges a use, uses a day, unlimited).
    Free,
}

impl ItemSpell {
    /// `iprp_chargecost.2da`: 1 single use, 2 to 6 five to one charges a
    /// use, 7 none, 8 to 12 uses a day, 13 unlimited (0, random, as one).
    pub fn cost(self) -> Cost {
        match self.cost {
            2..=6 => Cost::Charges(7 - self.cost),
            7..=13 => Cost::Free,
            _ => Cost::One,
        }
    }
}

/// The item templates scripts may make, by lowercase resref: every one the
/// game has where the install lists them, else those its module's objects
/// carry and its scripts name.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Catalog {
    items: std::collections::BTreeMap<String, ItemFacts>,
}

impl Catalog {
    pub fn new(items: impl IntoIterator<Item = ItemFacts>) -> Catalog {
        Catalog {
            items: items
                .into_iter()
                .map(|facts| (facts.template.clone(), facts))
                .collect(),
        }
    }

    pub fn get(&self, template: &str) -> Option<&ItemFacts> {
        self.items.get(template)
    }

    pub fn len(&self) -> usize {
        self.items.len()
    }

    /// Every template, by resref.
    pub fn iter(&self) -> impl Iterator<Item = &ItemFacts> {
        self.items.values()
    }

    /// This catalog with `more`'s templates, which win over its own of the
    /// same name (a module's own over the shared ones).
    pub fn with(mut self, more: Catalog) -> Catalog {
        self.items.extend(more.items);
        self
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    /// The colour crystal that gives lightsaber template `saber` its
    /// colour (`ItemFacts::recolours`).
    pub fn crystal_of(&self, saber: &str) -> Option<&ItemFacts> {
        self.iter()
            .find(|crystal| crystal.recolours.iter().any(|made| made == saber))
    }

    /// The lightsaber template colour crystal `crystal` makes of a hilt of
    /// base item `base_item`, if it makes one.
    pub fn recolour<'a>(&self, crystal: &'a ItemFacts, base_item: u16) -> Option<&'a str> {
        crystal
            .recolours
            .iter()
            .find(|template| {
                self.get(template)
                    .is_some_and(|saber| saber.base_item == base_item)
            })
            .map(String::as_str)
    }

    /// Every template's facts in one fixed encoding, for a digest.
    pub(crate) fn digest_into(&self, out: &mut Writer) {
        out.u32(self.items.len() as u32);
        for facts in self.items.values() {
            facts.digest(out);
        }
    }
}

/// An item a module's object starts with: its template, the stack, the
/// slot it is equipped in (`None`: carried), whether it drops as loot, and
/// (a store's) whether it never runs out (a UTM entry's `Infinite`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Held {
    pub template: String,
    pub stack: u16,
    pub slot: Option<u8>,
    pub droppable: bool,
    pub infinite: bool,
}

impl Held {
    /// `droppable` and `infinite` as one byte (an encoding's and a
    /// digest's: one not infinite is the byte it was before stores).
    pub(crate) fn flags(&self) -> u8 {
        u8::from(self.droppable) | u8::from(self.infinite) << 1
    }
}

/// A store's terms (its UTM): what it asks over an item's value to sell it
/// (`MarkUp`, %), what it pays of the value to buy one (`MarkDown`, %),
/// and whether it sells and buys at all (`BuySellFlag` bits 0 and 1).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Terms {
    pub markup: i32,
    pub markdown: i32,
    pub sells: bool,
    pub buys: bool,
}

/// A trade under way (`OpenStore`): the store, and the terms it trades on
/// with this player (the store's markup and markdown with the call's
/// bonuses).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Trade {
    pub store: u32,
    pub terms: Terms,
}

impl Terms {
    pub(crate) fn encode(&self, out: &mut Writer) {
        out.u32(self.markup as u32);
        out.u32(self.markdown as u32);
        out.u8(u8::from(self.sells) | u8::from(self.buys) << 1);
    }
}

impl Trade {
    pub(crate) fn encode(&self, out: &mut Writer) {
        out.u32(self.store);
        self.terms.encode(out);
    }

    pub(crate) fn decode(input: &mut Reader) -> Result<Trade, DecodeError> {
        let store = input.u32()?;
        let markup = input.u32()? as i32;
        let markdown = input.u32()? as i32;
        let flags = input.u8()?;
        Ok(Trade {
            store,
            terms: Terms {
                markup,
                markdown,
                sells: flags & 1 != 0,
                buys: flags & 2 != 0,
            },
        })
    }
}

/// The bit of `upgrade.2da` row `row` in an item's upgrades fitted
/// (`Item::upgrades`); none for a row past its 32.
pub fn upgrade_bit(row: u8) -> Option<u32> {
    1u32.checked_shl(u32::from(row))
}

/// Whether upgrades `fitted` (`Item::upgrades`) hold row `row`.
pub fn has_upgrade(fitted: u32, row: u8) -> bool {
    upgrade_bit(row).is_some_and(|bit| fitted & bit != 0)
}

/// What `percent` of an item's value (its UTI's `Cost`, one of a stack)
/// comes to, in whole credits rounded down; nothing below nothing.
pub fn price(cost: u32, percent: i32) -> i32 {
    (i64::from(cost) * i64::from(percent.max(0)) / 100).min(i64::from(i32::MAX)) as i32
}

/// What every player sees of the items in play (`Snapshot::goods`, from
/// the authority's scripts): the party's gold; its items, carried and worn
/// by its members; the trades under way, by player id; and the stock of
/// the stores in them; the items lying on the area's floor, where each
/// lies (each among the items too). Items, trades and those lying in id
/// order.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Goods {
    pub gold: i32,
    pub items: Vec<(u32, Item)>,
    pub trades: Vec<(u32, Trade)>,
    pub floor: Vec<(u32, Lying)>,
}

/// Where an item lies on an area's floor (`CreateItemOnFloor`): metres,
/// game space, facing `yaw` degrees. Its holder is the area.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Lying {
    pub at: [f32; 3],
    pub yaw: f32,
}

impl Lying {
    pub(crate) fn encode(&self, out: &mut Writer) {
        for v in self.at {
            out.f32(v);
        }
        out.f32(self.yaw);
    }

    pub(crate) fn decode(input: &mut Reader) -> Result<Lying, DecodeError> {
        Ok(Lying {
            at: [input.f32()?, input.f32()?, input.f32()?],
            yaw: input.f32()?,
        })
    }
}

impl Goods {
    /// The items `holder` carries (not worn), in id order.
    pub fn carried(&self, holder: u32) -> impl Iterator<Item = (u32, &Item)> {
        self.items
            .iter()
            .filter(move |(_, item)| item.holder == holder && item.slot.is_none())
            .map(|(id, item)| (*id, item))
    }

    /// The trade player `player` has under way.
    pub fn trade(&self, player: u32) -> Option<Trade> {
        self.trades
            .iter()
            .find(|(of, _)| *of == player)
            .map(|(_, trade)| *trade)
    }

    pub(crate) fn encode(&self, out: &mut Writer) {
        out.u32(self.gold as u32);
        out.u32(self.items.len() as u32);
        for (id, item) in &self.items {
            out.u32(*id);
            item.encode(out);
        }
        out.u32(self.trades.len() as u32);
        for (player, trade) in &self.trades {
            out.u32(*player);
            trade.encode(out);
        }
        out.u32(self.floor.len() as u32);
        for (id, lying) in &self.floor {
            out.u32(*id);
            lying.encode(out);
        }
    }

    pub(crate) fn decode(input: &mut Reader) -> Result<Goods, DecodeError> {
        let gold = input.u32()? as i32;
        let mut items: Vec<(u32, Item)> = Vec::new();
        for _ in 0..input.u32()? {
            let at = input.at();
            let id = input.u32()?;
            if items.last().is_some_and(|(last, _)| *last >= id) {
                return Err(input.invalid("goods item order", at));
            }
            items.push((id, Item::decode(input)?));
        }
        let mut trades: Vec<(u32, Trade)> = Vec::new();
        for _ in 0..input.u32()? {
            let at = input.at();
            let player = input.u32()?;
            if trades.last().is_some_and(|(last, _)| *last >= player) {
                return Err(input.invalid("goods trade order", at));
            }
            trades.push((player, Trade::decode(input)?));
        }
        let mut floor: Vec<(u32, Lying)> = Vec::new();
        for _ in 0..input.u32()? {
            let at = input.at();
            let id = input.u32()?;
            if floor.last().is_some_and(|(last, _)| *last >= id) {
                return Err(input.invalid("goods floor order", at));
            }
            floor.push((id, Lying::decode(input)?));
        }
        Ok(Goods {
            gold,
            items,
            trades,
            floor,
        })
    }
}

/// `baseitems.2da`'s credits (both games' row 57): taken, a stack of them
/// is that much gold.
pub const CREDITS: u16 = 57;

/// The first item's object id: items have their own range, apart from the
/// world's entities (`SimId`s) and the module and area.
pub const FIRST_ITEM: u32 = 0x7c00_0000;
/// The party's inventory as a holder: one for the whole party, the
/// leader's in scripts' eyes (`GetItemPossessor`).
pub const PARTY: u32 = 0x7e00_0003;

/// An item in play: its template, who holds it (an object id, or
/// `PARTY`), the slot it is equipped in, its stack and charges, what
/// scripts set on it, and whether it never runs out (a store's).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Item {
    pub template: String,
    pub holder: u32,
    pub slot: Option<u8>,
    pub stack: u16,
    pub charges: u8,
    pub droppable: bool,
    pub plot: bool,
    pub identified: bool,
    pub infinite: bool,
    /// A script's `SetItemNonEquippable`: a player may not put it on.
    pub non_equippable: bool,
    /// The upgrades fitted to it (`upgrade.2da` rows as bits; KOTOR's
    /// `Upgrades`).
    pub upgrades: u32,
    /// TSL's: the upgrade item in each of its slots (`ItemFacts::
    /// slot_kinds`), by template; empty for none.
    pub fitted: Vec<String>,
}

impl Item {
    /// How many uses it has left (`ItemFacts::uses_left`).
    pub fn uses(&self, facts: &ItemFacts) -> u16 {
        facts.uses_left(self.stack, self.charges)
    }

    pub(crate) fn encode(&self, out: &mut Writer) {
        out.string(&self.template);
        out.u32(self.holder);
        out.u8(self.slot.unwrap_or(u8::MAX));
        out.u16(self.stack);
        out.u8(self.charges);
        out.u8(u8::from(self.droppable)
            | u8::from(self.plot) << 1
            | u8::from(self.identified) << 2
            | u8::from(self.infinite) << 3
            | u8::from(self.non_equippable) << 4);
        out.u32(self.upgrades);
        out.u32(self.fitted.len() as u32);
        for template in &self.fitted {
            out.string(template);
        }
    }

    pub(crate) fn decode(input: &mut Reader) -> Result<Item, DecodeError> {
        let template = input.string()?;
        let holder = input.u32()?;
        let slot = Some(input.u8()?).filter(|slot| *slot != u8::MAX);
        let stack = input.u16()?;
        let charges = input.u8()?;
        let flags = input.u8()?;
        Ok(Item {
            template,
            holder,
            slot,
            stack,
            charges,
            droppable: flags & 1 != 0,
            plot: flags & 2 != 0,
            identified: flags & 4 != 0,
            infinite: flags & 8 != 0,
            non_equippable: flags & 16 != 0,
            upgrades: input.u32()?,
            fitted: (0..input.u32()?)
                .map(|_| input.string())
                .collect::<Result<_, _>>()?,
        })
    }
}

/// The items in play by id, and which each holder has: read as the map
/// (`Deref`), changed only through `insert`, `remove` and `get_mut`, which
/// keep the holders' index, so what a holder has is found without looking
/// at every item (the items of every module left stay in play, thousands
/// by the late game).
#[derive(Clone, Default)]
pub struct Items {
    by_id: BTreeMap<u32, Item>,
    by_holder: BTreeMap<u32, BTreeSet<u32>>,
}

impl Items {
    pub fn insert(&mut self, id: u32, item: Item) -> Option<Item> {
        let holder = item.holder;
        let was = self.by_id.insert(id, item);
        if let Some(was) = &was {
            unindex(&mut self.by_holder, id, was.holder);
        }
        self.by_holder.entry(holder).or_default().insert(id);
        was
    }

    pub fn remove(&mut self, id: &u32) -> Option<Item> {
        let item = self.by_id.remove(id)?;
        unindex(&mut self.by_holder, *id, item.holder);
        Some(item)
    }

    /// The item to change; its holder's index follows it when done.
    pub fn get_mut(&mut self, id: &u32) -> Option<ItemMut<'_>> {
        let item = self.by_id.get_mut(id)?;
        Some(ItemMut {
            id: *id,
            was: item.holder,
            item,
            by_holder: &mut self.by_holder,
        })
    }

    /// The ids of what `holder` has, to change one by one.
    pub fn ids_held_by(&self, holder: u32) -> Vec<u32> {
        self.held_by(holder).map(|(id, _)| id).collect()
    }

    /// What `holder` has, by id.
    pub fn held_by(&self, holder: u32) -> impl Iterator<Item = (u32, &Item)> {
        self.by_holder
            .get(&holder)
            .into_iter()
            .flatten()
            .map(|id| (*id, &self.by_id[id]))
    }
}

fn unindex(by_holder: &mut BTreeMap<u32, BTreeSet<u32>>, id: u32, holder: u32) {
    if let Some(held) = by_holder.get_mut(&holder) {
        held.remove(&id);
        if held.is_empty() {
            by_holder.remove(&holder);
        }
    }
}

fn reindex(by_holder: &mut BTreeMap<u32, BTreeSet<u32>>, id: u32, was: u32, now: u32) {
    if was != now {
        unindex(by_holder, id, was);
        by_holder.entry(now).or_default().insert(id);
    }
}

impl std::ops::Deref for Items {
    type Target = BTreeMap<u32, Item>;

    fn deref(&self) -> &BTreeMap<u32, Item> {
        &self.by_id
    }
}

impl PartialEq for Items {
    fn eq(&self, other: &Items) -> bool {
        self.by_id == other.by_id
    }
}

impl std::fmt::Debug for Items {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.by_id.fmt(f)
    }
}

impl<'a> IntoIterator for &'a Items {
    type Item = (&'a u32, &'a Item);
    type IntoIter = std::collections::btree_map::Iter<'a, u32, Item>;

    fn into_iter(self) -> Self::IntoIter {
        self.by_id.iter()
    }
}

/// An item being changed (`Items::get_mut`): moved to another holder, it
/// is indexed under it once let go.
pub struct ItemMut<'a> {
    id: u32,
    was: u32,
    item: &'a mut Item,
    by_holder: &'a mut BTreeMap<u32, BTreeSet<u32>>,
}

impl std::ops::Deref for ItemMut<'_> {
    type Target = Item;

    fn deref(&self) -> &Item {
        self.item
    }
}

impl std::ops::DerefMut for ItemMut<'_> {
    fn deref_mut(&mut self) -> &mut Item {
        self.item
    }
}

impl Drop for ItemMut<'_> {
    fn drop(&mut self) {
        reindex(self.by_holder, self.id, self.was, self.item.holder);
    }
}

/// What a sheet's equipment adds: a weapon in each hand, the body's armour,
/// the guards of everything worn (on a sheet read without its items).
pub fn dress<'a>(sheet: Sheet, worn: impl IntoIterator<Item = (u8, &'a ItemFacts)>) -> Sheet {
    rewear(sheet, &[], &worn.into_iter().collect::<Vec<_>>())
}

/// Whether `left` may be held in the off hand with `right` in the main
/// hand (KOTOR's pairing; the equip screen offers only these): both
/// one-handed and not double-bladed, both melee or both ranged. No saber
/// beside a pistol, nothing beside a two-handed weapon [verify: the K1
/// executable's own test].
pub fn pairs(right: &Weapon, left: &Weapon) -> bool {
    use crate::combat::sheet::build;
    let single = |weapon: &Weapon| {
        weapon.build & build::ONE_HANDED != 0 && weapon.build & build::DOUBLE == 0
    };
    single(right) && single(left) && right.delivery.ranged() == left.delivery.ranged()
}

/// `sheet` as wearing `after` instead of `before`: once either hand's item
/// changed, both hands take their items' weapons (none for an empty one; a
/// sheet no hand item ever touched keeps its own, a dev sheet's), the body's armour its defence and
/// DEX cap, and the guards, what they give the wearer (`Sheet::gear`) and
/// whether a stealth unit is worn (`flags::STEALTH_UNIT`) are those of
/// everything worn after (`slots::worn`: not the second weapon set).
pub fn rewear(mut sheet: Sheet, before: &[(u8, &ItemFacts)], after: &[(u8, &ItemFacts)]) -> Sheet {
    let after: Vec<(u8, &ItemFacts)> = after
        .iter()
        .copied()
        .filter(|(slot, _)| slots::worn(*slot))
        .collect();
    let after = after.as_slice();
    let changed = |slot: u8| in_slot(before, slot) != in_slot(after, slot);
    let hands = [slots::RIGHT_WEAPON, slots::LEFT_WEAPON];
    if hands.into_iter().any(changed) {
        for (hand, slot) in hands.into_iter().enumerate() {
            sheet.weapons[hand] = in_slot(after, slot).and_then(|facts| facts.weapon);
        }
    }
    if changed(slots::BODY) {
        let armour = |worn| in_slot(worn, slots::BODY).and_then(|facts| facts.armour);
        let (was, now) = (
            armour(before).unwrap_or(Armour::NONE),
            armour(after).unwrap_or(Armour::NONE),
        );
        sheet.defence = sheet
            .defence
            .saturating_sub(was.defence)
            .saturating_add(now.defence);
        sheet.dex_cap = now.dex_cap;
    }
    let mut guards = Guards::default();
    for (_, facts) in after {
        add_guards(&mut guards, &facts.guards);
    }
    sheet.guards = guards;
    sheet.gear = after
        .iter()
        .fold(Gear::default(), |gear, (_, facts)| gear.with(&facts.gear));
    let unit = crate::combat::sheet::flags::STEALTH_UNIT;
    if after.iter().any(|(_, facts)| facts.stealth_unit) {
        sheet.flags |= unit;
    } else {
        sheet.flags &= !unit;
    }
    sheet
}

/// The item worn in `slot`, if any.
fn in_slot<'a>(worn: &[(u8, &'a ItemFacts)], slot: u8) -> Option<&'a ItemFacts> {
    worn.iter()
        .find(|(at, _)| *at == slot)
        .map(|(_, facts)| *facts)
}

/// Guards adding up: immunity and vulnerability within ±100, the best
/// resistance.
fn add_guards(guards: &mut Guards, more: &Guards) {
    for row in 0..DAMAGE_TYPES {
        let immunity = i16::from(guards.immunity[row]) + i16::from(more.immunity[row]);
        guards.immunity[row] = immunity.clamp(-100, 100) as i8;
        guards.resist[row] = guards.resist[row].max(more.resist[row]);
    }
}

/// What `held` (items and their uses left) gives a sheet of race `race` to
/// use in a fight: its consumables, the items whose Cast Spell property
/// casts a row with an impact script (`scripted`: the games' `spells.2da`
/// `impactscript`, which does what the item does) that an item button
/// takes (`ItemFacts::combat_row`) and that are for its race
/// (`ItemFacts::fits`), counted by their uses, the first `items::KINDS`
/// kinds.
pub fn usable<'a>(
    race: u8,
    held: impl IntoIterator<Item = (u16, &'a ItemFacts)>,
    scripted: impl Fn(u16) -> bool,
) -> [Option<Carried>; items::KINDS] {
    let mut kinds: Vec<Carried> = Vec::new();
    for (uses, facts) in held {
        let Some(row) = facts
            .combat_row()
            .filter(|row| uses > 0 && facts.fits(race) && scripted(*row))
        else {
            continue;
        };
        let count = uses.min(255) as u8;
        match kinds.iter_mut().find(|kind| kind.row == row) {
            Some(kind) => kind.count = kind.count.saturating_add(count),
            None => kinds.push(Carried { row, count }),
        }
    }
    std::array::from_fn(|at| kinds.get(at).copied())
}
