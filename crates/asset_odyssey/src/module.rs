//! A module as the runtime needs it: its entry point and each area's rooms
//! (layout, visibility, walkmesh presence) and instances (from the GIT, with
//! their templates checked). Files come through a `Source`, so this module
//! does no I/O of its own.

use fmt_odyssey::ResType;
use fmt_odyssey::gff::{Gff, GffStruct, GffValue};
use fmt_odyssey::lyt::{Layout, Placed};
use fmt_odyssey::twoda::TwoDa;
use fmt_odyssey::vis::Visibility;

use crate::map::{AreaMap, MapNote};

/// Where a module's files come from (the resolver, with the module loaded).
pub trait Source {
    fn read(&self, resref: &str, restype: ResType) -> Option<Vec<u8>>;
    fn exists(&self, resref: &str, restype: ResType) -> bool;
    /// Every resource of `restype` it holds, lowercase, where it can list
    /// them (none by default).
    fn names(&self, restype: ResType) -> Vec<String> {
        let _ = restype;
        Vec::new()
    }
    /// The resources of `restype` the module's own archives hold (none by
    /// default): what an import of the shared data does not cover.
    fn own_names(&self, restype: ResType) -> Vec<String> {
        let _ = restype;
        Vec::new()
    }

    /// A voiced line's file (`streamwaves`, `streamvoice`), if any.
    fn voice(&self, _resref: &str) -> Option<Vec<u8>> {
        None
    }

    /// The install's talk table (`dialog.tlk`), if it has one.
    fn talk(&self) -> Option<std::sync::Arc<fmt_odyssey::tlk::Tlk>> {
        None
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct ModuleIr {
    pub name: String,
    /// `Mod_Name`: a strref, or the module's own text.
    pub title: Title,
    pub entry_area: String,
    pub entry_position: [f32; 3],
    pub entry_direction: [f32; 2],
    pub areas: Vec<AreaIr>,
    /// The module's event scripts (IFO `Mod_On…`: label, resref).
    pub scripts: Vec<(String, String)>,
    /// `Mod_MinPerHour`: how many real minutes a game hour lasts (the
    /// clock's, `GetTimeHour`); 0 where the IFO does not say.
    pub minutes_per_hour: u8,
}

/// A localized string: a talk-table strref, or its own text.
#[derive(Debug, Clone, PartialEq)]
pub enum Title {
    StrRef(u32),
    Text(Vec<u8>),
    None,
}

#[derive(Debug, Clone, PartialEq)]
pub struct AreaIr {
    pub resref: String,
    /// ARE `DynAmbientColor`, packed with red in the low byte.
    pub dyn_ambient: Option<u32>,
    /// ARE `SunFogOn` with its range and colour (as reone reads the fog).
    pub fog: Option<AreaFog>,
    pub name_strref: Option<u32>,
    pub tag: Vec<u8>,
    pub rooms: Vec<RoomIr>,
    /// ARE `LoadScreenID`: a row of `loadscreens.2da`.
    pub load_screen: Option<u32>,
    /// Rooms the ARE lists that the LYT does not place, and the reverse.
    pub rooms_only_in_are: Vec<String>,
    pub rooms_only_in_lyt: Vec<String>,
    pub instances: Vec<Instance>,
    pub cameras: Vec<Camera>,
    /// What the area names but the install does not provide.
    pub missing: Vec<Missing>,
    /// The area's event scripts (ARE `OnEnter`, `OnExit`, ...).
    pub scripts: Vec<(String, String)>,
    /// The ARE's `MiniGame`, for a swoop track or a turret.
    pub minigame: Option<crate::minigame::MiniGameIr>,
    /// The ARE's `Map`: where its picture shows the world.
    pub map: Option<AreaMap>,
    /// The ARE's stealth facts.
    pub stealth: AreaStealthIr,
    /// The ARE's grass; none without a texture or a density.
    pub grass: Option<AreaGrass>,
    /// The ARE's weather chances and wind.
    pub weather: AreaWeather,
    /// The ARE's `DisableTransit`: no leaving for the Ebon Hawk from the
    /// map page (TSL's).
    pub disable_transit: bool,
}

/// The ARE's grass (`Grass_*`); `None` without a texture or a density.
#[derive(Debug, Clone, PartialEq)]
pub struct AreaGrass {
    pub texture: String,
    pub density: f32,
    pub quad_size: f32,
    /// Packed with red in the low byte, as `dyn_ambient`.
    pub ambient: u32,
    pub diffuse: u32,
    /// TSL's `Grass_Emissive`; 0 in K1.
    pub emissive: u32,
    /// Upper left, upper right, lower left, lower right.
    pub chances: [f32; 4],
}

/// The ARE's weather chances, 0 to 100, and `WindPower` (0 to 2).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct AreaWeather {
    pub rain: i32,
    pub snow: i32,
    pub lightning: i32,
    pub wind: i32,
}

/// An ARE's stealth facts: `ModListenCheck`, and its stealth XP
/// (`StealthXPMax`, `StealthXPLoss`, `StealthXPEnabled`; `StealthXPCurrent`
/// where it has one, else the most: K1's areas give none).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct AreaStealthIr {
    pub mod_listen: i32,
    pub xp_max: i32,
    pub xp_current: i32,
    pub xp_loss: i32,
    pub xp_enabled: bool,
}

/// Distance fog: none up to `near` from the eye, all fog colour from `far`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AreaFog {
    pub near: f32,
    pub far: f32,
    /// Packed with red in the low byte, as `dyn_ambient`.
    pub color: u32,
}

