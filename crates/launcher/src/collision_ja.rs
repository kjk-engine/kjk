//! `kjk collision ja LEVEL`: a Jedi Academy level's collision and floor
//! (`asset_ja::level_collision`) from the command line, with no window.
//! `LEVEL` is a level's name (`t1_sour`, `mp/ffa3`) or `all`, for every
//! level in the game's archives.
//!
//! It says what the collision was built from, bakes the floor's navigation
//! mesh, and tests both against the places the level itself marks:
//!
//! - **Starts**: where a player starts (`info_player_start`,
//!   `info_player_deathmatch` and the duel starts).
//! - **Waypoints**: where the level's makers placed its characters' paths
//!   (`waypoint`, `waypoint_small`, `waypoint_navgoal` and its sizes, and
//!   `point_combat`).
//!
//! A place is tested with the upper part of a standing body: the box from
//! the place up to the head. A level's makers put a place at a body's
//! middle or lower, often with the feet a little in the floor, and the game
//! lets the body down onto the floor from there. A place passes three
//! tests. The upper body fits at it. Let down, it meets a floor within
//! [`DROP`] units. And where its soles land is on the navigation mesh. A
//! place that fails points at collision or floor that is missing or wrong.
//!
//! The mesh is in islands: pieces no walk on foot joins. The verb also says
//! how many of the places on the mesh are in the one island that holds the
//! most of them. Stairs join their treads, but doors, lifts and jumps do
//! not join floors yet. Writes `facts.json`, and for one level `mesh.json`:
//! the mesh's faces and the places, each with its island, which
//! `tools/nav-picture.py` draws as seen from above.

use std::process::ExitCode;
use std::time::Instant;

use asset_ja::level_collision::LevelCollision;
use collision::nav::NavMesh;
use collision::{CollisionWorld, Contents, Extents, UNIT};
use fmt_ja::bsp::Bsp;
use crate::viewer_ja::{self, Files};
use serde_json::json;
use vfs::Discovery;

/// How far under a place its floor is looked for, in units. Jedi Academy
/// drops a waypoint to the floor from where it was placed.
const DROP: f32 = 128.0;

/// How far over and under where a body lands the navigation mesh is
/// looked for, in metres.
const NAV_REACH: f32 = 0.5;

const STARTS: [&str; 5] = [
    "info_player_start",
    "info_player_deathmatch",
    "info_player_duel",
    "info_player_duel1",
    "info_player_duel2",
];

fn is_waypoint(class: &str) -> bool {
    class.starts_with("waypoint") || class == "point_combat"
}

/// How one kind of place fared.
#[derive(Default, Clone, Copy)]
struct Places {
    all: usize,
    fit: usize,
    floored: usize,
    on_mesh: usize,
    /// Of those on the mesh, the ones in the island that holds the most.
    together: usize,
}

impl Places {
    fn add(&mut self, other: Places) {
        self.all += other.all;
        self.fit += other.fit;
        self.floored += other.floored;
        self.on_mesh += other.on_mesh;
        self.together += other.together;
    }

    fn line(&self) -> String {
        format!(
            "{} ({} fit, {} over a floor, {} on the mesh, {} of those in one island)",
            self.all, self.fit, self.floored, self.on_mesh, self.together
        )
    }

    fn facts(&self) -> serde_json::Value {
        json!({
            "all": self.all, "fit": self.fit, "floored": self.floored,
            "on_mesh": self.on_mesh, "together": self.together,
        })
    }
}

/// One level's results.
struct Checked {
    built: LevelCollision,
    mesh: NavMesh,
    /// The mesh's faces in its largest island.
    largest: usize,
    build_ms: f64,
    bake_ms: f64,
    starts: Places,
    waypoints: Places,
    /// Each place on the mesh (starts and waypoints): its island and where
    /// its soles land (metres).
    landed: Vec<(u32, [f32; 3])>,
}

