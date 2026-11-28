//! Completing a word, and the "did you mean" of an unknown command.

use crate::{ABILITIES, Arg, ArgKind, Facts, SKILLS, table};

/// "Did you mean" suggests no name that is more than this many edits away
/// from the typed name.
const MOST_EDITS: usize = 2;

/// Every name a command is typed by: names, then aliases.
pub fn names() -> impl Iterator<Item = &'static str> {
    (table().iter().map(|spec| spec.name)).chain(
        table()
            .iter()
            .flat_map(|spec| spec.aliases.iter().map(|alias| alias.name)),
    )
}

/// The command name or alias closest to `name`, when one is at most
/// `MOST_EDITS` edits away.
pub fn closest(name: &str) -> Option<&'static str> {
    let name = name.to_ascii_lowercase();
    names()
        .map(|known| (edits(&name, known), known))
        .filter(|(edits, _)| *edits <= MOST_EDITS)
        .min_by_key(|(edits, _)| *edits)
        .map(|(_, known)| known)
}

/// The closest built-in or mod command name to `name`, within `MOST_EDITS`.
pub fn closest_with(name: &str, tables: &[&crate::mods::Table]) -> Option<String> {
    let name = name.to_ascii_lowercase();
    let mod_names = (tables.iter())
        .flat_map(|table| table.commands.iter())
        .map(|cmd| cmd.name.as_str());
    names()
        .map(|n| n as &str)
        .chain(mod_names)
        .map(|known| (edits(&name, known), known))
        .filter(|(edits, _)| *edits <= MOST_EDITS)
        .min_by_key(|(edits, _)| *edits)
        .map(|(_, known)| known.to_string())
}

/// The Levenshtein distance between two words.
fn edits(a: &str, b: &str) -> usize {
    let b: Vec<char> = b.chars().collect();
    let mut row: Vec<usize> = (0..=b.len()).collect();
    for (i, ca) in a.chars().enumerate() {
        let mut diagonal = row[0];
        row[0] = i + 1;
        for (j, cb) in b.iter().enumerate() {
            let above = row[j + 1];
            row[j + 1] = if ca == *cb {
                diagonal
            } else {
                1 + diagonal.min(above).min(row[j])
            };
            diagonal = above;
        }
    }
    row[b.len()]
}

/// `known` names that match `typed`: the names that start with it first,
/// then the names that contain it elsewhere, each in the list's order.
pub fn matching<'a>(known: impl IntoIterator<Item = &'a str>, typed: &str) -> Vec<String> {
    let typed = typed.to_ascii_lowercase();
    let known: Vec<(&str, String)> = (known.into_iter())
        .map(|name| (name, name.to_ascii_lowercase()))
        .collect();
    let starts = (known.iter()).filter(|(_, lower)| lower.starts_with(&typed));
    let inside =
        (known.iter()).filter(|(_, lower)| !lower.starts_with(&typed) && lower.contains(&typed));
    let mut found: Vec<String> = Vec::new();
    for (name, _) in starts.chain(inside) {
        if !found.iter().any(|seen| seen == name) {
            found.push(name.to_string());
        }
    }
    found
}

/// Returns the portion of `line` starting after the last unquoted `;`, `\n`,
/// or `\r`, so `;`-chained commands complete and hint independently.
fn last_command(line: &str) -> &str {
    let mut quoted = false;
    let mut start = 0;
    for (idx, c) in line.char_indices() {
        match c {
            '"' => quoted = !quoted,
            ';' | '\n' | '\r' if !quoted => start = idx + c.len_utf8(),
            _ => {}
        }
    }
    &line[start..]
}

