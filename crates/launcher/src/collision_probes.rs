//! `kjk collision trace` and `kjk collision probes`: traces through a
//! module's collision, one at a time or as a fixed set whose answers are
//! known (`collision::probes`, `collision_odyssey::probes`), written to
//! `facts.json`.

use std::process::ExitCode;

use collision::probes::{Probe, describe, player};
use collision::{Capsule, CollisionWorld, Contents, Extents, Shape, Trace};
use serde_json::json;
use vfs::Discovery;

use crate::collision_verbs::{ClipChoice, Doors, Loaded, load, set_doors, write_facts};

/// A shape by name: `point`, `player` (its box), `body` (the player's box round
/// across, as it moves), `crouch`, `capsule` (the player's).
pub fn shape(name: &str) -> Option<Shape> {
    Some(match name {
        "point" => Shape::Box(Extents::POINT),
        "player" => Shape::Box(player()),
        "body" => Shape::Upright(player()),
        "crouch" => Shape::Box(collision::probes::crouched()),
        "capsule" => Shape::Capsule(Capsule::from_extents(&player())),
        _ => return None,
    })
}

fn r4(value: f32) -> f64 {
    (f64::from(value) * 10_000.0).round() / 10_000.0
}

fn v4(p: [f32; 3]) -> serde_json::Value {
    json!([r4(p[0]), r4(p[1]), r4(p[2])])
}

/// A trace as JSON, with its material's label when `loaded` has one.
fn trace_json(trace: &Trace, loaded: Option<&Loaded>) -> serde_json::Value {
    let label = loaded.map(|l| l.surfaces.label(u32::from(trace.surface.material)));
    let entity = trace.entity.and_then(|entity| {
        let loaded = loaded?;
        let instance = loaded.area().instances.get(entity as usize)?;
        Some(format!(
            "{} {:?} {}",
            entity,
            instance.kind,
            String::from_utf8_lossy(&instance.tag)
        ))
    });
    json!({
        "fraction": trace.fraction,
        "end": v4(trace.end),
        "plane": { "normal": v4(trace.plane.normal), "dist": r4(trace.plane.dist) },
        "start_solid": trace.start_solid,
        "all_solid": trace.all_solid,
        "material": trace.surface.material,
        "label": label,
        "contents": trace.contents.0,
        "entity": entity.or(trace.entity.map(|e| e.to_string())),
    })
}

/// `describe` with the material's label from `loaded`.
fn described(trace: &Trace, loaded: Option<&Loaded>) -> String {
    let label = loaded.map(|l| l.surfaces.label(u32::from(trace.surface.material)));
    describe(trace, label)
}

pub struct TraceArgs<'a> {
    pub from: [f32; 3],
    pub to: [f32; 3],
    pub shape: &'a str,
    pub doors: Doors,
    /// Trace again this many times, both ends moved by the step each time.
    pub repeat: Option<([f32; 3], u32)>,
}

pub fn trace(
    discovery: &Discovery,
    game: &str,
    module: &str,
    clip: ClipChoice,
    args: &TraceArgs,
) -> ExitCode {
    let Some(shape) = shape(args.shape) else {
        eprintln!(
            "--shape {}: use point, player, crouch or capsule",
            args.shape
        );
        return ExitCode::from(2);
    };
    let mut loaded = match load(discovery, game, module, clip) {
        Ok(loaded) => loaded,
        Err(error) => {
            eprintln!("{error}");
            return ExitCode::FAILURE;
        }
    };
    set_doors(&mut loaded, args.doors);
    outln!(
        "{} {}, {}, doors {}:",
        loaded.game.key(),
        loaded.module.name,
        args.shape,
        args.doors.name()
    );
    let (step, count) = args.repeat.unwrap_or(([0.0; 3], 0));
    let mut traces = Vec::new();
    for k in 0..=count {
        let shift = |p: [f32; 3]| std::array::from_fn(|i| p[i] + step[i] * k as f32);
        let (from, to) = (shift(args.from), shift(args.to));
        let result = loaded
            .collision
            .level
            .sweep(from, to, &shape, Contents::PLAYER_SOLID);
        outln!(
            "  ({:.3}, {:.3}, {:.3}) -> ({:.3}, {:.3}, {:.3}): {}",
            from[0],
            from[1],
            from[2],
            to[0],
            to[1],
            to[2],
            described(&result, Some(&loaded))
        );
        traces.push(json!({
            "from": v4(from),
            "to": v4(to),
            "trace": trace_json(&result, Some(&loaded)),
        }));
    }
    write_facts(json!({
        "verb": "collision trace",
        "game": loaded.game.key(),
        "module": loaded.module.name,
        "shape": args.shape,
        "doors": args.doors.name(),
        "traces": traces,
    }));
    ExitCode::SUCCESS
}

