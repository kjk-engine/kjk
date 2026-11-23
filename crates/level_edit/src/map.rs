//! A new map: a module of the project's own, built from the games' rooms
//! (`docs/LEVEL-EDIT.md`, "New maps").
//!
//! A room is a model and a walkmesh. The layout (LYT) places a room's model
//! at a position, but the room's walkmesh is in the coordinates of the area
//! it was made for. So a room placed anywhere else is copied under a name of
//! its own: its model with that name in its header
//! (`fmt_odyssey::mdl::renamed`) and its walkmesh moved by how far the room
//! moved (`fmt_odyssey::bwm::Walkmesh::moved`). The copies are named after
//! the module and the room's place in the layout (`room_name`).
//!
//! The layout cannot turn a room, so a room turned by its `yaw` is turned
//! in its copy: each part of its model is turned round the model's origin
//! (`fmt_odyssey::mdl::turned`), and its walkmesh is turned round the
//! room's old place before it is moved (`Walkmesh::turned`). The model's
//! vertices, its MDX and its lightmaps stay as they are.
//!
//! The project's folder holds the map as `kjk-map.toml` (its title, the
//! module it started from, its rooms and its entry point) and the area's
//! `.are`. Saving it also writes `modules/<module>.mod`, which holds the
//! module's `module.ifo`, the area's layout and visibility, and the rooms'
//! copies (`module_files`). The area's objects stay in the folder's `.git`,
//! as in every project.
//!
//! A room can also be a room the project cut from a Jedi Academy level
//! (`ja_rooms`). The map keeps such a room as the level's path and the
//! room's name (`JaRoom`), never as its geometry. kjk builds the room from
//! the player's own Jedi Academy install each time a run loads the map
//! (`kjk_game::ja_made`), under the name `ja_rooms::room_resref` gives, and
//! places a copy of it as it places a KOTOR room's. The module's archive
//! leaves such a room's copy out.

use fmt_odyssey::ResType;
use fmt_odyssey::bwm::Walkmesh;
use fmt_odyssey::gff::{Gff, GffField, GffStruct, GffValue, LocString};
use fmt_odyssey::lyt::{Layout, Placed};
use fmt_odyssey::vis::Visibility;

/// The file of a project's map.
pub const MAP_FILE: &str = "kjk-map.toml";

/// The longest name of a new map's module. A room's copy is named after the
/// module, an underscore and its number, and that must fit a resref's 16
/// characters for up to 999 rooms.
pub const NAME_LEN: usize = 12;

/// A new map, as the project holds it.
#[derive(Debug, Clone, PartialEq)]
pub struct NewMap {
    /// The name the games show for the module.
    pub title: String,
    /// The module the map started from: its area's light, fog and sound.
    pub base: String,
    pub rooms: Vec<Room>,
    /// Where the player starts, and the way they face (degrees from +X).
    pub entry: [f32; 3],
    pub entry_facing: f32,
    /// The area's ARE: the base's, with its rooms and names the map's.
    pub are: Gff,
    /// The module's scripts (`module.ifo`'s `Mod_OnModLoad` and the like),
    /// by label; a label not here has none.
    pub module_scripts: std::collections::BTreeMap<String, String>,
    /// The Jedi Academy sky box drawn round the map, by the name its six
    /// pictures share (`textures/skies/hoth`). kjk reads the pictures from
    /// the player's Jedi Academy install (`kjk_game::ja_made::sky_of`).
    pub ja_sky: Option<String>,
}

/// One room of a new map.
#[derive(Debug, Clone, PartialEq)]
pub struct Room {
    /// The room's model as the install has it (`m12aa_01b`).
    pub model: String,
    /// Where the room's own area's layout places it. Its walkmesh is in
    /// that area's coordinates.
    pub from: [f32; 3],
    /// Where the map places it.
    pub position: [f32; 3],
    /// Degrees it is turned about Z, anticlockwise seen from above.
    pub yaw: f32,
    /// The Jedi Academy room the model is built from, for a room cut from a
    /// Jedi Academy level. Its model is then `ja_rooms::room_resref`'s
    /// name, and `from` is where the built room's origin lies in the level.
    pub ja: Option<JaRoom>,
}

/// A room the project cut from a Jedi Academy level.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct JaRoom {
    /// The level's path in Jedi Academy's files (`maps/t1_sour.bsp`).
    pub level: String,
    /// The room's name, as the project's rooms of the level name it.
    pub room: String,
}

