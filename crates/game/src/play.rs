//! `kjk play`: a KOTOR character on a stage, moved by JA's movement in the
//! sim and drawn from its snapshots (`play_view`). In a window the mouse
//! and keyboard drive it; headless, a `--moves` script does, and `--cmds`
//! takes the shots (`anim <seconds>` holds the moment, so shots repeat).
//! Each seat has a body of its own (`ROSTER` in turn, or `--body`), until
//! the party table (I19) picks them; `--body ja` draws JA's own player model
//! instead, playing the same pose unretargeted, to compare with. On a module stage, walking into a door
//! or trigger that leads out installs the next module (`sim_host`); its
//! scene is drawn with the same body (`compose`). The module's creatures
//! move and fight as the sim has them (`cast`, `combat_fx`); `--hostile`
//! and `--ally` turn some before the first install, to start a fight.

use std::collections::BTreeMap;
use std::sync::Arc;

use asset_odyssey::hooks;
use asset_odyssey::model::ModelIr;
use asset_odyssey::texture::TextureIr;
use fmt_ja::animation_cfg::AnimationConfig;
use fmt_ja::ghoul2::Glm;
use vfs::resolver::Resolver;
use vfs::{Discovery, Game};

use crate::cast::{BodyRig, Rig, finger_hold};
use crate::lifecycle::ModuleResolver;
use crate::play_view::{CameraMode, PlayViewSetup};
use crate::sim_host::{Driver, Lifecycle, SimSetup};
use crate::stage::{self, Stage};
use crate::viewer::{self, LoadedScene, Placed};
use crate::viewer_ja::{self, Files};
use crate::weapons::{self, Owner};
use sim::moves::Script;

/// The keyframed sounds of the humanoid animations.
const ANIM_EVENTS: &str = "models/players/_humanoid/animevents.cfg";

pub struct Options<'a> {
    pub stage: &'a str,
    /// On a module stage: the door or waypoint to arrive at (its entry
    /// without).
    pub arrive: Option<&'a str>,
    /// Each seat's KOTOR body and head models (the seats take them in
    /// turn), or `ja` for JA's own player model.
    pub bodies: Vec<(String, String)>,
    /// Scripted input; with none, the keyboard and mouse (in a window).
    pub moves: Option<&'a str>,
    /// The camera asked for; none for the player's own choice (`camera.view`).
    pub camera: Option<CameraMode>,
    pub quirks: bool,
    /// Where to write each frame's node places (CSV).
    pub trace: Option<&'a std::path::Path>,
    /// Tags of the first module's creatures turned hostile, and allied,
    /// before it is installed (`stage::with_factions`).
    pub hostile: &'a [String],
    pub allies: &'a [String],
    /// The players the world has (1 in `kjk play`; `kjk host` makes room
    /// for more).
    pub seats: u8,
    /// Every player's sheet (`--class`), until the party table (I19).
    pub sheet: sim::combat::Sheet,
    /// What the seats past the first play: a player each (`kjk play`, a
    /// joined player's own copy of the stage), or the party's companions
    /// (a hosted module).
    pub guests: sim::Guests,
    /// Draw the sim's blades beside the drawn ones (`sim_blades`).
    pub sim_blades: bool,
    /// A recording to play back on its own stage and seats instead (a
    /// probe's bout, a session's `inputs.kjkr`): its stage, players and
    /// input replace the options'.
    pub replay: Option<sim::Recording>,
    /// The seat the camera follows in `replay` (`--seat`), else seat 0.
    pub seat: u8,
    /// Seat 0's script looks at the nearest creature with this tag, the
    /// living first (`--aim`).
    pub aim: Option<&'a str>,
    /// Script globals set before the first step (`--global`): a story's
    /// state by hand until saves carry it (I19).
    pub globals: &'a [(String, i32)],
    /// The module's scripts run for the player at the end of the first
    /// step (`--run`), and the tags of its creatures that follow the player
    /// from the start (`--follow`): a story's moment and party by hand.
    pub scripts: &'a [String],
    pub followers: &'a [String],
    /// The party's gold at the start (`--gold`): credits by hand.
    pub gold: Option<i32>,
    /// The party's XP at the start (by hand, until saves load).
    pub xp: Option<i32>,
    /// Items the party carries from the start (`--item`).
    pub items: &'a [(String, u16)],
    /// The tag of an object the player talks to at the start (`--talk`).
    pub talk: Option<&'a str>,
    /// Companions' templates joined to the party at the start (`--party`).
    pub party: &'a [String],
    /// A saved game to go on from instead: its module and world replace the
    /// options' stage and arrival (the seats stay the options').
    pub save: Option<&'a sim::Save>,
    /// Told why when `save` cannot be made into a world exactly
    /// (`sim::Save::world` refused it), before the load fails with it: the
    /// menu offers the save's story then (`session_flow`).
    pub inexact: Option<&'a (dyn Fn(&str) + Sync)>,
    /// A save's story to restore instead (`saves::Story::restore`): its
    /// module the stage, the module installed fresh and the story laid over
    /// it; the save it is of is not touched.
    pub story: Option<&'a saves::Story>,
    /// The character the first seat plays as, when one was made (a new
    /// world's only: a save's players are its own).
    pub made: Option<crate::persona::Made<'a>>,
}

