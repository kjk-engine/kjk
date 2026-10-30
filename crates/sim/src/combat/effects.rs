//! KOTOR's effects in a real-time fight (`docs/COMBAT.md`:
//! "JA delivers, KOTOR decides"): what KOTOR's powers add to the
//! JA powers they ride on, and KOTOR's own effect powers, with the numbers
//! of K1's power descriptions (`spells.2da` `spelldesc`, the book's chapter
//! `force_effects`), standing in for `k_sp1_generic` where its scripts do
//! not run:
//!
//! - **Speed** (Burst, Knight, Master): defence; Knight and Master Speed's
//!   extra attacks a round scale each weapon hit's budget (C2: two attacks
//!   a round double each hit).
//! - **Protect** (Aura, Shield, Armor): defence and saves.
//! - **Valor** (Force, Knight, Master Valor; 22, 33, 36): STR, DEX, CON and
//!   saves, the caster and its side within `PARTY_REACH`; Knight and Master
//!   Valor make them immune to poison.
//! - **Energy Resistance** (42, Improved 40): resistance to sonic, fire,
//!   cold and electrical damage; the improved one for its side within
//!   `PARTY_REACH`, and immune to poison.
//! - **Slow** (45), **Affliction** (7), **Plague** (38), on the one aimed
//!   at (not droids): Slow's penalty to defence, Reflex and attack unless a
//!   Will save (`force::save_dc`); Affliction a point of STR, DEX and CON
//!   now and then, slowed (`SLOWED_SPEED`) meanwhile, unless a Fortitude
//!   save at `AFFLICTION_DC`; Plague faster and further, slowed, no save.
//!   The points come back when it ends.
//! - **Supress Force** (48) and **Force Breach** (19), on the one aimed at:
//!   end its first- and second-tier powers (Supress) or all of them
//!   (Breach): speed, protect, valor, energy resistance, absorb.
//! - **Whirlwind** (27, push's second level): each caught makes a Reflex
//!   save (`force::save_dc`) or is held (C11) and takes a share of the
//!   caster's level in damage (`ja::WHIRLWIND_DIVISOR`).
//!
//! And the lasting effects scripts apply (`script::effects`): up to
//! `LASTING_SLOTS` on a character (`Lasting`; a new one past them takes
//! the place of the one ending soonest), folded into what it is
//! (`sheet_now`): abilities, defence, saves, attack, damage, resistance
//! and immunity by damage type, skills; speed, extra attacks, Force
//! resistance and immunities where those are read. TSL's effects among
//! them (read from its executable): KOTOR's states (`state`, the highest
//! holding, settled each step: `settle`), blindness, Fury's kills
//! (`Effects::fury`), Force Sight's and Fury's video rows, Force Body's
//! split of a cast's cost (`pay`), Body Fuel and regeneration
//! (`regenerate`), a faction modifier, the vitality regeneration modifier.

use crate::combat::Fighter;
use crate::combat::force::{ForceState, Power, of_row};
use crate::combat::items;
use crate::combat::sheet::{Ability, Sheet, type_rows};
use crate::wire::{DecodeError, Reader, Writer, read_fixed, write_fixed};

pub const SPEED_DEFENCE: [i8; 3] = rules::force_effects::speed::DEFENCE;
pub const SPEED_ATTACKS: [i32; 3] = rules::force_effects::speed::ATTACKS;
pub const PROTECT_BONUS: [i8; 3] = rules::force_effects::protect::BONUS;
pub const VALOR_BONUS: [u8; 3] = rules::force_effects::valor::BONUS;
pub const VALOR_MS: u64 = rules::force_effects::valor::LENGTH_MS;
pub const RESIST: u8 = rules::force_effects::energy_resistance::RESISTANCE;
/// Energy Resistance's types (`DAMAGE_TYPES` rows: cold, electrical, fire,
/// sonic).
pub const RESIST_TYPES: [u8; 4] = [
    type_rows::COLD,
    type_rows::ELECTRICAL,
    type_rows::FIRE,
    type_rows::SONIC,
];
pub const RESIST_MS: u64 = rules::force_effects::energy_resistance::LENGTH_MS;
/// How far a party power reaches (units).
pub const PARTY_REACH: f32 = rules::force_effects::valor::PARTY_REACH_M / collision::UNIT;
pub const SLOW_PENALTY: i8 = rules::force_effects::slow::PENALTY;
pub const SLOW_MS: u64 = rules::force_effects::slow::LENGTH_MS;
/// Affliction and Plague: the most points sapped, and a point every so
/// often (ms).
pub const SAP_MOST: [u8; 2] = rules::force_effects::slow::SAP_MOST;
pub const SAP_EVERY_MS: [u32; 2] = rules::force_effects::slow::SAP_EVERY_MS;
pub const AFFLICTION_DC: i32 = rules::force_effects::slow::AFFLICTION_DC;
/// Movement under Affliction or Plague.
pub const SLOWED_SPEED: f32 = rules::force_effects::slow::MOVEMENT;
/// How far scripts' speed effects may move one's speed (percent).
pub const SPEED_LEAST: i32 = rules::force_effects::lasting::SPEED_LEAST;
pub const SPEED_MOST: i32 = rules::force_effects::lasting::SPEED_MOST;

