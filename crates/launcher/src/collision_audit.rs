//! `kjk collision audit`: the containment audit of a module's entry area
//! (`collision::audit`), printed, written to `facts.json`, and drawn from
//! above into `audit.png`.

use std::process::ExitCode;
use std::time::Instant;

use collision::audit::{self, LeakKind, Region, Report};
use serde_json::json;
use vfs::Discovery;

use crate::collision_verbs::{ClipChoice, Doors, Loaded, load, set_doors, write_facts};

/// The rooms' walkable walkmesh faces as a level of their own, and their
/// bounds.
pub struct Walkmeshes {
    pub walkable: collision::Level,
    pub bounds: ([f32; 3], [f32; 3]),
}

pub fn walkmeshes(loaded: &Loaded) -> Walkmeshes {
    let tag = |material: u32| collision::Tag::level(collision::Contents::SOLID, material as u16);
    let mut walkable = collision::LevelBuilder::new();
    let mut min = [f32::INFINITY; 3];
    let mut max = [f32::NEG_INFINITY; 3];
    for (corners, material) in &loaded.collision.walkable {
        walkable.triangle(*corners, tag(*material));
        for corner in corners {
            for i in 0..3 {
                min[i] = min[i].min(corner[i]);
                max[i] = max[i].max(corner[i]);
            }
        }
    }
    Walkmeshes {
        walkable: walkable.build(),
        bounds: (min, max),
    }
}

fn round(value: f32) -> f64 {
    (f64::from(value) * 100.0).round() / 100.0
}

fn point(p: [f32; 3]) -> serde_json::Value {
    json!([round(p[0]), round(p[1]), round(p[2])])
}

pub struct Options {
    pub cell: f32,
    pub doors: Doors,
    /// How many regions to print (all go to facts.json).
    pub show: usize,
    /// Also draw `zoom.png`: (x, y, radius) metres, 16 pixels a metre.
    pub zoom: Option<[f32; 3]>,
    /// Flood from every in-bounds spot instead of the entry and waypoints.
    pub from_walkmesh: bool,
}

/// A module's audit: what it ran on, and how long it took.
struct Audited {
    loaded: Loaded,
    walkmeshes: Walkmeshes,
    report: Report,
    open: usize,
    seconds: f64,
}

fn audited(
    discovery: &Discovery,
    game: &str,
    module: &str,
    clip: ClipChoice,
    options: &Options,
) -> Result<Audited, String> {
    Ok(audit_loaded(load(discovery, game, module, clip)?, options))
}

/// What an area that is a minigame is: the player rides a swoop along its
/// track or sits in a turret (`sim::minigame`, every seat parked while it
/// plays), never walks it, so walking out of it is no fault.
fn minigame(loaded: &Loaded) -> Option<&'static str> {
    loaded.area().minigame.as_ref().map(|game| match game.kind {
        1 => "swoop track",
        2 => "turret",
        _ => "minigame",
    })
}

fn audit_loaded(mut loaded: Loaded, options: &Options) -> Audited {
    let open = set_doors(&mut loaded, options.doors);
    let walkmeshes = walkmeshes(&loaded);
    let accepts = loaded
        .clip
        .as_ref()
        .map(|clip| clip.accepts.clone())
        .unwrap_or_else(Vec::new);
    let config = audit::Config {
        cell: options.cell,
        ..audit::Config::default()
    };
    // The module's entry, and every waypoint of the area; none floods
    // from the whole walkmesh.
    let mut starts = Vec::new();
    if !options.from_walkmesh {
        starts.push(loaded.module.entry_position);
        starts.extend(
            loaded
                .area()
                .instances
                .iter()
                .filter(|i| i.kind == asset_odyssey::module::Kind::Waypoint)
                .map(|i| i.position),
        );
    }
    let input = audit::Input {
        world: &loaded.collision.level,
        walkmesh: &walkmeshes.walkable,
        bounds: walkmeshes.bounds,
        accepts: &accepts,
        starts: &starts,
    };
    let started = Instant::now();
    let report = audit::run(&input, &config);
    let seconds = started.elapsed().as_secs_f64();
    Audited {
        loaded,
        walkmeshes,
        report,
        open,
        seconds,
    }
}

