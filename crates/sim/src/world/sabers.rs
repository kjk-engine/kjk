//! The step's blades (`docs/COMBAT.md`), after
//! the moves and the shots, before the bolts fly: what the server does
//! with every saber each frame, once per tick.
//!
//! 1. **Pose** (everywhere): each character's copy of the server's skeleton
//!    follows its animations (`blades::Posed`), frozen on its frame in a
//!    lock.
//! 2. **Place** (everywhere): the blades of a lit weapon in hand, when
//!    anyone else or a bolt is near enough to matter (`NEAR`, `BOLT_NEAR`;
//!    the rest keep their last place, which goes stale), from the pose;
//!    each with where it was a tick ago (its trail, if fresh).
//! 3. **Swings** (everywhere): a new move starts a new swing; what the old
//!    one still owed is delivered (on the authority).
//! 4. On the authority and in replays only (I22 C20: prediction shows no
//!    hits, blocks, clashes or locks):
//!    - **Grace** (C19, `saber::grace`): the hits held for remote players
//!      become blocks, or land;
//!    - **Locks** (`saber::lock`): presses, advances, breaks;
//!    - **Sweeps**: each attacking blade's sweep (`saber::sweep`) tested
//!      against the others' blades (a clash, `saber::clash`, with a lock
//!      tried first) and traced through the bodies it may harm; a body its
//!      holder turns by state (C6, `saber::defence`) blocks it (a clash
//!      from the tree's third branch, a lock tried sometimes:
//!      `rules::saber_locks::start`). A clash or block happens only on KOTOR's
//!      chance to miss, drawn once a swing a body (`Step::turned`); any
//!      other becomes a contact the swing
//!      owes, delivered as one hit once the blade leaves it (C5), held
//!      first for a remote player who could block it (C19). A player's
//!      sweep meets the others' blades, guards and bodies as it saw them
//!      (C18, `History`);
//!    - **Kicks** (`saber::kick`).
//! 5. **Throws** (`saber::throw`): the flight everywhere (the owner's
//!    prediction flies its own saber), hits and blocks on the authority.
//! 6. The blades' trails, the tick's saber boxes for `fly` (`Blades`), and
//!    on the authority everyone's blades and guard for the history.
//!
//! Every roll is the server's (the `authority` stream), in id order.
//!
//! `seen_cuts` is the sweep (`sweeping`, the segments, `cut`) run by a
//! client over its own predictions and the bodies it draws: where its
//! contact sparks go (I22 C20).

use bevy_ecs::prelude::*;
use collision::{Body, Contents, Extents, UNIT};

use super::history::{Fence, History, Stand};
use super::{
    Avatar, BODY, Bolt, Clock, Fighting, Hit, Hits, Id, Inputs, Object, Player, PlayerSheet,
    Static, StepReason, Streams, Targets, door_states, faction_of, held, in_hand, in_party,
    sheet_of,
};
use crate::combat::damage::{self, FriendlyFire, Source};
use crate::combat::fighter::{Event, Location, Outcome};
use crate::combat::saber::blades::{self, Slot, Sweep};
use crate::combat::saber::clash::{self, Fencer, roll_between};
use crate::combat::saber::defence::{self, Against, BoltFate, Guard};
use crate::combat::saber::grace::{self, Held};
use crate::combat::saber::lock::{self, Lock, Stance};
use crate::combat::saber::sweep::{self, Swing};
use crate::combat::saber::throw::{self, Flight, Thrown};
use crate::combat::saber::{Blade, Blocked, Hand, Kit, Move, Style, kick, power};
use crate::combat::sheet::{Ability, Delivery, flags, type_rows};
use crate::combat::{Fighter, Sheet, mind};
use crate::input::buttons;
use crate::math::{self, Vec3, scale};
use crate::motion::{self, Anim, MotionState};
use crate::rng::SimRng;
use crate::snapshot::SimId;
use rules::saber_locks::contest;
use rules::saber_locks::start;
use rules::saber_moves::kicks;
use rules::saber_throw::down;
use rules::sabers::smash;

/// A blade is placed when someone else is this near (units): two blades'
/// reach and a step's travel.
const NEAR: f32 = rules::sabers::blades::NEAR;
/// …or a bolt, or a thrown saber (units).
const BOLT_NEAR: f32 = rules::sabers::blades::BOLT_NEAR;
/// A saber entity's entity number in bolt traces: its holder's body's with
/// this bit too.
pub(super) const SABER: u32 = 1 << 30;
/// How far a thrown saber's steering looks along its owner's view (units):
/// beyond any level's reach.
const VIEW_REACH: f32 = 4096.0;

/// The tick's saber boxes (units), by holder: what bolts clip against
/// (rulebook 5.6), for `fly`.
#[derive(Resource, Default)]
pub(super) struct Blades(pub Vec<(SimId, Vec3, Vec3)>);

impl Blades {
    /// The saber boxes as bodies (metres), all but `owner`'s.
    pub(super) fn bodies(&self, owner: SimId) -> impl Iterator<Item = Body> + '_ {
        self.0
            .iter()
            .filter(move |(id, ..)| *id != owner)
            .map(|(id, min, max)| Body {
                entity: id.0 | BODY | SABER,
                min: scale(*min, UNIT),
                max: scale(*max, UNIT),
                contents: Contents::BODY,
            })
    }
}

/// A character as the blades see it, copied out for the step and written
/// back at its end.
struct Man {
    id: SimId,
    avatar: bool,
    /// A player's view lag (ms): its blades cut the others as it saw them,
    /// and blows on it are held for its grace; and whether its seat sent a
    /// command this tick.
    lag_ms: u16,
    commanded: bool,
    party: bool,
    /// It plays by KOTOR's rules (`scene::Held::scene`): no lock or kick
    /// takes it, as KOTOR has neither.
    scene: bool,
    faction: u8,
    sheet: Sheet,
    kit: Kit,
    ps: MotionState,
    f: Fighter,
}

impl Man {
    fn blade(&self) -> Option<Blade> {
        self.kit.blade(self.f.hand)
    }

    /// A lightsaber lit in hand (not thrown or knocked away).
    fn saber_out(&self) -> bool {
        self.f.alive() && self.f.saber.on && self.f.saber.thrown.is_none() && self.blade().is_some()
    }

    fn style(&self) -> Style {
        self.f.saber.style.unwrap_or(Style::Medium)
    }

    fn offence(&self) -> i32 {
        self.blade().map_or(0, |blade| {
            i32::from(self.kit.offence(self.style(), blade, self.f.hand))
        })
    }

    fn defence(&self) -> u8 {
        self.blade().map_or(0, |blade| self.kit.defence(blade))
    }

    /// A player standing still, pressing nothing.
    fn player_at_rest(&self) -> bool {
        let c = &self.f.last_command;
        self.avatar && c.forward == 0 && c.right == 0 && c.up == 0 && c.buttons == 0
    }

    fn guard(&self, level: u8, now: u64) -> Guard<'_> {
        Guard {
            saber: &self.f.saber,
            ps: &self.ps,
            weapon_wait_ms: self.f.weapon_wait_ms,
            level,
            attack_held: self.f.last_command.buttons & buttons::ATTACK != 0,
            hand_out: self.f.force.hand_out(now),
        }
    }

    /// As a lock's checks read it.
    fn stance(&self, now: u64) -> Stance<'_> {
        Stance {
            ps: &self.ps,
            player: self.avatar,
            blocking: self.f.saber.blocking,
            weapon_wait_ms: self.f.weapon_wait_ms,
            busy: self.f.saber.floored()
                || self.f.force.hand_out(now)
                || motion::is_leap(self.ps.legs.anim)
                || motion::roll_playing(&self.ps, self.ps.legs.anim),
        }
    }

    /// The weapon in hand as a block names it.
    fn delivery(&self) -> Delivery {
        in_hand(&self.sheet, &self.f, self.f.hand).delivery
    }

    /// A block by `by` (its `weapon` turned) at `at` (units).
    fn record(&mut self, by: SimId, result: Outcome, tick: u64, weapon: Delivery, at: Vec3) {
        self.f.record(Event {
            sequence: 0,
            tick,
            by: Some(by),
            result,
            location: Location::None,
            damage: 0,
            weapon: Some(weapon),
            at: Some(Event::units(at)),
            visual: None,
        });
    }
}

/// The step's view of the level and everyone.
struct Step<'a> {
    level: &'a crate::Level,
    /// How the factions stand (none without a module).
    standing: Option<crate::module::Standing<'a>>,
    states: Option<Vec<u8>>,
    men: Vec<Man>,
    now: u64,
    tick: u64,
    msec: u32,
    rng: &'a mut SimRng,
    hits: &'a mut Vec<Hit>,
    /// The tick's blades, by character (step 2).
    placed: Vec<[Option<Sweep>; 2]>,
    /// The bodies as they stand after the moves (`Targets`).
    bodies: Option<Targets>,
    /// What the party's blades may bash (`objects`): each door's or
    /// placeable's number among the level's entities, and its id.
    bashable: Vec<(u32, SimId)>,
    /// The pick volumes (metres) of the placeables among them: what a
    /// blade meets of one with no brushes of its own (a locker, a
    /// footlocker), as a fist or a bolt does.
    bashable_bodies: Vec<Body>,
    history: &'a History,
}

