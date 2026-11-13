//! What a character chose to fight with: the kind of item each item key uses
//! (`Picks::item`, `Action::PickItem`), and which feat attack a weapon
//! carries (KOTOR let a character pick any feat attack it knew;
//! `docs/COMBAT.md`, C7, C8). One
//! choice is kept for each gun type, for two blades and for a double-bladed
//! weapon, and the next-feat key (`Action::NextFeat`) moves on the one the
//! weapon in hand uses; a creature's script chooses by its talent
//! (`Picks::choose`).
//!
//! - A gun's alternate fire takes the chosen line (`Special::of`). Until one
//!   is chosen, it takes the first line the character knows. The cycle goes
//!   round the lines the character knows.
//! - Two blades or a double-bladed weapon carry the chosen melee line on
//!   every hit (`Kit::swing`), none until one is chosen, without changing
//!   their moves.
//! - A single blade has no choice: its style is its feat (`Kit::swing`).
//!
//! Kept beside a sheet rather than in `Sheet::encode`, which module packs
//! share: a template never has a choice.

use crate::combat::items::{self, Carried, KINDS, Shelf, Use};
use crate::combat::saber::{Held, Kit, Style};
use crate::combat::sheet::{Blaster, Sheet, feats};
use crate::combat::specials::Special;
use crate::wire::{DecodeError, Reader, Writer};

/// How many gun types there are (`Blaster`).
const GUNS: usize = 7;

/// A melee feat line two blades or a double-bladed weapon carry.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Melee {
    #[default]
    None,
    Power,
    Critical,
    Flurry,
}

impl Melee {
    /// The lines in cycling order.
    const LINES: [Melee; 3] = [Melee::Power, Melee::Critical, Melee::Flurry];

    /// The `feat.2da` rows of the line, by tier.
    fn rows(self) -> &'static [u16] {
        match self {
            Melee::None => &[],
            Melee::Power => &feats::POWER_ATTACK,
            Melee::Critical => &feats::CRITICAL_STRIKE,
            Melee::Flurry => &feats::FLURRY_RANKS,
        }
    }

    /// The feat row `sheet` holds of the line, its highest.
    fn held(self, sheet: &Sheet) -> Option<u16> {
        self.rows()
            .iter()
            .rev()
            .copied()
            .find(|row| sheet.feats.has(*row))
    }

    fn from_code(code: u8) -> Option<Melee> {
        match code {
            0 => Some(Melee::None),
            1 => Some(Melee::Power),
            2 => Some(Melee::Critical),
            3 => Some(Melee::Flurry),
            _ => None,
        }
    }
}

/// What the weapon in a hand chooses among.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Slot {
    Gun(Blaster),
    Blades(usize),
}

fn slot(sheet: &Sheet, hand: u8) -> Option<Slot> {
    let kit = Kit::of(sheet);
    match kit.held(hand) {
        Held::Blaster(gun) => Some(Slot::Gun(gun.blaster)),
        Held::Blade(_) => match kit.forced_style(hand)? {
            Style::Dual => Some(Slot::Blades(0)),
            Style::Staff => Some(Slot::Blades(1)),
            _ => None,
        },
        _ => None,
    }
}

/// What the weapon in a hand carries now, for the HUD.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Shown {
    /// A feat of this `feat.2da` row.
    Feat(u16),
    /// Nothing, though the character could choose a line.
    Nothing,
}

/// A character's choices.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Picks {
    /// Each gun type's line (`Special::LINES`), one up, 0 for its own.
    guns: [u8; GUNS],
    /// Two blades', then a double-bladed weapon's.
    blades: [Melee; 2],
    /// The `spells.2da` row of the kind each key uses (by `Shelf`), none for
    /// what it uses by itself.
    items: [Option<u16>; Shelf::ALL.len()],
}

/// What stands for no pick in the bytes.
const NO_ITEM: u16 = u16::MAX;

impl Picks {
    /// The line chosen for `blaster`.
    pub fn gun(&self, blaster: Blaster) -> Option<usize> {
        usize::from(self.guns[blaster as usize]).checked_sub(1)
    }

    /// What `Kit::swing` reads: two blades', then a double-bladed weapon's.
    pub fn blades(&self) -> [Melee; 2] {
        self.blades
    }

    /// The kind picked for `shelf`.
    pub fn item(&self, shelf: Shelf) -> Option<u16> {
        self.items[shelf as usize]
    }

    /// The kind of `row` picked for its key; which key, if a key takes it.
    pub fn pick_item(&mut self, row: u16) -> Option<Shelf> {
        let shelf = items::use_of(row)?.shelf();
        self.items[shelf as usize] = Some(row);
        Some(shelf)
    }

