//! What kjk's level-up would refuse of a feat or a power
//! (`levels::Classes::why_not_feat`, `why_not_power`), told in words: a
//! warning only, the editor still makes the change (spec).

use sim::combat::sheet::Ability;
use sim::levels::{Class, Classes, Unpickable};

use crate::Character;

/// Why no class `character` has would let it pick `feat` now, its known
/// feats and its sheet as they stand; `None` when one would.
pub fn feat_warning(classes: &Classes, character: &Character, feat: u16) -> Option<String> {
    let known: Vec<u16> = character.sheet.feats.rows().collect();
    let sheet = &character.sheet;
    warning(classes, character, "feat", |class| {
        classes.why_not_feat(class, feat, &known, sheet)
    })
}

/// Why no class `character` has would let it pick power `row` at its total
/// level, its known powers as they stand; `None` when one would.
pub fn power_warning(classes: &Classes, character: &Character, row: u16) -> Option<String> {
    let known: Vec<u16> = character.sheet.powers.rows().collect();
    let level = character.sheet.level;
    warning(classes, character, "power", |class| {
        classes.why_not_power(class, row, &known, level)
    })
}

/// The first class's refusal in words, unless a class of `character`'s
/// career has none.
fn warning(
    classes: &Classes,
    character: &Character,
    what: &str,
    why_not: impl Fn(&Class) -> Option<Unpickable>,
) -> Option<String> {
    let mut first = None;
    for &(row, _) in &character.career {
        let Some(class) = classes.classes.get(usize::from(row)) else {
            continue;
        };
        // A class that allows it leaves nothing to warn of.
        let why = why_not(class)?;
        first.get_or_insert((row, why));
    }
    let Some((row, why)) = first else {
        return Some(format!(
            "the save keeps no class for it: whether its level-up allows this {what} is unknown"
        ));
    };
    let sheet = &character.sheet;
    Some(match why {
        Unpickable::NotTheClass => format!("no class it has picks this {what} (class {row} first)"),
        Unpickable::Known => format!("the {what} is known already"),
        Unpickable::Needs(need) => format!("needs {what} {need} first"),
        Unpickable::Level(level) => format!("needs level {level} (it is {})", sheet.level),
        Unpickable::Bab(bab) => {
            format!("needs a base attack bonus of {bab} (it has {})", sheet.bab)
        }
        Unpickable::Ability(ability, least) => {
            let name = Ability::ALL
                .get(usize::from(ability))
                .map_or("an ability".to_string(), |a| format!("{a:?}"));
            format!(
                "needs {name} {least} (it has {})",
                sheet.abilities[usize::from(ability)]
            )
        }
    })
}
