//! What one fight's ticks add up to, side by side, read off the snapshots
//! (fighters' events, saber and duel state) and the blades' tap
//! (`saber::duel::tap`: what each blow meeting a blade decided). Nothing
//! here touches the world.

use std::collections::BTreeSet;

use serde::Serialize;
use sim::combat::{Fighter, effects};
use sim::combat::fighter::Outcome;
use sim::combat::saber::duel::exchange::{Guard, Took};
use sim::combat::saber::duel::meter;
use sim::combat::saber::duel::tap::Tapped;
use sim::combat::saber::throw::Flight;
use sim::motion::Anim;
use sim::{SimId, buttons};

/// How long after a lock ends or a kick lands its result is looked for.
const AFTER_MS: u64 = 1500;
const KICK_FLOORS_MS: u64 = 400;

/// The Force buttons a power is cast with.
const POWERS: u16 =
    buttons::FORCE_POWER | buttons::FORCE_GRIP | buttons::FORCE_LIGHTNING | buttons::FORCE_DRAIN;

/// One side's counts.
#[derive(Debug, Clone, Default, Serialize)]
pub(super) struct Side {
    pub hp_start: i32,
    pub hp_end: i32,
    pub hp_lost: i32,
    pub swings: u32,
    pub hits: u32,
    pub damage: i64,
    /// Hundredths it dealt the foe by other means (powers, scripts).
    pub other_damage: i64,
    /// Blows of its that a blade stopped.
    pub blows_stopped: u32,
    /// Blocks it made, by kind.
    pub blocks: Blocks,
    /// What it took from the exchanges (its own swing's or its guard's).
    pub took: Reactions,
    pub bolts_turned: u32,
    pub disarmed: u32,
    pub knockdowns: u32,
    pub kicks_thrown: u32,
    pub kicks_landed: u32,
    pub kicked_absorbed: u32,
    pub kicked_floored: u32,
    pub saber_breaks: u32,
    pub fatigued_ticks: u64,
    pub force_pct_sum: i64,
    pub points_sum: i64,
    pub power_casts: u32,
    /// Hundredths of the pool spent on ticks a power was cast or held.
    pub power_fp: i64,
    /// Speed's extra attacks a round (`effects::extra_attacks`), summed
    /// over the ticks.
    pub extra_attacks_sum: i64,
}

#[derive(Debug, Clone, Default, Serialize)]
pub(super) struct Blocks {
    /// A creature's own block (duel modes).
    pub creature: u32,
    /// Block held alone.
    pub held: u32,
    /// Block and attack held, the window closed.
    pub aimed: u32,
    pub perfect: u32,
    /// The blades met with no guard up (duel modes).
    pub unguarded: u32,
    /// JA: turned by state (the guard arc), and clashes either side swung.
    pub state: u32,
    pub clash: u32,
}

#[derive(Debug, Clone, Default, Serialize)]
pub(super) struct Reactions {
    pub bounce: u32,
    pub attack_bounce: u32,
    pub heavy_stagger: u32,
    pub stagger: u32,
    pub knockaway: u32,
    pub knocked_aside: u32,
    pub parry: u32,
    pub weary_parry: u32,
    pub disarm: u32,
    /// Blocks that took the blocker under 0 block points, so the blow went
    /// on (`exchange::resolve`).
    pub guard_break: u32,
}

impl Reactions {
    fn add(&mut self, took: Took) {
        let slot = match took {
            Took::Nothing => return,
            Took::Bounce(_) => &mut self.bounce,
            Took::AttackBounce => &mut self.attack_bounce,
            Took::HeavyStagger => &mut self.heavy_stagger,
            Took::Stagger => &mut self.stagger,
            Took::Knockaway => &mut self.knockaway,
            Took::KnockedAside => &mut self.knocked_aside,
            Took::Parry => &mut self.parry,
            Took::WearyParry => &mut self.weary_parry,
            Took::Disarm => &mut self.disarm,
        };
        *slot += 1;
    }
}

/// A lock's end: who won it and how.
#[derive(Debug, Clone, Serialize)]
pub(super) struct LockEnd {
    pub at_ms: u64,
    /// "a", "b" or "even": who floored, disarmed or killed the other, else
    /// who played a winner's break.
    pub winner: &'static str,
    /// "kill", "down", "disarm" or "none".
    pub result: &'static str,
}

/// What a side looked like last tick.
#[derive(Debug, Clone, Copy, Default)]
struct Was {
    attacking: bool,
    kicking: bool,
    floored: bool,
    thrown: bool,
    broken: bool,
    kicked: Option<(SimId, u64)>,
    pressed: bool,
    fp: i32,
    hp: i32,
}

