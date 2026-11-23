//! `kjk try-script`: one compiled script run once in a module installed
//! headless, for an object or the player, then stepped on, and what
//! happened from the run on printed: the trace, conversation lines, faults
//! and the scripts named but missing. The level editor's Try runs it
//! (`docs/LEVEL-EDIT.md`), so it is in every build of kjk, the player's
//! too.
//!
//! The script is read through the module's layers, as the game would read
//! it: the module's own archives, the install's shared ones, and with
//! `--project` the project's folder first. It is added to the module's
//! library (`Library::with_script`), so it runs where nothing names it
//! yet.
//!
//! The module's own scripts go on running while the world is stepped: its
//! creatures' heartbeats alone write thousands of lines in two seconds. So
//! the trace printed is the tried script's own (`own_lines`): its runs, the
//! code it deferred, and the scripts it started with `ExecuteScript`. One
//! line says how much of the other scripts' trace was left out, and
//! `--all` prints the whole trace.
//!
//! With `--debug FILE` (the script's debug notes, `NAME.ndb`, which the
//! level editor writes beside its `.ncs`), the run is also recorded line by
//! line (`script_ncs::vm::observe`): a stop at each source line the script
//! starts, with the functions called to get there, the locals living there
//! and the script's globals, which the level editor steps through.

use std::process::ExitCode;

use fmt_odyssey::ResType;
use fmt_odyssey::ncs::Ncs;
use vfs::{Discovery, Game};

/// `kjk try-script`. `all` prints the whole trace, the module's other
/// scripts' too.
pub fn run(
    discovery: &Discovery,
    (game, module, script): (&str, &str, &str),
    on: Option<&str>,
    seconds: f32,
    (debug, all): (Option<&str>, bool),
) -> ExitCode {
    match tried(
        discovery,
        (game, module, script),
        (on, seconds),
        (debug, all),
    ) {
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
    (game, module, script): (&str, &str, &str),
    (on, seconds): (Option<&str>, f32),
    (debug, all): (Option<&str>, bool),
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
    let anims = std::sync::Arc::new(sim::motion::AnimTable::stand_in());
    let installer = crate::lifecycle::Installer::new(discovery, game, anims)?;
    // Read through the module's layers, the project's folder first.
    let script = script.trim_end_matches(".ncs").to_ascii_lowercase();
    let bytes = installer
        .resolver(module)?
        .read(&script, ResType::NCS)
        .ok_or(format!(
            "{script}.ncs is not found in {module} or the install's shared files (a script written in the level editor is compiled first)"
        ))?;
    let ncs = Ncs::parse(&bytes).map_err(|error| format!("{script}.ncs: {error}"))?;

    // The module for one player (the dev soldier), its library with the
    // script in it.
    let prepared = installer.prepare_one(module, None)?;
    let level = prepared.plan.level.clone();
    let library = level.scripts().ok_or("the module has no scripts")?;
    let library = (**library).clone().with_script(&script, ncs);
    let declared = library.globals_written();
    let level = level.with_scripts(library).declaring(declared);
    let mut world = sim::SimWorld::new(sim::WorldConfig::one_seat(), level)
        .map_err(|error| error.to_string())?;
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
    let trace = report.trace_lines();
    let since = &trace[before.0.min(trace.len())..];
    if all {
        for line in since {
            outln!("    {line}");
        }
    } else {
        let (own, others) = own_lines(since, &script);
        for line in own {
            outln!("    {line}");
        }
        if others.0 > 0 {
            outln!(
                "  the module's other scripts wrote {} more lines in {} runs, left out (--all prints them)",
                others.0,
                others.1
            );
        }
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

/// The script a line of the trace starts a run of: `run NAME for #7`, or
/// `deferred NAME for #7` for code the script deferred, after the line's
/// time. `None` for the lines of a run, which are its calls.
pub(crate) fn run_of(line: &str) -> Option<&str> {
    let (_, said) = line.trim_start().split_once(' ')?;
    let mut words = said.split(' ');
    match (words.next(), words.next(), words.next()) {
        (Some("run" | "deferred"), Some(name), Some("for")) => Some(name),
        _ => None,
    }
}

/// The lines of `trace` that are `script`'s own, and how many lines and
/// runs of other scripts were left out. A script's own lines are its runs,
/// the code it deferred, and the runs of each script that one of those
/// starts with `ExecuteScript`. The lines before the first run are kept.
fn own_lines<'a>(trace: &'a [String], script: &str) -> (Vec<&'a str>, (usize, usize)) {
    let mut own_scripts = std::collections::BTreeSet::from([script.to_ascii_lowercase()]);
    let (mut kept, mut left, mut runs) = (Vec::new(), 0, 0);
    let mut own = true;
    for line in trace {
        if let Some(name) = run_of(line) {
            own = own_scripts.contains(&name.to_ascii_lowercase());
            runs += usize::from(!own);
        } else if own
            && let Some((_, rest)) = line.split_once("ExecuteScript(\"")
            && let Some((started, _)) = rest.split_once('"')
        {
            own_scripts.insert(started.to_ascii_lowercase());
        }
        if own {
            kept.push(line.as_str());
        } else {
            left += 1;
        }
    }
    (kept, (left, runs))
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
