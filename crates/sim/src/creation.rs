//! Character creation: the character a player made, as the game makes its
//! player creature. Its scores are bought with points from a floor up to a
//! top (a raise costing the score's modifier, within
//! `rules::progression::creation::RAISE_COST`), or start at the scores its
//! class recommends (`makeable`); its first level is a
//! level of its class taken from nothing (`levels::Classes::take`, the
//! rules a level-up follows): the class's hit die and CON modifier, its
//! attack, saves and defence, the feats the class grants, the feats picked
//! (the game's picks for those left unpicked), the skill ranks bought, and
//! the feats TSL's creation grants a player besides (`Class::pc_granted`:
//! `CSWGuiFeatsCharGen::InitiateFeats` asks `AddGrantedFeats` for them). It
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

/// The points a score costs bought from `LOWEST`. A score below `LOWEST`
/// counts as the points that raising it to `LOWEST` would cost, taken off.
fn bought(score: u8) -> i32 {
    let costs = |scores: std::ops::Range<u8>| scores.map(|from| i32::from(cost(from))).sum::<i32>();
    if score >= LOWEST {
        costs(LOWEST..score)
    } else {
        -costs(score..LOWEST)
    }
}

/// The points `abilities` cost bought from `LOWEST`, if each is in range.
pub fn spent(abilities: &[u8; 6]) -> Option<u32> {
    abilities
        .iter()
        .map(|&score| {
            (LOWEST..=HIGHEST)
                .contains(&score)
                .then(|| bought(score) as u32)
        })
        .sum()
}

/// Whether the creation screen can make `abilities` for a class that
/// recommends `recommended` (`Class::recommended`).
///
/// The screen makes scores in two ways. The first starts every score at
/// `LOWEST` with `POINTS` to spend, and raises a score no higher than
/// `HIGHEST`. The second starts at the recommended scores with no points
/// left: quick creation, or the Recommended button. From there a score may
/// be lowered for the points it refunds and another raised with them. A
/// mod's `classes.2da` may recommend scores that cost more than `POINTS`,
/// or that lie outside `LOWEST` to `HIGHEST`. The original game takes such
/// scores as they are, and so does this check.
pub fn makeable(abilities: &[u8; 6], recommended: &[u8; 6]) -> bool {
    let total = |scores: &[u8; 6]| scores.iter().map(|&score| bought(score)).sum::<i32>();
    let bought_freely = spent(abilities).is_some_and(|spent| spent <= u32::from(POINTS));
    let from_recommended =
        abilities.iter().zip(recommended).all(|(&score, &advised)| {
            (advised.min(LOWEST)..=advised.max(HIGHEST)).contains(&score)
        }) && total(abilities) <= total(recommended);
    bought_freely || from_recommended
}

/// Why a creation is refused.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Refused {
    /// Scores the creation screen cannot make (`makeable`).
    Scores,
    /// Its first level is not the class's to take (`Classes::take`).
    Level,
}

/// The sheet and career `creation` starts with, and the skill points it
/// leaves unspent: a level-0 character of its scores, of `faction`, taking
/// its first level.
pub fn sheet(
    classes: &Classes,
    creation: &Creation,
    faction: u8,
) -> Result<(Sheet, Career, u8), Refused> {
    let recommended = classes
        .classes
        .get(usize::from(creation.choices.class))
        .map_or([LOWEST; 6], |class| class.recommended);
    if !makeable(&creation.abilities, &recommended) {
        return Err(Refused::Scores);
    }
    let unmade = unmade(creation.abilities, faction);
    let none = Career::new();
    let choices = Choices {
        powers: Vec::new(),
        ..classes.complete(&unmade, &none, &creation.choices)
    };
    let (mut sheet, career, unspent) = classes
        .take(&unmade, &none, &choices, 0)
        .ok_or(Refused::Level)?;
    let of = &classes.classes[usize::from(choices.class)];
    for &feat in &of.pc_granted {
        if !sheet.feats.has(feat) {
            sheet.feats.insert(feat);
            crate::combat::passive::learn(&mut sheet, feat, 0);
        }
    }
    Ok((sheet, career, unspent))
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
