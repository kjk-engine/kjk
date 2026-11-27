//! `kjk script decompile`: compiled scripts read back into source
//! (`script_ncs::decompile`), each one proven by compiling it again.
//!
//! - `decompile <game> <script> [--module m] [--out folder]`: one script's
//!   source, printed or written to `folder/<script>.nss`.
//! - `decompile <game>`: every `.ncs` in every layer of the install (the
//!   shared layers and each module's archives) decompiled and compiled
//!   again. A script counts when the bytes are the install's own; the rest
//!   are listed by what the decompiler stopped at.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::process::ExitCode;

use fmt_odyssey::ncs::Ncs;
use script_ncs::decompile::decompile_checked;

use crate::script_verbs::{catalog, ncs_type, open};

pub fn decompile(
    discovery: &vfs::Discovery,
    game: &str,
    script: Option<&str>,
    module: Option<&str>,
    out: Option<&Path>,
) -> ExitCode {
    let done = match script {
        Some(script) => one(discovery, game, script, module, out),
        None => every(discovery, game),
    };
    match done {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("{error}");
            ExitCode::FAILURE
        }
    }
}

fn one(
    discovery: &vfs::Discovery,
    game: &str,
    script: &str,
    module: Option<&str>,
    out: Option<&Path>,
) -> Result<(), String> {
    let (_, index, base) = open(discovery, game)?;
    let resolver = match module {
        Some(module) => base
            .with_module(&index, module)
            .map_err(|error| error.to_string())?,
        None => base,
    };
    let catalog = catalog(&resolver)?;
    let name = script.trim_end_matches(".ncs").to_ascii_lowercase();
    let found = resolver
        .find(&name, ncs_type())
        .ok_or(format!("{name}.ncs: not found"))?;
    let bytes = found.location.read().map_err(|error| error.to_string())?;
    let ncs = Ncs::parse(&bytes).map_err(|error| format!("{name}.ncs: {error}"))?;
    let decompiled =
        decompile_checked(&name, &ncs, &catalog).map_err(|error| format!("{name}.ncs: {error}"))?;
    match out {
        Some(folder) => {
            let path = folder.join(format!("{name}.nss"));
            std::fs::write(&path, &decompiled.source)
                .map_err(|error| format!("{}: {error}", path.display()))?;
            outln!(
                "{name}.ncs from {}: written to {}",
                found.layer.name,
                path.display()
            );
        }
        None => {
            // The games' code page: every byte one character.
            let text: String = decompiled
                .source
                .iter()
                .map(|&byte| char::from(byte))
                .collect();
            outln!("{text}");
        }
    }
    Ok(())
}

fn every(discovery: &vfs::Discovery, game: &str) -> Result<(), String> {
    let (game, index, base) = open(discovery, game)?;
    let catalog = catalog(&base)?;
    let layers = base.every_layer(&index);
    let started = std::time::Instant::now();
    // The same script lies in many modules: each content is read once.
    let mut seen: BTreeSet<(String, Vec<u8>)> = BTreeSet::new();
    let (mut files, mut same, mut older) = (0usize, 0usize, 0usize);
    let mut refused: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for layer in &layers {
        for ((name, restype), location) in &layer.resources {
            if *restype != ncs_type() {
                continue;
            }
            files += 1;
            let Ok(bytes) = location.read() else {
                refused
                    .entry("the file cannot be read".into())
                    .or_default()
                    .push(format!("{}/{name}", layer.name));
                continue;
            };
            if !seen.insert((name.clone(), bytes.clone())) {
                continue;
            }
            let why = match Ncs::parse(&bytes) {
                Err(error) => error.to_string(),
                Ok(ncs) => match decompile_checked(name, &ncs, &catalog) {
                    Ok(decompiled) => {
                        same += 1;
                        older += usize::from(!decompiled.routines.is_empty());
                        continue;
                    }
                    Err(error) => error.what,
                },
            };
            // One line for each kind of refusal: its numbers and names left
            // out.
            let kind: String = why
                .split(|c: char| c.is_ascii_digit() || c == '"')
                .next()
                .unwrap_or("")
                .trim()
                .to_string();
            refused
                .entry(kind)
                .or_default()
                .push(format!("{}/{name}: {why}", layer.name));
        }
    }
    let ms = started.elapsed().as_secs_f64() * 1000.0;
    outln!(
        "{}: {files} .ncs in {} layers, {} different scripts ({ms:.0} ms)",
        game.key(),
        layers.len(),
        seen.len()
    );
    outln!("  decompiled, and compiled again to the same bytes: {same}");
    outln!("    of those, compiled against an older nwscript.nss: {older}");
    outln!("  not decompiled: {}", seen.len() - same);
    let mut kinds: Vec<(&String, &Vec<String>)> = refused.iter().collect();
    kinds.sort_by_key(|(_, all)| std::cmp::Reverse(all.len()));
    for (_, all) in kinds.iter().take(20) {
        outln!("    {} like: {}", all.len(), all[0]);
    }
    Ok(())
}
