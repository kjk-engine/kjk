//! What a blow meeting a blade decides under a duel mode
//! (`docs/SABER-RULES.md` 3 to 5): how the blocker blocks (`Guard`), what
//! each side plays (`Took`) and pays, how their mishaps move, whether the
//! blow is stopped, and whether a lock is forced. The bounce ladder
//! (`ladder`) and the mishap roll (`mishap`) are here too. A pure decision:
//! the world applies it (`world::sabers`). Its rolls are drawn from the
//! stream given, in a fixed order.

use crate::combat::saber::clash::roll_between;
use crate::rng::SimRng;
use rules::duel::block;
use rules::duel::gains;
use rules::duel::ladder;
use rules::duel::perfect;
use rules::duel::roll;

use super::SaberRules;

/// How a blocker blocks, in the order the decision tests them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Guard {
    /// Block and attack held, the perfect window open.
    Perfect,
    /// Block and attack held, the window closed.
    Active,
    /// Block held alone.
    Held,
    /// A creature's own block (creatures always block when they can).
    Npc,
    /// Standing ready, not blocking.
    None,
}

/// What a side plays after the exchange.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Took {
    #[default]
    Nothing,
    /// Its swing bounces off (blocked or parried), at the pace given.
    Bounce(super::Pace),
    /// An attack's bounce, one of the style's (both bounced).
    AttackBounce,
    /// A heavy stagger (an attacker's).
    HeavyStagger,
    /// A stagger (a blocker's).
    Stagger,
    /// The perfect parry: a knockaway where the blow came in.
    Knockaway,
    /// Knocked aside: a knockaway of the fatigued.
    KnockedAside,
    /// A parry where the blow came in.
    Parry,
    /// A weary parry: the blocker spent, or the blow heavy.
    WearyParry,
    /// The saber knocked out of the hand.
    Disarm,
}

/// One side of an exchange, as the decision reads it.
#[derive(Debug, Clone, Copy)]
pub struct Side {
    pub npc: bool,
    /// How far toward a master it is (`duel::mastery`).
    pub mastery: u8,
    pub running: bool,
    /// The meter (`meter::meter`) and the block points, both in points.
    pub meter: i32,
    pub points: i32,
    pub mishap: u8,
    /// Swinging a heavy style (`duel::heavy`).
    pub heavy: bool,
    /// Its swing's power (`power::attack`).
    pub power: i32,
    pub attacking: bool,
    pub faking: bool,
    pub kata: bool,
    /// Its move cannot be blocked but perfectly (`duel::unblockable`).
    pub unblockable: bool,
    /// A lunge or a spin (`duel::lunge_or_spin`).
    pub lunge: bool,
    /// Its swing is a lock's super break won.
    pub super_break: bool,
    /// Fatigued or drained (`Duel::weary`).
    pub weary: bool,
    pub guard: Guard,
}

/// What the exchange decided, attacker first in each pair.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Exchange {
    /// The blow is stopped: it harms no one.
    pub stopped: bool,
    /// A lock is forced (a fake that broke a guard).
    pub forced_lock: bool,
    /// A perfect block (`Duel::perfect_at`), and MDA's riposte with it.
    pub perfect: bool,
    pub riposte: bool,
    pub took: [Took; 2],
    /// Each side's meter moved (paid when negative).
    pub meter: [i32; 2],
    /// Each side's block points moved (the katas' clash reads and pays
    /// them in both modes).
    pub points: [i32; 2],
    /// Each side's Force moved (a parried fake's reward, in both modes).
    pub force: [i32; 2],
    /// Each side's mishaps moved, running's factor taken (`meter::shift`
    /// is given it).
    pub mishap: [i32; 2],
}

/// A chance of `percent` in 100.
fn chance(rng: &mut SimRng, percent: i32) -> bool {
    roll_between(rng, 1, 100) <= percent
}

/// Whether a lock is tried by chance on two blades meeting
/// (`docs/SABER-RULES.md` 8.1): both attacking with equal power, one
/// attacking and the other not of equal power, or a fake on either side;
/// never with a special or an unblockable blow.
pub fn lock_chance(a: &Side, d: &Side) -> bool {
    if a.kata || d.kata || a.unblockable || d.unblockable {
        return false;
    }
    let fake = a.faking || d.faking;
    let both = a.attacking && d.attacking;
    let one = a.attacking != d.attacking;
    fake || (both && a.power == d.power) || (one && a.power != d.power)
}

/// The mishap roll (`docs/SABER-RULES.md` 5.3): whether a roll of one to
/// `roll::SIDES` falls at or under the count.
pub fn mishap(rng: &mut SimRng, count: i32) -> bool {
    roll_between(rng, 1, roll::SIDES) <= count
}