/// The blades system (see the module documentation).
#[allow(clippy::type_complexity, clippy::too_many_arguments)]
pub(super) fn sabers(
    clock: Res<Clock>,
    level: Res<Static>,
    inputs: Res<Inputs>,
    mut history: ResMut<History>,
    mut streams: ResMut<Streams>,
    mut hits: ResMut<Hits>,
    mut cache: ResMut<Blades>,
    scripts: Res<super::scripts::Scripts>,
    (scene, talking): (Res<super::scene::Held>, Res<super::Talking>),
    objects: Query<&Object>,
    placed: Query<(&Id, &Object)>,
    bolts: Query<&Bolt>,
    mut actors: Query<(
        &Id,
        Option<&Avatar>,
        Option<&Object>,
        Option<&PlayerSheet>,
        &mut Player,
        &mut Fighting,
    )>,
) {
    let authority = clock.reason != StepReason::Predict;
    let now = clock.time_ms;
    let mut rows: Vec<_> = actors.iter_mut().collect();
    rows.sort_by_key(|(id, ..)| id.0);
    let men: Vec<Man> = rows
        .iter()
        .map(|(id, avatar, object, sheet, player, fighting)| {
            let sheet = sheet_of(*sheet, *object);
            let mut kit = Kit::of(&sheet);
            if let (Some(throw), Some(module)) = (kit.throw.as_mut(), level.0.module.as_ref())
                && let Some(cost) = module.force_costs.get(usize::from(throw.row))
            {
                throw.cost = i32::from(*cost) * 100;
            }
            Man {
                id: id.0,
                avatar: avatar.is_some(),
                lag_ms: avatar.map_or(0, |avatar| inputs.0.view_lag(avatar.0)),
                commanded: avatar.is_some_and(|avatar| inputs.0.get(avatar.0).is_some()),
                party: in_party(*avatar, &fighting.0),
                scene: scene.scene(&talking.0, id.0),
                faction: faction_of(&fighting.0, &sheet),
                sheet,
                kit,
                ps: player.0,
                f: fighting.0,
            }
        })
        .collect();
    let mut near: Vec<Vec3> = bolts.iter().map(|bolt| bolt.0.at(now)).collect();
    near.extend(
        men.iter()
            .filter_map(|man| man.f.saber.thrown.filter(Thrown::flying).map(|t| t.at)),
    );
    // What may be bashed draws blades too (`Step::bash`).
    near.extend(
        super::objects::bodies(placed.iter())
            .iter()
            .map(super::objects::middle),
    );
    let mut step = Step {
        level: &level.0,
        standing: (level.0.module.as_ref())
            .map(|module| module.repute.with(&scripts.state.reputation)),
        states: door_states(&level.0, &objects),
        men,
        now,
        tick: clock.tick,
        msec: clock.msec,
        rng: &mut streams.authority,
        hits: &mut hits.0,
        placed: Vec::new(),
        bodies: None,
        bashable: super::objects::bashable(placed.iter()),
        bashable_bodies: super::objects::bodies(
            placed
                .iter()
                .filter(|(_, object)| object.kind == crate::module::ObjectKind::Placeable),
        ),
        history: &history,
    };
    step.pose();
    step.placed = step.place(&near);
    step.bodies = Some(Targets::of(
        step.level.friendly_fire(),
        step.men
            .iter()
            .map(|man| (man.id, &man.ps, man.f.alive(), man.party)),
    ));
    step.swings(authority);
    if authority {
        step.graces();
        step.locks();
        step.sweeps();
        step.kicks();
        step.deliver();
    }
    step.throws(authority);
    cache.0 = step.boxes();
    let fences = if authority { step.fences() } else { Vec::new() };
    step.trails();
    for (row, man) in rows.iter_mut().zip(step.men) {
        row.4.0 = man.ps;
        row.5.0 = man.f;
    }
    for (id, fence) in fences {
        history.fence(clock.tick, id, fence);
    }
}

