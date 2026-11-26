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
        "Waits for a time (2s, 500ms), a number of frames (30f) or a state (world, loading, loading_screen).",
        "wait world",
    ),
    verb(
        "screenshot",
        &[NAME],
        "Saves the next settled frame as NAME.png in the run's folder.",
        "screenshot hall",
    ),
    verb(
        "film",
        &[Arg::new("TIME", ArgKind::Word), NAME],
        "Saves every frame for a length of wall time as raw RGB in NAME.rgb.",
        "film 8s fight",
    ),
    verb(
        "dump",
        &[NAME],
        "Writes the run's state as NAME.json in the run's folder.",
        "dump after",
    ),
    verb(
        "camera",
        &[REST],
        "Puts the camera at x y z (metres) looking along a yaw and a pitch (degrees), with an optional field of view.",
        "camera 20 20 1.5 90 0 60",
    ),
    verb(
        "anim",
        &[Arg::new("SECONDS", ArgKind::Word)],
        "Holds animation and game time at a second, or lets them run again with anim run.",
        "anim 2.5",
    ),
    verb(
        "tick",
        &[Arg::new("TICK", ArgKind::Word)],
        "Steps the world to a tick of the simulation and holds it there, or lets it run again with tick run.",
        "tick 400",
    ),
    verb(
        "module",
        &[
            Arg::new("MODULE", ArgKind::Word),
            Arg::optional("ARRIVAL", ArgKind::Word),
        ],
        "Loads a module, arriving at a door or a waypoint, and waits until its world is up.",
        "module end_m01ab",
    ),
    verb(
        "travel",
        &[
            Arg::new("MODULE", ArgKind::Word),
            Arg::optional("ARRIVAL", ArgKind::Word),
        ],
        "Asks for a module to load, as module does, and goes on at once while it loads.",
        "travel end_m01ab",
    ),
    verb(
        "record",
        &[REST],
        "Records a video: NAME, FROM and TO in seconds of game time, and frames a second.",
        "record walk 1.5 5.5 30",
    ),
    verb(
        "clip",
        &[REST],
        "Records the running game as a video: NAME, seconds of game time, and frames a second.",
        "clip fight 9 12",
    ),
    verb(
        "size",
        &[
            Arg::new("WIDTH", ArgKind::Word),
            Arg::new("HEIGHT", ArgKind::Word),
        ],
        "Resizes a headless run's frame to a width and a height in pixels. size 0 0 stands for a minimized window.",
        "size 1920 1080",
    ),
    verb("quit", &[], "Ends the run with success.", "quit"),
    verb(
        "crash",
        &[],
        "Makes the program panic, to show what a crash leaves in the run's folder.",
        "crash",
    ),
    Spec {
        name: "ui",
        aliases: &[],
        args: &[REST],
        help: "Passes words to the game's screens, such as ui open inventory or ui press BTN_OK. docs/UI.md lists them.",
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
        help: "Plays a row of visualeffects.2da on the creature with a tag, as a script would, for a number of seconds, with a beam from a second tag.",
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
