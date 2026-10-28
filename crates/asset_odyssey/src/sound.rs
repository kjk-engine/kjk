//! What an area says of its sound, for the view (`docs/SOUND.md`):
//!
//! - its tracks: the GIT's `AreaProperties` (`MusicDay`, `MusicNight`,
//!   `MusicBattle`, `MusicDelay`, `AmbientSndDay`, `AmbientSndNight`,
//!   `AmbientSndDayVol`, `AmbientSndNitVol`) as rows of `ambientmusic.2da`
//!   and `ambientsound.2da` (column `Resource`; row 0 none), and the ARE's
//!   `IsNight`;
//! - each sound object's template (UTS): its sounds and how it plays them;
//! - each door's and placeable's sounds: `placeableobjsnds.2da` by its
//!   appearance's `SoundAppType` (a door's through `genericdoors.2da` by
//!   its generic type, or `doortypes.2da` by a non-zero appearance; a
//!   placeable's through `placeables.2da`);
//! - each creature's footsteps: its appearance's (`Appearance_Type`)
//!   `footsteptype` in `appearance.2da`, a row of `footstepsounds.2da`
//!   (`Tables::steps`), whose column is the floor's: `surfacemat.2da`'s
//!   `sound` code (dirt, grass, stone, wood, water, carpet, metal, leaves).
//!
//! The objects come in the GIT's order, kind by kind as `Kind::ALL` lists
//! them: the order of the module's objects (`ModuleIr`'s instances).

use fmt_odyssey::ResType;
use fmt_odyssey::gff::{Gff, GffStruct, GffValue};
use fmt_odyssey::twoda::TwoDa;

use crate::module::{Kind, Source};

/// The area's tracks as its files set them (rows of the tables).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Tracks {
    pub music_day: u16,
    pub music_night: u16,
    pub battle: u16,
    pub ambient_day: u16,
    pub ambient_night: u16,
    /// 0 to 127.
    pub ambient_day_volume: u8,
    pub ambient_night_volume: u8,
    /// `MusicDelay` (ms): the pause before the music begins again.
    pub music_delay_ms: u32,
    /// The ARE's `IsNight`: its night tracks play.
    pub night: bool,
}

/// The floors `footstepsounds.2da` has columns for, in `surfacemat.2da`'s
/// codes and the columns' names.
const FLOORS: [(&str, &str); 8] = [
    ("DT", "dirt"),
    ("GR", "grass"),
    ("ST", "stone"),
    ("WD", "wood"),
    ("WT", "water"),
    ("CP", "carpet"),
    ("MT", "metal"),
    ("LV", "leaves"),
];

/// A row of `footstepsounds.2da`: three sounds for each floor (`FLOORS`),
/// or a sound held while moving (`rolling`: wheels, a hover), and the
/// pitch it moves by.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Steps {
    pub floors: [[Option<String>; 3]; 8],
    pub rolling: Option<String>,
    pub pitch: f32,
}

/// The rows of `ambientmusic.2da` and `ambientsound.2da`: each one's file
/// (and the battle music's stingers); the footsteps' tables.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Tables {
    music: Vec<(Option<String>, Vec<String>)>,
    ambient: Vec<Option<String>>,
    /// `footstepsounds.2da` by row, and each `surfacemat.2da` row's floor
    /// (a `FLOORS` index).
    steps: Vec<Steps>,
    floors: Vec<Option<usize>>,
}

