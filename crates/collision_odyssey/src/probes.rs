//! Probes on a module's own collision (`kjk collision probes k1 end_m01aa`,
//! and the approved test `collision_probes` when an install is there):
//! the entry's floor and walls, every door closed and open, the tree
//! against tracing every brush, and what 100 000 traces cost and answer.

use std::time::Instant;

use asset_odyssey::module::{AreaIr, Kind, ModuleIr};
use collision::probes::{Draw, Fnv, Probes, near, player};
use collision::{CollisionWorld, Contents, Extents, Trace};

use crate::{AreaCollision, Surfaces};

/// One run outward from the entry.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Ray {
    /// Degrees from +X.
    pub bearing: u32,
    pub trace: Trace,
    /// How far it got, metres (of 40).
    pub distance: f32,
}

/// A run through a door's doorway, the door closed and open.
#[derive(Debug, Clone, PartialEq)]
pub struct DoorRun {
    pub entity: u32,
    pub tag: String,
    pub model: String,
    pub closed: Trace,
    pub open: Trace,
}

/// What the module probes measured besides pass and fail.
#[derive(Debug, Clone, PartialEq)]
pub struct ModuleFacts {
    pub entry: [f32; 3],
    /// Where the player dropped at the entry came to rest.
    pub standing: [f32; 3],
    pub ring: Vec<Ray>,
    pub doors: Vec<DoorRun>,
    /// The cost run: traces, microseconds each, how many hit, and the
    /// digest of their answers.
    pub traces: u32,
    pub trace_us: f64,
    pub hits: usize,
    pub traces_digest: String,
    /// `Level::digest`.
    pub digest: String,
}

