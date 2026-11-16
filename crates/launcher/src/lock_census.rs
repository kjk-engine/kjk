//! `kjk script locks k1|tsl`: the lock census. Every module's own locked
//! door and placeable templates, each with the DC written on the lock
//! (`OpenLockDC`) and the difficulty the game plays by
//! (`sim::module::Lock::difficulty`). KOTOR 1's are always the same.
//! KOTOR 2's differ wherever the lock's difficulty class is not the plain
//! one. A template is counted once in each module that has it. The player's
//! level is taken as 0, so a scaled lock shows its lowest difficulty.

use std::collections::{BTreeMap, BTreeSet};
use std::process::ExitCode;

use fmt_odyssey::ResType;
use fmt_odyssey::gff::Gff;
use vfs::Discovery;
use vfs::resolver::Resolver;

use crate::script_verbs::open;

/// One locked template: where it is, what is written on it, what is played.
struct Row {
    module: String,
    template: String,
    door: bool,
    tag: String,
    class: Option<u8>,
    modifier: i8,
    written: u8,
    played: u8,
}

fn rows(game: &str, discovery: &Discovery) -> Result<Vec<Row>, String> {
    let (_, index, base) = open(discovery, game)?;
    let shared: BTreeSet<String> = base.layers.iter().map(|layer| layer.name.clone()).collect();
    let tsl = game == "tsl";
    let mut found = Vec::new();
    for module in Resolver::modules(&index) {
        let Ok(resolver) = base.with_module(&index, &module) else {
            continue;
        };
        let own = resolver
            .layers
            .iter()
            .filter(|layer| !shared.contains(&layer.name));
        for layer in own {
            for ((template, restype), location) in &layer.resources {
                let door = *restype == ResType::UTD;
                if !door && *restype != ResType::UTP {
                    continue;
                }
                let Some(gff) = location
                    .read()
                    .ok()
                    .and_then(|bytes| Gff::parse(&bytes).ok())
                else {
                    continue;
                };
                let root = &gff.root;
                if root.int("Locked").unwrap_or(0) == 0 {
                    continue;
                }
                let held = asset_odyssey::module::Lock::read(&[root]);
                let Some(lock) = session::lock_facts(&held, tsl, true) else {
                    continue;
                };
                found.push(Row {
                    module: module.clone(),
                    template: template.clone(),
                    door,
                    tag: root.resref("Tag").unwrap_or_else(String::new),
                    class: held.open_class,
                    modifier: held.open_mod,
                    written: held.open_dc,
                    played: lock.difficulty(0),
                });
            }
        }
    }
    found.sort_by(|a, b| (&a.module, &a.template).cmp(&(&b.module, &b.template)));
    Ok(found)
}

/// See the module documentation.
pub fn census(discovery: &Discovery, game: &str) -> ExitCode {
    let found = match rows(game, discovery) {
        Ok(found) => found,
        Err(error) => {
            eprintln!("{error}");
            return ExitCode::FAILURE;
        }
    };
    let moved: Vec<&Row> = found.iter().filter(|row| row.written != row.played).collect();
    println!("module\ttemplate\tkind\ttag\tclass\tmodifier\twritten\tplayed");
    for row in &moved {
        println!(
            "{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}",
            row.module,
            row.template,
            if row.door { "door" } else { "placeable" },
            row.tag,
            row.class.map_or("-".to_string(), |class| class.to_string()),
            row.modifier,
            row.written,
            row.played
        );
    }
    // How many locks went from each written DC to each played difficulty.
    let mut steps: BTreeMap<(bool, u8, u8), usize> = BTreeMap::new();
    for row in &found {
        *steps.entry((row.door, row.written, row.played)).or_default() += 1;
    }
    println!();
    for ((door, written, played), count) in &steps {
        let kind = if *door { "doors" } else { "placeables" };
        println!("{count} locked {kind}: written {written}, played {played}");
    }
    let doors = found.iter().filter(|row| row.door).count();
    println!(
        "{game}: {} locked templates ({doors} doors, {} placeables), {} with a difficulty other than the one written",
        found.len(),
        found.len() - doors,
        moved.len()
    );
    ExitCode::SUCCESS
}
