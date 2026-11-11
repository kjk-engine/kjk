//! Stance sets: the sequences a character's stance plays in place of JA's
//! own while its blade is lit. A set holds 28 sequences. The first 10
//! stand for JA's stances, fidgets and walks (`SWAPPED`). The next 3 are a
//! held block's standing guards (`STANDING`), and the last 15 are a held
//! block's parries (`PARRIES`, each toward one of `QUADRANTS`). A set's
//! frames are lent to the skeleton after its own frames
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

/// A held block's standing guard for one blade, two blades and a staff.
/// JA's skeleton has none of them.
pub const STANDING: [&str; 3] = [
    "BOTH_STAND_BLOCKING_ON",
    "BOTH_STAND_BLOCKING_ON_DUAL",
    "BOTH_STAND_BLOCKING_ON_STAFF",
];

/// A held block's parries for one blade, two blades and a staff. Each name
/// is followed by its quadrant (`QUADRANTS`).
pub const PARRIES: [&str; 3] = ["BOTH_P1_S1_", "BOTH_P6_S6_", "BOTH_P7_S7_"];

/// The parries' quadrants, in `Quadrant`'s order.
pub const QUADRANTS: [&str; 5] = ["T_", "TL", "TR", "BL", "BR"];

/// The sequences of a set: 10 swaps, then 3 standing guards, then 15
/// parries (blades * 5 + quadrant).
pub const SEQUENCES: usize = 28;

/// The most sets a level holds. A fighter numbers its set in a `u8`, and 0
/// is no set.
pub const MAX_SETS: usize = 255;

/// A set's place of its first standing guard.
const STANDING_AT: usize = SWAPPED.len();

/// A set's place of its first parry.
const PARRIES_AT: usize = STANDING_AT + STANDING.len();

/// The number of JA's first parry. JA's parries take the numbers 1 to 15.
const JA_PARRY: u16 = 1;

/// The number of the first set's first sequence.
const FIRST_SET: u16 = JA_PARRY + (PARRIES.len() * QUADRANTS.len()) as u16;

const _: () = assert!(PARRIES_AT + PARRIES.len() * QUADRANTS.len() == SEQUENCES);
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

/// A held block. `aimed` says whether attack is held with it, and `way`
/// says where its holder moves.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Guard {
    pub aimed: bool,
    pub way: Option<Quadrant>,
}

/// What a part plays in place of its animation's own sequence. 0 is the
/// animation's own. 1 to 15 is JA's own parry (1 + blades * 5 + quadrant).
/// 16 and up is 16 + (set - 1) * `SEQUENCES` + the sequence's place in the
/// set.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Played(pub u16);

impl Played {
    pub const OWN: Played = Played(0);
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