/// Everything a play run needs: the scene to draw, the sim to host, the
/// view's rig, and the fight's and the powers' effects.
pub struct Loaded {
    pub scene: LoadedScene,
    pub sim: SimSetup,
    pub view: PlayViewSetup,
    pub effects: crate::combat_fx::Setup,
    pub sounds: crate::sound::Bank,
    pub blades: crate::blade_fx::Setup,
    pub weapons: crate::weapon_sounds::WeaponSounds,
    pub powers: crate::force_view::Setup,
    pub hud: crate::hud::HudSetup,
}

/// The party's bodies (body, head) the seats take in turn until the party
/// table (I19) picks them: the same on every machine, so a joined player
/// draws each seat as its host does. Only `--body roster` and a seat with
/// no persona in a K1 game take them (`ROSTER_TSL` in a TSL game).
pub const ROSTER: [(&str, &str); 6] = [
    ("p_carthbb", "p_carthh"),
    ("p_bastilabb", "p_bastilah"),
    ("p_candbb", "p_candh"),
    ("p_missionbb", "p_missionh"),
    ("p_joleebb", "p_joleeh"),
    ("p_juhanibb", "p_juhanih"),
];

/// TSL's party bodies (body, head), the seats without a persona take in
/// turn in a TSL game: K1's are not all in TSL's files (no Canderous).
pub const ROSTER_TSL: [(&str, &str); 7] = [
    ("p_attonbb", "p_attonh"),
    ("p_kreiabb", "p_kreiah"),
    ("p_baodurbb", "p_baodurh"),
    ("p_mirabb", "p_mirah"),
    ("p_visasbb", "p_visash"),
    ("p_handmaidenbb", "p_handmaidenh"),
    ("p_disciplebb", "p_discipleh"),
];

/// The party bodies a seat without a persona takes: its game's, only those
/// whose body and head the install has (all of them when none are found,
/// so the missing model is still the one named).
fn stand_ins(resolver: Option<&Resolver>) -> Vec<(&'static str, &'static str)> {
    let Some(resolver) = resolver else {
        return ROSTER.to_vec();
    };
    let party: &[(&str, &str)] = match resolver.game {
        Game::Kotor2 => &ROSTER_TSL,
        _ => &ROSTER,
    };
    let has = |name: &str| resolver.find(name, fmt_odyssey::ResType::MDL).is_some();
    let found: Vec<_> = party
        .iter()
        .copied()
        .filter(|&(body, head)| has(body) && has(head))
        .collect();
    if found.is_empty() {
        party.to_vec()
    } else {
        found
    }
}

/// `ROSTER` as the options take it.
pub fn roster() -> Vec<(String, String)> {
    ROSTER
        .iter()
        .map(|&(body, head)| (body.to_string(), head.to_string()))
        .collect()
}

/// The seats' bodies from `--body` and `--head` (comma lists): none (each
/// seat's persona's) without; `roster` for the party's bodies; `ja` for
/// JA's own model; a roster body without a head takes its own.
pub fn bodies(body: &str, head: &str) -> Result<Vec<(String, String)>, String> {
    if body.is_empty() {
        return Ok(Vec::new());
    }
    if body.eq_ignore_ascii_case("roster") {
        return Ok(roster());
    }
    let mut heads = head.split(',').map(str::trim).filter(|h| !h.is_empty());
    body.split(',')
        .map(str::trim)
        .map(|body| {
            let ja = body.eq_ignore_ascii_case("ja");
            let head = heads.next().map(str::to_string).or_else(|| {
                let roster = ROSTER.iter().find(|(b, _)| b.eq_ignore_ascii_case(body));
                if ja {
                    Some(String::new())
                } else {
                    roster.map(|(_, head)| head.to_string())
                }
            });
            let head = head.ok_or(format!("--body {body}: name its head with --head"))?;
            Ok((body.to_string(), head))
        })
        .collect()
}

/// A seat's body to draw: its body and head models (`ja`: JA's own), the
/// texture its body wears (`appearance.2da`'s, by the armour worn), and
/// the metres its walk and run cycles cover (`gait`).
#[derive(Debug, Clone, PartialEq)]
pub struct Wear {
    pub body: String,
    pub head: String,
    pub texture: Option<String>,
    /// The model worn on the head (`headgear`), hung on its `gogglehook`.
    pub headgear: Option<String>,
    pub strides: [f32; 2],
}

/// The strides of a body `appearance.2da` does not give: a man's.
pub const STRIDES: [f32; 2] = [1.813, 3.96];

/// Each seat's body: `--body`'s, the seats taking them in turn; else each
/// seat's persona dressed in its body armour (`sim::persona::dressed`), a party
/// body for a seat without one.
fn wears(
    named: &[(String, String)],
    snapshot: &sim::Snapshot,
    seats: u8,
    resolver: Option<&Resolver>,
) -> Result<Vec<Wear>, String> {
    let count = usize::from(seats.max(1));
    let mut tables = asset_odyssey::tables::Tables::default();
    if !named.is_empty() {
        return Ok(named
            .iter()
            .take(count)
            .map(|(body, head)| Wear {
                body: body.clone(),
                head: head.clone(),
                texture: None,
                headgear: None,
                strides: STRIDES,
            })
            .collect());
    }
    let stand_ins = stand_ins(resolver);
    // Headgear as worn: a seat's player's choice to hide it is the
    // wardrobe's, in its first frame (`wardrobe`).
    (0..seats.max(1))
        .map(|seat| {
            let worn = resolver
                .zip(crate::headgear::seat_look(
                    snapshot,
                    sim::Seat(seat),
                    &|_| false,
                ))
                .and_then(|(resolver, worn)| crate::persona::wear(resolver, &mut tables, worn));
            Ok(worn.unwrap_or_else(|| {
                let (body, head) = stand_ins[usize::from(seat) % stand_ins.len()];
                Wear {
                    body: body.into(),
                    head: head.into(),
                    texture: None,
                    headgear: None,
                    strides: STRIDES,
                }
            }))
        })
        .collect()
}

