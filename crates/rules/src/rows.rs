//! Rows by label: the two numberings of `spells.2da` and `feat.2da`.
//!
//! The install number of a row is its index in the install's table, as
//! the game folder has it with every mod a patcher installed. The game's
//! own scripts, a mod's scripts, creature templates and item templates
//! speak install numbers. A patcher such as TSLPatcher or HoloPatcher
//! appends a mod's rows to the tables and compiles the mod's scripts with
//! the appended rows' numbers in them.
//!
//! The canonical number of a row is the number kjk's own rules, a
//! character sheet in memory, a snapshot and kjk's own save format use.
//! The engine names some rows by number (`spells::NAMED`, `feats::NAMED`),
//! and those numbers are KOTOR 2's own.
//!
//! The map from one numbering to the other (`RowMap::build`) depends only
//! on the install's table as the resolver serves it, after KOTOR 2's rows
//! were lent to a KOTOR 1 install. It gives each install row its canonical
//! number by these rules, in this order:
//!
//! 1. A row whose label is one the engine names gets that name's number.
//!    When two rows have the same label, the first row gets the number,
//!    and the later row is treated as a row the engine does not name.
//! 2. A row lent from KOTOR 2 (`vfs::lent`) gets its KOTOR 2 row number.
//! 3. Every other row keeps its own number when no row of rule 1 or 2 has
//!    it, and when it is not in the reserved range. For KOTOR 1 the
//!    reserved range runs from KOTOR 1's own row count to KOTOR 2's own row
//!    count (132 to 281 for powers, 125 to 244 for feats), since those
//!    numbers are KOTOR 2's rows. KOTOR 2 has no reserved range.
//! 4. Each row that rule 3 does not let keep its number is moved past
//!    KOTOR 2's own table. The moved rows take the lowest numbers from
//!    KOTOR 2's own row count up that no other row has, one after another
//!    in install order. A row that would get 512 or more gets no number,
//!    and the map reports it.
//! 5. A row with a blank label is a blank row, as a table has between rows
//!    it uses. It keeps its own number when rule 3 lets it, and otherwise
//!    gets no number. A blank row is never moved.
//!
//! The map is the identity on an unmodded KOTOR 1 install, on an unmodded
//! KOTOR 2 install, and on a KOTOR 2 install whose mods only add rows after
//! KOTOR 2's own: every named row is at its own number there, no row lies
//! in a reserved range that rule 2 does not give to it, and so every row
//! keeps its number.
//!
//! The rules are simple on purpose. Rule 4 never looks at another install
//! or at anything but the table, so two machines with the same tables get
//! the same map. Moving rows only past KOTOR 2's own table means that a
//! number of KOTOR 2's own rows never stands for a mod's row.

use std::collections::BTreeMap;

/// How many rows a set of known powers or feats can hold
/// (`sim::combat::sheet::Powers`): a canonical number is always below it.
pub const MAX_ROWS: u16 = 512;

/// The column a table served in canonical order (`vfs::rows`) holds each
/// row's install number in. It is blank for a canonical number no install
/// row has.
pub const INSTALL_COLUMN: &str = "kjk_install_row";

/// Which table a row number counts rows of.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Table {
    /// `spells.2da`: the Force powers, the item abilities and the grenades.
    Spells,
    /// `feat.2da`.
    Feats,
}

impl Table {
    pub const ALL: [Table; 2] = [Table::Spells, Table::Feats];

