//! Levels (I19, `docs/WORLD-STATE.md`): the classes as levels grow them
//! (`classes.2da` and the tables it names), the XP each level asks
//! (`exptable.2da`), and one level taken: what it grants and what it
//! offers to pick (feats, powers, skill ranks, an ability every
//! `levels::ABILITY_EVERY` levels), the picks checked, and the sheet it
//! makes.
//!
//! A level adds the class's hit die, the CON modifier and the Toughness
//! feats' HP (`combat::passive`) to the most HP and its force die to the
//! most FP (what the companions' templates hold: Carth, Mission, T3-M4,
//! Zaalbar, Bastila, Jolee), and moves the base attack bonus, saves and
//! class defence to the class's next row; a feat learnt gives what
//! learning it gives once (`passive::learn`).
//!
//! TSL grants powers with levels besides (`granted_powers`, its level-up's
//! `AddGrantedPowers`): a Jedi base class's lightsaber and Force Forms at
//! Jedi levels from `tsl_grants::FIRST_FORM_LEVEL`, and each prestige
//! class's own powers at its levels.
//!
//! The feat screen lists a level's feats as KOTOR's does, chains of up to
//! three (`Classes::chart`): a feat needing no other, the feat needing
//! only it, and the one needing it and that one (`feat.2da`'s
//! `prereqfeat1`, `prereqfeat2`).

use std::collections::BTreeMap;

use crate::combat::Sheet;
use crate::combat::passive;
use crate::combat::sheet::{Ability, Rows};
use crate::wire::{DecodeError, Reader, Writer};
use rules::progression::levels;
use rules::progression::tsl_grants;

/// The skills (`skills.2da` rows) a sheet ranks.
pub const SKILLS: usize = 8;

/// A class (`classes.2da` row) as levels grow it.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Class {
    pub hit_die: u8,
    pub force_die: u8,
    /// Skill points a level, before intelligence (with the INT modifier as
    /// `Classes::skill_points` counts them, as the level-up screen does).
    pub skill_points: u8,
    /// `primaryabil`: what the game's choice raises at an ability's level
    /// (`levels::ABILITY_EVERY`).
    pub primary: u8,
    /// By class level from 1: base attack bonus, saves (fortitude, reflex,
    /// will), class defence.
    pub bab: Vec<u8>,
    pub saves: Vec<[i8; 3]>,
    pub defence: Vec<i8>,
    /// Feats and powers picked at each class level from 1 (`featgain`,
    /// `classpowergain`).
    pub feat_picks: Vec<u8>,
    pub power_picks: Vec<u8>,
    /// Feats granted at a class level: (level, `feat.2da` row).
    pub granted: Vec<(u8, u16)>,
    /// Feats character creation grants besides (TSL's `<class>_pc_granted`:
    /// the Exile's old soldier's feats), by `feat.2da` row.
    pub pc_granted: Vec<u16>,
    /// Feats it may pick, recommended first.
    pub feats: Vec<u16>,
    /// Powers it may pick: (the class level it may from, `spells.2da`
    /// row), recommended first.
    pub powers: Vec<(u8, u16)>,
    pub class_skills: [bool; SKILLS],
    /// Skills (rows), recommended first.
    pub skill_order: Vec<u8>,
}

/// What a feat asks of who picks it (`feat.2da`): feats known, a total
/// level, a base attack bonus, the least score of each ability (in
/// `Ability` order; 0 for none).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Needs {
    pub feats: [Option<u16>; 2],
    pub level: u8,
    pub bab: u8,
    pub abilities: [u8; 6],
}

/// Classes (by `classes.2da` row), what levels cost, what feats need and
/// which power each power needs (`spells.2da` `prerequisites`).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Classes {
    pub classes: Vec<Class>,
    /// The XP to be level `n + 1` at `n` (`exptable.2da`).
    pub levels: Vec<i32>,
    pub needs: BTreeMap<u16, Needs>,
    pub prerequisites: BTreeMap<u16, u16>,
    /// A level's skill points are the class's and the INT modifier whole
    /// (TSL's level-up stats); else their sum over
    /// `levels::K1_POINTS_DIVISOR` (K1's, whose tables give more points).
    pub whole_skill_points: bool,
    /// Levels grant TSL's powers (`granted_powers`).
    pub granted_powers: bool,
}