impl Room {
    /// The room's walkmesh (`wok`, in its own area's coordinates) where the
    /// map places it, turned by its yaw.
    pub fn placed(&self, wok: &Walkmesh) -> Walkmesh {
        if self.yaw == 0.0 {
            let by = std::array::from_fn(|axis| self.position[axis] - self.from[axis]);
            return wok.moved(by);
        }
        wok.turned(
            self.from,
            self.position,
            collision::trig::sin_cos_deg(self.yaw),
        )
    }
}

/// What is wrong with a module's name for a new map, if anything.
pub fn name_problem(name: &str) -> Option<String> {
    if name.is_empty() || name.len() > NAME_LEN {
        return Some(format!("a map's name has 1 to {NAME_LEN} characters"));
    }
    if !name
        .bytes()
        .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_')
    {
        return Some("a map's name has lowercase letters, digits and underscores only".into());
    }
    None
}

/// The name of the copy of the map's room `index`: `<module>_<NN>`.
pub fn room_name(module: &str, index: usize) -> String {
    format!("{module}_{:02}", index + 1)
}

fn bad(what: impl std::fmt::Display) -> String {
    format!("{MAP_FILE}: {what}")
}

fn number(item: Option<&toml_edit::Item>, what: &str) -> Result<f32, String> {
    item.and_then(|item| {
        item.as_float()
            .or_else(|| item.as_integer().map(|value| value as f64))
    })
    .map(|value| value as f32)
    .filter(|value| value.is_finite())
    .ok_or_else(|| bad(format!("{what} is not a number")))
}

fn triple(item: Option<&toml_edit::Item>, what: &str) -> Result<[f32; 3], String> {
    let array = item
        .and_then(toml_edit::Item::as_array)
        .filter(|array| array.len() == 3)
        .ok_or_else(|| bad(format!("{what} is not a list of three numbers")))?;
    let mut out = [0.0; 3];
    for (slot, value) in out.iter_mut().zip(array.iter()) {
        *slot = value
            .as_float()
            .or_else(|| value.as_integer().map(|value| value as f64))
            .map(|value| value as f32)
            .filter(|value| value.is_finite())
            .ok_or_else(|| bad(format!("{what} is not a list of three numbers")))?;
    }
    Ok(out)
}

fn text(item: Option<&toml_edit::Item>, what: &str) -> Result<String, String> {
    item.and_then(toml_edit::Item::as_str)
        .map(str::to_string)
        .ok_or_else(|| bad(format!("{what} is missing")))
}

impl NewMap {
    /// A map of no rooms, starting from `base`'s area's ARE.
    pub fn new(title: &str, base: &str, module: &str, mut are: Gff) -> NewMap {
        set(
            &mut are.root,
            "Tag",
            GffValue::String(module.as_bytes().to_vec()),
        );
        set(&mut are.root, "Name", localized(title));
        // The base area's own scripts are its module's, which the new map
        // does not have: left in, the games look for them at every load.
        for label in crate::settings::AREA_SCRIPTS {
            if are.root.get(label).is_some() {
                set(&mut are.root, label, GffValue::ResRef(Vec::new()));
            }
        }
        // The base's rooms stay in the ARE until the map is written
        // (`rooms_into_are`): its first is the pattern of the map's rooms.
        NewMap {
            title: title.to_string(),
            base: base.to_ascii_lowercase(),
            rooms: Vec::new(),
            entry: [0.0; 3],
            entry_facing: 0.0,
            are,
            module_scripts: std::collections::BTreeMap::new(),
            ja_sky: None,
        }
    }

