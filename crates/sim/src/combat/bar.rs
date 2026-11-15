//! The action bar's stored state. The bar has six slots, and the player
//! puts Force powers, items, mines, feat attacks and stealth mode in them.
//! The bar also keeps which powers the player hid from the power wheel,
//! which lower tiers the player chose to show on it, and what the bar's
//! own fill has already considered. The bar is kept on the character
//! sheet (`Sheet::bar`), so it travels in snapshots and saves.
//!
//! Every set here is a set of bits, so the sheet stays `Copy`.

use crate::combat::items::Shelf;
use crate::combat::sheet::Rows;
use crate::wire::{DecodeError, Reader, Writer};

/// How many slots the bar has.
pub const SLOTS: usize = 6;
/// How many slots, counted from the first, the game fills on its own.
pub const FILLED: usize = 4;

/// A set of `spells.2da` rows, as wide as the sheet's known powers
/// (`Sheet::powers`).
type Powers = Rows<5>;

/// A feat attack line the bar can hold.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum FeatLine {
    PowerAttack,
    CriticalStrike,
    Flurry,
    PowerBlast,
    RapidShot,
    SniperShot,
}

impl FeatLine {
    pub const ALL: [FeatLine; 6] = [
        FeatLine::PowerAttack,
        FeatLine::CriticalStrike,
        FeatLine::Flurry,
        FeatLine::PowerBlast,
        FeatLine::RapidShot,
        FeatLine::SniperShot,
    ];
}

/// What a slot of the bar holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Content {
    /// A Force power, by its `spells.2da` row.
    Power(u16),
    /// The items of one kind (`combat::items::Shelf`).
    Item(Shelf),
    Mine,
    Feat(FeatLine),
    Stealth,
}

/// How many bits `Contents::others` uses: one for each item kind, one for
/// mines, one for each feat line and one for stealth mode.
const OTHER_BITS: usize = Shelf::ALL.len() + 1 + FeatLine::ALL.len() + 1;

/// The tag a slot's bytes start with when the slot is empty.
const EMPTY: u8 = 0;

impl Content {
    /// Its bytes: a tag (1 to 5, by kind) and a value (the power's row,
    /// the item kind's or feat line's place in its list, or 0). An empty
    /// slot is the tag `EMPTY` alone.
    fn code(self) -> (u8, u16) {
        match self {
            Content::Power(row) => (1, row),
            Content::Item(shelf) => (2, shelf as u16),
            Content::Mine => (3, 0),
            Content::Feat(line) => (4, line as u16),
            Content::Stealth => (5, 0),
        }
    }

    fn from_code(tag: u8, value: u16) -> Option<Content> {
        let at = usize::from(value);
        match (tag, value) {
            (1, row) => Some(Content::Power(row)),
            (2, _) => Shelf::ALL.get(at).copied().map(Content::Item),
            (3, 0) => Some(Content::Mine),
            (4, _) => FeatLine::ALL.get(at).copied().map(Content::Feat),
            (5, 0) => Some(Content::Stealth),
            _ => None,
        }
    }

    /// Where a `Contents` keeps it. A power is kept by its row. Anything
    /// else is kept by its bit among the others, where the item kinds come
    /// first, then mines, the feat lines and stealth mode.
    fn bit(self) -> Bit {
        const MINE: u16 = Shelf::ALL.len() as u16;
        match self {
            Content::Power(row) => Bit::Power(row),
            Content::Item(shelf) => Bit::Other(shelf as u16),
            Content::Mine => Bit::Other(MINE),
            Content::Feat(line) => Bit::Other(MINE + 1 + line as u16),
            Content::Stealth => Bit::Other(MINE + 1 + FeatLine::ALL.len() as u16),
        }
    }
}

/// Where a `Contents` keeps a content (`Content::bit`).
enum Bit {
    Power(u16),
    Other(u16),
}

/// A set of bar contents, as bits. A power past the last row the sheet
/// can know is never in it.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Contents {
    powers: Powers,
    others: Rows<1>,
}

impl Contents {
    pub fn insert(&mut self, content: Content) {
        match content.bit() {
            Bit::Power(row) => self.powers.insert(row),
            Bit::Other(bit) => self.others.insert(bit),
        }
    }

    pub fn has(&self, content: Content) -> bool {
        match content.bit() {
            Bit::Power(row) => self.powers.has(row),
            Bit::Other(bit) => self.others.has(bit),
        }
    }

