//! Character creation: the character a player made, as the game makes its
//! player creature. Its scores are bought with points from a floor up to a
//! top (a raise costing the score's modifier, within
//! `rules::progression::creation::RAISE_COST`); its first level is a
//! level of its class taken from nothing (`levels::Classes::take`, the
//! rules a level-up follows): the class's hit die and CON modifier, its
//! attack, saves and defence, the feats the class grants, the feats picked
//! (the game's picks for those left unpicked), the skill ranks bought. It
//! knows no powers: neither game's creation offers them (TSL's Jedi first
//! level would pick two, `classpowergain.2da`; its creation screens have no
//! such step and its character is saved knowing none). It wears and holds
//! nothing (the Endar Spire's footlocker dresses it), stands at neutral
//! alignment, and is a human of the player's faction. Who it is goes to its
//! seat's persona.

use crate::combat::Sheet;
use crate::combat::sheet::{Rows, races};
use crate::levels::{Career, Choices, Classes};
use crate::persona::Persona;

/// Points to buy scores with, and the scores' range.
pub const POINTS: u8 = rules::progression::creation::POINTS;
pub const LOWEST: u8 = rules::progression::creation::LOWEST;
pub const HIGHEST: u8 = rules::progression::creation::HIGHEST;
/// Where a new character's alignment stands.
const NEUTRAL: u8 = rules::progression::creation::NEUTRAL;

/// What character creation chose.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Creation {
    pub persona: Persona,
    /// Strength to charisma.
    pub abilities: [u8; 6],
    /// The first level: its class, skill ranks, feats and powers picked.
    pub choices: Choices,
}

/// What raising a score from `score` costs.
pub fn cost(score: u8) -> u8 {
    let [least, most] = rules::progression::creation::RAISE_COST;
    ((i32::from(score) - 10).div_euclid(2)).clamp(least, most) as u8
}

/// The points `abilities` cost bought from `LOWEST`, if each is in range.
pub fn spent(abilities: &[u8; 6]) -> Option<u32> {
    abilities
        .iter()
        .map(|&score| {
            (LOWEST..=HIGHEST).contains(&score).then(|| {
                (LOWEST..score)
                    .map(|from| u32::from(cost(from)))
                    .sum::<u32>()
            })
        })
        .sum()
}

/// Why a creation is refused.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Refused {
    /// Scores out of range, or more points spent than there are.
    Scores,
    /// Its first level is not the class's to take (`Classes::take`).
    Level,
}

/// The sheet and career `creation` starts with: a level-0 character of its
/// scores, of `faction`, taking its first level.
pub fn sheet(
    classes: &Classes,
    creation: &Creation,
    faction: u8,
) -> Result<(Sheet, Career), Refused> {
    if spent(&creation.abilities).is_none_or(|spent| spent > u32::from(POINTS)) {
        return Err(Refused::Scores);
    }
    let unmade = unmade(creation.abilities, faction);
    let none = Career::new();
    let choices = Choices {
        powers: Vec::new(),
        ..classes.complete(&unmade, &none, &creation.choices)
    };
    classes.take(&unmade, &none, &choices).ok_or(Refused::Level)
}

/// A character of `abilities` and `faction` before its first level: level
/// 0, no hit points, nothing known, worn or held.
pub fn unmade(abilities: [u8; 6], faction: u8) -> Sheet {
    Sheet {
        abilities,
        level: 0,
        max_hp: 0,
        faction,
        race: races::HUMAN,
        feats: Rows::default(),
        good_evil: NEUTRAL,
        ..Sheet::commoner()
    }
}
