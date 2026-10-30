//! The authority state: a bevy_ecs world the sim owns, and the systems one
//! step runs over it.
//!
//! Nothing outside this module can name the world, its components or its
//! resources; the only way out is a `Snapshot`. That is the structural half
//! of "render reads no authority state": there is nothing else to read.
//!
//! Determinism rules the systems keep:
//! - one thread, one fixed system order (`chain`);
//! - anything that draws random numbers, or otherwise depends on the order
//!   entities are visited in, visits them sorted by `SimId`, by seat or by
//!   object index, never in the ECS's storage order (which a restore does
//!   not preserve);
//! - time is the step's whole milliseconds, never a clock;
//! - floats only through `+ - * /` and `sqrt`, which IEEE 754 rounds the
//!   same everywhere; no `sin`, `cos` or `powf` from the platform library
//!   (`crate::math` has the sim's own).

use std::sync::{Arc, OnceLock};

use bevy_ecs::prelude::*;
use bevy_ecs::query::QueryState;
use bevy_ecs::schedule::SingleThreadedExecutor;
use collision::nav::NavMesh;
use collision::{Bodies, Body, BoxWorld, CollisionWorld, Contents, Extents, Solid, UNIT};

use crate::action::{Action, Verb};
use crate::combat::damage::{self, Blow, FriendlyFire, Relation, Source};
use crate::combat::fighter::Location;
use crate::combat::saber::defence::BoltFate;
use crate::combat::saber::{Change, Kit, Move as SaberMove, Style, grace};
use crate::combat::sheet::{Delivery, flags};
use crate::combat::{
    Fighter, Missile, Sheath, Sheet, death, effects, force, grenades, items, location, mind, pair,
    specials, weapons,
};
use crate::combat::{gear, idle, missiles};
use crate::input::{Command, Seat, TickInput, buttons};
use crate::install::InstallError;
use crate::math::{self, scale};
use crate::module::{Global, ModuleData, ObjectData, ObjectKind, Side};
use crate::motion::{self, AnimTable, MotionState, Touches};
use crate::rng::SimRng;
use crate::walk;
pub mod arena;
#[cfg(feature = "probes")]
pub mod arena_probes;
pub mod charting;
pub mod containers;
#[cfg(feature = "probes")]
pub mod coop_items;
pub mod coop_talk;
pub mod drivers;
use crate::script::dialogue::Told;
use crate::script::effects::Hold;
#[cfg(feature = "probes")]
pub mod destroy_probes;
pub mod falls;
#[cfg(feature = "probes")]
pub mod feats;
#[cfg(feature = "probes")]
pub mod galaxy;
mod history;
pub mod item_uses;
pub mod locks;
pub mod objects;
pub mod uses;
pub use history::REWIND_MS;
pub(crate) mod covert;
mod minds;
#[cfg(feature = "probes")]
pub use minds::probes as minds_probes;
#[cfg(feature = "probes")]
pub mod aim_probes;
#[cfg(feature = "probes")]
pub mod effects_probes;
#[cfg(feature = "probes")]
pub mod floor_probes;
#[cfg(feature = "probes")]
pub mod force_probes;
mod minigame;
#[cfg(feature = "probes")]
pub mod pace_probes;
#[cfg(feature = "probes")]
pub mod pair_probes;
pub mod party;
#[cfg(feature = "probes")]
pub mod player_probes;
mod powers;
#[cfg(feature = "probes")]
pub mod presentation_probes;
#[cfg(feature = "probes")]
pub mod regen_probes;
pub mod remains;
mod sabers;
mod scene;
#[cfg(feature = "probes")]
pub mod speaker_probes;
#[cfg(feature = "probes")]
pub mod talk_end_probes;
#[cfg(feature = "probes")]
pub mod tsl_probes;
#[cfg(feature = "probes")]
pub mod weapon_set_probes;
#[cfg(feature = "probes")]
pub mod workshop_probes;
#[cfg(feature = "probes")]
pub use scene::probes as scene_probes;
pub(crate) use scene::seat_bodies;
mod scripts;
pub use scripts::HEARTBEAT_MS;
#[cfg(feature = "probes")]
pub mod balance_probes;
#[cfg(feature = "probes")]
pub mod spawner_probes;
mod spawners;
pub mod strikes;
pub mod switch;
pub(crate) mod traps;

// The world's own parts, each a file of its own so parallel changes meet
// less, their names taken here as they were before the split.
mod bolts;
mod firing;
mod harm;
mod interaction;
mod level;
mod movement;
mod state;
pub(crate) use movement::*;
pub use sabers::seen_cuts;
use {bolts::*, harm::*, state::*};
pub use {firing::*, interaction::*, level::*};

use crate::instance::Instances;
use crate::snapshot::{
    Departing, EntityState, Going, Kind, ObjectState, Place, SimId, Snapshot, Stash, Transition,
};

/// Why a step runs. All three run the same systems on the same state; only
/// what the authority alone may do differs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StepReason {
    /// The host's step: its result is the truth clients receive.
    Authority,
    /// A client running ahead of the host with its own inputs. Skips what
    /// only the authority runs (scripts from I16, the placeholder spawner,
    /// module transitions), and is corrected by the next authority
    /// snapshot.
    Predict,
    /// Re-running recorded authority steps: the same systems as `Authority`,
    /// so a replay reproduces the recording.
    Replay,
}

/// The seats a world can hold, 0 to 63: a trigger or a doorway keeps who
/// stands in it as one bit per seat (`ObjectState::tenants`).
pub const MAX_SEATS: u8 = 64;

/// What the seats past the first play (I30 P2).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Guests {
    /// A player of its own for every seat: the arena, duels and probes.
    Avatars,
    /// Seat 0's player only; the others drive the party's companions
    /// (`drivers`), or watch while none is free.
    Companions,
}