/// What a lasting script effect changes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Modifies {
    Ability,
    Defence,
    Save,
    Attack,
    Damage,
    Resistance,
    DamageImmunity,
    Immunity,
    Speed,
    Attacks,
    Skill,
    AssuredHit,
    ForceResistance,
    /// `EffectTemporaryForcePoints`: points over its pool while it lasts.
    ForcePoints,
    /// Nothing: a visual effect lasting (`EffectVisualEffect`,
    /// `EffectBeam`), its `visualeffects.2da` row the amount, kept so that
    /// scripts see it and removing it ends it for the views (`visuals`).
    Visual,
    /// `EffectTemporaryHitpoints`: hit points taken before its own while it
    /// lasts (`Pack::vitality`).
    Vitality,
    /// A poison on it (its pack's, `items::Poisoned`), as scripts walk it
    /// among its effects (`POISON_ID`): an antidote's `RemoveEffect`.
    Poison,
    /// `EffectDisguise`: drawn as the `appearance.2da` row its amount
    /// names while it lasts, over a worn item's disguise (TSL's prologue
    /// makes the Exile's body in the medbay look like the player,
    /// `a_bet3m4`).
    Disguise,
    /// A KOTOR state (the executable's `SETSTATE` effect, `state`): its
    /// number the `of`, its amount 1 for a cutscene's (`EffectCutScene…`,
    /// which no immunity keeps off). The highest running holds.
    State,
    /// TSL's `EffectBlind`: it neither sees nor hears others while it
    /// lasts.
    Blind,
    /// `EffectTrueSeeing`: kept and walked, nothing more (no invisibility
    /// or darkness here to see through; no shipped script applies one).
    TrueSeeing,
    /// TSL's `EffectFury`: each kill its bearer makes adds a point to its
    /// weapons' damage, up to `FURY_MOST` (`Effects::fury`); the controlled
    /// player's view takes Fury's video effect (`Effects::video_row`).
    Fury,
    /// TSL's `EffectForceSight`: the controlled player's view takes Force
    /// Sight's video effect.
    ForceSight,
    /// TSL's `EffectForceBody`: its level (0 to 2) the `of`; a power's cost
    /// is paid half or less from Force points and as much from hit points
    /// (`force_body_share`).
    ForceBody,
    /// `EffectBodyFuel`: the Force points full at each of its bearer's
    /// combat rounds' ends.
    BodyFuel,
    /// `EffectPsychicStatic`: inert in TSL's executable (nothing reads it).
    PsychicStatic,
    /// TSL's `EffectFactionModifier`: its bearer of the standard faction
    /// its amount names while it lasts, the one it had before in `of`.
    Faction,
    /// TSL's `EffectVPRegenModifier`: its amount a percentage the
    /// executable never reads; each running one doubles natural healing.
    VpRegen,
    /// TSL's `EffectFPRegenModifier`: percent added to the Force point
    /// regeneration's rate (`world::powers`).
    FpRegen,
    /// `EffectRegenerate`: its `of` hit points every amount ms (from when
    /// it took hold, `Lasting::at`, the last time it healed after).
    Regenerate,
}

/// The id a character's poison goes by among its effects (`GetFirstEffect`,
/// `RemoveEffect`): it is its pack's, not in a lasting slot.
pub const POISON_ID: u32 = u32::MAX - 1;

impl Modifies {
    const ALL: [Modifies; 30] = [
        Modifies::Ability,
        Modifies::Defence,
        Modifies::Save,
        Modifies::Attack,
        Modifies::Damage,
        Modifies::Resistance,
        Modifies::DamageImmunity,
        Modifies::Immunity,
        Modifies::Speed,
        Modifies::Attacks,
        Modifies::Skill,
        Modifies::AssuredHit,
        Modifies::ForceResistance,
        Modifies::ForcePoints,
        Modifies::Visual,
        Modifies::Vitality,
        Modifies::Poison,
        Modifies::Disguise,
        Modifies::State,
        Modifies::Blind,
        Modifies::TrueSeeing,
        Modifies::Fury,
        Modifies::ForceSight,
        Modifies::ForceBody,
        Modifies::BodyFuel,
        Modifies::PsychicStatic,
        Modifies::Faction,
        Modifies::VpRegen,
        Modifies::FpRegen,
        Modifies::Regenerate,
    ];

    pub fn index(self) -> u8 {
        Modifies::ALL.iter().position(|m| *m == self).unwrap_or(0) as u8
    }

    pub fn of_index(index: u8) -> Option<Modifies> {
        Modifies::ALL.get(usize::from(index)).copied()
    }
}

/// A save a lasting effect changes: Fortitude, Reflex, Will, or all.
pub const ALL_SAVES: u8 = 3;

/// One lasting effect on a character: what it changes, by how much, of
/// what (an `Ability`, a save (0 Fortitude, 1 Reflex, 2 Will,
/// `ALL_SAVES`), a damage type row, a skill row, an `IMMUNITY_TYPE_*`),
/// until when (sim ms; `u64::MAX` until removed), from which `spells.2da`
/// row (`u16::MAX` for none) and whom, its id (`RemoveEffect`), and when it
/// took hold (sim ms; a regeneration's last healing after it,
/// `Modifies::Regenerate`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Lasting {
    pub modifies: Modifies,
    pub amount: i16,
    pub of: u8,
    pub until: u64,
    pub spell: u16,
    pub by: u32,
    pub id: u32,
    pub at: u64,
}

impl Lasting {
    const BLANK: Lasting = Lasting {
        modifies: Modifies::Ability,
        amount: 0,
        of: 0,
        until: 0,
        spell: 0,
        by: 0,
        id: 0,
        at: 0,
    };

    fn encode(&self, out: &mut Writer) {
        out.u8(self.modifies.index());
        out.u16(self.amount as u16);
        out.u8(self.of);
        out.u64(self.until);
        out.u16(self.spell);
        out.u32(self.by);
        out.u32(self.id);
        out.u64(self.at);
    }

    fn decode(input: &mut Reader) -> Result<Lasting, DecodeError> {
        let at = input.at();
        let modifies =
            Modifies::of_index(input.u8()?).ok_or_else(|| input.invalid("effect kind", at))?;
        Ok(Lasting {
            modifies,
            amount: input.u16()? as i16,
            of: input.u8()?,
            until: input.u64()?,
            spell: input.u16()?,
            by: input.u32()?,
            id: input.u32()?,
            at: input.u64()?,
        })
    }
}

/// KOTOR's states (TSL's executable: its `SETSTATE` effect, whose first
/// integer is the state's number and second a cutscene flag; the
/// `EffectStunned`, `EffectParalyze`, … family builds it). Several may run;
/// the highest number holds (mind trick over horror over stun), its taking
/// hold clearing its bearer's actions, its end running the bearer's
/// end-of-round script (the AI resumes).
pub mod state {
    use super::super::sheet::feats;
    use crate::combat::force::crowd::immunity;

    pub const CONFUSED: u8 = 1;
    pub const FRIGHTENED: u8 = 2;
    pub const DROID_STUN: u8 = 3;
    pub const STUNNED: u8 = 4;
    pub const PARALYZED: u8 = 5;
    pub const SLEEP: u8 = 6;
    pub const CHOKE: u8 = 7;
    pub const HORRIFIED: u8 = 8;
    pub const WHIRLWIND: u8 = 10;
    pub const CRUSH: u8 = 15;
    pub const DROID_CONFUSED: u8 = 16;
    pub const MIND_TRICK: u8 = 18;
    pub const DROID_SCRAMBLE: u8 = 19;

    /// `IMMUNITY_TYPE_*`s these states' checks name beyond `immunity`'s.
    const DAZED: u8 = 18;
    const KNOCKDOWN: u8 = 28;
    const DROID_CONFUSION: u8 = 33;
    /// TSL's Mandalorian Courage (`feat.2da` 200): fear does not take.
    const MANDALORIAN_COURAGE: u16 = 200;

