//! The Force (I26): the pool and its regeneration (I22 C13), and each
//! power's JA shape, timing and physics with KOTOR's powers deciding who
//! knows what at which level and what a cast costs (C10, C12;
//! `docs/COMBAT.md`). This module owns `ForceState`, its encoding and the
//! powers' numbers; the step's `force` system (`world::force`) runs them.
//!
//! **KOTOR's impact is its script where it runs.** In KOTOR every power's
//! effect on a target is its `spells.2da` impact script (`k_sp1_generic`);
//! where a level has it, the delivery here only finds whom a power catches
//! and the script does the rest (`world::powers`). Where no script can run
//! (a stage without the games' scripts, a test), a power's damage and
//! healing are JA's own numbers on JA's 100-point scale, turned into the
//! target's pools (a JA point is 1% of the target's maximum hit points, or
//! force points), so a power keeps JA's weight against any KOTOR body.
//! Costs are KOTOR's (`spells.2da` `forcepoints`,
//! `ModuleData::force_costs`); the crowd control rows (stun, stasis, fear
//! and their chains) are KOTOR effects with a Will save, capped by C11.
//!
//! How a character casts (`Command::force` and the force buttons):
//! - `buttons::FORCE_POWER` casts the selected power (`Command::force`: a
//!   `spells.2da` row the character knows, or `PULL` for JA's pull, known
//!   with push). Any row of a chain casts the character's best level of it
//!   (I22's chains become JA levels); a crowd-control row casts itself.
//!   Instant and toggled powers act on the press; a toggle pressed again
//!   (`ja::TOGGLE_MIN_MS` or more after it started) stops.
//! - Grip, lightning and drain run while their button is held (JA's own
//!   buttons, or the power button with them selected).

use crate::combat::fighter::{decode_id, encode_id};
use crate::combat::sheet::Rows;
use crate::script::effects::Hold;
use crate::snapshot::SimId;
use crate::wire::{DecodeError, Reader, Writer, read_fixed, write_fixed};

/// The JA powers KOTOR's chains map onto (I22 section 6).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Power {
    Push,
    Pull,
    Lightning,
    Drain,
    Grip,
    Speed,
    /// KOTOR's Cure and Heal: JA's team heal (I22's table).
    Heal,
    Protect,
    Absorb,
    MindTrick,
    /// KOTOR's own crowd control (stun, stasis, fear, …): a KOTOR effect
    /// delivered at the crosshair target or over an area (C11).
    Stun,
    /// TSL's Fury: JA's rage.
    Rage,
    /// TSL's Force Sight: JA's sight.
    Sight,
    /// KOTOR's own effect powers JA has no shape for (`combat::effects`):
    /// Valor on the caster's side, Energy Resistance on the caster (its
    /// side at level 2), Slow's chain and Supress / Breach at the one aimed
    /// at.
    Valor,
    Resist,
    Slow,
    Dispel,
}

/// `Command::force` for JA's pull, which KOTOR has no row for: known at
/// the level of push.
pub const PULL: u8 = 255;

