//! What worn items give a character beyond its weapons and guards
//! (`Gear`, KOTOR's item properties applied on equip as effects the item
//! creates, `inventory::properties`), and the pieces a weapon's
//! properties add to its hits: a bonus against one kind of foe
//! (`Versus`), extra dice (`Roll`), an effect on a hit (`OnHit`).
//!
//! Sums follow the executables (journal 2026-10-05-item-properties): an
//! item gives the best of its bonuses of a kind and all its penalties;
//! items add to each other; the totals with the scripts' effects are
//! capped (`Caps`) when the sheet is read (`effects::sheet_now`).

use crate::combat::sheet::{Rows, Sheet};
use crate::wire::{DecodeError, Reader, Writer};

/// The side of the Force an item's alignment group names
/// (`iprp_aligngrp.2da`), as the item rules count a good-evil value: dark,
/// light, neutral between (`rules::items::alignment`).
pub mod sides {
    pub const ALL: u8 = 0;
    pub const NEUTRAL: u8 = 1;
    pub const LIGHT: u8 = 2;
    pub const DARK: u8 = 3;

    /// The group `good_evil` (0 dark to 100 light) is in.
    pub fn of(good_evil: u8) -> u8 {
        use rules::items::alignment;
        if good_evil <= alignment::DARK_MOST {
            DARK
        } else if good_evil >= alignment::LIGHT_LEAST {
            LIGHT
        } else {
            NEUTRAL
        }
    }
}

/// Whom a bonus counts against: a race (`racialtypes.2da` row), a side
/// of the Force (`sides`; `sides::ALL` everyone), or (a defence's) attacks
/// of a damage type (a `DAMAGE_TYPES` row).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Against {
    Race(u8),
    Side(u8),
    Type(u8),
}

impl Against {
    /// Whether `foe`, attacking with damage types `damage_types` (a
    /// weapon's `damageflags`), is of it.
    pub fn matches(self, foe: &Sheet, damage_types: u16) -> bool {
        match self {
            Against::Race(race) => foe.race == race,
            Against::Side(sides::ALL) => true,
            Against::Side(side) => sides::of(foe.good_evil) == side,
            Against::Type(row) => row < 16 && damage_types & 1 << row != 0,
        }
    }

    pub(crate) fn encode(self, out: &mut Writer) {
        match self {
            Against::Race(race) => out.bytes(&[0, race]),
            Against::Side(side) => out.bytes(&[1, side]),
            Against::Type(row) => out.bytes(&[2, row]),
        }
    }

    pub(crate) fn decode(input: &mut Reader) -> Result<Against, DecodeError> {
        let at = input.at();
        match input.array()? {
            [0, race] => Ok(Against::Race(race)),
            [1, side] => Ok(Against::Side(side)),
            [2, row] => Ok(Against::Type(row)),
            _ => Err(input.invalid("against", at)),
        }
    }
}

/// Damage dice and a flat bonus: `dice` d `die` + `bonus`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Roll {
    pub dice: u8,
    pub die: u8,
    pub bonus: i8,
}

impl Roll {
    pub const NONE: Roll = Roll {
        dice: 0,
        die: 0,
        bonus: 0,
    };

    /// Its average, doubled (whole numbers for comparing).
    pub fn twice_average(self) -> i32 {
        i32::from(self.dice) * (i32::from(self.die) + 1) + 2 * i32::from(self.bonus)
    }

    pub fn is_none(self) -> bool {
        self == Roll::NONE
    }

    pub(crate) fn encode(self, out: &mut Writer) {
        out.bytes(&[self.dice, self.die]);
        out.i8(self.bonus);
    }

    pub(crate) fn decode(input: &mut Reader) -> Result<Roll, DecodeError> {
        let [dice, die] = input.array()?;
        Ok(Roll {
            dice,
            die,
            bonus: input.i8()?,
        })
    }
}

/// A weapon's bonus against one kind of foe (item properties 6, 7, 12, 13,
/// 39, 40: enhancement, damage and to hit against an alignment group or a
/// race): to hit, and damage of a type (a `DAMAGE_TYPES` row).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Versus {
    pub against: Against,
    pub attack: i8,
    pub damage: Roll,
    pub damage_type: u8,
}

impl Versus {
    pub(crate) const BLANK: Versus = Versus {
        against: Against::Race(0),
        attack: 0,
        damage: Roll::NONE,
        damage_type: 0,
    };

