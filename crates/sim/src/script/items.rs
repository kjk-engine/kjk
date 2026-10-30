//! What scripts do with items (I19, `sim::inventory`): make them on an
//! object (stacking onto a stack of the same template, as KOTOR's
//! inventory does), walk an inventory, equip and unequip, give and take,
//! destroy. A party member's carried items are the party's (`PARTY`);
//! its equipment its own.
//!
//! An equipment change dresses the character anew at the end of the step
//! (`Out::worn_before` keeps what it wore before the step's first change;
//! `inventory::rewear`), so every view gets the new sheet in the snapshot.

use script_ncs::vm::ObjectId;

use super::state::State;
use super::{Cast, Change, Library, Out, View, key, workbench};
use crate::action::{Action, Fitting};
use crate::combat::passive;
use crate::combat::sheet::Weapon;
use crate::inventory::properties::Wearer;
use crate::inventory::{
    CREDITS, CRYSTAL, Catalog, Cost, FIRST_ITEM, Held, Item, ItemFacts, PARTY, has_upgrade, pairs,
    properties, rewear, slots, upgrade_bit,
};
use crate::notice::Notice;

/// An item worn: its slot, template and upgrades fitted (K1's rows as
/// bits, TSL's upgrade items by slot).
pub(crate) type Worn = (u8, String, u32, Vec<String>);

/// Whether `id` is an item's object id.
pub fn is_item(id: ObjectId) -> bool {
    (FIRST_ITEM..super::MODULE.0).contains(&id.0)
}

/// Whether `item` is a stack of `template` that `holder` carries and more
/// may join (not one that never runs out).
fn joinable(item: &Item, template: &str, holder: u32) -> bool {
    item.holder == holder && item.slot.is_none() && !item.infinite && item.template == template
}

/// Where `target`'s carried items are: the party's inventory for a party
/// member, else its own.
pub fn holder(view: &View, target: ObjectId) -> u32 {
    let party = view
        .thing(target)
        .and_then(|thing| thing.character)
        .is_some_and(|character| character.party);
    if party { PARTY } else { target.0 }
}

/// The `spells.2da` row an item of `facts` casts when a character of race
/// `race` uses it, on itself (`on_self`) or on another: a spell that is no
/// hostile one on itself (a medpac, a shield), a hostile one on another (a
/// droid's weapon), by those the item is for (`ItemFacts::fits`).
pub fn spell_for(library: &Library, facts: &ItemFacts, race: u8, on_self: bool) -> Option<u16> {
    let spell = facts.spell?;
    (facts.fits(race) && library.spell(spell.row).hostile != on_self).then_some(spell.row)
}

/// Whether `seat`, whose body is `body`, may change the upgrades of
/// `item`: one the party carries, or one a party member wears that the
/// seat may dress (`may_dress`).
pub(crate) fn may_upgrade(
    view: &View,
    state: &State,
    seat: crate::input::Seat,
    body: ObjectId,
    item: u32,
) -> Result<(), Notice> {
    match state.items.get(&item) {
        Some(held) if held.slot.is_some() => {
            may_dress(view, state, seat, body, ObjectId(held.holder))
        }
        Some(held) if held.holder == PARTY => Ok(()),
        _ => Err(Notice::CannotUpgrade),
    }
}

/// Whether spell `row` cast from `from` reaches `to` (metres): its
/// `spells.2da` range, and a metre more for the bodies' size [verify in
/// K1's exe].
pub fn within(library: &Library, row: u16, from: [f32; 3], to: [f32; 3]) -> bool {
    let reach = f32::from(library.spell(row).reach_dm) / 10.0 + 1.0;
    super::flat(from, to) <= reach
}

/// Whether `seat`, whose body is `body`, may change what party member
/// `who` wears (I30 P14): its own body; for the host, also a companion of
/// the party table standing here that no seat plays. Else why not:
/// another seat plays it, or nobody does and only the host dresses it.
pub(crate) fn may_dress(
    view: &View,
    state: &State,
    seat: crate::input::Seat,
    body: ObjectId,
    who: ObjectId,
) -> Result<(), Notice> {
    if who == body {
        return Ok(());
    }
    let character = view
        .thing(who)
        .and_then(|thing| thing.character)
        .ok_or(Notice::CannotEquip)?;
    if let Some(by) = character.driver {
        return Err(Notice::NotYours { by: Some(by) });
    }
    let companion = state
        .party
        .available
        .values()
        .any(|npc| npc.object == Some(who.0));
    match (companion, seat == crate::world::LEADER_SEAT) {
        (true, true) => Ok(()),
        (true, false) => Err(Notice::NotYours { by: None }),
        (false, _) => Err(Notice::CannotEquip),
    }
}

