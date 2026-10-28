//! TSL's workbench and lab station: items broken down into components (or
//! chemicals), and items made of them (`docs/WORLD-STATE.md`, "Item
//! creation"). The rules, as TSL's executable applies them:
//!
//! - Components and chemicals are counts the party keeps, not items: an
//!   item of a base item whose `baseitems.2da` `itemtype` is 51 (50) taken
//!   adds its stack to them (`Pool`), as credits add to the gold.
//! - What an item is worth here (`value`): its cost, at least 1; nothing for
//!   a plot item (`CSWSItem::GetCost`, whose base item factor TSL's loader
//!   leaves at 1: `baseitems.2da`'s `itemmultiplier` is never read). Its price in
//!   components (`price`) is a share of that, at least 1: what making it
//!   costs, whatever the skill (`rules::items::workbench`).
//! - A recipe (a row of `itemcreate.2da`, `chemicalcreate.2da`, or the table
//!   a script names, TSL's prologue's `tutitemcreate.2da`) is listed when
//!   its `skill` is at most `rules::items::workbench::LISTED_ABOVE` above the
//!   maker's rank in its `base_skill`,
//!   it is of the group shown, the maker's level reaches its `level` and
//!   the maker's alignment is its `align` (1 neutral, 2 light, 3 dark; 0
//!   any); it may be made when the rank reaches its `skill`.
//! - Breaking an item down yields its price times the maker's rank over a
//!   whole yield's ranks (all of it from those), at least 1, and takes one
//!   of its stack: the
//!   rank in Repair at the workbench, in Treat Injury at the lab station
//!   (`yield_skill`). An item worth nothing, one the party does not carry
//!   and one with upgrades fitted are not broken down; nor at the workbench
//!   one the lab station's table lists, while the lab station breaks down
//!   only what its table lists outside its last group (`UNMADE`: HK-50
//!   parts, keys).

use std::collections::BTreeMap;

use crate::wire::Writer;

use super::ItemFacts;

/// What the workbench reads of an item: its template, cost, base item and
/// plot flag (an `ItemFacts`', or the screens' own reading of its UTI).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Piece<'a> {
    pub template: &'a str,
    pub cost: u32,
    pub base_item: u16,
    pub plot: bool,
}

impl<'a> From<&'a ItemFacts> for Piece<'a> {
    fn from(facts: &'a ItemFacts) -> Piece<'a> {
        Piece {
            template: &facts.template,
            cost: facts.cost,
            base_item: facts.base_item,
            plot: facts.plot,
        }
    }
}

/// The workbench's own table, and the lab station's.
pub const BENCH: &str = "itemcreate";
pub const LAB: &str = "chemicalcreate";

/// `skills.2da`'s Repair and Treat Injury: how much a breakdown at the
/// workbench, and at the lab station, yields.
pub const REPAIR: u8 = 5;
pub const TREAT_INJURY: u8 = 7;

/// The lab station's table's group of what it never breaks down.
pub const UNMADE: u8 = 5;

/// The skill a breakdown's yield goes by: Treat Injury at the lab station,
/// else Repair.
pub fn yield_skill(lab: bool) -> u8 {
    if lab { TREAT_INJURY } else { REPAIR }
}

/// What a party keeps count of to make items with.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Pool {
    Components,
    Chemicals,
}

/// A row of an item creation table: the template made, the rank it needs
/// (`skill`) in which skill (`base_skill`, a `skills.2da` row), its group
/// (the screen's tab), the least level (0 any) and the alignment (0 any).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Recipe {
    pub template: String,
    pub skill: u8,
    pub base_skill: u8,
    pub group: u8,
    pub level: u8,
    pub align: u8,
}

/// The game's item creation tables (by lowercase 2DA name) and the base
/// items counted into a pool.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Workshop {
    pub tables: BTreeMap<String, Vec<Recipe>>,
    pub pools: BTreeMap<u16, Pool>,
}