impl Audited {
    /// The whole grid from above, `scale` pixels a metre.
    fn whole(&self, scale: f32) -> View {
        let (low, high) = self.walkmeshes.bounds;
        let report = &self.report;
        View {
            min: [report.origin[0], report.origin[1]],
            max: [
                report.origin[0] + report.nx as f32 * report.config.cell,
                report.origin[1] + report.ny as f32 * report.config.cell,
            ],
            scale,
            z: [low[2] - 1.0, high[2] + 3.0],
        }
    }

    fn draw(&self, view: &View) -> image::RgbImage {
        draw(&self.report, &self.loaded.collision.triangles, view)
    }
}

pub fn run(
    discovery: &Discovery,
    game: &str,
    module: &str,
    clip: ClipChoice,
    options: &Options,
) -> ExitCode {
    let audited = match audited(discovery, game, module, clip, options) {
        Ok(audited) => audited,
        Err(error) => {
            eprintln!("{error}");
            return ExitCode::FAILURE;
        }
    };
    let Audited {
        loaded,
        report,
        open,
        seconds,
        ..
    } = &audited;
    let exits = exits(loaded, report);
    let floor = on_walkmesh(loaded, report);
    print(loaded, report, options, *open, *seconds);
    if let Some(minigame) = minigame(loaded) {
        outln!("  the area is a {minigame}: played parked, never walked (the sweep does not audit it)");
    }
    outln!(
        "  columns on walkable walkmesh (the topmost face): {} spots, {} reached on foot, {} not at all; only by jumping: {} joined to a start by the walkmesh, {} apart",
        floor.spots,
        floor.on_foot,
        floor.unreached,
        floor.cut_off.len(),
        floor.apart
    );
    print_exits(report, &exits);
    let mut facts = facts(loaded, report, options, *open, *seconds);
    facts["on_walkmesh"] = json!({
        "spots": floor.spots,
        "on_foot": floor.on_foot,
        "unreached": floor.unreached,
        "cut_off": floor.cut_off.iter().map(|&p| point(p)).collect::<Vec<_>>(),
        "apart": floor.apart,
    });
    facts["exits"] = exits
        .iter()
        .map(|exit| {
            json!({
                "kind": format!("{:?}", exit.kind),
                "tag": exit.tag,
                "reached": exit.level.map(|level| report.config.reaches[level as usize].name.clone()),
            })
        })
        .collect();
    let folder = write_facts(facts);
    if let Some(folder) = folder {
        let whole = audited.whole(8.0);
        let mut views = vec![("audit.png".to_string(), whole)];
        // Every region that is not accepted and escapes (outside the
        // footprint, or leaking), up to 24, zoomed.
        let escapes = report
            .regions
            .iter()
            .filter(|r| r.accepted.is_none() && r.escapes())
            .take(24);
        for (index, region) in escapes.enumerate() {
            let r = ((region.max[0] - region.min[0]).max(region.max[1] - region.min[1]) * 0.5
                + 4.0)
                .min(20.0);
            let (x, y) = (region.centroid[0], region.centroid[1]);
            views.push((
                format!("escape-{index:02}.png"),
                View {
                    min: [x - r, y - r],
                    max: [x + r, y + r],
                    scale: 24.0,
                    z: whole.z,
                },
            ));
        }
        if let Some([x, y, r]) = options.zoom {
            views.push((
                "zoom.png".to_string(),
                View {
                    min: [x - r, y - r],
                    max: [x + r, y + r],
                    scale: 16.0,
                    z: whole.z,
                },
            ));
        }
        for (name, view) in views {
            let path = folder.join(&name);
            match audited.draw(&view).save(&path) {
                Ok(()) => eprintln!("map: {}", path.display()),
                Err(error) => eprintln!("{}: {error}", path.display()),
            }
        }
    }
    ExitCode::SUCCESS
}

