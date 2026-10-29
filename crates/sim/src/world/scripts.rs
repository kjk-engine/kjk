//! The step's scripts (I16, I17; `crate::script`): on the authority, after
//! the rest of the step, the events the world raised are queued, what is
//! due runs against the world as it then stands (`scripts`), and what the
//! scripts changed is applied (`apply`). Installs carry the scripts' state
//! over and queue the new module's entry events (`SimWorld::enter_module`).
//!
//! The events, each running the object's hook with itself calling:
//! - **spawn** (`ScriptSpawn`): each creature of a module the first time it
//!   is installed, in the module's order, before the module's own events;
//! - **heartbeat** (`ScriptHeartbeat`, `OnHeartbeat`, `Mod_OnHeartbeat`):
//!   every `HEARTBEAT_MS`, each object at its own moment in the period (by
//!   its id: they spread over the period instead of all landing on one
//!   step), the area and the module at its start; not for the dead, nor
//!   for an object whose heartbeat `RemoveHeartbeat` took away;
//! - **encounters** (`world::spawners`): their creatures made, their
//!   resets, heartbeats, and those coming into and leaving them;
//! - **trigger enter and exit** (`ScriptOnEnter`, `ScriptOnExit`): a living
//!   character, player or creature, coming into or leaving a trigger (over
//!   its outline, on its floor: `ObjectData::contains`); a trigger that
//!   leads out runs no enter script (its transition is the module's);
//! - **door open and fail to open** (`OnOpen`, `OnFailToOpen`): a door
//!   opened by a player's use, a creature running into it, or another's
//!   script action (in the next step's scripts, the door open by then); a
//!   locked door or container used; the opener is `GetLastOpenedBy`'s;
//! - **unlock and lock** (`OnUnlock`, `OnLock`): a door or placeable a
//!   key or Security unlocked (`world::locks`), or a script's action
//!   (`ActionUnlockObject`, `ActionLockObject`); who did it is
//!   `GetLastUnlocked`'s, `GetLastLocked`'s;
//! - **damaged** (`ScriptDamaged`): a character losing HP, at most once
//!   every `DAMAGED_EVERY_MS` (I22 C15), HP lost meanwhile raising it after; **death** (`ScriptDeath`): one
//!   dying; the attacker is `GetLastDamager`'s and `GetLastKiller`'s;
//! - **notice** (`ScriptOnNotice`): each living creature with senses at
//!   KOTOR's cadence (`rules::stealth::perception`, shorter while its AI
//!   is active): another character within its hearing range is heard; one within
//!   its sight range with nothing solid between its eye and the other's
//!   chest is seen, whichever way it faces (xoreos; reone's field of view
//!   is not the game's, see `perceive`); a stealthed one only past the
//!   listen and spot checks (`stealth`).
//!   Each one newly heard or seen, or no longer, runs the hook with
//!   `GetLastPerceived` and the kind (`GetLastPerceptionSeen`, ...);
//! - **attacked** (`ScriptAttacked`): a character hit, at most every
//!   `DAMAGED_EVERY_MS` per attacker (C15), before its damaged event; the
//!   attacker is `GetLastAttacker`'s;
//! - **spell cast at** (`ScriptSpellAt`; C15): a power that reached it
//!   (`SpellsAt`, from the powers step), at most every `DAMAGED_EVERY_MS`
//!   per caster; `GetLastSpellCaster`, `GetLastSpell`,
//!   `GetLastSpellHarmful`;
//! - **end of round** (`ScriptEndRound`; I22 C14): a creature in combat
//!   (`Fighter::combat_until`), every `ROUND_MS` from when it entered it.
//!   K1's handler asks `k_ai_master` for the round; its `ActionAttack`
//!   becomes the mind's target for a round (`Intent`, `world::minds`).

use std::collections::{BTreeMap, BTreeSet};

use bevy_ecs::prelude::*;
use script_ncs::vm::ObjectId;

use super::{
    Avatar, Benched, COMBAT_MS, Clock, Fighting, Id, Level, Object, Placed, Player, PlayerSheet,
    SimWorld, Static, Streams, door_states, faction_of, in_party, sheet_of,
};
use crate::action::Action;
use crate::combat::Sheath;
use crate::combat::idle::Acting;
use crate::combat::mind;
use crate::math::{self, Vec3, scale};
use crate::module::{ObjectKind, Senses};
use crate::script::actions::{Goal, Intent, ROUND_MS};
use crate::script::dialogue::{Answer, Pick};
use crate::script::{
    self, AREA, Change, Character, Hook, MODULE, Out, Pending, Perceived, Report, State, Thing,
    View, Watch, Watched, perception,
};
use crate::snapshot::Going;
use crate::snapshot::{SimId, lock};
mod seats;
use seats::*;
mod perceiving;
pub(crate) use perceiving::*;
mod apply;
pub(super) use apply::*;

/// How often objects' heartbeats run (ms).
pub const HEARTBEAT_MS: u64 = rules::world::timings::HEARTBEAT_MS;
/// How soon a weapon may flourish again (ms; TSL's
/// `CreatureFlourishWeapon`).
const FLOURISH_MS: u64 = rules::world::timings::FLOURISH_MS;
/// The most often one character's damaged script runs (ms; C15).
pub const DAMAGED_EVERY_MS: u64 = rules::world::timings::DAMAGED_EVERY_MS;

/// Doors the step opened or failed to open (`interact`): the door, who
/// (a player; none for a creature running into it), whether it opened.
#[derive(Resource, Default)]
pub(crate) struct Opened(pub Vec<(SimId, Option<SimId>, bool)>);

/// Creatures whose walk ran into a shut door this step (`interact`): who,
/// the door. Their `OnBlocked` (`GetBlockingDoor`).
#[derive(Resource, Default)]
pub(crate) struct Blocked(pub Vec<(SimId, SimId)>);

/// How often a creature held at a door that stays shut hears its
/// `OnBlocked` again (ms) [verify: the executable's retry].
const BLOCKED_EVERY_MS: u64 = rules::scripts::hooks::BLOCKED_EVERY_MS;

/// Doors players closed this step (`world::uses`): the door, who.
#[derive(Resource, Default)]
pub(crate) struct Closed(pub Vec<(SimId, SimId)>);

/// Open doors players used this step (`world::uses`): the door, who (its
/// `OnClick`, `GetClickingObject`).
#[derive(Resource, Default)]
pub(crate) struct Clicked(pub Vec<(SimId, SimId)>);

/// What players used this step (`interact`): the object, the player. A
/// creature or placeable is talked to (its dialogue hook with pattern −1,
/// else its conversation; a placeable with neither its `OnUsed`), a
/// container with an `OnUsed` as well as it opens or stays locked
/// (`uses::use_object`); a locked door with a conversation starts it after
/// its fail-to-open script, if that left it locked (`door_speaks`).
#[derive(Resource, Default)]
pub(crate) struct Used(pub Vec<(SimId, SimId)>);

/// Whom the step's powers reached (`world::powers`): the caster, the one
/// reached, the `spells.2da` row, whether harmful; for `ScriptSpellAt`
/// (what KOTOR's impact script signals, `EventSpellCastAt`). Authority only.
#[derive(Resource, Default)]
pub(crate) struct SpellsAt(pub Vec<SpellAt>);

/// The powers cast this step whose rows have impact scripts (C10): they
/// run first in the scripts' step (`script::run_due`). Authority only.
#[derive(Resource, Default)]
pub(crate) struct Casts(pub Vec<script::Cast>);

/// The scripted grips let go this step while their impact's deferred code
/// still waited (caster, `spells.2da` row, victim): that code goes with
/// them, so a choke's damage is the grip's while it holds (K1's choke, kill
/// and wound tick from deferred code, `powers::Cast::squeeze`). Authority
/// only.
#[derive(Resource, Default)]
pub(crate) struct LetGo(pub Vec<(ObjectId, u16, ObjectId)>);

/// The party members whose players ordered a Force power this step
/// (`world::powers`): each breaks TSL's `FORFEIT_NO_FORCE_POWERS` in the
/// scripts' step (`script::forfeit`). Authority only.
#[derive(Resource, Default)]
pub(crate) struct Ordered(pub Vec<SimId>);

/// The items' spells minds used as talents this step (a droid's utility
/// item: `ActionUseTalentOnObject`), by user, `spells.2da` row and target:
/// each used in the scripts' step (`script::items::State::item_talent`).
/// Authority only.
#[derive(Resource, Default)]
pub(crate) struct ItemTalents(pub Vec<(crate::snapshot::SimId, u16, crate::snapshot::SimId)>);

/// The combat rounds fighters began this step (`minds`: a melee fight
/// between creatures nobody plays runs as KOTOR's rounds): each joins the
/// scripts' blows (`script::effects::Strike`) in their step. Authority only.
#[derive(Resource, Default)]
pub(crate) struct Rounds(pub Vec<script::effects::Strike>);

