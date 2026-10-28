//! Item templates (I19, `sim::inventory`): each UTI's facts (its base
//! item's numbers in `baseitems.2da`, its properties as combat reads them,
//! `sim::inventory::Property`), the items a module's creatures, placeables
//! and stores start with, and the catalog of templates scripts may make.
//!
//! A property with an `UpgradeType` (an `upgrade.2da` row: KOTOR's
//! lightsabers and upgradable gear list every crystal's and part's) is in
//! force only with its upgrade fitted: the template's `Upgrades` mask (none
//! on K1's: a stock lightsaber is its base item's 2d8), else an item's own
//! (`ItemFacts::upgrades`, `ItemFacts::fitted`).

use fmt_odyssey::ResType;
use fmt_odyssey::gff::{Gff, GffStruct, GffValue};
use sim::combat::gear::{Against, OnHit, Roll, Versus, traits, weapon_traits};
use sim::combat::sheet::{DAMAGE_TYPES, Delivery, Weapon};
use sim::inventory::properties::{ALL_SAVES, OWN_TYPE};
use sim::inventory::{
    Armour, COLOUR_SLOT, Catalog, Held, ItemFacts, ItemSpell, Limit, Property, Terms, Upgrade,
};
use sim::module::{ModuleData, ObjectKind, Placement};

use crate::sheets::Tables;

use asset_odyssey::item::{self, item_type, property};

/// An item's properties with the `upgrade.2da` row they come with
/// (`None`: no upgrade, or one its template has fitted: in force). One of
/// a row an item's upgrades fitted cannot hold (`upgrade_bit`) is dropped.
fn by_upgrade(uti: &GffStruct) -> Vec<(Option<u8>, &GffStruct)> {
    let fitted = uti.int("Upgrades").unwrap_or(0);
    uti.list("PropertiesList")
        .iter()
        .filter_map(|property| match property.int("UpgradeType") {
            None | Some(255) => Some((None, property)),
            Some(row) if (0..64).contains(&row) && fitted >> row & 1 == 1 => Some((None, property)),
            Some(row) => Some((Some(upgrade_row(row)?), property)),
        })
        .collect()
}

/// `upgrade.2da` row `row`, if an item's upgrades fitted can hold it.
fn upgrade_row(row: impl TryInto<u8>) -> Option<u8> {
    row.try_into()
        .ok()
        .filter(|row| sim::inventory::upgrade_bit(*row).is_some())
}

/// The `itempropdef.2da` rows of `properties`, as bits.
fn rows<'a>(properties: impl IntoIterator<Item = &'a GffStruct>) -> u128 {
    properties
        .into_iter()
        .filter_map(|gff| gff.int("PropertyName"))
        .filter(|row| (0..128).contains(row))
        .fold(0, |rows, row| rows | 1 << row)
}

/// The template `name`'s facts, read once per plan; `None` when it does
/// not read or names no base item.
pub(crate) fn facts(tables: &mut Tables, name: &str) -> Option<ItemFacts> {
    if let Some(known) = tables.items.get(name) {
        return known.clone();
    }
    let read = read_facts(tables, name);
    tables.items.insert(name.to_string(), read.clone());
    read
}

/// TSL's upgrade slots for a base item of `baseitems.2da` `itemtype`
/// `item_type` (`upgradetypes.2da` rows; a lightsaber's colour crystal
/// second): a lightsaber's two crystals, lens, emitter and cell; a ranged
/// weapon's targeting, power pack and firing chamber; a melee weapon's
/// grip, edge and cell; armour's overlay and underlay.
fn slot_kinds(kind: i64) -> Vec<u8> {
    let is = |group: std::ops::RangeInclusive<i64>| group.contains(&kind);
    if is(item_type::LIGHTSABER) {
        vec![0, COLOUR_SLOT, 0, 1, 2, 3]
    } else if is(item_type::RANGED) {
        vec![4, 5, 6]
    } else if is(item_type::MELEE) {
        vec![7, 8, 9]
    } else if is(item_type::ARMOUR) {
        vec![10, 11]
    } else {
        Vec::new()
    }
}