    /// Whether `state` holds its bearer still, unable to act: the states
    /// whose taking hold clears its "may act" AI bit (3 to 8, 10, 14, 15,
    /// 17 to 19); confusion (1, 16) and fright (2) leave it acting.
    pub fn holds(state: u8) -> bool {
        matches!(state, 3..=8 | 10 | 14 | 15 | 17..=19)
    }

    /// Whether `state`'s end runs its bearer's end-of-round script (the
    /// executable's mask: 1, 3 to 10, 14 to 19).
    pub fn ends_round(state: u8) -> bool {
        matches!(state, 1 | 3..=10 | 14..=19)
    }

    /// The visual a state shows while it holds (`visualeffects.2da`):
    /// `VFX_IMP_DROID_DISABLE` for a droid's stun, `VFX_DUR_STUN` for a
    /// stun, a mind trick and a droid's scramble.
    pub fn visual(state: u8) -> Option<u16> {
        match state {
            DROID_STUN => Some(1007),
            STUNNED | MIND_TRICK | DROID_SCRAMBLE => Some(2002),
            _ => None,
        }
    }

    /// `GetEffectType`'s `EFFECT_TYPE_*` for a state (the executable's
    /// table by state): 0 for choke, horror, whirlwind, crush and the rest
    /// it does not name. A cutscene's reports as the plain one.
    pub fn effect_type(state: u8) -> i32 {
        match state {
            CONFUSED => 24,
            FRIGHTENED => 25,
            DROID_STUN => 35,
            STUNNED => 29,
            PARALYZED => 27,
            SLEEP => 30,
            DROID_CONFUSED => 79,
            MIND_TRICK => 80,
            DROID_SCRAMBLE => 81,
            _ => 0,
        }
    }

    /// The immunities that keep `state` off as a link (`gameeffects.2da`'s
    /// row the executable picks for it: confused, frightened, stun, sleep
    /// and mind trick are refused by mind, stun or dazed immunity, a droid's
    /// stun and scramble by stun or dazed, paralysis by paralysis or
    /// entangle, horror by mind or fear); none for choke, crush, a droid's
    /// confusion and the rest.
    pub fn link_immunities(state: u8) -> &'static [u8] {
        use immunity::{FEAR, MIND_SPELLS, PARALYSIS, STUN};
        const ENTANGLE: u8 = 10;
        match state {
            CONFUSED | FRIGHTENED | STUNNED | SLEEP | MIND_TRICK => &[MIND_SPELLS, STUN, DAZED],
            DROID_STUN | DROID_SCRAMBLE => &[STUN, DAZED],
            PARALYZED => &[PARALYSIS, ENTANGLE],
            HORRIFIED => &[MIND_SPELLS, FEAR],
            _ => &[],
        }
    }

    /// Whether `state` is refused on one immune to an `IMMUNITY_TYPE_*`
    /// (`immune`) or with a feat (`has`): a cutscene's never but by sleep
    /// immunity; else its link's immunities, then its own (sleep's,
    /// paralysis's or Force Immunity: Paralysis, a mind trick's mind
    /// immunity, stun's or Force Immunity: Stun or Paralysis, confusion's,
    /// fright's or Mandalorian Courage, a droid's confusion's, horror's
    /// Force Immunity: Fear or Mandalorian Courage, knockdown's).
    pub fn refused(
        state: u8,
        cutscene: bool,
        immune: impl Fn(u8) -> bool,
        has: impl Fn(u16) -> bool,
    ) -> bool {
        if state == SLEEP && immune(immunity::SLEEP) {
            return true;
        }
        if cutscene {
            return false;
        }
        link_immunities(state).iter().any(|kind| immune(*kind))
            || match state {
                PARALYZED => immune(immunity::PARALYSIS) || has(feats::FORCE_IMMUNITY_PARALYSIS),
                MIND_TRICK => immune(immunity::MIND_SPELLS),
                STUNNED => {
                    immune(immunity::STUN)
                        || has(feats::FORCE_IMMUNITY_STUN)
                        || has(feats::FORCE_IMMUNITY_PARALYSIS)
                }
                CONFUSED => immune(16),
                FRIGHTENED => immune(immunity::FEAR) || has(MANDALORIAN_COURAGE),
                DROID_CONFUSED => immune(DROID_CONFUSION),
                HORRIFIED => has(feats::FORCE_IMMUNITY_FEAR) || has(MANDALORIAN_COURAGE),
                14 => immune(KNOCKDOWN),
                _ => false,
            }
    }

    /// Whether `state` breaks when its bearer is hurt, attacked or meets a
    /// hostile near (a mind trick, a droid's scramble).
    pub fn fragile(state: u8) -> bool {
        matches!(state, MIND_TRICK | DROID_SCRAMBLE)
    }

    /// How near a hostile it perceives breaks a fragile state (metres,
    /// squared: the executable compares the squared distance).
    pub const FRAGILE_REACH_SQUARED: f32 = rules::force_effects::states::FRAGILE_REACH_SQUARED;
}

/// The most Fury's kills add to its bearer's damage.
pub const FURY_MOST: u8 = rules::force_effects::fury::MOST;

/// Force Body's share of a power's cost taken from Force points (and as
/// much from hit points), in percent, by its level.
pub const FORCE_BODY_SHARE: [i32; 3] = rules::force_effects::force_body::SHARE;
/// The hit points a cast under Force Body must leave more than.
pub const FORCE_BODY_LEAST_HP: i32 = rules::force_effects::force_body::LEAST_HP_LEFT;

/// What a power's cost (whole Force points) takes under Force Body at
/// `level`: that share of it, rounded down, from Force points and as much
/// again from hit points.
pub fn force_body_share(cost: i32, level: u8) -> i32 {
    cost * FORCE_BODY_SHARE[usize::from(level.min(2))] / 100
}

/// What a power costing `cost` (hundredths) takes from one with `lasting`
/// effects at `now` (hundredths): from its Force points, and from its hit
/// points (Force Body's share of each, the first running one's level, or
/// all of it from Force points).
pub fn power_cost(lasting: &[Option<Lasting>], cost: i32, now: u64) -> (i32, i32) {
    let level = lasting
        .iter()
        .flatten()
        .find(|l| l.modifies == Modifies::ForceBody && l.until > now)
        .map(|l| l.of);
    match level {
        Some(level) => {
            let share = force_body_share(cost / 100, level) * 100;
            (share, share)
        }
        None => (cost, 0),
    }
}