impl Tables {
    pub fn read(source: &dyn Source) -> Tables {
        let table = |name| {
            source
                .read(name, ResType::TWO_DA)
                .and_then(|bytes| TwoDa::parse(&bytes).ok())
        };
        let rows = |table: &TwoDa, columns: &[&str]| -> Vec<Vec<Option<String>>> {
            let columns: Vec<Option<usize>> = columns.iter().map(|c| table.column(c)).collect();
            (0..table.cells.len())
                .map(|row| {
                    columns
                        .iter()
                        .map(|column| column.and_then(|column| table.named(row, column)))
                        .collect()
                })
                .collect()
        };
        let music = table("ambientmusic").map_or_else(Vec::new, |table| {
            rows(&table, &["resource", "stinger1", "stinger2", "stinger3"])
                .into_iter()
                .map(|mut row| {
                    let stingers = row.drain(1..).flatten().collect();
                    (row.remove(0), stingers)
                })
                .collect()
        });
        let ambient = table("ambientsound").map_or_else(Vec::new, |table| {
            rows(&table, &["resource"])
                .into_iter()
                .map(|mut row| row.remove(0))
                .collect()
        });
        let steps = table("footstepsounds").map_or_else(Vec::new, |table| {
            (0..table.cells.len())
                .map(|row| {
                    let mut steps = Steps {
                        rolling: table
                            .column("rolling")
                            .and_then(|column| table.named(row, column)),
                        pitch: table.float(row, "pitchoffset").unwrap_or(0.0),
                        ..Steps::default()
                    };
                    for (floor, (_, name)) in steps.floors.iter_mut().zip(FLOORS) {
                        for (n, sound) in floor.iter_mut().enumerate() {
                            *sound = table
                                .column(&format!("{name}{n}"))
                                .and_then(|column| table.named(row, column));
                        }
                    }
                    steps
                })
                .collect()
        });
        let floors = table("surfacemat").map_or_else(Vec::new, |table| {
            let sound = table.column("sound");
            (0..table.cells.len())
                .map(|row| {
                    let code = table.get(row, sound?)?;
                    FLOORS
                        .iter()
                        .position(|(floor, _)| floor.as_bytes().eq_ignore_ascii_case(code))
                })
                .collect()
        });
        Tables {
            music,
            ambient,
            steps,
            floors,
        }
    }

    /// Footstep row `row`.
    pub fn steps(&self, row: u8) -> Option<&Steps> {
        self.steps.get(usize::from(row))
    }

    /// A step's three sounds on a floor of `surfacemat.2da` row `material`
    /// (none where the floor makes no sound).
    pub fn step_sounds<'a>(&self, steps: &'a Steps, material: u16) -> &'a [Option<String>; 3] {
        const NONE: &[Option<String>; 3] = &[None, None, None];
        match self.floors.get(usize::from(material)).copied().flatten() {
            Some(floor) => &steps.floors[floor],
            None => NONE,
        }
    }

    /// The music of row `row` (row 0 is none).
    pub fn music(&self, row: u16) -> Option<&str> {
        self.music_row(row)?.0.as_deref()
    }

    /// The stingers of music row `row`, played as a fight's music ends.
    pub fn stingers(&self, row: u16) -> &[String] {
        self.music_row(row).map_or(&[], |(_, stingers)| stingers)
    }

    fn music_row(&self, row: u16) -> Option<&(Option<String>, Vec<String>)> {
        (row != 0).then(|| self.music.get(usize::from(row)))?
    }

    /// The ambience of row `row` (row 0 is none).
    pub fn ambient(&self, row: u16) -> Option<&str> {
        (row != 0)
            .then(|| self.ambient.get(usize::from(row)))??
            .as_deref()
    }

    /// Every file footstep row `row` names.
    pub fn step_files(&self, row: u8) -> impl Iterator<Item = &str> {
        self.steps(row)
            .into_iter()
            .flat_map(|steps| steps.floors.iter().flatten().chain([&steps.rolling]))
            .flatten()
            .map(String::as_str)
    }
}

