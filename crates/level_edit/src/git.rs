//! The objects an area's `.git` file places, read and changed in place.
//!
//! A GIT file has one list for each kind of object (`Kind::list_and_type`).
//! Doors and placeables stand at `X`, `Y` and `Z` and face by `Bearing`, in
//! radians about Z. Every other kind stands at `XPosition`, `YPosition` and
//! `ZPosition` and faces along (`XOrientation`, `YOrientation`). The editor
//! keeps both forms as the file has them, and writes the form an entry
//! already uses.

use collision::trig;
use fmt_odyssey::gff::{Gff, GffField, GffStruct, GffValue};

pub use asset_odyssey::module::Kind;

/// The longest tag the games keep (a GIT's `Tag` is a CExoString, which the
/// games cut at 32 characters when they compare tags).
pub const TAG_LEN: usize = 32;

/// The longest template name: a resref.
pub const RESREF_LEN: usize = 16;

/// One object of the area, as its GIT entry says.
#[derive(Debug, Clone, PartialEq)]
pub struct Entry {
    pub kind: Kind,
    /// Its place in its kind's GIT list.
    pub index: usize,
    /// `TemplateResRef` (a store's `ResRef`), lowercase.
    pub template: String,
    /// The entry's own `Tag`; empty when the entry has none, and the
    /// template's then names the object.
    pub tag: String,
    /// Metres in the game's space, Z up.
    pub position: [f32; 3],
    /// Degrees from +X, turning left.
    pub facing: f32,
}

/// Every object the GIT places: the kinds in `Kind::ALL`'s order, each
/// kind in its list's order. This is the order the module reader takes
/// them in (`asset_odyssey::module`), apart from the creatures it cannot
/// make.
pub fn entries(git: &Gff) -> Vec<Entry> {
    let mut entries = Vec::new();
    for kind in Kind::ALL {
        let (label, _) = kind.list_and_type();
        for (index, item) in git.root.list(label).iter().enumerate() {
            entries.push(entry(kind, index, item));
        }
    }
    entries
}

/// The object `kind` `index` alone, read from its own GIT entry without
/// the other objects being read.
pub fn entry_at(git: &Gff, kind: Kind, index: usize) -> Option<Entry> {
    item(git, kind, index).map(|item| entry(kind, index, item))
}

fn entry(kind: Kind, index: usize, item: &GffStruct) -> Entry {
    let template_label = if kind == Kind::Store {
        "ResRef"
    } else {
        "TemplateResRef"
    };
    let tag = item
        .bytes("Tag")
        .map(|bytes| String::from_utf8_lossy(bytes).into_owned())
        .unwrap_or_else(String::new);
    let (position, facing) = if item.get("X").is_some() {
        let bearing = item.float("Bearing").unwrap_or(0.0);
        (
            [
                item.float("X").unwrap_or(0.0),
                item.float("Y").unwrap_or(0.0),
                item.float("Z").unwrap_or(0.0),
            ],
            bearing.to_degrees(),
        )
    } else {
        let x = item.float("XOrientation").unwrap_or(1.0);
        let y = item.float("YOrientation").unwrap_or(0.0);
        (
            [
                item.float("XPosition").unwrap_or(0.0),
                item.float("YPosition").unwrap_or(0.0),
                item.float("ZPosition").unwrap_or(0.0),
            ],
            trig::atan2_deg(y, x),
        )
    };
    Entry {
        kind,
        index,
        template: item
            .resref(template_label)
            .unwrap_or_else(String::new)
            .to_ascii_lowercase(),
        tag,
        position,
        facing,
    }
}

/// The GIT list of `kind`, made empty when the file has none.
fn list_mut(git: &mut Gff, kind: Kind) -> &mut Vec<GffStruct> {
    let (label, _) = kind.list_and_type();
    let at = match git
        .root
        .fields
        .iter()
        .position(|field| field.label == label.as_bytes())
    {
        Some(at) => at,
        None => {
            git.root.fields.push(GffField {
                label: label.as_bytes().to_vec(),
                value: GffValue::List(Vec::new()),
            });
            git.root.fields.len() - 1
        }
    };
    let field = &mut git.root.fields[at];
    if !matches!(field.value, GffValue::List(_)) {
        field.value = GffValue::List(Vec::new());
    }
    match &mut field.value {
        GffValue::List(items) => items,
        _ => unreachable!("set to a list above"),
    }
}

