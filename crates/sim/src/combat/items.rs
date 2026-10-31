//! KOTOR's consumables in a fight (`docs/COMBAT.md`): what a character
//! carries (up to four kinds, each a `spells.2da` item-ability row and a
//! count, `Sheet::items`), which button uses one and what it does where no
//! script can (`use_of`), and what it has used and what still runs on it
//! (`Pack`, in the fighter).
//!
//! An item's use is its row's impact script wherever the level has it
//! (`k_sup_healing`, `k_sup_bands`, `k_sup_comshots`, `k_sup_grenade`):
//! cast on its user (`press`, `settle`) or where a grenade bursts. `use_of`
//! names each kind by its row's label (`combat::spells`) and stands in for
//! those scripts only where none can run (the arena, a test's stage), with
//! the numbers K1's scripts use:
//!
//! - **Grenades**: `combat::grenades`, thrown with their button.
//! - **Medpacs**: heal their points + WIS modifier + their points a rank ×
//!   Treat Injury; not for droids. **Repair kits**: their points + INT
//!   modifier + their points a rank × Repair; droids only. A use takes
//!   less time with each rank of the skill (`heal_ms`; the design's "use
//!   time shorter with rank"), the weapon idle meanwhile; the healing lands
//!   at its end.
//! - **Antidote kit**: ends poison and restores lost attributes.
//! - **Squad recovery stim**: its points (`k_sup_healing`'s) to everyone
//!   on the user's side within `SQUAD_REACH`, droids too [verify: the script heals
//!   every party member wherever it stands and raises the fallen; the
//!   reach and the side are this stand-in's].
//! - **Energy shields** (a droid's worn ones too): absorb up to their
//!   points (`forceshields.2da` `amount`, the row `k_sup_bands` gives
//!   each) of the damage types they name [verify: the types are the
//!   design's, not the rows' `damageflags`], in all, for `SHIELD_MS` or
//!   until spent; one at a time.
//! - **Stims**: for `STIM_MS`, adrenals raise STR, DEX or CON; battle stims
//!   add to hit and damage and give vitality, taken before hit points and
//!   gone when they end.
//!
//! The numbers are the book's (`rules::items`).
//!
//! Each has its button (`input::buttons`): `MEDPAC` (an antidote while
//! afflicted, else its own kind of heal, else a squad stim), `SHIELD`,
//! `STIM`; grenades `GRENADE`.

use crate::combat::idle::Acting;
use crate::combat::sheet::{Ability, DAMAGE_TYPES, Sheet, races, skills};
use crate::combat::{Fighter, effects};
use crate::snapshot::SimId;
use crate::wire::{DecodeError, Reader, Writer, read_fixed, write_fixed};

/// How many kinds a character carries.
pub const KINDS: usize = 4;
/// How long a shield and a stim last (ms).
pub const SHIELD_MS: u64 = rules::items::shields::MS;
pub const STIM_MS: u64 = rules::items::stims::MS;
/// A squad stim's reach (units).
pub const SQUAD_REACH: f32 = rules::items::squad_stim::REACH_M / collision::UNIT;
/// How long a use other than a heal takes (ms): an item's `spells.2da`
/// row's conjure and cast time, its impact at the end.
pub const USE_MS: u64 = rules::items::using::USE_MS;

/// The clips a use plays on its user (`ANIMATION_FIREFORGET_…`, whose
/// `animations.2da` rows TSL describes): a shield's band switched on at
/// the wrist (`activate`, "Activate_Wrist"), anything else injected
/// (`inject`, "Inject_Self_In_Leg"; its weapons hidden meanwhile,
/// `hideequippeditems`).
pub mod clips {
    pub const INJECT: u16 = 112;
    pub const ACTIVATE: u16 = 115;
}

/// What a stim does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stim {
    /// An adrenal: an ability (`Ability` as its index) up by `amount`.
    Ability(u8, u8),
    /// A battle stimulant: to hit and damage up by `bonus`, and `vitality`
    /// hit points for its time.
    Battle { bonus: u8, vitality: u8 },
}

