//! Idles: a character left standing still in its stance for a while
//! fidgets (JA's stand idles), and anything it does breaks the fidget off.
//!
//! Every step, after its move, each living character is checked (`check`).
//! It is busy when it moves, presses an action button or a move key, is
//! hurt, aims, charges, swings or shoots, gestures or channels the Force,
//! is knocked down or blocking, changes weapon (a player), holds fists or a
//! baton, or plays a script's animation (`Acting`). It is at rest when it is
//! not busy, its legs play one of the two stances, its view has hardly
//! turned since the rest began, and neither part is held. What the rest is
//! measured from is the fighter's `Rest`, part of its snapshot.
//!
//! After `REST_MS` at rest the legs' stance picks an idle, which plays on
//! both parts; the next can come no earlier than its end plus a random
//! wait of up to `SPREAD_MS`. A busy character playing an idle drops it at
//! once. Scripts' animations (`Acting`) are numbered as KOTOR's
//! `ANIMATION_…` constants and rows (`FIRE_AND_FORGET`, `LOOPED_ROWS`).

use crate::combat::Fighter;
use crate::combat::saber::{Blocked, Change, Hand, Held, Kit, Move};
use crate::input::{Command, buttons};
use crate::motion::{self, Anim, AnimTable, MotionState};
use crate::rng::SimRng;
use crate::wire::{DecodeError, Reader, Writer};

/// How long a character stands at rest before it idles (ms).
const REST_MS: u64 = rules::combat::idle::REST_MS;
/// The longest extra wait after an idle before the next (ms).
const SPREAD_MS: u32 = rules::combat::idle::SPREAD_MS;
/// How far the view may turn and still be at rest (degrees, over all axes).
const VIEW_SLACK: f32 = rules::combat::idle::VIEW_SLACK;

/// The buttons that keep a character busy: all but walking, the saber
/// style, stealth and laying a mine, which change how it stands but are
/// not something it does.
const ACTION_BUTTONS: u16 = buttons::ATTACK
    | buttons::ALT_ATTACK
    | buttons::USE
    | buttons::USE_HELD
    | buttons::FORCE_GRIP
    | buttons::FORCE_POWER
    | buttons::FORCE_LIGHTNING
    | buttons::FORCE_DRAIN
    | buttons::GRENADE
    | buttons::MEDPAC
    | buttons::SHIELD
    | buttons::STIM;

/// What a rest is measured from: since when (sim ms; later than now while
/// an idle plays out), or, `open`, from the step before the next one at
/// rest; and the health and view it began with.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Rest {
    pub since: u64,
    pub open: bool,
    pub hp: i32,
    pub view: [f32; 3],
}

impl Rest {
    pub(crate) fn encode(&self, out: &mut Writer) {
        out.u64(self.since);
        out.u8(u8::from(self.open));
        out.u32(self.hp as u32);
        for v in self.view {
            out.f32(v);
        }
    }

    pub(crate) fn decode(input: &mut Reader) -> Result<Rest, DecodeError> {
        Ok(Rest {
            since: input.u64()?,
            open: input.u8()? != 0,
            hp: input.u32()? as i32,
            view: [input.f32()?, input.f32()?, input.f32()?],
        })
    }
}

/// The first fire-and-forget `ANIMATION_…` constant: those below loop.
pub const FIRE_AND_FORGET: u16 = 100;
/// Where the animations by `animations.2da` row that loop are numbered
/// from (`asset_odyssey::creature::LOOPED`): from here they loop again.
pub const LOOPED_ROWS: u16 = 20_000;

/// An animation a script plays on a character (`ActionPlayAnimation`):
/// its `ANIMATION_…` constant, from when (sim ms) and for how long (ms;
/// `u32::MAX` until stopped). Views draw it as KOTOR's own clip; meanwhile
/// the character does not idle.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Acting {
    pub animation: u16,
    pub since: u64,
    pub ms: u32,
}

impl Acting {
    /// Whether it still plays at `now`.
    pub fn playing(&self, now: u64) -> bool {
        now < self.since.saturating_add(u64::from(self.ms))
    }

    pub(crate) fn encode(&self, out: &mut Writer) {
        out.u16(self.animation);
        out.u64(self.since);
        out.u32(self.ms);
    }

