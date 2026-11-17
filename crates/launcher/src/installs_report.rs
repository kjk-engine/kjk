//! `kjk installs --json`: the game folders kjk finds and whether `ffmpeg` is
//! there, as one JSON object for the launcher (`kjk-engine/kjk-launcher`).
//! `--check <game> <path>` reports one folder a player picked and writes
//! nothing.

use std::collections::HashMap;
use std::path::Path;
use std::process::ExitCode;

use serde_json::{Value, json};
use vfs::{Discovery, Game, Source};

/// Each game in the order k1, tsl, ja: found (with its folder and where it
/// was found), refused (with the folder and why), or missing. `chosen` holds
/// the folders the settings name, by variable; `env` is the process
/// environment. A variable's install counts as `settings` when only the
/// settings name it.
pub(crate) fn report(
    discovery: &Discovery,
    chosen: &HashMap<String, String>,
    env: impl Fn(&str) -> Option<String>,
    ffmpeg: bool,
) -> Value {
    let source = |source: &Source| match source {
        Source::SteamLibrary(_) => "steam",
        Source::GogDefault => "gog",
        Source::Env(var) if chosen.contains_key(*var) && env(var).is_none() => "settings",
        Source::Env(_) => "env",
    };
    let games: Vec<Value> = Game::ALL
        .into_iter()
        .map(|game| {
            if let Some(install) = discovery.get(game) {
                json!({
                    "game": game.key(),
                    "status": "found",
                    "path": install.root,
                    "source": source(&install.source),
                })
            } else if let Some(refusal) = discovery.refusals.iter().find(|r| r.game == game) {
                json!({
                    "game": game.key(),
                    "status": "refused",
                    "path": refusal.path,
                    "source": source(&refusal.source),
                    "why": refusal.reason,
                })
            } else {
                json!({"game": game.key(), "status": "missing"})
            }
        })
        .collect();
    let ffmpeg = if ffmpeg { "found" } else { "missing" };
    json!({"games": games, "ffmpeg": {"status": ffmpeg}})
}

/// `path` checked as an install of `game`, as `discover` checks a folder:
/// one entry, found or refused, and no `ffmpeg`.
pub(crate) fn check(game: Game, path: &Path) -> Value {
    let entry = match vfs::install_at(game, path) {
        Ok(install) => json!({
            "game": game.key(),
            "status": "found",
            "path": install.root,
            "source": "settings",
        }),
        Err(why) => json!({
            "game": game.key(),
            "status": "refused",
            "path": path,
            "source": "settings",
            "why": why,
        }),
    };
    json!({"games": [entry]})
}

/// `kjk installs`: the report (or with `check`, one folder's check), as
/// one JSON line with `json` and as plain lines without.
pub(crate) fn run(json: bool, check_args: Option<Vec<String>>) -> ExitCode {
    let value = match check_args.as_deref() {
        Some([game, path]) => match Game::from_key(game) {
            Some(game) => check(game, Path::new(path)),
            None => {
                eprintln!("--check {game}: k1, tsl or ja");
                return ExitCode::from(2);
            }
        },
        Some(_) => {
            eprintln!("--check takes a game (k1, tsl or ja) and a folder");
            return ExitCode::from(2);
        }
        None => report(
            &kjk_game::discover(),
            &kjk_game::settings::chosen_installs(),
            |name| std::env::var(name).ok(),
            movie::ffmpeg::Ffmpeg::find().is_ok(),
        ),
    };
    if json {
        outln!("{value}");
    } else {
        print_plain(&value);
    }
    ExitCode::SUCCESS
}

/// The report's facts as one line each.
fn print_plain(value: &Value) {
    let text = |value: &Value| value.as_str().unwrap_or_default().to_string();
    for game in value["games"].as_array().into_iter().flatten() {
        let mut line = format!("{}: {}", text(&game["game"]), text(&game["status"]));
        if let Some(path) = game["path"].as_str() {
            line += &format!(" {path}");
        }
        if let Some(source) = game["source"].as_str() {
            line += &format!(" ({source})");
        }
        if let Some(why) = game["why"].as_str() {
            line += &format!(": {why}");
        }
        outln!("{line}");
    }
    if let Some(status) = value["ffmpeg"]["status"].as_str() {
        outln!("ffmpeg: {status}");
    }
}