    /// The map `kjk-map.toml` holds, with `are` its area's ARE.
    pub fn parse(source: &str, are: Gff) -> Result<NewMap, String> {
        let document: toml_edit::DocumentMut = source.parse().map_err(bad)?;
        let mut rooms = Vec::new();
        if let Some(tables) = document
            .get("room")
            .and_then(toml_edit::Item::as_array_of_tables)
        {
            for table in tables.iter() {
                let ja = match (table.get("ja_level"), table.get("ja_room")) {
                    (None, None) => None,
                    (level, room) => Some(JaRoom {
                        level: text(level, "a Jedi Academy room's ja_level")?.to_ascii_lowercase(),
                        room: text(room, "a Jedi Academy room's ja_room")?,
                    }),
                };
                let model = match &ja {
                    Some(ja) => crate::ja_rooms::room_resref(&ja.level, &ja.room),
                    None => text(table.get("model"), "a room's model")?.to_ascii_lowercase(),
                };
                rooms.push(Room {
                    model,
                    from: triple(table.get("from"), "a room's from")?,
                    position: triple(table.get("position"), "a room's position")?,
                    yaw: table
                        .get("yaw")
                        .and_then(|item| {
                            item.as_float()
                                .or_else(|| item.as_integer().map(|whole| whole as f64))
                        })
                        .map_or(0.0, |degrees| degrees as f32),
                    ja,
                });
            }
        }
        Ok(NewMap {
            title: text(document.get("title"), "title")?,
            base: text(document.get("base"), "base")?.to_ascii_lowercase(),
            rooms,
            entry: triple(document.get("entry"), "entry")?,
            entry_facing: number(document.get("entry_facing"), "entry_facing")?,
            are,
            module_scripts: document
                .get("module_scripts")
                .and_then(toml_edit::Item::as_table)
                .map(|table| {
                    table
                        .iter()
                        .filter_map(|(label, script)| {
                            Some((label.to_string(), script.as_str()?.to_ascii_lowercase()))
                        })
                        .collect()
                })
                .unwrap_or_else(std::collections::BTreeMap::new),
            ja_sky: (document.get("ja_sky").and_then(toml_edit::Item::as_str))
                .map(str::to_ascii_lowercase)
                .filter(|sky| !sky.is_empty()),
        })
    }

    /// The file's text. Numbers are written so they read back to the same
    /// bits.
    pub fn write(&self) -> String {
        let triple = |v: [f32; 3]| format!("[{:?}, {:?}, {:?}]", v[0], v[1], v[2]);
        let mut out = format!(
            "# A new map made by kjk's level editor (docs/LEVEL-EDIT.md): its rooms\n\
             # and where the player starts. The area's light, fog and sound are in\n\
             # its .are file.\n\
             title = {:?}\nbase = {:?}\nentry = {}\nentry_facing = {:?}\n",
            self.title,
            self.base,
            triple(self.entry),
            self.entry_facing
        );
        if let Some(sky) = &self.ja_sky {
            out.push_str(&format!(
                "# The Jedi Academy sky box drawn round the map.\nja_sky = {sky:?}\n"
            ));
        }
        let scripts: Vec<_> = self
            .module_scripts
            .iter()
            .filter(|(_, script)| !script.is_empty())
            .collect();
        if !scripts.is_empty() {
            out.push_str("\n# The module's scripts (module.ifo).\n[module_scripts]\n");
            for (label, script) in scripts {
                out.push_str(&format!("{label} = {script:?}\n"));
            }
        }
        for room in &self.rooms {
            out.push_str(&format!(
                "\n[[room]]\nmodel = {:?}\nfrom = {}\nposition = {}\n",
                room.model,
                triple(room.from),
                triple(room.position)
            ));
            if room.yaw != 0.0 {
                out.push_str(&format!("yaw = {:?}\n", f64::from(room.yaw)));
            }
            if let Some(ja) = &room.ja {
                out.push_str(&format!(
                    "ja_level = {:?}\nja_room = {:?}\n",
                    ja.level, ja.room
                ));
            }
        }
        out
    }

    /// The ARE's rooms' list made the map's: one entry per room, named as
    /// its copy, each a copy of the list's first entry where it has one (the
    /// base's first room, until the map is written; then its own first).
    pub fn rooms_into_are(&mut self, module: &str) {
        let first = self.are.root.list("Rooms").first().cloned();
        let rooms = (0..self.rooms.len())
            .map(|index| {
                let mut entry = first.clone().unwrap_or(GffStruct {
                    id: 0,
                    fields: vec![
                        field("EnvAudio", GffValue::Int(0)),
                        field("AmbientScale", GffValue::Float(0f32.to_bits())),
                    ],
                });
                set(
                    &mut entry,
                    "RoomName",
                    GffValue::String(room_name(module, index).into_bytes()),
                );
                if entry.get("ForceRating").is_some() {
                    set(&mut entry, "ForceRating", GffValue::Int(0));
                }
                entry
            })
            .collect();
        set(&mut self.are.root, "Rooms", GffValue::List(rooms));
    }

