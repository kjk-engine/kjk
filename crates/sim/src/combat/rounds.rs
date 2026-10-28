//! KOTOR's melee round between creatures as K1's executable plays it: which
//! clip the attacker swings and which its target answers with, and when the
//! blow lands (`context/artifacts/2026-10-24-gang-war/EXE.md`).
//!
//! - **The round.** It is choreographed when both sides hold a blade in the
//!   right hand (no stun baton, no gun, not bare hands): the attacker swings
//!   its class's cinematic set, `c<class>a1` to `c<class>a5`. Any other
//!   round is plain, `g<class>a1` or `g<class>a2`; one against a creature
//!   model (a beast's own bones) the melee set, `m<class>a1` or `m<class>a2`.
//!   A stun baton swings `g1a1` or `g1a2` against anyone; bare hands
//!   `g8a1` or `g8a2`. (A creature model attacker strikes by its own
//!   clips, `world::strikes`, not by these rounds.)
//! - **The pace.** Each fighter attacks once a round (`ROUND_MS`), a duel's
//!   two in turn half a round apart (`HALF_ROUND_MS`), as the executable's
//!   round holds one attack action a side of 1500 ms.
//! - **A feat attack** swings its feat's own clip by the attacker's class
//!   (`feat_swing`): a baton's Critical Strike and Power Attack `g1a1`, its
//!   Flurry `g1a2`; a blade's `f<class>a1` to `f<class>a4`.
//! - **The answer.** `combatanimations.2da`, by the swing's
//!   `animations.2da` row, names the target's clip for a parry, a dodge and
//!   a hit by the target's class (a creature model's is 0), and the
//!   moment (`hit1`, ms into the swing) the blow lands. A cell left empty
//!   plays nothing. A hit plays the damage cell; a round's miss the parry
//!   cell for a target holding a blade or a baton, else the dodge
//!   [verify: the executable's roll between them]; a script's result
//!   (`CutsceneAttack`) names its own.
//!
//! Rows are `animations.2da`'s: the views draw a row's own clip by its name.
//! The table is the game's (`Table`, `script::Library::with_rounds`); the
//! choice rules are the executable's.

use std::collections::BTreeMap;

use crate::combat::Delivery;
use crate::combat::sheet::Sheet;

/// A cell or row the table leaves empty.
pub const NONE: u16 = u16::MAX;
/// Weapon classes past the last (`combatanimations.2da`'s columns `…0` to
/// `…8`).
pub const CLASSES: usize = 9;
/// Bare hands' class.
pub const UNARMED: u8 = 8;
/// Half a round (ms): an attack action's length, the moment a blow lands
/// into a swing the table has none for (a gun's), and how far apart a
/// duel's two attacks begin.
pub const HALF_ROUND_MS: u16 = (crate::combat::ROUND_MS / 2) as u16;
/// A creature model's class, as its target's column reads it.
pub const CREATURE: u8 = 0;

/// How a target answers a blow.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Answer {
    Parry = 0,
    Dodge = 1,
    Damage = 2,
}

impl Answer {
    /// A round's answer for a blow that `landed` or not on `target`.
    pub fn of_round(landed: bool, target: Side) -> Answer {
        let armed = matches!(
            target.sheet.main_weapon().delivery,
            Delivery::Melee | Delivery::Saber(_) | Delivery::Baton
        );
        match landed {
            true => Answer::Damage,
            false if armed && !target.creature => Answer::Parry,
            false => Answer::Dodge,
        }
    }
}

/// One row of `combatanimations.2da`: when the blow lands (ms into the
/// swing, `NONE` where the row gives none) and the target's clip (an
/// `animations.2da` row, `NONE` for none) by its class and `Answer`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Answers {
    pub hit_ms: u16,
    pub cells: [[u16; 3]; CLASSES],
}

/// The game's tables a round reads: the attack clips' `animations.2da`
/// rows by name, and `combatanimations.2da` by the swing's row.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Table {
    pub rows: BTreeMap<String, u16>,
    pub answers: BTreeMap<u16, Answers>,
}

/// The kind of round, as the executable tells them apart.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Round {
    Choreographed,
    Plain,
    /// A choreographed one with a creature model on a side.
    Creature,
}

/// A side of a round: its sheet and whether its body is a creature model
/// (`appearance.2da` model type S or L).
#[derive(Clone, Copy)]
pub struct Side<'a> {
    pub sheet: &'a Sheet,
    pub creature: bool,
}