/// The module probes over an area's collision. Doors are switched and put
/// back closed.
pub fn module(
    ir: &ModuleIr,
    area: &AreaIr,
    surfaces: &Surfaces,
    collision: &mut AreaCollision,
) -> (Probes, ModuleFacts) {
    let mut probes = Probes::default();
    let player = player();
    let feet = player.mins[2];
    let entry = ir.entry_position;
    let level = &collision.level;
    let start = [entry[0], entry[1], entry[2] + 1.0];
    let drop = level.trace(
        start,
        [entry[0], entry[1], entry[2] - 10.0],
        &player,
        Contents::PLAYER_SOLID,
    );
    let walkable = surfaces.walkable(u32::from(drop.surface.material));
    probes.check(
        "entry drop",
        "a player dropped at the module's entry point lands on a walkable walkmesh material, near the entry's height",
        Some(drop),
        drop.fraction < 1.0 && walkable && near(drop.end[2] + feet, entry[2], 0.3),
    );
    let standing = drop.end;
    let mut ring = Vec::new();
    let mut all_hit = true;
    for k in 0..16 {
        let (sin, cos) = collision::trig::sin_cos(k as f32 * std::f32::consts::PI / 8.0);
        let to = [
            standing[0] + cos * 40.0,
            standing[1] + sin * 40.0,
            standing[2],
        ];
        let t = level.trace(standing, to, &player, Contents::PLAYER_SOLID);
        all_hit &= t.fraction < 1.0 && !t.start_solid;
        ring.push(Ray {
            bearing: k * 45 / 2,
            trace: t,
            distance: t.fraction * 40.0,
        });
    }
    probes.check(
        "entry ring",
        "16 runs of 40 m outward from the entry each stop at a wall",
        None,
        all_hit,
    );
    // Every door: a player runs through its doorway (along the door's own
    // Y, the way through) closed and open. Where the open run gets past the
    // door, the closed one must stop at the door; some doorways lead
    // nowhere (a wall behind the door), and are counted apart.
    let mut doors = Vec::new();
    let entities: Vec<_> = collision
        .entities
        .iter()
        .filter(|e| e.kind == Kind::Door)
        .cloned()
        .collect();
    let mut passable = 0;
    let mut blocked_closed = 0;
    let mut clear_open = 0;
    for entity in &entities {
        let instance = &area.instances[entity.entity as usize];
        let (sin, cos) = crate::turn(instance.direction);
        // The door's local +Y in the world.
        let through = [-sin, cos];
        let p = instance.position;
        let z = p[2] - feet + 0.3;
        let from = [p[0] - through[0] * 1.5, p[1] - through[1] * 1.5, z];
        let to = [p[0] + through[0] * 1.5, p[1] + through[1] * 1.5, z];
        let level = &mut collision.level;
        level.set_state(entity.entity, 0);
        let closed = level.trace(from, to, &player, Contents::PLAYER_SOLID);
        level.set_state(entity.entity, 1);
        let open = level.trace(from, to, &player, Contents::PLAYER_SOLID);
        level.set_state(entity.entity, 0);
        let passes = open.fraction > 0.5 && !open.start_solid;
        if passes {
            passable += 1;
            blocked_closed += usize::from(closed.entity == Some(entity.entity));
            clear_open += usize::from(open.fraction == 1.0 || open.entity != Some(entity.entity));
        }
        doors.push(DoorRun {
            entity: entity.entity,
            tag: entity.tag.clone(),
            model: entity.model.clone(),
            closed,
            open,
        });
    }
    probes.check(
        "doors",
        &format!(
            "running through the doorways of the {passable} of {} doors whose doorway leads on: closed, the door stops the player ({blocked_closed}); open, it does not ({clear_open})",
            entities.len()
        ),
        None,
        passable > 0 && blocked_closed == passable && clear_open == passable,
    );
    // Cost: random player traces of up to 4 m inside the walkmesh's bounds.
    let (min, max) = collision.walkable.iter().flat_map(|(c, _)| c.iter()).fold(
        ([f32::INFINITY; 3], [f32::NEG_INFINITY; 3]),
        |(lo, hi), p| {
            (
                std::array::from_fn(|i| lo[i].min(p[i])),
                std::array::from_fn(|i| hi[i].max(p[i])),
            )
        },
    );
    let level = &collision.level;
    let segment = |draw: &mut Draw, reach: f32| {
        let start = [
            draw.range(min[0], max[0]),
            draw.range(min[1], max[1]),
            draw.range(min[2], max[2] + 2.0),
        ];
        let end = [
            start[0] + draw.range(-reach, reach),
            start[1] + draw.range(-reach, reach),
            start[2] + draw.range(-reach / 2.0, reach / 2.0),
        ];
        (start, end)
    };
    // The tree against every brush in order: the same answer, bit for bit.
    let mut draw = Draw(7);
    let (mut same, total) = (0, 2_000);
    let mut first_difference = None;
    for i in 0..total {
        let (start, end) = segment(&mut draw, 8.0);
        let extents = if i % 2 == 0 { player } else { Extents::POINT };
        let a = level.trace(start, end, &extents, Contents::PLAYER_SOLID);
        let b = level.trace_every_brush(start, end, &extents, Contents::PLAYER_SOLID);
        if a == b {
            same += 1;
        } else if first_difference.is_none() {
            first_difference = Some(a);
        }
    }
    probes.check(
        "tree differential",
        &format!(
            "{total} random player and point traces up to 8 m through the module: the tree gives the answer of tracing every brush in order, every field bit for bit ({same} of {total})"
        ),
        first_difference,
        same == total,
    );
    // What 100 000 traces answer, digested: the same on every build and
    // machine that builds the same level.
    let mut draw = Draw(99);
    let count = 100_000;
    let started = Instant::now();
    let mut hits = 0;
    let mut digest = Fnv::new();
    for _ in 0..count {
        let (start, end) = segment(&mut draw, 4.0);
        let t = level.trace(start, end, &player, Contents::PLAYER_SOLID);
        hits += usize::from(t.hit());
        digest.trace(&t);
    }
    let trace_us = started.elapsed().as_secs_f64() * 1e6 / f64::from(count);
    let facts = ModuleFacts {
        entry,
        standing,
        ring,
        doors,
        traces: count,
        trace_us,
        hits,
        traces_digest: format!("{:016x}", digest.0),
        digest: collision.level.digest(),
    };
    (probes, facts)
}

