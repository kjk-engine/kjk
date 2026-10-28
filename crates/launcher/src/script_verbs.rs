//! `kjk script` (I16): the games' compiled scripts and the routines they
//! call.
//!
//! - `dis <game> <script> [--module m]`: the listing of one script
//!   (`script_ncs::disasm`), each routine named from the game's
//!   `nwscript.nss`.
//! - `check <game>`: every `.ncs` in every layer of the install (the shared
//!   layers and each module's archives, shadowed copies too) decoded
//!   (`fmt_odyssey::ncs`) and written back byte for byte; the catalog
//!   read; every `ACTION` checked against it: a known routine. Calls
//!   with another number of arguments than the prototype are counted, not
//!   failed: scripts compiled against an older `nwscript.nss` pass fewer
//!   (the engine fills the rest from the defaults) or more. Counts go to
//!   `facts.json`.

use std::collections::{BTreeMap, BTreeSet};
use std::process::ExitCode;

use fmt_odyssey::ResType;
use fmt_odyssey::ncs::{Ncs, Op};
use script_ncs::Catalog;
use serde_json::json;
use vfs::resolver::{Resolver, TextureQuality};
use vfs::{DirIndex, Discovery, Game};

/// An install's index and its shared layers.
pub(crate) fn open(
    discovery: &Discovery,
    game: &str,
) -> Result<(Game, DirIndex, Resolver), String> {
    let game = Game::from_key(game)
        .filter(|game| *game != Game::JediAcademy)
        .ok_or(format!("{game:?}: use k1 or tsl"))?;
    let install = discovery.get(game).ok_or(format!(
        "{}: not found (set {})",
        game.key(),
        game.env_var()
    ))?;
    let index = DirIndex::build(&install.data_root).map_err(|error| error.to_string())?;
    let resolver =
        Resolver::base(game, &index, TextureQuality::High).map_err(|error| error.to_string())?;
    Ok((game, index, resolver))
}

/// The game's routines and constants, from its `nwscript.nss`.
pub(crate) fn catalog(resolver: &Resolver) -> Result<Catalog, String> {
    let nss = ResType::NSS;
    let found = resolver
        .find("nwscript", nss)
        .ok_or("nwscript.nss: not in the install")?;
    let bytes = found.location.read().map_err(|error| error.to_string())?;
    // The games' code page: every byte one character.
    let text: String = bytes.iter().map(|&byte| char::from(byte)).collect();
    Catalog::parse(&text).map_err(|error| error.to_string())
}

pub(crate) fn ncs_type() -> ResType {
    ResType::NCS
}

pub fn dis(discovery: &Discovery, game: &str, script: &str, module: Option<&str>) -> ExitCode {
    let run = || -> Result<String, String> {
        let (_, index, base) = open(discovery, game)?;
        let resolver = match module {
            Some(module) => base
                .with_module(&index, module)
                .map_err(|error| error.to_string())?,
            None => base,
        };
        let catalog = catalog(&resolver);
        let name = script.trim_end_matches(".ncs");
        let found = resolver
            .find(name, ncs_type())
            .ok_or(format!("{name}.ncs: not found"))?;
        let bytes = found.location.read().map_err(|error| error.to_string())?;
        let ncs = Ncs::parse(&bytes).map_err(|error| format!("{name}.ncs: {error}"))?;
        let mut out = format!(
            "; {name}.ncs from {}, {} bytes\n",
            found.layer.name,
            bytes.len()
        );
        if let Err(error) = &catalog {
            out.push_str(&format!("; no routine names: {error}\n"));
        }
        out.push_str(&script_ncs::disasm::listing(&ncs, catalog.as_ref().ok()));
        Ok(out)
    };
    match run() {
        Ok(listing) => {
            outln!("{listing}");
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("{error}");
            ExitCode::FAILURE
        }
    }
}

/// What `check` found.
#[derive(Default)]
struct Tally {
    files: usize,
    bytes: usize,
    decoded: usize,
    same_bytes: usize,
    instructions: usize,
    names: BTreeSet<String>,
    failures: Vec<String>,
    opcodes: BTreeMap<String, usize>,
    routines: BTreeMap<u16, usize>,
    /// The scripts (by name) calling each routine.
    callers: BTreeMap<u16, BTreeSet<String>>,
    unknown_routines: BTreeSet<u16>,
    argument_mismatches: BTreeMap<String, usize>,
}

