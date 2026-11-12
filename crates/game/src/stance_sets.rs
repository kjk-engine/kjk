//! Movie Duels' stance sets for the sim (`sim::skeleton::StanceSets`), read
//! from the player's Jedi Academy install.
//!
//! Each skeleton Movie Duels offers (`models/players/_humanoid_*`, without
//! its droids' and creatures') gives one set, in the order of its folder's
//! name. After them comes one set for each row of `data/duel_stances.toml`
//! whose named sequences or suffixes give a stance or walk another sequence
//! than its skeleton's own. Each set holds the sequences the sim plays in
//! place of JA's (`sim::skeleton::stance`). The frames of every set are
//! added after the frames of JA's own skeleton, set after set in list order
//! and place after place in each set, both to the view's skeleton and to
//! the sim's. So the place of every lent frame follows from the ordered
//! list of sets and their digests alone, and the sim's frame numbers and the
//! view's are the same numbers.
//!
//! A standing guard for two blades or a staff whose frames are the one-blade
//! guard's is a one-handed pose with the wrong weapon in the hand, so it
//! counts as missing. A held block's pose that a set lacks is taken from
//! the default row's set.

use std::collections::{BTreeMap, HashMap};
use std::rc::Rc;

use fmt_ja::animation_cfg::{AnimationConfig, Sequence};
use fmt_ja::ghoul2::Gla;
use sim::skeleton::stance::{
    self, MAX_SETS, MOVING_AT, SEQUENCES, STANDING, STANDING_AT, STEPS, SWAPPED,
};
use sim::skeleton::{SkeletonData, StanceRow, StanceSet, StanceSets};

use crate::viewer_ja::Files;

/// The settings a character's own pick is kept in: this and its `who`
/// (`headgear::who`: a seat's character by its seat, any other by its
/// tag). Its value is a skeleton's folder (`_humanoid_obi`). Unset or
/// empty, the character takes its row's set.
pub const PICKED: &str = "saber.stance.";

/// The setting `who`'s pick is kept in.
pub fn pick_setting(who: &str) -> String {
    format!("{PICKED}{who}")
}

/// The picks among `values` (a settings map's), by `who`.
pub fn picks<'a>(
    values: impl IntoIterator<Item = (&'a String, &'a String)>,
) -> BTreeMap<String, String> {
    values
        .into_iter()
        .filter_map(|(key, folder)| Some((key.strip_prefix(PICKED)?, folder)))
        .filter(|(_, folder)| !folder.is_empty())
        .map(|(who, folder)| (who.to_string(), folder.to_ascii_lowercase()))
        .collect()
}

/// The skeletons among `sets` a character may be picked into, by folder:
/// the sets of Movie Duels' skeletons, without the table rows' own.
pub fn offered_sets(sets: &StanceSets) -> Vec<String> {
    sets.ids()
        .into_iter()
        .map(|(name, _)| name)
        .filter(|name| !name.contains('+'))
        .collect()
}

/// The number of the set `picks` gives `who` among `sets` (counted from 1),
/// or 0, the table's set, when `who` has no pick or its pick names no set.
pub fn picked(sets: &StanceSets, picks: &BTreeMap<String, String>, who: &str) -> u8 {
    picks.get(who).map_or(0, |folder| {
        sets.ids()
            .iter()
            .position(|(name, _)| name.eq_ignore_ascii_case(folder))
            .map_or(0, |at| (at + 1) as u8)
    })
}

/// The setting that once turned the view's stance swap on. The sim plays
/// the stance sets under MDA whatever its value.
pub const SETTING: &str = "game.duel_stances";

/// The environment variable that turns the sets off when it is `off`. The
/// audit uses it for its cases of JA's stance under MDA.
pub const SWITCH: &str = "KJK_STANCE_SETS";

/// Whether this run loads stance sets. It does when Movie Duels is
/// installed and `KJK_STANCE_SETS` is not `off`.
pub fn wanted() -> bool {
    switched_on(std::env::var(SWITCH).ok().as_deref()) && crate::settings::movie_duels()
}

/// Whether the value `KJK_STANCE_SETS` has, if any, leaves the sets on.
fn switched_on(value: Option<&str>) -> bool {
    value != Some("off")
}

/// Where Movie Duels keeps its characters' skeletons: a folder each, with
/// its `_humanoid.gla` beside its `animation.cfg`.
const SKELETONS: &str = "_humanoid_";

/// The skeletons not offered: Movie Duels' droids' and creatures'.
const NOT_OFFERED: [&str; 4] = [
    "_humanoid_bdroid",
    "_humanoid_sbd",
    "_humanoid_deka",
    "_humanoid_jabba",
];

/// The names the character page shows for the skeletons it knows. Any
/// other is shown by its folder's last word.
const TITLES: [(&str, &str); 20] = [
    ("kotor", "KOTOR Jedi"),
    ("md", "Movie Duels"),
    ("ani", "Anakin"),
    ("ben", "Ben Kenobi"),
    ("obi", "Obi-Wan"),
    ("obi3", "Obi-Wan III"),
    ("yoda", "Yoda"),
    ("vader", "Vader"),
    ("maul", "Maul"),
    ("mace", "Mace Windu"),
    ("cal", "Cal Kestis"),
    ("rey", "Rey"),
    ("dooku", "Dooku"),
    ("galen", "Galen Marek"),
    ("ren", "Kylo Ren"),
    ("luke", "Luke"),
    ("gon", "Qui-Gon"),
    ("grievous", "Grievous"),
    ("jango", "Jango Fett"),
    ("pal", "Palpatine"),
];