/// Sets `label` to `value`, replacing the field when the struct has one,
/// else adding it.
fn set(item: &mut GffStruct, label: &str, value: GffValue) {
    match item
        .fields
        .iter_mut()
        .rev()
        .find(|field| field.label == label.as_bytes())
    {
        Some(field) => field.value = value,
        None => item.fields.push(GffField {
            label: label.as_bytes().to_vec(),
            value,
        }),
    }
}

fn float(value: f32) -> GffValue {
    GffValue::Float(value.to_bits())
}

/// The entry `index` of `kind`, if the list has it.
fn item_mut(git: &mut Gff, kind: Kind, index: usize) -> Option<&mut GffStruct> {
    list_mut(git, kind).get_mut(index)
}

/// Moves the entry to `position`. False when the list has no such entry.
pub fn set_position(git: &mut Gff, kind: Kind, index: usize, position: [f32; 3]) -> bool {
    let Some(item) = item_mut(git, kind, index) else {
        return false;
    };
    let labels = if item.get("X").is_some() {
        ["X", "Y", "Z"]
    } else {
        ["XPosition", "YPosition", "ZPosition"]
    };
    let was = labels.map(|label| item.float(label).unwrap_or(0.0));
    for (label, value) in labels.into_iter().zip(position) {
        set(item, label, float(value));
    }
    // An encounter's spawn points are where they are in the area, not
    // beside the encounter: they move with it. A trigger's and an
    // encounter's outline is beside it, and moves by itself.
    if kind == Kind::Encounter {
        let by = [0, 1, 2].map(|axis| position[axis] - was[axis]);
        for field in &mut item.fields {
            if field.label == b"SpawnPointList"
                && let GffValue::List(points) = &mut field.value
            {
                for point in points {
                    for (axis, label) in ["X", "Y", "Z"].into_iter().enumerate() {
                        let at = point.float(label).unwrap_or(0.0);
                        set(point, label, float(at + by[axis]));
                    }
                }
            }
        }
    }
    true
}

/// The entry `index` of `kind`, if the list has it.
pub fn item(git: &Gff, kind: Kind, index: usize) -> Option<&GffStruct> {
    git.root.list(kind.list_and_type().0).get(index)
}

/// A trigger's or an encounter's outline, beside its position; empty for
/// any other.
pub fn outline(git: &Gff, kind: Kind, index: usize) -> Vec<[f32; 3]> {
    let Some(item) = item(git, kind, index) else {
        return Vec::new();
    };
    item.list("Geometry")
        .iter()
        .map(|point| {
            let axis = |trigger: &str, encounter: &str| {
                point
                    .float(trigger)
                    .or_else(|| point.float(encounter))
                    .unwrap_or(0.0)
            };
            [
                axis("PointX", "X"),
                axis("PointY", "Y"),
                axis("PointZ", "Z"),
            ]
        })
        .collect()
}

/// Sets a trigger's or an encounter's outline: `points` relative to its
/// position, written as its `Geometry` (`PointX` for a trigger, `X` for an
/// encounter). False when the list has no such entry.
pub fn set_outline(git: &mut Gff, kind: Kind, index: usize, points: &[[f32; 3]]) -> bool {
    let Some(item) = item_mut(git, kind, index) else {
        return false;
    };
    let labels = if kind == Kind::Encounter {
        ["X", "Y", "Z"]
    } else {
        ["PointX", "PointY", "PointZ"]
    };
    let id = match item.get("Geometry") {
        Some(GffValue::List(old)) => old.first().map_or(3, |first| first.id),
        _ => 3,
    };
    let list = points
        .iter()
        .map(|point| GffStruct {
            id,
            fields: labels
                .into_iter()
                .zip(*point)
                .map(|(label, value)| GffField {
                    label: label.as_bytes().to_vec(),
                    value: float(value),
                })
                .collect(),
        })
        .collect();
    set(item, "Geometry", GffValue::List(list));
    true
}

/// An encounter's spawn points (`SpawnPointList`): where each is in the
/// area (metres) and the way it faces (radians).
pub fn spawn_points(git: &Gff, index: usize) -> Vec<([f32; 3], f32)> {
    let Some(item) = item(git, Kind::Encounter, index) else {
        return Vec::new();
    };
    item.list("SpawnPointList")
        .iter()
        .map(|point| {
            let axis = |label: &str| point.float(label).unwrap_or(0.0);
            ([axis("X"), axis("Y"), axis("Z")], axis("Orientation"))
        })
        .collect()
}