/// The Jedi classes (`classes.2da` rows; TSL's `CSWClass::IsJedi`): the
/// three base classes and the six prestige classes.
fn jedi(class: u8) -> bool {
    matches!(class, 3..=5 | 11..=16)
}

/// The Forms each Jedi base class (Guardian, Consular, Sentinel) is granted
/// at Jedi levels from `tsl_grants::FIRST_FORM_LEVEL`, one a level
/// (`spells.2da` rows; TSL's executable's tables).
const BASE_FORMS: [[u16; 4]; 3] = [
    [258, 259, 260, 265],
    [258, 265, 259, 260],
    [258, 260, 265, 259],
];

/// What each prestige class (Jedi Master, Watchman, Marauder, Sith Lord,
/// Assassin) is granted at its levels (`spells.2da` rows; TSL's
/// executable's): (class level, power).
const PRESTIGE_POWERS: [(u8, &[(u8, u16)]); 5] = [
    (12, &[(1, 167), (5, 168), (9, 169), (13, 170), (17, 171)]),
    (13, &[(1, 156), (7, 157), (13, 158)]),
    (14, &[(1, 164), (5, 165), (9, 166)]),
    (15, &[(1, 144), (5, 145), (9, 146), (13, 147), (17, 148)]),
    (16, &[(1, 156), (7, 157), (13, 158)]),
];

/// The powers TSL grants a character whose career, the level just taken
/// counted, is `career` (its level-up's `AddGrantedPowers`): for each class
/// it has, a Jedi base class's Form at total Jedi levels from
/// `tsl_grants::FIRST_FORM_LEVEL` (`BASE_FORMS`), a prestige class's powers at the level its last class
/// reached (`PRESTIGE_POWERS`).
pub fn granted_powers(career: &Career) -> Vec<u16> {
    let jedi_levels: u32 = career
        .iter()
        .filter(|(class, _)| jedi(*class))
        .map(|(_, levels)| u32::from(*levels))
        .sum();
    let last = career.last().map_or(0, |(_, levels)| *levels);
    career
        .iter()
        .filter_map(|&(class, _)| match class {
            3..=5 => jedi_levels
                .checked_sub(tsl_grants::FIRST_FORM_LEVEL)
                .and_then(|at| BASE_FORMS[usize::from(class - 3)].get(at as usize))
                .copied(),
            _ => PRESTIGE_POWERS
                .iter()
                .find(|(prestige, _)| *prestige == class)
                .and_then(|(_, powers)| powers.iter().find(|(at, _)| *at == last))
                .map(|(_, power)| *power),
        })
        .collect()
}

/// A character's classes (`classes.2da` rows) and its levels in each, in
/// the order taken.
pub type Career = Vec<(u8, u8)>;

/// What a level taken picks: its class, the ability raised (every
/// `levels::ABILITY_EVERY` levels), ranks added by skill, feats and powers.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Choices {
    pub class: u8,
    pub ability: Option<u8>,
    pub skills: [u8; SKILLS],
    pub feats: Vec<u16>,
    pub powers: Vec<u16>,
}

impl Choices {
    /// In a save, or in the message that asks for them.
    pub(crate) fn encode(&self, out: &mut Writer) {
        out.u8(self.class);
        out.u8(self.ability.unwrap_or(u8::MAX));
        out.bytes(&self.skills);
        for rows in [&self.feats, &self.powers] {
            out.u32(rows.len() as u32);
            for row in rows {
                out.u16(*row);
            }
        }
    }

    pub(crate) fn decode(input: &mut Reader) -> Result<Choices, DecodeError> {
        let class = input.u8()?;
        let ability = Some(input.u8()?).filter(|ability| *ability != u8::MAX);
        let skills = input.array()?;
        let mut rows = || {
            (0..input.u32()?)
                .map(|_| input.u16())
                .collect::<Result<Vec<_>, _>>()
        };
        let (feats, powers) = (rows()?, rows()?);
        Ok(Choices {
            class,
            ability,
            skills,
            feats,
            powers,
        })
    }
}

