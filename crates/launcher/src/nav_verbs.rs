//! `kjk collision nav`: the navigation mesh's evidence (I27). Bakes a
//! module's mesh from its walkable faces twice (the same digest both
//! times), reports its faces, portals, seams between rooms and islands,
//! then routes between face centres of its largest island drawn by a
//! seeded generator and checks every route: it ends where asked, every
//! leg stays over the mesh (sampled every 5 cm), and a standing body
//! lifted by a step's height goes along each leg without meeting the level
//! (doors as the module places them; a leg through a closed door is
//! counted apart). Writes `facts.json`.
//!
//! `kjk collision walk` walks such routes for real: a player stood at a
//! drawn face's centre of a planned module (the session bakes the level's
//! mesh), steered each step by a `sim::walk::Walker` (a waypoint at a
//! time, planned again from where it stands), through JA's movement step with the level's collision and
//! the module's doors, until it stands at the goal or its time is out
//! (three times the route's length at a run, and two seconds).

use std::process::ExitCode;
use std::time::Instant;

use collision::nav::NavMesh;
use collision::{CollisionWorld, Contents, Extents, UNIT};
use sim::motion;
use vfs::Discovery;

use crate::collision_verbs::{ClipChoice, load};

/// A splitmix step: the probe's own seeded draw.
fn draw(state: &mut u64) -> u64 {
    *state = state.wrapping_add(0x9E37_79B9_7F4A_7C15);
    let mut z = *state;
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

/// How a route's legs fared.
#[derive(Default)]
struct Legs {
    legs: usize,
    off_mesh: usize,
    blocked: usize,
    /// The first few legs that leave the mesh: their ends and the first
    /// point off it.
    strays: Vec<([f32; 3], [f32; 3], [f32; 3])>,
    walls: Vec<String>,
}

/// Checks each leg of `route` from `from`, stepping along it every 5 cm
/// at the floor's height: over the mesh all along (within 1.5 m of the
/// line's height: legs cross ramps), and clear for a standing body lifted a
/// step from one step to the next.
fn check_legs(
    mesh: &NavMesh,
    level: &dyn CollisionWorld,
    from: [f32; 3],
    route: &[[f32; 3]],
    legs: &mut Legs,
) {
    let half = motion::HALF_WIDTH * UNIT;
    // The body's box from a step above its soles to its head, a hair
    // narrower than the route keeps it from the edges.
    let extents = Extents {
        mins: [
            -half + 0.01,
            -half + 0.01,
            (motion::SOLE_Z + motion::STEP_HEIGHT) * UNIT,
        ],
        maxs: [half - 0.01, half - 0.01, 40.0 * UNIT],
    };
    let stand = |p: [f32; 3]| [p[0], p[1], p[2] - motion::SOLE_Z * UNIT + 0.01];
    let mut at = from;
    for &corner in route {
        legs.legs += 1;
        let length = collision::nav::flat_length(at, corner);
        let samples = ((length / 0.05) as usize).max(1);
        let mut floor = Vec::with_capacity(samples + 1);
        let mut off = None;
        for step in 0..=samples {
            let t = step as f32 / samples as f32;
            let p: [f32; 3] = std::array::from_fn(|axis| at[axis] + (corner[axis] - at[axis]) * t);
            match mesh.height(p, 1.5, 1.5) {
                Some(z) => floor.push([p[0], p[1], z]),
                None => {
                    off.get_or_insert(p);
                }
            }
        }
        if let Some(p) = off {
            legs.off_mesh += 1;
            if legs.strays.len() < 3 {
                legs.strays.push((at, corner, p));
            }
        }
        let blocked = floor.windows(2).find_map(|pair| {
            let trace = level.trace(stand(pair[0]), stand(pair[1]), &extents, Contents::SOLID);
            (trace.fraction < 1.0 || trace.start_solid).then_some((pair[0], trace))
        });
        if let Some((p, trace)) = blocked {
            legs.blocked += 1;
            if legs.walls.len() < 4 {
                legs.walls.push(format!(
                    "at {p:?}: fraction {:.3}, start solid {}, normal {:?}, entity {:?}",
                    trace.fraction, trace.start_solid, trace.plane.normal, trace.entity
                ));
            }
        }
        at = corner;
    }
}

/// `kjk collision nav`.
pub fn nav(
    discovery: &Discovery,
    game: &str,
    module: &str,
    clip: ClipChoice,
    routes: usize,
    seed: u64,
) -> ExitCode {
    let loaded = match load(discovery, game, module, clip) {
        Ok(loaded) => loaded,
        Err(error) => {
            eprintln!("{error}");
            return ExitCode::FAILURE;
        }
    };
    let walkable = loaded.collision.nav_faces();
    let started = Instant::now();
    let mut mesh = NavMesh::build(&walkable, sim::walk::CLEARANCE * UNIT);
    let tagged = mesh.mark(&loaded.collision.level);
    let bake_ms = started.elapsed().as_secs_f64() * 1000.0;
    let mut again = NavMesh::build(&walkable, sim::walk::CLEARANCE * UNIT);
    again.mark(&loaded.collision.level);
    let again = again.digest();
    let digest = mesh.digest();
    let same = digest == again;
    let mut sizes = vec![0usize; mesh.islands];
    for face in &mesh.faces {
        sizes[face.island as usize] += 1;
    }
    let largest = (0..sizes.len()).max_by_key(|&island| (sizes[island], usize::MAX - island));
    let mut by_size = sizes.clone();
    by_size.sort_unstable_by(|a, b| b.cmp(a));
    outln!(
        "{} {}: {} walkable faces ({} too steep to walk) -> {} faces, {} portals ({} shared edges, {} seams), {} islands (largest {:?}), baked in {bake_ms:.1} ms",
        loaded.game.key(),
        loaded.module.name,
        walkable.len(),
        mesh.steep,
        mesh.faces.len(),
        mesh.portals(),
        mesh.shared,
        mesh.seams,
        mesh.islands,
        &by_size[..by_size.len().min(8)]
    );
    outln!("  {tagged} faces under a door or placeable (every route below passes them)");
    // Where each smaller island lies (a piece of floor no way leads to or
    // from: what stands there cannot be walked up to).
    for (island, size) in sizes.iter().enumerate() {
        if Some(island) == largest || *size > 8 {
            continue;
        }
        let centres: Vec<String> = mesh
            .faces
            .iter()
            .filter(|face| face.island as usize == island)
            .map(|face| {
                format!(
                    "({:.2}, {:.2}, {:.2})",
                    face.centre[0], face.centre[1], face.centre[2]
                )
            })
            .collect();
        outln!("  island {island}: {size} faces at {}", centres.join(", "));
    }
    outln!(
        "  digest {} ({})",
        digest
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect::<String>(),
        if same {
            "the same baked twice"
        } else {
            "DIFFERENT when baked again"
        }
    );
    let Some(island) = largest else {
        eprintln!("no walkable faces");
        return ExitCode::FAILURE;
    };
    let pool: Vec<usize> = (0..mesh.faces.len())
        .filter(|&face| mesh.faces[face].island as usize == island)
        .collect();
    let mut state = seed;
    let mut legs = Legs::default();
    let (mut found, mut missed, mut wrong_end) = (0usize, 0usize, 0usize);
    let (mut corners, mut ratio, mut worst_us) = (0usize, 0.0f64, 0.0f64);
    let (mut tight, mut total_us) = (0usize, 0.0f64);
    let started = Instant::now();
    for _ in 0..routes {
        let from = mesh.faces[pool[(draw(&mut state) % pool.len() as u64) as usize]].centre;
        let to = mesh.faces[pool[(draw(&mut state) % pool.len() as u64) as usize]].centre;
        let one = Instant::now();
        let route = mesh.route(from, to, false, &|_| true);
        let took = one.elapsed().as_secs_f64() * 1e6;
        worst_us = worst_us.max(took);
        total_us += took;
        let Some(route) = route else {
            missed += 1;
            continue;
        };
        found += 1;
        if route.last() != Some(&to) {
            wrong_end += 1;
        }
        corners += route.len();
        let mut length = 0.0f64;
        let mut at = from;
        for corner in &route {
            length += f64::from(collision::nav::flat_length(at, *corner));
            at = *corner;
        }
        let straight = f64::from(collision::nav::flat_length(from, to));
        if straight > 0.5 {
            ratio += length / straight;
        }
        check_legs(&mesh, &loaded.collision.level, from, &route, &mut legs);
        tight += mesh
            .route(from, to, true, &|_| true)
            .map_or(0, |route| route.len());
    }
    let mean_us = total_us / routes.max(1) as f64;
    outln!(
        "  {routes} routes in the largest island ({} faces): {found} found, {missed} not, {wrong_end} ending elsewhere; {:.1} corners ({:.1} without the body's width) and {:.2}× the straight distance on average; {mean_us:.0} µs a route (worst {worst_us:.0} µs); checked in {:.1} s",
        pool.len(),
        corners as f64 / found.max(1) as f64,
        tight as f64 / found.max(1) as f64,
        ratio / found.max(1) as f64,
        started.elapsed().as_secs_f64()
    );
    outln!(
        "  legs: {} ({} leave the mesh, {} meet the level with a body lifted a step: doors and props on the floor)",
        legs.legs,
        legs.off_mesh,
        legs.blocked
    );
    for wall in &legs.walls {
        outln!("    met: {wall}");
    }
    for (from, to, off) in &legs.strays {
        outln!("    off the mesh: {from:?} -> {to:?}, first at {off:?}");
    }
    crate::runtime::write_facts(serde_json::json!({
        "verb": "collision nav", "game": game, "module": module,
        "walkable": walkable.len(), "faces": mesh.faces.len(), "portals": mesh.portals(),
        "shared": mesh.shared, "seams": mesh.seams, "islands": mesh.islands,
        "largest_islands": &by_size[..by_size.len().min(8)],
        "bake_ms": bake_ms, "digest_same": same,
        "routes": routes, "found": found, "missed": missed, "wrong_end": wrong_end,
        "legs": legs.legs, "off_mesh": legs.off_mesh, "blocked": legs.blocked,
        "mean_us": mean_us, "worst_us": worst_us,
    }));
    let pass = same && missed == 0 && wrong_end == 0 && legs.off_mesh == 0;
    if pass {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}

/// `kjk collision walk`.
pub fn walk(discovery: &Discovery, game: &str, module: &str, routes: usize, seed: u64) -> ExitCode {
    use sim::{Kind, Seat, SimWorld, StepReason, TickInput, TickRate, step};
    // Its scenes left out: a trigger's conversation holds the walker where
    // it stands (the North Apartments' interrogation, Sarna's party).
    let installed = crate::scenes::installed(discovery, game, module, None, &[], &[])
        .and_then(|world| crate::scenes::one_seat(world.level().clone().without_scripts(), None));
    let mut world = match installed {
        Ok(world) => world,
        Err(error) => {
            eprintln!("{error}");
            return ExitCode::FAILURE;
        }
    };
    let level = world.level().clone();
    let Some(mesh) = level.nav().cloned() else {
        eprintln!("{module} has no navigation mesh");
        return ExitCode::FAILURE;
    };
    // Everyone else lies dead: only the walker moves, and the dead block
    // nothing.
    let mut start = world.snapshot();
    for entity in &mut start.entities {
        if let (Kind::Object(..), Some(fighter), Some(player)) =
            (entity.kind, entity.fighter.as_mut(), entity.player.as_mut())
        {
            fighter.life = sim::combat::Life::Dead;
            player.flags |= motion::motion_flags::DEAD;
        }
    }
    let me = crate::scenes::seat0(&start);
    let origin_of = |floor: [f32; 3]| {
        let [x, y, z] = sim::math::scale(floor, 1.0 / UNIT);
        [x, y, z - motion::SOLE_Z + 0.125]
    };
    let island = (0..mesh.islands)
        .max_by_key(|&island| {
            mesh.faces
                .iter()
                .filter(|face| face.island as usize == island)
                .count()
        })
        .unwrap_or(0);
    // Faces clear of doors and props, where a body stands free.
    let pool: Vec<usize> = (0..mesh.faces.len())
        .filter(|&face| {
            let face = &mesh.faces[face];
            face.island as usize == island && face.entity.is_none() && {
                let half = motion::HALF_WIDTH * UNIT;
                let extents = Extents {
                    mins: [-half, -half, motion::SOLE_Z * UNIT],
                    maxs: [half, half, 40.0 * UNIT],
                };
                let at = sim::math::scale(origin_of(face.centre), UNIT);
                let trace =
                    level.with(None, |world| world.trace(at, at, &extents, Contents::SOLID));
                !trace.start_solid
            }
        })
        .collect();
    let rate = TickRate::default();
    let mut state = seed;
    let (mut arrived, mut ratios, mut stuck) = (0usize, Vec::new(), Vec::new());
    let mut behind = 0usize;
    let mut first_stuck = Vec::new();
    let started = Instant::now();
    let states_of = crate::scenes::door_states;
    let pick = |state: &mut u64| {
        origin_of(mesh.faces[pool[(draw(state) % pool.len() as u64) as usize]].centre)
    };
    for _ in 0..routes {
        // A pair with a way between them (the rest lie behind locked doors).
        let (from, goal, first) = loop {
            let (from, goal) = (pick(&mut state), pick(&mut state));
            if let Some(first) = sim::walk::route(&level, from, goal, &states_of(&start)) {
                break (from, goal, first);
            }
            behind += 1;
        };
        let mut length = 0.0;
        let mut at = from;
        for corner in &first {
            length += collision::nav::flat_length(at, *corner);
            at = *corner;
        }
        let allowed = (3.0 * length / motion::BASE_SPEED * 1000.0) as u64 + 2000;
        let mut snapshot = start.clone();
        snapshot.entities[me].player = Some(motion::MotionState::standing(from, 0.0));
        let mut walker =
            SimWorld::from_snapshot(&snapshot, level.clone()).expect("the walk starts");
        let mut walk = sim::walk::Walker::new(goal);
        let mut last = snapshot;
        let mut timeline = Vec::new();
        let outcome = loop {
            let player = last.entities[me].player.expect("a player");
            if (last.time_ms - start.time_ms).is_multiple_of(200) {
                timeline.push(format!(
                    "{:>6} ms at {:?} v {:?} for {:?}",
                    last.time_ms - start.time_ms,
                    player.origin.map(|v| v.round()),
                    player.velocity.map(|v| v.round()),
                    walk.waypoint.map(|p| p.map(|v| v.round()))
                ));
            }
            if walk.arrived(&player) {
                break Ok(last.time_ms - start.time_ms);
            }
            if last.time_ms - start.time_ms > allowed {
                // What stops it: a body-sized trace at its waypoint, level
                // with it and a step up (over the floor's rises).
                let next = walk.waypoint.unwrap_or(goal);
                let half = motion::HALF_WIDTH * UNIT;
                let extents = Extents {
                    mins: [-half, -half, motion::SOLE_Z * UNIT],
                    maxs: [half, half, player.maxs_z * UNIT],
                };
                let states = level.states_in(&last);
                let at = |p: [f32; 3], rise: f32| {
                    sim::math::scale([p[0], p[1], player.origin[2] + rise], UNIT)
                };
                let [flat, lifted] = [0.0, motion::STEP_HEIGHT].map(|rise| {
                    level.with(states.as_deref(), |world| {
                        world.trace(
                            at(player.origin, rise),
                            at(next, rise),
                            &extents,
                            Contents::SOLID,
                        )
                    })
                });
                break Err((
                    player.origin,
                    format!(
                        "making for {next:?}; a body level with it meets {:?} at {:.2} (normal {:?}, start solid {}); a step up, {:?} at {:.2}",
                        flat.entity,
                        flat.fraction,
                        flat.plane.normal,
                        flat.start_solid,
                        lifted.entity,
                        lifted.fraction
                    ),
                ));
            }
            // Holding use opens the doors in its way, as a player does.
            let mut command = walk.command(&level, &player, last.time_ms, &states_of(&last));
            command.buttons |= sim::buttons::USE;
            let mut input = TickInput::default();
            input.insert(Seat(0), command);
            let tick = last.tick + 1;
            last = step(
                &mut walker,
                tick,
                &input,
                rate.msec(tick),
                StepReason::Authority,
            );
        };
        match outcome {
            Ok(ms) => {
                arrived += 1;
                if length > 64.0 {
                    ratios.push(
                        f64::from(ms as u32)
                            / (f64::from(length) / f64::from(motion::BASE_SPEED) * 1000.0),
                    );
                }
            }
            Err((at, why)) => {
                if stuck.is_empty() {
                    first_stuck = timeline;
                }
                stuck.push((from, goal, at, why));
            }
        }
    }
    ratios.sort_by(f64::total_cmp);
    let median = ratios.get(ratios.len() / 2).copied().unwrap_or(0.0);
    let worst = ratios.last().copied().unwrap_or(0.0);
    outln!(
        "{game} {module}: {arrived} of {routes} walks arrived ({behind} drawn pairs had no way between them: locked doors); time over the route's length at a run: median {median:.2}×, worst {worst:.2}×; stepped in {:.1} s",
        started.elapsed().as_secs_f64()
    );
    for (from, goal, at, why) in stuck.iter().take(5) {
        outln!("    stuck: {from:?} -> {goal:?}, at {at:?} ({why})");
    }
    if !first_stuck.is_empty() {
        outln!("  the first stuck walk, every 200 ms:");
        for line in first_stuck.iter().rev().take(12).rev() {
            outln!("    {line}");
        }
    }
    crate::runtime::write_facts(serde_json::json!({
        "verb": "collision walk", "game": game, "module": module,
        "routes": routes, "behind_locked_doors": behind, "arrived": arrived, "median_ratio": median, "worst_ratio": worst,
        "stuck": stuck.iter().map(|(from, goal, at, why)| serde_json::json!({"from": from, "goal": goal, "at": at, "why": why})).collect::<Vec<_>>(),
    }));
    if arrived == routes {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}