/// Checks on the surface materials table: the grass column is read per
/// material. Run by the approved test `surface_probes`.
pub fn probes() -> Vec<(&'static str, Result<String, String>)> {
    let check = |ok: bool, detail: String| -> Result<String, String> {
        if ok { Ok(detail) } else { Err(detail) }
    };
    let mut checks = Vec::new();

    let table = fmt_odyssey::twoda::TwoDa {
        columns: ["label", "walk", "walkcheck", "lineofsight", "grass"]
            .map(|c| c.as_bytes().to_vec())
            .to_vec(),
        row_labels: [b"0".to_vec(), b"1".to_vec(), b"2".to_vec()].to_vec(),
        cells: [
            ["Dirt", "1", "1", "1", "0"],
            ["Grass", "1", "1", "1", "1"],
            ["Stone", "1", "1", "1", "****"],
        ]
        .map(|row| row.map(|cell| cell.as_bytes().to_vec()).to_vec())
        .to_vec(),
        column_separator: b'\t',
        row_separator: b'\t',
    };
    let grass = Surfaces::from_2da(&table).grass();
    checks.push((
        "surfaces_grass_column",
        check(grass == [false, true, false], format!("{grass:?}")),
    ));

    checks.push(("edge_walls_slight_bend", slight_bend()));
    checks.push(("edge_walls_wide_turn", wide_turn()));
    checks.push(("edge_walls_turn_back", turn_back()));
    checks.push(("footprint_walls", footprint_walls()));
    checks.push(("footprint_nav_marks", footprint_nav_marks()));
    checks.push(("sliver_nav_open", sliver_nav_open()));
    checks.push((
        "nav_past_gate_between_corners",
        nav_past_gate_between_corners(),
    ));
    checks.push(("additive_texture_not_met", additive_texture()));
    checks
}

/// A room mesh whose texture blends additively is drawn, never met
/// (`Loader::additive`, read by `build_area`): Korriban's secret tomb hangs
/// such a cloud of light (`korfx_clouds`) along the minefield of Dxun's
/// vision. One with its own blending, or with none, is met.
fn additive_texture() -> Result<String, String> {
    let files = asset_odyssey::made::Files(
        [
            ("cloud.txi", "blending additive\n"),
            ("leaves.txi", "blending punchthrough\n"),
            ("wall.txi", "decal 0\n"),
        ]
        .map(|(name, text)| (name.to_string(), text.as_bytes().to_vec()))
        .into(),
    );
    let loader = crate::Loader { source: &files };
    let seen = ["cloud", "leaves", "wall", "none"].map(|name| loader.additive(name));
    let what = format!("{seen:?}");
    if seen == [true, false, false, false] {
        Ok(what)
    } else {
        Err(what)
    }
}

/// Surfaces with row 1 walkable, for made-up walkmeshes.
fn test_surfaces() -> Surfaces {
    Surfaces {
        rows: [false, true]
            .map(|walk| crate::SurfaceMat {
                label: String::new(),
                walk,
                walk_check: true,
                line_of_sight: true,
                grass: false,
            })
            .to_vec(),
    }
}

/// A level of the edge walls of `walkable` (material 1 faces at any
/// height), 10 m high.
fn walls_of(walkable: &[([[f32; 3]; 3], u32)]) -> collision::Level {
    use collision::{LevelBuilder, Tag};
    let mut floor = LevelBuilder::new();
    for (corners, material) in walkable {
        floor.triangle(*corners, Tag::level(Contents::SOLID, *material as u16));
    }
    let floor = floor.build();
    let options = crate::EdgeWalls { height: 10.0 };
    let mut level = LevelBuilder::new();
    for wall in crate::edges::walls(walkable, &floor, &test_surfaces(), &options) {
        wall.add_to(&mut level, Tag::level(Contents::WALK_CLIP, 0));
    }
    level.build()
}