/// A build's picks for its levels (a playtest's written plan): skills in
/// the order their ranks are bought, feats and powers in the order they
/// are picked, each before the class's own recommended order (the game's
/// picks, `Classes::auto`); the ability raised at each level that raises
/// one, in turn, past its end the class's primary.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Plan {
    pub skills: Vec<u8>,
    pub feats: Vec<u16>,
    pub powers: Vec<u16>,
    pub abilities: Vec<u8>,
    /// The points the plan's skills cannot take are kept for a later level
    /// (both games' level-up screens leave them unspent) rather than spent
    /// in the class's order.
    pub bank: bool,
}

/// What a level of a class offers a character: the class level and total
/// level it reaches, feats and powers to pick, and whether an ability
/// rises.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Offer {
    pub class_level: u8,
    pub level: u8,
    pub feats: u8,
    pub powers: u8,
    pub ability: bool,
}

/// What a level of a class may pick: the feats and the powers it may
/// (recommended first) and how many of each.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Picks {
    pub feats: Vec<u16>,
    pub feat_count: u8,
    pub powers: Vec<u16>,
    pub power_count: u8,
    /// The feat chart (`Classes::chart`), by `feat.2da` row.
    pub chart: Vec<[Option<u16>; 3]>,
    /// The chart's feats the character has, with those the level grants.
    pub known: Vec<u16>,
}

/// Row `level` (from 1) of a by-level table, its last past its end; 0
/// before the first.
fn at<T: Copy + Default>(table: &[T], level: u8) -> T {
    usize::from(level)
        .checked_sub(1)
        .and_then(|at| table.get(at.min(table.len().saturating_sub(1))).copied())
        .unwrap_or_else(T::default)
}

/// What a by-level table gains from row `level - 1` to row `level`.
fn rise<T: Copy + Default + Into<i16>>(table: &[T], level: u8) -> i16 {
    at(table, level).into() - at(table, level - 1).into()
}

impl Classes {
    /// Whether a character of `level` may take another with `xp`.
    pub fn may_level(&self, level: u8, xp: i32) -> bool {
        self.levels
            .get(usize::from(level))
            .is_some_and(|need| xp >= *need)
    }

    /// The XP the level after `level` asks, if there is one.
    pub fn next(&self, level: u8) -> Option<i32> {
        self.levels.get(usize::from(level)).copied()
    }

    /// The level a character of `level` with `xp` reaches: its own, and
    /// each after it the XP is enough for (TSL's
    /// `CSWSCreatureStats::GetPotentialLevel`).
    pub fn potential_level(&self, level: u8, xp: i32) -> u8 {
        let mut reached = level;
        while self.may_level(reached, xp) && reached < u8::MAX {
            reached += 1;
        }
        reached
    }

    /// What a level of `class` offers a character with `sheet` and
    /// `career` (a class it has; any, if it has none yet).
    pub fn offer(&self, sheet: &Sheet, career: &Career, class: u8) -> Option<Offer> {
        let of = self.classes.get(usize::from(class))?;
        let known = career.iter().find(|(c, _)| *c == class).map(|(_, l)| *l);
        if known.is_none() && !career.is_empty() {
            return None;
        }
        let class_level = known.unwrap_or(0).checked_add(1)?;
        let level = sheet.level.checked_add(1)?;
        Some(Offer {
            class_level,
            level,
            feats: at(&of.feat_picks, class_level),
            powers: at(&of.power_picks, class_level),
            ability: level % levels::ABILITY_EVERY == 0,
        })
    }