/// What a new world starts with.
#[derive(Debug, Clone, Copy)]
pub struct WorldConfig {
    pub seed: u64,
    /// Players, one per seat from 0; at most `MAX_SEATS`.
    pub seats: u8,
    /// Wanderers placed at the start.
    pub wanderers: u32,
    /// Where seat 0 stands (its soles, metres); each further seat 2 m
    /// along +X. In a module's level the module's entry places them.
    pub spawn: [f32; 3],
    /// Which way the players look at the start (degrees from +X).
    pub yaw: f32,
    /// Every player's sheet, until the party table (I19).
    pub sheet: Sheet,
    /// What the seats past the first play.
    pub guests: Guests,
}

impl WorldConfig {
    /// One dev soldier at the origin looking along +X, seed 1, no
    /// wanderers: what a probe starts a module or the arena with.
    pub fn one_seat() -> WorldConfig {
        WorldConfig {
            seed: 1,
            seats: 1,
            wanderers: 0,
            spawn: [0.0; 3],
            yaw: 0.0,
            sheet: Sheet::dev_soldier(),
            guests: Guests::Avatars,
        }
    }
}

/// Placeholder tuning, metres and milliseconds.
const WANDER_SPEED: f32 = 2.0;
/// The arena is a square this far from the origin; wanderers bounce off it.
const ARENA: f32 = 20.0;
/// The authority adds a wanderer this often, living a random while.
const SPAWN_EVERY_MS: u64 = 2000;
const LIFE_MS: (u32, u32) = (3000, 8000);

/// How far a use reaches from the eye, units.
const USE_DISTANCE: f32 = rules::interaction::using::DISTANCE;
/// How far a use reaches a creature, units: a conversation's range
/// (`script::CONVERSATION_RANGE`): KOTOR talks across a shop's counter
/// (Janice Nall's, Greeta Holda's in Anchorhead's Czerka office), farther
/// than JA's use reaches a door.
const TALK_DISTANCE: f32 = crate::script::CONVERSATION_RANGE / UNIT;
/// How far (metres) past the wall a use meets a usable placeable's middle
/// may stand: a placeable's middle can stand inside the wall it is fixed
/// to or the hull it is part of (Davik's hangar's ramp, inside the Ebon
/// Hawk's) [verify: KOTOR picks by the placeable's own model, which the
/// sim does not hold].
const USE_SLACK_M: f32 = rules::interaction::using::SLACK_M;
/// How often a held use that has met nothing tries again, milliseconds;
/// one that acted is spent (`motion_flags::USE_SPENT`).
const USE_EVERY_MS: u64 = rules::interaction::using::EVERY_MS;

#[derive(Resource)]
struct Clock {
    tick: u64,
    time_ms: u64,
    /// The current step's length.
    msec: u32,
    reason: StepReason,
}

#[derive(Resource)]
struct Streams {
    shared: SimRng,
    authority: SimRng,
}

#[derive(Resource)]
struct NextId(u32);

/// The seats the session holds (`Snapshot::seats_held`): its players and
/// its guests, a body or not.
#[derive(Resource)]
struct SeatsHeld(u8);

#[derive(Resource, Default)]
struct Inputs(TickInput);

/// The conversations as everyone sees them (`Snapshot::talk`): the
/// authority's scripts write it, a snapshot restores it, and a player held
/// in one stands still (`hold_talkers`) wherever the step runs.
#[derive(Resource, Default)]
struct Talking(Told);

/// What players see of the items in play (`Snapshot::goods`): the
/// authority's scripts gather it, a snapshot restores it.
#[derive(Resource, Default)]
struct Holding(crate::inventory::Goods);

/// The story's progress as everyone sees it (`Snapshot::progress`): the
/// authority's scripts write it, a snapshot restores it.
#[derive(Resource, Default)]
struct Progressed(crate::snapshot::Progress);

/// The pazaak matches as everyone sees them (`Snapshot::pazaak`): the
/// authority's scripts write them, a snapshot restores them.
#[derive(Resource, Default)]
struct Tabled(Vec<crate::pazaak::Match>);

/// Who each seat's player is (`Snapshot::personas`): given where a session
/// starts (`SimWorld::set_persona`), a snapshot restores it.
#[derive(Resource, Default)]
pub(crate) struct Personas(pub Vec<crate::persona::Persona>);

/// The party is in combat (`Snapshot::in_combat`): `party::combat` writes it
/// every step after the hurts, a snapshot restores it. The test KOTOR
/// refuses an area exit and a break by; the downed get up when it ends
/// (`falls`).
#[derive(Resource, Default)]
pub(crate) struct InCombat(pub bool);

#[derive(Resource)]
struct Static(Level);

/// The module the world is in, the transition asked for, the modules left.
#[derive(Resource)]
struct Module {
    place: Option<Place>,
    pending: Option<Transition>,
    stash: Stash,
    /// The area's sound as scripts set it (`snapshot::AreaSound`).
    sound: crate::snapshot::AreaSound,
    /// The screen's fade as scripts set it (`snapshot::Fade`).
    fade: Option<crate::snapshot::Fade>,
    /// The engine's fade in at the module's beginning.
    world_fade_in: crate::snapshot::WorldFadeIn,
    /// The video effect scripts enabled (`Snapshot::video_effect`).
    video_effect: Option<u16>,
    /// Every object of the area, by number: its placements and what play
    /// made (`crate::instance`).
    pub(super) instances: Instances,
    /// What the party explored of each area's map (`crate::fog`).
    explored: Vec<crate::fog::Explored>,
    /// The scripts' visual effects on its doors and placeables
    /// (`crate::visuals`).
    visuals: Vec<crate::visuals::OnObject>,
}

/// What each seat's move ran into this step (in seat order). Filled by
/// `move_players`, read by `interact`; not state.
#[derive(Resource, Default)]
struct Touched(Vec<(Seat, Touches)>);

/// Who ran into which entity this step, in move order: a closed door
/// that opens opens for that creature, its opener (`interact`). Not state.
#[derive(Resource, Default)]
struct Bumped(Vec<(SimId, u32)>);

