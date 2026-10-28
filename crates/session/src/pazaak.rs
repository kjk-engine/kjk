//! Pazaak's data (`sim::pazaak::Decks`): `pazaakdecks.2da`'s rows, ten
//! cards each, and the card each item template is: those whose base item
//! is of the pazaak card class (`baseitems.2da` `itemclass`), by their
//! UTI's `ModelVariation`. The rows stop at the first that does not read,
//! so each keeps its number.

use std::collections::BTreeMap;

use fmt_odyssey::ResType;
use fmt_odyssey::gff::Gff;
use sim::inventory::Catalog;
use sim::pazaak::{Card, Decks, SIDE_DECK};

use crate::sheets::Tables;

/// The base items' item class for a pazaak side card.
const CARD_CLASS: &str = "I_PazCard";

pub(crate) fn decks(tables: &mut Tables, items: &Catalog) -> Decks {
    let rows = tables
        .table("pazaakdecks")
        .map(|table| {
            (0..table.cells.len())
                .map_while(|row| {
                    let cards: Vec<Card> = (0..SIDE_DECK)
                        .filter_map(|at| Card::parse(&table.text(row, &format!("card{at}"))?))
                        .collect();
                    cards.try_into().ok()
                })
                .collect()
        })
        .unwrap_or(Vec::new());
    let mut classes = BTreeMap::new();
    let mut cards = BTreeMap::new();
    for facts in items.iter() {
        let card_class = *classes.entry(facts.base_item).or_insert_with(|| {
            tables
                .text("baseitems", usize::from(facts.base_item), "itemclass")
                .is_some_and(|class| class.eq_ignore_ascii_case(CARD_CLASS))
        });
        let card = card_class
            .then(|| tables.source.read(&facts.template, ResType::UTI))
            .flatten()
            .and_then(|bytes| Gff::parse(&bytes).ok())
            .and_then(|uti| uti.root.int("ModelVariation"))
            .and_then(|variation| Card::of(u8::try_from(variation).ok()?));
        if let Some(card) = card {
            cards.insert(facts.template.clone(), card);
        }
    }
    Decks { rows, cards }
}