impl Tally {
    fn script(&mut self, layer: &str, name: &str, bytes: &[u8], catalog: Option<&Catalog>) {
        self.files += 1;
        self.bytes += bytes.len();
        self.names.insert(name.to_string());
        let ncs = match Ncs::parse(bytes) {
            Ok(ncs) => ncs,
            Err(error) => {
                self.failures.push(format!("{layer}/{name}.ncs: {error}"));
                return;
            }
        };
        self.decoded += 1;
        if ncs.write() == bytes {
            self.same_bytes += 1;
        } else {
            self.failures
                .push(format!("{layer}/{name}.ncs: written back differently"));
        }
        self.instructions += ncs.code.len();
        for instruction in &ncs.code {
            *self.opcodes.entry(instruction.op.name()).or_insert(0) += 1;
            let Op::Action { routine, args } = instruction.op else {
                continue;
            };
            *self.routines.entry(routine).or_insert(0) += 1;
            self.callers
                .entry(routine)
                .or_default()
                .insert(name.to_string());
            match catalog.map(|c| c.routine_by_id(routine)) {
                Some(None) => {
                    self.unknown_routines.insert(routine);
                }
                Some(Some(found)) if usize::from(args) != found.params.len() => {
                    *self
                        .argument_mismatches
                        .entry(format!(
                            "{} ({} of {})",
                            found.name,
                            args,
                            found.params.len()
                        ))
                        .or_insert(0) += 1;
                }
                _ => {}
            }
        }
    }
}

