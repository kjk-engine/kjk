//! `kjk try-script`: one compiled script run once in a module installed
//! headless, for an object or the player, then stepped on, and what
//! happened from the run on printed: the trace, conversation lines, faults
//! and the scripts named but missing. The level editor's Try runs it
//! (`docs/LEVEL-EDIT.md`), so it is in every build of kjk, the player's
//! too.
//!
//! The script is read through the install's layers (with `--project`, the
//! project's folder first) and added to the module's library
//! (`Library::with_script`), so it runs where nothing names it yet.
//!
//! With `--debug FILE` (the script's debug notes, `NAME.ndb`, which the
//! level editor writes beside its `.ncs`), the run is also recorded line by
//! line (`script_ncs::vm::observe`): a stop at each source line the script
//! starts, with the functions called to get there, the locals living there
//! and the script's globals, which the level editor steps through.

use std::process::ExitCode;

use fmt_odyssey::ResType;
use fmt_odyssey::ncs::Ncs;
use vfs::resolver::{Resolver, TextureQuality};
use vfs::{DirIndex, Discovery, Game};

/// `kjk try-script`.
pub fn run(
    discovery: &Discovery,
    game: &str,
    module: &str,
    script: &str,
    on: Option<&str>,
    seconds: f32,
    debug: Option<&str>,
) -> ExitCode {
    match tried(discovery, game, module, script, (on, seconds), debug) {
        Ok(true) => ExitCode::SUCCESS,
        Ok(false) => ExitCode::FAILURE,
        Err(error) => {
            eprintln!("{error}");
            ExitCode::FAILURE
        }
    }
}

