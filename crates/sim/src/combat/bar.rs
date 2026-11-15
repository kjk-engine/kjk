//! The action bar's stored state. The bar has six slots, and the player
//! puts Force powers, items, mines, feat attacks and stealth mode in them.
//! The bar also keeps which powers the player hid from the power wheel,
//! which lower tiers the player chose to show on it, and what the bar's
//! own fill has already considered. The bar is kept on the character
//! sheet (`Sheet::bar`), so it travels in snapshots and saves.
//!
//! Every set here is a set of bits, so the sheet stays `Copy`.
//!
//! The module also lists what a character can put on the bar
//! (`assignable`) and what the power wheel shows (`wheel`), and fills the
//! bar on its own (`Bar::settle`):
//!
//! - The first fill puts the character's attack powers in the first four
//!   slots, then the feat attacks that fit the weapon in hand, then
//!   grenades, then medpacs, until the four slots are full. An attack power
//!   that is a lower tier of another power the character knows is left out.
//! - Each later fill looks only at the powers and feat attacks the
//!   character learned since the fill before. A new attack power takes the
//!   slot of the lower tier it follows, if that tier is on the bar.
//!   Otherwise it takes the first empty slot of the four. A new feat attack
//!   takes the first empty slot of the four only when the character knows
//!   no Force powers. Nothing else changes a slot.
//! - The wheel lists offensive powers, neutral powers, defensive powers
//!   (`Group`), medpacs, grenades, stims, energy shields, mines, stealth
//!   mode and the feat attacks that fit the weapon in hand, in that order.
//!   It leaves out what is on the
//!   bar, the powers the player hid, and the lower tiers the player did not
//!   choose to show.

use std::collections::BTreeMap;

use crate::combat::force;
use crate::combat::items::{self, Shelf};
use crate::combat::picks;
use crate::combat::sheet::{Rows, Sheet, feats, flags, skills};
use crate::script::Library;
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

    /// The line's `feat.2da` rows, lowest tier first.
    pub fn rows(self) -> &'static [u16] {
        match self {
            FeatLine::PowerAttack => &feats::POWER_ATTACK,
            FeatLine::CriticalStrike => &feats::CRITICAL_STRIKE,
            FeatLine::Flurry => &feats::FLURRY_RANKS,
            FeatLine::PowerBlast => &feats::POWER_BLAST,
            FeatLine::RapidShot => &feats::RAPID_SHOT,
            FeatLine::SniperShot => &feats::SNIPER_SHOT,
        }
    }

    /// Whether blades and empty hands use the line. A gun uses the others.
    fn melee(self) -> bool {
        matches!(
            self,
            FeatLine::PowerAttack | FeatLine::CriticalStrike | FeatLine::Flurry
        )
    }

    /// The highest `feat.2da` row of the line that `sheet` has.
    pub fn known(self, sheet: &Sheet) -> Option<u16> {
        self.rows()
            .iter()
            .rev()
            .copied()
            .find(|row| sheet.feats.has(*row))
    }

    /// Whether the weapon in `hand` can use the line (`combat::picks`). A
    /// gun uses the gun lines. Two blades, a double-bladed weapon, a stun
    /// baton and empty hands use the melee lines. A single blade uses none.
    pub fn fits(self, sheet: &Sheet, hand: u8) -> bool {
        picks::chooses_melee(sheet, hand) == Some(self.melee())
    }

    /// The lines `sheet` knows, in `ALL`'s order.
    fn known_by(sheet: &Sheet) -> impl Iterator<Item = FeatLine> + '_ {
        Self::ALL
            .into_iter()
            .filter(|line| line.known(sheet).is_some())
    }
}