/// What a seat's body is drawn with: the models (body first), their
/// textures and the ones missing, and the body lined up with JA's skeleton
/// (none for JA's own model).
type Body = (
    Vec<Placed>,
    BTreeMap<String, TextureIr>,
    Vec<String>,
    Option<Arc<BodyRig>>,
);

/// What a play scene's body is drawn with, kept to draw it again in each
/// module the player goes to: one for each seat.
#[derive(Clone)]
pub struct BodyScene {
    /// Each seat's models (body, head, weapons), as many for every seat.
    pub models: Vec<Vec<Placed>>,
    /// Which of a seat's models are the weapons in its right and left
    /// weapon slots.
    pub weapons: [Option<usize>; 2],
    pub seats: u8,
    /// With every saber shader the blades may use.
    pub textures: BTreeMap<String, TextureIr>,
    pub missing: Vec<String>,
    pub notes: Vec<String>,
    pub label: String,
    /// The JA sabers the KOTOR hilts' blades take after.
    pub sabers: weapons::Sabers,
    /// The seats dressed again in play (`wardrobe`), shared with it.
    pub dressed: Arc<std::sync::Mutex<Redressed>>,
}

/// What each seat wears, and the seats dressed again since the run loaded
/// (`wardrobe`): their models (as `BodyScene::models` has a seat's) and
/// textures, which the next module's scene is composed with.
#[derive(Default)]
pub struct Redressed {
    pub wears: Vec<Wear>,
    pub models: BTreeMap<u8, Vec<Placed>>,
    pub textures: BTreeMap<String, TextureIr>,
}

/// A module's scene (or none, for the course) with a body for each seat
/// after it (seat `s`'s at `index + s × body.models.len()`), each head
/// hooked to its body, and the course's model last: the scene to draw, and
/// the first body's index in it. Every character's weapons are readied to
/// be held, their blades to be lit (`weapons::arm`).
pub fn compose(
    module: Option<LoadedScene>,
    body: &BodyScene,
    course: Option<&(ModelIr, Vec<(String, TextureIr)>)>,
) -> (LoadedScene, usize) {
    let dressed = body
        .dressed
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let mut textures = body.textures.clone();
    textures.extend(dressed.textures.clone());
    let mut models = Vec::new();
    let (mut ambient, mut lit, mut rooms, mut doors, mut creature_views, mut notes) = (
        [0.2; 3],
        false,
        Vec::new(),
        Vec::new(),
        Vec::new(),
        body.notes.clone(),
    );
    let (mut fog, mut cameras, mut cutscene_models) = (None, Vec::new(), BTreeMap::new());
    let mut instance_models = BTreeMap::new();
    let mut pull_backs = Vec::new();
    let mut map = None;
    let mut own_fx = Vec::new();
    let mut grass = None;
    let mut weather = viewer::SceneWeather::default();
    let mut sound = None;
    let mut name = body.label.clone();
    // A module's scene comes first: its rooms and doors name their models
    // by index.
    if let Some(module) = module {
        name = format!("{name} in {}", module.name);
        models = module.models;
        textures.extend(module.textures);
        ambient = module.ambient;
        fog = module.fog;
        cameras = module.cameras;
        cutscene_models = module.cutscene_models;
        lit = module.lit;
        rooms = module.rooms;
        own_fx = module.own_fx;
        grass = module.grass;
        weather = module.weather;
        doors = module.doors;
        instance_models = module.instance_models;
        pull_backs = module.pull_backs;
        map = module.map;
        creature_views = module.creature_views;
        notes.extend(module.creature_notes);
        sound = module.sound;
    }
    let mut blades = Vec::new();
    let mut arms = BTreeMap::new();
    for view in &creature_views {
        let held = weapons::arm(
            &mut models,
            &mut blades,
            view.weapons,
            &body.sabers,
            &mut notes,
        );
        arms.insert(Owner::Creature(view.object), held);
    }
    let index = models.len();
    for seat in 0..body.seats.max(1) {
        let first = models.len();
        let seat_models = dressed
            .models
            .get(&seat)
            .unwrap_or(&body.models[usize::from(seat) % body.models.len().max(1)]);
        for placed in seat_models {
            let mut placed = placed.clone();
            // Hooks name the seat's own models (the headgear its head).
            if let Some((on, _)) = &mut placed.hook {
                *on += first;
            }
            models.push(placed);
        }
        let held = weapons::arm(
            &mut models,
            &mut blades,
            body.weapons.map(|slot| slot.map(|model| first + model)),
            &body.sabers,
            &mut notes,
        );
        arms.insert(Owner::Seat(seat), held);
    }
    if let Some((ir, course_textures)) = course {
        textures.extend(course_textures.iter().cloned());
        models.push(Placed {
            is_static: true,
            ..Placed::at(ir.clone(), [0.0; 3])
        });
    }
    let scene = LoadedScene {
        name,
        models,
        textures,
        missing: body.missing.clone(),
        missing_body_textures: Vec::new(),
        entry: None,
        missing_models: Vec::new(),
        rooms,
        own_fx,
        instances: 0,
        creatures: 1,
        creature_refusals: Vec::new(),
        missing_clips: Vec::new(),
        creature_notes: notes,
        weapons: 0,
        ambient,
        fog,
        cameras,
        pull_backs,
        map,
        cutscene_models,
        lit,
        blades,
        shown: Vec::new(),
        effects: viewer::Effects::default(),
        doors,
        instance_models,
        creature_views,
        arms,
        grass,
        weather,
        sound,
    };
    (scene, index)
}