    /// The skill points a level of `class` gives a character with `sheet`
    /// raising `ability`: its base and the INT modifier (the raised one),
    /// divided by `levels::K1_POINTS_DIVISOR` unless `whole_skill_points`, at
    /// least `levels::LEAST_SKILL_POINTS`; `levels::FIRST_LEVEL_TIMES` that
    /// for a character's first level (character creation's, as reone's
    /// screen and TSL's level-up stats count it).
    fn skill_points(&self, class: &Class, sheet: &Sheet, ability: Option<u8>) -> u8 {
        let raised = u8::from(ability == Some(Ability::Intelligence as u8));
        let score = sheet.abilities[Ability::Intelligence as usize].saturating_add(raised);
        let intelligence = (i32::from(score) - 10).div_euclid(2);
        let first = if sheet.level == 0 {
            levels::FIRST_LEVEL_TIMES
        } else {
            1
        };
        let points = i32::from(class.skill_points) + intelligence;
        let points = if self.whole_skill_points {
            points
        } else {
            points / levels::K1_POINTS_DIVISOR
        };
        (points.max(levels::LEAST_SKILL_POINTS) * first) as u8
    }

    /// The most ranks a skill may have at `level`: `levels::RANK_CAP_OVER`
    /// more than the level for a class skill, that over
    /// `levels::CROSS_CLASS_CAP_DIVISOR` for another.
    fn most(class: &Class, skill: usize, level: u8) -> u8 {
        let most = level.saturating_add(levels::RANK_CAP_OVER);
        if class.class_skills[skill] {
            most
        } else {
            most / levels::CROSS_CLASS_CAP_DIVISOR
        }
    }

    /// What a rank of `skill` costs `class` in skill points.
    fn rank_cost(class: &Class, skill: usize) -> u8 {
        let [own, other] = levels::SKILL_COST;
        if class.class_skills[skill] {
            own
        } else {
            other
        }
    }

    /// Whether `feat` may be picked by `grown` (the sheet the level makes,
    /// its ability raised) knowing `known`.
    fn may_feat(&self, class: &Class, feat: u16, known: &[u16], grown: &Sheet) -> bool {
        let needs = self
            .needs
            .get(&feat)
            .copied()
            .unwrap_or_else(Needs::default);
        class.feats.contains(&feat)
            && !known.contains(&feat)
            && needs
                .feats
                .iter()
                .flatten()
                .all(|need| known.contains(need))
            && needs.level <= grown.level
            && needs.bab <= grown.bab
            && needs
                .abilities
                .iter()
                .zip(grown.abilities)
                .all(|(least, score)| score >= *least)
    }

    fn may_power(&self, class: &Class, power: u16, known: &[u16], class_level: u8) -> bool {
        class
            .powers
            .iter()
            .any(|(from, row)| *row == power && *from <= class_level)
            && !known.contains(&power)
            && self
                .prerequisites
                .get(&power)
                .is_none_or(|need| known.contains(need))
    }

    /// What a level of `class` lets a character with `sheet` and `career`
    /// pick (each feat and power as `take` checks it, before any other
    /// pick of the level).
    pub fn picks(&self, sheet: &Sheet, career: &Career, class: u8) -> Option<Picks> {
        let offer = self.offer(sheet, career, class)?;
        let of = &self.classes[usize::from(class)];
        let grown = Self::grown(sheet, of, offer);
        let feats = known_feats(sheet, of, offer.class_level);
        let powers = known_powers(sheet);
        let chart = self.chart(of, &feats);
        let known = chart
            .iter()
            .flatten()
            .flatten()
            .copied()
            .filter(|feat| feats.contains(feat))
            .collect();
        Some(Picks {
            feats: of
                .feats
                .iter()
                .copied()
                .filter(|&feat| self.may_feat(of, feat, &feats, &grown))
                .collect(),
            feat_count: offer.feats,
            powers: of
                .powers
                .iter()
                .map(|&(_, power)| power)
                .filter(|&power| self.may_power(of, power, &powers, offer.class_level))
                .collect(),
            power_count: offer.powers,
            chart,
            known,
        })
    }