/// One reach level of a survey: regions out of bounds that escape (not
/// accepted, outside the footprint or leaking), and leaks.
#[derive(Debug, Clone)]
pub struct Reach {
    pub name: String,
    pub escapes: usize,
    pub perches: usize,
    pub leaks: usize,
}

/// A module's containment audit in brief, for the whole-game sweep: the
/// default audit (`openable` doors, the built-in clip file, flooded from
/// the entry and waypoints).
#[derive(Debug, Clone)]
pub struct Survey {
    pub area: String,
    /// The area is a minigame (a swoop track, a turret): never walked, so
    /// not audited, and no reach levels.
    pub minigame: Option<&'static str>,
    pub clip_brushes: usize,
    pub edge_walls: usize,
    pub unreached_in_bounds: usize,
    pub voids: usize,
    /// Spots on the walkable walkmesh: all, reached on foot, reached
    /// only by jumping though the walkmesh joins them to a start.
    pub on_walkmesh: [usize; 3],
    /// Doors and triggers leading out: all, reached on foot.
    pub exits: [usize; 2],
    pub reaches: Vec<Reach>,
    pub seconds: f64,
}

impl Survey {
    pub fn json(&self) -> serde_json::Value {
        json!({
            "area": self.area,
            "minigame": self.minigame,
            "clip_brushes": self.clip_brushes,
            "edge_walls": self.edge_walls,
            "unreached_in_bounds": self.unreached_in_bounds,
            "voids": self.voids,
            "on_walkmesh": self.on_walkmesh,
            "exits": self.exits,
            "reaches": self.reaches.iter().map(|r| json!({
                "name": r.name,
                "escapes": r.escapes,
                "perches": r.perches,
                "leaks": r.leaks,
            })).collect::<Vec<_>>(),
            "seconds": self.seconds,
        })
    }

    /// Escapes at the first `levels` reach levels (walk, then jump, ...).
    pub fn escapes(&self, levels: usize) -> usize {
        self.reaches.iter().take(levels).map(|r| r.escapes).sum()
    }
}

/// The whole of a report's grid from above, `longest` pixels along its
/// longer side (8 a metre at most).
fn whole_map(audited: &Audited, longest: u32) -> image::RgbImage {
    let report = &audited.report;
    let extent = report.nx.max(report.ny).max(1) as f32 * report.config.cell;
    let scale = (longest as f32 / extent).min(8.0);
    audited.draw(&audited.whole(scale))
}

/// The default audit of `module`, and the whole of it drawn from above,
/// `longest` pixels along its longer side (8 a metre at most). A minigame's
/// area is drawn, not audited.
pub fn survey(
    discovery: &Discovery,
    game: &str,
    module: &str,
    longest: u32,
) -> Result<(Survey, image::RgbImage), String> {
    let options = Options {
        cell: 0.5,
        doors: Doors::Openable,
        show: 0,
        zoom: None,
        from_walkmesh: false,
    };
    let loaded = load(discovery, game, module, ClipChoice::BuiltIn)?;
    if let Some(minigame) = minigame(&loaded) {
        let walkmeshes = walkmeshes(&loaded);
        let report = audit::frame(walkmeshes.bounds, &audit::Config::default());
        let survey = Survey {
            area: loaded.area().resref.clone(),
            minigame: Some(minigame),
            clip_brushes: loaded.collision.stats.clip_brushes,
            edge_walls: loaded.collision.stats.edge_walls,
            unreached_in_bounds: 0,
            voids: 0,
            on_walkmesh: [0; 3],
            exits: [0; 2],
            reaches: Vec::new(),
            seconds: 0.0,
        };
        let audited = Audited {
            loaded,
            walkmeshes,
            report,
            open: 0,
            seconds: 0.0,
        };
        return Ok((survey, whole_map(&audited, longest)));
    }
    let audited = audit_loaded(loaded, &options);
    let report = &audited.report;
    let floor = on_walkmesh(&audited.loaded, report);
    let exits = exits(&audited.loaded, report);
    let survey = Survey {
        area: audited.loaded.area().resref.clone(),
        minigame: None,
        clip_brushes: audited.loaded.collision.stats.clip_brushes,
        edge_walls: audited.loaded.collision.stats.edge_walls,
        on_walkmesh: [floor.spots, floor.on_foot, floor.cut_off.len()],
        exits: [exits.len(), on_foot(&exits)],
        unreached_in_bounds: report.unreached_in_bounds,
        voids: report
            .leaks
            .iter()
            .filter(|leak| leak.kind == LeakKind::Void)
            .count(),
        reaches: report
            .levels
            .iter()
            .map(|level| Reach {
                name: level.name.clone(),
                escapes: level.escapes,
                perches: level.perches,
                leaks: level.leaks,
            })
            .collect(),
        seconds: audited.seconds,
    };
    Ok((survey, whole_map(&audited, longest)))
}