fn read_facts(tables: &mut Tables, name: &str) -> Option<ItemFacts> {
    let uti = Gff::parse(&tables.source.read(name, ResType::UTI)?).ok()?;
    let root = &uti.root;
    let base_item = root.int("BaseItem").and_then(|v| u16::try_from(v).ok())?;
    let base = usize::from(base_item);
    let word = |value: Option<i64>| value.unwrap_or(0).clamp(0, i64::from(u16::MAX)) as u16;
    let split = by_upgrade(root);
    let in_force: Vec<&GffStruct> = split
        .iter()
        .filter(|(row, _)| row.is_none())
        .map(|(_, gff)| *gff)
        .collect();
    let properties: Vec<Property> = in_force
        .iter()
        .flat_map(|gff| read_property(tables, gff))
        .collect();
    let mut upgrades: Vec<Upgrade> = Vec::new();
    for (row, gff) in split.iter().filter_map(|(row, gff)| Some(((*row)?, gff))) {
        let read = read_property(tables, gff);
        let bits = rows([*gff]);
        match upgrades.iter_mut().find(|upgrade| upgrade.row == row) {
            Some(upgrade) => {
                upgrade.properties.extend(read);
                upgrade.rows |= bits;
            }
            None => {
                let at = usize::from(row);
                let (Some(kind), Some(template)) = (
                    tables.int("upgrade", at, "upgradetype"),
                    tables.text("upgrade", at, "template"),
                ) else {
                    continue;
                };
                upgrades.push(Upgrade {
                    row,
                    kind: kind.clamp(0, 255) as u8,
                    template: template.to_ascii_lowercase(),
                    properties: read,
                    rows: bits,
                });
            }
        }
    }
    upgrades.sort_by_key(|upgrade| upgrade.row);
    let itempropdef = tables.table("itempropdef");
    let of_kind = |label: &str| {
        let row = itempropdef
            .as_ref()
            .and_then(|table| item::property_row(table, label));
        in_force
            .iter()
            .filter(move |gff| row.is_some() && gff.int("PropertyName") == row)
    };
    let spell = of_kind(property::CAST_SPELL).find_map(|property| {
        Some(ItemSpell {
            row: property
                .int("Subtype")
                .and_then(|row| u16::try_from(row).ok())?,
            cost: property.int("CostValue").unwrap_or(1).clamp(0, 255) as u8,
        })
    });
    // A security spike's bonus: `iprp_bonuscost.2da`'s value at its cost.
    let costs: Vec<i64> = of_kind(property::THIEVES_TOOLS)
        .filter_map(|property| property.int("CostValue"))
        .collect();
    let tools = costs
        .into_iter()
        .filter_map(|cost| tables.int("iprp_bonuscost", usize::try_from(cost).ok()?, "value"))
        .max()
        .map_or(0, |bonus| bonus.clamp(0, 255) as u8);
    // A trap kit's `traps.2da` row.
    let trap =
        of_kind(property::TRAP).find_map(|property| u8::try_from(property.int("Subtype")?).ok());
    let armour = tables.int("baseitems", base, "baseac").map(|ac| Armour {
        defence: ac.clamp(-128, 127) as i8,
        // −1 (or nothing) leaves DEX uncapped.
        dex_cap: tables
            .int("baseitems", base, "dexbonus")
            .and_then(|cap| u8::try_from(cap).ok()),
    });
    let slots = tables
        .text("baseitems", base, "equipableslots")
        .and_then(|text| {
            let digits = text.trim_start_matches("0x").trim_start_matches("0X");
            u32::from_str_radix(digits, 16).ok()
        })
        .unwrap_or(0);
    let name_ref = match root.get("LocalizedName") {
        Some(GffValue::LocString(text)) => text.strref,
        _ => u32::MAX,
    };
    let item_type = tables.int("baseitems", base, "itemtype");
    let upgrade_level = root.int("UpgradeLevel").unwrap_or(0).clamp(0, 255) as u8;
    let bare = weapon(tables, base_item);
    // TSL's (its own table), on an item of an upgrade level; the colour
    // slot's crystal is the template itself.
    let kinds = match item_type {
        Some(kind) if tables.tsl() && upgrade_level > 0 => slot_kinds(kind),
        _ => Vec::new(),
    };
    Some(
        ItemFacts {
            // Scripts name it by its file's resref.
            template: name.to_string(),
            tag: root.resref("Tag").unwrap_or_else(String::new),
            cased_tag: crate::sheets::cased(root.bytes("Tag").unwrap_or(&[])),
            name: name_ref,
            base_item,
            stack: word(root.int("StackSize")).max(1),
            stacking: word(tables.int("baseitems", base, "stacking")).max(1),
            charges: root.int("Charges").unwrap_or(0).clamp(0, 255) as u8,
            cost: root.int("Cost").unwrap_or(0).clamp(0, i64::from(u32::MAX)) as u32,
            plot: root.int("Plot").unwrap_or(0) != 0,
            slots,
            weapon: bare,
            bare,
            armour,
            guards: Default::default(),
            properties,
            gear: Default::default(),
            limits: Vec::new(),
            rows: rows(in_force.iter().copied()),
            spell,
            tools,
            droids: match tables.int("baseitems", base, "droidorhuman") {
                Some(1) => Some(false),
                Some(2) => Some(true),
                _ => None,
            },
            upgrades,
            upgrade: row_of(tables, "upgrade", name).and_then(|row| {
                let kind = tables.int("upgrade", row, "upgradetype")?;
                Some((u16::try_from(row).ok()?, kind.clamp(0, 255) as u8))
            }),
            upgrade_level,
            parts: kinds
                .iter()
                .enumerate()
                .map(|(at, kind)| {
                    let row = root
                        .int(&format!("UpgradeSlot{at}"))
                        .filter(|_| *kind != COLOUR_SLOT)
                        .and_then(|row| usize::try_from(row).ok());
                    row.and_then(|row| tables.text("upgrade", row, "template"))
                        .map_or_else(String::new, |template| template.to_ascii_lowercase())
                })
                .collect(),
            slot_kinds: kinds,
            recolours: row_of(tables, "upcrystals", name).map_or(Vec::new(), |row| {
                ["shortmdlvar", "longmdlvar", "doublemdlvar"]
                    .iter()
                    .filter_map(|column| tables.text("upcrystals", row, column))
                    .map(|template| template.to_ascii_lowercase())
                    .collect()
            }),
            needs: ["reqfeat0", "reqfeat1", "reqfeat2", "reqfeat3", "reqfeat4"]
                .iter()
                .filter_map(|column| tables.int("baseitems", base, column))
                .filter_map(|feat| u16::try_from(feat).ok())
                .collect(),
            trap,
            // `itemtype` 44: a stealth field generator (base item 88).
            stealth_unit: item_type == Some(44),
            // `itemtype` 20: an energy shield (base item 46, forearm bands).
            shield: item_type == Some(20),
        }
        .made(),
    )
}

