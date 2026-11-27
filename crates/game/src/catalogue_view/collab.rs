//! What the catalogue offers an agent and its owner beyond looking (docs/UI.md,
//! "Working with the catalogue"): the bridge an agent drives it through, a
//! tryout saved as a story, hot reload, outlines and backdrops. The
//! catalogue calls the functions here at a few places of its frame and from
//! its keys, so that its own file stays small.

mod backdrop;
mod bridge;
mod outline;
mod record;
mod reload;

use std::borrow::Cow;

use bevy::prelude::*;
use serde_json::{Value, json};
use ui::catalogue::{SIZES, Story};
use ui::draw::DrawList;
use ui::input::UiInput;
use ui::virtual_screen::Mode;

use super::{Catalogue, Start};
pub use backdrop::{Backdrop, Shown, show};
use bridge::Bridge;
use record::Recorder;
use reload::Watch;

/// The soonest the control tree is built again while the screens change, in
/// seconds. It draws the screens a second time to read the colours behind
/// the words.
const TREE_EVERY: f32 = 0.4;

/// What the catalogue showed when the log last looked, to log what changed.
#[derive(Debug, Clone, PartialEq, Default)]
struct Seen {
    story: String,
    size: String,
    mode: String,
    outlines: bool,
    backdrop: String,
    still: String,
}

pub struct Collab {
    bridge: Option<Bridge>,
    bridge_failed: bool,
    recorder: Recorder,
    watch: Watch,
    outlines: bool,
    backdrop: Backdrop,
    stills: backdrop::Stills,
    /// The still the play backdrop shows now: its key, and what it is
    /// (`making`, `ready` or the failure).
    still: Option<(String, String)>,
    /// Where the change this frame comes from, for the log: `keyboard`,
    /// `mouse`, `pad`, `console`, `agent` or `disk`. It is empty when
    /// nothing was asked this frame.
    pub source: &'static str,
    seen: Seen,
    /// The control tree of the frame it was built for, as that frame's
    /// controls were: their tags, places, focus and words.
    tree: Option<(Vec<String>, Value)>,
    tree_age: f32,
    /// What the state file holds besides its revision and its tree, and the
    /// version of the tree it holds.
    written: Option<(Value, u64)>,
    tree_version: u64,
    revision: u64,
    /// The stories this tryout has been saved as.
    saved: Vec<String>,
    /// The focus before the frame's input.
    focus: Option<String>,
}

impl Collab {
    pub fn new(start: &Start) -> Collab {
        Collab {
            bridge: None,
            bridge_failed: false,
            recorder: Recorder::default(),
            watch: Watch::of(&start.module),
            outlines: false,
            backdrop: Backdrop::Plain,
            stills: backdrop::Stills::default(),
            still: None,
            source: "",
            seen: Seen::default(),
            tree: None,
            tree_age: TREE_EVERY,
            written: None,
            tree_version: 0,
            revision: 0,
            saved: Vec::new(),
            focus: None,
        }
    }

    /// The story opened again: what was recorded of the last tryout goes.
    pub fn restaged(&mut self) {
        self.recorder.clear();
        self.saved.clear();
    }
}

/// What the catalogue's keys for the tools of this module do.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tool {
    Outlines,
    Backdrop,
    SaveTryout,
}

/// The catalogue's key for `tool` was pressed.
pub fn act(cat: &mut Catalogue, tool: Tool) {
    match tool {
        Tool::Outlines => cat.collab.outlines = !cat.collab.outlines,
        Tool::Backdrop => cat.collab.backdrop = cat.collab.backdrop.next(),
        Tool::SaveTryout => {
            let source = cat.collab.source;
            if let Err(why) = save(cat, None) {
                warn!(target: "catalogue", "tryout not saved: {why}");
                log(cat, source, "save", json!({ "error": why }));
            }
        }
    }
}

fn log(cat: &mut Catalogue, source: &str, change: &str, detail: Value) {
    if let Some(bridge) = cat.collab.bridge.as_mut() {
        bridge.log(source, change, detail);
    }
}