/// A kick landed, its victim watched until `until`.
#[derive(Debug, Clone, Copy)]
struct Pending {
    victim: usize,
    until: u64,
    floored: bool,
}

/// A fight's tally.
pub(super) struct Tally {
    pub ids: [SimId; 2],
    pub sides: [Side; 2],
    pub locks: Vec<LockEnd>,
    pub ticks: u64,
    was: [Was; 2],
    seen: [BTreeSet<u16>; 2],
    locked: bool,
    /// A lock just ended: when, each side's fates then, and who played the
    /// winner's break.
    lock_ended: Option<(u64, [bool; 3], [bool; 3], &'static str)>,
    kicks: Vec<Pending>,
}

impl Tally {
    pub fn new(ids: [SimId; 2], fighters: [&Fighter; 2]) -> Tally {
        let was = fighters.map(|f| Was {
            fp: f.fp,
            hp: f.hp,
            ..Was::default()
        });
        let mut sides: [Side; 2] = Default::default();
        for (side, f) in sides.iter_mut().zip(fighters) {
            side.hp_start = f.hp;
        }
        Tally {
            ids,
            sides,
            locks: Vec::new(),
            ticks: 0,
            was,
            seen: Default::default(),
            locked: false,
            lock_ended: None,
            kicks: Vec::new(),
        }
    }

    fn side_of(&self, id: SimId) -> Option<usize> {
        self.ids.iter().position(|known| *known == id)
    }

    /// One tick at `now`: both fighters (side a first, each with its pool
    /// and its torso's animation), and what the blades' tap told of it.
    pub fn tick(
        &mut self,
        now: u64,
        fighters: [(&Fighter, u16, Anim); 2],
        tapped: &[Tapped],
    ) {
        self.ticks += 1;
        for (at, &(f, pool, _)) in fighters.iter().enumerate() {
            self.side(at, f, pool, now);
        }
        for tap in tapped {
            self.tap(*tap);
        }
        self.lock(now, [fighters[0].0, fighters[1].0], [fighters[0].2, fighters[1].2]);
        self.watch_kicks(now, [fighters[0].0, fighters[1].0]);
    }

    fn side(&mut self, at: usize, f: &Fighter, pool: u16, now: u64) {
        let other = 1 - at;
        let foe = self.ids[other];
        let was = self.was[at];
        let saber = &f.saber;
        let mv = saber.current_move;
        let attacking = mv.attack();
        let kicking = mv.kick();
        let thrown = saber.thrown.is_some_and(|t| t.flight == Flight::Falling);
        let side = &mut self.sides[at];
        side.swings += u32::from(attacking && !was.attacking);
        side.kicks_thrown += u32::from(kicking && !was.kicking);
        side.knockdowns += u32::from(saber.floored() && !was.floored);
        side.disarmed += u32::from(thrown && !was.thrown);
        side.saber_breaks += u32::from(saber.duel.broken && !was.broken);
        side.fatigued_ticks += u64::from(saber.duel.fatigued);
        side.force_pct_sum += i64::from(meter::force(f, pool));
        side.points_sum += i64::from(saber.duel.points);
        side.extra_attacks_sum += i64::from(effects::extra_attacks(&f.force, &f.effects, now));
        let casting = f.last_command.buttons & POWERS != 0 || f.force.channel.is_some();
        let pressed = f.last_command.buttons & buttons::FORCE_POWER != 0;
        side.power_casts += u32::from(pressed && !was.pressed);
        if casting && f.fp < was.fp {
            side.power_fp += i64::from(was.fp - f.fp);
        }
        side.hp_lost += (was.hp.max(0) - f.hp.max(0)).max(0);
        side.hp_end = f.hp;
        // Blows that reached it: each event once, by its sequence; a
        // weapon's are hits, the rest (powers, scripts) the foe's other harm.
        for event in f.events.iter().flatten() {
            if !self.seen[at].insert(event.sequence) {
                continue;
            }
            match event.result {
                Outcome::Hit | Outcome::Critical | Outcome::Automatic
                    if event.by == Some(foe) && event.damage > 0 =>
                {
                    let foe_side = &mut self.sides[other];
                    if event.weapon.is_some() {
                        foe_side.hits += 1;
                        foe_side.damage += i64::from(event.damage);
                    } else {
                        foe_side.other_damage += i64::from(event.damage);
                    }
                }
                Outcome::Deflected => self.sides[at].bolts_turned += 1,
                _ => {}
            }
        }
        if let Some(kicked) = saber.kicked
            && was.kicked != Some(kicked)
            && kicked.0 == foe
        {
            self.sides[at].kicks_landed += 1;
            self.kicks.push(Pending {
                victim: other,
                until: now + KICK_FLOORS_MS,
                floored: false,
            });
        }
        self.was[at] = Was {
            attacking,
            kicking,
            floored: saber.floored(),
            thrown,
            broken: saber.duel.broken,
            kicked: saber.kicked,
            pressed,
            fp: f.fp,
            hp: f.hp,
        };
    }

