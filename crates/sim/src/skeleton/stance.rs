//! Stance sets: the sequences a character's stance plays in place of JA's
//! own while its blade is lit. A set holds 61 sequences. The first 10
//! stand for JA's stances, fidgets and walks (`SWAPPED`). The next 3 are a
//! held block's standing guards (`STANDING`), and the 15 after them are a
//! held block's parries (`PARRIES`, each toward one of `QUADRANTS`). Then
//! come 12 standing guards for a holder that moves (`STANDING` with each of
//! `STEPS`), and last the 21 perfect parries (`PERFECT`, by where the
//! blades met and the weapon). A set's frames
//! are lent to the skeleton after its own frames
//! (`SkeletonData::with_lent`).

use std::sync::LazyLock;

use super::{Sequence, SkeletonData};
use crate::motion::{Anim, AnimTable};

/// JA's sequences a lit blade stands, fidgets or walks in, which a set may
/// replace, in a set's order. The fidgets go with the stance, or a
/// character in another stance would snap back to JA's now and then.
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

/// The walks a lit blade's legs play, forward and back. Under a held block
/// the legs keep walking.
pub const WALKS: [Anim; 6] = [
    Anim::BOTH_WALK2,
    Anim::BOTH_WALK_DUAL,
    Anim::BOTH_WALK_STAFF,
    Anim::BOTH_WALKBACK2,
    Anim::BOTH_WALKBACK_DUAL,
    Anim::BOTH_WALKBACK_STAFF,
];

/// The runs a lit blade's legs play. Under a held block the legs keep
/// running, and the torso shows the guard as it does over a walk.
pub const RUNS: [Anim; 4] = [
    Anim::BOTH_RUN2,
    Anim::BOTH_RUNBACK2,
    Anim::BOTH_RUN_DUAL,
    Anim::BOTH_RUN_STAFF,
];

/// Whether `anim` is a lit blade's walk or run (`WALKS`, `RUNS`), which the
/// legs keep under a held block.
pub fn moves(anim: Anim) -> bool {
    WALKS.contains(&anim) || RUNS.contains(&anim)
}

/// A held block's standing guard for one blade, two blades and a staff.
/// JA's skeleton has none of them.
pub const STANDING: [&str; 3] = [
    "BOTH_STAND_BLOCKING_ON",
    "BOTH_STAND_BLOCKING_ON_DUAL",
    "BOTH_STAND_BLOCKING_ON_STAFF",
];

/// The endings of a standing guard's name (`STANDING`) that give the guard
/// a holder shows while it moves, in `Step`'s order. JA's skeleton has
/// none of them.
pub const STEPS: [&str; 4] = ["_FORWARD", "_BACK", "_LEFT", "_RIGHT"];

/// A held block's parries for one blade, two blades and a staff. Each name
/// is followed by its quadrant (`QUADRANTS`).
pub const PARRIES: [&str; 3] = ["BOTH_P1_S1_", "BOTH_P6_S6_", "BOTH_P7_S7_"];

/// The parries' quadrants, in `Quadrant`'s order.
pub const QUADRANTS: [&str; 5] = ["T_", "TL", "TR", "BL", "BR"];

/// The perfect parries (`docs/SABER-RULES.md` 3.1), one row for each place
/// the blades may meet (`Met`, in its order) and a last row for a
/// creature's good parry met at the centre. Each row names the parry for
/// one blade, two blades and a staff. The torso plays one over the ready
/// move after a perfect block.
pub const PERFECT: [[&str; 3]; 7] = [
    ["BOTH_K1_S1_TR_MD", "BOTH_B6_TR___", "BOTH_B7_TR___"],
    ["BOTH_K1_S1_TL_MD", "BOTH_B6_TL___", "BOTH_B7_TL___"],
    ["BOTH_B1_T____", "BOTH_B6_T____", "BOTH_B7_T____"],
    ["BOTH_B1_BR___", "BOTH_B6_BR___", "BOTH_B7_BR___"],
    ["BOTH_B1_BL___", "BOTH_B6_BL___", "BOTH_B7_BL___"],
    ["BOTH_P1_S1_B_", "BOTH_P6_S1_B_", "BOTH_P7_S1_B_"],
    ["BOTH_P1_S1_T__MD", "BOTH_P6_S6_T__MD", "BOTH_P7_S7_T__MD"],
];

