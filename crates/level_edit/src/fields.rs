//! The fields of a creature's, a store's and an encounter's template that
//! the editor changes beyond its scripts: its numbers (`NUMBERS`), its name,
//! and its lists (a creature's classes, powers, feats, skills and
//! equipment, a store's stock, an encounter's creatures).
//!
//! Each field here is one kjk reads when it makes the object
//! (`session::sheets` for a creature, `session::items` for a store's stock
//! and terms, `asset_odyssey::module`'s `EncounterIr` for an encounter), so
//! a change shows in play.

use fmt_odyssey::ResType;
use fmt_odyssey::gff::{Gff, GffField, GffStruct, GffValue, LocString};

use crate::git::RESREF_LEN;

/// How a number is stored in a template.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Store {
    Byte,
    Word,
    Short,
    Int,
    Dword,
    Float,
}

/// A number of a template the editor changes.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Number {
    /// The template's field.
    pub label: &'static str,
    /// What the editor calls it.
    pub shown: &'static str,
    /// How it is stored where the template does not have it yet.
    pub store: Store,
    pub min: f64,
    pub max: f64,
    /// The 2DA whose rows it names (`appearance`, `repute`), if any.
    pub table: Option<&'static str>,
    /// A switch (0 or 1), shown as a check box.
    pub switch: bool,
    pub hint: &'static str,
}

const fn number(
    label: &'static str,
    shown: &'static str,
    store: Store,
    (min, max): (f64, f64),
    hint: &'static str,
) -> Number {
    Number {
        label,
        shown,
        store,
        min,
        max,
        table: None,
        switch: false,
        hint,
    }
}

const fn row(
    label: &'static str,
    shown: &'static str,
    store: Store,
    table: &'static str,
    hint: &'static str,
) -> Number {
    Number {
        label,
        shown,
        store,
        min: 0.0,
        max: 65535.0,
        table: Some(table),
        switch: false,
        hint,
    }
}

const fn switch(label: &'static str, shown: &'static str, hint: &'static str) -> Number {
    Number {
        label,
        shown,
        store: Store::Byte,
        min: 0.0,
        max: 1.0,
        table: None,
        switch: true,
        hint,
    }
}

/// A creature's numbers.
pub const CREATURE: [Number; 22] = [
    row(
        "Appearance_Type",
        "Appearance",
        Store::Word,
        "appearance",
        "The creature's body: its model, its size and how fast it moves.",
    ),
    row(
        "FactionID",
        "Faction",
        Store::Word,
        "repute",
        "The side the creature is on. Creatures of hostile factions fight each other.",
    ),
    row(
        "Race",
        "Race",
        Store::Byte,
        "racialtypes",
        "The creature's race. A droid is immune to poison and to some Force powers.",
    ),
    number(
        "Str",
        "Strength",
        Store::Byte,
        (1.0, 100.0),
        "Adds to melee attacks and damage.",
    ),
    number(
        "Dex",
        "Dexterity",
        Store::Byte,
        (1.0, 100.0),
        "Adds to defence and to ranged attacks.",
    ),
    number(
        "Con",
        "Constitution",
        Store::Byte,
        (1.0, 100.0),
        "Adds hit points for each level.",
    ),
    number(
        "Int",
        "Intelligence",
        Store::Byte,
        (1.0, 100.0),
        "Adds to skills.",
    ),
    number(
        "Wis",
        "Wisdom",
        Store::Byte,
        (1.0, 100.0),
        "Adds Force points and to Force power saves.",
    ),
    number(
        "Cha",
        "Charisma",
        Store::Byte,
        (1.0, 100.0),
        "Adds Force points and to persuasion.",
    ),
    number(
        "HitPoints",
        "Base hit points",
        Store::Short,
        (1.0, 32767.0),
        "The creature's hit points before its Constitution adds to them.",
    ),
    number(
        "CurrentHitPoints",
        "Hit points now",
        Store::Short,
        (1.0, 32767.0),
        "The hit points it starts with, before its Constitution. Less than its base hit points makes it start hurt.",
    ),
    number(
        "MaxHitPoints",
        "Most hit points",
        Store::Short,
        (1.0, 32767.0),
        "The most hit points it can have, with its Constitution.",
    ),
    number(
        "ForcePoints",
        "Force points",
        Store::Short,
        (0.0, 32767.0),
        "The Force points it has to cast powers with.",
    ),
    number(
        "NaturalAC",
        "Natural defence",
        Store::Byte,
        (0.0, 100.0),
        "Defence it has without armour.",
    ),
    number(
        "ChallengeRating",
        "Challenge rating",
        Store::Float,
        (0.0, 100.0),
        "How hard the creature is to beat. The experience it gives is reckoned from it.",
    ),
    number(
        "WalkRate",
        "Speed",
        Store::Int,
        (0.0, 7.0),
        "Its row of creaturespeed.2da: 0 the player's, 1 not moving, 2 very slow to 6 very fast, 7 its appearance's speed.",
    ),
    number(
        "PerceptionRange",
        "Senses",
        Store::Byte,
        (0.0, 20.0),
        "Its row of ranges.2da: how far it sees and hears.",
    ),
    number(
        "GoodEvil",
        "Alignment",
        Store::Byte,
        (0.0, 100.0),
        "0 is wholly dark side, 50 neutral and 100 wholly light side.",
    ),
    switch("Plot", "Cannot be hurt", "A plot creature takes no damage."),
    switch(
        "Min1HP",
        "Never drops below 1 hit point",
        "Damage stops at 1 hit point: the creature cannot be killed in a fight.",
    ),
    switch(
        "Disarmable",
        "Can be disarmed",
        "Its weapons can be knocked out of its hands.",
    ),
    switch(
        "NoPermDeath",
        "Corpse stays",
        "Its body stays where it fell.",
    ),
];