/// The characters whose KOTOR state ended this step in a way that runs
/// their end-of-round script (`combat::effects::state::ends_round`: the AI
/// resumes), by `hurt`; raised in the scripts' step. Authority only.
#[derive(Resource, Default)]
pub(crate) struct StatesEnded(pub Vec<SimId>);

/// The items used this step, by user and `spells.2da` row: each taken
/// from its holder's goods in the scripts' step (`item_uses::spend`).
/// Authority only.
#[derive(Resource, Default)]
pub(crate) struct Spent(pub Vec<(crate::snapshot::SimId, u16)>);

#[derive(Clone, Copy)]
pub(crate) struct SpellAt {
    pub caster: SimId,
    pub target: SimId,
    pub row: u16,
    pub harmful: bool,
}

/// The scripts' state, what they did, and what they want of the world.
#[derive(Resource, Default)]
pub(crate) struct Scripts {
    pub state: State,
    pub report: Report,
    /// What the last step's scripts change in the world, tell its players
    /// (`apply`), the area's sound as they left it and the fade they set.
    pub changes: Vec<Change>,
    pub notices: Vec<(Option<ObjectId>, crate::notice::Notice)>,
    pub area_sound: Option<crate::snapshot::AreaSound>,
    pub fade: Option<crate::snapshot::Fade>,
    pub world_fade_in: Option<crate::snapshot::WorldFadeIn>,
    pub video_effect: Option<Option<u16>>,
    pub new_module: Option<script::NewModule>,
    /// The objects they made (`script::Made`), for `apply` to put in.
    pub made: Vec<script::Made>,
    /// A script ended the game lost (`script::Out::lost`), for `apply`.
    pub lost: bool,
    /// The party's leaving through the exit it asks for has been heard
    /// (`script::leaving`), while it goes on asking.
    leaving: bool,
    /// The party's characters the goods were last gathered for.
    members: Vec<u32>,
    /// The containers held open the goods were last gathered for.
    open: Vec<u32>,
}

/// A character's life as the events read it this step.
struct Life {
    id: ObjectId,
    creature: bool,
    hp: i32,
    alive: bool,
    dead: bool,
    in_combat: bool,
    /// The last to hit it, and when.
    attacker: Option<(u32, u64)>,
}

/// Queues `id`'s hook for `event`, if it has one.
pub(super) fn raise(
    view: &View,
    state: &mut State,
    id: ObjectId,
    event: Hook,
    entering: Option<ObjectId>,
) {
    raise_with(view, state, id, event, entering, 0);
}

/// `raise` for what `by` did: a companion a guest drives leads the party
/// in the script its own act raised (`Pending::leads`).
fn raise_by(
    view: &View,
    state: &mut State,
    id: ObjectId,
    event: Hook,
    entering: Option<ObjectId>,
    by: ObjectId,
) {
    if let Some(script) = view.hook(id, event) {
        let mut pending = Pending::new(&script, id);
        pending.entering = entering;
        pending.leads = guest_body(view, by);
        state.pending.push(pending);
    }
}

/// `by`, if it is a companion a guest drives (`script::Character::driver`).
fn guest_body(view: &View, by: ObjectId) -> Option<ObjectId> {
    view.thing(by)?
        .character?
        .driver
        .filter(|seat| *seat != super::LEADER_SEAT)
        .map(|_| by)
}

/// `raise`, a notice's with how `entering` was perceived.
fn raise_with(
    view: &View,
    state: &mut State,
    id: ObjectId,
    event: Hook,
    entering: Option<ObjectId>,
    perception: u8,
) {
    if let Some(script) = view.hook(id, event) {
        let mut pending = Pending::new(&script, id);
        pending.entering = entering;
        pending.perception = perception;
        state.pending.push(pending);
    }
}

/// Keeps what `by`'s attack that reached its target struck with
/// (`GetLastWeaponUsed`): the item in its right hand, else its own weapons
/// (claws, a bite), else its gloves; none with all empty [verify: which
/// hand an off-hand blow names].
fn struck_with(state: &mut State, by: u32) {
    use crate::inventory::slots::{
        CREATURE_BITE, CREATURE_LEFT, CREATURE_RIGHT, HANDS, RIGHT_WEAPON,
    };
    let item = [
        RIGHT_WEAPON,
        CREATURE_RIGHT,
        CREATURE_LEFT,
        CREATURE_BITE,
        HANDS,
    ]
    .into_iter()
    .find_map(|slot| state.in_slot(by, slot));
    match item {
        Some(item) => state.last.insert((by, script::last::WEAPON), item.0),
        None => state.last.remove(&(by, script::last::WEAPON)),
    };
}

/// Whether the moment `phase` (ms) of every `period` falls in this step,
/// the one ending at `now_ms`, `msec` long.
pub(super) fn moment(now_ms: u64, msec: u32, period: u64, phase: u64) -> bool {
    (now_ms + period - phase % period) % period < u64::from(msec)
}

/// What `dead`'s death gives the party: its challenge rating's worth at
/// the first PC's level (`script::xp`; the character the first seat is
/// switched to, while it is), if it was no party member and the first
/// PC's enemy [verify: whoever killed it].
fn kill_xp(view: &View, library: &script::Library, state: &script::State, dead: ObjectId) -> i32 {
    let worth = || {
        let thing = view.thing(dead)?;
        let character = thing.character?;
        let player = view.thing(view.first_pc(&state.party))?.character?;
        if character.party
            || !view
                .module
                .repute
                .with(&state.reputation)
                .hostile(character.faction, player.faction)
        {
            return None;
        }
        let facts = view.facts(dead)?;
        let experience = library.experience();
        if crate::combat::passive::Game::named(&view.module.game)
            != crate::combat::passive::Game::Tsl
        {
            return Some(experience.kill(player.level, facts.challenge));
        }
        // TSL's: by the level the PC's XP reaches; an autobalanced
        // creature rated by its `MultiplierSet`'s `CRMod` past the PC's
        // level, which the executable keeps as it spawned
        // (`PCLevelAtSpawn`: `G_PC_LEVEL`) and kjk reads as it dies
        // [verify: a level taken in between].
        let xp = state.xp(crate::inventory::PARTY);
        let potential = library.classes().potential_level(player.level, xp);
        let autobalanced = (facts.multiplier_set != 0).then(|| {
            let pc_level = script::global(&state.numbers, script::xp::PC_LEVEL)
                .and_then(|level| u8::try_from(*level).ok())
                .unwrap_or(player.level);
            (facts.multiplier_set, pc_level)
        });
        let members = state.party.members.len() + 1;
        Some(experience.tsl_kill(potential, facts.challenge, autobalanced, members))
    };
    worth().unwrap_or(0)
}

/// A character as perception sees it (units, degrees): where it is, its
/// senses, and what the stealth checks read of it (`stealth::Watcher`,
/// `Sneaker`).
struct Body {
    id: u32,
    origin: Vec3,
    eye: Vec3,
    chest: Vec3,
    alive: bool,
    senses: Option<Senses>,
    /// Its Stealth total while in stealth mode.
    stealth: Option<i16>,
    awareness: i16,
    in_combat: bool,
    still: bool,
    running: bool,
    npc: bool,
    /// Its facing across the floor (unit).
    facing: [f32; 2],
    /// What its worn items give: True Seeing, Dampen Sound (`gear::traits`).
    traits: u8,
    /// `appearance.2da` `sizecategory`.
    size: u8,
    /// Blind (`Modifies::Blind`): it neither sees nor hears others.
    blind: bool,
}

