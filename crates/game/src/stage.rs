//! Where movement runs: a level's collision, where the player starts, and
//! the JA skeleton's animation lengths the sim times holds by.
//!
//! The course is its own collision, and so is the duel floor (`duel`: two
//! players facing off, friendly fire full); a KOTOR module (`kjk play
//! --stage end_m01aa`) is installed as a game would: planned
//! (`lifecycle`), then committed into a new world (`Stage::world`), the
//! player arriving at the module's entry or at a door or waypoint
//! (`--arrive <tag>`).

use std::sync::Arc;

use asset_odyssey::model::ModelIr;
use asset_odyssey::texture::TextureIr;
use fmt_ja::animation_cfg::AnimationConfig;
use fmt_ja::ghoul2::Gla;
use sim::combat::sheet::factions;
use sim::motion::{AnimInfo, AnimTable};
use sim::{Announcement, SimWorld, WorldConfig};
use vfs::Discovery;

use crate::lifecycle::{Installer, Prepared};
use crate::viewer::LoadedScene;
use crate::viewer_ja::Files;

/// The JA body whose skeleton and `animation.cfg` time the moves (every
/// `_humanoid` model shares them).
pub const JA_BODY: &str = "models/players/kyle/model.glm";

/// A level to move in.
pub struct Stage {
    pub name: String,
    /// The sim's level: the course's, or the first module's.
    pub level: sim::Level,
    /// Where the player's soles start (metres) and which way it looks
    /// (degrees from +X), in the course; a module places the player itself.
    pub spawn: [f32; 3],
    pub yaw: f32,
    /// What to draw of it, with its textures, if the stage makes its own.
    pub model: Option<(ModelIr, Vec<(String, TextureIr)>)>,
    /// A module's scene (rooms, doors, creatures), if the stage is one.
    pub scene: Option<Box<LoadedScene>>,
    /// A module's plan, committed by `world`, and what plans the modules
    /// its doors lead to.
    pub plan: Option<Box<session::Plan>>,
    pub installer: Option<Arc<Installer>>,
    /// The players a world on this stage has (a module's plan made room for
    /// as many at the arrival).
    pub seats: u8,
    /// Every player's sheet (the dev soldier unless `with_sheet`).
    pub sheet: sim::combat::Sheet,
    /// What the seats past the first play (`Avatars` unless `with_guests`).
    pub guests: sim::Guests,
    /// A duel: seat 1 stands this many units ahead of seat 0, facing it.
    pub face_off: Option<f32>,
}

/// How far apart the duel stage's players start (units): in reach of a
/// swing.
const DUEL_APART: f32 = 56.0;

/// The duel stage's level: its floor (`course::duel`), friendly fire full
/// (players hurt, clash with and lock each other).
pub fn duel_level(anims: Arc<AnimTable>) -> sim::Level {
    sim::Level::new("duel", Arc::new(crate::course::duel()), anims)
        .with_friendly_fire(sim::combat::FriendlyFire::Full)
}

impl Stage {
    /// The I13 test course (`course`), for `seats` players.
    pub fn course(anims: Arc<AnimTable>, seats: u8) -> Stage {
        let world = crate::course::course();
        let model = crate::course::model(&world);
        Stage {
            name: "course".into(),
            level: sim::Level::new("course", Arc::new(world), anims),
            spawn: [0.0, 0.0, 0.0],
            yaw: 90.0,
            model: Some(model),
            scene: None,
            plan: None,
            installer: None,
            seats,
            sheet: sim::combat::Sheet::dev_soldier(),
            guests: sim::Guests::Avatars,
            face_off: None,
        }
    }

    /// The duel floor (`duel_level`) for `seats` players, seat 0 at its
    /// middle looking along +X, seat 1 facing it (`face_off`).
    pub fn duel(anims: Arc<AnimTable>, seats: u8) -> Stage {
        let level = duel_level(anims);
        Stage {
            name: "duel".into(),
            model: Some(crate::course::model(&crate::course::duel())),
            level,
            spawn: [0.0; 3],
            yaw: 0.0,
            scene: None,
            plan: None,
            installer: None,
            seats,
            sheet: sim::combat::Sheet::dev_soldier(),
            guests: sim::Guests::Avatars,
            face_off: Some(DUEL_APART),
        }
    }