impl Side<'_> {
    /// Its weapon class (`Sheet::wield_number`); a creature model's target
    /// column is `CREATURE`.
    fn class(&self) -> u8 {
        if self.creature {
            CREATURE
        } else {
            self.sheet.wield_number()
        }
    }

    /// A blade in the right hand: no baton, gun or bare hands.
    fn blade(&self) -> bool {
        matches!(
            self.sheet.main_weapon().delivery,
            Delivery::Melee | Delivery::Saber(_)
        )
    }
}

/// The round `by` fights `target` in.
pub fn round(by: Side, target: Side) -> Round {
    if by.creature || target.creature {
        Round::Creature
    } else if by.blade() && target.blade() {
        Round::Choreographed
    } else {
        Round::Plain
    }
}

/// How many swings a round's set has (`variant` runs from 1).
pub fn variants(round: Round) -> u8 {
    match round {
        Round::Choreographed => 5,
        _ => 2,
    }
}

/// The swing's clip name for an attacker of `class` in `round`.
pub fn swing_name(class: u8, round: Round, variant: u8) -> String {
    let set = match (class, round) {
        (2..=4, Round::Choreographed) => 'c',
        (2..=4, Round::Creature) => 'm',
        _ => 'g',
    };
    format!("{set}{class}a{variant}")
}

/// A feat attack's clip name for an attacker of `class` (K1's executable,
/// no choice): Critical Strike's line, Flurry's, Power Attack's and Force
/// Jump's; none for another feat or class.
pub fn feat_swing(feat: u16, class: u8) -> Option<String> {
    use crate::combat::sheet::feats;
    let n = if feats::CRITICAL_STRIKE.contains(&feat) {
        1
    } else if feats::FLURRY.contains(&feat) || feat == feats::MASTER_FLURRY {
        2
    } else if feats::POWER_ATTACK.contains(&feat) {
        3
    } else if feats::FORCE_JUMP.contains(&feat) {
        4
    } else {
        return None;
    };
    match class {
        1 if n == 2 => Some("g1a2".into()),
        1 if n != 4 => Some("g1a1".into()),
        2..=4 => Some(format!("f{class}a{n}")),
        _ => None,
    }
}

impl Table {
    /// The `animations.2da` row of the clip named `name`.
    pub fn row(&self, name: &str) -> Option<u16> {
        self.rows.get(name).copied()
    }

    /// The row `by` swings at `target`: its feat's clip where it uses one
    /// that has one, else its round's set at the `variant` drawn by `die`
    /// (1 to the set's size).
    pub fn swing(
        &self,
        (by, target): (Side, Side),
        feat: Option<u16>,
        die: impl FnOnce(u8) -> u8,
    ) -> Option<u16> {
        let class = by.class();
        if let Some(row) = feat
            .and_then(|feat| feat_swing(feat, class))
            .and_then(|name| self.row(&name))
        {
            return Some(row);
        }
        let round = round(by, target);
        self.row(&swing_name(class, round, die(variants(round))))
    }

    /// The clip `target` answers swing `row` with as `answer`; none where
    /// the table leaves it empty.
    pub fn answer(&self, row: u16, answer: Answer, target: Side) -> Option<u16> {
        let class = target.class();
        let cell = self.answers.get(&row)?.cells[usize::from(class)][answer as usize];
        (cell != NONE).then_some(cell)
    }

    /// When swing `row`'s blow lands (ms into it): its `hit1`; none for a
    /// row the table gives no moment (a gun's).
    pub fn hit_ms(&self, row: u16) -> Option<u16> {
        let ms = self.answers.get(&row)?.hit_ms;
        (ms != NONE).then_some(ms)
    }

    /// Hashed into a library's digest.
    pub fn digest(&self, hasher: &mut blake3::Hasher) {
        hasher.update(b"rounds");
        for (name, row) in &self.rows {
            hasher.update(name.as_bytes());
            hasher.update(&row.to_le_bytes());
        }
        for (row, answers) in &self.answers {
            hasher.update(&row.to_le_bytes());
            hasher.update(&answers.hit_ms.to_le_bytes());
            for cell in answers.cells.iter().flatten() {
                hasher.update(&cell.to_le_bytes());
            }
        }
    }
}
