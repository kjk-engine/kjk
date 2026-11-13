//! The stance sets' probes (`kjk sim probes --only "stance sets"`), on the
//! made-up sets of `stances::synthetic`. The sets leave every digest a
//! level had without them. Their frames are lent after the skeleton's own,
//! and a frame past the lent ones reads frame 0. `resolve` gives each part
//! what the view's duel stances give it today. The table's rows pick a set
//! by tag, by the player, and by default. Two sets with different frames
//! have different digests.
//!
//! The duels' checks play two fighters with the sets. Under JA's rules a
//! set changes nothing but the fighter's set number. Under MDA a lit blade
//! stands in its set's stance, a swing plays JA's own sequences, and a
//! held block plays what `resolve` gives it. A change of sequence keeps
//! the moment it started and blends in over 150 ms. The blade follows the
//! set's frames, and a fighter in a scene keeps its stance.
//!
//! The checks in `picks` give a fighter its set at spawn, take and refuse a
//! seat's picks, and switch the session's rules. The checks in `saves`
//! load saves and replay recordings in levels with other sets.

mod picks;
mod saves;

use std::sync::Arc;

use super::duels::{Missing, duel_anims, duel_skeleton, play};
use super::stances::{self, synthetic};
use crate::combat::Fighter;
use crate::combat::saber::blades::{self, Shown};
use crate::combat::saber::moves::Move;
use crate::combat::saber::{Kit, clash};
use crate::math::{self, Vec3};
use crate::motion::{Anim, AnimTable};
use crate::probe::{Checks, check};
use crate::skeleton::stance::{self, PERFECT_AT, SEQUENCES};
use crate::skeleton::{
    self, Bolt, Guard, Hand, Met, Parry, Playback, Played, Pose, Quadrant, Sequence, SkeletonData,
    StanceSets, Step, Wield,
};
use crate::snapshot::{Kind, Snapshot};
use crate::{Level, Seat};

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

/// A lit blade under a held block, `aimed` or not, whose holder moves by
/// `forward` and `right`.
fn guard(aimed: bool, forward: i8, right: i8) -> Wield {
    Wield {
        lit: true,
        guard: Some(Guard::new(aimed, forward, right)),
        parry: None,
    }
}

/// A lit blade without a held block.
const LIT: Wield = Wield {
    lit: true,
    guard: None,
    parry: None,
};

/// A lit blade whose torso plays the perfect parry for a blow `met` with
/// `blades` (a creature's `good` parry or not), with the block held
/// standing still or not.
fn perfect(met: Met, blades: usize, good: bool, held: bool) -> Wield {
    Wield {
        parry: Some(Parry {
            index: met.perfect(blades, good),
            at: 0,
        }),
        ..if held { guard(false, 0, 0) } else { LIT }
    }
}

/// The place in a set of the perfect parry for a blow `met` with `blades`.
fn perfect_place(met: Met, blades: usize, good: bool) -> usize {
    PERFECT_AT + usize::from(met.perfect(blades, good))
}

/// The number of the place `place` of set `set`.
fn in_set(set: u16, place: usize) -> Played {
    Played(16 + (set - 1) * SEQUENCES as u16 + place as u16)
}

/// The place of the standing guard while moving for `blades` (0 for one
/// blade, 1 for two, 2 for a staff) and `step`.
fn moving(blades: usize, step: Step) -> usize {
    stance::MOVING_AT + blades * stance::STEPS.len() + step as usize
}

/// The place of the parry for `blades` toward `quadrant`.
fn parry(blades: usize, quadrant: Quadrant) -> usize {
    stance::PARRIES_AT + blades * stance::QUADRANTS.len() + quadrant as usize
}

/// The place of the aimed block's pose for a holder of `blades` that stands
/// still (`stance::AIMED_STILL`).
fn aimed_still(blades: usize) -> usize {
    PERFECT_AT + stance::AIMED_STILL * stance::STANDING.len() + blades
}

/// The number of JA's own parry for `blades` toward `quadrant`.
fn ja_parry(blades: usize, quadrant: Quadrant) -> Played {
    Played(1 + (blades * stance::QUADRANTS.len() + quadrant as usize) as u16)
}

