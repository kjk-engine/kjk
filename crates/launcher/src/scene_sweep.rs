//! `kjk dialogue sweep k1 <module>`: every conversation a module's headless
//! runs start, filmed, on one page for review (scene mode's sweep,
//! `docs/DIALOGUE.md`).
//!
//! - **The runs.** One from the module's entry, or one for each of its
//!   lines in `data/dialogue/k1.sweep` (the flags that set its story up),
//!   stepped as `kjk dialogue run` steps one (`dialogue_verbs::play`, with
//!   JA's animation table, as `kjk play` has it), its player reading each
//!   reply set `READ_MS` before taking the first. Each conversation that
//!   held the world (S1: a party one, or one the player stood in) and said
//!   something is a scene, from its first line shown.
//! - **The films.** Each scene is `kjk play` of the same run, headless, its
//!   `record` from the scene's first line until three lines after its
//!   first reply set (12 s at least, 60 at most; `scenes`): the player's
//!   `--moves` send the same commands the run's did (looking the arrival's
//!   way, each answer from the moment the run made it), so it plays the
//!   same conversations at the same times. Its transcript says whether it
//!   did.
//! - **The page.** `index.html` in the run folder: every film with its
//!   DLG, its first words, when it starts and ends and the run's flags;
//!   `facts.json` the same.

use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode, Stdio};
use std::sync::Arc;

use clap::Parser;
use serde_json::json;
use sim::script::dialogue::Line;
use vfs::{Discovery, Game};

use crate::dialogue_verbs::{Played, Run, play};
use crate::lifecycle::Installer;
use crate::transcript::text;

/// How long a scene is filmed (ms of game time): at least, at most, and
/// how many of its lines after its first reply set (`scenes`); and at how
/// many frames a second.
const LEAST_CLIP_MS: u64 = 12_000;
const MOST_CLIP_MS: u64 = 60_000;
const LINES_AFTER: u32 = 3;
const FPS: u32 = 12;
/// How long the player reads each reply set before answering (ms), so its
/// shot is filmed.
const READ_MS: u64 = 2000;

/// A line of `data/dialogue/<game>.sweep` after its module: the flags of
/// `kjk play` it sets the run up with, and how long the run is stepped.
#[derive(Parser)]
#[command(no_binary_name = true)]
pub(crate) struct Hint {
    #[arg(long)]
    arrive: Option<String>,
    #[arg(long = "global", value_parser = crate::by_hand::parse_global)]
    globals: Vec<(String, i32)>,
    #[arg(long)]
    talk: Option<String>,
    #[arg(long)]
    made: Option<String>,
    #[arg(long = "item", value_parser = crate::by_hand::parse_item)]
    items: Vec<(String, u16)>,
    #[arg(long = "skill")]
    skills: Vec<String>,
    #[arg(long = "party")]
    party: Vec<String>,
    #[arg(long, default_value_t = 60)]
    seconds: u64,
}

impl Hint {
    /// The run it asks of `dialogue_verbs::play`.
    pub(crate) fn run(&self) -> Result<Run, String> {
        Ok(Run {
            seconds: self.seconds,
            arrive: self.arrive.clone(),
            globals: self.globals.clone(),
            talk: self.talk.clone(),
            items: self.items.clone(),
            skills: self
                .skills
                .iter()
                .map(|skill| crate::by_hand::parse_skill(skill))
                .collect::<Result<_, _>>()?,
            made: self.made.as_deref().map(crate::parse_made).transpose()?,
            party: self.party.clone(),
            picks: Vec::new(),
            read_ms: READ_MS,
        })
    }

    /// Its flags as `kjk play` takes them.
    pub(crate) fn play_flags(&self) -> Vec<String> {
        let mut flags = Vec::new();
        let mut flag = |name: &str, value: String| flags.extend([format!("--{name}"), value]);
        for (name, value) in [
            ("arrive", &self.arrive),
            ("talk", &self.talk),
            ("made", &self.made),
        ] {
            if let Some(value) = value {
                flag(name, value.clone());
            }
        }
        for (name, value) in &self.globals {
            flag("global", format!("{name}={value}"));
        }
        for (template, count) in &self.items {
            flag("item", format!("{template}={count}"));
        }
        for skill in &self.skills {
            flag("skill", skill.clone());
        }
        for template in &self.party {
            flag("party", template.clone());
        }
        flags
    }
}

