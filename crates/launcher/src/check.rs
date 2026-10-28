use std::collections::BTreeMap;
use std::process::ExitCode;

use vfs::archive::Archive;
use vfs::chitin::Chitin;
use vfs::{DirIndex, Discovery, Game};

/// Archive extensions read by `check`.
const ARCHIVE_EXTENSIONS: &[&str] = &["erf", "mod", "rim", "sav", "hak"];

/// Saved games are the player's data, not the install's, and are skipped.
const PLAYER_FOLDERS: &[&str] = &["saves/", "cloudsaves/"];

/// Reads every archive of one install and prints counts and problems. Exits
/// non-zero when anything failed, so a run can gate on it.
pub fn run(discovery: &Discovery, game: &str) -> ExitCode {
    let Some(game) = Game::from_key(game) else {
        eprintln!("unknown game {game:?}: use k1, tsl or ja");
        return ExitCode::from(2);
    };
    let Some(install) = discovery.get(game) else {
        eprintln!("{}: not found (set {})", game.key(), game.env_var());
        return ExitCode::FAILURE;
    };
    let index = match DirIndex::build(&install.data_root) {
        Ok(index) => index,
        Err(error) => {
            eprintln!("{}: cannot index: {error}", install.data_root.display());
            return ExitCode::FAILURE;
        }
    };
    outln!("{}: {}", game.key(), install.root.display());
    if game == Game::JediAcademy {
        return if ja(&index) {
            ExitCode::SUCCESS
        } else {
            ExitCode::FAILURE
        };
    }

    let chitin_ok = chitin(&index);
    let archives_ok = archives(&index);
    if chitin_ok && archives_ok {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}

fn chitin(index: &DirIndex) -> bool {
    let chitin = match Chitin::load(index) {
        Ok(chitin) => chitin,
        Err(error) => {
            outln!("chitin.key: {error}");
            return false;
        }
    };
    let found = chitin.bifs.iter().filter(|bif| bif.path.is_some()).count();
    outln!(
        "chitin.key: {} bifs ({found} found), {} entries resolved, {} problems",
        chitin.bifs.len(),
        chitin.entries.len(),
        chitin.problems.len()
    );

    let mut problems: BTreeMap<&str, Vec<String>> = BTreeMap::new();
    for problem in &chitin.problems {
        problems
            .entry(problem.kind())
            .or_default()
            .push(format!("{problem:?}"));
    }
    for (kind, list) in &problems {
        outln!("  problem {kind}: {}", list.len());
        for example in list.iter().take(5) {
            outln!("    {example}");
        }
    }

    let mut types: BTreeMap<String, usize> = BTreeMap::new();
    let mut names: BTreeMap<(&str, u32), usize> = BTreeMap::new();
    for entry in &chitin.entries {
        *types.entry(entry.restype.to_string()).or_insert(0) += 1;
        *names.entry((&entry.resref, entry.restype.0)).or_insert(0) += 1;
    }
    let duplicates = names.values().filter(|count| **count > 1).count();
    outln!("  duplicate resref+type: {duplicates}");
    outln!("  types: {}", histogram(&types));
    chitin.problems.is_empty()
}

fn archives(index: &DirIndex) -> bool {
    let paths: Vec<_> = index
        .iter()
        .filter(|(key, _)| !PLAYER_FOLDERS.iter().any(|dir| key.starts_with(dir)))
        .filter(|(key, _)| {
            key.rsplit_once('.')
                .is_some_and(|(_, extension)| ARCHIVE_EXTENSIONS.contains(&extension))
        })
        .collect();

    let mut by_folder: BTreeMap<String, (usize, usize)> = BTreeMap::new();
    let mut signatures: BTreeMap<String, usize> = BTreeMap::new();
    let mut types: BTreeMap<String, usize> = BTreeMap::new();
    let mut failures = Vec::new();
    let mut out_of_bounds = Vec::new();
    let mut reserved = Vec::new();
    let mut duplicates = Vec::new();
    for (key, path) in &paths {
        let folder = key.rsplit_once('/').map_or("(root)", |(dir, _)| dir);
        let archive = match Archive::open(path) {
            Ok(archive) => archive,
            Err(error) => {
                failures.push(format!("{key}: {error}"));
                continue;
            }
        };
        let slot = by_folder.entry(folder.to_string()).or_insert((0, 0));
        slot.0 += 1;
        slot.1 += archive.entries.len();
        *signatures.entry(archive.signature.clone()).or_insert(0) += 1;
        let mut names: BTreeMap<(&str, u32), usize> = BTreeMap::new();
        for entry in &archive.entries {
            *types.entry(entry.restype.to_string()).or_insert(0) += 1;
            *names.entry((&entry.resref, entry.restype.0)).or_insert(0) += 1;
            if entry.reserved != 0 {
                reserved.push(format!("{key}: {}.{}", entry.resref, entry.restype));
            }
        }
        for ((resref, restype), count) in names.into_iter().filter(|(_, count)| *count > 1) {
            duplicates.push(format!(
                "{key}: {resref}.{} x{count}",
                fmt_odyssey::ResType(restype)
            ));
        }
        for entry in archive.out_of_bounds() {
            out_of_bounds.push(format!("{key}: {}.{}", entry.resref, entry.restype));
        }
    }

    let parsed: usize = by_folder.values().map(|(archives, _)| archives).sum();
    let entries: usize = by_folder.values().map(|(_, entries)| entries).sum();
    outln!(
        "archives: {} found, {parsed} parsed, {} failed, {entries} entries",
        paths.len(),
        failures.len()
    );
    for (folder, (archives, entries)) in &by_folder {
        outln!("  {folder}: {archives} archives, {entries} entries");
    }
    outln!(
        "  signatures: {}",
        histogram(&signatures.into_iter().collect())
    );
    for (label, list) in [
        ("failed", &failures),
        ("data out of bounds", &out_of_bounds),
        ("reserved bytes non-zero", &reserved),
        ("duplicate resref+type within an archive", &duplicates),
    ] {
        outln!("  {label}: {}", list.len());
        for example in list.iter().take(5) {
            outln!("    {example}");
        }
    }
    outln!("  types: {}", histogram(&types));
    failures.is_empty() && out_of_bounds.is_empty()
}

/// Every PK3 in load order, every entry read and CRC-checked, and how the
/// archives shadow one another.
fn ja(index: &DirIndex) -> bool {
    let stack = vfs::ja::JaStack::open(index);
    let order: Vec<&str> = stack.pk3s.iter().map(|pak| pak.name.as_str()).collect();
    outln!(
        "base/: {} pk3 in load order: {}",
        order.len(),
        order.join(" < ")
    );
    outln!("  loose files: {}", stack.loose.len());
    let mut failures = stack.problems.clone();
    let mut methods: BTreeMap<String, usize> = BTreeMap::new();
    for pak in &stack.pk3s {
        let files: Vec<_> = pak
            .entries
            .iter()
            .filter(|entry| !entry.is_folder())
            .collect();
        let mut names: BTreeMap<&str, usize> = BTreeMap::new();
        let mut bytes: u64 = 0;
        for entry in &files {
            *names.entry(&entry.name).or_insert(0) += 1;
            *methods
                .entry(format!("method {}", entry.method))
                .or_insert(0) += 1;
            match pak.read(entry) {
                Ok(data) => bytes += data.len() as u64,
                Err(error) => failures.push(format!("{}: {error}", pak.name)),
            }
        }
        let duplicates = names.values().filter(|count| **count > 1).count();
        outln!(
            "  {}: {} entries ({} files, {} folders), {bytes} bytes decoded, {duplicates} duplicate names",
            pak.name,
            pak.entries.len(),
            files.len(),
            pak.entries.len() - files.len()
        );
    }
    outln!("  compression: {}", histogram(&methods));

    let mut pairs: BTreeMap<String, usize> = BTreeMap::new();
    let all = stack.all();
    for resolved in all
        .values()
        .filter(|resolved| !resolved.shadowed.is_empty())
    {
        let shadowed: Vec<String> = resolved
            .shadowed
            .iter()
            .map(|source| stack.source_name(*source))
            .collect();
        *pairs
            .entry(format!(
                "{} over {}",
                stack.source_name(resolved.source),
                shadowed.join(", ")
            ))
            .or_insert(0) += 1;
    }
    outln!(
        "  names: {}, shadowed: {}",
        all.len(),
        pairs.values().sum::<usize>()
    );
    for (pair, count) in &pairs {
        outln!("    {pair}: {count}");
    }
    outln!("  failed: {}", failures.len());
    for failure in failures.iter().take(10) {
        outln!("    {failure}");
    }
    failures.is_empty()
}

fn histogram(counts: &BTreeMap<String, usize>) -> String {
    counts
        .iter()
        .map(|(name, count)| format!("{name} {count}"))
        .collect::<Vec<_>>()
        .join(", ")
}