/// A corner's end set back by the tangent of half its turn, from straight
/// on to straight back (a sliver face's corners in a line): 0 for none, 1
/// for a right angle and on, straight back too, never under 0 nor over 1.
/// Straight back gave -2.3e7 in f32, a wall's end 9,860 km off across K1's
/// Korriban academy.
fn turn_back() -> Result<String, String> {
    let into = [1.0f32, 0.0];
    let turns: Vec<(f32, f32)> = [0.0f32, 45.0, 90.0, 135.0, 179.9, 180.0]
        .into_iter()
        .map(|degrees| {
            let r = degrees.to_radians();
            (degrees, crate::edges::half_turn(into, [r.cos(), r.sin()]))
        })
        .collect();
    let detail = turns
        .iter()
        .map(|(degrees, t)| format!("{degrees}: {t:.3}"))
        .collect::<Vec<_>>()
        .join(", ");
    let right = |degrees: f32, t: f32| {
        (0.0..=1.0).contains(&t) && (degrees < 90.0 || (t - 1.0).abs() < 1e-3)
    };
    if turns[0].1.abs() < 1e-6 && turns.iter().all(|&(d, t)| right(d, t)) {
        Ok(detail)
    } else {
        Err(detail)
    }
}

/// A floor whose open edge turns 45 degrees out round a corner (TSL
/// 710kor's path at 200.9, 138.6): a player walks along the second edge
/// past the corner, the first edge's wall ending where the two walls meet.
/// Run on by a flat half width, it reached into the floor there and walks
/// stuck on it.
fn wide_turn() -> Result<String, String> {
    let [a, b, c] = [[4.0, 0.0, 0.0], [0.0, 0.0, 0.0], [-3.0, 3.0, 0.0f32]];
    let [f, d] = [[-3.0, -4.0, 0.0], [4.0, -4.0, 0.0f32]];
    let walkable = vec![([a, b, d], 1), ([b, f, d], 1), ([b, c, f], 1)];
    let level = walls_of(&walkable);
    let box_ = player();
    let z = -box_.mins[2] + 0.05;
    // Along B to C, 0.2 m in from it (the square box's corner reaches out
    // to the wall there, its sides then clear of it).
    let along = [
        -std::f32::consts::FRAC_1_SQRT_2,
        std::f32::consts::FRAC_1_SQRT_2,
    ];
    let inward = [0.2 * along[0], -0.2 * along[1]];
    let start = [0.3 * along[0] + inward[0], 0.3 * along[1] + inward[1], z];
    let end = [start[0] + 1.5 * along[0], start[1] + 1.5 * along[1], z];
    let run = level.trace(start, end, &box_, Contents::PLAYER_SOLID);
    let detail = format!(
        "a walk along the turned edge from its corner gets {:.2} of 1.5 m (start solid {})",
        run.fraction * 1.5,
        run.start_solid
    );
    if run.fraction >= 1.0 && !run.start_solid {
        Ok(detail)
    } else {
        Err(detail)
    }
}

