//! Creatures' minds each step (I27; `combat::ai` has the state and the
//! rules), in id order, on the authority and in replays, and a guest's
//! party member's while a script or a conversation moves it (`Mind::steered`,
//! which the snapshot carries); prediction repeats the last command of each
//! creature and of each steered party member.
//!
//! **Observe**, on its look ticks (every `LOOK_EVERY_MS`, each creature
//! at its own moment): the nearest hostile character it can see, or the
//! one it has (its script's attack first: `ActionAttack`, I22 C14) while
//! that one lives, stays hostile, within the fight's reach (`ranges.2da`
//! PartyCombat; a script's however far) and in sight; where
//! it last saw it. In a level the games' scripts run, no mind picks a foe
//! itself: only its script's, or the one it is fighting while the fight
//! lasts. **Choose** its task (`ai::Task`).
//! **Route** with its `Walker` over the navigation mesh, through doors it
//! can open. **Act**: the command for this step, as a player's hands
//! would send it, with its script's talents (on a stage without scripts,
//! the powers `ai::choose_power` picks on a look tick); a blade wielder
//! paced as JA's Jedi (`fight`, `combat::pace`), a gunner as its troopers
//! (`shoot`, `combat::troop`), each deciding on its think ticks and turning
//! at its rate; a blade in a scene that can block holds off in some windows
//! (`GUARD_SHARE`), and one caught in a saber lock hammers attack.
//! In pain, stunned or confused it starts nothing new (JA's pain debounce,
//! C11); confused it has no enemy (a mind trick).

use std::collections::BTreeMap;

use collision::UNIT;

use super::{
    Avatar, Clock, Fighting, Id, Object, Player, PlayerSheet, Static, door_states, faction_of,
    held, sheet_of,
};
use crate::combat::ai::FeatUse;
use crate::combat::ai::{self, Cast, Mind, Task};
use crate::combat::items;
use crate::combat::mind;
use crate::combat::pace::{self, Way};
use crate::combat::saber::{Kit, Style};
use crate::combat::sheet::{Delivery, skills};
use crate::combat::specials::{self, Special};
use crate::combat::troop::{self, Troop};
use crate::input::{Command, Seat, buttons};
use crate::math::{self, Vec3};
use crate::motion::MotionState;
use crate::script::actions::{ROUND_MS, Use};
use crate::snapshot::{ObjectState, SimId};
use bevy_ecs::prelude::*;
use rules::ai::gunners;
use rules::minds::powers;
use rules::minds::tasks;
use script_ncs::vm::ObjectId;

/// How often a mind looks (ms), each creature at its own moment in the
/// period: between looks it keeps its target and aims.
const LOOK_EVERY_MS: u64 = rules::minds::looking::LOOK_EVERY_MS;
/// A blade that can block guards in this share of windows this long (ms)
/// instead of swinging (a stand-in for JA's Jedi AI's parries).
const GUARD_MS: u64 = rules::minds::guarding::GUARD_MS;
const GUARD_SHARE: f32 = rules::minds::guarding::GUARD_SHARE;
/// In a saber lock, attack is pressed and let go every this long (ms).
const LOCK_TAP_MS: u64 = rules::minds::guarding::LOCK_TAP_MS;

/// Someone a mind may see.
#[derive(Clone, Copy)]
struct Seen {
    id: SimId,
    ps: MotionState,
    faction: u8,
    alive: bool,
    stunned: bool,
    /// Its body is a creature model's (`appearance.2da` model type S or
    /// L), which fights with its own strike (`world::strikes`).
    beast: bool,
    droid: bool,
    /// Slow's chain on it, and the lowest level of the powers it could
    /// be dispelled of.
    slowed: bool,
    buff: Option<u8>,
    seat: Option<Seat>,
    /// In the party (a player or a companion), and whom it fights.
    party: bool,
    target: Option<SimId>,
    /// Its Stealth total while in stealth mode, whether it stands still,
    /// its `appearance.2da` size (what a spot check reads).
    stealth: Option<i16>,
    still: bool,
    size: u8,
    /// Its sheet as what runs on it changes it (a round's roll and damage
    /// are against it).
    sheet: crate::combat::Sheet,
    /// What it holds, and whether its swing is under way (a blade
    /// wielder's mind paces itself by them, `combat::pace`).
    arms: pace::Arms,
    swinging: bool,
}

impl Seen {
    fn side(&self) -> crate::module::Side {
        crate::module::Side {
            id: self.id.0,
            faction: self.faction,
            party: self.party,
        }
    }
}

/// Whether a mind picking its own foes (no scripts' perception) spots
/// `entry` from `ps` with `awareness`: any not stealthed; a stealthed one
/// past the spot check, every roll its middle (`stealth::Rolls::MEAN`).
fn spots(
    ps: &MotionState,
    (awareness, true_seeing): (i16, bool),
    in_combat: bool,
    entry: &Seen,
) -> bool {
    let Some(stealth) = entry.stealth else {
        return true;
    };
    let (still, running) = crate::stealth::pace(ps.velocity);
    let (sin, cos) = math::sin_cos_deg(ps.view[1]);
    let watcher = crate::stealth::Watcher {
        awareness,
        rolls: crate::stealth::Rolls::MEAN,
        in_combat,
        running,
        still,
        npc: true,
        facing: [cos, sin],
        true_seeing,
    };
    let sneaker = crate::stealth::Sneaker {
        stealth,
        rolls: crate::stealth::Rolls::MEAN,
        still: entry.still,
        player: entry.seat.is_some(),
        size: entry.size,
        silent: false,
    };
    let toward = [
        (entry.ps.origin[0] - ps.origin[0]) * collision::UNIT,
        (entry.ps.origin[1] - ps.origin[1]) * collision::UNIT,
    ];
    crate::stealth::spot_score(&watcher, &sneaker, toward) > 0
}

/// A number in [0, 1) from `window`, `id` and `salt`: a mind's dice.
fn pick(window: u64, id: SimId, salt: u64) -> f32 {
    crate::rng::hashed(window, id.0, salt.wrapping_mul(0x94D0_49BB_1331_11EB))
}

/// How far a script's goal (a moving target) goes before its route is
/// planned again (units).
const SCRIPT_GOAL_MOVED: f32 = tasks::SCRIPT_GOAL_MOVED;