/// The JA power and level a `spells.2da` row gives (I22's chain table,
/// and TSL's own powers where JA has their shape: Drain Force I–III is JA's
/// drain, which takes force points; Master Heal the heal's third level;
/// Force Crush the grip's third; Mind Trick and Confusion the mind trick's
/// two; Force Repulsion the push's third; Force Scream an area stun; Fury
/// JA's rage; Force Sight JA's sight). Valor, Energy Resistance, Slow's
/// chain and Supress / Breach are KOTOR effects delivered at the caster,
/// its side or the one aimed at (`combat::effects`); TSL's powers JA has no
/// shape for (Battle Meditation, the barriers, Revitalize, ...) wait for
/// scripts (I16). Keyed by the rows' labels (`combat::spells`), which K1
/// and TSL share.
pub fn of_row(row: u16) -> Option<(Power, u8)> {
    use crate::combat::spells::*;
    Some(match row {
        FORCE_POWER_DRAIN_FORCE => (Power::Drain, 1),
        FORCE_POWER_IMPROVED_DRAIN_FORCE => (Power::Drain, 2),
        FORCE_POWER_MASTER_DRAIN_FORCE => (Power::Drain, 3),
        FORCE_POWER_MASTER_HEAL => (Power::Heal, 3),
        FORCE_POWER_FORCE_CRUSH => (Power::Grip, 3),
        FORCE_POWER_MIND_TRICK => (Power::MindTrick, 1),
        FORCE_POWER_CONFUSION => (Power::MindTrick, 2),
        FORCE_POWER_FORCE_REPULSION => (Power::Push, 3),
        FORCE_POWER_FORCE_SCREAM
        | FORCE_POWER_IMPROVED_FORCE_SCREAM
        | FORCE_POWER_MASTER_FORCE_SCREAM => (Power::Stun, 1),
        FORCE_POWER_FURY => (Power::Rage, 1),
        FORCE_POWER_IMPROVED_FURY => (Power::Rage, 2),
        FORCE_POWER_MASTER_FURY => (Power::Rage, 3),
        FORCE_POWER_FORCE_SIGHT => (Power::Sight, 1),
        FORCE_POWER_FORCE_PUSH => (Power::Push, 1),
        FORCE_POWER_FORCE_WHIRLWIND => (Power::Push, 2),
        FORCE_POWER_FORCE_WAVE => (Power::Push, 3),
        FORCE_POWER_SHOCK => (Power::Lightning, 1),
        FORCE_POWER_LIGHTNING => (Power::Lightning, 2),
        FORCE_POWER_FORCE_STORM => (Power::Lightning, 3),
        FORCE_POWER_DRAIN_LIFE => (Power::Drain, 2),
        FORCE_POWER_DEATH_FIELD => (Power::Drain, 3),
        FORCE_POWER_WOUND => (Power::Grip, 1),
        FORCE_POWER_CHOKE => (Power::Grip, 2),
        FORCE_POWER_KILL => (Power::Grip, 3),
        FORCE_POWER_SPEED_BURST => (Power::Speed, 1),
        FORCE_POWER_KNIGHT_SPEED => (Power::Speed, 2),
        FORCE_POWER_SPEED_MASTERY => (Power::Speed, 3),
        FORCE_POWER_CURE => (Power::Heal, 1),
        FORCE_POWER_HEAL => (Power::Heal, 2),
        FORCE_POWER_FORCE_AURA => (Power::Protect, 1),
        FORCE_POWER_FORCE_SHIELD => (Power::Protect, 2),
        FORCE_POWER_FORCE_ARMOR => (Power::Protect, 3),
        FORCE_POWER_RESIST_FORCE => (Power::Absorb, 1),
        FORCE_POWER_FORCE_IMMUNITY => (Power::Absorb, 2),
        FORCE_POWER_AFFECT_MIND => (Power::MindTrick, 1),
        FORCE_POWER_DOMINATE => (Power::MindTrick, 2),
        FORCE_POWER_STUN
        | FORCE_POWER_STASIS
        | FORCE_POWER_MASS_STASIS
        | FORCE_POWER_FEAR
        | FORCE_POWER_HORROR
        | FORCE_POWER_INSANITY
        | FORCE_POWER_DROID_STUN
        | FORCE_POWER_DROID_DISABLE
        | FORCE_POWER_DROID_DESTROY => (Power::Stun, 1),
        FORCE_POWER_FORCE_VALOR => (Power::Valor, 1),
        FORCE_POWER_KNIGHT_VALOR => (Power::Valor, 2),
        FORCE_POWER_MASTER_VALOR => (Power::Valor, 3),
        FORCE_POWER_RESIST_ENERGY_1 => (Power::Resist, 1),
        FORCE_POWER_RESIST_ENERGY_2 => (Power::Resist, 2),
        FORCE_POWER_SLOW => (Power::Slow, 1),
        FORCE_POWER_AFFLICTION => (Power::Slow, 2),
        FORCE_POWER_PLAGUE => (Power::Slow, 3),
        FORCE_POWER_SUPRESS_FORCE => (Power::Dispel, 1),
        FORCE_POWER_FORCE_BREACH => (Power::Dispel, 2),
        _ => return None,
    })
}

/// KOTOR's Force power DC for `caster` (the engine's `GetSpellSaveDC`;
/// StrategyWiki's K1 rule): the book's `force.rolls.dc_base` + level +
/// WIS + CHA + Force Focus (`feats::FORCE_FOCUS_DC` by tier).
pub fn save_dc(caster: &crate::combat::Sheet) -> i32 {
    use crate::combat::sheet::{Ability, feats};
    let focus = feats::FORCE_FOCUS
        .iter()
        .zip(feats::FORCE_FOCUS_DC)
        .filter(|(feat, _)| caster.feats.has(**feat))
        .map(|(_, bonus)| bonus)
        .max()
        .unwrap_or(0);
    rules::force::rolls::DC_BASE
        + i32::from(caster.level)
        + caster.modifier(Ability::Wisdom)
        + caster.modifier(Ability::Charisma)
        + focus
}

/// One's Force resistance (`ResistForce`; 0 for none): the book's
/// `force.rolls` base + a share a tier + its level under Resist Force or
/// Force Immunity (absorb `tier` 1 or 2), or what effects grant it
/// (`granted`), the higher. A caster breaks it when d20 + its level
/// reaches it.
pub fn resistance(level: u8, tier: u8, granted: i32) -> i32 {
    let from_absorb = match tier.min(2) {
        0 => 0,
        tier => {
            use rules::force::rolls;
            rolls::RESISTANCE_BASE + rolls::RESISTANCE_PER_TIER * i32::from(tier) + i32::from(level)
        }
    };
    from_absorb.max(granted)
}

/// The crowd-control rows: which reach an area (the rest the crosshair
/// target), which frighten (ended by damage, C11), which only droids
/// feel (the rest only the living), the save each allows and the feat
/// that makes one immune (`k_sp1_generic`'s saves; the Sentinel's Force
/// Immunity feats), by the rows' labels. TSL's area stuns (Force Scream's
/// three) save as Stun does [verify].
pub mod crowd {
    use crate::combat::sheet::feats;
    use crate::combat::spells::*;

