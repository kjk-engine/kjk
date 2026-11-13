//! What a blow meeting a blade decides under a duel mode
//! (`docs/SABER-RULES.md` 3 to 5): how the blocker blocks (`Guard`), what
//! each side plays (`Took`) and what it pays in block points, whether the
//! blow is stopped, and whether a lock is forced. Block points alone decide
//! the bounce a blocked attacker plays (`bounce`), a guard break and a
//! disarm. No exchange gives block points to the attacker: the only reward
//! here is the blocker's for a perfect block. A pure decision, and a
//! decision without dice: the world applies it (`world::sabers`), and
//! nothing here reads a random number.

use rules::duel::block;
use rules::duel::gains;
use rules::duel::meters;
use rules::duel::perfect;

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
    /// A heavy stagger (an attacker's, and a broken guard's).
    HeavyStagger,
    /// A stagger (a blocker's).
    Stagger,
    /// The perfect parry: a knockaway where the blow came in.
    Knockaway,
    /// Knocked aside: a knockaway of the tired.
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
    /// Its block points.
    pub points: i32,
    /// KOTOR's chance that this side's swing hits the other side, in
    /// thousandths (500 is a chance of one in two). Only the attacking
    /// side's is read, in `Deal::blocker_pays`: a blocking side carries
    /// `Side::NO_ODDS`, so the world draws nothing for it.
    pub chance: i64,
    /// This side's swing is a KOTOR critical hit. Read where `chance` is.
    pub critical: bool,
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
    pub guard: Guard,
}

impl Side {
    /// What a side whose swing no rule weighs carries in `chance` and
    /// `critical`: no chance to hit and no critical hit. A blocking side
    /// is given this, so that the world draws no d20 for it.
    pub const NO_ODDS: (i64, bool) = (0, false);
}

/// What the exchange decided, attacker first in each pair.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Exchange {
    /// The blow is stopped: it harms no one.
    pub stopped: bool,
    /// A lock is forced (a fake that broke a guard).
    pub forced_lock: bool,
    /// A perfect block (`Duel::perfect_at`), and the riposte with it.
    pub perfect: bool,
    pub riposte: bool,
    /// A creature's good parry (block points above the plain band), which
    /// plays a perfect parry's animation.
    pub good: bool,
    pub took: [Took; 2],
    /// Each side's block points moved (paid when negative).
    pub points: [i32; 2],
    /// Each side's Force moved (a parried fake's reward, in both modes).
    pub force: [i32; 2],
}

/// Whether a fighter with `points` block points is tired
/// (`meters::TIRED`). A tired fighter cannot parry a blow and cannot block
/// a lunge or a spin.
pub fn tired(points: i32) -> bool {
    points <= meters::TIRED
}

/// What a block of `base` block points really costs its blocker: the base
/// raised by the attacker's `chance` to hit (half the base at a chance of
/// nothing, the whole base at one in two, one and a half at a certainty),
/// never less than one point, and multiplied by `block::CRITICAL_TIMES`
/// when the swing is a critical hit. Integer division, so a fraction of a
/// point is dropped.
pub fn blocked_cost(base: i32, chance: i64, critical: bool) -> i32 {
    if base <= 0 {
        return base;
    }
    let cost = (i64::from(base) * (500 + chance) / 1000).max(1) as i32;
    if critical {
        cost * block::CRITICAL_TIMES
    } else {
        cost
    }
}

/// What an attacker whose swing was blocked (not perfectly) plays, by the
/// `points` it has left once the swing and the block are paid for
/// (`block::BOUNCE_BANDS`): a heavy stagger when it is nearly spent, the
/// small bounce in the middle band, and `plain` (the bounce its own
/// exchange would play) above both.
pub fn bounce(points: i32, plain: Took) -> Took {
    let [staggers, small] = block::BOUNCE_BANDS;
    match points {
        points if points < staggers => Took::HeavyStagger,
        points if points < small => Took::Bounce(super::Pace::Blocked),
        _ => plain,
    }
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

/// How a blocker's guard answered a blockable blow.
enum Guarded {
    /// It parried the blow, or blocked it perfectly.
    Parried,
    /// Too tired to parry: the blow is stopped and knocks it aside.
    TooTired,
    /// Its guard broke: the blow goes on.
    Broken,
}

/// The decision's running state.
struct Deal<'a> {
    a: &'a Side,
    d: &'a Side,
    x: Exchange,
}