impl Step<'_> {
    fn find(&self, id: SimId) -> Option<usize> {
        self.men.binary_search_by_key(&id, |man| man.id).ok()
    }

    /// Whether `a` and `b` may fight blade to blade and blade to body: not
    /// one side (the party, or friendly factions) unless friendly fire is
    /// on (JA: no clashes between teammates without friendly sabers).
    fn foes(&self, a: usize, b: usize) -> bool {
        let side = |man: &Man| crate::module::Side {
            id: man.id.0,
            faction: man.faction,
            party: man.party,
        };
        let relation =
            crate::module::relation(self.standing, side(&self.men[a]), side(&self.men[b]));
        !self.level.friendly_fire().spares(relation)
    }

    /// Two players lock only in a duel: with friendly fire full (a
    /// delegated decision standing for JA's duel). No one in a scene locks.
    fn may_lock(&self, a: usize, b: usize) -> bool {
        let (a, b) = (&self.men[a], &self.men[b]);
        !(a.scene || b.scene)
            && (!(a.avatar && b.avatar) || self.level.friendly_fire() == FriendlyFire::Full)
    }

    fn trace(&self, from: Vec3, to: Vec3, half: f32, mask: Contents) -> collision::Trace {
        let h = half * UNIT;
        let extents = Extents {
            mins: [-h; 3],
            maxs: [h; 3],
        };
        self.level.with(self.states.as_deref(), |world| {
            world.trace(scale(from, UNIT), scale(to, UNIT), &extents, mask)
        })
    }

    /// `cut` through the level as this step has it.
    fn cut(
        &self,
        bodies: &[Body],
        segments: &[sweep::Segment],
        half: f32,
    ) -> Vec<(SimId, f32, Vec3, Vec3)> {
        let states = self.states.as_deref();
        cut(bodies, segments, half, |from, to| {
            walled(self.level, states, half, from, to)
        })
    }

    /// Step 1: Each skeleton follows its animations, or its lock's frame.
    fn pose(&mut self) {
        let anims = &self.level.anims;
        if anims.skeleton().is_none() {
            return;
        }
        for man in &mut self.men {
            if !man.f.alive() {
                continue;
            }
            match man.f.saber.lock {
                Some(lock) if man.f.saber.locked(self.now) => {
                    man.f.pose.freeze(lock.anim, lock.frame, anims, self.now);
                }
                _ => {
                    let speed = motion::play_speed(man.f.saber.style, man.ps.torso.anim);
                    man.f.pose.follow(&man.ps, speed, anims, self.now);
                }
            }
        }
    }

    /// Step 2: The blades of everyone with someone or something near.
    fn place(&self, near: &[Vec3]) -> Vec<[Option<Sweep>; 2]> {
        let Some(data) = self.level.anims.skeleton() else {
            return vec![[None; 2]; self.men.len()];
        };
        let close = |a: Vec3, b: Vec3, reach: f32| {
            let d = math::sub(a, b);
            math::dot(d, d) <= reach * reach
        };
        self.men
            .iter()
            .map(|man| {
                let slots = blades::slots(&man.kit, &man.f.saber, man.f.hand);
                if !man.f.alive() || slots.iter().all(Option::is_none) {
                    return [None; 2];
                }
                let someone = self.men.iter().any(|other| {
                    other.id != man.id
                        && other.f.alive()
                        && close(other.ps.origin, man.ps.origin, NEAR)
                });
                let something = near.iter().any(|at| close(*at, man.ps.origin, BOLT_NEAR));
                if !someone && !something {
                    return [None; 2];
                }
                let saber = (&man.f.saber, man.f.alive());
                let Some((pose, legs_yaw)) = man.f.pose.pose(data, &man.ps, saber, self.now) else {
                    return [None; 2];
                };
                let placement = blades::placement(&man.ps, self.msec, legs_yaw);
                let sweep = |i: usize, slot: &Slot| {
                    let now = blades::place(data, &pose, self.now, &placement, slot)?;
                    Some(Sweep {
                        before: blades::fresh(man.f.trails[i], self.now),
                        now,
                        length: math::length(math::sub(now.tip, now.muzzle)),
                    })
                };
                std::array::from_fn(|i| slots[i].as_ref().and_then(|slot| sweep(i, slot)))
            })
            .collect()
    }

    /// Step 3: A new move starts a new swing (an attack started over too); what
    /// the old one owed is delivered.
    fn swings(&mut self, authority: bool) {
        let mut due = Vec::new();
        for man in &mut self.men {
            if man.blade().is_none() {
                continue;
            }
            let mv = man.f.saber.current_move;
            let flip = mv.attack() && man.ps.torso.flip;
            if man.f.swing.is(mv, flip) {
                continue;
            }
            if authority {
                due.extend(owed(man, self.now, true));
            }
            let hold_ms = man.f.weapon_wait_ms.max(1) as u32;
            man.f.swing = Swing::begin(&man.f.swing, mv, flip, hold_ms, self.now);
        }
        for (by, delivery, special) in due {
            self.strike(by, delivery, special);
        }
    }

    /// A delivery as a hit of the holder's weapon, held first for a
    /// target with grace (C19) unless its move is a special (unblockable).
    fn strike(&mut self, by: SimId, delivery: sweep::Delivery, special: bool) {
        let Some(at) = self.find(by) else {
            return;
        };
        let man = &self.men[at];
        let blow = Held {
            by,
            tick: self.tick,
            wait: 0,
            hand: man.f.hand,
            style: man.style(),
            share: delivery.share,
            cadence_ms: delivery.cadence_ms,
            point: delivery.point,
            direction: delivery.direction,
        };
        let graced = (!special)
            .then(|| self.find(delivery.victim))
            .flatten()
            .filter(|&d| self.graced(d));
        match graced {
            Some(d) => {
                let wait = grace::wait(self.men[d].lag_ms, self.msec);
                let blow = Held { wait, ..blow };
                if let Some(oldest) = grace::hold(&mut self.men[d].f.grace, blow) {
                    self.land(delivery.victim, oldest);
                }
            }
            None => self.land(delivery.victim, blow),
        }
    }

    /// Whether blows on `d` are held for its grace: a remote player (its
    /// view lags) with a blade that could block them.
    fn graced(&self, d: usize) -> bool {
        let man = &self.men[d];
        man.avatar && grace::wait(man.lag_ms, self.msec) > 0 && man.saber_out() && man.defence() > 0
    }

    /// A blow landing on `victim` as a hit of its striker's weapon, in the
    /// hand and style it struck with (none if the striker is gone).
    fn land(&mut self, victim: SimId, blow: Held) {
        let Some(at) = self.find(blow.by) else {
            return;
        };
        let man = &self.men[at];
        self.hits.push(Hit {
            attacker: blow.by,
            target: victim,
            source: Source::Weapon {
                weapon: man
                    .kit
                    .swing(blow.style, in_hand(&man.sheet, &man.f, blow.hand)),
                cadence_ms: blow.cadence_ms,
                share: blow.share,
                area: false,
            },
            point: Some(blow.point),
            direction: Some(blow.direction),
        });
    }

    /// Step 4: The held blows (C19): one that this tick's command, made
    /// while its player saw the world before the blow, leaves blocking it
    /// by state is blocked (as the sweep's state block); one whose hold is
    /// over lands.
    fn graces(&mut self) {
        for d in 0..self.men.len() {
            for slot in 0..grace::HELD {
                let Some(blow) = self.men[d].f.grace[slot] else {
                    continue;
                };
                let man = &self.men[d];
                let saw =
                    man.commanded && grace::saw_before(self.tick, man.lag_ms, self.msec, blow.tick);
                // Judged from where the attacker stands (QUIRKS.md Q-0084).
                let attacker = self.find(blow.by);
                let from = attacker.map_or(blow.point, |a| self.men[a].ps.origin);
                let blocking = saw && self.blocks(d, from, Against::Blade);
                if !blocking && self.tick < blow.until() {
                    continue;
                }
                self.men[d].f.grace[slot] = None;
                // A blow its block would turn is turned on the chance to
                // miss (`turned`), drawn the once: either way the hold ends.
                match (blocking, attacker) {
                    (true, Some(a)) if self.misses(a, d) => self.state_block(a, d, blow.point),
                    (true, None) => {}
                    _ => self.land(self.men[d].id, blow),
                }
            }
        }
    }

    /// End of the tick: what every swing owes for contacts the blade left.
    fn deliver(&mut self) {
        let mut due = Vec::new();
        for man in &mut self.men {
            due.extend(owed(man, self.now, false));
        }
        for (by, delivery, special) in due {
            self.strike(by, delivery, special);
        }
    }

    /// The tick `a` saw the others at: a player's view, rewound within
    /// C18's bound (`History::view_tick`); none for a creature, or a player
    /// on the host, who meet the others as they stand.
    fn view(&self, a: usize) -> Option<u64> {
        let me = &self.men[a];
        me.avatar
            .then(|| self.history.view_tick(self.tick, me.lag_ms, self.msec))
            .flatten()
    }

    /// What `d` was at `view` for an attacker who saw it then: its fence
    /// that tick, if the history kept one.
    fn seen(&self, d: usize, view: Option<u64>) -> Option<&Fence> {
        view.and_then(|tick| self.history.fence_at(tick, self.men[d].id))
    }

    /// The bodies (metres) `a` may cut, near enough for its blades: a
    /// player's as it saw them (C18, one world per attacker).
    fn targets(&self, a: usize, reach: f32) -> Vec<Body> {
        let me = &self.men[a];
        let view = self.view(a);
        let rewound = view.map(|tick| self.history.targets_at(tick, self.level.friendly_fire()));
        let Some(bodies) = rewound.as_ref().or(self.bodies.as_ref()) else {
            return Vec::new();
        };
        bodies
            .met_by(me.id)
            .iter()
            .filter(|body| body.entity != me.id.0 | BODY)
            .filter(|body| {
                let centre: Vec3 =
                    std::array::from_fn(|i| (body.min[i] + body.max[i]) * 0.5 / UNIT);
                let d = math::sub(centre, me.ps.origin);
                math::dot(d, d) <= reach * reach
            })
            .copied()
            .collect()
    }

    /// Step 4: The sweeps (see the module documentation).
    fn sweeps(&mut self) {
        let anims = &self.level.anims;
        for a in 0..self.men.len() {
            for i in 0..2 {
                let Some(blade) = self.placed[a][i] else {
                    continue;
                };
                let man = &self.men[a];
                if man.f.saber.locked(self.now) {
                    break;
                }
                let torso = man.ps.torso;
                let Some((strength, arcs)) = sweeping(anims, &man.ps, &man.f) else {
                    continue;
                };
                let (b0, d0) = blade.start();
                let (b1, d1) = (blade.now.muzzle, blade.now.direction);
                let view = self.view(a);
                // Blade against blade (a super-break's win cuts through), unless
                // the blow gets past it (`turned`).
                if !power::final_break_won(torso.anim)
                    && let Some((d, point)) = self.meets(a, &blade, view)
                    && self.turned(a, d, point)
                {
                    self.clash(a, d, point, true);
                    continue;
                }
                let segments = sweep::segments(b0, d0, b1, d1, blade.length, arcs);
                if segments.is_empty() {
                    continue;
                }
                self.strike_thrown(a, &segments, strength);
                let half = sweep_half(&self.men[a].f);
                if self.men[a].party {
                    self.bash(a, &segments, half);
                }
                let bodies = self.targets(a, blade.length + 64.0 + motion::HALF_WIDTH * 2.0);
                if bodies.is_empty() {
                    continue;
                }
                let cuts = self.cut(&bodies, &segments, half);
                self.contacts(a, &cuts, view);
            }
        }
    }

    /// A party member's blade through a door or placeable that may be
    /// bashed (`objects`), JA's way: each segment meeting its collision is
    /// a contact of the swing, worth as a cut through a body is; a
    /// placeable's pick volume where it meets that first, before any wall
    /// (a locker or a footlocker has no brushes: a fist and a bolt meet
    /// that volume too).
    fn bash(&mut self, a: usize, segments: &[sweep::Segment], half: f32) {
        if self.bashable.is_empty() {
            return;
        }
        for segment in segments {
            let met = self.trace(segment.from, segment.to, half, Contents::SOLID);
            let struck = met
                .entity
                .filter(|_| met.fraction < 1.0)
                .and_then(|entity| {
                    (self.bashable.iter()).find_map(|&(at, id)| (at == entity).then_some(id))
                })
                .map(|id| (id, met.fraction));
            let picked = || {
                (self.bashable_bodies.iter())
                    .filter_map(|body| {
                        let fraction = sweep::enters(segment, body, half)?;
                        (fraction < met.fraction).then_some((SimId(body.entity & !BODY), fraction))
                    })
                    .min_by(|x, y| x.1.total_cmp(&y.1))
            };
            if let Some((object, fraction)) = struck.or_else(picked) {
                let direction = blades::toward(segment.from, segment.to).unwrap_or([0.0; 3]);
                let (worth, point) = (segment.worth(fraction), segment.at(fraction));
                self.men[a]
                    .f
                    .swing
                    .touch(object, worth, point, direction, self.now);
            }
        }
    }

    /// The first other blade `a`'s sweep meets, by id, and where: each as
    /// `a` saw it at `view` (C18), where the history kept it then.
    fn meets(&self, a: usize, blade: &Sweep, view: Option<u64>) -> Option<(usize, Vec3)> {
        let (b0, d0) = blade.start();
        let mine = sweep::sweep_quad(b0, d0, blade.now.muzzle, blade.now.direction, blade.length);
        (0..self.men.len())
            .filter(|&d| d != a && self.foes(a, d))
            .find_map(|d| {
                let (blades, solid) = self
                    .seen(d, view)
                    .map_or((&self.placed[d], self.solid(d)), |fence| {
                        (&fence.blades, fence.solid)
                    });
                if !solid {
                    return None;
                }
                blades.iter().flatten().find_map(|theirs| {
                    let (c0, e0) = theirs.start();
                    let quad = sweep::sweep_quad(
                        c0,
                        e0,
                        theirs.now.muzzle,
                        theirs.now.direction,
                        theirs.length,
                    );
                    sweep::quads_meet(&mine, &quad).then(|| (d, midpoint(&blade.now, &theirs.now)))
                })
            })
    }

    /// A swinging blade meeting another's flying saber (rulebook 10.6):
    /// knocked out of the air, along the striker's facing, when the striker
    /// is in an extra-defence move (the spins, the katas, the pair's jump
    /// attack) or the stroke would deal JA more than `smash::THRESHOLD`
    /// (SP-style damage, the tick's travel taken over JA's frame).
    fn strike_thrown(&mut self, a: usize, segments: &[sweep::Segment], strength: i32) {
        let mv = self.men[a].f.saber.current_move;
        let extra = mv.kata()
            || matches!(
                mv,
                Move::SPINATTACK | Move::SPINATTACK_DUAL | Move::JUMPATTACK_DUAL
            );
        let frame = power::JA_FRAME_MS as f32 / self.msec.max(1) as f32;
        for d in 0..self.men.len() {
            if d == a || !self.foes(a, d) {
                continue;
            }
            let Some(t) = self.men[d].f.saber.thrown.filter(Thrown::flying) else {
                continue;
            };
            let struck = segments.iter().find(|segment| {
                sweep::distance_to_segment(t.at, segment.from, segment.to)
                    <= throw::HALF_SIZE + smash::REACH
            });
            let Some(segment) = struck else {
                continue;
            };
            let ja = (smash::PER_POWER
                * strength as f32
                * smash::OUTSIDE_DUEL
                * segment.length
                * frame
                * smash::SCALE
                * smash::SHARE)
                .ceil();
            if extra || ja > smash::THRESHOLD {
                let (forward, ..) = math::angle_vectors([0.0, self.men[a].ps.view[1], 0.0]);
                let fall = math::scale(forward, throw::KNOCKED_SPEED);
                self.men[d].f.saber.thrown = Some(Thrown::knocked(t.at, fall, self.now));
            }
        }
    }

    /// Whether `d`'s blade blocks others (rulebook 5.6): a creature's
    /// always, a player's with offence; not in a broken parry or a lost
    /// super-break.
    fn solid(&self, d: usize) -> bool {
        let man = &self.men[d];
        man.saber_out()
            && (!man.avatar || man.offence() > 0)
            && !man.f.saber.current_move.broken()
            && !power::final_break_lost(man.ps.torso.anim)
    }

    /// What `a`'s cuts do: a body that blocks by state (C6) as `a` saw it
    /// at `view`, its arc judged from where `a` stands (QUIRKS.md Q-0084),
    /// blocks; the rest are contacts of the swing. A stab down cuts only
    /// those lying.
    fn contacts(&mut self, a: usize, cuts: &[(SimId, f32, Vec3, Vec3)], view: Option<u64>) {
        let mut seen: Vec<SimId> = Vec::new();
        for &(victim, _, point, direction) in cuts {
            if seen.contains(&victim) {
                continue;
            }
            seen.push(victim);
            let worth: f32 = cuts
                .iter()
                .filter(|cut| cut.0 == victim)
                .map(|cut| cut.1)
                .sum();
            let Some(d) = self.find(victim) else {
                continue;
            };
            let mv = self.men[a].f.saber.current_move;
            if matches!(
                mv,
                Move::STABDOWN | Move::STABDOWN_STAFF | Move::STABDOWN_DUAL
            ) && !self.men[d].f.saber.floored()
            {
                continue;
            }
            let from = self.men[a].ps.origin;
            // A blow assured (`EffectAssuredHit`, a scene's killing blow)
            // is not turned.
            let assured = self.men[a]
                .f
                .effects
                .sum(crate::combat::effects::Modifies::AssuredHit, self.now)
                > 0;
            if !mv.special()
                && !assured
                && self.turns(d, from, view)
                && self.foes(a, d)
                && self.turned(a, d, point)
            {
                self.state_block(a, d, point);
                continue;
            }
            self.men[a]
                .f
                .swing
                .touch(victim, worth, point, direction, self.now);
        }
    }

    /// Whether `d`'s blade, which would turn `a`'s blow (by state or by
    /// meeting it) at `point`, does: on KOTOR's chance to miss
    /// (`damage::chance`), drawn once a swing a body (the owner's balance of
    /// 2026-10-15; JA's state block and clash turn every such blow). The
    /// swing keeps the verdict (`Swing::verdict`).
    fn turned(&mut self, a: usize, d: usize, point: Vec3) -> bool {
        let victim = self.men[d].id;
        if let Some(turned) = self.men[a].f.swing.verdict(victim) {
            return turned;
        }
        let turned = self.misses(a, d);
        let swing = &mut self.men[a].f.swing;
        if turned {
            swing.turn(victim, point, self.now);
        } else {
            swing.pass(victim, point, self.now);
        }
        turned
    }

    /// A draw of KOTOR's chance that `a`'s blow, with what it swings, misses
    /// `d` (`damage::chance`).
    fn misses(&mut self, a: usize, d: usize) -> bool {
        let (att, def) = (&self.men[a], &self.men[d]);
        let weapon = att
            .kit
            .swing(att.style(), in_hand(&att.sheet, &att.f, att.f.hand));
        let defending = crate::combat::forms::defending(&def.sheet, def.f.target == Some(att.id));
        i64::from(self.rng.below(1000)) >= damage::chance(&att.sheet, &weapon, &defending)
    }

    /// Whether `d` blocks at `point` by state (C6).
    fn blocks(&self, d: usize, point: Vec3, against: Against) -> bool {
        let man = &self.men[d];
        if !man.saber_out() {
            return false;
        }
        let level = match against {
            Against::Bolt => {
                let deflect = held(&man.sheet, &man.f).deflect;
                defence::bolt_level(man.defence(), deflect)
            }
            _ => man.defence(),
        };
        defence::can_block(&man.guard(level, self.now), point, against, self.now)
    }

    /// Whether `d` blocks by state a blade swung from `from` as an attacker
    /// who saw it at `view` saw it (C18): its guard then, where the history
    /// kept it; else as it stands.
    fn turns(&self, d: usize, from: Vec3, view: Option<u64>) -> bool {
        match self.seen(d, view) {
            Some(fence) => fence.guard.is_some_and(|guard| {
                defence::in_arc(guard.origin, guard.yaw, guard.level, from, Against::Blade)
            }),
            None => self.blocks(d, from, Against::Blade),
        }
    }

    /// A blow `d` turned by state: JA's legacy block (rulebook 7.2), a lock
    /// sometimes (`start::BLOCK_ODDS`, `start::BLOCK_ODDS_UP` for a defender
    /// well above), else the clash tree from its third branch.
    fn state_block(&mut self, a: usize, d: usize, point: Vec3) {
        let (att, def) = (&self.men[a], &self.men[d]);
        let odds = if def.offence() > att.offence() + start::UP_LEVELS {
            roll_between(self.rng, 1, 10) <= start::BLOCK_ODDS_UP
        } else {
            roll_between(self.rng, 1, 20) <= start::BLOCK_ODDS
        };
        let tick = self.tick;
        let (by, weapon) = (self.men[a].id, self.men[a].delivery());
        let at = self.parried_at(d, point);
        self.men[d].record(by, Outcome::Parried, tick, weapon, at);
        self.men[d].f.combat_until = self.now + super::COMBAT_MS;
        if odds && !self.men[d].player_at_rest() && self.try_lock(a, d) {
            return;
        }
        self.tree(a, d, point, false);
    }

    /// Where `d`'s blade turned a blow that met it by state at `point` (on
    /// its body): the nearest point of its first blade, where a view shows
    /// the parry; `point` without a placed blade.
    fn parried_at(&self, d: usize, point: Vec3) -> Vec3 {
        self.placed[d][0].map_or(point, |blade| {
            sweep::closest_on_segment(point, blade.now.muzzle, blade.now.tip)
        })
    }

    /// Two blades met (rulebook 8): a lock sometimes (`start::CLASH_ODDS`;
    /// neither in a special, the other not an idle player), else the tree.
    fn clash(&mut self, a: usize, d: usize, point: Vec3, met: bool) {
        let tick = self.tick;
        let (ida, idd) = (self.men[a].id, self.men[d].id);
        let (wa, wd) = (self.men[a].delivery(), self.men[d].delivery());
        self.men[a].record(idd, Outcome::Parried, tick, wd, point);
        self.men[d].record(ida, Outcome::Parried, tick, wa, point);
        let special = self.men[a].f.saber.current_move.special()
            || self.men[d].f.saber.current_move.special();
        if met
            && !special
            && roll_between(self.rng, 1, 20) <= start::CLASH_ODDS
            && !self.men[d].player_at_rest()
            && self.try_lock(a, d)
        {
            return;
        }
        self.tree(a, d, point, met);
    }

    /// The fencer `at` is in a clash, swinging (`defending` false) or met.
    fn fencer(&self, at: usize, defending: bool) -> Fencer {
        let man = &self.men[at];
        let anims = &self.level.anims;
        let torso = man.ps.torso;
        let attacking = power::attacking(
            man.f.saber.current_move,
            man.f.saber.blocked,
            man.f.weapon_wait_ms,
        );
        let hilt = hilt_motion(&self.placed[at], self.msec).map(math::length);
        Fencer {
            saber: man.f.saber,
            ps: man.ps,
            style: man.style(),
            offence: man.offence(),
            defence: i32::from(man.defence()),
            power: power::attack(man.style(), attacking, hilt),
            attacking,
            anim_power: power::animation(anims, torso.anim, torso.timer, defending)
                + power::kata_bonus(torso.anim),
            weapon_wait_ms: man.f.weapon_wait_ms,
        }
    }

    /// The tree (`saber::clash`) on copies of both, written back; then a
    /// disarm for whoever lost a knockaway or a broken parry.
    fn tree(&mut self, a: usize, d: usize, point: Vec3, met: bool) {
        let mut att = self.fencer(a, false);
        let mut def = self.fencer(d, true);
        let result = clash::resolve(&mut att, &mut def, point, met, self.rng);
        self.men[a].f.saber = att.saber;
        self.men[d].f.saber = def.saber;
        for man in [a, d] {
            self.men[man].f.combat_until = self.now + super::COMBAT_MS;
        }
        if result.disarm_a {
            self.disarm(a, d);
        }
        if result.disarm_d {
            self.disarm(d, a);
        }
    }

    /// `loser`'s lightsaber knocked out of its hand by `winner` (rulebook
    /// 8.4): by the styles' odds, along the winner's hilt's motion (the
    /// loser's own when the winner's is still), none without motion.
    fn disarm(&mut self, loser: usize, winner: usize) {
        let (l, w) = (&self.men[loser], &self.men[winner]);
        if !l.saber_out() || !l.blade().is_some_and(|b| b.saber) || l.f.saber.lock.is_some() {
            return;
        }
        if !clash::disarm(l.style(), w.style(), self.rng) {
            return;
        }
        let motion = hilt_motion(&self.placed[winner], self.msec)
            .or(hilt_motion(&self.placed[loser], self.msec));
        let (Some(motion), Some(hilt)) = (motion, self.placed[loser][0]) else {
            return;
        };
        let speed = math::length(motion).max(down::DISARM_LEAST) * down::DISARM_TIMES;
        let mut way = motion;
        math::normalize(&mut way);
        let knocked = Thrown::knocked(hilt.now.muzzle, math::scale(way, speed), self.now);
        self.men[loser].f.saber.thrown = Some(knocked);
    }

    /// `a`'s blade met `d`'s (or its guard) and the clash asked for a lock
    /// (rulebook 9.2): whether one began. When it does, both are set into
    /// the opening M4's pick chose, turned to face each other and moved to
    /// the lock's distance. A refusal after the pick keeps its draw.
    fn try_lock(&mut self, a: usize, d: usize) -> bool {
        if !self.may_lock(a, d) || !self.men[a].saber_out() || !self.men[d].saber_out() {
            return false;
        }
        let now = self.now;
        let (stance_a, stance_d) = (self.men[a].stance(now), self.men[d].stance(now));
        if !lock::can_lock(&stance_a, &stance_d) {
            return false;
        }
        let Some((a_leads, kind)) = lock::pick(&stance_a, &stance_d, self.rng) else {
            return false;
        };
        let (leader, follower) = if a_leads { (a, d) } else { (d, a) };
        let opening = lock::opening(kind, self.men[leader].style(), self.men[follower].style());
        let anims = &self.level.anims;
        if anims.get(opening.attacker.0).missing() || anims.get(opening.defender.0).missing() {
            return false;
        }
        let sides = [
            (leader, follower, opening.attacker),
            (follower, leader, opening.defender),
        ];
        for (at, other, (anim, fraction)) in sides {
            let enemy = self.men[other].id;
            let man = &mut self.men[at];
            motion::react(&mut man.ps, anims, anim, false);
            man.ps.velocity = [0.0; 3];
            let saber = &mut man.f.saber;
            saber.lock = Some(Lock {
                enemy,
                anim,
                frame: lock::first_frame(anims, anim, fraction),
                hits: 0,
                advance: false,
                pressed: man.f.last_command.buttons & buttons::ATTACK != 0,
                checked_ms: now,
            });
            saber.lock_until = now + lock::LOCK_MS;
            saber.blocked = Blocked::None;
            saber.current_move = Move::NONE;
            man.f.weapon_wait_ms = 0;
        }
        self.face(leader, follower, true);
        self.close_in(leader, follower, opening.distance);
        true
    }

    /// `a` looks straight at `d` (level, its pitch kept) and `d` straight
    /// back, its pitch `a`'s turned over when `mirror`, else its own.
    fn face(&mut self, a: usize, d: usize, mirror: bool) {
        let yaw = sweep::yaw_to(self.men[a].ps.origin, self.men[d].ps.origin);
        let pitch = self.men[a].ps.view[0];
        self.men[a].ps.set_view([pitch, yaw, 0.0]);
        let back = blades::normalize_180(yaw + 180.0);
        let theirs = &mut self.men[d].ps;
        let their_pitch = if mirror { 0.0 - pitch } else { theirs.view[0] };
        theirs.set_view([their_pitch, back, 0.0]);
    }

    /// Brings `att` and `def` to `ideal` units apart along the line between
    /// them: `att` takes half the error, then `def` all that is left. Each
    /// moves as far as its box goes through the level, not at all from
    /// inside a solid.
    fn close_in(&mut self, att: usize, def: usize, ideal: f32) {
        for (mover, other, share) in [(att, def, 0.5), (def, att, 1.0)] {
            let from = self.men[mover].ps.origin;
            let mut way = math::sub(self.men[other].ps.origin, from);
            let error = math::normalize(&mut way) - ideal;
            let goal = math::mad(from, error * share, way);
            let extents = Extents {
                mins: [
                    -motion::HALF_WIDTH * UNIT,
                    -motion::HALF_WIDTH * UNIT,
                    motion::SOLE_Z * UNIT,
                ],
                maxs: [
                    motion::HALF_WIDTH * UNIT,
                    motion::HALF_WIDTH * UNIT,
                    self.men[mover].ps.maxs_z * UNIT,
                ],
            };
            let trace = self.level.with(self.states.as_deref(), |world| {
                world.trace(
                    scale(from, UNIT),
                    scale(goal, UNIT),
                    &extents,
                    Contents::SOLID,
                )
            });
            if !trace.start_solid && !trace.all_solid {
                self.men[mover].ps.origin = scale(trace.end, 1.0 / UNIT);
            }
        }
    }

    /// Step 4: The locks: each pair once, the lower id first (rulebook 9.3, 9.4).
    fn locks(&mut self) {
        for a in 0..self.men.len() {
            let Some(mine) = self.men[a].f.saber.lock else {
                continue;
            };
            let d = self.find(mine.enemy).filter(|&d| {
                self.men[d]
                    .f
                    .saber
                    .lock
                    .is_some_and(|l| l.enemy == self.men[a].id)
            });
            let Some(d) = d else {
                self.unlock(a);
                continue;
            };
            if d < a {
                continue;
            }
            self.contest(a, d);
        }
    }

    /// A lock that lost its other side.
    fn unlock(&mut self, at: usize) {
        let saber = &mut self.men[at].f.saber;
        saber.lock = None;
        saber.lock_until = 0;
        saber.hand = Hand::Risen;
    }

    /// One tick of the lock between `a` and `d` (`a` the lower id;
    /// rulebook 9.3): a break, the presses banking hits, a hit spent once
    /// a JA frame to push the lock a step, a win at its end; else both held
    /// facing, still, on their frames.
    fn contest(&mut self, a: usize, d: usize) {
        let now = self.now;
        let (ma, md) = (&self.men[a], &self.men[d]);
        let broken = now >= ma.f.saber.lock_until
            || !ma.f.alive()
            || !md.f.alive()
            || !lock::in_lock_pose(ma.ps.torso.anim)
            || !lock::in_lock_pose(md.ps.torso.anim)
            || !lock::in_reach(ma.ps.origin, md.ps.origin);
        if broken {
            self.end_lock(a, d, false, 0);
            return;
        }
        for at in [a, d] {
            self.bank(at);
        }
        let level = self.level;
        let anims = &level.anims;
        for (me, them) in [(a, d), (d, a)] {
            let Some(mut mine) = self.men[me].f.saber.lock else {
                return;
            };
            if !mine.advance {
                continue;
            }
            mine.advance = false;
            let strength = self.men[me].offence() + contest::STRENGTH_BONUS;
            match lock::advance(anims, &mine, strength) {
                None => {
                    self.men[me].f.saber.lock = Some(mine);
                    self.end_lock(me, them, true, strength);
                    return;
                }
                Some((frame, remaining)) => {
                    mine.frame = frame;
                    self.men[me].f.saber.lock = Some(mine);
                    if let Some(theirs) = self.men[them].f.saber.lock.as_mut() {
                        theirs.frame = lock::mirror(anims, theirs.anim, remaining);
                    }
                }
            }
        }
        self.face(a, d, false);
        for at in [a, d] {
            let man = &mut self.men[at];
            let Some(mine) = man.f.saber.lock else {
                continue;
            };
            let left = man
                .f
                .saber
                .lock_until
                .wrapping_sub(now)
                .min(i32::MAX as u64) as i32;
            man.ps.velocity = [0.0; 3];
            for part in [&mut man.ps.legs, &mut man.ps.torso] {
                part.anim = mine.anim;
                part.timer = left;
            }
            man.f.weapon_wait_ms = 0;
            man.f.pose.freeze(mine.anim, mine.frame, anims, now);
        }
    }

    /// A locked fighter's presses: a new press banks hits (more with a
    /// strong arm), and once a JA frame a banked hit is taken up to push.
    fn bank(&mut self, at: usize) {
        let man = &mut self.men[at];
        let Some(mut mine) = man.f.saber.lock else {
            return;
        };
        let held = man.f.last_command.buttons & buttons::ATTACK != 0;
        if held && !mine.pressed {
            let strong = man.sheet.modifier(Ability::Strength) >= contest::STRONG_ARM;
            let hits = lock::press_hits(man.style(), strong, self.rng);
            mine.hits = mine.hits.saturating_add(hits);
        }
        mine.pressed = held;
        if mine.hits > 0 && self.now >= mine.checked_ms + lock::ADVANCE_MS {
            mine.checked_ms = self.now;
            if !mine.advance {
                mine.hits -= 1;
            }
            mine.advance = true;
        }
        man.f.saber.lock = Some(mine);
    }

    /// The lock between `w` and `l` ends (rulebook 9.4): won by `w` with
    /// `strength` when `victory`, else broken with `w` the lower id. Both
    /// play their breaks; a won lock's loser may be floored, shoved and
    /// killed, a broken one's pair pushed apart.
    fn end_lock(&mut self, w: usize, l: usize, victory: bool, strength: i32) {
        let now = self.now;
        let banked = self.men[w].f.saber.lock.map_or(0, |mine| mine.hits);
        let super_break = lock::super_breaks(strength, banked, self.rng);
        let anim_of = |man: &Man| man.f.saber.lock.map_or(man.ps.torso.anim, |mine| mine.anim);
        let (won, lost) = lock::breaks(
            anim_of(&self.men[w]),
            anim_of(&self.men[l]),
            victory,
            super_break,
        );
        let anims = &self.level.anims;
        for (at, outcome, loser) in [(w, won, false), (l, lost, true)] {
            let man = &mut self.men[at];
            if man.f.alive() {
                motion::react(&mut man.ps, anims, outcome.anim, false);
                if let Some(mv) = outcome.saber_move {
                    man.f.saber.current_move = mv;
                }
                if outcome.defenceless {
                    man.ps.torso.timer += rules::saber_locks::end::DEFENCELESS_MS;
                }
            }
            let saber = &mut man.f.saber;
            saber.lock = None;
            saber.lock_until = 0;
            saber.blocked = Blocked::None;
            let broken = loser && victory && outcome.saber_move.is_some_and(Move::broken);
            man.f.weapon_wait_ms = if broken { man.ps.torso.timer } else { 0 };
        }
        let from = self.men[w].ps.origin;
        let to = self.men[l].ps.origin;
        if victory {
            if banked > 0 && !super_break && self.men[l].f.alive() {
                let winner = self.men[w].id;
                let loser = &mut self.men[l];
                loser.f.saber.knock_down(now, lock::LOSS_DOWN_MS);
                loser.ps.velocity = lock::shove(from, to, true);
                loser.f.last_attacker = Some((winner, now));
                if loser.sheet.flags & flags::PLOT == 0 {
                    let most = i32::from(loser.sheet.max_hp.max(1));
                    let percent = loser.f.hp / most;
                    if lock::loss_kills(percent, self.rng) {
                        self.hits.push(Hit {
                            attacker: winner,
                            target: self.men[l].id,
                            source: Source::Script {
                                damage: u16::MAX,
                                damage_type: None,
                                cast: false,
                                shot: None,
                                staged: false,
                            },
                            point: None,
                            direction: None,
                        });
                    }
                }
            }
        } else {
            self.men[l].ps.velocity = lock::shove(from, to, false);
            self.men[w].ps.velocity = lock::shove(to, from, false);
            self.men[l].f.saber.hand = Hand::Risen;
        }
        self.men[w].f.saber.hand = Hand::Risen;
    }

    /// Step 4: The kicks (rulebook 10.2): each kick in its striking window,
    /// in id order, hits the first body ahead of the kicker's middle once;
    /// a foe is also shoved, lifted and perhaps floored.
    fn kicks(&mut self) {
        for a in 0..self.men.len() {
            if !self.men[a].f.alive() {
                continue;
            }
            let Some(kick) = kick::kick(&self.men[a].ps, &self.level.anims, self.now) else {
                continue;
            };
            self.land_kick(a, kick);
        }
    }

    /// `a`'s kick traced ahead, and what it does to whoever it meets.
    fn land_kick(&mut self, a: usize, kick: kick::Kick) {
        let now = self.now;
        let ps = &self.men[a].ps;
        let start = math::add(ps.origin, [0.0, 0.0, ps.maxs_z * 0.5]);
        let segment = sweep::Segment {
            from: start,
            to: math::mad(start, kick.reach, kick.direction),
            length: kick.reach,
        };
        let bodies = self.targets(a, kick.reach + 32.0);
        let Some(&(victim, _, point, _)) = self.cut(&bodies, &[segment], kicks::BOX).first() else {
            return;
        };
        let Some(d) = self.find(victim) else {
            return;
        };
        if self.men[a].scene || self.men[d].scene {
            return;
        }
        if self.men[a]
            .f
            .saber
            .kicked
            .is_some_and(|(id, until)| id == victim && until > now)
        {
            return;
        }
        let kicker = &mut self.men[a];
        let legs_ms = u64::try_from(kicker.ps.legs.timer).unwrap_or(0);
        kicker.f.saber.kicked = Some((victim, now + legs_ms));
        let hardest = roll_between(self.rng, 1, kicks::HARDEST_ONE_IN) == kicks::HARDEST_ONE_IN;
        let points = kicks::POINTS[usize::from(hardest)];
        self.hits.push(Hit {
            attacker: self.men[a].id,
            target: victim,
            source: Source::Power {
                points,
                extra: 0,
                area: false,
                pushes: false,
                damage_type: Some(type_rows::BLUDGEONING),
            },
            point: Some(point),
            direction: Some(kick.direction),
        });
        if !self.foes(a, d) || self.men[d].ps.flags & motion::motion_flags::KNOCKBACK_HOLD != 0 {
            return;
        }
        let (low, high) = kick.push;
        let mut shove = self.rng.range(low, high).trunc();
        if shove >= kicks::SHOVE_FROM && roll_between(self.rng, 0, kicks::SHOVE_ONE_IN - 1) == 0 {
            shove = kicks::SHOVE;
        }
        let kicker = self.men[a].id;
        let victim = &mut self.men[d];
        let floored = kick::floors(
            &victim.ps,
            victim.f.weapon_wait_ms,
            victim.saber_out(),
            victim.f.saber.floored(),
        );
        victim.ps.velocity = math::mad(victim.ps.velocity, shove, kick.direction);
        victim.ps.velocity[2] = kick::LIFT;
        if victim.f.alive() && floored {
            victim.f.saber.knock_down(now, kick::DOWN_MS);
        }
        victim.f.last_attacker = Some((kicker, now));
    }

    /// Step 5: The thrown sabers (see `saber::throw`).
    fn throws(&mut self, authority: bool) {
        let dt = self.msec as f32 / 1000.0;
        for a in 0..self.men.len() {
            let Some(mut t) = self.men[a].f.saber.thrown else {
                continue;
            };
            let man = &self.men[a];
            if !man.f.alive() {
                self.men[a].f.saber.thrown = None;
                continue;
            }
            let hand = hand_of(man);
            let alt = man.f.last_command.buttons & buttons::ALT_ATTACK != 0;
            let attack = man.f.last_command.buttons & buttons::ATTACK != 0;
            let cannot = man.f.force.gripped_by.is_some() || man.f.force.stunned(self.now);
            let out_ms = self.now.saturating_sub(t.since);
            match t.flight {
                Flight::Leaving => {
                    // From the hilt, where the blade last was.
                    t.at = self.placed[a][0]
                        .map(|sweep| sweep.now.muzzle)
                        .or(blades::fresh(man.f.trails[0], self.now).map(|trail| trail.muzzle))
                        .unwrap_or(hand);
                    let (forward, ..) = math::angle_vectors(man.ps.view);
                    t.velocity = math::scale(forward, throw::OUT_SPEED);
                    t.flight = Flight::Out;
                    t.since = self.now;
                    t.aim_at = self.now
                        + if t.level >= 3 {
                            throw::AIM_L3_MS
                        } else {
                            throw::AIM_L2_MS
                        };
                }
                Flight::Out => {
                    let reach = throw::RANGE_PER_LEVEL * f32::from(t.level.max(1));
                    let too_far = math::length(math::sub(t.at, man.ps.origin)) > reach;
                    let let_go = out_ms >= throw::HOLD_MS && (!alt || out_ms >= throw::MAX_OUT_MS);
                    if cannot || let_go || too_far {
                        t.turn_back(self.now);
                    } else if t.level >= 2 && self.now >= t.aim_at {
                        t.steer(self.crosshair(a), throw::AIMED_SPEED);
                        t.aim_at = self.now
                            + if t.level >= 3 {
                                throw::AIM_L3_MS
                            } else {
                                throw::AIM_L2_MS
                            };
                    }
                    if t.flight == Flight::Out && !self.fly(&mut t, dt, false) {
                        t.turn_back(self.now);
                    }
                }
                Flight::Back => {
                    let distance = math::length(math::sub(hand, t.at));
                    if distance < throw::CATCH {
                        let f = &mut self.men[a].f;
                        f.saber.thrown = None;
                        f.weapon_wait_ms = throw::CATCH_MS;
                        continue;
                    }
                    if self.now >= t.aim_at {
                        t.steer(hand, Thrown::home_speed(t.level, distance));
                        t.aim_at = self.now + throw::AIM_BACK_MS;
                    }
                    self.fly(&mut t, dt, true);
                }
                Flight::Falling => {
                    t.velocity[2] -= throw::GRAVITY * dt;
                    if !self.fly(&mut t, dt, false) {
                        t.flight = Flight::Lying;
                        t.velocity = [0.0; 3];
                        t.since = self.now;
                    }
                }
                Flight::Lying => {
                    if (out_ms >= throw::RETRIEVE_MS && attack) || out_ms >= throw::AUTO_RETURN_MS {
                        t.flight = Flight::Back;
                        t.since = self.now;
                        t.aim_at = self.now;
                    }
                }
            }
            if authority
                && matches!(t.flight, Flight::Out | Flight::Back)
                && self.now >= t.hit_until
            {
                self.throw_hit(a, &mut t);
            }
            self.men[a].f.saber.thrown = Some(t);
        }
    }

    /// Moves a thrown saber one tick; `false` when the level stopped it
    /// (it stays where it met the level). Coming home it goes through.
    fn fly(&self, t: &mut Thrown, dt: f32, through: bool) -> bool {
        let next = math::mad(t.at, dt, t.velocity);
        if through {
            t.at = next;
            return true;
        }
        let trace = self.trace(t.at, next, throw::HALF_SIZE, Contents::SOLID);
        if trace.start_solid || trace.fraction < 1.0 {
            t.at = scale(trace.end, 1.0 / UNIT);
            return false;
        }
        t.at = next;
        true
    }

    /// Where `a`'s view meets the world (units): from a player's crosshair
    /// point, a creature's eye, along its view as far as any throw reaches.
    fn crosshair(&self, a: usize) -> Vec3 {
        let man = &self.men[a];
        let from = if man.avatar {
            mind::crosshair(&man.ps)
        } else {
            mind::eye(&man.ps)
        };
        let (forward, ..) = math::angle_vectors(man.ps.view);
        let end = math::mad(from, VIEW_REACH, forward);
        scale(self.trace(from, end, 0.0, Contents::SOLID).end, 1.0 / UNIT)
    }

    /// A flying saber's hit: the nearest character whose origin is within
    /// reach in clear sight and whom its owner may harm; a block parries
    /// (and may knock it down).
    fn throw_hit(&mut self, a: usize, t: &mut Thrown) {
        let reach = t.reach();
        let victim = (0..self.men.len())
            .filter(|&d| d != a && self.men[d].f.alive() && self.foes(a, d))
            .map(|d| (d, math::length(math::sub(t.at, self.men[d].ps.origin))))
            .filter(|&(_, distance)| distance <= reach)
            .filter(|&(d, _)| {
                (self.level).sees(self.states.as_deref(), t.at, self.men[d].ps.origin)
            })
            .min_by(|x, y| x.1.total_cmp(&y.1));
        let Some((d, _)) = victim else {
            return;
        };
        t.hit_until = self.now + throw::HIT_EVERY_MS;
        let (thrower, weapon) = (self.men[a].id, self.men[a].delivery());
        if self.blocks(d, t.at, Against::Thrown) {
            let tick = self.tick;
            let at = self.parried_at(d, t.at);
            let defender = &mut self.men[d];
            defender.f.saber.blocked = defence::parry_for(&defender.ps, t.at, false);
            defender.record(thrower, Outcome::Parried, tick, weapon, at);
            let level = defender.defence();
            if throw::blocked_down(level, t.level, roll_between(self.rng, 1, 10)) {
                let (forward, ..) = math::angle_vectors([0.0, self.men[d].ps.view[1], 0.0]);
                *t = Thrown::knocked(t.at, math::scale(forward, throw::KNOCKED_SPEED), self.now);
                return;
            }
            t.turn_back(self.now);
            return;
        }
        let man = &self.men[a];
        let mut direction = t.velocity;
        math::normalize(&mut direction);
        self.hits.push(Hit {
            attacker: thrower,
            target: self.men[d].id,
            source: Source::Weapon {
                weapon: held(&man.sheet, &man.f),
                cadence_ms: throw::CADENCE_MS,
                share: 1000,
                area: false,
            },
            point: Some(t.at),
            direction: Some(direction),
        });
        t.turn_back(self.now);
    }

    /// Step 6: The saber boxes bolts clip against: lit lightsabers of those who
    /// block with them (rulebook 5.6).
    fn boxes(&self) -> Vec<(SimId, Vec3, Vec3)> {
        (0..self.men.len())
            .filter(|&at| self.solid(at) && self.men[at].blade().is_some_and(|b| b.saber))
            .filter_map(|at| {
                let (min, max) = blades::bounds(self.placed[at].iter().flatten().map(|s| s.now))?;
                Some((self.men[at].id, min, max))
            })
            .collect()
    }

    /// Step 6: What the history keeps of everyone this tick (C18): its
    /// blades, whether they stop others', and its guard against blades
    /// ready to block (the arc left for the attacker's point).
    fn fences(&self) -> Vec<(SimId, Fence)> {
        (0..self.men.len())
            .map(|at| {
                let man = &self.men[at];
                let level = man.defence();
                let ready = man.saber_out()
                    && level > 0
                    && defence::ready(&man.guard(level, self.now), self.now);
                let guard = ready.then_some(Stand {
                    level,
                    origin: man.ps.origin,
                    yaw: man.ps.view[1],
                });
                let fence = Fence {
                    blades: self.placed[at],
                    solid: self.solid(at),
                    guard,
                };
                (man.id, fence)
            })
            .collect()
    }

    /// Step 6: Each placed blade's place, for the next tick's sweep, kept
    /// only while a swing is under way (a start, an attack, a transition or
    /// a return): an idle blade sweeps nothing, and a place that changes
    /// every tick would cost every snapshot its bytes (a delegated decision:
    /// JA keeps every blade's; an idle blade's sweep then starts from where
    /// it is, and its clash test sees the blade as it stands).
    fn trails(&mut self) {
        for (man, sweeps) in self.men.iter_mut().zip(&self.placed) {
            let mv = man.f.saber.current_move;
            let swinging = mv.attack() || mv.in_between();
            for (trail, sweep) in man.f.trails.iter_mut().zip(sweeps) {
                *trail = sweep
                    .filter(|_| swinging)
                    .map(|sweep| sweep.trail(self.now));
            }
        }
    }
}