/// Prints a probe as it is checked.
fn print(probe: &Probe) {
    outln!(
        "  {} {}: {}{}",
        if probe.pass { "ok  " } else { "FAIL" },
        probe.name,
        probe.expect,
        probe
            .trace
            .as_ref()
            .map(|t| format!("\n         {}", describe(t, None)))
            .unwrap_or_else(String::new)
    );
}

fn probes_json(list: &[Probe], loaded: Option<&Loaded>) -> serde_json::Value {
    json!(
        list.iter()
            .map(|probe| json!({
                "name": probe.name,
                "expect": probe.expect,
                "pass": probe.pass,
                "trace": probe.trace.as_ref().map(|t| trace_json(t, loaded)),
            }))
            .collect::<Vec<_>>()
    )
}

/// Probes on the module's own collision (`collision_odyssey::probes`),
/// printed, and what they measured as JSON.
fn module(loaded: &mut Loaded) -> (Vec<Probe>, serde_json::Value) {
    let area = &loaded.module.areas[loaded.area];
    let (probes, facts) = collision_odyssey::probes::module(
        &loaded.module,
        area,
        &loaded.surfaces,
        &mut loaded.collision,
    );
    for probe in &probes.0 {
        print(probe);
    }
    outln!(
        "  cost: {} random player traces up to 4 m, {:.2} µs each, {} hit; answers digest {}",
        facts.traces,
        facts.trace_us,
        facts.hits,
        facts.traces_digest
    );
    let json = json!({
        "entry": v4(facts.entry),
        "standing": v4(facts.standing),
        "ring": facts.ring.iter().map(|ray| json!({
            "bearing_deg": ray.bearing,
            "distance_m": r4(ray.distance),
            "material": ray.trace.surface.material,
            "normal": v4(ray.trace.plane.normal),
        })).collect::<Vec<_>>(),
        "doors": facts.doors.iter().map(|door| json!({
            "entity": door.entity,
            "tag": door.tag,
            "model": door.model,
            "closed": trace_json(&door.closed, Some(loaded)),
            "open": trace_json(&door.open, Some(loaded)),
        })).collect::<Vec<_>>(),
        "trace_us": facts.trace_us,
        "traces_digest": facts.traces_digest,
        "digest": facts.digest,
    });
    (probes.0, json)
}

pub fn probes(discovery: &Discovery, target: Option<(&str, &str)>, clip: ClipChoice) -> ExitCode {
    let mut loaded = match target {
        Some((game, module)) => match load(discovery, game, module, clip) {
            Ok(loaded) => Some(loaded),
            Err(error) => {
                eprintln!("{error}");
                return ExitCode::FAILURE;
            }
        },
        None => None,
    };
    outln!("synthetic probes:");
    let synthetic_list = collision::probes::synthetic().0;
    for probe in &synthetic_list {
        print(probe);
    }
    let mut module_facts = serde_json::Value::Null;
    let mut module_list = Vec::new();
    if let Some(loaded) = loaded.as_mut() {
        outln!(
            "{} {} probes (digest {}):",
            loaded.game.key(),
            loaded.module.name,
            loaded.collision.level.digest()
        );
        let (list, facts) = module(loaded);
        module_list = list;
        module_facts = facts;
        // The one-call constructor the sim uses gives this same level,
        // with the doors that can open open.
        if matches!(clip, ClipChoice::BuiltIn) {
            let direct = crate::viewer::resolver(discovery, loaded.game, Some(&loaded.module.name))
                .and_then(|resolver| {
                    crate::collision_verbs::module_level(
                        &resolver,
                        loaded.game,
                        &loaded.module.name,
                    )
                });
            set_doors(loaded, Doors::Openable);
            let same = direct.as_ref().is_ok_and(|level| {
                level.digest() == loaded.collision.level.digest()
                    && level
                        .entities()
                        .all(|e| level.state(e) == loaded.collision.level.state(e))
            });
            let probe = Probe {
                name: "module_level".to_string(),
                expect: "collision_verbs::module_level (a resolver in, a CollisionWorld out) builds the same level, digest and door states, as this probe's".to_string(),
                trace: None,
                pass: same,
            };
            print(&probe);
            module_list.push(probe);
        }
    }
    let failed = synthetic_list
        .iter()
        .chain(&module_list)
        .filter(|probe| !probe.pass)
        .count();
    outln!("{failed} failed");
    write_facts(json!({
        "verb": "collision probes",
        "module": loaded.as_ref().map(|l| format!("{} {}", l.game.key(), l.module.name)),
        "synthetic": probes_json(&synthetic_list, loaded.as_ref()),
        "module_probes": probes_json(&module_list, loaded.as_ref()),
        "module_facts": module_facts,
        "failed": failed,
    }));
    if failed == 0 {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}
