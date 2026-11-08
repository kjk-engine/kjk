//! The bot that plays a seat as a player would, with the rule set's
//! controls: it closes in, swings in the openings its foe leaves, holds the
//! block button against a swing it sees coming (MD, MDA), aims the block
//! (attack pressed with block held) to time a perfect block, and kicks.
//!
//! It reads only what a player sees: its foe's saber move (a wind-up, a cut,
//! a recovery), whether it is down, and how far away it is; and its own
//! character as a player knows it (saber lit, the hits it took). Every
//! reaction comes a reaction time after what it reacts to, and the impact
//! it times its perfect press for is what it has learnt of this foe: the
//! time from a wind-up seen to the blow landing, averaged over the blows so
//! far (a prior before the first). Its dice are its own stream, seeded from
//! the fight's seed, so a run reproduces.

use std::collections::BTreeSet;

use sim::combat::Fighter;
use sim::combat::fighter::Outcome;
use sim::combat::saber::{SaberRules, duel};
use sim::{SimId, buttons, holds};

/// What a skill level does (ms throughout).
#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) struct Skill {
    pub name: &'static str,
    /// Reaction time: mean and spread (a normal's deviation).
    pub reaction: (f64, f64),
    /// The chance it blocks a swing it sees coming (MD, MDA).
    pub block: f64,
    /// The chance it tries a perfect block when it blocks.
    pub perfect: f64,
    /// Its press against the blow it expects: how late on average, and the
    /// spread.
    pub timing: (f64, f64),
    /// The chance it waits for an opening (the foe's swing over) rather
    /// than swinging into a swing.
    pub discipline: f64,
    /// The chance it swings, each decision in reach and in an opening.
    pub aggression: f64,
    /// The chance it kicks, each decision within a kick's reach (MD, MDA).
    pub kick: f64,
    /// It swings no more with its mishaps at this (the HUD's lamp, MD and
    /// MDA).
    pub mishap_cap: u8,
}

pub(super) const SKILLS: [Skill; 3] = [
    Skill {
        name: "novice",
        reaction: (350.0, 80.0),
        block: 0.45,
        perfect: 0.3,
        timing: (40.0, 120.0),
        discipline: 0.4,
        aggression: 0.5,
        kick: 0.02,
        mishap_cap: 16,
    },
    Skill {
        name: "average",
        reaction: (220.0, 50.0),
        block: 0.7,
        perfect: 0.6,
        timing: (20.0, 70.0),
        discipline: 0.75,
        aggression: 0.4,
        kick: 0.05,
        mishap_cap: 11,
    },
    Skill {
        name: "expert",
        reaction: (120.0, 25.0),
        block: 0.92,
        perfect: 0.9,
        timing: (0.0, 35.0),
        discipline: 0.95,
        aggression: 0.45,
        kick: 0.08,
        mishap_cap: 8,
    },
];

/// How often it decides to swing or kick.
const DECIDE_MS: u64 = 100;
/// Reach (units): it walks in past `CLOSE`, backs off under `CROWDED`, swings
/// within `SWING`, kicks within `KICK`, minds swings begun within `THREAT`.
const CLOSE: f32 = 60.0;
const CROWDED: f32 = 30.0;
const SWING: f32 = 80.0;
const KICK: f32 = 56.0;
const THREAT: f32 = 140.0;
/// How long after a foe's swing it counts as an opening, and how long a foe
/// that does not swing makes one of its own.
const OPENING_MS: u64 = 700;
const STALE_MS: u64 = 1500;
/// How long it holds attack for a perfect press (past the window), and a
/// click for a swing or a kick.
const PRESS_MS: u64 = 200;
const CLICK_MS: u64 = 40;
/// The longest it holds a block for one swing.
const BLOCK_MOST_MS: u64 = 1500;
/// What it expects a blow to take from its wind-up before it has seen one.
const PRIOR_IMPACT_MS: f64 = 300.0;
/// How much each blow seen moves what it expects.
const LEARN: f64 = 0.25;

/// The directions a swing is pressed with (forward, right).
const SWINGS: [(i8, i8); 7] = [
    (0, 0),
    (127, 0),
    (0, 127),
    (0, -127),
    (127, 127),
    (127, -127),
    (-127, 0),
];

