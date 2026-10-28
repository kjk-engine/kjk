//! TSL's workbench and lab station on the scripts' state (the rules are
//! `inventory::workshop`'s): the party's components and chemicals, a
//! script's `ShowUpgradeScreen` with item creation (and
//! `ShowChemicalUpgradeScreen`), and a player's breakdown and item made
//! (`Action::Breakdown`, `Action::Create`), each told back as the bench
//! now stands (`notice::Screen::Workbench`) with the game's sound. A
//! refusal is the game's line: the skill not reached, too few components
//! (chemicals), upgrades still fitted.
//!
//! Components and chemicals are counts, never items, wherever they are
//! acquired (`pool_carried`, TSL's `CItemRepository::AddItem`): an item of
//! theirs a creature receives (made on it, given, bought, taken from a
//! container, among its starting items) is gone into a count, the party's
//! for a party member ("Acquired 5 Components."), else the creature's own
//! (`State::hoards`), which nothing reads but its joining the party
//! (`takes_in`).

use script_ncs::vm::ObjectId;

use super::state::State;
use super::{Character, Library, View};
use crate::action::Workbench;
use crate::combat::Sheet;
use crate::combat::sheet::Ability;
use crate::inventory::PARTY;
use crate::inventory::workshop::{self, Pool, Recipe};
use crate::notice::{Bench, Notice, Screen, Sound};

/// The game's lines: the skill not reached, too few components (too few
/// chemicals, at the lab station), an item with upgrades fitted.
const UNSKILLED: u32 = 116_292;
const TOO_FEW: u32 = 116_297;
const TOO_FEW_CHEMICALS: u32 = 116_298;
const UPGRADED: u32 = 124_119;

/// "Acquired <CUSTOM0> Components.", "… Chemicals." (the client's lines for
/// TSL's feedback messages 233 and 234).
pub const ACQUIRED_COMPONENTS: u32 = 128_117;
pub const ACQUIRED_CHEMICALS: u32 = 128_118;

/// The sounds of an item made and one broken down (`guisounds.2da`'s
/// `Inventory_Add`, `Inventory_Drop`).
pub const MADE: &str = "gui_invadd";
pub const BROKEN: &str = "gui_invdrop";

/// The bench's sound `name` for its player alone, heard nowhere in
/// particular (the area's).
pub fn sound(name: &str) -> Notice {
    Notice::Sound(Sound::new(super::AREA.0, name))
}

/// What the party's pools gained at once (`State::pool_carried`).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Acquired {
    pub components: i32,
    pub chemicals: i32,
}

impl Acquired {
    /// The game's lines for it, each pool that gained.
    pub fn told(self) -> impl Iterator<Item = Notice> {
        [
            (ACQUIRED_COMPONENTS, self.components),
            (ACQUIRED_CHEMICALS, self.chemicals),
        ]
        .into_iter()
        .filter(|(_, count)| *count > 0)
        .map(|(line, count)| Notice::Count { line, count })
    }
}

/// `sheet`'s total in skill `skill` (`skills.2da` row) whose key ability is
/// `key`: its ranks with what feats add and the ability's modifier, 0 at
/// least.
pub fn total(sheet: &Sheet, skill: u8, key: Option<Ability>) -> u8 {
    let ability = key.map_or(0, |ability| sheet.modifier(ability));
    (sheet.skill(usize::from(skill)) + ability).clamp(0, 255) as u8
}

/// The bench's maker, a party member; refused for anyone else.
fn maker<'a>(view: &'a View, bench: &Workbench) -> Result<&'a Character, Notice> {
    view.thing(ObjectId(bench.maker))
        .and_then(|thing| thing.character.as_ref())
        .filter(|character| character.party)
        .ok_or(Notice::CannotUse)
}

/// `maker`'s total in skill `skill`.
fn rank(library: &Library, maker: &Character, skill: u8) -> u8 {
    total(
        &maker.sheet,
        skill,
        library.skill_ability(usize::from(skill)),
    )
}

impl State {
    /// The party's components (chemicals for the lab station).
    pub fn pool(&self, lab: bool) -> i32 {
        if lab { self.chemicals } else { self.components }
    }

    fn pool_mut(&mut self, lab: bool) -> &mut i32 {
        if lab {
            &mut self.chemicals
        } else {
            &mut self.components
        }
    }

    /// `count` more in a pool of `holder` (the party's, else a creature's
    /// own), held to TSL's cap; how much it moved.
    fn add_to_pool(&mut self, holder: u32, lab: bool, count: i32) -> i32 {
        let stock = if holder == PARTY {
            self.pool_mut(lab)
        } else {
            let (components, chemicals) = self.hoards.entry(holder).or_default();
            if lab { chemicals } else { components }
        };
        let was = *stock;
        *stock = stock.saturating_add(count).min(MOST);
        *stock - was
    }

