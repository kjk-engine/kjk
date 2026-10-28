//! An item's properties (`itempropdef.2da`, their cost tables resolved by
//! the session), what those in force make of the item (`make`: its weapon,
//! its guards, what it gives its wearer, who may wear it), and the
//! upgrades an item takes (`Upgrade`: KOTOR's lightsabers and upgradable
//! gear list every crystal's and part's properties with its `upgrade.2da`
//! row, in force only while it is fitted; TSL's upgrade items carry their
//! own, added to their host's while fitted).
//!
//! The executables apply every property on equip but those used when the
//! item is used (Cast Spell, Thieves' Tools, Trap, Computer Spike: the
//! session reads them apart), as effects the item creates. An item gives
//! the best of its bonuses of a kind (to hit; damage of a type; a score, a
//! skill, a save) and all its penalties; its defence (always dodge
//! defence) all adds up. Use limits refuse the equip and never turn
//! properties off (journal 2026-10-05-item-properties).

use crate::combat::gear::{Against, Extra, Gear, OnHit, Roll, Versus, traits, weapon_traits};
use crate::combat::sheet::{DAMAGE_TYPES, EXTRAS, Guards, ON_HITS, Weapon};
use crate::wire::{DecodeError, Reader, Writer};

/// An item property in force, as the sim applies it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Property {
    /// Enhancement (5): to hit and damage of the weapon's own type.
    Enhancement(i8),
    /// To hit: an attack bonus (38), a penalty (8) negative.
    Attack(i8),
    /// Damage (11) of a `DAMAGE_TYPES` row (`OWN_TYPE`: the weapon's own,
    /// a Damage Penalty's, 15, negative).
    Damage {
        row: u8,
        roll: Roll,
    },
    /// The base damage's dice taken away (`DamageNone`, 31).
    NoDice,
    /// The blaster-bolt deflection modifier (55, 56).
    Deflect(i8),
    /// To hit and damage against a race or a side (6, 7, 12, 13, 39, 40).
    Versus(Versus),
    /// A damage type's immunity percent (14; a vulnerability, 18,
    /// negative).
    Immunity {
        row: u8,
        percent: i8,
    },
    /// A damage type's resistance points (17).
    Resist {
        row: u8,
        points: u8,
    },
    /// Damage reduction (16): points off each physical hit, unless the
    /// weapon's enhancement reaches `bypass`.
    Soak {
        points: u8,
        bypass: u8,
    },
    /// An `Ability` raised (0) or lowered (19).
    Ability {
        ability: u8,
        by: i8,
    },
    /// Defence (1-4; 20 negative), against one kind of foe or any.
    Defence {
        by: i8,
        against: Option<Against>,
    },
    /// A skill raised (36) or lowered (21).
    Skill {
        skill: u8,
        by: i8,
    },
    /// A save raised (26, 27) or lowered (33, 34): 0 Fortitude, 1 Reflex,
    /// 2 Will, `ALL_SAVES`.
    Save {
        save: u8,
        by: i8,
    },
    /// A bonus feat (9).
    Feat(u16),
    /// An `IMMUNITY_TYPE_*` (24, Freedom of Movement 50).
    Immune(u8),
    /// Force resistance (25).
    ForceResist(u8),
    /// Keen (28): the weapon's threat range multiplied
    /// (`rules::items::properties::KEEN`).
    Keen,
    /// Massive Criticals (49): damage a critical adds.
    Massive(Roll),
    /// Mighty (30): the most STR adds to a ranged weapon's damage.
    Mighty(u8),
    /// Monster damage (51): a creature weapon's dice instead of its base.
    MonsterDice {
        dice: u8,
        die: u8,
    },
    /// An effect on a hit (32).
    OnHit(OnHit),
    /// Hit points (35) and Force points (54) regained every
    /// `gear::REGENERATE_EVERY_MS`.
    Regenerate(u8),
    RegenerateForce(u8),
    /// Raised by an item worn: `gear::traits` (Light 29, True Seeing 47,
    /// TSL's Dampen Sound 63, Sniper 66 and Rapid Shot Bonus 67).
    Trait(u8),
    /// TSL's Door Cutting (64) and Door Sabering (65): `gear::weapon_traits`.
    WeaponTrait(u8),
    /// TSL's DEX Bonus Max (68): added to the armour's DEX cap.
    DexCap(u8),
    /// Disguise (59): an `appearance.2da` row.
    Disguise(u16),
    /// Special Walk (52): an `iprp_walk.2da` row.
    Walk(u8),
    /// Who may equip it (43-45, 57; TSL's 60-62, 69).
    Limit(Limit),
}