    /// The table's resource name.
    pub fn name(self) -> &'static str {
        match self {
            Table::Spells => "spells",
            Table::Feats => "feat",
        }
    }

    /// The rows the engine names, by label.
    pub fn named(self) -> &'static [(&'static str, u16)] {
        match self {
            Table::Spells => spells::NAMED,
            Table::Feats => feats::NAMED,
        }
    }

    /// How many rows KOTOR 1's own table has.
    pub fn kotor1_rows(self) -> u16 {
        match self {
            Table::Spells => spells::K1_ROWS,
            Table::Feats => feats::K1_ROWS,
        }
    }

    /// How many rows KOTOR 2's own table has.
    pub fn kotor2_rows(self) -> u16 {
        match self {
            Table::Spells => spells::K2_ROWS,
            Table::Feats => feats::K2_ROWS,
        }
    }

    /// The numbers of KOTOR 2's own table that a KOTOR 1 install's own rows
    /// do not reach. They are kept for KOTOR 2's rows (rule 3). KOTOR 2
    /// keeps none.
    pub fn reserved(self, kotor2: bool) -> std::ops::Range<u16> {
        match kotor2 {
            true => self.kotor2_rows()..self.kotor2_rows(),
            false => self.kotor1_rows()..self.kotor2_rows(),
        }
    }
}

/// One install row as the map is built from it (`RowMap::build`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InstallRow<'a> {
    /// The row's label, none when it is blank (`****` or nothing).
    pub label: Option<&'a str>,
    /// The KOTOR 2 row number of a row lent from KOTOR 2.
    pub lent: Option<u16>,
}

/// The map between the install numbers and the canonical numbers of one
/// table. The default map is the identity.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RowMap {
    /// Each install row's canonical number, by install row. Empty for the
    /// identity.
    canonical: Vec<Option<u16>>,
    /// Each canonical number's install row, by canonical number. Empty for
    /// the identity.
    install: Vec<Option<u16>>,
}

impl RowMap {
    /// The map of a table whose rows are `rows`, in install order, by the
    /// rules of the module comment. `kotor2` says whether the install is
    /// KOTOR 2's. The lines say which rows got no number for want of room.
    pub fn build(table: Table, kotor2: bool, rows: &[InstallRow]) -> (RowMap, Vec<String>) {
        let reserved = table.reserved(kotor2);
        let mut canonical: Vec<Option<u16>> = vec![None; rows.len()];
        let mut taken = vec![false; usize::from(MAX_ROWS)];
        fn take(number: u16, taken: &mut [bool]) -> bool {
            let free = taken.get(usize::from(number)) == Some(&false);
            if free {
                taken[usize::from(number)] = true;
            }
            free
        }
        // Rules 1 and 2: the named rows, then the lent rows.
        let named: BTreeMap<&str, u16> = table.named().iter().copied().collect();
        for (at, row) in rows.iter().enumerate() {
            let number = row.label.and_then(|label| named.get(label).copied());
            if let Some(number) = number.filter(|&number| take(number, &mut taken)) {
                canonical[at] = Some(number);
            }
        }
        for (at, row) in rows.iter().enumerate() {
            if canonical[at].is_none()
                && let Some(number) = row.lent.filter(|&number| take(number, &mut taken))
            {
                canonical[at] = Some(number);
            }
        }
        // Rules 3 and 5: the rows that keep their own number.
        let mut moved = Vec::new();
        for (at, row) in rows.iter().enumerate() {
            if canonical[at].is_some() {
                continue;
            }
            let own = u16::try_from(at).ok();
            match own.filter(|own| !reserved.contains(own) && take(*own, &mut taken)) {
                Some(own) => canonical[at] = Some(own),
                None if row.label.is_some() => moved.push(at),
                None => {}
            }
        }
        // Rule 4: the moved rows, past KOTOR 2's own table.
        let mut problems = Vec::new();
        let mut next = table.kotor2_rows();
        for at in moved {
            while next < MAX_ROWS && !take(next, &mut taken) {
                next += 1;
            }
            if next < MAX_ROWS {
                canonical[at] = Some(next);
            } else {
                problems.push(format!(
                    "{}.2da row {at} ({}) gets no number: every number up to {MAX_ROWS} is taken",
                    table.name(),
                    rows[at].label.unwrap_or("")
                ));
            }
        }
        let identity = canonical.iter().enumerate().all(|(at, number)| {
            *number == u16::try_from(at).ok() || (number.is_none() && rows[at].label.is_none())
        });
        if identity {
            return (RowMap::default(), problems);
        }
        (RowMap::from_canonical(canonical), problems)
    }

