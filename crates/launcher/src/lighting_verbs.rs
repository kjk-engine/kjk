//! `kjk lighting`: the new lighting as a download (`kjk_game::lighting_bakes`).
//! The launcher downloads a game's zip and its manifest, and these commands
//! take the two files from its folder. kjk itself downloads nothing.
//!
//! - `check <manifest>` compares the manifest with the player's files.
//! - `install <manifest> <zip>` checks, then installs the zip into the
//!   store when enough areas match.
//! - `status` says what is installed for each game.
//! - `remove <game>` removes a game's download.
//!
//! With `--json` each command prints JSON objects, one a line, shaped like
//! `kjk installs --json`. A check and an install first print a line
//! `{"progress": {"step": "check" or "unpack", "done", "total", "area"}}`
//! for each area. The last line is the answer: `{"check": {...}}` for a
//! check, and `{"games": [...], "check": {...}}` for an install. Without
//! `--json` they print plain sentences.
//!
//! The check gives `bake_format`, the format of the download's bakes, and
//! `reads_format`, the format this kjk reads. When they differ, every area
//! keeps the game's own lighting and the install is refused.
//!
//! A game's line with the status `failed` or `refused`, and a refused
//! check, give `kind` beside the sentence `why`, for the launcher to act
//! on (`lighting_bakes::Kind`). The values are:
//!
//! - `no_store`: kjk found no folder for its store.
//! - `store_not_writable`: kjk may not write into the store's folder.
//! - `disk_full`: the store's disk is full.
//! - `in_use`: another program, such as a running kjk, holds a file of the
//!   store open.
//! - `game_not_found`: kjk did not find the game.
//! - `game_unreadable`: kjk found the game, and its files could not be read.
//! - `list_unreadable`: the manifest's file could not be read.
//! - `list_invalid`: the manifest is not one this kjk reads.
//! - `wrong_game`: the manifest is for another game than `--game`, or the
//!   game asked for has no download.
//! - `zip_unreadable`: the zip could not be opened.
//! - `zip_mismatch`: the zip's size or BLAKE3 is not the manifest's, as for
//!   a damaged or incomplete download.
//! - `zip_damaged`: the zip does not unpack, or holds files the manifest
//!   does not list.
//! - `files_differ`: too few areas of the player's files match (refused).
//! - `bake_format`: the download is of another bake format (refused).
//! - `other`: anything else.
//!
//! A failed line of the kinds `store_not_writable`, `disk_full` and
//! `in_use` also gives `store`, the store's folder. An older kjk gives
//! none of `kind`, `store`, `bake_format` and `reads_format`.

use std::path::Path;
use std::process::ExitCode;

use kjk_game::bake_fingerprint;
use kjk_game::lighting_bakes::{self, Failure, Kind, Manifest, Progress, Report};
use serde_json::{Value, json};
use store::Store;
use vfs::Game;

/// The store, or why there is none.
fn opened() -> Result<Store, Failure> {
    Store::open().ok_or_else(|| {
        Failure::new(
            Kind::NoStore,
            "kjk found no folder for its store. Set KJK_STORE.",
        )
    })
}

/// A KOTOR game by its key.
fn kotor(key: &str) -> Result<Game, Failure> {
    Game::from_key(key)
        .filter(|game| *game != Game::JediAcademy)
        .ok_or_else(|| {
            Failure::new(
                Kind::WrongGame,
                format!("{key} is not a game with a lighting download. Use k1 or tsl."),
            )
        })
}

/// Prints a failure: one JSON line for `game` with `json`, else the reason.
/// A failure to write into the store names the store's folder as `store`.
fn failed(json: bool, game: Option<Game>, failure: &Failure) -> ExitCode {
    if json {
        let game = game.map_or(Value::Null, |game| json!(game.key()));
        let mut line = json!({
            "game": game,
            "status": "failed",
            "why": failure.why,
            "kind": failure.kind,
        });
        if matches!(
            failure.kind,
            Kind::StoreNotWritable | Kind::DiskFull | Kind::InUse
        ) && let Some(store) = Store::open()
        {
            line["store"] = json!(store.root());
        }
        outln!("{}", json!({"games": [line]}));
    } else {
        eprintln!("{}", failure.why);
    }
    ExitCode::FAILURE
}

/// A progress line for the launcher.
fn progress_line(step: &str, progress: &Progress) {
    outln!(
        "{}",
        json!({"progress": {
            "step": step,
            "done": progress.done,
            "total": progress.total,
            "area": progress.area,
        }})
    );
}

/// The check's report of `manifest` as JSON.
fn report_json(report: &Report, manifest: &Manifest) -> Value {
    json!({
        "game": report.game,
        "status": if report.allowed { "allowed" } else { "refused" },
        "needs": report.needs,
        "areas": report.areas,
        "matching": report.matching(),
        "matched": report.matched,
        "placeables_differ": report.placeables_differ,
        "fall_back": report.fall_back,
        "cached": report.cached,
        "why": report.refusal(),
        "kind": report.refusal_kind(manifest.format),
        "bake_format": manifest.format,
        "reads_format": bake_fingerprint::FORMAT,
    })
}

