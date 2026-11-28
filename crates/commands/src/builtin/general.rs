//! Commands that change nothing in the game: help, find, clear, whereami
//! and version.

use super::{Ran, print};
use crate::{Alias, Arg, ArgKind, Effect, Facts, Group, Kind, Parsed, Place, Refusal, Spec};

pub static SPECS: &[Spec] = &[
    Spec {
        name: "help",
        aliases: &[Alias::plain("?")],
        args: &[Arg::optional("NAME", ArgKind::CommandName)],
        help: "List available commands. Use \"help <command>\" for details, or \"help all\" to include unavailable commands.",
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
        help: "Search commands and creature templates matching TEXT.",
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
        help: "Clear the console output.",
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
        help: "Show the current module and your position (in meters).",
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
        help: "Show the kjk version.",
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
        Some(name) => match (crate::find(name), facts.mods.find(name)) {
            (Some(spec), _) => crate::help::describe(spec),
            (None, Some(command)) => crate::help::describe_mod(command),
            (None, None) => {
                let words: Vec<String> = name.split_whitespace().map(str::to_string).collect();
                let refusal = crate::resolve(&words).err();
                return Err(
                    refusal.unwrap_or_else(|| Refusal(format!("Unknown command \"{name}\".")))
                );
            }
        },
    };
    Ok(lines.into_iter().map(Effect::Print).collect())
}

fn find(parsed: &Parsed, facts: &Facts) -> Ran {
    let text = parsed.word(0).unwrap_or("");
    let commands = crate::complete::matching(crate::complete::names(), text);
    let mod_cmds = crate::complete::matching(
        facts.mods.commands.iter().map(|cmd| cmd.name.as_str()),
        text,
    );
    let creatures = (facts.creatures.iter()).flat_map(|creatures| {
        crate::complete::matching(creatures.iter().map(String::as_str), text)
    });
    let mut lines: Vec<Effect> = (commands.into_iter().map(|name| format!("command {name}")))
        .chain(
            mod_cmds
                .into_iter()
                .map(|name| format!("mod command {name}")),
        )
        .chain(creatures.map(|template| format!("creature {template}")))
        .map(Effect::Print)
        .collect();
    if lines.is_empty() {
        lines.push(Effect::Print(format!(
            "No commands or creatures match \"{text}\"."
        )));
    }
    Ok(lines)
}

fn clear(_: &Parsed, _: &Facts) -> Ran {
    Ok(vec![Effect::ClearLog])
}

fn whereami(_: &Parsed, facts: &Facts) -> Ran {
    let module = facts.module.as_deref().unwrap_or("unknown module");
    match facts.position {
        Some([x, y, z]) => print(format!("Module: {module} (x={x:.2}, y={y:.2}, z={z:.2})")),
        None => print(format!("Module: {module} (no player character)")),
    }
}

fn version(_: &Parsed, facts: &Facts) -> Ran {
    print(format!("kjk {}", facts.version))
}