/// What the lists and the fill need to know beyond the sheet. The caller
/// builds it when it needs them, most simply with `Rules::with`.
pub struct Rules<'a> {
    /// Whether the power of a `spells.2da` row is used on an enemy
    /// (`hostilesetting`). A power that is not is a defensive power.
    pub hostile: &'a dyn Fn(u16) -> bool,
    /// Whether the game marks the power of a `spells.2da` row as a light
    /// side power (`goodevil`). The wheel lists such a power that is not
    /// used on an enemy among the defensive powers (`Group`).
    pub light: &'a dyn Fn(u16) -> bool,
    /// The powers each power needs before it can be learned
    /// (`Classes::prerequisites`).
    pub prerequisites: &'a BTreeMap<u16, [Option<u16>; 2]>,
    /// The level's scripts. Without them, a power that only its impact
    /// script casts is not listed (`force::choices`).
    pub library: Option<&'a Library>,
    /// Whether the party carries a mine kit.
    pub mines: bool,
}

impl Rules<'_> {
    /// Runs `run` with the rules of `library`. With a library, the `spells.2da`
    /// row of each power says whether it is used on an enemy. That covers a
    /// scripted power too. Without a library (the combat arena, the proving
    /// ground) the powers are Jedi Academy's. A power is then used on an
    /// enemy unless `force::on_own_side` says otherwise, and each tier of a
    /// chain needs the tier below it (`force::of_row`).
    pub fn with<R>(library: Option<&Library>, mines: bool, run: impl FnOnce(&Rules) -> R) -> R {
        let hostile = |row: u16| match library {
            Some(library) => library.spell(row).hostile,
            None => force::of_row(row).is_some_and(|(power, _)| !force::on_own_side(power)),
        };
        // Without a library, the powers KOTOR marks as light side are its
        // heals, its armours and its valours.
        let light = |row: u16| match library {
            Some(library) => library.spell(row).light,
            None => force::of_row(row).is_some_and(|(power, _)| {
                use force::Power::{Absorb, Heal, Protect, Valor};
                matches!(power, Heal | Protect | Absorb | Valor)
            }),
        };
        run(&Rules {
            hostile: &hostile,
            light: &light,
            prerequisites: match library {
                Some(library) => &library.classes().prerequisites,
                None => chains(),
            },
            library,
            mines,
        })
    }

    /// Whether the power of `row` is used on an enemy. Pull has no row of
    /// its own in `spells.2da`, so it is used as Push is (`force::data_row`).
    pub fn hostile(&self, row: u16) -> bool {
        (self.hostile)(force::data_row(row))
    }

    /// The wheel's group of `content`.
    pub fn group(&self, content: Content) -> Group {
        match content {
            Content::Power(row) if self.hostile(row) => Group::Offensive,
            Content::Power(row) if (self.light)(force::data_row(row)) => Group::Defensive,
            Content::Power(_) => Group::Neutral,
            Content::Item(_) => Group::Items,
            Content::Mine | Content::Stealth | Content::Feat(_) => Group::Other,
        }
    }
}

/// The groups of the wheel's entries, in the wheel's order. A power used on
/// an enemy (`hostilesetting`) is offensive. Of the other powers, one the
/// game marks as light side (`goodevil`) is defensive: the heals, the
/// armours and the valours. The rest are neutral: the speeds, Force
/// Immunity, the energy resistances, Dominate Mind and TSL's Force Body.
/// The game has no column that tells a power that protects the character
/// from one that does not, so Force Immunity and the energy resistances
/// stand among the neutral powers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Group {
    Offensive,
    Neutral,
    Defensive,
    /// The item kinds.
    Items,
    /// Mines, stealth mode and the feat attacks.
    Other,
}

/// The powers each power needs where no game data says so: each tier of a
/// chain (`force::of_row`) needs the rows of the tier below it, at most two.
fn chains() -> &'static BTreeMap<u16, [Option<u16>; 2]> {
    static CHAINS: std::sync::OnceLock<BTreeMap<u16, [Option<u16>; 2]>> =
        std::sync::OnceLock::new();
    CHAINS.get_or_init(|| {
        let rows = || (0..Rows::<5>::ROWS).filter_map(|row| Some((row, force::of_row(row)?)));
        rows()
            .filter(|(_, (_, level))| *level > 1)
            .map(|(row, (power, level))| {
                let mut below = rows()
                    .filter(move |(_, tier)| *tier == (power, level - 1))
                    .map(|(below, _)| below);
                (row, [below.next(), below.next()])
            })
            .collect()
    })
}