/// A bolt that met `fighter` (standing as `ps`, with `sheet`) at `at`, on
/// its body or, `on_blade`, on its saber box (rulebook 7.5, I23 2.5): a lit
/// lightsaber in hand turns a blockable bolt, on the body only by state
/// (C6's arc for bolts, the level moved by the saber's deflect property,
/// the cooldown out, a level left as it stands), on the blade always
/// (QUIRKS.md Q-0047). The bolt is absorbed or sent on
/// (`defence::bolt`), the cooldown set, the reflect parry played (on the
/// blade only with the weapon idle), the event recorded. `None` when it
/// is not turned.
/// Whether `fighter` (with `sheet`) could turn `bolt` at all: a lit
/// lightsaber in hand, alive, a blockable bolt, a bolt level of 1 or more
/// (where it stands and faces aside).
pub(super) fn could_turn(fighter: &Fighter, sheet: &Sheet, bolt: &crate::combat::Missile) -> bool {
    let kit = Kit::of(sheet);
    let Some(blade) = kit.blade(fighter.hand).filter(|blade| blade.saber) else {
        return false;
    };
    bolt.blockable
        && fighter.alive()
        && fighter.saber.on
        && fighter.saber.thrown.is_none()
        && defence::bolt_level(kit.defence(blade), held(sheet, fighter).deflect) > 0
}

