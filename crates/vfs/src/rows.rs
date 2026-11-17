//! `spells.2da` and `feat.2da` served in canonical order (`rules::rows`).
//!
//! The resolver builds the rows-by-label map of both tables from the tables
//! as the layers below this one serve them, after KOTOR 2's rows were lent
//! to a KOTOR 1 install (`lent`). When a map is not the identity, this
//! layer serves the table again with each row at its canonical number, a
//! blank row at each number no install row has, and each row's install
//! number in the column `rules::rows::INSTALL_COLUMN`. Every reader of the
//! table then reads canonical numbers, and a reader that must speak install
//! numbers (scripts, creature and item templates) builds the map back from
//! that column (`session::rows::row_maps`).
//!
//! The cells of the install's tables that hold rows of these tables are
//! served as canonical numbers too (`LINKS`): `spells.2da`'s
//! `prerequisites`, `feat.2da`'s prerequisites, successor and spell,
//! `baseitems.2da`'s required, focus and specialization feats, and the item
//! property tables' `featindex` and `spellindex`.
//!
//! The map is made for each resolver, the install's and each module's,
//! from its own tables. A module whose archives hold a table of their own
//! gets the map of that table. Two machines with the same tables get the
//! same map, since it depends on nothing else. Without a mod's rows the map
//! is the identity and this layer serves nothing.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Arc;

use fmt_odyssey::ResType;
use fmt_odyssey::twoda::TwoDa;
use rules::rows::{INSTALL_COLUMN, InstallRow, RowMap, RowMaps, Table};

use crate::Game;
use crate::lent::LENT;
use crate::resolver::{Layer, Location, Rank, Resolver};

/// The layer's name.
pub const ROWS: &str = "rows-by-label";

/// The cells of the install's tables that hold rows of `spells.2da` or
/// `feat.2da`: each table, its column and the table its numbers count rows
/// of. A cell may hold several numbers joined by `_`.
pub const LINKS: [(&str, &str, Table); 21] = [
    ("spells", "prerequisites", Table::Spells),
    ("feat", "prereqfeat1", Table::Feats),
    ("feat", "prereqfeat2", Table::Feats),
    ("feat", "successor", Table::Feats),
    ("feat", "orreqfeat0", Table::Feats),
    ("feat", "orreqfeat1", Table::Feats),
    ("feat", "orreqfeat2", Table::Feats),
    ("feat", "orreqfeat3", Table::Feats),
    ("feat", "orreqfeat4", Table::Feats),
    ("feat", "spellid", Table::Spells),
    ("baseitems", "reqfeat0", Table::Feats),
    ("baseitems", "reqfeat1", Table::Feats),
    ("baseitems", "reqfeat2", Table::Feats),
    ("baseitems", "reqfeat3", Table::Feats),
    ("baseitems", "reqfeat4", Table::Feats),
    ("baseitems", "specfeat", Table::Feats),
    ("baseitems", "focfeat", Table::Feats),
    ("iprp_feats", "featindex", Table::Feats),
    ("iprp_spells", "spellindex", Table::Spells),
    ("iprp_spellcost", "spellindex", Table::Spells),
    ("metamagic", "featrequired", Table::Feats),
];

/// Both tables' maps for `resolver` as its layers stand (without this
/// layer), and why a row got no number. A lent row's KOTOR 2 number is
/// read from the loan when the lent layer is stacked.
pub fn maps(resolver: &Resolver) -> (RowMaps, Vec<String>) {
    let kotor2 = resolver.game == Game::Kotor2;
    let stacked = resolver.layers.iter().any(|layer| layer.name == LENT);
    let lent = resolver.lent.as_ref().filter(|_| stacked);
    let mut maps = RowMaps::default();
    let mut problems = Vec::new();
    for table in Table::ALL {
        let Some(parsed) = resolver.two_da(table.name()) else {
            continue;
        };
        let numbers = lent.map(|lent| match table {
            Table::Spells => &lent.spell_numbers,
            Table::Feats => &lent.feat_numbers,
        });
        let labels: Vec<Option<String>> = (0..parsed.cells.len())
            .map(|row| {
                let label = parsed.text(row, "label")?;
                (!label.is_empty() && label != "****").then_some(label)
            })
            .collect();
        let rows: Vec<InstallRow> = labels
            .iter()
            .enumerate()
            .map(|(at, label)| InstallRow {
                label: label.as_deref(),
                lent: numbers.and_then(|numbers| numbers.get(&at).copied()),
            })
            .collect();
        let (map, why) = RowMap::build(table, kotor2, &rows);
        *maps.get_mut(table) = map;
        problems.extend(why);
    }
    (maps, problems)
}

