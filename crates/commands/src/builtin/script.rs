//! The verbs of `--cmds` scripts that the script queue runs itself
//! (`console::script`), listed here so that help and the documentation
//! page show them. The `ui` and `vfx` words also run from the console:
//! they are passed on unchanged.

use super::{Ran, script_only};
use crate::{Arg, ArgKind, Effect, Facts, Group, Kind, Parsed, Place, Spec};

/// A verb the script queue runs.
const fn verb(
    name: &'static str,
    args: &'static [Arg],
    help: &'static str,
    example: &'static str,
) -> Spec {
    Spec {
        name,
        aliases: &[],
        args,
        help,
        example,
        group: Group::Script,
        place: Place::ScriptOnly,
        kind: Kind::Session,
        run: script_only,
    }
}

const NAME: Arg = Arg::optional("NAME", ArgKind::Word);
const REST: Arg = Arg::new("WORDS", ArgKind::Text);

pub static SPECS: &[Spec] = &[
    verb(
        "wait",
        &[Arg::new("FOR", ArgKind::Word)],
        "Wait for a duration (2s, 500ms), frame count (30f), or state (world, loading, loading_screen).",
        "wait world",
    ),
    verb(
        "screenshot",
        &[NAME],
        "Save the next settled frame as NAME.png in the run folder.",
        "screenshot hall",
    ),
    verb(
        "film",
        &[Arg::new("TIME", ArgKind::Word), NAME],
        "Capture every frame for TIME (real time) as raw RGB to NAME.rgb.",
        "film 8s fight",
    ),
    verb(
        "dump",
        &[NAME],
        "Dump current run state to NAME.json in the run folder.",
        "dump after",
    ),
    verb(
        "camera",
        &[REST],
        "Move camera to x y z (meters) with yaw and pitch (degrees), plus optional FOV.",
        "camera 20 20 1.5 90 0 60",
    ),
    verb(
        "anim",
        &[Arg::new("SECONDS", ArgKind::Word)],
        "Freeze animation and game time at SECONDS, or resume with \"anim run\".",
        "anim 2.5",
    ),
    verb(
        "tick",
        &[Arg::new("TICK", ArgKind::Word)],
        "Step simulation to TICK and pause, or resume with \"tick run\".",
        "tick 400",
    ),
    verb(
        "module",
        &[
            Arg::new("MODULE", ArgKind::Word),
            Arg::optional("ARRIVAL", ArgKind::Word),
        ],
        "Load MODULE (optionally at ARRIVAL) and wait until it finishes loading.",
        "module end_m01ab",
    ),
    verb(
        "travel",
        &[
            Arg::new("MODULE", ArgKind::Word),
            Arg::optional("ARRIVAL", ArgKind::Word),
        ],
        "Start loading MODULE (optionally at ARRIVAL) without waiting.",
        "travel end_m01ab",
    ),
    verb(
        "record",
        &[REST],
        "Record a video: NAME FROM TO FPS (game time in seconds).",
        "record walk 1.5 5.5 30",
    ),
    verb(
        "clip",
        &[REST],
        "Record a video from now: NAME SECONDS FPS.",
        "clip fight 9 12",
    ),
    verb(
        "size",
        &[
            Arg::new("WIDTH", ArgKind::Word),
            Arg::new("HEIGHT", ArgKind::Word),
        ],
        "Resize a headless frame to WIDTH x HEIGHT pixels (0 0 simulates a minimized window).",
        "size 1920 1080",
    ),
    verb("quit", &[], "Exit the run with status 0.", "quit"),
    verb(
        "crash",
        &[],
        "Trigger a deliberate panic to test crash reporting.",
        "crash",
    ),
    Spec {
        name: "ui",
        aliases: &[],
        args: &[REST],
        help: "Send a UI test command (e.g. \"ui open inventory\", \"ui press BTN_OK\"). See docs/UI.md.",
        example: "ui open inventory",
        group: Group::Script,
        place: Place::Anywhere,
        kind: Kind::Session,
        run: ui,
    },
    Spec {
        name: "vfx",
        aliases: &[],
        args: &[REST],
        help: "Play a visualeffects.2da row on a tagged creature for a duration, with an optional beam source tag.",
        example: "vfx 2007 end_trask 9",
        group: Group::Script,
        place: Place::Game,
        kind: Kind::Session,
        run: vfx,
    },
];

/// The words of a `Text` argument, split again.
fn words(parsed: &Parsed) -> Vec<String> {
    (parsed.word(0).unwrap_or("").split_whitespace())
        .map(str::to_string)
        .collect()
}

fn ui(parsed: &Parsed, _: &Facts) -> Ran {
    Ok(vec![Effect::Ui(words(parsed))])
}

fn vfx(parsed: &Parsed, _: &Facts) -> Ran {
    Ok(vec![Effect::Vfx(words(parsed))])
}