fn print(loaded: &Loaded, report: &Report, options: &Options, open: usize, seconds: f64) {
    let config = &report.config;
    outln!(
        "containment audit: {} {} (area {}), doors {} ({open} open), {} clip brushes from {}, {} edge walls",
        loaded.game.key(),
        loaded.module.name,
        loaded.area().resref,
        options.doors.name(),
        loaded.collision.stats.clip_brushes,
        loaded.clip_source,
        loaded.collision.stats.edge_walls
    );
    outln!(
        "  grid {} x {} columns of {} m; {} spots ({} in bounds, {} of them never reached); {} traces in {seconds:.1} s; {} spots found by landing",
        report.nx,
        report.ny,
        config.cell,
        report.spots.len(),
        report.spots.iter().filter(|s| s.in_bounds).count(),
        report.unreached_in_bounds,
        report.traces,
        report.landed
    );
    outln!(
        "  flood from {} spots {}",
        report.seeds.len(),
        if options.from_walkmesh {
            "(every in-bounds one)"
        } else {
            "under the module's entry and waypoints"
        }
    );
    for (reach, summary) in config.reaches.iter().zip(&report.levels) {
        outln!(
            "  {:<8} (ledge {:.2} m, carry {:.2} m): {:>6} reached, {:>5} out of bounds in {:>3} regions: {} escapes, {} perches, {} accepted; {} leaks",
            summary.name,
            reach.ledge,
            reach.distance,
            summary.reached,
            summary.out_of_bounds,
            summary.regions,
            summary.escapes,
            summary.perches,
            summary.regions - summary.escapes - summary.perches,
            summary.leaks
        );
    }
    let voids = report
        .leaks
        .iter()
        .filter(|leak| leak.kind == LeakKind::Void)
        .count();
    outln!(
        "  leaks: {voids} into the void, {} off the grid",
        report.leaks.len() - voids
    );
    // Escapes first.
    let mut unaccepted: Vec<&Region> = report
        .regions
        .iter()
        .filter(|region| region.accepted.is_none())
        .collect();
    unaccepted.sort_by_key(|region| !region.escapes());
    let feet = |spot| {
        let p = report.feet(spot);
        format!("({:.2}, {:.2}, {:.2})", p[0], p[1], p[2])
    };
    for (index, region) in unaccepted.iter().take(options.show).enumerate() {
        let outside = first_outside(report, region)
            .map(|spot| {
                format!(
                    ", first outside at {} from {}",
                    feet(spot),
                    feet(report.spots[spot as usize].from)
                )
            })
            .unwrap_or_else(String::new);
        outln!(
            "  region {index:>3}: {:<8} {:>5} spots {:>7.2} m2, off the walkmesh at {}, from {} to {}{outside}, z {:.2}..{:.2}, {}{}",
            config.reaches[region.level as usize].name,
            region.spots.len(),
            region.area,
            feet(left_from(report, region.entry)),
            feet(region.from),
            feet(region.entry),
            region.min[2],
            region.max[2],
            if region.escapes() {
                if region.inside {
                    "ESCAPE (leaks)"
                } else {
                    "ESCAPE (outside)"
                }
            } else {
                "perch"
            },
            if region.leaks > 0 {
                format!(", {} leaks", region.leaks)
            } else {
                String::new()
            }
        );
    }
    if unaccepted.len() > options.show {
        outln!(
            "  ... {} more regions in facts.json",
            unaccepted.len() - options.show
        );
    }
    for region in report.regions.iter().filter(|r| r.accepted.is_some()) {
        outln!(
            "  accepted: {:<8} {:>5} spots at ({:.2}, {:.2}, {:.2}) by {}",
            config.reaches[region.level as usize].name,
            region.spots.len(),
            region.centroid[0],
            region.centroid[1],
            region.centroid[2],
            region.accepted.as_deref().unwrap_or("?")
        );
    }
}

