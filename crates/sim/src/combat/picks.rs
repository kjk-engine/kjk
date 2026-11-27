//! What a character chose to fight with: the kind of item each item key uses
//! (`Picks::item`, `Action::PickItem`), and which feat attack a weapon
//! carries (KOTOR let a character pick any feat attack it knew;
//! `docs/COMBAT.md`, C7, C8). One
//! choice is kept for each gun type, for two blades and for a double-bladed
//! weapon, and the next-feat key (`Action::NextFeat`) moves on the one the
//! weapon in hand uses. A feat attack on the action bar chooses its own
//! line, or turns it off (`Action::Feat`, `Picks::set`). A creature's script
//! chooses by its talent (`Picks::choose`). The combat AI chooses a line in
//! a fight (`Picks::steer`), but never one that the player picked with the
//! next-feat key or the action bar (`Picks::player`).
//!
//! - A gun's alternate fire takes the chosen line (`Special::of`). Until one
//!   is chosen, it takes the first line the character knows. The cycle goes
//!   round the lines the character knows.
//! - Two blades or a double-bladed weapon carry the chosen melee line on
//!   every hit (`Kit::swing`), none until one is chosen, without changing
//!   their moves.
//! - A single blade has no choice: its style is its feat (`Kit::swing`).
//!   Choosing Flurry, Critical Strike or Power Attack on the action bar or
//!   the power wheel switches it to the fast, medium or strong style
//!   instead (`Picks::switches_to`, `world::feat_styles`). The next-feat key
//!   cycles its styles as the saber style key does (the player's command).
//! - Fists and a stun baton carry the chosen melee line on every blow
//!   (`Kit::carry`), none until one is chosen. Both games' action menus
//!   offer the melee feats for any attack that is not a ranged weapon's,
//!   with nothing in hand too.
//!
//! Kept beside a sheet rather than in `Sheet::encode`, which module packs
//! share: a template never has a choice.

use crate::combat::bar::FeatLine;
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
            .find(|row| sheet.knows(*row))
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
    /// Fists or a stun baton.
    Hands,
}

impl Slot {
    /// The slot's bit in `Picks::player`. The gun types come first, then
    /// two blades, a double-bladed weapon, and fists or a stun baton.
    fn bit(self) -> u16 {
        let at = match self {
            Slot::Gun(blaster) => blaster as usize,
            Slot::Blades(at) => GUNS + at,
            Slot::Hands => GUNS + 2,
        };
        1 << at
    }
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
        Held::Other => Some(Slot::Hands),
        Held::Nothing => None,
    }
}

/// Whether `hand` holds a single blade with no blade in the other hand: a
/// sword, a vibroblade or a lightsaber that is not double-bladed. Its feat
/// attacks and the next-feat key switch its saber style
/// (`Picks::switches_to`).
pub fn single_blade(sheet: &Sheet, hand: u8) -> bool {
    let kit = Kit::of(sheet);
    kit.blade(hand).is_some() && kit.forced_style(hand).is_none()
}

/// Whether the weapon in `hand` chooses among the melee feat lines
/// (`Some(true)`) or among the gun lines (`Some(false)`). A single blade
/// has no choice (`None`).
pub(crate) fn chooses_melee(sheet: &Sheet, hand: u8) -> Option<bool> {
    slot(sheet, hand).map(|slot| !matches!(slot, Slot::Gun(_)))
}

