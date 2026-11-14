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
use crate::combat::saber::Style;
use crate::combat::saber::duel::exchange;
use crate::combat::saber::kick;
use crate::math::{self, Vec3};
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
use rules::ai::duelling::charge;

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
/// The Stamina a creature keeps after a special move it starts.
pub const SPECIAL_KEEP: i32 = rules::ai::duelling::SPECIAL_KEEP;
/// How close (units) to a target a creature walks instead of running.
pub const WALK_WITHIN: f32 = rules::ai::duelling::WALK_WITHIN;
/// How long (ms) a target moves away before a creature runs after it.
pub const BACKING_MS: u64 = rules::ai::duelling::BACKING_MS;
/// How long (ms) a target must stop moving away before the count restarts.
const BACKING_PAUSE_MS: u64 = rules::ai::duelling::BACKING_PAUSE_MS;
/// A creature that has not thought for this long (ms) forgets what it saw.
const STALE_MS: u64 = rules::ai::duelling::STALE_MS;
/// How long (ms) a sidestep is kept, and a choice to advance, retreat or stand.
const SIDESTEP_HOLD_MS: [u64; 2] = rules::ai::duelling::SIDESTEP_HOLD_MS;
const DECISION_HOLD_MS: u64 = rules::ai::duelling::DECISION_HOLD_MS;
/// How long (ms) a target holds a block before a creature breaks it.
const BLOCK_LONG_MS: u64 = rules::ai::duelling::BLOCK_LONG_MS;
/// The kicks it tries against a held block before it swings again, and how
/// long it waits to start the first (ms).
const KICK_TRIES: i32 = rules::ai::duelling::KICK_TRIES;
const KICK_PATIENCE_MS: u64 = rules::ai::duelling::KICK_PATIENCE_MS;
/// How long (ms) after a shooter's last shot a creature that walks at it
/// stands with its guard up.
const GUN_HOLD_MS: u64 = rules::ai::duelling::GUN_HOLD_MS;
/// How far (units) from a creature a bolt's line passes and still counts as
/// headed at it.
const BOLT_LANE: f32 = rules::ai::duelling::BOLT_LANE;
/// The Stamina a creature that backs off must have again to attack.
const RECOVER_TO: i32 = rules::ai::duelling::RECOVER_TO;
/// How long (ms) after a won saber lock a creature attacks and does not circle.
const LOCK_PRESS_MS: u64 = rules::ai::duelling::LOCK_PRESS_MS;
/// How close (units, centre to centre) a kata makes a creature step away.
const STEP_AWAY_WITHIN: f32 = rules::ai::duelling::STEP_AWAY_WITHIN;

/// What a creature with a melee weapon can start, by who it is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Tier {
    #[default]
    Ordinary,
    Skilled,
    Boss,
}

/// A creature's tier: a boss by its tag, else Skilled with a lightsaber or at
/// least one Force power, else Ordinary. `forced` overrides all three.
pub fn tier(lightsaber: bool, force_powers: usize, boss_tag: bool, forced: Option<Tier>) -> Tier {
    forced.unwrap_or(if boss_tag {
        Tier::Boss
    } else if lightsaber || force_powers > 0 {
        Tier::Skilled
    } else {
        Tier::Ordinary
    })
}

/// What a creature does about a fact it reads of its target or of itself.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Reaction {
    #[default]
    None,
    Press,
    SwingNow,
    BreakBlock,
    BackOff,
    StepAway,
    /// A boss stays where it is with its guard up while its target winds up
    /// a kata or a spin.
    Guard,
}