/// The name shown for the skeleton in `folder`.
pub fn title(folder: &str) -> String {
    let short = folder.strip_prefix(SKELETONS).unwrap_or(folder);
    match TITLES
        .iter()
        .find(|(known, _)| known.eq_ignore_ascii_case(short))
    {
        Some((_, title)) => title.to_string(),
        None => {
            let mut letters = short.chars();
            letters.next().map_or(String::new(), |first| {
                first.to_ascii_uppercase().to_string() + letters.as_str()
            })
        }
    }
}

/// The skeletons `files` offer, by folder in sorted order: Movie Duels'
/// (`SKELETONS`), but its droids' and creatures'.
pub(crate) fn offered(files: &Files) -> Vec<String> {
    let mut folders: Vec<String> = files
        .names()
        .filter_map(|path| {
            let folder = path
                .strip_prefix("models/players/")?
                .strip_suffix("/_humanoid.gla")?;
            Some(folder.to_ascii_lowercase())
        })
        .filter(|folder| folder.starts_with(SKELETONS))
        .filter(|folder| !NOT_OFFERED.contains(&folder.as_str()))
        .collect();
    folders.sort();
    folders.dedup();
    folders
}

/// The skeletons offered, by folder, in the Jedi Academy install that
/// `discovery` finds (`offered`).
pub fn offered_folders(discovery: &vfs::Discovery) -> Result<Vec<String>, String> {
    let stack = crate::viewer_ja::stack(discovery)?;
    Ok(offered(&Files::new(&stack)))
}

/// The table, as shipped.
pub(crate) const TABLE: &str = include_str!("../../../data/duel_stances.toml");

/// One row of the table, read.
#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct Row {
    pub name: String,
    pub templates: Vec<String>,
    pub tags: Vec<String>,
    pub player: bool,
    pub suffixes: Vec<String>,
    /// The skeleton its sequences are looked up in (`_humanoid_kotor`, a
    /// folder of `models/players/`).
    pub skeleton: Option<String>,
    /// JA's name (upper case) to the one played, over the suffixes.
    pub sequences: BTreeMap<String, String>,
}

impl Row {
    /// Whether this is the default row, which matches no character.
    pub fn is_default(&self) -> bool {
        self.templates.is_empty() && self.tags.is_empty() && !self.player
    }
}

/// The table's rows: `[default]` first, then each `[[character]]`.
pub(crate) fn rows(text: &str) -> Result<Vec<Row>, String> {
    use toml_edit::{Document, Item};
    let doc = Document::parse(text).map_err(|error| error.to_string())?;
    let strings = |item: Option<&Item>| -> Vec<String> {
        item.and_then(Item::as_array)
            .into_iter()
            .flatten()
            .filter_map(|value| value.as_str().map(str::to_string))
            .collect()
    };
    let row = |table: &dyn toml_edit::TableLike, name: &str| Row {
        name: table
            .get("name")
            .and_then(Item::as_str)
            .unwrap_or(name)
            .to_string(),
        templates: strings(table.get("templates")),
        tags: strings(table.get("tags")),
        player: table.get("player").and_then(Item::as_bool).unwrap_or(false),
        suffixes: strings(table.get("suffixes")),
        skeleton: table
            .get("skeleton")
            .and_then(Item::as_str)
            .map(str::to_string),
        sequences: table
            .get("sequences")
            .and_then(Item::as_table_like)
            .into_iter()
            .flat_map(|named| named.iter())
            .filter_map(|(ja, drawn)| Some((ja.to_ascii_uppercase(), drawn.as_str()?.to_string())))
            .collect(),
    };
    let mut rows = Vec::new();
    if let Some(default) = doc.get("default").and_then(Item::as_table_like) {
        rows.push(Row {
            templates: Vec::new(),
            tags: Vec::new(),
            player: false,
            ..row(default, "everyone else")
        });
    }
    let characters = doc
        .get("character")
        .and_then(Item::as_array_of_tables)
        .into_iter()
        .flatten();
    for (n, character) in characters.enumerate() {
        let read = row(character, &format!("row {}", n + 1));
        if read.is_default() {
            return Err(format!("{}: names no template, tag or player", read.name));
        }
        rows.push(read);
    }
    Ok(rows)
}

/// A skeleton that lends its frames, and its sequences by upper-case name.
pub(crate) struct Lender {
    pub gla: Gla,
    pub sequences: BTreeMap<String, Sequence>,
}

/// The skeleton in `models/players/<folder>/` (its `_humanoid.gla`, or the
/// one named as its folder) and its `animation.cfg`.
pub(crate) fn read_lender(files: &Files, folder: &str) -> Result<Lender, String> {
    let base = format!("models/players/{folder}");
    let (path, bytes) = ["_humanoid", folder]
        .iter()
        .map(|stem| format!("{base}/{stem}.gla"))
        .find_map(|path| files.read(&path).map(|bytes| (path, bytes)))
        .ok_or_else(|| format!("{base}: no skeleton"))?;
    let gla = Gla::parse(&bytes).map_err(|error| format!("{path}: {error:?}"))?;
    let cfg = files
        .read(&format!("{base}/animation.cfg"))
        .map(|bytes| AnimationConfig::parse(&bytes))
        .ok_or_else(|| format!("{base}/animation.cfg: not found"))?;
    let sequences = cfg
        .sequences
        .into_iter()
        .map(|s| (s.name.to_ascii_uppercase(), s))
        .collect();
    Ok(Lender { gla, sequences })
}