    /// The probes' ring (`stage::ring`) for `seats` players: what
    /// `kjk play --replay` draws a probe's bout on.
    pub fn ring(anims: Arc<AnimTable>, seats: u8) -> Stage {
        Stage {
            name: "ring".into(),
            model: Some(crate::course::model(&crate::course::ring())),
            level: crate::stage::ring(&anims),
            spawn: [0.0; 3],
            yaw: 0.0,
            scene: None,
            plan: None,
            installer: None,
            seats,
            sheet: sim::combat::Sheet::dev_soldier(),
            guests: sim::Guests::Avatars,
            face_off: None,
        }
    }

    /// A KOTOR module's entry area (K1's or TSL's, by which install has
    /// it: `installs::game_of_module`), `seats` players arriving at `arrival`
    /// (a door or waypoint tag) or its entry, its encounters planned for
    /// `players` (I30 P21); the scene only when `scene`.
    pub fn module(
        discovery: &Discovery,
        module: &str,
        arrival: Option<&str>,
        anims: Arc<AnimTable>,
        scene: bool,
        seats: u8,
        players: u8,
    ) -> Result<Stage, String> {
        let installer = crate::launch_times::timed("kotor stage/installer", || {
            let game = crate::installs::game_of_module(discovery, module);
            Installer::new(discovery, game, anims)
        })?;
        let installer = Arc::new(installer);
        let request = installer
            .request(module, arrival, seats)
            .with_players(players);
        let prepared = installer.prepare(&request, scene)?;
        prepared.record("kotor stage");
        let Prepared { plan, scene, .. } = prepared;
        let module_data = plan.level.module().cloned().expect("a plan has its module");
        // Looking the arrival's way from the start, the first commands'
        // angles are the world's.
        let (spot, _) = module_data.entry_for(arrival);
        Ok(Stage {
            name: module.to_string(),
            level: plan.level.clone(),
            spawn: spot.position,
            yaw: spot.yaw,
            model: None,
            scene: scene.map(Box::new),
            plan: Some(Box::new(plan)),
            installer: Some(installer),
            seats,
            sheet: sim::combat::Sheet::dev_soldier(),
            guests: sim::Guests::Avatars,
            face_off: None,
        })
    }

    /// The combat arena (`crate::arena`) for `seats` players, on K1's data
    /// as `arena::BASE_MODULE`'s plan has it, the arena's Jedi playing.
    pub fn arena(discovery: &Discovery, anims: Arc<AnimTable>, seats: u8) -> Result<Stage, String> {
        use crate::arena;
        let module = arena::BASE_MODULE;
        let game = crate::installs::game_of_module(discovery, module);
        let installer = Arc::new(Installer::new(discovery, game, anims.clone())?);
        let prepared = installer.prepare(&installer.request(module, None, seats), false)?;
        let level = arena::level(&prepared.plan.level, anims)?;
        let world = arena::collision();
        Ok(Stage {
            name: "arena".into(),
            model: Some(crate::course::model(&world)),
            level,
            spawn: arena::SPAWN,
            yaw: arena::YAW,
            scene: None,
            plan: None,
            installer: Some(installer),
            seats,
            sheet: arena::sheet(),
            guests: sim::Guests::Avatars,
            face_off: None,
        })
    }

    /// `course`, `duel`, or a KOTOR 1 module (e.g. `end_m01aa`), for
    /// `seats` players; a module's encounters are planned for `players`
    /// (the stages without a module have none).
    pub fn named(
        discovery: &Discovery,
        name: &str,
        arrival: Option<&str>,
        anims: Arc<AnimTable>,
        scene: bool,
        seats: u8,
        players: u8,
    ) -> Result<Stage, String> {
        match name {
            "course" => Ok(Stage::course(anims, seats)),
            "duel" => Ok(Stage::duel(anims, seats)),
            "ring" => Ok(Stage::ring(anims, seats)),
            "arena" => Stage::arena(discovery, anims, seats),
            module => Stage::module(discovery, module, arrival, anims, scene, seats, players),
        }
    }