/// `who` as an equip's use limits ask of it (`properties::Wearer`): its
/// feats and scores with what it wears and what runs on it, its alignment
/// as this step leaves it; the module's game's rules.
pub(crate) fn wearer(view: &View, state: &State, out: &Out, who: ObjectId) -> Wearer {
    let thing = view.thing(who);
    let character = thing.and_then(|thing| thing.character);
    let sheet = character.map(|character| crate::combat::Sheet {
        good_evil: out.sheet(who, character.worn).good_evil,
        ..character.sheet
    });
    Wearer {
        good_evil: sheet.map_or(crate::combat::sheet::NEUTRAL, |sheet| sheet.good_evil),
        classes: character.map_or([u8::MAX; 2], |character| {
            character.classes.map(|(class, _)| class)
        }),
        race: sheet.map_or(0, |sheet| sheet.race),
        subrace: character.map_or(0, |character| character.subrace),
        feats: sheet.map_or_else(Default::default, |sheet| sheet.feats),
        gender: character.map_or(0, |character| character.gender),
        abilities: sheet.map_or([10; 6], |sheet| sheet.abilities),
        player: view.pcs(&state.party).contains(&who),
        tag: thing
            .and_then(|thing| thing.facts.as_ref())
            .map_or_else(String::new, |facts| facts.tag.clone()),
        tsl: crate::combat::passive::Game::named(&view.module.game)
            == crate::combat::passive::Game::Tsl,
    }
}

impl State {
    /// A new item of `template` with `holder`, unless the template is not
    /// known. A carried stackable item joins a stack of the same template
    /// the holder has room on (the rest a new stack); the item made or
    /// joined.
    pub fn make_item(
        &mut self,
        catalog: &Catalog,
        template: &str,
        holder: u32,
        stack: u16,
        slot: Option<u8>,
    ) -> Option<ObjectId> {
        let template = key(template);
        let facts = self.item_facts(catalog, &template)?.clone();
        let facts = &facts;
        let mut left = stack.max(1);
        let mut joined = None;
        if slot.is_none() && facts.stacking > 1 {
            let stacks: Vec<u32> = self
                .items
                .held_by(holder)
                .filter(|(_, item)| joinable(item, &template, holder))
                .map(|(id, _)| id)
                .collect();
            for id in stacks {
                let Some(mut item) = self.items.get_mut(&id) else {
                    continue;
                };
                let room = facts.stacking.saturating_sub(item.stack).min(left);
                item.stack += room;
                left -= room;
                joined = Some(ObjectId(id));
                if left == 0 {
                    return joined;
                }
            }
        }
        let rest = self.new_item(facts, holder, left, slot);
        Some(joined.unwrap_or(rest))
    }

    /// One of `item`'s stack, as its own item where the stack is more than
    /// one (the rest left where it was, one fewer), else the item itself:
    /// what equipping wears, one of a stack, as `wear` takes one for a
    /// player.
    pub(crate) fn one_of(&mut self, item: ObjectId) -> ObjectId {
        let Some(from) = self
            .items
            .get(&item.0)
            .filter(|from| from.stack > 1 && from.slot.is_none())
        else {
            return item;
        };
        let one = Item {
            stack: 1,
            ..from.clone()
        };
        if let Some(mut rest) = self.items.get_mut(&item.0) {
            rest.stack -= 1;
        }
        let id = FIRST_ITEM + self.next_item;
        self.next_item += 1;
        self.items.insert(id, one);
        ObjectId(id)
    }

    /// A new item of `facts`' template with `holder`, a stack of its own.
    fn new_item(
        &mut self,
        facts: &ItemFacts,
        holder: u32,
        stack: u16,
        slot: Option<u8>,
    ) -> ObjectId {
        let id = FIRST_ITEM + self.next_item;
        self.next_item += 1;
        self.items.insert(
            id,
            Item {
                template: facts.template.clone(),
                holder,
                slot,
                stack,
                charges: facts.charges,
                droppable: true,
                plot: facts.plot,
                identified: true,
                infinite: false,
                non_equippable: false,
                upgrades: 0,
                fitted: facts.parts.clone(),
            },
        );
        ObjectId(id)
    }

    /// A new item of `template` lying on the floor of `module`'s area at
    /// `at` (metres) facing `yaw` (degrees), held by the area: its
    /// template's stack, never joining another; none if the template is
    /// not known.
    pub(crate) fn lay_item(
        &mut self,
        catalog: &Catalog,
        template: &str,
        module: &str,
        (at, yaw): ([f32; 3], f32),
    ) -> Option<ObjectId> {
        let facts = self.item_facts(catalog, &key(template))?.clone();
        let made = self.new_item(&facts, super::AREA.0, facts.stack.max(1), None);
        let lying = crate::inventory::Lying { at, yaw };
        self.floor.insert(made.0, (module.to_string(), lying));
        Some(made)
    }