/// Where the input of this frame came from.
pub fn source_of(keyboard: bool, mouse: bool, pad: bool, scripted: bool) -> &'static str {
    match (scripted, keyboard, mouse, pad) {
        (true, ..) => "console",
        (_, true, ..) => "keyboard",
        (_, _, true, _) => "mouse",
        (_, _, _, true) => "pad",
        _ => "keyboard",
    }
}

/// The start of a frame: the bridge opened on the first one, the files
/// that changed on disk read again, and the agent's commands taken.
pub fn begin(cat: &mut Catalogue, run: Option<&std::path::Path>, dt: f32) {
    if cat.collab.seen.story.is_empty() {
        log_changes(cat);
    }
    if cat.collab.bridge.is_none() && !cat.collab.bridge_failed {
        let folder = run.map_or_else(
            || (crate::runtime::artifacts().join("catalogue")).join(std::process::id().to_string()),
            |run| run.join("catalogue"),
        );
        let pointers = crate::runtime::artifacts().join("catalogue-bridges");
        match Bridge::open(&folder, &pointers) {
            Ok(bridge) => {
                outln!("catalogue state file: {}", bridge.state.display());
                outln!("catalogue command file: {}", bridge.commands.display());
                outln!("catalogue log: {}", bridge.log.display());
                cat.collab.bridge = Some(bridge);
            }
            Err(why) => {
                warn!(target: "catalogue", "no bridge: {why}");
                cat.collab.bridge_failed = true;
            }
        }
    }
    let changed = cat.collab.watch.changed(dt);
    if !changed.is_empty() {
        cat.collab.source = "disk";
        reload(cat, &changed);
    }
    let Some(bridge) = cat.collab.bridge.as_mut() else {
        return;
    };
    let first = bridge.read;
    for (at, line) in bridge.new_commands(dt).into_iter().enumerate() {
        cat.collab.source = "agent";
        let result = command(cat, &line);
        log_changes(cat);
        let last = json!({
            "n": first + at as u64 + 1,
            "line": line,
            "ok": result.is_ok(),
            "error": result.as_ref().err(),
        });
        if let Some(bridge) = cat.collab.bridge.as_mut() {
            if let Err(why) = &result {
                bridge.log("agent", "refused", json!({ "line": line, "error": why }));
            }
            bridge.last = last;
        }
    }
}

/// One line of the command file.
fn command(cat: &mut Catalogue, line: &str) -> Result<(), String> {
    let (verb, rest) = line
        .split_once(' ')
        .map_or((line, ""), |(verb, rest)| (verb, rest.trim()));
    match verb {
        "open" => open(cat, rest),
        "size" => {
            let at = SIZES.iter().position(|size| size.name == rest);
            at.map(|at| cat.set_size(at))
                .ok_or(format!("{rest}: not one of the six sizes"))
        }
        "mode" => Mode::named(rest)
            .map(|mode| cat.set_mode(mode))
            .ok_or(format!("{rest}: not fit, pixel or seen")),
        "step" => step(cat, rest, "agent"),
        "outlines" => {
            switch(rest).map(|on| cat.collab.outlines = on.unwrap_or(!cat.collab.outlines))
        }
        "backdrop" => match rest {
            "" | "next" => {
                cat.collab.backdrop = cat.collab.backdrop.next();
                Ok(())
            }
            name => Backdrop::named(name)
                .map(|backdrop| cat.collab.backdrop = backdrop)
                .ok_or(format!("{name}: not plain, stripes or play")),
        },
        "save" => save(cat, (!rest.is_empty()).then_some(rest)),
        other => Err(format!(
            "{other}: `open MODULE/STORY`, `size NAME`, `mode fit|pixel|seen`, `step STEP`, `outlines on|off`, `backdrop plain|stripes|play` or `save [NAME]`"
        )),
    }
}

fn switch(word: &str) -> Result<Option<bool>, String> {
    match word {
        "" => Ok(None),
        "on" => Ok(Some(true)),
        "off" => Ok(Some(false)),
        other => Err(format!("{other}: on or off")),
    }
}