/// `Property::Damage`'s row for the weapon's own damage type.
pub const OWN_TYPE: u8 = u8::MAX;
/// `Property::Save`'s save for all three.
pub const ALL_SAVES: u8 = 3;

/// Who may equip an item (`Use Limitation` properties): of a side
/// (`gear::sides`; TSL's 0 any, 4 and 5 wholly dark and light), a class
/// (`classes.2da` row), a race, TSL's subrace, with a feat, of a gender
/// (TSL; 2 both), the player character or the party member of a tag
/// (TSL's `iprp_pc.2da`), an ability at least so high (TSL). Of several
/// limits of a kind, one met is enough.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Limit {
    Side(u8),
    Class(u8),
    Race(u8),
    Subrace(u8),
    Feat(u16),
    Gender(u8),
    Player,
    Tag(String),
    Ability { ability: u8, at_least: u8 },
}

/// The side limits TSL adds: every side, wholly dark (good-evil 0), wholly
/// light (100).
pub mod side_limits {
    pub const ANY: u8 = 0;
    pub const WHOLLY_DARK: u8 = 4;
    pub const WHOLLY_LIGHT: u8 = 5;
}

/// What an item's properties in force make of it: its weapon (from its
/// base item's), its guards, what it gives its wearer, its use limits.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Made {
    pub weapon: Option<Weapon>,
    pub guards: Guards,
    pub gear: Gear,
    pub limits: Vec<Limit>,
}

/// One item's bonuses and penalties of a kind: the best bonus counts, the
/// penalties add up.
#[derive(Debug, Clone, Copy, Default)]
struct Tally {
    best: i16,
    penalty: i16,
}

impl Tally {
    fn add(&mut self, by: i8) {
        let by = i16::from(by);
        if by > 0 {
            self.best = self.best.max(by);
        } else {
            self.penalty += by;
        }
    }

    fn total(self) -> i8 {
        (self.best + self.penalty).clamp(-128, 127) as i8
    }
}

/// The best of two rolls by their average.
fn best(a: Option<Roll>, b: Roll) -> Option<Roll> {
    Some(
        a.filter(|a| a.twice_average() >= b.twice_average())
            .unwrap_or(b),
    )
}