/// The number of perfect parries a set holds (`PERFECT`).
pub const PERFECTS: usize = PERFECT.len() * STANDING.len();

/// Where a perfect parry met the blow, as seen from the blocker
/// (`docs/SABER-RULES.md` 3.1). The upper and the middle bands share their
/// parries, and the low band has no centre.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Met {
    Right,
    Left,
    Centre,
    LowRight,
    LowLeft,
    Behind,
}

impl Met {
    /// The index in `PERFECT`'s rows and columns of the parry for a blow
    /// met here, by a fighter holding `blades` (0 for one blade, 1 for two,
    /// 2 for a staff). A creature's `good` parry at the centre takes the
    /// last row.
    pub fn perfect(self, blades: usize, good: bool) -> u8 {
        let row = if good && self == Met::Centre {
            PERFECT.len() - 1
        } else {
            self as usize
        };
        (row * STANDING.len() + blades.min(STANDING.len() - 1)) as u8
    }
}

/// The sequences of a set: 10 swaps, 3 standing guards, 15 parries
/// (blades * 5 + quadrant), 12 standing guards while moving (blades * 4 +
/// step) and 21 perfect parries (row * 3 + blades).
pub const SEQUENCES: usize = 61;

/// The most sets a level holds. A fighter numbers its set in a `u8`, and 0
/// is no set.
pub const MAX_SETS: usize = 255;

/// A set's place of its first standing guard.
pub const STANDING_AT: usize = SWAPPED.len();

/// A set's place of its first parry.
pub const PARRIES_AT: usize = STANDING_AT + STANDING.len();

/// A set's place of its first standing guard while moving.
pub const MOVING_AT: usize = PARRIES_AT + PARRIES.len() * QUADRANTS.len();

/// A set's place of its first perfect parry.
pub const PERFECT_AT: usize = MOVING_AT + STANDING.len() * STEPS.len();

/// The number of JA's first parry. JA's parries take the numbers 1 to 15.
const JA_PARRY: u16 = 1;

/// The number of the first set's first sequence.
const FIRST_SET: u16 = JA_PARRY + (PARRIES.len() * QUADRANTS.len()) as u16;

const _: () = assert!(PERFECT_AT + PERFECTS == SEQUENCES);
const _: () = assert!(FIRST_SET as usize + MAX_SETS * SEQUENCES <= u16::MAX as usize);

/// Where a held block's parry faces (`QUADRANTS`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Quadrant {
    Top,
    TopLeft,
    TopRight,
    BottomLeft,
    BottomRight,
}

impl Quadrant {
    /// The parry a held block shows while its holder moves by a command's
    /// `forward` and `right`, as Movie Duels' directional blocking does.
    /// Moving back and to a side, or to a side alone, gives the top left
    /// or the top right. Moving forward and to a side gives the bottom left
    /// or the bottom right. Moving straight back or forward gives the top.
    /// Standing still gives none.
    pub fn of(forward: i8, right: i8) -> Option<Quadrant> {
        Some(match (forward.signum(), right.signum()) {
            (0, 0) => return None,
            (1, -1) => Quadrant::BottomLeft,
            (1, 1) => Quadrant::BottomRight,
            (_, -1) => Quadrant::TopLeft,
            (_, 1) => Quadrant::TopRight,
            _ => Quadrant::Top,
        })
    }
}