/// The think system (see the module documentation).
#[allow(clippy::type_complexity)]
pub(super) fn think(
    clock: Res<Clock>,
    level: Res<Static>,
    scripts: Res<super::scripts::Scripts>,
    (mut driven, mut item_talents, mut rounds): (
        ResMut<super::Driven>,
        ResMut<super::scripts::ItemTalents>,
        ResMut<super::scripts::Rounds>,
    ),
    (talking, world_held, made_walls): (
        Res<super::Talking>,
        Res<super::scene::Held>,
        Res<super::MadeFaces>,
    ),
    (objects, placed): (
        Query<&Object>,
        Query<(&Id, &super::Placed), Without<Player>>,
    ),
    mut actors: Query<(
        &Id,
        Option<&Avatar>,
        Option<&Object>,
        Option<&PlayerSheet>,
        &Player,
        &mut Fighting,
    )>,
) {
    let scene_held = &*world_held;
    let talks = |id: SimId| scene_held.talks(&talking.0, id);
    // A conversation holding the world (S1) runs every mind of the area on
    // KOTOR's rules (S4).
    let world_held = scene_held.world;
    // A body KOTOR's walker carried on last step (`walk::kotor_step`): its
    // walk never stalls, and it goes round bodies as it bumps into them, not
    // after a stall. A walk move_body steps (no floor on the mesh ahead), or one
    // the walker held still, stalls and gives up as it always has.
    let carried = |ps: &MotionState| {
        ps.flags & crate::motion::motion_flags::CARRIED != 0 && ps.velocity != [0.0; 3]
    };
    // A party member a guest drives thinks only while its script or a
    // conversation moves it (`Mind::steered`); else its seat's commands do.
    let steers = |id: SimId| {
        scripts.state.goals.contains_key(&id.0)
            || scripts.state.intents.contains_key(&id.0)
            || talks(id)
    };
    // What its script queue holds, for the steps that move it
    // (`world::move_actors`), carried by the snapshot to the clients, which
    // run no scripts.
    let state = &scripts.state;
    for (id, avatar, object, .., mut fighting) in &mut actors {
        let steered = avatar.is_some() && object.is_some() && steers(id.0);
        let who = id.0.0;
        let moving = state.goals.contains_key(&who);
        let queued = moving
            || state.intents.contains_key(&who)
            || state
                .actions
                .get(&who)
                .is_some_and(|queue| !queue.is_empty());
        let mind = &fighting.0.mind;
        if (mind.steered, mind.queued, mind.moving) != (steered, queued, moving) {
            let mind = &mut fighting.0.mind;
            (mind.steered, mind.queued, mind.moving) = (steered, queued, moving);
        }
    }
    item_talents.0.clear();
    rounds.0.clear();
    let Some(module) = level.0.module.clone() else {
        return;
    };
    let now = clock.time_ms;
    let states = door_states(&level.0, &objects);
    let doors: BTreeMap<u32, ObjectState> = objects
        .iter()
        .map(|object| (object.index, object.state))
        .collect();
    let door_state = |index: u32| doors.get(&index).copied().unwrap_or(ObjectState::GONE);
    // While the world is held the party opens no door (S4): its walks route
    // round a closed one, or nowhere, and give up.
    let shut_state = |index: u32| crate::walk::opening_none(door_state(index));
    let party_doors: &dyn Fn(u32) -> ObjectState =
        if world_held { &shut_state } else { &door_state };
    // Their routes keep off the placeables play made (`walk::MadeWalls`),
    // as the navigation mesh, baked before, does not know them.
    let walled = &made_walls.0;
    let party_ways = crate::walk::Walled {
        doors: party_doors,
        walled,
    };
    let door_ways = crate::walk::Walled {
        doors: &door_state,
        walled,
    };
    // Everyone as they stand before this step, by id.
    let mut seen: Vec<Seen> = actors
        .iter()
        .map(|(id, avatar, object, sheet, player, fighting)| {
            let sheet = sheet_of(sheet, object);
            let now_sheet = crate::combat::effects::sheet_now(sheet, &fighting.0, now);
            Seen {
                id: id.0,
                ps: player.0,
                faction: faction_of(&fighting.0, &sheet),
                alive: fighting.0.alive(),
                stunned: fighting.0.force.stunned(now),
                slowed: fighting.0.effects.slow(now) > 0,
                buff: crate::combat::effects::weakest_buff(&fighting.0, now),
                droid: sheet.flags & crate::combat::sheet::flags::DROID != 0,
                beast: object
                    .and_then(|object| object.facts.as_ref())
                    .is_some_and(|facts| facts.strike.is_some()),
                seat: avatar.map(|a| a.0),
                party: super::in_party(avatar, &fighting.0),
                target: fighting.0.target,
                stealth: fighting.0.covert.stealth.then(|| {
                    crate::stealth::total(None, &sheet, &now_sheet, skills::STEALTH).unwrap_or(0)
                }),
                sheet: now_sheet,
                arms: match held(&sheet, &fighting.0).delivery {
                    Delivery::Saber(_) => pace::Arms::Saber {
                        lit: fighting.0.saber.on,
                    },
                    Delivery::Blaster(_) => pace::Arms::Gun {
                        firing: fighting.0.last_command.buttons & buttons::ATTACK != 0,
                    },
                    _ => pace::Arms::Other,
                },
                swinging: {
                    let swing = fighting.0.saber.current_move;
                    swing.attack() || swing.start() || swing.transition()
                },
                still: crate::stealth::pace(player.0.velocity).0,
                size: object
                    .and_then(|object| object.facts.as_ref())
                    .and_then(|facts| facts.body)
                    .map_or(3, |body| body.size),
            }
        })
        .collect();
    seen.sort_by_key(|entry| entry.id);
    // The doors and placeables a scene's shot is decided at, where it is
    // aimed: a little above where each stands [verify: the height].
    let object_aims: BTreeMap<SimId, Vec3> = placed
        .iter()
        .filter(|(id, _)| (scripts.state.strikes.iter()).any(|strike| strike.target.0 == id.0.0))
        .map(|(id, placed)| {
            let mut aim = math::scale(placed.0, 1.0 / UNIT);
            aim[2] += OBJECT_AIM_UP;
            (id.0, aim)
        })
        .collect();
    let visible = |from: Vec3, to: Vec3| level.0.sees(states.as_deref(), from, to);
    // Where the games' scripts run, creatures fight whom they name
    // (`ActionAttack`, KOTOR's AI in `k_ai_master`), as the original engine
    // starts no fight of its own: a scene keeps its creatures idle until it
    // plays (`SW_FLAG_AI_OFF`), and one with no AI scripts never fights.
    // On a stage without scripts (probes, duels) each picks its own.
    let own_choice = level.0.scripts().is_none();
    // The companion leading the party takes the first seat's commands, and
    // that seat's own character, nobody's now, goes with it as a companion
    // does: following the one led and joining its fights (`joins`).
    let lent = super::lent(&scripts.state.party);
    let unmanned = |avatar: Option<&Avatar>| {
        lent.is_some() && avatar.is_some_and(|avatar| avatar.0 == super::LEADER_SEAT)
    };
    let solo = scripts.state.party.solo;
    // A scene plays on those taking part in a conversation under way (a
    // cutscene's camera is on them): a fighter is in one when it or its
    // target is. While the world is held, everyone is (S4). Another seat's
    // own conversation elsewhere holds no one here.
    let in_scene = |id: SimId| scene_held.scene(&talking.0, id);
    // A player a script walks (a move it queued while the player is held in
    // a conversation): its route, walked as a creature's. A companion a
    // guest drives is its mind's while a script steers it (below).
    driven.0.clear();
    let mut players: Vec<_> = actors
        .iter_mut()
        .filter(|(_, avatar, object, ..)| {
            avatar.is_some() && object.is_none() && !unmanned(*avatar)
        })
        .collect();
    players.sort_by_key(|(id, ..)| id.0);
    for (id, _, _, _, player, mut fighting) in players {
        // Back in its player's hands: what it was doing unmanned is over.
        if fighting.0.mind.task != Task::Idle {
            fighting.0.mind.task = Task::Idle;
            fighting.0.target = None;
        }
        let mind = &mut fighting.0.mind;
        let Some(goal) = scripts.state.goals.get(&id.0.0) else {
            mind.walker = None;
            continue;
        };
        if let Some(walker) = mind.walker.as_mut().filter(|_| carried(&player.0)) {
            walker.keep_headway(now, &player.0);
        }
        let command = scripted_walk(mind, &level.0, &player.0, goal, now, &party_ways);
        fighting.0.last_command = command;
        driven.0.push(id.0);
    }
    let mut minds: Vec<_> = actors
        .iter_mut()
        .filter(|(_, avatar, .., fighting)| {
            avatar.is_none() || fighting.0.mind.steered || unmanned(*avatar)
        })
        .collect();
    minds.sort_by_key(|(id, ..)| id.0);
    for (id, avatar, object, sheet, player, mut fighting) in minds {
        let fighter = &mut fighting.0;
        // KOTOR's companions are knocked out, not killed: a party member at
        // 0 HP is downed and gets up when the fight is over (`falls`).
        if !fighter.alive() {
            continue;
        }
        let ps = &player.0;
        // Idle, it keeps its heading and looks level again, not down at a
        // target that is gone (JA's navigator levels a walking mind's pitch,
        // its troopers' and Jedi's minds do once they stop aiming; standing
        // idle [verify]).
        let mut idle = Command {
            angles: ps.command_angles,
            ..Command::default()
        };
        idle.angles[0] = mind::looking(ps, 0.0, 0.0).angles[0];
        if fighter.force.confused_until > now {
            fighter.target = None;
        }
        if fighter.pain_until > now
            || fighter.force.stunned(now)
            || fighter.force.confused_until > now
        {
            fighter.last_command = idle;
            continue;
        }
        let me = sheet_of(sheet, object);
        let mine = faction_of(fighter, &me);
        let me_now = crate::combat::effects::sheet_now(me, fighter, now);
        // What its powers cost it where it stands (hundredths; TSL's
        // adjustments, `ModuleData::power_cost`).
        let party = super::in_party(avatar, fighter);
        let place = crate::math::scale(ps.origin, collision::UNIT);
        let cost = |row: u16| {
            module
                .power_cost(row, &me_now, party, place)
                .map(|cost| cost * 100)
        };
        let awareness = crate::stealth::total(
            level.0.scripts().map(|library| &**library),
            &me,
            &me_now,
            skills::AWARENESS,
        )
        .unwrap_or(0);
        let senses = (
            awareness,
            me_now.gear.has(crate::combat::gear::traits::TRUE_SEEING),
        );
        // Bare hands punch, closer in, and a beast strikes within its own
        // reach (`strikes`): each walks up and stands there attacking, as
        // KOTOR's creatures do, not stepping about as a swing's keys would.
        let facts = object.and_then(|object| object.facts.as_ref());
        let space = facts.map_or(0.0, |facts| facts.personal_space);
        let unarmed = held(&me, fighter).delivery == Delivery::Unarmed;
        let close = match facts.and_then(|facts| facts.strike) {
            _ if !unarmed => mind::Attack::Swing,
            Some(strike) => mind::Attack::Blow(super::strikes::strike_reach(&strike, space)),
            None => mind::Attack::Blow(super::strikes::punch_reach(space)),
        };
        let eye = mind::eye(ps);
        let distance = |other: &MotionState| math::length(math::sub(other.origin, ps.origin));
        let standing = module.repute.with(&scripts.state.reputation);
        let me_side = crate::module::Side {
            id: id.0.0,
            faction: mine,
            party: super::in_party(avatar, fighter),
        };
        let hostile = |entry: &Seen| {
            entry.alive
                && crate::module::relation(Some(standing), me_side, entry.side())
                    == crate::combat::Relation::Hostile
        };
        // How far it spots a foe (its `PerceptionRange`'s sight) and how far
        // a fight it is in reaches (`ranges.2da` PartyCombat), units.
        let sight = facts
            .and_then(|facts| facts.senses)
            .map_or(mind::SIGHT, |senses| senses.sight / UNIT);
        let reach = module.ranges.party_combat_m / UNIT;
        // Observe.
        let looking = (now + u64::from(id.0.0) * 37) % LOOK_EVERY_MS < u64::from(clock.msec);
        // A script's attack (C14: `ActionAttack`) names the one it keeps
        // while it lives and stays hostile, however far (KOTOR's attack
        // walks to its target); else the one it had, within the fight's
        // reach.
        let intent = scripts.state.intents.get(&id.0.0);
        // A party member nobody plays, out of a scene and not moved by a
        // script, joins a fight the party is in when its script names no
        // foe: KOTOR's AI scripts answer only the moments they are told of,
        // and a foe seen before the fight began is told of no more. It
        // starts no fight of its own.
        let follows = fighter.mind.leader.is_some() || unmanned(avatar);
        let joins = follows
            && !solo
            && !world_held
            && !in_scene(id.0)
            && !scripts.state.goals.contains_key(&id.0.0);
        // In the party's fight: it aims at one of the party, or one of them
        // at it.
        let in_fight = |entry: &Seen| {
            let party = |aim: SimId| {
                seen.binary_search_by_key(&aim, |other| other.id)
                    .is_ok_and(|at| seen[at].party)
            };
            entry.target.is_some_and(party)
                || seen
                    .iter()
                    .any(|other| other.party && other.target == Some(entry.id))
        };
        // A scripted one keeps whom it fought only while the fight goes on.
        let kept = fighter
            .target
            .filter(|_| own_choice || joins || fighter.combat_until > now);
        let known = intent
            .map(|intent| SimId(intent.target.0))
            .or(kept)
            .and_then(|target| seen.binary_search_by_key(&target, |entry| entry.id).ok())
            .map(|at| &seen[at])
            .filter(|entry| hostile(entry) && (intent.is_some() || distance(&entry.ps) <= reach));
        // A script's attack keeps its target out of sight too, walking to
        // it, as KOTOR's attack does (reone's follows it until in range): a
        // Star Forge freezer told to attack lost the player behind a
        // railing, stood idle, and the door it held stayed shut.
        let target = match known {
            Some(entry) if !looking || intent.is_some() || visible(eye, mind::chest(&entry.ps)) => {
                Some(*entry)
            }
            _ if looking && own_choice => seen
                .iter()
                .filter(|entry| hostile(entry) && distance(&entry.ps) <= sight)
                .filter(|entry| visible(eye, mind::chest(&entry.ps)))
                .filter(|entry| spots(ps, senses, fighter.combat_until > now, entry))
                .min_by(|a, b| distance(&a.ps).total_cmp(&distance(&b.ps)))
                .copied(),
            _ if looking && joins && intent.is_none() => seen
                .iter()
                .filter(|entry| hostile(entry) && distance(&entry.ps) <= sight)
                .filter(|entry| in_fight(entry))
                .filter(|entry| visible(eye, mind::chest(&entry.ps)))
                .filter(|entry| spots(ps, senses, fighter.combat_until > now, entry))
                .min_by(|a, b| distance(&a.ps).total_cmp(&distance(&b.ps)))
                .copied(),
            _ => None,
        };
        // A foe newly spotted, with no fight under way (none hit it or
        // was attacked by it lately, `combat_until`): it takes a moment to
        // act, by its Awareness. A hit, or seeing its foe again after a
        // moment out of sight, gives no such moment.
        let spotted = fighter.target.is_none() && fighter.combat_until <= now;
        fighter.target = target.map(|entry| entry.id);
        let in_party = super::in_party(avatar, fighter);
        // The doors as this mind's walks may meet them: a party member's
        // shut while the world is held.
        let opening: &dyn crate::walk::Ways = if in_party { &party_ways } else { &door_ways };
        let own = Own {
            saber_move: fighter.saber.current_move,
            busy: fighter.weapon_wait_ms > 0,
            pain_until: fighter.pain_until,
            health: fighter.hp as f32 / 100.0,
            ally: in_party,
        };
        let in_hand = held(&me, fighter);
        // When its last round's swing began (sim ms): its next round waits
        // a round on (`begin_round`).
        let swung_at = scripts.state.swung.get(&id.0.0).copied();
        let mind = &mut fighter.mind;
        // In a scene a script's attack swings at once: KOTOR's round has no
        // moment of awareness.
        let scene = target.is_some_and(|entry| in_scene(id.0) || in_scene(entry.id));
        if spotted && target.is_some() && !scene {
            let awareness = me.skill(skills::AWARENESS).clamp(0, 255) as u8;
            mind.aware_at = now + ai::reaction_ms(awareness);
        }
        if looking && let Some(entry) = target {
            mind.last_seen = Some((entry.ps.origin, now));
        }
        // A companion that sees no one joins the party's fight: the
        // nearest hostile fighting one of the party, within the fight's reach,
        // is where it goes until it sees it. Not while the world is held (S4).
        let joining = ((own_choice && mind.leader.is_some()) || joins)
            && target.is_none()
            && looking
            && !world_held;
        let threat = joining
            .then(|| {
                seen.iter()
                    .filter(|entry| hostile(entry) && distance(&entry.ps) <= reach)
                    .filter(|entry| {
                        entry.target.is_some_and(|aim| {
                            seen.binary_search_by_key(&aim, |other| other.id)
                                .is_ok_and(|at| seen[at].party)
                        })
                    })
                    .min_by(|a, b| distance(&a.ps).total_cmp(&distance(&b.ps)))
            })
            .flatten();
        if let Some(entry) = threat {
            mind.last_seen = Some((entry.ps.origin, now));
            if mind.task == Task::Follow || mind.task == Task::Idle {
                mind.task = Task::Chase;
            }
        }
        // Choose.
        // Whom it follows: its seat's body, the companion that seat leads
        // through while it does (`lent`); none for the one led.
        let led = |seat: Seat| lent.filter(|_| seat == super::LEADER_SEAT);
        let leader = mind
            .leader
            .or(unmanned(avatar).then_some(super::LEADER_SEAT))
            .filter(|seat| led(*seat) != Some(id.0))
            .and_then(|seat| {
                seen.iter().find(|entry| match led(seat) {
                    Some(body) => entry.id == body,
                    None => entry.seat == Some(seat),
                })
            })
            .map(|entry| entry.ps);
        let leashed =
            leader.is_some_and(|leader| math::flat_distance(leader.origin, ps.origin) > ai::LEASH);
        let chasing = mind
            .last_seen
            .is_some_and(|(_, when)| now < when + ai::CHASE_MS)
            && matches!(mind.task, Task::Engage | Task::Chase);
        mind.task = match (target, leader) {
            (Some(_), _) if !leashed => Task::Engage,
            _ if chasing && !leashed => Task::Chase,
            (_, Some(_)) => Task::Follow,
            _ => Task::Idle,
        };
        // While the world is held a companion keeps no formation (S4): it
        // neither follows nor chases, and stands. A script's move still
        // moves it (below).
        if world_held && follows && matches!(mind.task, Task::Follow | Task::Chase) {
            mind.task = Task::Idle;
        }
        // Nor does anyone run after a foe it lost while the world is held:
        // a scene's creatures fight as its scripts tell them, and stand
        // when their foe is gone (Taris's duel: Gerlon, Deadeye Duncan down
        // and out of the fight, ran up to him).
        if world_held && mind.task == Task::Chase {
            mind.task = Task::Idle;
        }
        if mind.task != Task::Engage && mind.hold_until > now {
            mind.hold_until = 0;
        }
        // A body standing in its way, gone round: by move_body's walks after a
        // stall; KOTOR's walker goes round as it bumps (`walk::kotor_step`),
        // and never stalls.
        let kotor = carried(ps);
        if let Some(walker) = mind.walker.as_mut().filter(|_| kotor) {
            walker.keep_headway(now, ps);
        }
        if let Some(walker) = mind
            .walker
            .as_mut()
            .filter(|walker| !kotor && walker.stalls > 0)
        {
            let bodies: Vec<Vec3> = seen
                .iter()
                .filter(|entry| entry.alive && entry.id != id.0)
                .filter(|entry| !super::bodies_pass(&level.0, in_party, entry.party))
                .map(|entry| entry.ps.origin)
                .collect();
            walker.around(&level.0, ps, &bodies);
        }
        // A script's move (`script::actions`) when it is not fighting: its
        // mind routes there, walking unless told to run.
        let scripted = scripts
            .state
            .goals
            .get(&id.0.0)
            .filter(|_| mind.task != Task::Engage);
        // Route and act. A gunner with a passive attack (a scene's) fires
        // only the shots decided for it (`decides`).
        let mut decides = None;
        let mut command = match mind.task {
            _ if let Some(goal) = scripted => scripted_walk(mind, &level.0, ps, goal, now, opening),
            Task::Engage => {
                let entry = target.expect("engaging a target");
                let weapon = in_hand;
                // A gun's reach in units (none known: as far as it sees).
                let reach = weapon.delivery.ranged().then(|| match weapon.reach_m {
                    0 => f32::MAX,
                    metres => f32::from(metres) / UNIT,
                });
                let kit = Kit::of(&me);
                let passive = intent.is_some_and(|intent| intent.passive);
                // A creature's script attack (`ActionAttack`) on another
                // outside the party runs in KOTOR's combat rounds (reone's
                // `Combat`), as every fight of the original does; the party's
                // fights run on JA's swings (I22). It fights as a scene's
                // fighters do, wherever its camera looks.
                let duel = intent.is_some() && !entry.party && !in_party;
                let guards = kit.blade(0).is_some_and(|blade| kit.defence(blade) > 0);
                // KOTOR's fighting: a scene's, a scripted attack's, a round's.
                let kotor = scene || passive || duel;
                // A blade in hand, out of a scene: JA's Jedi minds' pacing.
                let paced = close == mind::Attack::Swing
                    && reach.is_none()
                    && !kotor
                    && matches!(
                        weapon.delivery,
                        Delivery::Melee | Delivery::Saber(_) | Delivery::Baton
                    );
                let mut command = match (reach, weapon.delivery) {
                    // A scene's gunner fires its script's shots (below).
                    (Some(_), _) if passive => idle,
                    (Some(reach), Delivery::Blaster(blaster)) => {
                        let gun = Gun {
                            reach,
                            blaster,
                            pair: me.pistol_pair(),
                            ally: in_party,
                            still: scene || duel,
                        };
                        let others: Vec<Vec3> = seen
                            .iter()
                            .filter(|other| other.alive && other.id != id.0)
                            .map(|other| other.ps.origin)
                            .collect();
                        shoot(
                            mind,
                            id.0,
                            ps,
                            &entry,
                            gun,
                            (now, clock.msec),
                            (&level.0, opening),
                            (&visible, &others),
                        )
                    }
                    _ if paced => fight(
                        mind,
                        id.0,
                        ps,
                        &own,
                        &entry,
                        (now, clock.msec),
                        &level.0,
                        opening,
                    ),
                    _ => engage(
                        mind,
                        id.0,
                        ps,
                        &entry,
                        close,
                        (now, kotor, world_held),
                        &level.0,
                        opening,
                    ),
                };
                // A passive attack (a scene's, `CutsceneAttack`) faces its
                // target and starts nothing: a gunner fires only the shots
                // its script decided, a melee attacker steps into reach and
                // stands, its swing the animation the script named
                // (`script::effects::blows`).
                // A melee fight between creatures nobody plays runs in
                // KOTOR's rounds, not the saber's own swings: its script's
                // attack, and any while a scene plays it (between the
                // script's attacks too); none while it closes in either (a
                // swing there lands nothing, Taris's gang war), a round once
                // its target is within reach.
                // A creature model strikes by its own clips (`strikes`).
                let beast = facts.is_some_and(|facts| facts.strike.is_some());
                let in_rounds = (duel || (scene && !entry.party && !in_party))
                    && !passive
                    && !beast
                    && matches!(
                        weapon.delivery,
                        Delivery::Melee | Delivery::Saber(_) | Delivery::Baton | Delivery::Unarmed
                    );
                if in_rounds {
                    command.buttons &= !buttons::ATTACK;
                }
                if in_rounds && math::flat_distance(entry.ps.origin, ps.origin) <= mind::STRIKE {
                    let strikes = scripts.state.strikes.iter().chain(&rounds.0);
                    let feat = intent.and_then(|intent| match intent.talent {
                        Some(crate::script::actions::Use::Feat(row)) => Some(row),
                        _ => None,
                    });
                    let me_side = (&me_now, beast);
                    if let Some(round) = level.0.scripts().and_then(|library| {
                        begin_round(
                            id.0,
                            (me_side, feat, swung_at),
                            &entry,
                            strikes,
                            (now, level.0.game()),
                            library.rounds(),
                        )
                    }) {
                        rounds.0.push(round);
                    }
                }
                if passive {
                    // A gunner's: its decided shots (below).
                    if weapon.delivery.ranged() {
                        decides = Some(entry);
                    } else if reach.is_some() {
                        let aim = (entry.id, mind::chest(&entry.ps));
                        command = scene_attack(id.0, ps, aim, &[], None, now, clock.msec);
                    } else {
                        command.buttons &= !buttons::ATTACK;
                    }
                }
                // A blade that can block by state (C6) holds off now and then:
                // not attacking, it turns blows and bolts in its arc (a paced
                // one parries when its enemy swings instead). KOTOR's
                // fighters attack every round.
                if guards && !paced && !kotor && pick(now / GUARD_MS, id.0, 9) < GUARD_SHARE {
                    command.buttons &= !buttons::ATTACK;
                }
                command
            }
            Task::Chase => {
                let (spot, _) = mind.last_seen.expect("chasing a place");
                if math::flat_distance(spot, ps.origin)
                    <= crate::walk::REACHED * tasks::CHASE_ARRIVED
                {
                    mind.last_seen = None;
                    mind.walker = None;
                    idle
                } else {
                    mind.walk_to(spot, tasks::REPLAN)
                        .command(&level.0, ps, now, opening)
                }
            }
            Task::Follow => {
                let leader = leader.expect("following a leader");
                // Each makes for its own place beside the leader (`follow_slot`)
                // once it falls behind or stands in another's place or the
                // leader's way, and goes on until it is there; with no place
                // to have, it runs back to the leader. In solo mode the
                // companions stand where they are.
                let followers: Vec<(SimId, Vec3)> = seen
                    .iter()
                    .filter(|entry| entry.party && entry.alive)
                    .filter(|entry| {
                        Some(entry.id) != lent
                            && entry
                                .seat
                                .is_none_or(|seat| lent.is_some() && seat == super::LEADER_SEAT)
                    })
                    .map(|entry| (entry.id, entry.ps.origin))
                    .collect();
                let far = math::flat_distance(leader.origin, ps.origin) > ai::FOLLOW_NEAR;
                let crowded = crowds(id.0, ps.origin, leader.origin, &followers);
                let walking = mind.walker.is_some_and(|walker| !walker.stuck());
                let slot = (far || crowded || walking)
                    .then(|| follow_slot(&level.0, &leader, id.0, &followers))
                    .flatten();
                let short = |slot: Vec3| {
                    math::flat_distance(slot, ps.origin)
                        > rules::walking::route::ARRIVED * crate::walk::REACHED
                };
                let goal = match slot {
                    Some(slot) if far || crowded || (walking && short(slot)) => Some(slot),
                    None if far => Some(leader.origin),
                    _ => None,
                };
                // A walk ended short of a place that has moved on since is
                // planned afresh to where the place is now.
                let ended = mind.walker.is_some_and(|walker| walker.arrived(ps));
                let moved = if ended { 0.0 } else { tasks::FOLLOW_REPLAN };
                match goal.filter(|_| !scripts.state.party.solo) {
                    Some(goal) => mind
                        .walk_to(goal, moved)
                        .command(&level.0, ps, now, opening),
                    None => {
                        mind.walker = None;
                        idle
                    }
                }
            }
            Task::Idle => {
                mind.walker = None;
                idle
            }
        };
        // A blow decided for a gunner (a scene's `CutsceneAttack`, an
        // assured hit) goes out as its shot, whatever its mind is about: it
        // stands facing its target through the round, as KOTOR's round
        // holds its attacker, and fires as the blow falls due.
        // A blow at an object that is no fighter (end_m01aa's Sith shoot
        // the power conduit, `end_explosion`) is aimed at the object.
        let mut fires_decided = false;
        if let Delivery::Blaster(blaster) = in_hand.delivery {
            let strikes = &scripts.state.strikes;
            // Its target a character's chest, or a door's or placeable's
            // place (a scene's shot at a fuel cylinder that then blows).
            let decided = strikes
                .iter()
                .find(|strike| strike.by.0 == id.0.0)
                .and_then(|strike| {
                    let target = SimId(strike.target.0);
                    match seen.binary_search_by_key(&target, |entry| entry.id) {
                        Ok(at) => Some((target, mind::chest(&seen[at].ps))),
                        Err(_) => object_aims.get(&target).map(|aim| (target, *aim)),
                    }
                });
            let decided = decided.or(decides.map(|entry| (entry.id, mind::chest(&entry.ps))));
            if let Some(target) = decided {
                command = scene_attack(id.0, ps, target, strikes, Some(blaster), now, clock.msec);
                fires_decided = true;
            }
        }

        // Powers, grenades and feat specials: its script's talent when it
        // chose one (C14); on a stage without AI scripts its own on a look
        // tick (where they run, `k_ai_master` picks every talent: powers,
        // medpacs, grenades, feats); a channel's, a throw's or a special's
        // button held on.
        let ready = mind.aware_at <= now && mind.cast_after <= now && mind.hold_until <= now;
        let engaged = mind.task == Task::Engage;
        let dice = pick(now / LOOK_EVERY_MS, id.0, 7);
        let gun_special = match held(&me, fighter).delivery {
            Delivery::Blaster(blaster) => Special::of(&me, blaster),
            _ => None,
        };
        let talent = intent.and_then(|intent| intent.talent);
        if let Some(talent) = talent.filter(|_| ready) {
            let fighter_now = *fighter;
            let on = intent
                .map(|intent| SimId(intent.target.0))
                .filter(|on| *on != id.0)
                .and_then(|on| seen.binary_search_by_key(&on, |entry| entry.id).ok());
            match talent {
                // A grenade it carries, at another: thrown at it as its
                // button throws one, the row chosen, once it faces it (its
                // fight turns it there meanwhile; the talent lasts its
                // round); its burst casts the row's impact script. Once a
                // round.
                Use::Power(row)
                    if on.is_some()
                        && items::use_of(row) == Some(items::Use::Grenade)
                        && fighter.pack.left_of(&me.items, row).is_some() =>
                {
                    let them = on.map(|at| &seen[at].ps);
                    let far = them.map_or(0.0, distance);
                    let facing = them.is_some_and(|them| {
                        let (_, yaw) = mind::aim(eye, mind::chest(them));
                        mind::turn(yaw, ps.view[1]).abs() <= ai::THROW_FACING
                    });
                    if facing
                        && cast_into(
                            &mut fighter.mind,
                            &mut command,
                            Some(ai::throw(far)),
                            now,
                            dice,
                        )
                    {
                        fighter.pack.queued = Some(row);
                        let mind = &mut fighter.mind;
                        mind.cast_after = mind.cast_after.max(now + ROUND_MS);
                    }
                }
                // An item's spell (a medpac, a droid's utility item): used
                // through its item in the scripts' step, once a round.
                Use::Power(row) if !me.powers.has(row) => {
                    let on = intent.map_or(id.0, |intent| SimId(intent.target.0));
                    item_talents.0.push((id.0, row, on));
                    let mind = &mut fighter.mind;
                    mind.cast_after = mind.cast_after.max(now + ROUND_MS);
                }
                // Within its reach of whom it is used on (its script's
                // target, or itself).
                Use::Power(row) => {
                    let on = intent
                        .map(|intent| SimId(intent.target.0))
                        .filter(|on| *on != id.0);
                    let near = on
                        .and_then(|on| seen.binary_search_by_key(&on, |entry| entry.id).ok())
                        .is_none_or(|at| distance(&seen[at].ps) <= ai::power_reach(row));
                    let cast = near
                        .then(|| ai::power_cast(row, &me, &fighter_now, &cost))
                        .flatten();
                    // Once a round: the talent's action lasts the round.
                    if cast_into(&mut fighter.mind, &mut command, cast, now, dice) {
                        let mind = &mut fighter.mind;
                        mind.cast_after = mind.cast_after.max(now + ROUND_MS);
                    }
                }
                Use::Feat(row) => match ai::feat_use(row, &me, &fighter_now, gun_special) {
                    Some(FeatUse::Cast(cast)) => {
                        cast_into(&mut fighter.mind, &mut command, Some(cast), now, dice);
                        let mind = &mut fighter.mind;
                        mind.cast_after = mind.cast_after.max(now + ROUND_MS);
                    }
                    Some(FeatUse::Style(style)) if wants_style(&me, fighter, style) => {
                        command.buttons |= buttons::SABER_STYLE;
                    }
                    Some(FeatUse::Style(_)) => {}
                    None => {}
                },
            }
        } else if own_choice
            && engaged
            && looking
            && ready
            && let Some(entry) = target
        {
            let fighter_now = *fighter;
            let foe = ai::Foe {
                distance: distance(&entry.ps),
                stunned: entry.stunned,
                droid: entry.droid,
                slowed: entry.slowed,
                buff: entry.buff,
            };
            let cast = ai::choose_power(&me, &fighter_now, foe, &cost, dice, now)
                .or_else(|| ai::choose_item(&me, &fighter_now, now))
                .or_else(|| ai::choose_grenade(&me, &fighter_now, distance(&entry.ps), dice))
                // A gunner's feat special when no power is due.
                .or_else(|| ai::choose_special(gun_special, &fighter_now, specials::cost(&me)));
            cast_into(&mut fighter.mind, &mut command, cast, now, dice);
        }
        if fighter.mind.hold_until > now {
            command.buttons |= fighter.mind.hold;
        }
        // Caught in a saber lock it hammers attack, a press every two
        // `LOCK_TAP_MS` (JA's NPCs push their locks; the rate is a stand-in).
        if fighter.saber.lock.is_some() {
            command.buttons &= !buttons::ATTACK;
            if (now / LOCK_TAP_MS + u64::from(id.0.0)).is_multiple_of(2) {
                command.buttons |= buttons::ATTACK;
            }
        }
        // Not yet aware: it turns to look, and nothing more (a scene's
        // attack, a decided shot, is its script's).
        let decided = intent.is_some_and(|intent| intent.passive) || fires_decided;
        if fighter.mind.aware_at > now && !decided && !scene {
            command = Command {
                angles: command.angles,
                ..Command::default()
            };
        }
        // While the world is held a party member rolls no attack of its own
        // (S4); a scene's attack fires the shots its script decided.
        if world_held && in_party && !decided {
            command.buttons &= !super::scene::ROLLS;
        }
        fighter.last_command = command;
    }
}

