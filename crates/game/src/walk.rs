//! Walk mode (`kjk play --walk`): a module drawn as play draws it, with a
//! free camera and nothing that plays it.
//!
//! The module runs with none of its scripts. No conversation, cutscene,
//! trigger or screen fade starts. Every creature and object with a sheet is
//! turned to the neutral faction, so no fight starts. The keys and the mouse
//! never reach the player, who stands where the module puts them. The camera
//! flies through walls and closed doors with the fly controls (`scene::fly`).
//! The bug-report key still makes a report.
//!
//! Page Down and Page Up go to the next and the last module of a list
//! (`step`), so a person can walk many modules in one run and report from
//! each.

use std::sync::atomic::{AtomicBool, Ordering};

use bevy::prelude::*;
use sim::combat::sheet::factions;

use crate::stage::Stage;

/// Whether this run is in walk mode. It is set once, before the stage loads.
static ON: AtomicBool = AtomicBool::new(false);

/// Puts this run in walk mode. The launcher calls it before the stage loads.
pub fn hold() {
    ON.store(true, Ordering::Relaxed);
}

/// Whether this run is in walk mode (`hold`).
pub fn on() -> bool {
    ON.load(Ordering::Relaxed)
}

/// The console's `camera` pose of a camera placed at `transform` (Bevy
/// space). The field of view is left as the camera has it.
pub(crate) fn pose(transform: &bevy::prelude::Transform) -> console::CameraPose {
    let to_game = crate::scene::game_to_bevy().inverse();
    let at = to_game * transform.translation;
    let forward = to_game * *transform.forward();
    console::CameraPose {
        position: at.to_array(),
        yaw: forward.y.atan2(forward.x).to_degrees(),
        pitch: forward.z.clamp(-1.0, 1.0).asin().to_degrees(),
        fov: None,
    }
}

/// The stage as walk mode plays it. Its level has no scripts. Every
/// creature and object with a sheet is in the neutral faction. A stage
/// without a module comes back as it is.
pub fn stage(mut stage: Stage) -> Stage {
    stage.level = level(stage.level);
    if let Some(plan) = &mut stage.plan {
        plan.level = stage.level.clone();
    }
    stage
}

/// The level as walk mode plays it (`stage`). A module the run goes to
/// later takes this too, before it is installed (`sim_host`'s `advance`).
pub(crate) fn level(level: sim::Level) -> sim::Level {
    let Some(module) = level.module() else {
        return level;
    };
    let mut data = module.as_ref().clone();
    data.change_each(
        |object| object.sheet.is_some(),
        |_, object| {
            if let Some(sheet) = object.sheet.as_mut() {
                sheet.faction = factions::NEUTRAL;
            }
        },
    );
    level.with_module(data).without_scripts()
}

/// The modules Page Down and Page Up step through, in lower case:
/// `KJK_WALK_MODULES` names a file with one module a line. A line that
/// starts with `#` is a comment. Without the variable the keys do nothing.
static MODULES: std::sync::LazyLock<Vec<String>> = std::sync::LazyLock::new(|| {
    let Some(path) = std::env::var_os("KJK_WALK_MODULES").filter(|path| !path.is_empty()) else {
        return Vec::new();
    };
    match std::fs::read_to_string(&path) {
        Ok(text) => (text.lines())
            .map(|line| line.trim().to_ascii_lowercase())
            .filter(|line| !line.is_empty() && !line.starts_with('#'))
            .collect(),
        Err(error) => {
            warn!("KJK_WALK_MODULES: {}: {error}", path.to_string_lossy());
            Vec::new()
        }
    }
});

/// Where `step` is in the list: the module it last asked for, and the
/// scene it last named in the window's title.
#[derive(Default)]
pub(crate) struct Stepped {
    asked: Option<usize>,
    named: Option<u32>,
}

/// Walk mode's Page Down and Page Up: the next and the last module of the
/// list (`MODULES`), by the way a console's `module` command goes there.
/// The list wraps around. The window's title names the module and its
/// place in the list, so a person knows where a report was made. The
/// owner asked on 21 November 2026 for "a stepper to the walker so we can
/// step through the modules without closing it down".
pub(crate) fn step(
    keys: Res<ButtonInput<KeyCode>>,
    swap: Option<Res<crate::sim_host::SceneSwap>>,
    request: Option<ResMut<console::ModuleRequest>>,
    mut windows: Query<&mut Window, With<bevy::window::PrimaryWindow>>,
    mut stepped: Local<Stepped>,
) {
    if !on() || MODULES.is_empty() {
        return;
    }
    let (Some(swap), Some(mut request)) = (swap, request) else {
        return;
    };
    let here = swap.level.name().to_ascii_lowercase();
    let index = MODULES.iter().position(|module| *module == here);
    if stepped.named != Some(swap.generation) {
        stepped.named = Some(swap.generation);
        let place = match index {
            Some(index) => format!("{} of {}", index + 1, MODULES.len()),
            None => "not in the list".to_string(),
        };
        info!("walk: {here} ({place})");
        for mut window in &mut windows {
            window.title = format!("kjk walk: {here} ({place}). Page Down: next. Page Up: last.");
        }
    }
    if let Some(why) = request.failed.take() {
        warn!("walk: the module did not load: {why}");
    }
    let by = if keys.just_pressed(KeyCode::PageDown) {
        1
    } else if keys.just_pressed(KeyCode::PageUp) {
        MODULES.len() - 1
    } else {
        return;
    };
    // From the module last asked for, so a module that does not load is
    // stepped past. From the module shown, where none was asked for.
    let from = stepped.asked.or(index);
    let next = from.map_or(0, |from| (from + by) % MODULES.len());
    stepped.asked = Some(next);
    info!("walk: going to {}", MODULES[next]);
    request.pending = Some((MODULES[next].clone(), None));
    request.failed = None;
}