    /// KOTOR's feat chart for `class` and a character knowing `known`, a
    /// row a chain, in `feat.2da` order of its first feat: each feat
    /// needing no other (`prereqfeat1` blank) the class may pick, or one it
    /// grants that is known; after it the last feat needing only it, then
    /// the first needing it and another (the game's own choice where a
    /// table has more), each the class may pick, grants or the character
    /// knows.
    pub fn chart(&self, class: &Class, known: &[u16]) -> Vec<[Option<u16>; 3]> {
        let granted = |feat: u16| class.granted.iter().any(|(_, of)| *of == feat);
        let open =
            |feat: u16| class.feats.contains(&feat) || granted(feat) || known.contains(&feat);
        let mut feats: Vec<u16> = class
            .feats
            .iter()
            .chain(class.granted.iter().map(|(_, feat)| feat))
            .chain(known)
            .copied()
            .collect();
        feats.sort_unstable();
        feats.dedup();
        let needs = |feat: u16| {
            self.needs
                .get(&feat)
                .map_or([None, None], |needs| needs.feats)
        };
        feats
            .iter()
            .filter(|&&feat| needs(feat)[0].is_none())
            .filter(|&&feat| {
                class.feats.contains(&feat) || (granted(feat) && known.contains(&feat))
            })
            .map(|&first| {
                let after = |two: bool| {
                    feats.iter().copied().filter(move |&feat| {
                        let [one, other] = needs(feat);
                        one == Some(first) && other.is_some() == two && open(feat)
                    })
                };
                [Some(first), after(false).next_back(), after(true).next()]
            })
            .collect()
    }

    /// The game's picks for a level of `class` (KOTOR's automatic level
    /// up) by a character with `banked` points left from its levels before:
    /// the class's primary ability, skill ranks in its recommended order
    /// (each as high as it may go before the next), its recommended feats
    /// and powers that may be picked, in order.
    pub fn auto(&self, sheet: &Sheet, career: &Career, class: u8, banked: u8) -> Option<Choices> {
        self.planned(sheet, career, class, &Plan::default(), banked)
    }

    /// The picks `plan` makes for a level of `class` (`auto`'s, the plan's
    /// orders first): its ability raised, skill ranks bought with the
    /// level's points and the `banked` ones in the plan's order then the
    /// class's (unless the plan banks what is left), feats and powers in
    /// the plan's order then the class's recommended, each that may be
    /// picked.
    pub fn planned(
        &self,
        sheet: &Sheet,
        career: &Career,
        class: u8,
        plan: &Plan,
        banked: u8,
    ) -> Option<Choices> {
        let offer = self.offer(sheet, career, class)?;
        let of = &self.classes[usize::from(class)];
        let raises = usize::from(offer.level / levels::ABILITY_EVERY);
        let ability = offer.ability.then(|| {
            raises
                .checked_sub(1)
                .and_then(|at| plan.abilities.get(at))
                .copied()
                .unwrap_or(of.primary)
        });
        let mut skills = [0u8; SKILLS];
        let mut points = banked.saturating_add(self.skill_points(of, sheet, ability));
        let class_order = if plan.bank {
            &[][..]
        } else {
            &of.skill_order[..]
        };
        for &skill in plan.skills.iter().chain(class_order) {
            let skill = usize::from(skill);
            let cost = Self::rank_cost(of, skill);
            let most = Self::most(of, skill, offer.level);
            while points >= cost && sheet.skills[skill] + skills[skill] < most {
                skills[skill] += 1;
                points -= cost;
            }
        }
        let grown = Self::raised(Self::grown(sheet, of, offer), ability);
        let mut feats = known_feats(sheet, of, offer.class_level);
        let mut picked = Vec::new();
        for &feat in plan.feats.iter().chain(&of.feats) {
            if picked.len() < usize::from(offer.feats) && self.may_feat(of, feat, &feats, &grown) {
                picked.push(feat);
                feats.push(feat);
            }
        }
        let mut powers = known_powers(sheet);
        let mut taught = Vec::new();
        let recommended = of.powers.iter().map(|(_, power)| power);
        for &power in plan.powers.iter().chain(recommended) {
            if taught.len() < usize::from(offer.powers)
                && self.may_power(of, power, &powers, offer.class_level)
            {
                taught.push(power);
                powers.push(power);
            }
        }
        Some(Choices {
            class,
            ability,
            skills,
            feats: picked,
            powers: taught,
        })
    }

