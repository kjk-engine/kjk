//! Commands that change a party member, through the Debug Mode map's
//! orders (`sim::action::ArenaOrder`).

use sim::action::{Action, ArenaOrder};
use sim::arena::edits::limits;

use super::{Ran, order};
use crate::{
    ABILITIES, Alias, Arg, ArgKind, Effect, Facts, Group, Kind, Parsed, Place, SKILLS, Spec,
};

/// A level from 1. The simulation holds it to the game's own level cap.
const LEVEL: ArgKind = ArgKind::Int {
    min: 1,
    max: u8::MAX as i64,
};

/// The death animations of `down` (`BOTH_DEATH1` to `BOTH_DEATH19`).
const DEATHS: ArgKind = ArgKind::Int { min: 1, max: 19 };

pub static SPECS: &[Spec] = &[
    Spec {
        name: "god",
        aliases: &[Alias::plain("invulnerability"), Alias::plain("arena god")],
        args: &[],
        help: "Toggle god mode (invulnerability) for your character.",
        example: "god",
        group: Group::Character,
        place: Place::Game,
        kind: Kind::Cheat,
        run: god,
    },
    Spec {
        name: "level",
        aliases: &[Alias::plain("setlevel"), Alias::plain("arena level")],
        args: &[
            Arg::new("CLASS", ArgKind::Class),
            Arg::new("LEVEL", LEVEL),
            Arg::optional("SECOND", ArgKind::Class),
            Arg::optional("SECONDLEVELS", LEVEL),
        ],
        help: "Rebuild your character at total level LEVEL in CLASS, auto-picking feats, powers, and skills. Optionally split SECONDLEVELS into a second class (SECOND). Pass class names or IDs from classes.2da.",
        example: "level 3 20 9 5",
        group: Group::Character,
        place: Place::Game,
        kind: Kind::Cheat,
        run: level,
    },
    Spec {
        name: "attribute",
        aliases: &[
            Alias::plain("arena ability"),
            Alias {
                name: "setstrength",
                words: &["strength"],
            },
            Alias {
                name: "setdexterity",
                words: &["dexterity"],
            },
            Alias {
                name: "setconstitution",
                words: &["constitution"],
            },
            Alias {
                name: "setintelligence",
                words: &["intelligence"],
            },
            Alias {
                name: "setwisdom",
                words: &["wisdom"],
            },
            Alias {
                name: "setcharisma",
                words: &["charisma"],
            },
        ],
        args: &[
            Arg::new("NAME", ArgKind::Ability),
            Arg::new(
                "SCORE",
                ArgKind::Int {
                    min: limits::ABILITY_LEAST as i64,
                    max: limits::ABILITY_MOST as i64,
                },
            ),
            Arg::optional("WHO", ArgKind::Who),
        ],
        help: "Set an attribute (strength, dexterity, constitution, intelligence, wisdom, charisma) for yourself or party member WHO.",
        example: "attribute strength 18",
        group: Group::Character,
        place: Place::Game,
        kind: Kind::Cheat,
        run: attribute,
    },
    Spec {
        name: "skill",
        aliases: &[
            Alias::plain("arena skill"),
            Alias {
                name: "setcomputeruse",
                words: &["computer"],
            },
            Alias {
                name: "setdemolitions",
                words: &["demolitions"],
            },
            Alias {
                name: "setstealth",
                words: &["stealth"],
            },
            Alias {
                name: "setawareness",
                words: &["awareness"],
            },
            Alias {
                name: "setpersuade",
                words: &["persuade"],
            },
            Alias {
                name: "setrepair",
                words: &["repair"],
            },
            Alias {
                name: "setsecurity",
                words: &["security"],
            },
            Alias {
                name: "settreatinjury",
                words: &["treat"],
            },
        ],
        args: &[
            Arg::new("NAME", ArgKind::Skill),
            Arg::new(
                "RANKS",
                ArgKind::Int {
                    min: 0,
                    max: limits::SKILL_MOST as i64,
                },
            ),
            Arg::optional("WHO", ArgKind::Who),
        ],
        help: "Set skill ranks (computer, demolitions, stealth, awareness, persuade, repair, security, treat) for yourself or party member WHO.",
        example: "skill security 10",
        group: Group::Character,
        place: Place::Game,
        kind: Kind::Cheat,
        run: skill,
    },
    Spec {
        name: "alignment",
        aliases: &[Alias::plain("arena alignment")],
        args: &[
            Arg::new(
                "VALUE",
                ArgKind::Int {
                    min: 0,
                    max: limits::ALIGNMENT_MOST as i64,
                },
            ),
            Arg::optional("WHO", ArgKind::Who),
        ],
        help: "Set alignment (0 = full Dark Side, 100 = full Light Side) for yourself or party member WHO.",
        example: "alignment 100",
        group: Group::Character,
        place: Place::Game,
        kind: Kind::Cheat,
        run: alignment,
    },
    Spec {
        name: "restore",
        aliases: &[Alias::plain("arena restore")],
        args: &[],
        help: "Reset your character to the Debug Mode default with full Vitality and Force.",
        example: "restore",
        group: Group::Character,
        place: Place::Arena,
        kind: Kind::Cheat,
        run: restore,
    },
    Spec {
        name: "down",
        aliases: &[Alias::plain("arena down")],
        args: &[Arg::new("DEATH", DEATHS)],
        help: "Play death animation DEATH (1-19) on your character. You stand back up after 7 seconds.",
        example: "down 4",
        group: Group::Character,
        place: Place::Arena,
        kind: Kind::Cheat,
        run: down,
    },
];