/// The item kinds, in the order the wheel lists them.
const SHELVES: [Shelf; 4] = [Shelf::Medpac, Shelf::Grenade, Shelf::Stim, Shelf::Shield];

/// Whether `sheet` carries at least one item of kind `shelf`.
fn carries(sheet: &Sheet, shelf: Shelf) -> bool {
    sheet.items.iter().flatten().any(|carried| {
        carried.count > 0 && items::use_of(carried.row).is_some_and(|used| used.shelf() == shelf)
    })
}

/// The powers `sheet` can put on the bar, in the order the power key
/// cycles them (`force::choices`).
fn powers(sheet: &Sheet, rules: &Rules) -> Vec<u16> {
    force::choices(&sheet.powers, rules.library)
}

/// A known power that is a prerequisite of another known power.
pub fn lower_tier(sheet: &Sheet, rules: &Rules, row: u16) -> bool {
    sheet.powers.has(row)
        && rules
            .prerequisites
            .iter()
            .any(|(power, needs)| sheet.powers.has(*power) && needs.contains(&Some(row)))
}

/// What `sheet` can use other than powers and feat attacks, in the wheel's
/// order. These are the item kinds it carries, mines while the party
/// carries a kit and the character has a rank in Demolitions, and stealth
/// mode while it wears a stealth field generator and has a rank in
/// Stealth.
fn others<'a>(sheet: &'a Sheet, rules: &Rules) -> impl Iterator<Item = Content> + 'a {
    let items = SHELVES
        .into_iter()
        .filter(|shelf| carries(sheet, *shelf))
        .map(Content::Item);
    let mines = rules.mines && sheet.skills[skills::DEMOLITIONS] > 0;
    let stealth = sheet.skills[skills::STEALTH] > 0 && sheet.flags & flags::STEALTH_UNIT != 0;
    items
        .chain(mines.then_some(Content::Mine))
        .chain(stealth.then_some(Content::Stealth))
}

/// Everything the character could put on the bar, in the setup screen's
/// order: every power it can cast, the feat attacks it knows, the item
/// kinds it carries, mines and stealth mode.
pub fn assignable(sheet: &Sheet, rules: &Rules) -> Vec<Content> {
    powers(sheet, rules)
        .into_iter()
        .map(Content::Power)
        .chain(FeatLine::known_by(sheet).map(Content::Feat))
        .chain(others(sheet, rules))
        .collect()
}