/// A swing of the foe's it is minding.
#[derive(Debug, Clone, Copy)]
struct Threat {
    seen: u64,
    block_at: Option<u64>,
    press_at: Option<u64>,
    /// When it lets go of block (set once it sees the swing end).
    release_at: Option<u64>,
}

/// A player's keys for one tick.
#[derive(Debug, Clone, Copy, Default)]
pub(super) struct Keys {
    pub forward: i8,
    pub right: i8,
    pub buttons: u16,
    pub holds: u8,
}

/// What it tried, for the facts.
#[derive(Debug, Clone, Copy, Default, serde::Serialize)]
pub(super) struct Tries {
    pub swings: u32,
    pub blocks: u32,
    pub perfect_presses: u32,
    pub kicks: u32,
    pub learnt_impact_ms: f64,
    pub impacts_seen: u32,
}

pub(super) struct Bot {
    skill: Skill,
    rules: SaberRules,
    dice: u64,
    impact_ms: f64,
    threat: Option<Threat>,
    /// The foe was winding up or cutting last tick.
    foe_swinging: bool,
    /// When it saw the foe's last swing end (an opening), if it has.
    opening: Option<u64>,
    next_decision: u64,
    /// A swing or kick pressed until then, with these keys.
    click: Option<(u64, Keys)>,
    /// Events on its own fighter already read, by sequence.
    read: BTreeSet<u16>,
    pub tries: Tries,
}

impl Bot {
    pub fn new(skill: Skill, rules: SaberRules, seed: u64) -> Bot {
        Bot {
            skill,
            rules,
            dice: seed ^ 0x9e37_79b9_7f4a_7c15,
            impact_ms: PRIOR_IMPACT_MS,
            threat: None,
            foe_swinging: false,
            opening: None,
            next_decision: 0,
            click: None,
            read: BTreeSet::new(),
            tries: Tries::default(),
        }
    }

    /// The next of its own dice in [0, 1) (splitmix64).
    fn roll(&mut self) -> f64 {
        self.dice = self.dice.wrapping_add(0x9e37_79b9_7f4a_7c15);
        let mut z = self.dice;
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        ((z ^ (z >> 31)) >> 11) as f64 / (1u64 << 53) as f64
    }

    fn chance(&mut self, p: f64) -> bool {
        self.roll() < p
    }

    /// A normal draw of `mean` and `spread` (Box-Muller).
    fn normal(&mut self, (mean, spread): (f64, f64)) -> f64 {
        let (u, v) = (self.roll().max(1e-12), self.roll());
        mean + spread * (-2.0 * u.ln()).sqrt() * (std::f64::consts::TAU * v).cos()
    }

    fn reaction(&mut self) -> u64 {
        self.normal(self.skill.reaction).max(60.0) as u64
    }

    /// Learns from the blows the foe landed or had blocked since it last
    /// looked: the time from the wind-up it saw to each.
    fn learn(&mut self, me: &Fighter, foe: SimId, tick_ms: f64) {
        for event in me.events.iter().flatten() {
            if !self.read.insert(event.sequence) {
                continue;
            }
            let blow = matches!(
                event.result,
                Outcome::Hit | Outcome::Critical | Outcome::Parried
            );
            let Some(threat) = self.threat.filter(|_| blow && event.by == Some(foe)) else {
                continue;
            };
            let took = event.tick as f64 * tick_ms - threat.seen as f64;
            if (0.0..BLOCK_MOST_MS as f64).contains(&took) {
                self.impact_ms += LEARN * (took - self.impact_ms);
                self.tries.impacts_seen += 1;
            }
        }
        self.tries.learnt_impact_ms = self.impact_ms;
    }