#[derive(Debug, Clone, PartialEq)]
pub struct RoomIr {
    pub model: String,
    pub position: [f32; 3],
    /// Rooms visible from this one; empty when the VIS does not list it.
    pub visible: Vec<String>,
    pub has_walkmesh: bool,
    /// The ARE's `Rooms` entry of this name keeps weather out (`DisableWeather`).
    pub disable_weather: bool,
    /// That entry's `ForceRating` (TSL's: what the room does to Force costs
    /// and regeneration); 0 where none.
    pub force_rating: i32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Kind {
    Creature,
    Door,
    Placeable,
    Trigger,
    Waypoint,
    Sound,
    Encounter,
    Store,
}

impl Kind {
    /// The GIT list and the template type for each kind.
    pub(crate) fn list_and_type(self) -> (&'static str, ResType) {
        match self {
            Kind::Creature => ("Creature List", ResType::UTC),
            Kind::Door => ("Door List", ResType::UTD),
            Kind::Placeable => ("Placeable List", ResType::UTP),
            Kind::Trigger => ("TriggerList", ResType::UTT),
            Kind::Waypoint => ("WaypointList", ResType::UTW),
            Kind::Sound => ("SoundList", ResType::UTS),
            Kind::Encounter => ("Encounter List", ResType::UTE),
            Kind::Store => ("StoreList", ResType::UTM),
        }
    }

    pub const ALL: [Kind; 8] = [
        Kind::Creature,
        Kind::Door,
        Kind::Placeable,
        Kind::Trigger,
        Kind::Waypoint,
        Kind::Sound,
        Kind::Encounter,
        Kind::Store,
    ];
}

#[derive(Debug, Clone, PartialEq)]
pub struct Instance {
    pub kind: Kind,
    pub template: String,
    pub tag: Vec<u8>,
    pub position: [f32; 3],
    /// Facing as an angle about Z, in radians: `Bearing`, or the angle of
    /// (`XOrientation`, `YOrientation`) by the platform's `atan2`, whose
    /// last bits differ between machines: for drawing only.
    pub facing: f32,
    /// The facing as stored. Whatever decides sim state turns by this,
    /// through the portable trigonometry (`collision::trig`): the level's
    /// collision (`collision_odyssey::turn`) and the sim's objects
    /// (`session`).
    pub direction: Direction,
    /// A door's or trigger's destination (`LinkedToModule`, `LinkedTo`),
    /// both empty when it leads nowhere; `None` for other kinds.
    pub linked_to: Option<(String, Vec<u8>)>,
    /// `TransitionDestin`: the destination's name (a strref).
    pub destination: Option<u32>,
    /// A door's or placeable's `Locked` and `AnimationState` (instance,
    /// else template).
    pub locked: bool,
    pub animation_state: u32,
    /// A door's or placeable's lock, field by field the instance's, else
    /// the template's.
    pub lock: Lock,
    /// Event scripts (label, resref) the template and the instance name:
    /// every non-empty resref field whose label starts with `On` or
    /// `Script`, the instance's winning over the template's.
    pub scripts: Vec<(String, String)>,
    /// `Conversation`: the DLG it speaks (instance, else template), lowercase;
    /// empty for none.
    pub conversation: String,
    /// A trigger's or encounter's outline, relative to its position.
    pub geometry: Vec<[f32; 3]>,
    pub template_found: bool,
    /// `Appearance`, and a door's `GenericType`: the instance's value, else
    /// the template's (xoreos loads the blueprint, then the instance).
    pub appearance: Option<u32>,
    pub generic_type: Option<u32>,
    /// A door's or placeable's model, lowercase, from its appearance table.
    pub model: Option<String>,
    /// A door that, broken by blows, plays its model's `busted`
    /// (`genericdoors.2da`'s `StaticAnim`).
    pub busted: bool,
    /// `Static` (instance, else template): reone lights static objects
    /// only with lights of dynamic type 1.
    pub is_static: bool,
    /// Its name (instance, else template): a creature's `FirstName` and
    /// `LastName`, another's `LocName` or `LocalizedName`.
    pub name: Title,
    pub last_name: Title,
    /// Whether a placeable holds things (`HasInventory`, else an
    /// `ItemList` with items), instance else template; none where neither
    /// says. Its items are the session's to read (I19).
    pub container: Option<bool>,
    /// A placeable's `Useable` (instance, else template).
    pub useable: Option<bool>,
    /// A door's or placeable's hit points, current and most (`CurrentHP`,
    /// `HP`; instance, else template); `None` where neither says.
    pub hit_points: Option<(i32, i32)>,
    /// What a door's or placeable's hit points stand against (instance,
    /// else template).
    pub toughness: Toughness,
    /// A trigger's, door's or placeable's `Faction` (a `repute.2da` row;
    /// instance, else template); `None` where neither says.
    pub faction: Option<u32>,
    /// A trigger's, door's or placeable's trap, if its `TrapFlag` is set
    /// (instance, else template).
    pub trap: Option<TrapIr>,
    /// A waypoint's note on the map (instance, else template).
    pub map_note: Option<MapNote>,
    /// An encounter's spawning: its UTE's fields and the GIT's spawn
    /// points; `None` for other kinds and an encounter whose template is
    /// missing (the executables skip it: "Encounter template %s doesn't
    /// exist").
    pub encounter: Option<EncounterIr>,
}

/// What a UTE encounter spawns and when, as both games' executables read
/// it (`CSWSEncounter::ReadEncounterFromGff`): the template's fields, each
/// at the constructor's default where the template lacks it, and the GIT's
/// `SpawnPointList`. Every other field comes from the template only.
#[derive(Debug, Clone, PartialEq)]
pub struct EncounterIr {
    pub active: bool,
    pub reset: bool,
    /// Seconds after exhaustion before a reset (`ResetTime`).
    pub reset_time: i32,
    /// Most resets, -1 for no end (`Respawns`).
    pub respawns: i32,
    pub spawn_option: i32,
    pub max_creatures: i32,
    pub rec_creatures: i32,
    pub player_only: bool,
    pub faction: u32,
    /// `encdifficulty.2da`'s row (`DifficultyIndex`, -1 where missing),
    /// and the value written beside it (`Difficulty`).
    pub difficulty_index: i32,
    pub difficulty: i32,
    /// `CreatureList`, in the template's order.
    pub creatures: Vec<EncounterCreatureIr>,
    /// `SpawnPointList`: places (absolute) and orientations (radians).
    pub spawn_points: Vec<([f32; 3], f32)>,
}

/// One entry of an encounter's `CreatureList`.
#[derive(Debug, Clone, PartialEq)]
pub struct EncounterCreatureIr {
    pub template: String,
    /// `CR`: its challenge rating as the encounter counts it.
    pub cr: f32,
    pub single: bool,
    /// `GuaranteedCount` (TSL only; 0 where absent, as in every K1 UTE).
    pub guaranteed: i32,
}

impl EncounterIr {
    /// The encounter `template` (its UTE root) and `git` (its GIT entry)
    /// describe.
    fn read(template: &GffStruct, git: &GffStruct) -> EncounterIr {
        let int = |label: &str, default: i32| {
            template.int(label).map_or(default, |value| {
                value.clamp(i64::from(i32::MIN), i64::from(i32::MAX)) as i32
            })
        };
        EncounterIr {
            active: int("Active", 1) != 0,
            reset: int("Reset", 0) != 0,
            reset_time: int("ResetTime", 60),
            respawns: int("Respawns", 0),
            spawn_option: int("SpawnOption", 0),
            max_creatures: int("MaxCreatures", 8),
            rec_creatures: int("RecCreatures", 2),
            player_only: int("PlayerOnly", 0) != 0,
            faction: template.uint("Faction").unwrap_or(1),
            difficulty_index: int("DifficultyIndex", -1),
            difficulty: int("Difficulty", 0),
            creatures: template
                .list("CreatureList")
                .iter()
                .map(|entry| EncounterCreatureIr {
                    template: entry.resref("ResRef").unwrap_or_else(String::new),
                    cr: entry.float("CR").unwrap_or(0.0),
                    single: entry.int("SingleSpawn").is_some_and(|value| value != 0),
                    guaranteed: entry.int("GuaranteedCount").map_or(0, |value| value as i32),
                })
                .collect(),
            spawn_points: git
                .list("SpawnPointList")
                .iter()
                .map(|point| {
                    let axis = |label: &str| point.float(label).unwrap_or(0.0);
                    ([axis("X"), axis("Y"), axis("Z")], axis("Orientation"))
                })
                .collect(),
        }
    }
}

/// What stands between a blow and a door's or placeable's hit points.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Toughness {
    /// `Hardness`: whole points taken off every blow.
    pub hardness: u8,
    /// `Plot`: nothing hurts it.
    pub plot: bool,
    /// `Min1HP`: it is hurt down to 1 hit point and no further.
    pub min_one_hp: bool,
}