/// The reaction to what `seen` shows, by the creature's own Stamina and tier.
/// `rested` is whether a boss has already backed off to recover,
/// `backing_off` whether the creature is backing off now, `block_ms` how long
/// the target has held its block and `lock_won_ms` how long ago the creature
/// won a saber lock.
#[allow(clippy::too_many_arguments)]
pub fn react(
    seen: &Sight,
    own_tired: bool,
    own_points: i32,
    tier: Tier,
    rested: bool,
    backing_off: bool,
    block_ms: u64,
    lock_won_ms: Option<u64>,
) -> Reaction {
    let boss = tier == Tier::Boss;
    let winding_up = seen.special && seen.gap + TIP < STEP_AWAY_WITHIN;
    if winding_up && !boss {
        return Reaction::StepAway;
    }
    if seen.reeling || seen.facing_away {
        return Reaction::SwingNow;
    }
    if seen.foe_bounced && tier != Tier::Ordinary {
        return Reaction::SwingNow;
    }
    if lock_won_ms.is_some_and(|ms| ms < LOCK_PRESS_MS) {
        return Reaction::SwingNow;
    }
    let tires = own_tired && !(boss && rested);
    let recovering = backing_off && own_points < RECOVER_TO;
    if !seen.ranged && (tires || recovering) {
        return Reaction::BackOff;
    }
    if winding_up {
        return Reaction::Guard;
    }
    if block_ms >= BLOCK_LONG_MS {
        return Reaction::BreakBlock;
    }
    if seen.tired {
        return Reaction::Press;
    }
    Reaction::None
}

/// Whether a creature with `points` Stamina starts a special move that uses
/// up `cost`: it would still have `SPECIAL_KEEP` Stamina or more afterwards.
pub fn ai_may_special(points: i32, cost: i32) -> bool {
    points - cost >= SPECIAL_KEEP
}

/// Whether a creature starts a charge: it is Skilled (`skilled`), no other
/// fighter winds up or makes a charge at its target (`other_charging`), its
/// wait (`again_at`) is over, and its target is `gap` units away (centre to
/// centre) within `charge::FROM`, or within `charge::FROM_GUN` when the target
/// holds a ranged weapon.
pub fn may_charge(
    skilled: bool,
    gap: f32,
    ranged_target: bool,
    now: u64,
    again_at: u64,
    other_charging: bool,
) -> bool {
    let [near, far] = if ranged_target {
        charge::FROM_GUN
    } else {
        charge::FROM
    };
    skilled && !other_charging && now >= again_at && (near..=far).contains(&gap)
}

/// What ends a charge.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Ending {
    Lunge,
    RunningSwing,
}

/// The attack that ends a charge: the lunge for the fast style with the
/// Stamina to start it (`stamina_enough`), else a swing made at a run.
pub fn ending(style: Option<Style>, stamina_enough: bool) -> Ending {
    if style == Some(Style::Fast) && stamina_enough {
        Ending::Lunge
    } else {
        Ending::RunningSwing
    }
}

/// How far a charge has come.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Default)]
pub enum Charge {
    #[default]
    None,
    /// Standing in the ready pose, shouting.
    Windup,
    /// Running at the target.
    Run,
    /// Pressing the keys of the ending attack.
    Strike,
    /// The ending attack is under way.
    Swing,
}

impl Charge {
    const ALL: [Charge; 5] = [
        Charge::None,
        Charge::Windup,
        Charge::Run,
        Charge::Strike,
        Charge::Swing,
    ];
}

/// Whether a target moving at `velocity` moves away from a creature that
/// sees it in direction `toward` (a unit vector from the creature to it):
/// faster than `ai.duelling.receding_speed`, and with at least
/// `ai.duelling.receding_share` of its speed along `toward`.
pub fn moving_away(velocity: Vec3, toward: Vec3) -> bool {
    let speed = crate::math::length(velocity);
    speed > rules::ai::duelling::RECEDING_SPEED
        && crate::math::dot(velocity, toward) >= rules::ai::duelling::RECEDING_SHARE * speed
}

/// Whether an AI fighter walks: near a target that has no ranged weapon and
/// is not backing away. A target that has backed away for longer than
/// `BACKING_MS` is run after, until it is within reach of the weapon.
pub fn walks(gap: f32, ranged_target: bool, receding_ms: u64, in_reach: bool) -> bool {
    !ranged_target && gap <= WALK_WITHIN && (receding_ms <= BACKING_MS || in_reach)
}

/// How a creature with a melee weapon closes on a target with a blaster.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Approach {
    /// A walk with its guard up, standing while bolts come.
    WalkGuarded,
    Run,
    Charge,
}

/// How a creature closes on a target with a blaster `gap` units away (centre to
/// centre), given that it deflects bolts (`deflects`), is tired, and may charge
/// (`may_charge`). A fighter that deflects walks guarded until it is within
/// the longest charge of a target with a melee weapon, and charges from
/// there. A tired fighter, and one that cannot deflect, charges when it may and
/// otherwise runs.
pub fn approach_gun(deflects: bool, tired: bool, gap: f32, may_charge: bool) -> Approach {
    let near = gap <= charge::FROM[1];
    match (deflects && !tired, may_charge) {
        (true, true) if near => Approach::Charge,
        (true, _) => Approach::WalkGuarded,
        (false, true) => Approach::Charge,
        (false, false) => Approach::Run,
    }
}