/// A floor whose open edge bends a little at a corner (heading down 9
/// degrees, a 0.11 m edge, then straight on: TSL 601dan's outline at
/// 252.7, 124.7): the walls of the edges meet, so a player run at the
/// corner from the floor is stopped by one. Read as corners the floor
/// wraps round, the walls had stopped half a box short of them and left a
/// gap a player walked through.
fn slight_bend() -> Result<String, String> {
    // The outline's open side is +Y: A to B heads down, B to B' and B' to
    // C go on.
    let [a, b, b2, c] = [
        [1.38, 4.88, 0.0],
        [2.69, 4.68, 0.0],
        [2.8, 4.67, 0.0],
        [23.31, 4.63, 0.0],
    ];
    let [d, e] = [[23.31, 0.0, 0.0], [1.38, 0.0, 0.0f32]];
    let walkable = vec![
        ([a, b, e], 1),
        ([b, b2, e], 1),
        ([b2, d, e], 1),
        ([b2, c, d], 1),
    ];
    let level = walls_of(&walkable);
    let box_ = player();
    let z = -box_.mins[2] + 0.05;
    // Runs north from the floor across the bend, a few centimetres apart.
    let mut through = Vec::new();
    for k in 0..=40 {
        let x = 2.0 + k as f32 * 0.04;
        let run = level.trace([x, 3.0, z], [x, 8.0, z], &box_, Contents::PLAYER_SOLID);
        if run.fraction >= 1.0 || run.end[1] > 4.9 {
            through.push(format!("x {x:.2} to y {:.2}", run.end[1]));
        }
    }
    if through.is_empty() {
        Ok("41 runs north over the bend each stop at a wall by the edge".into())
    } else {
        Err(format!("runs got past the edge: {}", through.join(", ")))
    }
}

/// Made-up placeable footprints flat on the floor (`footprints`): a 2 m
/// square, a 4 m spike a metre wide at its base (a swoop bike's nose) and
/// a 0.3 m square (a jar's). A player's box run at each from every side
/// stops, its centre at the outline as KOTOR's point would be, within a
/// few centimetres; at the spike's side, where it is narrower than a box,
/// up to a fifth of a metre out (its walls set back cross there); at the
/// small square, a box out from its middle. A body walked along a 0.15 m
/// arm a hand clear of it passes (its walls set back would stand out
/// beside it: Manaan's broken droid boxed the player into the pocket
/// between two).
fn footprint_walls() -> Result<String, String> {
    use collision::{LevelBuilder, Tag};
    let square = |x: f32, y: f32, half: f32| {
        let [a, b, c, d] = [
            [x - half, y - half],
            [x + half, y - half],
            [x + half, y + half],
            [x - half, y + half],
        ]
        .map(|[x, y]| [x, y, 0.0f32]);
        vec![[a, b, c], [a, c, d]]
    };
    let spike = vec![[[10.0, -0.5, 0.0], [14.0, 0.0, 0.0], [10.0, 0.5, 0.0f32]]];
    let floor = LevelBuilder::new().build();
    let options = crate::EdgeWalls { height: 10.0 };
    let mut level = LevelBuilder::new();
    // A 1.5 m arm 0.15 m wide (a broken droid's).
    let arm = {
        let [a, b, c, d] = [[20.0, -0.075], [21.5, -0.075], [21.5, 0.075], [20.0, 0.075]]
            .map(|[x, y]| [x, y, 0.0f32]);
        vec![[a, b, c], [a, c, d]]
    };
    for faces in [square(0.0, 0.0, 1.0), spike, square(-10.0, 0.0, 0.15), arm] {
        for wall in crate::footprints::walls(&faces, &floor, &test_surfaces(), &options) {
            wall.add_to(&mut level, Tag::level(Contents::WALK_CLIP, 0));
        }
    }
    let level = level.build();
    let box_ = player();
    let z = -box_.mins[2] + 0.05;
    // Each run: from, to, and where along it the centre should stop, with
    // how far short of that it may.
    let runs: [([f32; 2], [f32; 2], f32, f32); 7] = [
        ([-3.0, 0.3], [3.0, 0.3], -1.0, 0.06),
        ([0.4, 3.0], [0.4, -3.0], 1.0, 0.06),
        ([3.0, -0.7], [-3.0, -0.7], 1.0, 0.06),
        ([-0.9, -3.0], [-0.9, 3.0], -1.0, 0.06),
        ([17.0, 0.0], [11.0, 0.0], 14.0, 0.06),
        ([12.0, 3.0], [12.0, -3.0], 0.25, 0.2),
        ([-13.0, 0.0], [-7.0, 0.0], -10.15, 0.45),
    ];
    let mut wrong = Vec::new();
    // Along the arm, a hand clear of it: no wall stands out beside it.
    let beside = 0.075 + box_.maxs[1] + 0.07;
    let along = level.trace(
        [19.0, beside, z],
        [22.5, beside, z],
        &box_,
        Contents::PLAYER_SOLID,
    );
    if along.fraction < 1.0 || along.start_solid {
        wrong.push(format!(
            "along the arm {beside:.2} m out stopped at {:.2}",
            along.end[0]
        ));
    }
    for (from, to, stop, slack) in runs {
        let run = level.trace(
            [from[0], from[1], z],
            [to[0], to[1], z],
            &box_,
            Contents::PLAYER_SOLID,
        );
        let axis = usize::from(from[0] == to[0]);
        let ahead = (to[axis] - from[axis]).signum();
        let short = (stop - run.end[axis]) * ahead;
        if run.start_solid || run.fraction >= 1.0 || !(-0.01..=slack).contains(&short) {
            wrong.push(format!(
                "{from:?} to {to:?} stopped {short:.2} m short of {stop}"
            ));
        }
    }
    if wrong.is_empty() {
        Ok("7 runs at three footprints each stop with the centre at the outline; a walk beside an arm passes".into())
    } else {
        Err(wrong.join("; "))
    }
}