/// Which way the holder of a held block moves, for the standing guard it
/// shows while it moves (`STEPS`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Step {
    Forward,
    Back,
    Left,
    Right,
}

impl Step {
    /// The step of a command's `forward` and `right`. A move with a
    /// sideways part counts as a move to that side. Standing still gives
    /// none.
    pub fn of(forward: i8, right: i8) -> Option<Step> {
        Some(match (forward.signum(), right.signum()) {
            (_, -1) => Step::Left,
            (_, 1) => Step::Right,
            (1, _) => Step::Forward,
            (-1, _) => Step::Back,
            _ => return None,
        })
    }
}

/// A held block. `aimed` says whether attack is held with it. `way` and
/// `step` say where its holder moves, as a parry's quadrant and as a step.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Guard {
    pub aimed: bool,
    pub way: Option<Quadrant>,
    pub step: Option<Step>,
}

impl Guard {
    /// A held block, `aimed` or not, whose holder moves by a command's
    /// `forward` and `right`.
    pub fn new(aimed: bool, forward: i8, right: i8) -> Guard {
        Guard {
            aimed,
            way: Quadrant::of(forward, right),
            step: Step::of(forward, right),
        }
    }
}

/// A perfect parry the torso plays: its index in `PERFECT` (row * 3 +
/// blades) and the sim time it began at.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Parry {
    pub index: u8,
    pub at: u64,
}

/// What a fighter's stance is asked to show: whether its blade is `lit`,
/// its held block `guard` (none when no block is held), and the perfect
/// `parry` its torso plays, if any.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Wield {
    pub lit: bool,
    pub guard: Option<Guard>,
    pub parry: Option<Parry>,
}

/// What a part plays in place of its animation's own sequence. 0 is the
/// animation's own. 1 to 15 is JA's own parry (1 + blades * 5 + quadrant).
/// 16 and up is 16 + (set - 1) * `SEQUENCES` + the sequence's place in the
/// set.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Played(pub u16);

impl Played {
    pub const OWN: Played = Played(0);

    /// Whether this is a set's perfect parry.
    pub fn parries_perfectly(self) -> bool {
        self.0
            .checked_sub(FIRST_SET)
            .is_some_and(|number| usize::from(number) % SEQUENCES >= PERFECT_AT)
    }
}

/// One stance set: its name, its sequences by place (none where the set
/// lacks one), and its digest (`set_digest`).
#[derive(Debug, Clone, PartialEq)]
pub struct StanceSet {
    pub name: String,
    pub sequences: [Option<Sequence>; SEQUENCES],
    pub digest: [u8; 32],
}

/// One row of the stance table. A character takes the row's set (1-based)
/// when its template matches one of `templates`, its tag matches one of
/// `tags`, or it is a player's character and `player` is set.
#[derive(Debug, Clone, PartialEq)]
pub struct StanceRow {
    pub templates: Vec<String>,
    pub tags: Vec<String>,
    pub player: bool,
    pub set: u8,
}

/// A level's stance sets and the table that gives each character one.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct StanceSets {
    sets: Vec<StanceSet>,
    rows: Vec<StanceRow>,
    default: u8,
}

impl StanceSets {
    /// The sets, the table's rows, and the set a character takes when no
    /// row matches it (0 for none). Sets past `MAX_SETS` are dropped.
    pub fn new(mut sets: Vec<StanceSet>, rows: Vec<StanceRow>, default: u8) -> StanceSets {
        sets.truncate(MAX_SETS);
        StanceSets {
            sets,
            rows,
            default,
        }
    }

    pub fn len(&self) -> usize {
        self.sets.len()
    }

    pub fn is_empty(&self) -> bool {
        self.sets.is_empty()
    }

    /// Set `number`, counted from 1. Number 0 and numbers past the list
    /// give none.
    pub fn set(&self, number: u8) -> Option<&StanceSet> {
        self.sets.get(usize::from(number.checked_sub(1)?))
    }