/// What using an item does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Use {
    /// A grenade (`combat::grenades`).
    Grenade,
    /// Heals its user (living, or a droid for a repair kit): `points` + its
    /// WIS (INT) modifier + `per_rank` × its Treat Injury (Repair).
    Heal {
        points: u8,
        per_rank: u8,
        droid: bool,
    },
    /// Ends poison and restores lost attributes.
    Antidote,
    /// Heals everyone on its user's side nearby.
    Squad {
        points: u8,
    },
    /// Absorbs up to `points` of the damage types `types` (`damageflags`
    /// bits).
    Shield {
        points: u16,
        types: u16,
    },
    Stim(Stim),
}

impl Use {
    /// The clip its use plays (`clips`).
    pub fn clip(self) -> u16 {
        match self {
            Use::Shield { .. } => clips::ACTIVATE,
            _ => clips::INJECT,
        }
    }

    /// Which button uses it.
    pub fn button(self) -> u16 {
        use crate::input::buttons;
        match self {
            Use::Grenade => buttons::GRENADE,
            Use::Heal { .. } | Use::Antidote | Use::Squad { .. } => buttons::MEDPAC,
            Use::Shield { .. } => buttons::SHIELD,
            Use::Stim(_) => buttons::STIM,
        }
    }
}

/// What the item of `spells.2da` row `row` is, by the row's label: which
/// button takes it, and what it does where no impact script runs; `None`
/// for a row no button takes.
pub fn use_of(row: u16) -> Option<Use> {
    use crate::combat::sheet::damage_types as t;
    const ELECTRICAL: u16 = 128;
    const COLD: u16 = 32;
    const FIRE: u16 = 256;
    let physical = t::BLUDGEONING | t::PIERCING | t::SLASHING;
    let shield = |points: u16, types: u16| Use::Shield { points, types };
    let heal = |points, per_rank, droid| Use::Heal {
        points,
        per_rank,
        droid,
    };
    let adrenal = |ability: Ability, amount| Use::Stim(Stim::Ability(ability as u8, amount));
    let battle = |bonus, vitality| Use::Stim(Stim::Battle { bonus, vitality });
    if crate::combat::grenades::effect(row).is_some() {
        return Some(Use::Grenade);
    }
    use crate::combat::spells::*;
    use rules::items::medpacs;
    use rules::items::repair_kits;
    use rules::items::shields;
    use rules::items::squad_stim;
    use rules::items::stims;
    let hazard = t::ENERGY | t::SONIC | COLD | FIRE | ELECTRICAL;
    Some(match row {
        ITEM_ABILITY_MED_PACK => heal(medpacs::POINTS[0], medpacs::PER_RANK[0], false),
        ITEM_ABILITY_ADVANCED_MED_PACK => heal(medpacs::POINTS[1], medpacs::PER_RANK[1], false),
        ITEM_ABILITY_LIFE_SUPPORT_KIT => heal(medpacs::POINTS[2], medpacs::PER_RANK[2], false),
        ITEM_ABILITY_REMOVE_POISON => Use::Antidote,
        ITEM_ABILITY_DROID_REPAIR_KIT => {
            heal(repair_kits::POINTS[0], repair_kits::PER_RANK[0], true)
        }
        ITEM_ABILITY_DROID_REPAIR_KIT_ADV => {
            heal(repair_kits::POINTS[1], repair_kits::PER_RANK[1], true)
        }
        ITEM_ABILITY_DROID_CONSTRUCTION_KIT => {
            heal(repair_kits::POINTS[2], repair_kits::PER_RANK[2], true)
        }
        ITEM_ABILITY_RECOVERY_STIM => Use::Squad {
            points: squad_stim::POINTS,
        },
        ITEM_ABILITY_ADRENAL_SHOT_STRENGTH => adrenal(Ability::Strength, stims::ADRENAL),
        ITEM_ABILITY_ADRENAL_SHOT_ALACRITY => adrenal(Ability::Dexterity, stims::ADRENAL),
        ITEM_ABILITY_ADRENAL_SHOT_STAMINA => adrenal(Ability::Constitution, stims::ADRENAL),
        ITEM_ABILITY_HYPER_ADRENAL_SHOT_STRENGTH => {
            adrenal(Ability::Strength, stims::HYPER_ADRENAL)
        }
        ITEM_ABILITY_HYPER_ADRENAL_SHOT_ALACRITY => {
            adrenal(Ability::Dexterity, stims::HYPER_ADRENAL)
        }
        ITEM_ABILITY_HYPER_ADRENAL_SHOT_STAMINA => {
            adrenal(Ability::Constitution, stims::HYPER_ADRENAL)
        }
        ITEM_ABILITY_COMBAT_SHOT_1 => battle(stims::BATTLE_BONUS[0], stims::BATTLE_VITALITY[0]),
        ITEM_ABILITY_COMBAT_SHOT_2 => battle(stims::BATTLE_BONUS[1], stims::BATTLE_VITALITY[1]),
        ITEM_ABILITY_COMBAT_SHOT_3 => battle(stims::BATTLE_BONUS[2], stims::BATTLE_VITALITY[2]),
        ITEM_ABILITY_ENERGY_SHIELD => shield(shields::ENERGY, t::ENERGY | ELECTRICAL),
        ITEM_ABILITY_SITH_ENERGY_SHIELD => shield(shields::SITH, t::ENERGY | t::SONIC | ELECTRICAL),
        ITEM_ABILITY_ARKANIAN_ENERGY_SHIELD => shield(shields::ARKANIAN, hazard),
        ITEM_ABILITY_ECHANI_SHIELD => shield(shields::ECHANI, t::ENERGY | t::SONIC | ELECTRICAL),
        ITEM_ABILITY_MANDLORIAN_MELEE_SHIELD => shield(shields::MANDALORIAN_MELEE, physical),
        ITEM_ABILITY_MANDLORIAN_POWER_SHIELD => shield(
            shields::MANDALORIAN_POWER,
            t::ENERGY | physical | ELECTRICAL,
        ),
        ITEM_ABILITY_ECHANI_DUELING_SHIELD => {
            shield(shields::ECHANI_DUELING, t::ENERGY | ELECTRICAL)
        }
        ITEM_ABILITY_YUSANIS_DUELING_SHIELD => shield(shields::YUSANIS, t::ENERGY | ELECTRICAL),
        ITEM_ABILITY_VERPINE_ENERGY_SHIELD => shield(shields::VERPINE, hazard),
        DROID_ITEM_ENERGY_SHIELD_1 => shield(shields::DROID_ENERGY[0], t::ENERGY | ELECTRICAL),
        DROID_ITEM_ENERGY_SHIELD_2 => shield(shields::DROID_ENERGY[1], t::ENERGY | ELECTRICAL),
        DROID_ITEM_ENERGY_SHIELD_3 => shield(shields::DROID_ENERGY[2], t::ENERGY | ELECTRICAL),
        DROID_ITEM_HAZARD_SHIELD_1 => shield(shields::DROID_HAZARD[0], hazard),
        DROID_ITEM_HAZARD_SHIELD_2 => shield(shields::DROID_HAZARD[1], hazard),
        DROID_ITEM_HAZARD_SHIELD_3 => shield(shields::DROID_HAZARD[2], hazard),
        _ => return None,
    })
}