    /// Whether the map's room `index` is not the first of the map to place
    /// its model. Its copy then gets lightmaps of its own (`room_copy`),
    /// so a bake lights each copy where it stands.
    pub fn own_light(&self, index: usize) -> bool {
        let Some(room) = self.rooms.get(index) else {
            return false;
        };
        (self.rooms[..index].iter()).any(|before| before.model.eq_ignore_ascii_case(&room.model))
    }

    /// The area's layout: each room's copy at its position.
    pub fn layout(&self, module: &str) -> Layout {
        Layout {
            rooms: self
                .rooms
                .iter()
                .enumerate()
                .map(|(index, room)| Placed {
                    model: room_name(module, index),
                    position: room.position,
                })
                .collect(),
            file_dependency: Some(format!("{module}.max")),
            ..Layout::default()
        }
    }

    /// The area's visibility: every room sees every room.
    pub fn visibility(&self, module: &str) -> Visibility {
        let names: Vec<String> = (0..self.rooms.len())
            .map(|index| room_name(module, index))
            .collect();
        Visibility {
            rooms: names
                .iter()
                .map(|name| (name.clone(), names.clone()))
                .collect(),
            count_mismatches: Vec::new(),
        }
    }

    /// The module's `module.ifo`: its name, its one area and its entry.
    pub fn ifo(&self, module: &str, area: &str) -> Gff {
        let radians = self.entry_facing.to_radians();
        let float = |value: f32| GffValue::Float(value.to_bits());
        let resref = |value: &str| GffValue::ResRef(value.as_bytes().to_vec());
        let mut root = GffStruct {
            id: u32::MAX,
            fields: vec![
                field("Mod_ID", GffValue::Void(vec![0; 16])),
                field("Mod_Creator_ID", GffValue::Int(0)),
                field("Mod_Version", GffValue::Dword(3)),
                field("Mod_VO_ID", GffValue::String(module.as_bytes().to_vec())),
                field("Mod_Name", localized(&self.title)),
                field(
                    "Mod_Tag",
                    GffValue::String(module.to_ascii_uppercase().into_bytes()),
                ),
                field("Mod_IsSaveGame", GffValue::Byte(0)),
                field("Mod_Entry_Area", resref(area)),
                field("Mod_Entry_X", float(self.entry[0])),
                field("Mod_Entry_Y", float(self.entry[1])),
                field("Mod_Entry_Z", float(self.entry[2])),
                field("Mod_Entry_Dir_X", float(radians.cos())),
                field("Mod_Entry_Dir_Y", float(radians.sin())),
                field("Mod_Expan_List", GffValue::List(Vec::new())),
                field("Mod_DawnHour", GffValue::Byte(6)),
                field("Mod_DuskHour", GffValue::Byte(18)),
                field("Mod_MinPerHour", GffValue::Byte(2)),
                field("Mod_StartMonth", GffValue::Byte(6)),
                field("Mod_StartDay", GffValue::Byte(1)),
                field("Mod_StartHour", GffValue::Byte(13)),
                field("Mod_StartYear", GffValue::Dword(1372)),
                field("Mod_XPScale", GffValue::Byte(10)),
            ],
        };
        for script in [
            "Mod_OnHeartbeat",
            "Mod_OnModLoad",
            "Mod_OnModStart",
            "Mod_OnClientEntr",
            "Mod_OnClientLeav",
            "Mod_OnActvtItem",
            "Mod_OnAcquirItem",
            "Mod_OnUsrDefined",
            "Mod_OnUnAqreItem",
            "Mod_OnPlrDeath",
            "Mod_OnPlrDying",
            "Mod_OnSpawnBtnDn",
            "Mod_OnPlrRest",
            "Mod_OnPlrLvlUp",
            "Mod_OnEquipItem",
        ] {
            let own = self.module_scripts.get(script).map_or("", String::as_str);
            root.fields.push(field(script, resref(own)));
        }
        root.fields.push(field("Mod_StartMovie", resref("")));
        root.fields.push(field(
            "Mod_Area_list",
            GffValue::List(vec![GffStruct {
                id: 6,
                fields: vec![field("Area_Name", resref(area))],
            }]),
        ));
        Gff {
            file_type: *b"IFO ",
            root,
        }
    }