/// The last in-bounds spot on the way the flood reached `spot`: where the
/// player left the walkmesh for it (a start, when it never was on it).
fn left_from(report: &Report, mut spot: u32) -> u32 {
    loop {
        let s = &report.spots[spot as usize];
        if s.in_bounds || s.from == spot || s.from == u32::MAX {
            return spot;
        }
        spot = s.from;
    }
}

/// A region's first spot reached outside the level (neither under a roof
/// nor over the walkable walkmesh): where it got out.
fn first_outside(report: &Report, region: &Region) -> Option<u32> {
    region
        .spots
        .iter()
        .copied()
        .filter(|&s| !report.spots[s as usize].inside)
        .min_by_key(|&s| report.spots[s as usize].order)
}

/// A way out of the module: a door or trigger linked to another one.
struct Exit {
    kind: asset_odyssey::module::Kind,
    tag: String,
    /// The lowest reach level from which a player gets into it: for a
    /// door, the box a unit (and a column) larger overlapping its closed
    /// shape (as the sim's doorway asks); for a trigger, the feet inside
    /// its outline.
    level: Option<u8>,
}

fn exits(loaded: &Loaded, report: &Report) -> Vec<Exit> {
    use asset_odyssey::module::Kind;
    // A unit, as the doorway asks, and a column more: spots stand a
    // column apart, so the one nearest a door may stop that much short.
    let player = report.config.player;
    let by = collision::UNIT + report.config.cell;
    let grown = collision::Extents {
        mins: player.mins.map(|v| v - by),
        maxs: player.maxs.map(|v| v + by),
    };
    let module = &loaded.module.name;
    let reached: Vec<&audit::Spot> = report.spots.iter().filter(|s| s.level.is_some()).collect();
    loaded
        .area()
        .instances
        .iter()
        .enumerate()
        .filter(|(_, i)| {
            i.linked_to
                .as_ref()
                .is_some_and(|(to, _)| !to.is_empty() && !to.eq_ignore_ascii_case(module))
                && matches!(i.kind, Kind::Door | Kind::Trigger)
        })
        .map(|(index, instance)| {
            let into = |spot: &audit::Spot| match instance.kind {
                Kind::Door => {
                    let (dx, dy) = (
                        spot.origin[0] - instance.position[0],
                        spot.origin[1] - instance.position[1],
                    );
                    dx * dx + dy * dy < 25.0
                        && loaded.collision.level.entity_overlaps(
                            index as u32,
                            0,
                            spot.origin,
                            &grown,
                            collision::Contents::SOLID,
                        )
                }
                _ => {
                    let at = [
                        spot.origin[0] - instance.position[0],
                        spot.origin[1] - instance.position[1],
                    ];
                    sim::module::inside_outline(&instance.geometry, at)
                }
            };
            Exit {
                kind: instance.kind,
                tag: String::from_utf8_lossy(&instance.tag).into_owned(),
                level: reached
                    .iter()
                    .filter(|s| into(s))
                    .filter_map(|s| s.level)
                    .min(),
            }
        })
        .collect()
}