fn check(bsp: &Bsp) -> Checked {
    let started = Instant::now();
    let built = asset_ja::level_collision::collision(bsp);
    let build_ms = started.elapsed().as_secs_f64() * 1000.0;
    let started = Instant::now();
    let mesh = built.nav(sim::walk::CLEARANCE * UNIT);
    let bake_ms = started.elapsed().as_secs_f64() * 1000.0;
    let mut sizes = vec![0usize; mesh.islands];
    for face in &mesh.faces {
        sizes[face.island as usize] += 1;
    }

    let half = collision::player::HALF_WIDTH * UNIT;
    let upper = Extents {
        mins: [-half, -half, 0.0],
        maxs: [half, half, collision::player::STAND_HEIGHT * UNIT],
    };
    let (mut starts, mut waypoints) = (Places::default(), Places::default());
    // The islands the places of each kind stand on.
    let (mut start_islands, mut waypoint_islands) = (Vec::new(), Vec::new());
    let mut landed = Vec::new();
    for entity in &bsp.entities {
        let class = entity.class();
        let (places, islands) = if STARTS.contains(&class) {
            (&mut starts, &mut start_islands)
        } else if is_waypoint(class) {
            (&mut waypoints, &mut waypoint_islands)
        } else {
            continue;
        };
        let Some(origin) = entity.vector("origin") else {
            continue;
        };
        let at = origin.map(|part| part * UNIT);
        let under = [at[0], at[1], at[2] - DROP * UNIT];
        places.all += 1;
        let down = built.level.trace(at, under, &upper, Contents::PLAYER_SOLID);
        if down.start_solid {
            continue;
        }
        places.fit += 1;
        if down.fraction >= 1.0 {
            continue;
        }
        places.floored += 1;
        if let Some(face) = mesh.locate(down.end, NAV_REACH, NAV_REACH) {
            places.on_mesh += 1;
            islands.push(mesh.faces[face as usize].island);
            landed.push((mesh.faces[face as usize].island, down.end));
        }
    }
    let most = |islands: &[u32]| {
        let mut counts = std::collections::BTreeMap::new();
        for island in islands {
            *counts.entry(island).or_insert(0usize) += 1;
        }
        counts.into_values().max().unwrap_or(0)
    };
    starts.together = most(&start_islands);
    waypoints.together = most(&waypoint_islands);
    Checked {
        built,
        largest: sizes.into_iter().max().unwrap_or(0),
        mesh,
        build_ms,
        bake_ms,
        starts,
        waypoints,
        landed,
    }
}

fn facts(name: &str, checked: &Checked) -> serde_json::Value {
    let tally = &checked.built.tally;
    let stats = checked.built.level.stats();
    json!({
        "level": name,
        "brushes": {
            "solid": tally.solid, "player_clip": tally.player_clip,
            "other_contents": tally.other_contents, "doors": tally.doors,
            "no_solid": tally.no_solid, "curve_triangles": tally.curve_triangles,
        },
        "collision": {
            "brushes": stats.brushes, "planes": stats.planes, "depth": stats.depth,
            "digest": checked.built.level.digest(), "build_ms": checked.build_ms,
        },
        "nav": {
            "floor": checked.built.floor.len(), "faces": checked.mesh.faces.len(),
            "portals": checked.mesh.portals(), "islands": checked.mesh.islands,
            "largest_island": checked.largest, "bake_ms": checked.bake_ms,
        },
        "starts": checked.starts.facts(),
        "waypoints": checked.waypoints.facts(),
    })
}