/// Why `lent`'s bones are not `gla`'s, by name and in order, if they are
/// not.
pub(crate) fn other_bones(lent: &Gla, gla: &Gla) -> Option<String> {
    if lent.bones.len() != gla.bones.len() {
        return Some(format!(
            "its {} bones are not the skeleton's {}",
            lent.bones.len(),
            gla.bones.len()
        ));
    }
    let (at, (a, b)) = lent
        .bones
        .iter()
        .zip(&gla.bones)
        .enumerate()
        .find(|(_, (a, b))| !a.name.eq_ignore_ascii_case(&b.name))?;
    Some(format!(
        "its bone {at} is {}, where the skeleton has {}",
        a.name, b.name
    ))
}

/// The stance sets as loaded.
pub struct Loaded {
    /// The view's skeleton with the sets' frames after its own.
    pub gla: Gla,
    /// The sim's skeleton with the same frames lent to it, for the bones it
    /// has, by name.
    pub skeleton: SkeletonData,
    /// The sets, each with its digest taken from `skeleton` (`set_digest`).
    pub sets: StanceSets,
    /// The notes for the log, one per set and per skeleton left out.
    pub notes: Vec<String>,
}

/// Reads `data/duel_stances.toml` and every offered skeleton of `files`
/// (the whole stack's, Movie Duels' among them), in sorted order. `gla`,
/// `cfg` and `data` are JA's own, from `Files::base`.
pub fn load(
    files: &Files,
    gla: &Gla,
    cfg: &AnimationConfig,
    data: SkeletonData,
) -> Result<Loaded, String> {
    let started = std::time::Instant::now();
    let rows = rows(TABLE).map_err(|error| format!("data/duel_stances.toml: {error}"))?;
    let mut loaded = build(
        &rows,
        &offered(files),
        |folder| read_lender(files, folder),
        (gla, cfg),
        data,
    )?;
    if let Ok(mut here) = HERE.lock() {
        *here = loaded.sets.ids();
    }
    loaded.notes.push(format!(
        "stance sets: {} sets loaded in {} ms",
        loaded.sets.len(),
        started.elapsed().as_millis()
    ));
    Ok(loaded)
}

/// The sets this run loaded last, each by name and digest.
static HERE: std::sync::Mutex<Vec<(String, [u8; 32])>> = std::sync::Mutex::new(Vec::new());

/// The stance sets this run has loaded, each by name and digest. None
/// were loaded where Movie Duels is missing or the sets are off.
pub fn here() -> Vec<(String, [u8; 32])> {
    HERE.lock().map_or_else(|_| Vec::new(), |here| here.clone())
}

/// What a guest tells its player about `refusal`. A refusal for the stance
/// sets names the host's sets that this machine lacks or has with other
/// frames (`Refusal::stances_text`). Any other refusal says what it is.
pub fn refusal_text(refusal: &net::handshake::Refusal) -> String {
    match refusal {
        net::handshake::Refusal::Stances { sets, more } => {
            net::handshake::Refusal::stances_text(sets, *more, &here())
        }
        other => other.to_string(),
    }
}

/// A set's sequence and the skeleton it comes from.
type Slot = Option<(Rc<Lender>, Sequence)>;

/// A set to be built: its name and its sequences.
struct Planned {
    name: String,
    slots: Vec<Slot>,
}

