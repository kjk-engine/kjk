//! `kjk lighting pack`: a game's finished bakes packed into the lighting
//! download, one zip and its manifest (`kjk_game::lighting_bakes`). A
//! developer's command, so it is built with the `probes` feature only.
//!
//! The folder holds one folder for each module, as the canon bakes do
//! (`target/canon-bakes/<k1|tsl>/<module>/...`). Every set named `b` under a
//! module's folder is one area's set of lightmaps, the set the game loads.
//! From each set the zip takes only the files the game loads
//! (`lighting_bakes::shipped`): the PNGs, `fill.txt`, `floor.txt`,
//! `unlit.txt`, `placeables.txt` and `fingerprint.txt`. The probe files, the
//! bake's log and the set `a` stay out.
//!
//! - `--round <name>` takes only the sets under a folder of that name, such
//!   as the round `c1` in `<module>/bake/c1/<area>/b`.
//! - Some modules share their area with another module and are baked with
//!   that module's settings (`tools/light-kinds/canon/modules-<game>.txt`).
//!   The store holds one set for each area, so the pack ships the set of
//!   the module that gives its settings, and lists the other sets as left
//!   out (`choose_sets`). Any other two sets of one area stop the pack.
//! - A set without `fingerprint.txt` is not finished, or was never stamped.
//!   It is left out and listed.
//! - A set whose fingerprint does not read, or is of another format than
//!   this build's (`bake_fingerprint::FORMAT`), stops the pack.
//! - A module baked with `--stored-backdrop` names its backdrop's
//!   lightmaps in `stored.txt` and holds no picture of them, so it is packed
//!   like any other. A PNG that `stored.txt` names is left out all the same.
//!   A set baked with `--stored-backdrop` before the bake wrote `stored.txt`
//!   holds copies of the games' own lightmaps. It is left out and listed.
//!   The bake's log (`detail-bake.txt`, beside the set) says that a bake
//!   kept its backdrop as stored in its second line, and
//!   `data/lights/bake.toml` gives such a module `backdrop = "stored"`.
//!
//! The zip holds `<area>/<file>`. PNGs are stored as they are, and the text
//! files are deflated. The manifest gives each area's module and
//! fingerprint, the zip's BLAKE3 and size, and the patch the bakes need.
//!
//! When every packed set has exact lines (`bake_fingerprint::Exact`), the
//! pack also writes the second manifest, `<name>.manifest-2.txt`, which
//! holds them (`Manifest::exact_text`).
//!
//! The pack also writes `SHA256SUMS` beside the zip and the manifests, with
//! the SHA-256 of each, as the launcher reads it (`write_sums`). It keeps
//! the lines of the other files there, so both games can be packed into one
//! folder.
//!
//! `kjk lighting restamp <manifest> <zip> --out <file>` writes the second
//! manifest of a download packed before the exact lines, from its zip and
//! the files on this machine (`lighting_bakes::restamp`). The zip stays as
//! it is. It reads the Steam Workshop items as a player's game does, even in
//! a build with the probes, which leaves them out of every other verb.

use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use kjk_game::bake_fingerprint;
use kjk_game::lighting_bakes::{self, Archive, Area, Manifest};
use vfs::Game;

/// The modules `data/lights/bake.toml` bakes with their backdrop at its
/// stored light.
const BAKE_TOML: &str = include_str!("../../../data/lights/bake.toml");

/// The canon round's modules of each game. A line names a module and the
/// module whose settings it is baked with (`tools/light-kinds/canon`).
const MODULES_K1: &str = include_str!("../../../tools/light-kinds/canon/modules-k1.txt");
const MODULES_TSL: &str = include_str!("../../../tools/light-kinds/canon/modules-tsl.txt");

/// The words the bake's log writes in its second line for a bake with
/// `--stored-backdrop` (`detail_bake::gpu`).
const STORED_BACKDROP_LOG: &str = "with no walkable face kept as stored";

/// One area's set to pack.
struct Set {
    module: String,
    path: PathBuf,
    print: bake_fingerprint::Fingerprint,
}