    fn tap(&mut self, tapped: Tapped) {
        match tapped {
            Tapped::Exchange {
                a,
                d,
                guard,
                took,
                stopped,
                ..
            } => {
                let (Some(a), Some(d)) = (self.side_of(a), self.side_of(d)) else {
                    return;
                };
                self.sides[a].took.add(took[0]);
                self.sides[d].took.add(took[1]);
                if !stopped {
                    // A guard break, and not a lunge past a tired blocker
                    // or an unblockable blow: those leave the blocker with
                    // a stagger, nothing, or a knockaway aside.
                    if guard != Guard::None && took[1] == Took::HeavyStagger {
                        self.sides[d].took.guard_break += 1;
                    }
                    return;
                }
                self.sides[a].blows_stopped += 1;
                let blocks = &mut self.sides[d].blocks;
                let slot = match guard {
                    Guard::Perfect => &mut blocks.perfect,
                    Guard::Active => &mut blocks.aimed,
                    Guard::Held => &mut blocks.held,
                    Guard::Npc => &mut blocks.creature,
                    Guard::None => &mut blocks.unguarded,
                };
                *slot += 1;
            }
            Tapped::Block { a, d, .. } => {
                if let (Some(a), Some(d)) = (self.side_of(a), self.side_of(d)) {
                    self.sides[a].blows_stopped += 1;
                    self.sides[d].blocks.state += 1;
                }
            }
            Tapped::Clash { a, d, .. } => {
                if let (Some(a), Some(d)) = (self.side_of(a), self.side_of(d)) {
                    self.sides[a].blows_stopped += 1;
                    self.sides[d].blocks.clash += 1;
                }
            }
        }
    }

    /// Each side's (killed, floored, disarmed) now.
    fn fates(f: &Fighter) -> [bool; 3] {
        [
            !f.alive(),
            f.saber.floored(),
            f.saber.thrown.is_some_and(|t| t.flight == Flight::Falling),
        ]
    }

    /// Locks: begun, ended, and what the next `AFTER_MS` made of the loser.
    fn lock(&mut self, now: u64, fighters: [&Fighter; 2], torsos: [Anim; 2]) {
        let locked = fighters[0].saber.lock.is_some();
        if self.locked && !locked {
            let breaker = match torsos.map(crate::duel_verbs::wins) {
                [true, false] => "a",
                [false, true] => "b",
                _ => "even",
            };
            let fates = fighters.map(Self::fates);
            self.lock_ended = Some((now, fates[0], fates[1], breaker));
        }
        self.locked = locked;
        let Some((ended, before_a, before_b, breaker)) = self.lock_ended else {
            return;
        };
        let (now_a, now_b) = (Self::fates(fighters[0]), Self::fates(fighters[1]));
        let fresh = |before: [bool; 3], now: [bool; 3]| (0..3).find(|&k| now[k] && !before[k]);
        let (lost_a, lost_b) = (fresh(before_a, now_a), fresh(before_b, now_b));
        let result = |k: usize| ["kill", "down", "disarm"][k];
        let end = match (lost_a, lost_b) {
            (None, Some(k)) => Some(("a", result(k))),
            (Some(k), None) => Some(("b", result(k))),
            _ if now >= ended + AFTER_MS || locked => Some((breaker, "none")),
            _ => None,
        };
        if let Some((winner, result)) = end {
            self.locks.push(LockEnd {
                at_ms: ended,
                winner,
                result,
            });
            self.lock_ended = None;
        }
    }

    fn watch_kicks(&mut self, now: u64, fighters: [&Fighter; 2]) {
        let sides = &mut self.sides;
        self.kicks.retain_mut(|kick| {
            kick.floored |= fighters[kick.victim].saber.floored();
            if now < kick.until && !kick.floored {
                return true;
            }
            let side = &mut sides[kick.victim];
            if kick.floored {
                side.kicked_floored += 1;
            } else {
                side.kicked_absorbed += 1;
            }
            false
        });
    }

    /// Ends the fight: locks and kicks still watched are closed as they
    /// stand.
    pub fn finish(&mut self) {
        if let Some((ended, _, _, breaker)) = self.lock_ended.take() {
            self.locks.push(LockEnd {
                at_ms: ended,
                winner: breaker,
                result: "none",
            });
        }
        for kick in self.kicks.drain(..) {
            let side = &mut self.sides[kick.victim];
            if kick.floored {
                side.kicked_floored += 1;
            } else {
                side.kicked_absorbed += 1;
            }
        }
    }
}