impl Toughness {
    /// The toughness `records` give, each field from the first that has it.
    pub fn read(records: &[&GffStruct]) -> Toughness {
        let field = |label: &str| {
            records
                .iter()
                .find_map(|record| record.int(label).map(|v| v as i32))
        };
        let flag = |label: &str| field(label).is_some_and(|value| value != 0);
        Toughness {
            hardness: field("Hardness").map_or(0, |value| value.clamp(0, 255) as u8),
            plot: flag("Plot"),
            min_one_hp: flag("Min1HP"),
        }
    }
}

/// A trap's fields (`TrapType`, a `traps.2da` row; `TrapDetectable`,
/// `TrapDisarmable`, `TrapOneShot`, `TrapDetectDC`, `DisarmDC`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct TrapIr {
    pub trap_type: u32,
    pub detectable: bool,
    pub disarmable: bool,
    pub one_shot: bool,
    pub detect_dc: u8,
    pub disarm_dc: u8,
}

impl TrapIr {
    /// The trap `records` give, each field from the first that has it;
    /// `None` without `TrapFlag` set.
    pub fn read(records: &[&GffStruct]) -> Option<TrapIr> {
        let field = |label: &str| records.iter().find_map(|record| record.get(label));
        let flag = |label: &str| {
            field(label)
                .and_then(GffValue::uint)
                .is_some_and(|value| value != 0)
        };
        let dc = |label: &str| {
            field(label)
                .and_then(GffValue::uint)
                .map_or(0, |value| value.min(255) as u8)
        };
        flag("TrapFlag").then(|| TrapIr {
            trap_type: field("TrapType").and_then(GffValue::uint).unwrap_or(0),
            detectable: flag("TrapDetectable"),
            disarmable: flag("TrapDisarmable"),
            one_shot: flag("TrapOneShot"),
            detect_dc: dc("TrapDetectDC"),
            disarm_dc: dc("DisarmDC"),
        })
    }
}