pub fn run(discovery: &Discovery, level: &str) -> ExitCode {
    let stack = match viewer_ja::stack(discovery) {
        Ok(stack) => stack,
        Err(error) => {
            eprintln!("{error}");
            return ExitCode::FAILURE;
        }
    };
    let files = Files::new(&stack);
    let every = level.eq_ignore_ascii_case("all");
    let paths: Vec<String> = if every {
        let mut paths: Vec<String> = (files.names())
            .filter(|name| name.starts_with("maps/") && name.ends_with(".bsp"))
            .map(str::to_string)
            .collect();
        paths.sort();
        paths
    } else {
        vec![viewer_ja::level_path(level)]
    };

    let mut levels = Vec::new();
    let (mut starts, mut waypoints) = (Places::default(), Places::default());
    let mut failed = 0;
    // The places off the main piece, over every level, by how their piece
    // sits (`pieces`).
    let mut apart: std::collections::BTreeMap<Apart, (usize, usize, usize)> = Default::default();
    let mut mesh_file = None;
    for path in &paths {
        let parsed = files
            .read(path)
            .ok_or(format!("{path}: not in Jedi Academy's archives"))
            .and_then(|bytes| fmt_ja::bsp::parse(&bytes).map_err(|error| format!("{path}: {error}")));
        let bsp = match parsed {
            Ok(bsp) => bsp,
            Err(error) => {
                eprintln!("{error}");
                failed += 1;
                continue;
            }
        };
        let checked = check(&bsp);
        let tally = &checked.built.tally;
        let movers = asset_ja::movers::movers(&bsp);
        let found = pieces(&checked.mesh, &checked.landed, &movers);
        for (kind, (places, lift, door)) in &found.counts {
            let sum = apart.entry(*kind).or_default();
            (sum.0, sum.1, sum.2) = (sum.0 + places, sum.1 + lift, sum.2 + door);
        }
        if every {
            outln!(
                "{path}: {} brushes, {} mesh faces in {} islands; starts {}; waypoints {}; {:.0} + {:.0} ms",
                checked.built.level.brush_count(),
                checked.mesh.faces.len(),
                checked.mesh.islands,
                checked.starts.line(),
                checked.waypoints.line(),
                checked.build_ms,
                checked.bake_ms,
            );
        } else {
            outln!(
                "{path}: {} solid brushes, {} player clips, {} triangles of curved surfaces, built in {:.0} ms",
                tally.solid,
                tally.player_clip,
                tally.curve_triangles,
                checked.build_ms
            );
            outln!(
                "  {} brushes of doors, closed and open; left out: {} brushes of other contents, {} that bound no solid",
                tally.doors,
                tally.other_contents,
                tally.no_solid
            );
            outln!("  collision digest {}", checked.built.level.digest());
            outln!(
                "  floor: {} triangles that face up -> {} mesh faces, {} portals, {} islands (the largest has {} faces), baked in {:.0} ms",
                checked.built.floor.len(),
                checked.mesh.faces.len(),
                checked.mesh.portals(),
                checked.mesh.islands,
                checked.largest,
                checked.bake_ms
            );
            outln!("  starts: {}", checked.starts.line());
            outln!("  waypoints: {}", checked.waypoints.line());
            for line in gaps(&checked.mesh) {
                outln!("  {line}");
            }
            for line in &found.lines {
                outln!("  {line}");
            }
            // The mesh itself, to draw (`tools/nav-picture.py`): each face's
            // corners and island, and each place's island and soles.
            mesh_file = Some(json!({
                "faces": (checked.mesh.faces.iter())
                    .map(|face| json!([face.corners, face.island]))
                    .collect::<Vec<_>>(),
                "places": (checked.landed.iter())
                    .map(|(island, at)| json!([at, island]))
                    .collect::<Vec<_>>(),
            }));
        }
        starts.add(checked.starts);
        waypoints.add(checked.waypoints);
        levels.push(facts(path, &checked));
    }
    if every {
        outln!(
            "{} levels ({failed} not read): starts {}; waypoints {}",
            levels.len(),
            starts.line(),
            waypoints.line()
        );
        outln!("the places off their level's main piece, by how their piece sits against it:");
        for (kind, (places, lift, door)) in &apart {
            outln!(
                "  {}: {places} places ({lift} with a lift beside, {door} with a door beside)",
                kind.name()
            );
        }
    }
    let folder = crate::collision_verbs::write_facts(json!({
        "verb": "collision ja",
        "levels": levels,
        "starts": starts.facts(),
        "waypoints": waypoints.facts(),
    }));
    if let (Some(folder), Some(mesh)) = (folder, mesh_file) {
        let path = folder.join("mesh.json");
        match std::fs::write(&path, mesh.to_string()) {
            Ok(()) => eprintln!("mesh: {}", path.display()),
            Err(error) => eprintln!("{}: {error}", path.display()),
        }
    }
    if failed > 0 || levels.is_empty() {
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    }
}

