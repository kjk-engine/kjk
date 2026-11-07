//! A character with its blade lit stands and walks in its own stance and
//! walk from a mod's skeleton (Movie Duels' per-character sequences,
//! `docs/SABER-RULES.md` section 11), in place of JA's (`game.duel_stances`,
//! off by default).
//!
//! - **What:** while the sim's skeleton holds one of the ready stances or
//!   walks a lit blade plays (`SWAPPED`), the drawn body plays the
//!   character's own sequence instead, when the stack's `animation.cfg`
//!   has it: the first of its row's named sequence and JA's name with each
//!   of its row's suffixes (`BOTH_SABERDUAL_STANCE` + `_MAUL`). The table
//!   is `data/duel_stances.toml`: rows by template or tag (`*` a
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

use fmt_ja::animation_cfg::Sequence;
use sim::combat::saber::blades::Shown;
use sim::motion::Anim;
use sim::skeleton::Playback;
use sim::{EntityState, Instances, Kind};

/// The setting that turns them on (`true`).
pub const SETTING: &str = "game.duel_stances";

/// The table, as shipped.
const TABLE: &str = include_str!("../../../data/duel_stances.toml");

/// The sim's sequences a lit blade stands or walks in
/// (`sim::motion::weapon`): the ones a character's own may stand for.
pub const SWAPPED: [Anim; 8] = [
    Anim::BOTH_STAND2,
    Anim::BOTH_SABERFAST_STANCE,
    Anim::BOTH_SABERSLOW_STANCE,
    Anim::BOTH_SABERDUAL_STANCE,
    Anim::BOTH_SABERSTAFF_STANCE,
    Anim::BOTH_WALK2,
    Anim::BOTH_WALK_DUAL,
    Anim::BOTH_WALK_STAFF,
];

/// One row of the table, read.
#[derive(Debug, Clone, Default, PartialEq)]
struct Row {
    name: String,
    templates: Vec<String>,
    tags: Vec<String>,
    player: bool,
    suffixes: Vec<String>,
    /// JA's name (upper case) to the one drawn, over the suffixes.
    sequences: BTreeMap<String, String>,
}

/// A row as the stack's skeleton has it: each of JA's sequences it draws
/// otherwise, and the sequence drawn.
#[derive(Debug, Clone, PartialEq)]
pub struct Look {
    pub name: String,
    pub swaps: Vec<(Anim, Sequence)>,
}

/// Every row with a sequence in the stack, and whom each is for.
#[derive(Debug, Clone, Default)]
pub struct DuelStances {
    pub looks: Vec<Look>,
    /// (templates, tags, for the player) by look.
    who: Vec<(Vec<String>, Vec<String>, bool)>,
    default: Option<u16>,
}

impl DuelStances {
    /// Off: nothing changes.
    pub fn off() -> DuelStances {
        DuelStances::default()
    }

    /// Whether `settings` turn them on.
    pub fn wanted(settings: &BTreeMap<String, String>) -> bool {
        settings.get(SETTING).is_some_and(|value| value == "true")
    }

    /// The shipped table against `sequences` (the stack's, by upper-case
    /// name), with what it found said in `notes`.
    pub fn load(sequences: &BTreeMap<String, Sequence>, notes: &mut Vec<String>) -> DuelStances {
        match rows(TABLE) {
            Ok(rows) => DuelStances::of(&rows, sequences, notes),
            Err(error) => {
                notes.push(format!("data/duel_stances.toml: {error}"));
                DuelStances::off()
            }
        }
    }

    /// `rows` against `sequences`: a row none of whose sequences the stack
    /// has is left out.
    fn of(
        rows: &[Row],
        sequences: &BTreeMap<String, Sequence>,
        notes: &mut Vec<String>,
    ) -> DuelStances {
        let mut stances = DuelStances::default();
        for row in rows {
            let swaps: Vec<(Anim, Sequence)> = SWAPPED
                .iter()
                .filter_map(|&anim| {
                    let own = anim.name();
                    let named = row.sequences.get(own).cloned();
                    let suffixed = row.suffixes.iter().map(|suffix| format!("{own}{suffix}"));
                    let found = named
                        .into_iter()
                        .chain(suffixed)
                        .find_map(|name| sequences.get(&name.to_ascii_uppercase()))?;
                    Some((anim, found.clone()))
                })
                .collect();
            let shown: Vec<String> = swaps
                .iter()
                .map(|(anim, sequence)| format!("{} as {}", anim.name(), sequence.name))
                .collect();
            notes.push(format!(
                "duel stances, {}: {}",
                row.name,
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

    /// The sequence `look` draws for `anim`, if it draws another.
    pub fn swap(&self, look: Option<u16>, anim: Anim) -> Option<&Sequence> {
        let look = self.looks.get(usize::from(look?))?;
        look.swaps
            .iter()
            .find(|(swapped, _)| *swapped == anim)
            .map(|(_, sequence)| sequence)
    }
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
/// "duel stances"): the table reads, the wildcards match, a row finds the
/// sequences the stack has and only those, and a swapped walk keeps the
/// sim's moment and pace over the other frames.
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
    let stances = DuelStances::of(&[test], &stack, &mut Vec::new());
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
    vec![
        ("the shipped table reads", table),
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