    /// Which of `kinds` (more than a character has slots for) keep a slot,
    /// in their order: for each key its pick or the kind it would use, a
    /// medpac key's a heal for its race (a `droid`'s or not) over an antidote,
    /// then the rest in order.
    pub fn slotted(&self, kinds: &[Carried], droid: bool) -> Vec<Carried> {
        let on = |row: u16, shelf| items::use_of(row).is_some_and(|u| u.shelf() == shelf);
        let heal = |row: u16| matches!(items::use_of(row), Some(Use::Heal { droid: kit, .. }) if kit == droid);
        let mut kept: Vec<u16> = Shelf::ALL
            .into_iter()
            .filter_map(|shelf| {
                let rows = || kinds.iter().map(|kind| kind.row);
                self.item(shelf)
                    .filter(|row| rows().any(|known| known == *row))
                    .or_else(|| rows().find(|row| on(*row, shelf) && heal(*row)))
                    .or_else(|| rows().find(|row| on(*row, shelf)))
            })
            .collect();
        for kind in kinds {
            if kept.len() < KINDS && !kept.contains(&kind.row) {
                kept.push(kind.row);
            }
        }
        kinds
            .iter()
            .filter(|kind| kept.contains(&kind.row))
            .copied()
            .collect()
    }

    /// The choice after the current one for the weapon in `hand`, kept;
    /// whether it changed.
    pub fn next(sheet: &mut Sheet, hand: u8) -> bool {
        let before = sheet.picks;
        match slot(sheet, hand) {
            Some(Slot::Gun(blaster)) => {
                if let Some(line) = Special::next_line(sheet, blaster) {
                    sheet.picks.guns[blaster as usize] = line as u8 + 1;
                }
            }
            Some(Slot::Blades(at)) => {
                let known: Vec<Melee> = Melee::LINES
                    .into_iter()
                    .filter(|line| line.held(sheet).is_some())
                    .collect();
                let now = sheet.picks.blades[at];
                sheet.picks.blades[at] = match known.iter().position(|line| *line == now) {
                    Some(at) => known.get(at + 1).copied().unwrap_or(Melee::None),
                    None => known.first().copied().unwrap_or(Melee::None),
                };
            }
            None => {}
        }
        sheet.picks != before
    }

    /// A script's talent: the line of feat `row` chosen for the weapon in
    /// `hand`, if it is a line that weapon takes, `sheet` holds it and none
    /// is chosen yet (a choice made stands, a companion's by its player).
    /// The line a gun already takes needs no choosing. Whether it changed.
    pub fn choose(sheet: &mut Sheet, hand: u8, row: u16) -> bool {
        let before = sheet.picks;
        match slot(sheet, hand) {
            Some(Slot::Gun(blaster)) if sheet.picks.gun(blaster).is_none() => {
                if let Some(line) = Special::line_of(row).filter(|line| {
                    Special::known(sheet, *line)
                        && Special::of(sheet, blaster).map(Special::line) != Some(*line)
                }) {
                    sheet.picks.guns[blaster as usize] = line as u8 + 1;
                }
            }
            Some(Slot::Blades(at)) if sheet.picks.blades[at] == Melee::None => {
                let line = Melee::LINES
                    .into_iter()
                    .find(|line| line.rows().contains(&row));
                if let Some(line) = line.filter(|line| line.held(sheet).is_some()) {
                    sheet.picks.blades[at] = line;
                }
            }
            _ => {}
        }
        sheet.picks != before
    }

    /// What the weapon in `hand` carries, where there is a choice to show:
    /// more than one gun line known, or any melee line on two blades or a
    /// double-bladed weapon.
    pub fn shown(sheet: &Sheet, hand: u8) -> Option<Shown> {
        match slot(sheet, hand)? {
            Slot::Gun(blaster) => {
                Special::next_line(sheet, blaster)?;
                Some(Shown::Feat(Special::of(sheet, blaster)?.row()))
            }
            Slot::Blades(at) => Melee::LINES
                .iter()
                .any(|line| line.held(sheet).is_some())
                .then(|| {
                    sheet.picks.blades[at]
                        .held(sheet)
                        .map_or(Shown::Nothing, Shown::Feat)
                }),
        }
    }

    pub(crate) fn encode(&self, out: &mut Writer) {
        out.bytes(&self.guns);
        for line in self.blades {
            out.u8(line as u8);
        }
        for row in self.items {
            out.u16(row.unwrap_or(NO_ITEM));
        }
    }

    /// Its bytes; `with_items` where they hold the item picks.
    pub(crate) fn decode(input: &mut Reader, with_items: bool) -> Result<Picks, DecodeError> {
        let guns: [u8; GUNS] = input.array()?;
        let at = input.at();
        if guns.iter().any(|line| usize::from(*line) > Special::LINES) {
            return Err(input.invalid("gun feat choice", at));
        }
        let mut blade = || {
            let at = input.at();
            let code = input.u8()?;
            Melee::from_code(code).ok_or_else(|| input.invalid("melee feat choice", at))
        };
        let blades = [blade()?, blade()?];
        let mut items = [None; Shelf::ALL.len()];
        if with_items {
            for item in &mut items {
                *item = Some(input.u16()?).filter(|row| *row != NO_ITEM);
            }
        }
        Ok(Picks {
            guns,
            blades,
            items,
        })
    }
}