    /// The save against `row` (`Sheet::save`'s index): Will for the fear
    /// chain, else Fortitude.
    pub fn save(row: u16) -> usize {
        if fear(row) { 2 } else { 0 }
    }

    /// The `IMMUNITY_TYPE_*` that keeps `row` off (paralysis for stasis,
    /// fear for the fear chain, stun for the rest); mind-affecting
    /// immunity (`MIND_SPELLS`) keeps them all off.
    pub fn immunity_type(row: u16) -> u8 {
        match row {
            FORCE_POWER_FEAR | FORCE_POWER_HORROR | FORCE_POWER_INSANITY => immunity::FEAR,
            FORCE_POWER_STASIS | FORCE_POWER_MASS_STASIS => immunity::PARALYSIS,
            _ => immunity::STUN,
        }
    }

    /// `IMMUNITY_TYPE_*` values.
    pub mod immunity {
        pub const MIND_SPELLS: u8 = 1;
        pub const POISON: u8 = 2;
        pub const FEAR: u8 = 4;
        pub const PARALYSIS: u8 = 6;
        pub const STUN: u8 = 12;
        pub const SLEEP: u8 = 13;
    }

    /// The feat that makes one immune to `row`, if any.
    pub fn immunity(row: u16) -> Option<u16> {
        match row {
            FORCE_POWER_FEAR | FORCE_POWER_HORROR | FORCE_POWER_INSANITY => {
                Some(feats::FORCE_IMMUNITY_FEAR)
            }
            FORCE_POWER_STASIS | FORCE_POWER_MASS_STASIS => Some(feats::FORCE_IMMUNITY_PARALYSIS),
            FORCE_POWER_STUN
            | FORCE_POWER_FORCE_SCREAM
            | FORCE_POWER_IMPROVED_FORCE_SCREAM
            | FORCE_POWER_MASTER_FORCE_SCREAM => Some(feats::FORCE_IMMUNITY_STUN),
            _ => None,
        }
    }

    pub fn area(row: u16) -> bool {
        matches!(
            row,
            FORCE_POWER_MASS_STASIS
                | FORCE_POWER_HORROR
                | FORCE_POWER_INSANITY
                | FORCE_POWER_FORCE_SCREAM
                | FORCE_POWER_IMPROVED_FORCE_SCREAM
                | FORCE_POWER_MASTER_FORCE_SCREAM
        )
    }

    pub fn fear(row: u16) -> bool {
        matches!(
            row,
            FORCE_POWER_FEAR | FORCE_POWER_HORROR | FORCE_POWER_INSANITY
        )
    }

    pub fn droids(row: u16) -> bool {
        matches!(
            row,
            FORCE_POWER_DROID_STUN | FORCE_POWER_DROID_DISABLE | FORCE_POWER_DROID_DESTROY
        )
    }

    /// The sphere (metres) round the one aimed at that `row` also reaches:
    /// `k_sp1_generic`'s Disable Droid and Destroy Droid strike their
    /// target, then every creature within their spheres of it.
    pub fn splash(row: u16) -> Option<f32> {
        match row {
            FORCE_POWER_DROID_DISABLE => Some(rules::force::crowd::DISABLE_SPLASH_M),
            FORCE_POWER_DROID_DESTROY => Some(rules::force::crowd::DESTROY_SPLASH_M),
            _ => None,
        }
    }
}

/// The character's best row of `power`'s chain, and its level (push's for
/// pull); `None` if it knows none.
pub fn best(powers: &Rows<5>, power: Power) -> Option<(u16, u8)> {
    let chain = if power == Power::Pull {
        Power::Push
    } else {
        power
    };
    (0..Rows::<5>::ROWS)
        .filter(|row| powers.has(*row))
        .filter_map(|row| of_row(row).map(|(known, level)| (row, known, level)))
        .filter(|(_, known, _)| *known == chain)
        .max_by_key(|(_, _, level)| *level)
        .map(|(row, _, level)| (row, level))
}

/// The row of `power`'s chain at `level` the character knows, else its best
/// (a channel keeps its level, not its row: the row it runs as).
pub fn row_at(powers: &Rows<5>, power: Power, level: u8) -> Option<u16> {
    (0..Rows::<5>::ROWS)
        .find(|&row| powers.has(row) && of_row(row) == Some((power, level)))
        .or_else(|| best(powers, power).map(|(row, _)| row))
}

/// What the power button casts with `selected` (`Command::force`): the
/// power, its level and the row whose cost it pays. A player's choice
/// (`exact`) casts the row chosen, as KOTOR's quickbar casts the power
/// picked from it (Force Push though Force Wave is known); a creature's
/// casts the best row of its chain.
pub fn selected(powers: &Rows<5>, selected: u8, exact: bool) -> Option<(Power, u8, u16)> {
    if selected == PULL {
        return best(powers, Power::Pull).map(|(row, level)| (Power::Pull, level, row));
    }
    let row = u16::from(selected);
    let (power, level) = of_row(row).filter(|_| powers.has(row))?;
    match power {
        Power::Stun => Some((Power::Stun, 1, row)),
        _ if exact => Some((power, level, row)),
        _ => best(powers, power).map(|(row, level)| (power, level, row)),
    }
}