/// How long a heal takes a user of rank `rank` in its skill (ms): less a
/// step a rank, down to the quickest (`rules::items::using`).
pub fn heal_ms(rank: u8) -> u64 {
    use rules::items::using;
    using::HEAL_MS
        .saturating_sub(using::PER_RANK_MS * u64::from(rank))
        .max(using::LEAST_MS)
}

/// What a heal restores `sheet` (whole points).
pub fn healing(sheet: &Sheet, points: u8, per_rank: u8, droid: bool) -> i32 {
    let (ability, skill) = if droid {
        (Ability::Intelligence, skills::REPAIR)
    } else {
        (Ability::Wisdom, skills::TREAT_INJURY)
    };
    (i32::from(points) + sheet.modifier(ability) + i32::from(per_rank) * sheet.skill(skill).max(0))
        .max(1)
}

/// An item kind a character carries: its `spells.2da` row and how many.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Carried {
    pub row: u16,
    pub count: u8,
}

impl Carried {
    pub(crate) const BLANK: Carried = Carried { row: 0, count: 0 };

    pub(crate) fn encode(&self, out: &mut Writer) {
        out.u16(self.row);
        out.u8(self.count);
    }

    pub(crate) fn decode(input: &mut Reader) -> Result<Carried, DecodeError> {
        Ok(Carried {
            row: input.u16()?,
            count: input.u8()?,
        })
    }
}