/// The step's scripts (see the module documentation).
#[allow(clippy::type_complexity, clippy::too_many_arguments)]
pub(super) fn scripts(
    clock: Res<Clock>,
    level: Res<Static>,
    (opened, closed, clicked, blocked): (Res<Opened>, Res<Closed>, Res<Clicked>, Res<Blocked>),
    (containers, locks, battered): (
        Res<super::containers::Containers>,
        Res<super::locks::Locks>,
        Res<super::objects::Battered>,
    ),
    (used, spent, item_talents, mut game): (
        Res<Used>,
        Res<Spent>,
        Res<ItemTalents>,
        ResMut<super::minigame::Minigame>,
    ),
    (inputs, (fight, embattled), mut leaving): (
        Res<super::Inputs>,
        (Res<super::InCombat>, Res<super::party::Embattled>),
        ResMut<super::party::Leaving>,
    ),
    (mut talking, mut holding, mut progressed, personas, mut tabled): (
        ResMut<super::Talking>,
        ResMut<super::Holding>,
        ResMut<super::Progressed>,
        Res<super::Personas>,
        ResMut<super::Tabled>,
    ),
    (spells, held, mut states_ended, struck): (
        Res<SpellsAt>,
        Res<super::scene::Held>,
        ResMut<StatesEnded>,
        Res<super::minigame::Struck>,
    ),
    (casts, presses, begun, ordered, let_go): (
        Res<Casts>,
        Res<super::covert::Presses>,
        Res<Rounds>,
        Res<Ordered>,
        Res<LetGo>,
    ),
    area: Res<super::Module>,
    mut next_id: ResMut<super::NextId>,
    mut streams: ResMut<Streams>,
    mut scripts: ResMut<Scripts>,
    doors: Query<&Object>,
    things: Query<(
        &Id,
        Option<&Avatar>,
        Option<&Object>,
        Option<&Player>,
        Option<&Placed>,
        Option<&Fighting>,
        Option<&PlayerSheet>,
        Option<&Benched>,
    )>,
) {
    let level = &level.0;
    let (Some(library), Some(module)) = (level.scripts(), level.module()) else {
        return;
    };
    let scripts = &mut *scripts;
    let now = clock.time_ms;
    let mut list = Vec::new();
    let mut party = Vec::new();
    // The players' own characters, by seat: those benched too.
    let mut own = Vec::new();
    let mut lives = Vec::new();
    let mut bodies = Vec::new();
    let mut characters = Vec::new();
    for (id, avatar, object, player, placed, fighting, sheet, benched) in &things {
        let data = object.and_then(|object| object.facts.as_deref());
        if let Some(player) = player {
            let ps = &player.0;
            let worn = sheet_of(sheet, object);
            let now_sheet = fighting.map_or(worn, |fighting| {
                crate::combat::effects::sheet_now(worn, &fighting.0, now)
            });
            let skill = |row| crate::stealth::total(Some(library), &worn, &now_sheet, row);
            let (still, running) = crate::stealth::pace(ps.velocity);
            let (sin, cos) = math::sin_cos_deg(ps.view[1]);
            bodies.push(Body {
                id: id.0.0,
                origin: ps.origin,
                eye: mind::eye(ps),
                chest: mind::chest(ps),
                alive: fighting.is_none_or(|fighting| fighting.0.alive()),
                senses: data.and_then(|data| data.senses),
                // An attack out of stealth stays hidden until its opener
                // lands or runs out (`stealth::OPENER_MS`): KOTOR's round
                // begins in stealth.
                stealth: fighting
                    .filter(|fighting| {
                        fighting.0.covert.stealth || fighting.0.covert.opener_until >= now
                    })
                    .map(|_| skill(crate::combat::sheet::skills::STEALTH).unwrap_or(0)),
                awareness: skill(crate::combat::sheet::skills::AWARENESS).unwrap_or(0),
                in_combat: fighting.is_some_and(|fighting| fighting.0.combat_until > now),
                still,
                running,
                npc: object.is_some(),
                facing: [cos, sin],
                size: data.and_then(|data| data.body).map_or(3, |body| body.size),
                traits: now_sheet.gear.traits,
                blind: fighting.is_some_and(|fighting| fighting.0.effects.blind(now)),
            });
        }
        let (position, yaw) = match (player, placed) {
            (Some(player), _) => (scale(player.0.origin, collision::UNIT), player.0.view[1]),
            (None, Some(placed)) => (placed.0, data.map_or(0.0, |data| data.yaw)),
            _ => continue,
        };
        let fighter = fighting.map(|fighting| (&fighting.0, sheet_of(sheet, object)));
        if let Some((fighter, sheet)) = fighter.filter(|(fighter, _)| fighter.alive()) {
            characters.push(super::item_uses::Character {
                id: ObjectId(id.0.0),
                holder: if in_party(avatar, fighter) {
                    crate::inventory::PARTY
                } else {
                    id.0.0
                },
                sheet,
                used: fighter.pack.used,
            });
        }
        // A seat's own character, benched or not: its player's persona.
        let persona = avatar
            .map(|avatar| avatar.0)
            .or(benched.map(|benched| benched.0))
            .filter(|_| object.is_none())
            .and_then(|seat| crate::persona::of(&personas.0, seat))
            .cloned();
        // A door's or placeable's as blows left them (`world::objects`).
        let hp = fighter
            .map(|(fighter, sheet)| (fighter.whole_hp(), i32::from(sheet.max_hp)))
            .or_else(|| {
                object
                    .zip(data)
                    .and_then(|(o, data)| o.state.hit_points(data))
            });
        let character = fighter.map(|(fighter, sheet)| Character {
            faction: faction_of(fighter, &sheet),
            poisoned: fighter.pack.poison.map(|poison| ObjectId(poison.by.0)),
            race: sheet.race,
            level: sheet.level,
            party: in_party(avatar, fighter),
            // A creature's player is its mind's driver (`drivers::assign`
            // sets and clears it in place); a player's, its avatar's seat.
            driver: match object {
                Some(_) => fighter.mind.driver,
                None => avatar.map(|avatar| avatar.0),
            },
            target: fighter.target.map(|target| ObjectId(target.0)),
            in_combat: fighter.in_combat(now),
            fake_combat: fighter.faking(now),
            stuck: fighter.mind.walker.is_some_and(|walker| walker.stuck()),
            walking: fighter.mind.walker.is_some_and(|walker| walker.stalls == 0)
                && player.is_some_and(|player| {
                    crate::math::flat_length(player.0.velocity) * collision::UNIT
                        >= rules::walking::pace::LEAST_WALK
                }),
            airborne: player.is_some_and(|player| player.0.ground.is_none()),
            struck: fighter.last_attacker.map(|(by, at)| (ObjectId(by.0), at)),
            downed: fighter.downed(),
            debilitated: fighter.force.stunned(now) || fighter.force.confused_until > now,
            fp: fighter.fp,
            feats: sheet.feats,
            lasting: fighter.effects.lasting,
            sheet: crate::combat::effects::sheet_now(sheet, fighter, now),
            worn: sheet,
            hand: fighter.hand,
            absorbing: fighter.force.absorbing(now),
            gender: persona
                .as_ref()
                .map_or(data.map_or(0, |data| data.gender), |persona| persona.gender),
            subrace: data.map_or(0, |data| data.subrace),
            classes: std::array::from_fn(|at| {
                let career = scripts.state.careers.get(&id.0.0);
                career
                    .or(data.map(|data| &data.classes))
                    .and_then(|classes| classes.get(at).copied())
                    .unwrap_or((u8::MAX, 0))
            }),
            powers: sheet.powers,
            still: player.is_none_or(|player| crate::stealth::pace(player.0.velocity).0),
            running: player.is_some_and(|player| crate::stealth::pace(player.0.velocity).1),
            attacking: fighter.last_command.buttons
                & (crate::input::buttons::ATTACK | crate::input::buttons::ALT_ATTACK)
                != 0,
            stealth: fighter.covert.stealth,
            look: fighter.look,
        });
        let sim = ObjectId(id.0.0);
        if let Some(fighting) = fighting {
            let fighter = &fighting.0;
            lives.push(Life {
                id: sim,
                creature: object.is_some(),
                hp: fighter.hp,
                alive: fighter.alive(),
                dead: fighter.dead(),
                in_combat: fighter.combat_until > now,
                attacker: fighter.last_attacker.map(|(by, at)| (by.0, at)),
            });
        }
        list.push(Thing {
            id: sim,
            kind: object.map(|object| object.kind),
            index: object.map(|object| object.index),
            facts: object.and_then(|object| object.facts.clone()),
            position,
            yaw,
            hp,
            // A placeable's look is open or not (`snapshot::look`).
            open: object
                .filter(|object| {
                    object.kind == ObjectKind::Door || data.is_some_and(|data| data.container)
                })
                .map(|object| match object.kind {
                    ObjectKind::Placeable => {
                        u8::from(object.state.state == crate::snapshot::look::OPEN)
                    }
                    _ => object.state.state,
                }),
            locked: object
                .filter(|object| matches!(object.kind, ObjectKind::Door | ObjectKind::Placeable))
                .zip(data)
                .map(|(object, data)| object.state.locked(data.opens)),
            sound: object
                .filter(|object| object.kind == ObjectKind::Sound)
                .map(|object| (object.state.state, object.state.volume)),
            trap: object.map_or(0, |object| object.state.trap),
            character,
            persona,
        });
        if let Some(avatar) = avatar {
            party.push((avatar.0, sim));
            if object.is_none() {
                own.push((avatar.0, sim));
            }
        }
        if let Some(benched) = benched {
            own.push((benched.0, sim));
        }
    }
    // By id, each (large) thing moved once; ties, were there any, keep
    // their order as a stable sort would.
    let mut order: Vec<(ObjectId, usize)> = list
        .iter()
        .enumerate()
        .map(|(at, thing)| (thing.id, at))
        .collect();
    order.sort_unstable();
    let mut unsorted: Vec<Option<Thing>> = list.into_iter().map(Some).collect();
    let list: Vec<Thing> = order
        .iter()
        .map(|&(_, at)| unsorted[at].take().expect("each index once"))
        .collect();
    party.sort();
    let mut picks = answers(&mut scripts.state, &inputs.0, &party);
    // The companion leading answers for its player where it stands: a
    // conversation a script started with the party leader (Trask's Security
    // tutorial), or one its use started.
    if let Some(body) = super::lent(&scripts.state.party).map(|body| ObjectId(body.0))
        && scripts
            .state
            .talk
            .conversations
            .iter()
            .any(|c| c.holds(body))
        && let Some(answer) = picks.get_mut(&super::LEADER_SEAT.0)
    {
        answer.by = body;
    }
    // A seat whose ready for an exit leaves what it is in (R11,
    // `party::ready`): its conversation or its watching (R9a) as its Leave
    // would, whatever else it answered; its store below.
    let leaving = std::mem::take(&mut leaving.0);
    for &(seat, body) in &leaving {
        let by = ObjectId(body.0);
        picks.insert(
            seat.0,
            Answer {
                by,
                pick: Pick::Leave,
            },
        );
    }
    // The first seat's player making a companion in the party its leader,
    // or itself again (the party table is the host's, I30 P4); a player
    // turning solo mode on or off.
    let leader_own = own
        .iter()
        .find(|(seat, _)| *seat == super::LEADER_SEAT)
        .map(|(_, player)| player.0);
    // The body the first seat leads with now, to hand its view on should
    // the leader change (below).
    let leading_before = super::lent(&scripts.state.party)
        .map(|body| body.0)
        .or(leader_own);
    for (seat, action) in inputs.0.actions() {
        let table = &mut scripts.state.party;
        let leads = *seat == super::LEADER_SEAT;
        match *action {
            Action::SetLeader(u8::MAX) if leads => table.leader = None,
            Action::SetLeader(npc) if leads && table.has(npc) => table.leader = Some(npc),
            // Its own portrait, or the one it is switched to (`switch`).
            Action::Lead(body)
                if leads
                    && (Some(body.0) == leader_own || table.switched_body() == Some(body.0)) =>
            {
                table.leader = None
            }
            Action::Lead(body) if leads => {
                if let Some(npc) = table.members.iter().copied().find(|npc| {
                    table
                        .available
                        .get(npc)
                        .is_some_and(|known| known.object == Some(body.0))
                }) {
                    table.leader = Some(npc);
                }
            }
            Action::Solo(on) => table.solo = on,
            _ => {}
        }
    }
    // Players closing the party selection screen, in seat order.
    let chosen: Vec<(ObjectId, Option<[u8; 2]>)> = inputs
        .0
        .actions()
        .iter()
        .filter_map(|(seat, action)| match action {
            Action::ChooseParty(picks) => party
                .iter()
                .find(|(of, _)| of == seat)
                .map(|(_, player)| (*player, *picks)),
            _ => None,
        })
        .collect();
    // The host travelling from the galaxy map (its story is the host's, as
    // its conversations' replies are: I30 P6).
    let travels: Vec<(ObjectId, u8)> = inputs
        .0
        .actions()
        .iter()
        .filter_map(|(seat, action)| match action {
            Action::Travel(planet) if *seat == super::LEADER_SEAT => party
                .iter()
                .find(|(of, _)| of == seat)
                .map(|(_, player)| (*player, *planet)),
            _ => None,
        })
        .collect();
    // The host's answers to guests asking it into their conversations (R5).
    let requests: Vec<(u8, u32, bool)> = inputs
        .0
        .actions()
        .iter()
        .filter_map(|(seat, action)| match action {
            Action::Request { talk, accept } if *seat == super::LEADER_SEAT => {
                Some((seat.0, *talk, *accept))
            }
            _ => None,
        })
        .collect();
    // The pazaak moves, each with the body of the match its seat plays (I30
    // P8: whoever asked for a match plays it alone; the first seat one whose
    // body no seat drives now).
    let driven = party.iter().map(|(seat, body)| (*seat, body.0));
    let pazaak: Vec<(ObjectId, crate::pazaak::Move)> = inputs
        .0
        .actions()
        .iter()
        .filter_map(|(seat, action)| match action {
            Action::Pazaak(played) => {
                let table = crate::pazaak::played_by(&scripts.state.pazaak, *seat, driven.clone())?;
                Some((ObjectId(table.player), *played))
            }
            _ => None,
        })
        .collect();
    let seats = party.clone();
    let lent = super::lent(&scripts.state.party);
    let seat_bodies: Vec<(u8, ObjectId)> = seats
        .iter()
        .map(|(seat, body)| {
            let acting = super::acting_body(*seat, SimId(body.0), lent);
            (seat.0, ObjectId(acting.0))
        })
        .collect();
    // The asks to watch a match (R9a), in seat order: each seat's
    // `Action::Watch`, and its body's use of a body at a table (`talk_to`
    // leaves that use to them).
    let mut watches: Vec<(u8, ObjectId, u32)> = inputs
        .0
        .actions()
        .iter()
        .filter_map(|(seat, action)| match *action {
            Action::Watch { player } => seat_bodies
                .iter()
                .find(|(of, _)| *of == seat.0)
                .map(|&(seat, body)| (seat, body, player)),
            _ => None,
        })
        .collect();
    for (object, user) in &used.0 {
        let user = seat_bodies.iter().find(|(_, body)| body.0 == user.0);
        if let Some(&(seat, body)) = user
            && scripts.state.plays_pazaak(ObjectId(object.0))
        {
            watches.push((seat, body, object.0));
        }
    }
    watches.sort_by_key(|(seat, ..)| *seat);
    let watches: Vec<(ObjectId, u32)> = watches
        .into_iter()
        .map(|(_, body, player)| (body, player))
        .collect();
    let embattled: Vec<ObjectId> = embattled.0.iter().map(|id| ObjectId(id.0)).collect();
    let members: Vec<u32> = list
        .iter()
        .filter(|thing| thing.character.is_some_and(|character| character.party))
        .map(|thing| thing.id.0)
        .collect();
    // The seats' own characters, by seat, for the scripts' PCs
    // (`View::pcs`: the first seat's the creature it is switched to while
    // it is). A companion a guest drives is still the companion to them
    // (I30 P2), never a second PC.
    own.sort();
    let party: Vec<ObjectId> = own.into_iter().map(|(_, id)| id).collect();
    let door_state: BTreeMap<u32, crate::snapshot::ObjectState> = doors
        .iter()
        .map(|object| (object.index, object.state))
        .collect();
    let leads = |from: Vec3, to: Vec3| {
        let state = |index: u32| {
            door_state
                .get(&index)
                .copied()
                .unwrap_or(crate::snapshot::ObjectState::GONE)
        };
        let from = from.map(|v| v / collision::UNIT);
        crate::walk::leads(level, from, script::standing(to), &state)
    };
    let sight = level.states(door_state.iter().map(|(index, state)| (*index, state)));
    let sees = |from: Vec3, to: Vec3| {
        level.with(sight.as_deref(), |world| {
            super::unobstructed(&world.trace(
                from,
                to,
                &collision::Extents::POINT,
                collision::Contents::SOLID,
            ))
        })
    };
    let floor = |at: Vec3| {
        level
            .nav()
            .and_then(|nav| nav.height(at, script::FLOOR_SEARCH_M, script::FLOOR_SEARCH_M))
    };
    let walks = |from: Vec3, to: Vec3| {
        crate::walk::straight(level, script::standing(from), script::standing(to))
    };
    // The last step's, which this step's scripts replace.
    let intents = std::mem::take(&mut scripts.state.intents);
    let view = View {
        things: &list,
        party: &party,
        seats: &seat_bodies,
        module,
        instances: &area.instances,
        next_id: next_id.0,
        now_ms: now,
        intents: &intents,
        picks: &picks,
        requests: &requests,
        in_combat: fight.0,
        embattled: &embattled,
        chosen: &chosen,
        travels: &travels,
        pazaak: &pazaak,
        watches: &watches,
        area_sound: area.sound,
        fade: area.fade,
        world_fade_in: area.world_fade_in,
        leads: &leads,
        sees: &sees,
        floor: &floor,
        walks: &walks,
        minigame: game.0.as_ref().zip(level.minigame().map(|rules| &**rules)),
        held: held.world,
    };
    let state = &mut scripts.state;
    // What players did with items: taken from containers (before the
    // containers' scripts hear of it), traded, worn.
    let mut out = Out::default();
    // Components and chemicals taken are the party's counts, told to all.
    let mut acquired = Vec::new();
    for (container, item) in &containers.taken {
        acquired.push(state.take_item(library, container.0, *item));
    }
    for &(container, who) in &containers.took_all {
        let (any, gained) = state.take_rest(library, container.0);
        acquired.push(gained);
        if !any {
            scripts
                .notices
                .push((Some(ObjectId(who.0)), crate::notice::Notice::Empty));
        }
    }
    let told = acquired
        .into_iter()
        .flat_map(script::workbench::Acquired::told);
    scripts.notices.extend(told.map(|notice| (None, notice)));
    for (container, item) in &containers.put {
        state.put_in(*item, container.0);
        // What its disturbed event's script reads (`GetInventoryDisturbItem`;
        // an item taken out is not kept).
        state
            .last
            .insert((container.0, script::last::DISTURB_ITEM), *item);
    }
    let closed_now: Vec<ObjectId> = containers
        .events
        .iter()
        .filter(|(.., event)| *event == Hook::Closed)
        .map(|(container, ..)| ObjectId(container.0))
        .collect();
    super::remains::closed_empty(&view, state, &mut out, &closed_now);
    let taken = super::item_uses::spend(state, library.items(), &view, &mut out, &spent.0);
    let (acted, used_items) = seat_actions(
        state,
        library,
        &view,
        &inputs.0,
        &seats,
        &mut scripts.notices,
        &mut out,
    );
    if level.arena_rules().is_some() {
        let body = |seat: crate::input::Seat| {
            seats
                .iter()
                .find(|(of, _)| *of == seat)
                .map(|(_, player)| acting(&view, seat.0, *player))
        };
        super::arena::orders(library, &view, state, &inputs.0, body, &mut out);
    }
    // The items' spells the minds used as talents.
    // Each one used leaves the goods (`use_item`), so they are gathered
    // again.
    let mut talents_used = false;
    let talents: Vec<script::Cast> = item_talents
        .0
        .iter()
        .filter_map(|&(user, row, target)| {
            let (user, target) = (ObjectId(user.0), ObjectId(target.0));
            let item = state.item_talent(library.items(), &view, user, row)?;
            let used = state
                .use_item(library, &view, &mut out, user, item, target)
                .ok()?;
            talents_used = true;
            used
        })
        .collect();
    // The grips let go end their impacts' deferred code (`LetGo`).
    state.delayed.retain(|delayed| {
        !delayed
            .spell
            .as_ref()
            .is_some_and(|cast| let_go.0.contains(&(cast.caster, cast.row, cast.target)))
    });
    // The step's casts: the powers' and items' from the fight's step, the
    // items the players used from their screens and the minds' talents.
    let casts: Vec<script::Cast> = casts
        .0
        .iter()
        .cloned()
        .chain(used_items)
        .chain(talents)
        .collect();
    // TSL: a party member's player ordering a Force power in a duel that
    // forbids them.
    for who in &ordered.0 {
        if script::in_party(&view, state, ObjectId(who.0)) {
            script::forfeit(&view, state, script::party::forfeit::FORCE_POWERS);
        }
    }
    // The stores of the seats readying for an exit end (R11).
    let mut left_stores = false;
    for (_, body) in &leaving {
        left_stores |= state.end_trade(body.0);
    }
    let handled = acted
        || left_stores
        || !taken.is_empty()
        || talents_used
        || !containers.taken.is_empty()
        || !containers.took_all.is_empty()
        || !containers.put.is_empty()
        || locks.used_items;
    heartbeats(&view, state, clock.msec);
    super::traps::detect(&view, library, state, &mut out, &mut streams.authority);
    let mut handled = super::traps::skill_uses(
        &view,
        level,
        library,
        state,
        (&inputs.0, &presses.0),
        &seats,
        &mut out,
        &mut streams.authority,
    ) || handled;
    triggers(&view, level, library.items(), state, &mut out);
    super::spawners::step(
        &view,
        level,
        library,
        state,
        &mut out,
        &mut streams.authority,
    );
    super::traps::opened_and_used(&view, library.items(), state, &mut out, &opened.0, &used.0);
    bodies.sort_by_key(|body| body.id);
    let states = door_states(level, &doors);
    let sight = |from: Vec3, to: Vec3| level.sees(states.as_deref(), from, to);
    // How much solid stands between two points (metres): the stretch
    // neither end's trace crosses.
    let walls = |from: Vec3, to: Vec3| {
        let length = math::length(math::sub(to, from)) * collision::UNIT;
        let [there, back] = [(from, to), (to, from)].map(|(a, b)| {
            level.with(states.as_deref(), |world| {
                world
                    .trace(a, b, &collision::Extents::POINT, collision::Contents::SOLID)
                    .fraction
            })
        });
        (1.0 - there - back).max(0.0) * length
    };
    perceive(
        &view,
        state,
        &bodies,
        &sight,
        &walls,
        &mut streams.authority,
    );
    stealth_loss(&view, state);
    // A state that ended runs its bearer's end-of-round script: its AI
    // takes up the fight again. The dead have no round (`rounds`).
    for id in std::mem::take(&mut states_ended.0) {
        if view.thing(ObjectId(id.0)).is_some_and(Thing::dead) {
            continue;
        }
        raise(&view, state, ObjectId(id.0), Hook::EndRound, None);
    }
    for (object, by) in &locks.unlocked {
        let (object, by) = (ObjectId(object.0), ObjectId(by.0));
        state.last.insert((object.0, script::last::UNLOCKED), by.0);
        raise_by(&view, state, object, Hook::Unlock, None, by);
    }
    // Doors and placeables struck: attacked in melee (its striker's weapon
    // kept), damaged where the blow told, and dying when they broke.
    for &(object, by, lost, broke) in &battered.0 {
        state.last.insert((object.0, script::last::ATTACKER), by.0);
        struck_with(state, by.0);
        raise(&view, state, ObjectId(object.0), Hook::Attacked, None);
        if lost == 0 {
            continue;
        }
        state.last.insert((object.0, script::last::DAMAGER), by.0);
        state
            .last
            .insert((object.0, script::last::DAMAGE), u32::from(lost));
        raise(&view, state, ObjectId(object.0), Hook::Damaged, None);
        if broke {
            state.last.insert((object.0, script::last::KILLER), by.0);
            raise(&view, state, ObjectId(object.0), Hook::Death, None);
        }
    }
    // The 107per turret's bolts that met the area's things: each one's
    // user-defined event, the bolt's damage its number (TSL's executable;
    // `world::minigame`).
    for &(thing, damage) in &struck.0 {
        if let Some(script) = view.hook(ObjectId(thing.0), Hook::UserDefined) {
            let mut pending = Pending::new(&script, ObjectId(thing.0));
            pending.number = damage;
            state.pending.push(pending);
        }
    }
    for (door, by, ok) in &opened.0 {
        let door = ObjectId(door.0);
        let hook = if *ok { Hook::Open } else { Hook::FailToOpen };
        match by {
            Some(by) => {
                state.last.insert((door.0, script::last::OPENED), by.0);
                raise_by(&view, state, door, hook, None, ObjectId(by.0));
            }
            None => raise(&view, state, door, hook, None),
        }
    }
    // A creature whose walk a shut door holds (KOTOR's `OnBlocked`: its
    // AI opens the door or bashes a locked one, `k_ai_master`'s 1009):
    // once as the door opens for it, every `BLOCKED_EVERY_MS` while it
    // stays shut.
    for &(who, door) in &blocked.0 {
        let opens = opened.0.contains(&(door, Some(who), true));
        let again = moment(now, clock.msec, BLOCKED_EVERY_MS, u64::from(who.0));
        if opens || again {
            let who = ObjectId(who.0);
            state
                .last
                .insert((who.0, script::last::BLOCKING_DOOR), door.0);
            raise(&view, state, who, Hook::Blocked, None);
        }
    }
    for (door, by) in &closed.0 {
        let (door, by) = (ObjectId(door.0), ObjectId(by.0));
        state.last.insert((door.0, script::last::CLOSED), by.0);
        raise_by(&view, state, door, Hook::Closed, None, by);
    }
    for (door, by) in &clicked.0 {
        let by = ObjectId(by.0);
        raise_by(&view, state, ObjectId(door.0), Hook::Click, Some(by), by);
    }
    for (container, by, event) in &containers.events {
        let kind = if *event == Hook::Closed {
            script::last::CLOSED
        } else {
            script::last::DISTURBED
        };
        let (container, by) = (ObjectId(container.0), ObjectId(by.0));
        state.last.insert((container.0, kind), by.0);
        raise_by(&view, state, container, *event, None, by);
    }
    // One use of a thing a step for each user: a player's use key and its
    // click (KOTOR's) on it in the same step are one use, as KOTOR's click
    // is, not two (Manaan's water buttons swap a pair of rooms each use:
    // used twice, the swap undid itself).
    let mut once = BTreeSet::new();
    let mut doors = Vec::new();
    for (object, player) in &used.0 {
        let (object, player) = (ObjectId(object.0), ObjectId(player.0));
        if !once.insert((object.0, player.0)) {
            continue;
        }
        if view.thing(object).and_then(|thing| thing.kind) == Some(ObjectKind::Door) {
            doors.push((object, player));
        } else {
            talk_to(&view, state, object, player);
        }
    }
    // Powers that reached someone (C15): at most every `DAMAGED_EVERY_MS`
    // per target and caster, a channel's ticks so one event.
    for spell in &spells.0 {
        let (target, caster) = (spell.target.0, spell.caster.0);
        let last = state.watch.spelled.get(&(target, caster));
        if last.is_some_and(|last| now < last + DAMAGED_EVERY_MS) {
            continue;
        }
        state.watch.spelled.insert((target, caster), now);
        state
            .last
            .insert((target, script::last::SPELL_CASTER), caster);
        state
            .last
            .insert((target, script::last::SPELL), u32::from(spell.row));
        state.last.insert(
            (target, script::last::SPELL_HARMFUL),
            u32::from(spell.harmful),
        );
        if spell.harmful {
            state.last.insert((target, script::last::HOSTILE), caster);
        }
        raise(&view, state, ObjectId(target), Hook::SpellCastAt, None);
    }
    lives.sort_by_key(|life| life.id);
    let dead: Vec<ObjectId> = lives
        .iter()
        .filter(|life| life.dead)
        .map(|life| life.id)
        .collect();
    for life in lives {
        let Life {
            id,
            hp,
            alive,
            dead,
            ..
        } = life;
        rounds(&view, state, &life);
        let hit_at = life.attacker.map_or(0, |(_, at)| at);
        let was = state.watch.seen.insert(
            id.0,
            Watched {
                hp,
                alive,
                dead,
                hit_at,
                damaged_at: None,
            },
        );
        let Some(was) = was else { continue };
        let mut damaged_at = was.damaged_at;
        let by = life.attacker.map(|(by, _)| by);
        if let Some(by) = by {
            state.last.insert((id.0, script::last::DAMAGER), by);
        }
        // Attacked (C15): a new hit, at most every `DAMAGED_EVERY_MS` for
        // one attacker; before the damaged event, as KOTOR runs them.
        if let Some((by, at)) = life.attacker
            && was.alive
            && at > was.hit_at
        {
            state.last.insert((id.0, script::last::ATTACKER), by);
            state.last.insert((id.0, script::last::HOSTILE), by);
            struck_with(state, by);
            let last = state.watch.attacked.get(&(id.0, by));
            if last.is_none_or(|last| now >= last + DAMAGED_EVERY_MS) {
                state.watch.attacked.insert((id.0, by), now);
                raise(&view, state, id, Hook::Attacked, None);
            }
        }
        // HP lost while a damaged event waits out `DAMAGED_EVERY_MS` stays
        // owed: its event comes once the wait is over, for all lost since
        // (Davik, held at 1 HP by his script, waits for the event that sees
        // him there; the blow taking him from 2 to 1 came inside the wait,
        // and none after can take more).
        let owed = hp < was.hp && was.alive;
        let due = damaged_at.is_none_or(|at| now >= at + DAMAGED_EVERY_MS);
        if owed && due {
            damaged_at = Some(now);
            // In i64: a death's blow is `i32::MAX` hundredths, so the
            // difference is too.
            let scale = i64::from(script::HUNDREDTHS);
            let lost = (i64::from(was.hp) - i64::from(hp) + scale / 2) / scale;
            let lost = u32::try_from(lost).unwrap_or(u32::MAX);
            state.last.insert((id.0, script::last::DAMAGE), lost);
            raise(&view, state, id, Hook::Damaged, None);
            // A hurt body's conversation is interrupted (R7), its match
            // paused (R9), its watching stopped (R9a), decided from the
            // damage itself: DLG scripts clear actions mid-talk.
            if state.talk.holding(id).is_some()
                || state.plays_pazaak(id)
                || state.watching(id).is_some()
            {
                state.talk.hurt.push(id);
            }
            // A hurt shopper leaves the store (R8), while more than one seat
            // plays a body: one player shops on, as the originals do.
            if view.together() {
                handled |= state.end_trade(id.0);
            }
        }
        // A death, not a fall: a downed party member (I30 P11) raises
        // nothing until it dies.
        if !was.dead && dead {
            if let Some(by) = by {
                state.last.insert((id.0, script::last::KILLER), by);
            }
            raise(&view, state, id, Hook::Death, None);
            let xp = kill_xp(&view, library, state, id);
            state.reward(xp, &mut scripts.notices);
            script::xp::tell_pc_level(library, &view, state);
            if xp > 0 {
                progressed.0.kills.push(crate::snapshot::Kill {
                    dead: id.0,
                    xp,
                    at_ms: now,
                });
            }
            // A creature not of the party is taken away a while after,
            // leaving its remains (`remains`).
            let party = view
                .thing(id)
                .and_then(|thing| thing.character)
                .is_some_and(|character| character.party);
            if life.creature && !party {
                super::remains::died(&view, state, id, now);
            }
        }
        if let Some(watched) = state.watch.seen.get_mut(&id.0) {
            watched.damaged_at = damaged_at;
            if owed && !due && alive {
                watched.hp = was.hp;
            }
        }
    }
    super::remains::take_away(&view, library, state, &mut out, &dead);
    // Items the party gained and lost: the module's acquire hook for each
    // gained (`GetModuleItemAcquired`; Tatooine's chieftain's map sets a
    // global so), then its unacquire hook for each lost (`GetModuleItemLost`;
    // TSL's Ebon Hawk tells the mine is gone so). What its members wear is
    // the party's too: a weapon put back is neither.
    let held: BTreeSet<u32> = [crate::inventory::PARTY]
        .iter()
        .chain(&members)
        .flat_map(|holder| state.items.held_by(*holder).map(|(id, _)| id))
        .collect();
    let (gained, lost): (Vec<u32>, Vec<u32>) = match &state.watch.held {
        Some(before) => (
            held.difference(before).copied().collect(),
            before.difference(&held).copied().collect(),
        ),
        None => (Vec::new(), Vec::new()),
    };
    if state.watch.held.as_ref() != Some(&held) {
        state.watch.held = Some(held);
    }
    let changed = (gained.into_iter().map(|item| (Hook::AcquireItem, item)))
        .chain(lost.into_iter().map(|item| (Hook::UnacquireItem, item)));
    for (hook, item) in changed {
        raise(&view, state, script::MODULE, hook, Some(ObjectId(item)));
    }
    // The party going through an exit (the ready check's ask this step,
    // `party::ready`): the module goes after the step, so its leaving's
    // events run now, once while the party goes on asking.
    let asking = area.pending.is_some();
    if asking && !scripts.leaving {
        state.pending.extend(script::leaving(&view, &state.party));
    }
    scripts.leaving = asking;
    state.strikes.extend(begun.0.iter().copied());
    let talk = &state.talk;
    let conversing = !talk.conversations.is_empty() || !talk.asked.is_empty();
    let busy = !casts.is_empty()
        || !chosen.is_empty()
        || !travels.is_empty()
        || !pazaak.is_empty()
        || !state.pazaak.is_empty()
        || !state.strikes.is_empty()
        || !state.pending.is_empty()
        || !state.actions.is_empty()
        || state.delayed.iter().any(|d| d.due_ms <= now)
        || conversing
        || state.summary_owed
        || !talk.released.is_empty()
        || !talk.barks.is_empty()
        || script::levelling::due(library, state, &view);
    if busy {
        script::run_due(
            library,
            state,
            &view,
            &casts,
            &mut streams.authority,
            &mut scripts.report,
            &mut out,
        );
        door_speaks(&view, &mut scripts.state, &out.changes, &doors);
        super::traps::forget(&mut scripts.state, &out.changes);
        scripts.state.goals = out.goals;
        scripts.state.intents = out.intents;
        scripts.changes = out.changes;
        scripts.notices.extend(out.notices);
        progressed.0.statuses.extend(
            out.statuses
                .into_iter()
                .map(|kind| crate::snapshot::Status { kind, at_ms: now }),
        );
        progressed
            .0
            .feedback
            .extend(
                out.feedback
                    .into_iter()
                    .map(|(over, strref)| crate::snapshot::Feedback {
                        over: over.0,
                        strref,
                        at_ms: now,
                    }),
            );
        scripts.area_sound = out.area_sound;
        scripts.fade = out.fade;
        scripts.world_fade_in = out.world_fade_in;
        scripts.video_effect = out.video_effect;
        scripts.new_module = out.new_module;
        scripts.lost |= out.lost;
        if let Some(course) = out.minigame {
            game.0 = Some(course);
        }
        let told = scripts.state.talk.told();
        if talking.0 != told {
            talking.0 = told;
        }
    } else {
        // A seat's equipping dresses its character anew.
        state.dress_changed(library.items(), &view, &mut out);
        door_speaks(&view, &mut scripts.state, &out.changes, &doors);
        super::traps::forget(&mut scripts.state, &out.changes);
        scripts.changes = out.changes;
        // What the engine's own work told this step (`world::traps`).
        scripts.notices.extend(out.notices);
        scripts.state.goals.clear();
    }
    // A co-op race leaves once its runs have finished, or the first long
    // enough ago (M10).
    if scripts.new_module.is_none()
        && let Some(course) = game.0.as_mut()
        && let Some(leaving) = course.leave_due()
    {
        // The first to finish: its globals the real ones, its movies shown.
        if let Some(winner) = course.run(usize::from(leaving.run)) {
            script::commit_race(&mut scripts.state, &winner.overlay);
        }
        scripts.notices.extend(leaving.movies.iter().map(|movie| {
            let screen = crate::notice::Screen::movie(movie);
            (None, crate::notice::Notice::Show(screen))
        }));
        scripts.new_module = Some(script::NewModule {
            by: ObjectId(leaving.by),
            module: leaving.module,
            tag: leaving.tag,
        });
    }
    // The ids the made objects took are taken (scripts', and mines the
    // engine laid); their spawn scripts run once they stand in the world.
    next_id.0 += out.made.len() as u32;
    for made in &out.made {
        if let Some(script) = Hook::Spawn.of(&made.facts) {
            scripts.state.pending.push(Pending::new(script, made.id));
        }
    }
    scripts.made = out.made;
    // What players see of the items: gathered again when the scripts or a
    // player may have changed them, or the party changed.
    // The containers a seat holds open: what they hold is shown.
    let open: Vec<u32> = things
        .iter()
        .filter_map(|(id, _, object, ..)| {
            let object = object?;
            let container = object.facts.as_ref()?.container;
            (container && object.state.tenants != 0).then_some(id.0.0)
        })
        .collect();
    if busy || handled || scripts.members != members || scripts.open != open {
        // Those in the module: its things, and those made this step.
        let here = things
            .iter()
            .map(|(id, ..)| id.0.0)
            .chain(scripts.made.iter().map(|made| made.id.0));
        let goods = scripts.state.goods(&members, here, &open, &module.name);
        if holding.0 != goods {
            holding.0 = goods;
        }
        scripts.members = members;
        scripts.open = open;
        super::item_uses::usable(
            &scripts.state,
            library,
            &characters,
            &taken,
            &mut scripts.changes,
        );
    }
    // A new party leader (a portrait click, `SetPartyLeader`) takes the
    // view the first seat had: its player goes on looking the same way,
    // rather than through the body's own old aim, which turned it round.
    let leading_after = super::lent(&scripts.state.party)
        .map(|body| body.0)
        .or(leader_own);
    let seat_angles = inputs
        .0
        .get(super::LEADER_SEAT)
        .map(|command| command.angles);
    if let (Some(before), Some(after), Some(angles)) = (leading_before, leading_after, seat_angles)
        && before != after
        && let Some(view) = things.iter().find_map(|(id, _, _, player, ..)| {
            (id.0.0 == before).then(|| player.map(|player| player.0.view))?
        })
    {
        scripts.changes.push(Change::TakeView {
            who: ObjectId(after),
            angles,
            view,
        });
    }
    show_progress(&scripts.state, now, &mut progressed);
    if tabled.0 != scripts.state.pazaak {
        tabled.0.clone_from(&scripts.state.pazaak);
    }
}