fn arena(given: ArenaOrder, said: impl Into<String>) -> Ran {
    order(Action::Arena(given), said)
}

fn god(_: &Parsed, facts: &Facts) -> Ran {
    let said = if facts.god {
        "God mode: OFF."
    } else {
        "God mode: ON."
    };
    arena(ArenaOrder::God, said)
}

fn class_label(facts: &Facts, id: u8) -> String {
    facts
        .classes
        .as_ref()
        .and_then(|classes| classes.iter().find(|(row, _)| *row == id))
        .map_or_else(|| format!("class {id}"), |(_, label)| label.clone())
}

fn level(parsed: &Parsed, facts: &Facts) -> Ran {
    let who = parsed.who(usize::MAX, facts)?;
    let (Some(class), level) = (parsed.row(0), parsed.byte(1)) else {
        return Err(crate::Refusal::new("Missing CLASS."));
    };
    let second = match (parsed.row(2), parsed.byte(3)) {
        (None, _) => None,
        (Some(_), 0) => {
            return Err(crate::Refusal(format!(
                "Missing SECONDLEVELS. Usage: {}",
                crate::find("level").map_or_else(String::new, crate::help::usage)
            )));
        }
        (Some(second), levels) => Some((second, levels)),
    };
    let class_name = class_label(facts, class);
    let said = match second {
        None => format!("Rebuilding character: {class_name} (level {level})."),
        Some((second, levels)) => {
            let second_name = class_label(facts, second);
            format!(
                "Rebuilding character: {class_name} / {second_name} ({levels}), total level {level}."
            )
        }
    };
    arena(
        ArenaOrder::Rebuild {
            who,
            class,
            level,
            second,
        },
        said,
    )
}

fn attribute(parsed: &Parsed, facts: &Facts) -> Ran {
    let who = parsed.who(2, facts)?;
    let ability = parsed.row(0).unwrap_or(0);
    let score = parsed.byte(1);
    let name = ABILITIES.get(usize::from(ability)).copied().unwrap_or("");
    arena(
        ArenaOrder::Ability {
            who,
            ability,
            score,
        },
        format!("Set {name} to {score}."),
    )
}

fn skill(parsed: &Parsed, facts: &Facts) -> Ran {
    let who = parsed.who(2, facts)?;
    let skill = parsed.row(0).unwrap_or(0);
    let ranks = parsed.byte(1);
    let name = SKILLS.get(usize::from(skill)).map_or("", |(name, _)| name);
    arena(
        ArenaOrder::Skill { who, skill, ranks },
        format!("Set {name} to {ranks}."),
    )
}

fn alignment(parsed: &Parsed, facts: &Facts) -> Ran {
    let who = parsed.who(1, facts)?;
    let good_evil = parsed.byte(0);
    arena(
        ArenaOrder::Alignment { who, good_evil },
        format!("Set alignment to {good_evil}."),
    )
}

fn restore(parsed: &Parsed, facts: &Facts) -> Ran {
    let who = parsed.who(usize::MAX, facts)?;
    Ok(vec![
        Effect::Restore { who },
        Effect::Print("Restored default character.".into()),
    ])
}

fn down(parsed: &Parsed, _: &Facts) -> Ran {
    let death = parsed.byte(0);
    arena(
        ArenaOrder::Down(death),
        format!("Playing death animation {death}."),
    )
}