    /// The name and the digest of every set, in order.
    pub fn ids(&self) -> Vec<(String, [u8; 32])> {
        self.sets
            .iter()
            .map(|set| (set.name.clone(), set.digest))
            .collect()
    }

    /// BLAKE3 of the number of sets, every set's name and digest in order,
    /// the table's rows in order and its default. Two machines with the
    /// same digest give every character the same set.
    pub fn digest(&self) -> [u8; 32] {
        let mut h = collision::Digester::new();
        let text = |h: &mut collision::Digester, text: &str| {
            h.update(&(text.len() as u32).to_le_bytes());
            h.update(text.as_bytes());
        };
        let texts = |h: &mut collision::Digester, texts: &[String]| {
            h.update(&(texts.len() as u32).to_le_bytes());
            for one in texts {
                text(h, one);
            }
        };
        h.update(b"KJK stance sets 2");
        h.update(&(self.sets.len() as u32).to_le_bytes());
        for set in &self.sets {
            text(&mut h, &set.name);
            h.update(&set.digest);
        }
        h.update(&(self.rows.len() as u32).to_le_bytes());
        for row in &self.rows {
            texts(&mut h, &row.templates);
            texts(&mut h, &row.tags);
            h.update(&[u8::from(row.player), row.set]);
        }
        h.update(&[self.default]);
        *h.finalize().as_bytes()
    }

    /// The set the table gives a character. The first row that matches it
    /// decides, and the default decides when no row does.
    pub fn row_for(&self, template: &str, tag: &str, player: bool) -> u8 {
        self.rows
            .iter()
            .find(|row| {
                (player && row.player)
                    || row.templates.iter().any(|t| glob(t, template))
                    || row.tags.iter().any(|t| glob(t, tag))
            })
            .map_or(self.default, |row| row.set)
    }

    /// The name of the sequence `played` names, for logs. A set's sequence
    /// is named by the set and the place it fills. The animation's own
    /// sequence gives none.
    pub fn name_of(&self, played: Played) -> Option<String> {
        let parry = played.0.checked_sub(JA_PARRY)?;
        let Some(number) = played.0.checked_sub(FIRST_SET) else {
            return Some(parry_name(usize::from(parry)));
        };
        let (set, place) = (
            usize::from(number) / SEQUENCES,
            usize::from(number) % SEQUENCES,
        );
        let set = self.sets.get(set)?;
        Some(format!("{} {}", set.name, place_name(place)))
    }

    /// The perfect parry at `index` of `PERFECT` (row * 3 + blades) in set
    /// `set`, if the set has it.
    pub fn perfect(&self, set: u8, index: u8) -> Option<Sequence> {
        self.set(set)?
            .sequences
            .get(PERFECT_AT + usize::from(index))
            .copied()
            .flatten()
    }

    /// What the legs and the torso play in place of `anims` (legs, torso)
    /// for a character in set `set` (0 for none) that shows `wield`. None
    /// is the animation's own.
    pub fn resolve(
        &self,
        set: u8,
        wield: Wield,
        anims: [Anim; 2],
        table: &AnimTable,
    ) -> [Option<(Played, Sequence)>; 2] {
        let chosen = self.set(set).map(|found| (set, found));
        resolve_in(chosen, wield, anims, table)
    }
}

/// `StanceSets::resolve` for a level without sets. Only JA's own parries
/// show, under a held block.
pub fn resolve_plain(
    wield: Wield,
    anims: [Anim; 2],
    table: &AnimTable,
) -> [Option<(Played, Sequence)>; 2] {
    resolve_in(None, wield, anims, table)
}

