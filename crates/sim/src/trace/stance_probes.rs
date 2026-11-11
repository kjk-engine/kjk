//! The stance sets' probes (`kjk sim probes --only "stance sets"`), on the
//! made-up sets of `stances::synthetic`. The sets leave every digest a
//! level had without them. Their frames are lent after the skeleton's own,
//! and a frame past the lent ones reads frame 0. `resolve` gives each part
//! what the view's duel stances give it today. The table's rows pick a set
//! by tag, by the player, and by default. Two sets with different frames
//! have different digests.

use std::sync::Arc;

use super::duels::{Missing, duel_anims, duel_skeleton};
use super::stances::{self, synthetic};
use crate::Level;
use crate::motion::{Anim, AnimTable};
use crate::probe::{Checks, check};
use crate::skeleton::stance::{self, SEQUENCES};
use crate::skeleton::{
    self, Bolt, Guard, Hand, Playback, Played, Pose, Quadrant, SkeletonData, StanceSets,
};

/// The made-up table without sets, and the same table with them.
fn tables() -> Result<(AnimTable, AnimTable), String> {
    let plain = duel_anims(&Missing::None)?;
    let sets = synthetic(plain.clone())?;
    Ok((plain, sets))
}

fn skeleton_of(table: &AnimTable) -> Result<&SkeletonData, String> {
    table
        .skeleton()
        .ok_or_else(|| "The table has no skeleton.".to_string())
}

fn sets_of(table: &AnimTable) -> Result<&StanceSets, String> {
    table
        .stances()
        .ok_or_else(|| "The table has no stance sets.".to_string())
}

/// A level on the arena's floor with `table`'s animations.
fn level_digest(table: &AnimTable) -> [u8; 32] {
    let arena = Level::arena();
    Level::new("m6", arena.collision().clone(), Arc::new(table.clone())).digest()
}

fn digests_keep() -> Result<String, String> {
    let (plain, sets) = tables()?;
    let same = [
        ("the animation table's", plain.digest() == sets.digest()),
        (
            "the skeleton's",
            skeleton_of(&plain)?.digest() == skeleton_of(&sets)?.digest(),
        ),
        ("the level's", level_digest(&plain) == level_digest(&sets)),
    ];
    let changed: Vec<&str> = same
        .iter()
        .filter(|(_, same)| !same)
        .map(|(what, _)| *what)
        .collect();
    check(
        changed.is_empty(),
        if changed.is_empty() {
            "The animation table's, the skeleton's and the level's digests are the same with the sets as without them.".into()
        } else {
            format!("The sets changed {} digest.", changed.join(" and "))
        },
    )
}

fn lent_after_own() -> Result<String, String> {
    let (_, table) = tables()?;
    let data = skeleton_of(&table)?;
    let sets = sets_of(&table)?;
    let (own, all) = (data.own_frame_count(), data.frame_count());
    let outside: Vec<String> = (1..=sets.len() as u8)
        .filter_map(|number| sets.set(number))
        .flat_map(|set| {
            set.sequences.iter().flatten().filter_map(|sequence| {
                let end = i64::from(sequence.first_frame) + i64::from(sequence.frames);
                (sequence.first_frame < own as i32 || end > i64::from(all))
                    .then(|| format!("{} at {}", set.name, sequence.first_frame))
            })
        })
        .collect();
    check(
        own == 64 && all == 160 && outside.is_empty(),
        format!(
            "The skeleton has {own} frames of its own and {all} in all. The sets' sequences outside the lent frames: {outside:?}."
        ),
    )
}

fn past_lent_reads_first() -> Result<String, String> {
    let (_, table) = tables()?;
    let data = skeleton_of(&table)?;
    let at = |frame: i32| {
        let part = Playback {
            start_frame: frame,
            end_frame: frame + 1,
            speed: 0.0,
            start_ms: 0,
            looping: false,
            blend: None,
        };
        let pose = Pose {
            legs: part,
            torso: part,
            look: None,
        };
        skeleton::bolt(data, &pose, 0, Bolt::Hand(Hand::Right))
    };
    let (past, first) = (at(5000), at(0));
    check(
        past.is_some() && past == first,
        format!("The right hand's bolt at frame 5000 is {past:?}, and at frame 0 it is {first:?}."),
    )
}

/// A held block that is not aimed, toward `way`.
fn guard(way: Option<Quadrant>) -> Option<Guard> {
    Some(Guard { aimed: false, way })
}

/// The number of the place `place` of set `set`.
fn in_set(set: u16, place: u16) -> Played {
    Played(16 + (set - 1) * SEQUENCES as u16 + place)
}