    /// The stage with the module's creatures tagged in `hostile` and
    /// `allies` turned (`with_factions`) before its first install.
    pub fn with_factions(mut self, hostile: &[String], allies: &[String]) -> Stage {
        self.level = with_factions(&self.level, hostile, allies);
        if let Some(plan) = &mut self.plan {
            plan.level = self.level.clone();
        }
        self
    }

    /// The stage with every player as `sheet`.
    pub fn with_sheet(mut self, sheet: sim::combat::Sheet) -> Stage {
        self.sheet = sheet;
        self
    }

    /// The stage with the seats past the first playing as `guests`.
    pub fn with_guests(mut self, guests: sim::Guests) -> Stage {
        self.guests = guests;
        self
    }

    /// A new world of the stage's players: in the course at its spawn (on
    /// the duel floor facing off), in a module committed from its plan
    /// (the first install).
    pub fn world(&self, seed: u64) -> Result<(SimWorld, Option<Announcement>), String> {
        let config = WorldConfig {
            seed,
            seats: self.seats,
            wanderers: 0,
            spawn: self.spawn,
            yaw: self.yaw,
            sheet: self.sheet,
            guests: self.guests,
        };
        match &self.plan {
            None => {
                let mut world =
                    SimWorld::new(config, self.level.clone()).map_err(|e| e.to_string())?;
                if let Some(apart) = self.face_off {
                    world = face_off(&mut world, apart)?;
                }
                Ok((world, None))
            }
            Some(plan) => {
                let mut world =
                    SimWorld::new(config, sim::Level::arena()).map_err(|e| e.to_string())?;
                let announcement = plan.commit(&mut world).map_err(|e| e.to_string())?;
                Ok((world, Some(announcement)))
            }
        }
    }
}

/// `world` with seat 1 `apart` units ahead of seat 0, turned to face it
/// (a world without seat 1 as it is).
fn face_off(world: &mut SimWorld, apart: f32) -> Result<SimWorld, String> {
    let mut start = world.snapshot();
    let ps = |seat: u8| {
        start
            .entities
            .iter()
            .position(|entity| entity.kind == sim::Kind::Avatar(sim::Seat(seat)))
    };
    if let (Some(first), Some(second)) = (ps(0), ps(1))
        && let Some(me) = start.entities[first].player
    {
        let (sin, cos) = sim::math::sin_cos_deg(me.view[1]);
        let ahead = sim::math::mad(me.origin, apart, [cos, sin, 0.0]);
        start.entities[second].player = Some(sim::motion::MotionState::standing(
            ahead,
            me.view[1] + 180.0,
        ));
    }
    SimWorld::from_snapshot(&start, world.level().clone()).map_err(|e| e.to_string())
}