/// A door's or placeable's lock as its GFF has it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Lock {
    /// `Lockable`: a creature may lock it again.
    pub lockable: bool,
    /// `OpenLockDC`, `CloseLockDC`: Security's difficulty to unlock it and
    /// to lock it.
    pub open_dc: u8,
    pub close_dc: u8,
    /// `KeyName`: the tag of the item that opens it.
    pub key: Vec<u8>,
    /// `KeyRequired`: only the key opens it.
    pub key_required: bool,
    /// `AutoRemoveKey`: the key is used up opening it.
    pub auto_remove_key: bool,
}

impl Lock {
    /// The lock `records` give, each field from the first that has it.
    pub fn read(records: &[&GffStruct]) -> Lock {
        let field = |label: &str| records.iter().find_map(|record| record.get(label));
        let flag = |label: &str| {
            field(label)
                .and_then(GffValue::uint)
                .is_some_and(|value| value != 0)
        };
        let dc = |label: &str| {
            field(label)
                .and_then(GffValue::uint)
                .map_or(0, |value| value.min(255) as u8)
        };
        Lock {
            lockable: flag("Lockable"),
            open_dc: dc("OpenLockDC"),
            close_dc: dc("CloseLockDC"),
            key: field("KeyName")
                .and_then(GffValue::bytes)
                .unwrap_or(&[])
                .to_vec(),
            key_required: flag("KeyRequired"),
            auto_remove_key: flag("AutoRemoveKey"),
        }
    }
}

/// What `record` holds, when it says it holds anything (`HasInventory`
/// set, or an item list without that flag).
pub fn container(record: &GffStruct) -> Option<bool> {
    record
        .uint("HasInventory")
        .map(|value| value != 0)
        .or_else(|| (!record.list("ItemList").is_empty()).then_some(true))
}

/// How an instance's facing is stored.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Direction {
    /// `Bearing`: radians about Z.
    Bearing(f32),
    /// (`XOrientation`, `YOrientation`).
    Vector([f32; 2]),
}

#[derive(Debug, Clone, PartialEq)]
pub struct Camera {
    pub id: i32,
    pub position: [f32; 3],
    /// (w, x, y, z) as stored.
    pub orientation: [f32; 4],
    pub pitch: f32,
    pub field_of_view: f32,
    pub height: f32,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum Missing {
    RoomModel(String),
    RoomWalkmesh(String),
    Template(Kind, String),
    /// A door or placeable whose appearance names no model: (kind, template).
    InstanceModel(Kind, String),
    Layout,
    Visibility,
}

/// Why a module could not become a `ModuleIr`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ModuleRefusal {
    NoIfo,
    BadIfo(String),
    NoAreas,
    NoAre(String),
    BadAre(String, String),
    NoGit(String),
    BadGit(String, String),
}

/// The appearance tables doors and placeables name their models through.
struct Appearances {
    placeables: Option<TwoDa>,
    generic_doors: Option<TwoDa>,
    door_types: Option<TwoDa>,
}

impl Appearances {
    fn read(source: &dyn Source) -> Appearances {
        let table = |name: &str| {
            source
                .read(name, ResType::TWO_DA)
                .and_then(|bytes| TwoDa::parse(&bytes).ok())
        };
        Appearances {
            placeables: table("placeables"),
            generic_doors: table("genericdoors"),
            door_types: table("doortypes"),
        }
    }

    /// As xoreos loads a door's look: appearance 0 goes through
    /// `genericdoors` by generic type, any other through `doortypes`
    /// (reone: KOTOR doors always have appearance 0). The column is
    /// `ModelName`, else `Model`. Placeables: `placeables` by appearance.
    fn model(&self, instance: &Instance) -> Option<String> {
        let (table, row) = match instance.kind {
            Kind::Door => match instance.appearance? {
                0 => (self.generic_doors.as_ref()?, instance.generic_type?),
                appearance => (self.door_types.as_ref()?, appearance),
            },
            Kind::Placeable => (self.placeables.as_ref()?, instance.appearance?),
            _ => return None,
        };
        let column = table
            .column("modelname")
            .or_else(|| table.column("model"))?;
        table.named(row as usize, column)
    }

    /// Whether a door broken by blows plays its model's `busted`: its
    /// `genericdoors` row's `StaticAnim` says so (the executable reads it
    /// when a door is destroyed; Taris's `dor_lts05`, `dor_lts10`).
    fn busted(&self, instance: &Instance) -> bool {
        let (Kind::Door, Some(0), Some(row), Some(table)) = (
            instance.kind,
            instance.appearance,
            instance.generic_type,
            self.generic_doors.as_ref(),
        ) else {
            return false;
        };
        table
            .column("staticanim")
            .and_then(|column| table.named(row as usize, column))
            .is_some_and(|anim| anim.eq_ignore_ascii_case("busted"))
    }
}

/// A `CExoLocString` field: its strref when it has one, else its first
/// language's text.
pub fn localized(value: Option<&GffValue>) -> Title {
    match value {
        Some(GffValue::LocString(loc)) if loc.strref != u32::MAX => Title::StrRef(loc.strref),
        Some(GffValue::LocString(loc)) => loc
            .strings
            .first()
            .map_or(Title::None, |(_, text)| Title::Text(text.clone())),
        _ => Title::None,
    }
}

/// A struct's name: its first localized field of `NAMES`.
const NAMES: [&str; 3] = ["FirstName", "LocName", "LocalizedName"];

fn name(node: &GffStruct) -> Title {
    NAMES
        .iter()
        .map(|label| localized(node.get(label)))
        .find(|title| *title != Title::None)
        .unwrap_or(Title::None)
}

fn strref(value: Option<&GffValue>) -> Option<u32> {
    match value {
        Some(GffValue::LocString(loc)) if loc.strref != u32::MAX => Some(loc.strref),
        _ => None,
    }
}