/// What a player's power button can select (`Command::force`): every
/// power row it knows, each chain's ranks together and in rank order (the
/// chains as `Power` lists them, pull after push when push is known), the
/// rows of one rank by row.
pub fn choices(powers: &Rows<5>) -> Vec<u8> {
    let mut rows: Vec<(Power, u8, u8)> = (0..PULL)
        .filter_map(|row| selected(powers, row, true).map(|(power, level, _)| (power, level, row)))
        .collect();
    if best(powers, Power::Push).is_some() {
        rows.push((Power::Pull, 1, PULL));
    }
    rows.sort();
    rows.into_iter().map(|(.., row)| row).collect()
}

/// A power channelled while its button is held (JA's grip, lightning,
/// drain).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Channel {
    pub power: Power,
    pub level: u8,
    /// When it runs out unless held on (sim ms), and its next tick.
    pub until: u64,
    pub next_tick: u64,
    /// When it started (sim ms): costs past its first run (C12,
    /// `ja::CHANNEL_MS`), grip's lift (`ja::GRIP_LIFT_EVERY_MS`) and its one
    /// crush (`ja::GRIP_CRUSH_MS`).
    pub from: u64,
    /// The grip's victim.
    pub target: Option<SimId>,
}

impl Channel {
    const BLANK: Channel = Channel {
        power: Power::Push,
        level: 1,
        until: 0,
        next_tick: 0,
        from: 0,
        target: None,
    };

    fn encode(&self, out: &mut Writer) {
        out.u8(self.power as u8);
        out.u8(self.level);
        out.u64(self.until);
        out.u64(self.next_tick);
        out.u64(self.from);
        write_fixed(out, self.target.as_ref(), &SimId(0), encode_id);
    }

    fn decode(input: &mut Reader) -> Result<Channel, DecodeError> {
        let at = input.at();
        let power = POWERS
            .get(usize::from(input.u8()?))
            .copied()
            .ok_or(input.invalid("force power", at))?;
        let at = input.at();
        let level = input.u8()?;
        if !(1..=3).contains(&level) {
            return Err(input.invalid("channel level", at));
        }
        Ok(Channel {
            power,
            level,
            until: input.u64()?,
            next_tick: input.u64()?,
            from: input.u64()?,
            target: read_fixed(input, &SimId(0), decode_id, "grip target flag")?,
        })
    }
}

/// A hand held out (JA's `forceHandExtend`): what it plays on the torso
/// while it lasts. Grip, lightning and drain hold theirs while they run
/// (`Channel`); these are the timed ones.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Gesture {
    /// Push, and JA's mind trick and our crowd control, which use its hand.
    Push,
    Pull,
    /// A grip's victim at the crush (the whole body when off the ground).
    Choke,
}

const GESTURES: [Gesture; 3] = [Gesture::Push, Gesture::Pull, Gesture::Choke];

/// What a character's Force is doing (see the module documentation).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ForceState {
    /// Sim ms at which the pool next regains a point.
    pub regen_at: u64,
    /// TSL's regeneration's part of a hundredth not yet gained, in
    /// thousandths (`regenerate_tsl`, the executable's accumulator).
    pub fp_carry: i32,
    /// The force buttons held last step: instant powers fire on a press.
    pub held: u16,
    /// Running toggles: until when (sim ms; 0 off), and the level cast.
    pub speed_until: u64,
    pub protect_until: u64,
    pub absorb_until: u64,
    pub trick_until: u64,
    pub speed_level: u8,
    pub protect_level: u8,
    pub absorb_level: u8,
    pub trick_level: u8,
    /// A channel under way.
    pub channel: Option<Channel>,
    /// A timed hand, and until when (sim ms).
    pub hand: Option<(Gesture, u64)>,
    /// When each may start again (sim ms): push and pull (shared), grip,
    /// lightning, drain.
    pub push_pull_ready: u64,
    pub grip_ready: u64,
    pub lightning_ready: u64,
    pub drain_ready: u64,
    /// Effects on this character: gripped by whom; stunned (C11: at most
    /// one synthetic round), then immune until; what holds it (fear's ends
    /// on damage; the views show each its way); confused (a creature's mind
    /// trick: no enemy) until.
    pub gripped_by: Option<SimId>,
    pub stunned_until: u64,
    pub immune_until: u64,
    pub hold: Hold,
    pub confused_until: u64,
    /// Seats whose players do not see this character (JA's mind trick on
    /// players: presentation; the view hides it), one bit per seat.
    pub tricked: u64,
    /// A force jump is active (past a normal jump's height, until landing:
    /// `motion::force_jump`), and when it next pays.
    pub jumping: bool,
    pub jump_paid_at: u64,
    /// Rage (TSL's Fury): until when and at what level, when it next costs
    /// health, and until when its recovery lasts after it.
    pub rage_until: u64,
    pub rage_level: u8,
    pub rage_paid_at: u64,
    pub recover_until: u64,
    /// Sight: until when (the view shows others through walls; it breaks
    /// mind tricks on its user).
    pub sight_until: u64,
    /// The KOTOR state holding it (`effects::state`; 0 for none): the
    /// highest of its running state effects as last settled
    /// (`effects::settle`), and until when that effect runs. No cap, no
    /// immune second: it lasts as long as its effect.
    pub state: u8,
    pub state_until: u64,
}