/// Sets an encounter's spawn points. False when the list has no such
/// entry.
pub fn set_spawn_points(git: &mut Gff, index: usize, points: &[([f32; 3], f32)]) -> bool {
    let Some(item) = item_mut(git, Kind::Encounter, index) else {
        return false;
    };
    let list = points
        .iter()
        .map(|(at, facing)| GffStruct {
            id: 0,
            fields: ["X", "Y", "Z", "Orientation"]
                .into_iter()
                .zip([at[0], at[1], at[2], *facing])
                .map(|(label, value)| GffField {
                    label: label.as_bytes().to_vec(),
                    value: float(value),
                })
                .collect(),
        })
        .collect();
    set(item, "SpawnPointList", GffValue::List(list));
    true
}

/// Sets a field of the entry: its `Tag` (a string) or an event's script (a
/// resref). False when the list has no such entry.
pub fn set_entry_field(
    git: &mut Gff,
    kind: Kind,
    index: usize,
    label: &str,
    value: GffValue,
) -> bool {
    let Some(item) = item_mut(git, kind, index) else {
        return false;
    };
    set(item, label, value);
    true
}

/// Turns the entry to face `degrees` from +X. False when the list has no
/// such entry.
pub fn set_facing(git: &mut Gff, kind: Kind, index: usize, degrees: f32) -> bool {
    let Some(item) = item_mut(git, kind, index) else {
        return false;
    };
    if item.get("X").is_some() {
        set(item, "Bearing", float(degrees.to_radians()));
    } else {
        let (sin, cos) = trig::sin_cos_deg(degrees);
        set(item, "XOrientation", float(cos));
        set(item, "YOrientation", float(sin));
    }
    true
}

/// Adds `item` at the end of `kind`'s list, with the list's struct id,
/// and returns its index (a pasted entry, `clip`).
pub fn push(git: &mut Gff, kind: Kind, mut item: GffStruct) -> usize {
    let list = list_mut(git, kind);
    item.id = list.first().map_or(struct_id(kind), |first| first.id);
    list.push(item);
    list.len() - 1
}

/// Sets the field `label` of an entry, adding it where it has none.
pub fn set_field(item: &mut GffStruct, label: &str, value: GffValue) {
    set(item, label, value);
}

/// Adds a copy of the entry at the end of its list and returns the copy's
/// index.
pub fn duplicate(git: &mut Gff, kind: Kind, index: usize) -> Option<usize> {
    let list = list_mut(git, kind);
    let copy = list.get(index)?.clone();
    list.push(copy);
    Some(list.len() - 1)
}

/// Removes the entry. False when the list has no such entry.
pub fn remove(git: &mut Gff, kind: Kind, index: usize) -> bool {
    let list = list_mut(git, kind);
    if index >= list.len() {
        return false;
    }
    list.remove(index);
    true
}

/// The struct id a GIT list's entries carry in the games' own files, for an
/// entry added to an empty list.
fn struct_id(kind: Kind) -> u32 {
    match kind {
        Kind::Creature => 4,
        Kind::Door => 8,
        Kind::Placeable => 9,
        Kind::Trigger => 1,
        Kind::Waypoint => 5,
        Kind::Sound => 6,
        Kind::Encounter => 7,
        Kind::Store => 11,
    }
}

/// Adds an object of `kind` made from `template`, standing at `position`
/// and facing `degrees`, and returns its index in its list. The entry
/// holds only what the games read when they make an object from its
/// template: the template, the position and the facing. A placeable or a
/// door is written with `X`, `Y`, `Z` and `Bearing`; every other kind with
/// `XPosition`, `YPosition`, `ZPosition` and its orientation.
pub fn add(git: &mut Gff, kind: Kind, template: &str, position: [f32; 3], degrees: f32) -> usize {
    add_from(git, kind, (template, None), position, degrees)
}

/// The side of a new trigger's square outline, metres.
pub const NEW_TRIGGER_SIDE: f32 = 2.0;

/// The template's fields a waypoint does not keep: the toolset's own.
const TOOLSET_FIELDS: [&str; 2] = ["Comment", "PaletteID"];