/// The event scripts a struct names: non-empty resrefs under labels that
/// start with `On` or `Script` (an IFO's `Mod_On`).
pub fn scripts(node: &GffStruct) -> Vec<(String, String)> {
    node.fields
        .iter()
        .filter(|field| {
            let label = &field.label;
            label.starts_with(b"On") || label.starts_with(b"Script") || label.starts_with(b"Mod_On")
        })
        .filter_map(|field| match &field.value {
            GffValue::ResRef(bytes) if !bytes.is_empty() => Some((
                String::from_utf8_lossy(&field.label).into_owned(),
                String::from_utf8_lossy(bytes).to_ascii_lowercase(),
            )),
            _ => None,
        })
        .collect()
}

fn instance(kind: Kind, item: &GffStruct) -> Instance {
    let (position, facing, direction) = if item.get("X").is_some() {
        let bearing = item.float("Bearing").unwrap_or(0.0);
        (
            [
                item.float("X").unwrap_or(0.0),
                item.float("Y").unwrap_or(0.0),
                item.float("Z").unwrap_or(0.0),
            ],
            bearing,
            Direction::Bearing(bearing),
        )
    } else {
        let (dx, dy) = (
            item.float("XOrientation").unwrap_or(0.0),
            item.float("YOrientation").unwrap_or(0.0),
        );
        (
            [
                item.float("XPosition").unwrap_or(0.0),
                item.float("YPosition").unwrap_or(0.0),
                item.float("ZPosition").unwrap_or(0.0),
            ],
            dy.atan2(dx),
            Direction::Vector([dx, dy]),
        )
    };
    // `LinkedToFlags` 0 links to nothing, whatever the names say (xoreos;
    // the Taris hideout's door names its module, and its own script and
    // the party screen take the player out); one without the field is
    // read by its names.
    let unlinked = item.uint("LinkedToFlags") == Some(0);
    let linked_to = (matches!(kind, Kind::Door | Kind::Trigger) && !unlinked).then(|| {
        (
            item.resref("LinkedToModule").unwrap_or_else(String::new),
            item.bytes("LinkedTo").unwrap_or(&[]).to_vec(),
        )
    });
    let template_label = if kind == Kind::Store {
        "ResRef"
    } else {
        "TemplateResRef"
    };
    Instance {
        kind,
        template: item.resref(template_label).unwrap_or_else(String::new),
        tag: item.bytes("Tag").unwrap_or(&[]).to_vec(),
        position,
        facing,
        direction,
        linked_to,
        destination: strref(item.get("TransitionDestin")),
        locked: item.uint("Locked").is_some_and(|value| value != 0),
        animation_state: item.uint("AnimationState").unwrap_or(0),
        lock: Lock::default(),
        conversation: item.resref("Conversation").unwrap_or_else(String::new),
        scripts: scripts(item),
        // A trigger's points are `PointX`…, an encounter's `X`… (both
        // games' GITs).
        geometry: item
            .list("Geometry")
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
            .collect(),
        template_found: false,
        appearance: item.uint("Appearance"),
        generic_type: item.uint("GenericType"),
        model: None,
        busted: false,
        is_static: item.uint("Static").is_some_and(|value| value != 0),
        name: name(item),
        last_name: localized(item.get("LastName")),
        container: container(item),
        useable: item.uint("Useable").map(|value| value != 0),
        hit_points: None,
        toughness: Toughness::default(),
        faction: None,
        map_note: None,
        trap: None,
        encounter: None,
    }
}

impl ModuleIr {
    /// Builds the module from `module.ifo` and each area's ARE, GIT, LYT and
    /// VIS.
    pub fn build(name: &str, source: &dyn Source) -> Result<ModuleIr, ModuleRefusal> {
        let ifo = source
            .read("module", ResType::IFO)
            .ok_or(ModuleRefusal::NoIfo)?;
        let ifo = Gff::parse(&ifo).map_err(|error| ModuleRefusal::BadIfo(error.to_string()))?;
        let root = &ifo.root;
        let title = localized(root.get("Mod_Name"));
        let area_names: Vec<String> = root
            .list("Mod_Area_list")
            .iter()
            .map(|entry| entry.resref("Area_Name").unwrap_or_else(String::new))
            .collect();
        if area_names.is_empty() {
            return Err(ModuleRefusal::NoAreas);
        }
        let mut areas = Vec::new();
        for area in &area_names {
            areas.push(Self::area(area, source)?);
        }
        Ok(ModuleIr {
            name: name.to_ascii_lowercase(),
            title,
            entry_area: root.resref("Mod_Entry_Area").unwrap_or_else(String::new),
            entry_position: [
                root.float("Mod_Entry_X").unwrap_or(0.0),
                root.float("Mod_Entry_Y").unwrap_or(0.0),
                root.float("Mod_Entry_Z").unwrap_or(0.0),
            ],
            entry_direction: [
                root.float("Mod_Entry_Dir_X").unwrap_or(0.0),
                root.float("Mod_Entry_Dir_Y").unwrap_or(0.0),
            ],
            areas,
            scripts: scripts(root),
            minutes_per_hour: root.uint("Mod_MinPerHour").map_or(0, |v| v.min(255) as u8),
        })
    }