impl ForceState {
    /// Holds the character for `ms` under C11: at most a synthetic round
    /// (`ja::STUN_MS`), then immunity (`ja::IMMUNE_MS`); nothing while immune.
    /// Whether it took hold.
    pub fn hold(&mut self, now: u64, ms: u64) -> bool {
        if self.immune_until > now {
            return false;
        }
        self.stunned_until = now + ms.min(ja::STUN_MS);
        self.immune_until = self.stunned_until + ja::IMMUNE_MS;
        true
    }

    /// Whether a power runs that pauses regeneration (JA: any active power
    /// but drain alone).
    pub fn pausing(&self, now: u64) -> bool {
        self.speed_until > now
            || self.protect_until > now
            || self.absorb_until > now
            || self.trick_until > now
            || self.rage_until > now
            || self.sight_until > now
            || self.jumping
            || self
                .channel
                .is_some_and(|channel| channel.power != Power::Drain)
    }

    /// Whether a hand is out: no new hand power starts meanwhile.
    pub fn hand_out(&self, now: u64) -> bool {
        self.hand.is_some_and(|(_, until)| until > now) || self.channel.is_some()
    }

    /// Held still: by C11's hold, or by a KOTOR state that holds
    /// (`effects::state::holds`).
    pub fn stunned(&self, now: u64) -> bool {
        self.stunned_until > now
            || (self.state_until > now && crate::combat::effects::state::holds(self.state))
    }

    /// The absorb level against an incoming power, 0 when it is off.
    pub fn absorbing(&self, now: u64) -> u8 {
        if self.absorb_until > now {
            self.absorb_level
        } else {
            0
        }
    }

    /// Ends the running toggles, the channel and the hand (a death).
    pub fn stop(&mut self) {
        self.speed_until = 0;
        self.protect_until = 0;
        self.absorb_until = 0;
        self.trick_until = 0;
        self.channel = None;
        self.hand = None;
        self.tricked = 0;
        self.jumping = false;
        self.rage_until = 0;
        self.sight_until = 0;
    }

    /// Whether rage runs, and at what level (0 when it does not).
    pub fn raging(&self, now: u64) -> u8 {
        if self.rage_until > now {
            self.rage_level
        } else {
            0
        }
    }

    /// The fire time's factor (the weapon step's fire time): `ja::RAGE_REFIRE` raging,
    /// `ja::RECOVER_REFIRE` in rage's recovery.
    pub fn refire(&self, now: u64) -> f32 {
        if self.rage_until > now {
            ja::RAGE_REFIRE
        } else if self.recover_until > now {
            ja::RECOVER_REFIRE
        } else {
            1.0
        }
    }

    pub(crate) fn encode(&self, out: &mut Writer) {
        out.u64(self.regen_at);
        out.u32(self.fp_carry as u32);
        out.u16(self.held);
        for time in [
            self.speed_until,
            self.protect_until,
            self.absorb_until,
            self.trick_until,
        ] {
            out.u64(time);
        }
        out.bytes(&[
            self.speed_level,
            self.protect_level,
            self.absorb_level,
            self.trick_level,
        ]);
        write_fixed(out, self.channel.as_ref(), &Channel::BLANK, Channel::encode);
        write_fixed(
            out,
            self.hand.as_ref(),
            &(Gesture::Push, 0),
            |(gesture, until), out| {
                out.u8(*gesture as u8);
                out.u64(*until);
            },
        );
        for time in [
            self.push_pull_ready,
            self.grip_ready,
            self.lightning_ready,
            self.drain_ready,
        ] {
            out.u64(time);
        }
        write_fixed(out, self.gripped_by.as_ref(), &SimId(0), encode_id);
        out.u64(self.stunned_until);
        out.u64(self.immune_until);
        out.u8(self.hold.code());
        out.u64(self.confused_until);
        out.u64(self.tricked);
        out.u8(u8::from(self.jumping));
        out.u64(self.jump_paid_at);
        out.u64(self.rage_until);
        out.u8(self.rage_level);
        out.u64(self.rage_paid_at);
        out.u64(self.recover_until);
        out.u64(self.sight_until);
        out.u8(self.state);
        out.u64(self.state_until);
    }