/// `text`, a cell of numbers joined by `_` that count install rows, as
/// canonical numbers by `map`. A number with no canonical number is left
/// out.
pub fn canonical_cell(text: &str, map: &RowMap) -> String {
    text.split('_')
        .filter_map(|number| {
            let number = number.trim();
            if number.is_empty() || number == "****" {
                return None;
            }
            let row: u16 = number.parse().ok()?;
            map.canonical(row).map(|number| number.to_string())
        })
        .collect::<Vec<_>>()
        .join("_")
}

/// The layer serving `resolver`'s tables in canonical order by `maps`;
/// none when both maps are the identity.
pub(crate) fn layer(resolver: &Resolver, maps: &RowMaps) -> Option<Layer> {
    if maps.is_identity() {
        return None;
    }
    let mut tables: BTreeMap<&str, TwoDa> = BTreeMap::new();
    for (name, _, table) in LINKS {
        if !maps.get(table).is_identity()
            && !tables.contains_key(name)
            && let Some(parsed) = resolver.two_da(name)
        {
            tables.insert(name, parsed);
        }
    }
    for table in Table::ALL {
        if !maps.get(table).is_identity()
            && let Some(parsed) = resolver.two_da(table.name())
        {
            tables.insert(table.name(), parsed);
        }
    }
    let mut resources = BTreeMap::new();
    for (name, mut parsed) in tables {
        for (_, column, table) in LINKS.iter().filter(|(of, ..)| *of == name) {
            let (Some(at), map) = (parsed.column(column), maps.get(*table)) else {
                continue;
            };
            for row in &mut parsed.cells {
                if let Some(cell) = row.get_mut(at) {
                    let text = String::from_utf8_lossy(cell).into_owned();
                    let text = text.trim();
                    if !text.is_empty() && text != "****" {
                        *cell = canonical_cell(text, map).into_bytes();
                    }
                }
            }
        }
        let own = Table::ALL.into_iter().find(|table| table.name() == name);
        if let Some(map) = own
            .map(|table| maps.get(table))
            .filter(|map| !map.is_identity())
        {
            parsed = in_canonical_order(&parsed, map);
        }
        let bytes: Arc<[u8]> = parsed.write().into();
        let location = Location {
            path: PathBuf::from(format!("{ROWS}/{name}.2da")),
            offset: 0,
            size: bytes.len() as u64,
            held: Some(bytes),
        };
        resources.insert((name.to_string(), ResType::TWO_DA), location);
    }
    Some(Layer {
        name: ROWS.into(),
        rank: Rank::Rows,
        resources,
    })
}

/// `table` with each row at its canonical number by `map`, a blank row at
/// each number no row has, and each row's install number in
/// `INSTALL_COLUMN`.
fn in_canonical_order(table: &TwoDa, map: &RowMap) -> TwoDa {
    let mut columns = table.columns.clone();
    columns.push(INSTALL_COLUMN.as_bytes().to_vec());
    let width = columns.len();
    let cells = map
        .by_canonical()
        .iter()
        .map(|row| match row {
            Some(row) => {
                let mut cells = table.cells[usize::from(*row)].clone();
                cells.resize(width - 1, Vec::new());
                cells.push(row.to_string().into_bytes());
                cells
            }
            None => vec![Vec::new(); width],
        })
        .collect::<Vec<_>>();
    TwoDa {
        columns,
        row_labels: (0..cells.len())
            .map(|number| number.to_string().into_bytes())
            .collect(),
        cells,
        ..table.clone()
    }
}
