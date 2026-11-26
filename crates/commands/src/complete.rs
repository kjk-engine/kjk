//! Completing a word, and the "did you mean" of an unknown command.

use crate::{ABILITIES, ArgKind, Facts, SKILLS, table};

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

/// The candidates for the last word of `line` (the word being typed; empty
/// after a space). The first word completes to command names and aliases.
/// A later word completes by its argument's kind.
pub fn candidates(line: &str, facts: &Facts) -> Vec<String> {
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
        return matching(first, &typing);
    }
    let Ok((spec, args)) = crate::resolve(before) else {
        // A two-word name being typed (`arena sp`).
        let first = before.join(" ");
        let two = names().filter_map(|name| name.strip_prefix(first.as_str())?.strip_prefix(' '));
        return matching(two, &typing);
    };
    let Some(arg) = spec.args.get(args.len()) else {
        return Vec::new();
    };
    match arg.kind {
        ArgKind::Creature => match &facts.creatures {
            Some(creatures) => matching(creatures.iter().map(String::as_str), &typing),
            None => Vec::new(),
        },
        ArgKind::Item => match &facts.items {
            Some(items) => matching(items.iter().map(String::as_str), &typing),
            None => Vec::new(),
        },
        ArgKind::Ability => matching(ABILITIES.iter().copied(), &typing),
        ArgKind::Skill => matching(SKILLS.iter().map(|(name, _)| *name), &typing),
        ArgKind::Class => match &facts.classes {
            Some(classes) => matching(classes.iter().map(|(_, label)| label.as_str()), &typing),
            None => Vec::new(),
        },
        ArgKind::Who => {
            let numbers: Vec<String> = (1..=facts.party.len()).map(|n| n.to_string()).collect();
            let who = std::iter::once("me").chain(numbers.iter().map(String::as_str));
            matching(who, &typing)
        }
        ArgKind::CommandName => matching(table().iter().map(|spec| spec.name), &typing),
        ArgKind::Int { .. } | ArgKind::Word | ArgKind::Text => Vec::new(),
    }
}