/// The runs `data/dialogue/<game>.sweep` gives `module` (`#` starts a
/// comment), or one plain run.
pub(crate) fn hints(game: &str, module: &str) -> Result<Vec<Hint>, String> {
    let path = Path::new("data/dialogue").join(format!("{game}.sweep"));
    let text = match std::fs::read_to_string(&path) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(error) => return Err(format!("{}: {error}", path.display())),
    };
    let mut hints = Vec::new();
    for line in text.lines() {
        let mut words = line.split('#').next().unwrap_or("").split_whitespace();
        if words.next() != Some(module) {
            continue;
        }
        let hint =
            Hint::try_parse_from(words).map_err(|error| format!("{}: {error}", path.display()))?;
        hints.push(hint);
    }
    if hints.is_empty() {
        hints.push(Hint::try_parse_from(std::iter::empty::<&str>()).map_err(|e| e.to_string())?);
    }
    Ok(hints)
}

/// A conversation a run started: its DLG, the entry it opened on, when it
/// starts and when its film ends (ms), and its first words, "who: what".
struct Scene {
    dialog: String,
    entry: u32,
    ms: u64,
    end_ms: u64,
    said: String,
}

/// The conversations `played` started that held the world and said
/// something, in the order they started, each filmed (`Scene::end_ms`)
/// until `LINES_AFTER` lines after its first reply set have played, else
/// until it ends: at least `LEAST_CLIP_MS`, at most `MOST_CLIP_MS`.
fn scenes(played: &Played, installer: &Installer) -> Vec<Scene> {
    let talk = |strref| installer.text(strref);
    let report = played.world.script_report();
    let mut started: Vec<(u32, Scene)> = Vec::new();
    for line in &report.talk {
        let Line::Entry {
            ms,
            id,
            dialog,
            entry,
            speaker,
            ..
        } = line
        else {
            continue;
        };
        if !played.holding.contains(id) {
            continue;
        }
        let at = match started.iter().position(|(seen, _)| seen == id) {
            Some(at) => at,
            None => {
                started.push((
                    *id,
                    Scene {
                        dialog: dialog.clone(),
                        entry: *entry,
                        ms: *ms,
                        end_ms: ms + MOST_CLIP_MS,
                        said: String::new(),
                    },
                ));
                started.len() - 1
            }
        };
        let scene = &mut started[at].1;
        let words = played
            .library
            .dialogue(dialog)
            .and_then(|dlg| dlg.entries.get(*entry as usize))
            .map_or_else(String::new, |node| text(&talk, node));
        if scene.said.is_empty() && !words.is_empty() {
            let who = played
                .names
                .get(&speaker.0)
                .cloned()
                .unwrap_or_else(|| format!("#{}", speaker.0));
            scene.said = format!("{who}: {words}");
        }
    }
    for (id, scene) in &mut started {
        let (mut offered, mut after) = (false, 0);
        let mut end = None;
        for line in &report.talk {
            match line {
                Line::Offered { id: of, .. } if of == id => offered = true,
                Line::Entry { id: of, ms, .. } if of == id && offered => {
                    after += 1;
                    if after > LINES_AFTER {
                        end = Some(*ms);
                        break;
                    }
                }
                Line::Ended { id: of, ms, .. } if of == id => {
                    end = Some(*ms);
                    break;
                }
                _ => {}
            }
        }
        scene.end_ms = end
            .unwrap_or(scene.end_ms)
            .clamp(scene.ms + LEAST_CLIP_MS, scene.ms + MOST_CLIP_MS);
    }
    started
        .into_iter()
        .map(|(_, scene)| scene)
        .filter(|scene| !scene.said.is_empty())
        .collect()
}