    /// The map that gives each install row the canonical number at its
    /// index.
    fn from_canonical(canonical: Vec<Option<u16>>) -> RowMap {
        let size = canonical
            .iter()
            .flatten()
            .max()
            .map_or(0, |&last| usize::from(last) + 1);
        let mut install = vec![None; size];
        for (at, number) in canonical.iter().enumerate() {
            if let Some(number) = number {
                install[usize::from(*number)] = u16::try_from(at).ok();
            }
        }
        RowMap { canonical, install }
    }

    /// The map of a table served in canonical order, from each canonical
    /// number's install row (its `INSTALL_COLUMN`).
    pub fn from_install_rows(install: Vec<Option<u16>>) -> RowMap {
        let size = install
            .iter()
            .flatten()
            .max()
            .map_or(0, |&last| usize::from(last) + 1);
        let mut canonical = vec![None; size];
        for (number, row) in install.iter().enumerate() {
            if let Some(row) = row {
                canonical[usize::from(*row)] = u16::try_from(number).ok();
            }
        }
        RowMap { canonical, install }
    }

    /// Whether every row keeps its own number.
    pub fn is_identity(&self) -> bool {
        self.canonical.is_empty() && self.install.is_empty()
    }

    /// The canonical number of install row `row`: the same number for the
    /// identity, none for a row with no number.
    pub fn canonical(&self, row: u16) -> Option<u16> {
        match self.is_identity() {
            true => Some(row),
            false => self.canonical.get(usize::from(row)).copied().flatten(),
        }
    }

    /// The install row of canonical number `number`: the same number for
    /// the identity, none for a number no install row has.
    pub fn install(&self, number: u16) -> Option<u16> {
        match self.is_identity() {
            true => Some(number),
            false => self.install.get(usize::from(number)).copied().flatten(),
        }
    }

    /// How many install rows the map numbers (0 for the identity).
    pub fn install_rows(&self) -> usize {
        self.canonical.len()
    }

    /// How many canonical numbers the map reaches (0 for the identity).
    pub fn canonical_rows(&self) -> usize {
        self.install.len()
    }

    /// Each canonical number's install row, by canonical number (empty
    /// for the identity).
    pub fn by_canonical(&self) -> &[Option<u16>] {
        &self.install
    }

    /// A number a script or a template gave as an install row, as a
    /// canonical number. A number outside the install's table is no row of
    /// it, and it is kept as it is (-1 for none, a script's own constants
    /// past the table). A row of the table without a number reads as -1.
    pub fn value_to_canonical(&self, value: i32) -> i32 {
        Self::translate(value, self.canonical.len(), |row| self.canonical(row))
    }

    /// A canonical number as the install row a script expects, by the
    /// same rule as `value_to_canonical`.
    pub fn value_to_install(&self, value: i32) -> i32 {
        Self::translate(value, self.install.len(), |number| self.install(number))
    }

    fn translate(value: i32, rows: usize, map: impl Fn(u16) -> Option<u16>) -> i32 {
        match u16::try_from(value) {
            Ok(row) if usize::from(row) < rows => map(row).map_or(-1, i32::from),
            _ => value,
        }
    }

    /// The map's numbers, for a digest: each install row's canonical
    /// number (`u16::MAX` for none), little-endian.
    pub fn bytes(&self) -> Vec<u8> {
        self.canonical
            .iter()
            .flat_map(|number| number.unwrap_or(u16::MAX).to_le_bytes())
            .collect()
    }
}

/// The maps of both tables of one install (`vfs::rows`).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RowMaps {
    pub spells: RowMap,
    pub feats: RowMap,
}

impl RowMaps {
    pub fn get(&self, table: Table) -> &RowMap {
        match table {
            Table::Spells => &self.spells,
            Table::Feats => &self.feats,
        }
    }

