//! A character with its blade lit stands and walks in its own stance and
//! walk from a mod's skeleton (Movie Duels' per-character sequences,
//! `docs/SABER-RULES.md` section 11), in place of JA's (`game.duel_stances`,
//! off by default).
//!
//! - **What:** while the sim's skeleton holds one of the ready stances or
//!   walks a lit blade plays (`SWAPPED`), the drawn body plays the
//!   character's own sequence instead: the first its row finds of its named
//!   sequence and JA's name with each of its suffixes
//!   (`BOTH_SABERDUAL_STANCE` + `_MAUL`), looked up in the row's
//!   `skeleton` (Movie Duels' `_humanoid_kotor`: a skeleton of its own
//!   beside `_humanoid`, the same names played its way) or else in the
//!   stack's own `animation.cfg`. A skeleton's frames are added after the
//!   view's skeleton's own (`Borrowed`), when its bones are the same. The
//!   table is `data/duel_stances.toml`: rows by template or tag (`*` a
//!   wildcard), the player's own character, and a default row.
//! - **How:** the sim's playback of JA's sequence is replayed over the
//!   other one's frames from the same moment, at the same share of its
//!   own rate (a walk sped up by the pace stays sped up); a part going to
//!   or from such a sequence blends from the pose it shows (`Figure`).
//! - **Without it** (the option off, no such sequence in the stack, no
//!   blade lit, a body drawn the cgame's way): JA's sequence plays.
//!
//! Render only, as `stance`: the sim's skeleton, which the blades cut
//! with, holds JA's sequence, so no digest moves.

use std::collections::BTreeMap;

use fmt_ja::animation_cfg::{AnimationConfig, Sequence};
use fmt_ja::ghoul2::Gla;
use sim::combat::saber::blades::Shown;
use sim::motion::Anim;
use sim::skeleton::Playback;
use sim::{EntityState, Instances, Kind};

/// The setting that turns them on (`true`).
pub const SETTING: &str = "game.duel_stances";

/// The table, as shipped.
const TABLE: &str = include_str!("../../../data/duel_stances.toml");

/// The sim's sequences a lit blade stands, fidgets or walks in
/// (`sim::motion::weapon`, `sim::combat::idle`): the ones a character's
/// own may stand for. The fidgets go with the stance, or a character in
/// another's stance would snap back to JA's now and then.
pub const SWAPPED: [Anim; 10] = [
    Anim::BOTH_STAND2,
    Anim::BOTH_STAND2IDLE1,
    Anim::BOTH_STAND2IDLE2,
    Anim::BOTH_SABERFAST_STANCE,
    Anim::BOTH_SABERSLOW_STANCE,
    Anim::BOTH_SABERDUAL_STANCE,
    Anim::BOTH_SABERSTAFF_STANCE,
    Anim::BOTH_WALK2,
    Anim::BOTH_WALK_DUAL,
    Anim::BOTH_WALK_STAFF,
];

/// The sim's walks among `SWAPPED`: under a held block the legs walk on.
const WALKS: [Anim; 3] = [
    Anim::BOTH_WALK2,
    Anim::BOTH_WALK_DUAL,
    Anim::BOTH_WALK_STAFF,
];

/// What a held block shows (`docs/SABER-RULES.md` 11.5), for one blade,
/// two and a staff: the standing guard, and with attack held the top
/// parry. A skeleton without them (JA's) shows the stance as before.
const BLOCKING: [[&str; 3]; 2] = [
    [
        "BOTH_STAND_BLOCKING_ON",
        "BOTH_STAND_BLOCKING_ON_DUAL",
        "BOTH_STAND_BLOCKING_ON_STAFF",
    ],
    ["BOTH_P1_S1_T_", "BOTH_P6_S6_T_", "BOTH_P7_S7_T_"],
];

/// `BLOCKING`'s sequences as `find` finds them.
type Guards = [[Option<Sequence>; 3]; 2];

fn guards(mut find: impl FnMut(&str) -> Option<Sequence>) -> Guards {
    BLOCKING.map(|row| row.map(&mut find))
}

/// One row of the table, read.
#[derive(Debug, Clone, Default, PartialEq)]
struct Row {
    name: String,
    templates: Vec<String>,
    tags: Vec<String>,
    player: bool,
    suffixes: Vec<String>,
    /// The skeleton its sequences are looked up in (`_humanoid_kotor`, a
    /// folder of `models/players/`), else the stack's own.
    skeleton: Option<String>,
    /// JA's name (upper case) to the one drawn, over the suffixes.
    sequences: BTreeMap<String, String>,
}