    fn area(area: &str, source: &dyn Source) -> Result<AreaIr, ModuleRefusal> {
        let are = source
            .read(area, ResType::ARE)
            .ok_or(ModuleRefusal::NoAre(area.into()))?;
        let are = Gff::parse(&are)
            .map_err(|error| ModuleRefusal::BadAre(area.into(), error.to_string()))?;
        let git = source
            .read(area, ResType::GIT)
            .ok_or(ModuleRefusal::NoGit(area.into()))?;
        let git = Gff::parse(&git)
            .map_err(|error| ModuleRefusal::BadGit(area.into(), error.to_string()))?;
        let mut missing = Vec::new();

        let layout = match source
            .read(area, ResType::LYT)
            .map(|bytes| Layout::parse(&bytes))
        {
            Some(Ok(layout)) => layout,
            _ => {
                missing.push(Missing::Layout);
                Layout::default()
            }
        };
        let visibility = match source.read(area, ResType::VIS) {
            Some(bytes) => Visibility::parse(&bytes),
            None => {
                missing.push(Missing::Visibility);
                Visibility::default()
            }
        };
        let are_rooms: Vec<String> = are
            .root
            .list("Rooms")
            .iter()
            .map(|room| room.resref("RoomName").unwrap_or_else(String::new))
            .collect();
        let force_ratings: Vec<(String, i32)> = are
            .root
            .list("Rooms")
            .iter()
            .filter_map(|room| {
                let rating = room.int("ForceRating").filter(|rating| *rating != 0)?;
                let name = room.resref("RoomName")?;
                Some((name.to_ascii_lowercase(), rating as i32))
            })
            .collect();
        let dry_rooms: Vec<String> = are
            .root
            .list("Rooms")
            .iter()
            .filter(|room| room.uint("DisableWeather").is_some_and(|on| on != 0))
            .map(|room| room.resref("RoomName").unwrap_or_else(String::new))
            .collect();
        // A room the layout names `****` is left out: no model, no walkmesh
        // (xoreos loads nothing for it; reone places no room
        // whose model does not load).
        let placed_rooms: Vec<&Placed> = layout
            .rooms
            .iter()
            .filter(|room| !fmt_odyssey::is_blank(room.model.as_bytes()))
            .collect();
        let mut rooms = Vec::new();
        for placed in &placed_rooms {
            let has_model = source.exists(&placed.model, ResType::MDL);
            let has_walkmesh = source.exists(&placed.model, ResType::WOK);
            if !has_model {
                missing.push(Missing::RoomModel(placed.model.clone()));
            }
            if !has_walkmesh {
                missing.push(Missing::RoomWalkmesh(placed.model.clone()));
            }
            rooms.push(RoomIr {
                model: placed.model.clone(),
                position: placed.position,
                visible: visibility
                    .visible_from(&placed.model)
                    .map(<[String]>::to_vec)
                    .unwrap_or_else(Vec::new),
                has_walkmesh,
                disable_weather: dry_rooms.contains(&placed.model.to_ascii_lowercase()),
                force_rating: force_ratings
                    .iter()
                    .find(|(name, _)| name.eq_ignore_ascii_case(&placed.model))
                    .map_or(0, |(_, rating)| *rating),
            });
        }
        let placed: Vec<&str> = placed_rooms
            .iter()
            .map(|room| room.model.as_str())
            .collect();
        let rooms_only_in_are = are_rooms
            .iter()
            .filter(|room| !placed.contains(&room.as_str()))
            .cloned()
            .collect();
        let rooms_only_in_lyt = placed
            .iter()
            .filter(|room| !are_rooms.iter().any(|are_room| are_room == *room))
            .map(|room| room.to_string())
            .collect();

        let appearances = Appearances::read(source);
        let mut instances = Vec::new();
        for kind in Kind::ALL {
            let (label, extension) = kind.list_and_type();
            for item in git.root.list(label) {
                let mut instance = instance(kind, item);
                instance.template_found = source.exists(&instance.template, extension);
                if !instance.template_found {
                    missing.push(Missing::Template(kind, instance.template.clone()));
                }
                // xoreos loads the blueprint, then the instance over it.
                let blueprint = instance
                    .template_found
                    .then(|| source.read(&instance.template, extension))
                    .flatten()
                    .and_then(|bytes| Gff::parse(&bytes).ok());
                if let Some(blueprint) = &blueprint {
                    let root = &blueprint.root;
                    if item.get("Tag").is_none() {
                        instance.tag = root.bytes("Tag").unwrap_or(&[]).to_vec();
                    }
                    if item.get("Locked").is_none() {
                        instance.locked = root.uint("Locked").is_some_and(|value| value != 0);
                    }
                    if item.get("AnimationState").is_none() {
                        instance.animation_state = root.uint("AnimationState").unwrap_or(0);
                    }
                    if item.get("Conversation").is_none() {
                        instance.conversation =
                            root.resref("Conversation").unwrap_or_else(String::new);
                    }
                    if instance.name == Title::None {
                        instance.name = name(root);
                    }
                    if instance.last_name == Title::None {
                        instance.last_name = localized(root.get("LastName"));
                    }
                    let own = std::mem::take(&mut instance.scripts);
                    let mut all: Vec<(String, String)> = scripts(root)
                        .into_iter()
                        .filter(|(label, _)| !own.iter().any(|(mine, _)| mine == label))
                        .collect();
                    all.extend(own);
                    instance.scripts = all;
                }
                let records: Vec<&GffStruct> = std::iter::once(item)
                    .chain(blueprint.as_ref().map(|b| &b.root))
                    .collect();
                if kind == Kind::Waypoint {
                    instance.map_note = MapNote::read(&records);
                }
                if kind == Kind::Encounter
                    && let Some(blueprint) = &blueprint
                {
                    let ute = EncounterIr::read(&blueprint.root, item);
                    instance.faction = Some(ute.faction);
                    instance.encounter = Some(ute);
                }
                if matches!(kind, Kind::Door | Kind::Placeable) {
                    instance.lock = Lock::read(&records);
                    let field = |label: &str| {
                        records
                            .iter()
                            .find_map(|record| record.int(label).map(|v| v as i32))
                    };
                    instance.hit_points =
                        field("HP").map(|most| (field("CurrentHP").unwrap_or(most), most));
                    instance.toughness = Toughness::read(&records);
                    instance.faction = records.iter().find_map(|record| record.uint("Faction"));
                    if let Some(blueprint) = &blueprint {
                        instance.appearance =
                            instance.appearance.or(blueprint.root.uint("Appearance"));
                        instance.generic_type =
                            instance.generic_type.or(blueprint.root.uint("GenericType"));
                        if item.get("Static").is_none() {
                            instance.is_static = blueprint
                                .root
                                .uint("Static")
                                .is_some_and(|value| value != 0);
                        }
                        if kind == Kind::Placeable && instance.container.is_none() {
                            instance.container = container(&blueprint.root);
                        }
                        if kind == Kind::Placeable && instance.useable.is_none() {
                            instance.useable =
                                blueprint.root.uint("Useable").map(|value| value != 0);
                        }
                    }
                    instance.model = appearances.model(&instance);
                    instance.busted = appearances.busted(&instance);
                    if instance.model.is_none() {
                        missing.push(Missing::InstanceModel(kind, instance.template.clone()));
                    }
                }
                if matches!(kind, Kind::Trigger | Kind::Door | Kind::Placeable) {
                    let records: Vec<&GffStruct> = std::iter::once(item)
                        .chain(blueprint.as_ref().map(|b| &b.root))
                        .collect();
                    instance.trap = TrapIr::read(&records);
                    if kind == Kind::Trigger {
                        // A trap trigger's faction (whom it spares) and key.
                        instance.faction = records.iter().find_map(|record| record.uint("Faction"));
                        instance.lock = Lock::read(&records);
                    }
                }
                instances.push(instance);
            }
        }
        let cameras = git
            .root
            .list("CameraList")
            .iter()
            .map(|camera| Camera {
                id: camera.int("CameraID").map_or(-1, |id| id as i32),
                position: camera.vector("Position").unwrap_or([0.0; 3]),
                orientation: match camera.get("Orientation") {
                    Some(GffValue::Orientation(bits)) => bits.map(f32::from_bits),
                    _ => [1.0, 0.0, 0.0, 0.0],
                },
                pitch: camera.float("Pitch").unwrap_or(0.0),
                field_of_view: camera.float("FieldOfView").unwrap_or(0.0),
                height: camera.float("Height").unwrap_or(0.0),
            })
            .collect();
        missing.sort();
        missing.dedup();
        let root = &are.root;
        let grass = Some(root.resref("Grass_TexName").unwrap_or_else(String::new))
            .filter(|texture| !texture.is_empty())
            .map(|texture| AreaGrass {
                texture,
                density: root.float("Grass_Density").unwrap_or(0.0),
                quad_size: root.float("Grass_QuadSize").unwrap_or(0.0),
                ambient: root.uint("Grass_Ambient").unwrap_or(0),
                diffuse: root.uint("Grass_Diffuse").unwrap_or(0),
                emissive: root.uint("Grass_Emissive").unwrap_or(0),
                chances: [
                    "Grass_Prob_UL",
                    "Grass_Prob_UR",
                    "Grass_Prob_LL",
                    "Grass_Prob_LR",
                ]
                .map(|label| root.float(label).unwrap_or(0.0)),
            })
            .filter(|grass| grass.density > 0.0);
        // An interior area (ARE `Flags` bit 0) never has weather: the
        // executable zeroes its four values at load.
        let interior = root.uint("Flags").is_some_and(|flags| flags & 1 != 0);
        let weather = if interior {
            AreaWeather::default()
        } else {
            AreaWeather {
                rain: root.int("ChanceRain").map(|v| v as i32).unwrap_or(0),
                snow: root.int("ChanceSnow").map(|v| v as i32).unwrap_or(0),
                lightning: root.int("ChanceLightning").map(|v| v as i32).unwrap_or(0),
                wind: root.int("WindPower").map(|v| v as i32).unwrap_or(0),
            }
        };
        Ok(AreaIr {
            resref: area.to_string(),
            dyn_ambient: are.root.uint("DynAmbientColor"),
            fog: are
                .root
                .uint("SunFogOn")
                .is_some_and(|on| on != 0)
                .then(|| AreaFog {
                    near: are.root.float("SunFogNear").unwrap_or(0.0),
                    far: are.root.float("SunFogFar").unwrap_or(0.0),
                    color: are.root.uint("SunFogColor").unwrap_or(0),
                }),
            name_strref: strref(are.root.get("Name")),
            tag: are.root.bytes("Tag").unwrap_or(&[]).to_vec(),
            rooms,
            load_screen: are.root.uint("LoadScreenID"),
            rooms_only_in_are,
            rooms_only_in_lyt,
            instances,
            cameras,
            missing,
            scripts: scripts(&are.root),
            minigame: crate::minigame::MiniGameIr::read(&are.root, &layout),
            map: AreaMap::read(&are.root),
            stealth: {
                let root = &are.root;
                let xp_max = root.int("StealthXPMax").map(|v| v as i32).unwrap_or(0);
                AreaStealthIr {
                    mod_listen: root.int("ModListenCheck").map(|v| v as i32).unwrap_or(0),
                    xp_max,
                    xp_current: root
                        .int("StealthXPCurrent")
                        .map(|v| v as i32)
                        .unwrap_or(xp_max),
                    xp_loss: root.int("StealthXPLoss").map(|v| v as i32).unwrap_or(0),
                    xp_enabled: root.uint("StealthXPEnabled").is_some_and(|on| on != 0),
                }
            },
            grass,
            weather,
            disable_transit: are.root.uint("DisableTransit").is_some_and(|on| on != 0),
        })
    }
}