    pub fn get_mut(&mut self, table: Table) -> &mut RowMap {
        match table {
            Table::Spells => &mut self.spells,
            Table::Feats => &mut self.feats,
        }
    }

    pub fn is_identity(&self) -> bool {
        self.spells.is_identity() && self.feats.is_identity()
    }

    /// Both maps' numbers, for a digest.
    pub fn bytes(&self) -> Vec<u8> {
        let mut bytes = Vec::new();
        for map in [&self.spells, &self.feats] {
            bytes.extend((map.canonical.len() as u32).to_le_bytes());
            bytes.extend(map.bytes());
        }
        bytes
    }
}

/// The `spells.2da` rows the engine names. See `sim::combat::spells`.
pub mod spells {
    /// How many rows KOTOR 1's own `spells.2da` has.
    pub const K1_ROWS: u16 = 132;
    /// How many rows KOTOR 2's own `spells.2da` has.
    pub const K2_ROWS: u16 = 282;

    /// Declares each row as a constant named by its label, and `NAMED`:
    /// every one, by label.
    macro_rules! rows {
        ($($label:ident = $row:literal),* $(,)?) => {
            $(pub const $label: u16 = $row;)*

            /// Every row named here, with its label.
            pub const NAMED: &[(&str, u16)] = &[$((stringify!($label), $label)),*];
        };
    }

