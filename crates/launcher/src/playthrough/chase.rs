//! A fight's chase and reach, the waits on talks and on the world settling,
//! and a round's record.

use super::*;

/// Whether a way leads to a foe, asked every `REACH_EVERY_MS` rather than
/// each step (a way's search is most of a fight's time): to the floor
/// where it stands, and to where a walk to it ends (`Here::approach`).
#[derive(Default)]
pub(super) struct Reach(pub(super) BTreeMap<sim::SimId, (u64, bool, bool)>);

impl Reach {
    /// (to its floor, to where a walk to it ends) for foe `id` at `at`.
    pub(super) fn ways(
        &mut self,
        here: &Here,
        player: &sim::motion::MotionState,
        (id, at): (sim::SimId, [f32; 3]),
        now: u64,
    ) -> (bool, bool) {
        if let Some((asked, floor, walk)) = self.0.get(&id)
            && now < asked + REACH_EVERY_MS
        {
            return (*floor, *walk);
        }
        let floor = here.open_way(player.origin, walker_to(here.level, player, at).goal);
        let walk = here.open_way(player.origin, here.approach(player, at).goal);
        self.0.insert(id, (now, floor, walk));
        (floor, walk)
    }

    /// Whether the player can get at foe `id` at `at`: one seen close by
    /// with no closed door between (a gun hits it; a blade closes on it
    /// straight, off the mesh as a scene may stand it), or one with a way
    /// to it, or to the floor before one in sight (one on a rock off the
    /// mesh, a Sand Person by the herd); not one unseen behind what shuts
    /// it off (a Sith teacher in the academy's doorway out to the valley).
    pub(super) fn gettable(
        &mut self,
        here: &Here,
        player: &sim::motion::MotionState,
        me: [f32; 3],
        foe: (sim::SimId, [f32; 3]),
        now: u64,
    ) -> bool {
        let seen = here.sees(player, foe.0);
        let door_between = here.snapshot.entities.iter().any(|entity| {
            matches!(entity.kind, sim::Kind::Object(ObjectKind::Door, _))
                && entity.object.is_some_and(|o| o.state == 0)
                && off_the_way(me, foe.1, entity.position) <= 1.0
        });
        if seen && flat(foe.1, me) <= GUN_M && !door_between {
            return true;
        }
        let (floor, walk) = self.ways(here, player, foe, now);
        (seen || floor) && walk
    }
}

/// A fight with one foe at a time by the fight goal's rules, for a `fight`
/// and for any goal's fight back alike: a gun fires at a foe it sees within
/// `SHOOT_M`; a blade or fists swing at one they see within the sim's
/// reach, stepping in as a creature's mind does (`melee_m`); else the foe
/// is walked up to, planned again as it moves (`chase_replan_m`), and, the
/// walk arrived short of it or no way leading to where it ends (it stands
/// off the floor, in a doorway, over rubble), gone at straight, stepping
/// aside each way in turn when that gains nothing. A foe left unhurt at hand for `SWITCH_MS` gets the other
/// kind of weapon the party carries; one seen but out of every way's reach,
/// a gun. (A fight back that stood where its walk ended, short of a guard
/// droid strafing in Dantooine's ruins, and swung at the air out of its
/// blade's reach, held the party in combat for good.)
#[derive(Default)]
pub(super) struct Chase {
    /// The foe last struck at or gone after.
    pub(super) foe: Option<sim::SimId>,
    /// The foe the walk was planned to, and where it stood.
    pub(super) planned: Option<(sim::SimId, [f32; 3])>,
    /// A foe at hand the player has not hurt: its HP, and since when.
    pub(super) unhurt: Option<(sim::SimId, i32, u64)>,
    /// A gun taken up for a foe out of every way's reach.
    pub(super) armed_far: bool,
    /// The foe a walk stalled on and on to: shot from where the player is.
    pub(super) lost: Option<sim::SimId>,
}

impl Chase {
    /// The foe to fight among `foes` (nearest first) that `fits`: the one
    /// already fought while it still fits, as a player keeps their target,
    /// else the nearest. (Two Sith troopers as far off as each other in
    /// Manaan's Sith base, picked afresh each step, took turns as the nearer;
    /// each turn planned the walk to them anew, its stalls forgotten before
    /// one stepped aside the free way, and the player stood pressed against
    /// a door's jamb for good.)
    pub(super) fn pick(
        &self,
        foes: Vec<(sim::SimId, [f32; 3])>,
        mut fits: impl FnMut(&(sim::SimId, [f32; 3])) -> bool,
    ) -> Option<(sim::SimId, [f32; 3])> {
        let kept = self
            .foe
            .and_then(|id| foes.iter().find(|(foe, _)| *foe == id).copied())
            .filter(&mut fits);
        kept.or_else(|| foes.into_iter().find(fits))
    }