/// `world` with its module's creatures tagged in `tags` in the party, as
/// `AddAvailableNPCByObject` and `AddPartyMember` leave them (a party by
/// hand, until saves carry one): each a companion of the party table
/// following the first seat in the player's faction, going where it goes,
/// into the next module too. Its `NPC_*` number is the one the game's
/// `npc.2da` gives its tag (Carth is 2 in KOTOR), as the story's
/// `AddAvailableNPCByTemplate` would, so the scripts that add or remove
/// another number (the Endar Spire's take Trask, 0, away) leave it be; a
/// creature the table does not name takes the first free number. Made
/// again from its snapshot and script state, as a save loads; without
/// tags, as it is.
pub fn with_followers(
    mut world: SimWorld,
    tags: &[String],
    installer: Option<&Installer>,
) -> Result<SimWorld, String> {
    if tags.is_empty() || world.level().module().is_none() {
        return Ok(world);
    }
    let numbers = installer.map_or_else(Vec::new, npc_tags);
    let mut start = world.snapshot();
    let objects = sim::Instances::in_snapshot(world.level(), &start);
    let mut state = world.script_state().clone();
    let party = &mut state.party;
    let mut followers = 0;
    for entity in &mut start.entities {
        let Some(object) = objects.of_entity(entity).filter(|object| {
            object.kind == sim::module::ObjectKind::Creature
                && tags.iter().any(|t| t.eq_ignore_ascii_case(&object.tag))
        }) else {
            continue;
        };
        let Some(fighter) = entity.fighter.as_mut() else {
            continue;
        };
        let free = |npc: &u8| !party.available.contains_key(npc);
        let npc = numbers
            .iter()
            .position(|tag| tag.eq_ignore_ascii_case(&object.tag))
            .and_then(|row| u8::try_from(row).ok())
            .filter(free)
            .or_else(|| (0..=u8::MAX).find(free))
            .ok_or("no free NPC number")?;
        let companion = sim::script::party::Npc {
            template: object.template.to_ascii_lowercase(),
            object: None,
            kept: None,
            template_data: None,
        };
        party.available.insert(npc, companion);
        if !party.join(npc, entity.id.0) {
            return Err(format!("--follow {}: the party is full", object.tag));
        }
        fighter.mind.leader = Some(sim::Seat(0));
        fighter.faction = Some(factions::PLAYER);
        followers += 1;
    }
    if followers == 0 {
        return Err(format!(
            "--follow {}: no creature has that tag",
            tags.join(", ")
        ));
    }
    let mut world =
        SimWorld::from_snapshot(&start, world.level().clone()).map_err(|e| e.to_string())?;
    world.set_script_state(state);
    Ok(world)
}

/// The companions' tags by `NPC_*` number, from the game's `npc.2da` (its
/// row is the number); none when the table cannot be read.
pub(crate) fn npc_tags(installer: &Installer) -> Vec<String> {
    let Some(table) = installer
        .base_resolver()
        .ok()
        .and_then(|resolver| resolver.two_da("npc"))
    else {
        return Vec::new();
    };
    (0..table.cells.len())
        .map(|row| table.resref(row, "tag").unwrap_or_else(String::new))
        .collect()
}

/// A module's level with the creatures tagged in `hostile` turned to the
/// first hostile faction and those in `allies` to the player's: what a
/// trigger's `ChangeToStandardFaction` does before a fight, until scripts
/// run (I16). The level's digest changes with it. A level without a
/// module, or no tags, comes back as it is.
pub fn with_factions(level: &sim::Level, hostile: &[String], allies: &[String]) -> sim::Level {
    let Some(module) = level
        .module()
        .filter(|_| !(hostile.is_empty() && allies.is_empty()))
    else {
        return level.clone();
    };
    let tagged = |tags: &[String], tag: &str| tags.iter().any(|t| t.eq_ignore_ascii_case(tag));
    let mut data = module.as_ref().clone();
    let picked = |object: &sim::module::ObjectData| {
        object.sheet.is_some() && (tagged(hostile, &object.tag) || tagged(allies, &object.tag))
    };
    data.change_each(picked, |_, object| {
        let hostile = tagged(hostile, &object.tag);
        if let Some(sheet) = object.sheet.as_mut() {
            sheet.faction = if hostile {
                factions::HOSTILE_1
            } else {
                factions::PLAYER
            };
        }
    });
    // Everything else the level has (its navigation mesh) stays.
    level.clone().with_module(data)
}

/// The movement animations' lengths from JA's `_humanoid` skeleton, and
/// where the soles stand in each one's first frame (the talus bones, in
/// the model's frame: +X forward, +Y left, units), with the sim's
/// skeleton as `anims` has it: a recording made with this table replays
/// with that one (the level's digest covers both).
pub fn anim_table(files: &Files, gla: &Gla, cfg: &AnimationConfig) -> Result<AnimTable, String> {
    let table = table_of(gla, cfg);
    let skeleton = crate::skeleton::skeleton_data(files, &sim::combat::saber::blades::SABERS)?;
    Ok(table.with_skeleton(Arc::new(skeleton)))
}