/// `kjk lighting pack <game> <folder> --out <dir> [--label <label>]
/// [--round <name>]`.
pub(crate) fn run(
    game: &str,
    folder: &Path,
    out: &Path,
    label: &str,
    round: Option<&str>,
) -> ExitCode {
    match pack(game, folder, out, label, round) {
        Ok(()) => ExitCode::SUCCESS,
        Err(why) => {
            eprintln!("lighting pack: {why}");
            ExitCode::FAILURE
        }
    }
}

/// Every folder named `b` under `folder`, sorted.
fn sets_under(folder: &Path) -> Vec<PathBuf> {
    let mut found = Vec::new();
    let mut folders = vec![folder.to_path_buf()];
    while let Some(folder) = folders.pop() {
        let Ok(entries) = std::fs::read_dir(&folder) else {
            continue;
        };
        for path in entries.flatten().map(|entry| entry.path()) {
            if !path.is_dir() {
                continue;
            }
            if path.file_name().is_some_and(|name| name == "b") {
                found.push(path);
            } else if path.file_name().is_some_and(|name| name != "a") {
                folders.push(path);
            }
        }
    }
    found.sort();
    found
}

/// Whether `module`'s entry in `data/lights/bake.toml` has
/// `backdrop = "stored"`.
fn stored_backdrop_listed(module: &str) -> bool {
    let header = format!("[modules.{module}]");
    let mut inside = false;
    for line in BAKE_TOML.lines().map(str::trim) {
        if line.starts_with('[') {
            inside = line.eq_ignore_ascii_case(&header);
        } else if inside
            && line.split_once('=').is_some_and(|(key, value)| {
                key.trim() == "backdrop" && value.trim().trim_matches('"') == "stored"
            })
        {
            return true;
        }
    }
    false
}

/// Whether the set `set` of module `module` was baked with
/// `--stored-backdrop` before the bake wrote `stored.txt`, so that its
/// backdrop's PNGs copy the games' own lightmaps: its bake's log says it
/// kept the backdrop as stored, or `bake.toml` asks for that, and the set
/// has no `stored.txt`.
fn copied_backdrop(module: &str, set: &Path) -> bool {
    if set.join(kjk_game::stored_fill::STORED_FILE).is_file() {
        return false;
    }
    let logged = set
        .parent()
        .map(|area| area.join("detail-bake.txt"))
        .and_then(|log| std::fs::read_to_string(log).ok())
        .is_some_and(|log| log.contains(STORED_BACKDROP_LOG));
    logged || stored_backdrop_listed(module)
}

/// The module whose settings `module` is baked with in the canon round, as
/// the list `modules` gives it. A module not listed takes its own settings.
fn settings_from<'a>(modules: &'a str, module: &'a str) -> &'a str {
    (modules.lines())
        .filter_map(|line| line.split_once(char::is_whitespace))
        .find(|(name, _)| name.eq_ignore_ascii_case(module))
        .map_or(module, |(_, from)| from.trim())
}

/// A set left out, and the module whose set of the same area is packed.
type LeftOut = (Set, String);

/// One set for each area of `sets`, and the sets left out with the module
/// whose set was shipped in their place.
///
/// The canon driver bakes a module that shares another module's area with
/// that module's settings, and its list of modules names that module on
/// the sharing module's line. The pack keeps the set of the module whose
/// settings the others take. The list is the rule because it is the record
/// the bakes were made from, and it is in the repository. A guess from the
/// names would fail, since a module's name does not always hold its area's
/// name (`danm13` holds `m13aa`). A set is left out only when the module it
/// takes its settings from has a set of the same area. Two sets of one area
/// that the list does not tie together stop the pack.
fn choose_sets(modules: &str, sets: Vec<Set>) -> Result<(Vec<Set>, Vec<LeftOut>), String> {
    let (mut kept, mut left): (Vec<Set>, Vec<LeftOut>) = (Vec::new(), Vec::new());
    let giver = |set: &Set| settings_from(modules, &set.module).to_ascii_lowercase();
    let (takers, givers): (Vec<Set>, Vec<Set>) = sets.into_iter().partition(|set| {
        giver(set) != set.module
    });
    for set in givers {
        if let Some(other) = kept.iter().find(|other| other.print.area == set.print.area) {
            return Err(format!(
                "{} and {} both hold a bake of area {}. Name the round to pack with --round.",
                other.path.display(),
                set.path.display(),
                set.print.area
            ));
        }
        kept.push(set);
    }
    for set in takers {
        let from = giver(&set);
        if kept.iter().any(|other| other.print.area == set.print.area && other.module == from) {
            left.push((set, from));
        } else if let Some(other) = kept.iter().find(|other| other.print.area == set.print.area) {
            return Err(format!(
                "{} and {} both hold a bake of area {}. Name the round to pack with --round.",
                other.path.display(),
                set.path.display(),
                set.print.area
            ));
        } else {
            kept.push(set);
        }
    }
    Ok((kept, left))
}

