//! KOTOR's character sheet as combat reads it (`docs/COMBAT.md`):
//! attributes, the base attack bonus, saves, the HP and FP maxima,
//! defence, feats and powers, faction, flags, and the two
//! weapons. Plain `Copy` values, built by `session` from a creature's UTC,
//! its UTIs and the 2DAs, or a dev preset for a player until the party
//! table exists (I19).

pub use crate::combat::gear::Versus;
use crate::combat::gear::{Extra, Gear, OnHit, Roll};
use crate::combat::items::{Carried, KINDS};
use crate::combat::passive;
use crate::wire::{DecodeError, Reader, Writer, read_fixed, write_fixed};

/// A set of 2DA rows (`feat.2da`, `spells.2da`) as bits: row `n` is bit
/// `n % 64` of word `n / 64`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rows<const WORDS: usize>(pub [u64; WORDS]);

impl<const WORDS: usize> Default for Rows<WORDS> {
    fn default() -> Self {
        Rows([0; WORDS])
    }
}

impl<const WORDS: usize> Rows<WORDS> {
    pub fn with(mut self, row: u16) -> Self {
        self.insert(row);
        self
    }

    /// How many rows the set can hold.
    pub const ROWS: u16 = (WORDS * 64) as u16;

    /// Adds `row`; a row past the set's end is dropped.
    pub fn insert(&mut self, row: u16) {
        if let Some(word) = self.0.get_mut(usize::from(row / 64)) {
            *word |= 1 << (row % 64);
        }
    }

    pub fn has(&self, row: u16) -> bool {
        self.0
            .get(usize::from(row / 64))
            .is_some_and(|word| word & (1 << (row % 64)) != 0)
    }

    /// How many of `rows` are in the set (a feat line's tier).
    pub fn count(&self, rows: &[u16]) -> u8 {
        rows.iter().filter(|row| self.has(**row)).count() as u8
    }
}

/// Feat rows (`feat.2da`) combat and the skills read.
pub mod feats {
    pub const TWO_WEAPON_FIGHTING: [u16; 3] = [3, 9, 85];
    pub const CRITICAL_STRIKE: [u16; 3] = [8, 19, 81];
    pub const FLURRY: [u16; 2] = [11, 91];
    /// Master Flurry (`WHIRLWIND_ATTACK`), Flurry's third line.
    pub const MASTER_FLURRY: u16 = 53;
    pub const POWER_ATTACK: [u16; 3] = [28, 17, 83];
    pub const POWER_BLAST: [u16; 3] = [29, 18, 82];
    pub const RAPID_SHOT: [u16; 3] = [30, 92, 26];
    pub const SNIPER_SHOT: [u16; 3] = [31, 20, 77];
    pub const JEDI_DEFENSE: [u16; 3] = [55, 1, 24];
    pub const DUELING: [u16; 3] = [113, 114, 115];
    pub const FORCE_JUMP: [u16; 3] = [101, 102, 103];
    pub const UNCANNY_DODGE: [u16; 2] = [56, 57];
    pub const SNEAK_ATTACK: [u16; 10] = [60, 61, 62, 63, 64, 65, 66, 67, 68, 69];
    /// Toughness and Master Toughness: +1 HP a level each, retroactive.
    pub const TOUGHNESS: [u16; 2] = [84, 124];
    pub const IMPROVED_TOUGHNESS: u16 = 123;
    /// Wookiee Toughness I to III (TSL's second and third).
    pub const WOOKIEE_TOUGHNESS: [u16; 3] = [95, 224, 225];
    pub const CONDITIONING: [u16; 3] = [13, 21, 22];
    pub const FORCE_SENSITIVE: u16 = 116;
    /// Caution, Empathy and Gear Head (each tier replaces the last) and
    /// Perceptive (`XXXX_` in K1's table), what each tier adds
    /// (`rules::feats::skills::LINE_PER_TIER`), with the skills they raise.
    pub const SKILL_LINES: [(&[u16], i32, &[usize]); 4] = {
        use super::skills::*;
        let [caution, empathy, gear_head, perceptive] = rules::feats::skills::LINE_PER_TIER;
        [
            (&[7, 117, 118], caution, &[DEMOLITIONS, STEALTH]),
            (
                &[10, 121, 122],
                empathy,
                &[PERSUADE, AWARENESS, TREAT_INJURY],
            ),
            (
                &[12, 119, 120],
                gear_head,
                &[REPAIR, SECURITY, COMPUTER_USE],
            ),
            (&[27], perceptive, &[STEALTH, AWARENESS]),
        ]
    };
    /// Skill Focus (`XXXX_` in K1's table): `rules::feats::skills::SKILL_FOCUS`
    /// to its skill.
    pub const SKILL_FOCUS: [(u16, usize); 8] = {
        use super::skills::*;
        [
            (58, COMPUTER_USE),
            (70, DEMOLITIONS),
            (71, STEALTH),
            (72, AWARENESS),
            (73, PERSUADE),
            (74, REPAIR),
            (75, SECURITY),
            (76, TREAT_INJURY),
        ]
    };
    /// The armour and weapon proficiencies Proficiency All stands for.
    pub const PROFICIENCIES: [u16; 10] = [4, 5, 6, 39, 40, 41, 42, 43, 44, 45];
    pub const WEAPON_FOCUS_BLASTER: u16 = 32;
    pub const WEAPON_FOCUS_BLASTER_RIFLE: u16 = 33;
    pub const WEAPON_FOCUS_HEAVY_WEAPONS: u16 = 35;
    pub const WEAPON_FOCUS_LIGHTSABER: u16 = 36;
    pub const WEAPON_FOCUS_MELEE: u16 = 37;
    pub const PROFICIENCY_BLASTER: u16 = 39;
    pub const PROFICIENCY_BLASTER_RIFLE: u16 = 40;
    pub const PROFICIENCY_HEAVY_WEAPONS: u16 = 42;
    pub const PROFICIENCY_LIGHTSABER: u16 = 43;
    pub const PROFICIENCY_MELEE: u16 = 44;
    pub const WEAPON_SPEC_BLASTER: u16 = 46;
    pub const WEAPON_SPEC_BLASTER_RIFLE: u16 = 47;
    pub const WEAPON_SPEC_HEAVY_WEAPONS: u16 = 49;
    pub const WEAPON_SPEC_LIGHTSABER: u16 = 50;
    pub const WEAPON_SPEC_MELEE: u16 = 51;
    pub const PROFICIENCY_ALL: u16 = 93;
    /// Force Focus, Advanced, Mastery, and what each adds to the powers' DC.
    pub const FORCE_FOCUS: [u16; 3] = [88, 89, 90];
    pub const FORCE_FOCUS_DC: [i32; 3] = rules::feats::force_focus::DC;
    pub const FORCE_IMMUNITY_FEAR: u16 = 98;
    pub const FORCE_IMMUNITY_STUN: u16 = 99;
    pub const FORCE_IMMUNITY_PARALYSIS: u16 = 100;
}

