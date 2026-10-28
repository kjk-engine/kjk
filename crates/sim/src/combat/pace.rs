//! How a blade wielder paces a fight, one think at a time.
//!
//! Every `THINK_MS` a fencing creature thinks in two halves. [`Pace::decide`]
//! weighs its Aggression (against a new foe, after its saber's blocks,
//! parries and hurts, and every few seconds by what the foe holds), maybe
//! starts to circle, and picks a [`Way`]: close in, hold or back off. The
//! caller turns that way into keys, then [`Pace::finish`] shapes them: it
//! parries or steps aside from the foe's blade, walks while circling, keeps
//! each direction it takes for a while, attacks within reach and never
//! walks into a wall or off a ledge. Everything it remembers between
//! thinks is in [`Pace`]; every number is a rule (`ai.blades`).

use crate::combat::mind;
use crate::motion::HALF_WIDTH;
use crate::snapshot::SimId;
use crate::wire::{DecodeError, Reader, Writer};
use rules::ai::blades;
use rules::ai::blades::aggression;
use rules::ai::blades::circling;
use rules::ai::blades::distance;
use rules::ai::blades::events;
use rules::ai::blades::holds;
use rules::ai::blades::parrying;
use rules::ai::blades::rating;
use rules::ai::blades::striking;

/// How often a fencer thinks (ms).
pub const THINK_MS: u64 = rules::ai::tempo::THINK_MS;
/// How fast a fencer turns to face its foe.
pub const TURN_RATE: f32 = rules::ai::tempo::TURN_RATE;
/// The game's rank number of a fencer.
pub const FENCER: u8 = 3;
/// The game's rank number of a lieutenant, one above a fencer: up to it, a
/// parrying creature does not close in.
const LIEUTENANT: u8 = 4;
/// The rank every blade wielder fences at.
pub const RANK: u8 = blades::RANK;
/// How far out a blade's tip is, centre to centre (units), summed left to
/// right.
pub const TIP: f32 = blades::BLADE_LENGTH + blades::TIP_WIDTHS * HALF_WIDTH + blades::TIP_MARGIN;
/// How far ahead a fencer judges where its foe will be (ms).
pub const LEAD_MS: f32 = blades::LEAD_MS;
/// Within this gap a swinging foe's blade is parried at once.
const BLOCK_GAP: f32 = blades::BLOCK_WIDTHS * HALF_WIDTH + blades::BLADE_LENGTH - TIP;
/// A key held all the way.
const FULL: i8 = i8::MAX;

/// Where a think means to go relative to its foe.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Way {
    Hold,
    Advance,
    Retreat,
}

/// What its enemy holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Arms {
    Saber { lit: bool },
    Gun { firing: bool },
    Other,
}

/// What a mind knows on a think.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Sight {
    pub foe: SimId,
    /// From its blade's tip to where its enemy will be (`LEAD_MS`; units).
    pub gap: f32,
    /// Within its view (`ai.blades.view`).
    pub in_view: bool,
    /// How fast the two close (units a second, as JA reckons it: its own
    /// speed toward the enemy, or the enemy's toward it when it stands),
    /// none when neither moves.
    pub closing: Option<f32>,
    pub arms: Arms,
    /// Its enemy swings, stands on the ground, moves.
    pub swinging: bool,
    pub grounded: bool,
    pub moving: bool,
    /// How squarely its enemy comes at it or faces it (a dot).
    pub coming: f32,
    /// How squarely it faces its enemy (a dot).
    pub facing: f32,
    /// Its own swing is under way.
    pub busy: bool,
    pub ally: bool,
    /// Its hit points.
    pub health: f32,
    /// Its saber move (`saber::Move`'s number) and when its pain ends.
    pub saber_move: u8,
    pub bounced: bool,
    pub broken: bool,
    pub parried: bool,
    pub pain_until: u64,
}

/// The keys a think leaves held.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Keys {
    pub forward: i8,
    pub right: i8,
    pub walk: bool,
    pub attack: bool,
}

/// Its timers (sim ms each ends at) and aggression.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Pace {
    pub keys: Keys,
    /// With no straight way to its enemy it hunts along a route, facing
    /// the way it goes.
    pub hunting: bool,
    pub aggression: i8,
    /// Whom it last weighed as a new enemy (id + 1; 0: none).
    pub weighed: u32,
    pub rise_due: u64,
    pub circle_left_until: u64,
    pub circle_right_until: u64,
    pub circle_pause_until: u64,
    pub walk_until: u64,
    /// When each kept way of the keys ends: on the forward axis ahead,
    /// behind and still, then on the side axis rightward, leftward and
    /// centred; and when each of the four moving ways may be kept again.
    pub holds: [u64; 6],
    pub again: [u64; 4],
    pub guard_until: u64,
    pub guard_review_until: u64,
    /// The saber move and pain it saw last (for its saber events).
    pub seen_move: u8,
    pub seen_pain: u64,
}