/// The faces of the navigation mesh under the placeables play made
/// (`walk::MadeWalls`), which every walk keeps off: worked out each step
/// from the made placeables, again only when they change. Not state: a
/// client works it out from the same placeables.
#[derive(Resource, Default)]
struct MadeFaces(Arc<[bool]>, walk::MadeWalls);

/// The players a script walks this step (a move it queued while they are
/// held in a conversation, a cinematic's), their minds' commands in place
/// of their seats' (`minds::think`). Not state.
#[derive(Resource, Default)]
pub(super) struct Driven(pub Vec<SimId>);

/// Each character's torso animation as its move began this step: a shot
/// that starts the one it showed then starts it over, though the move
/// changed it meanwhile (JA's game entity's `torsoAnim`, the one last
/// sent). Not state.
#[derive(Resource, Default)]
struct TorsoSent(std::collections::BTreeMap<SimId, motion::Anim>);

#[derive(Component)]
struct Id(SimId);

#[derive(Component)]
struct Avatar(Seat);

/// A seat's own character while the seat plays another (`switch`: the
/// creature `SwitchPlayerCharacter` named): it stays where it is as a
/// character nobody plays, and the snapshot still has it as the seat's
/// avatar (`Kind::Avatar`). A world restored from a snapshot benches an
/// avatar whose seat drives a creature.
#[derive(Component)]
struct Benched(Seat);

#[derive(Component)]
struct Player(MotionState);

#[derive(Component)]
struct Wanderer {
    retarget_ms: u32,
}

#[derive(Component)]
struct Motion {
    position: [f32; 3],
    velocity: [f32; 3],
}

/// Milliseconds until the entity is removed, and how it is drawn going
/// (`Going`; none: it is not).
#[derive(Component)]
struct Lifetime {
    left_ms: u32,
    going: Option<Going>,
}

/// The things out of the world still drawn going, by id (`Departing`).
#[derive(Resource, Default)]
struct Leaving(Vec<Departing>);

/// An object of the area: its number among its instances, what changes
/// about it, and its facts at hand (none for one made from a template the
/// level lacks).
#[derive(Component)]
struct Object {
    kind: ObjectKind,
    index: u32,
    state: ObjectState,
    facts: Option<Arc<ObjectData>>,
}

/// Where a module object stands (metres).
#[derive(Component)]
struct Placed([f32; 3]);

/// A character's fight: players and creatures.
#[derive(Component)]
struct Fighting(Fighter);

/// A player's sheet (a creature's is its module's data).
#[derive(Component)]
struct PlayerSheet(Sheet);

/// A bolt in flight.
#[derive(Component)]
struct Bolt(Missile);

/// One hit made this step, waiting for `hurt`: in the order made (the
/// shooters' order, then the bolts' ids), which is the order they land in.
#[derive(Debug, Clone, Copy)]
struct Hit {
    attacker: SimId,
    target: SimId,
    /// What struck: a weapon's attack or a power (`damage::Source`).
    source: Source,
    /// Where it struck (units; none for a grip's squeeze, which has no
    /// location) and which way it pushes, if it does.
    point: Option<[f32; 3]>,
    direction: Option<[f32; 3]>,
}

/// What an on-hit property (`gear::Struck`) does to `fighter`, struck by
/// `by` at `now`: holds as C11 holds the Force's (fear's ends on damage),
/// confusion, Slow's first level, a point of an ability drained, a
/// `poison.2da` row's poison (the level's table; none where it has none), a
/// knockdown as a thermal detonator's.
fn struck_by(fighter: &mut Fighter, effect: gear::Struck, by: SimId, now: u64, level: &Level) {
    match effect {
        gear::Struck::Hold { ms, fear } => {
            if fighter.force.hold(now, ms) {
                fighter.force.hold = if fear { Hold::Fear } else { Hold::Stun };
            }
        }
        gear::Struck::Confuse(ms) => {
            fighter.force.confused_until = fighter.force.confused_until.max(now + ms);
        }
        gear::Struck::Slow(ms) => {
            if fighter.effects.slow(now) <= 1 {
                fighter.effects.slowed = (1, fighter.effects.slowed.1.max(now + ms));
            }
        }
        gear::Struck::Drain(ability) => {
            let ability = crate::combat::sheet::Ability::ALL[usize::from(ability.min(5))];
            fighter.pack.drained = (items::drain(ability, 1), now + gear::DRAIN_MS);
        }
        gear::Struck::Poison(row) => {
            if let Some(course) = level
                .scripts()
                .and_then(|library| library.poison(usize::from(row)))
                && !fighter.effects.poison_immune(now)
            {
                fighter.pack.poison = Some(items::Poisoned::new(by, course, now, None));
            }
        }
        gear::Struck::Knockdown => {
            fighter
                .saber
                .knock_down(now, crate::combat::force::ja::KNOCKDOWN_MS);
        }
        gear::Struck::Death => {}
    }
}

/// A script's push (`EffectForcePushed`, `EffectForcePushTargeted` outside
/// a cast) on a living `fighter` standing as `ps`: knocked down at `now` and
/// thrown away from `centre` (units) as JA's push throws, at its first
/// level's strength less its falloff with the distance, lifted off the
/// floor.
fn pushed(ps: &mut MotionState, fighter: &mut Fighter, centre: [f32; 3], now: u64) {
    use crate::combat::force::ja;
    if !fighter.alive() {
        return;
    }
    if !fighter.saber.floored() {
        fighter.saber.knock_down(now, ja::KNOCKDOWN_MS);
    }
    let mut direction = math::sub(ps.origin, centre);
    direction[2] = 0.0;
    let distance = math::normalize(&mut direction);
    if distance == 0.0 {
        return;
    }
    let speed = (ja::PUSH_PULL_STRENGTH - ja::SHOVE_FALLOFF * distance).max(ja::SHOVE_MIN);
    ps.velocity = [
        direction[0] * speed,
        direction[1] * speed,
        ps.velocity[2].max(ja::SHOVE_LIFT),
    ];
}

