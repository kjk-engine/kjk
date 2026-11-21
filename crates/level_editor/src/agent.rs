//! Working with an agent (Claude, or any program on this machine): the
//! selection copied as text, and the agent link.
//!
//! The link is off until the toolbar's Agent link (or `KJK_EDITOR_LINK=1`)
//! turns it on. While it is on, the editor keeps `live.json` in
//! `<kjk's folder>/editor-link/` up to date: the project, the level, the
//! selection as the details panel describes it, the camera, the last
//! messages, and the answers to the agent's commands. And it runs each line
//! an agent writes to `inbox.txt` there as a command, as `--cmds` runs
//! them: an `edit` command (`docs/LEVEL-EDIT.md`, "Commands"), `camera X Y
//! Z YAW PITCH`, or `frame`. Only programs that can write to the user's
//! own folder can reach it; nothing listens on the network.

use std::collections::VecDeque;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use bevy::prelude::*;
use kjk_game::scene::{FlyCamera, game_to_bevy};
use level_edit::Target;

use crate::state::{self, EditorState};

/// How often the link writes the state and reads the inbox.
const EVERY: Duration = Duration::from_millis(250);

/// How many answers to commands `live.json` keeps.
const ANSWERS: usize = 20;

/// The guide for agents (`docs/LEVEL-EDIT-AGENT.md`), which the link
/// writes into its folder.
const GUIDE: &str = include_str!("../../../docs/LEVEL-EDIT-AGENT.md");

/// `docs/LEVEL-EDIT.md`, whose list of commands follows the guide.
const EDITOR_DOC: &str = include_str!("../../../docs/LEVEL-EDIT.md");

/// The README the link writes: the guide, then the editor's commands as
/// `docs/LEVEL-EDIT.md` lists them (its "Commands" section), both as this
/// build of the editor was made with them.
fn readme() -> String {
    let commands = EDITOR_DOC
        .split_once("### Commands\n")
        .map(|(_, rest)| rest.split("\n## ").next().unwrap_or(rest))
        .unwrap_or("See docs/LEVEL-EDIT.md, \"Commands\".");
    format!("{GUIDE}\n## The editor's commands\n{commands}")
}

/// The selected object, or the level when nothing is selected, as text to
/// paste to an agent: what it is, where it is, the command that selects it
/// again, and everything the details panel knows of it.
pub fn selection_text(
    state: &EditorState,
    cameras: &Query<(&Camera, &GlobalTransform), With<FlyCamera>>,
) -> String {
    let project = &state.editor.project;
    let mut out = format!(
        "kjk level editor: project {}, {} module {} (area {}){}.\n",
        project.name,
        project.game,
        project.module,
        project.area,
        if state.unsaved() { ", not saved" } else { "" }
    );
    match state.editor.selected {
        Some(target) => {
            let facts = state::describe(state, target);
            if let Some(again) = select_command(state, target) {
                out.push_str(&format!(
                    "Selected: {target:?}. To select it again: {again}\n"
                ));
            } else {
                out.push_str(&format!("Selected: {target:?}.\n"));
            }
            out.push_str(
                &serde_json::to_string_pretty(&facts).unwrap_or_else(|_| facts.to_string()),
            );
            out.push('\n');
        }
        None => out.push_str("Nothing is selected.\n"),
    }
    if let Some((at, yaw, pitch)) = camera_pose(cameras) {
        out.push_str(&format!(
            "Camera: camera {:.2} {:.2} {:.2} {yaw:.1} {pitch:.1}\n",
            at[0], at[1], at[2]
        ));
    }
    out
}

/// The command that selects `target` again, where there is one.
fn select_command(state: &EditorState, target: Target) -> Option<String> {
    Some(match target {
        Target::Sun => "select sun".to_string(),
        Target::Object(kind, index) => {
            let entry = state.editor.entry(kind, index)?;
            let tag = state.tag_of(&entry);
            // A tag that names more than one object selects the first.
            let first = state
                .editor
                .entries()
                .into_iter()
                .find(|other| state.tag_of(other).eq_ignore_ascii_case(&tag))
                .is_some_and(|other| other.kind == kind && other.index == index);
            if tag.is_empty() || tag.contains(' ') || !first {
                return None;
            }
            format!("edit select {tag}")
        }
        Target::Prop(n) => format!("edit select prop {n}"),
        Target::Box(n) => format!("edit select box {n}"),
        Target::Room(n) => format!("edit select room {n}"),
        Target::Entry => "edit select entry".to_string(),
        Target::Light(n) => format!("edit select light {n}"),
        Target::Fitted(n) => format!("edit select fitted {n}"),
        Target::Lamp(n) => format!("edit select lamp {n}"),
    })
}

/// Where the camera is (game space), and its yaw and pitch in degrees.
fn camera_pose(
    cameras: &Query<(&Camera, &GlobalTransform), With<FlyCamera>>,
) -> Option<([f32; 3], f32, f32)> {
    let (_, eye) = cameras.iter().next()?;
    let to_game = game_to_bevy().inverse();
    let at = to_game * eye.translation();
    let look = (to_game * eye.forward().as_vec3()).normalize_or_zero();
    Some((
        at.to_array(),
        look.y.atan2(look.x).to_degrees(),
        look.z.clamp(-1.0, 1.0).asin().to_degrees(),
    ))
}

/// The agent link, while it is on.
pub struct Link {
    pub folder: PathBuf,
    written: String,
    last: Instant,
    answers: VecDeque<(String, String)>,
}