/// A script's move for a body standing as `ps`: its mind routes to the
/// goal, walking unless told to run. Within the walker's arrival but short
/// of the move's range (its spot to `minds.tasks.script_spot_m`,
/// `script::actions`), the last step
/// goes straight in, as KOTOR's mover stops on the spot itself: the walker
/// stood 0.3 m off the Unknown World's elder's mark, his move never done
/// and his conversation paused on it.
fn scripted_walk(
    mind: &mut Mind,
    level: &super::Level,
    ps: &MotionState,
    goal: &crate::script::actions::Goal,
    now: u64,
    state: &dyn crate::walk::Ways,
) -> Command {
    let point = crate::walk::on_floor(level, goal.point);
    let short = math::flat_distance(point, ps.origin) > goal.range.max(tasks::SCRIPT_SPOT_M / UNIT);
    let walker = mind.walk_to(point, SCRIPT_GOAL_MOVED);
    let mut command = if short && walker.arrived(ps) {
        crate::walk::toward(ps, point)
    } else {
        walker.command(level, ps, now, state)
    };
    if !goal.run {
        walking(&mut command);
    }
    command
}

/// `command` walked rather than run, as a script's move without its run
/// flag is in KOTOR (`ActionMoveToObject(.., FALSE)`, `ActionRandomWalk`):
/// the walking button with the axes JA's client sends with it, `WALK_AXIS`
/// at most (JA's movement lets the button go with fuller ones).
fn walking(command: &mut Command) {
    command.buttons |= buttons::WALKING;
    command.forward = command.forward.clamp(-WALK_AXIS, WALK_AXIS);
    command.right = command.right.clamp(-WALK_AXIS, WALK_AXIS);
}