/// Sheet flags.
pub mod flags {
    /// `Plot`: takes no damage at all.
    pub const PLOT: u8 = 1;
    /// `Min1HP`: damage stops at 1 HP.
    pub const MIN_ONE_HP: u8 = 2;
    /// A test's god mode (`kjk playthrough --god`): damage stops at 1 HP
    /// whatever scripts say (`SetMinOneHP` leaves it: the Endar Spire's
    /// clears the player's Min1HP on arriving at its second deck).
    pub const IMMORTAL: u8 = 16;
    /// A test's mighty player (`kjk playthrough --mighty`): its blows
    /// count many times over (`damage::MIGHTY_TIMES`), so a playtest's
    /// fights end quickly; what the scripts do is unchanged.
    pub const MIGHTY: u8 = 32;
    /// A droid (`races::DROID`): no pain animations by chance; droid powers
    /// work on it and the living's do not.
    pub const DROID: u8 = 4;
    /// Wears a stealth field generator (`inventory::ItemFacts::stealth_unit`):
    /// stealth mode may be turned on (S11).
    pub const STEALTH_UNIT: u8 = 64;
    /// Played by TSL's rules (`gear::Caps`: its effects' caps three times
    /// K1's).
    pub const TSL: u8 = 128;
    /// `EffectAssuredHit` on it now (`effects::sheet_now`): its attacks a
    /// script asked for land (`script::actions`).
    pub const ASSURED_HIT: u8 = 8;
}

/// Abilities, in `ABILITY_*` order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Ability {
    Strength = 0,
    Dexterity = 1,
    Constitution = 2,
    Intelligence = 3,
    Wisdom = 4,
    Charisma = 5,
}

impl Ability {
    /// In `ABILITY_*` order: an ability by its number.
    pub const ALL: [Ability; 6] = [
        Ability::Strength,
        Ability::Dexterity,
        Ability::Constitution,
        Ability::Intelligence,
        Ability::Wisdom,
        Ability::Charisma,
    ];
}

/// How a weapon delivers its hits: the JA weapon or move set it takes
/// (I22 section 7), from its base item (`baseitems.2da` row).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Delivery {
    /// Fists (no weapon).
    Unarmed,
    /// The stun baton's short trace.
    Baton,
    /// Swords, vibroblades and staves: the saber move tables (C9).
    Melee,
    /// A lightsaber: single, double-bladed (staff) or short.
    Saber(SaberShape),
    /// A JA blaster: which one, by its cadence and bolt.
    Blaster(Blaster),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SaberShape {
    Single,
    Staff,
    Short,
}

/// The JA weapon a KOTOR ranged base item fires as (I22 section 7).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Blaster {
    /// Pistols (Bryar): alternate fire charged.
    Pistol,
    /// Rifles and carbines (E-11).
    Rifle,
    /// Repeating blasters.
    Repeater,
    Bowcaster,
    /// Disruptors: hitscan.
    Disruptor,
    /// Ion weapons (DEMP2): not deflectable.
    Ion,
    /// Sonic weapons: the flechette's spread.
    Sonic,
}

/// The `baseitems.2da` rows of the melee weapons held in both hands
/// (`weaponwield` 3 in K1 and TSL): the quarterstaff, the double-bladed
/// sword and vibro double blade, the gaffi stick, the Wookiee warblade.
pub const TWO_HANDED_MELEE: [u16; 5] = [0, 6, 7, 78, 79];

impl Delivery {
    /// The delivery of a `baseitems.2da` row (the rows K1 and TSL share for
    /// weapons); `None` for a row that is not a weapon combat knows.
    pub fn of_base_item(row: u16) -> Option<Delivery> {
        use Blaster::*;
        Some(match row {
            1 => Delivery::Baton,
            // 81..=83: a creature's claws, bite and gore (`Creature_Item_*`):
            // a beast fights in melee, as KOTOR's creature weapons strike.
            0 | 2..=7 | 78..=83 => Delivery::Melee,
            8 => Delivery::Saber(SaberShape::Single),
            9 => Delivery::Saber(SaberShape::Staff),
            10 => Delivery::Saber(SaberShape::Short),
            12..=14 => Delivery::Blaster(Pistol),
            15 | 18 => Delivery::Blaster(Ion),
            16 | 21 => Delivery::Blaster(Disruptor),
            17 | 22 => Delivery::Blaster(Sonic),
            19 => Delivery::Blaster(Bowcaster),
            20 | 77 => Delivery::Blaster(Rifle),
            23 | 24 => Delivery::Blaster(Repeater),
            _ => return None,
        })
    }