    /// This step's command against foe `id` at `at`, walked by `walker`,
    /// and whether it walks (not at hand); weapons changed go into `acts`.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn strike(
        &mut self,
        walker: &mut Option<Walker>,
        here: &Here,
        world: &sim::SimWorld,
        reach: &mut Reach,
        player: &sim::motion::MotionState,
        me: &sim::EntityState,
        (id, at): (sim::SimId, [f32; 3]),
        now: u64,
        acts: &mut Vec<Action>,
    ) -> (Command, bool) {
        self.foe = Some(id);
        let ranged = ranged(me);
        let d = flat(at, me.position);
        let seen = here.sees(player, id);
        // One no way leads to but seen (a turret on the Star Forge's wall),
        // or one the walk to stalled on and on (Peragus's assassin droid
        // shooting from past the dormitories' wreckage, its way blocked):
        // shot from here, a gun taken up for it, as a player would.
        let (_, routable) = reach.ways(here, player, (id, at), now);
        if walker.as_ref().is_some_and(|w| w.stalls > LOST_STALLS) {
            self.lost = Some(id);
        }
        let far_shot = (!routable || self.lost == Some(id)) && seen && d <= GUN_M;
        if far_shot && !ranged && !self.armed_far {
            acts.extend(other_weapon(world, here.snapshot, false));
            self.armed_far = true;
        }
        let (close, swing) = melee_m();
        let within = match (ranged, far_shot) {
            (true, true) => GUN_M,
            (true, false) => SHOOT_M,
            (false, _) => swing,
        };
        let at_hand = d <= within && seen;
        // Time out of reach or sight does not count (a wounded Sand Person
        // fleeing over the dunes).
        let hp = here
            .snapshot
            .get(id)
            .and_then(|e| e.fighter)
            .map_or(0, |f| f.hp);
        match self.unhurt {
            Some((foe, was, since)) if foe == id && hp >= was && at_hand && now >= since + SWITCH_MS => {
                acts.extend(other_weapon(world, here.snapshot, ranged));
                self.unhurt = Some((id, hp, now));
            }
            Some((foe, was, _)) if foe == id && hp >= was && at_hand => {}
            _ => self.unhurt = Some((id, hp, now)),
        }
        if at_hand {
            let mut command = Command {
                buttons: buttons::ATTACK,
                ..aiming(player, me, at)
            };
            if !ranged && d > close {
                command.forward = 127;
            }
            return (command, false);
        }
        // A walk stalling (no headway for half a stall's time, or getting
        // round a stall) keeps its plan while the same foe shifts about: a
        // plan made afresh forgets the stall, and a gunner kiting at 6 to
        // 10 m (Hrakert's heavy sentry droid) kept the player pressed into
        // a corridor's corner.
        let stalling = walker.as_ref().is_some_and(|w| {
            !w.arrived(player) && (w.stalls > 0 || now >= w.since_ms + sim::walk::STALL_MS / 2)
        });
        let moved = self.planned.is_none_or(|(foe, was)| {
            foe != id || (!stalling && flat(was, at) > chase_replan_m(ranged))
        });
        if walker.is_none() || moved {
            *walker = Some(here.approach(player, at));
            self.planned = Some((id, at));
        }
        let w = walker.as_mut().expect("planned above");
        if !w.arrived(player) && here.leads(player.origin, w.goal) {
            return (w.command(here.level, player, now, &|index| here.to_try(index)), true);
        }
        if d < w.best - 0.05 {
            w.best = d;
            w.since_ms = now;
        } else if now >= w.since_ms + sim::walk::STALL_MS {
            w.stalls = w.stalls.saturating_add(1).min(127);
            w.aside_until_ms = now + sim::walk::SIDESTEP_MS * 2;
            w.best = d;
            w.since_ms = now;
        }
        let mut straight = Command {
            forward: 127,
            ..facing(player, at)
        };
        if now < w.aside_until_ms {
            straight.right = if w.stalls % 2 == 1 { 127 } else { -127 };
        }
        (straight, true)
    }
}

/// A conversation with the player (`Talks`): its id, its owner's object
/// number, and when it ended.
#[derive(Debug, Clone)]
pub(super) struct Talk {
    pub(super) id: u32,
    pub(super) owner: Option<u32>,
    pub(super) ended_ms: u64,
}