/// The `--moves` under which `kjk play`'s player sends what `played`'s
/// sent: looking the arrival's way, each answer from the ms it was made,
/// the last held to `end_ms`.
fn moves(played: &Played, end_ms: u64) -> String {
    let mut segments = Vec::new();
    let (mut from, mut reply) = (0, String::new());
    for &(ms, nth) in &played.answers {
        segments.push(format!("{}ms{reply}", ms - from));
        (from, reply) = (ms, format!(" reply {nth}"));
    }
    segments.push(format!("{}ms{reply}", end_ms.saturating_sub(from).max(1)));
    segments[0].push_str(&format!(" yaw {}", played.yaw));
    segments.join("; ")
}

/// What a film came to: its video in the sweep's folder, and when its own
/// run showed the scene's first line (none: not at all).
struct Film {
    video: Result<String, String>,
    shown_s: Option<f64>,
}

/// `scene` filmed by a headless `kjk play` of `module` set up by `hint`,
/// as `name`.mp4 in `folder`; the child's own run folder goes once its
/// video and transcript are read.
fn film(
    module: &str,
    hint: &Hint,
    played: &Played,
    scene: &Scene,
    name: &str,
    size: Option<(u32, u32)>,
    folder: &Path,
) -> Result<Film, String> {
    let exe = std::env::current_exe().map_err(|error| error.to_string())?;
    let from = scene.ms as f64 / 1000.0;
    let to = scene.end_ms as f64 / 1000.0;
    let log_path = folder.join(format!("{name}.log"));
    let log = std::fs::File::create(&log_path)
        .map_err(|error| format!("{}: {error}", log_path.display()))?;
    let mut command = Command::new(exe);
    command
        .args(["play", "--stage", module])
        .args(hint.play_flags())
        .args(["--moves", &moves(played, scene.end_ms + 1000)])
        .args(["--headless", "--cmds"])
        .arg(format!(
            "wait world; record {name} {from:.3} {to:.3} {FPS}; quit"
        ));
    if let Some((width, height)) = size {
        command.args(["--size", &format!("{width}x{height}")]);
    }
    let status = command
        .stdout(Stdio::null())
        .stderr(log)
        .status()
        .map_err(|error| error.to_string())?;
    let log = std::fs::read_to_string(&log_path).unwrap_or_else(|_| String::new());
    let child = crate::tour::run_folder(&log).ok_or(format!("{name}: no run folder ({status})"))?;
    let video = format!("{name}.mp4");
    let copied = std::fs::copy(child.join(&video), folder.join(&video))
        .map(|_| video)
        .map_err(|error| format!("{name}.mp4: {error} ({status})"));
    let shown_s = std::fs::read_to_string(child.join("transcript.txt"))
        .ok()
        .and_then(|transcript| shown_at(&transcript, scene));
    if child.starts_with(crate::runtime::runs()) {
        let _ = std::fs::remove_dir_all(&child);
    }
    if copied.is_ok() {
        let _ = std::fs::remove_file(&log_path);
    }
    Ok(Film {
        video: copied,
        shown_s,
    })
}

/// When a transcript (`transcript::transcript`'s lines) shows `scene`'s
/// first line, in seconds.
fn shown_at(transcript: &str, scene: &Scene) -> Option<f64> {
    let tag = format!("({} E{}", scene.dialog, scene.entry);
    transcript
        .lines()
        .filter(|line| line.contains(&format!("{tag})")) || line.contains(&format!("{tag},")))
        .find_map(|line| line.trim_start().split_once('s')?.0.parse().ok())
}

fn escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