/// What `properties` (in force) make of an item whose base item's weapon
/// is `base`: to hit, the best bonus of each damage type (the weapon's own
/// type, its lowest, as its bonus dice and flat bonus, others as extras
/// rolled as their types, the strongest first), penalties; deflection adds
/// up; the bonuses against one kind of
/// foe join (another kind's replace them); immunity and vulnerability add
/// up within ±100, of resistances the best counts; scores, skills and saves
/// by the tally, defence adding up, the rest raised or joined.
pub fn make(base: Option<Weapon>, properties: &[Property]) -> Made {
    let mut weapon = base;
    let own_row = base
        .map(|weapon| weapon.damage_types.trailing_zeros())
        .filter(|row| (*row as usize) < DAMAGE_TYPES)
        .map_or(0, |row| row as u8);
    let mut attack = Tally::default();
    let mut damage: [Option<Roll>; DAMAGE_TYPES] = [None; DAMAGE_TYPES];
    let mut damage_penalty = 0i16;
    let mut abilities = [Tally::default(); 6];
    let mut skills = [Tally::default(); 8];
    let mut saves = [Tally::default(); 4];
    let mut made = Made::default();
    let gear = &mut made.gear;
    let guards = &mut made.guards;
    let mut keen = false;
    for property in properties {
        match property.clone() {
            Property::Enhancement(by) => {
                attack.add(by);
                damage[usize::from(own_row)] = best(
                    damage[usize::from(own_row)],
                    Roll {
                        bonus: by,
                        ..Roll::NONE
                    },
                );
                if let Some(weapon) = weapon.as_mut() {
                    weapon.enhancement = weapon.enhancement.max(by.max(0) as u8);
                }
            }
            Property::Attack(by) => attack.add(by),
            Property::Damage { roll, .. } if roll.bonus < 0 && roll.dice == 0 => {
                damage_penalty += i16::from(roll.bonus);
            }
            Property::Damage { row, roll } => {
                let row = if row == OWN_TYPE { own_row } else { row };
                if let Some(slot) = damage.get_mut(usize::from(row)) {
                    *slot = best(*slot, roll);
                }
            }
            Property::NoDice => {
                if let Some(weapon) = weapon.as_mut() {
                    weapon.dice = 0;
                }
            }
            Property::Deflect(by) => {
                if let Some(weapon) = weapon.as_mut() {
                    weapon.deflect = weapon.deflect.saturating_add(by);
                }
            }
            Property::Versus(more) => {
                if let Some(weapon) = weapon.as_mut() {
                    weapon.versus = Some(match weapon.versus {
                        Some(had) if had.against == more.against => Versus {
                            attack: had.attack.max(more.attack),
                            damage: best(Some(had.damage), more.damage).unwrap_or(had.damage),
                            damage_type: if more.damage.twice_average() > had.damage.twice_average()
                            {
                                more.damage_type
                            } else {
                                had.damage_type
                            },
                            ..had
                        },
                        _ => more,
                    });
                }
            }
            Property::Immunity { row, percent } => {
                if let Some(immunity) = guards.immunity.get_mut(usize::from(row)) {
                    *immunity = (i16::from(*immunity) + i16::from(percent)).clamp(-100, 100) as i8;
                }
            }
            Property::Resist { row, points } => {
                if let Some(resist) = guards.resist.get_mut(usize::from(row)) {
                    *resist = (*resist).max(points);
                }
            }
            Property::Soak { points, bypass } => {
                if points > gear.soak {
                    (gear.soak, gear.soak_bypass) = (points, bypass);
                }
            }
            Property::Ability { ability, by } => {
                if let Some(tally) = abilities.get_mut(usize::from(ability)) {
                    tally.add(by);
                }
            }
            Property::Defence { by, against: None } => {
                gear.defence = gear.defence.saturating_add(by);
            }
            Property::Defence {
                by,
                against: Some(against),
            } => gear.add_defence_vs((against, by)),
            Property::Skill { skill, by } => {
                if let Some(tally) = skills.get_mut(usize::from(skill)) {
                    tally.add(by);
                }
            }
            Property::Save { save, by } => {
                if let Some(tally) = saves.get_mut(usize::from(save.min(ALL_SAVES))) {
                    tally.add(by);
                }
            }
            Property::Feat(feat) => gear.feats.insert(feat),
            Property::Immune(immunity) => {
                if immunity < 64 {
                    gear.immune |= 1 << immunity;
                }
            }
            Property::ForceResist(value) => gear.force_resist = gear.force_resist.max(value),
            Property::Keen => keen = true,
            Property::Massive(roll) => {
                if let Some(weapon) = weapon.as_mut() {
                    weapon.massive = best(Some(weapon.massive), roll).unwrap_or(roll);
                }
            }
            Property::Mighty(most) => {
                if let Some(weapon) = weapon.as_mut() {
                    weapon.mighty = Some(weapon.mighty.map_or(most, |had| had.max(most)));
                }
            }
            Property::MonsterDice { dice, die } => {
                if let Some(weapon) = weapon.as_mut() {
                    (weapon.dice, weapon.die) = (dice, die);
                }
            }
            Property::OnHit(on_hit) => {
                if let Some(free) = weapon
                    .as_mut()
                    .and_then(|weapon| weapon.on_hit.iter_mut().find(|slot| slot.is_none()))
                {
                    *free = Some(on_hit);
                }
            }
            Property::Regenerate(points) => gear.regenerate = gear.regenerate.max(points),
            Property::RegenerateForce(points) => {
                gear.regenerate_fp = gear.regenerate_fp.max(points)
            }
            Property::Trait(bit) => gear.traits |= bit,
            Property::WeaponTrait(bit) => {
                if let Some(weapon) = weapon.as_mut() {
                    weapon.traits |= bit;
                }
            }
            Property::DexCap(more) => gear.dex_cap = gear.dex_cap.max(more),
            Property::Disguise(appearance) => {
                gear.disguise = gear.disguise.or(Some(appearance));
            }
            Property::Walk(walk) => {
                if gear.walk == 0 {
                    gear.walk = walk;
                }
                gear.traits |= traits::WALK_ONLY;
            }
            Property::Limit(limit) => made.limits.push(limit),
        }
    }
    gear.abilities = abilities.map(Tally::total);
    gear.skills = skills.map(Tally::total);
    let all = saves[usize::from(ALL_SAVES)].total();
    gear.saves = std::array::from_fn(|save| saves[save].total().saturating_add(all));
    if let Some(weapon) = weapon.as_mut() {
        weapon.attack_bonus = weapon.attack_bonus.saturating_add(attack.total());
        if keen {
            weapon.crit_threat = weapon
                .crit_threat
                .max(1)
                .saturating_mul(rules::items::properties::KEEN);
        }
        let own = damage[usize::from(own_row)].unwrap_or(Roll::NONE);
        weapon.bonus_dice = own.dice;
        weapon.bonus_die = own.die;
        weapon.damage_bonus =
            (i16::from(weapon.damage_bonus) + i16::from(own.bonus) + damage_penalty)
                .clamp(-128, 127) as i8;
        // Other types' bonuses, each rolled as its own type, the strongest
        // first.
        let mut extras: Vec<Extra> = (0..DAMAGE_TYPES as u8)
            .filter(|row| *row != own_row)
            .filter_map(|row| {
                Some(Extra {
                    damage_type: row,
                    roll: damage[usize::from(row)]?,
                })
            })
            .collect();
        extras.sort_by_key(|extra| std::cmp::Reverse(extra.roll.twice_average()));
        for (slot, extra) in weapon.extras.iter_mut().zip(extras) {
            *slot = Some(extra);
        }
    }
    made.weapon = weapon;
    made
}