#[allow(clippy::too_many_arguments)]
pub(super) fn bolt_block(
    fighter: &mut Fighter,
    ps: &MotionState,
    sheet: &Sheet,
    bolt: &crate::combat::Missile,
    at: Vec3,
    on_blade: bool,
    shooter: Option<Vec3>,
    (now, tick): (u64, u64),
    rng: &mut SimRng,
) -> Option<BoltFate> {
    let kit = Kit::of(sheet);
    let lightsaber = kit.blade(fighter.hand).is_some_and(|blade| blade.saber);
    if !bolt.blockable
        || !fighter.alive()
        || !lightsaber
        || !fighter.saber.on
        || fighter.saber.thrown.is_some()
    {
        return None;
    }
    let blade = kit.blade(fighter.hand)?;
    let level = defence::bolt_level(kit.defence(blade), held(sheet, fighter).deflect);
    let guard = Guard {
        saber: &fighter.saber,
        ps,
        weapon_wait_ms: fighter.weapon_wait_ms,
        level,
        attack_held: fighter.last_command.buttons & buttons::ATTACK != 0,
        hand_out: fighter.force.hand_out(now),
    };
    let now_level = defence::bolt_level_now(ps, fighter.last_command.forward < 0, level);
    if !on_blade && (now_level == 0 || !defence::can_block(&guard, at, Against::Bolt, now)) {
        return None;
    }
    let away = shooter.unwrap_or_else(|| math::sub(math::scale(at, 2.0), ps.origin));
    let (fate, cooldown) = defence::bolt(ps, now_level, at, away, rng, now);
    fighter.saber.bolt_block_until = cooldown;
    if !on_blade || fighter.weapon_wait_ms <= 0 {
        fighter.saber.blocked = defence::parry_for(ps, at, true);
    }
    fighter.record(Event {
        sequence: 0,
        tick,
        by: Some(bolt.owner),
        result: Outcome::Deflected,
        location: Location::None,
        damage: 0,
        weapon: Some(bolt.weapon.delivery),
        at: Some(Event::units(at)),
        visual: None,
    });
    Some(fate)
}