/// An attacker's mishap: half a heavy stagger, half a disarm, a disarm
/// spared a creature (`spared`) `roll::CREATURE_SPARED`% of the time.
fn attacker_mishap(rng: &mut SimRng, spared: bool) -> Took {
    if !chance(rng, roll::ATTACKER_DISARM) {
        return Took::HeavyStagger;
    }
    if spared && chance(rng, roll::CREATURE_SPARED) {
        Took::HeavyStagger
    } else {
        Took::Disarm
    }
}

/// A blocker's mishap: half a stagger; else a creature staggered a quarter
/// of the time and disarmed the rest, a player disarmed.
fn blocker_mishap(rng: &mut SimRng, npc: bool) -> Took {
    let [stagger, npc_stagger] = roll::BLOCKER_STAGGER;
    if chance(rng, stagger) || (npc && chance(rng, npc_stagger)) {
        Took::Stagger
    } else {
        Took::Disarm
    }
}

/// The bounce ladder (`docs/SABER-RULES.md` 5.2): what an attacker whose
/// swing was blocked (not perfectly) plays by its mishaps `count`, from
/// `bounce` (the blocked bounce it would play) up; and where a creature's
/// count falls to.
pub fn ladder(
    rng: &mut SimRng,
    npc: bool,
    count: u8,
    bounce: Took,
    both: bool,
) -> (Took, Option<u8>) {
    let [small, heavy, top] = ladder::RUNGS;
    let [heavy_reset, top_reset] = ladder::CREATURE_RESET;
    match count {
        c if c >= top && npc => {
            let took = if chance(rng, ladder::CREATURE_TOP_ROLL) {
                attacker_mishap(rng, true)
            } else {
                Took::HeavyStagger
            };
            (took, Some(top_reset))
        }
        c if c >= top => (attacker_mishap(rng, false), None),
        c if c >= heavy && npc => (Took::Bounce(super::Pace::Blocked), Some(heavy_reset)),
        c if c >= heavy => (Took::HeavyStagger, None),
        c if c >= small => (Took::Bounce(super::Pace::Blocked), None),
        _ if both => (Took::AttackBounce, None),
        _ => (bounce, None),
    }
}

/// The decision's running state.
struct Deal<'a> {
    rules: SaberRules,
    a: &'a Side,
    d: &'a Side,
    x: Exchange,
}

