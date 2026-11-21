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
//! Z YAW PITCH`, `frame`, `list`, which writes the level's objects to
//! `objects.json`, `shot`, which writes a picture of the window to
//! `view.png`, or `ground X Y`, which answers how high the level's surfaces
//! are there. Only programs that can write to the user's
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
            // An object no tag names alone is named by its kind and place.
            if tag.is_empty() || tag.contains(' ') || !first {
                format!("edit select {} {index}", state::kind_name(kind))
            } else {
                format!("edit select {tag}")
            }
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
    /// The last answers: each command's number, its line and its answer.
    answers: VecDeque<(u64, String, String)>,
    /// How many commands the link has answered since it was turned on: the
    /// number of the last one.
    answered: u64,
    /// How deep the undo list was at `begin`, while a batch is open.
    batch: Option<usize>,
}

/// A line's words. A part between double quotes is one word, spaces and
/// all, so a value can have a space in it.
fn words_of(line: &str) -> Vec<String> {
    let mut words: Vec<String> = Vec::new();
    let mut word = String::new();
    let (mut quoted, mut any) = (false, false);
    for c in line.chars() {
        match c {
            '"' => {
                quoted = !quoted;
                any = true;
            }
            c if c.is_whitespace() && !quoted => {
                if any {
                    words.push(std::mem::take(&mut word));
                    any = false;
                }
            }
            c => {
                word.push(c);
                any = true;
            }
        }
    }
    if any {
        words.push(word);
    }
    words
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
            answered: 0,
            batch: None,
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
            let owned = words_of(line);
            let words: Vec<&str> = owned.iter().map(String::as_str).collect();
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
                // The edits from `begin` to `end` are one undo step.
                ["begin"] => {
                    self.batch = Some(state.editor.undo_depth());
                    "ok".to_string()
                }
                ["end"] => match self.batch.take() {
                    Some(depth) => {
                        let made = state.editor.join_undo_since(depth);
                        format!("ok: {made} edits are one undo step")
                    }
                    None => "no batch is open: begin starts one".to_string(),
                },
                ["catalog"] => match self.write_catalog(state) {
                    Ok(count) => format!("ok: {count} names listed in catalog.json"),
                    Err(why) => why,
                },
                ["level"] => match self.write_json("level.json", &state::facts_of(state)) {
                    Ok(()) => "ok: level.json is written".to_string(),
                    Err(why) => why,
                },
                // A picture of the window as the person sees it.
                ["shot"] => {
                    let path = self.folder.join("view.png");
                    // The old picture goes first, so the file is there
                    // again only once the new one is written.
                    let _ = std::fs::remove_file(&path);
                    state.pending_shot = Some(path);
                    "ok: view.png is written once the next frame is drawn".to_string()
                }
                ["ground", x, y] => match (state::number(x), state::number(y)) {
                    (Ok(x), Ok(y)) => match state::surfaces_at(state, x, y) {
                        Some(found) if found.is_empty() => {
                            format!("nothing of the level is at {x} {y}")
                        }
                        Some(found) => {
                            let heights: Vec<String> =
                                found.iter().map(|z| format!("{z:.2}")).collect();
                            format!("ok: {}", heights.join(" "))
                        }
                        None => "the level's collision does not build".to_string(),
                    },
                    _ => "ground takes a place of the level in metres: X Y".to_string(),
                },
                ["ground", ..] => "ground takes a place of the level in metres: X Y".to_string(),
                // The floor that can be walked on there, without the
                // furniture, walls and roofs `ground` also finds.
                ["floor", x, y] => match (state::number(x), state::number(y)) {
                    (Ok(x), Ok(y)) => {
                        let found = state::walkable_at(state, x, y, None);
                        if found.is_empty() {
                            format!("no walkable floor is at {x} {y}")
                        } else {
                            let heights: Vec<String> =
                                found.iter().map(|z| format!("{z:.2}")).collect();
                            format!("ok: {}", heights.join(" "))
                        }
                    }
                    _ => "floor takes a place of the level in metres: X Y".to_string(),
                },
                ["floor", ..] => "floor takes a place of the level in metres: X Y".to_string(),
                ["list"] => match self.write_objects(state) {
                    Ok(count) => format!("ok: {count} things listed in objects.json"),
                    Err(why) => why,
                },
                ["frame"] => {
                    let placed =
                        (state.editor.selected).and_then(|target| state.editor.placement(target));
                    if placed.is_some() {
                        crate::ui::frame_selection(state, camera_request, cameras);
                        "ok".to_string()
                    } else {
                        "nothing is selected".to_string()
                    }
                }
                // A click of the view, as `--cmds` takes it: picked once
                // this frame's panels are done.
                ["pick", x, y] => match (state::number(x), state::number(y)) {
                    (Ok(x), Ok(y)) => {
                        state.pending_pick = Some(Vec2::new(x, y));
                        state.pending_pick_adds = false;
                        "ok".to_string()
                    }
                    _ => "pick takes a point of the view in pixels: X Y".to_string(),
                },
                // The window's own state, as `--cmds` hands it over: the
                // panels take it on the next frame, and say so among the
                // messages when the words are none of theirs.
                ["ui", rest @ ..] if !rest.is_empty() => {
                    let words = rest.iter().map(|word| (*word).to_string()).collect();
                    state.pending_ui.push(words);
                    "ok".to_string()
                }
                words => match state::command(state, words, cameras) {
                    Ok(()) => "ok".to_string(),
                    Err(why) => why,
                },
            };
            state.say(format!("agent: {line}: {answer}"));
            self.answered += 1;
            self.answers
                .push_back((self.answered, line.to_string(), answer));
            while self.answers.len() > ANSWERS {
                self.answers.pop_front();
            }
        }
        // A batch left open ends with its file.
        if let Some(depth) = self.batch.take() {
            state.editor.join_undo_since(depth);
        }
    }

    /// Writes `value` as the file `name` of the link's folder, beside then
    /// over the old one.
    fn write_json(&self, name: &str, value: &serde_json::Value) -> Result<(), String> {
        let text = serde_json::to_string_pretty(value).map_err(|error| error.to_string())?;
        let path = self.folder.join(name);
        let fresh = self.folder.join(format!("{name}.new"));
        std::fs::write(&fresh, text)
            .and_then(|()| std::fs::rename(&fresh, &path))
            .map_err(|error| format!("{}: {error}", path.display()))
    }

    /// Writes `catalog.json`: every name a command can add or set, by what
    /// it is. How many names it lists.
    fn write_catalog(&self, state: &EditorState) -> Result<usize, String> {
        let setup = &state.setup;
        let rooms: Vec<serde_json::Value> = (setup.rooms.iter())
            .map(|room| serde_json::json!({"model": room.model, "area": room.area}))
            .collect();
        let lamps: Vec<serde_json::Value> = (state.lamp_library.found.iter())
            .map(|lamp| {
                let mesh = if lamp.mesh.is_empty() {
                    "-"
                } else {
                    &lamp.mesh
                };
                serde_json::json!({
                    "add_command": format!("edit add lamp {} {mesh}", lamp.model),
                    "texture": lamp.texture,
                })
            })
            .collect();
        let lists: [(&str, &Vec<String>); 12] = [
            ("placeables", &setup.placeables),
            ("creatures", &setup.creatures),
            ("doors", &setup.doors),
            ("waypoints", &setup.waypoints),
            ("triggers", &setup.triggers),
            ("sounds", &setup.sounds),
            ("stores", &setup.stores),
            ("encounters", &setup.encounters),
            ("items", &setup.items),
            ("scripts", &setup.scripts),
            ("modules", &setup.modules),
            ("props", &setup.models),
        ];
        let count =
            lists.iter().map(|(_, names)| names.len()).sum::<usize>() + rooms.len() + lamps.len();
        let mut catalog = serde_json::Map::new();
        for (name, names) in lists {
            catalog.insert(name.to_string(), serde_json::json!(names));
        }
        catalog.insert("rooms".into(), serde_json::json!(rooms));
        catalog.insert("lamps".into(), serde_json::json!(lamps));
        catalog.insert(
            "lamps_note".into(),
            serde_json::json!(
                "The lamps found so far. Send `lamps list` first to look through the whole install."
            ),
        );
        self.write_json("catalog.json", &serde_json::Value::Object(catalog))?;
        Ok(count)
    }

    /// Writes `objects.json`: everything the level holds that a command
    /// can select, each with the command that selects it. How many it
    /// lists.
    fn write_objects(&self, state: &EditorState) -> Result<usize, String> {
        let mut listed: Vec<serde_json::Value> = Vec::new();
        let mut list = |target: Target, name: String| {
            let placed = state.editor.placement(target);
            listed.push(serde_json::json!({
                "target": format!("{target:?}"),
                "name": name,
                "position": placed.map(|(at, _)| at),
                "facing": placed.map(|(_, facing)| facing),
                "select_command": select_command(state, target),
            }));
        };
        for entry in state.editor.entries() {
            let tag = state.tag_of(&entry);
            list(
                Target::Object(entry.kind, entry.index),
                format!("{tag} ({})", entry.template),
            );
        }
        let project = &state.editor.project;
        for (at, prop) in project.kjk.props.iter().enumerate() {
            list(Target::Prop(at), format!("{} ({})", prop.name, prop.model));
        }
        for (at, shape) in project.kjk.boxes.iter().enumerate() {
            list(Target::Box(at), shape.name.clone());
        }
        for (at, lamp) in project.kjk.lamps.iter().enumerate() {
            list(Target::Lamp(at), lamp.label());
        }
        for (at, light) in project.kjk.lights.iter().enumerate() {
            list(Target::Light(at), light.reason.clone());
        }
        if let Some(map) = &project.map {
            for (at, room) in map.rooms.iter().enumerate() {
                list(Target::Room(at), room.model.clone());
            }
            list(Target::Entry, "entry".to_string());
        }
        let count = listed.len();
        self.write_json("objects.json", &serde_json::Value::Array(listed))?;
        Ok(count)
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
            "selection": state
                .editor
                .selection()
                .iter()
                .map(|target| format!("{target:?}"))
                .collect::<Vec<_>>(),
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
            "recovery_found": state.recovery.is_some(),
            "markers_hidden": state
                .hidden
                .iter()
                .map(|kind| state::kind_name(*kind))
                .collect::<Vec<_>>(),
            "auditing": state.audit_job.is_some(),
            "audit": state.audit,
            "baking": state.bake_job.as_ref().map(|job| serde_json::json!({
                "label": job.label,
                "status": job.status,
            })),
            "bake_shown": state.bake.as_ref().map(|bake| bake.display().to_string()),
            "light_view": format!("{:?}", state.light_view),
            "answered": self.answered,
            "answers": self
                .answers
                .iter()
                .map(|(n, command, answer)| serde_json::json!({"n": n, "command": command, "answer": answer}))
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
