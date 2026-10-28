//! What an item is called on disk and what it is, from its UTI and its base
//! item's `baseitems.2da` row: its model's and its inventory icon's names,
//! and the kind its row's typed columns make it.

use std::ops::RangeInclusive;

/// `baseitems.2da` `itemtype` groups, the same in both games' tables: the
/// melee blades (double-bladed, short and long sword), the blasters
/// (pistols, rifles, repeaters), body armour and clothing (armour classes,
/// clothes, robes) and the lightsabers (double-bladed, short, standard).
/// TSL's upgrade slots are by these groups (`upgradetypes.2da`).
pub mod item_type {
    use super::RangeInclusive;

    pub const MELEE: RangeInclusive<i64> = 0..=2;
    pub const RANGED: RangeInclusive<i64> = 3..=5;
    pub const DATAPAD: i64 = 24;
    pub const ARMOUR: RangeInclusive<i64> = 31..=38;
    pub const LIGHTSABER: RangeInclusive<i64> = 39..=41;
}

/// `baseitems.2da` `weapontype` of a ranged weapon.
pub const RANGED_WEAPON_TYPE: i64 = 4;

/// The body slot's bit in `baseitems.2da` `equipableslots`.
pub const BODY_SLOT: u32 = 0x2;

/// How an item's model is turned lying on the floor, before its facing
/// turns it about Z: by its base item's `baseitems.2da` `rotateonground`, a
/// quarter turn about its own Y axis for 1 (K1's guns and blades, laid on
/// their side), about its X axis for 2, none for the rest. KOTOR's client
/// item builds its orientation so: its facing, then those quarter turns.
pub fn ground_turn(rotate: i64) -> crate::model::Matrix {
    match rotate {
        1 => [
            [0.0, 0.0, 1.0, 0.0],
            [0.0, 1.0, 0.0, 0.0],
            [-1.0, 0.0, 0.0, 0.0],
            [0.0, 0.0, 0.0, 1.0],
        ],
        2 => [
            [1.0, 0.0, 0.0, 0.0],
            [0.0, 0.0, -1.0, 0.0],
            [0.0, 1.0, 0.0, 0.0],
            [0.0, 0.0, 0.0, 1.0],
        ],
        _ => crate::model::IDENTITY,
    }
}

/// An item's model: its base item's `itemclass`, `_`, its `ModelVariation`
/// in three digits (xoreos and reone name it so). A lightsaber's variation is its colour
/// (K1 and TSL: 1 blue, 2 red, 3 green, 4 yellow, 5 violet).
pub fn model_name(item_class: &str, model_variation: i64) -> String {
    format!("{}_{model_variation:03}", item_class.to_ascii_lowercase())
}

/// An item's inventory icon: `i`, its base item's `itemclass`, `_`, a
/// variation in three digits. reone takes the
/// `TextureVar` of an item worn on the body and the `ModelVariation` of any
/// other; xoreos takes the other when the first is 0, and
/// 1 when both are.
pub fn icon_name(
    item_class: &str,
    worn_on_body: bool,
    model_variation: i64,
    texture_variation: i64,
) -> String {
    let (first, second) = if worn_on_body {
        (texture_variation, model_variation)
    } else {
        (model_variation, texture_variation)
    };
    let variation = [first, second]
        .into_iter()
        .find(|variation| *variation > 0)
        .unwrap_or(1);
    format!("i{}_{variation:03}", item_class.to_ascii_lowercase())
}

/// The `itempropdef.2da` labels of the properties an item is used by, read
/// apart from those it wears (both games' rows: 10, 37, 46).
pub mod property {
    pub const CAST_SPELL: &str = "CastSpell";
    pub const THIEVES_TOOLS: &str = "ThievesTools";
    pub const TRAP: &str = "Trap";
}

/// The `itempropdef.2da` row labelled `label` (`property`).
pub fn property_row(itempropdef: &fmt_odyssey::twoda::TwoDa, label: &str) -> Option<i64> {
    itempropdef.find("label", label).map(|row| row as i64)
}