/// A poison's course, a `poison.2da` row's (`EffectPoison`; a poison
/// grenade's): hit points a dose, ability points a dose drains (STR, DEX,
/// CON, INT, WIS, CHA, `Ability`'s order), a dose every `every_ms` for
/// `for_ms`, the drained points back `drain_ms` after the last dose, and the
/// Fortitude DC that keeps it off where it lands (0 for none).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Course {
    pub points: u8,
    pub drains: [u8; 6],
    pub every_ms: u32,
    pub for_ms: u32,
    pub drain_ms: u32,
    pub dc: u8,
}

impl Course {
    pub(crate) fn encode(&self, out: &mut Writer) {
        out.u8(self.points);
        out.bytes(&self.drains);
        out.u32(self.every_ms);
        out.u32(self.for_ms);
        out.u32(self.drain_ms);
        out.u8(self.dc);
    }

    pub(crate) fn decode(input: &mut Reader) -> Result<Course, DecodeError> {
        Ok(Course {
            points: input.u8()?,
            drains: input.array()?,
            every_ms: input.u32()?,
            for_ms: input.u32()?,
            drain_ms: input.u32()?,
            dc: input.u8()?,
        })
    }
}

/// `points` drained of `ability` alone (a sonic grenade's DEX).
pub fn drain(ability: Ability, points: u8) -> [u8; 6] {
    let mut drained = [0; 6];
    drained[ability as usize] = points;
    drained
}

/// A poison running: its course, who poisoned it (the doses' attacker),
/// the next dose and the last (sim ms), and the script effect it came
/// linked in (`EffectLinkEffects`), whose other parts end with it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Poisoned {
    pub by: SimId,
    pub course: Course,
    pub next: u64,
    pub until: u64,
    pub link: Option<u32>,
}

impl Poisoned {
    const BLANK: Poisoned = Poisoned {
        by: SimId(0),
        course: Course {
            points: 0,
            drains: [0; 6],
            every_ms: 0,
            for_ms: 0,
            drain_ms: 0,
            dc: 0,
        },
        next: 0,
        until: 0,
        link: None,
    };

    /// `course` taken at `now`, `by` (in the script effect `link`): its
    /// first dose a period on.
    pub fn new(by: SimId, course: Course, now: u64, link: Option<u32>) -> Poisoned {
        Poisoned {
            by,
            course,
            next: now + u64::from(course.every_ms),
            until: now + u64::from(course.for_ms),
            link,
        }
    }

    fn encode(&self, out: &mut Writer) {
        out.u32(self.by.0);
        self.course.encode(out);
        out.u64(self.next);
        out.u64(self.until);
        write_fixed(out, self.link.as_ref(), &0, |link, out| out.u32(*link));
    }

    fn decode(input: &mut Reader) -> Result<Poisoned, DecodeError> {
        Ok(Poisoned {
            by: SimId(input.u32()?),
            course: Course::decode(input)?,
            next: input.u64()?,
            until: input.u64()?,
            link: read_fixed(input, &0, |input| input.u32(), "poison link flag")?,
        })
    }
}

/// A shield up: its row (`spells.2da`, the item's: how it looks and
/// sounds), the damage types it takes (`DAMAGE_TYPE_*` bits), the points it
/// has left, and until when.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Shielded {
    pub row: u16,
    pub types: u16,
    pub left: u16,
    pub until: u64,
}

/// A stim running: its row, and until when.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Stimmed {
    pub row: u16,
    pub until: u64,
}

/// What a character has used of what it carries and what still runs on
/// it: how many of each kind it used that its holder's items do not yet
/// show (the authority takes each use from them in the step it is made,
/// `world::item_uses`), the last use, a throw's charge, a use under way,
/// a shield and a stim, what poisons and grenades left on it (a poison,
/// drained abilities), and a use Use Item asked for. Carried in snapshots.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Pack {
    pub used: [u8; KINDS],
    /// The last item used: its `spells.2da` row and when (sim ms; 0 none).
    pub last: (u16, u64),
    /// When the grenade button began a throw's charge (sim ms).
    pub charging_from: Option<u64>,
    /// A use under way: its row, and when it lands (sim ms).
    pub using: Option<(u16, u64)>,
    pub shield: Option<Shielded>,
    pub stim: Option<Stimmed>,
    pub poison: Option<Poisoned>,
    /// Ability points drained (a poison's doses, a sonic grenade's DEX; in
    /// `Ability`'s order), until when (sim ms).
    pub drained: ([u8; 6], u64),
    /// The item buttons held on the last step (an item is used on a press).
    pub held: u16,
    /// A battle stim's vitality left (hundredths of an HP): taken before
    /// hit points, gone when the stim ends (KOTOR's temporary hit points).
    pub vitality: i32,
    /// A use chosen by its `spells.2da` row: an item a button takes, used
    /// on itself through Use Item or an item talent, begun on the next
    /// press step as its action's (`press`, `ordered`), its item leaving
    /// the goods as the use lands; or a grenade an item talent chose,
    /// thrown with the next throw (`grenades::pull`), leaving the goods
    /// then.
    pub queued: Option<u16>,
    /// The use under way is its action's (Use Item's, an item talent's,
    /// begun from `queued`): its item leaves the goods as it lands, and a
    /// script's `ClearAllActions` before then breaks it off, nothing spent,
    /// as KOTOR's action queue does (the AI's `ClearAllActions` before a
    /// new talent).
    pub ordered: bool,
}

