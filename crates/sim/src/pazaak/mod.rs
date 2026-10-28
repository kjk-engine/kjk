//! KOTOR's pazaak (`docs/PAZAAK.md`): a match a conversation's script
//! starts (`PlayPazaak`), played in the sim by whoever asked for it, alone
//! (I30 P20: its seat's screen shows the table, the others play on). The cards are the
//! game's own data: the opponents' side decks are `pazaakdecks.2da`'s rows,
//! the player's are the basic deck (its first row) and the card items the
//! party holds, each a `Card` by its `ModelVariation`.

use std::collections::BTreeMap;

mod game;
#[cfg(feature = "probes")]
pub mod probes;

pub use game::{
    END_MS, Laid, Match, Move, OPPONENT_MS, Outcome, PLAYER, Phase, SET_MS, SETS_TO_WIN, Side,
    TABLE,
};

use crate::wire::{DecodeError, Reader, Writer};

/// The seat that plays `table` among `seats` (each with the body it
/// drives, the first seat first): the one driving its player, else the
/// first (no seat drives it, the host plays it out); none with no seat.
pub fn playing(
    table: &Match,
    seats: impl IntoIterator<Item = (crate::input::Seat, u32)>,
) -> Option<crate::input::Seat> {
    let mut seats = seats.into_iter().peekable();
    let first = seats.peek().map(|(seat, _)| *seat);
    seats
        .find(|(_, body)| *body == table.player)
        .map(|(seat, _)| seat)
        .or(first)
}

/// The match `seat` plays among `tables`: the one its screen shows and its
/// moves go to. The one whose player it drives, else the first it plays
/// out (`playing`), never one whose player's seat dropped (`Match::away`,
/// held for that seat).
pub fn played_by(
    tables: &[Match],
    seat: crate::input::Seat,
    seats: impl IntoIterator<Item = (crate::input::Seat, u32)> + Clone,
) -> Option<&Match> {
    let drives = |table: &Match| {
        (seats.clone().into_iter()).any(|(of, body)| of == seat && body == table.player)
    };
    tables.iter().find(|table| drives(table)).or_else(|| {
        tables
            .iter()
            .find(|table| table.away.is_none() && playing(table, seats.clone()) == Some(seat))
    })
}

/// A side deck's cards and the hand drawn from it.
pub const SIDE_DECK: usize = rules::pazaak::deck::SIDE_DECK;
pub const HAND: usize = rules::pazaak::deck::HAND;
/// The most a set's total may be, and the stand a total of it forces.
pub const LIMIT: i8 = rules::pazaak::sets::LIMIT;

/// A side card, by its item's `ModelVariation` (the number its icon,
/// `ii_pazcard_NNN`, carries): 1–6 ±1..±6, 7–12 +1..+6, 13–18 −1..−6;
/// TSL's 19 ±1/2, 20 Double, 21 Tie Breaker, 22 Flip 2&4, 23 Flip 3&6.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Card(u8);

/// What a side card does on the table.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// Adds its value, its sign the player's choice when `either`.
    Value { value: i8, either: bool },
    /// ±1 or ±2, the player's choice.
    OneOrTwo,
    /// Doubles the card laid before it.
    Double,
    /// ±1, and a tied set is its player's.
    TieBreaker,
    /// Turns the sign of its player's table cards of these values; counts 0.
    Flip([i8; 2]),
}

impl Card {
    const LAST: u8 = 23;

    pub fn of(variation: u8) -> Option<Card> {
        (1..=Card::LAST)
            .contains(&variation)
            .then_some(Card(variation))
    }

    pub fn variation(self) -> u8 {
        self.0
    }

    pub fn kind(self) -> Kind {
        let value = ((self.0 - 1) % 6 + 1) as i8;
        match self.0 {
            1..=6 => Kind::Value {
                value,
                either: true,
            },
            7..=12 => Kind::Value {
                value,
                either: false,
            },
            13..=18 => Kind::Value {
                value: -value,
                either: false,
            },
            19 => Kind::OneOrTwo,
            20 => Kind::Double,
            21 => Kind::TieBreaker,
            22 => Kind::Flip([2, 4]),
            _ => Kind::Flip([3, 6]),
        }
    }

    /// A `pazaakdecks.2da` cell: `+N`, `-N`, `*N` (±N), `VV` (±1/2), `$$`
    /// (Double), `TT` (Tie Breaker), `F1` (Flip 2&4), `F2` (Flip 3&6).
    pub fn parse(code: &str) -> Option<Card> {
        let special = match code {
            "VV" => Some(19),
            "$$" => Some(20),
            "TT" => Some(21),
            "F1" => Some(22),
            "F2" => Some(23),
            _ => None,
        };
        if let Some(variation) = special {
            return Card::of(variation);
        }
        let (sign, digits) = code.split_at_checked(1)?;
        let value: u8 = digits.parse().ok().filter(|v| (1..=6).contains(v))?;
        let first = match sign {
            "*" => 0,
            "+" => 6,
            "-" => 12,
            _ => return None,
        };
        Card::of(first + value)
    }