    /// This tick's keys at `now`: `me` its own fighter, `foe` the other's,
    /// `apart` the distance between them (units).
    pub fn keys(
        &mut self,
        now: u64,
        tick_ms: f64,
        me: &Fighter,
        foe: (SimId, &Fighter),
        apart: f32,
    ) -> Keys {
        let (foe_id, them) = foe;
        self.learn(me, foe_id, tick_ms);
        let mv = them.saber.current_move;
        let swinging = mv.start() || mv.attack() || mv.kick();
        if swinging && !self.foe_swinging && apart < THREAT {
            self.see_swing(now);
        }
        if !swinging && self.foe_swinging {
            let after = now + self.reaction();
            self.opening = Some(after);
            if let Some(threat) = self.threat.as_mut() {
                threat.release_at = Some(after);
            }
        }
        self.foe_swinging = swinging;
        if let Some(threat) = self.threat
            && (threat.release_at.is_some_and(|at| now >= at) || now > threat.seen + BLOCK_MOST_MS)
        {
            self.threat = None;
        }

        let mut keys = Keys::default();
        // Light the saber with a press, as a player does.
        if !me.saber.on && me.saber.thrown.is_none() {
            keys.buttons |= if now.is_multiple_of(2 * DECIDE_MS) {
                buttons::ATTACK
            } else {
                0
            };
            keys.forward = 127;
            return keys;
        }
        if let Some(threat) = self.threat
            && self.rules.duel()
            && let Some(block_at) = threat.block_at
            && now >= block_at
        {
            keys.holds |= holds::BLOCK;
            if let Some(press) = threat.press_at
                && now >= press
                && now < press + PRESS_MS
            {
                keys.buttons |= buttons::ATTACK;
            }
            return keys;
        }
        if let Some((until, clicked)) = self.click {
            if now < until {
                return clicked;
            }
            self.click = None;
        }
        keys.forward = if apart > CLOSE {
            127
        } else if apart < CROWDED {
            -64
        } else {
            0
        };
        if now >= self.next_decision {
            self.next_decision = now + DECIDE_MS;
            self.decide(now, me, them, apart, keys.forward);
        }
        self.click.map_or(keys, |(_, clicked)| clicked)
    }

    /// A swing of the foe's seen beginning at `now`.
    fn see_swing(&mut self, now: u64) {
        let seen = now;
        let (mut block_at, mut press_at) = (None, None);
        if self.rules.duel() && self.chance(self.skill.block) {
            let at = seen + self.reaction();
            block_at = Some(at);
            self.tries.blocks += 1;
            if self.chance(self.skill.perfect) {
                let half = duel::window() as f64 / 2.0;
                let press = seen as f64 + self.impact_ms - half + self.normal(self.skill.timing);
                press_at = Some((press.max(at as f64)) as u64);
                self.tries.perfect_presses += 1;
            }
        }
        self.opening = None;
        self.threat = Some(Threat {
            seen,
            block_at,
            press_at,
            release_at: None,
        });
    }

    /// Whether to swing or kick now, `forward` its walk.
    fn decide(&mut self, now: u64, me: &Fighter, them: &Fighter, apart: f32, forward: i8) {
        let mv = them.saber.current_move;
        // An opening: the foe down or reeling (bounced, broken, knocked
        // away); under JA's rules also recovering from its swing, the
        // moments after it seen (a duel mode's creature guards then too);
        // or a foe that has not swung for a while (no fight waits for ever).
        let duel = self.rules.duel();
        let reeling = mv.bounce() || mv.broken() || mv.knockaway();
        let recovering = !duel
            && (mv.ret()
                || self
                    .opening
                    .is_some_and(|at| now >= at && now < at + OPENING_MS));
        let open = them.saber.floored()
            || reeling
            || recovering
            || (!self.foe_swinging && self.opening.is_none_or(|at| now >= at + STALE_MS));
        let spent = duel && me.saber.duel.mishap >= self.skill.mishap_cap;
        let busy = self.threat.is_some() || spent;
        let waits = !open || busy;
        if waits && self.chance(self.skill.discipline) {
            return;
        }
        if self.rules.duel() && apart < KICK && !self.foe_swinging && self.chance(self.skill.kick) {
            self.tries.kicks += 1;
            let kick = Keys {
                holds: holds::KICK,
                ..Keys::default()
            };
            self.click = Some((now + CLICK_MS, kick));
            return;
        }
        if apart < SWING && self.chance(self.skill.aggression) {
            self.tries.swings += 1;
            let (ahead, right) = SWINGS[(self.roll() * SWINGS.len() as f64) as usize];
            let swing = Keys {
                forward: if ahead == 0 { forward } else { ahead },
                right,
                buttons: buttons::ATTACK,
                holds: 0,
            };
            self.click = Some((now + CLICK_MS, swing));
        }
    }
}
