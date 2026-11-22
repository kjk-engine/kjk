//! An item's properties: what it does when worn, wielded or used.
//!
//! An item template (`.uti`) lists its properties in `PropertiesList`. Each
//! is a row of the game's `itempropdef.2da` (an attribute bonus, a damage
//! bonus, a use limited to an alignment), with:
//!
//! - a **subtype**, a row of the table that property names
//!   (`itempropdef.2da`'s `subtyperesref`): which attribute, which kind of
//!   damage, which alignment;
//! - a **value**, a row of the property's cost table
//!   (`iprp_costtable.2da`, by `itempropdef.2da`'s `costtableresref`): how
//!   much;
//! - for a few properties a **parameter**, a row of the table
//!   `iprp_paramtable.2da` names (`param1resref`).
//!
//! The editor reads the tables' rows from the install (`level_editor`) and
//! changes the list here. An item of the project (`items`) keeps the
//! properties of the game's item it was copied from until they are
//! changed.

use fmt_odyssey::gff::{Gff, GffField, GffStruct, GffValue};

/// One property of an item, as its template holds it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Property {
    /// `PropertyName`: the row of `itempropdef.2da`.
    pub name: u16,
    /// `Subtype`: the row of the property's subtype table; 0 without one.
    pub subtype: u16,
    /// `CostTable`: the row of `iprp_costtable.2da` the value is of.
    pub cost_table: u8,
    /// `CostValue`: the row of that cost table.
    pub value: u16,
    /// `Param1`: the row of `iprp_paramtable.2da`; 255 without one.
    pub param_table: u8,
    /// `Param1Value`: the row of that table.
    pub param: u8,
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
        None => item.fields.push(self::field(label, value)),
    }
}

/// The item's `PropertiesList`, made where it has none.
fn list(gff: &mut Gff) -> &mut Vec<GffStruct> {
    let root = &mut gff.root;
    let at = match root
        .fields
        .iter()
        .position(|field| field.label == b"PropertiesList")
    {
        Some(at) => at,
        None => {
            root.fields
                .push(field("PropertiesList", GffValue::List(Vec::new())));
            root.fields.len() - 1
        }
    };
    let slot = &mut root.fields[at];
    if !matches!(slot.value, GffValue::List(_)) {
        slot.value = GffValue::List(Vec::new());
    }
    match &mut slot.value {
        GffValue::List(items) => items,
        _ => unreachable!("set to a list above"),
    }
}

fn read(property: &GffStruct) -> Property {
    let number = |label: &str, none: i64| property.int(label).unwrap_or(none);
    Property {
        name: number("PropertyName", 0).clamp(0, 65535) as u16,
        subtype: number("Subtype", 0).clamp(0, 65535) as u16,
        cost_table: number("CostTable", 0).clamp(0, 255) as u8,
        value: number("CostValue", 0).clamp(0, 65535) as u16,
        param_table: number("Param1", 255).clamp(0, 255) as u8,
        param: number("Param1Value", 0).clamp(0, 255) as u8,
    }
}

/// The item's properties, in its list's order.
pub fn properties(gff: &Gff) -> Vec<Property> {
    gff.root.list("PropertiesList").iter().map(read).collect()
}

/// Adds `property` at the end of the item's list. A property the item has
/// already, of the same row and subtype, is changed where it stands, so
/// an item never holds the same bonus twice.
pub fn add(gff: &mut Gff, property: Property) {
    let list = list(gff);
    let had = list.iter().position(|had| {
        let had = read(had);
        had.name == property.name && had.subtype == property.subtype
    });
    // A new entry is shaped as the item's own entries are, so a field the
    // game's items carry (TSL's `UpgradeType`) is there. Without one it
    // holds the fields both games' items have.
    let mut entry = match (had, list.first()) {
        (Some(at), _) => list[at].clone(),
        (None, Some(first)) => first.clone(),
        (None, None) => GffStruct {
            id: 0,
            fields: vec![field("ChanceAppear", GffValue::Byte(100))],
        },
    };
    set(&mut entry, "PropertyName", GffValue::Word(property.name));
    set(&mut entry, "Subtype", GffValue::Word(property.subtype));
    set(&mut entry, "CostTable", GffValue::Byte(property.cost_table));
    set(&mut entry, "CostValue", GffValue::Word(property.value));
    set(&mut entry, "Param1", GffValue::Byte(property.param_table));
    set(&mut entry, "Param1Value", GffValue::Byte(property.param));
    match had {
        Some(at) => list[at] = entry,
        None => list.push(entry),
    }
}

/// Takes the item's property at place `at` (from 0) out of its list.
pub fn remove(gff: &mut Gff, at: usize) -> Result<(), String> {
    let list = list(gff);
    if at >= list.len() {
        return Err(format!("the item has no property {at}"));
    }
    list.remove(at);
    Ok(())
}

/// Takes every property out of the item's list.
pub fn clear(gff: &mut Gff) {
    list(gff).clear();
}