    pub(crate) fn encode(&self, out: &mut Writer) {
        self.against.encode(out);
        out.i8(self.attack);
        self.damage.encode(out);
        out.u8(self.damage_type);
    }

    pub(crate) fn decode(input: &mut Reader) -> Result<Versus, DecodeError> {
        Ok(Versus {
            against: Against::decode(input)?,
            attack: input.i8()?,
            damage: Roll::decode(input)?,
            damage_type: input.u8()?,
        })
    }
}

/// What a hit may do besides its damage (item property 32, `OnHit`): its
/// kind (`iprp_onhit.2da` row), the chance in percent and how long it
/// lasts in seconds (`iprp_onhitdur.2da`'s rounds: 6 s each in K1, 3 s in
/// TSL), the save's DC (`iprp_onhitdc.2da`), and the kind's parameter (an
/// ability, a poison, a race or an alignment group).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OnHit {
    pub kind: u8,
    pub chance: u8,
    pub seconds: u8,
    pub dc: u8,
    pub param: u8,
}

/// `iprp_onhit.2da` rows.
pub mod on_hit {
    pub const SLEEP: u8 = 0;
    pub const STUN: u8 = 1;
    pub const PARALYZE: u8 = 2;
    pub const CONFUSION: u8 = 3;
    pub const FEAR: u8 = 4;
    pub const SLOW: u8 = 5;
    pub const ABILITY_DRAIN: u8 = 6;
    pub const POISON: u8 = 7;
    pub const SLAY_RACE: u8 = 8;
    pub const SLAY_SIDE: u8 = 9;
    pub const DEATH: u8 = 10;
    /// TSL's.
    pub const KNOCKDOWN: u8 = 11;
}

impl OnHit {
    pub(crate) const BLANK: OnHit = OnHit {
        kind: 0,
        chance: 0,
        seconds: 0,
        dc: 0,
        param: 0,
    };

    pub(crate) fn encode(&self, out: &mut Writer) {
        out.bytes(&[self.kind, self.chance, self.seconds, self.dc, self.param]);
    }

    pub(crate) fn decode(input: &mut Reader) -> Result<OnHit, DecodeError> {
        let [kind, chance, seconds, dc, param] = input.array()?;
        Ok(OnHit {
            kind,
            chance,
            seconds,
            dc,
            param,
        })
    }
}

/// `IMMUNITY_TYPE_*`s the item rules name (both games' `nwscript.nss`).
pub mod immunity {
    pub const MIND_SPELLS: u8 = 1;
    pub const POISON: u8 = 2;
    pub const FEAR: u8 = 4;
    pub const PARALYSIS: u8 = 6;
    pub const SLOW: u8 = 9;
    pub const STUN: u8 = 12;
    pub const SLEEP: u8 = 13;
    pub const CONFUSED: u8 = 16;
    pub const ABILITY_DECREASE: u8 = 19;
    pub const KNOCKDOWN: u8 = 28;
    pub const SNEAK_ATTACK: u8 = 30;
    pub const CRITICAL_HIT: u8 = 31;
    pub const DEATH: u8 = 32;
}

/// `Gear::traits` bits.
pub mod traits {
    /// Light (29): the wearer carries `visualeffects.2da` row 5000.
    pub const LIGHT: u8 = 1;
    /// True Seeing (47): sees the stealthed.
    pub const TRUE_SEEING: u8 = 2;
    /// TSL's Dampen Sound (63): the wearer is never heard.
    pub const SILENT: u8 = 4;
    /// TSL's Sniper Bonus (66): +4 to hit with Sniper Shot.
    pub const SNIPER: u8 = 8;
    /// TSL's Rapid Shot Bonus (67): Rapid Shot's penalties a point less.
    pub const RAPID_SHOT: u8 = 16;
    /// Special Walk (52): its walk, and no running.
    pub const WALK_ONLY: u8 = 32;
}

/// The light an item property gives (`visualeffects.2da`: VFX_ARD_LIGHT_
/// YELLOW_10, whatever the property's colour and brightness).
pub const LIGHT_VISUAL: u16 = 5000;