    /// Every file of the module's archive (`modules/<module>.mod`):
    /// `module.ifo`, the area's layout and visibility, and each room's copy.
    /// `read` gives a room's files as the install has them. A room cut from
    /// a Jedi Academy level has no copy here: its copy is made in memory
    /// when a run loads the map, so no part of Jedi Academy's files is
    /// written.
    pub fn module_files(
        &self,
        module: &str,
        area: &str,
        read: &dyn Fn(&str, ResType) -> Option<Vec<u8>>,
    ) -> Result<Vec<(String, ResType, Vec<u8>)>, String> {
        let mut files = vec![
            (
                "module".to_string(),
                ResType::IFO,
                self.ifo(module, area).write(),
            ),
            (area.to_string(), ResType::LYT, self.layout(module).write()),
            (
                area.to_string(),
                ResType::VIS,
                self.visibility(module).write(),
            ),
        ];
        for (index, room) in self.rooms.iter().enumerate() {
            if room.ja.is_some() {
                continue;
            }
            let name = room_name(module, index);
            for (resref, restype, bytes) in room_copy(room, &name, self.own_light(index), read)? {
                files.push((resref, restype, bytes));
            }
        }
        Ok(files)
    }
}

/// The name of the copy `name`'s own lightmap in place of `lightmap`
/// (`room_copy`): `lm` and 14 letters and digits of a digest of the two.
pub fn own_lightmap(name: &str, lightmap: &str) -> String {
    let key = format!(
        "{}#{}",
        name.to_ascii_lowercase(),
        lightmap.to_ascii_lowercase()
    );
    let digest = blake3::hash(key.as_bytes()).to_hex();
    format!("lm{}", &digest[..14])
}

/// The copy of `room` named `name`: its model renamed, its model's MDX as
/// it is, and its walkmesh moved to where the map places the room.
///
/// With `own_light`, the copy's meshes name lightmaps of its own
/// (`own_lightmap`), each a copy of the room's lightmap with its TXI, if
/// it has one. A room placed twice in one map would otherwise share its
/// lightmaps between its two places. A bake writes one picture for each
/// lightmap, so the second place showed the first one's light
/// (`NewMap::own_light`).
pub fn room_copy(
    room: &Room,
    name: &str,
    own_light: bool,
    read: &dyn Fn(&str, ResType) -> Option<Vec<u8>>,
) -> Result<Vec<(String, ResType, Vec<u8>)>, String> {
    let mdl = read(&room.model, ResType::MDL)
        .ok_or_else(|| format!("the room {} has no model in the install", room.model))?;
    let mdl = fmt_odyssey::mdl::renamed(&mdl, name)
        .map_err(|error| format!("{}.mdl: {error}", room.model))?;
    // A room cut from a Jedi Academy level is built in the level's own
    // coordinates. Its copy is moved so that the room's `from`, the place
    // the map keeps for it, is the copy's origin, as a KOTOR room's model
    // is made round its own place. Its walkmesh is moved from that same
    // place, so the two stay together even when the room's cuts change.
    let mdl = match room.ja {
        Some(_) => fmt_odyssey::mdl::shifted(&mdl, room.from.map(|part| -part))
            .map_err(|error| format!("{}.mdl moved: {error}", room.model))?,
        None => mdl,
    };
    let mut lights = Vec::new();
    let mdl = if own_light {
        let (relit, names) =
            fmt_odyssey::mdl::relit(&mdl, &|lightmap| Some(own_lightmap(name, lightmap)))
                .map_err(|error| format!("{}.mdl: {error}", room.model))?;
        for lightmap in names {
            let own = own_lightmap(name, &lightmap);
            let picture = [ResType::TPC, ResType::TGA]
                .into_iter()
                .find_map(|restype| Some((restype, read(&lightmap, restype)?)))
                .ok_or_else(|| {
                    format!(
                        "the room {} has no lightmap {lightmap} in the install",
                        room.model
                    )
                })?;
            lights.push((own.clone(), picture.0, picture.1));
            if let Some(txi) = read(&lightmap, ResType::TXI) {
                lights.push((own, ResType::TXI, txi));
            }
        }
        relit
    } else {
        mdl
    };
    let mdl = if room.yaw == 0.0 {
        mdl
    } else {
        fmt_odyssey::mdl::turned(&mdl, collision::trig::sin_cos_deg(room.yaw))
            .map_err(|error| format!("{}.mdl turned: {error}", room.model))?
    };
    let mut files = vec![(name.to_string(), ResType::MDL, mdl)];
    if let Some(mdx) = read(&room.model, ResType::MDX) {
        files.push((name.to_string(), ResType::MDX, mdx));
    }
    if let Some(wok) = read(&room.model, ResType::WOK) {
        let walkmesh =
            Walkmesh::parse(&wok).map_err(|error| format!("{}.wok: {error}", room.model))?;
        files.push((
            name.to_string(),
            ResType::WOK,
            room.placed(&walkmesh).write(),
        ));
    }
    files.extend(lights);
    Ok(files)
}