/// `load` with each skeleton read by `read` (a folder), against `rows` and
/// the offered `folders` in sorted order. `base` is JA's own skeleton and
/// its `animation.cfg`, and `data` is the sim's skeleton made from them.
fn build(
    rows: &[Row],
    folders: &[String],
    mut read: impl FnMut(&str) -> Result<Lender, String>,
    (gla, cfg): (&Gla, &AnimationConfig),
    data: SkeletonData,
) -> Result<Loaded, String> {
    let bones = sim_bones(gla, &data)?;
    own_frames_kept(gla, cfg, &data)?;
    let mut notes = Vec::new();
    let mut left_out = Vec::new();
    let mut read = |folder: &str| -> Option<Rc<Lender>> {
        let lender = read(folder).and_then(|lender| match other_bones(&lender.gla, gla) {
            Some(why) => Err(why),
            None => Ok(lender),
        });
        lender
            .inspect_err(|why| left_out.push(format!("stance sets: {folder} is left out: {why}")))
            .ok()
            .map(Rc::new)
    };
    let same = |a: &str, b: &str| a.eq_ignore_ascii_case(b);
    let default_row = rows.iter().find(|row| row.is_default());
    let default_folder = default_row
        .and_then(|row| row.skeleton.as_deref())
        .filter(|folder| folders.iter().any(|f| same(f, folder)));
    // The default row's skeleton, read first: its held block's poses stand
    // in for those other sets lack.
    let mut default = default_folder.and_then(|folder| Some((folder, read(folder)?)));
    let fallback = match &default {
        Some((_, lender)) => slots_of(lender, None)
            .map_err(|why| format!("stance sets: the default skeleton names {why}")),
        None => Ok(empty()),
    };
    let fallback = fallback.unwrap_or_else(|why| {
        notes.push(why);
        empty()
    });
    let mut planned = Vec::new();
    let mut kept: Vec<(String, Rc<Lender>)> = Vec::new();
    let mut left = 0;
    for folder in folders {
        if planned.len() == MAX_SETS {
            left += 1;
            continue;
        }
        let lender = match default.take_if(|(name, _)| same(name, folder)) {
            Some((_, lender)) => Some(lender),
            None => read(folder),
        };
        let Some(lender) = lender else {
            continue;
        };
        match slots_of(&lender, None) {
            Ok(slots) => planned.push(Planned {
                name: folder.clone(),
                slots: filled(slots, &fallback),
            }),
            Err(why) => {
                notes.push(format!("stance sets: {folder} is left out: it names {why}"));
                continue;
            }
        }
        if rows
            .iter()
            .any(|row| row.skeleton.as_deref().is_some_and(|s| same(s, folder)))
        {
            kept.push((folder.clone(), lender));
        }
    }
    notes.append(&mut left_out);
    if left > 0 {
        notes.push(format!(
            "stance sets: {left} skeletons are left out, because a level holds at most {MAX_SETS} sets"
        ));
    }
    // The rows whose named sequences or suffixes give a set of their own.
    let mut own_sets = BTreeMap::new();
    for (index, row) in rows.iter().enumerate() {
        let Some((folder, lender)) = row
            .skeleton
            .as_deref()
            .and_then(|s| kept.iter().find(|(folder, _)| same(folder, s)))
        else {
            continue;
        };
        let (Ok(plain), Ok(own)) = (slots_of(lender, None), slots_of(lender, Some(row))) else {
            continue;
        };
        let differs = plain
            .iter()
            .zip(&own)
            .any(|(a, b)| a.as_ref().map(|a| &a.1) != b.as_ref().map(|b| &b.1));
        if !differs {
            continue;
        }
        if planned.len() == MAX_SETS {
            notes.push(format!(
                "stance sets: {folder}+{} is left out, because a level holds at most {MAX_SETS} sets",
                row.name
            ));
            continue;
        }
        let name = format!("{folder}+{}", row.name);
        own_sets.insert(index, name.clone());
        planned.push(Planned {
            name,
            slots: filled(own, &fallback),
        });
    }
    planned.retain(|set| {
        let any = set.slots.iter().any(Option::is_some);
        if !any {
            notes.push(format!(
                "stance sets: {} is left out: it has no sequence",
                set.name
            ));
        }
        any
    });
    let (view, sim, sequences) = lend(gla, &bones, &planned);
    let skeleton = data
        .with_lent(sim.0, sim.1)
        .map_err(|error| error.to_string())?;
    let sets: Vec<StanceSet> = planned
        .iter()
        .zip(sequences)
        .map(|(set, sequences)| StanceSet {
            name: set.name.clone(),
            digest: stance::set_digest(&sequences, &skeleton),
            sequences,
        })
        .collect();
    for (number, set) in sets.iter().enumerate() {
        let found = set.sequences.iter().flatten().count();
        notes.push(format!(
            "stance set {}, {}: {found} of {SEQUENCES} sequences",
            number + 1,
            set.name
        ));
    }
    let number_of = |name: &str| {
        sets.iter()
            .position(|set| set.name == name)
            .map(|at| (at + 1) as u8)
    };
    let mut table = Vec::new();
    let mut default_set = 0;
    for (index, row) in rows.iter().enumerate() {
        let named = match own_sets.get(&index) {
            Some(name) => number_of(name),
            None => row.skeleton.as_deref().and_then(|skeleton| {
                let folder = folders.iter().find(|f| same(f, skeleton))?;
                number_of(folder)
            }),
        };
        let Some(set) = named else {
            notes.push(format!(
                "stance sets: the row {} is dropped: its skeleton {} gives no set",
                row.name,
                row.skeleton.as_deref().unwrap_or("(none)")
            ));
            continue;
        };
        if row.is_default() {
            default_set = set;
            continue;
        }
        table.push(StanceRow {
            templates: row.templates.clone(),
            tags: row.tags.clone(),
            player: row.player,
            set,
        });
    }
    Ok(Loaded {
        gla: view,
        skeleton,
        sets: StanceSets::new(sets, table, default_set),
        notes,
    })
}

/// No sequences.
fn empty() -> Vec<Slot> {
    vec![None; SEQUENCES]
}

/// For each of the sim's bones, the view's bone of the same name.
fn sim_bones(gla: &Gla, data: &SkeletonData) -> Result<Vec<usize>, String> {
    data.bones()
        .iter()
        .map(|bone| {
            gla.bones
                .iter()
                .position(|b| b.name.eq_ignore_ascii_case(&bone.name))
                .ok_or_else(|| format!("stance sets: the skeleton has no bone {}", bone.name))
        })
        .collect()
}

/// Refuses to lend frames to a skeleton whose frames are not the view's,
/// or one of whose own sequences reaches past its own frames: such a
/// sequence would read a lent frame with the sets and frame 0 without
/// them.
fn own_frames_kept(gla: &Gla, cfg: &AnimationConfig, data: &SkeletonData) -> Result<(), String> {
    let own = data.own_frame_count();
    if gla.frame_count != own as usize || data.frame_count() != own {
        return Err(format!(
            "stance sets: the sim's skeleton has {} frames and the view's {}",
            data.frame_count(),
            gla.frame_count
        ));
    }
    match cfg
        .sequences
        .iter()
        .find(|s| i64::from(s.first_frame) + i64::from(s.frame_count) > i64::from(own))
    {
        Some(past) => Err(format!(
            "stance sets: JA's {} reaches past the skeleton's {own} frames",
            past.name
        )),
        None => Ok(()),
    }
}