/// The step's hits; empty between steps.
#[derive(Resource, Default)]
struct Hits(Vec<Hit>);

/// The step's heals from others (a squad stim's): who, and hundredths of
/// an HP; empty between steps.
#[derive(Resource, Default)]
struct Heals(Vec<(SimId, i32)>);

/// A body's entity number in traces: its `SimId` with the top bit set,
/// apart from the level's own entities (the module's object indices).
pub const BODY: u32 = 1 << 31;

/// The body of character `id` standing as `player` is (metres): what
/// shots and blades meet.
pub fn body(id: SimId, player: &MotionState) -> Body {
    let [x, y, z] = player.origin;
    let half = motion::HALF_WIDTH;
    Body {
        entity: id.0 | BODY,
        min: scale([x - half, y - half, z + motion::SOLE_Z], UNIT),
        max: scale([x + half, y + half, z + player.maxs_z], UNIT),
        contents: Contents::BODY,
    }
}

/// `creaturespeed.2da`'s `PLAYER` and `DEFAULT` rows, as the level's
/// library has them (none without one).
fn move_rates(level: &Level) -> crate::script::MoveRates {
    level
        .scripts()
        .map_or_else(Default::default, |library| library.move_rates())
}

/// Where a standing body's box (`body`, `blocking`) puts its origin (units).
fn origin_of(body: &Body) -> [f32; 3] {
    [
        (body.min[0] + body.max[0]) * 0.5 / UNIT,
        (body.min[1] + body.max[1]) * 0.5 / UNIT,
        body.min[2] / UNIT - motion::SOLE_Z,
    ]
}

/// The box character `id` standing as `player` stops others' moves with:
/// its body (`body`), narrowed to its appearance's personal space
/// (`perspace`, metres: KOTOR's creatures keep that far apart, 0.35 m for
/// humanoids and Wookiees alike) where it has one under JA's half width.
/// Never widened [verify: a beast's larger space].
fn blocking(id: SimId, player: &MotionState, object: Option<&Object>) -> Body {
    let mut body = body(id, player);
    let space = object
        .and_then(|object| object.facts.as_ref())
        .map_or(0.0, |facts| facts.personal_space);
    if space > 0.0 && space / UNIT < motion::HALF_WIDTH {
        let [x, y, _] = player.origin;
        let half = space / UNIT;
        body.min[0] = (x - half) * UNIT;
        body.min[1] = (y - half) * UNIT;
        body.max[0] = (x + half) * UNIT;
        body.max[1] = (y + half) * UNIT;
    }
    body
}

/// The boxes object `id` collides as when play made it (metres): each cell
/// of a placeable's collision footprint (`ObjectData::geometry`, along the
/// edge of what its walkmesh covers) turned to its facing and stood where
/// it was made, the box round it, so a footprint turned off the axes is met
/// close to its edges rather than across one box round it all (Davik's
/// hangar's fallen rubble, 8 m square at 51 degrees, shut the way to the
/// Ebon Hawk as one 11 m box). The level's own objects are in its
/// collision, built before play made any.
fn made_bodies(id: SimId, object: &Object, at: [f32; 3], placed: usize) -> Vec<Body> {
    if object.kind != ObjectKind::Placeable || (object.index as usize) < placed {
        return Vec::new();
    }
    let Some(facts) = object.facts.as_ref() else {
        return Vec::new();
    };
    let (sin, cos) = math::sin_cos_deg(facts.yaw);
    facts
        .geometry
        .as_chunks::<2>()
        .0
        .iter()
        .map(|&[least, most]| {
            let (mut min, mut max) = ([f32::MAX; 3], [f32::MIN; 3]);
            for (x, y) in [
                (least[0], least[1]),
                (most[0], least[1]),
                (least[0], most[1]),
                (most[0], most[1]),
            ] {
                let turned = [at[0] + x * cos - y * sin, at[1] + x * sin + y * cos];
                for axis in 0..2 {
                    min[axis] = min[axis].min(turned[axis]);
                    max[axis] = max[axis].max(turned[axis]);
                }
            }
            // A wall from a body's soles below where it was made (a
            // location taken from a creature is its middle) to a body's
            // height over its top: a flat walkmesh is no step to climb.
            let (soles, height) = (
                motion::SOLE_Z * UNIT,
                (motion::STAND_HEIGHT - motion::SOLE_Z) * UNIT,
            );
            (min[2], max[2]) = (at[2] + least[2] + soles, at[2] + most[2] + height);
            Body {
                entity: id.0 | BODY,
                min,
                max,
                contents: Contents::SOLID,
            }
        })
        .collect()
}

/// Where a made placeable stands and the bounds of its collision cells in
/// its own frame (metres).
struct Footprint {
    /// Its body entity (`made_bodies`').
    entity: u32,
    at: [f32; 3],
    /// Its facing (degrees).
    yaw: f32,
    least: [f32; 2],
    most: [f32; 2],
}

impl Footprint {
    /// Whether `point` (metres) stands within it, seen from above.
    fn holds(&self, point: [f32; 3]) -> bool {
        let (sin, cos) = math::sin_cos_deg(self.yaw);
        let (dx, dy) = (point[0] - self.at[0], point[1] - self.at[1]);
        // Into the footprint's own frame: turned back by its facing.
        let (x, y) = (dx * cos + dy * sin, -dx * sin + dy * cos);
        (self.least[0]..=self.most[0]).contains(&x) && (self.least[1]..=self.most[1]).contains(&y)
    }
}