/// Spots whose feet stand on a walkable walkmesh face (the topmost within
/// the audit's height): where KOTOR walks, and whether we still do. Those
/// reached only by jumping are split by whether the walkmesh itself joins
/// them to a start (an island of the navigation mesh a start stands on):
/// there a KOTOR walker gets, so a player on foot should too.
fn on_walkmesh(loaded: &Loaded, report: &Report) -> Floor {
    use collision::CollisionWorld;
    // Every walkmesh face, whatever its material: the topmost one under
    // the feet decides, as it does KOTOR's walker.
    let mut faces = collision::LevelBuilder::new();
    for triangle in &loaded.collision.triangles {
        if triangle.origin == collision_odyssey::Origin::Walkmesh {
            faces.triangle(
                triangle.corners,
                collision::Tag::level(collision::Contents::SOLID, triangle.material as u16),
            );
        }
    }
    let walkmesh = faces.build();
    let triangles = loaded.collision.nav_faces();
    let nav = collision::nav::NavMesh::build(&triangles, 0.0);
    let island = |feet: [f32; 3]| {
        nav.locate(feet, audit::IN_BOUNDS_UP, audit::IN_BOUNDS_UP)
            .map(|face| nav.faces[face as usize].island)
    };
    let started: Vec<u32> = report
        .seeds
        .iter()
        .filter_map(|&seed| island(report.feet(seed)))
        .collect();
    let mut counts = Floor::default();
    for (index, spot) in report.spots.iter().enumerate() {
        // Column centres only: a spot found by landing off one (beside a
        // wall the centre is too close to) is the grid's, not the floor's.
        let centre = report.column_centre(spot.column);
        if (spot.origin[0] - centre[0]).abs() > 0.001 || (spot.origin[1] - centre[1]).abs() > 0.001
        {
            continue;
        }
        let feet = report.feet(index as u32);
        let hit = walkmesh.trace(
            [feet[0], feet[1], feet[2] + audit::IN_BOUNDS_UP],
            [feet[0], feet[1], feet[2] - audit::IN_BOUNDS_UP],
            &collision::Extents::POINT,
            collision::Contents::SOLID,
        );
        if hit.fraction >= 1.0 || !loaded.surfaces.walkable(u32::from(hit.surface.material)) {
            continue;
        }
        counts.spots += 1;
        match spot.level {
            Some(0) => counts.on_foot += 1,
            None => counts.unreached += 1,
            Some(_) if island(feet).is_some_and(|i| started.contains(&i)) => {
                counts.cut_off.push(feet)
            }
            Some(_) => counts.apart += 1,
        }
    }
    counts
}

/// Spots on the walkable walkmesh (see [`on_walkmesh`]).
#[derive(Default)]
struct Floor {
    spots: usize,
    on_foot: usize,
    unreached: usize,
    /// Reached only by jumping, on walkmesh joined to a start.
    cut_off: Vec<[f32; 3]>,
    /// Reached only by jumping, on walkmesh no start is joined to.
    apart: usize,
}

fn on_foot(exits: &[Exit]) -> usize {
    exits.iter().filter(|exit| exit.level == Some(0)).count()
}

fn print_exits(report: &Report, exits: &[Exit]) {
    outln!(
        "  exits: {} of {} doors and triggers leading out reached on foot",
        on_foot(exits),
        exits.len()
    );
    for exit in exits.iter().filter(|exit| exit.level != Some(0)) {
        outln!(
            "    {:?} {}: {}",
            exit.kind,
            exit.tag,
            exit.level.map_or("not reached", |level| {
                report.config.reaches[level as usize].name.as_str()
            })
        );
    }
}