/// The sequences `lender` gives a set, place by place, for `row` or for
/// the skeleton itself. A stance or walk is the first found of the row's
/// named sequence, JA's name with each of the row's suffixes, and JA's
/// name. Every other place is looked up by its own name. A guard for two
/// blades or the staff that plays the one-blade guard's frames is dropped
/// (`own_weapons`). The error names the first sequence that reaches past
/// the skeleton's frames.
fn slots_of(lender: &Rc<Lender>, row: Option<&Row>) -> Result<Vec<Slot>, String> {
    let mut found: Vec<Option<Sequence>> = (0..SEQUENCES)
        .map(|place| {
            let plain = stance::place_name(place);
            let named = row
                .filter(|_| place < SWAPPED.len())
                .map(|row| {
                    let suffixed = row.suffixes.iter().map(|suffix| format!("{plain}{suffix}"));
                    row.sequences
                        .get(&plain)
                        .cloned()
                        .into_iter()
                        .chain(suffixed)
                })
                .into_iter()
                .flatten();
            named
                .chain([plain.clone()])
                .find_map(|name| lender.sequences.get(&name.to_ascii_uppercase()))
                .cloned()
        })
        .collect();
    let frames = lender.gla.frame_count as i64;
    if let Some(past) = found.iter().flatten().find(|s| {
        s.first_frame < 0
            || s.frame_count < 1
            || i64::from(s.first_frame) + i64::from(s.frame_count) > frames
    }) {
        return Err(format!(
            "{} at frames {} to {}, past its {frames} frames",
            past.name,
            past.first_frame,
            i64::from(past.first_frame) + i64::from(past.frame_count)
        ));
    }
    own_weapons(&mut found);
    Ok(found
        .into_iter()
        .map(|s| s.map(|s| (lender.clone(), s)))
        .collect())
}

/// Drops the standing guards for two blades and the staff that are the
/// one-blade guard's. When the standing guard for two blades or the staff
/// plays the one-blade standing guard's frames, all of that weapon's
/// guards, standing and moving, are dropped. A moving guard that plays the
/// one-blade guard's frames for the same step, or its standing guard's,
/// is dropped on its own.
fn own_weapons(found: &mut [Option<Sequence>]) {
    let frames = |s: &Option<Sequence>| s.as_ref().map(|s| (s.first_frame, s.frame_count));
    let moving = |blades: usize, step: usize| MOVING_AT + blades * STEPS.len() + step;
    let single = frames(&found[STANDING_AT]);
    for blades in 1..STANDING.len() {
        let standing = STANDING_AT + blades;
        if frames(&found[standing]).is_some_and(|f| Some(f) == single) {
            found[standing] = None;
            for step in 0..STEPS.len() {
                found[moving(blades, step)] = None;
            }
        }
        for step in 0..STEPS.len() {
            let guard = frames(&found[moving(blades, step)]);
            let one_blade = [single, frames(&found[moving(0, step)])];
            if guard.is_some_and(|g| one_blade.contains(&Some(g))) {
                found[moving(blades, step)] = None;
            }
        }
    }
}

/// `slots` with each held block's pose it lacks taken from `fallback`.
fn filled(slots: Vec<Slot>, fallback: &[Slot]) -> Vec<Slot> {
    slots
        .into_iter()
        .zip(fallback)
        .enumerate()
        .map(|(place, (slot, other))| match slot {
            None if place >= STANDING_AT => other.clone(),
            slot => slot,
        })
        .collect()
}

/// The sets' frames lent, set after set and place after place: the view's
/// skeleton `gla` with them after its own, the sim's keys and pool for
/// its bones (`bones`, by the view's bone of each), and each set's
/// sequences where their frames now are.
#[allow(clippy::type_complexity)]
fn lend(
    gla: &Gla,
    bones: &[usize],
    planned: &[Planned],
) -> (
    Gla,
    (Vec<u32>, Vec<[u16; 7]>),
    Vec<[Option<sim::skeleton::Sequence>; SEQUENCES]>,
) {
    let mut view = gla.clone();
    let mut view_pool: HashMap<[u16; 7], u32> = HashMap::new();
    let (mut keys, mut pool) = (Vec::new(), Vec::new());
    let mut sim_pool: HashMap<[u16; 7], u32> = HashMap::new();
    let all = gla.bones.len();
    let sequences = planned
        .iter()
        .map(|set| {
            std::array::from_fn(|place| {
                let (lender, sequence) = set.slots[place].as_ref()?;
                let first = sequence.first_frame as usize;
                let count = sequence.frame_count as usize;
                let start = view.frame_count;
                for frame in first..first + count {
                    let entry = |bone: usize| {
                        lender.gla.pool[lender.gla.frames[frame * all + bone] as usize]
                    };
                    for bone in 0..all {
                        let value = entry(bone);
                        let at = *view_pool.entry(value).or_insert_with(|| {
                            view.pool.push(value);
                            (view.pool.len() - 1) as u32
                        });
                        view.frames.push(at);
                    }
                    for &bone in bones {
                        let value = entry(bone);
                        let at = *sim_pool.entry(value).or_insert_with(|| {
                            pool.push(value);
                            (pool.len() - 1) as u32
                        });
                        keys.push(at);
                    }
                }
                view.frame_count += count;
                Some(crate::stage::sim_sequence(&Sequence {
                    first_frame: start as i32,
                    ..sequence.clone()
                }))
            })
        })
        .collect();
    (view, (keys, pool), sequences)
}

/// The bones of the made-up skeletons: the sim's five, and a leg the sim
/// does not have.
#[cfg(feature = "probes")]
const MADE_UP_BONES: [&str; 6] = [
    "model_root",
    "lower_lumbar",
    "upper_lumbar",
    "thoracic",
    "Motion",
    "l_leg",
];