    /// `choices` with the feats and powers it leaves unpicked, and the
    /// ability point it leaves unspent, taken from the game's picks (the
    /// level-up screen offers no feats or powers yet).
    pub fn complete(&self, sheet: &Sheet, career: &Career, choices: &Choices) -> Choices {
        let Some(auto) = self.auto(sheet, career, choices.class, 0) else {
            return choices.clone();
        };
        let mut done = choices.clone();
        for (picked, offered) in [
            (&mut done.feats, auto.feats),
            (&mut done.powers, auto.powers),
        ] {
            if picked.is_empty() {
                *picked = offered;
            }
        }
        done.ability = done.ability.or(auto.ability);
        done
    }

    /// `sheet` with `ability` (if any) a point higher.
    fn raised(mut sheet: Sheet, ability: Option<u8>) -> Sheet {
        if let Some(score) =
            ability.and_then(|ability| sheet.abilities.get_mut(usize::from(ability)))
        {
            *score = score.saturating_add(1);
        }
        sheet
    }

    /// The sheet with a level of `class` and nothing picked: HP, FP, base
    /// attack bonus, saves, class defence, the level.
    fn grown(sheet: &Sheet, class: &Class, offer: Offer) -> Sheet {
        let now = offer.class_level;
        let mut grown = *sheet;
        grown.level = offer.level;
        let bab = i16::from(grown.bab) + rise(&class.bab, now);
        grown.bab = bab.clamp(0, 255) as u8;
        for (n, save) in grown.saves.iter_mut().enumerate() {
            let column: Vec<i8> = class.saves.iter().map(|saves| saves[n]).collect();
            *save = (i16::from(*save) + rise(&column, now)).clamp(-128, 127) as i8;
        }
        let defence = i16::from(grown.defence) + rise(&class.defence, now);
        grown.defence = defence.clamp(-128, 127) as i8;
        grown
    }

    /// `choices` taken by a character with `sheet`, `career` and `banked`
    /// points left from its levels before: its new sheet and career and the
    /// points it leaves unspent (the level's added to those banked, TSL's
    /// `CSWCLevelUpStats::AddSkillPointsPerLevel`), or `None` when the picks
    /// are not the level's to make (a class it has not, more picks than
    /// offered, a feat or power it may not have, skill points overspent,
    /// ranks past the most).
    pub fn take(
        &self,
        sheet: &Sheet,
        career: &Career,
        choices: &Choices,
        banked: u8,
    ) -> Option<(Sheet, Career, u8)> {
        let offer = self.offer(sheet, career, choices.class)?;
        let of = &self.classes[usize::from(choices.class)];
        if choices.ability.is_some() != offer.ability
            || choices.ability.is_some_and(|ability| ability > 5)
            || choices.feats.len() > usize::from(offer.feats)
            || choices.powers.len() > usize::from(offer.powers)
        {
            return None;
        }
        let mut spent = 0u32;
        for (skill, &ranks) in choices.skills.iter().enumerate() {
            let cost = u32::from(Self::rank_cost(of, skill));
            spent += u32::from(ranks) * cost;
            // Only ranks added are held to the cap: a skill already past it
            // (a playtest's `skill` goal, standing in for a build) refuses
            // no level.
            if ranks > 0
                && sheet.skills[skill].saturating_add(ranks) > Self::most(of, skill, offer.level)
            {
                return None;
            }
        }
        let points = u32::from(banked) + u32::from(self.skill_points(of, sheet, choices.ability));
        let left = points.checked_sub(spent)?;
        let mut grown = Self::raised(Self::grown(sheet, of, offer), choices.ability);
        let mut feats = known_feats(sheet, of, offer.class_level);
        for &feat in &choices.feats {
            if !self.may_feat(of, feat, &feats, &grown) {
                return None;
            }
            feats.push(feat);
        }
        let mut powers = known_powers(sheet);
        for &power in &choices.powers {
            if !self.may_power(of, power, &powers, offer.class_level) {
                return None;
            }
            powers.push(power);
        }
        for &feat in &feats {
            if !grown.feats.has(feat) {
                grown.feats.insert(feat);
                passive::learn(&mut grown, feat, sheet.level);
            }
        }
        let hp = i32::from(of.hit_die)
            + grown.modifier(Ability::Constitution)
            + passive::hp_per_level(&grown.feats);
        grown.max_hp = grown.max_hp.saturating_add(hp.max(levels::LEAST_HP) as u16);
        grown.max_fp = grown.max_fp.saturating_add(u16::from(of.force_die));
        for power in &choices.powers {
            grown.powers.insert(*power);
        }
        for (rank, added) in grown.skills.iter_mut().zip(choices.skills) {
            *rank = rank.saturating_add(added);
        }
        let mut career = career.clone();
        match career.iter_mut().find(|(c, _)| *c == choices.class) {
            Some((_, levels)) => *levels = offer.class_level,
            None => career.push((choices.class, 1)),
        }
        if self.granted_powers {
            for power in granted_powers(&career) {
                grown.powers.insert(power);
            }
        }
        Some((grown, career, left.min(u32::from(u8::MAX)) as u8))
    }