/// Whether one with `fp` and `hp` (hundredths) and `lasting` effects may
/// cast a power costing `cost` at `now` (TSL's `HasSpell`): its Force
/// points hold what it takes from them (`power_cost`), and what it takes
/// from its hit points leaves more than `FORCE_BODY_LEAST_HP`.
pub fn can_pay(lasting: &[Option<Lasting>], (fp, hp): (i32, i32), cost: i32, now: u64) -> bool {
    let (from_fp, from_hp) = power_cost(lasting, cost, now);
    from_fp <= fp && (from_hp == 0 || hp / 100 - from_hp / 100 > FORCE_BODY_LEAST_HP)
}

/// Pays a power's `cost` (hundredths) from `fighter` at `now` if it may
/// (`can_pay`; `charge`). Whether it paid.
pub fn pay(fighter: &mut Fighter, cost: i32, now: u64) -> bool {
    let payable = can_pay(
        &fighter.effects.lasting,
        (fighter.fp, fighter.hp),
        cost,
        now,
    );
    if payable {
        charge(fighter, cost, now);
    }
    payable
}

/// Takes a power's `cost` (hundredths) from `fighter` at `now`: from its
/// Force points, and under Force Body as much from its hit points,
/// temporary ones first, each floored at 0 and the shortfall forgiven, as
/// TSL's executable takes them when a cast lands.
pub fn charge(fighter: &mut Fighter, cost: i32, now: u64) {
    let (from_fp, from_hp) = power_cost(&fighter.effects.lasting, cost, now);
    fighter.fp = (fighter.fp - from_fp).max(0);
    let temporary = from_hp.min(fighter.pack.vitality.max(0));
    fighter.pack.vitality -= temporary;
    fighter.hp = (fighter.hp - (from_hp - temporary)).max(0);
}

/// The `videoeffects.2da` row Fury's spell row shows (Fury, Improved,
/// Master; Wookiee Rage I to III): 7, 8, 9.
fn fury_video(spell: u16) -> Option<u16> {
    match spell {
        164 | 271 => Some(7),
        165 | 272 => Some(8),
        166 | 273 => Some(9),
        _ => None,
    }
}

/// Force Sight's `videoeffects.2da` row.
const FORCE_SIGHT_VIDEO: u16 = 4;

/// How many lasting effects a character holds at once.
pub const LASTING_SLOTS: usize = 24;

/// The KOTOR effects on a character (see the module documentation).
/// Carried in snapshots.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Effects {
    /// Valor: its level, until when (sim ms).
    pub valor: (u8, u64),
    /// Energy Resistance: its level, until when.
    pub resist: (u8, u64),
    /// Slow's chain on it: its level (1 Slow, 2 Affliction, 3 Plague),
    /// until when.
    pub slowed: (u8, u64),
    /// Physical attribute points sapped so far, and when the next goes
    /// (Affliction and Plague: the first at the cast).
    pub sapped: (u8, u64),
    /// The lasting effects scripts applied.
    pub lasting: [Option<Lasting>; LASTING_SLOTS],
    /// The `IMMUNITY_TYPE_*`s and `gear::traits` its worn items give
    /// (`Gear::immune`, `Gear::traits`: their light, a walk only), kept here
    /// each step by `worn`, and when their regeneration last gave.
    pub worn_immune: u64,
    pub worn_traits: u8,
    pub regenerated_at: u64,
    /// Fury's kills (`Modifies::Fury`): counted from 0 when one takes hold,
    /// none once the last ends.
    pub fury: Option<u8>,
}

/// What `Effects::settle` found changed: the state that ended (when the one
/// holding changed), the one that took hold, and the faction modifiers that
/// ended (the faction to restore, their spell row).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Settled {
    pub ended: Option<u8>,
    pub took: Option<Lasting>,
    pub restored: Vec<(u8, u16)>,
}

impl Effects {
    /// Valor's level running at `now` (0 for none).
    pub fn valor(&self, now: u64) -> u8 {
        if self.valor.1 > now { self.valor.0 } else { 0 }
    }

    /// Energy Resistance's level running at `now`.
    pub fn resisting(&self, now: u64) -> u8 {
        if self.resist.1 > now {
            self.resist.0
        } else {
            0
        }
    }

    /// Slow's level on it at `now`.
    pub fn slow(&self, now: u64) -> u8 {
        if self.slowed.1 > now {
            self.slowed.0
        } else {
            0
        }
    }

    /// The lasting effects running at `now`.
    pub fn running(&self, now: u64) -> impl Iterator<Item = &Lasting> {
        self.lasting.iter().flatten().filter(move |l| l.until > now)
    }