    pub fn ranged(self) -> bool {
        matches!(self, Delivery::Blaster(_))
    }

    pub(crate) fn index(self) -> u8 {
        match self {
            Delivery::Unarmed => 0,
            Delivery::Baton => 1,
            Delivery::Melee => 2,
            Delivery::Saber(SaberShape::Single) => 3,
            Delivery::Saber(SaberShape::Staff) => 4,
            Delivery::Saber(SaberShape::Short) => 5,
            Delivery::Blaster(Blaster::Pistol) => 6,
            Delivery::Blaster(Blaster::Rifle) => 7,
            Delivery::Blaster(Blaster::Repeater) => 8,
            Delivery::Blaster(Blaster::Bowcaster) => 9,
            Delivery::Blaster(Blaster::Disruptor) => 10,
            Delivery::Blaster(Blaster::Ion) => 11,
            Delivery::Blaster(Blaster::Sonic) => 12,
        }
    }

    pub(crate) fn from_index(index: u8) -> Option<Delivery> {
        use Blaster::*;
        Some(match index {
            0 => Delivery::Unarmed,
            1 => Delivery::Baton,
            2 => Delivery::Melee,
            3 => Delivery::Saber(SaberShape::Single),
            4 => Delivery::Saber(SaberShape::Staff),
            5 => Delivery::Saber(SaberShape::Short),
            6 => Delivery::Blaster(Pistol),
            7 => Delivery::Blaster(Rifle),
            8 => Delivery::Blaster(Repeater),
            9 => Delivery::Blaster(Bowcaster),
            10 => Delivery::Blaster(Disruptor),
            11 => Delivery::Blaster(Ion),
            12 => Delivery::Blaster(Sonic),
            _ => return None,
        })
    }
}

/// Damage types, as `baseitems.2da`'s `damageflags` bits.
pub mod damage_types {
    pub const BLUDGEONING: u16 = 1;
    pub const PIERCING: u16 = 2;
    pub const SLASHING: u16 = 4;
    pub const SONIC: u16 = 1024;
    pub const ION: u16 = 2048;
    pub const ENERGY: u16 = 4096;
}

/// KOTOR's damage types: `iprp_damagetype.2da`'s rows, whose `damageflags`
/// bit is `1 << row` (bludgeoning 0, piercing 1, slashing 2, universal 3,
/// acid 4, cold 5, light side 6, electrical 7, fire 8, dark side 9, sonic
/// 10, ion 11, energy 12).
pub const DAMAGE_TYPES: usize = 13;

/// Damage type rows by name (`DAMAGE_TYPES`).
pub mod type_rows {
    pub const BLUDGEONING: u8 = 0;
    pub const PIERCING: u8 = 1;
    pub const SLASHING: u8 = 2;
    pub const COLD: u8 = 5;
    pub const ELECTRICAL: u8 = 7;
    pub const FIRE: u8 = 8;
    pub const SONIC: u8 = 10;
    pub const ION: u8 = 11;
    pub const ENERGY: u8 = 12;
}

/// Skills (`skills.2da` rows), as `Sheet::skills` holds their ranks.
pub mod skills {
    pub const COMPUTER_USE: usize = 0;
    pub const DEMOLITIONS: usize = 1;
    pub const STEALTH: usize = 2;
    pub const AWARENESS: usize = 3;
    pub const PERSUADE: usize = 4;
    pub const REPAIR: usize = 5;
    pub const SECURITY: usize = 6;
    pub const TREAT_INJURY: usize = 7;
}

/// Races combat reads (`racialtypes.2da` rows).
pub mod races {
    pub const DROID: u8 = 5;
    pub const HUMAN: u8 = 6;
}

/// What a character's items do to each damage type (I22 section 1 step
/// 9): immunity in percent, a vulnerability counting against it (−50 is
/// half again), and resistance, whole points off each KOTOR attack.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Guards {
    pub immunity: [i8; DAMAGE_TYPES],
    pub resist: [u8; DAMAGE_TYPES],
}

impl Guards {
    /// What is left of `roll` whole points of damage type `row`.
    pub fn take(&self, row: usize, roll: i64) -> i64 {
        let (Some(immunity), Some(resist)) = (self.immunity.get(row), self.resist.get(row)) else {
            return roll;
        };
        let through = roll * (100 - i64::from(*immunity).min(100)) / 100;
        (through - i64::from(*resist)).max(0)
    }

    /// Of the types in `flags`, the one letting most of `roll` through (a
    /// weapon of several types hurts as the worst for its target; the
    /// lowest row on a tie); `None` when it has none.
    pub fn weakest(&self, flags: u16, roll: i64) -> Option<usize> {
        (0..DAMAGE_TYPES)
            .filter(|row| flags & (1 << row) != 0)
            .max_by_key(|&row| (self.take(row, roll), std::cmp::Reverse(row)))
    }
}