fn pack(
    game: &str,
    folder: &Path,
    out: &Path,
    label: &str,
    round: Option<&str>,
) -> Result<(), String> {
    let game = Game::from_key(game)
        .filter(|game| *game != Game::JediAcademy)
        .ok_or_else(|| format!("{game}: use k1 or tsl"))?;
    if label.is_empty() || !label.bytes().all(|byte| byte.is_ascii_alphanumeric() || b"_-.".contains(&byte)) {
        return Err(format!("the label {label:?} must be letters, digits, '_', '-' or '.'"));
    }
    let mut sets: Vec<Set> = Vec::new();
    let (mut unfinished, mut backdrops) = (Vec::new(), Vec::new());
    for path in sets_under(folder) {
        let relative = path.strip_prefix(folder).unwrap_or(&path);
        if let Some(round) = round
            && !relative.components().any(|part| part.as_os_str() == round)
        {
            continue;
        }
        let module = (relative.components().next())
            .and_then(|first| first.as_os_str().to_str())
            .map(str::to_ascii_lowercase)
            .ok_or_else(|| format!("{} is not under a module's folder", path.display()))?;
        let print = match bake_fingerprint::read(&path) {
            None => {
                unfinished.push(path);
                continue;
            }
            Some(Err(why)) => return Err(why),
            Some(Ok(print)) => print,
        };
        if print.format != bake_fingerprint::FORMAT {
            return Err(format!(
                "{} holds a bake of format {}, and this build reads format {}. Bake it again.",
                path.display(),
                print.format,
                bake_fingerprint::FORMAT
            ));
        }
        if copied_backdrop(&module, &path) {
            backdrops.push(path);
            continue;
        }
        sets.push(Set { module, path, print });
    }
    let modules = if game == Game::Kotor1 { MODULES_K1 } else { MODULES_TSL };
    let (mut sets, shared) = choose_sets(modules, sets)?;
    if !unfinished.is_empty() {
        outln!("left out, with no {} (not finished or not stamped):", bake_fingerprint::FILE);
        for path in &unfinished {
            outln!("  {}", path.display());
        }
    }
    if !backdrops.is_empty() {
        outln!("left out, baked with --stored-backdrop before stored.txt (its PNGs copy the games' own lightmaps):");
        for path in &backdrops {
            outln!("  {}", path.display());
        }
    }
    if !shared.is_empty() {
        outln!("left out, of a module baked with the settings of another module of the same area, whose set is packed:");
        for (set, from) in &shared {
            outln!("  {} (area {}, packed from {from})", set.path.display(), set.print.area);
        }
    }
    if sets.is_empty() {
        return Err(format!("{} holds no finished set to pack", folder.display()));
    }
    sets.sort_by(|a, b| a.print.area.cmp(&b.print.area));
    std::fs::create_dir_all(out).map_err(|error| format!("{}: {error}", out.display()))?;
    let name = format!("kjk-lighting-{}-{label}", game.key());
    let zip_path = out.join(format!("{name}.zip"));
    let (files, bytes_in) = write_zip(&zip_path, &sets)?;
    let (blake3, bytes) = lighting_bakes::digest_of(&zip_path)?;
    let manifest = Manifest {
        game,
        format: bake_fingerprint::FORMAT,
        needs: lighting_bakes::needs(game).to_string(),
        archive: Archive {
            file: format!("{name}.zip"),
            blake3,
            bytes,
        },
        areas: (sets.iter())
            .map(|set| Area {
                area: set.print.area.clone(),
                module: set.module.clone(),
                rooms: set.print.rooms.clone(),
                placeables: set.print.placeables.clone(),
                exact: set.print.exact.clone(),
            })
            .collect(),
    };
    let manifest_path = out.join(format!("{name}.txt"));
    std::fs::write(&manifest_path, manifest.text())
        .map_err(|error| format!("{}: {error}", manifest_path.display()))?;
    let mut written = vec![zip_path.clone(), manifest_path.clone()];
    if manifest.areas.iter().all(|area| area.exact.is_some()) {
        let second = out.join(second_manifest(&name));
        std::fs::write(&second, manifest.exact_text())
            .map_err(|error| format!("{}: {error}", second.display()))?;
        outln!("wrote {}", second.display());
        written.push(second);
    } else {
        outln!("no second manifest: some sets have no exact lines. Stamp them again with kjk detail-bake --stamp.");
    }
    outln!(
        "packed {} areas, {files} files of {:.1} MB, into {} ({:.1} MB)",
        sets.len(),
        bytes_in as f64 / 1e6,
        zip_path.display(),
        bytes as f64 / 1e6
    );
    outln!("wrote {}", manifest_path.display());
    let written: Vec<&Path> = written.iter().map(PathBuf::as_path).collect();
    let sums = write_sums(out, &written)?;
    outln!("wrote {}", sums.display());
    Ok(())
}