/// The alignment band a recipe's `align` names: 3 dark, 2 light, else 1
/// neutral, by the item rules' groups (`rules::items::alignment`).
pub fn band(good_evil: u8) -> u8 {
    use rules::items::alignment;
    if good_evil <= alignment::DARK_MOST {
        3
    } else if good_evil >= alignment::LIGHT_LEAST {
        2
    } else {
        1
    }
}

impl Workshop {
    /// The pool a base item counts into, if any.
    pub fn pool(&self, base_item: u16) -> Option<Pool> {
        self.pools.get(&base_item).copied()
    }

    /// The table `name` (any case; the workbench's when empty).
    pub fn table(&self, name: &str) -> Option<&[Recipe]> {
        let name = if name.is_empty() { BENCH } else { name };
        self.tables
            .get(&name.to_ascii_lowercase())
            .map(Vec::as_slice)
    }

    /// What `piece` is worth to the workbench.
    pub fn value(&self, piece: Piece) -> u32 {
        if piece.plot {
            return 0;
        }
        piece.cost.max(1)
    }

    /// Its price in components: what making it costs, and what a breakdown
    /// yields with a whole yield's ranks of Repair.
    pub fn price(&self, piece: Piece) -> u32 {
        (self.value(piece) / rules::items::workbench::PRICE_SHARE).max(1)
    }

    /// What breaking `piece` down yields to a maker with `rank` in the
    /// bench's skill (`yield_skill`).
    pub fn yields(&self, piece: Piece, rank: u8) -> u32 {
        let share = (f32::from(rank) / rules::items::workbench::FULL_YIELD_RANKS).min(1.0);
        let yielded = (self.price(piece) as f32 * share) as u32;
        yielded.max(1)
    }

    /// Whether the bench breaks `piece` down at all (worth something, no
    /// plot item): the lab station (`lab`) only what its table lists
    /// outside `UNMADE`; the workbench anything else. `list` is the table
    /// the workbench was opened with: one a script names keeps no lab
    /// station's list.
    pub fn breaks_down(&self, piece: Piece, list: &str, lab: bool) -> bool {
        let listed = |group: &dyn Fn(u8) -> bool| {
            self.table(LAB).unwrap_or(&[]).iter().any(|recipe| {
                group(recipe.group) && recipe.template.eq_ignore_ascii_case(piece.template)
            })
        };
        let kept = if lab {
            listed(&|group| group != UNMADE)
        } else {
            let own = list.is_empty() || list.eq_ignore_ascii_case(BENCH);
            !(own && listed(&|_| true))
        };
        !piece.plot && piece.cost > 0 && kept
    }

    pub(crate) fn digest_into(&self, out: &mut Writer) {
        out.u32(self.tables.len() as u32);
        for (name, recipes) in &self.tables {
            out.string(name);
            out.u32(recipes.len() as u32);
            for recipe in recipes {
                out.string(&recipe.template);
                out.bytes(&[
                    recipe.skill,
                    recipe.base_skill,
                    recipe.group,
                    recipe.level,
                    recipe.align,
                ]);
            }
        }
        out.u32(self.pools.len() as u32);
        for (base, pool) in &self.pools {
            out.u16(*base);
            out.u8(*pool as u8);
        }
    }
}

impl Recipe {
    /// Whether a maker with `rank` in its skill, of `level` and
    /// `good_evil`, sees it listed.
    pub fn listed(&self, rank: u8, level: u8, good_evil: u8) -> bool {
        u16::from(self.skill) <= u16::from(rank) + rules::items::workbench::LISTED_ABOVE
            && (self.level == 0 || level >= self.level)
            && (self.align == 0 || self.align == band(good_evil))
    }

    /// Whether a maker with `rank` in its skill may make it.
    pub fn open(&self, rank: u8) -> bool {
        self.skill <= rank
    }
}
