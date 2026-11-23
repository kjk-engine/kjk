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
//!   as the round `c1` in `<module>/bake/c1/<area>/b`. Without it, two sets
//!   of one area stop the pack.
//! - A set without `fingerprint.txt` is not finished, or was never stamped.
//!   It is left out and listed.
//! - A set whose fingerprint does not read, or is of another format than
//!   this build's (`bake_fingerprint::FORMAT`), stops the pack.
//! - A module baked with `--stored-backdrop` is left out and listed. Its
//!   backdrop PNGs are copies of the games' own lightmaps. The bake's log
//!   (`detail-bake.txt`, beside the set) says so in its second line, and
//!   `data/lights/bake.toml` gives such a module `backdrop = "stored"`.
//!
//! The zip holds `<area>/<file>`. PNGs are stored as they are, and the text
//! files are deflated. The manifest gives each area's module and
//! fingerprint, the zip's BLAKE3 and size, and the patch the bakes need.

use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use kjk_game::bake_fingerprint;
use kjk_game::lighting_bakes::{self, Archive, Area, Manifest};
use vfs::Game;

/// The modules `data/lights/bake.toml` bakes with their backdrop at its
/// stored light.
const BAKE_TOML: &str = include_str!("../../../data/lights/bake.toml");

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
/// `--stored-backdrop`: its bake's log says so, or `bake.toml` asks for it.
fn stored_backdrop(module: &str, set: &Path) -> bool {
    let logged = set
        .parent()
        .map(|area| area.join("detail-bake.txt"))
        .and_then(|log| std::fs::read_to_string(log).ok())
        .is_some_and(|log| log.contains(STORED_BACKDROP_LOG));
    logged || stored_backdrop_listed(module)
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
        if stored_backdrop(&module, &path) {
            backdrops.push(path);
            continue;
        }
        if let Some(other) = sets.iter().find(|set| set.print.area == print.area) {
            return Err(format!(
                "{} and {} both hold a bake of area {}. Name the round to pack with --round.",
                other.path.display(),
                path.display(),
                print.area
            ));
        }
        sets.push(Set { module, path, print });
    }
    if !unfinished.is_empty() {
        outln!("left out, with no {} (not finished or not stamped):", bake_fingerprint::FILE);
        for path in &unfinished {
            outln!("  {}", path.display());
        }
    }
    if !backdrops.is_empty() {
        outln!("left out, baked with --stored-backdrop (its PNGs copy the games' own lightmaps):");
        for path in &backdrops {
            outln!("  {}", path.display());
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
            })
            .collect(),
    };
    let manifest_path = out.join(format!("{name}.txt"));
    std::fs::write(&manifest_path, manifest.text())
        .map_err(|error| format!("{}: {error}", manifest_path.display()))?;
    outln!(
        "packed {} areas, {files} files of {:.1} MB, into {} ({:.1} MB)",
        sets.len(),
        bytes_in as f64 / 1e6,
        zip_path.display(),
        bytes as f64 / 1e6
    );
    outln!("wrote {}", manifest_path.display());
    Ok(())
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
        let mut names: Vec<String> = std::fs::read_dir(&set.path)
            .map_err(|error| format!("{}: {error}", set.path.display()))?
            .flatten()
            .filter(|entry| entry.path().is_file())
            .filter_map(|entry| entry.file_name().to_str().map(str::to_string))
            .filter(|name| lighting_bakes::shipped(name))
            .collect();
        names.sort();
        for name in names {
            let source = set.path.join(&name);
            let bytes =
                std::fs::read(&source).map_err(|error| format!("{}: {error}", source.display()))?;
            let options = if name.to_ascii_lowercase().ends_with(".png") {
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