/// `add`, from the template's own fields (`base`) where the kind is read
/// whole from its entry (a waypoint: its tag, name and map note are the
/// entry's). A new trigger or encounter gets a square outline of
/// `NEW_TRIGGER_SIDE` round its position; a new encounter has no spawn
/// points, so its creatures appear where the party walks in.
pub fn add_from(
    git: &mut Gff,
    kind: Kind,
    (template, base): (&str, Option<&GffStruct>),
    position: [f32; 3],
    degrees: f32,
) -> usize {
    let list = list_mut(git, kind);
    let id = list.first().map_or(struct_id(kind), |first| first.id);
    let template_label = if kind == Kind::Store {
        "ResRef"
    } else {
        "TemplateResRef"
    };
    let mut item = GffStruct {
        id,
        fields: base.map_or_else(Vec::new, |base| {
            base.fields
                .iter()
                .filter(|field| {
                    !TOOLSET_FIELDS
                        .iter()
                        .any(|label| field.label == label.as_bytes())
                })
                .cloned()
                .collect()
        }),
    };
    set(
        &mut item,
        template_label,
        GffValue::ResRef(template.to_ascii_lowercase().into_bytes()),
    );
    if matches!(kind, Kind::Trigger | Kind::Encounter) {
        let labels = if kind == Kind::Encounter {
            ["X", "Y", "Z"]
        } else {
            ["PointX", "PointY", "PointZ"]
        };
        let half = NEW_TRIGGER_SIDE * 0.5;
        let corners = [[-half, -half], [half, -half], [half, half], [-half, half]];
        let points = corners
            .iter()
            .map(|[x, y]| GffStruct {
                id: 3,
                fields: labels
                    .into_iter()
                    .zip([*x, *y, 0.0])
                    .map(|(label, value)| GffField {
                        label: label.as_bytes().to_vec(),
                        value: float(value),
                    })
                    .collect(),
            })
            .collect();
        set(&mut item, "Geometry", GffValue::List(points));
        if kind == Kind::Encounter {
            set(&mut item, "SpawnPointList", GffValue::List(Vec::new()));
        }
    }
    if matches!(kind, Kind::Door | Kind::Placeable) {
        for (label, value) in ["X", "Y", "Z"].into_iter().zip(position) {
            set(&mut item, label, float(value));
        }
        set(&mut item, "Bearing", float(degrees.to_radians()));
    } else {
        for (label, value) in ["XPosition", "YPosition", "ZPosition"]
            .into_iter()
            .zip(position)
        {
            set(&mut item, label, float(value));
        }
        let (sin, cos) = trig::sin_cos_deg(degrees);
        set(&mut item, "XOrientation", float(cos));
        set(&mut item, "YOrientation", float(sin));
    }
    list.push(item);
    list.len() - 1
}

/// The doors and waypoints of `git` a link can arrive at: each one's tag,
/// and whether it is a door. Those without a tag are left out.
pub fn arrivals(git: &Gff) -> Vec<(String, bool)> {
    entries(git)
        .into_iter()
        .filter(|entry| matches!(entry.kind, Kind::Door | Kind::Waypoint) && !entry.tag.is_empty())
        .map(|entry| (entry.tag, entry.kind == Kind::Door))
        .collect()
}

/// The doors and triggers of `git`, each with its place in its list, its
/// tag (the template's when the entry has none) and where it leads.
pub fn exits(git: &Gff) -> Vec<(Kind, usize, String, crate::template::Link)> {
    entries(git)
        .into_iter()
        .filter(|entry| matches!(entry.kind, Kind::Door | Kind::Trigger))
        .filter_map(|entry| {
            let link = crate::template::link_of(item(git, entry.kind, entry.index)?);
            let tag = if entry.tag.is_empty() {
                entry.template.clone()
            } else {
                entry.tag.clone()
            };
            Some((entry.kind, entry.index, tag, link))
        })
        .collect()
}

/// An empty GIT: every list, none holding anything.
pub fn empty() -> Gff {
    Gff {
        file_type: *b"GIT ",
        root: GffStruct {
            id: u32::MAX,
            fields: Kind::ALL
                .into_iter()
                .map(|kind| GffField {
                    label: kind.list_and_type().0.as_bytes().to_vec(),
                    value: GffValue::List(Vec::new()),
                })
                .collect(),
        },
    }
}

/// Points the entry at another template. False when the list has no such
/// entry.
pub fn set_template(git: &mut Gff, kind: Kind, index: usize, template: &str) -> bool {
    let Some(item) = item_mut(git, kind, index) else {
        return false;
    };
    let label = if kind == Kind::Store {
        "ResRef"
    } else {
        "TemplateResRef"
    };
    set(
        item,
        label,
        GffValue::ResRef(template.to_ascii_lowercase().into_bytes()),
    );
    true
}