/// The journal, the party's XP and the careers as everyone sees them, when
/// they moved; the kills' XP and the statuses kept until their time
/// (`now_ms`) is up.
fn show_progress(state: &State, now_ms: u64, progressed: &mut super::Progressed) {
    use crate::snapshot::{Feedback, Kill, Status};
    let xp = state.xp(crate::inventory::PARTY);
    let shown = &mut progressed.0;
    shown
        .kills
        .retain(|kill| now_ms < kill.at_ms + Kill::SHOWN_MS);
    shown
        .feedback
        .retain(|feedback| now_ms < feedback.at_ms + Feedback::SHOWN_MS);
    shown
        .statuses
        .retain(|status| now_ms < status.at_ms + Status::KEPT_MS);
    if shown.xp != xp || shown.journal != state.journal || shown.careers != state.careers {
        shown.journal.clone_from(&state.journal);
        shown.xp = xp;
        shown.careers.clone_from(&state.careers);
    }
}

/// TSL's forfeit conditions an equip by a player's `action` on `who` broke
/// (`script::forfeit_equip`).
fn equipped(
    library: &script::Library,
    view: &View,
    state: &mut State,
    who: ObjectId,
    action: &Action,
) {
    if let Action::Equip { item, slot } | Action::EquipOn { item, slot, .. } = action {
        script::forfeit_equip(library, view, state, who, (*item, *slot));
    }
}