/// The row of 2DA `table` whose `template` is `name` (case aside).
fn row_of(tables: &mut Tables, table: &str, name: &str) -> Option<usize> {
    let rows = tables.table(table).map_or(0, |table| table.cells.len());
    (0..rows).find(|row| {
        tables
            .text(table, *row, "template")
            .is_some_and(|template| template.eq_ignore_ascii_case(name))
    })
}

/// A base item's weapon (its numbers in `baseitems.2da`, before its
/// properties); `None` for one combat does not know as a weapon.
pub(crate) fn weapon(tables: &mut Tables, row: u16) -> Option<Weapon> {
    let delivery = Delivery::of_base_item(row)?;
    let base = usize::from(row);
    let small = |value: Option<i64>| value.unwrap_or(0).clamp(0, 255) as u8;
    let feat = |value: Option<i64>| value.and_then(|row| u16::try_from(row).ok());
    Some(Weapon {
        base_item: row,
        delivery,
        dice: small(tables.int("baseitems", base, "numdice")),
        die: small(tables.int("baseitems", base, "dietoroll")),
        crit_threat: small(tables.int("baseitems", base, "critthreat")).max(1),
        crit_mult: small(tables.int("baseitems", base, "crithitmult")).max(1),
        damage_types: tables
            .int("baseitems", base, "damageflags")
            .unwrap_or(0)
            .clamp(0, i64::from(u16::MAX)) as u16,
        proficiency: feat(tables.int("baseitems", base, "reqfeat0")),
        focus: feat(tables.int("baseitems", base, "focfeat")),
        specialization: feat(tables.int("baseitems", base, "specfeat")),
        reach_m: small(tables.int("baseitems", base, "maxattackrange")),
        build: sim::combat::sheet::build::of(
            tables.int("baseitems", base, "weaponwield"),
            tables.int("baseitems", base, "weaponsize"),
            &tables
                .text("baseitems", base, "label")
                .unwrap_or_else(String::new),
        ),
        ..Weapon::UNARMED
    })
}