/// An upgrade an item takes: its `upgrade.2da` row, its kind
/// (`upgradetype`: 0 a lightsaber crystal, 1-3 melee, 4-7 ranged, 8-9
/// armour), the upgrade item's template, and what it adds fitted: its
/// properties, and their `itempropdef.2da` rows as bits.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Upgrade {
    pub row: u8,
    pub kind: u8,
    pub template: String,
    pub properties: Vec<Property>,
    pub rows: u128,
}

/// How many upgrades of `kind` an item holds at once: a lightsaber's power
/// crystals (its colour crystal is its template, `ItemFacts::recolours`),
/// and of every other kind (`rules::items::upgrades`).
pub fn room(kind: u8) -> usize {
    use rules::items::upgrades;
    if kind == 0 {
        upgrades::CRYSTALS
    } else {
        upgrades::OTHERS
    }
}

/// What `Property::Trait`'s bits may be.
const TRAITS: u8 = traits::LIGHT
    | traits::TRUE_SEEING
    | traits::SILENT
    | traits::SNIPER
    | traits::RAPID_SHOT
    | traits::WALK_ONLY;
const WEAPON_TRAITS: u8 = weapon_traits::DOOR_CUTTING | weapon_traits::DOOR_SABERING;

impl Property {
    pub(crate) fn encode(&self, out: &mut Writer) {
        match self {
            Property::Enhancement(by) => {
                out.u8(0);
                out.i8(*by);
            }
            Property::Attack(by) => {
                out.u8(1);
                out.i8(*by);
            }
            Property::Damage { row, roll } => {
                out.bytes(&[2, *row]);
                roll.encode(out);
            }
            Property::NoDice => out.u8(3),
            Property::Deflect(by) => {
                out.u8(4);
                out.i8(*by);
            }
            Property::Versus(versus) => {
                out.u8(5);
                versus.encode(out);
            }
            Property::Immunity { row, percent } => {
                out.bytes(&[6, *row]);
                out.i8(*percent);
            }
            Property::Resist { row, points } => out.bytes(&[7, *row, *points]),
            Property::Soak { points, bypass } => out.bytes(&[8, *points, *bypass]),
            Property::Ability { ability, by } => {
                out.bytes(&[9, *ability]);
                out.i8(*by);
            }
            Property::Defence { by, against } => {
                out.u8(10);
                out.i8(*by);
                out.u8(u8::from(against.is_some()));
                against.unwrap_or(Against::Race(0)).encode(out);
            }
            Property::Skill { skill, by } => {
                out.bytes(&[11, *skill]);
                out.i8(*by);
            }
            Property::Save { save, by } => {
                out.bytes(&[12, *save]);
                out.i8(*by);
            }
            Property::Feat(feat) => {
                out.u8(13);
                out.u16(*feat);
            }
            Property::Immune(immunity) => out.bytes(&[14, *immunity]),
            Property::ForceResist(value) => out.bytes(&[15, *value]),
            Property::Keen => out.u8(16),
            Property::Massive(roll) => {
                out.u8(17);
                roll.encode(out);
            }
            Property::Mighty(most) => out.bytes(&[18, *most]),
            Property::MonsterDice { dice, die } => out.bytes(&[19, *dice, *die]),
            Property::OnHit(on_hit) => {
                out.u8(20);
                on_hit.encode(out);
            }
            Property::Regenerate(points) => out.bytes(&[21, *points]),
            Property::RegenerateForce(points) => out.bytes(&[22, *points]),
            Property::Trait(bit) => out.bytes(&[23, *bit]),
            Property::WeaponTrait(bit) => out.bytes(&[24, *bit]),
            Property::DexCap(more) => out.bytes(&[25, *more]),
            Property::Disguise(appearance) => {
                out.u8(26);
                out.u16(*appearance);
            }
            Property::Walk(walk) => out.bytes(&[27, *walk]),
            Property::Limit(limit) => {
                out.u8(28);
                limit.encode(out);
            }
        }
    }

