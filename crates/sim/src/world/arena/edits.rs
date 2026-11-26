//! The Debug Mode character page's changes to a character
//! (`ArenaOrder::Rebuild` to `ArenaOrder::Restore`), as pure functions of a
//! sheet. The numbers they set are held to the rulebook's limits
//! (`rules::world::arena::terminal`). A rebuilt character takes its first
//! level as character creation makes one (`creation::first_level`) and
//! each further level as the save editor levels one
//! (`levels::Classes::auto_levels`), so an edit made at the terminal and
//! one made in the save editor come out the same.

use crate::combat::Sheet;
use crate::levels::{Career, Choices, Classes};

/// The limits the terminal holds the numbers it sets to (`rules::world::
/// arena::terminal`).
pub mod limits {
    pub const ABILITY_LEAST: u8 = rules::world::arena::terminal::ABILITY_LEAST;
    pub const ABILITY_MOST: u8 = rules::world::arena::terminal::ABILITY_MOST;
    pub const SKILL_MOST: u8 = rules::world::arena::terminal::SKILL_MOST;
    pub const ALIGNMENT_MOST: u8 = rules::world::arena::terminal::ALIGNMENT_MOST;
}

/// `sheet` with ability `ability` (`Ability` order) at `score`, held to
/// the limits. A Constitution moved moves the most hit points as effects
/// move them (`effects::constitution_moved`). `None` for no such ability.
pub fn ability(sheet: &Sheet, ability: u8, score: u8) -> Option<Sheet> {
    let mut after = *sheet;
    *after.abilities.get_mut(usize::from(ability))? =
        score.clamp(limits::ABILITY_LEAST, limits::ABILITY_MOST);
    crate::combat::effects::constitution_moved(sheet, &mut after);
    Some(after)
}

/// `sheet` with `ranks` in skill `skill` (`skills.2da` row), held to the
/// limit. `None` for no such skill.
pub fn skill(sheet: &Sheet, skill: u8, ranks: u8) -> Option<Sheet> {
    let mut after = *sheet;
    *after.skills.get_mut(usize::from(skill))? = ranks.min(limits::SKILL_MOST);
    Some(after)
}

/// `sheet` at alignment `good_evil`, held to the limit.
pub fn alignment(sheet: &Sheet, good_evil: u8) -> Sheet {
    Sheet {
        good_evil: good_evil.min(limits::ALIGNMENT_MOST),
        ..*sheet
    }
}

/// `sheet`'s character made anew as one of class `class` at total level
/// `level` (held between 1 and the game's cap). It starts from the class's
/// recommended scores, takes its first level as character creation does,
/// and every further level with the game's own picks. It keeps who it is
/// and what it wears (`kept`). The sheet, the career and the skill points
/// left unspent, or `None` when the class offers no such character.
pub fn rebuild(
    classes: &Classes,
    sheet: &Sheet,
    class: u8,
    level: u8,
) -> Option<(Sheet, Career, u8)> {
    let recommended = classes.classes.get(usize::from(class))?.recommended;
    let first = Choices {
        class,
        ..Choices::default()
    };
    let (made, career, banked) =
        crate::creation::first_level(classes, recommended, sheet.faction, &first)?;
    let level = level.clamp(1, classes.cap().max(1));
    let (made, career, banked) = classes
        .auto_levels(&made, &career, class, banked, level)
        .ok()?;
    Some((kept(made, sheet), career, banked))
}

/// `made`, a character's numbers made anew (a rebuild, or the map's
/// starting character), with what `current` keeps of its own: its faction
/// and race, its alignment, its flags (god mode among them), the weapons its sheet holds
/// and the items it carries, its feat attacks and action bar, and its Form
/// while it still knows it. Its worn items dress it again afterwards
/// (`State::dressed`).
pub fn kept(made: Sheet, current: &Sheet) -> Sheet {
    let form = if made.powers.has(current.form) {
        current.form
    } else {
        0
    };
    Sheet {
        faction: current.faction,
        race: current.race,
        flags: current.flags,
        good_evil: current.good_evil,
        weapons: current.weapons,
        items: current.items,
        picks: current.picks,
        bar: current.bar,
        form,
        ..made
    }
}