/// Whether a creature that walks at a target with a blaster stands still: a
/// bolt from the target is on its way to it (`bolt_incoming`), or the target's
/// last shot was `since_last_shot_ms` ago, less than `GUN_HOLD_MS`.
pub fn holds_for_bolts(bolt_incoming: bool, since_last_shot_ms: u64) -> bool {
    bolt_incoming || since_last_shot_ms < GUN_HOLD_MS
}

/// Whether a bolt that left `base` at `from_ms` with `velocity` (units a
/// second) is headed at a body at `at` at time `now`: it moves toward the
/// body, and its line passes within `BOLT_LANE` of it, seen from above.
pub fn bolt_incoming(base: Vec3, velocity: Vec3, from_ms: u64, now: u64, at: Vec3) -> bool {
    let flat = |v: Vec3| [v[0], v[1], 0.0];
    let travelled = now.saturating_sub(from_ms) as f32 / 1000.0;
    let here = math::mad(flat(base), travelled, flat(velocity));
    let to = math::sub(flat(at), here);
    let mut way = flat(velocity);
    if math::normalize(&mut way) == 0.0 {
        return false;
    }
    let along = math::dot(to, way);
    let beside = math::sub(to, math::scale(way, along));
    along > 0.0 && math::length(beside) <= BOLT_LANE
}

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
    /// The Advanced rules' pacing applies (`SaberRules::duel`).
    pub duel: bool,
    /// Its enemy holds a ranged weapon, and moves directly away from it.
    pub ranged: bool,
    /// It turns bolts (`shots::deflects`), when its enemy last fired (sim
    /// ms; 0: it has not) and whether a bolt from it is on its way here.
    pub deflects: bool,
    pub shot_at: u64,
    pub bolt_incoming: bool,
    pub receding: bool,
    /// Its enemy holds its block, is tired (`Duel::tired`), is staggered,
    /// knocked aside or has its guard broken, faces away from it, winds up
    /// a kata or a spin, or has a swing that has just bounced off its block.
    pub blocking: bool,
    pub tired: bool,
    pub reeling: bool,
    pub facing_away: bool,
    pub special: bool,
    /// The target's swing is a bounce while it fights this creature. The sim
    /// does not record what a swing bounced off, so a bounce off something
    /// else (a wall, another fighter's blade) also counts.
    pub foe_bounced: bool,
    /// Its own Stamina, tier, and whether it is in a saber lock.
    pub own_points: i32,
    pub tier: Tier,
    pub own_lock: bool,
    /// It stands on the ground, not floored and not in a lock: it may start
    /// a kick.
    pub own_free: bool,
    /// Its saber style, what its lunge costs, and whether its own attack is
    /// under way.
    pub style: Option<Style>,
    pub lunge_cost: i32,
    pub own_swinging: bool,
    /// Another fighter winds up or makes a charge at its target.
    pub other_charging: bool,
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
    /// Crouch (-1), or jump (1).
    pub up: i8,
    /// The alternate attack, and the kick.
    pub alt: bool,
    pub kick: bool,
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
    /// Since when its target has moved away (0: it has not).
    pub receding_since: u64,
    /// The last think that saw it move away, and the last think at all.
    pub receding_last: u64,
    pub last_look: u64,
    /// A sidestep it keeps: when it ends, and its side (1 right, -1 left).
    pub sidestep_until: u64,
    pub sidestep_side: i8,
    /// A choice to advance, retreat or stand (`Way` as 1, -1, 0), kept until
    /// then.
    pub decided_until: u64,
    pub decided_way: i8,
    /// Since when its target has held its block (0: it does not).
    pub block_since: u64,
    /// The kicks it pressed against this held block, and when it pressed the
    /// last.
    pub kick_tries: u8,
    pub kick_last: u64,
    /// When it won its last saber lock (0: it has not), and whether it was
    /// in one on its last think.
    pub lock_won_at: u64,
    pub locked: bool,
    /// It is backing off to recover Stamina, and a boss has done so once.
    pub backing_off: bool,
    pub rested: bool,
    /// A charge: how far it has come, when its wind-up ends or the run is
    /// given up, whom it is at (id + 1), whether it ends with the lunge, and
    /// when it may charge again.
    pub charge: Charge,
    pub charge_until: u64,
    pub charge_foe: u32,
    pub charge_lunge: bool,
    pub again_at: u64,
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
        out.u64(self.receding_since);
        out.u64(self.receding_last);
        out.u64(self.last_look);
        out.u64(self.sidestep_until);
        out.u8(self.sidestep_side as u8);
        out.u64(self.decided_until);
        out.u8(self.decided_way as u8);
        out.u64(self.block_since);
        out.u64(self.lock_won_at);
        out.u8(u8::from(self.locked)
            | u8::from(self.backing_off) << 1
            | u8::from(self.rested) << 2);
        out.u8(keys.up as u8);
        out.u8(u8::from(keys.alt) | u8::from(keys.kick) << 1);
        out.u8(self.kick_tries);
        out.u64(self.kick_last);
        out.u8(self.charge as u8 | u8::from(self.charge_lunge) << 3);
        out.u64(self.charge_until);
        out.u32(self.charge_foe);
        out.u64(self.again_at);
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
        let seen_move = input.u8()?;
        let mut pace = Pace {
            keys: Keys {
                forward,
                right,
                walk: flags & 1 != 0,
                attack: flags & 2 != 0,
                ..Keys::default()
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
            seen_move,
            seen_pain,
            ..Pace::default()
        };
        if input.since(crate::since::snapshot::FEAT_ADVANCED_PACING) {
            pace.receding_since = input.u64()?;
            pace.receding_last = input.u64()?;
            pace.last_look = input.u64()?;
            pace.sidestep_until = input.u64()?;
            pace.sidestep_side = input.u8()? as i8;
            pace.decided_until = input.u64()?;
            pace.decided_way = input.u8()? as i8;
            pace.block_since = input.u64()?;
            pace.lock_won_at = input.u64()?;
            let at = input.at();
            let flags = input.u8()?;
            if flags > 7 {
                return Err(input.invalid("pace flags", at));
            }
            pace.locked = flags & 1 != 0;
            pace.backing_off = flags & 2 != 0;
            pace.rested = flags & 4 != 0;
            let at = input.at();
            pace.keys.up = input.u8()? as i8;
            if !(-1..=1).contains(&pace.keys.up) {
                return Err(input.invalid("pace up key", at));
            }
            let at = input.at();
            let flags = input.u8()?;
            if flags > 3 {
                return Err(input.invalid("pace keys", at));
            }
            pace.keys.alt = flags & 1 != 0;
            pace.keys.kick = flags & 2 != 0;
            pace.kick_tries = input.u8()?;
            pace.kick_last = input.u64()?;
            let at = input.at();
            let flags = input.u8()?;
            pace.charge = *Charge::ALL
                .get(usize::from(flags & 7))
                .filter(|_| flags >> 4 == 0)
                .ok_or(input.invalid("pace charge", at))?;
            pace.charge_lunge = flags & 8 != 0;
            pace.charge_until = input.u64()?;
            pace.charge_foe = input.u32()?;
            pace.again_at = input.u64()?;
        }
        Ok(pace)
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
    ) -> (Way, Reaction) {
        if sight.duel && self.charge_foe != 0 && self.charge_foe != sight.foe.0.wrapping_add(1) {
            self.end_charge(now, dice);
        }
        self.meet(sight);
        self.events(sight, now, dice);
        self.rise(sight, now, dice);
        if !sight.duel {
            self.circle(now, dice, clear);
            return (self.way(sight, now), Reaction::None);
        }
        self.forget_when_stale(now);
        self.note_lock(sight, now);
        if self.pressing_lock(now) || self.charge != Charge::None {
            self.circle_left_until = 0;
            self.circle_right_until = 0;
        } else {
            self.circle(now, dice, clear);
        }
        self.watch_target(sight, now);
        let reaction = self.read(sight, now);
        if let Some(way) = self.charging_step(sight, reaction, now, dice) {
            self.interrupt();
            return (way, Reaction::None);
        }
        if matches!(
            reaction,
            Reaction::BackOff | Reaction::StepAway | Reaction::Guard
        ) {
            self.interrupt();
            let way = if reaction == Reaction::Guard {
                Way::Hold
            } else {
                Way::Retreat
            };
            return (way, reaction);
        }
        if reaction == Reaction::SwingNow {
            self.interrupt();
        }
        let way = if sight.ranged && sight.gap > 0.0 {
            Way::Advance
        } else {
            self.way(sight, now)
        };
        (self.keep_decision(way, now), reaction)
    }

    /// `may_charge` as this think sees it.
    fn may_charge_now(&self, sight: &Sight, now: u64) -> bool {
        may_charge(
            sight.tier != Tier::Ordinary,
            sight.gap + TIP,
            sight.ranged,
            now,
            self.again_at,
            sight.other_charging,
        )
    }

    /// How it closes on a target with a blaster on this think.
    fn gun_approach(&self, sight: &Sight, now: u64) -> Approach {
        approach_gun(
            sight.deflects,
            exchange::tired(sight.own_points),
            sight.gap + TIP,
            self.may_charge_now(sight, now),
        )
    }

    /// It walks at a target with a blaster with its guard up: no charge is
    /// under way, and `approach_gun` says to walk.
    fn walks_guarded(&self, sight: &Sight, now: u64) -> bool {
        sight.duel
            && sight.ranged
            && self.charge == Charge::None
            && self.gun_approach(sight, now) == Approach::WalkGuarded
    }

    /// Lets a choice and a sidestep made under the Advanced rules end early,
    /// as a read of the target's move does: the next think chooses afresh.
    pub fn interrupt(&mut self) {
        self.decided_until = 0;
        self.sidestep_until = 0;
    }

    /// A charge's next step on a think: it ends, goes on to its next part or
    /// starts. The way to go while it lasts, none when there is no charge.
    fn charging_step(
        &mut self,
        sight: &Sight,
        reaction: Reaction,
        now: u64,
        dice: &mut Dice,
    ) -> Option<Way> {
        if self.charge != Charge::None {
            self.charge_on(sight, now, dice);
        } else if self.may_start_charge(sight, reaction, now) {
            self.begin_charge(sight, now);
        }
        match self.charge {
            Charge::None => None,
            Charge::Windup => Some(Way::Hold),
            _ => Some(Way::Advance),
        }
    }

    /// Whether it starts a charge on this think: it may by `may_charge`, it
    /// is free to move, and it is not backing away from its target or
    /// guarding. Against a target with a melee weapon it is not tired; against
    /// a target with a blaster `approach_gun` says.
    fn may_start_charge(&self, sight: &Sight, reaction: Reaction, now: u64) -> bool {
        let wants = if sight.ranged {
            self.gun_approach(sight, now) == Approach::Charge
        } else {
            self.may_charge_now(sight, now) && !exchange::tired(sight.own_points)
        };
        wants
            && !matches!(
                reaction,
                Reaction::BackOff | Reaction::StepAway | Reaction::Guard
            )
            && !self.backing_off
            && sight.own_free
            && !sight.busy
            && !sight.own_swinging
    }

    fn begin_charge(&mut self, sight: &Sight, now: u64) {
        self.charge = Charge::Windup;
        self.charge_until = now + charge::WINDUP_MS;
        self.charge_foe = sight.foe.0.wrapping_add(1);
        self.charge_lunge = false;
        self.holds = [0; 6];
        self.guard_until = 0;
        self.interrupt();
    }

    /// A charge that goes on or ends on a think. It ends when the fighter is
    /// locked (pain, stun and death end it in `world::minds`), when it
    /// is not on its feet before the ending attack, and when the run or the ending has gone on for too long.
    fn charge_on(&mut self, sight: &Sight, now: u64, dice: &mut Dice) {
        let before_ending = matches!(self.charge, Charge::Windup | Charge::Run);
        let lost = sight.own_lock || (before_ending && !sight.own_free);
        let late = self.charge != Charge::Windup && now >= self.charge_until;
        if lost || late {
            return self.end_charge(now, dice);
        }
        match self.charge {
            Charge::Windup if now >= self.charge_until => {
                self.charge = Charge::Run;
                self.charge_until = now + charge::GIVE_UP_MS;
            }
            Charge::Run => {
                let lunges = matches!(
                    ending(
                        sight.style,
                        ai_may_special(sight.own_points, sight.lunge_cost)
                    ),
                    Ending::Lunge
                );
                let reaches = if lunges {
                    sight.gap + TIP <= charge::LUNGE_FROM
                } else {
                    striking_range(sight)
                };
                if reaches {
                    self.charge = Charge::Strike;
                    self.charge_lunge = lunges;
                    self.charge_until = now + charge::GIVE_UP_MS;
                }
            }
            Charge::Strike if sight.own_swinging => self.charge = Charge::Swing,
            Charge::Swing if !sight.own_swinging => self.end_charge(now, dice),
            _ => {}
        }
    }

    /// Ends a charge, if one is under way, and starts the wait before the next.
    pub fn end_charge(&mut self, now: u64, dice: &mut Dice) {
        if self.charge == Charge::None {
            return;
        }
        self.drop_charge();
        self.again_at = now + roll_ms(dice, charge::AGAIN_MS);
    }

    /// A charge forgotten with no wait: what it knew of its target is gone.
    fn drop_charge(&mut self) {
        self.charge = Charge::None;
        self.charge_until = 0;
        self.charge_foe = 0;
        self.charge_lunge = false;
    }

    /// When the ready pose ends, while it is held (else 0).
    pub fn windup_until(&self) -> u64 {
        if self.charge == Charge::Windup {
            self.charge_until
        } else {
            0
        }
    }

    /// The run or its ending attack is under way.
    pub fn charging(&self) -> bool {
        matches!(self.charge, Charge::Run | Charge::Strike | Charge::Swing)
    }

    /// Whom the charge, or its wind-up, is at.
    pub fn charging_at(&self) -> Option<SimId> {
        (self.charge != Charge::None).then(|| SimId(self.charge_foe.wrapping_sub(1)))
    }

    /// The run or its ending has gone on past its time.
    pub fn charge_late(&self, now: u64) -> bool {
        self.charge > Charge::Windup && now >= self.charge_until
    }

    /// The keys of a charge: it stands for the ready pose, runs with no
    /// swing, then presses the ending's keys, and runs on while the ending
    /// attack plays. The lunge needs a crouch from a stand (a crouch while
    /// running is a roll): a think of crouching still, then the attack
    /// (`crouched`: the crouch key of the think before).
    fn charge_keys(&self, mut keys: Keys, crouched: i8, clear: &dyn Fn(i8, i8) -> bool) -> Keys {
        keys.walk = false;
        keys.attack = false;
        match self.charge {
            Charge::Strike => {
                keys.right = 0;
                keys.attack = !self.charge_lunge || crouched < 0;
                if self.charge_lunge {
                    keys.up = -1;
                    if !keys.attack {
                        keys.forward = 0;
                    }
                }
            }
            Charge::Windup => {
                keys.forward = 0;
                keys.right = 0;
            }
            _ => {}
        }
        if (keys.forward != 0 || keys.right != 0) && !clear(keys.forward, keys.right) {
            keys.forward = 0;
            keys.right = 0;
        }
        keys
    }

    /// How long ago it won a saber lock, if it ever did.
    fn lock_won_ms(&self, now: u64) -> Option<u64> {
        (self.lock_won_at != 0).then(|| now.saturating_sub(self.lock_won_at))
    }

    /// It won a saber lock lately: it attacks and does not circle.
    fn pressing_lock(&self, now: u64) -> bool {
        self.lock_won_ms(now).is_some_and(|ms| ms < LOCK_PRESS_MS)
    }

    /// A lock that has ended with its target beaten was won. The sim records
    /// no winner, so this takes a lock that broke apart for a won one when the
    /// target happens to be reeling on that think, and misses a won lock whose
    /// loser is not reeling.
    fn note_lock(&mut self, sight: &Sight, now: u64) {
        if sight.own_lock {
            self.locked = true;
        } else if self.locked {
            self.locked = false;
            if sight.reeling {
                self.lock_won_at = now.max(1);
            }
        }
    }

    /// The reaction to what it sees, with the memory it keeps of a block, a
    /// back off and a rest.
    fn read(&mut self, sight: &Sight, now: u64) -> Reaction {
        if !sight.blocking {
            self.block_since = 0;
            self.kick_tries = 0;
            self.kick_last = 0;
        } else if self.block_since == 0 {
            self.block_since = now.max(1);
        }
        let block_ms = match self.block_since {
            0 => 0,
            since => now.saturating_sub(since),
        };
        let reaction = react(
            sight,
            exchange::tired(sight.own_points),
            sight.own_points,
            sight.tier,
            self.rested,
            self.backing_off,
            block_ms,
            self.lock_won_ms(now),
        );
        if sight.own_points >= RECOVER_TO || sight.ranged {
            self.backing_off = false;
        } else if reaction == Reaction::BackOff {
            self.backing_off = true;
            self.rested |= sight.tier == Tier::Boss;
        }
        reaction
    }

    /// A creature that comes back to a fight after a long time away does
    /// not act on what it saw then.
    fn forget_when_stale(&mut self, now: u64) {
        if self.last_look != 0 && now > self.last_look + STALE_MS {
            self.forget_fight();
            self.rested = false;
        }
        self.last_look = now.max(1);
    }

    /// What it knew of its target before it looked away or met another.
    fn forget_fight(&mut self) {
        self.receding_since = 0;
        self.decided_until = 0;
        self.sidestep_until = 0;
        self.block_since = 0;
        self.kick_tries = 0;
        self.kick_last = 0;
        self.lock_won_at = 0;
        self.locked = false;
        self.backing_off = false;
        self.drop_charge();
    }

    /// Notes since when its target has moved away from it. A pause of
    /// less than `BACKING_PAUSE_MS` does not restart the count.
    fn watch_target(&mut self, sight: &Sight, now: u64) {
        if sight.receding {
            self.receding_last = now;
            if self.receding_since == 0 {
                self.receding_since = now.max(1);
            }
        } else if now >= self.receding_last + BACKING_PAUSE_MS {
            self.receding_since = 0;
        }
    }

    /// The way it keeps until its choice may change: a new choice is kept
    /// for `DECISION_HOLD_MS`.
    fn keep_decision(&mut self, way: Way, now: u64) -> Way {
        if now < self.decided_until {
            return match self.decided_way.signum() {
                1 => Way::Advance,
                -1 => Way::Retreat,
                _ => Way::Hold,
            };
        }
        self.decided_until = now + DECISION_HOLD_MS;
        self.decided_way = match way {
            Way::Advance => 1,
            Way::Retreat => -1,
            Way::Hold => 0,
        };
        way
    }

    /// It has kicked as often as it will against this held block, or has
    /// waited too long to start the first.
    fn kicks_given_up(&self, now: u64) -> bool {
        let began = self.block_since + BLOCK_LONG_MS;
        i32::from(self.kick_tries) >= KICK_TRIES
            || (self.kick_tries == 0 && now >= began + KICK_PATIENCE_MS)
    }

    /// A kick key may go down now: it is free to kick, its last swing or kick
    /// is over, and the key was up on the think before (a held key makes no
    /// new press).
    fn may_press_kick(&self, sight: &Sight, now: u64) -> bool {
        sight.own_free
            && !sight.busy
            && (self.kick_last == 0 || now >= self.kick_last + THINK_MS * 3 / 2)
    }

    /// The second half: from the keys for the chosen way, the keys held
    /// until the next think. A fighter that charges takes the charge's keys
    /// (`charge_keys`) and does not parry or step aside from its target's
    /// blade during the wind-up or the run.
    #[allow(clippy::too_many_arguments)]
    pub fn finish(
        &mut self,
        keys: Keys,
        sight: &Sight,
        reaction: Reaction,
        now: u64,
        dice: &mut Dice,
        clear: &dyn Fn(i8, i8) -> bool,
        turn: f32,
        from_foe: f32,
    ) -> Keys {
        if sight.duel && self.charge != Charge::None {
            self.keys = self.charge_keys(keys, self.keys.up, clear);
            return self.keys;
        }
        // Stepping aside from the foe's blade looks at the forward key given.
        self.keys = keys;
        if matches!(sight.arms, Arms::Saber { .. }) {
            self.evade(sight, now, dice, clear);
        }

        let mut keys = keys;
        let stands = matches!(reaction, Reaction::StepAway | Reaction::Guard);
        let no_side = sight.duel && (stands || self.pressing_lock(now));
        if no_side {
            self.circle_left_until = 0;
            self.circle_right_until = 0;
            self.sidestep_until = 0;
            self.holds[RIGHT] = 0;
            self.holds[LEFT] = 0;
            keys.right = 0;
        }
        if sight.duel && stands {
            self.holds[FORWARD] = 0;
            self.holds[BACK] = 0;
            self.holds[NONE] = 0;
            if reaction == Reaction::Guard {
                keys.forward = 0;
            }
        }
        let guarded = self.walks_guarded(sight, now);
        if guarded {
            let since_shot = match sight.shot_at {
                0 => u64::MAX,
                shot => now.saturating_sub(shot),
            };
            if !striking_range(sight) {
                self.guard_until = self.guard_until.max(now + THINK_MS * 2);
            }
            if holds_for_bolts(sight.bolt_incoming, since_shot) {
                self.holds[FORWARD] = 0;
                self.holds[BACK] = 0;
                self.holds[NONE] = 0;
                keys.forward = 0;
            }
        }
        if sight.duel {
            let receding_ms = match self.receding_since {
                0 => 0,
                since => now.saturating_sub(since),
            };
            let centres = sight.gap + TIP;
            keys.walk = match reaction {
                Reaction::StepAway => false,
                Reaction::BackOff => true,
                _ if guarded => true,
                _ => walks(centres, sight.ranged, receding_ms, within_reach(sight)),
            };
        } else if keys.forward == 0 && now < self.walk_until {
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

        if sight.duel {
            match reaction {
                Reaction::BackOff | Reaction::Guard => {
                    self.guard_until = self.guard_until.max(now + THINK_MS * 2);
                }
                // A target that swings is still guarded against.
                Reaction::SwingNow if !sight.swinging => self.guard_until = 0,
                _ => {}
            }
        }
        keys.attack = striking_range(sight) && now >= self.guard_until;
        // A kick is pressed only once its swing is over, and no new swing
        // begins before it.
        let breaking = sight.duel
            && reaction == Reaction::BreakBlock
            && kicks_reach(sight)
            && !self.kicks_given_up(now);
        keys.attack &= !breaking;
        keys.kick = breaking && self.may_press_kick(sight, now);
        if keys.kick {
            self.kick_tries = self.kick_tries.saturating_add(1);
            self.kick_last = now;
        }
        if sight.duel {
            if !no_side {
                self.keep_sidestep(&mut keys, now, dice, from_foe);
            }
        } else if keys.attack && keys.right == 0 && dice.one_in(striking::SIDESTEP_ODDS) {
            keys.right = if from_foe > 0.0 { -FULL } else { FULL };
        }

        if keys.kick {
            // The kick goes the way the keys point: ahead.
            keys.forward = 0;
            keys.right = 0;
        }

        if (keys.forward != 0 || keys.right != 0) && !clear(keys.forward, keys.right) {
            self.circle_left_until = 0;
            self.circle_right_until = 0;
            self.sidestep_until = 0;
            keys.forward = 0;
            keys.right = 0;
        }

        self.keys = keys;
        keys
    }

    /// The sidestep of an attack under the Advanced rules: chosen by the
    /// same odds as before, then kept for `SIDESTEP_HOLD_MS` on its side.
    fn keep_sidestep(&mut self, keys: &mut Keys, now: u64, dice: &mut Dice, from_foe: f32) {
        if keys.right != 0 {
            return;
        }
        if now >= self.sidestep_until && keys.attack && dice.one_in(striking::SIDESTEP_ODDS) {
            self.sidestep_until = now + roll_ms(dice, SIDESTEP_HOLD_MS);
            self.sidestep_side = if from_foe > 0.0 { -1 } else { 1 };
        }
        if now < self.sidestep_until {
            keys.right = FULL * self.sidestep_side;
        }
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
        if sight.duel {
            self.forget_fight();
        }
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
        if !sight.duel {
            self.walk_until = 0;
        }
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
    within_reach(sight)
        || (gap >= striking::CLOSING_FROM && sight.closing.is_some_and(|closing| closing >= gap))
}

/// The foe is within reach of a kick.
fn kicks_reach(sight: &Sight) -> bool {
    sight.gap + TIP <= kick::REACH
}

/// The foe is touching, or in view within reach of the blade's tip.
fn within_reach(sight: &Sight) -> bool {
    sight.gap <= 0.0 || (sight.gap <= striking::IN_VIEW && sight.in_view)
}
