//! Module installs as transactions: a `Plan` resolves everything a module
//! needs (its IR, its objects as the sim's facts, its collision, where a
//! player arrives) and fails with a typed `PlanError` before anything
//! running is touched; `Plan::commit` then swaps the sim's world in one
//! call and returns the `Announcement` a remote client installs the same
//! module from (I15 carries it). A client builds its own plan from the
//! announcement (`Request::of`), checks it matches (`Plan::check`), commits,
//! and checks it reached the host's state (`Announcement::check_after`).
//!
//! Files come through `asset_odyssey::module::Source` (the resolver opened
//! with the module), so this crate does no I/O of its own. The render
//! side (the scene, the loading screen) is the host's: it plans the scene
//! from `Plan::ir` alongside.
//!
//! See `docs/LIFECYCLE.md`.

pub mod classes;
pub mod codex;
mod doors;
pub mod encounters;
pub mod imported;
mod items;
pub mod journal;
mod library;
pub mod links;
mod minigame;
mod objects;
mod pazaak;
mod sheets;
pub mod workshop;

use fmt_odyssey::ResType;
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::sync::{Arc, Mutex};
use std::time::Instant;

use asset_odyssey::module::{AreaIr, ModuleIr, ModuleRefusal, Source};
use sim::motion::AnimTable;
use sim::{Announcement, InstallError, Level, Mismatch, SimWorld, WorldConfig};

pub use library::{clip_names, clip_probes};
pub use objects::module_data;
pub use sheets::speed_probes;

/// What to install: a module of a game, arriving at a door or waypoint (or
/// the module's entry), for a world of `seats` players.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Request {
    /// `k1` or `tsl`.
    pub game: String,
    pub module: String,
    pub arrival: Option<String>,
    /// The players the world has: the plan checks there is room for all of
    /// them where they arrive.
    pub seats: u8,
    /// The players driving a body when the module is planned (I30 P21):
    /// what its encounters scale by. 1 unless set (`with_players`); a
    /// running world's plan takes `Snapshot::players_for`.
    pub players: u8,
}

impl Request {
    pub fn new(game: &str, module: &str, arrival: Option<&str>, seats: u8) -> Request {
        Request {
            game: game.to_ascii_lowercase(),
            module: module.to_ascii_lowercase(),
            arrival: arrival
                .filter(|tag| !tag.is_empty())
                .map(str::to_ascii_lowercase),
            seats,
            players: 1,
        }
    }

    /// This request, planned for `players` (at least one).
    pub fn with_players(self, players: u8) -> Request {
        Request {
            players: players.max(1),
            ..self
        }
    }

    /// The install a host announced.
    pub fn of(announcement: &Announcement) -> Request {
        Request::new(
            &announcement.game,
            &announcement.module,
            announcement.arrival(),
            announcement.seats,
        )
        .with_players(announcement.players)
    }
}

/// Why a module cannot be installed. Nothing running was touched.
#[derive(Debug, Clone, PartialEq)]
pub enum PlanError {
    /// The module's files do not make a module.
    Module(String, ModuleRefusal),
    /// It has no area.
    NoArea(String),
    /// `surfacemat.2da` does not resolve.
    NoSurfaces,
    /// A built-in clip or link file does not parse, or names what the
    /// module does not have.
    Data(String),
    /// Not every seat's player can stand where the module is entered (or
    /// the world would have more seats than it can hold).
    Arrival(InstallError),
}

impl fmt::Display for PlanError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PlanError::Module(module, refusal) => write!(f, "{module}: {refusal:?}"),
            PlanError::NoArea(module) => write!(f, "{module}: no area"),
            PlanError::NoSurfaces => f.write_str("surfacemat.2da does not resolve"),
            PlanError::Data(error) => f.write_str(error),
            PlanError::Arrival(error) => write!(f, "arrival: {error}"),
        }
    }
}

impl std::error::Error for PlanError {}

/// A module resolved and ready to commit.
pub struct Plan {
    pub request: Request,
    pub ir: ModuleIr,
    /// The entry area's index in `ir.areas`.
    pub area: usize,
    /// What the sim runs: the collision, the animation lengths and the
    /// module's objects.
    pub level: Level,
    pub facts: Facts,
}