impl Pack {
    /// A use under way: KOTOR's use is an action, so a weapon change or a
    /// power with a hand (`motion::weapon::lower_weapon`, the powers' free
    /// hand) waits for it rather than break it off.
    pub fn busy(&self) -> bool {
        self.using.is_some()
    }

    /// Ability points drained at `now`.
    pub fn drained(&self, now: u64) -> [u8; 6] {
        if self.drained.1 > now {
            self.drained.0
        } else {
            [0; 6]
        }
    }

    /// Whether a poison runs on it or drained points stay (what an antidote
    /// cures).
    pub fn afflicted(&self, now: u64) -> bool {
        self.poison.is_some() || self.drained(now).iter().any(|points| *points > 0)
    }

    /// The first kind of `items` it has left that `fits`, and which it is.
    pub fn next(
        &self,
        items: &[Option<Carried>; KINDS],
        fits: impl Fn(Use) -> bool,
    ) -> Option<(usize, Carried)> {
        items.iter().enumerate().find_map(|(at, carried)| {
            carried
                .filter(|carried| self.used[at] < carried.count)
                .filter(|carried| use_of(carried.row).is_some_and(&fits))
                .map(|carried| (at, carried))
        })
    }

    /// The kind of `items` of `row`, if it has one left, and which it is.
    pub fn left_of(&self, items: &[Option<Carried>; KINDS], row: u16) -> Option<(usize, Carried)> {
        items.iter().enumerate().find_map(|(at, carried)| {
            carried
                .filter(|carried| carried.row == row && self.used[at] < carried.count)
                .map(|carried| (at, carried))
        })
    }

    /// One of kind `at` (of `row`) used at `now`.
    pub fn spend(&mut self, at: usize, row: u16, now: u64) {
        self.used[at] = self.used[at].saturating_add(1);
        self.last = (row, now);
    }

    /// The poison doses due by `now` (whole hit points each, and who
    /// poisoned it), the points each drains kept until its course says, the
    /// schedule moved on; the poison ends with its last dose.
    pub fn doses(&mut self, now: u64) -> Vec<(u8, SimId)> {
        let mut due = Vec::new();
        while let Some(poison) = self.poison.as_mut().filter(|poison| poison.next <= now) {
            let (course, by, at) = (poison.course, poison.by, poison.next);
            poison.next += u64::from(course.every_ms.max(1));
            if poison.next > poison.until {
                self.poison = None;
            }
            if course.points > 0 {
                due.push((course.points, by));
            }
            if course.drains.iter().any(|points| *points > 0) {
                let mut drained = self.drained(at);
                for (lost, more) in drained.iter_mut().zip(course.drains) {
                    *lost = lost.saturating_add(more);
                }
                self.drained = (drained, at + u64::from(course.drain_ms));
            }
        }
        due
    }

    /// Updates which item buttons are held without using any (a fighter
    /// that cannot act this step), so a press during it is a press after.
    pub fn hold(&mut self, held: u16) {
        use crate::input::buttons;
        self.held = held & (buttons::MEDPAC | buttons::SHIELD | buttons::STIM);
    }

    /// What a battle stim's vitality takes of `damage` hundredths: the part
    /// it takes, spent from it.
    pub fn soak(&mut self, damage: i32) -> i32 {
        let taken = damage.clamp(0, self.vitality);
        self.vitality -= taken;
        taken
    }