pub fn load(discovery: &Discovery, options: &Options) -> Result<Loaded, String> {
    load_for(discovery, options, None)
}

/// `load`, the stage's module planned for `players` when given (a joined
/// player's copy of the host's module, I30 P21).
pub fn load_for(
    discovery: &Discovery,
    options: &Options,
    players: Option<u8>,
) -> Result<Loaded, String> {
    let script = options.moves.map(Script::parse).transpose()?;
    use crate::launch_times::timed;
    let stack = timed("ja/archives", || viewer_ja::stack(discovery))?;
    let files = Files::new(&stack);
    let (glm, gla, gla_path) = timed("ja/body and skeleton", || {
        viewer_ja::glm_and_gla(&files, stage::JA_BODY)
    })?;
    let cfg = viewer_ja::animation_cfg(&files, &gla_path)?;
    let anims = timed("ja/animation table", || {
        stage::anim_table(&files, &gla, &cfg)
    })?;
    let replay = options.replay.as_ref();
    let saved = options
        .save
        .map(|save| {
            let place = save.snapshot.place.as_ref();
            let place = place.ok_or("the save was not made in a module")?;
            let rate = sim::TickRate::new(save.hz).ok_or(format!("the save's {} Hz", save.hz))?;
            Ok::<_, String>((save, place.module.as_str(), rate))
        })
        .transpose()?;
    // A save does not fix the session's seats (I31): it holds those asked
    // for, and guests take the save's companions.
    // The players a story has keep their seats, as a save's do.
    let story_seats = options.story.map(saves::Story::seats);
    let seats = match (replay, saved, story_seats) {
        (Some(recording), ..) => recording.start.seats(),
        (None, Some((save, ..)), _) => save.seats_for(options.seats),
        (None, None, Some(own)) => options.seats.max(own),
        (None, None, None) => options.seats,
    };
    if options.seat >= seats {
        return Err(format!(
            "--seat {}: the recording has {seats} seats",
            options.seat
        ));
    }
    // The rig lives as long as the run.
    let gla: &'static fmt_ja::ghoul2::Gla = Box::leak(Box::new(gla));
    let (stage_name, arrive) = match (replay, saved, options.story) {
        (Some(recording), ..) => (recording.level.name.as_str(), options.arrive),
        (None, Some((_, module, _)), _) => (module, None),
        (None, None, Some(story)) => (story.place.module.as_str(), None),
        (None, None, None) => (options.stage, options.arrive),
    };
    // The KOTOR stage (the module's plan and scene) and JA's rig, effects
    // and sounds need nothing of each other: they load side by side.
    // A new stage's module is planned for the host alone, whoever joins
    // later (I30 P21: a guest counts from the next module planned); a
    // save's for the players its module was planned for; a joined player's
    // for the host's count.
    let players = players.unwrap_or_else(|| {
        replay
            .map(|recording| &recording.start)
            .or(saved.map(|(save, ..)| &save.snapshot))
            .and_then(|start| start.place.as_ref())
            .map(|place| place.players)
            .or(story_seats)
            .unwrap_or(1)
    });
    let (stage, ja) = std::thread::scope(|scope| {
        let stage = scope.spawn(|| {
            timed("kotor stage", || {
                Stage::named(
                    discovery,
                    stage_name,
                    arrive,
                    Arc::new(anims.clone()),
                    true,
                    seats,
                    players,
                )
            })
        });
        let ja = load_ja(&files, &cfg, &glm, gla, options.quirks);
        (stage.join().expect("the stage's thread does not panic"), ja)
    });
    let mut stage = stage?
        .with_factions(options.hostile, options.allies)
        .with_sheet(options.sheet)
        .with_guests(options.guests);
    let Ja {
        rig,
        sabers,
        blade_textures,
        lost_blade_textures,
        effects,
        powers,
        blades,
        mut sounds,
        notes: ja_notes,
    } = ja?;
    let mut notes = vec![format!(
        "stage {}; BOTH_ROLL_F held {} ms",
        stage.name,
        anims.get(sim::motion::Anim::BOTH_ROLL_F).length_ms()
    )];
    notes.extend(ja_notes);
    // KOTOR 1 for the bodies and the weapons (JA's own body on the course
    // goes without both).
    // The module stage's installer has the base layers open already.
    let base = timed("kotor resolver", || match &stage.installer {
        Some(installer) => installer.base_resolver().cloned(),
        None => viewer::resolver(discovery, Game::Kotor1, None),
    });
    // Through the first module's archives, as the armory and the wardrobe
    // read later (a module's own armour, body or weapon).
    let mut resources = ModuleResolver::new(
        base.as_ref().ok().cloned().map(Arc::new),
        stage.installer.clone(),
    );
    resources.enter(stage.level.name());
    let resolver = resources
        .get()
        .ok_or_else(|| base.err().unwrap_or_else(String::new));
    let mut cache = BTreeMap::new();
    let weapon_models = timed("weapons", || match &resolver {
        Ok(resolver) => weapons::sheet_models(resolver, &mut cache, &options.sheet, &[None, None]),
        Err(_) => Ok([None, None]),
    })?;
    let (mut world, first) = match (replay, saved, options.story) {
        (Some(recording), ..) => {
            let mut world = sim::SimWorld::from_snapshot(&recording.start, stage.level.clone())
                .map_err(|error| error.to_string())?;
            world.set_script_state(recording.script.clone());
            (world, None)
        }
        (None, Some((save, ..)), _) => {
            let made = from_save(save, &stage.level, seats, None);
            if let (Err(why), Some(inexact)) = (&made, options.inexact) {
                inexact(why);
            }
            let (world, first) = made?;
            (world, Some(first))
        }
        (None, None, Some(story)) => {
            let search = stage
                .installer
                .as_deref()
                .map(crate::lifecycle::ItemSearch::new);
            let (world, first) = from_story(story, &stage.level, seats, search.as_ref())?;
            (world, Some(first))
        }
        (None, None, None) => {
            let (world, first) = stage.world(1)?;
            let mut world =
                stage::with_followers(world, options.followers, stage.installer.as_deref())?;
            crate::by_hand::set_globals(&mut world, options.globals);
            if options.gold.is_some() || options.xp.is_some() {
                let mut state = world.script_state().clone();
                if let Some(gold) = options.gold {
                    state.gold = gold;
                }
                if let Some(xp) = options.xp {
                    state.xp.insert(sim::inventory::PARTY, xp);
                }
                world.set_script_state(state);
            }
            crate::by_hand::give_items(&mut world, options.items)?;
            // The arena's player carries every hand weapon and armour K1 has, the
            // saber in hand unless its sheet holds a blaster (`--class
            // soldier@ROW`).
            let blaster = options.sheet.weapons[0]
                .is_some_and(|w| matches!(w.delivery, sim::combat::Delivery::Blaster(_)));
            if let Some(library) = world.level().scripts().cloned()
                && world.level().arena_rules().is_some()
            {
                crate::by_hand::give_items(&mut world, &crate::arena::gear(&library))?;
                // The guns `--class soldier@ROW,ROW` names, drawn in hand as
                // the first of K1's weapons of that row by template (`g_w_`).
                let slots = [
                    sim::inventory::slots::RIGHT_WEAPON,
                    sim::inventory::slots::LEFT_WEAPON,
                ];
                let guns: Vec<(&str, u8)> = options
                    .sheet
                    .weapons
                    .iter()
                    .zip(slots)
                    .filter_map(|(weapon, slot)| {
                        let row = weapon
                            .filter(|w| matches!(w.delivery, sim::combat::Delivery::Blaster(_)))?
                            .base_item;
                        let item = library
                            .items()
                            .iter()
                            .filter(|item| !item.plot && item.base_item == row)
                            .filter(|item| item.weapon.is_some_and(|w| w.delivery.ranged()))
                            .min_by_key(|item| {
                                (!item.template.starts_with("g_w_"), &item.template)
                            })?;
                        Some((item.template.as_str(), slot))
                    })
                    .collect();
                if !guns.is_empty() {
                    world.wear_new_items(sim::Seat(0), &guns)?;
                }
            }
            if world.level().arena_rules().is_some() && !blaster {
                // A second saber in the left hand where the sheet holds two
                // (`--class duelist@dual`).
                use sim::inventory::slots::{LEFT_WEAPON, RIGHT_WEAPON};
                let mut sabers = vec![(crate::arena::SABER, RIGHT_WEAPON)];
                if options.sheet.weapons[1]
                    .is_some_and(|w| matches!(w.delivery, sim::combat::Delivery::Saber(_)))
                {
                    sabers.push((crate::arena::LEFT_SABER, LEFT_WEAPON));
                }
                world.wear_new_items(sim::Seat(0), &sabers)?;
            }
            // A companion talked to (`--party`) is there once it joins; any
            // other is talked to first, as before.
            let talk_later = !options.party.is_empty();
            if let Some(tag) = options.talk.filter(|_| !talk_later) {
                crate::by_hand::talk_to(&mut world, tag)?;
            }
            crate::by_hand::queue_scripts(&mut world, options.scripts)?;
            if let Ok(resolver) = &resolver {
                notes.extend(crate::persona::start(
                    &mut world,
                    resolver,
                    seats,
                    &options.bodies,
                    options.made,
                )?);
            }
            crate::by_hand::party(&mut world, options.party, stage.installer.as_deref())?;
            if let Some(tag) = options.talk.filter(|_| talk_later) {
                crate::by_hand::talk_to(&mut world, tag)?;
            }
            (world, first)
        }
    };
    let bodies_started = std::time::Instant::now();
    let wears = wears(
        &options.bodies,
        &world.snapshot(),
        seats,
        resolver.as_deref().ok(),
    )?;
    let (mut textures, mut missing) = (BTreeMap::new(), Vec::new());
    let (mut seat_models, mut body_rigs) = (Vec::new(), Vec::new());
    let mut seat_held = Vec::new();
    for wear in &wears {
        let ja = wear.body.eq_ignore_ascii_case("ja");
        let (mut body_models, found, lost, body_rig): Body = if ja {
            let skin = viewer_ja::skin_beside(&files, stage::JA_BODY);
            let ir = asset_ja::model(&glm, gla, &skin);
            let models = vec![Placed::at(ir, stage.spawn)];
            let (textures, missing) =
                viewer_ja::textures(&files, &viewer_ja::scene_textures(&models, &[]));
            (models, textures, missing, None)
        } else {
            let resolver = resolver.as_ref().map_err(Clone::clone)?;
            kotor_body(resolver, &mut cache, wear, &rig, &stage, &mut notes)?
        };
        textures.extend(found);
        missing.extend(lost);
        // The sheet's weapons in the body's hands (hung again as the sheet
        // holds them once the player is drawn, `cast`).
        let held = hang(&mut body_models, &weapon_models, ja.then_some(&rig.hands));
        if let Ok(resolver) = &resolver {
            let held_models = held.iter().flatten().map(|&model| &body_models[model]);
            let (found, lost) = viewer::textures(resolver, held_models);
            textures.extend(found);
            missing.extend(lost);
        }
        seat_models.push(body_models);
        seat_held.push(held);
        body_rigs.extend(body_rig);
    }
    crate::launch_times::record("bodies", bodies_started.elapsed().as_secs_f64() * 1000.0);
    // Every seat's models must line up (`compose`: seat `s` at `first + s
    // × stride`, the weapons at the same places).
    let stride = seat_models.first().map_or(0, Vec::len);
    let held = seat_held.first().copied().unwrap_or([None, None]);
    if seat_models.iter().any(|models| models.len() != stride)
        || seat_held.iter().any(|seat| *seat != held)
    {
        return Err(
            "--body: the seats' bodies differ in their models or weapon hands (ja \
             and KOTOR bodies do not mix)"
                .to_string(),
        );
    }
    textures.extend(blade_textures.clone());
    missing.extend(lost_blade_textures);
    let resolver = resolver.ok();
    let weapon_sounds = crate::weapon_sounds::WeaponSounds::read(resolver.as_deref());
    timed("kotor sounds", || {
        let files = weapon_sounds.files();
        let resrefs = crate::power_fx::kotor_sounds()
            .into_iter()
            .chain(crate::ui_sounds::resrefs())
            .chain(files.iter().map(String::as_str));
        sounds.load_kotor(
            resolver.as_deref(),
            &resrefs.collect::<Vec<_>>(),
            &mut notes,
        );
    });
    let armory = crate::armory::Setup {
        resources,
        sabers: sabers.clone(),
        blade_textures: blade_textures.clone(),
        composed: options.sheet,
    };
    let made = crate::made::Setup {
        installer: stage.installer.clone(),
        sabers: sabers.clone(),
        blade_textures,
    };
    let label: Vec<&str> = wears.iter().map(|wear| wear.body.as_str()).collect();
    let body = BodyScene {
        models: seat_models,
        weapons: held,
        seats,
        textures,
        missing,
        notes,
        label: format!("play {}", label.join(", ")),
        sabers,
        dressed: Arc::new(std::sync::Mutex::new(Redressed {
            wears: wears.clone(),
            ..Redressed::default()
        })),
    };
    let wardrobe = crate::wardrobe::Setup {
        resources: armory.resources.clone(),
        dressed: body.dressed.clone(),
        weapons: weapon_models,
        named: !options.bodies.is_empty(),
    };
    let (scene, body_index) = compose(
        stage.scene.take().map(|scene| *scene),
        &body,
        stage.model.as_ref(),
    );
    if let Some(recording) = replay {
        recording
            .level
            .check(&stage.level)
            .map_err(|error| error.to_string())?;
    }
    let level = stage.level.clone();
    // A save's player looks where it looked when saved (the camera too).
    let yaw = saved
        .and_then(|(save, ..)| save.snapshot.own(sim::Seat(0))?.player)
        .map_or(stage.yaw, |player| player.view[1]);
    let sim = SimSetup {
        setup: sim::determinism::Setup {
            world: sim::WorldConfig {
                seed: 1,
                seats,
                wanderers: 0,
                spawn: stage.spawn,
                yaw,
                sheet: stage.sheet,
                guests: stage.guests,
            },
            rate: saved.map_or_else(sim::TickRate::default, |(.., rate)| rate),
            pilot: 7,
        },
        level: level.clone(),
        driver: Driver::Player(script),
        aim: options.aim.map(str::to_string),
        recording: options.replay.clone(),
        world: Some(world),
        lifecycle: stage.installer.clone().map(|installer| Lifecycle {
            installer,
            body: Some(Arc::new(body)),
            first,
        }),
        net: crate::sim_host::Net::Solo,
        seat: sim::Seat(options.seat),
    };
    let view = PlayViewSetup {
        body: body_index,
        stride,
        seats,
        rig,
        body_rigs,
        trace: options.trace.map(std::path::Path::to_path_buf),
        level,
        camera: options.camera,
        armory,
        made,
        wardrobe,
        sim_blades: options.sim_blades,
        wields: crate::stance::Wields::read(resolver.as_deref()),
    };
    let hud = timed("hud", || {
        crate::hud::load(discovery, Game::Kotor1, stage.installer.clone())
    });
    crate::launch_times::mark("loaded");
    Ok(Loaded {
        scene,
        sim,
        view,
        effects,
        sounds,
        blades,
        weapons: weapon_sounds,
        powers,
        hud,
    })
}

