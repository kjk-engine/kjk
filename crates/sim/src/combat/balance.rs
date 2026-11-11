//! KOTOR II's enemy scaling (`rules::encounters::balance`, which says the
//! whole rule): a creature whose template names a scaling set
//! (`ObjectData::multiplier_set`, a row of `autobalance.2da`) grows with
//! the main character's level as the scripts keep it (`State::pc_level`).
//!
//! Its sheet is scaled once, as it first stands in the world and before its
//! spawn script runs (`scaled`: TSL's executable does it there too), and is
//! its own from then on (`Change::Sheet`). The dice of its weapon hits are
//! multiplied as each lands (`damage_times`, `world::harm`), whatever it
//! hits with: a held weapon, a beast's strike or its fists. A party member
//! is never scaled.

use rules::encounters::balance;

use super::{Sheet, Weapon};

/// A scaling set: `autobalance.2da`'s `vpmult`, `tohitmult`, `armormult`,
/// `damagemult`, `savemult` and `levelmult` of one row.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Balance {
    pub vitality: f32,
    pub to_hit: f32,
    pub armor: f32,
    pub damage: f32,
    pub save: f32,
    pub level: f32,
}

/// `levels` times a set's multiplier, rounded toward 0 as the executable's
/// conversion does.
fn share(levels: i32, by: f32) -> i32 {
    (levels as f32 * by) as i32
}

/// `value` plus `more`, kept within what the sheet's field holds.
fn grown<T: TryFrom<i32> + Into<i32> + Copy>(value: T, more: i32, (least, most): (T, T)) -> T {
    let sum = value.into().saturating_add(more);
    T::try_from(sum.clamp(least.into(), most.into())).unwrap_or(value)
}

impl Balance {
    /// The columns in a row's order, for the tables' digest.
    pub(crate) fn columns(&self) -> [f32; 6] {
        [
            self.vitality,
            self.to_hit,
            self.armor,
            self.damage,
            self.save,
            self.level,
        ]
    }

    /// `sheet` as this set scales it for a main character of `pc_level`
    /// (0: no script has told it yet).
    pub fn scaled(&self, mut sheet: Sheet, pc_level: i32) -> Sheet {
        let told = if pc_level > 0 {
            pc_level
        } else {
            balance::UNSET_LEVEL
        };
        let times = 1 + share((pc_level - balance::VITALITY_FROM).max(0), self.vitality);
        let most = i32::from(sheet.max_hp).saturating_mul(times);
        sheet.max_hp = most.clamp(1, i32::from(u16::MAX)) as u16;
        sheet.bab = grown(sheet.bab, share(told, self.to_hit), (u8::MIN, u8::MAX));
        sheet.defence = grown(sheet.defence, share(told, self.armor), (i8::MIN, i8::MAX));
        for save in &mut sheet.saves {
            *save = grown(*save, share(told, self.save), (i8::MIN, i8::MAX));
        }
        let level = i32::from(sheet.level) + share(told, self.level) - balance::LEVEL_LESS;
        let level = if level < 0 {
            balance::LEAST_LEVEL
        } else {
            level
        };
        sheet.level = level.clamp(0, i32::from(u8::MAX)) as u8;
        sheet
    }

    /// How many times over the dice of a weapon hit count for a main
    /// character of `pc_level`.
    pub fn damage_times(&self, pc_level: i32) -> u8 {
        let times = 1 + share((pc_level - balance::DAMAGE_FROM).max(0), self.damage);
        times.clamp(1, i32::from(u8::MAX)) as u8
    }

    /// `weapon` as a scaled creature hits with it: its own dice so many
    /// times over (`damage_times`), its bonus dice and bonuses as they were.
    pub fn weapon(&self, mut weapon: Weapon, pc_level: i32) -> Weapon {
        weapon.dice = weapon.dice.saturating_mul(self.damage_times(pc_level));
        weapon
    }
}