/// A row as the stack's skeleton has it: each of JA's sequences it draws
/// otherwise, and the sequence drawn.
#[derive(Debug, Clone, PartialEq)]
pub struct Look {
    pub name: String,
    pub swaps: Vec<(Anim, Sequence)>,
    /// Its held block's poses (`BLOCKING`).
    guards: Guards,
}

/// Every row with a sequence in the stack, and whom each is for.
#[derive(Debug, Clone, Default)]
pub struct DuelStances {
    pub looks: Vec<Look>,
    /// (templates, tags, for the player) by look.
    who: Vec<(Vec<String>, Vec<String>, bool)>,
    default: Option<u16>,
    /// The stack's own held block's poses, for those without a look's.
    guards: Guards,
}

impl DuelStances {
    /// Off: no looks, the held block's poses the stack's (`sequences`, by
    /// upper-case name), if it has them (Movie Duels').
    pub fn plain(sequences: &BTreeMap<String, Sequence>) -> DuelStances {
        DuelStances {
            guards: guards(|name| sequences.get(name).cloned()),
            ..DuelStances::default()
        }
    }

    /// Whether `settings` turn them on, with Movie Duels installed
    /// (`settings::movie_duels`): its characters' skeletons are its own.
    pub fn wanted(settings: &BTreeMap<String, String>) -> bool {
        settings.get(SETTING).is_some_and(|value| value == "true")
            && crate::settings::movie_duels()
    }

    /// The shipped table against the stack (`files`; `sequences` its
    /// `animation.cfg`'s, by upper-case name, played on `gla`), with what
    /// it found said in `notes`; and `gla` with the frames the rows' own
    /// skeletons lend it, when they lend any.
    pub fn load(
        files: &crate::viewer_ja::Files,
        gla: &Gla,
        sequences: &BTreeMap<String, Sequence>,
        notes: &mut Vec<String>,
    ) -> (DuelStances, Option<Gla>) {
        let rows = match rows(TABLE) {
            Ok(rows) => rows,
            Err(error) => {
                notes.push(format!("data/duel_stances.toml: {error}"));
                return (DuelStances::plain(sequences), None);
            }
        };
        let mut borrowed = Borrowed::new(gla);
        let mut stances = DuelStances::of(
            &rows,
            |row, name| match &row.skeleton {
                Some(skeleton) => borrowed.take(files, skeleton, name),
                None => sequences.get(name).cloned(),
            },
            notes,
        );
        notes.append(&mut borrowed.notes);
        stances.guards = DuelStances::plain(sequences).guards;
        (stances, borrowed.extended)
    }

    /// `rows`, each sequence looked up by `find` (a row, a name in upper
    /// case): a row none of whose sequences is found is left out.
    fn of(
        rows: &[Row],
        mut find: impl FnMut(&Row, &str) -> Option<Sequence>,
        notes: &mut Vec<String>,
    ) -> DuelStances {
        let mut stances = DuelStances::default();
        for row in rows {
            let swaps: Vec<(Anim, Sequence)> = SWAPPED
                .iter()
                .filter_map(|&anim| {
                    let own = anim.name();
                    // In a skeleton of its own, JA's name is its own look.
                    let plain = row.skeleton.is_some().then(|| own.to_string());
                    let named = row.sequences.get(own).cloned();
                    let suffixed = row.suffixes.iter().map(|suffix| format!("{own}{suffix}"));
                    let found = named
                        .into_iter()
                        .chain(suffixed)
                        .chain(plain)
                        .find_map(|name| find(row, &name.to_ascii_uppercase()))?;
                    Some((anim, found))
                })
                .collect();
            let shown: Vec<String> = swaps
                .iter()
                .map(|(anim, sequence)| format!("{} as {}", anim.name(), sequence.name))
                .collect();
            notes.push(format!(
                "duel stances, {}{}: {}",
                row.name,
                row.skeleton
                    .as_ref()
                    .map_or(String::new(), |skeleton| format!(" ({skeleton})")),
                if shown.is_empty() {
                    "none in the stack".to_string()
                } else {
                    shown.join(", ")
                }
            ));
            if swaps.is_empty() {
                continue;
            }
            let look = stances.looks.len() as u16;
            let default = row.templates.is_empty() && row.tags.is_empty() && !row.player;
            if default && stances.default.is_none() {
                stances.default = Some(look);
            }
            stances.looks.push(Look {
                name: row.name.clone(),
                swaps,
                guards: guards(|name| find(row, name)),
            });
            stances
                .who
                .push((row.templates.clone(), row.tags.clone(), row.player));
        }
        stances
    }