/// The most a walking command's axes are (JA's client's walk).
const WALK_AXIS: i8 = tasks::WALK_AXIS;

/// Where a following party member `me` stands beside `leader` (its origin,
/// units), among `followers` (every party member nobody plays, by id):
/// behind the way the leader faces (steady through a stalled walk's
/// sidesteps, where the way it moves swings round), the first
/// to its left, the second to its right, a second pair a row further back,
/// as a squad keeps formation. The other side or straight behind when its
/// own place is no straight walk from the leader (a wall, a ledge) or a
/// follower before it stands there; none when no place is.
fn follow_slot(
    level: &crate::Level,
    leader: &MotionState,
    me: SimId,
    followers: &[(SimId, Vec3)],
) -> Option<Vec3> {
    let rank = followers.iter().position(|(id, _)| *id == me).unwrap_or(0);
    let before = &followers[..rank];
    let (ahead, right, _) = math::angle_vectors([0.0, leader.view[1], 0.0]);
    let back = tasks::FOLLOW_BACK * (rank / 2 + 1) as f32;
    let side = if rank % 2 == 0 { -1.0 } else { 1.0 } * tasks::FOLLOW_SIDE;
    [side, -side, 0.0]
        .into_iter()
        .map(|side| {
            let behind = math::mad(leader.origin, -back, ahead);
            math::mad(behind, side, right)
        })
        .find(|spot| {
            before
                .iter()
                .all(|(_, at)| math::flat_distance(*at, *spot) > tasks::FOLLOW_APART)
                && crate::walk::straight(level, leader.origin, *spot)
        })
}

