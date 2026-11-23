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

use std::path::Path;
use std::process::ExitCode;

use kjk_game::lighting_bakes::{self, Manifest, Progress, Report};
use serde_json::{Value, json};
use store::Store;
use vfs::Game;

/// The store, or why there is none.
fn opened() -> Result<Store, String> {
    Store::open().ok_or_else(|| "kjk found no folder for its store. Set KJK_STORE.".to_string())
}

/// A KOTOR game by its key.
fn kotor(key: &str) -> Result<Game, String> {
    Game::from_key(key)
        .filter(|game| *game != Game::JediAcademy)
        .ok_or_else(|| format!("{key} is not a game with a lighting download. Use k1 or tsl."))
}

/// Prints a failure: one JSON line for `game` with `json`, else the reason.
fn failed(json: bool, game: Option<Game>, why: &str) -> ExitCode {
    if json {
        let game = game.map_or(Value::Null, |game| json!(game.key()));
        outln!(
            "{}",
            json!({"games": [{"game": game, "status": "failed", "why": why}]})
        );
    } else {
        eprintln!("{why}");
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

/// The check's report as JSON.
fn report_json(report: &Report) -> Value {
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
    })
}

/// The manifest at `path`, checked against the player's files, with
/// progress lines under `json`.
fn checked(
    path: &Path,
    game: Option<&str>,
    json: bool,
) -> Result<(Manifest, Report), (Option<Game>, String)> {
    let manifest = Manifest::read(path).map_err(|why| (None, why))?;
    if let Some(key) = game {
        let asked = kotor(key).map_err(|why| (None, why))?;
        if asked != manifest.game {
            return Err((
                Some(asked),
                format!(
                    "{} is the lighting download for {}, not for {}.",
                    path.display(),
                    manifest.game.key(),
                    asked.key()
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
    .map_err(|why| (Some(manifest.game), why))?;
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
    let (_, report) = match checked(path, game, json) {
        Ok(found) => found,
        Err((game, why)) => return failed(json, game, &why),
    };
    if json {
        outln!("{}", json!({"check": report_json(&report)}));
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
        Err((game, why)) => return failed(json, game, &why),
    };
    let game = manifest.game;
    if let Some(refusal) = report.refusal() {
        let refusal = format!("{refusal} Nothing was installed.");
        if json {
            outln!(
                "{}",
                json!({
                    "games": [{"game": game.key(), "status": "refused", "why": refusal}],
                    "check": report_json(&report),
                })
            );
        } else {
            outln!("{refusal}");
        }
        return ExitCode::FAILURE;
    }
    let store = match opened() {
        Ok(store) => store,
        Err(why) => return failed(json, Some(game), &why),
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
                        "check": report_json(&report),
                    })
                );
            } else {
                for line in report.sentences() {
                    outln!("{line}");
                }
                outln!(
                    "The new lighting for {} is installed, with {} areas. Choose Lighting: Enhanced Lightmaps in the advanced graphics options to use it.",
                    game.key(),
                    manifest.areas.len()
                );
            }
            ExitCode::SUCCESS
        }
        Err(why) => failed(json, Some(game), &why),
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
        Err(why) => return failed(json, None, &why),
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
        match installed
            .as_ref()
            .map(|installed| (&installed.manifest, &installed.folder))
        {
            None => outln!("No lighting download is installed for {}.", game.key()),
            Some((Ok(manifest), folder)) => outln!(
                "The lighting download {} is installed for {}, with {} areas, in {}.",
                manifest.archive.file,
                game.key(),
                manifest.areas.len(),
                folder.display()
            ),
            Some((Err(why), folder)) => outln!(
                "The lighting download for {} in {} does not read, so the game does not use it: {why}",
                game.key(),
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
        Err(why) => return failed(json, None, &why),
    };
    let store = match opened() {
        Ok(store) => store,
        Err(why) => return failed(json, Some(game), &why),
    };
    match lighting_bakes::remove(&store, game) {
        Ok(removed) => {
            if json {
                let status = if removed { "removed" } else { "missing" };
                outln!(
                    "{}",
                    json!({"games": [{"game": game.key(), "status": status}]})
                );
            } else if removed {
                outln!("The lighting download for {} is removed.", game.key());
            } else {
                outln!("No lighting download was installed for {}.", game.key());
            }
            ExitCode::SUCCESS
        }
        Err(why) => failed(json, Some(game), &why),
    }
}