/// The navigation mesh over a placeable's footprint
/// (`footprints::cut`, `collision::nav::NavMesh::mark`): a 2 m by 5 m
/// footprint standing on part of a 12 m street's floor face. The street is
/// not shut (Taris's lower city, where a junked swoop's walls had shut the
/// street: its face carried the placeable), and a route across it goes
/// round the footprint, never over it (Manaan's Sith base, where a route
/// ran straight over a broken droid's footprint into its walls).
fn footprint_nav_marks() -> Result<String, String> {
    use collision::{LevelBuilder, Tag};
    let square = |x0: f32, y0: f32, x1: f32, y1: f32| {
        let [a, b, c, d] = [[x0, y0], [x1, y0], [x1, y1], [x0, y1]].map(|[x, y]| [x, y, 0.0f32]);
        vec![[a, b, c], [a, c, d]]
    };
    let street = square(0.0, 0.0, 12.0, 12.0);
    let print = square(5.0, 3.0, 7.0, 8.0);
    let mut floor = LevelBuilder::new();
    let mut level = LevelBuilder::new();
    for corners in &street {
        floor.triangle(*corners, Tag::level(Contents::SOLID, 1));
        level.triangle(*corners, Tag::level(Contents::SOLID, 1));
    }
    let floor = floor.build();
    let tag = |contents| Tag {
        contents,
        surface: collision::Surface { material: 0 },
        entity: Some(1),
        states: 1,
    };
    for corners in &print {
        level.triangle(*corners, tag(Contents::SOLID | Contents::NONWALK));
    }
    let options = crate::EdgeWalls { height: 10.0 };
    for wall in crate::footprints::walls(&print, &floor, &test_surfaces(), &options) {
        wall.add_to(&mut level, tag(Contents::WALK_CLIP));
    }
    let level = level.build();
    let faces = crate::footprints::cut(&street, &print);
    let mut nav = collision::nav::NavMesh::build(&faces, 0.5);
    nav.mark(&level);
    let shut = nav
        .faces
        .iter()
        .filter(|face| face.entity.is_some())
        .count();
    let route = nav.route_round(
        [6.0, 1.0, 0.0],
        [6.0, 11.0, 0.0],
        false,
        &|_| false,
        &|_| false,
    );
    let Some(route) = route else {
        return Err(format!(
            "{} faces, {shut} under the footprint: no way across the street",
            faces.len()
        ));
    };
    // Each leg sampled every 5 cm: none inside the footprint.
    let mut points = vec![[6.0, 1.0, 0.0f32]];
    points.extend(&route);
    let over = points.windows(2).any(|leg| {
        (0..=200).any(|k| {
            let t = k as f32 / 200.0;
            let p: [f32; 2] = std::array::from_fn(|i| leg[0][i] + (leg[1][i] - leg[0][i]) * t);
            p[0] > 5.01 && p[0] < 6.99 && p[1] > 3.01 && p[1] < 7.99
        })
    });
    let detail = format!(
        "{} faces, {shut} under the footprint; the way across by {} corners",
        faces.len(),
        route.len()
    );
    if shut > 0 && !over {
        Ok(detail)
    } else {
        Err(format!("{detail} crosses the footprint"))
    }
}