    /// The stim running, if it is one of `row`'s.
    pub fn stim(&self) -> Option<Stim> {
        match use_of(self.stim?.row)? {
            Use::Stim(stim) => Some(stim),
            _ => None,
        }
    }

    pub(crate) fn encode(&self, out: &mut Writer) {
        out.bytes(&self.used);
        out.u16(self.last.0);
        out.u64(self.last.1);
        write_fixed(out, self.charging_from.as_ref(), &0, |from, out| {
            out.u64(*from)
        });
        write_fixed(out, self.using.as_ref(), &(0, 0), |(row, until), out| {
            out.u16(*row);
            out.u64(*until);
        });
        let blank_shield = Shielded {
            row: 0,
            types: 0,
            left: 0,
            until: 0,
        };
        write_fixed(out, self.shield.as_ref(), &blank_shield, |shield, out| {
            out.u16(shield.row);
            out.u16(shield.types);
            out.u16(shield.left);
            out.u64(shield.until);
        });
        let blank_stim = Stimmed { row: 0, until: 0 };
        write_fixed(out, self.stim.as_ref(), &blank_stim, |stim, out| {
            out.u16(stim.row);
            out.u64(stim.until);
        });
        write_fixed(
            out,
            self.poison.as_ref(),
            &Poisoned::BLANK,
            Poisoned::encode,
        );
        out.bytes(&self.drained.0);
        out.u64(self.drained.1);
        out.u16(self.held);
        out.u32(self.vitality as u32);
        write_fixed(out, self.queued.as_ref(), &0, |row, out| out.u16(*row));
        out.u8(u8::from(self.ordered));
    }

    pub(crate) fn decode(input: &mut Reader) -> Result<Pack, DecodeError> {
        Ok(Pack {
            used: input.array()?,
            last: (input.u16()?, input.u64()?),
            charging_from: read_fixed(input, &0, |input| input.u64(), "throw flag")?,
            using: read_fixed(
                input,
                &(0, 0),
                |input| Ok((input.u16()?, input.u64()?)),
                "use flag",
            )?,
            shield: read_fixed(
                input,
                &Shielded {
                    row: 0,
                    types: 0,
                    left: 0,
                    until: 0,
                },
                |input| {
                    Ok(Shielded {
                        row: input.u16()?,
                        types: input.u16()?,
                        left: input.u16()?,
                        until: input.u64()?,
                    })
                },
                "shield flag",
            )?,
            stim: read_fixed(
                input,
                &Stimmed { row: 0, until: 0 },
                |input| {
                    Ok(Stimmed {
                        row: input.u16()?,
                        until: input.u64()?,
                    })
                },
                "stim flag",
            )?,
            poison: read_fixed(input, &Poisoned::BLANK, Poisoned::decode, "poison flag")?,
            drained: (input.array()?, input.u64()?),
            held: input.u16()?,
            vitality: input.u32()? as i32,
            queued: read_fixed(input, &0, |input| input.u16(), "queued flag")?,
            ordered: if input.since(crate::since::snapshot::CLAUDE_ITEMS_LOAD) {
                input.u8()? != 0
            } else {
                false
            },
        })
    }
}

/// `sheet` as what runs on its fighter at `now` changes it: abilities a
/// poison or a grenade drained, an adrenal's ability, a battle stim's attack bonus (its damage
/// rides on the weapon, `stim_damage`).
pub fn affected(mut sheet: Sheet, pack: &Pack, now: u64) -> Sheet {
    for (score, lost) in sheet.abilities.iter_mut().zip(pack.drained(now)) {
        *score = score.saturating_sub(lost).max(1);
    }
    match pack.stim() {
        Some(Stim::Ability(ability, amount)) => {
            if let Some(score) = sheet.abilities.get_mut(usize::from(ability)) {
                *score = score.saturating_add(amount);
            }
        }
        Some(Stim::Battle { bonus, .. }) => sheet.attack = sheet.attack.saturating_add(bonus as i8),
        None => {}
    }
    sheet
}

/// The damage a battle stim adds to every weapon hit.
pub fn stim_damage(pack: &Pack) -> i8 {
    match pack.stim() {
        Some(Stim::Battle { bonus, .. }) => bonus as i8,
        _ => 0,
    }
}