    /// The look `entity` stands in: its template's or tag's row, the
    /// player's for a seat's own character, else the default row's.
    pub fn look_of(&self, entity: &EntityState, objects: &Instances) -> Option<u16> {
        if self.looks.is_empty() {
            return None;
        }
        let data = objects.of_entity(entity);
        let player = matches!(entity.kind, Kind::Avatar(_));
        let found = self.who.iter().position(|(templates, tags, for_player)| {
            (player && *for_player)
                || data.is_some_and(|data| {
                    templates.iter().any(|t| glob(t, &data.template))
                        || tags.iter().any(|t| glob(t, &data.tag))
                })
        });
        found.map(|look| look as u16).or(self.default)
    }

    /// The pose a held block (`aimed`: with attack held) shows over `anim`
    /// in `look`, else the stack's (`BLOCKING`), by the blades `anim` holds.
    pub fn guard(&self, look: Option<u16>, anim: Anim, aimed: bool) -> Option<&Sequence> {
        let blades = match anim {
            Anim::BOTH_SABERDUAL_STANCE | Anim::BOTH_WALK_DUAL => 1,
            Anim::BOTH_SABERSTAFF_STANCE | Anim::BOTH_WALK_STAFF => 2,
            _ => 0,
        };
        let row = usize::from(aimed);
        look.and_then(|look| self.looks.get(usize::from(look)))
            .and_then(|look| look.guards[row][blades].as_ref())
            .or(self.guards[row][blades].as_ref())
    }

    /// Whether `anim` is a walk, which the legs keep under a held block.
    pub fn walks(anim: Anim) -> bool {
        WALKS.contains(&anim)
    }

    /// The sequence `look` draws for `anim`, if it draws another.
    pub fn swap(&self, look: Option<u16>, anim: Anim) -> Option<&Sequence> {
        let look = self.looks.get(usize::from(look?))?;
        look.swaps
            .iter()
            .find(|(swapped, _)| *swapped == anim)
            .map(|(_, sequence)| sequence)
    }
}

/// The skeletons the rows look their sequences up in, each read once, and
/// the view's skeleton with the frames taken from them added after its
/// own (`extended`): a sequence taken is its frames there.
struct Borrowed<'g> {
    gla: &'g Gla,
    extended: Option<Gla>,
    /// By folder: its frames and `animation.cfg`, or none (said once).
    read: BTreeMap<String, Option<Lender>>,
    /// (folder, name) to the frames taken.
    taken: BTreeMap<(String, String), Option<Sequence>>,
    notes: Vec<String>,
}