/// A store's numbers.
pub const STORE: [Number; 3] = [
    number(
        "MarkUp",
        "Sells at (%)",
        Store::Int,
        (0.0, 1000.0),
        "What the store charges for an item, as a share of the item's value. 100 is the value.",
    ),
    number(
        "MarkDown",
        "Buys at (%)",
        Store::Int,
        (0.0, 1000.0),
        "What the store pays for an item, as a share of the item's value.",
    ),
    number(
        "BuySellFlag",
        "Trades",
        Store::Byte,
        (0.0, 3.0),
        "1 only sells, 2 only buys, 3 sells and buys.",
    ),
];

/// An encounter's numbers.
pub const ENCOUNTER: [Number; 10] = [
    switch(
        "Active",
        "Active",
        "An encounter that is not active spawns nothing until a script turns it on.",
    ),
    row(
        "DifficultyIndex",
        "Difficulty",
        Store::Int,
        "encdifficulty",
        "Its row of encdifficulty.2da: how much challenge it spawns for the party's level.",
    ),
    number(
        "MaxCreatures",
        "Most creatures",
        Store::Int,
        (1.0, 64.0),
        "The most creatures it spawns at once.",
    ),
    number(
        "RecCreatures",
        "Fewest creatures",
        Store::Int,
        (1.0, 64.0),
        "The creatures it spawns at the least.",
    ),
    switch(
        "PlayerOnly",
        "Only the player",
        "Only the player's party walking in spawns it, not other creatures.",
    ),
    switch(
        "Reset",
        "Resets",
        "It can spawn again once its creatures are gone.",
    ),
    number(
        "ResetTime",
        "Reset after (s)",
        Store::Int,
        (0.0, 32000.0),
        "The seconds before it can spawn again.",
    ),
    number(
        "Respawns",
        "Spawns again (times)",
        Store::Int,
        (-1.0, 1000.0),
        "How many times it can spawn again. -1 is without end.",
    ),
    switch(
        "SpawnOption",
        "Can spawn again",
        "Only an encounter with this on resets. Without it, the encounter spawns once and never again.",
    ),
    row(
        "Faction",
        "Faction",
        Store::Dword,
        "repute",
        "The side its creatures are on.",
    ),
];