/// Made object `id`'s footprint, none for one with no collision cells
/// (`made_bodies`).
fn made_footprint(id: SimId, object: &Object, at: [f32; 3], placed: usize) -> Option<Footprint> {
    if object.kind != ObjectKind::Placeable || (object.index as usize) < placed {
        return None;
    }
    let facts = object.facts.as_ref()?;
    let (least, most) =
        facts
            .geometry
            .iter()
            .fold(([f32::MAX; 2], [f32::MIN; 2]), |(least, most), p| {
                (
                    [least[0].min(p[0]), least[1].min(p[1])],
                    [most[0].max(p[0]), most[1].max(p[1])],
                )
            });
    (least[0] <= most[0]).then_some(Footprint {
        entity: id.0 | BODY,
        at,
        yaw: facts.yaw,
        least,
        most,
    })
}

/// The sim's world. It changes only through `step` and `install`.
pub struct SimWorld {
    world: World,
    schedule: Schedule,
    rows: QueryState<EntityRow>,
}

impl SimWorld {
    /// Seat `persona.seat`'s player is `persona` from now on (character
    /// creation's, or the host's default for a seat without one).
    pub fn set_persona(&mut self, persona: crate::persona::Persona) {
        crate::persona::put(&mut self.world.resource_mut::<Personas>().0, persona);
    }

    /// Seat `creation.persona.seat`'s player becomes the character made
    /// (`creation`): its sheet, its career and its persona, its hit points
    /// full, and the globals a new game sets of it. Refused for a seat with
    /// no character of its own here, a level
    /// without classes, or a creation the rules refuse.
    pub fn create(&mut self, creation: &crate::creation::Creation) -> Result<(), String> {
        let seat = creation.persona.seat;
        let classes = self
            .level()
            .scripts()
            .map(|library| library.classes().clone())
            .filter(|classes| !classes.classes.is_empty())
            .ok_or("no classes in this level")?;
        let mut players = self
            .world
            .query::<(&Id, &Avatar, &mut PlayerSheet, &mut Fighting)>();
        let (id, mut sheet, mut fighting) = players
            .iter_mut(&mut self.world)
            .find(|(_, avatar, ..)| avatar.0 == seat)
            .map(|(id, _, sheet, fighting)| (id.0, sheet, fighting))
            .ok_or(format!("seat {} has no character", seat.0))?;
        let (made, career, unspent) =
            crate::creation::sheet(&classes, creation, sheet.0.faction)
                .map_err(|refused| format!("creation refused: {refused:?}"))?;
        sheet.0 = made;
        fighting.0 = Fighter::of(&made);
        let mut state = self.script_state().clone();
        state.careers.insert(id.0, career);
        if unspent > 0 {
            state.unspent.insert(id.0, unspent);
        }
        // A new game tells the campaign's scripts the made character's level
        // and alignment where its globals keep them (TSL's `G_PC_LEVEL`,
        // `G_PC_Align_Val`; K1 declares neither).
        let declared = |name: &str| {
            self.level().module().is_some_and(|module| {
                module.globals.get(&name.to_ascii_lowercase())
                    == Some(&crate::module::Global::Number)
            })
        };
        for (name, value) in [
            ("G_PC_LEVEL", made.level),
            ("G_PC_Align_Val", made.good_evil),
        ] {
            if declared(name) {
                crate::script::set_global(&mut state.numbers, name, i32::from(value));
            }
        }
        self.set_script_state(state);
        self.set_persona(creation.persona.clone());
        Ok(())
    }

    /// Seat `seat`'s player wears an item of `template` in `slot`, made for
    /// it, its sheet dressed by it (a start by hand: the arena's Jedi's
    /// saber). Refused for a seat with no character here or a template the
    /// level's library does not have.
    pub fn wear_new_item(&mut self, seat: Seat, template: &str, slot: u8) -> Result<(), String> {
        let library = self
            .level()
            .scripts()
            .cloned()
            .ok_or("the level has no scripts")?;
        let mut state = self.script_state().clone();
        let facts = state
            .item_facts(library.items(), template)
            .cloned()
            .ok_or(format!("{template}: no such item template"))?;
        let mut players = self.world.query::<(&Id, &Avatar, &mut PlayerSheet)>();
        let (id, mut sheet) = players
            .iter_mut(&mut self.world)
            .find(|(_, avatar, _)| avatar.0 == seat)
            .map(|(id, _, sheet)| (id.0, sheet))
            .ok_or(format!("seat {} has no character", seat.0))?;
        sheet.0 = crate::inventory::rewear(sheet.0, &[], &[(slot, &facts)]);
        state.make_item(library.items(), template, id.0, 1, Some(slot));
        self.set_script_state(state);
        Ok(())
    }

    /// Sets `flags` on every player's sheet (a test's god mode on a world
    /// loaded from a save).
    pub fn add_player_flags(&mut self, flags: u8) {
        let mut players = self.world.query::<(&Avatar, &mut PlayerSheet)>();
        for (_, mut sheet) in players.iter_mut(&mut self.world) {
            sheet.0.flags |= flags;
        }
    }

    /// Every player's rank in skill `row` (`skills.2da`) set to `rank`.
    pub fn set_player_skill(&mut self, row: usize, rank: u8) {
        let mut players = self.world.query::<(&Avatar, &mut PlayerSheet)>();
        for (_, mut sheet) in players.iter_mut(&mut self.world) {
            if let Some(skill) = sheet.0.skills.get_mut(row) {
                *skill = rank;
            }
        }
    }

    /// The notices since the last call, oldest first: they gather over the
    /// steps between (a caller stepping several ticks gets them all).
    pub fn take_notices(&mut self) -> Vec<(Seat, crate::notice::Notice)> {
        std::mem::take(&mut self.world.resource_mut::<crate::notice::Notices>().0)
    }