impl Deal<'_> {
    /// Mishaps moved on side `at` (0 the attacker), running's factor taken.
    fn mishaps(&mut self, at: usize, by: i32) {
        let running = [self.a.running, self.d.running][at];
        let by = match (by > 0, running) {
            (true, true) => by * rules::duel::mishap::RUN_RISE,
            (false, true) => by / rules::duel::mishap::RUN_RISE,
            _ => by,
        };
        self.x.mishap[at] += by;
    }

    /// The count side `at` has after the exchange's moves so far.
    fn count(&self, at: usize) -> u8 {
        let before = [self.a.mishap, self.d.mishap][at];
        (i32::from(before) + self.x.mishap[at]).clamp(0, i32::from(rules::duel::mishap::MAX)) as u8
    }

    /// The blocker pays `cost` (its master's price).
    fn blocker_pays(&mut self, cost: i32) {
        self.x.meter[1] -= super::meter::price(self.rules, self.d.mastery, cost, false);
    }

    /// A perfect block's rewards (`docs/SABER-RULES.md` 3), for a blow
    /// blockable or not.
    fn perfect(&mut self) {
        let who = usize::from(self.a.npc);
        self.x.perfect = true;
        self.x.riposte = self.rules.own_meter();
        self.x.stopped = true;
        self.x.meter[1] += gains::PERFECT;
        self.mishaps(1, -perfect::BLOCKER_MISHAP);
        self.x.meter[0] -= perfect::ATTACKER_COST[who];
        self.mishaps(0, perfect::ATTACKER_MISHAP[who]);
    }

    /// An unblockable blow (`docs/SABER-RULES.md` 4.2, 4.3 1): a perfect
    /// block disarms its attacker; anything else is knocked aside and
    /// harmed, or disarmed when spent.
    fn unblockable(&mut self) {
        if self.d.guard == Guard::Perfect {
            self.perfect();
            self.x.took[0] = Took::Disarm;
            self.x.took[1] = Took::Knockaway;
            return;
        }
        if self.d.meter < block::LOWEST {
            self.x.took[1] = Took::Disarm;
            return;
        }
        self.x.took[1] = Took::KnockedAside;
        let [cost, npc_md] = block::UNBLOCKABLE;
        let cost = if self.d.npc && !self.rules.own_meter() {
            npc_md
        } else {
            cost
        };
        self.blocker_pays(cost);
    }

    /// Two swings meeting (`docs/SABER-RULES.md` 4.4).
    fn both_swing(&mut self, rng: &mut SimRng) {
        let (a, d) = (self.a, self.d);
        let blocked = rules::duel::mishap::BLOCKED;
        match (a.faking, d.faking) {
            (true, false) => {
                self.mishaps(0, blocked);
                self.mishaps(1, -blocked);
            }
            (false, true) => {
                self.mishaps(0, -blocked);
                self.mishaps(1, blocked);
            }
            (true, true) => {
                self.mishaps(0, blocked);
                self.mishaps(1, blocked);
            }
            (false, false) if a.kata || d.kata => {
                let (winner, loser) = if a.kata { (0, 1) } else { (1, 0) };
                self.mishaps(winner, blocked);
                self.mishaps(loser, -blocked);
                let points = [a.points, d.points][loser];
                if points < block::LOWEST {
                    self.x.took[loser] = Took::Disarm;
                } else {
                    self.x.took[loser] = Took::Stagger;
                    // A blocker's price (the attacker pays it whole).
                    let mastery = if loser == 1 { d.mastery } else { 0 };
                    let cost = super::meter::price(self.rules, mastery, block::KATA, false);
                    self.x.points[loser] -= cost;
                }
                self.x.stopped = true;
            }
            (false, false) => {
                self.mishaps(0, blocked);
                self.mishaps(1, blocked);
                for at in 0..2 {
                    let npc = [a.npc, d.npc][at];
                    let (took, reset) = ladder(rng, npc, self.count(at), Took::AttackBounce, true);
                    self.x.took[at] = took;
                    if let Some(reset) = reset {
                        self.x.mishap[at] = i32::from(reset) - i32::from([a.mishap, d.mishap][at]);
                    }
                }
                self.x.stopped = true;
            }
        }
        if a.faking || d.faking {
            self.x.stopped = true;
        }
    }

    /// The blocker's side of a blockable blow (`docs/SABER-RULES.md` 4.1).
    /// Whether it blocked (else it was too spent and took what it took).
    fn blocker(&mut self, rng: &mut SimRng) -> bool {
        let d = self.d;
        let (md, who) = (!self.rules.own_meter(), usize::from(d.npc));
        if d.meter <= block::LOW {
            if d.meter <= block::LOWEST {
                self.x.took[1] = if d.npc {
                    if mishap(rng, i32::from(self.count(1))) {
                        blocker_mishap(rng, true)
                    } else {
                        Took::KnockedAside
                    }
                } else {
                    Took::Disarm
                };
                if self.x.took[1] != Took::Disarm {
                    self.x.meter[1] += gains::DISARMED;
                }
            } else {
                self.x.took[1] = Took::KnockedAside;
                self.blocker_pays(block::LOW_COST);
            }
            self.x.stopped = true;
            return false;
        }
        let weary = d.meter <= block::WEARY_PARRY || self.a.heavy;
        let cost = match d.guard {
            Guard::Perfect => {
                self.x.took[1] = Took::Knockaway;
                return true;
            }
            Guard::Active => {
                if self.a.npc {
                    self.mishaps(0, rules::duel::mishap::CREATURE_ACTIVE);
                }
                match md {
                    true => block::MD_HELD[who],
                    false => block::MDA_HELD[0],
                }
            }
            Guard::Held => {
                if self.a.npc {
                    let [low, high] = gains::SPAM;
                    self.x.meter[0] += if self.a.meter <= gains::SPAM_LOW {
                        low
                    } else {
                        high
                    };
                }
                match md {
                    true => block::MD_HELD[who],
                    false => block::MDA_HELD[who],
                }
            }
            Guard::Npc => {
                let [weary_to, plain_to] = block::CREATURE_BANDS;
                let band = match d.meter {
                    _ if self.a.heavy => None,
                    m if m <= weary_to => Some(0),
                    m if m <= plain_to => Some(1),
                    _ => Some(2),
                };
                if band.is_none_or(|band| band == 0) {
                    self.x.took[1] = Took::WearyParry;
                }
                match band {
                    None => 0,
                    Some(2) if !d.npc => self.rules.of(block::PLAYER_GOOD),
                    Some(band) if md => block::MD_CREATURE_PARRY[band],
                    Some(band) => block::MDA_CREATURE_PARRY[band],
                }
            }
            Guard::None => unreachable!("a blocker blocks"),
        };
        if self.x.took[1] == Took::Nothing {
            self.x.took[1] = if weary && d.guard != Guard::Npc {
                Took::WearyParry
            } else {
                Took::Parry
            };
        }
        self.blocker_pays(cost);
        true
    }

    /// A plain swing against a block (`docs/SABER-RULES.md` 4.3 5).
    fn blocked(&mut self, rng: &mut SimRng) {
        let (a, d) = (self.a, self.d);
        self.x.stopped = true;
        if !self.blocker(rng) {
            self.mishaps(0, rules::duel::mishap::BLOCKED);
            return;
        }
        if d.guard == Guard::Perfect {
            self.perfect();
            self.x.took[0] = if self.count(0) >= perfect::ROLL_FROM {
                if mishap(rng, i32::from(self.count(0))) {
                    attacker_mishap(rng, a.npc)
                } else {
                    Took::HeavyStagger
                }
            } else {
                Took::HeavyStagger
            };
            return;
        }
        self.mishaps(0, rules::duel::mishap::BLOCKED);
        self.mishaps(1, -rules::duel::mishap::BLOCK);
        let [meter_from, mishap_from, odds] = block::CREATURE_HEAVY;
        let bounce = match d.guard {
            Guard::Held => Took::Bounce(super::Pace::Parried),
            _ if d.npc
                && d.meter >= meter_from
                && i32::from(a.mishap) >= mishap_from
                && chance(rng, odds) =>
            {
                self.x.took[0] = Took::HeavyStagger;
                return;
            }
            _ => Took::Bounce(super::Pace::Blocked),
        };
        let (took, reset) = ladder(rng, a.npc, self.count(0), bounce, false);
        self.x.took[0] = took;
        if let Some(reset) = reset {
            self.x.mishap[0] = i32::from(reset) - i32::from(a.mishap);
        }
    }

    /// A fake met (`docs/SABER-RULES.md` 4.3 4): parried by a perfect or a
    /// creature's block, or breaking the guard into a lock.
    fn fake(&mut self, rng: &mut SimRng) {
        if matches!(self.d.guard, Guard::Perfect | Guard::Npc) {
            let [gained, lost] = rules::duel::mishap::FAKE_PARRIED;
            self.mishaps(0, gained);
            self.x.took[0] = if mishap(rng, i32::from(self.count(0))) {
                attacker_mishap(rng, self.d.npc)
            } else {
                Took::Bounce(super::Pace::Parried)
            };
            self.mishaps(1, -lost);
            self.x.force[1] += gains::FAKE_PARRIED;
            self.x.took[1] = Took::Parry;
            self.x.stopped = true;
        } else {
            self.mishaps(0, -rules::duel::mishap::GUARD_BROKEN);
            self.x.forced_lock = true;
        }
    }

    /// No block (`docs/SABER-RULES.md` 4.1, 4.3 6): the blow goes on; the
    /// blocker pays and rolls; the attacker bounces as both do, unless
    /// the blocker's own blow was unblockable, which staggers it.
    fn unguarded(&mut self, rng: &mut SimRng) {
        let (a, d) = (self.a, self.d);
        if !(self.rules.own_meter() && d.npc) {
            self.blocker_pays(block::UNBLOCKED);
        }
        self.x.took[1] = if mishap(rng, i32::from(self.count(1))) {
            blocker_mishap(rng, d.npc)
        } else {
            Took::Stagger
        };
        if d.unblockable {
            self.x.took[0] = Took::HeavyStagger;
            self.mishaps(1, -rules::duel::mishap::BLOCK);
            return;
        }
        self.mishaps(0, rules::duel::mishap::BLOCKED);
        let (took, reset) = ladder(rng, a.npc, self.count(0), Took::AttackBounce, true);
        self.x.took[0] = took;
        if let Some(reset) = reset {
            self.x.mishap[0] = i32::from(reset) - i32::from(a.mishap);
        }
    }
}

/// What `a`'s blow meeting `d`'s blade decides (`docs/SABER-RULES.md` 4):
/// an unblockable blow; two swings (`met`, both attacking); a super
/// break's win blocked; a fake; a plain blow against a block; or no
/// block. Lunges and spins pass a weary blocker unblocked.
pub fn resolve(rules: SaberRules, a: &Side, d: &Side, met: bool, rng: &mut SimRng) -> Exchange {
    let mut deal = Deal {
        rules,
        a,
        d,
        x: Exchange::default(),
    };
    let unguarded = d.guard == Guard::None || (a.lunge && d.weary);
    if a.unblockable {
        deal.unblockable();
    } else if met && d.attacking && d.guard != Guard::Npc {
        deal.both_swing(rng);
    } else if a.super_break && !unguarded {
        deal.mishaps(0, rules::duel::mishap::BLOCKED);
        deal.mishaps(1, -rules::duel::mishap::BLOCK);
        deal.x.took[0] = Took::HeavyStagger;
        deal.x.stopped = true;
    } else if a.faking {
        deal.fake(rng);
    } else if unguarded {
        deal.unguarded(rng);
    } else {
        deal.blocked(rng);
    }
    deal.x
}