/// The locked doors players used this step (`doors`: door, user) speak
/// their `Conversation` once their fail-to-open scripts have run: only a
/// door the scripts left locked and shut (`changes`) that nobody talks
/// with already and that barked nothing. A door's `Conversation` is the
/// UTD's own field; reone never speaks one, and when K1's exe does is not
/// read [verify in the game]: Room 3's door on the Endar Spire explains
/// Security so (its script only brings Trask over); the first door's
/// script unlocks and opens it for Trask, or speaks itself.
fn door_speaks(view: &View, state: &mut State, changes: &[Change], doors: &[(ObjectId, ObjectId)]) {
    for &(door, user) in doors {
        let unlocked = changes.iter().any(|change| match *change {
            Change::Lock { door: of, locked } => of == door && !locked,
            Change::Door { door: of, open, .. } => of == door && open,
            _ => false,
        });
        let locked = view.thing(door).and_then(|thing| thing.locked) == Some(true);
        // A conversation of one line is a bark, over at once: one its
        // script barked is its answer too ("Locked" over it is the
        // engine's, `world::locks`).
        let talk = &state.talk;
        let locked_word = script::dialogue::Said::StrRef(super::locks::lines::LOCKED_WORD);
        let talking = talk.conversations.iter().any(|c| c.owner == door)
            || talk.asked.iter().any(|start| start.owner == door)
            || talk
                .barks
                .iter()
                .any(|bark| bark.who == door && bark.text != locked_word);
        if locked && !unlocked && !talking {
            talk_to(view, state, door, user);
        }
    }
}