fn facts(
    loaded: &Loaded,
    report: &Report,
    options: &Options,
    open: usize,
    seconds: f64,
) -> serde_json::Value {
    let config = &report.config;
    let region = |region: &Region| {
        json!({
            "level": config.reaches[region.level as usize].name,
            "spots": region.spots.len(),
            "area_m2": round(region.area),
            "min": point(region.min),
            "max": point(region.max),
            "centroid": point(region.centroid),
            "inside": region.inside,
            "escape": region.escapes(),
            "outside_spots": region
                .spots
                .iter()
                .filter(|&&s| !report.spots[s as usize].inside)
                .map(|&s| point(report.feet(s)))
                .collect::<Vec<_>>(),
            "entry": point(report.feet(region.entry)),
            "off_walkmesh_at": point(report.feet(left_from(report, region.entry))),
            "first_outside": first_outside(report, region).map(|s| json!({
                "at": point(report.feet(s)),
                "from": point(report.feet(report.spots[s as usize].from)),
            })),
            "from": point(report.feet(region.from)),
            "leaks": region.leaks,
            "accepted": region.accepted,
        })
    };
    json!({
        "verb": "collision audit",
        "game": loaded.game.key(),
        "module": loaded.module.name,
        "area": loaded.area().resref,
        "doors": options.doors.name(),
        "seeds_from": if options.from_walkmesh { "walkmesh" } else { "entry and waypoints" },
        "doors_open": open,
        "clip_source": loaded.clip_source,
        "clip_brushes": loaded.collision.stats.clip_brushes,
        "edge_walls": loaded.collision.stats.edge_walls,
        "level_digest": loaded.collision.level.digest(),
        "cell": config.cell,
        "grid": [report.nx, report.ny],
        "spots": report.spots.len(),
        "traces": report.traces,
        "seconds": seconds,
        "landed_spots": report.landed,
        "seeds": report.seeds.iter().map(|&s| point(report.feet(s))).collect::<Vec<_>>(),
        "unreached_in_bounds": report.unreached_in_bounds,
        "levels": config.reaches.iter().zip(&report.levels).map(|(reach, s)| json!({
            "name": s.name,
            "ledge_m": round(reach.ledge),
            "carry_m": round(reach.distance),
            "reached": s.reached,
            "out_of_bounds": s.out_of_bounds,
            "regions": s.regions,
            "escapes": s.escapes,
            "perches": s.perches,
            "leaks": s.leaks,
        })).collect::<Vec<_>>(),
        "regions": report.regions.iter().map(region).collect::<Vec<_>>(),
        "leaks": report.leaks.iter().map(|leak| json!({
            "kind": format!("{:?}", leak.kind),
            "level": config.reaches[leak.level as usize].name,
            "from": point(report.feet(leak.spot)),
            "at": point(leak.at),
        })).collect::<Vec<_>>(),
    })
}

/// A window onto the audit from above: metres, pixels a metre, and the
/// heights whose walls are drawn.
#[derive(Debug, Clone, Copy)]
struct View {
    min: [f32; 2],
    max: [f32; 2],
    scale: f32,
    z: [f32; 2],
}