/// A weapon in hand, as its base item and its item properties give it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Weapon {
    pub base_item: u16,
    pub delivery: Delivery,
    /// `numdice` d `dietoroll` (no dice for a weapon whose items take its
    /// base damage away, `DamageNone`).
    pub dice: u8,
    pub die: u8,
    /// Extra dice from its `Damage` properties of its own type (`1d4` to
    /// `2d6`).
    pub bonus_dice: u8,
    pub bonus_die: u8,
    /// `critthreat` (1: a natural 20, 2: 19 and 20; Keen doubles it) and
    /// `crithitmult`.
    pub crit_threat: u8,
    pub crit_mult: u8,
    /// `damageflags`.
    pub damage_types: u16,
    /// Item properties: attack bonus (or penalty), damage bonus, and the
    /// blaster-bolt deflection modifier (a lightsaber's, read by bolt
    /// blocking, I24).
    pub attack_bonus: i8,
    pub damage_bonus: i8,
    pub deflect: i8,
    /// The feats its base item names (`baseitems.2da` `reqfeat0`,
    /// `focfeat`, `specfeat`): without the first, a penalty to hit; the
    /// focus a bonus to hit; the specialization to damage
    /// (`rules::feats::weapon`).
    pub proficiency: Option<u16>,
    pub focus: Option<u16>,
    pub specialization: Option<u16>,
    /// To hit and damage against one kind of foe (a race, a side).
    pub versus: Option<Versus>,
    /// Damage of other types its properties add to each hit.
    pub extras: [Option<Extra>; EXTRAS],
    /// Massive Criticals (49): damage a critical adds, not multiplied.
    pub massive: Roll,
    /// What its hits may do besides damage (32, `OnHit`).
    pub on_hit: [Option<OnHit>; ON_HITS],
    /// Mighty (30): held ranged in the right hand, the most STR adds to its
    /// damage; without it a ranged weapon takes only STR's penalty.
    pub mighty: Option<u8>,
    /// Its enhancement (Enhancement's, 5): what overcomes a soak.
    pub enhancement: u8,
    /// `gear::weapon_traits` bits.
    pub traits: u8,
    /// How far it reaches for a fight (`baseitems.2da` `maxattackrange`,
    /// metres; 0 for a melee weapon): a mind closes in on a target beyond
    /// it (I22 section 7).
    pub reach_m: u8,
    /// How it is held (`build` bits).
    pub build: u8,
}

/// How many other damage types and on-hit effects a weapon holds.
pub const EXTRAS: usize = 3;
pub const ON_HITS: usize = 2;

/// How a weapon is held, from its base item (`baseitems.2da`), as bits:
/// what Dueling and two-weapon fighting ask (`combat::passive`).
pub mod build {
    /// `weaponwield` 1, 2 or 4: a baton, a one-handed blade, a pistol.
    pub const ONE_HANDED: u8 = 1;
    /// `weaponsize` 2 or less: balanced in the off hand.
    pub const SMALL: u8 = 2;
    /// Double-bladed (its `label`): fought with as two weapons. No column
    /// says so: `weaponwield` 3 (reone's double-bladed wield) also holds the
    /// quarterstaff, the gaffi stick and the warblade [verify: whether
    /// KOTOR fights those as two weapons].
    pub const DOUBLE: u8 = 4;

    /// The bits of a `baseitems.2da` row's `weaponwield`, `weaponsize` and
    /// `label`.
    pub fn of(wield: Option<i64>, size: Option<i64>, label: &str) -> u8 {
        let mut bits = 0;
        if matches!(wield, Some(1 | 2 | 4)) {
            bits |= ONE_HANDED;
        }
        if size.is_some_and(|size| (1..=2).contains(&size)) {
            bits |= SMALL;
        }
        if label.to_ascii_lowercase().contains("double") {
            bits |= DOUBLE;
        }
        bits
    }
}

impl Weapon {
    /// K1's `maxattackrange` for a ranged base item row (metres), for
    /// weapons built without the table (a test's); a game's weapons read
    /// the column.
    pub fn reach_of(row: u16) -> u8 {
        match row {
            12..=14 | 16 => 23,
            15 | 17 => 17,
            20 => 25,
            18 | 19 | 21..=24 | 77 => 28,
            _ => 0,
        }
    }

    /// K1's `build` for a weapon base item row, for weapons built without
    /// the table (a test's); a game's weapons read its columns.
    pub fn build_of(row: u16) -> u8 {
        let (wield, size) = match row {
            0 | 6 | 7 | 9 | 78 | 79 => (3, 4),
            1 => (1, 1),
            2 | 3 | 8 => (2, 3),
            4 | 5 | 10 => (2, 2),
            80 => (2, 4),
            12..=17 => (4, 2),
            18..=22 | 77 => (5, 4),
            23 | 24 => (6, 4),
            _ => return 0,
        };
        let double = if matches!(row, 6 | 7 | 9) {
            "double"
        } else {
            ""
        };
        build::of(Some(wield), Some(size), double)
    }

    /// A plain weapon of base item `row` with `baseitems.2da`'s numbers and
    /// feats (proficiency, focus, specialization).
    pub fn plain(
        row: u16,
        dice: u8,
        die: u8,
        crit: (u8, u8),
        damage_types: u16,
        feats: [u16; 3],
    ) -> Weapon {
        Weapon {
            base_item: row,
            delivery: Delivery::of_base_item(row).unwrap_or(Delivery::Unarmed),
            dice,
            die,
            crit_threat: crit.0,
            crit_mult: crit.1,
            damage_types,
            proficiency: Some(feats[0]),
            focus: Some(feats[1]),
            specialization: Some(feats[2]),
            reach_m: Weapon::reach_of(row),
            build: Weapon::build_of(row),
            ..Weapon::UNARMED
        }
    }

    /// Fists: KOTOR's unarmed strike, bludgeoning (`rules::weapons::unarmed`).
    pub const UNARMED: Weapon = Weapon {
        base_item: u16::MAX,
        delivery: Delivery::Unarmed,
        dice: rules::weapons::unarmed::DICE,
        die: rules::weapons::unarmed::DIE,
        crit_threat: rules::weapons::unarmed::CRIT_THREAT,
        crit_mult: rules::weapons::unarmed::CRIT_MULT,
        bonus_dice: 0,
        bonus_die: 0,
        damage_types: damage_types::BLUDGEONING,
        attack_bonus: 0,
        damage_bonus: 0,
        deflect: 0,
        proficiency: None,
        focus: None,
        specialization: None,
        versus: None,
        extras: [None; EXTRAS],
        massive: Roll::NONE,
        on_hit: [None; ON_HITS],
        mighty: None,
        enhancement: 0,
        traits: 0,
        reach_m: 0,
        build: 0,
    };
}