/// The navigation mesh over a sliver (`collision::nav::NavMesh::mark`): a
/// west room (x 0 to 4 m) and an east one (x 4 to 8 m, y 4 to 8 m, a rock
/// wall below it) whose floors meet only through a face 3 mm wide between
/// them, running along the wall's face, as TSL's Peragus mining tunnels
/// join the hallway past the damaged droid to the cave. The column the
/// furniture check looks over each point of the sliver with meets the wall
/// along its edge; the sliver is still open, so a way leads from one room
/// to the other.
fn sliver_nav_open() -> Result<String, String> {
    use collision::{LevelBuilder, Tag};
    let at = |x: f32, y: f32| [x, y, 0.0f32];
    let (p, q, r) = (at(4.0, 0.0), at(4.0, 8.0), at(4.003, 4.0));
    let faces = vec![
        // The west room, its east edge the sliver's long one.
        [at(0.0, 0.0), p, q],
        [at(0.0, 0.0), q, at(0.0, 8.0)],
        // The sliver.
        [p, r, q],
        // The east room, its west edge the sliver's upper short one.
        [r, at(8.0, 4.0), at(8.0, 8.0)],
        [r, at(8.0, 8.0), q],
    ];
    let mut level = LevelBuilder::new();
    for corners in &faces {
        level.triangle(*corners, Tag::level(Contents::SOLID, 1));
    }
    // The rock beside the sliver's lower short edge.
    level.aabb(
        [4.0, 0.0, 0.0],
        [8.0, 4.0, 4.0],
        Tag::level(Contents::SOLID, 0),
    );
    let level = level.build();
    let mut nav = collision::nav::NavMesh::build(&faces, 0.5);
    nav.mark(&level);
    let covered = nav.faces.iter().filter(|face| face.covered).count();
    let route = nav.route([1.0, 4.0, 0.0], [6.0, 6.5, 0.0], false, &|_| false);
    let detail = format!("{} faces, {covered} covered", nav.faces.len());
    match route {
        Some(route) if covered == 0 => Ok(format!(
            "{detail}; the way from room to room by {} corners",
            route.len()
        )),
        _ => Err(format!("{detail}: no way from room to room")),
    }
}