pub(crate) fn check(ok: bool, what: String) -> Result<String, String> {
    if ok { Ok(what) } else { Err(what) }
}

/// The checks of the ARE's grass and weather facts, from modules made here.
pub fn probes() -> Vec<(&'static str, Result<String, String>)> {
    use crate::made::{blank_module, node, resref};
    let float = |value: f32| GffValue::Float(value.to_bits());
    let room = |name: &str, dry: u8| {
        node(vec![
            ("RoomName", GffValue::String(name.as_bytes().to_vec())),
            ("DisableWeather", GffValue::Byte(dry)),
        ])
    };
    let wet = blank_module(
        vec![
            ("Grass_TexName", resref("lda_grass2")),
            ("Grass_Density", float(4.0)),
            ("Grass_QuadSize", float(0.75)),
            ("Grass_Prob_UL", float(0.1)),
            ("Grass_Prob_UR", float(0.2)),
            ("Grass_Prob_LL", float(0.3)),
            ("Grass_Prob_LR", float(0.4)),
            ("ChanceRain", GffValue::Int(100)),
            ("ChanceLightning", GffValue::Int(100)),
            ("WindPower", GffValue::Int(1)),
            (
                "Rooms",
                GffValue::List(vec![room("room_a", 0), room("room_b", 1)]),
            ),
        ],
        &["room_a", "room_b"],
    );
    let bare = blank_module(vec![("Grass_TexName", resref(""))], &["room_a"]);
    let indoor = blank_module(
        vec![
            ("Flags", GffValue::Dword(1)),
            ("ChanceRain", GffValue::Int(100)),
        ],
        &["room_a"],
    );
    let area = |files| ModuleIr::build("blank", &files).map(|module| module.areas[0].clone());
    let (wet, bare, indoor) = match (area(wet), area(bare), area(indoor)) {
        (Ok(wet), Ok(bare), Ok(indoor)) => (wet, bare, indoor),
        (wet, bare, indoor) => {
            return vec![(
                "are_builds",
                Err(format!(
                    "{:?} {:?} {:?}",
                    wet.err(),
                    bare.err(),
                    indoor.err()
                )),
            )];
        }
    };
    let grass = wet.grass.clone();
    let dry: Vec<bool> = wet.rooms.iter().map(|room| room.disable_weather).collect();
    vec![
        (
            "are_grass",
            check(
                grass.as_ref().is_some_and(|g| {
                    g.texture == "lda_grass2"
                        && g.density == 4.0
                        && g.quad_size == 0.75
                        && g.chances == [0.1, 0.2, 0.3, 0.4]
                }),
                format!("{grass:?}"),
            ),
        ),
        (
            "are_no_grass_texture",
            check(bare.grass.is_none(), format!("{:?}", bare.grass)),
        ),
        (
            "are_weather",
            check(
                wet.weather
                    == AreaWeather {
                        rain: 100,
                        snow: 0,
                        lightning: 100,
                        wind: 1,
                    },
                format!("{:?}", wet.weather),
            ),
        ),
        (
            "are_dry_rooms",
            check(dry == [false, true], format!("{dry:?}")),
        ),
        (
            "are_interior_no_weather",
            check(
                indoor.weather == AreaWeather::default(),
                format!("{:?}", indoor.weather),
            ),
        ),
    ]
}