/// Whether a fighter (standing as `ps`) sweeps to cut this tick: an
/// attack, or a special in its animation's power window, a super-break's
/// win, a spin. Its animation's strength, and whether its sweep arcs.
fn sweeping(anims: &motion::AnimTable, ps: &MotionState, f: &Fighter) -> Option<(i32, bool)> {
    let torso = ps.torso;
    let mv = f.saber.current_move;
    let spin = matches!(torso.anim, Anim::BOTH_SPINATTACK6 | Anim::BOTH_SPINATTACK7);
    let attacking = power::attacking(mv, f.saber.blocked, f.weapon_wait_ms)
        || power::final_break_won(torso.anim);
    let strength = power::animation(anims, torso.anim, torso.timer, false);
    if !attacking || (strength == 0 && !spin) {
        return None;
    }
    let arcs = mv.attack()
        || motion::is_special_move(torso.anim)
        || motion::is_spin(torso.anim)
        || motion::is_leap(torso.anim);
    Some((strength, arcs))
}

/// How far each way a fighter's sweep traces a box (units): a melee
/// weapon's `rules::sabers::sweep::MELEE_BOX`, a saber's a line.
fn sweep_half(f: &Fighter) -> f32 {
    if f.weapon_wait_ms > 0 {
        rules::sabers::sweep::MELEE_BOX
    } else {
        0.0
    }
}