/// The wheel's entries (see the module documentation), by their group
/// (`Rules::group`). Within a group they keep the game's order.
pub fn wheel(sheet: &Sheet, rules: &Rules, hand: u8) -> Vec<Content> {
    let bar = &sheet.bar;
    let feats = FeatLine::known_by(sheet).filter(|line| line.fits(sheet, hand));
    let mut listed: Vec<Content> = powers(sheet, rules)
        .into_iter()
        .filter(|row| !bar.hidden(sheet, rules, *row))
        .map(Content::Power)
        .chain(others(sheet, rules))
        .chain(feats.map(Content::Feat))
        .filter(|content| bar.slot_of(*content).is_none())
        .collect();
    listed.sort_by_key(|content| rules.group(*content));
    listed
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
    pub(crate) fn code(self) -> (u8, u16) {
        match self {
            Content::Power(row) => (1, row),
            Content::Item(shelf) => (2, shelf as u16),
            Content::Mine => (3, 0),
            Content::Feat(line) => (4, line as u16),
            Content::Stealth => (5, 0),
        }
    }

    pub(crate) fn from_code(tag: u8, value: u16) -> Option<Content> {
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

    /// Whether the wheel leaves out the power of `row`. It does when the
    /// player hid the power, or when the power is a lower tier the player
    /// did not choose to show.
    pub fn hidden(&self, sheet: &Sheet, rules: &Rules, row: u16) -> bool {
        self.hidden.has(row) || (lower_tier(sheet, rules, row) && !self.unhidden.has(row))
    }

    /// Hides the power of `row` from the wheel, or shows it again. A lower
    /// tier is hidden by default, so showing one keeps it among the shown
    /// lower tiers, and hiding one takes it out of them.
    pub fn hide(&mut self, sheet: &Sheet, rules: &Rules, row: u16, hide: bool) {
        match (lower_tier(sheet, rules, row), hide) {
            (true, true) => self.unhidden.remove(row),
            (true, false) => {
                self.unhidden.insert(row);
                self.hidden.remove(row);
            }
            (false, true) => self.hidden.insert(row),
            (false, false) => self.hidden.remove(row),
        }
    }

    /// Fills the bar on its own (see the module documentation): the first
    /// fill once, then the powers and feat attacks learned since the fill
    /// before. `hand` is the hand whose weapon the first fill fits feat
    /// attacks to. Whether the bar changed.
    pub fn settle(&mut self, sheet: &Sheet, rules: &Rules, hand: u8) -> bool {
        let before = *self;
        if !self.filled {
            self.first_fill(sheet, rules, hand);
        } else if self.learned(sheet) {
            self.place_learned(sheet, rules);
        }
        *self != before
    }

    fn first_fill(&mut self, sheet: &Sheet, rules: &Rules, hand: u8) {
        let attacks = powers(sheet, rules).into_iter().filter(|row| {
            sheet.powers.has(*row) && rules.hostile(*row) && !lower_tier(sheet, rules, *row)
        });
        let feats = FeatLine::known_by(sheet).filter(|line| line.fits(sheet, hand));
        let items = [Shelf::Grenade, Shelf::Medpac]
            .into_iter()
            .filter(|shelf| carries(sheet, *shelf));
        let contents = attacks
            .map(Content::Power)
            .chain(feats.map(Content::Feat))
            .chain(items.map(Content::Item));
        for content in contents {
            let Some(slot) = self.first_empty() else {
                break;
            };
            if self.slot_of(content).is_none() {
                self.slots[slot] = Some(content);
            }
        }
        self.see(sheet);
        self.filled = true;
    }

    /// Whether `sheet` knows a power or a feat attack the fill has not seen.
    fn learned(&self, sheet: &Sheet) -> bool {
        let mut powers = sheet.powers.0.iter().zip(self.seen.powers.0);
        powers.any(|(known, seen)| known & !seen != 0)
            || FeatLine::known_by(sheet).any(|line| !self.seen.has(Content::Feat(line)))
    }

    fn place_learned(&mut self, sheet: &Sheet, rules: &Rules) {
        let new_powers: Vec<u16> = powers(sheet, rules)
            .into_iter()
            .filter(|row| sheet.powers.has(*row) && !self.seen.has(Content::Power(*row)))
            .filter(|row| rules.hostile(*row))
            .collect();
        for row in new_powers {
            let content = Content::Power(row);
            if self.slot_of(content).is_some() {
                continue;
            }
            let lower = rules.prerequisites.get(&row).into_iter().flatten();
            let replaced = lower
                .flatten()
                .find_map(|needs| self.slot_of(Content::Power(*needs)));
            if let Some(slot) = replaced.or_else(|| self.first_empty()) {
                self.slots[slot] = Some(content);
            }
        }
        if sheet.powers == Rows::default() {
            let new_feats: Vec<FeatLine> = FeatLine::known_by(sheet)
                .filter(|line| !self.seen.has(Content::Feat(*line)))
                .collect();
            for line in new_feats {
                if let Some(slot) = self.first_empty() {
                    self.slots[slot] = Some(Content::Feat(line));
                }
            }
        }
        self.see(sheet);
    }

    /// The first empty slot of those the fill uses.
    fn first_empty(&self) -> Option<usize> {
        self.slots[..FILLED].iter().position(Option::is_none)
    }

    /// Marks every power and feat attack `sheet` knows as seen by the fill.
    fn see(&mut self, sheet: &Sheet) {
        for row in sheet.powers.rows() {
            self.seen.insert(Content::Power(row));
        }
        for line in FeatLine::known_by(sheet) {
            self.seen.insert(Content::Feat(line));
        }
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