/// The candidates for the last word of `line` (the word being typed; empty
/// after a space). The first word completes to command names and aliases.
/// A later word completes by its argument's kind.
pub fn candidates(line: &str, facts: &Facts) -> Vec<String> {
    let line = last_command(line);
    let words: Vec<String> = line.split_whitespace().map(str::to_string).collect();
    let typing = match line.ends_with(char::is_whitespace) {
        true => String::new(),
        false => words.last().cloned().unwrap_or_else(String::new),
    };
    let before: &[String] = match line.ends_with(char::is_whitespace) {
        true => &words,
        false => words.split_last().map_or(&[], |(_, before)| before),
    };
    if before.is_empty() {
        // A two-word name completes by its first word (`arena`).
        let first = names().filter_map(|name| name.split(' ').next());
        let added = facts
            .mods
            .commands
            .iter()
            .map(|command| command.name.as_str());
        return matching(first.chain(added), &typing);
    }
    if let Some(added) = (before.first())
        .filter(|_| crate::resolve(before).is_err())
        .and_then(|first| facts.mods.find(first))
    {
        return match added.args.get(before.len() - 1) {
            Some(arg) => by_kind(arg.kind, facts, &typing),
            None => Vec::new(),
        };
    }
    let Ok((spec, args)) = crate::resolve(before) else {
        // A two-word name being typed (`arena sp`).
        let first = before.join(" ").to_ascii_lowercase();
        let two = names().filter_map(|name| name.strip_prefix(first.as_str())?.strip_prefix(' '));
        return matching(two, &typing);
    };
    let Some(arg) = spec.args.get(args.len()) else {
        return Vec::new();
    };
    by_kind(arg.kind, facts, &typing)
}

/// The candidates for `typing`, a word of an argument of `kind`.
fn by_kind(kind: ArgKind, facts: &Facts, typing: &str) -> Vec<String> {
    match kind {
        ArgKind::Creature => match &facts.creatures {
            Some(creatures) => matching(creatures.iter().map(String::as_str), typing),
            None => Vec::new(),
        },
        ArgKind::Item => match &facts.items {
            Some(items) => matching(items.iter().map(String::as_str), typing),
            None => Vec::new(),
        },
        ArgKind::Script => match &facts.scripts {
            Some(scripts) => matching(scripts.iter().map(String::as_str), typing),
            None => Vec::new(),
        },
        ArgKind::Ability => matching(ABILITIES.iter().copied(), typing),
        ArgKind::Skill => matching(SKILLS.iter().map(|(name, _)| *name), typing),
        ArgKind::Class => match &facts.classes {
            Some(classes) => matching(classes.iter().map(|(_, label)| label.as_str()), typing),
            None => Vec::new(),
        },
        ArgKind::Who => {
            let numbers: Vec<String> = (1..=facts.party.len()).map(|n| n.to_string()).collect();
            let who = std::iter::once("me").chain(numbers.iter().map(String::as_str));
            matching(who, typing)
        }
        ArgKind::CommandName => {
            let commands = table().iter().map(|spec| spec.name);
            let added = (facts.mods.commands.iter()).map(|command| command.name.as_str());
            matching(commands.chain(std::iter::once("all")).chain(added), typing)
        }
        ArgKind::Int { .. } | ArgKind::Word | ArgKind::Text => Vec::new(),
    }
}

/// The arguments of `line`'s command that are still to be typed, as its
/// usage line names them (`TEMPLATE [COUNT]` after `giveitem `). A word
/// being typed counts as typed. Empty until the command's word is complete,
/// and for a line that names no command.
pub fn hint(line: &str, facts: &Facts) -> String {
    let line = last_command(line);
    let words: Vec<String> = line.split_whitespace().map(str::to_string).collect();
    let spaced = line.ends_with(char::is_whitespace);
    if words.is_empty() || (words.len() == 1 && !spaced) {
        return String::new();
    }
    let (args, typed): (&[Arg], usize) = match crate::resolve(&words) {
        Ok((spec, args)) => (spec.args, args.len()),
        Err(_) => match facts.mods.find(&words[0]) {
            Some(command) => (&command.args, words.len() - 1),
            None => return String::new(),
        },
    };
    let rest = args.get(typed..).unwrap_or(&[]);
    crate::help::usage_of("", rest).trim_start().to_string()
}