/// The file name of the second manifest of the download `name`.
fn second_manifest(name: &str) -> String {
    format!("{name}.manifest-2.txt")
}

/// `kjk lighting restamp <manifest> <zip> --out <file>`: the second
/// manifest of a download packed before the exact lines, stamped over this
/// machine's files with their Steam Workshop items. It lists the areas it
/// left without exact lines, and the lightmaps that no room mesh draws.
pub(crate) fn restamp(manifest: &Path, zip: &Path, out: &Path) -> ExitCode {
    let result = Manifest::read(manifest).and_then(|manifest| {
        let started = std::time::Instant::now();
        let restamped = lighting_bakes::restamp(&manifest, zip, &mut |_| {}).map_err(String::from)?;
        std::fs::write(out, restamped.manifest.exact_text()).map_err(|error| format!("{}: {error}", out.display()))?;
        let stamped = manifest.areas.len() - restamped.skipped.len();
        let workshop = if restamped.workshop { "with" } else { "without" };
        outln!(
            "stamped {stamped} of {} areas over the game's files {workshop} the Steam Workshop items in {:.1} seconds, into {}",
            manifest.areas.len(),
            started.elapsed().as_secs_f64(),
            out.display()
        );
        if !restamped.skipped.is_empty() {
            outln!("left with their old lines only, checked by the old rules:");
            for (area, why) in &restamped.skipped {
                outln!("  {area}: {why}");
            }
        }
        if !restamped.undrawn.is_empty() {
            outln!("lightmaps with a PNG that no room mesh draws, stamped as drawn by no triangles:");
            for (area, names) in &restamped.undrawn {
                outln!("  {area}: {}", names.join(", "));
            }
        }
        Ok(())
    });
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(why) => {
            eprintln!("lighting restamp: {why}");
            ExitCode::FAILURE
        }
    }
}

/// The file of SHA-256 checksums beside the downloads, which the launcher
/// checks each file against before it hands the file to kjk.
pub const SUMS: &str = "SHA256SUMS";

/// The SHA-256 of the file at `path`, hexadecimal.
pub fn sha256_of(path: &Path) -> Result<String, String> {
    use sha2::Digest as _;
    let failed = |error: std::io::Error| format!("{}: {error}", path.display());
    let mut file = std::fs::File::open(path).map_err(failed)?;
    let mut hasher = sha2::Sha256::new();
    std::io::copy(&mut file, &mut hasher).map_err(failed)?;
    Ok(hasher
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect())
}