/// The manifest at `path`, checked against the player's files, with
/// progress lines under `json`.
fn checked(
    path: &Path,
    game: Option<&str>,
    json: bool,
) -> Result<(Manifest, Report), (Option<Game>, Failure)> {
    let manifest = Manifest::load(path).map_err(|failure| (None, failure))?;
    if let Some(key) = game {
        let asked = kotor(key).map_err(|failure| (None, failure))?;
        if asked != manifest.game {
            return Err((
                Some(asked),
                Failure::new(
                    Kind::WrongGame,
                    format!(
                        "{} is the lighting download for {}, not for {}.",
                        path.display(),
                        lighting_bakes::game_name(manifest.game),
                        lighting_bakes::game_name(asked)
                    ),
                ),
            ));
        }
    }
    let store = Store::open();
    let started = std::time::Instant::now();
    let report = lighting_bakes::check(store.as_ref(), &manifest, &mut |progress| {
        if json {
            progress_line("check", &progress);
        }
    })
    .map_err(|failure| (Some(manifest.game), failure))?;
    if !json {
        let how = if report.cached {
            "from an earlier check".to_string()
        } else {
            format!("in {:.1} seconds", started.elapsed().as_secs_f64())
        };
        outln!("Checked {} areas {how}.", report.areas);
    }
    Ok((manifest, report))
}

/// `kjk lighting check <manifest> [--game] [--json]`.
pub(crate) fn check(path: &Path, game: Option<&str>, json: bool) -> ExitCode {
    let (manifest, report) = match checked(path, game, json) {
        Ok(found) => found,
        Err((game, failure)) => return failed(json, game, &failure),
    };
    if json {
        outln!("{}", json!({"check": report_json(&report, &manifest)}));
    } else {
        for line in report.sentences() {
            outln!("{line}");
        }
    }
    if report.allowed {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}

/// `kjk lighting install <manifest> <zip> [--json]`: the check, then the
/// install when it allows one.
pub(crate) fn install(path: &Path, zip: &Path, json: bool) -> ExitCode {
    let (manifest, report) = match checked(path, None, json) {
        Ok(found) => found,
        Err((game, failure)) => return failed(json, game, &failure),
    };
    let game = manifest.game;
    if let Some(refusal) = report.refusal() {
        let refusal = format!("{refusal} Nothing was installed.");
        if json {
            outln!(
                "{}",
                json!({
                    "games": [{
                        "game": game.key(),
                        "status": "refused",
                        "why": refusal,
                        "kind": report.refusal_kind(manifest.format),
                    }],
                    "check": report_json(&report, &manifest),
                })
            );
        } else {
            outln!("{refusal}");
        }
        return ExitCode::FAILURE;
    }
    let store = match opened() {
        Ok(store) => store,
        Err(failure) => return failed(json, Some(game), &failure),
    };
    let installed = lighting_bakes::install(&store, &manifest, zip, &mut |progress| {
        if json {
            progress_line("unpack", &progress);
        }
    });
    match installed {
        Ok(folder) => {
            if json {
                outln!(
                    "{}",
                    json!({
                        "games": [{
                            "game": game.key(),
                            "status": "installed",
                            "path": folder,
                            "areas": manifest.areas.len(),
                        }],
                        "check": report_json(&report, &manifest),
                    })
                );
            } else {
                for line in report.sentences() {
                    outln!("{line}");
                }
                outln!(
                    "The new lighting for {} is installed, with {} areas. Choose Lighting: Enhanced Lightmaps in the advanced graphics options to use it.",
                    lighting_bakes::game_name(game),
                    manifest.areas.len()
                );
            }
            ExitCode::SUCCESS
        }
        Err(failure) => failed(json, Some(game), &failure),
    }
}

/// One game's line of `status`.
fn status_json(game: Game, installed: Option<&lighting_bakes::Installed>) -> Value {
    match installed {
        None => json!({"game": game.key(), "status": "missing"}),
        Some(installed) => match &installed.manifest {
            Ok(manifest) => json!({
                "game": game.key(),
                "status": "installed",
                "path": installed.folder,
                "archive": manifest.archive.file,
                "areas": manifest.areas.len(),
                "needs": manifest.needs,
            }),
            Err(why) => json!({
                "game": game.key(),
                "status": "broken",
                "path": installed.folder,
                "why": why,
            }),
        },
    }
}

/// `kjk lighting status [--json]`.
pub(crate) fn status(json: bool) -> ExitCode {
    let store = match opened() {
        Ok(store) => store,
        Err(failure) => return failed(json, None, &failure),
    };
    let found = lighting_bakes::status(&store);
    if json {
        let games: Vec<Value> = (found.iter())
            .map(|(game, installed)| status_json(*game, installed.as_ref()))
            .collect();
        outln!("{}", json!({"games": games}));
        return ExitCode::SUCCESS;
    }
    for (game, installed) in &found {
        let name = lighting_bakes::game_name(*game);
        match installed
            .as_ref()
            .map(|installed| (&installed.manifest, &installed.folder))
        {
            None => outln!("No lighting download is installed for {name}."),
            Some((Ok(manifest), folder)) => outln!(
                "The lighting download {} is installed for {name}, with {} areas, in {}.",
                manifest.archive.file,
                manifest.areas.len(),
                folder.display()
            ),
            Some((Err(why), folder)) => outln!(
                "The lighting download for {name} in {} does not read, so the game does not use it: {why}",
                folder.display()
            ),
        }
    }
    ExitCode::SUCCESS
}

/// `kjk lighting remove <game> [--json]`.
pub(crate) fn remove(key: &str, json: bool) -> ExitCode {
    let game = match kotor(key) {
        Ok(game) => game,
        Err(failure) => return failed(json, None, &failure),
    };
    let store = match opened() {
        Ok(store) => store,
        Err(failure) => return failed(json, Some(game), &failure),
    };
    let name = lighting_bakes::game_name(game);
    match lighting_bakes::remove(&store, game) {
        Ok(removed) => {
            if json {
                let status = if removed { "removed" } else { "missing" };
                outln!(
                    "{}",
                    json!({"games": [{"game": game.key(), "status": status}]})
                );
            } else if removed {
                outln!("The lighting download for {name} is removed.");
            } else {
                outln!("No lighting download was installed for {name}.");
            }
            ExitCode::SUCCESS
        }
        Err(failure) => failed(json, Some(game), &failure),
    }
}