const FORWARD: usize = 0;
const BACK: usize = 1;
const NONE: usize = 2;
const RIGHT: usize = 3;
const LEFT: usize = 4;
const CENTRE: usize = 5;

/// One of the two key axes the holds keep steady: its three holds (indexes
/// into `Pace::holds`), its two again times (into `Pace::again`) and how long
/// a newly taken way is kept.
struct Axis {
    plus: usize,
    minus: usize,
    still: usize,
    again: [usize; 2],
    range: [u64; 2],
}

/// Forward and back.
const ALONG: Axis = Axis {
    plus: FORWARD,
    minus: BACK,
    still: NONE,
    again: [0, 1],
    range: holds::ALONG_MS,
};

/// Right and left.
const ACROSS: Axis = Axis {
    plus: RIGHT,
    minus: LEFT,
    still: CENTRE,
    again: [2, 3],
    range: holds::ACROSS_MS,
};

/// A think's dice, from a hash.
pub struct Dice {
    now: u64,
    id: SimId,
    salt: u64,
}

impl Dice {
    pub fn new(now: u64, id: SimId) -> Dice {
        Dice { now, id, salt: 40 }
    }

    /// A number in [0, 1).
    fn unit(&mut self) -> f32 {
        self.salt += 1;
        mind::dice(self.now, self.id, self.salt)
    }

    /// `low` to `high`, both in.
    pub(crate) fn roll_between(&mut self, low: i64, high: i64) -> i64 {
        let span = (high - low).max(0) + 1;
        low + ((self.unit() * span as f32) as i64).min(span - 1)
    }

    /// `low` to `high` ms.
    pub(crate) fn ms(&mut self, low: u64, high: u64) -> u64 {
        self.roll_between(low as i64, high as i64) as u64
    }

    fn real_between(&mut self, low: f32, high: f32) -> f32 {
        low + self.unit() * (high - low)
    }

    /// One time in `n + 1`.
    fn one_in(&mut self, n: i64) -> bool {
        self.roll_between(0, n) == 0
    }
}

impl Pace {
    pub(crate) fn encode(&self, out: &mut Writer) {
        let keys = self.keys;
        out.u8(keys.forward as u8);
        out.u8(keys.right as u8);
        out.u8(u8::from(keys.walk) | u8::from(keys.attack) << 1 | u8::from(self.hunting) << 2);
        out.u8(self.aggression as u8);
        out.u32(self.weighed);
        for time in [
            self.rise_due,
            self.circle_left_until,
            self.circle_right_until,
            self.circle_pause_until,
            self.walk_until,
            self.guard_until,
            self.guard_review_until,
            self.seen_pain,
        ]
        .into_iter()
        .chain(self.holds)
        .chain(self.again)
        {
            out.u64(time);
        }
        out.u8(self.seen_move);
    }

    pub(crate) fn decode(input: &mut Reader) -> Result<Pace, DecodeError> {
        let forward = input.u8()? as i8;
        let right = input.u8()? as i8;
        let at = input.at();
        let flags = input.u8()?;
        if flags > 7 {
            return Err(input.invalid("pace keys", at));
        }
        let aggression = input.u8()? as i8;
        let weighed = input.u32()?;
        let mut times = [0u64; 8];
        for time in &mut times {
            *time = input.u64()?;
        }
        let mut holds = [0u64; 6];
        for time in &mut holds {
            *time = input.u64()?;
        }
        let mut again = [0u64; 4];
        for time in &mut again {
            *time = input.u64()?;
        }
        let [
            rise_due,
            circle_left_until,
            circle_right_until,
            circle_pause_until,
            walk_until,
            guard_until,
            guard_review_until,
            seen_pain,
        ] = times;
        Ok(Pace {
            keys: Keys {
                forward,
                right,
                walk: flags & 1 != 0,
                attack: flags & 2 != 0,
            },
            hunting: flags & 4 != 0,
            aggression,
            weighed,
            rise_due,
            circle_left_until,
            circle_right_until,
            circle_pause_until,
            walk_until,
            holds,
            again,
            guard_until,
            guard_review_until,
            seen_move: input.u8()?,
            seen_pain,
        })
    }
}