    /// The items lying on the floor of `module`'s area, by id: those the
    /// area still holds.
    pub fn lying_in<'a>(
        &'a self,
        module: &'a str,
    ) -> impl Iterator<Item = (u32, crate::inventory::Lying)> + 'a {
        self.floor
            .keys()
            .filter_map(move |id| Some((*id, self.lying(module, ObjectId(*id))?)))
    }

    /// Where `item` lies on the floor of `module`'s area, if it does.
    pub(crate) fn lying(&self, module: &str, item: ObjectId) -> Option<crate::inventory::Lying> {
        let (of, lying) = self.floor.get(&item.0)?;
        let held = self.items.get(&item.0)?.holder == super::AREA.0;
        (of == module && held).then_some(*lying)
    }

    /// Whether a stack of `template` that `holder` carries has room for
    /// one more.
    pub(super) fn joins(&self, catalog: &Catalog, template: &str, holder: u32) -> bool {
        self.item_facts(catalog, template).is_some_and(|facts| {
            facts.stacking > 1
                && self.items.held_by(holder).any(|(_, item)| {
                    joinable(item, template, holder) && item.stack < facts.stacking
                })
        })
    }

    /// `object`'s starting items (a module's first visit, or as it is
    /// made). One that never runs out (a store's) is one item of its own,
    /// the first of its template there. Only the items made take the
    /// entry's flags: one joining a stack listed before keeps that stack's,
    /// as the engines add a loaded item to an inventory. A creature's
    /// components and chemicals are its own counts (`pool_carried`).
    pub(crate) fn stock(&mut self, library: &Library, object: u32, held: &[Held], creature: bool) {
        let catalog = library.items();
        for held in held {
            let first_new = FIRST_ITEM + self.next_item;
            if held.infinite {
                let had = self
                    .items
                    .held_by(object)
                    .any(|(_, item)| item.infinite && item.template == held.template);
                if let Some(facts) = catalog.get(&held.template).filter(|_| !had) {
                    self.new_item(facts, object, held.stack.max(1), None);
                }
            } else {
                self.make_item(catalog, &held.template, object, held.stack, held.slot);
            }
            let made: Vec<u32> = self.items.range(first_new..).map(|(id, _)| *id).collect();
            for id in made {
                if let Some(mut item) = self.items.get_mut(&id) {
                    item.droppable = held.droppable;
                    item.infinite = held.infinite;
                }
            }
        }
        if creature {
            self.pool_carried(library, object);
        }
    }

    /// The items `holder` carries (not equipped), in id order.
    pub fn carried(&self, holder: u32) -> Vec<ObjectId> {
        self.items
            .held_by(holder)
            .filter(|(_, item)| item.slot.is_none())
            .map(|(id, _)| ObjectId(id))
            .collect()
    }

    /// The item `who` has equipped in `slot`.
    pub fn in_slot(&self, who: u32, slot: u8) -> Option<ObjectId> {
        self.items
            .held_by(who)
            .find(|(_, item)| item.slot == Some(slot))
            .map(|(id, _)| ObjectId(id))
    }

    /// What `who` wears: (slot, template, upgrades fitted), by slot.
    fn worn(&self, who: u32) -> Vec<Worn> {
        let mut worn: Vec<Worn> = self
            .items
            .held_by(who)
            .filter_map(|(_, item)| {
                Some((
                    item.slot?,
                    item.template.clone(),
                    item.upgrades,
                    item.fitted.clone(),
                ))
            })
            .collect();
        worn.sort();
        worn
    }

    /// Puts `item` on `who` in `slot`, what was there back with `back` (its
    /// carried items' holder); keeps what `who` wore before in `out`.
    pub(crate) fn equip(&mut self, out: &mut Out, item: ObjectId, who: u32, slot: u8, back: u32) {
        if !self.items.contains_key(&item.0) {
            return;
        }
        self.wearing(out, who);
        if let Some(was) = self.in_slot(who, slot).filter(|was| *was != item)
            && let Some(mut was) = self.items.get_mut(&was.0)
        {
            (was.holder, was.slot) = (back, None);
        }
        if let Some(previous) = self.items.get(&item.0).map(|item| (item.holder, item.slot))
            && let (from, Some(_)) = previous
        {
            self.wearing(out, from);
        }
        if let Some(mut item) = self.items.get_mut(&item.0) {
            (item.holder, item.slot) = (who, Some(slot));
        }
    }

    /// Moves `item` to `holder`'s carried items (unequipping it).
    pub(crate) fn carry(&mut self, out: &mut Out, item: ObjectId, holder: u32) {
        let Some((from, slot)) = self.items.get(&item.0).map(|item| (item.holder, item.slot))
        else {
            return;
        };
        if slot.is_some() {
            self.wearing(out, from);
        }
        if let Some(mut item) = self.items.get_mut(&item.0) {
            (item.holder, item.slot) = (holder, None);
        }
    }

    /// One of `item` to `holder`'s carried items: the item itself when it
    /// is the last of its stack and no stack there takes it, else one made
    /// there (one fewer left, unless it never runs out).
    pub(crate) fn hand_over(&mut self, catalog: &Catalog, item: u32, holder: u32) {
        let Some(from) = self.items.get(&item) else {
            return;
        };
        let (template, infinite, stack) = (from.template.clone(), from.infinite, from.stack);
        if !infinite && stack <= 1 && !self.joins(catalog, &template, holder) {
            if let Some(mut moved) = self.items.get_mut(&item) {
                moved.holder = holder;
                moved.slot = None;
            }
            return;
        }
        if !infinite {
            self.use_one(item);
        }
        self.make_item(catalog, &template, holder, 1, None);
    }

    /// One of `item` gone: its stack one less, the last of it out of play
    /// (a credit taken, a key or a spike used up).
    pub(crate) fn use_one(&mut self, item: u32) {
        let Some(mut left) = self.items.get_mut(&item) else {
            return;
        };
        if left.stack > 1 {
            left.stack -= 1;
            return;
        }
        drop(left);
        self.items.remove(&item);
    }

    /// The first item casting `spells.2da` row `row` with a use left, in id
    /// order, that `holder` carries and may use so (`ItemFacts::ready`) or
    /// `user` wears.
    pub(crate) fn casting(
        &self,
        catalog: &Catalog,
        holder: u32,
        user: ObjectId,
        row: u16,
    ) -> Option<u32> {
        let casts = |(_, item): &(u32, &Item)| {
            self.item_facts(catalog, &item.template)
                .is_some_and(|facts| {
                    facts.ready(item.slot.is_some())
                        && facts.spell.is_some_and(|spell| spell.row == row)
                        && item.uses(facts) > 0
                })
        };
        let first = |holder: u32, worn: bool| {
            self.items
                .held_by(holder)
                .filter(|(_, item)| item.slot.is_some() == worn)
                .find(casts)
                .map(|(id, _)| id)
        };
        first(holder, false)
            .into_iter()
            .chain(first(user.0, true))
            .min()
    }

    /// The item character `who` may cast `spells.2da` row `row` from, as a
    /// talent (an item's spell is one: the AI's `GetCreatureTalent…` finds
    /// a droid's utility item so): one it carries or wears with a use left
    /// (`casting`), for its race.
    pub(crate) fn item_talent(
        &self,
        catalog: &Catalog,
        view: &View,
        who: ObjectId,
        row: u16,
    ) -> Option<u32> {
        let race = view.thing(who)?.character?.race;
        let item = self.casting(catalog, holder(view, who), who, row)?;
        let facts = self.item_facts(catalog, &self.items.get(&item)?.template)?;
        facts.fits(race).then_some(item)
    }

    /// One use of `item` (`ItemSpell::cost`): one of its stack, or its
    /// charges, the item gone with the last (K1's "automatically discarded
    /// after all available charges are consumed"); whether it had the use.
    pub(crate) fn spend(&mut self, catalog: &Catalog, out: &mut Out, item: u32) -> bool {
        let Some(held) = self.items.get(&item) else {
            return false;
        };
        let Some(spell) = self
            .item_facts(catalog, &held.template)
            .and_then(|facts| facts.spell)
        else {
            return false;
        };
        let left = match spell.cost() {
            Cost::Free => return true,
            Cost::One => held.stack.saturating_sub(1),
            Cost::Charges(each) if held.charges >= each => u16::from(held.charges - each),
            Cost::Charges(_) => return false,
        };
        if left == 0 {
            self.destroy_item(out, ObjectId(item));
        } else if let Some(mut held) = self.items.get_mut(&item) {
            match spell.cost() {
                Cost::Charges(_) => held.charges = left as u8,
                _ => held.stack = left,
            }
        }
        true
    }

    /// Item `item` used by character `who` on `target` (`Action::UseItem`,
    /// and the AI's item talents): one its holder carries that is not worn
    /// (a medpac, a stim), or one it wears (a droid's utility item, its
    /// shield), whose spell it may cast on that target (`spell_for`) and
    /// whose spell has an impact script to do what it does; else it is
    /// refused and kept. A kind one of the item buttons takes, used on
    /// itself, is used as that button's press would (`Change::UseItem`:
    /// its use time, its script cast and one use taken as the use lands,
    /// `combat::items::press`); any other has one use taken (`spend`) and
    /// is cast now for the step's scripts.
    pub(crate) fn use_item(
        &mut self,
        library: &Library,
        view: &View,
        out: &mut Out,
        who: ObjectId,
        item: u32,
        target: ObjectId,
    ) -> Result<Option<Cast>, Notice> {
        let catalog = library.items();
        let race = view
            .thing(who)
            .and_then(|thing| thing.character)
            .ok_or(Notice::CannotUse)?
            .race;
        let at = view
            .thing(target)
            .filter(|thing| thing.character.is_some())
            .ok_or(Notice::CannotUse)?
            .position;
        let from = view.thing(who).map_or(at, |thing| thing.position);
        let held = self.items.get(&item).ok_or(Notice::CannotUse)?;
        let facts = self
            .item_facts(catalog, &held.template)
            .ok_or(Notice::CannotUse)?;
        let carried = held.holder == holder(view, who) && held.slot.is_none();
        let worn = held.holder == who.0 && held.slot.is_some();
        let row = spell_for(library, facts, race, target == who)
            .filter(|_| (carried || worn) && facts.ready(worn))
            .filter(|row| target == who || within(library, *row, from, at))
            .ok_or(Notice::CannotUse)?;
        let pressed = target == who
            && crate::combat::items::use_of(row)
                .is_some_and(|known| known != crate::combat::items::Use::Grenade);
        let shield = facts.shield;
        // A kind a button takes leaves the goods as its use lands
        // (`combat::items::press`); any other now.
        if library.impact(row).is_none() || (!pressed && !self.spend(catalog, out, item)) {
            return Err(Notice::CannotUse);
        }
        // TSL: a party member using an item in a duel that forbids it.
        if super::in_party(view, self, who)
            && let Some(broken) = super::party::forfeit::using(self.party.forfeit, shield)
        {
            super::forfeit(view, self, broken);
        }
        if pressed {
            out.changes.push(Change::UseItem { who, row });
            return Ok(None);
        }
        Ok(Some(Cast {
            caster: who,
            row,
            target,
            at,
            caught: Vec::new(),
        }))
    }

    /// What fitting `upgrade` (one the party carries) into `item` comes to
    /// (`Action::Fit`; whose it may be is `may_upgrade`'s): a colour crystal
    /// makes a lightsaber its colour's of its hilt (`ItemFacts::recolours`),
    /// the crystal it had back to the party, what is fitted kept; else an
    /// upgrade goes into it (one of the party's): TSL's into a slot of its
    /// kind (`ItemFacts::slot_kinds`), the one `replace` names or the first
    /// empty, and not of an upgrade level above the item's (a crystal of
    /// any, as TSL's bench lists them, `CRYSTAL`); K1's of a row
    /// and kind the item takes, the one of that kind `replace` names back to
    /// the party, which a full kind needs (`properties::room`). Its wearer
    /// is dressed anew.
    pub(crate) fn fit(
        &mut self,
        catalog: &Catalog,
        out: &mut Out,
        item: u32,
        upgrade: u32,
        replace: Option<u8>,
    ) -> Result<(), Notice> {
        let refused = Notice::CannotUpgrade;
        let target = self.items.get(&item).ok_or(refused.clone())?;
        let facts = self
            .item_facts(catalog, &target.template)
            .ok_or(refused.clone())?
            .clone();
        let fitted = target.upgrades;
        let in_slots = target.fitted.clone();
        let part = self
            .items
            .get(&upgrade)
            .filter(|part| part.holder == PARTY && part.slot.is_none() && upgrade != item)
            .and_then(|part| self.item_facts(catalog, &part.template))
            .ok_or(refused.clone())?
            .clone();
        if let Some(colour) = catalog
            .recolour(&part, facts.base_item)
            .filter(|template| *template != facts.template)
        {
            let colour = colour.to_string();
            let had = catalog
                .crystal_of(&facts.template)
                .map(|crystal| crystal.template.clone());
            self.refit(out, item, |held| held.template = colour);
            if let Some(had) = had {
                self.make_item(catalog, &had, PARTY, 1, None);
            }
        } else if !facts.slot_kinds.is_empty() {
            let (_, kind) = part.upgrade.ok_or(refused.clone())?;
            if part.upgrade_level > facts.upgrade_level && kind != CRYSTAL {
                return Err(refused);
            }
            let of_kind = |slot: &usize| facts.slot_kinds[*slot] == kind;
            let empty = |slot: &usize| in_slots.get(*slot).is_none_or(String::is_empty);
            let slot = match replace.map(usize::from) {
                Some(slot) if slot < facts.slot_kinds.len() && of_kind(&slot) => slot,
                Some(_) => return Err(refused),
                None => (0..facts.slot_kinds.len())
                    .filter(of_kind)
                    .find(empty)
                    .ok_or(refused)?,
            };
            if !empty(&slot) {
                self.unfit(catalog, out, item, slot as u8)?;
            }
            let slots = facts.slot_kinds.len();
            self.refit(out, item, |held| {
                held.fitted.resize(slots, String::new());
                held.fitted[slot] = part.template.clone();
            });
        } else {
            let (row, kind) = part
                .upgrade
                .and_then(|(row, kind)| Some((u8::try_from(row).ok()?, kind)))
                .ok_or(refused.clone())?;
            let bit = upgrade_bit(row)
                .filter(|bit| fitted & bit == 0)
                .filter(|_| facts.takes(row).is_some_and(|takes| takes.kind == kind))
                .ok_or(refused.clone())?;
            let of_kind: Vec<u8> = facts
                .upgrades
                .iter()
                .filter(|other| other.kind == kind && has_upgrade(fitted, other.row))
                .map(|other| other.row)
                .collect();
            let out_row = replace.filter(|row| of_kind.contains(row));
            if out_row.is_none() && of_kind.len() >= properties::room(kind) {
                return Err(refused);
            }
            if let Some(row) = out_row {
                self.unfit(catalog, out, item, row)?;
            }
            self.refit(out, item, |held| held.upgrades |= bit);
        }
        self.use_one(upgrade);
        Ok(())
    }

    /// An upgrade taken out of `item` and back to the party
    /// (`Action::Unfit`), its wearer dressed anew: TSL's in slot `row` of
    /// one with slots, else K1's of `upgrade.2da` row `row`.
    pub(crate) fn unfit(
        &mut self,
        catalog: &Catalog,
        out: &mut Out,
        item: u32,
        row: u8,
    ) -> Result<(), Notice> {
        let held = self.items.get(&item).ok_or(Notice::CannotUpgrade)?;
        let facts = self
            .item_facts(catalog, &held.template)
            .ok_or(Notice::CannotUpgrade)?;
        let template = if facts.slot_kinds.is_empty() {
            let bit = upgrade_bit(row).ok_or(Notice::CannotUpgrade)?;
            let template = facts
                .takes(row)
                .filter(|_| held.upgrades & bit != 0)
                .map(|upgrade| upgrade.template.clone())
                .ok_or(Notice::CannotUpgrade)?;
            self.refit(out, item, |held| held.upgrades &= !bit);
            template
        } else {
            let slot = usize::from(row);
            let template = held
                .fitted
                .get(slot)
                .filter(|template| !template.is_empty())
                .cloned()
                .ok_or(Notice::CannotUpgrade)?;
            self.refit(out, item, |held| held.fitted[slot].clear());
            template
        };
        self.make_item(catalog, &template, PARTY, 1, None);
        Ok(())
    }

    /// What an upgrade bench's Assemble comes to (`Action::Assemble`; whose
    /// the item may be is `may_upgrade`'s): its changes made in turn
    /// (`fit`, `unfit`), every one or, one refused, none. TSL's item (one
    /// with slots) is off its wearer while on TSL's bench: back on after
    /// only if `wearer`, who wore it, may wear it as it now is (its feats,
    /// its limits), else it stays with the party.
    pub(crate) fn assemble(
        &mut self,
        catalog: &Catalog,
        out: &mut Out,
        (item, changes): (u32, &[Fitting]),
        wearer: Option<&Wearer>,
    ) -> Result<(), Notice> {
        let before = (self.items.clone(), self.next_item);
        let worn = self
            .items
            .get(&item)
            .is_some_and(|held| held.slot.is_some());
        let made = changes.iter().try_for_each(|change| match *change {
            Fitting::In { upgrade, replace } => self.fit(catalog, out, item, upgrade, replace),
            Fitting::Out(row) => self.unfit(catalog, out, item, row),
        });
        if let Err(refused) = made {
            (self.items, self.next_item) = before;
            return Err(refused);
        }
        let held = self.items.get(&item).ok_or(Notice::CannotUpgrade)?;
        let facts = self
            .item_facts(catalog, &held.template)
            .ok_or(Notice::CannotUpgrade)?;
        if let Some(wearer) = wearer.filter(|_| worn && !facts.slot_kinds.is_empty()) {
            let now = facts.fitted(held.upgrades, &held.fitted, catalog);
            let wears = passive::may_equip(&wearer.feats, &now.needs)
                && properties::may_wear(&now.limits, wearer).is_ok();
            if !wears {
                self.carry(out, ObjectId(item), PARTY);
            }
        }
        Ok(())
    }

    /// `change` made to `item`, what its wearer wore noted first.
    fn refit(&mut self, out: &mut Out, item: u32, change: impl FnOnce(&mut Item)) {
        if let Some((holder, Some(_))) = self.items.get(&item).map(|held| (held.holder, held.slot))
        {
            self.wearing(out, holder);
        }
        if let Some(mut held) = self.items.get_mut(&item) {
            change(&mut held);
        }
    }

    /// One of `item`, carried by the party, into `container` (a player's
    /// put, `world::containers`); anything else stays where it is.
    pub(crate) fn put_in(&mut self, item: u32, container: u32) {
        let carried = self
            .items
            .get(&item)
            .is_some_and(|held| held.holder == PARTY && held.slot.is_none());
        if carried && let Some(mut held) = self.items.get_mut(&item) {
            held.holder = container;
        }
    }

    /// What `container` holds that a player may take, in id order: what it
    /// carries and drops (its template's items, what scripts gave it, what
    /// the party put in). The loot window lists these
    /// (`Goods::carried`, a container held open).
    pub fn loot(&self, container: u32) -> impl Iterator<Item = (u32, &Item)> {
        self.items
            .held_by(container)
            .filter(|(_, item)| item.slot.is_none() && item.droppable)
    }

    /// The whole stack of `item`, which `container` holds, to the party:
    /// what a player took from it (`world::containers`). Credits are the
    /// party's gold, a credit each; components and chemicals its pools,
    /// as every item the party receives (`pool_carried`): what they gained.
    pub(crate) fn take_item(
        &mut self,
        library: &Library,
        container: u32,
        item: u32,
    ) -> workbench::Acquired {
        if self.loot(container).any(|(id, _)| id == item) {
            self.hand_stack(library.items(), item);
        }
        self.pool_carried(library, PARTY)
    }

    /// A take all: everything `container` holds that may be taken
    /// (`loot`), as `take_item` takes each. Whether anything went, and
    /// what the pools gained.
    pub(crate) fn take_rest(
        &mut self,
        library: &Library,
        container: u32,
    ) -> (bool, workbench::Acquired) {
        let rest: Vec<u32> = self.loot(container).map(|(id, _)| id).collect();
        for &item in &rest {
            self.hand_stack(library.items(), item);
        }
        (!rest.is_empty(), self.pool_carried(library, PARTY))
    }

    /// `item`'s whole stack to the party: credits as its gold, the rest
    /// joining its stacks (`hand_over`).
    fn hand_stack(&mut self, catalog: &Catalog, item: u32) {
        let Some(held) = self.items.get(&item) else {
            return;
        };
        let stack = held.stack;
        let credits = self
            .item_facts(catalog, &held.template)
            .is_some_and(|facts| facts.base_item == CREDITS);
        if credits {
            self.gold = self.gold.saturating_add(i32::from(stack));
            self.items.remove(&item);
        } else {
            for _ in 0..stack.max(1) {
                self.hand_over(catalog, item, PARTY);
            }
        }
    }

    /// What an equipping action on `who` (whose feats are `feats`) comes to
    /// (`Action::Equip`, `Unequip`, `EquipOn`, `UnequipFrom`; whose it may
    /// be is `may_dress`'s): one of an item the party carries that fits
    /// `slot` (a second-set hand by its first-set hand's rules,
    /// `slots::rules`) and whose feats `who` has worn there, what was there back
    /// with the party; or the item in `slot` back with the party. A refusal
    /// is for its player.
    pub(crate) fn wear(
        &mut self,
        catalog: &Catalog,
        out: &mut Out,
        (who, wearer): (u32, &Wearer),
        action: Action,
    ) -> Result<(), Notice> {
        let feats = &wearer.feats;
        match action {
            Action::Equip { item, slot } | Action::EquipOn { item, slot, .. } => {
                let (stack, facts) = self
                    .items
                    .get(&item)
                    .filter(|carried| {
                        carried.holder == PARTY && carried.slot.is_none() && !carried.non_equippable
                    })
                    .and_then(|carried| {
                        let facts = self.item_facts(catalog, &carried.template)?;
                        // As its upgrades leave it: a fitted crystal's limits
                        // are its own.
                        Some((
                            carried.stack,
                            facts.fitted(carried.upgrades, &carried.fitted, catalog),
                        ))
                    })
                    .filter(|(_, facts)| {
                        slot < slots::COUNT && facts.slots & 1 << slots::rules(slot) != 0
                    })
                    // A droid wears only what is for droids, a human only what
                    // is for humans (`droidorhuman`): T3-M4 takes no forearm
                    // band nor stealth unit, nor the Exile a droid's device.
                    .filter(|(_, facts)| facts.fits(wearer.race))
                    .ok_or(Notice::CannotEquip)?;
                if !passive::may_equip(feats, &facts.needs) {
                    return Err(Notice::Line(passive::NOT_PROFICIENT));
                }
                properties::may_wear(&facts.limits, wearer).map_err(Notice::Line)?;
                // The hands pair (`inventory::pairs`): an off hand takes only
                // what goes with its main hand's weapon; a main hand's new
                // weapon sends an off hand that no longer goes with it back.
                if let Some(&[right, left]) = slots::SETS.iter().find(|set| set.contains(&slot)) {
                    if slot == left {
                        let paired = self
                            .held_weapon(catalog, who, right)
                            .zip(facts.weapon)
                            .is_some_and(|(right, left)| pairs(&right, &left));
                        if !paired {
                            return Err(Notice::CannotEquip);
                        }
                    } else if let Some(off) = self.held_weapon(catalog, who, left)
                        && !facts.weapon.is_some_and(|right| pairs(&right, &off))
                        && let Some(off) = self.in_slot(who, left)
                    {
                        self.carry(out, off, PARTY);
                    }
                }
                let one = if stack > 1 {
                    if let Some(mut rest) = self.items.get_mut(&item) {
                        rest.stack -= 1;
                    }
                    self.new_item(&facts, PARTY, 1, None)
                } else {
                    ObjectId(item)
                };
                self.equip(out, one, who, slot, PARTY);
            }
            Action::Unequip(slot) | Action::UnequipFrom { slot, .. } => {
                if let Some(worn) = self.in_slot(who, slot) {
                    self.carry(out, worn, PARTY);
                }
                // An off hand goes with its main hand [verify: K1's].
                if let Some(&[_, left]) = slots::SETS.iter().find(|set| set[0] == slot)
                    && let Some(off) = self.in_slot(who, left)
                {
                    self.carry(out, off, PARTY);
                }
            }
            _ => {}
        }
        Ok(())
    }

    /// The weapon of the item `who` holds in `slot`, if it is one.
    fn held_weapon(&self, catalog: &Catalog, who: u32, slot: u8) -> Option<Weapon> {
        let item = self.items.get(&self.in_slot(who, slot)?.0)?;
        self.item_facts(catalog, &item.template)?.weapon
    }

    /// `who`'s weapon sets exchanged at once (TSL's `ActionSwitchWeapons`,
    /// the swap button; `slots::SETS`): the second set's hands to the
    /// first's, the first's to the second's, an empty hand as it is (an
    /// empty second set leaves it unarmed and keeps the first in the
    /// second). Each item met its set's rules on the way in, and the sets'
    /// rules are one (`slots::rules`), so none is refused on the way across.
    /// Its sheet is dressed anew at the end of the step. The items moved,
    /// with the slots they went to.
    pub(crate) fn switch_weapons(&mut self, out: &mut Out, who: u32) -> Vec<(u32, u8)> {
        let held: Vec<(u32, u8)> = self
            .items
            .held_by(who)
            .filter_map(|(id, item)| {
                let slot = item.slot?;
                let set = slots::SETS.iter().position(|set| set.contains(&slot))?;
                let hand = slots::SETS[set].iter().position(|at| *at == slot)?;
                Some((id, slots::SETS[1 - set][hand]))
            })
            .collect();
        if held.is_empty() {
            return held;
        }
        self.wearing(out, who);
        for &(id, slot) in &held {
            if let Some(mut item) = self.items.get_mut(&id) {
                item.slot = Some(slot);
            }
        }
        held
    }

    /// Takes `item` out of play.
    pub(crate) fn destroy_item(&mut self, out: &mut Out, item: ObjectId) {
        if let Some((from, Some(_))) = self.items.get(&item.0).map(|item| (item.holder, item.slot))
        {
            self.wearing(out, from);
        }
        self.items.remove(&item.0);
        self.floor.remove(&item.0);
    }

    /// Notes what `who` wears before this step's first change to it.
    fn wearing(&self, out: &mut Out, who: u32) {
        if who != PARTY && !out.worn_before.contains_key(&who) {
            out.worn_before.insert(who, self.worn(who));
        }
    }

    /// The new sheets of those whose equipment changed this step.
    pub(crate) fn dress_changed(&self, catalog: &Catalog, view: &View, out: &mut Out) {
        let facts = |worn: &[Worn]| -> Vec<(u8, ItemFacts)> {
            worn.iter()
                .filter_map(|(slot, template, upgrades, fitted)| {
                    let facts = self.item_facts(catalog, template)?;
                    Some((*slot, facts.fitted(*upgrades, fitted, catalog)))
                })
                .collect()
        };
        fn by_ref(worn: &[(u8, ItemFacts)]) -> Vec<(u8, &ItemFacts)> {
            worn.iter().map(|(slot, facts)| (*slot, facts)).collect()
        }
        for (who, before) in std::mem::take(&mut out.worn_before) {
            let Some(character) = view.thing(ObjectId(who)).and_then(|thing| thing.character)
            else {
                continue;
            };
            let after = self.worn(who);
            if after == before {
                continue;
            }
            let worn = out.sheet(ObjectId(who), character.worn);
            let (before, after) = (facts(&before), facts(&after));
            let sheet = rewear(worn, &by_ref(&before), &by_ref(&after));
            out.changes.push(Change::Sheet {
                who: ObjectId(who),
                sheet,
            });
        }
    }
}
