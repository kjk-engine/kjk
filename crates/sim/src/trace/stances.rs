//! Two made-up stance sets on the duels' made-up skeleton, for the stance
//! sets' probes (`stance_probes`). Each set lends 48 frames after the
//! skeleton's 64 own frames.

use std::sync::Arc;

use super::duels::key;
use crate::motion::AnimTable;
use crate::skeleton::stance::{SEQUENCES, set_digest};
use crate::skeleton::{Sequence, SkeletonData, StanceRow, StanceSet, StanceSets};

/// The frames each set lends.
const LENT: i32 = 48;

/// The sets' names, in order.
const NAMES: [&str; 2] = ["synthetic_a", "synthetic_b"];

/// A set's places of the standing guards, which set b lacks.
const STANDING: std::ops::Range<usize> = 10..13;

/// `own` with both sets' frames lent after its own. Each lent frame turns
/// every bone by an angle of its own. With `nudged`, the first lent frame
/// turns its second bone 10 degrees further, so set a's first sequence
/// differs by one key.
pub(super) fn lent_skeleton(own: &SkeletonData, nudged: bool) -> Result<SkeletonData, String> {
    let bones = own.bones().len();
    let mut keys = Vec::new();
    let mut pool = Vec::new();
    for frame in 0..NAMES.len() * LENT as usize {
        for bone in 0..bones {
            let mut degrees = (frame * 7 + bone * 13) as f32 % 90.0 - 45.0;
            if nudged && frame == 0 && bone == 1 {
                degrees += 10.0;
            }
            keys.push(pool.len() as u32);
            pool.push(key(
                [0.2 * bone as f32, 1.0, 0.3],
                degrees,
                [0.0, 0.0, bone as f32],
            ));
        }
    }
    own.clone()
        .with_lent(keys, pool)
        .map_err(|error| error.to_string())
}

/// Set `set`'s sequences (0 for set a, 1 for set b), each a short run of
/// the set's own lent frames. Set b has no standing guards.
fn sequences(own_frames: i32, set: usize) -> [Option<Sequence>; SEQUENCES] {
    let first = own_frames + set as i32 * LENT;
    std::array::from_fn(|place| {
        let place_i = place as i32;
        (set == 0 || !STANDING.contains(&place)).then_some(Sequence {
            first_frame: first + (place_i * 5) % 36,
            frames: 4 + place_i % 8,
            frame_lerp: 40 + place_i,
            looping: place < STANDING.start,
        })
    })
}

/// How `synthetic_as` builds the made-up sets.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Variant {
    /// The sets as `synthetic` gives them.
    Same,
    /// The same sets with set b listed first, so set a is number 2.
    Swapped,
    /// Set a's first lent frame is turned (`lent_skeleton`'s `nudged`), so
    /// set a has another digest. Set b is unchanged.
    Nudged,
    /// Set b has set a's sequences on set a's frames, so the two sets have
    /// equal digests.
    Twins,
}

/// `table` with two made-up sets, "synthetic_a" and "synthetic_b", on its
/// skeleton (the duels' 64 frames), which lends 48 frames to each. Set a
/// has all 28 sequences, and set b lacks the 3 standing guards. A tag
/// matching "*sith*" takes set 2, a player's character takes set 1, and
/// every other character takes set 1 by default.
pub fn synthetic(table: AnimTable) -> Result<AnimTable, String> {
    synthetic_as(table, Variant::Same)
}

/// `synthetic`, with the sets built as `variant` says. The table's rows
/// are the same in every variant.
pub(super) fn synthetic_as(table: AnimTable, variant: Variant) -> Result<AnimTable, String> {
    let own = table.skeleton().ok_or("The table has no skeleton.")?;
    let own_frames = own.frame_count() as i32;
    let data = lent_skeleton(own, variant == Variant::Nudged)?;
    let mut sets: Vec<StanceSet> = NAMES
        .iter()
        .enumerate()
        .map(|(set, name)| {
            let from = if variant == Variant::Twins { 0 } else { set };
            let sequences = sequences(own_frames, from);
            StanceSet {
                name: name.to_string(),
                digest: set_digest(&sequences, &data),
                sequences,
            }
        })
        .collect();
    if variant == Variant::Swapped {
        sets.reverse();
    }
    let row = |tags: &[&str], player, set| StanceRow {
        templates: Vec::new(),
        tags: tags.iter().map(|tag| tag.to_string()).collect(),
        player,
        set,
    };
    let rows = vec![row(&["*sith*"], false, 2), row(&[], true, 1)];
    Ok(table
        .with_skeleton(Arc::new(data))
        .with_stances(Arc::new(StanceSets::new(sets, rows, 1))))
}
