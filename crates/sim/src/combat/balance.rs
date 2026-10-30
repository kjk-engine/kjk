//! TSL's creature autobalance: a creature whose template names an
//! `autobalance.2da` row (`MultiplierSet`, `module::ObjectData::multiplier_set`)
//! grows with the player's level as it was when the creature spawned.
//!
//! The executable (`CSWSCreature::HandleAutoBalancing`) does it once, from
//! the creature's first perception heartbeat (`SpawnInHeartbeatPerception`'s
//! first beat), just before its `OnSpawn` runs: it keeps the global number
//! `G_PC_LEVEL` as the creature's `PCLevelAtSpawn` (P), and multiplies its
//! hit points (the template's `HitPoints`) by the row's `VPMult`, its current
//! hit points the new most. Its getters read P from then on, every time:
//! `GetBaseAttackBonus` adds `ToHitMult` × P, `GetArmorClass` `ArmorMult` ×
//! P, `GetBase{Fort,Will,Reflex}SavingThrow` `SaveMult` × P, each truncated
//! (P 0 counting as 1); `GetDamageRoll` and `GetUnarmedDamageRoll` multiply
//! each weapon die by `DamageMult` × (P − 1), truncated, + 1;
//! `CSWSCreatureStats::GetLevel` adds `LevelMult` × P, truncated, − 1; and
//! `CalculateDeathExperience` rates its death `CRMod` past P
//! (`script::xp::Experience::tsl_kill`). None of it runs again on a load:
//! `SaveStats` and `ReadStatsFromGff` keep the set, P and the grown hit
//! points. A player's character and the party's are never balanced (the
//! executable's guard: the set not 0 and the creature no PC).
//!
//! Here the spawn's handling is `autobalance`, where kjk queues a creature's
//! spawn script (`SimWorld::enter_module` for the module's own on its first
//! visit, `world::scripts::apply` for one a script or an encounter made);
//! P and what the getters add are kept in the creature's fighter
//! (`Fighter::balance`, carried in the snapshot a save holds), and
//! `effects::sheet_now` folds them into the sheet every reader of it takes,
//! as the executable's getters do; the grown hit points are the creature's
//! own sheet's (`Sheet::max_hp`), as the executable stores them.

use crate::combat::fighter::Fighter;
use crate::combat::sheet::Sheet;
use crate::wire::{DecodeError, Reader, Writer};

/// A row of TSL's `autobalance.2da` (`MultiplierSet` indexes it; row 0,
/// `No_Auto_Balance`, is no balance): the game's data, read by the session
/// (`session::journal::experience`) into `script::xp::Experience::balance`.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Multipliers {
    /// `VPMult`: hit points.
    pub vp: f32,
    /// `ToHitMult`: the base attack bonus.
    pub to_hit: f32,
    /// `ArmorMult`: defence.
    pub armour: f32,
    /// `DamageMult`: the weapon's dice.
    pub damage: f32,
    /// `SaveMult`: the three base saves.
    pub save: f32,
    /// `CRMod`: the death's challenge rating past P.
    pub crmod: i32,
    /// `LevelMult`: the level (`GetHitDice`).
    pub level: f32,
}

/// `multiplier` × `by`, truncated toward 0, in single precision as TSL's
/// Mac executable has it (`cvtsi2ss`, `mulss`, `cvttss2si` in
/// `HandleAutoBalancing`, `GetBaseAttackBonus`, `GetArmorClass`, `GetLevel`
/// and `GetDamageRoll`): 0.9 × 10 is 9.
fn times(multiplier: f32, by: i32) -> i32 {
    (multiplier * by as f32) as i32
}

impl Multipliers {
    /// What the getters add for a creature spawned at P `pc_level`
    /// (`Balance`), and P itself.
    pub fn balance(&self, pc_level: u8) -> Balance {
        let p = i32::from(pc_level);
        // `P ? P : 1.0`: the attack, armour and save getters' level.
        let at_least_one = p.max(1);
        let small = |value: i32| value.clamp(i32::from(i8::MIN), i32::from(i8::MAX)) as i8;
        Balance {
            pc_level,
            attack: small(times(self.to_hit, at_least_one)),
            armour: small(times(self.armour, at_least_one)),
            save: small(times(self.save, at_least_one)),
            // `GetLevel`'s level: P, else `G_PC_LEVEL` while P is 0, else
            // 1. P is `G_PC_LEVEL` as the creature spawned, so a P of 0 had
            // it at 0 then: 1, and a later global is not followed.
            level: small(times(self.level, at_least_one) - 1),
            dice: if p == 0 {
                1
            } else {
                (times(self.damage, p - 1) + 1).clamp(0, i32::from(u8::MAX)) as u8
            },
        }
    }