fn resolve_table() -> Result<String, String> {
    use Anim::*;
    let (_, table) = tables()?;
    let sets = sets_of(&table)?;
    let own = Played::OWN;
    let standing = [BOTH_STAND2, BOTH_STAND2];
    let aimed = Some(Guard {
        aimed: true,
        way: None,
    });
    type Row = (u8, bool, Option<Guard>, [Anim; 2], [Played; 2]);
    let rows: [Row; 13] = [
        (1, false, None, standing, [own, own]),
        (1, true, None, standing, [in_set(1, 0); 2]),
        (2, true, None, standing, [in_set(2, 0); 2]),
        (1, true, None, [BOTH_WALK2, BOTH_A2_T__B_], [own, own]),
        (1, true, guard(None), standing, [in_set(1, 10); 2]),
        (2, true, guard(None), standing, [in_set(2, 13); 2]),
        (1, true, aimed, standing, [in_set(1, 13); 2]),
        (
            1,
            true,
            guard(Some(Quadrant::BottomLeft)),
            [BOTH_WALK2, BOTH_WALK2],
            [in_set(1, 7), in_set(1, 13 + 3)],
        ),
        (0, true, None, standing, [own, own]),
        (0, true, guard(None), standing, [Played(1); 2]),
        (
            0,
            true,
            aimed,
            [BOTH_SABERDUAL_STANCE, BOTH_SABERDUAL_STANCE],
            [Played(1 + 5); 2],
        ),
        (
            0,
            true,
            guard(Some(Quadrant::TopRight)),
            [BOTH_WALK_STAFF, BOTH_WALK_STAFF],
            [own, Played(1 + 2 * 5 + 2)],
        ),
        (1, true, None, [BOTH_RUN2, BOTH_RUN2], [own, own]),
    ];
    let played = |found: [Option<(Played, crate::skeleton::Sequence)>; 2]| {
        found.map(|part| part.map_or(own, |(played, _)| played))
    };
    let mut wrong = Vec::new();
    for (index, (set, lit, guard, anims, expected)) in rows.into_iter().enumerate() {
        let found = played(sets.resolve(set, lit, guard, anims, &table));
        let plain = (set == 0).then(|| played(stance::resolve_plain(lit, guard, anims, &table)));
        if found != expected || plain.is_some_and(|plain| plain != expected) {
            let names = found.map(|part| sets.name_of(part));
            wrong.push(format!(
                "Row {index} gave {found:?} ({names:?}) and without sets {plain:?}, where {expected:?} was expected."
            ));
        }
    }
    check(
        wrong.is_empty(),
        if wrong.is_empty() {
            format!("All {} rows resolved as expected.", rows.len())
        } else {
            wrong.join(" ")
        },
    )
}

fn rows() -> Result<String, String> {
    let (_, table) = tables()?;
    let sets = sets_of(&table)?;
    let cases = [
        (("n_sith01", "g_sith", false), 2),
        (("x", "y", true), 1),
        (("x", "y", false), 1),
    ];
    let found: Vec<String> = cases
        .iter()
        .filter_map(|&((template, tag, player), expected)| {
            let set = sets.row_for(template, tag, player);
            (set != expected).then(|| {
                format!("The template {template}, the tag {tag} and player {player} gave set {set}, where set {expected} was expected.")
            })
        })
        .collect();
    check(
        found.is_empty(),
        if found.is_empty() {
            format!("All {} characters got the set they should.", cases.len())
        } else {
            found.join(" ")
        },
    )
}

fn set_digests_differ() -> Result<String, String> {
    let (plain, table) = tables()?;
    let sets = sets_of(&table)?;
    let ids = sets.ids();
    let again = synthetic(plain.clone())?;
    let again = sets_of(&again)?.ids();
    let nudged = stances::lent_skeleton(&duel_skeleton()?, true)?;
    let first = sets.set(1).ok_or("The table has no set 1.")?;
    let nudged = stance::set_digest(&first.sequences, &nudged);
    let differ = ids.len() == 2 && ids[0].1 != ids[1].1;
    let repeat = ids == again;
    let moved = nudged != first.digest;
    check(
        differ && repeat && moved,
        format!(
            "The two sets' digests differ: {differ}. A set built twice has the same digest: {repeat}. A set with one key changed has another digest: {moved}."
        ),
    )
}

/// The stance sets' checks, each by name.
pub fn probes() -> Checks {
    vec![
        ("digests_keep", digests_keep()),
        ("lent_after_own", lent_after_own()),
        ("past_lent_reads_first", past_lent_reads_first()),
        ("resolve_table", resolve_table()),
        ("rows", rows()),
        ("set_digests_differ", set_digests_differ()),
    ]
}