/// A sound object (UTS), as it plays.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Emitter {
    /// Playing from the start (`Active`).
    pub active: bool,
    /// `Continuous`: one sound after another without end (else each once).
    pub continuous: bool,
    /// `Looping`: a single sound looped seamlessly; several, one after
    /// another without end, as `continuous`.
    pub looping: bool,
    /// `Positional`: heard from where it stands, else alike everywhere.
    pub positional: bool,
    /// `RandomPosition`: each sound from a random spot within `range` (m) of it.
    pub random_position: bool,
    pub range: [f32; 2],
    /// `Random`: the next sound picked at random, else in order.
    pub random: bool,
    /// Metres above its position.
    pub elevation: f32,
    /// Full volume within `near`, nothing beyond `far` (m).
    pub near: f32,
    pub far: f32,
    /// The pause between one sound's end and the next's start, and how much
    /// it varies either way (ms).
    pub interval_ms: u32,
    pub interval_variation_ms: u32,
    /// How far a sound's pitch varies either way (a fraction).
    pub pitch_variation: f32,
    /// 0 to 127, and how much it varies either way.
    pub volume: u8,
    pub volume_variation: u8,
    pub sounds: Vec<String>,
}

impl Emitter {
    fn read(uts: &GffStruct) -> Emitter {
        let flag = |label| uts.uint(label).is_some_and(|v| v != 0);
        Emitter {
            active: flag("Active"),
            continuous: flag("Continuous"),
            looping: flag("Looping"),
            positional: flag("Positional"),
            random_position: flag("RandomPosition"),
            range: [
                uts.float("RandomRangeX").unwrap_or(0.0),
                uts.float("RandomRangeY").unwrap_or(0.0),
            ],
            random: flag("Random"),
            elevation: uts.float("Elevation").unwrap_or(0.0),
            near: uts.float("MinDistance").unwrap_or(0.0),
            far: uts.float("MaxDistance").unwrap_or(0.0),
            interval_ms: uts.uint("Interval").unwrap_or(0),
            interval_variation_ms: uts.uint("IntervalVrtn").unwrap_or(0),
            pitch_variation: uts.float("PitchVariation").unwrap_or(0.0),
            volume: uts.uint("Volume").map_or(127, |v| v.min(127) as u8),
            volume_variation: uts.uint("VolumeVrtn").map_or(0, |v| v.min(127) as u8),
            sounds: match uts.get("Sounds") {
                Some(GffValue::List(items)) => items
                    .iter()
                    .filter_map(|item| match item.get("Sound") {
                        Some(GffValue::ResRef(bytes)) if !bytes.is_empty() => {
                            Some(String::from_utf8_lossy(bytes).to_ascii_lowercase())
                        }
                        _ => None,
                    })
                    .collect(),
                _ => Vec::new(),
            },
        }
    }
}

/// A door's or placeable's sounds (`placeableobjsnds.2da`).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Moves {
    pub opened: Option<String>,
    pub closed: Option<String>,
    pub locked: Option<String>,
    pub used: Option<String>,
    pub destroyed: Option<String>,
}

/// What an object sounds like.
#[derive(Debug, Clone, PartialEq)]
pub enum Voice {
    Emitter(Emitter),
    Moves(Moves),
    /// A creature: its footsteps' row of `footstepsounds.2da`.
    Steps(u8),
    None,
}

/// An area's sound: its tracks, and each object's, in the module's order.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct AreaSound {
    pub tracks: Tracks,
    pub objects: Vec<Voice>,
}

impl AreaSound {
    /// Every file the area plays (not the tables' tracks).
    pub fn files(&self) -> impl Iterator<Item = &str> {
        self.objects.iter().flat_map(|voice| -> Vec<&str> {
            match voice {
                Voice::Emitter(emitter) => emitter.sounds.iter().map(String::as_str).collect(),
                Voice::Moves(moves) => [
                    &moves.opened,
                    &moves.closed,
                    &moves.locked,
                    &moves.used,
                    &moves.destroyed,
                ]
                .into_iter()
                .flatten()
                .map(String::as_str)
                .collect(),
                Voice::Steps(_) | Voice::None => Vec::new(),
            }
        })
    }

    /// The footstep rows its creatures walk with.
    pub fn step_rows(&self) -> impl Iterator<Item = u8> + '_ {
        self.objects.iter().filter_map(|voice| match voice {
            Voice::Steps(row) => Some(*row),
            _ => None,
        })
    }
}