/// The story `module/name` opened, on the game it is on now when it is
/// drawn on it.
fn open(cat: &mut Catalogue, wanted: &str) -> Result<(), String> {
    let (module, name) = wanted
        .split_once('/')
        .ok_or(format!("{wanted}: name a story as MODULE/STORY"))?;
    let files = ui::catalogue::read_all(&ui::catalogue::folder())?;
    let file = files
        .iter()
        .find(|file| file.module == module)
        .ok_or(format!("{module}: no story file"))?;
    let story = file
        .stories
        .iter()
        .find(|story| story.name == name || ui::catalogue::slug(&story.name) == name)
        .ok_or(format!("{module}: no story {name:?}"))?;
    let key = cat.start.game.key();
    if !story.games.iter().any(|game| game == key) {
        return Err(format!("{name}: not drawn on {key}"));
    }
    let start = Start {
        module: file.module.clone(),
        story: story.clone(),
        ..cat.start.clone()
    };
    cat.staged = Catalogue::stage(&cat.discovery, &start, cat.size())?;
    cat.start = start;
    cat.reopen_failed = false;
    cat.collab.watch.take(&cat.start.module);
    cat.collab.restaged();
    Ok(())
}

/// `step` taken on the staged screen, as `source` asked.
fn step(cat: &mut Catalogue, step: &str, source: &str) -> Result<(), String> {
    cat.staged.step(&cat.discovery, step)?;
    cat.collab.recorder.steps.push(step.to_string());
    log(cat, source, "step", json!(step));
    Ok(())
}

/// The open story read from disk and staged again, the steps of the tryout
/// taken once more.
fn reload(cat: &mut Catalogue, changed: &[String]) {
    let wanted = format!("{}/{}", cat.start.module, cat.start.story.name);
    let story: Result<Story, String> =
        ui::catalogue::read_all(&ui::catalogue::folder()).and_then(|files| {
            files
                .into_iter()
                .find(|file| file.module == cat.start.module)
                .and_then(|file| {
                    file.stories
                        .into_iter()
                        .find(|story| story.name == cat.start.story.name)
                })
                .ok_or(format!("{wanted}: the story is not in its file any more"))
        });
    let result = story.and_then(|story| {
        let start = Start {
            story,
            ..cat.start.clone()
        };
        let mut staged = Catalogue::stage(&cat.discovery, &start, cat.size())?;
        for step in &cat.collab.recorder.steps {
            staged.step(&cat.discovery, step)?;
        }
        Ok((start, staged))
    });
    match result {
        Ok((start, staged)) => {
            cat.start = start;
            cat.staged = staged;
            cat.reopen_failed = false;
            log(cat, "disk", "reload", json!({ "changed": changed }));
            info!(target: "catalogue", ?changed, "reloaded");
        }
        Err(why) => {
            warn!(target: "catalogue", "not reloaded: {why}");
            log(
                cat,
                "disk",
                "reload refused",
                json!({ "changed": changed, "error": why }),
            );
        }
    }
}

/// The tryout saved as a story at the end of its module's story file. It
/// starts as the open story does and takes the recorded steps after its own,
/// and its replay is checked before it is written.
fn save(cat: &mut Catalogue, name: Option<&str>) -> Result<(), String> {
    let source = cat.collab.source;
    let path = ui::catalogue::folder().join(format!("{}.toml", cat.start.module));
    let taken: Vec<String> = ui::catalogue::read_all(&ui::catalogue::folder())?
        .into_iter()
        .filter(|file| file.module == cat.start.module)
        .flat_map(|file| file.stories)
        .map(|story| story.name)
        .collect();
    let name = record::free_name(
        &name.map_or_else(record::tryout_name, str::to_string),
        &taken,
    );
    let steps: Vec<String> = (cat.start.story.steps.iter())
        .chain(&cat.collab.recorder.steps)
        .cloned()
        .collect();
    let replay = record::replays(cat, &steps);
    replay.as_ref().map_err(|why| format!("not saved, {why}"))?;
    record::append(
        &path,
        &record::story_text(&name, &cat.start.story, &cat.collab.recorder.steps),
    )?;
    cat.collab.watch.take(&cat.start.module);
    cat.collab.saved.push(name.clone());
    let detail = json!({
        "story": format!("{}/{name}", cat.start.module),
        "file": path,
        "steps": steps,
        "replays": true,
    });
    log(cat, source, "save", detail);
    info!(target: "catalogue", story = name, "tryout saved");
    Ok(())
}