/// A time roll over a range's two ends.
fn roll_ms(dice: &mut Dice, [low, high]: [u64; 2]) -> u64 {
    dice.ms(low, high)
}

/// A roll over a range of whole numbers.
fn roll(dice: &mut Dice, [low, high]: [i64; 2]) -> i64 {
    dice.roll_between(low, high)
}

impl Pace {
    /// The first half of a think: weigh Aggression, maybe start circling,
    /// and pick the way to go.
    pub fn decide(
        &mut self,
        sight: &Sight,
        now: u64,
        dice: &mut Dice,
        clear: &dyn Fn(i8, i8) -> bool,
    ) -> Way {
        self.meet(sight);
        self.events(sight, now, dice);
        self.rise(sight, now, dice);
        self.circle(now, dice, clear);
        self.way(sight, now)
    }

    /// The second half: from the keys for the chosen way, the keys held
    /// until the next think.
    #[allow(clippy::too_many_arguments)]
    pub fn finish(
        &mut self,
        keys: Keys,
        sight: &Sight,
        now: u64,
        dice: &mut Dice,
        clear: &dyn Fn(i8, i8) -> bool,
        turn: f32,
        from_foe: f32,
    ) -> Keys {
        // Stepping aside from the foe's blade looks at the forward key given.
        self.keys = keys;
        if matches!(sight.arms, Arms::Saber { .. }) {
            self.evade(sight, now, dice, clear);
        }

        let mut keys = keys;
        if keys.forward == 0 && now < self.walk_until {
            keys.walk = true;
        }

        // Circling sets a side only when the way left the right key free.
        if keys.right == 0 {
            if now < self.circle_left_until {
                if turn <= circling::TURN_LIMIT {
                    keys.right = -FULL;
                }
            } else if now < self.circle_right_until && turn >= -circling::TURN_LIMIT {
                keys.right = FULL;
            }
        }

        (keys.forward, keys.right) = self.hold(&ALONG, keys.forward, keys.right, now, dice);
        (keys.right, keys.forward) = self.hold(&ACROSS, keys.right, keys.forward, now, dice);

        keys.attack = striking_range(sight) && now >= self.guard_until;
        if keys.attack && keys.right == 0 && dice.one_in(striking::SIDESTEP_ODDS) {
            keys.right = if from_foe > 0.0 { -FULL } else { FULL };
        }

        if (keys.forward != 0 || keys.right != 0) && !clear(keys.forward, keys.right) {
            self.circle_left_until = 0;
            self.circle_right_until = 0;
            keys.forward = 0;
            keys.right = 0;
        }

        self.keys = keys;
        keys
    }

    /// Aggression's bounds on this think: the party's or its foes'.
    fn bounds(sight: &Sight) -> [i64; 2] {
        if sight.ally {
            aggression::ALLY
        } else {
            aggression::ENEMY
        }
    }

    /// Sets Aggression to `value`, kept within this think's bounds.
    fn aggression(&mut self, value: i64, sight: &Sight) {
        let [low, high] = Self::bounds(sight);
        self.aggression = value.clamp(low, high) as i8;
    }

    /// Moves Aggression by `change`, kept within this think's bounds.
    fn shift(&mut self, change: i64, sight: &Sight) {
        self.aggression(i64::from(self.aggression) + change, sight);
    }

    /// A foe it has not weighed yet: it starts from its first Aggression
    /// when it never had a foe, then rates this one.
    fn meet(&mut self, sight: &Sight) {
        let key = sight.foe.0.wrapping_add(1);
        if key == self.weighed {
            return;
        }
        if self.weighed == 0 {
            self.aggression = aggression::START;
        }
        self.weighed = key;
        self.rate(sight);
    }

    /// Aggression becomes the mean, rounded up, of its health and the foe's
    /// weapon (each weighted) and its Aggression so far.
    fn rate(&mut self, sight: &Sight) {
        let health = sight.health / rating::HEALTH_FULL;
        let [against_saber, against_other] = rating::HEALTH_WEIGHT;
        let [saber, gun_near, gun_far, other] = rating::WEAPON_WEIGHT;
        let (health, weapon) = match sight.arms {
            Arms::Saber { .. } => (health * against_saber, saber),
            Arms::Gun { .. } if sight.gap < aggression::GUN_NEAR => {
                (health * against_other, gun_near)
            }
            // The healthier it is, the more it hangs back from a far gun.
            Arms::Gun { .. } => (against_other - health * against_other, gun_far),
            Arms::Other => (health * against_other, other),
        };
        let mean = (((health + weapon) + f32::from(self.aggression)) / 3.0).ceil();
        self.aggression(mean as i64, sight);
    }