    fn encode(&self, out: &mut Writer) {
        write_rows(&self.powers, out);
        write_rows(&self.others, out);
    }

    fn decode(input: &mut Reader) -> Result<Contents, DecodeError> {
        let powers = read_rows(input, "action bar power seen")?;
        let at = input.at();
        let others: Rows<1> = read_rows(input, "action bar content seen")?;
        if others.rows().any(|bit| usize::from(bit) >= OTHER_BITS) {
            return Err(input.invalid("action bar content seen", at));
        }
        Ok(Contents { powers, others })
    }
}

/// The action bar (see the module documentation).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Bar {
    pub(super) slots: [Option<Content>; SLOTS],
    /// The powers the player hid from the power wheel.
    pub(super) hidden: Powers,
    /// The lower tiers of powers the player chose to show on the wheel.
    pub(super) unhidden: Powers,
    /// What the bar's automatic fill has already considered.
    pub(super) seen: Contents,
    /// Whether the bar's first fill has run.
    pub(super) filled: bool,
}

impl Bar {
    pub fn slots(&self) -> &[Option<Content>; SLOTS] {
        &self.slots
    }

    /// The slot that holds `content`, if one does.
    pub fn slot_of(&self, content: Content) -> Option<usize> {
        self.slots.iter().position(|slot| *slot == Some(content))
    }

    /// Puts `content` in `slot`. If `content` is in another slot, the two
    /// slots swap their contents. A slot past the bar's end is refused,
    /// and the bar does not change.
    pub fn put(&mut self, slot: usize, content: Content) -> bool {
        if slot >= SLOTS {
            return false;
        }
        match self.slot_of(content) {
            Some(from) => self.slots.swap(from, slot),
            None => self.slots[slot] = Some(content),
        }
        true
    }

    /// Empties `slot`. A slot past the bar's end is refused.
    pub fn clear(&mut self, slot: usize) -> bool {
        self.slots.get_mut(slot).map(|held| *held = None).is_some()
    }

    pub(crate) fn encode(&self, out: &mut Writer) {
        for slot in self.slots {
            match slot.map(Content::code) {
                None => out.u8(EMPTY),
                Some((tag, value)) => {
                    out.u8(tag);
                    out.u16(value);
                }
            }
        }
        write_rows(&self.hidden, out);
        write_rows(&self.unhidden, out);
        self.seen.encode(out);
        out.u8(u8::from(self.filled));
    }

    pub(crate) fn decode(input: &mut Reader) -> Result<Bar, DecodeError> {
        let mut bar = Bar::default();
        for slot in 0..SLOTS {
            let at = input.at();
            let content = match input.u8()? {
                EMPTY => None,
                tag => Some(
                    Content::from_code(tag, input.u16()?)
                        .filter(|content| bar.slot_of(*content).is_none())
                        .ok_or_else(|| input.invalid("action bar slot", at))?,
                ),
            };
            bar.slots[slot] = content;
        }
        bar.hidden = read_rows(input, "action bar hidden power")?;
        bar.unhidden = read_rows(input, "action bar shown lower tier")?;
        bar.seen = Contents::decode(input)?;
        let at = input.at();
        bar.filled = match input.u8()? {
            0 => false,
            1 => true,
            _ => return Err(input.invalid("action bar filled flag", at)),
        };
        Ok(bar)
    }
}

/// Writes a set as how many rows it has, then each row, lowest first.
fn write_rows<const WORDS: usize>(rows: &Rows<WORDS>, out: &mut Writer) {
    out.u16(rows.rows().count() as u16);
    for row in rows.rows() {
        out.u16(row);
    }
}

/// Reads what `write_rows` wrote. A row past the set's end, or one that is
/// not above the row before it, is refused.
fn read_rows<const WORDS: usize>(
    input: &mut Reader,
    what: &'static str,
) -> Result<Rows<WORDS>, DecodeError> {
    let mut rows = Rows::default();
    let mut last = None;
    for _ in 0..input.u16()? {
        let at = input.at();
        let row = input.u16()?;
        if row >= Rows::<WORDS>::ROWS || last.is_some_and(|last| row <= last) {
            return Err(input.invalid(what, at));
        }
        rows.insert(row);
        last = Some(row);
    }
    Ok(rows)
}
