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
    /// `MusicDelay` (ms): the pause before the music first plays in the
    /// area, and before it begins again after each time it ends (`Song`).
    pub music_delay_ms: u32,
    /// The ARE's `IsNight`: its night tracks play.
    pub night: bool,
}

/// The area's music's turn, as K1's executable times it. When the
/// player enters an area (a module change or a load alike), the music waits
/// the area's `MusicDelay` before it first plays. It plays its track through
/// to the end, then waits `MusicDelay` again before it begins anew. The last
/// area's track never carries over into the next. A script's
/// `MusicBackgroundPlay` starts the music at once, and a new track (a
/// script's `MusicBackgroundChangeDay`) starts at once too.
#[derive(Debug, Clone, PartialEq)]
pub enum Song {
    /// The area was just entered: its first wait begins on the first moment
    /// its music is known.
    Entered,
    /// The track playing (its name).
    Playing(String),
    /// Waiting until that moment (ms).
    Resting(f64),
}

impl Song {
    /// The music's turn at `now` (ms) for the area's track `track`, after
    /// `last`. `heard` says whether that track's sound is still playing,
    /// `started` whether a script started the music since the last moment,
    /// and `delay_ms` is the area's `MusicDelay`. Returns the new turn and
    /// whether the track plays now.
    pub fn turn(
        last: Option<Song>,
        track: &str,
        (heard, started): (bool, bool),
        now: f64,
        delay_ms: f64,
    ) -> (Song, bool) {
        match last {
            Some(Song::Playing(playing)) if playing == track => {
                if heard {
                    (Song::Playing(playing), true)
                } else {
                    (Song::Resting(now + delay_ms), false)
                }
            }
            Some(Song::Entered) if !started => (Song::Resting(now + delay_ms), false),
            Some(Song::Resting(until)) if until > now && !started => (Song::Resting(until), false),
            _ => (Song::Playing(track.to_string()), true),
        }
    }
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

/// A `prioritygroups.2da` row: the executables' kinds of sound. A sound
/// plays at its group's volume (out of 127) on top of its own, and a placed
/// one at full volume within its group's near distance, falling to nothing
/// at its far one. These are the rows the executables give the sounds kjk
/// plays from KOTOR's files (read in TSL's Mac executable, which keeps its
/// function names).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Group {
    Stingers = 1,
    Music = 2,
    /// A sound object's loop heard alike everywhere.
    AreaAmbient = 4,
    /// A sound object's loop heard from where it stands.
    PositionalAmbient = 5,
    Conversation = 9,
    /// A script's `PlaySound`.
    Scripted = 10,
    /// A visual effect's sound.
    SpellEffect = 13,
    /// A sound set's line (a grunt, a battle cry) by a creature of size
    /// category 4 or more, and by a smaller one.
    LargeVoice = 16,
    SmallVoice = 17,
    /// A footstep or a droid's rolling or hovering sound: a creature's of
    /// size category 4 or more, the player's character's, and a smaller
    /// creature's.
    LargeSteps = 18,
    PlayerSteps = 19,
    SmallSteps = 20,
    /// A sound object's single sound heard alike everywhere, and one heard
    /// from where it stands (also a door's or a container's sound).
    GlobalShot = 21,
    PositionalShot = 22,
}

/// The highest `sizecategory` a creature of the small groups has.
const SMALL_SIZE: u8 = 3;

/// The volume sliders' default (85 of 127), the same in both games.
const SLIDER: f32 = 85.0 / 127.0;

/// A `prioritygroups.2da` row as read: its volume (0 to 127) and the
/// distances a placed sound of it is full within and silent beyond (m).
#[derive(Debug, Clone, Copy, PartialEq)]
struct GroupRow {
    volume: u8,
    range: Option<(f32, f32)>,
}

