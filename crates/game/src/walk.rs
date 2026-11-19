//! Walk mode (`kjk play --walk`): a module drawn as play draws it, with a
//! free camera and nothing that plays it.
//!
//! The module runs with none of its scripts. No conversation, cutscene,
//! trigger or screen fade starts. Every creature and object with a sheet is
//! turned to the neutral faction, so no fight starts. The keys and the mouse
//! never reach the player, who stands where the module puts them. The camera
//! flies through walls and closed doors with the fly controls (`scene::fly`).
//! The bug-report key still makes a report.

use std::sync::atomic::{AtomicBool, Ordering};

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
    let Some(module) = stage.level.module() else {
        return stage;
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
    stage.level = stage.level.clone().with_module(data).without_scripts();
    if let Some(plan) = &mut stage.plan {
        plan.level = stage.level.clone();
    }
    stage
}
