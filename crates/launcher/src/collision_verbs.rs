//! `kjk collision …`: a module's collision level (I12) from the command
//! line, with no window: what it was built from, traces through it, and
//! the containment audit. Each verb prints what it found and writes it to
//! `facts.json` in a run folder.

use std::path::Path;
use std::process::ExitCode;
use std::time::Instant;

use asset_odyssey::module::ModuleIr;
use collision::clip_file::ClipFile;
pub use collision_odyssey::Doors;
use collision_odyssey::{AreaCollision, BuildOptions, EdgeWalls, Surfaces};
use serde_json::json;
use vfs::{Discovery, Game};

use crate::modules::ResolverSource;

/// Where a module's clip brushes come from.
#[derive(Debug, Clone, Copy)]
pub enum ClipChoice<'a> {
    /// The file built into kjk, if the module has one.
    BuiltIn,
    /// This file instead (for authoring).
    File(&'a Path),
    None,
}

/// A module's entry area with its collision built.
pub struct Loaded {
    pub game: Game,
    pub module: ModuleIr,
    pub area: usize,
    pub surfaces: Surfaces,
    pub collision: AreaCollision,
    pub clip: Option<ClipFile>,
    pub clip_source: String,
    pub build_ms: f64,
}

impl Loaded {
    pub fn area(&self) -> &asset_odyssey::module::AreaIr {
        &self.module.areas[self.area]
    }
}

/// The collision world of `module`'s entry area, from a resolver opened
/// with the module: its built-in clip brushes in, the doors that can open
/// open (`Doors::Openable`). Change a door with `Level::set_state` (its
/// entity is its index in the area's instances).
pub fn module_level(
    resolver: &vfs::resolver::Resolver,
    game: Game,
    module: &str,
) -> Result<collision::Level, String> {
    let clip = collision_odyssey::built_in_clip(game.key(), &module.to_ascii_lowercase())?;
    let mut built = collision_odyssey::build_module(
        &ResolverSource(resolver),
        module,
        clip.as_ref(),
        &build_options()?,
    )?;
    built.set_doors(Doors::Openable);
    Ok(built.collision.level)
}

/// The build's options, the edge walls' from `KJK_EDGE_WALLS` when set
/// (`off`, or their height in metres) to compare them in the verbs.
fn build_options() -> Result<BuildOptions, String> {
    let mut options = BuildOptions::default();
    if let Ok(value) = std::env::var("KJK_EDGE_WALLS") {
        options.edge_walls = match value.as_str() {
            "off" => None,
            height => Some(EdgeWalls {
                height: height
                    .parse()
                    .map_err(|_| format!("KJK_EDGE_WALLS: {value:?} is not off or a height"))?,
            }),
        };
    }
    Ok(options)
}

pub fn load(
    discovery: &Discovery,
    game: &str,
    module: &str,
    clip: ClipChoice,
) -> Result<Loaded, String> {
    let game = Game::from_key(game)
        .filter(|game| *game != Game::JediAcademy)
        .ok_or("use k1 or tsl")?;
    let module = module.to_ascii_lowercase();
    let resolver = crate::viewer::resolver(discovery, game, Some(&module))?;
    let (clip, clip_source) = match clip {
        ClipChoice::BuiltIn => match collision_odyssey::built_in_clip(game.key(), &module)? {
            Some(clip) => (
                Some(clip),
                format!("data/clip/{}/{module}.clip (built in)", game.key()),
            ),
            None => (None, "none (no built-in file)".to_string()),
        },
        ClipChoice::File(path) => {
            let text = std::fs::read_to_string(path)
                .map_err(|error| format!("{}: {error}", path.display()))?;
            let clip =
                ClipFile::parse(&text).map_err(|error| format!("{}: {error}", path.display()))?;
            (Some(clip), path.display().to_string())
        }
        ClipChoice::None => (None, "none (--no-clip)".to_string()),
    };
    let started = Instant::now();
    let built = collision_odyssey::build_module(
        &ResolverSource(&resolver),
        &module,
        clip.as_ref(),
        &build_options()?,
    )?;
    let build_ms = started.elapsed().as_secs_f64() * 1000.0;
    Ok(Loaded {
        game,
        module: built.module,
        area: built.area,
        surfaces: built.surfaces,
        collision: built.collision,
        clip,
        clip_source,
        build_ms,
    })
}