    /// What its saber met since the last think: its own move changing (a
    /// parry broken, a swing blocked, a blow parried), then its pain.
    fn events(&mut self, sight: &Sight, now: u64, dice: &mut Dice) {
        if sight.saber_move != self.seen_move {
            self.seen_move = sight.saber_move;
            if sight.broken {
                self.shift(events::BROKEN, sight);
            } else if sight.bounced {
                if dice.one_in(events::BLOCKED_ODDS) {
                    self.shift(events::BLOCKED, sight);
                }
            } else if sight.parried {
                self.guard_until = 0;
                if dice.one_in(events::PARRIED_ODDS) {
                    self.shift(events::PARRIED, sight);
                }
            }
        }

        if sight.pain_until != self.seen_pain {
            self.seen_pain = sight.pain_until;
            if matches!(sight.arms, Arms::Saber { .. }) {
                if dice.one_in(events::CUT_ODDS) {
                    self.shift(events::CUT, sight);
                }
                // A saber's hit drops its guard for a moment.
                self.guard_until = 0;
                self.guard_review_until = now + parrying::HIT_LOCK_MS;
            } else {
                self.shift(events::HURT, sight);
            }
        }
    }

    /// Every few seconds Aggression rises by what the foe holds.
    fn rise(&mut self, sight: &Sight, now: u64, dice: &mut Dice) {
        if now < self.rise_due {
            return;
        }
        self.rise_due = now + roll_ms(dice, aggression::RISE_MS);
        let change = match sight.arms {
            Arms::Saber { lit: false } => aggression::UNLIT_SABER,
            Arms::Saber { lit: true } => aggression::LIT_SABER,
            Arms::Gun { firing } => {
                i64::from(!firing) + i64::from(sight.gap < aggression::GUN_NEAR)
            }
            Arms::Other => 0,
        };
        // No rise leaves Aggression as it is, even outside its bounds.
        if change != 0 {
            self.shift(change, sight);
        }
    }

    /// With no step and no wait running, it may start to circle at a walk;
    /// otherwise it waits before asking again.
    fn circle(&mut self, now: u64, dice: &mut Dice, clear: &dyn Fn(i8, i8) -> bool) {
        let idle = now >= self.circle_pause_until
            && now >= self.circle_left_until
            && now >= self.circle_right_until;
        if !idle {
            return;
        }
        if dice.one_in(circling::ODDS) {
            self.step_aside(
                now,
                dice,
                clear,
                circling::STRAFE_MS,
                circling::AFTER_MS,
                true,
            );
        } else {
            self.circle_pause_until = now + roll_ms(dice, circling::WAIT_MS);
        }
    }

    /// The way by the gap to the foe and its Aggression.
    fn way(&self, sight: &Sight, now: u64) -> Way {
        let parrying = now < self.guard_until && RANK <= LIEUTENANT;
        let may_close = !parrying && sight.grounded;
        let timid = self.aggression < distance::BACK_BELOW;
        let bold = self.aggression > distance::CLOSE_ABOVE && may_close;
        let gap = sight.gap;
        if gap <= distance::INSIDE {
            Way::Retreat
        } else if gap <= 0.0 {
            if timid { Way::Retreat } else { Way::Hold }
        } else if gap > distance::FAR {
            if self.aggression > distance::FAR_ABOVE {
                Way::Advance
            } else {
                Way::Hold
            }
        } else if gap > distance::NEAR {
            if bold { Way::Advance } else { Way::Hold }
        } else if timid {
            Way::Retreat
        } else if bold {
            Way::Advance
        } else {
            Way::Hold
        }
    }

    /// Takes a step to a clear side for a time from `time`, then waits a
    /// time from `wait` before circling again. Nothing while a step runs or
    /// when neither side is clear.
    fn step_aside(
        &mut self,
        now: u64,
        dice: &mut Dice,
        clear: &dyn Fn(i8, i8) -> bool,
        time: [u64; 2],
        wait: [u64; 2],
        walk: bool,
    ) {
        if now < self.circle_left_until || now < self.circle_right_until {
            return;
        }
        let length = roll_ms(dice, time);
        let sides = if dice.one_in(1) {
            [-FULL, FULL]
        } else {
            [FULL, -FULL]
        };
        let forward = self.keys.forward;
        let Some(side) = sides.into_iter().find(|&side| clear(forward, side)) else {
            return;
        };
        let end = now + length;
        if side < 0 {
            self.circle_left_until = end;
        } else {
            self.circle_right_until = end;
        }
        self.circle_pause_until = end + roll_ms(dice, wait);
        if walk {
            self.walk_until = end;
        }
    }