/// A door's numbers. A door is read whole from its GIT entry, so they are
/// changed there (`apply_door`), and an entry that lacks one takes it from
/// the door's template.
pub const DOOR: [Number; 9] = [
    switch(
        "Locked",
        "Locked",
        "A locked door does not open until it is unlocked.",
    ),
    switch(
        "Lockable",
        "Can be locked again",
        "Whether the door can be locked once it has been opened.",
    ),
    switch(
        "KeyRequired",
        "Needs its key",
        "Security cannot open it: only its key, or a script, unlocks it.",
    ),
    number(
        "OpenLockDC",
        "Security to unlock",
        Store::Byte,
        (0.0, 255.0),
        "How hard the lock is to open with Security.",
    ),
    switch(
        "Plot",
        "Cannot be hurt",
        "A plot door takes no damage, so it cannot be bashed open.",
    ),
    switch(
        "Min1HP",
        "Never destroyed",
        "It is hurt down to 1 hit point and no further.",
    ),
    number(
        "Hardness",
        "Hardness",
        Store::Byte,
        (0.0, 255.0),
        "Whole points taken off every blow against it.",
    ),
    number(
        "HP",
        "Most hit points",
        Store::Short,
        (1.0, 32000.0),
        "How much damage it takes before it breaks.",
    ),
    number(
        "CurrentHP",
        "Hit points now",
        Store::Short,
        (1.0, 32000.0),
        "The hit points it starts with.",
    ),
];

/// Makes `change` in a door's GIT entry `item`: one of `DOOR`'s numbers.
pub fn apply_door(item: &mut GffStruct, change: &FieldChange) -> Result<(), String> {
    let FieldChange::Number(label, value) = change else {
        return Err("only a door's numbers are changed in its entry".into());
    };
    let number = DOOR
        .iter()
        .find(|number| number.label == label)
        .ok_or_else(|| format!("{label} is not a number of a door the editor changes"))?;
    if !value.is_finite() || !(number.min..=number.max).contains(value) {
        return Err(format!(
            "{} is from {} to {}",
            number.shown, number.min, number.max
        ));
    }
    let value = stored(item.get(label), number.store, *value);
    set(item, label, value);
    Ok(())
}

/// The numbers of templates of `restype` the editor changes.
pub fn numbers(restype: ResType) -> &'static [Number] {
    match restype {
        ResType::UTD => &DOOR,
        ResType::UTC => &CREATURE,
        ResType::UTM => &STORE,
        ResType::UTE => &ENCOUNTER,
        _ => &[],
    }
}

/// The name fields of templates of `restype`: a creature's first and last
/// names, a store's and an encounter's name.
pub fn names(restype: ResType) -> &'static [(&'static str, &'static str)] {
    match restype {
        ResType::UTC => &[("FirstName", "First name"), ("LastName", "Last name")],
        ResType::UTM => &[("LocName", "Name")],
        ResType::UTE => &[("LocalizedName", "Name")],
        _ => &[],
    }
}

/// The equipment slots a creature's template fills, by slot number
/// (`Equip_ItemList`'s struct id is `1 << slot`), with their names.
pub const SLOTS: [(u8, &str); 13] = [
    (0, "Head"),
    (1, "Body"),
    (3, "Hands"),
    (4, "Right hand"),
    (5, "Left hand"),
    (7, "Left arm"),
    (8, "Right arm"),
    (9, "Implant"),
    (10, "Belt"),
    (14, "Claw, left"),
    (15, "Claw, right"),
    (16, "Bite"),
    (17, "Hide"),
];