/// Puts the SHA-256 of each of `files` into `SHA256SUMS` in `out`, in the
/// format of `sha256sum`: the checksum, two spaces and the file's name, one
/// file a line, sorted by name. The lines of the other files stay as they
/// were, so the two games packed into one folder in two runs are both
/// listed. A second run with the same files writes the same text. Returns
/// the file's path.
fn write_sums(out: &Path, files: &[&Path]) -> Result<PathBuf, String> {
    let path = out.join(SUMS);
    let mut lines = std::collections::BTreeMap::new();
    match std::fs::read_to_string(&path) {
        Ok(text) => {
            for (number, line) in text.lines().enumerate() {
                if line.trim().is_empty() {
                    continue;
                }
                let mut words = line.split_whitespace();
                let (Some(hash), Some(name), None) = (words.next(), words.next(), words.next())
                else {
                    return Err(format!(
                        "line {} of {} does not read: {line}",
                        number + 1,
                        path.display()
                    ));
                };
                let name = name.trim_start_matches('*');
                lines.insert(name.to_string(), hash.to_ascii_lowercase());
            }
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(format!("{}: {error}", path.display())),
    }
    for file in files {
        let name = (file.file_name())
            .and_then(|name| name.to_str())
            .ok_or_else(|| format!("{} has no file name", file.display()))?;
        lines.insert(name.to_string(), sha256_of(file)?);
    }
    let text: String = (lines.iter())
        .map(|(name, hash)| format!("{hash}  {name}\n"))
        .collect();
    std::fs::write(&path, text).map_err(|error| format!("{}: {error}", path.display()))?;
    Ok(path)
}

/// Writes the files the game loads from each of `sets` into a zip at
/// `path`, under `<area>/`. How many files and bytes went in.
fn write_zip(path: &Path, sets: &[Set]) -> Result<(usize, u64), String> {
    let failed = |error: &dyn std::fmt::Display| format!("{}: {error}", path.display());
    let file = std::fs::File::create(path).map_err(|error| failed(&error))?;
    let mut zip = ::zip::ZipWriter::new(std::io::BufWriter::new(file));
    let stored = ::zip::write::SimpleFileOptions::default()
        .compression_method(::zip::CompressionMethod::Stored);
    let deflated = ::zip::write::SimpleFileOptions::default()
        .compression_method(::zip::CompressionMethod::Deflated);
    let (mut files, mut bytes_in) = (0, 0);
    for set in sets {
        let backdrop = kjk_game::stored_fill::stored_names(&set.path);
        let mut names: Vec<String> = std::fs::read_dir(&set.path)
            .map_err(|error| format!("{}: {error}", set.path.display()))?
            .flatten()
            .filter(|entry| entry.path().is_file())
            .filter_map(|entry| entry.file_name().to_str().map(str::to_string))
            .filter(|name| lighting_bakes::shipped(name))
            .filter(|name| {
                // A backdrop's lightmap the game makes from the player's own.
                let lower = name.to_ascii_lowercase();
                !(lower
                    .strip_suffix(".klm")
                    .or_else(|| lower.strip_suffix(".png")))
                    .is_some_and(|stem| backdrop.contains(stem))
            })
            .collect();
        names.sort();
        for name in names {
            let source = set.path.join(&name);
            let bytes =
                std::fs::read(&source).map_err(|error| format!("{}: {error}", source.display()))?;
            let lower = name.to_ascii_lowercase();
            let options = if lower.ends_with(".png") || lower.ends_with(".klm") {
                stored
            } else {
                deflated
            };
            zip.start_file(format!("{}/{}", set.print.area, name.to_ascii_lowercase()), options)
                .map_err(|error| failed(&error))?;
            zip.write_all(&bytes).map_err(|error| failed(&error))?;
            files += 1;
            bytes_in += bytes.len() as u64;
        }
    }
    let writer = zip.finish().map_err(|error| failed(&error))?;
    writer.into_inner().map_err(|error| failed(&error.error()))?;
    Ok((files, bytes_in))
}

pub mod probes;