    pub(crate) fn decode(input: &mut Reader) -> Result<ForceState, DecodeError> {
        let regen_at = input.u64()?;
        let fp_carry = input.u32()? as i32;
        let held = input.u16()?;
        let [speed_until, protect_until, absorb_until, trick_until] =
            [input.u64()?, input.u64()?, input.u64()?, input.u64()?];
        let [speed_level, protect_level, absorb_level, trick_level] = input.array()?;
        let channel = read_fixed(input, &Channel::BLANK, Channel::decode, "channel flag")?;
        let hand = read_fixed(
            input,
            &(Gesture::Push, 0),
            |input| {
                let at = input.at();
                let gesture = GESTURES
                    .get(usize::from(input.u8()?))
                    .copied()
                    .ok_or(input.invalid("gesture", at))?;
                Ok((gesture, input.u64()?))
            },
            "hand flag",
        )?;
        let [push_pull_ready, grip_ready, lightning_ready, drain_ready] =
            [input.u64()?, input.u64()?, input.u64()?, input.u64()?];
        let gripped_by = read_fixed(input, &SimId(0), decode_id, "grip flag")?;
        let stunned_until = input.u64()?;
        let immune_until = input.u64()?;
        let at = input.at();
        let hold = Hold::of_code(input.u8()?).ok_or(input.invalid("hold", at))?;
        let confused_until = input.u64()?;
        let tricked = input.u64()?;
        let at = input.at();
        let jumping = match input.u8()? {
            0 => false,
            1 => true,
            _ => return Err(input.invalid("force jump flag", at)),
        };
        Ok(ForceState {
            regen_at,
            fp_carry,
            held,
            speed_until,
            protect_until,
            absorb_until,
            trick_until,
            speed_level,
            protect_level,
            absorb_level,
            trick_level,
            channel,
            hand,
            push_pull_ready,
            grip_ready,
            lightning_ready,
            drain_ready,
            gripped_by,
            stunned_until,
            immune_until,
            hold,
            confused_until,
            tricked,
            jumping,
            jump_paid_at: input.u64()?,
            rage_until: input.u64()?,
            rage_level: input.u8()?,
            rage_paid_at: input.u64()?,
            recover_until: input.u64()?,
            sight_until: input.u64()?,
            state: input.u8()?,
            state_until: input.u64()?,
        })
    }
}

/// `Power`s in their numbering (`as u8`).
const POWERS: [Power; 13] = [
    Power::Push,
    Power::Pull,
    Power::Lightning,
    Power::Drain,
    Power::Grip,
    Power::Speed,
    Power::Heal,
    Power::Protect,
    Power::Absorb,
    Power::MindTrick,
    Power::Stun,
    Power::Rage,
    Power::Sight,
];

/// How long regaining one point takes (ms): the whole pool in the book's
/// `force.pool.refill_ms` (C13).
pub fn regen_every(max_fp: u16) -> u64 {
    (rules::force::pool::REFILL_MS / u64::from(max_fp.max(1))).max(1)
}

/// TSL's feat that adds to Force point regeneration (`feat.2da` 143,
/// `REGENERATE_FORCE_POINTS`), and how much.
pub const REGENERATE_FEAT: u16 = 143;
pub const REGENERATE_FEAT_RATE: f32 = rules::force::tsl_regeneration::FORCE_FEAT_RATE;

/// TSL's Force point regeneration for one step of `msec` (its executable's
/// `HandleFPRegeneration`, which replaces C13 in TSL): a rate of the
/// table's out-of-combat base, or in a real fight its in-combat base (Niman
/// and Force Affinity a share of the other, `forms::fp_base`); plus its
/// Wisdom modifier times the table's bonus, `REGENERATE_FEAT_RATE` with
/// `REGENERATE_FEAT`, each regeneration effect's percent (`effect_percent`)
/// and Force Focus's (`forms::fp_bonus`); then the room's Force rating
/// (`rating`) takes that share off for the dark or adds it for the light
/// (the book's `force.tsl_regeneration` alignments). The pool and its temporary points over it (`over`,
/// whole; the map reads the executable's `maxFP + stats[+0x18a]`, that
/// field unnamed: [verify] whether it is the temporary points) fill at that
/// rate a `table.time` seconds, by hundredths kept in
/// an accumulator (`ForceState::fp_carry`); within 0 and the pool and
/// those points. A negative rate drains it.
#[allow(clippy::too_many_arguments)]
pub fn regenerate_tsl(
    state: &mut ForceState,
    fp: &mut i32,
    sheet: &crate::combat::Sheet,
    table: crate::script::FpRegeneration,
    (fighting, effect_percent, rating, over): (bool, i32, i32, i32),
    msec: u32,
) {
    use crate::combat::forms;
    use crate::combat::sheet::Ability;
    const DARK: u8 = rules::force::tsl_regeneration::DARK_BELOW;
    const LIGHT: u8 = rules::force::tsl_regeneration::LIGHT_FROM;
    let mut rate = if fighting {
        forms::fp_base(sheet, table.out_of_combat, table.in_combat)
    } else {
        table.out_of_combat
    };
    rate += table.wis_bonus * sheet.modifier(Ability::Wisdom) as f32;
    if sheet.feats.has(REGENERATE_FEAT) {
        rate += REGENERATE_FEAT_RATE;
    }
    rate += effect_percent as f32 / 100.0;
    rate += forms::fp_bonus(sheet);
    let shift = rating as f32 * rate / 100.0;
    match sheet.good_evil {
        ..DARK => rate -= shift,
        LIGHT.. => rate += shift,
        _ => {}
    }
    let most = i32::from(sheet.force_points()) + over.max(0);
    let per_second = rate / table.time.max(0.0001) * most as f32;
    let gained = per_second * 100.0 * msec as f32 / 1000.0 + state.fp_carry as f32 / 1000.0;
    let whole = gained.trunc();
    state.fp_carry = ((gained - whole) * 1000.0).round() as i32;
    *fp = (*fp + whole as i32).clamp(0, most * 100);
}