    /// Raises its blade for the parry time (half of it, rounded up, now and
    /// then), unless it may not choose a parry yet. A running parry is not
    /// made longer.
    fn parry(&mut self, now: u64, dice: &mut Dice) {
        if now < self.guard_review_until {
            return;
        }
        let time = if dice.one_in(parrying::HALF_ODDS) {
            parrying::PARRY_MS.div_ceil(2)
        } else {
            parrying::PARRY_MS
        };
        self.guard_review_until = now + dice.ms(0, time);
        if now >= self.guard_until {
            self.guard_until = now + time;
        }
    }

    /// Against a foe's blade: a parry at once when it swings close, else by
    /// chance a defence roll that does nothing, parries or steps aside at a
    /// run. Changes timers only.
    fn evade(&mut self, sight: &Sight, now: u64, dice: &mut Dice, clear: &dyn Fn(i8, i8) -> bool) {
        if sight.swinging && sight.gap <= BLOCK_GAP && !sight.busy {
            self.parry(now, dice);
            return;
        }

        let [if_swinging, if_not] = parrying::CHANCE;
        let chance = if sight.swinging { if_swinging } else { if_not };
        if dice.roll_between(0, 100) >= chance
            || dice.real_between(parrying::SQUARENESS, 1.0) >= sight.coming
        {
            return;
        }

        // The defence roll; none means it steps aside.
        let aggression = i64::from(self.aggression);
        let defence = if sight.busy {
            if dice.roll_between(0, parrying::ROLL) < aggression {
                return;
            }
            None
        } else if sight.gap > parrying::REACH || !sight.swinging {
            if !sight.moving || dice.roll_between(0, parrying::ROLL) < aggression {
                return;
            }
            None
        } else if sight.facing < parrying::FACING {
            Some(roll(dice, parrying::TURNED_AWAY))
        } else if sight.gap < parrying::CLOSE {
            Some(dice.roll_between(aggression, parrying::CLOSE_MOST))
        } else {
            Some(roll(dice, parrying::OPEN))
        };

        match defence {
            Some(roll) if (0..=parrying::KICK_MOST).contains(&roll) => {}
            Some(roll) if (0..=parrying::PARRY_MOST).contains(&roll) => self.parry(now, dice),
            _ => {
                if !dice.one_in(parrying::STRAFE_ODDS) {
                    self.step_aside(
                        now,
                        dice,
                        clear,
                        parrying::STRAFE_MS,
                        parrying::AFTER_MS,
                        false,
                    );
                }
            }
        }
        self.walk_until = 0;
    }

    /// Keeps one axis's key steady: a way once taken is held for a while, a
    /// reversal first stands still on that axis, and a key let go keeps
    /// going while its hold runs. Returns the axis's key and the other one.
    fn hold(&mut self, axis: &Axis, key: i8, other: i8, now: u64, dice: &mut Dice) -> (i8, i8) {
        if key == 0 {
            let key = if now < self.holds[axis.plus] {
                FULL
            } else if now < self.holds[axis.minus] {
                -FULL
            } else {
                0
            };
            return (key, other);
        }

        let (own, opposite, again) = if key > 0 {
            (axis.plus, axis.minus, axis.again[0])
        } else {
            (axis.minus, axis.plus, axis.again[1])
        };

        if now < self.holds[opposite] || now < self.holds[axis.still] {
            // Turning back, or pausing: this axis stops and the other one
            // goes on all the way.
            self.holds[opposite] = 0;
            if now >= self.holds[axis.still] {
                self.holds[axis.still] = now + roll_ms(dice, holds::STILL_MS);
            }
            (0, FULL * other.signum())
        } else if now >= self.holds[own] {
            if now >= self.again[again] {
                let end = now + roll_ms(dice, axis.range);
                self.holds[own] = end;
                self.again[again] = end + roll_ms(dice, holds::AGAIN_MS);
            }
            (key, other)
        } else {
            (FULL * key.signum(), other)
        }
    }
}

/// Close enough to swing: touching, in view within reach, or closing fast
/// from a little way off.
fn striking_range(sight: &Sight) -> bool {
    let gap = sight.gap;
    gap <= 0.0
        || (gap <= striking::IN_VIEW && sight.in_view)
        || (gap >= striking::CLOSING_FROM && sight.closing.is_some_and(|closing| closing >= gap))
}