    /// Its sign (and TSL's ±1/2 its size) are the player's to choose.
    pub fn chosen(self) -> bool {
        matches!(
            self.kind(),
            Kind::Value { either: true, .. } | Kind::OneOrTwo | Kind::TieBreaker
        )
    }

    /// Its inventory icon.
    pub fn icon(self) -> String {
        format!("ii_pazcard_{:03}", self.0)
    }

    /// As its face shows it with the sign chosen (`minus`, and ±1/2's
    /// `two`): `+3`, `-2`, `±4` before one is chosen.
    pub fn face(self, chosen: Option<(bool, bool)>) -> String {
        let sign = |minus: bool| if minus { '-' } else { '+' };
        match (self.kind(), chosen) {
            (Kind::Value { value, either }, choice) => match choice.filter(|_| either) {
                Some((minus, _)) => format!("{}{}", sign(minus), value.abs()),
                None if either => format!("\u{b1}{value}"),
                None => format!("{value:+}"),
            },
            (Kind::OneOrTwo, Some((minus, two))) => {
                format!("{}{}", sign(minus), if two { 2 } else { 1 })
            }
            (Kind::OneOrTwo, None) => "\u{b1}1/2".into(),
            (Kind::Double, _) => "D".into(),
            (Kind::TieBreaker, Some((minus, _))) => format!("{}1T", sign(minus)),
            (Kind::TieBreaker, None) => "\u{b1}1T".into(),
            (Kind::Flip([a, b]), _) => format!("{a}&{b}"),
        }
    }
}

/// What the install says of pazaak: each `pazaakdecks.2da` row's ten
/// cards (row 0 the basic deck every player has), and the card each item
/// template is (`Pazaak_Card` base items, by their `ModelVariation`).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Decks {
    pub rows: Vec<[Card; SIDE_DECK]>,
    pub cards: BTreeMap<String, Card>,
}

impl Decks {
    /// Opponent deck `row`; row 0 for one the table lacks.
    pub fn row(&self, row: i32) -> Option<[Card; SIDE_DECK]> {
        let at = usize::try_from(row)
            .ok()
            .filter(|row| *row < self.rows.len());
        self.rows.get(at.unwrap_or(0)).copied()
    }

    /// The cards a party may choose its side deck from, by card with how
    /// many: the basic deck's, and those of the item `templates` held
    /// (with their stacks).
    pub fn owned<'a>(
        &self,
        templates: impl IntoIterator<Item = (&'a str, u16)>,
    ) -> Vec<(Card, u8)> {
        let mut owned: BTreeMap<Card, u32> = BTreeMap::new();
        for card in self.rows.first().into_iter().flatten() {
            *owned.entry(*card).or_default() += 1;
        }
        for (template, stack) in templates {
            if let Some(card) = self.cards.get(template) {
                *owned.entry(*card).or_default() += u32::from(stack);
            }
        }
        owned
            .into_iter()
            .map(|(card, count)| (card, count.min(u32::from(u8::MAX)) as u8))
            .collect()
    }

    pub(crate) fn digest_into(&self, out: &mut Writer) {
        out.u32(self.rows.len() as u32);
        for row in &self.rows {
            encode_cards(row, out);
        }
        out.u32(self.cards.len() as u32);
        for (template, card) in &self.cards {
            out.string(template);
            out.u8(card.0);
        }
    }
}

pub(crate) fn encode_cards(cards: &[Card], out: &mut Writer) {
    for card in cards {
        out.u8(card.0);
    }
}

/// A card slot as one byte: its variation, 0 for none.
pub(crate) fn encode_slot(card: Option<Card>, out: &mut Writer) {
    out.u8(card.map_or(0, Card::variation));
}

pub(crate) fn decode_slot(input: &mut Reader) -> Result<Option<Card>, DecodeError> {
    let at = input.at();
    match input.u8()? {
        0 => Ok(None),
        variation => Card::of(variation)
            .map(Some)
            .ok_or_else(|| input.invalid("pazaak card", at)),
    }
}

/// The matches under way, counted in a byte (a snapshot's and the scripts'
/// state's): one a player body, so a handful at most.
pub(crate) fn encode_tables(tables: &[Match], out: &mut Writer) {
    out.u8(u8::try_from(tables.len()).expect("a match a player body"));
    for table in tables {
        table.encode(out);
    }
}

pub(crate) fn decode_tables(input: &mut Reader) -> Result<Vec<Match>, DecodeError> {
    (0..input.u8()?).map(|_| Match::decode(input)).collect()
}

pub(crate) fn decode_card(input: &mut Reader) -> Result<Card, DecodeError> {
    let at = input.at();
    decode_slot(input)?.ok_or_else(|| input.invalid("pazaak card", at))
}

pub(crate) fn decode_cards<const N: usize>(input: &mut Reader) -> Result<[Card; N], DecodeError> {
    let mut cards = [Card(1); N];
    for card in &mut cards {
        *card = decode_card(input)?;
    }
    Ok(cards)
}