/// What worn items give a character beyond its weapons and guards, by
/// `Ability`, skill row and save (Fortitude, Reflex, Will); defence (all
/// an item's is dodge defence, which adds up) and defence against kinds of
/// foe (`VERSUS` at most); Force resistance; hit points and Force points regained every
/// `REGENERATE_EVERY_MS`; the `IMMUNITY_TYPE_*`s as bits; bonus feats; what
/// it adds to the armour's DEX cap; the appearance it disguises the wearer
/// as; its walk (`iprp_walk.2da`); `traits`; and damage reduction: points
/// off each physical hit (`soak`) from a weapon of less enhancement than
/// `soak_bypass`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Gear {
    pub abilities: [i8; 6],
    pub skills: [i8; 8],
    pub saves: [i8; 3],
    pub defence: i8,
    pub defence_vs: [Option<(Against, i8)>; VERSUS],
    pub force_resist: u8,
    pub regenerate: u8,
    pub regenerate_fp: u8,
    pub immune: u64,
    pub feats: Rows<4>,
    pub dex_cap: u8,
    pub disguise: Option<u16>,
    pub walk: u8,
    pub traits: u8,
    pub soak: u8,
    pub soak_bypass: u8,
}

/// How many kinds of foe worn items' defence counts against at once.
pub const VERSUS: usize = 3;

/// How often item regeneration gives its points (ms).
pub const REGENERATE_EVERY_MS: u64 = rules::items::properties::REGENERATE_MS;

fn add(a: i8, b: i8) -> i8 {
    a.saturating_add(b)
}

impl Gear {
    /// `self` and `more` worn together: numbers add (Force resistance and
    /// the soak the best), immunities, feats, traits and defences against a
    /// foe join; the first disguise and walk stand.
    pub fn with(mut self, more: &Gear) -> Gear {
        for (a, b) in self.abilities.iter_mut().zip(more.abilities) {
            *a = add(*a, b);
        }
        for (a, b) in self.skills.iter_mut().zip(more.skills) {
            *a = add(*a, b);
        }
        for (a, b) in self.saves.iter_mut().zip(more.saves) {
            *a = add(*a, b);
        }
        self.defence = add(self.defence, more.defence);
        for versus in more.defence_vs.iter().flatten() {
            self.add_defence_vs(*versus);
        }
        self.force_resist = self.force_resist.max(more.force_resist);
        self.regenerate = self.regenerate.saturating_add(more.regenerate);
        self.regenerate_fp = self.regenerate_fp.saturating_add(more.regenerate_fp);
        self.immune |= more.immune;
        for (a, b) in self.feats.0.iter_mut().zip(more.feats.0) {
            *a |= b;
        }
        self.dex_cap = self.dex_cap.saturating_add(more.dex_cap);
        self.disguise = self.disguise.or(more.disguise);
        if self.walk == 0 {
            self.walk = more.walk;
        }
        self.traits |= more.traits;
        if more.soak > self.soak {
            (self.soak, self.soak_bypass) = (more.soak, more.soak_bypass);
        }
        self
    }

    /// Whether it makes its wearer immune to `IMMUNITY_TYPE_*` `immunity`.
    pub fn immune_to(&self, immunity: u8) -> bool {
        immunity < 64 && self.immune & 1 << immunity != 0
    }

    pub fn has(&self, trait_bit: u8) -> bool {
        self.traits & trait_bit != 0
    }

    /// A defence against a kind of foe, in the first free place (one past
    /// `VERSUS` is dropped).
    pub fn add_defence_vs(&mut self, versus: (Against, i8)) {
        if let Some(free) = self.defence_vs.iter_mut().find(|slot| slot.is_none()) {
            *free = Some(versus);
        }
    }

    /// The defence it adds against `attacker` dealing `damage_types`: those
    /// against a foe or an attack of its kind (its dodge defence is in the
    /// sheet's, `effects::sheet_now`).
    pub fn defence_against(&self, attacker: &Sheet, damage_types: u16) -> i32 {
        self.defence_vs
            .iter()
            .flatten()
            .filter(|(against, _)| against.matches(attacker, damage_types))
            .map(|(_, by)| i32::from(*by))
            .sum()
    }