/// A made-up skeleton of `bones` with `frames` frames, each frame's bone a
/// transform of its own that names `seed`.
#[cfg(feature = "probes")]
fn made_up_gla(seed: u16, bones: &[&str], frames: usize) -> Gla {
    Gla {
        name: "probe".into(),
        scale: 1.0,
        bones: bones
            .iter()
            .enumerate()
            .map(|(n, name)| fmt_ja::ghoul2::Bone {
                name: name.to_string(),
                flags: 0,
                parent: n as i32 - 1,
                base_pose: [[0.0; 4]; 3],
                base_pose_inverse: [[0.0; 4]; 3],
                children: Vec::new(),
            })
            .collect(),
        frame_count: frames,
        frames: (0..(frames * bones.len()) as u32).collect(),
        pool: (0..frames * bones.len())
            .map(|at| {
                let (frame, bone) = (at / bones.len(), at % bones.len());
                [seed, frame as u16, bone as u16, 1, 2, 3, 4]
            })
            .collect(),
    }
}

#[cfg(feature = "probes")]
fn made_up_sequence(name: &str, first_frame: i32, frame_count: i32) -> Sequence {
    Sequence {
        name: name.to_string(),
        first_frame,
        frame_count,
        loop_frame: 0,
        frames_per_second: 20,
    }
}

/// A made-up Movie Duels skeleton: every place of a set at 2 frames of its
/// own, and `extra` sequences besides, over `frames` frames.
#[cfg(feature = "probes")]
fn made_up_lender(seed: u16, bones: &[&str], frames: usize, extra: &[(&str, i32, i32)]) -> Lender {
    let places = (0..SEQUENCES).map(|place| (stance::place_name(place), place as i32 * 2, 2));
    let extra = extra
        .iter()
        .map(|&(name, first, count)| (name.to_string(), first, count));
    Lender {
        gla: made_up_gla(seed, bones, frames),
        sequences: places
            .chain(extra)
            .map(|(name, first, count)| (name.clone(), made_up_sequence(&name, first, count)))
            .collect(),
    }
}

/// Frames a made-up skeleton needs for every place and 10 more.
#[cfg(feature = "probes")]
const MADE_UP_FRAMES: usize = SEQUENCES * 2 + 10;

/// `build` on the made-up base skeleton (64 frames, as the sim's made-up
/// skeleton has), against `rows`, with the skeletons `lenders` by folder.
#[cfg(feature = "probes")]
fn made_up_build(
    rows: &[Row],
    lenders: Vec<(String, Lender)>,
    base: &[Sequence],
) -> Result<Loaded, String> {
    let gla = made_up_gla(0, &MADE_UP_BONES, 64);
    let cfg = AnimationConfig {
        sequences: base.to_vec(),
        bad_lines: Vec::new(),
    };
    let data = sim::trace::stances::skeleton()?;
    let folders: Vec<String> = lenders.iter().map(|(folder, _)| folder.clone()).collect();
    let mut lenders: BTreeMap<String, Lender> = lenders.into_iter().collect();
    let mut sorted = folders;
    sorted.sort();
    build(
        rows,
        &sorted,
        |folder| {
            lenders
                .remove(folder)
                .ok_or_else(|| format!("{folder}: no skeleton"))
        },
        (&gla, &cfg),
        data,
    )
}

/// The checks of the stance sets' loader, without an install (`kjk sim
/// probes`, group "stance sets table").
#[cfg(feature = "probes")]
pub fn probes() -> sim::probe::Checks {
    vec![
        ("the shipped table reads", table_reads()),
        ("wildcards match as written", wildcards()),
        ("titles", titles()),
        ("numbers_sorted", numbers_sorted()),
        ("picks_name_sets", picks_name_sets()),
        ("same_frames_both", same_frames_both()),
        ("packed", packed()),
        ("bad_skeleton_left_out", bad_skeleton_left_out()),
        ("too_many", too_many()),
        ("row_without_set", row_without_set()),
        ("copies_and_fallback", copies_and_fallback()),
        ("own_sequence_past_frames", own_sequence_past_frames()),
        ("off_switch", off_switch()),
    ]
}

#[cfg(feature = "probes")]
fn table_reads() -> Result<String, String> {
    rows(TABLE).map(|rows| format!("The shipped table has {} rows.", rows.len()))
}

#[cfg(feature = "probes")]
fn wildcards() -> Result<String, String> {
    let globs = [
        ("p_bastilla*", "P_Bastilla001", true),
        ("p_bastilla*", "p_bastil", false),
        ("*malak*", "end_darthmalak", true),
        ("bastila", "Bastila", true),
        ("bastila", "bastila2", false),
    ];
    let wrong: Vec<_> = globs
        .iter()
        .filter(|(pattern, name, want)| stance::glob(pattern, name) != *want)
        .collect();
    sim::probe::check(
        wrong.is_empty(),
        format!("Of {} cases these matched wrongly: {wrong:?}.", globs.len()),
    )
}

#[cfg(feature = "probes")]
fn titles() -> Result<String, String> {
    let found = [
        title("_humanoid_obi"),
        title("_humanoid_kotor"),
        title("_humanoid_xyz"),
    ];
    sim::probe::check(
        found == ["Obi-Wan", "KOTOR Jedi", "Xyz"],
        format!("The titles are {found:?}."),
    )
}

/// The shipped table's rows.
#[cfg(feature = "probes")]
fn shipped() -> Result<Vec<Row>, String> {
    rows(TABLE)
}

/// Made-up skeletons for `folders`, each with frames of its own and a
/// staff stance whose frames differ from its plain stance's.
#[cfg(feature = "probes")]
fn lenders_for(folders: &[&str]) -> Vec<(String, Lender)> {
    folders
        .iter()
        .enumerate()
        .map(|(n, folder)| {
            let lender = made_up_lender(n as u16 + 1, &MADE_UP_BONES, MADE_UP_FRAMES, &[]);
            (format!("_humanoid_{folder}"), lender)
        })
        .collect()
}