impl<'g> Borrowed<'g> {
    fn new(gla: &'g Gla) -> Borrowed<'g> {
        Borrowed {
            gla,
            extended: None,
            read: BTreeMap::new(),
            taken: BTreeMap::new(),
            notes: Vec::new(),
        }
    }

    /// `skeleton`'s sequence `name` (upper case), its frames added to the
    /// view's skeleton.
    fn take(
        &mut self,
        files: &crate::viewer_ja::Files,
        skeleton: &str,
        name: &str,
    ) -> Option<Sequence> {
        let key = (skeleton.to_ascii_lowercase(), name.to_string());
        if let Some(taken) = self.taken.get(&key) {
            return taken.clone();
        }
        let (gla, notes) = (self.gla, &mut self.notes);
        let lent = self
            .read
            .entry(key.0.clone())
            .or_insert_with(|| {
                lender(files, gla, &key.0)
                    .inspect_err(|error| notes.push(format!("duel stances, {}: {error}", key.0)))
                    .ok()
            })
            .as_ref();
        let taken = lent.and_then(|(from, sequences)| {
            let sequence = sequences.get(name)?;
            let into = self.extended.get_or_insert_with(|| gla.clone());
            append(into, from, sequence)
        });
        self.taken.insert(key, taken.clone());
        taken
    }
}

/// A skeleton that lends its frames, and its sequences by upper-case name.
type Lender = (Gla, BTreeMap<String, Sequence>);

/// The skeleton in `models/players/<folder>/` (its `_humanoid.gla`, or
/// the one named as its folder) and its `animation.cfg`, when its bones
/// are `gla`'s, in order.
fn lender(files: &crate::viewer_ja::Files, gla: &Gla, folder: &str) -> Result<Lender, String> {
    let base = format!("models/players/{folder}");
    let (path, bytes) = ["_humanoid", folder]
        .iter()
        .map(|stem| format!("{base}/{stem}.gla"))
        .find_map(|path| files.read(&path).map(|bytes| (path, bytes)))
        .ok_or_else(|| format!("{base}: no skeleton"))?;
    let lent = Gla::parse(&bytes).map_err(|error| format!("{path}: {error:?}"))?;
    let same = lent.bones.len() == gla.bones.len()
        && lent
            .bones
            .iter()
            .zip(&gla.bones)
            .all(|(a, b)| a.name.eq_ignore_ascii_case(&b.name));
    if !same {
        return Err(format!(
            "{path}: its {} bones are not the skeleton's {}",
            lent.bones.len(),
            gla.bones.len()
        ));
    }
    let cfg = files
        .read(&format!("{base}/animation.cfg"))
        .map(|bytes| AnimationConfig::parse(&bytes))
        .ok_or_else(|| format!("{base}/animation.cfg: not found"))?;
    let sequences = cfg
        .sequences
        .into_iter()
        .map(|s| (s.name.to_ascii_uppercase(), s))
        .collect();
    Ok((lent, sequences))
}

/// `sequence`'s frames of `from` added after `into`'s own (the bone
/// transforms they use with them): the sequence where they now are.
fn append(into: &mut Gla, from: &Gla, sequence: &Sequence) -> Option<Sequence> {
    let bones = into.bones.len();
    let first = usize::try_from(sequence.first_frame).ok()?;
    let count = usize::try_from(sequence.frame_count).ok()?;
    let frames = from.frames.get(first * bones..(first + count) * bones)?;
    let mut moved = BTreeMap::new();
    for &index in frames {
        let pool = into.pool.len() as u32;
        let at = *moved.entry(index).or_insert_with(|| {
            into.pool.push(from.pool[index as usize]);
            pool
        });
        into.frames.push(at);
    }
    let start = into.frame_count;
    into.frame_count += count;
    Some(Sequence {
        first_frame: i32::try_from(start).ok()?,
        ..sequence.clone()
    })
}

/// `shown` (the sim's playback of `own`) replayed over `other`'s frames
/// from the same moment, at the same share of its own rate. Its blend is
/// the sim's, whose frames are what the sim played before: the caller
/// takes only its length, blending from the pose it shows.
pub fn replayed(shown: Shown, own: Option<&Sequence>, other: &Sequence) -> Shown {
    let rate = |sequence: &Sequence| crate::stage::sim_sequence(sequence).frame_lerp;
    let scale = match own.map(rate) {
        Some(lerp) if lerp != 0 && shown.playback.speed != 0.0 => {
            shown.playback.speed * lerp as f32 / 50.0
        }
        _ => 1.0,
    };
    let playback = Playback::set(
        None,
        &crate::stage::sim_sequence(other),
        scale,
        shown.playback.start_ms,
        0,
        None,
    );
    Shown {
        playback: Playback {
            blend: shown.playback.blend,
            ..playback
        },
        ..shown
    }
}

/// Whether `name` matches `pattern`, any case, `*` standing for any run
/// of characters.
fn glob(pattern: &str, name: &str) -> bool {
    let (pattern, name) = (pattern.to_ascii_lowercase(), name.to_ascii_lowercase());
    let mut parts = pattern.split('*');
    let first = parts.next().unwrap_or("");
    let Some(mut rest) = name.strip_prefix(first) else {
        return false;
    };
    let parts: Vec<&str> = parts.collect();
    let Some((last, middle)) = parts.split_last() else {
        return rest.is_empty();
    };
    for part in middle {
        match rest.find(part) {
            Some(at) => rest = &rest[at + part.len()..],
            None => return false,
        }
    }
    rest.ends_with(last)
}

/// The table's rows: `[default]` first, then each `[[character]]`.
fn rows(text: &str) -> Result<Vec<Row>, String> {
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
        if read.templates.is_empty() && read.tags.is_empty() && !read.player {
            return Err(format!("{}: names no template, tag or player", read.name));
        }
        rows.push(read);
    }
    Ok(rows)
}