impl Link {
    /// The link's folder: `editor-link` in kjk's own folder.
    pub fn folder() -> Option<PathBuf> {
        store::kjk_folder(|var| std::env::var(var).ok()).map(|folder| folder.join("editor-link"))
    }

    /// Turns the link on: its folder made, with the README for agents.
    pub fn open() -> Result<Link, String> {
        let folder = Link::folder().ok_or("kjk's folder is not known (set KJK_HOME)")?;
        std::fs::create_dir_all(&folder)
            .map_err(|error| format!("{}: {error}", folder.display()))?;
        std::fs::write(folder.join("README.md"), readme())
            .map_err(|error| format!("{}: {error}", folder.display()))?;
        Ok(Link {
            folder,
            written: String::new(),
            last: Instant::now() - EVERY,
            answers: VecDeque::new(),
        })
    }

    /// Runs the inbox's commands and writes the state, at most every
    /// `EVERY`.
    pub fn tick(
        &mut self,
        state: &mut EditorState,
        cameras: &Query<(&Camera, &GlobalTransform), With<FlyCamera>>,
        camera_request: &mut console::CameraRequest,
    ) {
        if self.last.elapsed() < EVERY {
            return;
        }
        self.last = Instant::now();
        self.run_inbox(state, cameras, camera_request);
        self.write_state(state, cameras);
    }

    fn run_inbox(
        &mut self,
        state: &mut EditorState,
        cameras: &Query<(&Camera, &GlobalTransform), With<FlyCamera>>,
        camera_request: &mut console::CameraRequest,
    ) {
        let inbox = self.folder.join("inbox.txt");
        // Taken first, so lines the agent writes meanwhile wait for the
        // next turn.
        let taken = self.folder.join("inbox.taken");
        if std::fs::rename(&inbox, &taken).is_err() {
            return;
        }
        let text = std::fs::read_to_string(&taken).unwrap_or_else(|_| String::new());
        let _ = std::fs::remove_file(&taken);
        for line in text.lines().map(str::trim).filter(|line| !line.is_empty()) {
            let words: Vec<&str> = line.split_whitespace().collect();
            let words = match words.as_slice() {
                ["edit", rest @ ..] => rest.to_vec(),
                _ => words,
            };
            let answer = match words.as_slice() {
                ["camera", numbers @ ..] if numbers.len() == 5 => {
                    let parsed: Vec<f32> = numbers.iter().filter_map(|n| n.parse().ok()).collect();
                    if parsed.len() == 5 && parsed.iter().all(|n| n.is_finite()) {
                        camera_request.0 = Some(console::CameraPose {
                            position: [parsed[0], parsed[1], parsed[2]],
                            yaw: parsed[3],
                            pitch: parsed[4],
                            fov: None,
                        });
                        "ok".to_string()
                    } else {
                        "camera takes five numbers: X Y Z YAW PITCH".to_string()
                    }
                }
                ["frame"] => {
                    crate::ui::frame_selection(state, camera_request, cameras);
                    "ok".to_string()
                }
                words => match state::command(state, words, cameras) {
                    Ok(()) => "ok".to_string(),
                    Err(why) => why,
                },
            };
            state.say(format!("agent: {line}: {answer}"));
            self.answers.push_back((line.to_string(), answer));
            while self.answers.len() > ANSWERS {
                self.answers.pop_front();
            }
        }
    }

    fn write_state(
        &mut self,
        state: &EditorState,
        cameras: &Query<(&Camera, &GlobalTransform), With<FlyCamera>>,
    ) {
        let project = &state.editor.project;
        let selected = state.editor.selected;
        let camera = camera_pose(cameras).map(|(at, yaw, pitch)| {
            serde_json::json!({
                "at": at,
                "yaw": yaw,
                "pitch": pitch,
                "command": format!("camera {:.2} {:.2} {:.2} {yaw:.1} {pitch:.1}", at[0], at[1], at[2]),
            })
        });
        let live = serde_json::json!({
            "project": project.name,
            "folder": project.folder,
            "game": project.game,
            "module": project.module,
            "area": project.area,
            "new_map": project.map.is_some(),
            "unsaved": state.unsaved(),
            "selected": selected.map(|target| format!("{target:?}")),
            "select_command": selected.and_then(|target| select_command(state, target)),
            "details": selected.map(|target| state::describe(state, target)),
            "camera": camera,
            "messages": state.messages,
            "script_errors": state.script_errors,
            "last_try": state.tried.as_ref().map(|tried| serde_json::json!({
                "script": tried.script,
                "on": tried.on,
                "ok": tried.ok,
                "lines": tried.lines.iter().take(200).collect::<Vec<_>>(),
                "debug_stops": tried.stops.len(),
            })),
            "trying": state.script_try.as_ref().map(|run| run.script.clone()),
            "answers": self
                .answers
                .iter()
                .map(|(command, answer)| serde_json::json!({"command": command, "answer": answer}))
                .collect::<Vec<_>>(),
            "guide": "README.md in this folder (docs/LEVEL-EDIT-AGENT.md)",
        });
        let text = serde_json::to_string_pretty(&live).unwrap_or_else(|_| live.to_string());
        if text == self.written {
            return;
        }
        let path = self.folder.join("live.json");
        let fresh = self.folder.join("live.json.new");
        if std::fs::write(&fresh, &text)
            .and_then(|()| std::fs::rename(&fresh, &path))
            .is_ok()
        {
            self.written = text;
        }
    }
}