#[cfg(feature = "probes")]
fn numbers_sorted() -> Result<String, String> {
    let loaded = made_up_build(&shipped()?, lenders_for(&["obi", "kotor", "ani"]), &[])?;
    let names: Vec<String> = loaded
        .sets
        .ids()
        .into_iter()
        .map(|(name, _)| name)
        .collect();
    let wanted = [
        "_humanoid_ani",
        "_humanoid_kotor",
        "_humanoid_obi",
        "_humanoid_kotor+Bastila Shan",
    ];
    sim::probe::check(names == wanted, format!("The sets are {names:?}."))
}

/// The skeletons offered for a pick leave out the table rows' own sets,
/// and a saved pick names its set by folder, in any case. No pick, or a
/// pick of a skeleton the level lacks, gives the table's set (0).
#[cfg(feature = "probes")]
fn picks_name_sets() -> Result<String, String> {
    let loaded = made_up_build(&shipped()?, lenders_for(&["obi", "kotor", "ani"]), &[])?;
    let offered = offered_sets(&loaded.sets);
    let saved: BTreeMap<String, String> = [
        ("saber.stance.seat0", "_humanoid_obi"),
        ("saber.stance.seat1", "_HUMANOID_KOTOR"),
        ("saber.stance.seat2", "_humanoid_xyz"),
        ("saber.stance.seat3", ""),
    ]
    .map(|(key, value)| (key.to_string(), value.to_string()))
    .into();
    let picks = picks(&saved);
    let found =
        ["seat0", "seat1", "seat2", "seat3", "seat4"].map(|who| picked(&loaded.sets, &picks, who));
    sim::probe::check(
        offered == ["_humanoid_ani", "_humanoid_kotor", "_humanoid_obi"]
            && found == [3, 2, 0, 0, 0],
        format!("The offered skeletons are {offered:?}, and the picks give the sets {found:?}."),
    )
}

#[cfg(feature = "probes")]
fn same_frames_both() -> Result<String, String> {
    let loaded = made_up_build(&shipped()?, lenders_for(&["obi", "kotor", "ani"]), &[])?;
    let bones = sim_bones(&loaded.gla, &loaded.skeleton)?;
    let all = loaded.gla.bones.len();
    let mut checked = 0;
    let mut wrong = Vec::new();
    for number in 1..=loaded.sets.len() as u8 {
        let set = loaded.sets.set(number).ok_or("A set is missing.")?;
        for sequence in set.sequences.iter().flatten() {
            for frame in sequence.first_frame..sequence.first_frame + sequence.frames {
                for (bone, &view_bone) in bones.iter().enumerate() {
                    checked += 1;
                    let at = loaded.gla.frames[frame as usize * all + view_bone];
                    let view = loaded.gla.pool[at as usize];
                    if view != loaded.skeleton.transform(frame, bone) {
                        wrong.push(format!("{} frame {frame} bone {bone}", set.name));
                    }
                }
            }
        }
    }
    wrong.truncate(4);
    sim::probe::check(
        wrong.is_empty() && checked > 0,
        format!("Of {checked} keys these differ between the view and the sim: {wrong:?}."),
    )
}

#[cfg(feature = "probes")]
fn packed() -> Result<String, String> {
    let loaded = made_up_build(&shipped()?, lenders_for(&["obi", "kotor", "ani"]), &[])?;
    let mut next = 64;
    let mut wrong = Vec::new();
    for number in 1..=loaded.sets.len() as u8 {
        let set = loaded.sets.set(number).ok_or("A set is missing.")?;
        for (place, sequence) in set.sequences.iter().enumerate() {
            let Some(sequence) = sequence else { continue };
            if sequence.first_frame != next {
                wrong.push(format!(
                    "{} place {place} at {}",
                    set.name, sequence.first_frame
                ));
            }
            next = sequence.first_frame + sequence.frames;
        }
    }
    let all = loaded.skeleton.frame_count() as i32;
    sim::probe::check(
        wrong.is_empty() && next == all && loaded.gla.frame_count as i32 == all,
        format!("The lent frames end at {next} of {all}. Sequences out of their place: {wrong:?}."),
    )
}

#[cfg(feature = "probes")]
fn bad_skeleton_left_out() -> Result<String, String> {
    let mut renamed = MADE_UP_BONES;
    renamed[3] = "spine";
    let cases: [(&str, Lender); 3] = [
        (
            "few",
            made_up_lender(7, &MADE_UP_BONES[..5], MADE_UP_FRAMES, &[]),
        ),
        ("renamed", made_up_lender(8, &renamed, MADE_UP_FRAMES, &[])),
        ("past", {
            let mut lender = made_up_lender(9, &MADE_UP_BONES, MADE_UP_FRAMES, &[]);
            let walk = lender.sequences.get_mut("BOTH_WALK2").ok_or("No walk.")?;
            walk.first_frame = MADE_UP_FRAMES as i32 - 1;
            lender
        }),
    ];
    let mut lenders = lenders_for(&["kotor"]);
    lenders.extend(
        cases
            .into_iter()
            .map(|(name, lender)| (format!("_humanoid_{name}"), lender)),
    );
    let loaded = made_up_build(&shipped()?, lenders, &[])?;
    let names: Vec<String> = loaded
        .sets
        .ids()
        .into_iter()
        .map(|(name, _)| name)
        .collect();
    let noted = ["few", "renamed", "past"].map(|name| {
        let folder = format!("_humanoid_{name}");
        loaded
            .notes
            .iter()
            .any(|note| note.contains(&folder) && note.contains("left out"))
    });
    sim::probe::check(
        names == ["_humanoid_kotor", "_humanoid_kotor+Bastila Shan"] && noted == [true; 3],
        format!("The sets are {names:?}. Each bad skeleton is noted: {noted:?}."),
    )
}