/// The audit from above, north up: walls (steep collision within the
/// walkmesh's heights) dark blue, walls along open walkmesh edges light
/// blue, unreached floor dark grey, in bounds grey, out of bounds by the
/// level that first reaches it (walk magenta, jump red, force 1 orange, 2
/// yellow, 3 cyan; darker when inside the footprint, on top of things),
/// accepted green, leaks white.
fn draw(
    report: &Report,
    triangles: &[collision_odyssey::SourceTriangle],
    view: &View,
) -> image::RgbImage {
    const COLOURS: [[u8; 3]; 5] = [
        [230, 40, 230],
        [230, 40, 40],
        [240, 140, 20],
        [230, 220, 30],
        [40, 210, 230],
    ];
    let width = ((view.max[0] - view.min[0]) * view.scale) as u32;
    let height = ((view.max[1] - view.min[1]) * view.scale) as u32;
    let mut image = image::RgbImage::new(width.max(1), height.max(1));
    let pixel = |x: f32, y: f32| -> Option<(u32, u32)> {
        let px = (x - view.min[0]) * view.scale;
        let py = (view.max[1] - y) * view.scale;
        (px >= 0.0 && py >= 0.0 && (px as u32) < width && (py as u32) < height)
            .then_some((px as u32, py as u32))
    };
    let mut accepted = vec![false; report.spots.len()];
    for region in report.regions.iter().filter(|r| r.accepted.is_some()) {
        for &spot in &region.spots {
            accepted[spot as usize] = true;
        }
    }
    // Spots from the lowest in the ranking up, so the most telling one of
    // a column is drawn last.
    let rank = |index: usize| -> (u8, [u8; 3]) {
        let spot = &report.spots[index];
        match spot.level {
            None => (0, [45, 45, 55]),
            Some(_) if spot.in_bounds => (1, [120, 120, 120]),
            Some(_) if accepted[index] => (2, [60, 200, 80]),
            Some(level) => {
                let c = COLOURS[(level as usize).min(4)];
                if spot.inside {
                    (3, c.map(|v| v / 2 + 20))
                } else {
                    (4 + 5 - level.min(4), c)
                }
            }
        }
    };
    let mut order: Vec<usize> = (0..report.spots.len()).collect();
    order.sort_by_key(|&index| (rank(index).0, index));
    let half = report.config.cell * 0.5;
    for index in order {
        let colour = rank(index).1;
        let o = report.spots[index].origin;
        let steps = (report.config.cell * view.scale).ceil() as i32;
        for dy in 0..steps.max(1) {
            for dx in 0..steps.max(1) {
                let x = o[0] - half + dx as f32 / view.scale;
                let y = o[1] - half + dy as f32 / view.scale;
                if let Some((px, py)) = pixel(x, y) {
                    image.put_pixel(px, py, image::Rgb(colour));
                }
            }
        }
    }
    // Walls over the spots.
    for triangle in triangles {
        let c = triangle.corners;
        let low = c.iter().map(|p| p[2]).fold(f32::INFINITY, f32::min);
        let high = c.iter().map(|p| p[2]).fold(f32::NEG_INFINITY, f32::max);
        if high < view.z[0] || low > view.z[1] {
            continue;
        }
        let u = [c[1][0] - c[0][0], c[1][1] - c[0][1], c[1][2] - c[0][2]];
        let v = [c[2][0] - c[0][0], c[2][1] - c[0][1], c[2][2] - c[0][2]];
        let n = [
            u[1] * v[2] - u[2] * v[1],
            u[2] * v[0] - u[0] * v[2],
            u[0] * v[1] - u[1] * v[0],
        ];
        let length = (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt();
        if length <= 0.0 || (n[2] / length).abs() >= 0.7 {
            continue;
        }
        let colour = match triangle.origin {
            collision_odyssey::Origin::EdgeWall => [140, 200, 255],
            _ => [30, 60, 200],
        };
        for (a, b) in [(0, 1), (1, 2), (2, 0)] {
            let (p, q) = (c[a], c[b]);
            let steps = (((q[0] - p[0]).abs() + (q[1] - p[1]).abs()) * view.scale) as u32 + 1;
            for k in 0..=steps {
                let t = k as f32 / steps as f32;
                if let Some((px, py)) = pixel(p[0] + (q[0] - p[0]) * t, p[1] + (q[1] - p[1]) * t) {
                    image.put_pixel(px, py, image::Rgb(colour));
                }
            }
        }
    }
    for leak in &report.leaks {
        let Some((x, y)) = pixel(leak.at[0], leak.at[1]) else {
            continue;
        };
        for (dx, dy) in [(0i32, 0i32), (1, 0), (-1, 0), (0, 1), (0, -1)] {
            let (px, py) = (x as i32 + dx, y as i32 + dy);
            if px >= 0 && py >= 0 && (px as u32) < width && (py as u32) < height {
                image.put_pixel(px as u32, py as u32, image::Rgb([255, 255, 255]));
            }
        }
    }
    image
}