/// `iprp_immunity.2da` rows' `IMMUNITY_TYPE_*`s (the drain row two).
const IMMUNITIES: [&[u8]; 10] = [
    &[30],
    &[19, 29],
    &[1],
    &[2],
    &[3],
    &[4],
    &[28],
    &[6],
    &[31],
    &[32],
];
/// Freedom of Movement's: paralysis, slow, entangle, movement speed.
const FREEDOM: [u8; 4] = [6, 9, 10, 24];

/// A UTI property in force as its wearer has it, its cost table resolved
/// as the executables read each (the property's own table's `value`, else
/// the raw `CostValue`; journal 2026-10-05-item-properties); none for one
/// applied on use (read apart) or by nothing; Immunity and Freedom of
/// Movement may give several.
fn read_property(tables: &mut Tables, gff: &GffStruct) -> Vec<Property> {
    let raw = gff.int("CostValue").unwrap_or(0);
    let cost = usize::try_from(raw).unwrap_or(0);
    let subtype = gff.int("Subtype").unwrap_or(0);
    let param = gff.int("Param1Value").unwrap_or(0);
    let byte = |value: i64| value.clamp(-128, 127) as i8;
    let small = |value: i64| value.clamp(0, 255) as u8;
    let sub = small(subtype);
    let damage_type = |row: i64| {
        u8::try_from(row)
            .ok()
            .filter(|row| usize::from(*row) < DAMAGE_TYPES)
    };
    let value = |tables: &mut Tables, table: &str| tables.int(table, cost, "value").unwrap_or(0);
    // A bonus's and a penalty's size (penalty tables hold it negative).
    let bonus = byte(value(tables, "iprp_bonuscost").abs());
    let melee = byte(value(tables, "iprp_meleecost").abs());
    let neg5 = byte(-value(tables, "iprp_neg5cost").abs());
    let neg10 = byte(-value(tables, "iprp_neg10cost").abs());
    let damage = damage_roll(tables, cost);
    let side = |subtype: u8| Against::Side(subtype);
    let save = |subtype: i64| match subtype {
        1..=3 => (subtype - 1) as u8,
        _ => ALL_SAVES,
    };
    let one = |property: Property| vec![property];
    match gff.int("PropertyName").unwrap_or(-1) {
        0 => one(Property::Ability {
            ability: sub,
            by: bonus,
        }),
        1 => one(Property::Defence {
            by: bonus,
            against: None,
        }),
        2 => one(Property::Defence {
            by: bonus,
            against: Some(side(sub)),
        }),
        3 => one(Property::Defence {
            by: bonus,
            against: Some(Against::Type(sub)),
        }),
        4 => one(Property::Defence {
            by: bonus,
            against: Some(Against::Race(sub)),
        }),
        5 => one(Property::Enhancement(melee)),
        kind @ (6 | 7) => one(Property::Versus(Versus {
            against: if kind == 6 {
                side(sub)
            } else {
                Against::Race(sub)
            },
            attack: melee,
            damage: Roll {
                bonus: melee,
                ..Roll::NONE
            },
            damage_type: OWN_TYPE,
        })),
        8 => one(Property::Attack(neg5)),
        9 => u16::try_from(subtype).map_or(Vec::new(), |feat| one(Property::Feat(feat))),
        11 => damage_type(subtype).map_or(Vec::new(), |row| {
            one(Property::Damage { row, roll: damage })
        }),
        kind @ (12 | 13) => one(Property::Versus(Versus {
            against: if kind == 12 {
                side(sub)
            } else {
                Against::Race(sub)
            },
            attack: 0,
            damage,
            damage_type: damage_type(param).unwrap_or(OWN_TYPE),
        })),
        14 => damage_type(subtype).map_or(Vec::new(), |row| {
            one(Property::Immunity {
                row,
                percent: byte(value(tables, "iprp_immuncost")),
            })
        }),
        15 => one(Property::Damage {
            row: OWN_TYPE,
            roll: Roll {
                bonus: neg5,
                ..Roll::NONE
            },
        }),
        16 => one(Property::Soak {
            points: small(tables.int("iprp_soakcost", cost, "amount").unwrap_or(0)),
            bypass: sub,
        }),
        17 => damage_type(subtype).map_or(Vec::new(), |row| {
            one(Property::Resist {
                row,
                points: small(tables.int("iprp_resistcost", cost, "amount").unwrap_or(0)),
            })
        }),
        18 => damage_type(subtype).map_or(Vec::new(), |row| {
            one(Property::Immunity {
                row,
                percent: byte(-value(tables, "iprp_damvulcost").abs()),
            })
        }),
        19 => one(Property::Ability {
            ability: sub,
            by: neg10,
        }),
        20 => one(Property::Defence {
            by: neg5,
            against: None,
        }),
        21 => one(Property::Skill {
            skill: sub,
            by: neg10,
        }),
        24 => IMMUNITIES
            .get(usize::from(sub))
            .map_or(Vec::new(), |types| {
                types.iter().map(|kind| Property::Immune(*kind)).collect()
            }),
        25 => one(Property::ForceResist(small(value(tables, "iprp_srcost")))),
        // 26's subtype is an element, read as a save (QUIRKS.md Q-0159).
        26 | 27 => one(Property::Save {
            save: save(subtype),
            by: bonus,
        }),
        28 => one(Property::Keen),
        29 => one(Property::Trait(traits::LIGHT)),
        // (`u8::MAX` is a weapon's "no Mighty".)
        30 => one(Property::Mighty(small(raw).min(u8::MAX - 1))),
        31 => one(Property::NoDice),
        32 => one(Property::OnHit(on_hit(tables, sub, cost, small(param)))),
        33 | 34 => one(Property::Save {
            save: save(subtype),
            by: neg5,
        }),
        35 => one(Property::Regenerate(small(raw))),
        36 => one(Property::Skill {
            skill: sub,
            by: byte(raw),
        }),
        38 => one(Property::Attack(melee)),
        kind @ (39 | 40) => one(Property::Versus(Versus {
            against: if kind == 39 {
                side(sub)
            } else {
                Against::Race(sub)
            },
            attack: melee,
            damage: Roll::NONE,
            damage_type: OWN_TYPE,
        })),
        43 => one(Property::Limit(Limit::Side(sub))),
        44 => one(Property::Limit(Limit::Class(sub))),
        45 => one(Property::Limit(Limit::Race(sub))),
        47 => one(Property::Trait(traits::TRUE_SEEING)),
        49 => one(Property::Massive(if damage.dice > 0 {
            damage
        } else {
            Roll {
                bonus: byte(raw),
                ..Roll::NONE
            }
        })),
        50 => FREEDOM.iter().map(|kind| Property::Immune(*kind)).collect(),
        51 => one(Property::MonsterDice {
            dice: small(tables.int("iprp_monstcost", cost, "numdice").unwrap_or(0)),
            die: small(tables.int("iprp_monstcost", cost, "die").unwrap_or(0)),
        }),
        52 => one(Property::Walk(sub)),
        54 => one(Property::RegenerateForce(small(raw))),
        55 => one(Property::Deflect(byte(raw))),
        56 => one(Property::Deflect(byte(-raw))),
        57 => u16::try_from(subtype)
            .map_or(Vec::new(), |feat| one(Property::Limit(Limit::Feat(feat)))),
        59 => u16::try_from(subtype).map_or(Vec::new(), |row| one(Property::Disguise(row))),
        // TSL's.
        60 => one(Property::Limit(Limit::Gender(sub))),
        61 => one(Property::Limit(Limit::Subrace(sub))),
        62 if subtype == 0 => one(Property::Limit(Limit::Player)),
        62 => tables
            .text("iprp_pc", usize::from(sub), "expectedtag")
            .map_or(Vec::new(), |tag| one(Property::Limit(Limit::Tag(tag)))),
        63 => one(Property::Trait(traits::SILENT)),
        64 => one(Property::WeaponTrait(weapon_traits::DOOR_CUTTING)),
        65 => one(Property::WeaponTrait(weapon_traits::DOOR_SABERING)),
        66 => one(Property::Trait(traits::SNIPER)),
        67 => one(Property::Trait(traits::RAPID_SHOT)),
        68 => one(Property::DexCap(small(raw))),
        69 => one(Property::Limit(Limit::Ability {
            ability: sub,
            at_least: small(value(tables, "iprp_attribcost")),
        })),
        // Applied on use (Cast Spell, Thieves' Tools, Trap, Computer
        // Spike), or by nothing worn (22, 23, 41, 42, 48, the droid repair
        // kit's 58).
        _ => Vec::new(),
    }
}