    /// The lasting visual effects running at `now`: each one's
    /// `visualeffects.2da` row, until when, and who applied it.
    /// A worn item's light is one (`gear::LIGHT_VISUAL`) for as long as it
    /// is worn.
    pub fn visuals(&self, now: u64) -> impl Iterator<Item = (u16, u64, u32)> + '_ {
        let light = (self.worn_traits & super::gear::traits::LIGHT != 0).then_some((
            super::gear::LIGHT_VISUAL,
            u64::MAX,
            0,
        ));
        self.running(now)
            .filter(|l| l.modifies == Modifies::Visual)
            .map(|l| (l.amount as u16, l.until, l.by))
            .chain(light)
    }

    /// The running state that holds over the others (`state`): the highest
    /// number, the first of equals.
    pub fn top_state(&self, now: u64) -> Option<&Lasting> {
        self.running(now)
            .filter(|l| l.modifies == Modifies::State)
            .fold(None, |top: Option<&Lasting>, l| match top {
                Some(top) if top.of >= l.of => Some(top),
                _ => Some(l),
            })
    }

    /// Whether one running effect of `modifies` is on it.
    pub fn has(&self, modifies: Modifies, now: u64) -> bool {
        self.running(now).any(|l| l.modifies == modifies)
    }

    /// Whether it is blind (`Modifies::Blind`): it neither sees nor hears.
    pub fn blind(&self, now: u64) -> bool {
        self.has(Modifies::Blind, now)
    }

    /// How many times its natural healing doubles: once for each running
    /// vitality regeneration modifier, whatever its percentage.
    pub fn healing_doubled(&self, now: u64) -> u32 {
        self.running(now)
            .filter(|l| l.modifies == Modifies::VpRegen)
            .count() as u32
    }

    /// The `videoeffects.2da` row its effects give the view of the player
    /// controlling it: Force Sight's, else the latest Fury's by its spell
    /// row (Fury, Improved, Master and Wookiee Rage I to III: 7, 8, 9).
    pub fn video_row(&self, now: u64) -> Option<u16> {
        if self.has(Modifies::ForceSight, now) {
            return Some(FORCE_SIGHT_VIDEO);
        }
        self.running(now)
            .filter(|l| l.modifies == Modifies::Fury)
            .max_by_key(|l| l.at)
            .and_then(|l| fury_video(l.spell))
    }

    /// Whether an immunity effect or its worn items make it immune to
    /// `IMMUNITY_TYPE_*` `immunity`, an immunity to all (0) counting for
    /// every type (TSL's `GetEffectImmunity`); no feats, no mind umbrella.
    pub fn immune_effect(&self, immunity: u8, now: u64) -> bool {
        self.immune_to(immunity, now)
            || self
                .running(now)
                .any(|l| l.modifies == Modifies::Immunity && l.of == 0)
    }

    /// The appearance a running `EffectDisguise` draws it as (the last
    /// applied), if any.
    pub fn disguise(&self, now: u64) -> Option<u16> {
        self.running(now)
            .filter(|l| l.modifies == Modifies::Disguise)
            .last()
            .map(|l| l.amount as u16)
    }

    /// The running lasting effects that count: of the bonuses (and of the
    /// penalties) one spell gives to the same thing, only the strongest
    /// (the first of equals), so a recast or a script linking the same
    /// bonus twice does not stack. Different spells' and non-spell
    /// effects stack.
    pub fn counted(&self, now: u64) -> impl Iterator<Item = &Lasting> {
        let running: Vec<(usize, &Lasting)> = self
            .lasting
            .iter()
            .enumerate()
            .filter_map(|(at, l)| l.as_ref().filter(|l| l.until > now).map(|l| (at, l)))
            .collect();
        let all = running.clone();
        let beaten = move |at: usize, l: &Lasting| {
            l.spell != u16::MAX
                && all.iter().any(|&(other, m)| {
                    other != at
                        && (m.spell, m.modifies, m.of) == (l.spell, l.modifies, l.of)
                        && m.amount.signum() == l.amount.signum()
                        && (m.amount.abs(), at) > (l.amount.abs(), other)
                })
        };
        running
            .into_iter()
            .filter(move |&(at, l)| !beaten(at, l))
            .map(|(_, l)| l)
    }

    /// The sum of the counted lasting effects that change `modifies`.
    pub fn sum(&self, modifies: Modifies, now: u64) -> i32 {
        self.counted(now)
            .filter(|l| l.modifies == modifies)
            .map(|l| i32::from(l.amount))
            .sum()
    }

    /// Whether a running effect makes it immune to `IMMUNITY_TYPE_*`
    /// `immunity`.
    pub fn immune_to(&self, immunity: u8, now: u64) -> bool {
        (immunity < 64 && self.worn_immune & 1 << immunity != 0)
            || self
                .running(now)
                .any(|l| l.modifies == Modifies::Immunity && l.of == immunity)
    }

    /// Takes `effect` on: in a free or ended slot, else in place of the one
    /// ending soonest; never over a faction modifier whose end is still to
    /// settle (`settle` gives the faction back first).
    pub fn add(&mut self, effect: Lasting, now: u64) {
        let unsettled = |l: &Lasting| l.until <= now && l.modifies == Modifies::Faction;
        let slot = self
            .lasting
            .iter()
            .position(|l| l.is_none_or(|l| l.until <= now && !unsettled(&l)))
            .or_else(|| {
                (0..LASTING_SLOTS)
                    .filter(|&at| !self.lasting[at].is_some_and(|l| unsettled(&l)))
                    .min_by_key(|&at| self.lasting[at].map_or(0, |l| l.until))
            })
            .unwrap_or(0);
        self.lasting[slot] = Some(effect);
    }

    /// Whether an effect of `power`'s own script runs: its native
    /// stand-in (speed's and protect's defence, ...) then gives way.
    pub fn scripted(&self, power: Power, now: u64) -> bool {
        self.running(now)
            .any(|l| of_row(l.spell).is_some_and(|(of, _)| of == power))
    }

    /// Whether poison takes it: not under Knight or Master Valor or
    /// Improved Energy Resistance, nor a script's poison immunity (their
    /// scripts').
    pub fn poison_immune(&self, now: u64) -> bool {
        self.valor(now) >= 2
            || self.resisting(now) >= 2
            || self.immune_to(crate::combat::force::crowd::immunity::POISON, now)
    }

    /// Whether it moves at half speed (Affliction or Plague on it).
    pub fn slowed_down(&self, now: u64) -> bool {
        self.slow(now) >= 2
    }

    /// Saps a point now and then while Affliction or Plague runs; the
    /// points come back when it ends.
    pub fn sap(&mut self, now: u64) {
        let level = self.slow(now);
        if level < 2 {
            self.sapped = (0, 0);
            return;
        }
        let at = usize::from(level - 2);
        let (most, every) = (SAP_MOST[at], SAP_EVERY_MS[at]);
        let (lost, next) = &mut self.sapped;
        while *next <= now && *lost < most {
            *lost += 1;
            *next += u64::from(every);
        }
    }

    pub(crate) fn encode(&self, out: &mut Writer) {
        for (level, until) in [self.valor, self.resist, self.slowed] {
            out.u8(level);
            out.u64(until);
        }
        out.u8(self.sapped.0);
        out.u64(self.sapped.1);
        for slot in &self.lasting {
            write_fixed(out, slot.as_ref(), &Lasting::BLANK, Lasting::encode);
        }
        out.u64(self.worn_immune);
        out.u8(self.worn_traits);
        out.u64(self.regenerated_at);
        out.u8(self.fury.unwrap_or(u8::MAX));
    }

    pub(crate) fn decode(input: &mut Reader) -> Result<Effects, DecodeError> {
        let mut timed = || -> Result<(u8, u64), DecodeError> { Ok((input.u8()?, input.u64()?)) };
        let (valor, resist, slowed) = (timed()?, timed()?, timed()?);
        let sapped = (input.u8()?, input.u64()?);
        let mut lasting = [None; LASTING_SLOTS];
        for slot in &mut lasting {
            *slot = read_fixed(input, &Lasting::BLANK, Lasting::decode, "lasting effect")?;
        }
        Ok(Effects {
            valor,
            resist,
            slowed,
            sapped,
            lasting,
            worn_immune: input.u64()?,
            worn_traits: input.u8()?,
            regenerated_at: input.u64()?,
            fury: Some(input.u8()?).filter(|kills| *kills != u8::MAX),
        })
    }
}