#[cfg(feature = "probes")]
fn too_many() -> Result<String, String> {
    let lenders = (0..300)
        .map(|n| {
            let lender = Lender {
                gla: made_up_gla(n, &MADE_UP_BONES, 2),
                sequences: [(
                    "BOTH_STAND2".to_string(),
                    made_up_sequence("BOTH_STAND2", 0, 2),
                )]
                .into(),
            };
            (format!("_humanoid_{n:03}"), lender)
        })
        .collect();
    let loaded = made_up_build(&[], lenders, &[])?;
    let noted = loaded
        .notes
        .iter()
        .any(|note| note.contains("45 skeletons are left out"));
    sim::probe::check(
        loaded.sets.len() == MAX_SETS && noted,
        format!(
            "300 skeletons give {} sets. The note of the 45 left out is there: {noted}.",
            loaded.sets.len()
        ),
    )
}

#[cfg(feature = "probes")]
fn row_without_set() -> Result<String, String> {
    let row = |name: &str, templates: &[&str], skeleton: &str| Row {
        name: name.into(),
        templates: templates.iter().map(|t| t.to_string()).collect(),
        skeleton: Some(skeleton.into()),
        ..Row::default()
    };
    let rows = [
        row("everyone else", &[], "_humanoid_kotor"),
        row("gone", &["n_gone*"], "_humanoid_gone"),
        row("obi", &["n_obi*"], "_humanoid_obi"),
    ];
    let loaded = made_up_build(&rows, lenders_for(&["kotor", "obi"]), &[])?;
    let found = [
        loaded.sets.row_for("n_gone01", "", false),
        loaded.sets.row_for("n_obi01", "", false),
    ];
    let noted = loaded
        .notes
        .iter()
        .any(|note| note.contains("row gone is dropped"));
    sim::probe::check(
        found == [1, 2] && noted,
        format!(
            "The gone row's character takes set {}, and obi's set {}. The drop is noted: {noted}.",
            found[0], found[1]
        ),
    )
}

#[cfg(feature = "probes")]
fn copies_and_fallback() -> Result<String, String> {
    // Obi's staff guard plays the one-blade guard's frames, and its two
    // blades' guard to the left plays the one-blade guard's to the left.
    // Both count as missing, so obi takes kotor's, the default row's.
    let mut obi = made_up_lender(2, &MADE_UP_BONES, MADE_UP_FRAMES, &[]);
    let copy = |obi: &mut Lender, of: usize, to: usize| -> Result<(), String> {
        let from = obi
            .sequences
            .get(&stance::place_name(of))
            .cloned()
            .ok_or("No guard.")?;
        let at = obi
            .sequences
            .get_mut(&stance::place_name(to))
            .ok_or("No guard.")?;
        (at.first_frame, at.frame_count) = (from.first_frame, from.frame_count);
        Ok(())
    };
    let left = |blades: usize| MOVING_AT + blades * STEPS.len() + 2;
    copy(&mut obi, STANDING_AT, STANDING_AT + 2)?;
    copy(&mut obi, left(0), left(1))?;
    let mut lenders = lenders_for(&["kotor"]);
    lenders.push(("_humanoid_obi".into(), obi));
    let default = Row {
        name: "everyone else".into(),
        skeleton: Some("_humanoid_kotor".into()),
        ..Row::default()
    };
    let loaded = made_up_build(&[default], lenders, &[])?;
    let content = |number: u8, place: usize| -> Option<Vec<[u16; 7]>> {
        let sequence = loaded.sets.set(number)?.sequences[place]?;
        Some(
            (sequence.first_frame..sequence.first_frame + sequence.frames)
                .map(|frame| loaded.skeleton.transform(frame, 1))
                .collect(),
        )
    };
    let places = [
        STANDING_AT + 2,
        left(1),
        MOVING_AT + 2 * STEPS.len(),
        STANDING_AT + 1,
    ];
    let from_kotor = places.map(|place| content(2, place) == content(1, place));
    sim::probe::check(
        from_kotor == [true, true, true, false],
        format!(
            "Obi's staff guard, its two blades' guard to the left and its staff's guard forward are kotor's: {:?}. Its own guard for two blades stays its own: {}.",
            &from_kotor[..3],
            !from_kotor[3]
        ),
    )
}

#[cfg(feature = "probes")]
fn own_sequence_past_frames() -> Result<String, String> {
    let past = made_up_sequence("BOTH_LONG", 60, 10);
    match made_up_build(&shipped()?, lenders_for(&["kotor"]), &[past]) {
        Err(why) => sim::probe::check(
            why.contains("BOTH_LONG"),
            format!("A base sequence past the skeleton's frames refuses the sets: {why}"),
        ),
        Ok(_) => Err("A base sequence past the skeleton's frames was accepted.".into()),
    }
}

#[cfg(feature = "probes")]
fn off_switch() -> Result<String, String> {
    let found = [
        switched_on(Some("off")),
        switched_on(Some("on")),
        switched_on(None),
    ];
    sim::probe::check(
        found == [false, true, true],
        format!("With KJK_STANCE_SETS off, on and unset the sets are wanted: {found:?}."),
    )
}