/// The world going on from `save` in `level` (its module's, planned), for
/// `seats`, numbered from `tick` when given (`sim::Save::world`), logged as
/// `saves … loaded` with its tick, its hash and the whole save's (the world
/// made again and saved again: the save itself when the load changed
/// nothing, while one without a roster gains its module's);
/// and the announcement of its module, as if just installed: guests plan
/// it. The menu's load and the reload after the party fell (`sim_host`).
pub(crate) fn from_save(
    save: &sim::Save,
    level: &sim::Level,
    seats: u8,
    tick: Option<u64>,
) -> Result<(sim::SimWorld, sim::Announcement), String> {
    if let Some(restamp) = save.restamp(level)? {
        bevy::log::warn!(target: "save-migrate", "{restamp}");
    }
    let mut world = save.world(level, seats, tick)?;
    if let Some(report) = world.reconciled() {
        bevy::log::warn!(target: "save-migrate", "{report}");
        for snap in &report.snaps {
            bevy::log::warn!(target: "save-migrate", "{snap}");
        }
    }
    for line in world.dropped_code() {
        bevy::log::warn!(target: "save-migrate", "{line}");
    }
    let again = sim::Save {
        hz: save.hz,
        level: level.id(),
        snapshot: world.snapshot(),
        script: world.script_state().clone(),
    };
    bevy::log::info!(
        target: "saves",
        tick = again.snapshot.tick,
        seats = again.snapshot.seats(),
        hash = %again.snapshot.hash(),
        save = %blake3::hash(&again.encode()),
        "loaded"
    );
    let first = sim::Announcement::new(level, None, &again.snapshot)
        .ok_or("the save was not made in a module")?;
    Ok((world, first))
}