/// A creature's combat rounds (I22 C14): from when it is in combat, its
/// end-of-round event every `ROUND_MS` while it lives and stays so. None
/// while it holds a conversation (its owner, its player, the one speaking),
/// as KOTOR's dialogue holds its creatures out of combat: Jorak Uln, in
/// combat from the punishment his questions deal, ran `k_ai_master`'s round,
/// whose `ClearAllActions` took the resume his scene had queued, and the
/// conversation stayed paused for good.
fn rounds(view: &View, state: &mut State, life: &Life) {
    let id = life.id;
    let talking = state.talk.with(id, view.host(&state.party)).is_some();
    if !(life.creature && life.alive && life.in_combat) || talking {
        state.rounds.remove(&id.0);
        return;
    }
    let now = view.now_ms;
    let next = state.rounds.entry(id.0).or_insert(now + ROUND_MS);
    if now >= *next {
        *next = now + ROUND_MS;
        raise(view, state, id, Hook::EndRound, None);
    }
}

/// Each object's heartbeat when its moment in the period comes this step.
fn heartbeats(view: &View, state: &mut State, msec: u32) {
    let due = |phase: u64| moment(view.now_ms, msec, HEARTBEAT_MS, phase);
    for id in [AREA, MODULE] {
        if due(0) {
            raise(view, state, id, Hook::Heartbeat, None);
        }
    }
    // An encounter's runs on its own clock while it is active
    // (`world::spawners`).
    let beating =
        |thing: &&Thing| thing.index.is_some() && thing.kind != Some(ObjectKind::Encounter);
    for thing in view.things.iter().filter(beating) {
        if thing.dead() || state.heartless.contains(&thing.id.0) {
            continue;
        }
        // KOTOR's objects keep each its own timer from when it was
        // made; a prime spreads the phases by id instead [verify in K1's
        // exe], but one TSL's `ForceHeartbeat` ran keeps its period from
        // then (`State::beats`).
        let phase = state
            .beats
            .get(&thing.id.0)
            .copied()
            .unwrap_or(u64::from(thing.id.0) * 997);
        if due(phase) {
            raise(view, state, thing.id, Hook::Heartbeat, None);
        }
    }
}