/// The ARE facts of three modules of the games, read from `source` (the
/// module's own, opened by the caller): TSL's `401dxn` (rain and lightning,
/// some rooms dry), `261tel` (snow), and K1's `danm14aa` (no rain, snow or
/// lightning, the grass `lda_grass2`). No checks for another module name.
pub fn install_probes(
    module: &str,
    source: &dyn Source,
) -> Vec<(&'static str, Result<String, String>)> {
    let area = match ModuleIr::build(module, source) {
        Ok(built) => built.areas.into_iter().next(),
        Err(refusal) => return vec![("game_are_builds", Err(format!("{module}: {refusal:?}")))],
    };
    let Some(area) = area else {
        return vec![("game_are_builds", Err(format!("{module}: no area")))];
    };
    let weather = area.weather;
    let dry = area
        .rooms
        .iter()
        .filter(|room| room.disable_weather)
        .count();
    let wet = area.rooms.len() - dry;
    match module {
        "401dxn" => vec![(
            "game_dxn_weather",
            check(
                weather.rain == 100 && weather.lightning == 100 && dry > 0 && wet > 0,
                format!("{weather:?}, {dry} dry rooms, {wet} wet"),
            ),
        )],
        "261tel" => vec![(
            "game_tel_snow",
            check(weather.snow == 100, format!("{weather:?}")),
        )],
        "danm14aa" => vec![(
            "game_danm14aa_grass",
            check(
                (weather.rain, weather.snow, weather.lightning) == (0, 0, 0)
                    && area
                        .grass
                        .as_ref()
                        .is_some_and(|grass| grass.texture == "lda_grass2"),
                format!("{weather:?}, {:?}", area.grass),
            ),
        )],
        _ => Vec::new(),
    }
}