    pub(crate) fn digest_into(&self, out: &mut Writer) {
        out.u32(self.classes.len() as u32);
        for class in &self.classes {
            out.bytes(&[
                class.hit_die,
                class.force_die,
                class.skill_points,
                class.primary,
            ]);
            out.section(&class.bab);
            out.u32(class.saves.len() as u32);
            for saves in &class.saves {
                for save in saves {
                    out.i8(*save);
                }
            }
            out.u32(class.defence.len() as u32);
            for defence in &class.defence {
                out.i8(*defence);
            }
            out.section(&class.feat_picks);
            out.section(&class.power_picks);
            out.u32(class.granted.len() as u32);
            for (level, feat) in &class.granted {
                out.u8(*level);
                out.u16(*feat);
            }
            out.u32(class.feats.len() as u32);
            for feat in &class.feats {
                out.u16(*feat);
            }
            out.u32(class.powers.len() as u32);
            for (from, power) in &class.powers {
                out.u8(*from);
                out.u16(*power);
            }
            out.bytes(&class.class_skills.map(u8::from));
            out.section(&class.skill_order);
            // Only TSL's tables grant them, so K1's classes keep their digest.
            if !class.pc_granted.is_empty() {
                out.string("pc granted");
                out.u32(class.pc_granted.len() as u32);
                for feat in &class.pc_granted {
                    out.u16(*feat);
                }
            }
        }
        out.u32(self.levels.len() as u32);
        for xp in &self.levels {
            out.u32(*xp as u32);
        }
        out.u32(self.needs.len() as u32);
        for (feat, needs) in &self.needs {
            out.u16(*feat);
            for need in needs.feats {
                out.u16(need.unwrap_or(u16::MAX));
            }
            out.bytes(&[needs.level, needs.bab]);
            out.bytes(&needs.abilities);
        }
        out.u32(self.prerequisites.len() as u32);
        for (power, need) in &self.prerequisites {
            out.u16(*power);
            out.u16(*need);
        }
        // Only TSL's rules list them, so K1's classes keep their digest.
        if self.whole_skill_points {
            out.string("whole skill points");
        }
        if self.granted_powers {
            out.string("granted powers");
        }
    }
}

/// The feats a sheet knows, with those the class grants at `class_level`.
fn known_feats(sheet: &Sheet, class: &Class, class_level: u8) -> Vec<u16> {
    let mut feats: Vec<u16> = (0..Rows::<4>::ROWS)
        .filter(|feat| sheet.feats.has(*feat))
        .collect();
    for &(level, feat) in &class.granted {
        if level == class_level && !feats.contains(&feat) {
            feats.push(feat);
        }
    }
    feats
}

fn known_powers(sheet: &Sheet) -> Vec<u16> {
    (0..Rows::<5>::ROWS)
        .filter(|power| sheet.powers.has(*power))
        .collect()
}