/// Builds every module of an install and reports each in a line.
fn build_all(discovery: &Discovery, game: &str, clip: ClipChoice) -> ExitCode {
    let Some(key) = Game::from_key(game).filter(|game| *game != Game::JediAcademy) else {
        eprintln!("use k1 or tsl");
        return ExitCode::from(2);
    };
    let Some(install) = discovery.get(key) else {
        eprintln!("{}: not found (set {})", key.key(), key.env_var());
        return ExitCode::FAILURE;
    };
    let names = match vfs::DirIndex::build(&install.data_root) {
        Ok(index) => crate::modules::module_names(&index),
        Err(error) => {
            eprintln!("{error}");
            return ExitCode::FAILURE;
        }
    };
    let mut rows = Vec::new();
    let mut refused = Vec::new();
    let mut census = Census::default();
    let (mut total_ms, mut total_brushes) = (0.0, 0);
    for name in &names {
        match load(discovery, game, name, clip) {
            Ok(loaded) => {
                census.add(name, &loaded);
                let stats = &loaded.collision.stats;
                let level = loaded.collision.level.stats();
                total_ms += loaded.build_ms;
                total_brushes += level.brushes;
                outln!(
                    "{name:<14} {:>3} rooms {:>7} brushes {:>8.1} ms  walkable faces {:>5}  degenerate {:>3}  no model {}  no walkmesh {}",
                    stats.rooms,
                    level.brushes,
                    loaded.build_ms,
                    loaded.collision.walkable.len(),
                    stats.degenerate,
                    stats.rooms_without_model.len(),
                    stats.rooms_without_walkmesh.len()
                );
                rows.push(json!({
                    "module": name,
                    "rooms": stats.rooms,
                    "brushes": level.brushes,
                    "planes": level.planes,
                    "build_ms": loaded.build_ms,
                    "walkable_faces": loaded.collision.walkable.len(),
                    "degenerate": stats.degenerate,
                    "rooms_without_model": stats.rooms_without_model,
                    "rooms_without_walkmesh": stats.rooms_without_walkmesh,
                    "digest": loaded.collision.level.digest(),
                }));
            }
            Err(error) => {
                outln!("{name:<14} refused: {error}");
                refused.push(json!({ "module": name, "error": error }));
            }
        }
    }
    outln!(
        "{} modules built, {} refused; {total_brushes} brushes in {:.1} s of building",
        rows.len(),
        refused.len(),
        total_ms / 1000.0
    );
    census.print();
    write_facts(json!({
        "verb": "collision build all",
        "game": key.key(),
        "modules": rows,
        "refused": refused,
        "walkmeshes": census.facts(),
    }));
    if refused.is_empty() {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}

/// Every door's and placeable's walkmesh over a game's modules, by model:
/// whether it rises (blocks a JA box by its own faces) or is a flat
/// footprint (blocks only by the walls round it, `footprints`), and who the
/// game puts inside a footprint.
#[derive(Default)]
struct Census {
    models: std::collections::BTreeMap<(String, String), CensusModel>,
    /// Module, who (kind and tag), and the footprint's placeable tag.
    inside: Vec<(String, String, String)>,
}

#[derive(Default)]
struct CensusModel {
    instances: usize,
    rise: f32,
    footprint_faces: usize,
    walls: usize,
    /// Tags of instances, by module.
    modules: std::collections::BTreeMap<String, Vec<String>>,
}

/// How high a walkmesh rises before its own faces stop a walking box: no
/// more than a step.
const CENSUS_FLAT: f32 = collision::audit::STEP;

impl Census {
    fn add(&mut self, module: &str, loaded: &Loaded) {
        let area = loaded.area();
        for entity in &loaded.collision.entities {
            let row = self
                .models
                .entry((format!("{:?}", entity.kind), entity.model.clone()))
                .or_default();
            row.instances += 1;
            row.rise = row.rise.max(entity.rise);
            row.footprint_faces = entity.footprint.len();
            row.walls += entity.footprint_walls;
            row.modules.entry(module.to_string()).or_default().push(entity.tag.clone());
            if entity.footprint_walls == 0 {
                continue;
            }
            for (index, instance) in area.instances.iter().enumerate() {
                use asset_odyssey::module::Kind;
                if index as u32 == entity.entity
                    || !matches!(instance.kind, Kind::Creature | Kind::Waypoint)
                {
                    continue;
                }
                let at = instance.position;
                let over = entity.footprint.iter().any(|corners| {
                    let low = corners.iter().map(|c| c[2]).fold(f32::INFINITY, f32::min);
                    (at[2] - low).abs() < 1.0 && inside_triangle([at[0], at[1]], corners)
                });
                if over {
                    self.inside.push((
                        module.to_string(),
                        format!("{:?} {}", instance.kind, String::from_utf8_lossy(&instance.tag)),
                        entity.tag.clone(),
                    ));
                }
            }
        }
    }

    fn print(&self) {
        let flat = |row: &CensusModel| row.rise <= CENSUS_FLAT;
        for (title, pick) in [
            ("flat footprints, walked over before their walls", true),
            ("walkmeshes that rise past a step (their own faces block)", false),
        ] {
            let rows: Vec<_> = self.models.iter().filter(|(_, row)| flat(row) == pick).collect();
            let instances: usize = rows.iter().map(|(_, row)| row.instances).sum();
            outln!("{title}: {} models, {instances} instances", rows.len());
            for ((kind, model), row) in rows {
                let examples: Vec<String> = row
                    .modules
                    .iter()
                    .take(4)
                    .map(|(module, tags)| format!("{module} {}", tags[0]))
                    .collect();
                outln!(
                    "  {kind:<9} {model:<16} {:>4} in {:>3} modules, rise {:>5.2} m, {:>3} footprint faces, {:>4} walls; {}",
                    row.instances,
                    row.modules.len(),
                    row.rise,
                    row.footprint_faces,
                    row.walls,
                    examples.join(", ")
                );
            }
        }
        outln!("creatures and waypoints over a walled footprint: {}", self.inside.len());
        for (module, who, placeable) in &self.inside {
            outln!("  {module:<14} {who} over {placeable}");
        }
    }

    fn facts(&self) -> serde_json::Value {
        json!({
            "models": self.models.iter().map(|((kind, model), row)| json!({
                "kind": kind,
                "model": model,
                "instances": row.instances,
                "rise": row.rise,
                "flat": row.rise <= CENSUS_FLAT,
                "footprint_faces": row.footprint_faces,
                "walls": row.walls,
                "modules": row.modules,
            })).collect::<Vec<_>>(),
            "inside": self.inside.iter().map(|(module, who, placeable)| json!({
                "module": module, "who": who, "placeable": placeable,
            })).collect::<Vec<_>>(),
        })
    }
}

/// Whether `p` lies within the triangle's outline seen from above.
fn inside_triangle(p: [f32; 2], corners: &[[f32; 3]; 3]) -> bool {
    let side = |a: [f32; 3], b: [f32; 3]| (b[0] - a[0]) * (p[1] - a[1]) - (b[1] - a[1]) * (p[0] - a[0]);
    let [a, b, c] = *corners;
    let (d0, d1, d2) = (side(a, b), side(b, c), side(c, a));
    (d0 >= 0.0 && d1 >= 0.0 && d2 >= 0.0) || (d0 <= 0.0 && d1 <= 0.0 && d2 <= 0.0)
}

pub fn build(discovery: &Discovery, game: &str, module: &str, clip: ClipChoice) -> ExitCode {
    if module.eq_ignore_ascii_case("all") {
        return build_all(discovery, game, clip);
    }
    let loaded = match load(discovery, game, module, clip) {
        Ok(loaded) => loaded,
        Err(error) => {
            eprintln!("{error}");
            return ExitCode::FAILURE;
        }
    };
    let stats = &loaded.collision.stats;
    let level = loaded.collision.level.stats();
    let digest = loaded.collision.level.digest();
    outln!(
        "{} {}: area {}, {} rooms, built in {:.1} ms",
        loaded.game.key(),
        loaded.module.name,
        loaded.area().resref,
        stats.rooms,
        loaded.build_ms
    );
    outln!(
        "  level: {} brushes, {} planes, {} nodes, depth {}, {} entities",
        level.brushes,
        level.planes,
        level.nodes,
        level.depth,
        level.entities
    );
    outln!("  digest {digest}");
    outln!(
        "  room meshes: {} triangles ({} steep), {} on a walkmesh face",
        stats.render.triangles,
        stats.render.steep,
        stats.render_classified
    );
    outln!("  walkmesh faces by material:");
    for (material, count) in &stats.walkmesh_materials {
        outln!(
            "    {material:>2} {:<14} walk {:<5} {:>6} ({} steep)",
            loaded.surfaces.label(*material),
            loaded.surfaces.walkable(*material),
            count.triangles,
            count.steep
        );
    }
    outln!(
        "  doors {} ({} walkmeshes, {} open ones left out as placeholders), placeables {} ({} walkmeshes)",
        stats.doors,
        stats.door_walkmeshes,
        stats.door_placeholders,
        stats.placeables,
        stats.placeable_walkmeshes
    );
    outln!(
        "  clip brushes {} from {}; {} walls along open walkmesh edges, {} round placeables' footprints",
        stats.clip_brushes,
        loaded.clip_source,
        stats.edge_walls,
        stats.footprint_walls
    );
    outln!("  degenerate triangles {}", stats.degenerate);
    for (why, count) in &stats.render_skipped {
        outln!("  skipped {count}: {why}");
    }
    for room in &stats.rooms_without_model {
        outln!("  room without a model: {room}");
    }
    for room in &stats.rooms_without_walkmesh {
        outln!("  room without a walkmesh: {room}");
    }
    for entity in &loaded.collision.entities {
        outln!(
            "  entity {:>3} {:?} {} ({}) states {:03b}, rise {:.2} m, {} footprint walls",
            entity.entity,
            entity.kind,
            entity.tag,
            entity.model,
            entity.states,
            entity.rise,
            entity.footprint_walls
        );
    }
    write_facts(json!({
        "verb": "collision build",
        "game": loaded.game.key(),
        "module": loaded.module.name,
        "area": loaded.area().resref,
        "build_ms": loaded.build_ms,
        "digest": digest,
        "brushes": level.brushes,
        "planes": level.planes,
        "nodes": level.nodes,
        "depth": level.depth,
        "entities": loaded.collision.entities.iter().map(|e| json!({
            "entity": e.entity,
            "kind": format!("{:?}", e.kind),
            "tag": e.tag,
            "model": e.model,
            "states": e.states,
        })).collect::<Vec<_>>(),
        "room_triangles": stats.render.triangles,
        "room_triangles_classified": stats.render_classified,
        "walkmesh_materials": stats.walkmesh_materials.iter().map(|(m, c)| json!({
            "material": m,
            "label": loaded.surfaces.label(*m),
            "faces": c.triangles,
            "steep": c.steep,
        })).collect::<Vec<_>>(),
        "clip_brushes": stats.clip_brushes,
        "clip_source": loaded.clip_source,
        "edge_walls": stats.edge_walls,
        "footprint_walls": stats.footprint_walls,
        "degenerate": stats.degenerate,
    }));
    ExitCode::SUCCESS
}

/// Writes `facts` to `facts.json` in a new run folder, and says where.
pub fn write_facts(facts: serde_json::Value) -> Option<std::path::PathBuf> {
    let folder = match crate::runtime::create_run_folder() {
        Ok(folder) => folder,
        Err(error) => {
            eprintln!("cannot create the run folder: {error}");
            return None;
        }
    };
    let mut facts = facts;
    facts["git"] = env!("KJK_GIT_SHA").into();
    let path = folder.join("facts.json");
    let text = serde_json::to_string_pretty(&facts).expect("json values serialize");
    match std::fs::write(&path, text) {
        Ok(()) => eprintln!("facts: {}", path.display()),
        Err(error) => eprintln!("{}: {error}", path.display()),
    }
    Some(folder)
}

/// Opens (state 1) the doors `doors` names and closes (state 0) the rest;
/// returns how many are open.
pub fn set_doors(loaded: &mut Loaded, doors: Doors) -> usize {
    let area = &loaded.module.areas[loaded.area];
    collision_odyssey::set_doors(&mut loaded.collision, area, &loaded.module.name, doors)
}