/// `iprp_damagecost.2da` row `row` as damage: its dice, else (rows 1 to 5)
/// its label's flat bonus.
fn damage_roll(tables: &mut Tables, row: usize) -> Roll {
    let small = |value: Option<i64>| value.unwrap_or(0).clamp(0, 255) as u8;
    let dice = small(tables.int("iprp_damagecost", row, "numdice"));
    let die = small(tables.int("iprp_damagecost", row, "die"));
    if dice > 0 && die > 0 {
        Roll {
            dice,
            die,
            bonus: 0,
        }
    } else {
        let flat = tables.int("iprp_damagecost", row, "label").unwrap_or(0);
        Roll {
            bonus: flat.clamp(-128, 127) as i8,
            ..Roll::NONE
        }
    }
}

/// An on-hit property of `kind` (`iprp_onhit.2da` row): a kind reading
/// `iprp_onhitdur.2da` (`param1resref` 1: sleep to slow) has its chance
/// and rounds at `param` (6 s each in K1, 3 s in TSL); the DC is
/// `iprp_onhitdc.2da`'s at `cost`, else 20.
fn on_hit(tables: &mut Tables, kind: u8, cost: usize, param: u8) -> OnHit {
    let timed = tables.int("iprp_onhit", usize::from(kind), "param1resref") == Some(1);
    let at = usize::from(param);
    let (chance, rounds) = if timed {
        (
            tables.int("iprp_onhitdur", at, "effectchance").unwrap_or(0),
            tables
                .int("iprp_onhitdur", at, "durationrounds")
                .unwrap_or(0),
        )
    } else {
        (0, 0)
    };
    let round_s = if tables.tsl() { 3 } else { 6 };
    OnHit {
        kind,
        chance: chance.clamp(0, 100) as u8,
        seconds: (rounds * round_s).clamp(0, 255) as u8,
        dc: tables
            .int("iprp_onhitdc", cost, "value")
            .unwrap_or(20)
            .clamp(0, 255) as u8,
        param,
    }
}