/// A change to a template's fields beyond its scripts.
#[derive(Debug, Clone, PartialEq)]
pub enum FieldChange {
    /// A number of `numbers`, by its label.
    Number(String, f64),
    /// A name of `names`, by its label: the template's own text, with no
    /// talk-table line. Empty for none.
    Name(String, String),
    /// A creature's class at this place in `ClassList`, its `classes.2da`
    /// row and levels; at the list's end adds one, `None` removes it.
    Class(usize, Option<(u16, u16)>),
    /// A creature's feat (`feat.2da` row) given or taken.
    Feat(u16, bool),
    /// A Force power (`spells.2da` row) given to or taken from the class
    /// at this place in `ClassList`.
    Power(usize, u16, bool),
    /// A creature's skill rank, by its place in `SkillList` (0 Computer
    /// Use to 7 Treat Injury).
    Skill(usize, u8),
    /// The item a creature has in a slot (`SLOTS`); empty for none.
    Equip(u8, String),
    /// Whether a carried item, by its place in `ItemList`, drops when the
    /// creature dies (`Dropable`) or, in a store, never runs out
    /// (`Infinite`).
    ItemFlag(usize, &'static str, bool),
    /// An encounter's creature at this place in `CreatureList`; at the
    /// list's end adds one, `None` removes it.
    Spawn(usize, Option<Spawn>),
}

/// A creature an encounter can spawn.
#[derive(Debug, Clone, PartialEq)]
pub struct Spawn {
    /// Its template (`.utc`).
    pub template: String,
    /// The challenge it counts for (`CR`).
    pub cr: f32,
    /// Whether it spawns once at most (`SingleSpawn`).
    pub single: bool,
}

/// The number `label` of the template; `None` where it has no such field.
pub fn number_of(gff: &Gff, label: &str) -> Option<f64> {
    match gff.root.get(label)? {
        GffValue::Float(_) => gff.root.float(label).map(f64::from),
        _ => gff.root.int(label).map(|value| value as f64),
    }
}

/// The text of the name `label`: its own text, else its talk-table line
/// through `talk`.
pub fn name_of(gff: &Gff, label: &str, talk: &dyn Fn(u32) -> Option<String>) -> String {
    match gff.root.get(label) {
        Some(GffValue::LocString(loc)) if loc.strref != u32::MAX => {
            talk(loc.strref).unwrap_or_else(String::new)
        }
        Some(GffValue::LocString(loc)) => loc
            .strings
            .first()
            .map(|(_, text)| String::from_utf8_lossy(text).into_owned())
            .unwrap_or_else(String::new),
        _ => String::new(),
    }
}

/// A creature's classes: each `classes.2da` row, its levels and the Force
/// powers it knows.
pub fn classes(gff: &Gff) -> Vec<(u16, u16, Vec<u16>)> {
    gff.root
        .list("ClassList")
        .iter()
        .map(|class| {
            let powers = class
                .list("KnownList0")
                .iter()
                .filter_map(|known| known.int("Spell"))
                .map(|row| row.clamp(0, 65535) as u16)
                .collect();
            (
                class.int("Class").unwrap_or(0).clamp(0, 65535) as u16,
                class.int("ClassLevel").unwrap_or(0).clamp(0, 65535) as u16,
                powers,
            )
        })
        .collect()
}

/// A creature's feats, as `feat.2da` rows.
pub fn feats(gff: &Gff) -> Vec<u16> {
    gff.root
        .list("FeatList")
        .iter()
        .filter_map(|feat| feat.int("Feat"))
        .map(|row| row.clamp(0, 65535) as u16)
        .collect()
}

/// A creature's skill ranks, in `SkillList`'s order.
pub fn skills(gff: &Gff) -> Vec<u8> {
    gff.root
        .list("SkillList")
        .iter()
        .map(|skill| skill.int("Rank").unwrap_or(0).clamp(0, 255) as u8)
        .collect()
}

/// The items a creature has in its slots: each slot number and item.
pub fn equipped(gff: &Gff) -> Vec<(u8, String)> {
    gff.root
        .list("Equip_ItemList")
        .iter()
        .filter_map(|item| {
            let slot = item.id.trailing_zeros();
            let template = item.resref("EquippedRes")?.to_ascii_lowercase();
            (slot < 32).then_some((slot as u8, template))
        })
        .collect()
}

/// Whether the carried item at `at` has the switch `label` on.
pub fn item_flag(gff: &Gff, at: usize, label: &str) -> bool {
    gff.root
        .list("ItemList")
        .get(at)
        .and_then(|item| item.int(label))
        .is_some_and(|value| value != 0)
}

/// An encounter's creatures.
pub fn spawns(gff: &Gff) -> Vec<Spawn> {
    gff.root
        .list("CreatureList")
        .iter()
        .map(|entry| Spawn {
            template: entry
                .resref("ResRef")
                .unwrap_or_else(String::new)
                .to_ascii_lowercase(),
            cr: entry.float("CR").unwrap_or(0.0),
            single: entry.int("SingleSpawn").is_some_and(|value| value != 0),
        })
        .collect()
}

fn field(label: &str, value: GffValue) -> GffField {
    GffField {
        label: label.as_bytes().to_vec(),
        value,
    }
}

fn set(item: &mut GffStruct, label: &str, value: GffValue) {
    match item
        .fields
        .iter_mut()
        .rev()
        .find(|field| field.label == label.as_bytes())
    {
        Some(field) => field.value = value,
        None => item.fields.push(field(label, value)),
    }
}

/// The list `label` of `item`, made where it has none.
fn list<'a>(item: &'a mut GffStruct, label: &str) -> &'a mut Vec<GffStruct> {
    let at = match item
        .fields
        .iter()
        .position(|field| field.label == label.as_bytes())
    {
        Some(at) => at,
        None => {
            item.fields.push(field(label, GffValue::List(Vec::new())));
            item.fields.len() - 1
        }
    };
    let slot = &mut item.fields[at];
    if !matches!(slot.value, GffValue::List(_)) {
        slot.value = GffValue::List(Vec::new());
    }
    match &mut slot.value {
        GffValue::List(items) => items,
        _ => unreachable!("set to a list above"),
    }
}

