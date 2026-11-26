//! Usage lines and help text.

use crate::{Facts, Group, Place, Spec, place_refusal, table};

/// The command's usage line: its name, then each argument, an optional
/// one in brackets (`level CLASS LEVEL [SECOND] [SECONDLEVELS]`).
pub fn usage(spec: &Spec) -> String {
    let mut line = spec.name.to_string();
    for arg in spec.args {
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
        lines.push(format!("Also accepted: {}", aliases.join(", ")));
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
                let refusal = place_refusal(spec, facts).map(|refusal| refusal.0);
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
    lines.push("Type help NAME for more about one command.".into());
    lines
}

/// Where a command may run, as a sentence for the documentation page.
pub fn place(spec: &Spec) -> &'static str {
    match spec.place {
        Place::Anywhere => "It works at any time.",
        Place::Game => "It works while a game is loaded.",
        Place::Arena => "It works on the Debug Mode map and the other combat test levels.",
        Place::ScriptOnly => "It works only in a command script (--cmds).",
    }
}

/// The help's first sentence, for the list.
pub fn first_sentence(help: &str) -> &str {
    match help.find(". ") {
        Some(at) => &help[..=at],
        None => help,
    }
}