    pub(crate) fn encode(&self, out: &mut Writer) {
        for value in self.abilities.iter().chain(&self.skills).chain(&self.saves) {
            out.i8(*value);
        }
        out.i8(self.defence);
        for versus in &self.defence_vs {
            let (against, by) = versus.unwrap_or((Against::Race(0), 0));
            out.u8(u8::from(versus.is_some()));
            against.encode(out);
            out.i8(by);
        }
        out.bytes(&[self.force_resist, self.regenerate, self.regenerate_fp]);
        out.u64(self.immune);
        for word in self.feats.0 {
            out.u64(word);
        }
        out.u8(self.dex_cap);
        out.u16(self.disguise.unwrap_or(u16::MAX));
        out.bytes(&[self.walk, self.traits, self.soak, self.soak_bypass]);
    }

    pub(crate) fn decode(input: &mut Reader) -> Result<Gear, DecodeError> {
        let mut gear = Gear::default();
        for value in gear
            .abilities
            .iter_mut()
            .chain(&mut gear.skills)
            .chain(&mut gear.saves)
        {
            *value = input.i8()?;
        }
        gear.defence = input.i8()?;
        for versus in &mut gear.defence_vs {
            let at = input.at();
            let flag = input.u8()?;
            let against = Against::decode(input)?;
            let by = input.i8()?;
            *versus = match flag {
                0 if (against, by) == (Against::Race(0), 0) => None,
                1 => Some((against, by)),
                _ => return Err(input.invalid("defence against", at)),
            };
        }
        [gear.force_resist, gear.regenerate, gear.regenerate_fp] = input.array()?;
        gear.immune = input.u64()?;
        for word in &mut gear.feats.0 {
            *word = input.u64()?;
        }
        gear.dex_cap = input.u8()?;
        gear.disguise = Some(input.u16()?).filter(|row| *row != u16::MAX);
        [gear.walk, gear.traits, gear.soak, gear.soak_bypass] = input.array()?;
        Ok(gear)
    }
}

/// The most effects (an item's and the scripts') may raise or lower a
/// score, a skill, a save, to hit and damage: K1's, so many times over in
/// TSL (`flags::TSL`; `rules::items::caps`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Caps {
    pub ability: (i32, i32),
    pub skill: (i32, i32),
    pub save: (i32, i32),
    pub attack: (i32, i32),
    pub damage: (i32, i32),
}

impl Caps {
    /// A sheet's caps, as bonus and penalty.
    pub fn of(sheet: &Sheet) -> Caps {
        use rules::items::caps;
        let times = if sheet.flags & crate::combat::sheet::flags::TSL != 0 {
            caps::TSL_TIMES
        } else {
            1
        };
        let cap = |[bonus, penalty]: [i32; 2]| (bonus * times, penalty * times);
        Caps {
            ability: cap(caps::ABILITY),
            skill: cap(caps::SKILL),
            save: cap(caps::SAVE),
            attack: cap(caps::ATTACK),
            damage: cap(caps::DAMAGE),
        }
    }

    /// `change` held within `cap`.
    pub fn hold((bonus, penalty): (i32, i32), change: i32) -> i32 {
        change.clamp(-penalty, bonus)
    }
}

/// Damage of another type a weapon's properties add to its hits (item
/// property 11 of a type the weapon does not deal), as a `DAMAGE_TYPES`
/// row.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Extra {
    pub damage_type: u8,
    pub roll: Roll,
}

impl Extra {
    pub(crate) const BLANK: Extra = Extra {
        damage_type: 0,
        roll: Roll::NONE,
    };

    pub(crate) fn encode(&self, out: &mut Writer) {
        out.u8(self.damage_type);
        self.roll.encode(out);
    }

    pub(crate) fn decode(input: &mut Reader) -> Result<Extra, DecodeError> {
        Ok(Extra {
            damage_type: input.u8()?,
            roll: Roll::decode(input)?,
        })
    }
}

/// `Weapon::traits` bits.
pub mod weapon_traits {
    /// TSL's Door Cutting (64): a hit on a door or a placeable takes all
    /// its hit points.
    pub const DOOR_CUTTING: u8 = 1;
    /// TSL's Door Sabering (65): set, and read by nothing in either game.
    pub const DOOR_SABERING: u8 = 2;
}

/// What a weapon's on-hit property does to the creature it hits, as the
/// world applies it once the hit has landed (`on_hits`): held (stun,
/// sleep, paralysis; fear's hold ends on damage) or confused or slowed
/// for some ms, an ability drained a point for `DRAIN_MS`, a `poison.2da` row's
/// poison, knocked down (TSL's), or death.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Struck {
    Hold { ms: u64, fear: bool },
    Confuse(u64),
    Slow(u64),
    Drain(u8),
    Poison(u8),
    Knockdown,
    Death,
}

