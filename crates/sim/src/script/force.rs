//! TSL's combat and Force routines (`context/artifacts/i35/combat-force.md`,
//! from its executable's commands): the spell book (a power's base cost and
//! `formmask`, known or granted), the Forms in use (`combat::forms`), bonus
//! Force points, the story's permanent stat changes (abilities, skills,
//! base saves), the skill ranks scripts read, a creature's queue and gait,
//! fake combat and the weapon flourish, and a heartbeat run at once.

use script_ncs::vm::{ObjectId, Value};

use super::engine::{Bound, Engine, int, object, truth};
use super::{Change, Character, Pending};
use crate::combat::Sheet;
use crate::combat::forms;
use crate::combat::sheet::{Ability, Rows, skills};
use crate::module::ObjectKind;
use crate::snapshot::StatusKind;

/// Force Camouflage (`spells.2da` 156): known, a Stealth of no ranks counts
/// `CAMOUFLAGE_RANK`, as the executable's skill rank has it.
const FORCE_CAMOUFLAGE: u16 = 156;
const CAMOUFLAGE_RANK: u8 = rules::force::routines::CAMOUFLAGE_STEALTH;

/// `Modify…SavingThrowBase`'s saves, by routine: Fortitude, Reflex, Will
/// (`Sheet::save_bonus`).
fn save_of(bound: Bound) -> usize {
    match bound {
        Bound::ModifyFortitudeSavingThrowBase => 0,
        Bound::ModifyReflexSavingThrowBase => 1,
        _ => 2,
    }
}

/// How many lines `GetRandomDestination` tries, how much shorter each
/// blocked one is tried again, the length below which it turns instead and
/// the one below which it gives up (metres; TSL's executable's, the book's
/// `force.routines`).
const DESTINATION_TRIES: usize = rules::force::routines::DESTINATION_TRIES;
const DESTINATION_SHORTER: f32 = rules::force::routines::DESTINATION_SHORTER;
const DESTINATION_TURN_M: f32 = rules::force::routines::DESTINATION_TURN_M;
const DESTINATION_LEAST_M: f32 = rules::force::routines::DESTINATION_LEAST_M;

/// TSL's `GetRandomDestination` from `from` within `range` (its
/// executable's): a place a whole number of metres off along each axis
/// (`range` at most, each way; `below` draws 0 to its argument less one)
/// on the floor there (`floor`); while a body cannot walk straight to it
/// (`walks`), tried again `DESTINATION_SHORTER` as far along the same line,
/// and once that is under `DESTINATION_TURN_M` turned a quarter left, a
/// quarter right or right round (one in three each) at the first length;
/// given up under `DESTINATION_LEAST_M` or after `DESTINATION_TRIES` lines:
/// where it stands. A range of 0 or less (the executable
/// divides by it) is where it stands.
#[allow(clippy::type_complexity)]
pub(crate) fn random_destination(
    from: [f32; 3],
    range: i32,
    below: &mut dyn FnMut(u32) -> u32,
    (floor, walks): (
        &dyn Fn([f32; 3]) -> Option<f32>,
        &dyn Fn([f32; 3], [f32; 3]) -> bool,
    ),
) -> [f32; 3] {
    let Ok(range) = u32::try_from(range).map(|range| range.min(u32::MAX / 2)) else {
        return from;
    };
    if range == 0 {
        return from;
    }
    let mut offset = || below(range * 2) as f32 - range as f32;
    let (dx, dy) = (offset(), offset());
    let mut to = [from[0] + dx, from[1] + dy, from[2]];
    to[2] = floor(to).unwrap_or(from[2]);
    let mut way = crate::math::sub(to, from);
    let first = crate::math::normalize(&mut way);
    let mut far = first;
    for _ in 0..DESTINATION_TRIES {
        if walks(from, to) {
            return to;
        }
        far *= DESTINATION_SHORTER;
        if far < DESTINATION_TURN_M {
            way = match below(3) {
                1 => [-way[1], way[0], 0.0],
                2 => [way[1], -way[0], 0.0],
                _ => way.map(|v| -v),
            };
            far = first;
        }
        to = crate::math::mad(from, far, way);
        if far < DESTINATION_LEAST_M {
            break;
        }
    }
    from
}