    /// BLAKE3 of the number of sets and every set's name and digest, in
    /// order.
    pub fn digest(&self) -> [u8; 32] {
        let mut h = collision::Digester::new();
        h.update(b"KJK stance sets 1");
        h.update(&(self.sets.len() as u32).to_le_bytes());
        for set in &self.sets {
            h.update(&(set.name.len() as u32).to_le_bytes());
            h.update(set.name.as_bytes());
            h.update(&set.digest);
        }
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

    /// What the legs and the torso play in place of `anims` (legs, torso)
    /// for a character in set `set` (0 for none), with its blade `lit` or
    /// not, under a held block `guard` or none. None is the animation's own.
    pub fn resolve(
        &self,
        set: u8,
        lit: bool,
        guard: Option<Guard>,
        anims: [Anim; 2],
        table: &AnimTable,
    ) -> [Option<(Played, Sequence)>; 2] {
        let chosen = self.set(set).map(|found| (set, found));
        resolve_in(chosen, lit, guard, anims, table)
    }
}

/// `StanceSets::resolve` for a level without sets. Only JA's own parries
/// show, under a held block.
pub fn resolve_plain(
    lit: bool,
    guard: Option<Guard>,
    anims: [Anim; 2],
    table: &AnimTable,
) -> [Option<(Played, Sequence)>; 2] {
    resolve_in(None, lit, guard, anims, table)
}

/// The rule both `resolve`s follow, for set `set` (its number and the set)
/// or none.
///
/// An unlit blade plays its own on both parts. A part is guarded under a
/// held block when its animation is a stance, a fidget or a walk, or the
/// lowering of a blade to rest and back, and when it is the torso or does
/// not walk. A guarded part plays the guard's pose. A part that is not
/// guarded plays the set's sequence for its animation, if the set has one.
/// When the torso plays its own, the legs play their own too, because a
/// stance on the legs turns the hips away from the pose the torso plays.
fn resolve_in(
    set: Option<(u8, &StanceSet)>,
    lit: bool,
    guard: Option<Guard>,
    anims: [Anim; 2],
    table: &AnimTable,
) -> [Option<(Played, Sequence)>; 2] {
    if !lit {
        return [None; 2];
    }
    let parts = [0, 1].map(|part| {
        let anim = anims[part];
        let guarded = guard
            .filter(|_| guarded(anim) && (part == 1 || !WALKS.contains(&anim)))
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
/// blade's stance, fidget or walk, and over its lowering to rest and back,
/// because the sim's idle blade lowers while block is held.
fn guarded(anim: Anim) -> bool {
    SWAPPED.contains(&anim)
        || WALKS.contains(&anim)
        || matches!(anim, Anim::BOTH_STAND2TO1 | Anim::BOTH_STAND1TO2)
}

/// The blades `anim` holds, as an index of `STANDING` and `PARRIES`. The
/// dual stance and walks hold two blades (1), the staff's hold a staff
/// (2), and any other holds one blade (0).
fn blades(anim: Anim) -> usize {
    match anim {
        Anim::BOTH_SABERDUAL_STANCE | Anim::BOTH_WALK_DUAL | Anim::BOTH_WALKBACK_DUAL => 1,
        Anim::BOTH_SABERSTAFF_STANCE | Anim::BOTH_WALK_STAFF | Anim::BOTH_WALKBACK_STAFF => 2,
        _ => 0,
    }
}

/// The pose a held block shows over `anim`. Moving toward a quadrant, it
/// is that quadrant's parry. Standing with attack held, it is the top
/// parry. Standing otherwise, it is the standing guard. Where that pose is
/// missing, the top parry shows.
fn guard_pose(
    set: Option<(u8, &StanceSet)>,
    table: &AnimTable,
    anim: Anim,
    guard: Guard,
) -> Option<(Played, Sequence)> {
    let blades = blades(anim);
    let parry = guard.way.or(guard.aimed.then_some(Quadrant::Top));
    pose(set, table, blades, parry).or_else(|| pose(set, table, blades, Some(Quadrant::Top)))
}

/// A held block's pose for `blades`: the parry toward `parry`, or the
/// standing guard for none. The set's own pose comes first, and JA's own
/// parry from `table` after it. JA has no standing guard.
fn pose(
    set: Option<(u8, &StanceSet)>,
    table: &AnimTable,
    blades: usize,
    parry: Option<Quadrant>,
) -> Option<(Played, Sequence)> {
    let place = match parry {
        Some(quadrant) => PARRIES_AT + blades * QUADRANTS.len() + quadrant as usize,
        None => STANDING_AT + blades,
    };
    from_set(set, place).or_else(|| {
        let index = blades * QUADRANTS.len() + parry? as usize;
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

/// The name of JA's sequence that place `place` of a set stands for.
fn place_name(place: usize) -> String {
    if let Some(anim) = SWAPPED.get(place) {
        anim.name().to_string()
    } else if place < PARRIES_AT {
        STANDING[place - STANDING_AT].to_string()
    } else {
        parry_name(place - PARRIES_AT)
    }
}

/// BLAKE3 of a set's 28 places. For each place it covers whether the set
/// has a sequence there, and for a sequence its frame count, its
/// `frame_lerp`, whether it loops, and every frame's key for every bone of
/// `data`, as bits. Where the frames sit among the lent ones is left out.
/// Snapshots, saves and recordings name absolute frame numbers in every
/// playback. Two machines with equal digests therefore agree only if the
/// place of every lent frame follows from the ordered list of sets alone.
pub fn set_digest(sequences: &[Option<Sequence>; SEQUENCES], data: &SkeletonData) -> [u8; 32] {
    let mut h = collision::Digester::new();
    h.update(b"KJK stance set 1");
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