/// Whether a follower at `at` stands within `FOLLOW_APART` of its leader
/// (in its way) or of one before it among `followers` (`follow_slot`): the
/// later of two moves.
fn crowds(me: SimId, at: Vec3, leader: Vec3, followers: &[(SimId, Vec3)]) -> bool {
    let near = |other: Vec3| math::flat_distance(other, at) <= tasks::FOLLOW_APART;
    near(leader)
        || followers
            .iter()
            .take_while(|(id, _)| *id != me)
            .any(|(_, other)| near(*other))
}

/// Puts `cast` into this step's command: a power pressed with its row
/// selected, or a button held on; the next cast waits a while after it, by
/// `dice` (`minds.powers.cast_wait_ms`). Whether there was one.
fn cast_into(
    mind: &mut Mind,
    command: &mut Command,
    cast: Option<Cast>,
    now: u64,
    dice: f32,
) -> bool {
    match cast {
        Some(Cast::Press(row)) => {
            command.buttons |= buttons::FORCE_POWER;
            command.force = row;
            mind.cast_after = now + powers::CAST_WAIT_MS + (dice * powers::CAST_SPREAD_MS) as u64;
        }
        Some(Cast::Hold(button, ms)) => {
            (mind.hold, mind.hold_until) = (button, now + ms);
            mind.cast_after =
                now + ms + powers::HOLD_WAIT_MS + (dice * powers::CAST_SPREAD_MS) as u64;
        }
        None => return false,
    }
    true
}

