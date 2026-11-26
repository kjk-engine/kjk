//! Commands that change nothing in the game: help, find, clear, whereami
//! and version.

use super::{Ran, print};
use crate::{Alias, Arg, ArgKind, Effect, Facts, Group, Kind, Parsed, Place, Refusal, Spec};

pub static SPECS: &[Spec] = &[
    Spec {
        name: "help",
        aliases: &[Alias::plain("?")],
        args: &[Arg::optional("NAME", ArgKind::CommandName)],
        help: "Lists the commands that can run now. help NAME describes one command, and help all also lists the commands that cannot run now.",
        example: "help spawn",
        group: Group::General,
        place: Place::Anywhere,
        kind: Kind::Local,
        run: help,
    },
    Spec {
        name: "find",
        aliases: &[],
        args: &[Arg::new("TEXT", ArgKind::Word)],
        help: "Lists every command and every creature template whose name contains the text.",
        example: "find sith",
        group: Group::General,
        place: Place::Anywhere,
        kind: Kind::Local,
        run: find,
    },
    Spec {
        name: "clear",
        aliases: &[],
        args: &[],
        help: "Empties the console's log.",
        example: "clear",
        group: Group::General,
        place: Place::Anywhere,
        kind: Kind::Local,
        run: clear,
    },
    Spec {
        name: "whereami",
        aliases: &[],
        args: &[],
        help: "Prints the module the player is in and where the player's character stands, in metres.",
        example: "whereami",
        group: Group::General,
        place: Place::Game,
        kind: Kind::Local,
        run: whereami,
    },
    Spec {
        name: "version",
        aliases: &[],
        args: &[],
        help: "Prints the version of kjk.",
        example: "version",
        group: Group::General,
        place: Place::Anywhere,
        kind: Kind::Local,
        run: version,
    },
];

fn help(parsed: &Parsed, facts: &Facts) -> Ran {
    let lines = match parsed.word(0) {
        None => crate::help::list(facts, false),
        Some("all") => crate::help::list(facts, true),
        Some(name) => match crate::find(name) {
            Some(spec) => crate::help::describe(spec),
            None => {
                let refusal = crate::resolve(&[name.to_string()]).err();
                return Err(
                    refusal.unwrap_or_else(|| Refusal(format!("No command is named \"{name}\".")))
                );
            }
        },
    };
    Ok(lines.into_iter().map(Effect::Print).collect())
}

fn find(parsed: &Parsed, facts: &Facts) -> Ran {
    let text = parsed.word(0).unwrap_or("");
    let commands = crate::complete::matching(crate::complete::names(), text);
    let creatures = (facts.creatures.iter()).flat_map(|creatures| {
        crate::complete::matching(creatures.iter().map(String::as_str), text)
    });
    let mut lines: Vec<Effect> = (commands.into_iter().map(|name| format!("command {name}")))
        .chain(creatures.map(|template| format!("creature {template}")))
        .map(Effect::Print)
        .collect();
    if lines.is_empty() {
        lines.push(Effect::Print(format!(
            "No command or creature template contains \"{text}\"."
        )));
    }
    Ok(lines)
}

fn clear(_: &Parsed, _: &Facts) -> Ran {
    Ok(vec![Effect::ClearLog])
}

fn whereami(_: &Parsed, facts: &Facts) -> Ran {
    let module = facts.module.as_deref().unwrap_or("an unnamed module");
    match facts.position {
        Some([x, y, z]) => print(format!(
            "You are in {module} at x {x:.2}, y {y:.2}, z {z:.2}."
        )),
        None => print(format!("You are in {module}. You have no character here.")),
    }
}

fn version(_: &Parsed, facts: &Facts) -> Ran {
    print(format!("kjk {}", facts.version))
}