/// What an item button's press did, or a use landing (`press`).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Pressed {
    /// A squad heal's points to share out (the native rule).
    pub squad: Option<u8>,
    /// A use began: the weapon waits for it.
    pub used: bool,
    /// The `spells.2da` row whose impact script does what the item does,
    /// cast on its user now (`scripted` rows).
    pub cast: Option<u16>,
    /// The row of the item used: one of it leaves the goods now, as a
    /// button's use begins or an action's lands (`Pack::ordered`).
    pub spent: Option<u16>,
}

/// How long the use of the item of `row` takes a user with `sheet` (ms): a
/// heal less with each rank of its skill (`heal_ms`), anything else its
/// row's conjure and cast time (`USE_MS`).
fn use_ms(sheet: &Sheet, row: u16) -> u64 {
    match use_of(row) {
        Some(Use::Heal { .. }) => {
            let skill = if sheet.race == races::DROID {
                skills::REPAIR
            } else {
                skills::TREAT_INJURY
            };
            heal_ms(sheet.skill(skill).clamp(0, 255) as u8)
        }
        _ => USE_MS,
    }
}

/// One step of the item buttons (`held` this step; an item is used on a
/// press) for a fighter with `sheet`. A use runs as KOTOR's: the item
/// leaves the goods as it begins, its user plays its clip (`Use::clip`)
/// for the use's time (`use_ms`), and what the item does lands at the end
/// (`land`). Damage does not break it off; a stun (a choke), a knockdown
/// or a fall does (`break_off`, in the firing step, and `effects::strip`),
/// the item gone with it; a weapon change or a power waits (`busy`). One use
/// at a time: a press meanwhile does nothing. A use Use Item or an item
/// talent asked for (`Pack::queued`) is begun first, once none runs: its
/// item leaves the goods as it lands, and a script's `ClearAllActions`
/// breaks it off before then (`Pack::ordered`).
pub fn press(
    fighter: &mut Fighter,
    sheet: &Sheet,
    held: u16,
    now: u64,
    authority: bool,
    scripted: impl Fn(u16) -> bool,
) -> Pressed {
    use crate::input::buttons;
    let droid = sheet.race == races::DROID;
    let alive = fighter.alive();
    let items = buttons::MEDPAC | buttons::SHIELD | buttons::STIM;
    let pressed = held & items & !fighter.pack.held;
    fighter.pack.held = held & items;
    if let Some((row, until)) = fighter.pack.using {
        if !alive {
            break_off(fighter, now);
        } else if until <= now {
            fighter.pack.using = None;
            let ordered = std::mem::take(&mut fighter.pack.ordered);
            let landed = land(fighter, sheet, row, now, authority, scripted(row));
            return Pressed {
                spent: ordered.then_some(row),
                ..landed
            };
        }
        return Pressed::default();
    }
    let pack = &mut fighter.pack;
    if let Some(row) = pack
        .queued
        .filter(|row| alive && use_of(*row).is_some_and(|u| u != Use::Grenade))
    {
        pack.queued = None;
        fighter.pack.ordered = true;
        return Pressed {
            spent: None,
            ..begin(fighter, sheet, row, now)
        };
    }
    let medpac = || {
        let afflicted = pack.afflicted(now);
        pack.next(&sheet.items, |u| u == Use::Antidote)
            .filter(|_| afflicted)
            .or_else(|| {
                pack.next(
                    &sheet.items,
                    |u| matches!(u, Use::Heal { droid: kit, .. } if kit == droid),
                )
            })
            .or_else(|| pack.next(&sheet.items, |u| matches!(u, Use::Squad { .. })))
    };
    let found = (pressed & buttons::MEDPAC != 0)
        .then(medpac)
        .flatten()
        .or_else(|| {
            (pressed & buttons::SHIELD != 0 && pack.shield.is_none())
                .then(|| pack.next(&sheet.items, |u| matches!(u, Use::Shield { .. })))
                .flatten()
        })
        .or_else(|| {
            (pressed & buttons::STIM != 0 && pack.stim.is_none())
                .then(|| pack.next(&sheet.items, |u| matches!(u, Use::Stim(_))))
                .flatten()
        });
    let Some((at, carried)) = found.filter(|_| alive) else {
        return Pressed::default();
    };
    pack.spend(at, carried.row, now);
    begin(fighter, sheet, carried.row, now)
}