/// The rule both `resolve`s follow, for set `set` (its number and the set)
/// or none.
///
/// An unlit blade plays its own on both parts. While a perfect parry
/// plays, the torso plays the set's parry over any animation. A part is
/// guarded under a held block when its animation is a stance, a fidget, a
/// walk or a run, or the lowering of a blade to rest and back, and when it
/// is the torso or does not walk or run. While a perfect parry plays, the
/// legs are guarded over any animation but a walk or a run, because the
/// parry plays on the torso alone. A guarded part plays the guard's pose.
/// Any other part plays the set's sequence for its animation, if the set
/// has one. When the torso plays its own, the legs play their own too,
/// because a stance on the legs turns the hips away from the pose the
/// torso plays.
fn resolve_in(
    set: Option<(u8, &StanceSet)>,
    wield: Wield,
    anims: [Anim; 2],
    table: &AnimTable,
) -> [Option<(Played, Sequence)>; 2] {
    if !wield.lit {
        return [None; 2];
    }
    let parts = [0, 1].map(|part| {
        let anim = anims[part];
        let parried = wield
            .parry
            .filter(|_| part == 1)
            .and_then(|parry| from_set(set, PERFECT_AT + usize::from(parry.index)));
        if parried.is_some() {
            return parried;
        }
        let under = guarded(anim) || (part == 0 && wield.parry.is_some());
        let guarded = wield
            .guard
            .filter(|_| under && (part == 1 || !moves(anim)))
            .and_then(|guard| guard_pose(set, table, anim, guard));
        guarded.or_else(|| {
            let place = SWAPPED.iter().position(|&swapped| swapped == anim)?;
            from_set(set, place)
        })
    });
    if parts[1].is_none() {
        return [None; 2];
    }
    parts
}

/// Whether a held block's pose shows over `anim`. It shows over a lit
/// blade's stance, fidget, walk or run, and over its lowering to rest and
/// back, because the sim's idle blade lowers while block is held.
fn guarded(anim: Anim) -> bool {
    SWAPPED.contains(&anim)
        || moves(anim)
        || matches!(anim, Anim::BOTH_STAND2TO1 | Anim::BOTH_STAND1TO2)
}

/// The blades `anim` holds, as an index of `STANDING` and `PARRIES`. The
/// dual stance, walks and run hold two blades (1), the staff's hold a
/// staff (2), and any other holds one blade (0).
pub fn blades(anim: Anim) -> usize {
    match anim {
        Anim::BOTH_SABERDUAL_STANCE
        | Anim::BOTH_WALK_DUAL
        | Anim::BOTH_WALKBACK_DUAL
        | Anim::BOTH_RUN_DUAL => 1,
        Anim::BOTH_SABERSTAFF_STANCE
        | Anim::BOTH_WALK_STAFF
        | Anim::BOTH_WALKBACK_STAFF
        | Anim::BOTH_RUN_STAFF => 2,
        _ => 0,
    }
}

/// The pose a held block shows over `anim`.
///
/// Without attack held, a holder that moves shows the standing guard for
/// its step, and a holder that stands still shows the standing guard.
/// Where the guard for a step is missing, the standing guard shows. A
/// staff with attack held standing still also shows its standing guard,
/// because JA's top parry with a staff holds the staff level through a
/// KOTOR body's face. With attack held otherwise, or where the guard is
/// missing, the pose is the parry toward the move, and the top parry last.
fn guard_pose(
    set: Option<(u8, &StanceSet)>,
    table: &AnimTable,
    anim: Anim,
    guard: Guard,
) -> Option<(Played, Sequence)> {
    let blades = blades(anim);
    let staff_still = guard.aimed && blades == 2 && guard.way.is_none();
    let standing = || {
        guard
            .step
            .and_then(|step| from_set(set, MOVING_AT + blades * STEPS.len() + step as usize))
            .or_else(|| from_set(set, STANDING_AT + blades))
    };
    (!guard.aimed || staff_still)
        .then(standing)
        .flatten()
        .or_else(|| parry(set, table, blades, guard.way?))
        .or_else(|| parry(set, table, blades, Quadrant::Top))
}