fn field(label: &str, value: GffValue) -> GffField {
    GffField {
        label: label.as_bytes().to_vec(),
        value,
    }
}

/// A localized string of `text` alone, in English.
fn localized(text: &str) -> GffValue {
    GffValue::LocString(LocString {
        strref: u32::MAX,
        strings: vec![(0, text.as_bytes().to_vec())],
    })
}

/// Sets `label` in `item`, adding the field when it has none.
fn set(item: &mut GffStruct, label: &str, value: GffValue) {
    match item
        .fields
        .iter_mut()
        .rev()
        .find(|field| field.label == label.as_bytes())
    {
        Some(found) => found.value = value,
        None => item.fields.push(field(label, value)),
    }
}

/// How far a room is moved at most to meet another's seam, metres.
pub const SEAM_REACH: f32 = 1.5;

/// The outline edges of a walkmesh's walkable faces (`Walkmesh::edges`):
/// each edge's two corners.
fn outline(walkmesh: &Walkmesh) -> Vec<([f32; 3], [f32; 3])> {
    walkmesh
        .edges
        .iter()
        .filter_map(|&(edge, _)| {
            let face = walkmesh.faces.get(usize::try_from(edge).ok()? / 3)?;
            let at = edge as usize % 3;
            let corner = |k: usize| walkmesh.vertices.get(face.vertices[k] as usize).copied();
            Some((corner(at)?, corner((at + 1) % 3)?))
        })
        .collect()
}

/// How far to move the room whose walkmesh is `moving` so that an edge of
/// its outline lies on an edge of another room's (`others`): the shortest
/// such move under `reach` metres, between two edges of the same length
/// (within 5 cm) that run along each other. `None` when no edge is that
/// near. Rooms that meet edge to edge are walked across in kjk
/// (`docs/NAV.md`, room seams).
pub fn seam_offset(moving: &Walkmesh, others: &[Walkmesh], reach: f32) -> Option<[f32; 3]> {
    let length = |(a, b): ([f32; 3], [f32; 3])| {
        ((b[0] - a[0]).powi(2) + (b[1] - a[1]).powi(2) + (b[2] - a[2]).powi(2)).sqrt()
    };
    let middle = |(a, b): ([f32; 3], [f32; 3])| -> [f32; 3] {
        std::array::from_fn(|axis| (a[axis] + b[axis]) * 0.5)
    };
    let along = |(a, b): ([f32; 3], [f32; 3]), (c, d): ([f32; 3], [f32; 3])| {
        let one: [f32; 3] = std::array::from_fn(|axis| b[axis] - a[axis]);
        let two: [f32; 3] = std::array::from_fn(|axis| d[axis] - c[axis]);
        let dot: f32 = (0..3).map(|axis| one[axis] * two[axis]).sum();
        dot.abs() / (length((a, b)) * length((c, d))).max(1e-6)
    };
    let theirs: Vec<_> = others.iter().flat_map(outline).collect();
    let mut best: Option<(f32, [f32; 3])> = None;
    for mine in outline(moving) {
        let size = length(mine);
        if size < 0.1 {
            continue;
        }
        for &other in &theirs {
            if (length(other) - size).abs() > 0.05 || along(mine, other) < 0.995 {
                continue;
            }
            let (from, to) = (middle(mine), middle(other));
            let by: [f32; 3] = std::array::from_fn(|axis| to[axis] - from[axis]);
            let distance = length(([0.0; 3], by));
            if distance <= reach && best.is_none_or(|(least, _)| distance < least) {
                best = Some((distance, by));
            }
        }
    }
    best.map(|(_, by)| by)
}