/// The use of the item of `row` begun by `fighter` (with `sheet`): its
/// clip played for its time, landing at its end. A grenade is thrown, not
/// used so: nothing.
fn begin(fighter: &mut Fighter, sheet: &Sheet, row: u16, now: u64) -> Pressed {
    let Some(kind) = use_of(row).filter(|u| *u != Use::Grenade) else {
        return Pressed::default();
    };
    let ms = use_ms(sheet, row);
    fighter.pack.using = Some((row, now + ms));
    fighter.pack.last = (row, now);
    fighter.acting = Some(Acting {
        animation: kind.clip(),
        since: now,
        ms: ms as u32,
    });
    Pressed {
        used: true,
        spent: Some(row),
        ..Pressed::default()
    }
}

/// The use under way on `fighter` broken off at `now`: nothing done (a
/// button's item already gone, an action's kept), its clip ended.
pub fn break_off(fighter: &mut Fighter, now: u64) {
    fighter.pack.ordered = false;
    if fighter.pack.using.take().is_none() {
        return;
    }
    if fighter.acting.is_some_and(|acting| {
        acting.playing(now) && [clips::INJECT, clips::ACTIVATE].contains(&acting.animation)
    }) {
        fighter.acting = None;
    }
}

/// The use of the item of `row`, run its time on `fighter` (with `sheet`),
/// lands at `now`. An item whose row is `scripted` (its `spells.2da`
/// impact script, `k_sup_healing`, `k_sup_bands`, `k_sup_comshots`) is
/// cast, its script doing what it does; the rest do as their descriptions
/// say (on the authority where they heal), a shield or a stim in place of
/// one up.
fn land(
    fighter: &mut Fighter,
    sheet: &Sheet,
    row: u16,
    now: u64,
    authority: bool,
    scripted: bool,
) -> Pressed {
    match use_of(row) {
        None | Some(Use::Grenade) => Pressed::default(),
        Some(_) if scripted => Pressed {
            cast: Some(row),
            ..Pressed::default()
        },
        Some(Use::Heal {
            points,
            per_rank,
            droid,
        }) => {
            if authority {
                let max_hp = i32::from(sheet.max_hp) * 100;
                let healed = healing(sheet, points, per_rank, droid) * 100;
                fighter.hp = (fighter.hp + healed).min(max_hp).max(fighter.hp);
            }
            Pressed::default()
        }
        Some(Use::Antidote) => {
            effects::remove(fighter, Some(effects::POISON_ID), now);
            Pressed::default()
        }
        Some(Use::Squad { points }) => Pressed {
            squad: authority.then_some(points),
            ..Pressed::default()
        },
        Some(Use::Shield { points, types }) => {
            fighter.pack.shield = Some(Shielded {
                row,
                types,
                left: points,
                until: now + SHIELD_MS,
            });
            Pressed::default()
        }
        Some(Use::Stim(stim)) => {
            fighter.pack.stim = Some(Stimmed {
                row,
                until: now + STIM_MS,
            });
            fighter.pack.vitality = match stim {
                Stim::Battle { vitality, .. } => i32::from(vitality) * 100,
                Stim::Ability(..) => 0,
            };
            Pressed::default()
        }
    }
}

/// What runs out at `now` on a fighter: a shield and a stim end (a battle
/// stim's vitality left with it).
pub fn settle(fighter: &mut Fighter, now: u64) {
    let pack = &mut fighter.pack;
    if pack
        .shield
        .is_some_and(|shield| shield.until <= now || shield.left == 0)
    {
        pack.shield = None;
    }
    if pack.stim.is_some_and(|stim| stim.until <= now) {
        pack.stim = None;
        pack.vitality = 0;
    }
}

/// What a shield up on `pack` takes of `damage` hundredths of damage type
/// `row`: the part it absorbs, its points spent (whole points rounded up).
pub fn absorb(pack: &mut Pack, row: Option<usize>, damage: i32) -> i32 {
    let Some(shield) = pack.shield.as_mut() else {
        return 0;
    };
    let covers = row.is_some_and(|row| row < DAMAGE_TYPES && shield.types & (1 << row) != 0);
    if !covers || damage <= 0 {
        return 0;
    }
    let absorbed = damage.min(i32::from(shield.left) * 100);
    shield.left = shield.left.saturating_sub(((absorbed + 99) / 100) as u16);
    absorbed
}