/// Characters coming into and leaving triggers; one coming into a trap
/// springs it first (`traps::spring`), then the trigger's `OnEnter` runs.
fn triggers(
    view: &View,
    level: &Level,
    catalog: &crate::inventory::Catalog,
    state: &mut State,
    out: &mut Out,
) {
    let characters: Vec<&Thing> = view
        .things
        .iter()
        // Characters only: a door or placeable has hit points too (the
        // Unknown World temple's tile lights stand on its tiles). A downed
        // party member stays where it fell, inside.
        .filter(|thing| {
            thing.character.is_some_and(|c| {
                thing.hp.is_some_and(|(hp, _)| hp > 0) || c.downed
            })
        })
        .collect();
    for trigger in view
        .things
        .iter()
        .filter(|t| t.kind == Some(ObjectKind::Trigger))
    {
        let Some(facts) = trigger.index.and_then(|index| view.object(index)) else {
            continue;
        };
        let inside: Vec<u32> = characters
            .iter()
            .filter(|thing| super::traps::holds(level, facts, trigger.position, thing.position))
            .map(|thing| thing.id.0)
            .collect();
        let before = state
            .occupants
            .get(&trigger.id.0)
            .cloned()
            .unwrap_or_else(Vec::new);
        for id in &inside {
            if !before.contains(id) {
                if let Some(creature) = view.thing(ObjectId(*id))
                    && super::traps::armed(trigger)
                {
                    super::traps::spring(view, catalog, state, out, trigger, creature, false);
                }
                if facts.exit().is_none() {
                    raise(view, state, trigger.id, Hook::Enter, Some(ObjectId(*id)));
                }
            }
        }
        for id in &before {
            if !inside.contains(id) {
                raise(view, state, trigger.id, Hook::Exit, Some(ObjectId(*id)));
            }
        }
        if inside.is_empty() {
            state.occupants.remove(&trigger.id.0);
        } else {
            state.occupants.insert(trigger.id.0, inside);
        }
    }
}