    /// Every component and chemical item creature holder `holder` carries
    /// (the party's, `PARTY`, or a creature's outside it) counted instead,
    /// the item gone: the party's pools, or the creature's own. What the
    /// party's pools gained. Nothing in a game without pools (K1).
    pub(crate) fn pool_carried(&mut self, library: &Library, holder: u32) -> Acquired {
        let shop = library.workshop();
        let catalog = library.items();
        let pooled: Vec<(u32, Pool, u16)> = self
            .items
            .held_by(holder)
            .filter(|(_, item)| item.slot.is_none())
            .filter_map(|(id, item)| {
                let base = self.item_facts(catalog, &item.template)?.base_item;
                Some((id, shop.pool(base)?, item.stack))
            })
            .collect();
        let mut gained = Acquired::default();
        for (id, pool, stack) in pooled {
            self.items.remove(&id);
            let lab = pool == Pool::Chemicals;
            let added = self.add_to_pool(holder, lab, i32::from(stack));
            if holder == PARTY {
                let total = if lab {
                    &mut gained.chemicals
                } else {
                    &mut gained.components
                };
                *total += added;
            }
        }
        gained
    }

    /// Creature `who` joining the party: what it holds of its own added to
    /// the party's pools (`CSWPartyTable::TransferInventory`, uncapped), its
    /// own count left as it was (QUIRKS Q-0232).
    pub(crate) fn takes_in(&mut self, who: u32) {
        if let Some(&(components, chemicals)) = self.hoards.get(&who) {
            self.components = self.components.saturating_add(components);
            self.chemicals = self.chemicals.saturating_add(chemicals);
        }
    }

    /// The bench `bench` as its player sees it now.
    pub(crate) fn bench(&self, bench: &Workbench, item: Option<u32>, upgrade: bool) -> Screen {
        Screen::Workbench(Bench {
            item,
            maker: bench.maker,
            upgrade,
            list: bench.list.clone(),
            lab: bench.lab,
            stock: self.pool(bench.lab),
        })
    }

    /// One of `item` broken down at `bench` (`Action::Breakdown`) by a party
    /// member: one the party carries, the bench breaks down, without
    /// upgrades fitted; what it yields by the maker's Repair (Treat Injury
    /// at the lab station) added to the pool.
    pub(crate) fn breakdown(
        &mut self,
        library: &Library,
        view: &View,
        bench: &Workbench,
        item: u32,
    ) -> Result<(), Notice> {
        let shop = library.workshop();
        let maker = maker(view, bench)?;
        let held = self
            .items
            .get(&item)
            .filter(|held| held.holder == PARTY && held.slot.is_none())
            .ok_or(Notice::CannotUse)?;
        if held.upgrades != 0 || held.fitted.iter().any(|part| !part.is_empty()) {
            return Err(Notice::Line(UPGRADED));
        }
        let facts = self
            .item_facts(library.items(), &held.template)
            .filter(|facts| shop.breaks_down((*facts).into(), &bench.list, bench.lab))
            .ok_or(Notice::CannotUse)?;
        let skill = rank(library, maker, workshop::yield_skill(bench.lab));
        let yielded = shop.yields(facts.into(), skill);
        self.use_one(item);
        let yielded = i32::try_from(yielded).unwrap_or(i32::MAX);
        self.add_to_pool(PARTY, bench.lab, yielded);
        Ok(())
    }

    /// The item of row `row` of the bench's table made (`Action::Create`):
    /// listed for the maker and its skill reached, its price taken from
    /// the pool; one of its template to the party.
    pub(crate) fn create(
        &mut self,
        library: &Library,
        view: &View,
        bench: &Workbench,
        row: u16,
    ) -> Result<(), Notice> {
        let shop = library.workshop();
        let recipe: &Recipe = shop
            .table(&bench.list)
            .and_then(|table| table.get(usize::from(row)))
            .ok_or(Notice::CannotUse)?;
        let maker = maker(view, bench)?;
        let rank = rank(library, maker, recipe.base_skill);
        if !recipe.listed(rank, maker.level, maker.sheet.good_evil) {
            return Err(Notice::CannotUse);
        }
        if !recipe.open(rank) {
            return Err(Notice::Line(UNSKILLED));
        }
        let facts = self
            .item_facts(library.items(), &recipe.template)
            .ok_or(Notice::CannotUse)?;
        let price = i32::try_from(shop.price(facts.into())).unwrap_or(i32::MAX);
        if self.pool(bench.lab) < price {
            let line = if bench.lab {
                TOO_FEW_CHEMICALS
            } else {
                TOO_FEW
            };
            return Err(Notice::Line(line));
        }
        let template = recipe.template.clone();
        self.make_item(library.items(), &template, PARTY, 1, None)
            .ok_or(Notice::CannotUse)?;
        *self.pool_mut(bench.lab) -= price;
        Ok(())
    }
}

/// The most of a pool the party keeps (TSL's cap).
const MOST: i32 = rules::items::workbench::MOST;