/// Whether the script ran without a fault.
fn tried(
    discovery: &Discovery,
    game: &str,
    module: &str,
    script: &str,
    (on, seconds): (Option<&str>, f32),
    debug: Option<&str>,
) -> Result<bool, String> {
    let debug = debug
        .map(|path| {
            std::fs::read_to_string(path)
                .map_err(|error| format!("{path}: {error}"))
                .and_then(|text| {
                    script_ncs::compile::Debug::parse(&text).map_err(|why| format!("{path}: {why}"))
                })
        })
        .transpose()?;
    let game = Game::from_key(game)
        .filter(|game| *game != Game::JediAcademy)
        .ok_or(format!("{game:?}: use k1 or tsl"))?;
    let install = discovery.get(game).ok_or(format!(
        "{}: not found (set {})",
        game.key(),
        game.env_var()
    ))?;
    let index = DirIndex::of_install(install).map_err(|error| error.to_string())?;
    let resolver =
        Resolver::base(game, &index, TextureQuality::High).map_err(|error| error.to_string())?;
    let script = script.trim_end_matches(".ncs").to_ascii_lowercase();
    let bytes = resolver
        .read(&script, ResType::NCS)
        .ok_or(format!("{script}.ncs is not found (compile it first)"))?;
    let ncs = Ncs::parse(&bytes).map_err(|error| format!("{script}.ncs: {error}"))?;

    // The module for one player (the dev soldier), its library with the
    // script in it.
    let anims = std::sync::Arc::new(sim::motion::AnimTable::stand_in());
    let installer = crate::lifecycle::Installer::new(discovery, game, anims)?;
    let prepared = installer.prepare_one(module, None)?;
    let level = prepared.plan.level.clone();
    let library = level.scripts().ok_or("the module has no scripts")?;
    let library = (**library).clone().with_script(&script, ncs);
    let declared = library.globals_written();
    let level = level.with_scripts(library).declaring(declared);
    let config = sim::WorldConfig {
        seed: 1,
        seats: 1,
        wanderers: 0,
        spawn: [0.0; 3],
        yaw: 0.0,
        sheet: sim::combat::Sheet::dev_soldier(),
        guests: sim::Guests::Avatars,
    };
    let mut world = sim::SimWorld::new(config, level).map_err(|error| error.to_string())?;
    let me = world
        .snapshot()
        .entities
        .iter()
        .find(|entity| matches!(entity.kind, sim::Kind::Avatar(_)))
        .map(|entity| entity.id)
        .ok_or("no player")?;

    let (caller, named) = match on {
        None => (me, "the player".to_string()),
        Some(tag) => {
            let level = world.level().clone();
            let data = level.module().ok_or("the module did not install")?;
            let index = data
                .objects()
                .position(|object| object.tag.eq_ignore_ascii_case(tag))
                .ok_or(format!("no object of {module} is tagged {tag}"))?
                as u32;
            let id = world
                .snapshot()
                .entities
                .iter()
                .find(|entity| matches!(entity.kind, sim::Kind::Object(_, at) if at == index))
                .map(|entity| entity.id)
                .ok_or(format!("{tag} is not in the world"))?;
            (id, tag.to_string())
        }
    };

    // What the module's own scripts did as it was installed is left out.
    let report = world.script_report();
    let before = (
        report.trace_lines().len(),
        report.talk.len(),
        report.faults.len(),
        report.runaways.len(),
        report.aborted.len(),
        report.missing.clone(),
    );
    let mut state = world.script_state().clone();
    state.pending.clear();
    state.pending.push(sim::script::Pending::new(
        &script,
        script_ncs::vm::ObjectId(caller.0),
    ));
    world.set_script_state(state);
    let stops = std::rc::Rc::new(std::cell::RefCell::new(Vec::<Stop>::new()));
    if let Some(debug) = debug {
        let program = world
            .level()
            .scripts()
            .and_then(|library| library.program(&script))
            .map(|program| program as *const script_ncs::vm::Program)
            .ok_or("the script is not in the library")?;
        let stops = stops.clone();
        script_ncs::vm::observe(Some(Box::new(move |watch| {
            if !std::ptr::eq(watch.program, program) {
                return;
            }
            let mut stops = stops.borrow_mut();
            if stops.len() < MOST_STOPS
                && let Some(stop) = stop_at(&debug, watch)
            {
                stops.push(stop);
            }
        })));
    }
    let ticks = ((seconds.max(0.0) * sim::TICK_HZ as f32) as u64).max(2);
    let input = sim::TickInput::default();
    for tick in 1..=ticks {
        sim::step(
            &mut world,
            tick,
            &input,
            sim::TickRate::default().msec(tick),
            sim::StepReason::Authority,
        );
    }

    script_ncs::vm::observe(None);
    let report = world.script_report();
    outln!(
        "{script} ran for {named} in {module}, then {:.1} s stepped",
        ticks as f32 / sim::TICK_HZ as f32
    );
    let ran = report.runs.get(&script).copied().unwrap_or(0);
    if ran == 0 {
        outln!("  it did not run");
    }
    outln!("  trace:");
    for line in report.trace_lines().iter().skip(before.0) {
        outln!("    {line}");
    }
    let talk: Vec<_> = report.talk.iter().skip(before.1).collect();
    if !talk.is_empty() {
        outln!("  conversation:");
        for line in talk {
            outln!("    {line:?}");
        }
    }
    let mut bad = false;
    for (title, list, from) in [
        ("faults", &report.faults, before.2),
        (
            "stopped at the instruction limit",
            &report.runaways,
            before.3,
        ),
        (
            "stopped at an argument of the wrong type",
            &report.aborted,
            before.4,
        ),
    ] {
        let new: Vec<&String> = list.iter().skip(from).collect();
        if !new.is_empty() {
            bad = true;
            outln!("  {title}:");
            for each in new {
                outln!("    {each}");
            }
        }
    }
    let missing: Vec<&str> = report
        .missing
        .iter()
        .filter(|(name, count)| before.5.get(*name) != Some(count))
        .map(|(name, _)| name.as_str())
        .collect();
    if !missing.is_empty() {
        outln!(
            "  scripts named that the module does not have: {}",
            missing.join(", ")
        );
    }
    let stops = stops.borrow();
    if !stops.is_empty() {
        outln!("  debug: {} stops", stops.len());
        for stop in stops.iter() {
            outln!("    stop {} {} {}", stop.file, stop.line, stop.function);
            outln!("      calls {}", stop.calls.join(" > "));
            for (name, ty, value) in &stop.locals {
                outln!("      local {name} {ty} = {value}");
            }
            for (name, ty, value) in &stop.globals {
                outln!("      global {name} {ty} = {value}");
            }
        }
    }
    Ok(ran > 0 && !bad)
}