    pub(crate) fn decode(input: &mut Reader) -> Result<Property, DecodeError> {
        let at = input.at();
        let damage_row = |input: &mut Reader| -> Result<u8, DecodeError> {
            let at = input.at();
            let row = input.u8()?;
            if usize::from(row) >= DAMAGE_TYPES && row != OWN_TYPE {
                return Err(input.invalid("damage type", at));
            }
            Ok(row)
        };
        let kind = input.u8()?;
        Ok(match kind {
            0 => Property::Enhancement(input.i8()?),
            1 => Property::Attack(input.i8()?),
            2 => Property::Damage {
                row: damage_row(input)?,
                roll: Roll::decode(input)?,
            },
            3 => Property::NoDice,
            4 => Property::Deflect(input.i8()?),
            5 => Property::Versus(Versus::decode(input)?),
            6 => Property::Immunity {
                row: damage_row(input)?,
                percent: input.i8()?,
            },
            7 => Property::Resist {
                row: damage_row(input)?,
                points: input.u8()?,
            },
            8 => Property::Soak {
                points: input.u8()?,
                bypass: input.u8()?,
            },
            9 => Property::Ability {
                ability: input.u8()?,
                by: input.i8()?,
            },
            10 => {
                let by = input.i8()?;
                let flag = input.u8()?;
                let against = Against::decode(input)?;
                Property::Defence {
                    by,
                    against: match flag {
                        0 if against == Against::Race(0) => None,
                        1 => Some(against),
                        _ => return Err(input.invalid("defence against", at)),
                    },
                }
            }
            11 => Property::Skill {
                skill: input.u8()?,
                by: input.i8()?,
            },
            12 => Property::Save {
                save: input.u8()?,
                by: input.i8()?,
            },
            13 => Property::Feat(input.u16()?),
            14 => Property::Immune(input.u8()?),
            15 => Property::ForceResist(input.u8()?),
            16 => Property::Keen,
            17 => Property::Massive(Roll::decode(input)?),
            18 => Property::Mighty(input.u8()?),
            19 => Property::MonsterDice {
                dice: input.u8()?,
                die: input.u8()?,
            },
            20 => Property::OnHit(OnHit::decode(input)?),
            21 => Property::Regenerate(input.u8()?),
            22 => Property::RegenerateForce(input.u8()?),
            23 | 24 => {
                let (bit, known) = (input.u8()?, if kind == 23 { TRAITS } else { WEAPON_TRAITS });
                if bit & !known != 0 {
                    return Err(input.invalid("trait", at));
                }
                if kind == 23 {
                    Property::Trait(bit)
                } else {
                    Property::WeaponTrait(bit)
                }
            }
            25 => Property::DexCap(input.u8()?),
            26 => Property::Disguise(input.u16()?),
            27 => Property::Walk(input.u8()?),
            28 => Property::Limit(Limit::decode(input)?),
            _ => return Err(input.invalid("property", at)),
        })
    }
}