/// The world `story` restores in `level` (its module's, planned) for
/// `seats` (`saves::Story::restore`), what it lost logged as `save-migrate`
/// lines, and the announcement of its module, as if just installed.
fn from_story(
    story: &saves::Story,
    level: &sim::Level,
    seats: u8,
    search: Option<&crate::lifecycle::ItemSearch>,
) -> Result<(sim::SimWorld, sim::Announcement), String> {
    let items = |template: &str| search.and_then(|search| search.facts(template));
    let (mut world, lost) = story.restore(level, seats, &items)?;
    bevy::log::warn!(
        target: "save-migrate",
        build = story.build,
        lost = lost.len(),
        "restored from the save's story"
    );
    for line in &lost {
        bevy::log::warn!(target: "save-migrate", "{line}");
    }
    let first = sim::Announcement::new(level, None, &world.snapshot())
        .ok_or("the story was not made in a module")?;
    Ok((world, first))
}

/// JA's side of a run: the rig, the sabers, every saber colour's blade
/// textures, the combat effects, the powers' and blades' effects and their
/// sounds, and what could not be read.
struct Ja {
    rig: Rig,
    sabers: weapons::Sabers,
    blade_textures: BTreeMap<String, TextureIr>,
    lost_blade_textures: Vec<String>,
    effects: crate::combat_fx::Setup,
    powers: crate::force_view::Setup,
    blades: crate::blade_fx::Setup,
    sounds: crate::sound::Bank,
    notes: Vec<String>,
}