/// A held block's parry for `blades` toward `quadrant`. The set's own
/// parry comes first, and JA's own parry from `table` after it.
fn parry(
    set: Option<(u8, &StanceSet)>,
    table: &AnimTable,
    blades: usize,
    quadrant: Quadrant,
) -> Option<(Played, Sequence)> {
    let index = blades * QUADRANTS.len() + quadrant as usize;
    from_set(set, PARRIES_AT + index).or_else(|| {
        let info = table.get(JA_PARRIES[index]?);
        (!info.missing()).then(|| {
            (
                Played(JA_PARRY + index as u16),
                crate::combat::saber::blades::sequence(info),
            )
        })
    })
}

/// The sequence at `place` of `set`, if the set has one.
fn from_set(set: Option<(u8, &StanceSet)>, place: usize) -> Option<(Played, Sequence)> {
    let (number, set) = set?;
    let sequence = set.sequences[place]?;
    let first = FIRST_SET + u16::from(number - 1) * SEQUENCES as u16;
    Some((Played(first + place as u16), sequence))
}

/// JA's own parries (`PARRIES` and `QUADRANTS`), by blades * 5 + quadrant.
static JA_PARRIES: LazyLock<[Option<Anim>; PARRIES.len() * QUADRANTS.len()]> =
    LazyLock::new(|| std::array::from_fn(|index| Anim::from_name(&parry_name(index))));

/// The name of the parry at `index` (blades * 5 + quadrant).
fn parry_name(index: usize) -> String {
    format!(
        "{}{}",
        PARRIES[index / QUADRANTS.len()],
        QUADRANTS[index % QUADRANTS.len()]
    )
}

/// The name of the sequence that place `place` of a set stands for: JA's
/// name for a swap and a parry, and the name a skeleton gives a standing
/// guard or a perfect parry.
pub fn place_name(place: usize) -> String {
    if let Some(anim) = SWAPPED.get(place) {
        anim.name().to_string()
    } else if place < PARRIES_AT {
        STANDING[place - STANDING_AT].to_string()
    } else if place < MOVING_AT {
        parry_name(place - PARRIES_AT)
    } else if place < PERFECT_AT {
        let index = place - MOVING_AT;
        format!(
            "{}{}",
            STANDING[index / STEPS.len()],
            STEPS[index % STEPS.len()]
        )
    } else {
        let index = place - PERFECT_AT;
        PERFECT[index / STANDING.len()][index % STANDING.len()].to_string()
    }
}

/// BLAKE3 of a set's 61 places. For each place it covers whether the set
/// has a sequence there, and for a sequence its frame count, its
/// `frame_lerp`, whether it loops, and every frame's key for every bone of
/// `data`, as bits. Where the frames sit among the lent ones is left out.
/// Snapshots, saves and recordings name absolute frame numbers in every
/// playback. Two machines with equal digests therefore agree only if the
/// place of every lent frame follows from the ordered list of sets alone.
pub fn set_digest(sequences: &[Option<Sequence>; SEQUENCES], data: &SkeletonData) -> [u8; 32] {
    let mut h = collision::Digester::new();
    h.update(b"KJK stance set 2");
    for slot in sequences {
        let Some(sequence) = slot else {
            h.update(&[0]);
            continue;
        };
        h.update(&[1]);
        h.update(&sequence.frames.to_le_bytes());
        h.update(&sequence.frame_lerp.to_le_bytes());
        h.update(&[u8::from(sequence.looping)]);
        for frame in sequence.first_frame..sequence.first_frame + sequence.frames {
            for bone in 0..data.bones().len() {
                for value in data.key(frame, bone) {
                    h.update(&value.to_le_bytes());
                }
            }
        }
    }
    *h.finalize().as_bytes()
}

/// Whether `name` matches `pattern`, in any case. A `*` in the pattern
/// stands for any run of characters.
pub fn glob(pattern: &str, name: &str) -> bool {
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