impl Limit {
    pub(crate) fn encode(&self, out: &mut Writer) {
        match self {
            Limit::Side(side) => out.bytes(&[0, *side]),
            Limit::Class(class) => out.bytes(&[1, *class]),
            Limit::Race(race) => out.bytes(&[2, *race]),
            Limit::Subrace(subrace) => out.bytes(&[3, *subrace]),
            Limit::Feat(feat) => {
                out.u8(4);
                out.u16(*feat);
            }
            Limit::Gender(gender) => out.bytes(&[5, *gender]),
            Limit::Player => out.u8(6),
            Limit::Tag(tag) => {
                out.u8(7);
                out.string(tag);
            }
            Limit::Ability { ability, at_least } => out.bytes(&[8, *ability, *at_least]),
        }
    }

    pub(crate) fn decode(input: &mut Reader) -> Result<Limit, DecodeError> {
        let at = input.at();
        Ok(match input.u8()? {
            0 => Limit::Side(input.u8()?),
            1 => Limit::Class(input.u8()?),
            2 => Limit::Race(input.u8()?),
            3 => Limit::Subrace(input.u8()?),
            4 => Limit::Feat(input.u16()?),
            5 => Limit::Gender(input.u8()?),
            6 => Limit::Player,
            7 => Limit::Tag(input.string()?),
            8 => Limit::Ability {
                ability: input.u8()?,
                at_least: input.u8()?,
            },
            _ => return Err(input.invalid("use limit", at)),
        })
    }

    /// Its kind, for "one met of a kind is enough" (a subrace counts as a
    /// race, as TSL's check takes either).
    fn kind(&self) -> u8 {
        match self {
            Limit::Side(_) => 0,
            Limit::Class(_) => 1,
            Limit::Race(_) | Limit::Subrace(_) => 2,
            Limit::Feat(_) => 3,
            Limit::Gender(_) => 4,
            Limit::Player | Limit::Tag(_) => 5,
            Limit::Ability { .. } => 6,
        }
    }
}

/// Who would wear an item, as its use limits ask: its alignment (0 dark
/// to 100 light), classes (`classes.2da` rows, `u8::MAX` for none), race,
/// subrace, feats, gender, scores, whether a player character and its tag;
/// and whether TSL's rules are played.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Wearer {
    pub good_evil: u8,
    pub classes: [u8; 2],
    pub race: u8,
    pub subrace: u8,
    pub feats: crate::combat::sheet::Rows<4>,
    pub gender: u8,
    pub abilities: [u8; 6],
    pub player: bool,
    pub tag: String,
    pub tsl: bool,
}

/// The talk-table lines a refused equip shows, by the limit refused: K1's
/// and TSL's alignment, class and race lines, the proficiency line for a
/// feat, TSL's gender and attribute lines (its party-member limit has
/// none: the class line stands in).
pub mod refusals {
    pub const SIDE: u32 = 1512;
    pub const CLASS: u32 = 1513;
    pub const RACE: u32 = 1514;
    pub const FEAT: u32 = crate::combat::passive::NOT_PROFICIENT;
    pub const GENDER: u32 = 47993;
    pub const ABILITY: u32 = 117590;
}