/// `value` stored as the field `was` is, else as `store` says.
fn stored(was: Option<&GffValue>, store: Store, value: f64) -> GffValue {
    let whole = value.round();
    match (was, store) {
        (Some(GffValue::Byte(_)), _) | (None, Store::Byte) => GffValue::Byte(whole as u8),
        (Some(GffValue::Char(_)), _) => GffValue::Char(whole as i8),
        (Some(GffValue::Word(_)), _) | (None, Store::Word) => GffValue::Word(whole as u16),
        (Some(GffValue::Short(_)), _) | (None, Store::Short) => GffValue::Short(whole as i16),
        (Some(GffValue::Dword(_)), _) | (None, Store::Dword) => GffValue::Dword(whole as u32),
        (Some(GffValue::Int(_)), _) | (None, Store::Int) => GffValue::Int(whole as i32),
        (Some(GffValue::Float(_)), _) | (None, Store::Float) => {
            GffValue::Float((value as f32).to_bits())
        }
        // A field stored some other way is written as the table says.
        (Some(_), Store::Byte) => GffValue::Byte(whole as u8),
        (Some(_), Store::Word) => GffValue::Word(whole as u16),
        (Some(_), Store::Short) => GffValue::Short(whole as i16),
        (Some(_), Store::Dword) => GffValue::Dword(whole as u32),
        (Some(_), Store::Int) => GffValue::Int(whole as i32),
        (Some(_), Store::Float) => GffValue::Float((value as f32).to_bits()),
    }
}

fn template_name(name: &str, what: &str) -> Result<String, String> {
    if name.is_empty() || name.len() > RESREF_LEN {
        return Err(format!("{name:?} is not {what}'s name"));
    }
    Ok(name.to_ascii_lowercase())
}