    /// What seat `seat`'s use key would act on, pressed in a command of
    /// `angles`, as the step's own press picks it (`interact`): for a driver
    /// that presses the key for one thing only, which it would otherwise
    /// spend on whatever else is before it (the playthrough's press for a
    /// closed door, beside it a button of Manaan's water, used instead).
    pub fn key_use(&mut self, seat: Seat, angles: [i16; 3]) -> Option<KeyUse> {
        let time_ms = self.world.resource::<Clock>().time_ms;
        let lent = lent(&self.world.resource::<scripts::Scripts>().state.party);
        let mut creatures = self
            .world
            .query_filtered::<(&Id, &Player, &Fighting), Without<Avatar>>();
        let mut players = self
            .world
            .query::<(&Id, &Avatar, &Player, Option<&Fighting>)>();
        let mut objects = self.world.query::<&Object>();
        let world = &self.world;
        let lent_fighter = lent.and_then(|id| {
            (creatures.iter(world))
                .find(|(of, ..)| of.0 == id)
                .map(|(_, _, fighting)| (id, &fighting.0))
        });
        let lent = lent.and_then(|id| {
            creatures
                .iter(world)
                .find(|(of, ..)| of.0 == id)
                .map(|(_, body, _)| (id, body.0))
        });
        let (own_id, own) = players
            .iter(world)
            .find(|(_, avatar, ..)| avatar.0 == seat)
            .map(|(id, _, player, _)| (id.0, player.0))?;
        let mut user = lent
            .filter(|_| seat == LEADER_SEAT)
            .map_or(own, |(_, body)| body);
        user.view = motion::view_of(angles, user.view_offset);
        let talkers = talkers(time_ms, lent.map(|(id, _)| id), creatures.iter(world));
        let seats: Vec<(Seat, SimId, Option<&Fighter>)> = (players.iter(world))
            .map(|(id, avatar, _, fighting)| (avatar.0, id.0, fighting.map(|fighting| &fighting.0)))
            .collect();
        let seated = (players
            .iter(world)
            .map(|(id, _, player, _)| (id.0, &player.0)))
        .chain(lent.as_ref().map(|(id, body)| (*id, body)));
        let who = acting_body(seat, own_id, lent.map(|(id, _)| id));
        let at_tables = (
            &world.resource::<Talking>().0,
            &world.resource::<Holding>().0,
            &world.resource::<Tabled>().0[..],
            &world.resource::<party::Embattled>().0[..],
        );
        let watchable = watchable((&seats, lent_fighter), at_tables, (who, time_ms));
        let met = with_tables(&talkers, seated, &watchable);
        let mut all: Vec<&Object> = objects.iter(world).collect();
        all.sort_by_key(|object| object.index);
        key_use(self.level(), &all, &user, &met)
    }

    /// The uses the last step heard (`interact`), as its scripts were told
    /// them: what was used, by whom, and what it came to. A driver waits on
    /// these to know a use was made, not on what changed around it.
    pub fn uses_heard(&self) -> Vec<UseHeard> {
        let used = &self.world.resource::<scripts::Used>().0;
        let opened = &self.world.resource::<scripts::Opened>().0;
        let clicked = &self.world.resource::<scripts::Clicked>().0;
        let heard = |object: SimId, by: SimId, came: UseCame| UseHeard { object, by, came };
        used.iter()
            .map(|&(object, by)| heard(object, by, UseCame::Used))
            .chain(opened.iter().filter_map(|&(object, by, ok)| {
                let came = if ok {
                    UseCame::Opened
                } else {
                    UseCame::Refused
                };
                by.map(|by| heard(object, by, came))
            }))
            .chain(
                clicked
                    .iter()
                    .map(|&(object, by)| heard(object, by, UseCame::Clicked)),
            )
            .collect()
    }

    /// The tick the world's state is the result of.
    pub fn tick(&self) -> u64 {
        self.world.resource::<Clock>().tick
    }

    /// Whether a body standing as `user` would be let use object `index`
    /// with `verb` now, as a seat's `Action::Use` is checked (`interact`,
    /// `uses::may_use`): an object that takes the verb (a creature living),
    /// within the verb's reach across the floor and `uses::HEIGHT_REACH_M`
    /// in height, its eye in sight of the object's middle (`uses::MIDDLE_M`
    /// over where it stands; a placeable's nearest action point,
    /// `uses::Target::aimed`) past nothing but the object, doors as they
    /// stand. For a driver choosing where to stand to use something.
    pub fn may_use(&mut self, user: &MotionState, index: u32, verb: Verb) -> bool {
        let mut objects = self.world.query::<(&Id, &Object)>();
        let mut bodies = self.world.query::<(&Id, &Player, &Fighting)>();
        let world = &self.world;
        let Some((id, object)) = objects
            .iter(world)
            .find(|(_, object)| object.index == index)
        else {
            return false;
        };
        let Some(facts) = object.facts.as_deref() else {
            return false;
        };
        let placed = facts.position;
        let body = bodies
            .iter(world)
            .find(|(of, ..)| of.0 == id.0)
            .map(|(_, player, fighting)| (scale(player.0.origin, UNIT), fighting.0.alive()));
        if !uses::applies(object, verb, body.is_some_and(|(_, alive)| alive)) {
            return false;
        }
        let target = uses::Target::new(index, body.map(|(at, _)| at), placed).aimed(
            object.kind,
            facts,
            scale(user.origin, UNIT),
        );
        let level = &world.resource::<Static>().0;
        let states = level.states(
            objects
                .iter(world)
                .map(|(_, object)| (object.index, &object.state)),
        );
        uses::may_use(level, states.as_deref(), user, &target, object.kind, verb)
    }

    /// How character `a` stands to character `b` now (`module::relation`:
    /// hostile, one side, an outsider), the factions as scripts left them;
    /// `None` if either is no character. Whether a creature is hostile to
    /// a seat's player: `relation(creature, player) == Some(Relation::Hostile)`.
    pub fn relation(&self, a: SimId, b: SimId) -> Option<Relation> {
        let mut actors = self.world.try_query::<(
            &Id,
            Option<&Avatar>,
            Option<&Object>,
            Option<&PlayerSheet>,
            &Fighting,
        )>()?;
        let world = &self.world;
        let mut side = |of: SimId| {
            actors.iter(world).find(|(id, ..)| id.0 == of).map(
                |(id, avatar, object, sheet, fighting)| {
                    side_of(id.0, avatar, &fighting.0, &sheet_of(sheet, object))
                },
            )
        };
        let (a, b) = (side(a)?, side(b)?);
        let level = &world.resource::<Static>().0;
        let scripts = world.resource::<scripts::Scripts>();
        let standing = level
            .module()
            .map(|module| module.repute.with(&scripts.state.reputation));
        Some(crate::module::relation(standing, a, b))
    }