/// An alignment of neither side: where a made character starts.
pub const NEUTRAL: u8 = rules::combat::sheet::NEUTRAL;

/// A side of the Force an alignment counts as.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Side {
    Light,
    Neutral,
    Dark,
}

/// The side alignment `good_evil` counts as (`GetAlignmentGoodEvil`, the
/// character sheet's pose), as NWN's good and evil (reone's thresholds
/// too: `rules::combat::sheet::LIGHT_LEAST`, `DARK_MOST`).
pub fn side(good_evil: u8) -> Side {
    use rules::combat::sheet;
    if good_evil >= sheet::LIGHT_LEAST {
        Side::Light
    } else if good_evil <= sheet::DARK_MOST {
        Side::Dark
    } else {
        Side::Neutral
    }
}

/// The sheet (see the module documentation).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Sheet {
    /// Scores, in `Ability` order.
    pub abilities: [u8; 6],
    /// Total character level.
    pub level: u8,
    /// Base attack bonus: the classes' `attackbonustable` at their levels.
    pub bab: u8,
    /// Fortitude, Reflex and Will before abilities.
    pub saves: [i8; 3],
    pub max_hp: u16,
    pub max_fp: u16,
    /// Defence beyond 10 + DEX: armour, natural and class bonuses.
    pub defence: i8,
    /// The most DEX may add to defence (the armour's `dexbonus`), if the
    /// armour caps it.
    pub dex_cap: Option<u8>,
    /// `feat.2da` rows known (256 rows).
    pub feats: Rows<4>,
    /// `spells.2da` rows known (320 rows: TSL's table has 283).
    pub powers: Rows<5>,
    /// The standard faction (`repute.2da` row).
    pub faction: u8,
    /// `racialtypes.2da` row (`races`).
    pub race: u8,
    /// What its items do to each damage type.
    pub guards: Guards,
    /// `flags`.
    pub flags: u8,
    /// Right hand, left hand.
    pub weapons: [Option<Weapon>; 2],
    /// What it carries to use in a fight, four kinds at most
    /// (`combat::items`: grenades, medpacs, shields, stims).
    pub items: [Option<Carried>; KINDS],
    /// Skill ranks, by `skills` row (I22 section 5).
    pub skills: [u8; 8],
    /// What worn items give beyond weapons and guards (`inventory::rewear`):
    /// scores, skills, saves, defence, feats, immunities…, folded in by
    /// `effects::sheet_now`; the sheet's own fields stay its own.
    pub gear: Gear,
    /// To hit from what runs on it (Slow's −2, a battle stim's bonus,
    /// `rules::items::stims::BATTLE_BONUS`: `combat::effects::sheet_now`); 0
    /// on a sheet as read.
    pub attack: i8,
    /// Good and evil, 0 (dark) to 100 (light): the template's `GoodEvil`,
    /// as scripts move it (`AdjustAlignment`, `SetGoodEvilValue`).
    pub good_evil: u8,
    /// TSL's bonus Force points (`BonusForcePoints`, the story's rewards:
    /// `AddBonusForcePoints`), added to a Force user's most FP (`pool`).
    pub bonus_fp: i16,
    /// TSL's base save bonuses, Fortitude, Reflex and Will (the template's
    /// `fortbonus`, `refbonus`, `willbonus`, as `Modify…SavingThrowBase`
    /// moves them), added to each save (`save`).
    pub save_bonus: [i8; 3],
    /// TSL's Form in use (`CurrentForm`, `combat::forms`): a `spells.2da`
    /// row, 0 for none.
    pub form: u16,
}

impl Sheet {
    /// An ability's modifier: (score − 10) / 2, rounded down.
    pub fn modifier(&self, ability: Ability) -> i32 {
        (i32::from(self.abilities[ability as usize]) - 10).div_euclid(2)
    }

    /// The second bar's size in whole points (I22 C7): a Force user's FP;
    /// everyone else's stamina (`rules::combat::sheet`: a base and so much
    /// a point of the CON modifier, at least the least). It pays for saber
    /// and feat specials (`combat::specials`), and refills as the Force does
    /// (C13).
    pub fn pool(&self) -> u16 {
        use rules::combat::sheet;
        if self.max_fp > 0 {
            self.force_points()
        } else {
            (sheet::STAMINA_BASE + sheet::STAMINA_PER_CON * self.modifier(Ability::Constitution))
                .max(sheet::STAMINA_LEAST) as u16
        }
    }

    /// Its most FP (`GetMaxForcePoints`): the sheet's with TSL's bonus
    /// (`bonus_fp`, a plain add as its executable makes it), never below 0.
    /// The bonus counts only for a Force user: anyone else's bar is
    /// stamina (`pool`).
    pub fn force_points(&self) -> u16 {
        if self.max_fp == 0 {
            return 0;
        }
        (i32::from(self.max_fp) + i32::from(self.bonus_fp)).clamp(0, i32::from(u16::MAX)) as u16
    }

    /// The weapon in the right hand, else fists.
    pub fn main_weapon(&self) -> Weapon {
        self.weapons[0].unwrap_or(Weapon::UNARMED)
    }