/// The floor of TSL's Peragus hangar bay start room (106PER) before the
/// lower door, as the bake makes it: a fan of long faces between two
/// blocks' corners, the portals' ends stepped off the walls as the bake
/// steps them. A walk from a spot on the portal between the two corners,
/// past its stepped gate and two metres and more from either corner, goes
/// straight to the doorway: stepped off along the corners' bisectors, the
/// ends lay behind the walker, and the funnel sent it back round one and
/// then the other, (4.07, 71.91), (4.08, 71.30), (-3.11, 68.33), and the
/// walk turned back and forth between the two ways.
fn nav_past_gate_between_corners() -> Result<String, String> {
    let faces: Vec<[[f32; 3]; 3]> = vec![
        [
            [-2.250001, 65.36038, 9.344971],
            [-2.254681, 64.38335, 9.34506],
            [2.254674, 64.38335, 9.34506],
        ],
        [
            [-3.622631, 67.79607, 9.344871],
            [-3.584011, 65.34815, 9.34496],
            [-2.250001, 65.36038, 9.344971],
        ],
        [
            [2.251145, 65.374664, 9.344971],
            [4.724559, 66.06493, 9.34496],
            [4.599969, 71.394905, 9.344871],
        ],
        [
            [4.599969, 71.394905, 9.344871],
            [-3.622631, 67.79607, 9.344871],
            [2.251145, 65.374664, 9.344971],
        ],
        [
            [2.251145, 65.374664, 9.344971],
            [-3.622631, 67.79607, 9.344871],
            [-2.250001, 65.36038, 9.344971],
        ],
        [
            [-2.250001, 65.36038, 9.344971],
            [2.254674, 64.38335, 9.34506],
            [2.251145, 65.374664, 9.344971],
        ],
        [
            [-4.301151, 69.52805, 9.346041],
            [-4.713741, 67.77001, 9.34463],
            [4.599969, 71.394905, 9.344871],
        ],
        [
            [-4.713741, 67.77001, 9.34463],
            [-4.301151, 69.52805, 9.346041],
            [-4.717761, 68.55516, 9.34576],
        ],
        [
            [-3.622631, 67.79607, 9.344871],
            [4.599969, 71.394905, 9.344871],
            [-4.713741, 67.77001, 9.34463],
        ],
        [
            [-4.8982615, 71.29964, 9.349582],
            [4.149571, 71.300446, 9.34493],
            [4.599969, 71.394905, 9.344871],
        ],
        [
            [-4.898169, 70.207726, 9.347589],
            [-4.301151, 69.52805, 9.346041],
            [4.149571, 71.300446, 9.34493],
        ],
        [
            [-4.898169, 70.207726, 9.347589],
            [4.149571, 71.300446, 9.34493],
            [-4.8982615, 71.29964, 9.349582],
        ],
        [
            [-4.8982615, 71.29964, 9.349582],
            [4.599969, 71.394905, 9.344871],
            [-4.898306, 71.82357, 9.350538],
        ],
        [
            [-0.07020235, 71.605675, 9.347657],
            [4.599969, 71.394905, 9.344871],
            [1.4687049, 73.09409, 9.34805],
        ],
        // Stand-ins for the hall north and east of the east block.
        [
            [4.599969, 71.394905, 9.344871],
            [9.0, 71.4, 9.345],
            [4.6, 75.0, 9.345],
        ],
        [
            [4.599969, 71.394905, 9.344871],
            [4.6, 75.0, 9.345],
            [1.4687049, 73.09409, 9.34805],
        ],
    ];
    // Where the whole room's bake steps each wall corner (its walls reach
    // past these faces).
    let stepped: [([f32; 2], [f32; 2]); 10] = [
        ([-2.250001, 65.36038], [-1.7348809, 65.87776]),
        ([-2.254681, 64.38335], [-1.7442496, 64.382965]),
        ([-3.622631, 67.79607], [-3.110983, 68.328415]),
        ([-4.301151, 69.52805], [-3.4207606, 69.823044]),
        ([-4.713741, 67.77001], [-4.1964784, 68.30249]),
        ([-4.898169, 70.207726], [-4.6997395, 70.55463]),
        ([2.251145, 65.374664], [1.6235957, 65.849304]),
        ([2.254674, 64.38335], [1.7448657, 64.38328]),
        ([4.599969, 71.394905], [4.0679455, 71.914635]),
        ([4.149571, 71.300446], [4.0823383, 71.298874]),
    ];
    let step = |p: [f32; 3]| {
        stepped
            .iter()
            .find(|(bare, _)| bare[0] == p[0] && bare[1] == p[1])
            .map_or(p, |(_, to)| [to[0], to[1], p[2]])
    };
    let mut nav = collision::nav::NavMesh::build(&faces, 0.508);
    for face in &mut nav.faces {
        for portal in &mut face.portals {
            portal.a_round = step(portal.a);
            portal.b_round = step(portal.b);
        }
    }
    let to = [0.0, 64.6, 9.35];
    let route = nav.route([-1.58, 68.70, 9.35], to, false, &|_| true);
    match route {
        Some(route) if route == [to] => Ok("straight to the doorway".into()),
        other => Err(format!("the way turned back: {other:?}")),
    }
}