/// Whether to press the style button toward `style` this step: a single
/// blade in hand that has it, not in it nor queued for it, the button let
/// go last step (one style a press).
fn wants_style(
    sheet: &crate::combat::Sheet,
    fighter: &crate::combat::Fighter,
    style: Style,
) -> bool {
    let kit = Kit::of(sheet);
    let saber = &fighter.saber;
    kit.blade(fighter.hand).is_some_and(|blade| {
        kit.forced_style(fighter.hand).is_none()
            && kit.singles(blade).contains(&style)
            && saber.style != Some(style)
            && saber.queued != Some(style)
            && !saber.cycle_held
    })
}

/// How far past its target a scripted miss is fired (units) [verify: chosen
/// to clear a body; no reference].
const MISS_ASIDE: f32 = rules::minds::scenes::MISS_ASIDE_M / UNIT;
const MISS_UP: f32 = rules::minds::scenes::MISS_UP_M / UNIT;
/// How far above where a door or placeable stands a scene's shot at it is
/// aimed (units).
const OBJECT_AIM_UP: f32 = rules::minds::scenes::OBJECT_AIM_UP_M / UNIT;

/// A scene attacker's command (a passive attack: `CutsceneAttack`'s; a
/// gunner's decided blow, an assured one too): it stands facing its
/// target's chest and starts nothing, as KOTOR's scene attack plays where
/// it stands; a gunner fires each blow decided on `target` as its shots
/// (`Strike::shot_times`: as its clip fires them), the last carrying the
/// blow; a blow without them as one shot, on the step that has its bolt
/// meet the target as the blow falls due. The shot carrying a hit goes at
/// the chest, the rest past the shoulder (`world::fly` lands the blow);
/// with a pistol in each hand, each from the hand its clip fires it from.
fn scene_attack(
    me: SimId,
    ps: &MotionState,
    (target, chest): (SimId, Vec3),
    strikes: &[crate::script::effects::Strike],
    blaster: Option<crate::combat::sheet::Blaster>,
    now: u64,
    msec: u32,
) -> Command {
    let eye = mind::eye(ps);
    let speed = blaster.and_then(|blaster| crate::combat::weapons::primary(blaster).speed);
    let flight_ms = speed.map_or(0, |speed| {
        (math::length(math::sub(chest, eye)) / speed * 1000.0) as u64
    });
    let now_fired = |at: u64| (at..at + u64::from(msec)).contains(&now);
    // The strike fired at now, and whether this shot carries its blow.
    let shot = blaster.and_then(|_| {
        strikes
            .iter()
            .filter(|strike| strike.by.0 == me.0 && strike.target.0 == target.0)
            .find_map(|strike| {
                let count = strike.shots.times().len();
                if count == 0 {
                    return now_fired(strike.due_ms.saturating_sub(flight_ms)).then_some((
                        strike,
                        0,
                        strike.landed,
                    ));
                }
                let index = strike.shot_times().position(now_fired)?;
                Some((strike, index, strike.landed && index + 1 == count))
            })
    });
    let point = match shot {
        Some((strike, index, false)) => {
            let toward = math::sub(chest, eye);
            let flat = (toward[0] * toward[0] + toward[1] * toward[1])
                .sqrt()
                .max(1.0);
            let side = if pick(strike.due_ms, me, 13 + index as u64) < 0.5 {
                -1.0
            } else {
                1.0
            };
            [
                chest[0] - toward[1] / flat * MISS_ASIDE * side,
                chest[1] + toward[0] / flat * MISS_ASIDE * side,
                chest[2] + MISS_UP,
            ]
        }
        _ => chest,
    };
    let (pitch, yaw) = mind::aim(eye, point);
    let mut command = mind::looking(ps, pitch, yaw);
    if let Some((strike, index, _)) = shot {
        command.buttons |= buttons::ATTACK;
        // The hand its clip fires this shot from (`Discharge::hand`): a
        // pair fires the gun the command names (`world::fire`).
        command.weapon = strike.shots.hand(index);
    }
    command
}

/// An engaging melee mind's command (a gunner's is `shoot`'s, a paced blade
/// wielder's `fight`'s): a saber or melee fighter, or one with its fists
/// (`close`), routes to its target when it is farther than
/// `ai::ROUTE_BEYOND` or out of sight a moment ago, and otherwise walks in
/// and swings or punches (`mind::engage`).
///
/// In a scene (`scene`: it or its target takes part in a conversation under
/// way, so a cutscene plays its camera on them, or one holds the world)
/// KOTOR's fighters hold their ground as its combat rounds
/// do, a creature in a round not moving: they close in, then stand and fight
/// where they are, not strafing, flanking or stepping with a swing, so they
/// stay in the shot. While the world is held (`held`) its aim doesn't
/// wander either, so nothing carries it sideways.
#[allow(clippy::too_many_arguments)]
fn engage(
    mind: &mut Mind,
    me: SimId,
    ps: &MotionState,
    target: &Seen,
    close: mind::Attack,
    (now, scene, held): (u64, bool, bool),
    level: &crate::Level,
    doors: &dyn crate::walk::Ways,
) -> Command {
    let apart = math::flat_distance(target.ps.origin, ps.origin);
    let seen_lately = mind
        .last_seen
        .is_some_and(|(_, when)| now < when + tasks::SEEN_LATELY_MS);
    // A scene's fighter within its blow's reach stands where it is, seen
    // lately or not: a route's walker would carry it into its target.
    let standing = scene && apart <= mind::STRIKE;
    if apart > ai::ROUTE_BEYOND || (!seen_lately && !standing) {
        mind.walk_to(target.ps.origin, tasks::REPLAN)
            .command(level, ps, now, doors)
    } else {
        mind.walker = None;
        let at = mind::look_at(&target.ps, close);
        let mut command = mind::engage(me, ps, mind::eye(ps), at, close, now, held);
        if scene {
            // Within its blow's reach it stands, swinging with no step
            // (the swing's movement keys would carry it off): the swing
            // is the one a standing fighter makes. It comes to a stop at
            // `mind::SCENE_REACH`, letting go of its run as far before
            // that as it and its target, coming at it, slide.
            if apart <= mind::STRIKE {
                let mut toward = math::sub(ps.origin, target.ps.origin);
                toward[2] = 0.0;
                math::normalize(&mut toward);
                let coming = math::dot(target.ps.velocity, toward).max(0.0);
                let slides = crate::motion::stopping_distance(math::flat_length(ps.velocity))
                    + crate::motion::stopping_distance(coming);
                command.forward = if apart - mind::SCENE_REACH > slides {
                    127
                } else {
                    0
                };
                command.right = 0;
            }
        } else if apart > ai::FLANK_FROM {
            // Closing in, it bears off to one side (by its id), so several
            // come at a target from its flanks rather than in a line.
            command.right = if me.0.is_multiple_of(2) {
                tasks::FLANK_AXIS
            } else {
                -tasks::FLANK_AXIS
            };
        }
        on_the_mesh(level, ps, command)
    }
}

/// How far ahead of a body (units) its step is checked for floor: past its
/// box's half width, where it would drop.
const LEDGE_AHEAD: f32 = crate::motion::HALF_WIDTH + tasks::LEDGE_MARGIN;

/// `command` without its steps where they would carry the body standing as
/// `ps` off the navigation mesh it stands on: KOTOR's creatures keep to
/// their walkmesh and never step off a walkway's edge (reone's and xoreos's
/// movers stay on walkable faces). Fighting in place, a mind's strafe or
/// swing step does not leave it either: Saul Karath, swinging at the player
/// by the Leviathan bridge's rail, stepped down into the crew pit and fought
/// on from where nothing reached him.
fn on_the_mesh(level: &crate::Level, ps: &MotionState, command: Command) -> Command {
    if (command.forward == 0 && command.right == 0) || !crate::walk::on_mesh(level, ps.origin) {
        return command;
    }
    let yaw = math::short_to_degrees(command.angles[1].wrapping_add(ps.view_offset[1]));
    let (sin, cos) = math::sin_cos_deg(yaw);
    let (forward, right) = (f32::from(command.forward), f32::from(command.right));
    // JA's forward is (cos, sin) of the yaw, its right (sin, −cos).
    let way = [cos * forward + sin * right, sin * forward - cos * right];
    let length = math::flat_length([way[0], way[1], 0.0]);
    let ahead = [
        ps.origin[0] + way[0] / length * LEDGE_AHEAD,
        ps.origin[1] + way[1] / length * LEDGE_AHEAD,
        ps.origin[2],
    ];
    if crate::walk::on_mesh(level, ahead) {
        command
    } else {
        Command {
            forward: 0,
            right: 0,
            ..command
        }
    }
}