    /// The level the world runs in.
    pub fn level(&self) -> &Level {
        &self.world.resource::<Static>().0
    }

    /// The modules left behind, as they were left (`Snapshot::stash`).
    pub fn stash(&self) -> &Stash {
        &self.world.resource::<Module>().stash
    }

    fn advance(&mut self, tick: u64, input: &TickInput, msec: u32, reason: StepReason) {
        {
            let mut clock = self.world.resource_mut::<Clock>();
            assert_eq!(
                tick,
                clock.tick + 1,
                "step {tick} on a world at tick {}",
                clock.tick
            );
            clock.tick = tick;
            clock.time_ms += u64::from(msec);
            clock.msec = msec;
            clock.reason = reason;
        }
        self.world.resource_mut::<Inputs>().0 = input.clone();
        self.schedule.run(&mut self.world);
    }
}

/// One tick: the world moves from `tick - 1` to `tick` over `msec`
/// milliseconds with `input`, and the result comes back as a snapshot.
/// Authority, prediction and replay all call this, differing only in
/// `reason`.
pub fn step(
    world: &mut SimWorld,
    tick: u64,
    input: &TickInput,
    msec: u32,
    reason: StepReason,
) -> Snapshot {
    world.advance(tick, input, msec, reason);
    world.snapshot()
}

fn on_authority(clock: Res<Clock>) -> bool {
    clock.reason != StepReason::Predict
}

/// The objects' collision states, in the level's order.
fn door_states(level: &Level, objects: &Query<&Object>) -> Option<Vec<u8>> {
    level.states(objects.iter().map(|object| (object.index, &object.state)))
}

/// Whether a character is in the party: a player, or a companion of one
/// (`combat::ai::Mind::leader`).
fn in_party(avatar: Option<&Avatar>, fighter: &Fighter) -> bool {
    avatar.is_some() || fighter.mind.leader.is_some()
}

/// `in_party` for a snapshot's character: with friendly fire off, the
/// party's shots and blades pass through each other (`Targets::met_by`).
pub fn party_member(entity: &EntityState) -> bool {
    matches!(entity.kind, Kind::Avatar(_))
        || entity
            .fighter
            .is_some_and(|f| f.mind.leader.is_some() || f.mind.driver.is_some())
}

/// Whether two characters' bodies pass through each other as `move_actors`
/// moves them: both of the party, with friendly fire off. A walk goes round
/// only the bodies it would run into (`walk::Walker::around`): a party's
/// walk that stepped aside from its own companions stood stuck between
/// them and a wall it could have walked through them past.
pub fn bodies_pass(level: &Level, mine: bool, theirs: bool) -> bool {
    mine && theirs && level.friendly_fire == FriendlyFire::Off
}

/// The seat whose player the party table's leader stands for: KOTOR's
/// party is the first player's.
pub(crate) const LEADER_SEAT: Seat = Seat(0);

/// The body of the companion leading the party (`SetPartyLeader`, a
/// portrait click), which its player controls as KOTOR's does, if one does.
/// It is seat 0's while it leads: no guest drives it (`drivers::assign`).
/// The first seat switched to another character (`switch`) leads through a
/// companion the same way, that character standing as its own would (TSL's
/// prologue: T3-M4 played, 3C-FD led to the security console while T3
/// waits in the garage's airlock); a switch makes the one switched to lead
/// (`script::engine`'s `SwitchPlayerCharacter`).
pub fn lent(party: &crate::script::party::Party) -> Option<SimId> {
    party
        .leader
        .filter(|npc| party.has(*npc) && party.switched != Some(*npc))
        .and_then(|npc| party.available.get(&npc)?.object)
        .map(SimId)
}

/// The body seat `seat`'s commands act through: its own (`own`: its
/// avatar, or the companion it drives), or, for the leader's seat, the
/// companion leading the party (`lent`).
pub(crate) fn acting_body(seat: Seat, own: SimId, lent: Option<SimId>) -> SimId {
    lent.filter(|_| seat == LEADER_SEAT).unwrap_or(own)
}

/// The host's two bodies as the scripts' `View::host` has them, from the
/// body holding the first seat's avatar (`seat_0`: its character, or the
/// one it is switched to): that one and the body it acts through
/// (`acting_body`).
pub(crate) fn host_bodies(
    seat_0: Option<SimId>,
    lent: Option<SimId>,
) -> [script_ncs::vm::ObjectId; 2] {
    use script_ncs::vm::ObjectId;
    seat_0.map_or([ObjectId::INVALID; 2], |pc| {
        let acting = acting_body(LEADER_SEAT, pc, lent);
        [ObjectId(pc.0), ObjectId(acting.0)]
    })
}

/// What a character acts on this step: a seat its player's command (one
/// without a command this tick keeps looking where it looked and presses
/// nothing), a creature what its mind chose (or, predicting, last), and a
/// party member a guest drives its mind's while that is steered
/// (`Mind::steered`, carried by the snapshot: predicting, its last, as a
/// creature's); the dead and the stunned press nothing.
fn command_for(
    inputs: &TickInput,
    avatar: Option<&Avatar>,
    player: &MotionState,
    fighter: &Fighter,
    now: u64,
) -> Command {
    let idle = Command {
        angles: player.command_angles,
        ..Command::default()
    };
    let mut command = match avatar {
        _ if !fighter.alive() || fighter.force.stunned(now) => idle,
        Some(_) if fighter.mind.steered => fighter.last_command,
        Some(avatar) => inputs.get(avatar.0).copied().unwrap_or(idle),
        None => fighter.last_command,
    };
    // No jumping while aiming a Sniper Shot (JA's disruptor zoom).
    if fighter.shooting.aiming {
        command.up = command.up.min(0);
    }
    command
}