/// `anim_table` from a loaded skeleton and its `animation.cfg`.
pub fn table_of(gla: &Gla, cfg: &AnimationConfig) -> AnimTable {
    let feet = [
        gla.bones
            .iter()
            .position(|b| b.name.eq_ignore_ascii_case("ltalus")),
        gla.bones
            .iter()
            .position(|b| b.name.eq_ignore_ascii_case("rtalus")),
    ];
    let root = gla
        .bones
        .iter()
        .position(|b| b.name.eq_ignore_ascii_case("model_root"));
    AnimTable::from_lookup(|name| {
        let sequence = cfg
            .sequences
            .iter()
            .find(|s| s.name.eq_ignore_ascii_case(name))?;
        let fps = match sequence.frames_per_second {
            0 => 1.0,
            fps => fps as f32,
        };
        let frame_lerp = if fps < 0.0 {
            (1000.0 / fps).floor()
        } else {
            (1000.0 / fps).ceil()
        } as i32;
        let feet = match (feet, root) {
            ([Some(left), Some(right)], Some(root)) => {
                let mut player = asset_ja::g2::Player::new(gla);
                player.set(root, sequence, 0.0, 0.0, 1.0);
                let pose = player.pose(0.0);
                let sole = |bone| {
                    let p = player.bone_point(&pose, bone) / asset_ja::UNIT;
                    // JA's model space faces −Y with its left at +X.
                    [-p.y, p.x]
                };
                Some([sole(left), sole(right)])
            }
            _ => None,
        };
        Some(AnimInfo {
            first_frame: sequence.first_frame,
            frames: sequence.frame_count,
            frame_lerp,
            feet,
            looping: sequence.loop_frame != -1,
        })
    })
}

/// JA's player skeleton and its `animation.cfg`, from the install.
#[cfg(feature = "probes")]
pub fn ja_skeleton(discovery: &Discovery) -> Result<(Gla, AnimationConfig), String> {
    let stack = crate::viewer_ja::stack(discovery)?;
    let files = Files::new(&stack);
    let (_, gla, gla_path) = crate::viewer_ja::glm_and_gla(&files, JA_BODY)?;
    Ok((gla, crate::viewer_ja::animation_cfg(&files, &gla_path)?))
}

/// The table from the JA install with the sim's skeleton (the hands'
/// bones and the sabers blades are placed from,
/// `sim::combat::saber::blades::SABERS`), or the stand-in (with why).
pub fn anims(discovery: &Discovery) -> (AnimTable, String) {
    let loaded = crate::viewer_ja::stack(discovery).and_then(|stack| {
        let files = Files::new(&stack);
        let (_, gla, gla_path) = crate::viewer_ja::glm_and_gla(&files, JA_BODY)?;
        let table = table_of(&gla, &crate::viewer_ja::animation_cfg(&files, &gla_path)?);
        let skeleton = crate::skeleton::skeleton_data(&files, &sim::combat::saber::blades::SABERS);
        Ok((table, skeleton))
    });
    match loaded {
        Ok((table, skeleton)) => {
            let roll = table.get(sim::motion::Anim::BOTH_ROLL_F).length_ms();
            let lengths = format!("animation lengths from JA's {JA_BODY} (BOTH_ROLL_F {roll} ms)");
            match skeleton {
                Ok(skeleton) => (
                    table.with_skeleton(Arc::new(skeleton)),
                    format!("{lengths}, its skeleton and sabers"),
                ),
                Err(error) => (table, format!("{lengths}, no skeleton ({error})")),
            }
        }
        Err(error) => (
            AnimTable::stand_in(),
            format!("stand-in animation lengths ({error})"),
        ),
    }
}

/// The floor the bouts stand on: 40 m square, the JA skeleton's lengths
/// and sabers, friendly fire full (`kjk play --replay` draws it as the
/// `ring` stage).
pub fn ring(anims: &std::sync::Arc<sim::motion::AnimTable>) -> sim::Level {
    let floor = collision::BoxWorld::floor(RING_HALF, 0.0, 1.0);
    sim::Level::new("ring", std::sync::Arc::new(floor), anims.clone())
        .with_friendly_fire(sim::combat::damage::FriendlyFire::Full)
}

/// Half the ring's side (metres).
pub(crate) const RING_HALF: f32 = 20.0;