/// Makes `change` in `gff`, a template of `restype`.
pub fn apply(gff: &mut Gff, restype: ResType, change: &FieldChange) -> Result<(), String> {
    let creature = restype == ResType::UTC;
    let only_creature = || -> Result<(), String> {
        if creature {
            Ok(())
        } else {
            Err("only a creature has classes, feats, powers, skills and equipment".into())
        }
    };
    let root = &mut gff.root;
    match change {
        FieldChange::Number(label, value) => {
            let number = numbers(restype)
                .iter()
                .find(|number| number.label == label)
                .ok_or_else(|| format!("{label} is not a number the editor changes here"))?;
            if !(number.min..=number.max).contains(value) {
                return Err(format!(
                    "{} is from {} to {}",
                    number.shown, number.min, number.max
                ));
            }
            let value = stored(root.get(label), number.store, *value);
            set(root, label, value);
        }
        FieldChange::Name(label, text) => {
            if !names(restype).iter().any(|(name, _)| name == label) {
                return Err(format!("{label} is not a name of this template"));
            }
            let strings = if text.is_empty() {
                Vec::new()
            } else {
                vec![(0, text.as_bytes().to_vec())]
            };
            set(
                root,
                label,
                GffValue::LocString(LocString {
                    strref: u32::MAX,
                    strings,
                }),
            );
        }
        FieldChange::Class(at, class) => {
            only_creature()?;
            let classes = list(root, "ClassList");
            match (class, *at) {
                (None, at) if at < classes.len() => {
                    classes.remove(at);
                }
                (None, at) => return Err(format!("there is no class {at}")),
                (Some(_), _) if *at == classes.len() && classes.len() >= 3 => {
                    return Err("a creature has three classes at most".into());
                }
                (Some((row, levels)), at) if at < classes.len() => {
                    let class = &mut classes[at];
                    set(class, "Class", GffValue::Int(i32::from(*row)));
                    set(class, "ClassLevel", GffValue::Short(*levels as i16));
                }
                (Some((row, levels)), at) if at == classes.len() => {
                    classes.push(GffStruct {
                        id: 2,
                        fields: vec![
                            field("Class", GffValue::Int(i32::from(*row))),
                            field("ClassLevel", GffValue::Short(*levels as i16)),
                            field("KnownList0", GffValue::List(Vec::new())),
                        ],
                    });
                }
                (Some(_), at) => return Err(format!("there is no class {at}")),
            }
        }
        FieldChange::Feat(row, on) => {
            only_creature()?;
            let feats = list(root, "FeatList");
            let has = feats
                .iter()
                .position(|feat| feat.int("Feat") == Some(i64::from(*row)));
            match (has, on) {
                (None, true) => feats.push(GffStruct {
                    id: 1,
                    fields: vec![field("Feat", GffValue::Word(*row))],
                }),
                (Some(at), false) => {
                    feats.remove(at);
                }
                _ => {}
            }
        }
        FieldChange::Power(class, row, on) => {
            only_creature()?;
            let classes = list(root, "ClassList");
            let class = classes
                .get_mut(*class)
                .ok_or_else(|| format!("there is no class {class}"))?;
            let known = list(class, "KnownList0");
            let has = known
                .iter()
                .position(|power| power.int("Spell") == Some(i64::from(*row)));
            match (has, on) {
                (None, true) => known.push(GffStruct {
                    id: 3,
                    fields: vec![
                        field("Spell", GffValue::Word(*row)),
                        field("SpellFlags", GffValue::Byte(1)),
                        field("SpellMetaMagic", GffValue::Byte(0)),
                    ],
                }),
                (Some(at), false) => {
                    known.remove(at);
                }
                _ => {}
            }
        }
        FieldChange::Skill(at, rank) => {
            only_creature()?;
            if *at >= 8 {
                return Err(format!("there is no skill {at}: the games have 8"));
            }
            let skills = list(root, "SkillList");
            while skills.len() <= *at {
                skills.push(GffStruct {
                    id: 0,
                    fields: vec![field("Rank", GffValue::Byte(0))],
                });
            }
            set(&mut skills[*at], "Rank", GffValue::Byte(*rank));
        }
        FieldChange::Equip(slot, item) => {
            only_creature()?;
            if !SLOTS.iter().any(|(known, _)| known == slot) {
                return Err(format!("{slot} is not an equipment slot"));
            }
            let id = 1u32 << slot;
            let worn = list(root, "Equip_ItemList");
            worn.retain(|held| held.id != id);
            if !item.is_empty() {
                let item = template_name(item, "an item template")?;
                worn.push(GffStruct {
                    id,
                    fields: vec![
                        field("EquippedRes", GffValue::ResRef(item.into_bytes())),
                        field("Dropable", GffValue::Byte(0)),
                    ],
                });
                worn.sort_by_key(|held| held.id);
            }
        }
        FieldChange::ItemFlag(at, label, on) => {
            let allowed = match restype {
                ResType::UTC | ResType::UTP => *label == "Dropable",
                ResType::UTM => *label == "Infinite",
                _ => false,
            };
            if !allowed {
                return Err(format!("{label} is not an item switch of this template"));
            }
            let items = list(root, "ItemList");
            let item = items
                .get_mut(*at)
                .ok_or_else(|| format!("there is no item {at}"))?;
            set(item, label, GffValue::Byte(u8::from(*on)));
        }
        FieldChange::Spawn(at, spawn) => {
            if restype != ResType::UTE {
                return Err("only an encounter spawns creatures".into());
            }
            let creatures = list(root, "CreatureList");
            match (spawn, *at) {
                (None, at) if at < creatures.len() => {
                    creatures.remove(at);
                }
                (Some(spawn), at) if at <= creatures.len() => {
                    let template = template_name(&spawn.template, "a creature template")?;
                    let made = GffStruct {
                        id: 0,
                        fields: vec![
                            field("ResRef", GffValue::ResRef(template.into_bytes())),
                            field("CR", GffValue::Float(spawn.cr.to_bits())),
                            field("SingleSpawn", GffValue::Byte(u8::from(spawn.single))),
                            field("Appearance", GffValue::Int(0)),
                        ],
                    };
                    if at == creatures.len() {
                        creatures.push(made);
                    } else {
                        // Keeps what else the entry holds (its appearance).
                        let entry = &mut creatures[at];
                        for field in made.fields.into_iter().take(3) {
                            let label = String::from_utf8_lossy(&field.label).into_owned();
                            set(entry, &label, field.value);
                        }
                    }
                }
                (_, at) => return Err(format!("there is no creature {at}")),
            }
        }
    }
    Ok(())
}