/// Where the navigation mesh's islands come close: for each edge of a
/// face that leads to no other face (the mesh's edge), the nearest edge of
/// another island's face, sorted by how far apart they are across (seen
/// from above) and in height. Says how many come within a centimetre, a
/// few centimetres and 30 centimetres across, each at the height of a
/// step or less and at more, with an example of each. Islands that close
/// should be one: the gap says why they are not.
fn gaps(mesh: &NavMesh) -> Vec<String> {
    type P = [f32; 3];
    let sub = |a: P, b: P| [a[0] - b[0], a[1] - b[1], a[2] - b[2]];
    // The mesh's edges: a face's edge that no portal of the face covers.
    let mut edges: Vec<(u32, P, P)> = Vec::new();
    for face in &mesh.faces {
        for side in 0..3 {
            let (a, b) = (face.corners[side], face.corners[(side + 1) % 3]);
            let length = collision::nav::flat_length(a, b);
            let covered: f32 = (face.portals.iter())
                .filter(|portal| {
                    let on = |p: P| {
                        let ab = sub(b, a);
                        let ap = sub(p, a);
                        (ab[0] * ap[1] - ab[1] * ap[0]).abs() / length.max(1e-6) < 0.01
                    };
                    on(portal.a) && on(portal.b)
                })
                .map(|portal| collision::nav::flat_length(portal.a, portal.b))
                .sum();
            if length > 0.05 && covered < length * 0.9 {
                edges.push((face.island, a, b));
            }
        }
    }
    // Seen from above, how far apart two segments are, and the height
    // between them there.
    let apart = |(a, b): (P, P), (c, d): (P, P)| -> (f32, f32) {
        let point_to = |p: P, (s, e): (P, P)| -> (f32, f32) {
            let d = sub(e, s);
            let length = d[0] * d[0] + d[1] * d[1];
            let t = if length > 0.0 {
                (((p[0] - s[0]) * d[0] + (p[1] - s[1]) * d[1]) / length).clamp(0.0, 1.0)
            } else {
                0.0
            };
            let q = [s[0] + d[0] * t, s[1] + d[1] * t, s[2] + d[2] * t];
            (collision::nav::flat_length(p, q), (p[2] - q[2]).abs())
        };
        [point_to(a, (c, d)), point_to(b, (c, d)), point_to(c, (a, b)), point_to(d, (a, b))]
            .into_iter()
            .min_by(|x, y| x.0.total_cmp(&y.0).then(x.1.total_cmp(&y.1)))
            .expect("four")
    };
    let step = collision::player::STEP_HEIGHT * UNIT;
    let classes = [(0.01, "within 1 cm"), (0.05, "1 to 5 cm"), (0.3, "5 to 30 cm")];
    let mut counts = [[0usize; 2]; 3];
    let mut examples: [[Option<(P, f32, f32)>; 2]; 3] = Default::default();
    let mut along: Vec<usize> = (0..edges.len()).collect();
    along.sort_by(|&x, &y| {
        let low = |e: &(u32, P, P)| e.1[0].min(e.2[0]);
        low(&edges[x]).total_cmp(&low(&edges[y]))
    });
    for (at, &one) in along.iter().enumerate() {
        let (island, a, b) = edges[one];
        let high = a[0].max(b[0]) + 0.3;
        let mut best: Option<(f32, f32)> = None;
        for &other in &along[at + 1..] {
            let (theirs, c, d) = edges[other];
            if c[0].min(d[0]) > high {
                break;
            }
            if theirs == island {
                continue;
            }
            let (across, rise) = apart((a, b), (c, d));
            if across <= 0.3 && best.is_none_or(|(x, y)| (across, rise) < (x, y)) {
                best = Some((across, rise));
            }
        }
        let Some((across, rise)) = best else {
            continue;
        };
        let class = classes.iter().position(|(limit, _)| across <= *limit).expect("within 30 cm");
        let high_step = usize::from(rise > step);
        counts[class][high_step] += 1;
        examples[class][high_step].get_or_insert((a, across, rise));
    }
    let mut lines = vec![format!(
        "the mesh's edges near another island's, of {} edges (a step is {step:.2} m):",
        edges.len()
    )];
    for (class, (_, name)) in classes.iter().enumerate() {
        for (high_step, height) in ["a step or less apart in height", "more than a step apart"]
            .iter()
            .enumerate()
        {
            let example = examples[class][high_step].map_or(String::new(), |(at, across, rise)| {
                format!(
                    ", e.g. at {:.2},{:.2},{:.2} ({across:.3} m across, {rise:.3} m in height)",
                    at[0], at[1], at[2]
                )
            });
            lines.push(format!(
                "  {name} across, {height}: {}{example}",
                counts[class][high_step]
            ));
        }
    }
    lines
}