impl Limit {
    /// Whether `wearer` meets it.
    fn met(&self, wearer: &Wearer) -> bool {
        use crate::combat::gear::sides;
        match self {
            Limit::Side(side_limits::ANY) => wearer.tsl,
            Limit::Side(side_limits::WHOLLY_DARK) => wearer.tsl && wearer.good_evil == 0,
            Limit::Side(side_limits::WHOLLY_LIGHT) => wearer.tsl && wearer.good_evil == 100,
            Limit::Side(side) => sides::of(wearer.good_evil) == *side,
            Limit::Class(class) => wearer.classes.contains(class),
            Limit::Race(race) => wearer.race == *race,
            Limit::Subrace(subrace) => wearer.subrace == *subrace,
            Limit::Feat(feat) => wearer.feats.has(*feat),
            // 0 male, 1 female; 2 both.
            Limit::Gender(gender) => *gender == 2 && wearer.gender <= 1 || *gender == wearer.gender,
            Limit::Player => wearer.player,
            Limit::Tag(tag) => wearer.tag.eq_ignore_ascii_case(tag),
            Limit::Ability { ability, at_least } => wearer
                .abilities
                .get(usize::from(*ability))
                .is_some_and(|score| score >= at_least),
        }
    }

    /// The line refusing it.
    fn refusal(&self) -> u32 {
        match self {
            Limit::Side(_) => refusals::SIDE,
            Limit::Class(_) | Limit::Player | Limit::Tag(_) => refusals::CLASS,
            Limit::Race(_) | Limit::Subrace(_) => refusals::RACE,
            Limit::Feat(_) => refusals::FEAT,
            Limit::Gender(_) => refusals::GENDER,
            Limit::Ability { .. } => refusals::ABILITY,
        }
    }

    /// Whether Proficiency All waives it (alignment, class and feat).
    fn waived(&self, wearer: &Wearer) -> bool {
        matches!(self, Limit::Side(_) | Limit::Class(_) | Limit::Feat(_))
            && wearer
                .feats
                .has(crate::combat::sheet::feats::PROFICIENCY_ALL)
    }
}

/// Whether `wearer` may equip an item of `limits`: of each kind of limit
/// it has, one met (or waived); else the line refusing it.
pub fn may_wear(limits: &[Limit], wearer: &Wearer) -> Result<(), u32> {
    for kind in 0..=6 {
        let of_kind: Vec<&Limit> = limits.iter().filter(|limit| limit.kind() == kind).collect();
        if let Some(first) = of_kind.first()
            && !of_kind
                .iter()
                .any(|limit| limit.met(wearer) || limit.waived(wearer))
        {
            return Err(first.refusal());
        }
    }
    Ok(())
}

impl Upgrade {
    pub(crate) fn encode(&self, out: &mut Writer) {
        out.u8(self.row);
        out.u8(self.kind);
        out.string(&self.template);
        out.u32(self.properties.len() as u32);
        for property in &self.properties {
            property.encode(out);
        }
        out.u64(self.rows as u64);
        out.u64((self.rows >> 64) as u64);
    }

    pub(crate) fn decode(input: &mut Reader) -> Result<Upgrade, DecodeError> {
        let at = input.at();
        let row = input.u8()?;
        if super::upgrade_bit(row).is_none() {
            return Err(input.invalid("upgrade row", at));
        }
        let kind = input.u8()?;
        let template = input.string()?;
        let properties = (0..input.u32()?)
            .map(|_| Property::decode(input))
            .collect::<Result<_, _>>()?;
        let rows = u128::from(input.u64()?) | u128::from(input.u64()?) << 64;
        Ok(Upgrade {
            row,
            kind,
            template,
            properties,
            rows,
        })
    }
}

/// `make`'s extras and on-hit effects fit the weapon's (`EXTRAS`,
/// `ON_HITS`).
const _: () = assert!(EXTRAS >= 2 && ON_HITS >= 1);
