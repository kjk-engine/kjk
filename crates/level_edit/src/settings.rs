//! The area's own settings, as the level editor changes them: its music
//! and ambient sound (the GIT's `AreaProperties`), its fog, ambient light
//! and scripts (the ARE), and a new map's module scripts (its
//! `module.ifo`).
//!
//! An installed area's ARE is changed in a copy the project keeps
//! (`Project::are`), made from the install's at the first change. Its
//! `module.ifo` is never changed: every module's is named `module`, so a
//! copy in the project's folder would stand in for every module's.

use fmt_odyssey::gff::{Gff, GffField, GffStruct, GffValue};

/// The GIT's music rows (`ambientmusic.2da`).
pub const MUSIC: [&str; 3] = ["MusicDay", "MusicNight", "MusicBattle"];
/// The GIT's ambient sound rows (`ambientsound.2da`) and their volumes
/// (0 to 127).
pub const AMBIENCE: [(&str, &str); 2] = [
    ("AmbientSndDay", "AmbientSndDayVol"),
    ("AmbientSndNight", "AmbientSndNitVol"),
];
/// The area's scripts (its ARE).
pub const AREA_SCRIPTS: [&str; 4] = ["OnEnter", "OnExit", "OnHeartbeat", "OnUserDefined"];
/// A new map's module scripts (its `module.ifo`) that the editor offers.
pub const MODULE_SCRIPTS: [&str; 6] = [
    "Mod_OnModLoad",
    "Mod_OnClientEntr",
    "Mod_OnClientLeav",
    "Mod_OnHeartbeat",
    "Mod_OnUsrDefined",
    "Mod_OnPlrDeath",
];

/// One change to the area's settings.
#[derive(Debug, Clone, PartialEq)]
pub enum AreaChange {
    /// A number of the GIT's `AreaProperties`: a music or ambient sound
    /// row, or a volume.
    Property(String, u32),
    /// A field of the area's ARE.
    Are(String, GffValue),
    /// A new map's module script: the `module.ifo` label and the script,
    /// empty for none.
    ModuleScript(String, String),
}

/// The fields of the ARE the editor changes, and the value each has when
/// the ARE lacks it.
pub fn are_default(label: &str) -> Option<GffValue> {
    let float = |value: f32| GffValue::Float(value.to_bits());
    Some(match label {
        "SunFogOn" => GffValue::Byte(0),
        "SunFogNear" => float(0.0),
        "SunFogFar" => float(0.0),
        "SunFogColor" | "DynAmbientColor" => GffValue::Dword(0),
        "OnEnter" | "OnExit" | "OnHeartbeat" | "OnUserDefined" => GffValue::ResRef(Vec::new()),
        _ => return None,
    })
}

/// A number of the GIT's `AreaProperties`; 0 when it has none.
pub fn property(git: &Gff, label: &str) -> u32 {
    match git.root.get("AreaProperties") {
        Some(GffValue::Struct(properties)) => properties.uint(label).unwrap_or(0),
        _ => 0,
    }
}

/// Sets a number of the GIT's `AreaProperties`, keeping the field's type
/// where it has one (an `INT` in the games' GITs), and making the struct
/// when the GIT has none.
pub fn set_property(git: &mut Gff, label: &str, value: u32) {
    let at = git
        .root
        .fields
        .iter()
        .position(|field| field.label == b"AreaProperties");
    let at = at.unwrap_or_else(|| {
        git.root.fields.push(GffField {
            label: b"AreaProperties".to_vec(),
            value: GffValue::Struct(Box::new(GffStruct {
                id: 100,
                fields: Vec::new(),
            })),
        });
        git.root.fields.len() - 1
    });
    let GffValue::Struct(properties) = &mut git.root.fields[at].value else {
        return;
    };
    set_keeping_type(properties, label, value);
}

/// Sets `label` of `item` to `value`, as the type the field has (an `INT`
/// when it is new).
fn set_keeping_type(item: &mut GffStruct, label: &str, value: u32) {
    let typed = |old: Option<&GffValue>| match old {
        Some(GffValue::Byte(_)) => GffValue::Byte(value.min(255) as u8),
        Some(GffValue::Word(_)) => GffValue::Word(value.min(65_535) as u16),
        Some(GffValue::Dword(_)) => GffValue::Dword(value),
        _ => GffValue::Int(value as i32),
    };
    match item
        .fields
        .iter_mut()
        .rev()
        .find(|field| field.label == label.as_bytes())
    {
        Some(field) => field.value = typed(Some(&field.value)),
        None => item.fields.push(GffField {
            label: label.as_bytes().to_vec(),
            value: typed(None),
        }),
    }
}

/// Sets a field of the ARE `are`, refusing one the editor does not change
/// and a value of another type than its own.
pub fn set_are(are: &mut Gff, label: &str, value: GffValue) -> Result<(), String> {
    let default = are_default(label).ok_or(format!("{label} is not an area setting"))?;
    if std::mem::discriminant(&default) != std::mem::discriminant(&value) {
        return Err(format!("{label} takes a value of another type"));
    }
    if let GffValue::ResRef(script) = &value
        && script.len() > crate::git::RESREF_LEN
    {
        return Err(format!(
            "{:?} is longer than a script's name",
            String::from_utf8_lossy(script)
        ));
    }
    match are
        .root
        .fields
        .iter_mut()
        .rev()
        .find(|field| field.label == label.as_bytes())
    {
        Some(field) => field.value = value,
        None => are.root.fields.push(GffField {
            label: label.as_bytes().to_vec(),
            value,
        }),
    }
    Ok(())
}

/// A colour as the games keep it (`0x00BBGGRR`), as red, green and blue
/// from 0 to 1.
pub fn colour_of(value: u32) -> [f32; 3] {
    [0, 8, 16].map(|shift| ((value >> shift) & 0xff) as f32 / 255.0)
}

/// Red, green and blue from 0 to 1 as the games keep a colour.
pub fn colour_value(rgb: [f32; 3]) -> u32 {
    let byte = |c: f32| (c.clamp(0.0, 1.0) * 255.0).round() as u32;
    byte(rgb[0]) | (byte(rgb[1]) << 8) | (byte(rgb[2]) << 16)
}