fn load_ja(
    files: &Files,
    cfg: &AnimationConfig,
    glm: &Glm,
    gla: &'static fmt_ja::ghoul2::Gla,
    quirks: bool,
) -> Result<Ja, String> {
    use crate::launch_times::timed;
    let mut notes = Vec::new();
    let sabers = weapons::Sabers::load(files)?;
    let events = files.read(ANIM_EVENTS).map_or_else(
        || {
            notes.push(format!(
                "{ANIM_EVENTS}: not read; animations play no sounds"
            ));
            asset_ja::events::Events::default()
        },
        |bytes| {
            asset_ja::events::parse(&String::from_utf8_lossy(&bytes), |name| {
                cfg.sequences
                    .iter()
                    .find(|s| s.name.eq_ignore_ascii_case(name))
            })
        },
    );
    let rig = Rig {
        gla,
        sequences: cfg
            .sequences
            .iter()
            .map(|s| (s.name.to_ascii_uppercase(), s.clone()))
            .collect(),
        ja_sole: crate::viewer_rig::sole_height(glm),
        quirks,
        hands: ["*r_hand", "*l_hand"].map(|tag| asset_ja::bolt(glm, tag)),
        sabers: sabers.clone(),
        events: Arc::new(events),
    };
    // Every saber colour's shaders for the blades.
    let (blade_textures, lost_blade_textures) = timed("ja/blade textures", || {
        viewer_ja::textures(files, &weapons::shaders())
    });
    let named: Vec<&'static str> = crate::blade_fx::EFFECTS
        .iter()
        .copied()
        .chain(crate::severing::EFFECTS)
        .chain(crate::landing_fx::EFFECTS)
        .collect();
    let shaders: Vec<&str> = crate::blade_fx::SHADERS
        .iter()
        .copied()
        .chain(crate::grenade_view::SHADERS.iter().copied())
        .chain(crate::saber_marks::SHADERS.iter().copied())
        .chain(crate::body_marks::TEXTURES.iter().copied())
        .collect();
    let effects = timed("ja/effects", || {
        crate::combat_fx::load(files, &named, &shaders, &mut notes)
    });
    let powers = timed("ja/powers", || crate::force_view::load(files, &mut notes));
    let blades = timed("ja/blades", || crate::blade_fx::load(files, &mut notes));
    let keyframed = rig.events.torso.iter().chain(&rig.events.legs);
    let names: Vec<String> = crate::combat_fx::sounds()
        .chain(effects.sounds().map(String::from))
        .chain(crate::power_fx::sounds())
        .chain(crate::landing_fx::sounds())
        .chain(crate::ja_footsteps::sounds())
        .chain(blades.sounds())
        .chain(keyframed.flat_map(asset_ja::events::Keyed::names))
        .collect();
    let sounds = timed("ja/sounds", || crate::sound::load(files, names, &mut notes));
    Ok(Ja {
        rig,
        sabers,
        blade_textures,
        lost_blade_textures,
        effects,
        powers,
        blades,
        sounds,
        notes,
    })
}