/// The recorder before the screens take `input`, which came from `source`.
pub fn before(cat: &mut Catalogue, input: &UiInput, source: &'static str) {
    cat.collab.focus = cat.staged.stack.focused().map(str::to_string);
    let wanted = !input.keys.is_empty()
        || !input.text.is_empty()
        || !input.raw.is_empty()
        || input.click
        || input.wheel != 0.0;
    if !wanted {
        return;
    }
    let links = cat.staged.stack.focus_links(&cat.staged.vm);
    let frame = record::Frame {
        layers: &cat.layers,
        links: &links,
        focused: cat.collab.focus.as_deref(),
        window: cat.size().window(),
        depth: cat.staged.stack.screens().count().saturating_sub(1),
    };
    let made = cat.collab.recorder.before(input, &frame);
    for step in made {
        log(cat, source, "step", json!(step));
    }
}

/// The end of a frame: what changed is logged, and the state file written
/// when something it holds changed.
pub fn end(cat: &mut Catalogue, input: &UiInput, dt: f32) {
    still_tick(cat);
    let now = cat.staged.stack.focused().map(str::to_string);
    cat.collab
        .recorder
        .after(input, cat.collab.focus.as_deref(), now.as_deref());
    {
        let stack = &cat.staged.stack;
        let row = |tag: &str, at: usize| stack.list_at_in(at, tag).map(|(_, row)| row);
        if let Some(step) = cat.collab.recorder.row_step(row) {
            let source = cat.collab.source;
            log(
                cat,
                if source.is_empty() { "mouse" } else { source },
                "step",
                json!(step),
            );
        }
    }
    cat.collab.tree_age += dt;
    log_changes(cat);
    cat.collab.source = "";
    publish(cat);
}

/// What the catalogue shows differently from the last look is logged, with
/// the source that asked for it, and taken as the new look. The first look
/// logs nothing.
fn log_changes(cat: &mut Catalogue) {
    let seen = Seen {
        story: format!("{}/{}", cat.start.module, cat.start.story.name),
        size: cat.size().name.to_string(),
        mode: cat.mode.name().to_string(),
        outlines: cat.collab.outlines,
        backdrop: cat.collab.backdrop.name().to_string(),
        still: cat
            .collab
            .still
            .as_ref()
            .map_or_else(String::new, |(_, state)| state.clone()),
    };
    if seen == cat.collab.seen {
        return;
    }
    let source = if cat.collab.source.is_empty() {
        "keyboard"
    } else {
        cat.collab.source
    };
    let before = std::mem::replace(&mut cat.collab.seen, seen.clone());
    if before.story.is_empty() {
        return;
    }
    let changes = [
        ("story", &before.story, &seen.story),
        ("size", &before.size, &seen.size),
        ("mode", &before.mode, &seen.mode),
        ("backdrop", &before.backdrop, &seen.backdrop),
        ("backdrop still", &before.still, &seen.still),
    ];
    for (change, was, is) in changes {
        if was != is {
            log(cat, source, change, json!({ "from": was, "to": is }));
        }
    }
    if before.outlines != seen.outlines {
        log(cat, source, "outlines", json!({ "on": seen.outlines }));
    }
}

/// The control tree of the frame is built when the frame's controls changed,
/// and no oftener than `TREE_EVERY`. Whether the tree is of this frame.
fn tree(cat: &mut Catalogue) -> bool {
    let mut signature = record::signature(&cat.layers);
    signature.push(format!("{} {}", cat.size().name, cat.layers.len()));
    let fresh = cat
        .collab
        .tree
        .as_ref()
        .is_some_and(|(seen, _)| *seen == signature);
    if !fresh && (cat.collab.tree.is_none() || cat.collab.tree_age >= TREE_EVERY) {
        let size = cat.size();
        let layers: Vec<crate::gui_tree::Layer<'_>> = (cat.names.iter().zip(&cat.layers))
            .map(|((screen, layout), list)| crate::gui_tree::Layer {
                screen,
                layout,
                list,
            })
            .collect();
        let shown = json!({
            "game": cat.start.game.key(),
            "screen": cat.start.story.screen,
            "story": format!("{}/{}", cat.start.module, cat.start.story.name),
            "steps": cat.start.story.steps.iter().chain(&cat.collab.recorder.steps).collect::<Vec<_>>(),
            "scale_factor": size.factor,
        });
        let links = cat.staged.stack.focus_links(&cat.staged.vm);
        let tree = crate::gui_tree::tree(
            &cat.staged.assets,
            shown,
            &layers,
            size.pixels,
            (&links, cat.staged.stack.focused()),
            Value::Null,
        );
        cat.collab.tree = Some((signature, tree));
        cat.collab.tree_age = 0.0;
        cat.collab.tree_version += 1;
        return true;
    }
    fresh
}