/// Whether the level stops a box `half` units each way going from `from`
/// to `to` (units), as a sweep's cut stops at a wall.
fn walled(level: &crate::Level, states: Option<&[u8]>, half: f32, from: Vec3, to: Vec3) -> bool {
    let h = half * UNIT;
    let extents = Extents {
        mins: [-h; 3],
        maxs: [h; 3],
    };
    let wall = level.with(states, |world| {
        world.trace(
            scale(from, UNIT),
            scale(to, UNIT),
            &extents,
            Contents::SOLID,
        )
    });
    wall.fraction < 1.0 || wall.start_solid
}

/// Each segment's nearest body and where it entered, stopping at a wall
/// in front of one (`walled`: the way from a segment's start to where it
/// meets the body is not clear): who, the sweep's worth, the point, the
/// blade's way.
fn cut(
    bodies: &[Body],
    segments: &[sweep::Segment],
    half: f32,
    walled: impl Fn(Vec3, Vec3) -> bool,
) -> Vec<(SimId, f32, Vec3, Vec3)> {
    let mut out = Vec::new();
    for segment in segments {
        let nearest = bodies
            .iter()
            .filter_map(|body| Some((body.entity, sweep::enters(segment, body, half)?)))
            .min_by(|a, b| a.1.total_cmp(&b.1));
        let Some((entity, fraction)) = nearest else {
            continue;
        };
        let point = segment.at(fraction);
        if walled(segment.from, point) {
            break;
        }
        let direction = blades::toward(segment.from, segment.to).unwrap_or([0.0; 3]);
        out.push((
            SimId(entity & !BODY),
            segment.worth(fraction),
            point,
            direction,
        ));
    }
    out
}