/// The single-blade style a melee feat line switches to: Flurry the fast
/// style, Critical Strike the medium style, Power Attack the strong style.
pub fn style_of(line: FeatLine) -> Option<Style> {
    match line {
        FeatLine::Flurry => Some(Style::Fast),
        FeatLine::CriticalStrike => Some(Style::Medium),
        FeatLine::PowerAttack => Some(Style::Strong),
        FeatLine::PowerBlast | FeatLine::RapidShot | FeatLine::SniperShot => None,
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
    /// Fists' and a stun baton's.
    hands: Melee,
    /// The `spells.2da` row of the kind each key uses (by `Shelf`), none for
    /// what it uses by itself.
    items: [Option<u16>; Shelf::ALL.len()],
    /// The slots whose line the player picked (`Slot::bit`), with the
    /// next-feat key or the action bar. The combat AI never changes such a
    /// line (`steer`).
    player: u16,
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

    /// The line fists and a stun baton carry (`world::strikes`).
    pub fn hands(&self) -> Melee {
        self.hands
    }

    /// The melee line kept for `slot`, if it keeps one.
    fn melee(&mut self, slot: Slot) -> Option<&mut Melee> {
        match slot {
            Slot::Blades(at) => self.blades.get_mut(at),
            Slot::Hands => Some(&mut self.hands),
            Slot::Gun(_) => None,
        }
    }

    /// The kind picked for `shelf`.
    pub fn item(&self, shelf: Shelf) -> Option<u16> {
        self.items[shelf as usize]
    }

    /// The kind of `row` picked for its key; which key, if a key takes it:
    /// the key of the kind of `row` among `carried`, else the key of a row
    /// the engine names.
    pub fn pick_item(&mut self, row: u16, carried: &[Option<Carried>]) -> Option<Shelf> {
        let kind = (carried.iter().flatten())
            .find(|kind| kind.row == row)
            .map(|kind| kind.kind());
        let shelf = kind.or_else(|| items::use_of(row))?.shelf();
        self.items[shelf as usize] = Some(row);
        Some(shelf)
    }

    /// Which of `kinds` (more than a character has slots for) keep a slot,
    /// in their order: for each key its pick or the kind it would use, a
    /// medpac key's a heal for its race (a `droid`'s or not) over an antidote,
    /// then the rest in order.
    pub fn slotted(&self, kinds: &[Carried], droid: bool) -> Vec<Carried> {
        let on = |kind: &&Carried, shelf| kind.kind().shelf() == shelf;
        let heal =
            |kind: &&Carried| matches!(kind.kind(), Use::Heal { droid: kit, .. } if kit == droid);
        let mut kept: Vec<u16> = Shelf::ALL
            .into_iter()
            .filter_map(|shelf| {
                self.item(shelf)
                    .filter(|row| kinds.iter().any(|known| known.row == *row))
                    .or_else(|| {
                        kinds
                            .iter()
                            .find(|kind| on(kind, shelf) && heal(kind))
                            .map(|kind| kind.row)
                    })
                    .or_else(|| {
                        kinds
                            .iter()
                            .find(|kind| on(kind, shelf))
                            .map(|kind| kind.row)
                    })
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
            Some(melee @ (Slot::Blades(_) | Slot::Hands)) => {
                let known: Vec<Melee> = Melee::LINES
                    .into_iter()
                    .filter(|line| line.held(sheet).is_some())
                    .collect();
                if let Some(chosen) = sheet.picks.melee(melee) {
                    *chosen = match known.iter().position(|line| line == chosen) {
                        Some(at) => known.get(at + 1).copied().unwrap_or(Melee::None),
                        None => known.first().copied().unwrap_or(Melee::None),
                    };
                }
            }
            None => {}
        }
        let changed = sheet.picks != before;
        if let Some(slot) = slot(sheet, hand).filter(|_| changed) {
            sheet.picks.player |= slot.bit();
        }
        changed
    }

    /// Makes `line` the feat attack in use for the weapon in `hand`. For
    /// blades and empty hands, choosing the line in use turns it off. A gun
    /// always uses one of the lines the character knows (`Special::of`), so
    /// a gun's alternate fire can never be turned off, and choosing the line
    /// a gun uses already changes nothing. False when nothing changed: the
    /// weapon cannot use the line, the sheet does not know it, or a gun
    /// uses it already. Whenever the sheet knows the line and the weapon can
    /// use it, the slot is marked as the player's, also when the line was in
    /// use already and nothing else changed (`Picks::player`). A caller that
    /// keeps the sheet compares the picks to keep the mark too.
    pub fn set(sheet: &mut Sheet, hand: u8, line: FeatLine) -> bool {
        let Some(row) = line.known(sheet).filter(|_| line.fits(sheet, hand)) else {
            return false;
        };
        let changed = Self::put(sheet, hand, row, true);
        if let Some(slot) = slot(sheet, hand) {
            sheet.picks.player |= slot.bit();
        }
        changed
    }

    /// These picks without the marks of the lines the player picked: what
    /// the character fights with.
    pub fn without_marks(self) -> Picks {
        Picks { player: 0, ..self }
    }

    /// Whether the player picked the line of the weapon in `hand`.
    pub fn player_picked(sheet: &Sheet, hand: u8) -> bool {
        slot(sheet, hand).is_some_and(|slot| sheet.picks.player & slot.bit() != 0)
    }

    /// The combat AI's choice (`combat::choose::feat_line`): makes the line
    /// of feat `row` the feat attack the weapon in `hand` uses. The AI never
    /// changes a line the player picked for that weapon, and never turns a
    /// line off. Whether it changed.
    pub fn steer(sheet: &mut Sheet, hand: u8, row: u16) -> bool {
        !Self::player_picked(sheet, hand) && sheet.knows(row) && Self::put(sheet, hand, row, false)
    }

    /// Makes the line of feat `row` the one the weapon in `hand` uses. For
    /// blades and empty hands, when `toggle` is set and the line is in use
    /// already, the line is turned off. Whether it changed.
    fn put(sheet: &mut Sheet, hand: u8, row: u16, toggle: bool) -> bool {
        match slot(sheet, hand) {
            Some(Slot::Gun(blaster)) => {
                let Some(at) = Special::line_of(row) else {
                    return false;
                };
                if Special::of(sheet, blaster).map(Special::line) == Some(at) {
                    return false;
                }
                sheet.picks.guns[blaster as usize] = at as u8 + 1;
                true
            }
            Some(melee @ (Slot::Blades(_) | Slot::Hands)) => {
                let Some(wanted) = Melee::LINES
                    .into_iter()
                    .find(|melee| melee.rows().contains(&row))
                else {
                    return false;
                };
                let Some(chosen) = sheet.picks.melee(melee) else {
                    return false;
                };
                if *chosen == wanted && !toggle {
                    return false;
                }
                *chosen = if *chosen == wanted {
                    Melee::None
                } else {
                    wanted
                };
                true
            }
            None => false,
        }
    }

    /// The feat attack line the weapon in `hand` uses now. A gun uses the
    /// line its wielder chose, or else the first line the wielder knows
    /// (`Special::of`). Two blades, a double-bladed weapon, fists and a stun
    /// baton use the line chosen for them, if the wielder knows it. A single
    /// blade uses none.
    pub fn in_use(sheet: &Sheet, hand: u8) -> Option<FeatLine> {
        let row = match slot(sheet, hand)? {
            Slot::Gun(blaster) => Special::of(sheet, blaster)?.row(),
            melee => {
                let mut picks = sheet.picks;
                picks.melee(melee).copied()?.held(sheet)?
            }
        };
        FeatLine::of(row)
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
            Some(melee @ (Slot::Blades(_) | Slot::Hands)) => {
                let line = Melee::LINES
                    .into_iter()
                    .find(|line| line.rows().contains(&row))
                    .filter(|line| line.held(sheet).is_some());
                if let (Some(line), Some(chosen)) = (line, sheet.picks.melee(melee))
                    && *chosen == Melee::None
                {
                    *chosen = line;
                }
            }
            _ => {}
        }
        sheet.picks != before
    }

    /// The style that choosing `line` switches the single blade in `hand`
    /// to (`style_of`). There is none when `hand` holds anything but a
    /// single blade alone, when the sheet does not know the line, or when
    /// the blade cannot fight in the style (`Kit::singles`).
    pub fn switches_to(sheet: &Sheet, hand: u8, line: FeatLine) -> Option<Style> {
        let style = style_of(line)?;
        line.known(sheet)?;
        let kit = Kit::of(sheet);
        let blade = kit.blade(hand).filter(|_| single_blade(sheet, hand))?;
        kit.singles(blade).contains(&style).then_some(style)
    }

    /// What the weapon in `hand` carries, where there is a choice to show:
    /// more than one gun line known, or any melee line on two blades, a
    /// double-bladed weapon, fists or a stun baton.
    pub fn shown(sheet: &Sheet, hand: u8) -> Option<Shown> {
        match slot(sheet, hand)? {
            Slot::Gun(blaster) => {
                Special::next_line(sheet, blaster)?;
                Some(Shown::Feat(Special::of(sheet, blaster)?.row()))
            }
            melee => {
                let mut picks = sheet.picks;
                let chosen = picks.melee(melee).copied()?;
                Melee::LINES
                    .iter()
                    .any(|line| line.held(sheet).is_some())
                    .then(|| chosen.held(sheet).map_or(Shown::Nothing, Shown::Feat))
            }
        }
    }

    pub(crate) fn encode(&self, out: &mut Writer) {
        self.encode_as(out, true);
    }

    /// Its bytes, with the slots the player picked only when `with_player`,
    /// as a build from before them wrote it.
    pub(crate) fn encode_as(&self, out: &mut Writer, with_player: bool) {
        out.bytes(&self.guns);
        for line in self.blades {
            out.u8(line as u8);
        }
        for row in self.items {
            out.u16(row.unwrap_or(NO_ITEM));
        }
        out.u8(self.hands as u8);
        if with_player {
            out.u16(self.player);
        }
    }

    /// Its bytes; `with_items` where they hold the item picks, `with_hands`
    /// where they hold the line fists carry (none before), `with_player`
    /// where they hold the slots the player picked (none before).
    pub(crate) fn decode(
        input: &mut Reader,
        with_items: bool,
        with_hands: bool,
        with_player: bool,
    ) -> Result<Picks, DecodeError> {
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
        let hands = match with_hands {
            true => {
                let at = input.at();
                let code = input.u8()?;
                Melee::from_code(code).ok_or_else(|| input.invalid("melee feat choice", at))?
            }
            false => Melee::None,
        };
        let player = if with_player { input.u16()? } else { 0 };
        Ok(Picks {
            guns,
            blades,
            hands,
            items,
            player,
        })
    }
}