/// What a gunner fights with (`shoot`).
#[derive(Clone, Copy)]
struct Gun {
    /// Its weapon's reach (units: KOTOR's `maxattackrange`).
    reach: f32,
    blaster: crate::combat::sheet::Blaster,
    /// A pistol in each hand (`combat::pair`).
    pair: bool,
    ally: bool,
    /// In a scene, or a fight between creatures nobody plays: it holds its
    /// ground.
    still: bool,
}

/// How fast a gunner turns (degrees a second): JA's yaw speed ten times
/// over for a mind with a gun and an enemy, so near
/// enough at once.
const GUN_TURN_RATE: f32 = rules::ai::tempo::GUN_TURN_RATE;
/// How near a spot it made for counts as reached (units: by the walker's
/// arrival, near JA's 16 for a combat point).
const SPOT_REACHED: f32 = rules::ai::gunners::spots::REACHED * crate::walk::REACHED;

/// A gunner's command against `target`, paced as JA's troopers are
/// (`combat::troop`): beyond its weapon's reach it routes in (KOTOR's
/// `maxattackrange`, I22 section 7); within it, it stands, faces its
/// target's chest squarely and fires on its spacing while it sees it. On
/// its think ticks it settles its aim and, every few seconds
/// (`ai.gunners.sight_check_ms`), checks it still sees its target; when it
/// does not it runs for a spot that sees it (`spot_for`), firing on the way
/// once it sees it, and stands there, holding its fire a moment as it
/// settles. JA's troopers also move off a near enemy; KOTOR's gunner holds
/// its ground against a blade
/// instead (one that backed off, Calo Nord at the krayt's cave, could never
/// be caught; the Rakatan guard droid in Dantooine's ruins kept a party in
/// its fight three minutes so), and so does this one. In a scene, or a
/// fight between creatures nobody plays, it holds its ground.
#[allow(clippy::too_many_arguments)]
fn shoot(
    mind: &mut Mind,
    me: SimId,
    ps: &MotionState,
    target: &Seen,
    gun: Gun,
    (now, msec): (u64, u32),
    (level, doors): (&crate::Level, &dyn crate::walk::Ways),
    (visible, others): (&dyn Fn(Vec3, Vec3) -> bool, &[Vec3]),
) -> Command {
    let thinking = (now + u64::from(me.0) * 53) % pace::THINK_MS < u64::from(msec);
    let eye = mind::eye(ps);
    let chest = mind::chest(&target.ps);
    let apart = math::flat_distance(target.ps.origin, ps.origin);
    let mut dice = pace::Dice::new(now, me);
    let troop = &mut mind.troop;
    if thinking {
        troop.rate(target.id.0, gun.ally, &mut dice);
        troop.sees = visible(eye, chest);
        let change = if troop.sees {
            rules::ai::gunners::aim::SETTLE
        } else {
            rules::ai::gunners::aim::LOSE
        };
        troop.adjust_aim(change, now, &mut dice);
        let [least, most] = gunners::SIGHT_CHECK_MS;
        if troop.spot.is_none()
            && !gun.still
            && apart <= gun.reach
            && Troop::due(&mut troop.sight_check_at, now, (least, most), &mut dice)
            && !troop.sees
        {
            troop.spot = spot_for(level, ps, target, (visible, others), doors);
        }
    }
    let step = match mind.troop.spot {
        Some(spot) if math::flat_distance(spot, ps.origin) > SPOT_REACHED => {
            let step = mind
                .walk_to(spot, SPOT_REACHED)
                .command(level, ps, now, doors);
            // No way there: it stands where it is.
            if mind.walker.is_some_and(|walker| walker.waypoint.is_none()) {
                mind.troop.spot = None;
                None
            } else {
                Some(step)
            }
        }
        Some(_) => {
            // There: it settles before it fires (`attackDelay`).
            let troop = &mut mind.troop;
            troop.spot = None;
            let [least, most] = gunners::SETTLE_MS;
            troop.shot_at = troop.shot_at.max(now + dice.ms(least, most));
            None
        }
        None if apart > gun.reach => Some(
            mind.walk_to(target.ps.origin, gun.reach * gunners::REPLAN_SHARE)
                .command(level, ps, now, doors),
        ),
        None => None,
    };
    if step.is_none() {
        mind.walker = None;
    }
    let troop = &mut mind.troop;
    // It faces its target while it sees it, else the way it goes.
    let (pitch, yaw) = match step {
        Some(step) if !troop.sees => (
            0.0,
            math::short_to_degrees(step.angles[1].wrapping_add(ps.view_offset[1])),
        ),
        _ => mind::aim(eye, chest),
    };
    let (pitch, yaw) = mind::turned(ps, (pitch, yaw), GUN_TURN_RATE, msec);
    let (forward, right) = step.map_or((0, 0), |step| crate::walk::rebased(ps, &step, yaw));
    let mut command = Command {
        forward,
        right,
        ..mind::looking(ps, pitch, yaw)
    };
    let walking_in = apart > gun.reach && troop.spot.is_none();
    let refire = crate::combat::weapons::primary(gun.blaster).refire_ms;
    if troop.sees
        && !walking_in
        && mind::turn(ps.view[1], yaw).abs() < mind::FIRE_CONE
        && troop.fire((gun.blaster, gun.pair), refire, now, &mut dice)
    {
        command.buttons |= buttons::ATTACK;
    }
    command
}

/// How far out a gunner looks for a spot (units), in sixteen ways at
/// each.
const SPOT_RINGS: [f32; 6] = rules::ai::gunners::spots::RINGS;

/// Where a gunner at `ps` makes for to fight `target` from
/// (JA's NPCs pick a combat point; KOTOR's areas have no combat points, so spots
/// round it stand in for them): the nearest where a body stands on the
/// navigation mesh, no nearer `target` than `troop::AVOID`, not taken by
/// another (`others`, a body's width and a half), from whose eye `target`
/// is seen and which a way leads to; one `target` cannot see the middle of
/// first (cover), else any.
fn spot_for(
    level: &crate::Level,
    ps: &MotionState,
    target: &Seen,
    (visible, others): (&dyn Fn(Vec3, Vec3) -> bool, &[Vec3]),
    doors: &dyn crate::walk::Ways,
) -> Option<Vec3> {
    let from = ps.origin;
    let spots: Vec<Vec3> = SPOT_RINGS
        .iter()
        .flat_map(|&far| {
            (0..16).map(move |k| {
                let (sin, cos) = math::sin_cos_deg(22.5 * k as f32);
                [from[0] + cos * far, from[1] + sin * far, from[2]]
            })
        })
        .map(|at| crate::walk::on_floor(level, at))
        .filter(|at| crate::walk::stands(level, *at))
        .filter(|at| math::flat_distance(*at, target.ps.origin) >= troop::AVOID)
        .filter(|at| {
            let taken = rules::ai::gunners::spots::TAKEN * 2.0 * crate::motion::HALF_WIDTH;
            others
                .iter()
                .all(|other| math::flat_distance(*other, *at) > taken)
        })
        .collect();
    let clear = |at: &Vec3| {
        visible(
            [at[0], at[1], at[2] + ps.eye_height],
            mind::chest(&target.ps),
        )
    };
    let covered = |at: &Vec3| !visible(mind::eye(&target.ps), *at);
    let routed = |at: &Vec3| crate::walk::route_ways(level, from, *at, doors).is_some();
    spots
        .iter()
        .find(|at| clear(at) && covered(at) && routed(at))
        .or_else(|| spots.iter().find(|at| clear(at) && routed(at)))
        .copied()
}

/// What a blade wielder's mind knows of itself (`fight`).
struct Own {
    saber_move: crate::combat::saber::Move,
    busy: bool,
    pain_until: u64,
    health: f32,
    ally: bool,
}