    rows! {
        // Force powers (`usertype` 1).
        FORCE_POWER_LIGHT_SABER_THROW_ADVANCED = 4,
        FORCE_POWER_AFFECT_MIND = 6,
        FORCE_POWER_AFFLICTION = 7,
        FORCE_POWER_SPEED_BURST = 8,
        FORCE_POWER_CHOKE = 9,
        FORCE_POWER_CURE = 10,
        FORCE_POWER_DEATH_FIELD = 11,
        FORCE_POWER_DROID_DISABLE = 12,
        FORCE_POWER_DROID_DESTROY = 13,
        FORCE_POWER_DOMINATE = 14,
        FORCE_POWER_DRAIN_LIFE = 15,
        FORCE_POWER_FEAR = 16,
        FORCE_POWER_FORCE_ARMOR = 17,
        FORCE_POWER_FORCE_AURA = 18,
        FORCE_POWER_FORCE_BREACH = 19,
        FORCE_POWER_FORCE_IMMUNITY = 20,
        FORCE_POWER_FORCE_VALOR = 22,
        FORCE_POWER_FORCE_PUSH = 23,
        FORCE_POWER_FORCE_SHIELD = 24,
        FORCE_POWER_FORCE_STORM = 25,
        FORCE_POWER_FORCE_WAVE = 26,
        FORCE_POWER_FORCE_WHIRLWIND = 27,
        FORCE_POWER_HEAL = 28,
        FORCE_POWER_STASIS = 29,
        FORCE_POWER_HORROR = 30,
        FORCE_POWER_INSANITY = 31,
        FORCE_POWER_KILL = 32,
        FORCE_POWER_KNIGHT_VALOR = 33,
        FORCE_POWER_KNIGHT_SPEED = 34,
        FORCE_POWER_LIGHTNING = 35,
        FORCE_POWER_MASTER_VALOR = 36,
        FORCE_POWER_SPEED_MASTERY = 37,
        FORCE_POWER_PLAGUE = 38,
        FORCE_POWER_RESIST_ENERGY_2 = 40,
        FORCE_POWER_RESIST_FORCE = 41,
        FORCE_POWER_RESIST_ENERGY_1 = 42,
        FORCE_POWER_SHOCK = 43,
        FORCE_POWER_MASS_STASIS = 44,
        FORCE_POWER_SLOW = 45,
        FORCE_POWER_STUN = 46,
        FORCE_POWER_DROID_STUN = 47,
        FORCE_POWER_SUPRESS_FORCE = 48,
        FORCE_POWER_LIGHT_SABER_THROW = 49,
        FORCE_POWER_WOUND = 50,
        // TSL's own powers.
        FORCE_POWER_MASTER_HEAL = 134,
        FORCE_POWER_FORCE_BARRIER = 135,
        FORCE_POWER_BATTLE_MEDITATION_PC = 138,
        FORCE_POWER_FORCE_BODY = 150,
        FORCE_POWER_DRAIN_FORCE = 153,
        FORCE_POWER_IMPROVED_DRAIN_FORCE = 154,
        FORCE_POWER_MASTER_DRAIN_FORCE = 155,
        FORCE_POWER_FORCE_SCREAM = 159,
        FORCE_POWER_IMPROVED_FORCE_SCREAM = 160,
        FORCE_POWER_MASTER_FORCE_SCREAM = 161,
        // TSL shows rows 162 and 163 as Force Deflection and Force
        // Redirection. Both are always in effect and have no impact script.
        FORCE_POWER_FORCE_REPULSION = 162,
        FORCE_POWER_FORCE_REDIRECTION = 163,
        FORCE_POWER_FURY = 164,
        FORCE_POWER_IMPROVED_FURY = 165,
        FORCE_POWER_MASTER_FURY = 166,
        FORCE_POWER_REVITALIZE = 173,
        FORCE_POWER_IMPROVED_REVITALIZE = 174,
        FORCE_POWER_MASTER_REVITALIZE = 175,
        FORCE_POWER_FORCE_SIGHT = 176,
        FORCE_POWER_FORCE_CRUSH = 177,
        FORCE_POWER_MIND_TRICK = 181,
        FORCE_POWER_CONFUSION = 200,
        // Item abilities (`usertype` 4).
        ITEM_ABILITY_MED_PACK = 64,
        ITEM_ABILITY_ADVANCED_MED_PACK = 65,
        ITEM_ABILITY_LIFE_SUPPORT_KIT = 66,
        ITEM_ABILITY_REMOVE_POISON = 67,
        ITEM_ABILITY_ADRENAL_SHOT_STRENGTH = 69,
        ITEM_ABILITY_ADRENAL_SHOT_ALACRITY = 70,
        ITEM_ABILITY_ADRENAL_SHOT_STAMINA = 71,
        ITEM_ABILITY_HYPER_ADRENAL_SHOT_STRENGTH = 72,
        ITEM_ABILITY_HYPER_ADRENAL_SHOT_ALACRITY = 73,
        ITEM_ABILITY_HYPER_ADRENAL_SHOT_STAMINA = 74,
        ITEM_ABILITY_COMBAT_SHOT_1 = 75,
        ITEM_ABILITY_COMBAT_SHOT_2 = 76,
        ITEM_ABILITY_COMBAT_SHOT_3 = 77,
        ITEM_ABILITY_DROID_REPAIR_KIT = 84,
        ITEM_ABILITY_GRENADE_FRAGMENTATION = 87,
        ITEM_ABILITY_GRENADE_STUN = 88,
        ITEM_ABILITY_GRENADE_THERMAL_DETONATOR = 89,
        ITEM_ABILITY_GRENADE_POISON = 90,
        ITEM_ABILITY_GRENADE_SONIC = 91,
        ITEM_ABILITY_GRENADE_ADHESIVE = 92,
        ITEM_ABILITY_GRENADE_CRYOBAN = 93,
        ITEM_ABILITY_GRENADE_FIRE = 94,
        ITEM_ABILITY_GRENADE_ION = 95,
        ITEM_ABILITY_ENERGY_SHIELD = 99,
        ITEM_ABILITY_SITH_ENERGY_SHIELD = 100,
        ITEM_ABILITY_ARKANIAN_ENERGY_SHIELD = 101,
        ITEM_ABILITY_ECHANI_SHIELD = 102,
        ITEM_ABILITY_MANDLORIAN_MELEE_SHIELD = 103,
        ITEM_ABILITY_MANDLORIAN_POWER_SHIELD = 104,
        ITEM_ABILITY_ECHANI_DUELING_SHIELD = 105,
        ITEM_ABILITY_YUSANIS_DUELING_SHIELD = 106,
        ITEM_ABILITY_VERPINE_ENERGY_SHIELD = 107,
        DROID_ITEM_ENERGY_SHIELD_1 = 110,
        DROID_ITEM_ENERGY_SHIELD_2 = 111,
        DROID_ITEM_ENERGY_SHIELD_3 = 112,
        DROID_ITEM_HAZARD_SHIELD_1 = 113,
        DROID_ITEM_HAZARD_SHIELD_2 = 114,
        DROID_ITEM_HAZARD_SHIELD_3 = 115,
        ITEM_ABILITY_DROID_REPAIR_KIT_ADV = 127,
        ITEM_ABILITY_DROID_CONSTRUCTION_KIT = 128,
        ITEM_ABILITY_RECOVERY_STIM = 129,
        // TSL's other forearm bands (`k_sup_bands`): the heat shield (the
        // Telos mining shield), and Drexl's below.
        ITEM_ABILITY_HEAT_SHIELD = 132,
        // TSL's stronger sonic grenades and its launchers' rows: the
        // rockets and darts a wrist launcher alone fires, and the grenades
        // it fires.
        ITEM_ABILITY_ROCKET_EXPLOSIVE = 185,
        ITEM_ABILITY_GRENADE_SONIC_2 = 186,
        ITEM_ABILITY_GRENADE_SONIC_3 = 187,
        ITEM_ABILITY_ROCKET_PIERCING = 190,
        ITEM_ABILITY_ROCKET_POISON = 191,
        ITEM_ABILITY_ROCKET_ION = 192,
        ITEM_ABILITY_ROCKET_PLASMA = 193,
        ITEM_ABILITY_ROCKET_CONCUSSION = 194,
        ITEM_ABILITY_DART_POISON = 195,
        ITEM_ABILITY_DART_KYBER = 196,
        ITEM_ABILITY_DART_PARALYSIS = 197,
        ITEM_ABILITY_DART_TRANQUILIZER = 198,
        ITEM_ABILITY_DART_LAWN = 199,
        ITEM_ABILITY_GRENADE_LAUNCHED_FRAGMENTATION = 246,
        ITEM_ABILITY_GRENADE_LAUNCHED_STUN = 247,
        ITEM_ABILITY_GRENADE_LAUNCHED_THERMAL_DETONATOR = 248,
        ITEM_ABILITY_GRENADE_LAUNCHED_POISON = 249,
        ITEM_ABILITY_GRENADE_LAUNCHED_SONIC = 250,
        ITEM_ABILITY_GRENADE_LAUNCHED_ADHESIVE = 251,
        ITEM_ABILITY_GRENADE_LAUNCHED_CRYOBAN = 252,
        ITEM_ABILITY_GRENADE_LAUNCHED_FIRE = 253,
        ITEM_ABILITY_GRENADE_LAUNCHED_ION = 254,
        ITEM_ABILITY_GRENADE_LAUNCHED_SONIC_2 = 255,
        ITEM_ABILITY_GRENADE_LAUNCHED_SONIC_3 = 256,
        ITEM_ABILITY_DREXL_SHIELD = 257,
    }
}