impl Deal<'_> {
    /// The blocker pays for the block: `base` raised by the attacker's
    /// chance and its critical hit (`blocked_cost`). Its guard breaks when
    /// that would take it under 0 points: the blow is not stopped, the
    /// blocker is heavily staggered, and it is left with
    /// `meters::BROKEN_TO` points, above the tired mark, so that the very
    /// next swing does not break it again.
    fn blocker_pays(&mut self, base: i32) -> Guarded {
        let cost = blocked_cost(base, self.a.chance, self.a.critical);
        if self.d.points - cost < 0 {
            self.x.stopped = false;
            self.x.took[1] = Took::HeavyStagger;
            self.x.points[1] = meters::BROKEN_TO - self.d.points;
            return Guarded::Broken;
        }
        self.x.points[1] -= cost;
        Guarded::Parried
    }

    /// A perfect block's rewards (`docs/SABER-RULES.md` 3), for a blow
    /// blockable or not: the blocker gains `gains::PERFECT` points and the
    /// attacker loses `perfect::ATTACKER_COST`.
    fn perfect(&mut self) {
        self.x.perfect = true;
        self.x.riposte = true;
        self.x.stopped = true;
        self.x.points[1] += gains::PERFECT;
        self.x.points[0] -= perfect::ATTACKER_COST;
    }

    /// An unblockable blow (`docs/SABER-RULES.md` 4.2, 4.3 1): a perfect
    /// block disarms its attacker; anything else is knocked aside and
    /// harmed.
    fn unblockable(&mut self) {
        if self.d.guard == Guard::Perfect {
            self.perfect();
            self.x.took[0] = Took::Disarm;
            self.x.took[1] = Took::Knockaway;
            return;
        }
        self.x.took[1] = Took::KnockedAside;
        self.x.points[1] -= block::UNBLOCKABLE;
    }

    /// Two swings meeting (`docs/SABER-RULES.md` 4.4): a special attack
    /// staggers the ordinary swing it meets, or disarms a fighter nearly
    /// spent (`block::KATA_DISARM_UNDER`); a fake on either side simply
    /// stops the blow; two plain swings both bounce by their own points.
    fn both_swing(&mut self) {
        let (a, d) = (self.a, self.d);
        self.x.stopped = true;
        if a.faking || d.faking {
            return;
        }
        if a.kata || d.kata {
            let loser = usize::from(a.kata);
            let points = [a.points, d.points][loser];
            if points < block::KATA_DISARM_UNDER {
                self.x.took[loser] = Took::Disarm;
            } else {
                self.x.took[loser] = Took::Stagger;
                self.x.points[loser] -= block::KATA;
            }
            return;
        }
        for (at, points) in [a.points, d.points].into_iter().enumerate() {
            self.x.points[at] -= block::BLOCKED;
            self.x.took[at] = bounce(points - block::BLOCKED, Took::AttackBounce);
        }
    }

    /// The blocker's side of a blockable blow (`docs/SABER-RULES.md` 4.1):
    /// what its block costs it, which parry it plays, and how its guard
    /// answered.
    fn blocker(&mut self) -> Guarded {
        let d = self.d;
        let who = usize::from(d.npc);
        if d.guard == Guard::Perfect {
            self.x.took[1] = Took::Knockaway;
            return Guarded::Parried;
        }
        if tired(d.points) {
            self.x.took[1] = Took::KnockedAside;
            return match self.blocker_pays(block::LOW_COST) {
                Guarded::Broken => Guarded::Broken,
                _ => Guarded::TooTired,
            };
        }
        let weary = d.points <= block::WEARY_PARRY || self.a.heavy;
        let cost = match d.guard {
            Guard::Active => block::HELD[0],
            Guard::Held => block::HELD[who],
            Guard::Npc => {
                let [weary_to, plain_to] = block::CREATURE_BANDS;
                let band = match d.points {
                    _ if self.a.heavy => None,
                    points if points <= weary_to => Some(0),
                    points if points <= plain_to => Some(1),
                    _ => Some(2),
                };
                if band.is_none_or(|band| band == 0) {
                    self.x.took[1] = Took::WearyParry;
                }
                self.x.good = band == Some(2) && d.npc;
                match band {
                    None => 0,
                    Some(2) if !d.npc => block::PLAYER_GOOD,
                    Some(band) => block::CREATURE_PARRY[band],
                }
            }
            Guard::Perfect | Guard::None => unreachable!("a blocker blocks"),
        };
        if self.x.took[1] == Took::Nothing {
            self.x.took[1] = if weary && d.guard != Guard::Npc {
                Took::WearyParry
            } else {
                Took::Parry
            };
        }
        self.blocker_pays(cost)
    }

    /// A plain swing against a block (`docs/SABER-RULES.md` 4.3 5): the
    /// blocker's side first, then the attacker's. A perfect block costs the
    /// attacker `perfect::ATTACKER_COST` and disarms it when that leaves it
    /// with no points. Any other block costs it `block::BLOCKED`, and the
    /// points it has left pick what it plays (`bounce`).
    fn blocked(&mut self) {
        self.x.stopped = true;
        let guarded = self.blocker();
        if matches!(guarded, Guarded::Broken) {
            return;
        }
        if self.d.guard == Guard::Perfect {
            self.perfect();
            let left = self.a.points - perfect::ATTACKER_COST;
            self.x.took[0] = if left <= 0 {
                Took::Disarm
            } else {
                Took::HeavyStagger
            };
            return;
        }
        self.x.points[0] -= block::BLOCKED;
        if matches!(guarded, Guarded::TooTired) {
            return;
        }
        let plain = match self.d.guard {
            Guard::Held => Took::Bounce(super::Pace::Parried),
            _ => Took::Bounce(super::Pace::Blocked),
        };
        self.x.took[0] = bounce(self.a.points - block::BLOCKED, plain);
    }

    /// A fake met (`docs/SABER-RULES.md` 4.3 4): parried by a perfect or a
    /// creature's block, which costs the faker `block::FAKE_PARRIED` points
    /// and disarms it only when that leaves it with none; or breaking the
    /// guard into a lock.
    fn fake(&mut self) {
        if !matches!(self.d.guard, Guard::Perfect | Guard::Npc) {
            self.x.forced_lock = true;
            return;
        }
        self.x.points[0] -= block::FAKE_PARRIED;
        self.x.took[0] = if self.a.points - block::FAKE_PARRIED <= 0 {
            Took::Disarm
        } else {
            Took::Bounce(super::Pace::Parried)
        };
        self.x.force[1] += gains::FAKE_PARRIED;
        self.x.took[1] = Took::Parry;
        self.x.stopped = true;
    }

    /// No block (`docs/SABER-RULES.md` 4.1, 4.3 6): the blow goes on; a
    /// player pays `block::UNBLOCKED` for the blade struck and is staggered
    /// while tired; the attacker bounces as both do, unless the blocker's
    /// own blow was unblockable, which staggers the attacker.
    fn unguarded(&mut self) {
        let (a, d) = (self.a, self.d);
        if !d.npc {
            self.x.points[1] -= block::UNBLOCKED;
        }
        self.x.took[1] = if tired(d.points) {
            Took::Stagger
        } else {
            Took::Nothing
        };
        if d.unblockable {
            self.x.took[0] = Took::HeavyStagger;
            return;
        }
        self.x.points[0] -= block::BLOCKED;
        self.x.took[0] = bounce(a.points - block::BLOCKED, Took::AttackBounce);
    }
}

/// What `a`'s blow meeting `d`'s blade decides (`docs/SABER-RULES.md` 4):
/// an unblockable blow; two swings (`met`, both attacking); a super
/// break's win blocked; a fake; a plain blow against a block; or no
/// block. Lunges and spins pass a tired blocker unblocked.
pub fn resolve(a: &Side, d: &Side, met: bool) -> Exchange {
    let mut deal = Deal {
        a,
        d,
        x: Exchange::default(),
    };
    let unguarded = d.guard == Guard::None || (a.lunge && tired(d.points));
    if a.unblockable {
        deal.unblockable();
    } else if met && d.attacking && d.guard != Guard::Npc {
        deal.both_swing();
    } else if a.super_break && !unguarded {
        deal.x.points[0] -= block::BLOCKED;
        deal.x.took[0] = Took::HeavyStagger;
        deal.x.stopped = true;
    } else if a.faking {
        deal.fake();
    } else if unguarded {
        deal.unguarded();
    } else {
        deal.blocked();
    }
    deal.x
}