/// Settles what ended on `fighter` since the last step: faction modifiers
/// that ran out or were taken off (each faction to restore; their slots
/// freed), Fury's kills once no Fury runs, and the state that holds
/// (`ForceState::state`): the highest running, its change reported.
pub fn settle(fighter: &mut Fighter, now: u64) -> Settled {
    let mut settled = Settled::default();
    for slot in &mut fighter.effects.lasting {
        if let Some(ended) = slot.filter(|l| l.modifies == Modifies::Faction && l.until <= now) {
            settled.restored.push((ended.of, ended.spell));
            *slot = None;
        }
    }
    let effects = &mut fighter.effects;
    if effects.fury.is_some() && !effects.has(Modifies::Fury, now) {
        effects.fury = None;
    }
    let top = effects.top_state(now).copied();
    let force = &mut fighter.force;
    let state = top.map_or(0, |l| l.of);
    force.state_until = top.map_or(0, |l| l.until);
    if state != force.state {
        settled.ended = Some(force.state).filter(|state| *state != 0);
        settled.took = top;
        force.state = state;
    }
    settled
}

/// What `sheet`'s worn items do to `fighter` at `now` (step `tick`,
/// `Gear`): their immunities and traits kept on its effects (a light put
/// on is a visual event for the views, `fighter::Visual`, lasting); their
/// regeneration, while it lives short of its maximum, giving its points
/// every `REGENERATE_EVERY_MS` since the last it gave (hit points, and
/// Force points up to `pool`), in combat too.
pub fn worn(fighter: &mut Fighter, sheet: &Sheet, pool: u16, (now, tick): (u64, u64)) {
    let gear = &sheet.gear;
    fighter.effects.worn_immune = gear.immune;
    // A Constitution lost takes the hit points over the maximum with it.
    fighter.hp = fighter.hp.min(i32::from(sheet.max_hp) * 100);
    let light = super::gear::traits::LIGHT;
    if gear.has(light) && fighter.effects.worn_traits & light == 0 {
        let visual = super::fighter::Visual {
            row: super::gear::LIGHT_VISUAL,
            ms: u32::MAX,
            beam: false,
            part: 0,
        };
        fighter.record(super::fighter::Event::visual(visual, None, None, tick));
    }
    fighter.effects.worn_traits = gear.traits;
    if gear.regenerate == 0 && gear.regenerate_fp == 0 || fighter.hp <= 0 || !fighter.alive() {
        return;
    }
    let hp_short = fighter.hp < i32::from(sheet.max_hp) * 100;
    let fp_short = fighter.fp < i32::from(pool) * 100;
    let gives = gear.regenerate > 0 && hp_short || gear.regenerate_fp > 0 && fp_short;
    if !gives || now < fighter.effects.regenerated_at + super::gear::REGENERATE_EVERY_MS {
        return;
    }
    fighter.effects.regenerated_at = now;
    if hp_short {
        let max = i32::from(sheet.max_hp) * 100;
        fighter.hp = (fighter.hp + i32::from(gear.regenerate) * 100).min(max);
    }
    if fp_short {
        let max = i32::from(pool) * 100;
        fighter.fp = (fighter.fp + i32::from(gear.regenerate_fp) * 100).min(max);
    }
}

/// What `fighter`'s regeneration and Body Fuel effects give at `now` (step
/// `msec` long), while it lives: each regeneration its hit points (up to
/// `max_hp`) once its interval has passed since it took hold or last gave,
/// its interval then starting again; Body Fuel the Force points full (to
/// `pool`, temporary ones kept) at each of its combat rounds' ends, the
/// rounds counted from when it took hold [verify: the bearer's own round].
pub fn regenerate(fighter: &mut Fighter, (max_hp, pool): (u16, u16), (now, msec): (u64, u32)) {
    if !fighter.alive() {
        return;
    }
    let max = i32::from(max_hp) * 100;
    let in_combat = fighter.combat_until > now;
    for l in fighter.effects.lasting.iter_mut().flatten() {
        if l.until <= now {
            continue;
        }
        match l.modifies {
            Modifies::Regenerate if now >= l.at + l.amount.max(1) as u64 => {
                l.at = now;
                fighter.hp = (fighter.hp + i32::from(l.of) * 100)
                    .min(max)
                    .max(fighter.hp);
            }
            Modifies::BodyFuel
                if in_combat && now > l.at && (now - l.at) % super::ROUND_MS < u64::from(msec) =>
            {
                fighter.fp = fighter.fp.max(i32::from(pool) * 100);
            }
            _ => {}
        }
    }
}

/// When the toggle that `spell` (a `spells.2da` row) belongs to ends, for
/// the powers JA toggles and KOTOR's scripts buff with (speed, protect,
/// absorb); `None` for the others.
fn toggle_until(force: &mut ForceState, spell: u16) -> Option<&mut u64> {
    match of_row(spell)? {
        (Power::Speed, _) => Some(&mut force.speed_until),
        (Power::Protect, _) => Some(&mut force.protect_until),
        (Power::Absorb, _) => Some(&mut force.absorb_until),
        _ => None,
    }
}

/// Puts a script's lasting effect on `fighter`. A toggled power's own
/// effects last as long as the toggle does (JA's length, turned off early).
pub fn add(fighter: &mut Fighter, mut lasting: Lasting, now: u64) {
    if let Some(until) = toggle_until(&mut fighter.force, lasting.spell) {
        lasting.until = lasting.until.min(*until);
    }
    fighter.effects.add(lasting, now);
}

/// Takes the lasting effect `id` (all with `None`) off `fighter`. Taking
/// off a toggled power's effect (a script's dispel) ends the toggle too.
pub fn remove(fighter: &mut Fighter, id: Option<u32>, now: u64) {
    // Its poison, and what the poison sapped (an antidote's, all effects');
    // what came linked with the poison (`EffectLinkEffects`: Affliction's
    // slow) goes with it, and it with any part of its link.
    let link = fighter.pack.poison.and_then(|poison| poison.link);
    if id.is_none_or(|id| id == POISON_ID) {
        remove_poison(fighter);
    }
    if id.is_some() && id == link {
        fighter.pack.poison = None;
    }
    // Its energy shield, an effect too (`ClearAllEffects`: Hrakert's
    // sentry droid, its shield cut from the station's computer).
    if id.is_none() {
        fighter.pack.shield = None;
    }
    let id = match id {
        Some(POISON_ID) => match link {
            Some(link) => Some(link),
            None => return,
        },
        id => id,
    };
    for slot in &mut fighter.effects.lasting {
        let Some(lasting) = slot.filter(|l| id.is_none_or(|id| l.id == id)) else {
            continue;
        };
        // A faction modifier ends, its faction restored as it settles.
        match slot.as_mut() {
            Some(faction) if faction.modifies == Modifies::Faction => {
                faction.until = faction.until.min(now);
            }
            _ => *slot = None,
        }
        if let Some(until) = toggle_until(&mut fighter.force, lasting.spell) {
            *until = (*until).min(now);
        }
    }
}