    /// The weapon in `hand` (0 right, 1 left: `Fighter::hand`, the one it
    /// has out), else its main one: what it strikes or fires with.
    pub fn held(&self, hand: u8) -> Weapon {
        self.weapons[usize::from(hand.min(1))].unwrap_or(self.main_weapon())
    }

    /// Whether the weapon in `hand` fires (a gun): a gunner that drew a
    /// blade (k_ai_master's pack blade within 3 m) strikes as a blade does.
    pub fn gunner(&self, hand: u8) -> bool {
        self.held(hand).delivery.ranged()
    }

    /// A pistol in each hand (KOTOR's two pistols, wield number 6): both
    /// guns fire, taking turns (`combat::pair`). The hands pair only
    /// one-handed guns (`inventory::pairs`).
    pub fn pistol_pair(&self) -> bool {
        self.weapons
            .iter()
            .all(|weapon| weapon.is_some_and(|w| matches!(w.delivery, Delivery::Blaster(_))))
    }

    /// KOTOR's wield number of what it holds (as reone numbers it,
    /// the digit of its combat clips `c3a2`, `g3r1`, `b7a1`): 1 a stun
    /// baton, 3 a staff or double blade (`TWO_HANDED_MELEE`, a double
    /// lightsaber), 2 another blade, 8 bare hands; by `baseitems.2da`'s
    /// `weaponwield` for a gun, 5 a pistol, 7 a rifle, 9 a heavy weapon;
    /// with a weapon in each hand by the left one's, 4 a blade, 6 a pistol,
    /// else 0.
    pub fn wield_number(&self) -> u8 {
        // A weapon in each hand: by the left's kind, a blade (4), a pistol
        // (6), anything else 0, as K1's executable reads it.
        match self.weapons[1].map(|off| off.delivery) {
            Some(Delivery::Melee | Delivery::Saber(_)) => 4,
            Some(Delivery::Blaster(_)) => 6,
            Some(_) => 0,
            None => self.right_class(),
        }
    }

    /// The wield number of the right hand's weapon alone (`wield_number`
    /// with nothing in the left): a gun's shot clip goes by it.
    pub fn right_class(&self) -> u8 {
        let main = self.main_weapon();
        match (main.delivery, main.base_item) {
            (Delivery::Unarmed, _) => crate::combat::rounds::UNARMED,
            (Delivery::Baton, _) => 1,
            (Delivery::Saber(SaberShape::Staff), _) => 3,
            (Delivery::Melee, row) if TWO_HANDED_MELEE.contains(&row) => 3,
            (Delivery::Blaster(_), 23 | 24) => 9,
            (Delivery::Blaster(_), 18..=22 | 77) => 7,
            (Delivery::Blaster(_), _) => 5,
            _ => 2,
        }
    }

    /// KOTOR's defence: `rules::combat::sheet::DEFENCE_BASE` + the DEX
    /// modifier (up to the armour's cap) + the rest + Dueling's.
    pub fn defence(&self) -> i32 {
        let dex = self.modifier(Ability::Dexterity);
        let dex = self.dex_cap.map_or(dex, |cap| dex.min(i32::from(cap)));
        rules::combat::sheet::DEFENCE_BASE + dex + i32::from(self.defence) + passive::dueling(self)
    }

    /// A save's total before the d20 (`index` 0 Fortitude, 1 Reflex, 2
    /// Will): its class value, TSL's base bonus (`save_bonus`), its
    /// ability's modifier (CON, DEX, WIS) and Conditioning's.
    pub fn save(&self, index: usize) -> i32 {
        let index = index.min(2);
        let ability = [Ability::Constitution, Ability::Dexterity, Ability::Wisdom][index];
        i32::from(self.saves[index])
            + i32::from(self.save_bonus[index])
            + self.modifier(ability)
            + passive::save_bonus(&self.feats)
    }

    /// A skill's ranks with what feats add (`skills` row; its ability is
    /// the caller's).
    pub fn skill(&self, skill: usize) -> i32 {
        let ranks = self.skills.get(skill).copied().unwrap_or(0);
        i32::from(ranks) + passive::skill_bonus(&self.feats, skill, ranks)
    }

    /// What a creature whose sheet could not be read fights as: level 1,
    /// every score 10, 4 HP, fists, the neutral faction.
    pub fn commoner() -> Sheet {
        Sheet {
            abilities: [10; 6],
            level: 1,
            bab: 0,
            saves: [0; 3],
            max_hp: 4,
            max_fp: 0,
            defence: 0,
            dex_cap: None,
            feats: Rows::default(),
            powers: Rows::default(),
            faction: factions::NEUTRAL,
            race: races::HUMAN,
            guards: Guards::default(),
            flags: 0,
            weapons: [None; 2],
            items: [None; KINDS],
            skills: [0; 8],
            gear: Gear::default(),
            attack: 0,
            good_evil: NEUTRAL,
            bonus_fp: 0,
            save_bonus: [0; 3],
            form: 0,
        }
    }