/// How long an on-hit drain lasts (ms).
pub const DRAIN_MS: u64 = rules::items::properties::DRAIN_MS;

/// What `weapon`'s on-hit properties do on a hit on `target` (its sheet as
/// it is now, its fighter for the immunities its effects and items give),
/// rolled from `rng` (the authority's stream), in the properties' order:
/// a property's chance first (a timed kind's own, sleep to slow; a drain's,
/// a poison's, a knockdown's and death's whole; a slay's the chance a timed
/// kind before it left this hit, else none: QUIRKS.md Q-0158), then its
/// save against its DC (a natural 20 saves, a natural 1 fails), then its
/// effect unless an immunity keeps it off. Sleep, stun, confusion and fear
/// ask Will (mind-affecting), paralysis, a slay and a knockdown Fortitude,
/// slow Will, a drain Reflex (mind-affecting, as the games code it);
/// poison and death no save. A slay against a side does nothing (an empty
/// handler in both games).
pub fn on_hits(
    weapon: &crate::combat::Weapon,
    target: &Sheet,
    fighter: &crate::combat::Fighter,
    now: u64,
    rng: &mut crate::rng::SimRng,
) -> Vec<Struck> {
    use crate::combat::sheet::feats;
    const FORT: usize = 0;
    const REFLEX: usize = 1;
    const WILL: usize = 2;
    let immune = |kind: u8| fighter.effects.immune_to(kind, now);
    let feat = |feat: u16| target.feats.has(feat);
    let mind = immune(immunity::MIND_SPELLS);
    // The chance the last timed kind left (a slay's).
    let mut timed = 0u8;
    let mut struck = Vec::new();
    for on_hit in weapon.on_hit.iter().flatten() {
        let ms = u64::from(on_hit.seconds) * 1000;
        if on_hit.kind <= on_hit::SLOW {
            timed = on_hit.chance;
        }
        let (chance, save, blocked, effect) = match on_hit.kind {
            on_hit::SLEEP => (
                timed,
                Some(WILL),
                mind || immune(immunity::SLEEP),
                Struck::Hold { ms, fear: false },
            ),
            on_hit::STUN => (
                timed,
                Some(WILL),
                mind || immune(immunity::STUN) || feat(feats::FORCE_IMMUNITY_STUN),
                Struck::Hold { ms, fear: false },
            ),
            on_hit::PARALYZE => (
                timed,
                Some(FORT),
                immune(immunity::PARALYSIS) || feat(feats::FORCE_IMMUNITY_PARALYSIS),
                Struck::Hold { ms, fear: false },
            ),
            on_hit::CONFUSION => (
                timed,
                Some(WILL),
                mind || immune(immunity::CONFUSED),
                Struck::Confuse(ms),
            ),
            on_hit::FEAR => (
                timed,
                Some(WILL),
                mind || immune(immunity::FEAR) || feat(feats::FORCE_IMMUNITY_FEAR),
                Struck::Hold { ms, fear: true },
            ),
            on_hit::SLOW => (timed, Some(WILL), immune(immunity::SLOW), Struck::Slow(ms)),
            on_hit::ABILITY_DRAIN => (
                100,
                Some(REFLEX),
                mind || immune(immunity::ABILITY_DECREASE),
                Struck::Drain(on_hit.param),
            ),
            on_hit::POISON => (
                100,
                None,
                immune(immunity::POISON),
                Struck::Poison(on_hit.param),
            ),
            on_hit::SLAY_RACE => (
                timed,
                Some(FORT),
                immune(immunity::DEATH) || target.race != on_hit.param,
                Struck::Death,
            ),
            on_hit::DEATH => (100, None, immune(immunity::DEATH), Struck::Death),
            on_hit::KNOCKDOWN => (
                100,
                Some(FORT),
                immune(immunity::KNOCKDOWN),
                Struck::Knockdown,
            ),
            _ => continue,
        };
        if rng.below(100) >= u32::from(chance) {
            continue;
        }
        let saved = save.is_some_and(|save| {
            let d20 = rng.below(rules::interaction::checks::DIE) as i32 + 1;
            crate::combat::check(d20, target.save(save), i32::from(on_hit.dc))
        });
        if !saved && !blocked {
            struck.push(effect);
        }
    }
    struck
}