/// Ends the poison on `fighter` (its course kept off now), and what came
/// linked with it.
pub fn end_poison(fighter: &mut Fighter, now: u64) {
    if let Some(link) = fighter.pack.poison.take().and_then(|poison| poison.link) {
        remove(fighter, Some(link), now);
    }
}

/// Going down (a party member knocked out, `Fighter::fall`) and getting up
/// (`Fighter::rise`): the effects on `fighter` end as at a death in the
/// Aurora engines KOTOR's is built on, which strip a body's effects
/// (`docs/FALLING.md`, "Effects"). The poison and what it drained,
/// Affliction's and Plague's sapping and slow, Valor, Energy Resistance,
/// the shield and stim up, its own powers running and every timed script
/// effect end now (their ends settled as when they run out); permanent
/// ones (`u64::MAX`) stay, a plot's disguise or visual [verify: whether
/// the executable keeps any].
pub fn strip(fighter: &mut Fighter, now: u64) {
    remove_poison(fighter);
    fighter.pack.shield = None;
    fighter.pack.stim = None;
    fighter.force.stop();
    let effects = &mut fighter.effects;
    for (_, until) in [&mut effects.valor, &mut effects.resist, &mut effects.slowed] {
        *until = (*until).min(now);
    }
    effects.sapped = (0, 0);
    for lasting in effects.lasting.iter_mut().flatten() {
        if lasting.until != u64::MAX {
            lasting.until = lasting.until.min(now);
        }
    }
}

/// The poison on `fighter` and the points it drained, gone.
fn remove_poison(fighter: &mut Fighter) {
    fighter.pack.poison = None;
    fighter.pack.drained = ([0; 6], 0);
}

/// Ends the effects of `power`'s scripts on `fighter` (its toggle turned
/// off).
pub fn end_power(fighter: &mut Fighter, power: Power, now: u64) {
    for lasting in fighter.effects.lasting.iter_mut().flatten() {
        if of_row(lasting.spell).is_some_and(|(of, _)| of == power) {
            lasting.until = lasting.until.min(now);
        }
    }
}

/// The level of a timed power running at `now` (0 for none).
fn running(until: u64, level: u8, now: u64) -> usize {
    if until > now {
        usize::from(level.clamp(1, 3))
    } else {
        0
    }
}

/// `sheet` as everything running on `fighter` at `now` changes it: its
/// items (`items::affected`: a grenade's weakness, a stim; what worn items
/// give, `Sheet::gear`), protect's and speed's defence (and protect's
/// saves), valor, Slow's penalties, Affliction's and Plague's sapped
/// points, Energy Resistance; scores, skills, saves and to hit moved by
/// them no further than `gear::Caps`. What the pipeline, the saves and the
/// powers read.
pub fn sheet_now(sheet: Sheet, fighter: &Fighter, now: u64) -> Sheet {
    let worn = sheet;
    let mut sheet = items::affected(sheet, &fighter.pack, now);
    wear(&mut sheet);
    let force = &fighter.force;
    let effects = &fighter.effects;
    let mut defence = 0i8;
    let mut saves = 0i8;
    // The stand-ins give way to the power's own script's effects.
    if let protect @ 1.. = running(force.protect_until, force.protect_level, now)
        && !effects.scripted(Power::Protect, now)
    {
        defence += PROTECT_BONUS[protect - 1];
        saves += PROTECT_BONUS[protect - 1];
    }
    if let speed @ 1.. = running(force.speed_until, force.speed_level, now)
        && !effects.scripted(Power::Speed, now)
    {
        defence += SPEED_DEFENCE[speed - 1];
    }
    let valor = effects.valor(now);
    if valor > 0 {
        let bonus = VALOR_BONUS[usize::from(valor.min(3)) - 1];
        saves += bonus as i8;
        for ability in [Ability::Strength, Ability::Dexterity, Ability::Constitution] {
            let score = &mut sheet.abilities[ability as usize];
            *score = score.saturating_add(bonus);
        }
    }
    let sapped = effects.sapped.0;
    for ability in [Ability::Strength, Ability::Dexterity, Ability::Constitution] {
        let score = &mut sheet.abilities[ability as usize];
        *score = score.saturating_sub(sapped).max(1);
    }
    if effects.slow(now) == 1 {
        defence -= SLOW_PENALTY;
        sheet.saves[1] = sheet.saves[1].saturating_sub(SLOW_PENALTY);
        sheet.attack = sheet.attack.saturating_sub(SLOW_PENALTY);
    }
    if effects.resisting(now) > 0 {
        for row in RESIST_TYPES.map(usize::from) {
            sheet.guards.resist[row] = sheet.guards.resist[row].max(RESIST);
        }
    }
    // The scripts' lasting effects.
    let small = |amount: i16| amount.clamp(-128, 127) as i8;
    for l in effects.counted(now) {
        let of = usize::from(l.of);
        match l.modifies {
            Modifies::Ability => {
                if let Some(score) = sheet.abilities.get_mut(of) {
                    *score = (i16::from(*score) + l.amount).clamp(1, 255) as u8;
                }
            }
            Modifies::Defence => defence = defence.saturating_add(small(l.amount)),
            Modifies::Save if l.of == ALL_SAVES => saves = saves.saturating_add(small(l.amount)),
            Modifies::Save => {
                if let Some(save) = sheet.saves.get_mut(of) {
                    *save = save.saturating_add(small(l.amount));
                }
            }
            Modifies::Attack => sheet.attack = sheet.attack.saturating_add(small(l.amount)),
            Modifies::Damage => {
                for weapon in sheet.weapons.iter_mut().flatten() {
                    weapon.damage_bonus = weapon.damage_bonus.saturating_add(small(l.amount));
                }
            }
            Modifies::Resistance => {
                if let Some(resist) = sheet.guards.resist.get_mut(of) {
                    *resist = (*resist).max(l.amount.clamp(0, 255) as u8);
                }
            }
            Modifies::DamageImmunity => {
                if let Some(immunity) = sheet.guards.immunity.get_mut(of) {
                    *immunity = immunity.saturating_add(small(l.amount));
                }
            }
            Modifies::Skill => {
                if let Some(rank) = sheet.skills.get_mut(of) {
                    *rank = (i16::from(*rank) + l.amount).clamp(0, 255) as u8;
                }
            }
            Modifies::Disguise => sheet.gear.disguise = Some(l.amount as u16),
            // Read where they act: speed, the round's attacks, resisting
            // the Force, holds and poison, the Force points over the pool and
            // their regeneration; a visual by the views; the states, perception,
            // the casts' costs, healing, factions; the rest kept only.
            Modifies::Immunity
            | Modifies::Speed
            | Modifies::Attacks
            | Modifies::AssuredHit
            | Modifies::ForceResistance
            | Modifies::ForcePoints
            | Modifies::Visual
            | Modifies::Vitality
            | Modifies::Poison
            | Modifies::State
            | Modifies::Blind
            | Modifies::TrueSeeing
            | Modifies::Fury
            | Modifies::ForceSight
            | Modifies::ForceBody
            | Modifies::BodyFuel
            | Modifies::PsychicStatic
            | Modifies::Faction
            | Modifies::VpRegen
            | Modifies::FpRegen
            | Modifies::Regenerate => {}
        }
    }
    sheet.defence = sheet.defence.saturating_add(defence);
    for save in &mut sheet.saves {
        *save = save.saturating_add(saves);
    }
    if effects.sum(Modifies::AssuredHit, now) > 0 {
        sheet.flags |= super::sheet::flags::ASSURED_HIT;
    }
    capped(&worn, &mut sheet);
    // Fury's kills add to its weapons' damage past the caps (TSL's damage
    // bonus adds the count of its own).
    if let Some(kills) = effects.fury.filter(|_| effects.has(Modifies::Fury, now)) {
        for weapon in sheet.weapons.iter_mut().flatten() {
            weapon.damage_bonus = weapon.damage_bonus.saturating_add(kills as i8);
        }
    }
    sheet
}