/// The state file, rewritten when anything but the revision differs from
/// what it holds.
fn publish(cat: &mut Catalogue) {
    if cat.collab.bridge.is_none() {
        return;
    }
    let current = tree(cat);
    let size = cat.size();
    let Some(bridge) = cat.collab.bridge.as_ref() else {
        return;
    };
    let core = json!({
        "format": 1,
        "story": format!("{}/{}", cat.start.module, cat.start.story.name),
        "game": cat.start.game.key(),
        "size": size.name,
        "device": [size.pixels.0, size.pixels.1],
        "factor": size.factor,
        "mode": cat.mode.name(),
        "focus": cat.staged.stack.focused(),
        "screens": cat.staged.stack.screens().map(|screen| screen.0).collect::<Vec<_>>(),
        "outlines": cat.collab.outlines,
        "backdrop": cat.collab.backdrop.name(),
        "backdrop_still": cat.collab.still.as_ref().map(|(_, state)| state),
        "recording": {
            "steps": cat.collab.recorder.steps,
            "saved": cat.collab.saved,
        },
        "commands_read": bridge.read,
        "last_command": bridge.last,
        "files": { "commands": bridge.commands, "log": bridge.log },
        "tree_current": current,
    });
    let version = cat.collab.tree_version;
    if let Some((written, built)) = &cat.collab.written
        && *written == core
        && *built == version
    {
        return;
    }
    cat.collab.revision += 1;
    let mut state = core.clone();
    state["revision"] = cat.collab.revision.into();
    state["tree"] = cat
        .collab
        .tree
        .as_ref()
        .map_or(Value::Null, |(_, tree)| tree.clone());
    bridge.write_state(&state);
    cat.collab.written = Some((core, version));
}

/// What the screens' image is drawn from: the screens, with the backdrop
/// under them and the outlines over them when those are on.
pub fn composed(cat: &Catalogue) -> Cow<'_, [DrawList]> {
    let collab = &cat.collab;
    if collab.backdrop == Backdrop::Plain && !collab.outlines {
        return Cow::Borrowed(&cat.layers);
    }
    let size = cat.size();
    let window = size.window();
    let mut lists = Vec::new();
    lists.extend(collab.backdrop.list(window));
    lists.extend(cat.layers.iter().cloned());
    if collab.outlines {
        let pixel = 1.0 / (cat.placement.scale * size.factor).max(0.01);
        lists.push(outline::list(&cat.layers, window, pixel));
    }
    Cow::Owned(lists)
}

/// The still of the game world that the play backdrop needs, asked for
/// and looked at once a frame. It is made in the background, so the
/// screens show the plain backdrop until it is ready.
fn still_tick(cat: &mut Catalogue) {
    if cat.collab.backdrop != Backdrop::Play {
        cat.collab.still = None;
        return;
    }
    let game = cat.start.game;
    let capture = cat
        .start
        .story
        .capture_for(game.key(), ui::catalogue::Input::Mouse)
        .flatten();
    let stage = backdrop::stage_for(game, capture);
    let cache = crate::runtime::artifacts().join("catalogue-stills");
    let (key, state) = cat.collab.stills.of(stage, cat.size().pixels, &cache);
    let state = match state {
        backdrop::State::Making => "making".to_string(),
        backdrop::State::Ready => "ready".to_string(),
        backdrop::State::Failed(why) => format!("failed: {why}"),
    };
    cat.collab.still = Some((key, state));
}

/// The still the play backdrop shows now, with its name, once it is made.
pub fn still(cat: &Catalogue) -> Option<(&str, &backdrop::Picture)> {
    let (key, _) = cat.collab.still.as_ref()?;
    Some((key, cat.collab.stills.ready(key)?))
}
