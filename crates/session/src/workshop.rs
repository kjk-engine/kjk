//! TSL's item creation tables (`sim::inventory::workshop`): the
//! workbench's (`itemcreate.2da`), the lab station's (`chemicalcreate.2da`,
//! whose rank column is `skilllevel`) and the tables TSL's scripts name in
//! their place (`tutitemcreate.2da`, the prologue's; `itemcreatemira.2da`;
//! `tutchemcreate.2da`, which TSL does not ship); and the base items whose
//! `itemtype` is a pool (51 components, 50 chemicals). Empty for K1, whose
//! data has none of them.

use asset_odyssey::module::Source;
use sim::inventory::workshop::{BENCH, LAB, Pool, Recipe, Workshop};

use crate::sheets::Tables;

/// The tables read, beside the bench's and the lab's.
const NAMED: [&str; 3] = ["tutitemcreate", "itemcreatemira", "tutchemcreate"];

/// `baseitems.2da` `itemtype`s counted into a pool.
const POOLS: [(i64, Pool); 2] = [(51, Pool::Components), (50, Pool::Chemicals)];

fn small(value: Option<i64>) -> u8 {
    value
        .and_then(|value| u8::try_from(value).ok())
        .unwrap_or(0)
}

fn recipes(tables: &mut Tables, name: &str) -> Option<Vec<Recipe>> {
    let table = tables.table(name)?;
    let rank = if table.column("skill").is_some() {
        "skill"
    } else {
        "skilllevel"
    };
    Some(
        (0..table.row_labels.len())
            .filter_map(|row| {
                Some(Recipe {
                    template: table.text(row, "label")?.to_ascii_lowercase(),
                    skill: small(table.int(row, rank)),
                    base_skill: small(table.int(row, "base_skill")),
                    group: small(table.int(row, "group")),
                    level: small(table.int(row, "level")),
                    align: small(table.int(row, "align")),
                })
            })
            .collect(),
    )
}

/// The install's item creation tables; empty where it has none.
pub fn workshop(source: &dyn Source) -> Workshop {
    let mut tables = Tables::new(source);
    let mut workshop = Workshop::default();
    for name in [BENCH, LAB].into_iter().chain(NAMED) {
        if let Some(recipes) = recipes(&mut tables, name) {
            workshop.tables.insert(name.to_string(), recipes);
        }
    }
    if workshop.tables.is_empty() {
        return workshop;
    }
    if let Some(base) = tables.table("baseitems") {
        for row in 0..base.row_labels.len() {
            let kind = base.int(row, "itemtype");
            if let Some((_, pool)) = POOLS.iter().find(|(of, _)| Some(*of) == kind)
                && let Ok(row) = u16::try_from(row)
            {
                workshop.pools.insert(row, *pool);
            }
        }
    }
    workshop
}