pub fn check(discovery: &Discovery, game: &str) -> ExitCode {
    let (game, index, base) = match open(discovery, game) {
        Ok(opened) => opened,
        Err(error) => {
            eprintln!("{error}");
            return ExitCode::FAILURE;
        }
    };
    let catalog = catalog(&base);
    match &catalog {
        Ok(catalog) => outln!(
            "{}: nwscript.nss: {} routines, {} constants, engine structures {:?}",
            game.key(),
            catalog.routines.len(),
            catalog.constants.len(),
            catalog.structures
        ),
        Err(error) => outln!("{}: {error}", game.key()),
    }
    let layers = base.every_layer(&index);
    let modules = Resolver::modules(&index);
    let started = std::time::Instant::now();
    let mut tally = Tally::default();
    for layer in &layers {
        for ((name, restype), location) in &layer.resources {
            if *restype != ncs_type() {
                continue;
            }
            match location.read() {
                Ok(bytes) => tally.script(&layer.name, name, &bytes, catalog.as_ref().ok()),
                Err(error) => tally
                    .failures
                    .push(format!("{}/{name}.ncs: {error}", layer.name)),
            }
        }
    }
    let ms = started.elapsed().as_secs_f64() * 1000.0;
    outln!(
        "{}: {} .ncs in {} layers ({} modules): {} scripts by name, {} bytes, {} instructions",
        game.key(),
        tally.files,
        layers.len(),
        modules.len(),
        tally.names.len(),
        tally.bytes,
        tally.instructions
    );
    outln!(
        "  decoded {} of {}, written back the same {}, failures {} ({ms:.0} ms)",
        tally.decoded,
        tally.files,
        tally.same_bytes,
        tally.failures.len()
    );
    for failure in tally.failures.iter().take(20) {
        outln!("    {failure}");
    }
    outln!(
        "  routines called: {} distinct; unknown to the catalog: {:?}; called with another argument count than the prototype: {}",
        tally.routines.len(),
        tally.unknown_routines,
        tally.argument_mismatches.len()
    );
    for (mismatch, count) in tally.argument_mismatches.iter().take(10) {
        outln!("    {mismatch}: {count}");
    }
    let named = |id: u16| {
        catalog
            .as_ref()
            .ok()
            .and_then(|c| c.routine_by_id(id))
            .map_or(format!("#{id}"), |r| r.name.clone())
    };
    let mut busiest: Vec<(&u16, &usize)> = tally.routines.iter().collect();
    busiest.sort_by(|a, b| b.1.cmp(a.1).then(a.0.cmp(b.0)));
    outln!("  most called:");
    for (id, count) in busiest.iter().take(15) {
        outln!("    {:<32} {count}", named(**id));
    }
    // The routines called that the sim leaves a gap, the most scripts
    // calling them first.
    let mut gaps: Vec<(u16, String, usize, usize)> = tally
        .routines
        .iter()
        .map(|(id, calls)| (*id, named(*id), *calls, tally.callers[id].len()))
        .filter(|(_, name, _, _)| !sim::script::bound(name))
        .collect();
    gaps.sort_by(|a, b| b.3.cmp(&a.3).then(b.2.cmp(&a.2)).then(a.1.cmp(&b.1)));
    outln!(
        "  called but not bound: {} routines (number, scripts calling, calls):",
        gaps.len()
    );
    for (id, name, calls, scripts) in &gaps {
        outln!("    {name:<36} {id:>4} {scripts:>5} {calls:>6}");
    }
    let mut opcodes: Vec<(&String, &usize)> = tally.opcodes.iter().collect();
    opcodes.sort_by(|a, b| b.1.cmp(a.1).then(a.0.cmp(b.0)));
    outln!(
        "  opcodes: {}",
        opcodes
            .iter()
            .map(|(name, count)| format!("{name} {count}"))
            .collect::<Vec<_>>()
            .join(", ")
    );
    let ok = tally.failures.is_empty() && catalog.is_ok() && tally.unknown_routines.is_empty();
    crate::collision_verbs::write_facts(json!({
        "game": game.key(),
        "files": tally.files,
        "layers": layers.len(),
        "modules": modules.len(),
        "scripts_by_name": tally.names.len(),
        "bytes": tally.bytes,
        "instructions": tally.instructions,
        "decoded": tally.decoded,
        "same_bytes": tally.same_bytes,
        "failures": tally.failures,
        "catalog": catalog.as_ref().map_or_else(
            |error| json!({ "error": error }),
            |c| json!({ "routines": c.routines.len(), "constants": c.constants.len(), "by_return": c.counts() }),
        ),
        "routines_called": tally.routines.len(),
        "unknown_routines": tally.unknown_routines,
        "unbound": gaps
            .iter()
            .map(|(id, name, calls, scripts)| {
                json!({ "routine": name, "number": id, "calls": calls, "scripts": scripts })
            })
            .collect::<Vec<_>>(),
        "argument_mismatches": tally.argument_mismatches,
        "callers": tally
            .callers
            .iter()
            .map(|(id, scripts)| (named(*id), scripts))
            .collect::<BTreeMap<_, _>>(),
        "opcodes": tally.opcodes,
        "ms": ms,
        "ok": ok,
    }));
    if ok {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}

/// The routines as nothing: each returns its type's empty value (and
/// `GetIsObjectValid`-style tests fail), deferred code is kept to run
/// after. What `run` exercises the VM with.
#[derive(Default)]
struct StandIn {
    calls: BTreeMap<u16, usize>,
    deferred: Vec<script_ncs::vm::Deferred>,
}

impl script_ncs::vm::Host for StandIn {
    fn action(
        &mut self,
        routine: &script_ncs::Routine,
        _caller: script_ncs::vm::ObjectId,
        args: Vec<script_ncs::vm::Value>,
    ) -> Result<script_ncs::vm::Value, String> {
        *self.calls.entry(routine.id).or_insert(0) += 1;
        for arg in args {
            if let script_ncs::vm::Value::Action(deferred) = arg {
                self.deferred.push(*deferred);
            }
        }
        Ok(script_ncs::vm::Value::empty(routine.ret))
    }
}

/// The most deferred blocks one script's run goes on to (they may defer
/// more).
const DEFERRED_PER_SCRIPT: usize = 64;

/// Every compiled script of the install, by name (the copy precedence
/// picks; each module's own once), run from its start with stand-in
/// routines, then the code it deferred.
pub fn run(discovery: &Discovery, game: &str, only: Option<&str>) -> ExitCode {
    use script_ncs::vm::{self, ObjectId, Program, Routines};
    let (game, index, base) = match open(discovery, game) {
        Ok(opened) => opened,
        Err(error) => {
            eprintln!("{error}");
            return ExitCode::FAILURE;
        }
    };
    let catalog = match catalog(&base) {
        Ok(catalog) => catalog,
        Err(error) => {
            eprintln!("{error}");
            return ExitCode::FAILURE;
        }
    };
    let catalog = std::sync::Arc::new(catalog);
    let routines = Routines::new(catalog.clone());
    // Each script name once: the shared layers' winner, then each module's
    // own copies.
    let mut scripts: BTreeMap<String, vfs::resolver::Location> = BTreeMap::new();
    let mut add = |resolver: &Resolver, scope: &str| {
        for (key, resolved) in resolver.all() {
            if key.1 == ncs_type() && only.is_none_or(|only| key.0.contains(only)) {
                scripts
                    .entry(format!("{scope}{}", key.0))
                    .or_insert_with(|| resolved.location.clone());
            }
        }
    };
    add(&base, "");
    for module in Resolver::modules(&index) {
        if let Ok(resolver) = base.with_module(&index, &module) {
            // The module's own archives: the layers the base has not (the
            // rulebook's, stacked again over them, aside).
            let own_layer = |layer: &&std::sync::Arc<vfs::resolver::Layer>| {
                layer.name != vfs::resolver::RULEBOOK
                    && !base.layers.iter().any(|shared| std::sync::Arc::ptr_eq(shared, layer))
            };
            let mut own = Resolver {
                game: resolver.game,
                layers: resolver.layers.iter().filter(own_layer).cloned().collect(),
                streams: Vec::new(),
                problems: Vec::new(),
                talk: vfs::resolver::TalkTable::default(),
            };
            own.layers.retain(|layer| !layer.resources.is_empty());
            add(&own, &format!("{module}/"));
        }
    }
    let started = std::time::Instant::now();
    let (mut finished, mut instructions, mut deferred_runs) = (0usize, 0usize, 0usize);
    let mut vm_time = std::time::Duration::ZERO;
    let mut faults: BTreeMap<&'static str, Vec<String>> = BTreeMap::new();
    let mut calls: BTreeMap<u16, usize> = BTreeMap::new();
    for (name, location) in &scripts {
        let Ok(ncs) = location
            .read()
            .map_err(|e| e.to_string())
            .and_then(|bytes| fmt_odyssey::ncs::Ncs::parse(&bytes).map_err(|e| e.to_string()))
        else {
            faults.entry("unreadable").or_default().push(name.clone());
            continue;
        };
        let program = Program::new(ncs);
        let mut host = StandIn::default();
        let running = std::time::Instant::now();
        let caller = ObjectId(2);
        let mut outcome = vm::run(&program, &routines, caller, &mut host);
        let mut done = 0;
        while outcome.is_ok() && done < DEFERRED_PER_SCRIPT {
            let Some(deferred) = host.deferred.pop() else {
                break;
            };
            done += 1;
            outcome = outcome.and_then(|first| {
                vm::resume(&program, &routines, caller, &deferred, &mut host).map(|then| {
                    vm::Finished {
                        returned: first.returned,
                        instructions: first.instructions + then.instructions,
                    }
                })
            });
        }
        deferred_runs += done;
        vm_time += running.elapsed();
        match outcome {
            Ok(ran) => {
                finished += 1;
                instructions += ran.instructions;
            }
            Err(fault) => faults
                .entry(fault.kind())
                .or_default()
                .push(format!("{name}: {fault}")),
        }
        for (routine, count) in host.calls {
            *calls.entry(routine).or_insert(0) += count;
        }
    }
    let ms = started.elapsed().as_secs_f64() * 1000.0;
    let faulted: usize = faults.values().map(Vec::len).sum();
    outln!(
        "{}: {} scripts run with stand-in routines: {finished} finished, {faulted} faulted; {instructions} instructions, {deferred_runs} deferred blocks run ({ms:.0} ms)",
        game.key(),
        scripts.len()
    );
    for (kind, examples) in &faults {
        outln!("  {kind}: {}", examples.len());
        for example in examples.iter().take(5) {
            outln!("    {example}");
        }
    }
    outln!("  routines reached: {}", calls.len());
    let vm_ms = vm_time.as_secs_f64() * 1000.0;
    outln!("  in the VM: {vm_ms:.0} ms");
    crate::collision_verbs::write_facts(json!({
        "game": game.key(),
        "scripts": scripts.len(),
        "finished": finished,
        "faulted": faulted,
        "faults": faults,
        "instructions": instructions,
        "deferred_runs": deferred_runs,
        "routines_reached": calls
            .iter()
            .map(|(id, count)| {
                let name = catalog.routine_by_id(*id).map_or("?", |r| r.name.as_str());
                (name.to_string(), *count)
            })
            .collect::<BTreeMap<_, _>>(),
        "ms": ms,
        "vm_ms": vm_ms,
    }));
    if faulted == 0 {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}

/// The VM's probes (`script_ncs::probes`), printed; what the approved
/// test `script_vm` asserts.
pub fn probes() -> ExitCode {
    let checks = script_ncs::probes::all();
    let failed = checks.iter().filter(|check| !check.ok).count();
    for check in &checks {
        let mark = if check.ok { "ok  " } else { "FAIL" };
        outln!("  {mark} {}: {}", check.name, check.detail);
    }
    outln!("script probes: {} checks, {failed} failed", checks.len());
    if failed == 0 {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}

/// How `coverage` runs: for how long, the whole trace or its first lines,
/// where the player walks (metres, game space; else it stands), and where
/// the frames go (`perception_frame`), if anywhere.
pub struct Coverage {
    pub seconds: u64,
    pub trace: bool,
    pub walk: Option<[f32; 2]>,
    pub frames: Option<std::path::PathBuf>,
    /// The player is the dev Jedi (`Sheet::dev_jedi`), else the soldier.
    pub jedi: bool,
}

/// A walker from where `start` stands to the floor at (`x`, `y`) metres.
fn walk_to(
    level: &sim::Level,
    start: &sim::motion::MotionState,
    x: f32,
    y: f32,
) -> Result<sim::walk::Walker, String> {
    let unit = collision::UNIT;
    let soles = (start.origin[2] + sim::motion::SOLE_Z) * unit;
    let floor = level
        .nav()
        .and_then(|nav| nav.height([x, y, soles], 50.0, 50.0))
        .ok_or(format!("no floor at {x}, {y}"))?;
    let goal = [x / unit, y / unit, floor / unit - sim::motion::SOLE_Z];
    Ok(sim::walk::Walker::new(goal))
}

/// One frame of who perceives whom: each character's place (metres),
/// facing, life, HP, FP and target, its script's attack and next round,
/// senses, and whom it sees and hears (`script::State`).
fn perception_frame(snapshot: &sim::Snapshot, world: &sim::SimWorld) -> serde_json::Value {
    let objects = sim::Instances::in_snapshot(world.level(), snapshot);
    let perceived = &world.script_state().perceived;
    let rounds = &world.script_state().rounds;
    let intents = world.script_intents();
    let bodies: Vec<serde_json::Value> = snapshot
        .entities
        .iter()
        .filter_map(|entity| {
            let ps = entity.player?;
            let data = objects.of_entity(entity);
            let senses = data.and_then(|data| data.senses);
            let them = perceived.get(&entity.id.0);
            Some(json!({
                "id": entity.id.0,
                "tag": data.map_or("player", |data| data.tag.as_str()),
                "player": matches!(entity.kind, sim::Kind::Avatar(_)),
                "x": entity.position[0],
                "y": entity.position[1],
                "z": entity.position[2],
                "yaw": ps.view[1],
                "alive": entity.fighter.is_none_or(|fighter| fighter.alive()),
                "hp": entity.fighter.map(|fighter| fighter.hp / 100),
                "fp": entity.fighter.map(|fighter| fighter.fp / 100),
                "target": entity.fighter.and_then(|fighter| fighter.target).map(|id| id.0),
                "intent": intents.get(&entity.id.0).map(|intent| intent.target.0),
                "talent": intents.get(&entity.id.0).and_then(|intent| intent.talent).map(|talent| {
                    match talent {
                        sim::script::actions::Use::Power(row) => format!("power {row}"),
                        sim::script::actions::Use::Feat(row) => format!("feat {row}"),
                    }
                }),
                "round_at": rounds.get(&entity.id.0),
                // The spells whose scripts' lasting effects run on it.
                "spells": entity.fighter.map_or_else(Vec::new, |fighter| {
                    let mut rows: Vec<u16> = fighter
                        .effects
                        .running(snapshot.time_ms)
                        .map(|lasting| lasting.spell)
                        .filter(|row| *row != u16::MAX)
                        .collect();
                    rows.dedup();
                    rows
                }),
                "sight": senses.map(|s| s.sight),
                "hearing": senses.map(|s| s.hearing),
                "seen": them.map_or(&[][..], |p| &p.seen[..]),
                "heard": them.map_or(&[][..], |p| &p.heard[..]),
            }))
        })
        .collect();
    json!({ "ms": snapshot.time_ms, "bodies": bodies })
}

/// Ticks between two frames (200 ms).
const FRAME_TICKS: u64 = 25;

/// A module installed for one player and stepped on the authority, its
/// scripts running: the world at the end, its level, how long stepping
/// took (ms), and the frames asked for (`Coverage::frames`, written).
pub(crate) struct Stepped {
    pub world: sim::SimWorld,
    pub level: sim::Level,
    pub library_size: Option<usize>,
    pub script_ms: f64,
    pub ms: f64,
}

/// Steps `module` as `options` say (see `coverage`).
pub(crate) fn step_module(
    installer: &crate::lifecycle::Installer,
    module: &str,
    options: &Coverage,
) -> Result<Stepped, String> {
    step_module_watched(installer, module, options, &[], &mut |_, _| {})
}

/// `step_module`, the creatures tagged `follow` following the player as
/// companions (`kjk play --follow`), with `watch` shown each tick's
/// snapshot and the world after it.
pub(crate) fn step_module_watched(
    installer: &crate::lifecycle::Installer,
    module: &str,
    options: &Coverage,
    follow: &[String],
    watch: &mut dyn FnMut(&sim::Snapshot, &sim::SimWorld),
) -> Result<Stepped, String> {
    let Coverage {
        seconds,
        walk,
        ref frames,
        jedi,
        ..
    } = *options;
    let prepared = installer.prepare_one(module, None)?;
    let facts = &prepared.plan.facts;
    let level = prepared.plan.level.clone();
    if level.scripts().is_none() {
        return Err("the module has no scripts (no nwscript.nss)".into());
    }
    let config = sim::WorldConfig {
        seed: 1,
        seats: 1,
        wanderers: 0,
        spawn: [0.0; 3],
        yaw: 0.0,
        sheet: if jedi {
            sim::combat::Sheet::dev_jedi()
        } else {
            sim::combat::Sheet::dev_soldier()
        },
        guests: sim::Guests::Avatars,
    };
    let world = sim::SimWorld::new(config, level.clone()).map_err(|error| error.to_string())?;
    let mut world = crate::stage::with_followers(world, follow, Some(installer))?;
    let rate = sim::TickRate::default();
    let ticks = seconds * u64::from(sim::TICK_HZ);
    let mut last = world.snapshot();
    let player = |snapshot: &sim::Snapshot| {
        snapshot
            .entities
            .iter()
            .find(|entity| matches!(entity.kind, sim::Kind::Avatar(_)))
            .and_then(|entity| entity.player)
    };
    let mut walker = match (walk, player(&last)) {
        (Some([x, y]), Some(start)) => Some(walk_to(&level, &start, x, y)?),
        (Some(_), None) => return Err("no player to walk".into()),
        _ => None,
    };
    let mut shots = Vec::new();
    let started = std::time::Instant::now();
    for tick in 1..=ticks {
        let mut input = sim::TickInput::default();
        if let (Some(walker), Some(at)) = (walker.as_mut(), player(&last)) {
            let doors = crate::scenes::door_states(&last);
            let mut command = walker.command(&level, &at, last.time_ms, &doors);
            command.buttons |= sim::buttons::USE;
            input.insert(sim::Seat(0), command);
        }
        last = sim::step(
            &mut world,
            tick,
            &input,
            rate.msec(tick),
            sim::StepReason::Authority,
        );
        watch(&last, &world);
        if frames.is_some() && tick % FRAME_TICKS == 0 {
            shots.push(perception_frame(&last, &world));
        }
    }
    let ms = started.elapsed().as_secs_f64() * 1000.0;
    if let Some(dir) = frames {
        let floor: Vec<[[f32; 2]; 3]> = level.nav().map_or_else(Vec::new, |nav| {
            nav.faces
                .iter()
                .map(|face| face.corners.map(|[x, y, _]| [x, y]))
                .collect()
        });
        std::fs::create_dir_all(dir).map_err(|error| error.to_string())?;
        let path = dir.join("frames.json");
        let body = json!({ "module": module, "floor": floor, "frames": shots });
        std::fs::write(&path, body.to_string()).map_err(|error| error.to_string())?;
        outln!("  frames: {} ({} of them)", path.display(), shots.len());
    }
    Ok(Stepped {
        world,
        level,
        library_size: facts.library,
        script_ms: facts.script_ms,
        ms,
    })
}

/// A module installed for one player and stepped headless on the
/// authority, its scripts running (`sim::script`): what ran, the routines
/// they called (bound, and gaps), faults, the globals set, and the trace.
/// The player stands, or walks (`sim::walk`, holding use) to a point.
pub fn coverage(discovery: &Discovery, game: &str, module: &str, options: &Coverage) -> ExitCode {
    let (seconds, trace) = (options.seconds, options.trace);
    let run = || -> Result<serde_json::Value, String> {
        let game = Game::from_key(game).ok_or(format!("{game:?}: use k1 or tsl"))?;
        let anims = std::sync::Arc::new(sim::motion::AnimTable::stand_in());
        let installer = crate::lifecycle::Installer::new(discovery, game, anims)?;
        let Stepped {
            world,
            level,
            library_size,
            script_ms,
            ms,
        } = step_module(&installer, module, options)?;
        let library = level.scripts().cloned().ok_or("no library")?;
        outln!(
            "{} {module}: {} scripts in the library ({:.0} ms), {} of {} routines bound",
            game.key(),
            library_size.unwrap_or(0),
            script_ms,
            library.bound(),
            library.routines().catalog().routines.len()
        );
        let ticks = seconds * u64::from(sim::TICK_HZ);
        let report = world.script_report();
        let state = world.script_state();
        outln!(
            "  at the end: {} objects with actions queued, {} moving",
            state.actions.len(),
            world.script_goals().len()
        );
        let list = |title: &str, counts: &BTreeMap<String, usize>| {
            let mut sorted: Vec<(&String, &usize)> = counts.iter().collect();
            sorted.sort_by(|a, b| b.1.cmp(a.1).then(a.0.cmp(b.0)));
            outln!(
                "  {title} ({}): {}",
                sorted.len(),
                sorted
                    .iter()
                    .map(|(name, count)| format!("{name} {count}"))
                    .collect::<Vec<_>>()
                    .join(", ")
            );
        };
        outln!(
            "  stepped {seconds} s ({ticks} ticks) in {ms:.0} ms; {} instructions",
            report.instructions
        );
        list("scripts run", &report.runs);
        list("deferred blocks run", &report.deferred);
        list("routines bound and called", &report.calls);
        list("gaps called", &report.gaps);
        list("scripts named but missing (nothing runs)", &report.missing);
        let actions: BTreeMap<String, usize> = report
            .actions
            .iter()
            .map(|(kind, count)| (kind.to_string(), *count))
            .collect();
        list("actions completed", &actions);
        outln!("  faults: {}", report.faults.len());
        for fault in &report.faults {
            outln!("    {fault}");
        }
        outln!("  runaways: {}", report.runaways.len());
        for runaway in &report.runaways {
            outln!("    {runaway}");
        }
        outln!(
            "  globals: {:?} {:?} {:?}; locals {}; waiting: {} events, {} deferred",
            state.numbers,
            state.booleans,
            state.strings,
            state.locals.len(),
            state.pending.len(),
            state.delayed.len()
        );
        let shown = if trace { report.trace.len() } else { 60 };
        outln!(
            "  trace ({} lines{}):",
            report.trace.len(),
            if report.trace_dropped > 0 {
                format!(", {} more dropped", report.trace_dropped)
            } else {
                String::new()
            }
        );
        for line in report.trace.iter().take(shown) {
            outln!("  {line}");
        }
        Ok(json!({
            "game": game.key(),
            "module": module,
            "library": library_size,
            "bound": library.bound(),
            "seconds": seconds,
            "runs": report.runs,
            "deferred": report.deferred,
            "calls": report.calls,
            "gaps": report.gaps,
            "actions": report.actions,
            "faults": report.faults,
            "runaways": report.runaways,
            "missing": report.missing,
            "instructions": report.instructions,
            "globals": { "numbers": state.numbers, "booleans": state.booleans, "strings": state.strings },
            "trace": report.trace,
        }))
    };
    match run() {
        Ok(facts) => {
            crate::collision_verbs::write_facts(facts);
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("{error}");
            ExitCode::FAILURE
        }
    }
}