/// What a plan found, for reports.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Facts {
    /// Objects by kind.
    pub counts: Vec<(&'static str, usize)>,
    /// Doors and triggers leading to a module (their own included):
    /// (kind, tag, module, arrival).
    pub exits: Vec<(&'static str, String, String, String)>,
    /// Link overrides applied, and event scripts recorded (not run).
    pub overridden: usize,
    /// Creatures whose sheet was read (the rest fight as commoners).
    pub sheets: usize,
    /// The encounter overrides applied for the plan's players (I28).
    pub encounter: encounters::Applied,
    pub scripts: usize,
    /// Compiled scripts the level carries (`sim::script::Library`): those
    /// the hooks name and those they name in turn; and the time to read
    /// them (ms). None without the game's `nwscript.nss`.
    pub library: Option<usize>,
    pub script_ms: f64,
    /// Item templates scripts may make (`sim::inventory::Catalog`), and the
    /// time to read them (ms).
    pub items: usize,
    pub items_ms: f64,
    /// Creature templates the party and scripts may make.
    pub creatures: usize,
    pub brushes: usize,
    /// Milliseconds: the module IR and objects, the collision and its
    /// navigation mesh.
    pub read_ms: f64,
    pub collision_ms: f64,
    /// The collision and navigation mesh came from an `AreaCache`.
    pub cached: bool,
    /// The script library's gathering came from the store.
    pub gathered_stored: bool,
    /// The loading screen's image for the module (reone: `load_<module>`,
    /// else the area's `loadscreens.2da` row), if either exists.
    pub load_screen: Option<String>,
}

/// Areas built before: each one's collision and navigation mesh (the
/// costly part of a plan), by game and module, the `AREAS_KEPT` used last.
/// Plans made through one cache take the first build (a host's planner and
/// its own client's, a module just left and gone back to); one dropped is
/// taken from the store's bake again. A process's data does not change
/// under it, so the key needs no digest; the level made from it has its
/// own. It also carries the import step's products for the install, when
/// there are (`imported`): plans take them instead of reading the shared
/// data.
#[derive(Default)]
pub struct AreaCache {
    /// The least recently used first.
    areas: Mutex<Vec<((String, String), Area)>>,
    imported: Option<Arc<imported::Imported>>,
    bakes: Option<Bakes>,
}

/// Where areas' collision and navigation are kept between runs and across
/// worktrees: the store, and the install's world data they are built from
/// (`Fingerprint::world`'s digest) for their keys (`docs/IMPORT.md`).
pub struct Bakes {
    pub store: store::Store,
    pub world: [u8; 32],
}

/// Bump when what an area's bake holds changes (with the builders' own
/// hash, `collision_odyssey::BUILDER`, and the collision formats). It names
/// the bakes' folder too, so `kjk store gc` drops the older ones.
const AREA_FORMAT: &str = "area-v6";

/// The scripts the game's own screens run, not named by any object: the
/// galaxy map's travel (`sim::script` runs it for the traveller).
const SCREEN_SCRIPTS: [&str; 1] = [sim::script::TRAVEL_SCRIPT];

/// Areas an `AreaCache` keeps in memory: the module played and the next
/// (or the one just left, gone back to). Each is tens of megabytes.
const AREAS_KEPT: usize = 2;

#[derive(Clone)]
struct Area {
    collision: Arc<collision::Level>,
    nav: Arc<collision::nav::NavMesh>,
}

impl Area {
    /// The collision's bytes, then the mesh's, each with its length, in
    /// LZ4 (about 60% of the size; a few milliseconds to read back, where
    /// deflate took 50 and more for a large area, against a build's hundred
    /// and more).
    fn encode(&self) -> Vec<u8> {
        let mut out = Vec::new();
        for part in [self.collision.encode(), self.nav.encode()] {
            out.extend((part.len() as u64).to_le_bytes());
            out.extend(part);
        }
        lz4_flex::compress_prepend_size(&out)
    }

    /// An area `encode` wrote; `None` if it does not read (it is built
    /// again).
    fn decode(bytes: &[u8]) -> Option<Area> {
        // LZ4 expands at most 255 times: a larger size is refused before
        // anything is allocated.
        let (size, _) = bytes.split_first_chunk::<4>()?;
        if u32::from_le_bytes(*size) as usize > bytes.len().saturating_mul(255) {
            return None;
        }
        let bytes = lz4_flex::decompress_size_prepended(bytes).ok()?;
        let (len, rest) = bytes.split_first_chunk::<8>()?;
        let (collision, rest) =
            rest.split_at_checked(usize::try_from(u64::from_le_bytes(*len)).ok()?)?;
        let (len, rest) = rest.split_first_chunk::<8>()?;
        let (nav, rest) = rest.split_at_checked(usize::try_from(u64::from_le_bytes(*len)).ok()?)?;
        if !rest.is_empty() {
            return None;
        }
        Some(Area {
            collision: Arc::new(collision::Level::decode(collision).ok()?),
            nav: Arc::new(collision::nav::NavMesh::decode(nav).ok()?),
        })
    }
}

impl AreaCache {
    /// A cache whose plans take `imported` (the import step's products for
    /// the install they read).
    pub fn with_imported(imported: Arc<imported::Imported>) -> AreaCache {
        AreaCache {
            imported: Some(imported),
            ..AreaCache::default()
        }
    }

    /// This cache keeping the areas it builds in `bakes` too, and taking
    /// them from there.
    pub fn with_bakes(self, bakes: Bakes) -> AreaCache {
        AreaCache {
            bakes: Some(bakes),
            ..self
        }
    }

    /// Whether it keeps areas in the store.
    pub fn bakes(&self) -> bool {
        self.bakes.is_some()
    }

    /// `request`'s area from the store, if baked there (the bake's kind and
    /// key with it, to put one built).
    fn baked(&self, request: &Request) -> (Option<Area>, Option<(String, store::Key)>) {
        let Some(bakes) = &self.bakes else {
            return (None, None);
        };
        let clearance = sim::walk::CLEARANCE.to_bits().to_le_bytes();
        // The rulebook's edition: the bodies' box, step and walkable slope
        // the area is built for, and its table changes, are the book's.
        let edition = rules::EDITION.to_le_bytes();
        let key = store::Key::of(
            &format!("{AREA_FORMAT}/{}", collision_odyssey::BUILDER),
            &[
                &bakes.world,
                request.game.as_bytes(),
                request.module.as_bytes(),
                &clearance,
                &edition,
            ],
        );
        let kind = format!("bake/{}/{AREA_FORMAT}", request.game);
        let area = bakes
            .store
            .get(&kind, &key)
            .and_then(|bytes| Area::decode(&bytes));
        (area, Some((kind, key)))
    }

    fn bake(&self, place: Option<(String, store::Key)>, area: &Area) {
        if let (Some(bakes), Some((kind, key))) = (&self.bakes, place) {
            let _ = bakes.store.put(&kind, &key, &area.encode());
        }
    }

    /// The import step's products it carries, if any.
    pub fn imported(&self) -> Option<&imported::Imported> {
        self.imported.as_deref()
    }

    /// How many areas it holds.
    pub fn len(&self) -> usize {
        self.areas.lock().map_or(0, |areas| areas.len())
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    fn get(&self, request: &Request) -> Option<Area> {
        let mut areas = self.areas.lock().ok()?;
        let at = areas
            .iter()
            .position(|((game, module), _)| *game == request.game && *module == request.module)?;
        let used = areas.remove(at);
        let area = used.1.clone();
        areas.push(used);
        Some(area)
    }

    fn put(&self, request: &Request, area: Area) {
        if let Ok(mut areas) = self.areas.lock() {
            areas.retain(|((game, module), _)| *game != request.game || *module != request.module);
            areas.push(((request.game.clone(), request.module.clone()), area));
            let over = areas.len().saturating_sub(AREAS_KEPT);
            areas.drain(..over);
        }
    }
}

/// Resolves `request` through `source` (a resolver opened with the
/// module), with the player skeleton's animation lengths.
pub fn plan(
    source: &dyn Source,
    request: &Request,
    anims: Arc<AnimTable>,
) -> Result<Plan, PlanError> {
    plan_with(source, request, anims, Overrides::PLAY, None)
}

/// Which of our own data files a plan applies over the module's.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Overrides {
    /// The module's encounter overrides (`data/encounters`): off only for
    /// comparing with the module as the game has it, never to play, as
    /// every machine must install the same.
    pub encounters: bool,
    /// The link overrides (`data/links`): ways the game keeps sealed,
    /// opened for playtests that go back through them. Off in play; every
    /// machine of a session must agree.
    pub playtest_links: bool,
}

impl Overrides {
    /// What play installs.
    pub const PLAY: Overrides = Overrides {
        encounters: true,
        playtest_links: false,
    };
}

/// `plan`, with `overrides`, its area taken from `cache` when built before
/// (and kept there when not).
pub fn plan_with(
    source: &dyn Source,
    request: &Request,
    anims: Arc<AnimTable>,
    overrides: Overrides,
    cache: Option<&AreaCache>,
) -> Result<Plan, PlanError> {
    let started = Instant::now();
    let module = request.module.as_str();
    let ir = ModuleIr::build(module, source)
        .map_err(|refusal| PlanError::Module(module.to_string(), refusal))?;
    let area = ir
        .areas
        .iter()
        .position(|area| area.resref == ir.entry_area)
        .or((!ir.areas.is_empty()).then_some(0))
        .ok_or(PlanError::NoArea(module.to_string()))?;
    let links = if overrides.playtest_links {
        links::built_in(&request.game, module).map_err(PlanError::Data)?
    } else {
        Vec::new()
    };
    let talk = source.talk();
    let (mut data, overridden) =
        objects::module_data(&request.game, &ir, area, &links, talk.as_deref())
            .map_err(PlanError::Data)?;
    doors::fill(source, &mut data, &ir.areas[area]);
    let mut tables = sheets::Tables::new(source);
    // The creature templates the module's creatures and the party and
    // scripts are made from. With the import step's products only the
    // module's own templates are read, and those that name an item the
    // module gives its own copy of; else every one the install lists.
    let started_templates = Instant::now();
    let imported = cache.and_then(AreaCache::imported);
    let own = |kind| source.own_names(kind);
    let creatures = match imported {
        Some(imported) => {
            let mut creatures = imported.templates.creatures.clone();
            let items: BTreeSet<String> = own(fmt_odyssey::ResType::UTI).into_iter().collect();
            let dressed = imported
                .templates
                .named_items
                .iter()
                .filter(|(_, named)| named.iter().any(|item| items.contains(item)))
                .map(|(name, _)| name.clone());
            let names: BTreeSet<String> = own(fmt_odyssey::ResType::UTC)
                .into_iter()
                .chain(dressed)
                .collect();
            for name in names {
                if let Some(facts) = sheets::creature_template(&mut tables, &name) {
                    creatures.insert(name, Arc::new(facts));
                }
            }
            creatures
        }
        None => sheets::creature_catalog(&mut tables, std::iter::empty()),
    };
    let templates_ms = started_templates.elapsed().as_secs_f64() * 1000.0;
    let sheets = sheets::fill(&mut tables, &mut data, &creatures);
    data.force.rooms = force_rooms(source, &ir.areas[area]);
    objects::encounters(&mut data, &ir.areas[area]);
    data.bags = sheets::bags(&mut tables, talk.as_deref());
    let changes = if overrides.encounters {
        encounters::built_in(&request.game, module).map_err(PlanError::Data)?
    } else {
        Vec::new()
    };
    let encounter = encounters::apply(&mut data, &changes, request.players);
    let read_ms = started.elapsed().as_secs_f64() * 1000.0 - templates_ms;

    let started = Instant::now();
    // This process's build, else the store's bake, else built (and kept).
    let (built, cached) = match cache.and_then(|cache| cache.get(request)) {
        Some(built) => (built, true),
        None => {
            let (baked, place) = cache.map_or((None, None), |cache| cache.baked(request));
            let from_store = baked.is_some();
            let built = match baked {
                Some(baked) => baked,
                None => build_area(source, request, &ir.areas[area])?,
            };
            if let Some(cache) = cache {
                if !from_store {
                    cache.bake(place, &built);
                }
                cache.put(request, built.clone());
            }
            (built, from_store)
        }
    };
    let collision_ms = started.elapsed().as_secs_f64() * 1000.0;
    let brushes = built.collision.brush_count();

    let exits = data
        .objects()
        .filter_map(|object| {
            let link = object.exit()?;
            Some((
                object.kind.name(),
                object.tag.clone(),
                link.module.clone(),
                link.tag.clone(),
            ))
        })
        .collect();
    let scripts = data.objects().map(|object| object.scripts.len()).sum();
    // The creatures the party and scripts may make: their hooks' scripts
    // join the library (a companion's party AI).
    let started = Instant::now();
    // The placeable templates scripts may make: the module's own (a party's
    // backpack, Kashyyyk's lift basket); the install's shared ones its
    // scripts name join once the library says which.
    let placeables: BTreeMap<String, Arc<sim::module::ObjectData>> = own(fmt_odyssey::ResType::UTP)
        .into_iter()
        .filter_map(|name| {
            let facts = sheets::placeable_template(&mut tables, &name)?;
            Some((name, Arc::new(facts)))
        })
        .collect();
    // Their hooks and conversations join the library as the creatures'
    // do; and the scripts the game's own screens run (the galaxy map's
    // travel), and the area's minigame's hooks.
    let minigame = ir.areas[area].minigame.as_ref().map(|game| {
        let per_player_hp = encounters::per_player_hp(&changes);
        minigame::rules(source, game, per_player_hp)
    });
    let made = || creatures.iter().chain(placeables.iter());
    let hooked: Vec<String> = made()
        .flat_map(|(_, facts)| &facts.scripts)
        .map(|(_, script)| script.clone())
        .chain(SCREEN_SCRIPTS.iter().map(|script| script.to_string()))
        .chain(
            minigame
                .iter()
                .flat_map(|rules| rules.scripts().map(str::to_string)),
        )
        .collect();
    let spoken: BTreeMap<String, String> = made()
        .filter(|(_, facts)| !facts.conversation.is_empty())
        .map(|(name, facts)| (name.clone(), facts.conversation.clone()))
        .collect();
    let voices = imported.map(|imported| &imported.voices);
    let bakes = cache.and_then(|cache| cache.bakes.as_ref());
    let library = library::script_library(
        source,
        &data,
        hooked,
        &spoken,
        voices,
        bakes.map(|bakes| (bakes, request)),
    );
    let script_ms = started.elapsed().as_secs_f64() * 1000.0;
    let started = Instant::now();
    let gathered_stored = library.as_ref().is_some_and(|library| library.stored);
    let library = library.map(
        |library::Library {
             library,
             strings: named,
             ..
         }| {
            let items = match imported {
                Some(imported) => {
                    let mine = items::catalog_of(&mut tables, own(fmt_odyssey::ResType::UTI));
                    imported.templates.items.clone().with(mine)
                }
                None => items::catalog(&mut tables, &data, named.iter().map(String::as_str)),
            };
            // The install's shared placeable templates its scripts name join
            // the module's own (`CreateObject` takes a template its script
            // holds); their hooks are not in the library, gathered before
            // the names are known [read them first when one needs its own].
            let shared: Vec<String> = named
                .iter()
                .filter(|name| !placeables.contains_key(*name) && source.exists(name, ResType::UTP))
                .cloned()
                .collect::<std::collections::BTreeSet<String>>()
                .into_iter()
                .collect();
            let mut placeables = placeables;
            for name in shared {
                if let Some(facts) = sheets::placeable_template(&mut tables, &name) {
                    placeables.insert(name, Arc::new(facts));
                }
            }
            let decks = pazaak::decks(&mut tables, &items);
            let library = library
                .with_pazaak(decks)
                .with_items(items)
                .with_creatures(creatures)
                .with_placeables(placeables)
                .with_quests(journal::quests(source))
                .with_experience(journal::experience(source))
                .with_classes(classes::classes(source))
                .with_workshop(workshop::workshop(source));
            match talk {
                Some(talk) => library.with_talk(talk),
                None => library,
            }
        },
    );
    let items_ms = templates_ms + started.elapsed().as_secs_f64() * 1000.0;
    let facts = Facts {
        counts: sim::counts(&data),
        exits,
        overridden,
        sheets,
        encounter,
        scripts,
        library: library.as_ref().map(|library| library.names().count()),
        script_ms,
        items: library.as_ref().map_or(0, |library| library.items().len()),
        creatures: library
            .as_ref()
            .map_or(0, |library| library.creature_count()),
        items_ms,
        brushes,
        read_ms,
        collision_ms,
        cached,
        gathered_stored,
        load_screen: load_screen(source, module, ir.areas[area].load_screen),
    };
    let mut level = Level::new(&request.module, built.collision, anims)
        .with_module(data)
        .with_nav(built.nav);
    if let Some(library) = library {
        level = level.with_scripts(library);
    }
    if let Some(rules) = minigame {
        level = level.with_minigame(rules);
    }
    // Every seat's player must be able to stand where the module is
    // entered: tried on a world of as many, before anything running is
    // touched.
    let config = WorldConfig {
        seed: 0,
        seats: request.seats,
        wanderers: 0,
        spawn: [0.0; 3],
        yaw: 0.0,
        sheet: sim::combat::Sheet::dev_soldier(),
        guests: sim::Guests::Avatars,
    };
    SimWorld::new(config, Level::arena())
        .and_then(|mut trial| trial.install(level.clone(), request.arrival.as_deref()))
        .map_err(PlanError::Arrival)?;
    Ok(Plan {
        request: request.clone(),
        ir,
        area,
        level,
        facts,
    })
}

/// An area's collision (with the module's built-in clip) and its
/// navigation mesh.
fn build_area(source: &dyn Source, request: &Request, area: &AreaIr) -> Result<Area, PlanError> {
    let clip = collision_odyssey::built_in_clip(&request.game, &request.module)
        .map_err(PlanError::Data)?;
    let surfaces = collision_odyssey::Surfaces::read(source).ok_or(PlanError::NoSurfaces)?;
    let loader = collision_odyssey::Loader { source };
    let built = collision_odyssey::build_area(
        &loader,
        area,
        &surfaces,
        clip.as_ref(),
        &collision_odyssey::BuildOptions::default(),
    );
    let walkable = built.nav_faces();
    let mut nav = collision::nav::NavMesh::build(&walkable, sim::walk::CLEARANCE * collision::UNIT);
    nav.mark(&built.level);
    Ok(Area {
        collision: Arc::new(built.level),
        nav: Arc::new(nav),
    })
}

/// TSL's rooms with their Force ratings (`sim::module::ForceRoom`), where
/// any room of `area` has one: each room in the area's order up to the
/// last rated, its walkable floor seen from above. None elsewhere (every
/// K1 area, most of TSL's).
fn force_rooms(source: &dyn Source, area: &AreaIr) -> Vec<sim::module::ForceRoom> {
    let Some(last) = area.rooms.iter().rposition(|room| room.force_rating != 0) else {
        return Vec::new();
    };
    let Some(surfaces) = collision_odyssey::Surfaces::read(source) else {
        return Vec::new();
    };
    let loader = collision_odyssey::Loader { source };
    let floors = collision_odyssey::room_floors(&loader, area, &surfaces);
    area.rooms[..=last]
        .iter()
        .zip(floors)
        .map(|(room, faces)| {
            let faces = faces
                .iter()
                .map(|face| face.map(|corner| [corner[0], corner[1]]))
                .collect();
            sim::module::ForceRoom::new(room.force_rating, faces)
        })
        .collect()
}

/// reone shows `load_<module>`; a module without one gets its area's row of
/// `loadscreens.2da` (`BmpResRef`), as the game's own table names it.
fn load_screen(source: &dyn Source, module: &str, row: Option<u32>) -> Option<String> {
    let tpc = fmt_odyssey::ResType::TPC;
    let own = format!("load_{module}");
    if source.exists(&own, tpc) {
        return Some(own);
    }
    let table = source.read("loadscreens", fmt_odyssey::ResType::TWO_DA)?;
    let table = fmt_odyssey::twoda::TwoDa::parse(&table).ok()?;
    let column = table.column("bmpresref")?;
    table.named(row.unwrap_or(0) as usize, column)
}

impl Plan {
    /// The install's commit: `world` moves into the module in one call (it
    /// is unchanged on an error), and the announcement says what happened.
    pub fn commit(&self, world: &mut SimWorld) -> Result<Announcement, InstallError> {
        let after = world.install(self.level.clone(), self.request.arrival.as_deref())?;
        Ok(
            Announcement::new(&self.level, self.request.arrival.as_deref(), &after)
                .expect("a plan's level has its module"),
        )
    }

    /// Whether this plan (made here from `announcement`) is the host's.
    pub fn check(&self, announcement: &Announcement) -> Result<(), Mismatch> {
        announcement.check(&self.level)
    }
}