/// The most stops a debugged run keeps.
const MOST_STOPS: usize = 2000;

/// The most globals a stop shows.
const MOST_GLOBALS: usize = 40;

/// A debugged run where a source line starts: the file, the line and the
/// function, the functions called to get there (the outermost first), the
/// locals living there and the script's globals, each as name, type and
/// value.
struct Stop {
    file: String,
    line: u32,
    function: String,
    calls: Vec<String>,
    locals: Vec<(String, String, String)>,
    globals: Vec<(String, String, String)>,
}

/// The stop `watch` is at, when a source line starts there.
fn stop_at(debug: &script_ncs::compile::Debug, watch: &script_ncs::vm::Watch<'_>) -> Option<Stop> {
    let (file, line) = debug.line_at(watch.offset)?;
    let function = debug
        .function_at(watch.offset)
        .map_or("?", |(name, ..)| name.as_str())
        .to_string();
    let named = |first: u32| {
        debug
            .functions
            .iter()
            .find(|(_, _, from, _)| *from == first)
            .map_or("?", |(name, ..)| name.as_str())
            .to_string()
    };
    let calls = if watch.deferred {
        vec![format!("deferred code of {function}")]
    } else {
        watch
            .frames
            .iter()
            .map(|(_, first)| named(*first))
            .collect()
    };
    // The locals of the function the run is in, where its call's frame is
    // known (deferred code keeps none).
    let frame = watch
        .frames
        .last()
        .filter(|_| !watch.deferred)
        .map(|(start, _)| *start);
    let locals = frame.map_or_else(Vec::new, |start| {
        debug
            .locals
            .iter()
            .filter(|local| {
                local.function == function && (local.from..local.to).contains(&watch.offset)
            })
            .map(|local| {
                let at = start as i64 + i64::from(local.home) / 4;
                (
                    local.name.clone(),
                    local.ty.clone(),
                    cells(watch.stack, at, &local.ty),
                )
            })
            .collect()
    });
    let globals = debug
        .globals
        .iter()
        .take(MOST_GLOBALS)
        .filter(|_| watch.bp > 0)
        .map(|(name, ty, home)| {
            (
                name.clone(),
                ty.clone(),
                cells(watch.stack, i64::from(*home) / 4, ty),
            )
        })
        .collect();
    Some(Stop {
        file: file.to_string(),
        line,
        function,
        calls,
        locals,
        globals,
    })
}

/// The value of type `ty` whose first cell is the stack's `at`.
fn cells(stack: &[script_ncs::vm::Value], at: i64, ty: &str) -> String {
    use script_ncs::vm::Value;
    let cell = |at: i64| usize::try_from(at).ok().and_then(|at| stack.get(at));
    let shown = |value: &Value| match value {
        Value::Int(v) => v.to_string(),
        Value::Float(v) => format!("{v}"),
        Value::String(text) => format!("{text:?}"),
        Value::Object(id) => format!("object {}", id.0),
        Value::Vector(v) => format!("[{}, {}, {}]", v[0], v[1], v[2]),
        other => format!("{other:?}"),
    };
    match ty {
        "vector" => {
            let parts: Vec<String> = (0..3).filter_map(|n| cell(at + n).map(shown)).collect();
            format!("[{}]", parts.join(", "))
        }
        _ if ty.starts_with("struct:") => "(a struct)".to_string(),
        _ => cell(at).map_or_else(|| "(not on the stack)".to_string(), shown),
    }
}