/// One step of regeneration (C13, JA's power upkeep): a point
/// each `regen_every`, up to the maximum; while `paused` (or full) the
/// clock starts over, so it resumes a period after the pause ends.
pub fn regenerate(state: &mut ForceState, fp: &mut i32, max_fp: u16, paused: bool, now: u64) {
    let every = regen_every(max_fp);
    let max = i32::from(max_fp) * 100;
    if paused || *fp >= max {
        state.regen_at = state.regen_at.max(now + every);
        return;
    }
    while state.regen_at <= now && *fp < max {
        *fp = (*fp + 100).min(max);
        state.regen_at += every;
    }
}

/// A channel's cost this step (C12, hundredths of an FP): nothing for its
/// first run (`ja::CHANNEL_MS`: the cast paid it), then `cost` per second
/// held.
pub fn channel_cost(cost: i32, from: u64, now: u64, msec: u32) -> i32 {
    if now < from + ja::CHANNEL_MS {
        0
    } else {
        (cost * i32::try_from(msec).unwrap_or(0) / 1000).max(1)
    }
}

/// Protect at `level` against a hit of `damage` hundredths of health on one
/// with `max_hp` most health and `fp` of its `max_fp` pool (hundredths):
/// the hundredths it prevents and the hundredths of pool it pays. JA counts
/// in its points (1% of the most): a share of the hit up to the level's cap
/// is stopped at the level's rate of pool, and what the pool cannot pay
/// gets through; the part above the cap gets through free (QUIRKS.md
/// Q-0033).
pub fn protect(level: u8, damage: i32, max_hp: u16, fp: i32, max_fp: u16) -> (i32, i32) {
    let index = usize::from(level.clamp(1, 3) - 1);
    let max_hp = i64::from(max_hp.max(1));
    let max_fp = i64::from(max_fp.max(1));
    let damage = i64::from(damage);
    // Hundredths of JA points.
    let hit = damage * 100 / max_hp;
    let counted = hit.min(ja::PROTECT_CAP[index] * 100);
    let owed = counted * ja::PROTECT_RATE[index] / 1000;
    let stopped = counted * ja::PROTECT_SHARE[index] / 1000;
    let pool = i64::from(fp) * 100 / max_fp;
    let paid = owed.min(pool);
    let prevented = stopped - (owed - paid);
    let prevented = (prevented.max(0) * max_hp / 100).min(damage);
    (prevented as i32, (paid * max_fp / 100) as i32)
}

/// JA's numbers for the powers (I23 section 4), from the book's chapter
/// `force` (`data/rules/force.toml`).
pub mod ja {
    use rules::force::absorb;
    use rules::force::channel;
    use rules::force::crowd;
    use rules::force::drain;
    use rules::force::grip;
    use rules::force::heal;
    use rules::force::lightning;
    use rules::force::mind_trick;
    use rules::force::protect;
    use rules::force::push;
    use rules::force::rage;
    use rules::force::sight;
    use rules::force::speed;