/// How a piece of the mesh that holds places sits against the main piece
/// (the one that holds the most places), where they come nearest.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Apart {
    /// Within 30 cm across and a step in height: the two should be one.
    Joinable,
    /// Lower than the main piece by more than a step, within 1 m across: a
    /// character walks off the main piece's edge and drops onto it.
    Below,
    /// Higher than the main piece by more than a step, within 1 m across: a
    /// character must jump, climb or ride up to it.
    Above,
    /// More than 1 m across at the nearest: something else joins them, if
    /// anything (a lift, a jump across, a door the mesh does not cross, or
    /// a part of the level no one reaches).
    Away,
}

impl Apart {
    fn name(self) -> &'static str {
        match self {
            Apart::Joinable => "joinable (30 cm across, a step in height)",
            Apart::Below => "below it, within 1 m across",
            Apart::Above => "above it, within 1 m across",
            Apart::Away => "more than 1 m away",
        }
    }
}

/// What `pieces` found.
struct Pieces {
    lines: Vec<String>,
    /// The places off the main piece, by how their piece sits, and how
    /// many of those have a lift or a door beside them.
    counts: std::collections::BTreeMap<Apart, (usize, usize, usize)>,
}

/// For each piece of `mesh` other than the main one that holds places
/// (`landed`), where it comes nearest the main piece: how far apart across
/// (seen from above) and in height, and whether a lift or a door of
/// `movers` is within 2 m of there. Says how many places are in pieces of
/// each kind (`Apart`), and the largest such pieces.
fn pieces(mesh: &NavMesh, landed: &[(u32, [f32; 3])], movers: &[asset_ja::movers::Mover]) -> Pieces {
    type P = [f32; 3];
    let mut held: std::collections::BTreeMap<u32, Vec<P>> = std::collections::BTreeMap::new();
    for (island, at) in landed {
        held.entry(*island).or_default().push(*at);
    }
    let Some(main) = (held.iter())
        .max_by_key(|(island, places)| (places.len(), std::cmp::Reverse(**island)))
        .map(|(island, _)| *island)
    else {
        return Pieces {
            lines: Vec::new(),
            counts: Default::default(),
        };
    };
    // Every face's corners, by island: the nearest approach is looked for
    // between corners and edges of faces.
    let mut edges: std::collections::BTreeMap<u32, Vec<(P, P)>> = std::collections::BTreeMap::new();
    for face in &mesh.faces {
        if face.island == main || held.contains_key(&face.island) {
            let sides = (0..3).map(|side| (face.corners[side], face.corners[(side + 1) % 3]));
            edges.entry(face.island).or_default().extend(sides);
        }
    }
    let main_edges = edges.remove(&main).unwrap_or_else(Vec::new);
    let nearest = |p: P| -> (f32, f32, P) {
        let mut best = (f32::INFINITY, 0.0, p);
        for &(a, b) in &main_edges {
            let d = [b[0] - a[0], b[1] - a[1]];
            let length = d[0] * d[0] + d[1] * d[1];
            let t = if length > 0.0 {
                (((p[0] - a[0]) * d[0] + (p[1] - a[1]) * d[1]) / length).clamp(0.0, 1.0)
            } else {
                0.0
            };
            let q = [a[0] + d[0] * t, a[1] + d[1] * t, a[2] + (b[2] - a[2]) * t];
            let across = ((p[0] - q[0]).powi(2) + (p[1] - q[1]).powi(2)).sqrt();
            let rise = p[2] - q[2];
            // Nearest by distance in space.
            if across.hypot(rise) < best.0.hypot(best.1) {
                best = (across, rise, q);
            }
        }
        best
    };
    let step = collision::player::STEP_HEIGHT * UNIT;
    let beside = |at: P, kind: asset_ja::movers::Kind| {
        (movers.iter()).filter(|mover| mover.kind == kind).any(|mover| {
            (0..3).all(|axis| {
                let (low, high) = (mover.min[axis] * UNIT, mover.max[axis] * UNIT);
                at[axis] >= low - 2.0 && at[axis] <= high + 2.0
            })
        })
    };
    let mut counts: std::collections::BTreeMap<Apart, (usize, usize, usize)> = Default::default();
    let mut found = Vec::new();
    for (island, places) in &held {
        if *island == main {
            continue;
        }
        // The piece's corners, each tried against the main piece's edges.
        let corners = (edges.get(island).into_iter().flatten()).map(|(a, _)| *a);
        let Some((across, rise, at)) = corners
            .map(&nearest)
            .min_by(|x, y| x.0.hypot(x.1).total_cmp(&y.0.hypot(y.1)))
        else {
            continue;
        };
        let apart = if across <= 0.3 && rise.abs() <= step {
            Apart::Joinable
        } else if across <= 1.0 && rise < -step {
            Apart::Below
        } else if across <= 1.0 && rise > step {
            Apart::Above
        } else {
            Apart::Away
        };
        let (lift, door) = (
            beside(at, asset_ja::movers::Kind::Lift),
            beside(at, asset_ja::movers::Kind::Door),
        );
        let count = counts.entry(apart).or_default();
        count.0 += places.len();
        count.1 += usize::from(lift) * places.len();
        count.2 += usize::from(door) * places.len();
        found.push((places.len(), apart, across, rise, at, lift, door));
    }
    let total: usize = held.values().map(Vec::len).sum();
    let mut lines = vec![format!(
        "the places off the main piece ({} of {total} places are on it), by how their piece sits against it:",
        held[&main].len()
    )];
    for (apart, (places, lift, door)) in &counts {
        lines.push(format!(
            "  {}: {places} places ({lift} with a lift beside, {door} with a door beside)",
            apart.name()
        ));
    }
    found.sort_by_key(|x| std::cmp::Reverse(x.0));
    lines.push("  the pieces with the most places:".to_string());
    for (places, apart, across, rise, at, lift, door) in found.iter().take(8) {
        lines.push(format!(
            "    {places} places, {}: {across:.2} m across, {rise:+.2} m in height, at {:.1},{:.1},{:.1}{}{}",
            apart.name(),
            at[0],
            at[1],
            at[2],
            if *lift { ", a lift beside" } else { "" },
            if *door { ", a door beside" } else { "" },
        ));
    }
    Pieces { lines, counts }
}