    /// A level-1 soldier as I22's worked fights assume (STR 16, DEX 14,
    /// CON 14; 12 HP; a blaster pistol and a vibroblade), with lightsaber
    /// proficiency and Jedi Defense for the dev saber.
    pub fn dev_soldier() -> Sheet {
        let feats = [
            feats::PROFICIENCY_BLASTER,
            feats::PROFICIENCY_BLASTER_RIFLE,
            feats::PROFICIENCY_MELEE,
            feats::PROFICIENCY_LIGHTSABER,
            feats::JEDI_DEFENSE[0],
        ];
        Sheet {
            abilities: [16, 14, 14, 10, 12, 10],
            level: 1,
            bab: 1,
            saves: [2, 0, 0],
            max_hp: 12,
            max_fp: 0,
            defence: 0,
            dex_cap: None,
            feats: feats
                .iter()
                .fold(Rows::default(), |rows, feat| rows.with(*feat)),
            powers: Rows::default(),
            faction: factions::PLAYER,
            race: races::HUMAN,
            guards: Guards::default(),
            flags: 0,
            weapons: [
                Some(Weapon::plain(
                    12,
                    1,
                    6,
                    (1, 2),
                    damage_types::ENERGY,
                    [
                        feats::PROFICIENCY_BLASTER,
                        feats::WEAPON_FOCUS_BLASTER,
                        feats::WEAPON_SPEC_BLASTER,
                    ],
                )),
                Some(Weapon::plain(
                    5,
                    1,
                    10,
                    (2, 2),
                    damage_types::SLASHING,
                    [
                        feats::PROFICIENCY_MELEE,
                        feats::WEAPON_FOCUS_MELEE,
                        feats::WEAPON_SPEC_MELEE,
                    ],
                )),
            ],
            // Three fragmentation grenades, a thermal detonator, two
            // medpacs and an energy shield.
            items: [
                Some(Carried { row: 87, count: 3 }),
                Some(Carried { row: 89, count: 1 }),
                Some(Carried { row: 64, count: 2 }),
                Some(Carried { row: 99, count: 1 }),
            ],
            skills: [0; 8],
            gear: Gear::default(),
            attack: 0,
            good_evil: NEUTRAL,
            bonus_fp: 0,
            save_bonus: [0; 3],
            form: 0,
        }
    }

    /// A level-6 Jedi Guardian for trying the Force (WIS 16; 54 HP, 60
    /// FP; a lightsaber and a blaster pistol; Force Jump and Advanced Force
    /// Jump, as K1's Guardians have by level 6) who knows every chain the
    /// sim maps at its top (I22's table: push to Wave, lightning to Storm,
    /// Drain Life and Death Field, grip to Kill, speed, Cure and Heal,
    /// protect to Armor, absorb, mind trick, and Stun and Fear; KOTOR's
    /// effect powers at their top: Master Valor, Improved Energy
    /// Resistance, Plague, Force Breach; and TSL's Master Fury and Force
    /// Sight, castable where the module's table has them).
    pub fn dev_jedi() -> Sheet {
        let powers = [
            23, 27, 26, 43, 35, 25, 15, 11, 50, 9, 32, 8, 34, 37, 10, 28, 18, 24, 17, 41, 20, 6,
            14, 46, 16, 166, 176, 36, 40, 38, 19,
        ];
        let feats = [
            feats::PROFICIENCY_BLASTER,
            feats::PROFICIENCY_LIGHTSABER,
            feats::WEAPON_FOCUS_LIGHTSABER,
            feats::JEDI_DEFENSE[0],
            feats::FORCE_JUMP[0],
            feats::FORCE_JUMP[1],
        ];
        Sheet {
            abilities: [14, 14, 12, 10, 16, 12],
            level: 6,
            bab: 6,
            saves: [5, 5, 5],
            max_hp: 54,
            max_fp: 60,
            defence: 2,
            dex_cap: None,
            feats: feats
                .iter()
                .fold(Rows::default(), |rows, feat| rows.with(*feat)),
            powers: powers
                .iter()
                .fold(Rows::default(), |rows, power| rows.with(*power)),
            faction: factions::PLAYER,
            race: races::HUMAN,
            guards: Guards::default(),
            flags: 0,
            weapons: [
                Some(Weapon::plain(
                    8,
                    2,
                    8,
                    (2, 2),
                    damage_types::ENERGY,
                    [
                        feats::PROFICIENCY_LIGHTSABER,
                        feats::WEAPON_FOCUS_LIGHTSABER,
                        feats::WEAPON_SPEC_LIGHTSABER,
                    ],
                )),
                Some(Weapon::plain(
                    12,
                    1,
                    6,
                    (1, 2),
                    damage_types::ENERGY,
                    [
                        feats::PROFICIENCY_BLASTER,
                        feats::WEAPON_FOCUS_BLASTER,
                        feats::WEAPON_SPEC_BLASTER,
                    ],
                )),
            ],
            // Two fragmentation grenades and a medpac.
            items: [
                Some(Carried { row: 87, count: 2 }),
                Some(Carried { row: 64, count: 1 }),
                None,
                None,
            ],
            skills: [0; 8],
            gear: Gear::default(),
            attack: 0,
            good_evil: NEUTRAL,
            bonus_fp: 0,
            save_bonus: [0; 3],
            form: 0,
        }
    }

    pub(crate) fn encode(&self, out: &mut Writer) {
        out.bytes(&self.abilities);
        out.u8(self.level);
        out.u8(self.bab);
        for save in self.saves {
            out.i8(save);
        }
        out.u16(self.max_hp);
        out.u16(self.max_fp);
        out.i8(self.defence);
        out.u8(self.dex_cap.unwrap_or(u8::MAX));
        for word in self.feats.0.iter().chain(&self.powers.0) {
            out.u64(*word);
        }
        out.u8(self.faction);
        out.u8(self.race);
        for immunity in self.guards.immunity {
            out.i8(immunity);
        }
        out.bytes(&self.guards.resist);
        out.u8(self.flags);
        for weapon in &self.weapons {
            write_fixed(out, weapon.as_ref(), &Weapon::UNARMED, encode_weapon);
        }
        for carried in &self.items {
            write_fixed(out, carried.as_ref(), &Carried::BLANK, Carried::encode);
        }
        out.bytes(&self.skills);
        self.gear.encode(out);
        out.i8(self.attack);
        out.u8(self.good_evil);
        out.u16(self.bonus_fp as u16);
        for bonus in self.save_bonus {
            out.i8(bonus);
        }
        out.u16(self.form);
    }