impl Engine<'_> {
    /// `who`'s character and its stored sheet as this step leaves it.
    fn stored(&self, who: ObjectId) -> Option<(Character, Sheet)> {
        let c = self.character(who)?;
        Some((c, self.out.sheet(who, c.worn)))
    }

    /// `who`'s stored sheet changed by `change`, if it is a character; it
    /// rides in the snapshot from now on.
    fn restore(&mut self, who: ObjectId, change: impl FnOnce(&mut Sheet, &Character)) {
        if let Some((c, mut sheet)) = self.stored(who) {
            change(&mut sheet, &c);
            self.out.changes.push(Change::Sheet { who, sheet });
        }
    }

    /// What power `row` costs `c` now (whole FP; `ModuleData::power_cost`,
    /// TSL's adjustments by alignment, Charisma, the room's Force rating
    /// and the Form in use), where `who` stands; `None` for a row the table
    /// lacks.
    pub(super) fn power_cost(&self, who: ObjectId, c: &Character, row: u16) -> Option<i32> {
        let at = self.position(who).unwrap_or([0.0; 3]);
        self.view.module.power_cost(row, &c.sheet, c.party, at)
    }

    /// A skill's rank as TSL's `GetSkillRank` reads it: the stored rank,
    /// with effects and worn items, the feats' and the key ability's
    /// modifier (`stealth::total`); 0 for a skill only the trained use that
    /// it has no ranks in (Stealth counts `CAMOUFLAGE_RANK` with Force
    /// Camouflage known).
    pub(super) fn skill_rank(&self, c: &Character, skill: usize) -> i32 {
        let camouflaged = skill == skills::STEALTH
            && c.worn.skills[skill] == 0
            && c.worn.powers.has(FORCE_CAMOUFLAGE);
        if camouflaged {
            let mut worn = c.worn;
            worn.skills[skill] = CAMOUFLAGE_RANK;
            let mut now = c.sheet;
            now.skills[skill] = now.skills[skill].saturating_add(CAMOUFLAGE_RANK);
            return i32::from(
                crate::stealth::total(Some(self.library), &worn, &now, skill).unwrap_or(0),
            );
        }
        i32::from(crate::stealth::total(Some(self.library), &c.worn, &c.sheet, skill).unwrap_or(0))
    }

    /// TSL's combat and Force routines (see the module documentation).
    pub(super) fn force_call(&mut self, bound: Bound, args: &[Value]) -> Value {
        use Bound::*;
        let module = self.view.module;
        let row = || u16::try_from(int(args, 0)).ok();
        match bound {
            // The row's `forcepoints`, no caster's adjustment; 0 for a row
            // the table lacks.
            GetSpellBaseForcePointCost => Value::Int(
                row()
                    .and_then(|row| module.force_costs.get(usize::from(row)))
                    .map_or(0, |cost| i32::from(*cost)),
            ),
            GetSpellFormMask => Value::Int(
                row()
                    .and_then(|row| module.force.masks.get(usize::from(row)))
                    .map_or(0, |mask| i32::from(*mask)),
            ),
            // Known, whatever it costs (`GetHasSpell` asks it be paid for).
            GetSpellAcquired => truth(
                row()
                    .zip(self.stored(object(args, 1)))
                    .is_some_and(|(row, (_, sheet))| sheet.powers.has(row)),
            ),
            // Known from now, no prerequisite or level asked (the last
            // class's known list in the executable).
            GrantSpell => {
                if let Some(row) = row().filter(|row| *row < Rows::<5>::ROWS) {
                    self.restore(object(args, 1), |sheet, _| sheet.powers.insert(row));
                }
                Value::Int(0)
            }
            // In force: its Form in use as this step leaves it, with what it
            // holds now.
            IsFormActive => truth(
                u16::try_from(int(args, 1))
                    .ok()
                    .zip(self.stored(object(args, 0)))
                    .is_some_and(|(form, (c, sheet))| {
                        let now = Sheet {
                            form: sheet.form,
                            ..c.sheet
                        };
                        forms::is_active(&now, form)
                    }),
            ),
            // A power it knows becomes the one in use, whatever its row (the
            // executable checks no more).
            SetCurrentForm => {
                if let Ok(form) = u16::try_from(int(args, 1)) {
                    self.restore(object(args, 0), |sheet, _| {
                        if sheet.powers.has(form) {
                            sheet.form = form;
                        }
                    });
                }
                Value::Int(0)
            }
            // No stance is ever taken here (TSL's total defence is reached
            // by no input nor script), so none is in it.
            IsInTotalDefense => truth(false),
            AddBonusForcePoints | SetBonusForcePoints => {
                let who = object(args, 0);
                let by = int(args, 1);
                let told = self.is_pc(who);
                let Some((_, sheet)) = self.stored(who) else {
                    return Value::Int(0);
                };
                let was = i32::from(sheet.bonus_fp);
                let to = match bound {
                    AddBonusForcePoints => was + by,
                    _ => by,
                };
                // The executable tells an add, and a set that changes the
                // bonus, by its new value.
                let status = match bound {
                    AddBonusForcePoints => Some((by >= 0, by)),
                    _ if to != was => Some((to > was, to)),
                    _ => None,
                };
                if let Some((gained, by)) = status.filter(|_| told) {
                    self.out
                        .statuses
                        .push(StatusKind::MaxForcePoints { gained, by });
                }
                self.restore(who, |sheet, _| {
                    sheet.bonus_fp = to.clamp(i32::from(i16::MIN), i32::from(i16::MAX)) as i16;
                });
                Value::Int(0)
            }
            GetBonusForcePoints => Value::Int(
                self.stored(object(args, 0))
                    .map_or(0, |(_, sheet)| i32::from(sheet.bonus_fp)),
            ),
            // The base score moved as a byte (it wraps); Constitution's
            // change moves the most hit points and the hit points with it,
            // Wisdom's a Force user's most FP, each by the modifier's change
            // a level.
            AdjustCreatureAttributes => {
                let who = object(args, 0);
                let (Some(ability), Some((c, mut sheet))) = (
                    usize::try_from(int(args, 1))
                        .ok()
                        .and_then(|at| Ability::ALL.get(at).copied()),
                    self.stored(who),
                ) else {
                    return Value::Int(0);
                };
                let before = sheet.modifier(ability);
                let score = &mut sheet.abilities[ability as usize];
                *score = score.wrapping_add(int(args, 2) as u8);
                let by = (sheet.modifier(ability) - before) * i32::from(sheet.level);
                let moved = |most: u16| (i32::from(most) + by).clamp(0, i32::from(u16::MAX)) as u16;
                let mut hp = None;
                match ability {
                    Ability::Constitution => {
                        sheet.max_hp = moved(sheet.max_hp).max(1);
                        hp = self
                            .view
                            .thing(who)
                            .and_then(|thing| thing.hp)
                            .map(|(hp, _)| (hp + by).clamp(1, i32::from(sheet.max_hp)) as u16);
                    }
                    Ability::Wisdom if c.worn.max_fp > 0 => sheet.max_fp = moved(sheet.max_fp),
                    _ => {}
                }
                self.out.changes.push(Change::Sheet { who, sheet });
                if let Some(hp) = hp {
                    self.out.changes.push(Change::HitPoints { who, hp });
                }
                Value::Int(0)
            }
            // The stored rank moved (a signed byte; none below 0 here).
            AdjustCreatureSkills => {
                if let Ok(skill) = usize::try_from(int(args, 1)) {
                    let by = int(args, 2);
                    self.restore(object(args, 0), |sheet, _| {
                        if let Some(rank) = sheet.skills.get_mut(skill) {
                            *rank = (i32::from(*rank) + by).clamp(0, i32::from(i8::MAX)) as u8;
                        }
                    });
                }
                Value::Int(0)
            }
            ModifyFortitudeSavingThrowBase
            | ModifyReflexSavingThrowBase
            | ModifyWillSavingThrowBase => {
                let (save, by) = (save_of(bound), int(args, 1) as i8);
                self.restore(object(args, 0), |sheet, _| {
                    sheet.save_bonus[save] = sheet.save_bonus[save].wrapping_add(by);
                });
                Value::Int(0)
            }
            // The stored rank, no effects, items nor ability; 0 for a skill
            // the table lacks; −1 for what is not a character.
            GetSkillRankBase => Value::Int(match self.character(object(args, 1)) {
                Some(c) => usize::try_from(int(args, 0))
                    .ok()
                    .and_then(|skill| c.worn.skills.get(skill))
                    .map_or(0, |rank| i32::from(*rank)),
                None => -1,
            }),
            // An object not a creature −1, no object 0.
            GetRacialSubType => {
                let who = object(args, 0);
                Value::Int(match self.character(who) {
                    Some(c) => i32::from(c.subrace),
                    None if self.view.thing(who).is_some() => -1,
                    None => 0,
                })
            }
            IsRunning => truth(self.character(object(args, 0)).is_some_and(|c| c.running)),
            // Anything in its queue (the executable's action list).
            GetCombatActionsPending => {
                let who = object(args, 0);
                truth(
                    self.character(who).is_some()
                        && self
                            .state
                            .actions
                            .get(&who.0)
                            .is_some_and(|queue| !queue.is_empty()),
                )
            }
            SetFakeCombatState => {
                let who = object(args, 0);
                if self.character(who).is_some() {
                    let on = int(args, 1) != 0;
                    self.out.changes.push(Change::FakeCombat { who, on });
                }
                Value::Int(0)
            }
            CreatureFlourishWeapon => {
                let who = object(args, 0);
                if self.character(who).is_some() {
                    self.out.changes.push(Change::Flourish { who });
                }
                Value::Int(0)
            }
            // Its heartbeat script now, and its period from now (the
            // executable runs it whether or not its heartbeat was removed).
            ForceHeartbeat => {
                let who = object(args, 0);
                if self
                    .view
                    .thing(who)
                    .is_some_and(|thing| thing.kind == Some(ObjectKind::Creature))
                {
                    if let Some(script) = self.view.hook(who, super::Hook::Heartbeat) {
                        self.state.pending.push(Pending::new(&script, who));
                    }
                    self.state.beats.insert(who.0, self.view.now_ms);
                }
                Value::Int(0)
            }
            _ => Value::Int(0),
        }
    }
}
