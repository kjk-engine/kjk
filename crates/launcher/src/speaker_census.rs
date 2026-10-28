//! What answers to each entry `Speaker` of a module's conversations
//! (`kjk dialogue check`, check 13, information): the census beside the
//! runs, which reach only some lines. Both executables pass a line over
//! when no one answers to its `Speaker` (`sim::script::dialogue`), and take
//! the one nearest the owner when several do.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use serde_json::{Value, json};
use sim::module::ObjectData;
use sim::script::{Library, key};

/// Examples kept for each class.
const EXAMPLES: usize = 8;

/// What answers to a `Speaker` in a module, by class name.
fn class(tag: &str, owners: &BTreeSet<String>, placed: &BTreeMap<String, usize>, party: &BTreeSet<String>, made: &BTreeSet<String>) -> &'static str {
    let in_party = party.contains(tag);
    match placed.get(tag).copied().unwrap_or(0) {
        _ if owners.contains(tag) => "the owner's own tag",
        _ if tag == "owner" || tag == "player" => "OWNER or PLAYER (searched as a tag)",
        0 if in_party => "a party member (passed over when not in the party)",
        0 if made.contains(tag) => "a template scripts may make (passed over until made)",
        0 => "no object nor gathered template (passed over unless a script makes one)",
        1 if in_party => "a module object sharing a party member's tag (the nearest speaks)",
        1 => "one module object",
        _ => "several module objects (the nearest speaks)",
    }
}

/// Each entry with a `Speaker` in `library`'s conversations, counted by its
/// class against `placed` (the module's objects), the party's templates
/// (`p_…`) and the templates scripts may make; up to `EXAMPLES` of each.
pub fn census(library: &Library, placed: &[Arc<ObjectData>]) -> Value {
    let mut tags: BTreeMap<String, usize> = BTreeMap::new();
    for facts in placed.iter().filter(|facts| !facts.tag.is_empty()) {
        *tags.entry(key(&facts.tag)).or_default() += 1;
    }
    let templates = library.creatures();
    let party: BTreeSet<String> = templates
        .iter()
        .filter(|(template, _)| template.starts_with("p_"))
        .map(|(_, facts)| key(&facts.tag))
        .collect();
    let made: BTreeSet<String> = templates.values().map(|facts| key(&facts.tag)).collect();
    let mut counts: BTreeMap<&str, usize> = BTreeMap::new();
    let mut examples: BTreeMap<&str, Vec<String>> = BTreeMap::new();
    let mut entries = 0;
    for (name, dlg) in library.dialogues() {
        let owners: BTreeSet<String> = placed
            .iter()
            .filter(|facts| key(&facts.conversation) == name)
            .map(|facts| key(&facts.tag))
            .collect();
        for (index, node) in dlg.entries.iter().enumerate() {
            if node.speaker.is_empty() {
                continue;
            }
            entries += 1;
            let tag = key(&node.speaker);
            let class = class(&tag, &owners, &tags, &party, &made);
            *counts.entry(class).or_default() += 1;
            let kept = examples.entry(class).or_default();
            if kept.len() < EXAMPLES {
                kept.push(format!("{name} E{index} {}", node.speaker));
            }
        }
    }
    json!({ "entries": entries, "counts": counts, "examples": examples })
}

/// The census of every module's report, summed: a table of the classes
/// with an example each.
pub fn summary(reports: &[Value]) -> String {
    let mut counts: BTreeMap<String, u64> = BTreeMap::new();
    let mut example: BTreeMap<String, String> = BTreeMap::new();
    let mut entries = 0;
    for census in reports.iter().filter_map(|report| report.get("speakers")) {
        entries += census["entries"].as_u64().unwrap_or(0);
        for (class, count) in census["counts"].as_object().into_iter().flatten() {
            *counts.entry(class.clone()).or_default() += count.as_u64().unwrap_or(0);
            if let Some(first) = census["examples"][class][0].as_str() {
                example.entry(class.clone()).or_insert_with(|| first.to_string());
            }
        }
    }
    let mut out = format!(
        "\n## 13. Speakers by what answers to them (information)\n\n{entries} entries naming a speaker, counted once per module whose conversations hold them.\n\n| class | entries | example |\n|---|---|---|\n"
    );
    for (class, count) in &counts {
        let first = example.get(class).map_or("", String::as_str);
        out.push_str(&format!("| {class} | {count} | {first} |\n"));
    }
    out
}