/// How near a player a creature must be for prediction to move it (units:
/// 16 m, far more than anything covers between two snapshots).
const PREDICT_NEAR: f32 = 640.0;

/// How far (units, from above) a body goes in a step before its walk counts
/// as carrying it (`move_actors`): a tenth of a metre a second at 125 Hz.
const WALKED: f32 = 0.1 / UNIT / crate::TICK_HZ as f32;

/// A character's sheet: a player's own, or one a creature's equipment
/// changed; else a creature's from its facts (a commoner's when they could
/// not be read).
fn sheet_of(sheet: Option<&PlayerSheet>, object: Option<&Object>) -> Sheet {
    match (sheet, object) {
        (Some(sheet), _) => sheet.0,
        (None, Some(object)) => object
            .facts
            .as_ref()
            .and_then(|facts| facts.sheet)
            .unwrap_or_else(Sheet::commoner),
        (None, None) => Sheet::commoner(),
    }
}

/// The faction a character fights for: the one a script set
/// (`ChangeToStandardFaction`, any creature's but the player's own; joining
/// the party sets the player's), else its sheet's
/// (a player's sheet is the player's faction).
fn faction_of(fighter: &Fighter, sheet: &Sheet) -> u8 {
    fighter.faction.unwrap_or(sheet.faction)
}

/// A character as `module::relation` reads it.
fn side_of(id: SimId, avatar: Option<&Avatar>, fighter: &Fighter, sheet: &Sheet) -> Side {
    Side {
        id: id.0,
        faction: faction_of(fighter, sheet),
        party: in_party(avatar, fighter),
    }
}

pub(crate) use crate::combat::COMBAT_MS;

/// Wanderers in id order, each picking a new heading when its time is up.
fn wander(
    clock: Res<Clock>,
    mut streams: ResMut<Streams>,
    mut wanderers: Query<(&Id, &mut Wanderer, &mut Motion)>,
) {
    let mut wanderers: Vec<_> = wanderers.iter_mut().collect();
    wanderers.sort_by_key(|(id, _, _)| id.0);
    for (_, mut wanderer, mut motion) in wanderers {
        wanderer.retarget_ms = wanderer.retarget_ms.saturating_sub(clock.msec);
        if wanderer.retarget_ms > 0 {
            continue;
        }
        let (x, y) = (
            streams.shared.range(-1.0, 1.0),
            streams.shared.range(-1.0, 1.0),
        );
        let length = (x * x + y * y).sqrt().max(1e-3);
        motion.velocity = [x / length * WANDER_SPEED, y / length * WANDER_SPEED, 0.0];
        wanderer.retarget_ms = 500 + streams.shared.below(1500);
    }
}

/// Authority only: a new wanderer each time sim time passes a multiple of
/// `SPAWN_EVERY_MS`, in the placeholder world (a module has none).
fn spawn_wanderers(
    clock: Res<Clock>,
    level: Res<Static>,
    mut streams: ResMut<Streams>,
    mut next_id: ResMut<NextId>,
    mut commands: Commands,
) {
    if level.0.module.is_some() {
        return;
    }
    let before = clock.time_ms - u64::from(clock.msec);
    for _ in before / SPAWN_EVERY_MS..clock.time_ms / SPAWN_EVERY_MS {
        let rng = &mut streams.authority;
        let position = [rng.range(-ARENA, ARENA), rng.range(-ARENA, ARENA), 0.0];
        let life = LIFE_MS.0 + rng.below(LIFE_MS.1 - LIFE_MS.0);
        commands.spawn((
            Id(SimId(next_id.0)),
            Motion {
                position,
                velocity: [0.0; 3],
            },
            Wanderer { retarget_ms: 0 },
            Lifetime {
                left_ms: life,
                going: None,
            },
        ));
        next_id.0 += 1;
    }
}

/// Moves the wanderers by their velocity; the arena's walls reflect them.
fn integrate(clock: Res<Clock>, mut moving: Query<&mut Motion>) {
    let seconds = clock.msec as f32 / 1000.0;
    for mut motion in &mut moving {
        for axis in 0..3 {
            let mut position = motion.position[axis] + motion.velocity[axis] * seconds;
            if position.abs() > ARENA {
                let wall = ARENA.copysign(position);
                position = wall - (position - wall);
                motion.velocity[axis] = -motion.velocity[axis];
            }
            motion.position[axis] = position;
        }
    }
}

/// Those whose lifetime is up leave the world as the step ends (after
/// they have taken their part in it, as before departing was the view's:
/// moving it earlier moved the K1 chain's timings), each one drawn going kept
/// as it went (`Leaving`), until it has been drawn its time.
#[allow(clippy::type_complexity)]
fn expire(
    clock: Res<Clock>,
    mut commands: Commands,
    mut leaving: ResMut<Leaving>,
    mut lives: ParamSet<(Query<(Entity, &mut Lifetime)>, Query<EntityRow>)>,
) {
    leaving.0.retain_mut(|going| {
        going.left_ms = going.left_ms.saturating_sub(clock.msec);
        going.left_ms > 0
    });
    let mut gone = Vec::new();
    for (entity, mut life) in &mut lives.p0() {
        life.left_ms = life.left_ms.saturating_sub(clock.msec);
        if life.left_ms == 0 {
            commands.entity(entity).despawn();
            if let Some(Going::Departs(departure)) = life.going {
                gone.push((entity, departure));
            }
        }
    }
    if gone.is_empty() {
        return;
    }
    let rows = lives.p1();
    for (entity, departure) in gone {
        if let Ok(row) = rows.get(entity) {
            let state = entity_state(row, clock.time_ms);
            leaving.0.push(Departing::new(state, departure));
        }
    }
    leaving.0.sort_unstable_by_key(|going| going.entity.id);
}