    pub const TICK_MS: u64 = channel::TICK_MS;
    pub const CHANNEL_MS: u64 = channel::RUN_MS;
    pub const DELAY_MS: [u64; 2] = channel::DELAY_MS;
    pub const LINE_RANGE: f32 = channel::LINE_RANGE;
    pub const CONE_DOT: f32 = channel::CONE_DOT;
    pub const IMPACT_EVERY_MS: u64 = channel::IMPACT_EVERY_MS;
    pub const LIGHTNING_RADIUS: f32 = lightning::RADIUS;
    pub const LIGHTNING_POINTS: i32 = lightning::POINTS;
    pub const LIGHTNING_EXTRA: u8 = lightning::EXTRA;
    pub const DRAIN_RADIUS: f32 = drain::RADIUS;
    pub const DRAIN: [i32; 3] = drain::POINTS;
    pub const DRAIN_CASTER_WAIT_MS: u64 = drain::CASTER_WAIT_MS;
    pub const DRAIN_VICTIM_WAIT_MS: u64 = drain::VICTIM_WAIT_MS;
    pub const GRIP_RANGE: f32 = grip::RANGE;
    pub const GRIP_POINTS: i32 = grip::POINTS;
    pub const GRIP_SQUEEZE_MS: u64 = grip::SQUEEZE_EVERY_MS;
    pub const GRIP_HOLD_MS: [u64; 3] = grip::HOLD_MS;
    pub const GRIP_CRUSH_MS: u64 = grip::CRUSH_MS;
    pub const GRIP_CRUSH: [i32; 3] = grip::CRUSH;
    pub const GRIP_CRUSH_HAND_MS: u64 = grip::CRUSH_HAND_MS;
    pub const GRIP_FRONT_DOT: f32 = grip::FRONT_DOT;
    pub const GRIP_LIFT_EVERY_MS: u64 = grip::LIFT_EVERY_MS;
    pub const GRIP_LIFT: f32 = grip::LIFT;
    pub const GRIP_CARRY_AHEAD: f32 = grip::CARRY_AHEAD;
    pub const GRIP_CARRY_UP: f32 = grip::CARRY_UP;
    pub const GRIP_CARRY_WITHIN: [f32; 4] = grip::CARRY_WITHIN;
    pub const GRIP_CARRY_SPEEDS: [f32; 4] = grip::CARRY_SPEEDS;
    pub const GRIP_CARRY_BEYOND: f32 = grip::CARRY_BEYOND;
    pub const GRIP_COOLDOWN_MS: u64 = grip::COOLDOWN_MS;
    pub const GRIP_BROKEN_MS: u64 = grip::BROKEN_MS;
    pub const GRIPPING_SCALE: f32 = grip::GRIPPING_SCALE;
    pub const GRIPPED_SCALE: [f32; 2] = grip::GRIPPED_SCALE;
    pub const PUSH_PULL_AIMED: f32 = push::AIMED_REACH;
    pub const PUSH_PULL_RANGE: f32 = push::AREA_REACH;
    pub const PUSH_PULL_FIELD: [f32; 2] = push::FIELD;
    pub const PUSH_PULL_FRONT: f32 = push::FRONT_DOT;
    pub const PUSH_PULL_STRENGTH: f32 = push::STRENGTH;
    pub const SHOVE_FALLOFF: f32 = push::FALLOFF;
    pub const SHOVE_MIN: f32 = push::LEAST_SHOVE;
    pub const SHOVE_LIFT: f32 = push::LEAST_LIFT;
    pub const PUSH_HAND_MS: u64 = push::PUSH_HAND_MS;
    pub const PULL_HAND_MS: u64 = push::PULL_HAND_MS;
    pub const PUSH_COOLDOWN_MS: u64 = push::PUSH_COOLDOWN_MS;
    pub const PULL_COOLDOWN_MS: u64 = push::PULL_COOLDOWN_MS;
    pub const KNOCKDOWN_MS: u64 = push::KNOCKDOWN_MS;
    pub const KNOCKDOWN_REACH: f32 = push::KNOCKDOWN_REACH;
    pub const COUNTERED: [f32; 3] = push::COUNTERED;
    pub const WHIRLWIND_DIVISOR: u16 = rules::force::push::whirlwind::LEVEL_DIVISOR;
    pub const WHIRLWIND_LEAST: u16 = rules::force::push::whirlwind::LEAST_DAMAGE;
    pub const ABSORB_THROW: i32 = absorb::THROW_GAIN;
    pub const ABSORB_TICK: i32 = absorb::TICK_GAIN;
    pub const SPEED_SCALE: f32 = speed::SCALE;
    pub const SPEED_MS: [u64; 3] = speed::LENGTH_MS;
    pub const TOGGLE_MIN_MS: u64 = rules::force::casting::TOGGLE_MIN_MS;
    pub const GUARD_MS: u64 = protect::LENGTH_MS;
    pub const PROTECT_SHARE: [i64; 3] = protect::SHARE;
    pub const PROTECT_RATE: [i64; 3] = protect::RATE;
    pub const PROTECT_CAP: [i64; 3] = protect::CAP;
    pub const HEAL_RANGE: [f32; 3] = heal::REACH;
    pub const HEAL: [i32; 3] = heal::POINTS;
    pub const TRICK_AIMED: f32 = mind_trick::AIMED_REACH;
    pub const TRICK_AREA: f32 = mind_trick::AREA_REACH;
    pub const TRICK_FIELD: f32 = mind_trick::AREA_FIELD;
    pub const TRICK_MS: [u64; 3] = mind_trick::LENGTH_MS;
    pub const CONFUSE_MS: [u64; 3] = mind_trick::CONFUSE_MS;
    /// The hand a cast puts out (mind trick's, and every KOTOR power's).
    pub const TRICK_HAND_MS: u64 = rules::force::casting::HAND_MS;
    pub const STUN_RANGE: f32 = crowd::REACH;
    pub const STUN_FIELD: f32 = crowd::AREA_FIELD;
    pub const STUN_MS: u64 = crowd::HOLD_MS;
    pub const IMMUNE_MS: u64 = crowd::IMMUNE_MS;
    pub const RAGE_MS: [u64; 3] = rage::LENGTH_MS;
    pub const RAGE_HP_EVERY_MS: [u64; 3] = rage::HP_EVERY_MS;
    pub const RAGE_HP: i32 = rage::HP;
    pub const RAGE_LEAST: i32 = rage::LEAST;
    pub const RAGE_FLOOR_HP: i32 = rage::FLOOR_HP;
    pub const RECOVER_MS: u64 = rage::RECOVER_MS;
    pub const RAGE_SPEED: f32 = rage::SPEED;
    pub const RECOVER_SPEED: f32 = rage::RECOVER_SPEED;
    pub const RAGE_REFIRE: f32 = rage::REFIRE;
    pub const RECOVER_REFIRE: f32 = rage::RECOVER_REFIRE;
    pub const SIGHT_MS: [u64; 3] = sight::LENGTH_MS;
}