/// A jump of `who` at `ms` to `mark` (metres, soles) as it came out: the
/// place `free` found (a box origin, units), and of the living bodies
/// `others` (id, box origin in units, clearance in units) the one nearest
/// the mark whose clearance covers it: what displaced the jump.
#[cfg(feature = "probes")]
fn jumped(
    (ms, who): (u64, ObjectId),
    (mark, from, on_mesh): ([f32; 3], [f32; 3], bool),
    free: Option<[f32; 3]>,
    others: impl Iterator<Item = (u32, [f32; 3], f32)>,
) -> crate::script::Jumped {
    let soles = |origin: [f32; 3]| {
        let at = scale(origin, collision::UNIT);
        [
            at[0],
            at[1],
            at[2] + crate::motion::SOLE_Z * collision::UNIT,
        ]
    };
    let flat = |a: [f32; 3], b: [f32; 3]| (a[0] - b[0]).hypot(a[1] - b[1]);
    let displaced_by = others
        .map(|(id, origin, clearance)| (id, flat(soles(origin), mark), clearance * collision::UNIT))
        .filter(|&(_, apart, clearance)| apart < clearance)
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .map(|(id, ..)| id);
    crate::script::Jumped {
        ms,
        who,
        mark,
        from: soles(from),
        on_mesh,
        landed: free.map(soles),
        displaced_by,
    }
}

impl SimWorld {
    /// What the scripts keep (a save carries it).
    pub fn script_state(&self) -> &State {
        &self.world.resource::<Scripts>().state
    }

    /// The scripts' state from a save.
    pub fn set_script_state(&mut self, state: State) {
        let mut scripts = self.world.resource_mut::<Scripts>();
        scripts.state = state;
        // The goods are gathered from it again on the next step.
        scripts.members.clear();
    }

    /// Where scripted moves route objects now (units), by id.
    pub fn script_goals(&self) -> &BTreeMap<u32, Goal> {
        &self.world.resource::<Scripts>().state.goals
    }

    /// Whom scripted attacks have objects fight now, by id.
    pub fn script_intents(&self) -> &BTreeMap<u32, Intent> {
        &self.world.resource::<Scripts>().state.intents
    }

    /// The door or placeable a script's attack has `who` (a player) strike
    /// now, at the front of its queue: struck once it stands within
    /// `script::DOOR_REACH` of it, its legs its seat's (`Action::Attack` on
    /// an object). TSL's Peragus shift console's "[Destroy the console]".
    pub fn assigned_strike(&self, who: SimId) -> Option<SimId> {
        let state = &self.world.resource::<Scripts>().state;
        match state.actions.get(&who.0)?.first()?.action {
            crate::script::actions::Action::Attack { target, .. } => Some(SimId(target.0)),
            _ => None,
        }
    }

    /// What the scripts did, for reports.
    pub fn script_report(&self) -> &Report {
        &self.world.resource::<Scripts>().report
    }

    /// The scripts' state and report, out of this world (an install
    /// carries them to the next).
    pub(crate) fn take_scripts(&mut self) -> Scripts {
        self.world
            .remove_resource::<Scripts>()
            .unwrap_or_else(Scripts::default)
    }

    /// An install's scripts: those carried over, without what the module
    /// left behind still waiting (its events, deferred code and queues),
    /// then the module's entry events: on its first install each
    /// creature's spawn script in the module's order, `Mod_OnModLoad`, then
    /// for each seat's PC (its own player, or the creature the first seat
    /// is switched to; not a companion a guest drives: it came with the
    /// party) `Mod_OnClientEntr` and the area's `OnEnter`.
    pub(crate) fn enter_module(&mut self, carried: Scripts) {
        let mut carried = Scripts {
            state: carried.state,
            report: carried.report,
            ..Scripts::default()
        };
        let state = &mut carried.state;
        let left =
            state.pending.len() + state.delayed.len() + state.actions.len() + state.strikes.len();
        if left > 0 {
            carried
                .report
                .trace
                .push(format!("left behind: {left} scripts and queues waiting"));
        }
        // The module left took what waited for its own (`State::left`);
        // what is still here is the party's, which arrives afresh. A queue
        // dropped frees its owner: KOTOR's cutscene locks one
        // (`SetCommandable(FALSE)`) behind a queue that ends by freeing it
        // (Carth's walk to the Council on Dantooine, dropped by the dream:
        // locked for good, he never rejoined).
        state.pending.clear();
        state.delayed.clear();
        for id in state.actions.keys() {
            state.uncommandable.remove(id);
        }
        state.actions.clear();
        state.occupants.clear();
        state.perceived.clear();
        state.rounds.clear();
        // Heartbeats a script ran keep their period in their own module.
        state.beats.clear();
        // What the left module's queues asked of the minds, and the world
        // as its step last saw it: the new one's is seen afresh.
        state.goals.clear();
        state.intents.clear();
        state.watch = Watch::default();
        // Blows on their way and a screen still open are the left module's.
        state.strikes.clear();
        // The bodies kept after death are the left module's (their ids are
        // its own: kept on, they named creatures of the next one).
        state.kept.clear();
        state.party.selection = None;
        // The player restrict mode is the module's: TSL's asteroid walk
        // sets it on entering (a_104per_enter, every time) and in the
        // airlock before it (a_airlockout), and no script of the games
        // ever turns it off. Carried on, it held for the rest of the game,
        // and every hostile's AI that asks it (k_ai_master's perception
        // and heartbeat) stood idle beside the player: Czerka's mercenaries
        // in the Ithorian compound, and the companions' talk
        // (k_hen_*dlg) refused.
        state.party.restrict = false;
        // The left module's conversations end without their scripts
        // (those belong to the module left behind).
        state.talk.conversations.clear();
        state.talk.asked.clear();
        state.talk.barks.clear();
        // Trades end with the module left (its stores stay in it), and
        // matches (an exit's: a script's ended as it asked).
        state.trades.clear();
        state.leave_matches();
        // A module come back to: what it left waiting goes on.
        if let Some(module) = self.level().module() {
            let now = self.world.resource::<super::Clock>().time_ms;
            state.return_to(&module.name, now);
        }
        // The animations the dropped queues played stop with them.
        for mut fighting in self
            .world
            .query::<&mut Fighting>()
            .iter_mut(&mut self.world)
        {
            fighting.0.acting = None;
        }
        let level = self.level().clone();
        if let (Some(module), Some(library)) = (level.module(), level.scripts()) {
            let ids: BTreeMap<u32, u32> = self
                .world
                .query::<(&Id, &Object)>()
                .iter(&self.world)
                .map(|(id, object)| (object.index, id.0.0))
                .collect();
            if state.spawned.insert(script::key(&module.name)) {
                // The first visit: the module's placements come alive,
                // every one's starting items (I19), then the creatures'
                // spawn scripts. What play made (the companions carried in
                // among it) came alive when it was made.
                let instances = &self.world.resource::<super::Module>().instances;
                let placed: Vec<(u32, &crate::module::ObjectData)> = instances
                    .placements()
                    .filter_map(|(number, facts)| Some((*ids.get(&number)?, facts)))
                    .collect();
                for (id, facts) in &placed {
                    let creature = facts.kind == crate::module::ObjectKind::Creature;
                    state.stock(library, *id, &facts.inventory, creature);
                }
                for (id, facts) in &placed {
                    if let Some(script) = Hook::Spawn.of(facts) {
                        state.pending.push(Pending::new(script, ObjectId(*id)));
                    }
                }
            }
            let mut players: Vec<(u8, ObjectId)> = self
                .world
                .query::<(&Id, &Avatar, Option<&Object>)>()
                .iter(&self.world)
                .filter(|(_, avatar, object)| super::switch::pc(avatar, *object))
                .map(|(id, avatar, _)| (avatar.0.0, ObjectId(id.0.0)))
                .collect();
            players.sort();
            let mut queue = |hook: Hook, caller: ObjectId, entering: Option<ObjectId>| {
                let (holder, scripts) = if caller == MODULE {
                    (script::Holder::Module, &module.module_scripts)
                } else {
                    (script::Holder::Area, &module.area_scripts)
                };
                if let Some(script) = hook.in_scripts(holder, scripts) {
                    let mut pending = Pending::new(script, caller);
                    pending.entering = entering;
                    state.pending.push(pending);
                }
            };
            queue(Hook::ModuleLoad, MODULE, None);
            for (_, player) in players {
                queue(Hook::Enter, MODULE, Some(player));
                queue(Hook::Enter, AREA, Some(player));
            }
        }
        self.world.insert_resource(carried);
    }
}