/// The duel stances' checks, without an install (`kjk sim probes`, group
/// "duel stances"): the table reads, a held block finds its blades'
/// guard, a skeleton's frames are lent after
/// the view's own, the wildcards match, a row finds the sequences the
/// stack has and only those, and a swapped walk keeps the sim's moment and
/// pace over the other frames.
#[cfg(feature = "probes")]
pub fn probes() -> sim::probe::Checks {
    use sim::probe::check;
    let table = match rows(TABLE) {
        Ok(rows) => Ok(format!("{} rows", rows.len())),
        Err(error) => Err(error),
    };
    let globs = [
        ("p_bastilla*", "P_Bastilla001", true),
        ("p_bastilla*", "p_bastil", false),
        ("*malak*", "end_darthmalak", true),
        ("bastila", "Bastila", true),
        ("bastila", "bastila2", false),
    ];
    let wrong: Vec<_> = globs
        .iter()
        .filter(|(pattern, name, want)| glob(pattern, name) != *want)
        .collect();
    let sequence = |name: &str, first_frame, frame_count| Sequence {
        name: name.to_string(),
        first_frame,
        frame_count,
        loop_frame: 0,
        frames_per_second: 20,
    };
    let stack: BTreeMap<String, Sequence> = [
        sequence("BOTH_SABERSTAFF_STANCE", 100, 1),
        sequence("BOTH_SABERSTAFF_STANCE_TEST", 500, 40),
        sequence("BOTH_WALK2", 200, 30),
    ]
    .into_iter()
    .map(|s| (s.name.clone(), s))
    .collect();
    let test = Row {
        name: "test".into(),
        tags: vec!["test".into()],
        suffixes: vec!["_MISSING".into(), "_TEST".into()],
        ..Row::default()
    };
    let stances = DuelStances::of(&[test], |_, name| stack.get(name).cloned(), &mut Vec::new());
    let swaps: Vec<(&str, &str)> = stances
        .looks
        .iter()
        .flat_map(|look| &look.swaps)
        .map(|(anim, sequence)| (anim.name(), sequence.name.as_str()))
        .collect();
    let walk = &stack["BOTH_WALK2"];
    let shown = Shown {
        anim: Anim::BOTH_WALK2,
        flip: false,
        playback: Playback::set(None, &crate::stage::sim_sequence(walk), 1.5, 1000, 0, None),
    };
    let replay = replayed(shown, Some(walk), &sequence("BOTH_WALK2_TEST", 700, 20)).playback;
    let kept = (replay.start_frame, replay.end_frame, replay.start_ms) == (700, 720, 1000)
        && (replay.speed - shown.playback.speed).abs() < 1e-4;
    // Two frames of a two-bone skeleton lent to a one-frame one: added
    // after its own, each bone's transform the lender's.
    let skeleton = |frames: &[[u16; 7]]| Gla {
        name: "probe".into(),
        scale: 1.0,
        bones: (0..2)
            .map(|n| fmt_ja::ghoul2::Bone {
                name: format!("bone{n}"),
                flags: 0,
                parent: n - 1,
                base_pose: [[0.0; 4]; 3],
                base_pose_inverse: [[0.0; 4]; 3],
                children: Vec::new(),
            })
            .collect(),
        frame_count: frames.len() / 2,
        frames: (0..frames.len() as u32).collect(),
        pool: frames.to_vec(),
    };
    let mut into = skeleton(&[[1; 7], [2; 7]]);
    let from = skeleton(&[[3; 7], [4; 7], [5; 7], [6; 7], [7; 7], [8; 7]]);
    let lent = append(&mut into, &from, &sequence("LENT", 1, 2));
    let lent_frames: Vec<u16> = (0..into.frame_count * 2)
        .map(|i| into.pool[into.frames[i] as usize][0])
        .collect();
    let lends = lent
        .as_ref()
        .is_some_and(|s| s.first_frame == 1 && s.frame_count == 2)
        && lent_frames == [1, 2, 5, 6, 7, 8];
    // A held block's poses, as the stack has them: a staff's guard, no
    // single blade's aimed parry (the stance plays on).
    let plain = DuelStances::plain(
        &[sequence("BOTH_STAND_BLOCKING_ON_STAFF", 900, 2)]
            .into_iter()
            .map(|s| (s.name.clone(), s))
            .collect(),
    );
    let guarded = (
        plain
            .guard(None, Anim::BOTH_SABERSTAFF_STANCE, false)
            .map(|s| s.name.as_str()),
        plain.guard(None, Anim::BOTH_STAND2, true).is_none(),
    );
    vec![
        ("the shipped table reads", table),
        (
            "a held block shows the stack's guard for its blades",
            check(
                guarded == (Some("BOTH_STAND_BLOCKING_ON_STAFF"), true),
                format!("{guarded:?}"),
            ),
        ),
        (
            "a skeleton's frames are lent after the view's own",
            check(lends, format!("{lent:?}, frames {lent_frames:?}")),
        ),
        (
            "wildcards match as written",
            check(
                wrong.is_empty(),
                format!("{} cases, wrong {wrong:?}", globs.len()),
            ),
        ),
        (
            "a row draws only the sequences the stack has",
            check(
                swaps == [("BOTH_SABERSTAFF_STANCE", "BOTH_SABERSTAFF_STANCE_TEST")],
                format!("{swaps:?}"),
            ),
        ),
        (
            "a swapped walk keeps the sim's moment and pace",
            check(kept, format!("{replay:?} from {:?}", shown.playback)),
        ),
    ]
}