fn resolve_table() -> Result<String, String> {
    use Anim::*;
    let (_, table) = tables()?;
    let sets = sets_of(&table)?;
    let own = Played::OWN;
    let standing = [BOTH_STAND2, BOTH_STAND2];
    let staff = [BOTH_SABERSTAFF_STANCE, BOTH_SABERSTAFF_STANCE];
    let unlit = Wield::default();
    let still = guard(false, 0, 0);
    let aimed = guard(true, 0, 0);
    let right = perfect_place(Met::Right, 0, false);
    type Row = (u8, Wield, [Anim; 2], [Played; 2]);
    let rows: [Row; 34] = [
        (1, unlit, standing, [own, own]),
        (1, LIT, standing, [in_set(1, 0); 2]),
        (2, LIT, standing, [in_set(2, 0); 2]),
        (1, LIT, [BOTH_WALK2, BOTH_A2_T__B_], [own, own]),
        (1, still, standing, [in_set(1, 10); 2]),
        (2, still, standing, [in_set(2, parry(0, Quadrant::Top)); 2]),
        (1, aimed, standing, [in_set(1, aimed_still(0)); 2]),
        (
            1,
            guard(false, 1, -1),
            [BOTH_WALK2, BOTH_WALK2],
            [in_set(1, 7), in_set(1, moving(0, Step::Left))],
        ),
        (
            1,
            guard(false, 1, 0),
            [BOTH_WALK2, BOTH_WALK2],
            [in_set(1, 7), in_set(1, moving(0, Step::Forward))],
        ),
        (
            1,
            guard(false, -1, 0),
            [BOTH_WALKBACK_DUAL, BOTH_WALKBACK_DUAL],
            [own, in_set(1, moving(1, Step::Back))],
        ),
        (
            1,
            guard(false, -1, 1),
            [BOTH_WALKBACK_STAFF, BOTH_WALKBACK_STAFF],
            [own, in_set(1, moving(2, Step::Right))],
        ),
        (
            2,
            guard(false, 1, -1),
            [BOTH_WALK2, BOTH_WALK2],
            [in_set(2, 7), in_set(2, parry(0, Quadrant::BottomLeft))],
        ),
        (
            1,
            guard(true, 1, -1),
            [BOTH_WALK2, BOTH_WALK2],
            [in_set(1, 7), in_set(1, parry(0, Quadrant::BottomLeft))],
        ),
        (1, aimed, staff, [in_set(1, aimed_still(2)); 2]),
        (
            1,
            guard(true, 1, 0),
            [BOTH_WALK_STAFF, BOTH_WALK_STAFF],
            [in_set(1, 9), in_set(1, parry(2, Quadrant::Top))],
        ),
        (2, aimed, staff, [in_set(2, aimed_still(2)); 2]),
        (0, LIT, standing, [own, own]),
        (0, still, standing, [ja_parry(0, Quadrant::Top); 2]),
        (
            0,
            aimed,
            [BOTH_SABERDUAL_STANCE, BOTH_SABERDUAL_STANCE],
            [ja_parry(1, Quadrant::Top); 2],
        ),
        (0, aimed, staff, [ja_parry(2, Quadrant::Top); 2]),
        (
            0,
            guard(false, 0, 1),
            [BOTH_WALK_STAFF, BOTH_WALK_STAFF],
            [own, ja_parry(2, Quadrant::TopRight)],
        ),
        (1, LIT, [BOTH_RUN2, BOTH_RUN2], [own, own]),
        (
            1,
            guard(false, 1, 0),
            [BOTH_RUN2, BOTH_RUN2],
            [own, in_set(1, moving(0, Step::Forward))],
        ),
        (
            1,
            guard(false, 1, 1),
            [BOTH_RUN_DUAL, BOTH_RUN_DUAL],
            [own, in_set(1, moving(1, Step::Right))],
        ),
        (
            1,
            guard(false, -1, 0),
            [BOTH_RUNBACK2, BOTH_RUNBACK2],
            [own, in_set(1, moving(0, Step::Back))],
        ),
        (
            1,
            perfect(Met::Right, 0, false, true),
            [BOTH_RUN_STAFF, BOTH_RUN_STAFF],
            [own, in_set(1, right)],
        ),
        (
            1,
            perfect(Met::Left, 0, false, false),
            standing,
            [in_set(1, 0), in_set(1, perfect_place(Met::Left, 0, false))],
        ),
        (
            1,
            perfect(Met::Right, 0, false, true),
            standing,
            [in_set(1, 10), in_set(1, right)],
        ),
        (
            1,
            perfect(Met::Right, 0, false, true),
            [BOTH_K1_S1_TR; 2],
            [in_set(1, 10), in_set(1, right)],
        ),
        (
            1,
            perfect(Met::LowLeft, 1, false, false),
            [BOTH_WALK_DUAL; 2],
            [
                in_set(1, 8),
                in_set(1, perfect_place(Met::LowLeft, 1, false)),
            ],
        ),
        (
            2,
            perfect(Met::Behind, 2, false, true),
            staff,
            [
                in_set(2, parry(2, Quadrant::Top)),
                in_set(2, perfect_place(Met::Behind, 2, false)),
            ],
        ),
        (
            2,
            perfect(Met::Centre, 2, true, false),
            staff,
            [in_set(2, 6), in_set(2, perfect_place(Met::Centre, 2, true))],
        ),
        (1, LIT, [BOTH_K1_S1_TL; 2], [own, own]),
        (
            0,
            perfect(Met::Left, 0, false, true),
            standing,
            [ja_parry(0, Quadrant::Top); 2],
        ),
    ];
    let played = |found: [Option<(Played, Sequence)>; 2]| {
        found.map(|part| part.map_or(own, |(played, _)| played))
    };
    let mut wrong = Vec::new();
    for (index, (set, wield, anims, expected)) in rows.into_iter().enumerate() {
        let found = played(sets.resolve(set, wield, anims, &table));
        let plain = (set == 0).then(|| played(stance::resolve_plain(wield, anims, &table)));
        if found != expected || plain.is_some_and(|plain| plain != expected) {
            let names = found.map(|part| sets.name_of(part));
            wrong.push(format!(
                "Row {index} gave {found:?} ({names:?}) and without sets {plain:?}, where {expected:?} was expected."
            ));
        }
    }
    none_wrong(
        wrong,
        format!("All {} rows resolved as expected.", rows.len()),
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
    none_wrong(
        found,
        format!("All {} characters got the set they should.", cases.len()),
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

/// The number of frames the duels' skeleton has of its own. A frame at or
/// past it is lent by a set.
const OWN_FRAMES: i32 = 64;

/// The ticks that light both blades and wait until both fighters stand
/// ready. The made-up draw takes about 170 ticks.
const LIGHT: &str = "tick 1 b0=attack b1=attack\ntick 200";

/// A duel under `rules` of seat 0's fighter `kind` in stance set `set`
/// against seat 1, a medium fighter in no set, 45 units apart and facing
/// each other. `sets` gives the level the made-up sets. `then` follows
/// `begin`.
fn duel(rules: &str, sets: bool, kind: &str, set: u8, then: &str) -> String {
    let stances = if sets { "stances synthetic\n" } else { "" };
    format!(
        "scenario probe\nrules {rules}\nseed 7\n{stances}\
         fighter 0 {kind} x=0 defence=2 tiers=2 fp=100 hp=400\n\
         fighter 1 medium x=45 yaw=180 defence=2 tiers=2 fp=100 hp=400\n\
         stance 0 {set}\nstance 1 0\nbegin\n{then}\n"
    )
}

/// Seat `n`'s fighter in `snapshot`.
fn fighter(snapshot: &Snapshot, n: u8) -> Result<Fighter, String> {
    snapshot
        .own(Seat(n))
        .and_then(|entity| entity.fighter)
        .ok_or_else(|| format!("Seat {n} has no fighter at tick {}.", snapshot.tick))
}

/// The two parts a fighter's pose shows, legs first.
fn parts(f: &Fighter) -> Result<[Shown; 2], String> {
    match (f.pose.legs, f.pose.torso) {
        (Some(legs), Some(torso)) => Ok([legs, torso]),
        _ => Err("The fighter's pose has no parts.".into()),
    }
}

/// What each part of a fighter's pose plays, legs first.
fn plays(f: &Fighter) -> Result<[Played; 2], String> {
    Ok(parts(f)?.map(|part| part.plays))
}

/// What `resolve` gives a fighter's parts for the state the pose step
/// reads: its set, its blade, its held block and the animations it shows.
fn resolved(table: &AnimTable, f: &Fighter) -> Result<[Played; 2], String> {
    let duel = &f.saber.duel;
    let command = f.last_command;
    let wield = Wield {
        lit: f.saber.on,
        guard: duel
            .block_held
            .then(|| Guard::new(duel.aimed, command.forward, command.right)),
        parry: None,
    };
    let anims = parts(f)?.map(|part| part.anim);
    let found = sets_of(table)?.resolve(f.stance_set, wield, anims, table);
    Ok(found.map(|part| part.map_or(Played::OWN, |(played, _)| played)))
}

/// Whether `anim` is a swing's attack, start or return in any style.
fn swinging(anim: Anim) -> bool {
    Move::ALL
        .iter()
        .any(|m| (m.attack() || m.start() || m.ret()) && clash::attack_anim(anim, m.data().anim))
}

/// `found` as a check, with `ok` said when nothing was found wrong.
fn none_wrong(found: Vec<String>, ok: String) -> Result<String, String> {
    check(
        found.is_empty(),
        if found.is_empty() {
            ok
        } else {
            found.join(" ")
        },
    )
}

fn ja_rules_ignore_sets() -> Result<String, String> {
    let then = format!("{LIGHT}\ntick 400 b0=attack h0=block h1=kick b1=attack");
    let run = |sets: bool, set: u8| play(&duel("ja", sets, "medium", set, &then));
    let (plain, sets, set_one) = (run(false, 0)?, run(true, 0)?, run(true, 1)?);
    let mut wrong = Vec::new();
    if plain.len() != sets.len() || plain.len() != set_one.len() {
        wrong.push("The three runs played different numbers of ticks.".to_string());
    }
    for ((a, b), c) in plain.iter().zip(&sets).zip(&set_one) {
        let tick = a.tick;
        if b.encode() != a.encode() {
            wrong.push(format!(
                "At tick {tick} the run with sets differs from the run without them."
            ));
        }
        let mut unset = c.clone();
        let seat = unset
            .entities
            .iter_mut()
            .find(|entity| entity.kind == Kind::Avatar(Seat(0)))
            .and_then(|entity| entity.fighter.as_mut())
            .ok_or("Seat 0 has no fighter.")?;
        seat.stance_set = 0;
        let (plain_bytes, set_bytes) = (a.encode(), c.encode());
        let differing = plain_bytes
            .iter()
            .zip(&set_bytes)
            .filter(|(x, y)| x != y)
            .count();
        if unset.encode() != plain_bytes || differing != 1 {
            wrong.push(format!(
                "At tick {tick} the run in set 1 differs in {differing} bytes, or in more than seat 0's set."
            ));
        }
        for snapshot in [a, b, c] {
            for n in 0..2 {
                if plays(&fighter(snapshot, n)?)? != [Played::OWN; 2] {
                    wrong.push(format!(
                        "At tick {tick} seat {n} plays another sequence than its own."
                    ));
                }
            }
        }
    }
    wrong.truncate(4);
    none_wrong(
        wrong,
        format!(
            "Under JA's rules all three runs agree on all {} ticks, and every part plays its own sequence.",
            plain.len()
        ),
    )
}

fn stand_plays_set() -> Result<String, String> {
    let ticks = play(&duel(
        "advanced",
        true,
        "medium",
        1,
        &format!("{LIGHT}\ntick 30"),
    ))?;
    let last = ticks.last().ok_or("The duel played no ticks.")?;
    let (set, plain) = (fighter(last, 0)?, fighter(last, 1)?);
    let frames = parts(&set)?.map(|part| part.playback.start_frame);
    let (set_plays, plain_plays) = (plays(&set)?, plays(&plain)?);
    check(
        set_plays == [Played(16); 2]
            && frames.iter().all(|&frame| frame >= OWN_FRAMES)
            && plain_plays == [Played::OWN; 2],
        format!(
            "Seat 0 in set 1 plays {set_plays:?} from frames {frames:?}. Seat 1 in no set plays {plain_plays:?}."
        ),
    )
}

/// Whether a lit blade's stance would replace `anim`. A set replaces JA's
/// stances, fidgets and walks. A held block (`block`) also guards the walks
/// back and the blade's lowering to rest and back.
fn replaced(anim: Anim, block: bool) -> bool {
    stance::SWAPPED.contains(&anim)
        || (block
            && (stance::WALKS.contains(&anim)
                || matches!(anim, Anim::BOTH_STAND1TO2 | Anim::BOTH_STAND2TO1)))
}

fn unlit_plays_own() -> Result<String, String> {
    // The fighter lights its blade and walks, and the blade is turned off
    // mid-walk. On the tick after that the torso still shows the lit walk,
    // which a set replaces. Only such ticks test that an unlit blade plays
    // its own sequences.
    let then = format!("{LIGHT}\ntick 30 f0=1 b0=walk\nsaber 0 on=0\ntick 60 f0=1 b0=walk");
    let ticks = play(&duel("advanced", true, "medium", 1, &then))?;
    let mut unlit = 0;
    let mut wrong = Vec::new();
    for snapshot in &ticks {
        let f = fighter(snapshot, 0)?;
        let block = f.saber.duel.block_held;
        if f.saber.on || !parts(&f)?.iter().any(|part| replaced(part.anim, block)) {
            continue;
        }
        unlit += 1;
        if plays(&f)? != [Played::OWN; 2] {
            wrong.push(format!(
                "At tick {} seat 0 plays {:?} over {:?} with its blade unlit.",
                snapshot.tick,
                plays(&f)?,
                parts(&f)?.map(|part| part.anim.name())
            ));
        }
    }
    wrong.truncate(4);
    if unlit == 0 {
        wrong.push(
            "Seat 0 never showed an animation a set replaces while its blade was unlit.".into(),
        );
    }
    none_wrong(
        wrong,
        format!(
            "On all {unlit} ticks where seat 0's blade was unlit over an animation a set replaces, its parts played their own sequences."
        ),
    )
}

fn swing_keeps_ja() -> Result<String, String> {
    // The walk button keeps the legs in a walk, which a set replaces.
    // Without it the legs run, and no set replaces a run.
    let then = format!("{LIGHT}\ntick 200 b0=attack+walk f0=1");
    let ticks = play(&duel("advanced", true, "medium", 1, &then))?;
    let mut swings = 0;
    let mut walking = 0;
    let mut wrong = Vec::new();
    for snapshot in &ticks {
        let f = fighter(snapshot, 0)?;
        let shown = parts(&f)?;
        if !swinging(shown[1].anim) {
            continue;
        }
        swings += 1;
        if stance::WALKS.contains(&shown[0].anim) {
            walking += 1;
        }
        let lent = shown.iter().any(|part| {
            part.playback.start_frame >= OWN_FRAMES || part.playback.end_frame > OWN_FRAMES
        });
        if plays(&f)? != [Played::OWN; 2] || lent {
            wrong.push(format!(
                "At tick {} the torso swings in {} and the parts play {:?} from frames {:?}.",
                snapshot.tick,
                shown[1].anim.name(),
                plays(&f)?,
                shown.map(|part| part.playback.start_frame)
            ));
        }
    }
    if swings == 0 {
        wrong.push("Seat 0 never swung.".into());
    }
    if walking == 0 {
        wrong.push("Seat 0's legs never walked while its torso swung.".into());
    }
    wrong.truncate(4);
    none_wrong(
        wrong,
        format!(
            "On all {swings} ticks of a swing both parts played their own sequences from the skeleton's own frames. The legs walked on {walking} of them."
        ),
    )
}

fn held_block() -> Result<String, String> {
    let (_, table) = tables()?;
    let ways = [
        ("standing", ""),
        ("aimed", " b0=attack"),
        ("forward", " f0=1 b0=walk"),
        ("forward and left", " f0=1 r0=-1 b0=walk"),
        ("back and right", " f0=-1 r0=1 b0=walk"),
        ("right", " r0=1 b0=walk"),
    ];
    let kinds = ["medium", "staff", "dual"];
    // A few rows' codes, said outright: the parts' (legs, torso) for a
    // set, a fighter kind and a way. None leaves a part to `resolve`.
    type Literal = (u8, &'static str, &'static str, [Option<Played>; 2]);
    let literals: [Literal; 8] = [
        (1, "medium", "standing", [None, Some(in_set(1, 10))]),
        (
            2,
            "medium",
            "standing",
            [None, Some(in_set(2, parry(0, Quadrant::Top)))],
        ),
        (
            0,
            "medium",
            "standing",
            [None, Some(ja_parry(0, Quadrant::Top))],
        ),
        (
            1,
            "medium",
            "forward",
            [
                Some(in_set(1, 7)),
                Some(in_set(1, moving(0, Step::Forward))),
            ],
        ),
        (
            1,
            "medium",
            "forward and left",
            [None, Some(in_set(1, moving(0, Step::Left)))],
        ),
        (1, "staff", "aimed", [None, Some(in_set(1, aimed_still(2)))]),
        (
            1,
            "dual",
            "right",
            [None, Some(in_set(1, moving(1, Step::Right)))],
        ),
        (
            2,
            "dual",
            "right",
            [None, Some(in_set(2, parry(1, Quadrant::TopRight)))],
        ),
    ];
    let mut wrong = Vec::new();
    let mut rows = 0;
    for set in [1, 2, 0] {
        for kind in kinds {
            for (way, keys) in ways {
                rows += 1;
                let then = format!("{LIGHT}\ntick 40 h0=block{keys}");
                let ticks = play(&duel("advanced", true, kind, set, &then))?;
                let f = fighter(ticks.last().ok_or("The duel played no ticks.")?, 0)?;
                let (found, expected) = (plays(&f)?, resolved(&table, &f)?);
                let legs = parts(&f)?[0].anim;
                let literal = literals
                    .iter()
                    .find(|row| (row.0, row.1, row.2) == (set, kind, way))
                    .map_or([None; 2], |row| row.3);
                let said = (0..2).all(|part| literal[part].is_none_or(|code| code == found[part]));
                let walking = stance::WALKS.contains(&legs);
                // Walking legs are not given the guard's pose.
                let posed = walking && found[0] != Played::OWN && found[0] == found[1];
                let moving = !keys.contains("f0") || walking;
                if found != expected || !said || posed || !moving || !f.saber.duel.block_held {
                    wrong.push(format!(
                        "Set {set}, {kind}, {way}: the parts play {found:?} with the legs in {}, where resolve gives {expected:?} and the row says {literal:?}.",
                        legs.name()
                    ));
                }
            }
        }
    }
    none_wrong(
        wrong,
        format!("All {rows} held blocks played what resolve gives them."),
    )
}

/// Set 1's sequence for a lit blade's medium stance (`BOTH_STAND2`).
fn set_one_stance(table: &AnimTable) -> Result<Sequence, String> {
    sets_of(table)?
        .set(1)
        .and_then(|set| set.sequences[0])
        .ok_or_else(|| "Set 1 has no standing sequence.".into())
}

fn change_keeps_moment() -> Result<String, String> {
    let (_, table) = tables()?;
    let stand = set_one_stance(&table)?;
    let then = format!("{LIGHT}\ntick 20\ntick 20 h0=block");
    let ticks = play(&duel("advanced", true, "medium", 1, &then))?;
    let torsos = ticks
        .iter()
        .map(|snapshot| Ok(parts(&fighter(snapshot, 0)?)?[1]))
        .collect::<Result<Vec<Shown>, String>>()?;
    let at = (1..torsos.len())
        .find(|&i| torsos[i - 1].plays == in_set(1, 0) && torsos[i].plays != in_set(1, 0))
        .ok_or("The torso never changed from set 1's stance.")?;
    let (before, after) = (torsos[at - 1], torsos[at]);
    let blend = after.playback.blend;
    let range = stand.first_frame as f32..=(stand.first_frame + stand.frames) as f32;
    check(
        after.anim == before.anim
            && after.playback.start_ms == before.playback.start_ms
            && blend.is_some_and(|b| b.length_ms == 150 && range.contains(&b.frame)),
        format!(
            "The torso went from {:?} in {} to {:?} in {}. It started at {} ms and then at {} ms. Its blend is {blend:?}, and the stance's frames are {range:?}.",
            before.plays,
            before.anim.name(),
            after.plays,
            after.anim.name(),
            before.playback.start_ms,
            after.playback.start_ms
        ),
    )
}

/// Seat 0's blade tip in `snapshot`, placed as the sim's place step places
/// it from the fighter's pose. With `own`, both parts' playbacks are first
/// made afresh from that sequence, from the same start and at the same
/// speed, with no blend.
fn tip(table: &AnimTable, snapshot: &Snapshot, own: Option<Sequence>) -> Result<Vec3, String> {
    let data = skeleton_of(table)?;
    let entity = snapshot.own(Seat(0)).ok_or("Seat 0 is missing.")?;
    let (f, ps, sheet) = (
        entity.fighter.ok_or("Seat 0 has no fighter.")?,
        entity.player.ok_or("Seat 0 has no player.")?,
        entity.sheet.ok_or("Seat 0 has no sheet.")?,
    );
    let now = snapshot.time_ms;
    let slot =
        blades::slots(&Kit::of(&sheet), &f.saber, f.hand)[0].ok_or("Seat 0 holds no blade.")?;
    let (mut pose, legs_yaw) = f
        .pose
        .pose(data, &ps, (&f.saber, true), now)
        .ok_or("Seat 0 has no pose.")?;
    if let Some(sequence) = own {
        for part in [&mut pose.legs, &mut pose.torso] {
            *part = Playback {
                start_frame: sequence.first_frame,
                end_frame: sequence.first_frame + sequence.frames,
                looping: sequence.looping,
                blend: None,
                ..*part
            };
        }
    }
    let placement = blades::placement(&ps, 0, legs_yaw);
    blades::place(data, &pose, now, &placement, &slot)
        .map(|blade| blade.tip)
        .ok_or_else(|| "Seat 0's blade cannot be placed.".into())
}

fn blade_follows() -> Result<String, String> {
    let (_, table) = tables()?;
    let stand = set_one_stance(&table)?;
    let then = format!("{LIGHT}\ntick 30");
    let last = |set: u8| -> Result<Snapshot, String> {
        play(&duel("advanced", true, "medium", set, &then))?
            .pop()
            .ok_or_else(|| "The duel played no ticks.".into())
    };
    let (in_set, plain) = (last(1)?, last(0)?);
    let (set, framed, own) = (
        tip(&table, &in_set, None)?,
        tip(&table, &in_set, Some(stand))?,
        tip(&table, &plain, None)?,
    );
    let apart = math::length(math::sub(set, own));
    let off = math::length(math::sub(set, framed));
    check(
        apart > 1.0 && off < 0.01,
        format!(
            "The tip in set 1 is {apart:.2} units from the tip in no set, and {off:.4} units from the tip placed from the set's own frames."
        ),
    )
}

fn scene_keeps_stance() -> Result<String, String> {
    let then = format!("{LIGHT}\ntarget 0 1\ntalk party\ntick 30");
    let ticks = play(&duel("advanced", true, "medium", 1, &then))?;
    let scene = &ticks[ticks.len().saturating_sub(30)..];
    let mut wrong = Vec::new();
    for snapshot in scene {
        let found = plays(&fighter(snapshot, 0)?)?;
        let in_one = found
            .iter()
            .all(|played| (16..16 + SEQUENCES as u16).contains(&played.0));
        if !snapshot.holds_world() || !in_one {
            wrong.push(format!(
                "At tick {} the conversation holds the world: {}. Seat 0 plays {found:?}.",
                snapshot.tick,
                snapshot.holds_world()
            ));
        }
    }
    wrong.truncate(4);
    none_wrong(
        wrong,
        format!(
            "On all {} ticks of a scene both parts played set 1's sequences.",
            scene.len()
        ),
    )
}

fn parry_directions() -> Result<String, String> {
    use crate::combat::saber::duel::parry_met;
    const EYE: f32 = 50.0;
    // A blocker at the origin facing +x, its right toward -y. Each row is
    // where the blades met (x, y, height against the eyes), whether the
    // blocker guards behind, and where the parry goes.
    let rows: [([f32; 3], bool, Met); 11] = [
        ([20.0, -20.0, 10.0], true, Met::Right),
        ([20.0, 20.0, 10.0], true, Met::Left),
        ([20.0, 2.0, 10.0], true, Met::Centre),
        ([20.0, -4.0, -10.0], true, Met::Right),
        ([20.0, -1.0, -10.0], true, Met::Centre),
        ([20.0, 4.0, -10.0], true, Met::Left),
        ([20.0, 1.0, -30.0], true, Met::LowLeft),
        ([20.0, 0.0, -30.0], true, Met::LowRight),
        ([-20.0, -1.0, 0.0], true, Met::Behind),
        ([-20.0, -1.0, 0.0], false, Met::Centre),
        ([-20.0, -20.0, 0.0], false, Met::Right),
    ];
    let wrong: Vec<String> = rows
        .iter()
        .filter_map(|&([x, y, height], behind, expected)| {
            let found = parry_met([0.0; 3], EYE, 0.0, [x, y, EYE + height], behind);
            (found != expected).then(|| {
                format!("A blow met at {x}, {y}, {height} gave {found:?}, where {expected:?} was expected.")
            })
        })
        .collect();
    none_wrong(
        wrong,
        format!(
            "All {} meeting points gave the parry's direction they should.",
            rows.len()
        ),
    )
}

/// A duel in which seat 1 swings at seat 0, which holds block and taps
/// attack, so a perfect block falls inside a window. Seat 0 is a fighter
/// of `kind` in set `set`. `sets` gives the level the made-up sets.
fn perfect_duel(kind: &str, sets: bool, set: u8) -> Result<Vec<Snapshot>, String> {
    let taps = vec!["tick 15 b1=attack h0=block b0=attack\ntick 1 b1=attack h0=block"; 40];
    let then = format!(
        "{LIGHT}\nduel 0 points=50\nstat 0 fp 5000\n{}",
        taps.join("\n")
    );
    play(&duel("advanced", sets, kind, set, &then))
}

/// The first tick at which `ticks`' seat 0 blocked perfectly.
fn perfect_tick(ticks: &[Snapshot]) -> Result<usize, String> {
    let at = |i: usize| fighter(&ticks[i], 0).map(|f| f.saber.duel.perfect_at);
    (1..ticks.len())
        .find(|&i| matches!((at(i - 1), at(i)), (Ok(a), Ok(b)) if a != b))
        .ok_or_else(|| "Seat 0 never blocked perfectly.".into())
}

fn perfect_parry() -> Result<String, String> {
    let (_, table) = tables()?;
    let sets = sets_of(&table)?;
    let kinds = [("medium", 0), ("dual", 1), ("staff", 2)];
    let wait = rules::duel::perfect::PARRY_WAIT_MS;
    let mut wrong = Vec::new();
    let mut said = Vec::new();
    for (kind, blades) in kinds {
        let ticks = perfect_duel(kind, true, 1)?;
        let at = perfect_tick(&ticks)?;
        let f = fighter(&ticks[at], 0)?;
        let duel = f.saber.duel;
        let index = duel.parry.checked_sub(1).map(usize::from);
        let next = ticks
            .get(at + 1)
            .ok_or("The duel ended at the perfect block.")?;
        let torso = parts(&fighter(next, 0)?)?[1];
        let place = index.map(|index| in_set(1, PERFECT_AT + index));
        let ok = f.saber.current_move == Move::READY
            && (wait[0]..=wait[1]).contains(&f.weapon_wait_ms)
            && index.is_some_and(|index| index % 3 == blades && index / 3 < 6)
            && Some(torso.plays) == place
            && torso.playback.start_ms == duel.parry_at as i64
            && torso.playback.blend.is_some_and(|b| b.length_ms == 100);
        let named = place.and_then(|place| sets.name_of(place));
        let line = format!(
            "{kind} at tick {}: move {}, weapon timer {}, parry {:?} ({named:?}), the next tick's torso plays {:?} from {} ms with blend {:?}.",
            ticks[at].tick,
            f.saber.current_move.name(),
            f.weapon_wait_ms,
            index,
            torso.plays,
            torso.playback.start_ms,
            torso.playback.blend.map(|b| b.length_ms),
        );
        if ok {
            said.push(line);
        } else {
            wrong.push(line);
        }
    }
    for (level_sets, what) in [(false, "a level without sets"), (true, "set 0")] {
        let ticks = perfect_duel("medium", level_sets, 0)?;
        let f = fighter(&ticks[perfect_tick(&ticks)?], 0)?;
        if !f.saber.current_move.knockaway() || f.saber.duel.parry != 0 {
            wrong.push(format!(
                "In {what} seat 0 plays {} with parry {}, where JA's knockaway was expected.",
                f.saber.current_move.name(),
                f.saber.duel.parry
            ));
        }
    }
    none_wrong(
        wrong,
        format!(
            "{} A fighter in no set, or in a level without sets, plays JA's knockaway.",
            said.join(" ")
        ),
    )
}

fn stance_needs_fighter() -> Result<String, String> {
    let script = duel("advanced", true, "medium", 1, "tick 1").replace("stance 1 0", "stance 2 1");
    match play(&script) {
        Err(why) => check(
            why.contains("seat 2 has no fighter"),
            format!("A stance for seat 2, which has no fighter, is refused: {why}"),
        ),
        Ok(_) => Err("A stance for seat 2, which has no fighter, was accepted.".into()),
    }
}

/// The stance sets' checks, each by name.
pub fn probes() -> Checks {
    let mut checks = vec![
        ("digests_keep", digests_keep()),
        ("lent_after_own", lent_after_own()),
        ("past_lent_reads_first", past_lent_reads_first()),
        ("resolve_table", resolve_table()),
        ("rows", rows()),
        ("set_digests_differ", set_digests_differ()),
        ("ja_rules_ignore_sets", ja_rules_ignore_sets()),
        ("stand_plays_set", stand_plays_set()),
        ("unlit_plays_own", unlit_plays_own()),
        ("swing_keeps_ja", swing_keeps_ja()),
        ("held_block", held_block()),
        ("change_keeps_moment", change_keeps_moment()),
        ("blade_follows", blade_follows()),
        ("scene_keeps_stance", scene_keeps_stance()),
        ("stance_needs_fighter", stance_needs_fighter()),
        ("parry_directions", parry_directions()),
        ("perfect_parry", perfect_parry()),
    ];
    checks.extend(picks::checks());
    checks.extend(saves::checks());
    checks
}