/// The `feat.2da` rows the engine names (`sim::combat::sheet::feats`), by
/// label. A row whose label differs between the games is named by both
/// labels: KOTOR 2's first, then KOTOR 1's.
pub mod feats {
    /// How many rows KOTOR 1's own `feat.2da` has.
    pub const K1_ROWS: u16 = 125;
    /// How many rows KOTOR 2's own `feat.2da` has.
    pub const K2_ROWS: u16 = 245;

    pub const NAMED: &[(&str, u16)] = &[
        ("ADVANCED_JEDI_DEFENSE", 1),
        ("TWO_WEAPON_FIGHTING", 3),
        ("ARMOUR_PROF_HEAVY", 4),
        ("ARMOUR_PROF_LIGHT", 5),
        ("ARMOUR_PROF_MEDIUM", 6),
        ("CAUTIOUS", 7),
        ("CRITICAL_STRIKE", 8),
        ("TWO_WEAPON_ADVANCED", 9),
        ("EMPATHY", 10),
        ("FLURRY", 11),
        ("GEAR_HEAD", 12),
        ("CONDITIONING", 13),
        ("IMPROVED_POWER_ATTACK", 17),
        ("IMPROVED_POWER_BLAST", 18),
        ("IMPROVED_CRITICAL_STRIKE", 19),
        ("IMPROVED_SNIPER_SHOT", 20),
        ("IMPROVED_CONDITIONING", 21),
        ("MASTER_CONDITIONING", 22),
        ("MASTER_JEDI_DEFENSE", 24),
        ("MULTI_SHOT", 26),
        ("XXXX_PERCEPTIVE", 27),
        ("POWER_ATTACK", 28),
        ("POWER_BLAST", 29),
        ("RAPID_SHOT", 30),
        ("SNIPER_SHOT", 31),
        ("WEAPON_FOCUS_BLASTER", 32),
        ("WEAPON_FOCUS_BLASTER_RIFLE", 33),
        ("XXXX_WEAPON_FOCUS_HEAVY_WEAPONS", 35),
        ("WEAPON_FOCUS_HEAVY_WEAPONS", 35),
        ("WEAPON_FOCUS_LIGHTSABER", 36),
        ("WEAPON_FOCUS_MELEE_WEAPONS", 37),
        ("WEAPON_PROF_BLASTER", 39),
        ("WEAPON_PROF_BLASTER_RIFLE", 40),
        ("XXXX_WEAPON_PROF_GRENADE", 41),
        ("XXXX_WEAPON_PROF_HEAVY_WEAPONS", 42),
        ("WEAPON_PROF_HEAVY_WEAPONS", 42),
        ("WEAPON_PROF_LIGHTSABER", 43),
        ("WEAPON_PROF_MELEE_WEAPONS", 44),
        ("XXXX_WEAPON_PROF_SIMPLE_WEAPONS", 45),
        ("WEAPON_SPEC_BLASTER", 46),
        ("WEAPON_SPEC_BLASTER_RIFLE", 47),
        ("XXXX_WEAPON_SPEC_HEAVY_WEAPONS", 49),
        ("WEAPON_SPEC_HEAVY_WEAPONS", 49),
        ("WEAPON_SPEC_LIGHTSABER", 50),
        ("WEAPON_SPEC_MELEE_WEAPONS", 51),
        ("WHIRLWIND_ATTACK", 53),
        ("JEDI_DEFENSE", 55),
        ("UNCANNY_DODGE_1", 56),
        ("UNCANNY_DODGE_2", 57),
        ("XXXX_SKILL_FOCUS_COMPUTER_USE", 58),
        ("SNEAK_ATTACK_1D6", 60),
        ("SNEAK_ATTACK_2D6", 61),
        ("SNEAK_ATTACK_3D6", 62),
        ("SNEAK_ATTACK_4D6", 63),
        ("SNEAK_ATTACK_5D6", 64),
        ("SNEAK_ATTACK_6D6", 65),
        ("SNEAK_ATTACK_7D6", 66),
        ("SNEAK_ATTACK_8D6", 67),
        ("SNEAK_ATTACK_9D6", 68),
        ("SNEAK_ATTACK_10D6", 69),
        ("XXXX_SKILL_FOCUS_DEMOLITIONS", 70),
        ("XXXX_SKILL_FOCUS_STEALTH", 71),
        ("XXXX_SKILL_FOCUS_AWARENESS", 72),
        ("XXXX_SKILL_FOCUS_PERSUADE", 73),
        ("XXXX_SKILL_FOCUS_REPAIR", 74),
        ("XXXX_SKILL_FOCUS_SECURITY", 75),
        ("XXXX_SKILL_FOCUS_TREAT_INJURY", 76),
        ("MASTER_SNIPER_SHOT", 77),
        ("MASTER_CRITICAL_STRIKE", 81),
        ("MASTER_POWER_BLAST", 82),
        ("MASTER_POWER_ATTACK", 83),
        ("TOUGHNESS", 84),
        ("TWO_WEAPON_MASTERY", 85),
        ("FORCE_FOCUS", 88),
        ("FORCE_FOCUS_ADVANCED", 89),
        ("FORCE_FOCUS_MASTERY", 90),
        ("IMPROVED_FLURRY", 91),
        ("IMPROVED_RAPID_SHOT", 92),
        ("PROFICIENCY_ALL", 93),
        ("WOOKIEE_TOUGHNESS", 95),
        ("WOOKIE_ENDURANCE", 95),
        ("FORCE_IMMUNITY_FEAR", 98),
        ("FORCE_IMMUNITY_STUN", 99),
        ("FORCE_IMMUNITY_PARALYSIS", 100),
        ("FORCE_JUMP", 101),
        ("FORCE_JUMP_ADVANCED", 102),
        ("FORCE_JUMP_MASTERY", 103),
        ("DUELING", 113),
        ("ADVANCED_DUELING", 114),
        ("MASTER_DUELING", 115),
        ("FORCE_SENSITIVE", 116),
        ("IMPROVED_CAUTION", 117),
        ("MASTER_CAUTION", 118),
        ("GEAR_HEAD_ADEPT", 119),
        ("GEAR_HEAD_MASTER", 120),
        ("IMPROVED_EMPATHY", 121),
        ("MASTER__EMPATHY", 122),
        ("IMPROVED_TOUGHNESS", 123),
        ("MASTER_TOUGHNESS", 124),
        ("EVASION", 125),
        ("TARGETING_1", 126),
        ("TARGETING_2", 127),
        ("TARGETING_3", 128),
        ("TARGETING_4", 129),
        ("TARGETING_5", 130),
        ("TARGETING_6", 131),
        ("TARGETING_7", 132),
        ("TARGETING_8", 133),
        ("CLOSE_COMBAT", 139),
        ("IMPROVED_CLOSE_COMBAT", 140),
        ("REGENERATE_FORCE_POINTS", 143),
        ("DUAL_STRIKE", 190),
        ("IMPROVED_DUAL_STRIKE", 191),
        ("MASTER_DUAL_STRIKE", 192),
        ("FINESSE_LIGHTSABERS", 193),
        ("FINESSE_MELEE_WEAPONS", 194),
        ("REGENERATE_VITALITY_POINTS", 196),
        ("STEALTH_RUN", 197),
        ("UNARMED_SPECIALIST_I", 212),
        ("UNARMED_SPECIALIST_II", 213),
        ("UNARMED_SPECIALIST_III", 214),
        ("UNARMED_SPECIALIST_IV", 215),
        ("UNARMED_SPECIALIST_V", 216),
        ("UNARMED_SPECIALIST_VI", 217),
        ("UNARMED_SPECIALIST_VII", 218),
        ("UNARMED_SPECIALIST_VIII", 219),
        ("SHIELD_BREAKER", 220),
        ("WOOKIEE_TOUGHNESS_II", 224),
        ("WOOKIEE_TOUGHNESS_III", 225),
        ("MINE_IMMUNITY", 238),
        ("POINT_GUARD", 239),
        ("PRECISE_SHOT_I", 240),
        ("PRECISE_SHOT_II", 241),
        ("PRECISE_SHOT_III", 242),
        ("PRECISE_SHOT_IV", 243),
        ("PRECISE_SHOT_V", 244),
    ];
}