/// Area `area`'s sound, from its ARE, GIT and the templates; `None` when
/// its GIT does not read.
pub fn area(source: &dyn Source, area: &str) -> Option<AreaSound> {
    let gff = |resref: &str, extension: ResType| {
        source
            .read(resref, extension)
            .and_then(|bytes| Gff::parse(&bytes).ok())
    };
    let git = gff(area, ResType::GIT)?;
    let properties = match git.root.get("AreaProperties") {
        Some(GffValue::Struct(properties)) => Some(&**properties),
        _ => None,
    };
    let row = |label| {
        properties
            .and_then(|p| p.uint(label))
            .map_or(0, |v| v.min(u32::from(u16::MAX)) as u16)
    };
    let volume = |label| {
        properties
            .and_then(|p| p.uint(label))
            .map_or(127, |v| v.min(127) as u8)
    };
    let night =
        gff(area, ResType::ARE).is_some_and(|are| are.root.uint("IsNight").is_some_and(|v| v != 0));
    let tracks = Tracks {
        music_day: row("MusicDay"),
        music_night: row("MusicNight"),
        battle: row("MusicBattle"),
        ambient_day: row("AmbientSndDay"),
        ambient_night: row("AmbientSndNight"),
        ambient_day_volume: volume("AmbientSndDayVol"),
        ambient_night_volume: volume("AmbientSndNitVol"),
        music_delay_ms: properties.and_then(|p| p.uint("MusicDelay")).unwrap_or(0),
        night,
    };
    let table = |name: &str| {
        source
            .read(name, ResType::TWO_DA)
            .and_then(|bytes| TwoDa::parse(&bytes).ok())
    };
    let (sounds, generic_doors, door_types, placeables, appearances) = (
        table("placeableobjsnds"),
        table("genericdoors"),
        table("doortypes"),
        table("placeables"),
        table("appearance"),
    );
    let moves = |row: Option<u32>| -> Voice {
        let (Some(table), Some(row)) = (&sounds, row) else {
            return Voice::None;
        };
        let cell = |column: &str| {
            table
                .column(column)
                .and_then(|column| table.named(row as usize, column))
        };
        Voice::Moves(Moves {
            opened: cell("opened"),
            closed: cell("closed"),
            locked: cell("locked"),
            used: cell("used"),
            destroyed: cell("destroyed"),
        })
    };
    let number = |table: &Option<TwoDa>, column: &str, row: Option<u32>| -> Option<u32> {
        u32::try_from(table.as_ref()?.int(row? as usize, column)?).ok()
    };
    let app_type = |table: &Option<TwoDa>, row: Option<u32>| number(table, "soundapptype", row);
    let mut objects = Vec::new();
    for kind in Kind::ALL {
        let (label, extension) = kind.list_and_type();
        let Some(GffValue::List(items)) = git.root.get(label) else {
            continue;
        };
        for item in items {
            let template = item
                .resref(if kind == Kind::Store {
                    "ResRef"
                } else {
                    "TemplateResRef"
                })
                .unwrap_or_else(String::new);
            let blueprint = match kind {
                Kind::Sound | Kind::Door | Kind::Placeable | Kind::Creature => {
                    gff(&template, extension)
                }
                _ => None,
            };
            // The instance's value, else the template's.
            let field = |label| {
                item.uint(label)
                    .or_else(|| blueprint.as_ref()?.root.uint(label))
            };
            objects.push(match kind {
                Kind::Sound => blueprint
                    .as_ref()
                    .map_or(Voice::None, |uts| Voice::Emitter(Emitter::read(&uts.root))),
                Kind::Door => match field("Appearance") {
                    Some(0) | None => moves(app_type(&generic_doors, field("GenericType"))),
                    appearance => moves(app_type(&door_types, appearance)),
                },
                Kind::Placeable => moves(app_type(&placeables, field("Appearance"))),
                Kind::Creature => number(&appearances, "footsteptype", field("Appearance_Type"))
                    .map_or(Voice::None, |row| Voice::Steps(row.min(255) as u8)),
                _ => Voice::None,
            });
        }
    }
    Some(AreaSound { tracks, objects })
}