/// The items a template's lists give: `Equip_ItemList` (a creature's: the
/// slot is the struct id's bit) and `ItemList` (carried), with their
/// stacks as their templates have them. Templates that do not read are
/// left out. A creature's items drop only when marked `Dropable`, worn or
/// carried, as both engines load a template (a soldier's filler blade has
/// no mark and stays with him; a lieutenant's pass card is marked). A
/// placeable's or store's carried item without the field drops when
/// `carried_drop`.
pub(crate) fn held(tables: &mut Tables, root: &GffStruct, carried_drop: bool) -> Vec<Held> {
    let mut held = Vec::new();
    for item in root.list("Equip_ItemList") {
        let Some(template) = item.resref("EquippedRes") else {
            continue;
        };
        let Some(facts) = facts(tables, &template) else {
            continue;
        };
        held.push(Held {
            template,
            stack: facts.stack,
            slot: Some(item.id.trailing_zeros().min(u32::from(u8::MAX)) as u8),
            droppable: item.int("Dropable").unwrap_or(0) != 0,
            infinite: false,
        });
    }
    for item in root.list("ItemList") {
        let Some(template) = item.resref("InventoryRes") else {
            continue;
        };
        let Some(facts) = facts(tables, &template) else {
            continue;
        };
        held.push(Held {
            template,
            stack: facts.stack,
            slot: None,
            droppable: item
                .int("Dropable")
                .map_or(carried_drop, |dropable| dropable != 0),
            infinite: item.int("Infinite").unwrap_or(0) != 0,
        });
    }
    held
}