/// What worn items give folded into `sheet` as effects of theirs: scores,
/// skills, saves, dodge defence, bonus feats and the armour's DEX cap
/// (taken once: those numbers are spent, so a sheet folded again adds
/// nothing; the rest of the gear stays for where it acts).
fn wear(sheet: &mut Sheet) {
    let gear = &mut sheet.gear;
    let moved = |value: u8, by: i8| (i16::from(value) + i16::from(by)).clamp(0, 255) as u8;
    for (score, by) in sheet.abilities.iter_mut().zip(gear.abilities) {
        *score = moved(*score, by).max(1);
    }
    for (rank, by) in sheet.skills.iter_mut().zip(gear.skills) {
        *rank = moved(*rank, by);
    }
    for (save, by) in sheet.saves.iter_mut().zip(gear.saves) {
        *save = save.saturating_add(by);
    }
    sheet.defence = sheet.defence.saturating_add(gear.defence);
    for (word, more) in sheet.feats.0.iter_mut().zip(gear.feats.0) {
        *word |= more;
    }
    if let Some(cap) = sheet.dex_cap.as_mut() {
        *cap = cap.saturating_add(gear.dex_cap);
    }
    (gear.abilities, gear.skills, gear.saves) = Default::default();
    (gear.defence, gear.dex_cap) = (0, 0);
}

/// `now`'s scores, skills, saves, to hit and its weapons' damage bonus
/// held within the caps (`gear::Caps`) of how far effects and items move
/// them; its hit points by its Constitution's change.
fn capped(worn: &Sheet, now: &mut Sheet) {
    let caps = super::gear::Caps::of(worn);
    let hold = |cap, was: i32, is: i32| was + super::gear::Caps::hold(cap, is - was);
    for (is, was) in now.abilities.iter_mut().zip(worn.abilities) {
        *is = hold(caps.ability, i32::from(was), i32::from(*is)).clamp(1, 255) as u8;
    }
    for (is, was) in now.skills.iter_mut().zip(worn.skills) {
        *is = hold(caps.skill, i32::from(was), i32::from(*is)).clamp(0, 255) as u8;
    }
    for (is, was) in now.saves.iter_mut().zip(worn.saves) {
        *is = hold(caps.save, i32::from(was), i32::from(*is)).clamp(-128, 127) as i8;
    }
    // A Constitution moved moves the hit points: its modifier's change a
    // level.
    let con = |sheet: &Sheet| sheet.modifier(super::sheet::Ability::Constitution);
    let gained = (con(now) - con(worn)) * i32::from(worn.level.max(1));
    now.max_hp = (i32::from(worn.max_hp) + gained).clamp(1, i32::from(u16::MAX)) as u16;
    now.attack = super::gear::Caps::hold(caps.attack, i32::from(now.attack)).clamp(-128, 127) as i8;
    // A weapon's flat damage bonus is its items' and the effects' alone.
    for weapon in now.weapons.iter_mut().flatten() {
        let bonus = super::gear::Caps::hold(caps.damage, i32::from(weapon.damage_bonus));
        weapon.damage_bonus = bonus.clamp(-128, 127) as i8;
    }
}

/// The extra attacks a round speed gives a fighter at `now` (Knight 1,
/// Master 2): a weapon hit's budget is multiplied by one more than these.
pub fn extra_attacks(force: &ForceState, effects: &Effects, now: u64) -> i32 {
    let speed = match running(force.speed_until, force.speed_level, now) {
        _ if effects.scripted(Power::Speed, now) => 0,
        0 => 0,
        level => SPEED_ATTACKS[level - 1],
    };
    speed + effects.sum(Modifies::Attacks, now)
}

/// The lowest level among the powers a dispel could end running on a
/// fighter at `now` (speed, protect, absorb, valor, energy resistance);
/// `None` with none running.
pub fn weakest_buff(fighter: &Fighter, now: u64) -> Option<u8> {
    let force = &fighter.force;
    let effects = &fighter.effects;
    [
        (force.speed_until, force.speed_level),
        (force.protect_until, force.protect_level),
        (force.absorb_until, force.absorb_level),
        (effects.valor.1, effects.valor.0),
        (effects.resist.1, effects.resist.0),
    ]
    .into_iter()
    .filter(|(until, _)| *until > now)
    .map(|(_, level)| level)
    .min()
}

/// Supress Force (`all` false: first- and second-tier powers) or Force
/// Breach (`all`: every one) on a fighter at `now`: speed, protect, valor,
/// energy resistance, absorb end.
pub fn dispel(fighter: &mut Fighter, all: bool, now: u64) {
    let ends = |level: u8| all || level <= 2;
    let force = &mut fighter.force;
    for (until, level) in [
        (&mut force.speed_until, force.speed_level),
        (&mut force.protect_until, force.protect_level),
        (&mut force.absorb_until, force.absorb_level),
    ] {
        if *until > now && ends(level) {
            *until = now;
        }
    }
    let effects = &mut fighter.effects;
    for (level, until) in [&mut effects.valor, &mut effects.resist] {
        if *until > now && (all || *level <= 2) {
            *until = now;
        }
    }
}