/// Where a client's own blades cut the bodies it sees (I22 C20, the
/// contact sparks): its fighter `before` and `now` (its predictions a tick
/// apart, `now_ms` the later's time, standing as `ps`, with `sheet`)
/// swept as the authority sweeps (the same segments, box and walls),
/// through `bodies` as the client draws them (`sim::body`). Each body cut
/// once, where it was first met, with the blade's way there. What the
/// authority decides from the same view (C18) unless a blade or a guard
/// turns it; prediction shows neither.
pub fn seen_cuts(
    level: &crate::Level,
    states: Option<&[u8]>,
    (ps, sheet): (&MotionState, &Sheet),
    before: &Fighter,
    now: &Fighter,
    now_ms: u64,
    bodies: &[Body],
) -> Vec<(SimId, Vec3, Vec3)> {
    let anims = &level.anims;
    let Some(data) = anims.skeleton() else {
        return Vec::new();
    };
    if bodies.is_empty() || !now.alive() || now.saber.locked(now_ms) {
        return Vec::new();
    }
    let Some((_, arcs)) = sweeping(anims, ps, now) else {
        return Vec::new();
    };
    let half = sweep_half(now);
    let walled = |from: Vec3, to: Vec3| walled(level, states, half, from, to);
    let slots = blades::slots(&Kit::of(sheet), &now.saber, now.hand);
    let mut out: Vec<(SimId, Vec3, Vec3)> = Vec::new();
    for (i, slot) in slots.iter().enumerate() {
        let (Some(slot), Some(trail)) = (slot, blades::fresh(now.trails[i], now_ms)) else {
            continue;
        };
        let Some(length) = slot.length.or(data.saber_blade(slot.id).map(|b| b.length)) else {
            continue;
        };
        let (b0, d0) = blades::fresh(before.trails[i], now_ms)
            .map_or((trail.muzzle, trail.direction), |t| (t.muzzle, t.direction));
        let segments = sweep::segments(b0, d0, trail.muzzle, trail.direction, length, arcs);
        for (victim, _, point, direction) in cut(bodies, &segments, half, walled) {
            if out.iter().all(|(seen, ..)| *seen != victim) {
                out.push((victim, point, direction));
            }
        }
    }
    out
}

/// What `man`'s swing owes at `now` (all of it when `ending`): who owes it,
/// each delivery, and whether the swing's move is a special.
fn owed(man: &mut Man, now: u64, ending: bool) -> Vec<(SimId, sweep::Delivery, bool)> {
    let special = man.f.swing.saber_move.special();
    man.f
        .swing
        .due(now, ending)
        .into_iter()
        .map(|delivery| (man.id, delivery, special))
        .collect()
}

/// Where a character's hand is, for a saber coming home (a stand-in: its
/// chest, the skeleton's hand being posed only near others).
fn hand_of(man: &Man) -> Vec3 {
    mind::chest(&man.ps)
}

/// The first blade's hilt's travel over the last tick, scaled to a JA
/// server frame (`power::JA_FRAME_MS`), if it had a fresh place.
fn hilt_motion(sweeps: &[Option<Sweep>; 2], msec: u32) -> Option<Vec3> {
    let sweep = sweeps[0]?;
    let before = sweep.before?;
    let scale = power::JA_FRAME_MS as f32 / msec.max(1) as f32;
    let motion = math::scale(math::sub(sweep.now.muzzle, before.muzzle), scale);
    (motion != [0.0; 3]).then_some(motion)
}

/// Where two blades come nearest (the middle of their nearest points):
/// where they met.
fn midpoint(a: &crate::skeleton::Blade, b: &crate::skeleton::Blade) -> Vec3 {
    let (p, q) = (a.muzzle, b.muzzle);
    let (u, v) = (math::sub(a.tip, p), math::sub(b.tip, q));
    let w = math::sub(p, q);
    let (uu, uv, vv) = (math::dot(u, u), math::dot(u, v), math::dot(v, v));
    let (uw, vw) = (math::dot(u, w), math::dot(v, w));
    let denominator = uu * vv - uv * uv;
    let s = if denominator > 0.0 {
        ((uv * vw - vv * uw) / denominator).clamp(0.0, 1.0)
    } else {
        0.0
    };
    let t = if vv > 0.0 {
        ((uv * s + vw) / vv).clamp(0.0, 1.0)
    } else {
        0.0
    };
    let on_a = math::mad(p, s, u);
    let on_b = math::mad(q, t, v);
    math::scale(math::add(on_a, on_b), 0.5)
}