/// A blade wielder's command against `target`, paced as JA's Jedi minds
/// are (`combat::pace`): it decides on its think ticks (`pace::THINK_MS`,
/// at its own moment), holding its keys between them, and turns its view
/// at `pace::TURN_RATE`. With no straight way to its target
/// (`walk::straight`), or having lost sight of it a moment, it hunts along a route
/// at a run, facing the way it goes; else it faces its target's eyes and
/// advances, holds or backs off by its aggression, the keys the walker's
/// way toward the target turned to its facing (reversed backing off).
#[allow(clippy::too_many_arguments)]
fn fight(
    mind: &mut Mind,
    me: SimId,
    ps: &MotionState,
    own: &Own,
    target: &Seen,
    (now, msec): (u64, u32),
    level: &crate::Level,
    doors: &dyn crate::walk::Ways,
) -> Command {
    let thinking = (now + u64::from(me.0) * 53) % pace::THINK_MS < u64::from(msec);
    if thinking {
        let straight = crate::walk::straight(level, ps.origin, target.ps.origin);
        let seen_lately = mind
            .last_seen
            .is_some_and(|(_, when)| now < when + rules::ai::blades::SEEN_LATELY_MS);
        mind.pace.hunting = !straight || !seen_lately;
        if mind.pace.hunting {
            mind.pace.keys = pace::Keys::default();
        }
    }
    if mind.pace.hunting {
        let step = mind
            .walk_to(target.ps.origin, tasks::REPLAN)
            .command(level, ps, now, doors);
        let looks = math::short_to_degrees(step.angles[1].wrapping_add(ps.view_offset[1]));
        let (pitch, yaw) = mind::turned(ps, (0.0, looks), pace::TURN_RATE, msec);
        let (forward, right) = crate::walk::rebased(ps, &step, yaw);
        return Command {
            forward,
            right,
            ..mind::looking(ps, pitch, yaw)
        };
    }
    let face = mind::aim(mind::eye(ps), mind::eye(&target.ps));
    let (pitch, yaw) = mind::turned(ps, face, pace::TURN_RATE, msec);
    if thinking {
        let sight = sight_of(ps, own, target);
        let clear = |forward: i8, right: i8| crate::walk::way_clear(level, ps, forward, right);
        let mut dice = pace::Dice::new(now, me);
        let way = mind.pace.decide(&sight, now, &mut dice, &clear);
        let (forward, right) = match way {
            Way::Hold => (0, 0),
            Way::Advance | Way::Retreat => {
                let step = mind
                    .walk_to(target.ps.origin, tasks::REPLAN)
                    .command(level, ps, now, doors);
                let (forward, right) = crate::walk::rebased(ps, &step, yaw);
                if way == Way::Retreat {
                    (forward.saturating_neg(), right.saturating_neg())
                } else {
                    (forward, right)
                }
            }
        };
        // JA reads its enemy's side off its own angles taken for a point
        // (QUIRKS.md Q-0160).
        let (_, side, _) = math::angle_vectors([0.0, ps.view[1], 0.0]);
        let from_foe = math::dot(side, math::sub(target.ps.origin, [0.0, ps.view[1], 0.0]));
        let keys = pace::Keys {
            forward,
            right,
            ..pace::Keys::default()
        };
        let turn = mind::turn(face.1, ps.view[1]);
        mind.pace
            .finish(keys, &sight, now, &mut dice, &clear, turn, from_foe);
    }
    let keys = mind.pace.keys;
    let mut command = Command {
        forward: keys.forward,
        right: keys.right,
        ..mind::looking(ps, pitch, yaw)
    };
    if keys.walk {
        walking(&mut command);
    }
    if keys.attack {
        command.buttons |= buttons::ATTACK;
    }
    command
}

/// What a blade wielder at `ps` sees of `target` on a think (`pace::Sight`).
fn sight_of(ps: &MotionState, own: &Own, target: &Seen) -> pace::Sight {
    let speed = |velocity: Vec3| math::length(velocity);
    let unit = |mut v: Vec3| {
        math::normalize(&mut v);
        v
    };
    let lead = math::mad(target.ps.origin, pace::LEAD_MS / 1000.0, target.ps.velocity);
    let toward = math::sub(lead, ps.origin);
    let gap = math::length(toward) - pace::TIP;
    let toward = unit(toward);
    // Its view (`InFOV`).
    let (pitch, yaw) = mind::aim(ps.origin, target.ps.origin);
    let [across, up] = rules::ai::blades::VIEW;
    let in_view = mind::turn(yaw, ps.view[1]).abs() <= across && pitch.abs() <= up;
    let me_to_it = unit(math::sub(target.ps.origin, ps.origin));
    let it_to_me = math::scale(me_to_it, -1.0);
    let closing = if speed(ps.velocity) > 1.0 {
        Some(math::dot(me_to_it, ps.velocity))
    } else if speed(target.ps.velocity) > 1.0 {
        Some(math::dot(it_to_me, target.ps.velocity))
    } else {
        None
    };
    let moving = speed(target.ps.velocity) > 1.0;
    let coming = if moving {
        math::dot(unit(target.ps.velocity), it_to_me)
    } else {
        let (ahead, ..) = math::angle_vectors(target.ps.view);
        math::dot(ahead, it_to_me)
    };
    let (ahead, ..) = math::angle_vectors(ps.view);
    let swing = own.saber_move;
    pace::Sight {
        foe: target.id,
        gap,
        in_view,
        closing,
        arms: target.arms,
        swinging: target.swinging,
        grounded: target.ps.ground.is_some(),
        moving,
        coming,
        facing: math::dot(ahead, toward),
        busy: own.busy,
        ally: own.ally,
        health: own.health,
        saber_move: swing as u8,
        bounced: swing.bounce(),
        broken: swing.broken(),
        parried: swing.parry(),
        pain_until: own.pain_until,
    }
}

/// The round a melee fighter begins on `target` (KOTOR's combat round), if
/// it has none under way (`strikes`: the blows waiting): its attack roll
/// (`damage::odds`, `combat::check`: d20 + attack bonus against the
/// target's defence, a natural 20 always hits and a 1 always misses; a
/// critical as every hit's, `damage::critical`), the hit's damage as every
/// weapon hit's (`damage::weapon_roll`, `damage::weapon_damage`: the target's
/// guards and toughness, sneak attacks aside), and its clips as K1's
/// executable picks them (`combat::rounds`): the swing of its feat (`feat`,
/// a talent its script uses) or of its round's set, the target's answer to
/// it from `combatanimations.2da`, and the blow landing at the swing's
/// `hit1`. It attacks once a round: its next swing a round after its last
/// began (`swung_at`), and half a round after its target's waiting one
/// began, so the two alternate as a duel's attacks do. The dice are
/// the mind's own hash (`pick`), not the sim's stream: minds draw nothing.
/// The executable rerolls a choreographed swing that repeats either of the
/// attacker's last two; this draw does not [verify how often it shows].
fn begin_round<'a>(
    me: SimId,
    ((sheet, beast), feat, swung_at): (
        (&crate::combat::sheet::Sheet, bool),
        Option<u16>,
        Option<u64>,
    ),
    target: &Seen,
    strikes: impl Iterator<Item = &'a crate::script::effects::Strike> + Clone,
    (now, game): (u64, crate::combat::passive::Game),
    table: &crate::combat::rounds::Table,
) -> Option<crate::script::effects::Strike> {
    use crate::combat::damage;
    use crate::combat::rounds::{Answer, HALF_ROUND_MS, NONE, Side};
    use crate::script::effects::Strike;
    let (by, at) = (ObjectId(me.0), ObjectId(target.id.0));
    if strikes.clone().any(|strike| strike.by == by) {
        return None;
    }
    let half = u64::from(HALF_ROUND_MS);
    let answering = strikes
        .filter(|strike| strike.by == at && strike.target == by && strike.animation != 0)
        .map(|strike| strike.starts_ms() + half)
        .max();
    let begins = [Some(now), answering, swung_at.map(|at| at + 2 * half)]
        .into_iter()
        .flatten()
        .max()
        .unwrap_or(now);
    let weapon = crate::combat::forms::weapon(sheet, sheet.main_weapon());
    let defending = crate::combat::forms::defending(&target.sheet, target.target == Some(me));
    let die = |salt: u64, sides: u8| {
        1 + ((pick(now, me, salt) * f32::from(sides)) as i64).min(i64::from(sides) - 1)
    };
    let mut salt = 30;
    let mut next = |sides: u8| {
        salt += 1;
        die(salt, sides)
    };
    let odds = damage::odds(sheet, &weapon, &defending);
    let roll = die(21, 20);
    // An assured hit (`EffectAssuredHit`) lands whatever the roll.
    let assured = sheet.flags & crate::combat::sheet::flags::ASSURED_HIT != 0;
    let landed = assured || crate::combat::check(roll as i32, odds.0, odds.1);
    let confirm = damage::confirming(sheet, odds);
    let critical = landed
        && damage::critical(roll, &weapon, &defending, Some(confirm), &mut || {
            die(22, 20)
        });
    let rolled = damage::weapon_roll(&weapon, Some(sheet), &mut next);
    let (hurt, _) = damage::weapon_damage(
        &weapon,
        Some(sheet),
        &target.sheet,
        rolled,
        critical,
        0,
        game,
        &mut next,
    );
    let damage = i32::try_from(hurt).unwrap_or(i32::MAX);
    let sides = (
        Side {
            sheet,
            creature: beast,
        },
        Side {
            sheet: &target.sheet,
            creature: target.beast,
        },
    );
    let swing = table.swing(sides, feat, |count| die(23, count) as u8)?;
    Some(Strike {
        target: at,
        by,
        amount: crate::script::effects::amount(damage),
        due_ms: begins + u64::from(table.hit_ms(swing).unwrap_or(HALF_ROUND_MS)),
        assured: false,
        landed,
        animation: swing,
        answer: table
            .answer(swing, Answer::of_round(landed, sides.1), sides.1)
            .unwrap_or(NONE),
        hit_ms: table.hit_ms(swing).unwrap_or(HALF_ROUND_MS),
        began: false,
        shots: Default::default(),
    })
}

#[cfg(feature = "probes")]
mod probes;
#[cfg(feature = "probes")]
pub use probes::probes;