/// The KOTOR clip whose first frame gives the fingers' pose.
const HANDS: &str = "pause1";

/// A KOTOR body and head, retargeted from JA's skeleton, the fingers as
/// the `HANDS` clip holds them.
fn kotor_body(
    resolver: &Resolver,
    cache: &mut BTreeMap<String, Option<ModelIr>>,
    wear: &Wear,
    rig: &Rig,
    stage: &Stage,
    notes: &mut Vec<String>,
) -> Result<Body, String> {
    let dressed = dress(resolver, cache, wear, stage.spawn)?;
    notes.push(format!(
        "{}'s hands from {HANDS}: {} finger nodes; out of a fight {:?}",
        wear.body,
        dressed.hold.len(),
        dressed.gait.clips.each_ref().map(Option::is_some)
    ));
    let Dressed {
        models,
        textures,
        missing,
        hold,
        gait,
        acted,
    } = dressed;
    let body_rig = BodyRig::new(&models[0].ir, rig, hold)
        .walking(gait)
        .with_acted(acted);
    let body_rig = Arc::new(body_rig);
    Ok((models, textures, missing, Some(body_rig)))
}

/// A body as `wear` names it: its models (the body, its head hung at its
/// `headhook`), their textures and those missing, its fingers' hold
/// (`HANDS`), its clips out of a fight and those the engine plays on it.
pub(crate) struct Dressed {
    pub models: Vec<Placed>,
    pub textures: BTreeMap<String, TextureIr>,
    pub missing: Vec<String>,
    pub hold: Vec<(String, [f32; 4])>,
    pub gait: crate::gait::GaitClips,
    /// What the engine plays on a player (`acting::clips`).
    pub acted: Vec<(u16, Arc<asset_odyssey::anim::AnimationIr>)>,
}

/// A dressed KOTOR body's models: the body, its head and its headgear.
const SEAT_MODELS: usize = 3;

/// `wear`'s body (in its texture) standing at `at`, dressed.
pub(crate) fn dress(
    resolver: &Resolver,
    cache: &mut BTreeMap<String, Option<ModelIr>>,
    wear: &Wear,
    at: [f32; 3],
) -> Result<Dressed, String> {
    let (body_name, head_name) = (wear.body.as_str(), wear.head.as_str());
    let mut body =
        viewer::cached(cache, resolver, body_name)?.ok_or(format!("{body_name}.mdl: not found"))?;
    viewer::retexture(&mut body, wear.texture.as_deref());
    let head =
        viewer::cached(cache, resolver, head_name)?.ok_or(format!("{head_name}.mdl: not found"))?;
    let hook_node = body
        .node_index(hooks::HEAD_HOOK)
        .ok_or("headhook: not in the body")?;
    let hook = body.nodes[hook_node].world;
    let hands = viewer::clip(cache, resolver, Game::Kotor1, &body, HANDS)?
        .ok_or(format!("{HANDS}: in no model of {body_name}'s chain"))?;
    let mut clips = [None, None, None];
    for (clip, name) in clips.iter_mut().zip(crate::gait::NAMES) {
        *clip = viewer::clip(cache, resolver, Game::Kotor1, &body, name)?;
    }
    // What the engine plays on a player (`acting::clips`: a mine's disarm).
    let acted = viewer::scripted_clips(cache, resolver, (Game::Kotor1, false), &body)?;
    let mut models = vec![
        Placed::at(body, at),
        Placed {
            attach: Some(hook),
            hook: Some((0, hook_node)),
            ..Placed::at(head, [0.0; 3])
        },
    ];
    let absent = viewer::hang_headgear(cache, resolver, &mut models, 1, wear.headgear.as_deref())?;
    // A bare head keeps the headgear's place, empty: every seat's body is
    // as many models, its weapons at the same places (`compose`), whoever
    // wears a mask.
    if models.len() < SEAT_MODELS {
        models.push(Placed::at(
            crate::course::model_of("headgear", Vec::new()),
            at,
        ));
    }
    let (textures, mut missing) = viewer::textures(resolver, models.iter());
    missing.extend(absent.map(|name| format!("{name}.mdl")));
    Ok(Dressed {
        models,
        textures,
        missing,
        hold: finger_hold(&hands),
        gait: crate::gait::GaitClips {
            clips,
            strides: wear.strides,
        },
        acted,
    })
}

/// `weapons` hung in the hands of the body first in `models` (JA's hand
/// bolts for JA's own model): which of `models` each hand holds.
pub(crate) fn hang(
    models: &mut Vec<Placed>,
    weapons: &[Option<ModelIr>; 2],
    ja_hands: Option<&[Option<asset_ja::Bolt>; 2]>,
) -> [Option<usize>; 2] {
    let hands = weapons::hand_nodes(&models[0].ir, ja_hands);
    let mut held = [None, None];
    for (slot, ir) in weapons.iter().enumerate() {
        let (Some(ir), Some((node, local))) = (ir, hands[slot]) else {
            continue;
        };
        held[slot] = Some(models.len());
        models.push(Placed {
            hook: Some((0, node)),
            hook_local: Some(local),
            ..Placed::at(ir.clone(), [0.0; 3])
        });
    }
    held
}
