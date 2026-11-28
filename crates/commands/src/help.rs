//! Usage lines and help text.

use crate::{
    Arg, Facts, Group, Kind, Place, Spec, kind_refused, mods, place_refused, spec_refusal, table,
};

/// The command's usage line: its name, then each argument, an optional
/// one in brackets (`level CLASS LEVEL [SECOND] [SECONDLEVELS]`).
pub fn usage(spec: &Spec) -> String {
    usage_of(spec.name, spec.args)
}

/// The usage line of a command named `name` with the arguments `args`.
pub fn usage_of(name: &str, args: &[Arg]) -> String {
    let mut line = name.to_string();
    for arg in args {
        line.push(' ');
        match arg.optional {
            true => line.push_str(&format!("[{}]", arg.name)),
            false => line.push_str(arg.name),
        }
    }
    line
}

/// The other names of a command, as they are typed.
pub fn aliases(spec: &Spec) -> Vec<String> {
    spec.aliases
        .iter()
        .map(|alias| alias.name.to_string())
        .collect()
}

/// `help NAME`: the usage, the other names, the help and an example.
pub fn describe(spec: &Spec) -> Vec<String> {
    let mut lines = vec![format!("Usage: {}", usage(spec))];
    let aliases = aliases(spec);
    if !aliases.is_empty() {
        lines.push(format!("Aliases: {}", aliases.join(", ")));
    }
    lines.push(spec.help.to_string());
    lines.push(format!("Example: {}", spec.example));
    lines
}

/// `help` and `help all`: the commands in their groups, each a usage line
/// and its help. Without `all`, only the commands that can run now are
/// listed. With `all`, a command that cannot run now says why.
pub fn list(facts: &Facts, all: bool) -> Vec<String> {
    let mut lines = Vec::new();
    for group in Group::ALL {
        let specs: Vec<(&Spec, Option<String>)> = (table().iter().copied())
            .filter(|spec| spec.group == group)
            .filter_map(|spec| {
                let refusal = spec_refusal(spec, facts).map(|refusal| refusal.0);
                (all || refusal.is_none()).then_some((spec, refusal))
            })
            .collect();
        if specs.is_empty() {
            continue;
        }
        lines.push(format!("{}:", group.title()));
        for (spec, refusal) in specs {
            lines.push(format!("  {} - {}", usage(spec), first_sentence(spec.help)));
            if let Some(refusal) = refusal {
                lines.push(format!("    {refusal}"));
            }
        }
    }
    lines.extend(mods_list(facts, all));
    lines.push("Type \"help <command>\" for details, or \"help all\" to list all commands.".into());
    lines
}

/// The mod commands for `help`, in a group of their own, as `list` lists
/// the built-in commands.
fn mods_list(facts: &Facts, all: bool) -> Vec<String> {
    let refusal = (place_refused(Place::Game, facts).or_else(|| kind_refused(Kind::Cheat, facts)))
        .map(|refusal| refusal.0);
    if facts.mods.commands.is_empty() || (refusal.is_some() && !all) {
        return Vec::new();
    }
    let mut lines = vec!["Mod commands:".to_string()];
    for command in &facts.mods.commands {
        lines.push(format!(
            "  {} - {}",
            mods::usage(command),
            first_sentence(&command.help)
        ));
        if let Some(refusal) = &refusal {
            lines.push(format!("    {refusal}"));
        }
    }
    lines
}

/// `help NAME` for a mod command: the usage, the help, and the script it
/// runs.
pub fn describe_mod(command: &mods::Command) -> Vec<String> {
    vec![
        format!("Usage: {}", mods::usage(command)),
        command.help.clone(),
        format!("Mod command (cheat). Runs script {}.", command.script),
    ]
}

/// Where a command may run, as a sentence for the documentation page.
pub fn place(spec: &Spec) -> &'static str {
    match spec.place {
        Place::Anywhere => "Available: anywhere.",
        Place::Game => "Available: in-game.",
        Place::Menu => "Available: main menu.",
        Place::Arena => "Available: Debug Mode / combat test maps.",
        Place::ScriptOnly => "Available: --cmds scripts only.",
    }
}

/// The help's first sentence, for the list.
pub fn first_sentence(help: &str) -> &str {
    match help.find(". ") {
        Some(at) => &help[..=at],
        None => help,
    }
}
