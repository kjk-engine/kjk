//! Which schedules run their systems on one thread. Bevy's multithreaded
//! executor hands each system to the task pool and waits for it; a schedule
//! of many short systems (the render graph's nodes, run once per camera)
//! spends more on those hand-overs than the systems take.
//!
//! On macOS a window's surface is made by a render system that must run on
//! the main thread, and only the multithreaded executor sends it there: run
//! on one thread, it ran on the render thread and every windowed run
//! panicked at startup (2026-11-08, an M4 Pro Mac: the default and
//! `render` alone crashed, `main,graph` and `none` opened). So on macOS a
//! run with a window keeps the render schedule multithreaded whatever
//! `KJK_EXEC` names; headless runs make no surface and still take it.

use bevy::core_pipeline::{Core2d, Core3d};
use bevy::ecs::schedule::{ScheduleLabel, SingleThreadedExecutor};
use bevy::prelude::*;
use bevy::render::renderer::RenderGraph;
use bevy::render::{ExtractSchedule, Render, RenderApp};
use bevy::winit::WinitPlugin;

/// The schedules run on one thread: the main app's, the render
/// schedule and the render graph's (measured 2026-11-07, 1920x1080 on a
/// Ryzen 7 3700X: Harbinger 130 to 141 fps, the Dune Sea 219 to 230; the
/// extract schedule on one thread was slower).
const DEFAULT: &str = "main,render,graph";

/// Whether the window's surface must be made on the main thread, as Bevy
/// has it for these.
const SURFACE_ON_MAIN_THREAD: bool = cfg!(any(target_os = "macos", target_os = "ios"));

pub struct ExecutorsPlugin;

impl Plugin for ExecutorsPlugin {
    fn build(&self, _app: &mut App) {}

    fn finish(&self, app: &mut App) {
        // `KJK_EXEC` names others (a comma list; `none` for none), to
        // measure against.
        let modes = std::env::var("KJK_EXEC").unwrap_or_else(|_| DEFAULT.to_string());
        let has = |mode: &str| modes.split(',').any(|named| named == mode);
        fn single(app: &mut SubApp, label: impl ScheduleLabel) {
            app.edit_schedule(label, |schedule| {
                schedule.set_executor(SingleThreadedExecutor::new());
            });
        }
        let windowed = app.is_plugin_added::<WinitPlugin>();
        if has("main") {
            let main = app.main_mut();
            single(main, First);
            single(main, PreUpdate);
            single(main, Update);
            single(main, PostUpdate);
            single(main, Last);
        }
        let Some(render) = app.get_sub_app_mut(RenderApp) else {
            return;
        };
        if has("graph") {
            single(render, Core3d);
            single(render, Core2d);
            single(render, RenderGraph);
        }
        let mut note = "";
        if has("render") {
            if SURFACE_ON_MAIN_THREAD && windowed {
                note = " (render stays multithreaded: macOS makes the window's surface on the main thread)";
            } else {
                single(render, Render);
            }
        }
        if has("extract") {
            single(render, ExtractSchedule);
        }
        info!("single-threaded schedules: {modes}{note}");
    }
}