/// The item templates a template's lists name (`Equip_ItemList`, then
/// `ItemList`), whether they read or not.
pub(crate) fn named(root: &GffStruct) -> Vec<String> {
    let equipped = root
        .list("Equip_ItemList")
        .iter()
        .filter_map(|item| item.resref("EquippedRes"));
    let carried = root
        .list("ItemList")
        .iter()
        .filter_map(|item| item.resref("InventoryRes"));
    equipped.chain(carried).collect()
}

/// A store's terms from its UTM: `MarkUp`, `MarkDown` (%), `BuySellFlag`
/// (bit 0 it sells, bit 1 it buys).
fn terms(utm: &GffStruct) -> Terms {
    let flags = utm.int("BuySellFlag").unwrap_or(0);
    Terms {
        markup: utm.int("MarkUp").unwrap_or(100) as i32,
        markdown: utm.int("MarkDown").unwrap_or(100) as i32,
        sells: flags & 1 != 0,
        buys: flags & 2 != 0,
    }
}

/// Fills in the starting items of `module`'s placeables and stores (their
/// UTP's or UTM's `ItemList`; creatures' come with their sheets), and the
/// stores' terms.
pub(crate) fn fill_containers(tables: &mut Tables, module: &mut ModuleData) {
    for placement in &mut module.placements {
        // A creature made as its template is has its items from it.
        let Placement::Own(object) = placement else {
            continue;
        };
        let extension = match object.kind {
            ObjectKind::Placeable => ResType::UTP,
            ObjectKind::Store => ResType::UTM,
            _ => continue,
        };
        let Some(template) = tables
            .source
            .read(&object.template, extension)
            .and_then(|bytes| Gff::parse(&bytes).ok())
        else {
            continue;
        };
        object.inventory = held(tables, &template.root, true);
        if object.kind == ObjectKind::Store {
            object.store = Some(terms(&template.root));
        }
    }
}

/// The templates scripts may make: every UTI the install lists, else the
/// module's objects' and those `named` (strings in its scripts) that read.
pub(crate) fn catalog<'a>(
    tables: &mut Tables,
    module: &ModuleData,
    named: impl IntoIterator<Item = &'a str>,
) -> Catalog {
    let mut names: Vec<String> = tables.source.names(ResType::UTI);
    for object in module.objects() {
        names.extend(object.inventory.iter().map(|held| held.template.clone()));
    }
    names.extend(named.into_iter().map(str::to_string));
    catalog_of(tables, names)
}

/// The templates `names` that read, each once.
pub(crate) fn catalog_of(tables: &mut Tables, mut names: Vec<String>) -> Catalog {
    names.sort();
    names.dedup();
    Catalog::new(names.iter().filter_map(|name| facts(tables, name)))
}