/// The player's conversations as they end, for the `talk` goals: a `talk`
/// takes the one it saw under way once it ends, else one that ended while
/// the goal before it ran (a trigger's, fired a step early by the walk
/// before; a scene's, set off by a fight or a use), whatever that goal was.
#[derive(Default)]
pub(super) struct Talks {
    /// The one under way at the last step.
    pub(super) under_way: Option<Talk>,
    /// Those ended since and not taken by a `talk`, oldest first.
    pub(super) ended: VecDeque<Talk>,
}

impl Talks {
    /// The step's conversation with the player, if any (`id`, its owner's
    /// number), at `now`: one gone since the last step has ended.
    pub(super) fn note(&mut self, now_talking: Option<(u32, Option<u32>)>, now: u64) {
        if self.under_way.as_ref().map(|talk| talk.id) == now_talking.map(|(id, _)| id) {
            return;
        }
        if let Some(mut gone) = self.under_way.take() {
            gone.ended_ms = now;
            self.ended.push_back(gone);
        }
        self.under_way = now_talking.map(|(id, owner)| Talk {
            id,
            owner,
            ended_ms: u64::MAX,
        });
    }

    /// Whether a `talk` is answered, nothing under way: by the one it saw
    /// (`seen`), once ended; else, having seen none, by one that ended
    /// since `since` (the goal before it began). Every conversation ended
    /// so far goes with it: a chain of them, one begun as the last ended, is
    /// one talk.
    pub(super) fn claim(&mut self, seen: Option<u32>, since: u64) -> bool {
        let answered = match seen {
            Some(id) => self.ended.iter().any(|talk| talk.id == id),
            None => self.ended.iter().any(|talk| talk.ended_ms >= since),
        };
        if answered {
            self.ended.clear();
        }
        answered
    }

    /// Whether object `owner`'s conversation ended since `since`, not yet
    /// taken by a `talk`: a use already answered (Bastila's after the swoop's
    /// fight). It stays for the `talk` after the use.
    pub(super) fn ended_with(&self, owner: u32, since: u64) -> bool {
        self.ended
            .iter()
            .any(|talk| talk.owner == Some(owner) && talk.ended_ms >= since)
    }
}

/// Whether the world stands settled for a condition to be read (`if`,
/// `while`, `until`): no conversation with the player, no module change
/// asked nor the party waiting at an exit, the screen clear
/// (`Fade::settled`), and no code the party's scripts, or those of the
/// player's last partner in a conversation, deferred due within `CUE_MS` (a
/// scene's last word, its module change: the Leviathan's lift moves to the
/// hangar a tenth of a second after its menu closes). Counted in steps in a
/// row (`SETTLED_STEPS`): a global written a few steps after a scene's line
/// sent the route down the wrong branch.
#[derive(Default)]
pub(super) struct Settle {
    pub(super) tick: u64,
    pub(super) steps: u32,
    /// The owner of the player's last conversation.
    pub(super) partner: Option<u32>,
}

impl Settle {
    /// The world as step `tick` left it: settled or not.
    pub(super) fn note(&mut self, tick: u64, settled: bool) {
        if tick != self.tick {
            self.tick = tick;
            self.steps = if settled { self.steps + 1 } else { 0 };
        }
    }

    pub(super) fn settled(&self) -> bool {
        self.steps >= SETTLED_STEPS
    }
}

/// Whether `snapshot` stands settled (`Settle`), the deferred code of
/// `watched` (the party's bodies, the last partner) looked at.
pub(super) fn settled(snapshot: &Snapshot, state: &sim::script::State, talking: bool, watched: &[u32]) -> bool {
    let now = snapshot.time_ms;
    !talking
        && snapshot.transition.is_none()
        && snapshot.ready.is_none()
        && snapshot.fade.is_none_or(|fade| fade.settled(now))
        && !watched.iter().any(|id| state.due_soon(*id, now, CUE_MS))
}

/// What a `while` round may change (`Goal::Again`): the module, the script
/// globals, and each creature's hit points outside the party. A round that
/// changed none of them goes round no more.
pub(super) type Round = (
    String,
    BTreeMap<String, i32>,
    BTreeMap<String, bool>,
    Vec<(sim::SimId, i32)>,
);

pub(super) fn round_of(world: &sim::SimWorld, snapshot: &Snapshot, module: String) -> Round {
    let state = world.script_state();
    let lives = snapshot
        .entities
        .iter()
        .filter(|entity| {
            matches!(entity.kind, sim::Kind::Object(ObjectKind::Creature, _))
                && !snapshot.in_party(entity)
        })
        .filter_map(|entity| Some((entity.id, entity.fighter?.hp)))
        .collect();
    (module, state.numbers.clone(), state.booleans.clone(), lives)
}