/// The rows of `ambientmusic.2da` and `ambientsound.2da`: each one's file
/// (and the battle music's stingers); the footsteps' tables; the priority
/// groups' volumes.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Tables {
    music: Vec<(Option<String>, Vec<String>)>,
    ambient: Vec<Option<String>>,
    /// `footstepsounds.2da` by row, and each `surfacemat.2da` row's floor
    /// (a `FLOORS` index).
    steps: Vec<Steps>,
    floors: Vec<Option<usize>>,
    /// Each `appearance.2da` row's footstep row (`footsteptype`) and size
    /// (`sizecategory`).
    walks: Vec<Option<u8>>,
    sizes: Vec<Option<u8>>,
    /// `prioritygroups.2da` by row (TSL's `_pc` columns).
    groups: Vec<Option<GroupRow>>,
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
        let (walks, sizes) = table("appearance").map_or_else(Default::default, |table| {
            let column = |name| {
                (0..table.cells.len())
                    .map(|row| u8::try_from(table.int(row, name)?).ok())
                    .collect()
            };
            (column("footsteptype"), column("sizecategory"))
        });
        let groups = table("prioritygroups").map_or_else(Vec::new, |table| {
            // TSL names its PC columns `…_pc`, K1 has no suffix.
            let named = |name: &str| {
                [format!("{name}_pc"), name.to_string()]
                    .into_iter()
                    .find(|column| table.column(column).is_some())
            };
            let [volume, near, far] = ["volume", "maxvolumedist", "minvolumedist"].map(named);
            (0..table.cells.len())
                .map(|row| {
                    let volume = u8::try_from(table.int(row, volume.as_deref()?)?).ok()?;
                    let distance = |column: &Option<String>| table.float(row, column.as_deref()?);
                    let range = distance(&near).zip(distance(&far));
                    Some(GroupRow {
                        volume: volume.min(127),
                        range,
                    })
                })
                .collect()
        });
        Tables {
            music,
            ambient,
            steps,
            floors,
            walks,
            sizes,
            groups,
        }
    }

    fn group(&self, group: Group) -> Option<GroupRow> {
        self.groups.get(group as usize).copied().flatten()
    }

    /// The executables' gain for a sound of `group`: its row's volume over
    /// 127 (full where the table has none).
    pub fn group_gain(&self, group: Group) -> f32 {
        f32::from(self.group_volume(group)) / 127.0
    }

    /// The distances (m) a placed sound of `group` plays at full volume
    /// within and is silent beyond; none for a group heard alike
    /// everywhere.
    pub fn group_range(&self, group: Group) -> Option<(f32, f32)> {
        self.group(group)?.range
    }

    /// The level a placed sound of `group` at `volume` (0 to 127) is given
    /// at the sliders' default, from 0 to 1. The executables first scale
    /// the volume by the group's (`volume × group / 127`, rounded down),
    /// then take that over 127 times the slider. A streamed sound (a sound
    /// object's loop, a sound set's line) plays at this level. Any other
    /// placed sound (a footstep, a door, a script's sound) plays at it
    /// raised to the power 0.6 (the view does that: `ambience::placed`).
    pub fn placed_level(&self, group: Group, volume: u8) -> f32 {
        let scaled = (u32::from(volume.min(127)) * u32::from(self.group_volume(group))) / 127;
        scaled as f32 / 127.0 * SLIDER
    }

    /// A conversation's voice's level at the sliders' default, which the
    /// view measures every placed sound's loudness against.
    pub fn voice_level(&self) -> f32 {
        self.group_gain(Group::Conversation) * SLIDER
    }

    fn group_volume(&self, group: Group) -> u8 {
        self.group(group).map_or(127, |row| row.volume)
    }

    /// The group of a footstep (or a rolling droid's sound) by a body of
    /// `appearance.2da` row `appearance`. `player` is the player's own
    /// character, whose steps have a group of their own. A body of size
    /// category 1 to 3 steps as a small creature, any other as a large one,
    /// as the executables have it. A body whose row is not known steps as a
    /// small one.
    pub fn step_group(&self, appearance: Option<u16>, player: bool) -> Group {
        match (player, self.size(appearance)) {
            (true, _) => Group::PlayerSteps,
            (false, None | Some(1..=SMALL_SIZE)) => Group::SmallSteps,
            (false, Some(_)) => Group::LargeSteps,
        }
    }

    /// The group of a sound set's line by a body of `appearance.2da` row
    /// `appearance`: a creature of size category 4 or more speaks as a
    /// large one, any other (and one whose row is not known) as a small one.
    pub fn voice_group(&self, appearance: Option<u16>) -> Group {
        match self.size(appearance) {
            Some(size) if size > SMALL_SIZE => Group::LargeVoice,
            _ => Group::SmallVoice,
        }
    }

    fn size(&self, appearance: Option<u16>) -> Option<u8> {
        self.sizes.get(usize::from(appearance?)).copied().flatten()
    }

    /// Footstep row `row`.
    pub fn steps(&self, row: u8) -> Option<&Steps> {
        self.steps.get(usize::from(row))
    }

    /// The footstep row a body of `appearance.2da` row `appearance` walks
    /// with.
    pub fn walk(&self, appearance: u16) -> Option<u8> {
        self.walks.get(usize::from(appearance)).copied().flatten()
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

    /// Whether `ambientmusic.2da` has row `row` (row 0, none, always): a
    /// row it lacks is a track lost, not a silence the area asks for.
    pub fn music_listed(&self, row: u16) -> bool {
        row == 0 || usize::from(row) < self.music.len()
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
    /// The `prioritygroups.2da` row its sounds play in. A looping object
    /// plays in an ambient group and any other in a shot group. Each kind
    /// has a group for a positional object and one for an object heard
    /// alike everywhere.
    pub fn group(&self) -> Group {
        match (self.looping, self.positional) {
            (true, true) => Group::PositionalAmbient,
            (true, false) => Group::AreaAmbient,
            (false, true) => Group::PositionalShot,
            (false, false) => Group::GlobalShot,
        }
    }

    /// The sound object a UTS (or a GIT's sound entry) makes.
    pub fn read(uts: &GffStruct) -> Emitter {
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

impl Moves {
    /// Every file it plays.
    pub fn files(&self) -> impl Iterator<Item = &str> {
        [
            &self.opened,
            &self.closed,
            &self.locked,
            &self.used,
            &self.destroyed,
        ]
        .into_iter()
        .flatten()
        .map(String::as_str)
    }
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

/// An area's sound: its tracks, and each object's, in the module's order;
/// each placeable appearance's (`placeables.2da` row's), for the objects
/// play makes (remains, a script's `CreateObject`).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct AreaSound {
    pub tracks: Tracks,
    pub objects: Vec<Voice>,
    pub placeables: Vec<Voice>,
}

impl AreaSound {
    /// Every file the area plays (not the tables' tracks).
    pub fn files(&self) -> impl Iterator<Item = &str> {
        self.objects.iter().flat_map(|voice| -> Vec<&str> {
            match voice {
                Voice::Emitter(emitter) => emitter.sounds.iter().map(String::as_str).collect(),
                Voice::Moves(moves) => moves.files().collect(),
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
    let placeable_rows = placeables.as_ref().map_or(0, |table| table.cells.len());
    let placeables = (0..placeable_rows as u32)
        .map(|row| moves(app_type(&placeables, Some(row))))
        .collect();
    Some(AreaSound {
        tracks,
        objects,
        placeables,
    })
}

/// Placeable template `template`'s appearance (its UTP's `Appearance`, a
/// `placeables.2da` row).
pub fn placeable_appearance(source: &dyn Source, template: &str) -> Option<u16> {
    let utp = Gff::parse(&source.read(template, ResType::UTP)?).ok()?;
    u16::try_from(utp.root.uint("Appearance")?).ok()
}

/// K1's `prioritygroups.2da` rows the group probes use: (row, volume,
/// range), and three bodies of size categories 3, 4 and 0.
fn k1_group_tables() -> Tables {
    let mut groups = vec![None; 23];
    for (row, volume, range) in [
        (5, 100, Some((3.0, 30.0))),
        (9, 90, Some((5.0, 20.0))),
        (18, 100, Some((25.0, 30.0))),
        (19, 65, Some((4.0, 8.0))),
        (20, 65, Some((4.0, 8.0))),
        (21, 120, None),
    ] {
        groups[row] = Some(GroupRow { volume, range });
    }
    Tables {
        groups,
        sizes: vec![Some(3), Some(4), Some(0)],
        ..Tables::default()
    }
}

/// Placed sounds' gains and groups as the executables have them (`kjk sim
/// probes --only "sound groups"`), each a name and whether it passed with
/// what it saw.
pub fn group_probes() -> Vec<(&'static str, Result<String, String>)> {
    let tables = k1_group_tables();
    let checked = |ok: bool, what: String| if ok { Ok(what) } else { Err(what) };
    // (name, group, volume, level as a share of a voice's)
    let levels = [
        (
            "a small creature's footstep is at 65 of a voice's 90",
            Group::SmallSteps,
            127,
            65.0 / 90.0,
        ),
        (
            "a positional loop is at 100 of a voice's 90",
            Group::PositionalAmbient,
            127,
            100.0 / 90.0,
        ),
        ("a voice is the reference", Group::Conversation, 127, 1.0),
        (
            "a group the table lacks plays at its own volume",
            Group::Scripted,
            127,
            127.0 / 90.0,
        ),
        (
            "a volume scaled by its group is rounded down",
            Group::SmallSteps,
            100,
            51.0 / 90.0,
        ),
    ];
    let mut checks: Vec<_> = levels
        .into_iter()
        .map(|(name, group, volume, expected)| {
            let share = tables.placed_level(group, volume) / tables.voice_level();
            (
                name,
                checked(
                    (share - expected).abs() < 1e-5,
                    format!("{share} (expected {expected})"),
                ),
            )
        })
        .collect();
    // (name, appearance, player, steps' group, voice's group)
    let sizes = [
        (
            "size 3 steps and speaks as a small creature",
            Some(0),
            false,
            Group::SmallSteps,
            Group::SmallVoice,
        ),
        (
            "size 4 steps and speaks as a large creature",
            Some(1),
            false,
            Group::LargeSteps,
            Group::LargeVoice,
        ),
        (
            "size 0 steps as a large creature but speaks as a small one",
            Some(2),
            false,
            Group::LargeSteps,
            Group::SmallVoice,
        ),
        (
            "an unknown body steps and speaks as a small creature",
            None,
            false,
            Group::SmallSteps,
            Group::SmallVoice,
        ),
        (
            "the player's character steps in its own group",
            Some(1),
            true,
            Group::PlayerSteps,
            Group::LargeVoice,
        ),
    ];
    checks.extend(
        sizes
            .into_iter()
            .map(|(name, appearance, player, steps, voice)| {
                let seen = (
                    tables.step_group(appearance, player),
                    tables.voice_group(appearance),
                );
                (name, checked(seen == (steps, voice), format!("{seen:?}")))
            }),
    );
    let ranges = (
        tables.group_range(Group::SmallSteps),
        tables.group_range(Group::GlobalShot),
    );
    checks.push((
        "a group's distances are read, and a global group has none",
        checked(ranges == (Some((4.0, 8.0)), None), format!("{ranges:?}")),
    ));
    checks
}