    /// The template's `HitPoints` (`base`) grown for P `pc_level`
    /// (`HandleAutoBalancing`): × (`VPMult` × (P − 2), truncated, + 1) from
    /// P 2 on; as it is below.
    pub fn hit_points(&self, base: i32, pc_level: u8) -> i32 {
        let p = i32::from(pc_level);
        if p < 2 {
            return base;
        }
        base.saturating_mul(times(self.vp, p - 2) + 1)
    }
}

/// What an autobalanced creature's getters add, kept from its spawn
/// (`Fighter::balance`): P and each term the executable works out from P and
/// the creature's row whenever it is asked (the row is the game's data and P
/// never changes, so the terms do not).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Balance {
    /// `PCLevelAtSpawn`: `G_PC_LEVEL` as it spawned.
    pub pc_level: u8,
    /// `ToHitMult` × P, added to the base attack bonus (`GetBaseAttackBonus`).
    pub attack: i8,
    /// `ArmorMult` × P, added to defence (`GetArmorClass`,
    /// `GetArmorClassVersus`).
    pub armour: i8,
    /// `SaveMult` × P, added to each base save (`GetBase…SavingThrow`).
    pub save: i8,
    /// `LevelMult` × P − 1, added to the level (`CSWSCreatureStats::GetLevel`).
    pub level: i8,
    /// What each die of its weapon's damage is multiplied by (`GetDamageRoll`,
    /// `GetUnarmedDamageRoll`): `DamageMult` × (P − 1), truncated, + 1.
    pub dice: u8,
}

impl Balance {
    /// `sheet` as the getters read it: the base attack bonus, defence, the
    /// three base saves and the level with the terms added; a level below 0
    /// is 1, as `GetLevel` makes it.
    pub fn fold(&self, sheet: &mut Sheet) {
        sheet.bab = (i32::from(sheet.bab) + i32::from(self.attack)).clamp(0, 255) as u8;
        sheet.defence = sheet.defence.saturating_add(self.armour);
        for save in &mut sheet.saves {
            *save = save.saturating_add(self.save);
        }
        let level = i32::from(sheet.level) + i32::from(self.level);
        sheet.level = if level < 0 { 1 } else { level.min(255) as u8 };
    }

    pub(crate) fn encode(&self, out: &mut Writer) {
        out.u8(self.pc_level);
        out.i8(self.attack);
        out.i8(self.armour);
        out.i8(self.save);
        out.i8(self.level);
        out.u8(self.dice);
    }

    pub(crate) fn decode(input: &mut Reader) -> Result<Balance, DecodeError> {
        Ok(Balance {
            pc_level: input.u8()?,
            attack: input.i8()?,
            armour: input.i8()?,
            save: input.i8()?,
            level: input.i8()?,
            dice: input.u8()?,
        })
    }
}

/// The weapon dice factor of a fighter's blows: its balance's, else 1.
pub fn dice(fighter: &Fighter) -> u8 {
    fighter.balance.map_or(1, |balance| balance.dice)
}

/// A creature's spawn handled (`HandleAutoBalancing`), in TSL only: its
/// template's set `set` a row of `rows`, not 0, P the global number
/// `G_PC_LEVEL` now (`pc_level`, 0 where it was never set). Its sheet's most
/// hit points grow from the template's `HitPoints` (`base`; the share
/// Constitution adds over it, which the sheet's most carries, as it was),
/// its fighter stands at the new most, and keeps P and the terms
/// (`Fighter::balance`). Whether it may be balanced at all (no player, no
/// party member, TSL) is the caller's.
pub fn autobalance(
    sheet: &mut Sheet,
    fighter: &mut Fighter,
    (set, rows): (u8, &[Multipliers]),
    base: Option<i32>,
    pc_level: u8,
) {
    let Some(row) = rows.get(usize::from(set)).filter(|_| set != 0) else {
        return;
    };
    let most = i32::from(sheet.max_hp);
    let base = base.unwrap_or(most).clamp(0, most);
    let grown = row.hit_points(base, pc_level) + (most - base);
    sheet.max_hp = grown.clamp(1, i32::from(u16::MAX)) as u16;
    fighter.hp = i32::from(sheet.max_hp) * 100;
    fighter.balance = Some(row.balance(pc_level));
}