    pub(crate) fn decode(input: &mut Reader) -> Result<Acting, DecodeError> {
        Ok(Acting {
            animation: input.u16()?,
            since: input.u64()?,
            ms: input.u32()?,
        })
    }
}

/// The stand idles (JA's): the first stance's one and the second's two.
const IDLES: [Anim; 3] = [
    Anim::BOTH_STAND1IDLE1,
    Anim::BOTH_STAND2IDLE1,
    Anim::BOTH_STAND2IDLE2,
];

/// Checks one character after its move at `now` (sim ms, `msec` after the
/// step before): breaks off an idle it is too busy for, keeps its rest's
/// record, and starts an idle once it has stood at rest for `REST_MS`.
/// `player`: it is a player's, whose weapon choice in `command` counts.
/// Draws from `rng` only when an idle starts.
pub fn check(
    ps: &mut MotionState,
    fighter: &mut Fighter,
    (command, player): (&Command, bool),
    kit: &Kit,
    anims: &AnimTable,
    (now, msec): (u64, u32),
    rng: &mut SimRng,
) {
    if !fighter.alive() {
        return;
    }
    let charging = fighter.shooting.charging_from.is_some() || fighter.pack.charging_from.is_some();
    let in_hand = kit.hands[usize::from(fighter.hand.min(1))];
    let saber = &fighter.saber;
    let busy = ps.velocity.iter().any(|&speed| speed != 0.0)
        || command.buttons & ACTION_BUTTONS != 0
        || command.forward != 0
        || command.right != 0
        || command.up != 0
        || fighter.hp != fighter.rest.hp
        || fighter.shooting.aiming
        || charging
        || fighter.weapon_wait_ms > 0
        || fighter.force.hand.is_some()
        || fighter.force.channel.is_some()
        || saber.hand != Hand::None
        || saber.blocked != Blocked::None
        || saber.change != Change::None
        || in_hand == Held::Other
        || fighter.acting.is_some_and(|acting| acting.playing(now))
        || (player && command.weapon != fighter.hand);
    let turn = |axis: usize| ps.view[axis] - fighter.rest.view[axis];
    let (pitch, yaw, roll) = (turn(0), turn(1), turn(2));
    let turned = (pitch * pitch + yaw * yaw + roll * roll).sqrt();
    let stance = matches!(ps.legs.anim, Anim::BOTH_STAND1 | Anim::BOTH_STAND2);
    let at_rest =
        !busy && stance && turned <= VIEW_SLACK && ps.legs.timer <= 0 && ps.torso.timer <= 0;

    if !at_rest {
        if busy {
            break_idle(ps, fighter, charging);
        }
        let rest = &mut fighter.rest;
        rest.hp = fighter.hp;
        rest.view = ps.view;
        if rest.since < now {
            rest.open = true;
        }
        return;
    }

    let rest = &mut fighter.rest;
    if rest.open {
        // The rest began with the step before this one.
        rest.open = false;
        rest.since = now.saturating_sub(u64::from(msec));
    }
    // Still waiting, or an idle still plays out (`since` lies ahead).
    if now.saturating_sub(rest.since) <= REST_MS {
        return;
    }
    let idle = if ps.legs.anim == Anim::BOTH_STAND2 {
        if rng.below(10) < rules::combat::idle::SECOND_IDLE {
            Anim::BOTH_STAND2IDLE2
        } else {
            Anim::BOTH_STAND2IDLE1
        }
    } else {
        Anim::BOTH_STAND1IDLE1
    };
    motion::react(ps, anims, idle, false);
    // The next idle waits for this one to end, and a little more.
    let held = u64::try_from(ps.legs.timer).unwrap_or(0);
    let spread = u64::from(rng.below(SPREAD_MS + 1));
    rest.since = now + held + spread;
}

/// Drops the idle a busy character plays: the parts playing one are let go,
/// a torso's weapon and saber made ready again, and a charging weapon held
/// up.
fn break_idle(ps: &mut MotionState, fighter: &mut Fighter, charging: bool) {
    let legs = IDLES.contains(&ps.legs.anim);
    let torso = IDLES.contains(&ps.torso.anim);
    if legs {
        ps.legs.timer = 0;
    }
    if torso {
        ps.torso.timer = 0;
        fighter.weapon_wait_ms = 0;
        fighter.saber.current_move = Move::READY;
    }
    if (legs || torso) && charging {
        ps.torso.anim = Anim::TORSO_RAISEWEAP1;
    }
}
