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
    /// Not one kept out of a blade's swing while another that fits stands
    /// within it of the player at `me`: that one is struck first, as a
    /// player turns on what claws at them (Malachor's surface: a storm beast
    /// at 0.8 m attacked the player for 116 s while the walk made for
    /// another 17 m off, stalled against the first).
    pub(super) fn pick(
        &self,
        me: [f32; 3],
        foes: Vec<(sim::SimId, [f32; 3])>,
        mut fits: impl FnMut(&(sim::SimId, [f32; 3])) -> bool,
    ) -> Option<(sim::SimId, [f32; 3])> {
        let at_hand = |(_, at): &(sim::SimId, [f32; 3])| flat(*at, me) <= melee_m().1;
        let kept = self
            .foe
            .and_then(|id| foes.iter().find(|(foe, _)| *foe == id).copied())
            .filter(&mut fits);
        let nearest = foes.into_iter().find(fits);
        match (kept, nearest) {
            (Some(kept), Some(near)) if !at_hand(&kept) && at_hand(&near) => Some(near),
            (Some(kept), _) => Some(kept),
            (None, near) => near,
        }
    }

    /// Whether foe `id` is given up: the walk to it stalled on and on
    /// (`lost`) and the player does not see it, as a player leaves a beast
    /// that wandered into scenery the floor runs on under (KOTOR's walker
    /// passes through what is drawn) and stands hidden there, not fighting:
    /// no gun reaches it either. Seen again, it is fought again. (Dxun's
    /// turret ramp, 410DXN: a boma, wandering, stood inside a drawn rock
    /// past a wall; the walk to it stalled for 90 s.)
    pub(super) fn given_up(&self, here: &Here, player: &sim::motion::MotionState, id: sim::SimId) -> bool {
        self.lost == Some(id) && !here.sees(player, id)
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
        // Fists (and a baton, which punches as they do) reach shorter
        // than a blade: stepped in to the sim's own punch (`strikes`).
        let punching = me.sheet.is_some_and(|sheet| {
            matches!(sheet.main_weapon().delivery, sim::combat::sheet::Delivery::Unarmed)
        });
        let (close, swing) = if punching {
            let punch = sim::player_punch_reach_m();
            (punch * PUNCH_IN, punch)
        } else {
            melee_m()
        };
        let within = match (ranged, far_shot) {
            (true, true) => GUN_M,
            (true, false) => SHOOT_M,
            (false, _) => swing,
        };
        // A punch is measured to the body where the sim's blow meets it, a
        // beast's or a droid's round wider than a humanoid's
        // (`player_punch_apart_m`): a boma's held the player past the punch.
        let apart = (punching && !ranged)
            .then(|| here.snapshot.get(id))
            .flatten()
            .and_then(|entity| {
                let facts = here.facts_of(entity);
                let foe = entity.player?;
                Some(sim::player_punch_apart_m(player.origin, &foe, sim::bulk_of(facts.as_deref())))
            })
            .unwrap_or(d);
        let at_hand = apart <= within && seen;
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
                ..aiming(player, me, strike_at(here, Some(id), at))
            };
            if !ranged && apart > close {
                command.forward = 127;
            }
            // A body standing in the line, short of a foe left unhurt a
            // while, takes the blow or the bolt: the player steps round it
            // while striking, one way then the other (Bao-Dur's remote
            // fighting the Elite Sith Assassin in the Trayus Academy's
            // doorway, the Exile walked in behind it by the scene). Not at
            // once: stepping round every companion in the line moved Nar
            // Shaddaa's refugee-ship fight, and the scene after it.
            let unhurt_a_while = self
                .unhurt
                .is_some_and(|(foe, _, since)| foe == id && now >= since + IN_LINE_MS);
            let in_line = unhurt_a_while
                && here.snapshot.entities.iter().any(|entity| {
                entity.id != id
                    && entity.id != me.id
                    && matches!(entity.kind, sim::Kind::Object(ObjectKind::Creature, _))
                    && entity.fighter.is_some_and(|fighter| fighter.hp > 0)
                    && flat(entity.position, me.position) < d
                    && off_the_way(me.position, at, entity.position) <= IN_LINE_M
            });
            if in_line {
                let turn = (now / (sim::walk::SIDESTEP_MS * 2)).is_multiple_of(2);
                command.right = if turn { 127 } else { -127 };
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
            return (w.command_eager(here.level, player, now, &|index| here.to_try(index)), true);
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

/// How near the line to a foe (metres) another body stands to take a blow
/// meant for it: about a body's half width.
const IN_LINE_M: f32 = 0.4;
/// How long (ms) a foe at hand stands unhurt before the player steps round
/// a body in the line to it.
const IN_LINE_MS: u64 = 2_000;

/// How far into a punch's reach (its share) the chase steps before it
/// stops pressing on: a little inside, as a foe sways back.
const PUNCH_IN: f32 = 0.8;

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

/// The triggers whose own scene the player set off this visit of a module:
/// a conversation the run of a trigger's `OnEnter` for the player's body
/// asked for, as the sim reports it (`Line::Cued`), and when. A `to` such a
/// trigger, its scene set off during a goal before it, is met, and the
/// `talk` after it takes that scene: Onderon's throne room, where the
/// fight walked the player into `tr_kavar_enter` and Kavar's entrance and
/// Vaklu's fall played out, and the `to` after the fight walked back for
/// the trigger into the one that keeps the player in the hall
/// (`tr_end_riiken`), jumped back again and again for 900 s. Another body
/// entering (Telos's salvager reaching `tr_czerka_escape`) is not the
/// player's scene.
#[derive(Default)]
pub(super) struct Scenes {
    module: String,
    /// How many of the report's lines have been read.
    read: usize,
    /// By trigger number: when its scene began, and that conversation.
    played: BTreeMap<u32, (u64, Talk)>,
}

impl Scenes {
    /// The step: the player's body `me` in `module`, the scripts' report
    /// `lines` so far, `trigger_of` a script object's trigger number.
    pub(super) fn note(
        &mut self,
        module: &str,
        lines: &[Line],
        me: sim::SimId,
        trigger_of: impl Fn(ObjectId) -> Option<u32>,
        now: u64,
    ) {
        if self.module != module {
            self.module = module.to_string();
            self.played.clear();
        }
        if self.read > lines.len() {
            self.read = 0;
        }
        for line in &lines[self.read..] {
            if let Line::Cued { id, cue, .. } = line
                && cue.entering.0 == me.0
                && let Some(trigger) = trigger_of(cue.trigger)
            {
                let talk = Talk {
                    id: *id,
                    owner: None,
                    ended_ms: now,
                };
                self.played.entry(trigger).or_insert((now, talk));
            }
        }
        self.read = lines.len();
    }

    /// The scene of trigger `index` set off before `since` (the `to`'s
    /// start), if any. Taken either way: one set off since is the `to`'s
    /// own, met by stepping in; a later `to` the same trigger wants another.
    pub(super) fn take_before(&mut self, index: u32, since: u64) -> Option<Talk> {
        let (at, talk) = self.played.remove(&index)?;
        (at < since).then_some(talk)
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
/// globals and objects' locals (a pazaak player's count of games won), the
/// party's credits (a game lost), and each creature's hit points outside
/// the party. A round that
/// changed none of them goes round no more.
pub(super) type Round = (
    String,
    BTreeMap<String, i32>,
    BTreeMap<String, bool>,
    BTreeMap<(u32, u8, i32), i32>,
    i32,
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
    (
        module,
        state.numbers.clone(),
        state.booleans.clone(),
        state.locals.clone(),
        state.gold,
        lives,
    )
}