/// The review page: a card a film.
fn page(module: &str, cards: &[serde_json::Value]) -> String {
    let mut body = String::new();
    for card in cards {
        let text = |key: &str| escape(card[key].as_str().unwrap_or(""));
        let note = match (card["video"].as_str(), card["agrees"].as_bool()) {
            (None, _) => format!("<p class=off>no film: {}</p>", text("error")),
            (Some(_), Some(false)) => format!(
                "<p class=off>the film's run showed this line at {}</p>",
                card["shown_s"]
                    .as_f64()
                    .map_or("no time".into(), |s| format!("{s:.2} s"))
            ),
            _ => String::new(),
        };
        let video = card["video"]
            .as_str()
            .map(|video| {
                format!(
                    "<video src=\"{}\" controls preload=metadata></video>",
                    escape(video)
                )
            })
            .unwrap_or_else(String::new);
        body.push_str(&format!(
            "<section><h2>{} E{}</h2><p class=said>{}</p><p class=meta>{:.2} to {:.2} s, run: {}</p>{note}{video}</section>\n",
            text("dialog"),
            card["entry"],
            text("said"),
            card["start_s"].as_f64().unwrap_or(0.0),
            card["end_s"].as_f64().unwrap_or(0.0),
            text("run"),
        ));
    }
    format!(
        "<!doctype html>\n<html><head><meta charset=utf-8><meta name=viewport content=\"width=device-width\">\
         <title>{module} conversations</title><style>\
         body{{font:15px system-ui,sans-serif;margin:16px;background:#111;color:#ddd}}\
         main{{display:grid;grid-template-columns:repeat(auto-fill,minmax(420px,1fr));gap:16px}}\
         section{{background:#1c1c1c;padding:12px;border-radius:6px}}\
         h2{{font-size:16px;margin:0 0 6px}} .said{{margin:0 0 4px}} .meta{{color:#999;margin:0 0 8px;font-size:13px}}\
         .off{{color:#f88}} video{{width:100%}}</style></head>\
         <body><h1>{module}: every conversation, from its first line</h1><main>\n{body}</main></body></html>\n"
    )
}

/// Films every conversation `module`'s runs start (see the module's doc)
/// at `size`, into one run folder with its page.
pub fn sweep(
    discovery: &Discovery,
    game: &str,
    module: &str,
    size: Option<(u32, u32)>,
) -> ExitCode {
    let go = || -> Result<(PathBuf, Vec<serde_json::Value>), String> {
        if Game::from_key(game) != Some(Game::Kotor1) {
            return Err(format!("{game}: k1 (kjk play stages K1's modules)"));
        }
        let hints = hints(game, module)?;
        let (anims, _) = crate::stage::anims(discovery);
        let installer = Installer::new(discovery, Game::Kotor1, Arc::new(anims))?;
        let folder = crate::runtime::create_run_folder()
            .map_err(|error| format!("the run folder: {error}"))?;
        eprintln!("run folder: {}", folder.display());
        let mut cards = Vec::new();
        for hint in &hints {
            let played = play(&installer, module, &hint.run()?)?;
            let flags = hint.play_flags().join(" ");
            for scene in scenes(&played, &installer) {
                let name = format!("{:02}-{}", cards.len() + 1, scene.dialog);
                eprintln!("{name}: from {:.2} s", scene.ms as f64 / 1000.0);
                let film = film(module, hint, &played, &scene, &name, size, &folder)?;
                let start_s = scene.ms as f64 / 1000.0;
                cards.push(json!({
                    "name": name,
                    "dialog": scene.dialog,
                    "entry": scene.entry,
                    "said": scene.said,
                    "start_s": start_s,
                    "end_s": scene.end_ms as f64 / 1000.0,
                    "run": if flags.is_empty() { "from the entry".to_string() } else { flags.clone() },
                    "video": film.video.as_ref().ok(),
                    "error": film.video.as_ref().err(),
                    "shown_s": film.shown_s,
                    "agrees": film.shown_s.is_some_and(|shown| (shown - start_s).abs() < 0.015),
                }));
            }
        }
        std::fs::write(folder.join("index.html"), page(module, &cards))
            .map_err(|error| format!("index.html: {error}"))?;
        Ok((folder, cards))
    };
    match go() {
        Ok((folder, cards)) => {
            let films = cards
                .iter()
                .filter(|card| card["video"].is_string())
                .count();
            let agree = cards.iter().filter(|card| card["agrees"] == true).count();
            println!(
                "{module}: {} conversations, {films} filmed, {agree} at the run's time; {}",
                cards.len(),
                folder.join("index.html").display()
            );
            crate::runtime::write_facts_in(&folder, json!({ "module": module, "scenes": cards }));
            if films == cards.len() {
                ExitCode::SUCCESS
            } else {
                ExitCode::FAILURE
            }
        }
        Err(error) => {
            eprintln!("{error}");
            ExitCode::FAILURE
        }
    }
}