    pub(crate) fn decode(input: &mut Reader) -> Result<Sheet, DecodeError> {
        let abilities = input.array()?;
        let level = input.u8()?;
        let bab = input.u8()?;
        let saves = [input.i8()?, input.i8()?, input.i8()?];
        let max_hp = input.u16()?;
        let max_fp = input.u16()?;
        let defence = input.i8()?;
        let dex_cap = Some(input.u8()?).filter(|cap| *cap != u8::MAX);
        let mut feats = Rows::<4>::default();
        for word in &mut feats.0 {
            *word = input.u64()?;
        }
        let mut powers = Rows::<5>::default();
        for word in &mut powers.0 {
            *word = input.u64()?;
        }
        let faction = input.u8()?;
        let race = input.u8()?;
        let mut guards = Guards::default();
        for immunity in &mut guards.immunity {
            *immunity = input.i8()?;
        }
        guards.resist = input.array()?;
        let flags = input.u8()?;
        let mut weapons = [None; 2];
        for weapon in &mut weapons {
            *weapon = read_fixed(input, &Weapon::UNARMED, decode_weapon, "weapon flag")?;
        }
        let mut items = [None; KINDS];
        for carried in &mut items {
            *carried = read_fixed(input, &Carried::BLANK, Carried::decode, "item flag")?;
        }
        let skills = input.array()?;
        let gear = Gear::decode(input)?;
        let attack = input.i8()?;
        let good_evil = input.u8()?;
        let bonus_fp = input.u16()? as i16;
        let save_bonus = [input.i8()?, input.i8()?, input.i8()?];
        let form = input.u16()?;
        Ok(Sheet {
            abilities,
            level,
            bab,
            saves,
            max_hp,
            max_fp,
            defence,
            dex_cap,
            feats,
            powers,
            faction,
            race,
            guards,
            flags,
            weapons,
            items,
            skills,
            gear,
            attack,
            good_evil,
            bonus_fp,
            save_bonus,
            form,
        })
    }
}

/// A feat row, or none (`u16::MAX`), in two bytes.
fn feat_of(value: u16) -> Option<u16> {
    (value != u16::MAX).then_some(value)
}

pub(crate) fn encode_weapon(weapon: &Weapon, out: &mut Writer) {
    out.u16(weapon.base_item);
    out.u8(weapon.delivery.index());
    out.bytes(&[
        weapon.dice,
        weapon.die,
        weapon.crit_threat,
        weapon.crit_mult,
        weapon.bonus_dice,
        weapon.bonus_die,
    ]);
    out.u16(weapon.damage_types);
    out.i8(weapon.attack_bonus);
    out.i8(weapon.damage_bonus);
    out.i8(weapon.deflect);
    for feat in [weapon.proficiency, weapon.focus, weapon.specialization] {
        out.u16(feat.unwrap_or(u16::MAX));
    }
    write_fixed(out, weapon.versus.as_ref(), &Versus::BLANK, Versus::encode);
    for extra in &weapon.extras {
        write_fixed(out, extra.as_ref(), &Extra::BLANK, Extra::encode);
    }
    weapon.massive.encode(out);
    for on_hit in &weapon.on_hit {
        write_fixed(out, on_hit.as_ref(), &OnHit::BLANK, OnHit::encode);
    }
    out.u8(weapon.mighty.unwrap_or(u8::MAX));
    out.bytes(&[weapon.enhancement, weapon.traits]);
    out.u8(weapon.reach_m);
    out.u8(weapon.build);
}

pub(crate) fn decode_weapon(input: &mut Reader) -> Result<Weapon, DecodeError> {
    let base_item = input.u16()?;
    let at = input.at();
    let delivery = Delivery::from_index(input.u8()?).ok_or(input.invalid("weapon delivery", at))?;
    let [dice, die, crit_threat, crit_mult, bonus_dice, bonus_die] = input.array()?;
    Ok(Weapon {
        base_item,
        delivery,
        dice,
        die,
        crit_threat,
        crit_mult,
        bonus_dice,
        bonus_die,
        damage_types: input.u16()?,
        attack_bonus: input.i8()?,
        damage_bonus: input.i8()?,
        deflect: input.i8()?,
        proficiency: feat_of(input.u16()?),
        focus: feat_of(input.u16()?),
        specialization: feat_of(input.u16()?),
        versus: read_fixed(input, &Versus::BLANK, Versus::decode, "versus flag")?,
        extras: {
            let mut extras = [None; EXTRAS];
            for extra in &mut extras {
                *extra = read_fixed(input, &Extra::BLANK, Extra::decode, "extra flag")?;
            }
            extras
        },
        massive: Roll::decode(input)?,
        on_hit: {
            let mut on_hit = [None; ON_HITS];
            for one in &mut on_hit {
                *one = read_fixed(input, &OnHit::BLANK, OnHit::decode, "on-hit flag")?;
            }
            on_hit
        },
        mighty: Some(input.u8()?).filter(|most| *most != u8::MAX),
        enhancement: input.u8()?,
        traits: input.u8()?,
        reach_m: input.u8()?,
        build: input.u8()?,
    })
}

/// Standard factions (`repute.2da` rows) combat names.
pub mod factions {
    pub const HOSTILE_1: u8 = 1;
    pub const FRIENDLY_1: u8 = 2;
    pub const HOSTILE_2: u8 = 3;
    pub const FRIENDLY_2: u8 = 4;
    pub const NEUTRAL: u8 = 5;
    pub const INSANE: u8 = 6;
    /// The player's own faction row.
    pub const PLAYER: u8 = 0;
}
