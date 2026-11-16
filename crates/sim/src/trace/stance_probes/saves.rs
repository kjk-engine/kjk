//! The checks of saves and recordings with stance sets. A save names the
//! sets its world played, and a load fits the save to the sets its level
//! has. A fighter in the second of two sets with equal digests keeps its
//! set, and a queued set 0 stays queued. A recording names the sets its
//! world played and refuses a level whose sets are not exactly those.

use super::*;
use crate::combat::damage::FriendlyFire;
use crate::trace::stances::{Variant, synthetic_as};
use crate::{
    InstallError, Recorder, Recording, Save, SimWorld, StepReason, TICK_HZ, TickInput, TickRate,
};

/// The duels' level with `table`'s animations.
fn level_of(table: AnimTable) -> Level {
    let arena = Level::arena();
    Level::new("m6", arena.collision().clone(), Arc::new(table))
        .with_friendly_fire(FriendlyFire::Full)
}

/// The duels' level without sets (`None`) or with the made-up sets built
/// as `variant` says.
fn level(variant: Option<Variant>) -> Result<Level, String> {
    let plain = duel_anims(&Missing::None)?;
    Ok(level_of(match variant {
        Some(variant) => synthetic_as(plain, variant)?,
        None => plain,
    }))
}

/// A save of `snapshot` in `level`, naming the sets its world plays, as the
/// game saves one.
fn save_of(snapshot: &Snapshot, level: &Level) -> Result<Save, String> {
    let world =
        SimWorld::from_snapshot(snapshot, level.clone()).map_err(|error| error.to_string())?;
    Ok(Save {
        hz: TICK_HZ,
        level: level.id(),
        snapshot: snapshot.clone(),
        script: world.script_state().clone(),
        stances: world.stance_ids(),
    })
}

/// What a load of a save in a level gives: the names of the sets gone, the
/// save as fitted, and the world made from it.
struct Loaded {
    gone: Vec<String>,
    fitted: Save,
    world: SimWorld,
}

/// `save` loaded in `level` as the game loads it, made a world for the
/// duel's two seats (`Save::world`, which fits the save to the level's
/// sets). The names of the sets gone are the world's. The save as fitted
/// comes from `Save::fit`, so the rows can read the snapshot the world was
/// made from.
fn load(save: &Save, level: &Level) -> Result<Loaded, String> {
    let mut fitted = save.clone();
    fitted.fit(level);
    let world = save.world(level, 2, None)?;
    Ok(Loaded {
        gone: world.stances_gone().to_vec(),
        fitted,
        world,
    })
}

/// The next tick of `world`, with no seat pressing anything.
fn next_tick(world: &mut SimWorld, after: &Snapshot) -> Snapshot {
    let tick = after.tick + 1;
    let msec = TickRate::default().msec(tick);
    crate::step(
        world,
        tick,
        &TickInput::default(),
        msec,
        StepReason::Authority,
    )
}

/// Every fighter in `snapshot`, by its entity's id.
fn fighters(snapshot: &Snapshot) -> impl Iterator<Item = (u32, &Fighter)> {
    snapshot
        .entities
        .iter()
        .filter_map(|entity| entity.fighter.as_ref().map(|f| (entity.id.0, f)))
}

/// Row `same`: the save loaded with the sets it was made with is unchanged.
fn same(save: &Save, loaded: &mut Loaded) -> Result<Option<String>, String> {
    let unchanged = loaded.fitted.encode() == save.encode();
    Ok((!loaded.gone.is_empty() || !unchanged).then(|| {
        format!(
            "Loaded with the same sets, the save lost the sets {:?}, and it is unchanged: {unchanged}.",
            loaded.gone
        )
    }))
}

/// Row `gone`: the save loaded in a level without sets leaves every fighter
/// in no set. No part it keeps plays a set's sequence, and no part it keeps
/// names a lent frame in its playback or its blend. The next tick sets every
/// dropped part afresh, with no blend and no lent frame.
fn gone(_: &Save, loaded: &mut Loaded) -> Result<Option<String>, String> {
    let fitted = &loaded.fitted.snapshot;
    let mut wrong = Vec::new();
    let mut dropped = Vec::new();
    for (id, f) in fighters(fitted) {
        let shown = [f.pose.legs, f.pose.torso];
        let played = shown
            .iter()
            .flatten()
            .any(|part| part.plays != Played::OWN || part.reads_lent(OWN_FRAMES));
        if f.stance_set != 0 || played {
            wrong.push(format!(
                "After the fit, entity {id} has set {} and its parts show {:?}.",
                f.stance_set,
                shown.map(|part| part.map(|part| (part.plays, part.playback)))
            ));
        }
        dropped.extend(
            (0..2)
                .filter(|&part| shown[part].is_none())
                .map(|part| (id, part)),
        );
    }
    if dropped.is_empty() {
        wrong.push("The fit dropped no part.".into());
    }
    let next = next_tick(&mut loaded.world, fitted);
    for (id, f) in fighters(&next) {
        let shown = [f.pose.legs, f.pose.torso];
        for (part, at) in shown.iter().enumerate() {
            let fresh =
                !dropped.contains(&(id, part)) || at.is_some_and(|at| at.playback.blend.is_none());
            if !fresh || at.is_some_and(|at| at.reads_lent(OWN_FRAMES)) {
                wrong.push(format!(
                    "On the next tick, part {part} of entity {id} shows {:?}.",
                    at.map(|at| at.playback)
                ));
            }
        }
    }
    Ok((!wrong.is_empty()).then(|| wrong.join(" ")))
}

/// Row `moved`: where set a is number 2, seat 0 has set 2 and its pose was
/// dropped.
fn moved(_: &Save, loaded: &mut Loaded) -> Result<Option<String>, String> {
    let f = fighter(&loaded.fitted.snapshot, 0)?;
    let dropped = f.pose.legs.is_none() && f.pose.torso.is_none();
    Ok((f.stance_set != 2 || !dropped || !loaded.gone.is_empty()).then(|| {
        format!(
            "Where set a is number 2, seat 0 has set {}, its pose was dropped: {dropped}, and the sets {:?} are gone.",
            f.stance_set, loaded.gone
        )
    }))
}

/// Row `changed`: where set a has another digest, the set is gone. The fit
/// leaves seat 0 in set 0, and the world then gives it its table row's set,
/// which is set 1 for a player's character.
fn changed(_: &Save, loaded: &mut Loaded) -> Result<Option<String>, String> {
    let fitted = fighter(&loaded.fitted.snapshot, 0)?.stance_set;
    let set = fighter(&loaded.world.snapshot(), 0)?.stance_set;
    let named = loaded.gone.iter().any(|name| name == "synthetic_a");
    Ok((fitted != 0 || set != 1 || !named).then(|| {
        format!(
            "Where set a has another digest, the sets {:?} are gone. The fit leaves seat 0 in set {fitted}, and the world gives it set {set}.",
            loaded.gone
        )
    }))
}

fn save_refit() -> Result<String, String> {
    // Seat 0 starts in set 2 and picks set 1 once its blade is lit.
    let then = format!("{LIGHT}\npick 0 0 1\ntick 30\ntick 20 h0=block");
    let ticks = play(&duel("advanced", true, "medium", 2, &then))?;
    let last = ticks.last().ok_or("The duel played no ticks.")?;
    let made = fighter(last, 0)?;
    let posed = plays(&made)?;
    if made.stance_set != 1 || !made.saber.duel.block_held || posed.contains(&Played::OWN) {
        return Err(format!(
            "Before the save, seat 0 has set {}, holds its block: {}, and plays {posed:?}.",
            made.stance_set, made.saber.duel.block_held
        ));
    }
    let save = save_of(last, &level(Some(Variant::Same))?)?;
    // The last tick before the pick is taken, with seat 0 in set 2 and
    // posed, saved in a level whose two sets have equal digests.
    let in_two = ticks
        .iter()
        .rev()
        .find(|snapshot| {
            fighter(snapshot, 0).is_ok_and(|f| f.stance_set == 2 && f.pose.torso.is_some())
        })
        .ok_or("Seat 0 was never posed in set 2.")?;
    let twins = save_of(in_two, &level(Some(Variant::Twins))?)?;
    // The same save with set 0 queued for seat 0, as a pick of set 0 queues
    // it for a character whose table row gives no set.
    let mut queued_none = save.clone();
    queued_none
        .snapshot
        .entities
        .iter_mut()
        .find(|entity| entity.kind == Kind::Avatar(Seat(0)))
        .and_then(|entity| entity.fighter.as_mut())
        .ok_or("Seat 0 has no fighter.")?
        .stance_queued = Some(0);
    type Check = fn(&Save, &mut Loaded) -> Result<Option<String>, String>;
    let rows: [(&str, &Save, Option<Variant>, Check); 6] = [
        ("same", &save, Some(Variant::Same), same),
        ("gone", &save, None, gone),
        ("moved", &save, Some(Variant::Swapped), moved),
        ("changed", &save, Some(Variant::Nudged), changed),
        ("twins", &twins, Some(Variant::Twins), same),
        ("queued none", &queued_none, Some(Variant::Same), same),
    ];
    let mut wrong = Vec::new();
    for (row, save, variant, check) in rows {
        let mut loaded = load(save, &level(variant)?)?;
        if let Some(why) = check(save, &mut loaded)? {
            wrong.push(format!("Row {row}: {why}"));
        }
    }
    none_wrong(
        wrong,
        format!(
            "All {} loads fitted their saves as expected. Four load a save made under MDA with set 1 and a held block. One loads a save of a fighter in the second of two sets with equal digests, and one a save with set 0 queued, each with the sets it was made with.",
            rows.len()
        ),
    )
}

fn ja_save_names_none() -> Result<String, String> {
    let then = format!("{LIGHT}\ntick 20 h0=block");
    let ticks = play(&duel("ja", true, "medium", 0, &then))?;
    let last = ticks.last().ok_or("The duel played no ticks.")?;
    let save = save_of(last, &level(Some(Variant::Same))?)?;
    let mut plain = load(&save, &level(None)?)?;
    let mut sets = load(&save, &level(Some(Variant::Same))?)?;
    let unchanged = plain.fitted.encode() == save.encode();
    let equal = plain.world.snapshot().encode() == sets.world.snapshot().encode();
    check(
        save.stances.is_empty() && plain.gone.is_empty() && unchanged && equal,
        format!(
            "A save made under JA's rules names the sets {:?}. Loaded without sets, it lost the sets {:?} and is unchanged: {unchanged}. The world it gives without sets is byte for byte the world it gives with them: {equal}.",
            save.stances, plain.gone
        ),
    )
}

/// A recording of 40 ticks under `rules`, from a duel whose blades are lit,
/// naming the sets its world plays. With `sets` the level has the made-up
/// sets, and without it the level has none.
fn recording(rules: &str, sets: bool) -> Result<Recording, String> {
    let set = u8::from(sets);
    let ticks = play(&duel(
        rules,
        sets,
        "medium",
        set,
        &format!("{LIGHT}\ntick 5"),
    ))?;
    let start = ticks.last().ok_or("The duel played no ticks.")?;
    let made_in = level(sets.then_some(Variant::Same))?;
    let mut world =
        SimWorld::from_snapshot(start, made_in.clone()).map_err(|error| error.to_string())?;
    let mut recorder = Recorder::new(TICK_HZ, start.clone(), made_in.id(), 10, world.stance_ids());
    let mut last = start.clone();
    for _ in 0..40 {
        last = next_tick(&mut world, &last);
        recorder.record(
            TickRate::default().msec(last.tick),
            &TickInput::default(),
            &last,
        );
    }
    Ok(recorder.finish(&last))
}

fn recording_refuses() -> Result<String, String> {
    let (with, without) = (level(Some(Variant::Same))?, level(None)?);
    let mda = recording("advanced", true)?;
    let lacks = InstallError::StanceSets {
        missing: vec!["synthetic_a".into(), "synthetic_b".into()],
    }
    .to_string();
    let refusal = |recording: &Recording, level: &Level| {
        recording
            .play(level)
            .err()
            .and_then(|divergence| divergence.reason)
    };
    let refused = refusal(&mda, &without);
    // A recording made under MDA in a level without sets names none, and a
    // level with sets refuses it with its own text.
    let unset = recording("advanced", false)?;
    let needs_none = InstallError::NoStanceSets.to_string();
    let unset_refused = refusal(&unset, &with);
    let ends = |recording: &Recording, level: &Level| {
        recording
            .play(level)
            .is_ok_and(|last| last.hash() == recording.end)
    };
    let replayed = ends(&mda, &with);
    let unset_plays = ends(&unset, &without);
    let ja = recording("ja", true)?;
    let ja_plays = ja.stances == Some(Vec::new()) && ends(&ja, &with) && ends(&ja, &without);
    // A recording read from a flight recorder's segment does not know its
    // sets. Its replay with the data check on does not check them.
    let unknown = Recording {
        stances: None,
        ..mda.clone()
    };
    let unknown_plays = ends(&unknown, &with);
    let names: Vec<_> = mda.stances.iter().flatten().map(|(name, _)| name).collect();
    check(
        refused.as_deref() == Some(lacks.as_str())
            && replayed
            && ja_plays
            && unknown_plays
            && unset_refused.as_deref() == Some(needs_none.as_str())
            && unset_plays,
        format!(
            "A recording made under MDA names the sets {names:?}. In a level without sets it was refused with {refused:?}. In a level with them it replayed to its end hash: {replayed}. A recording made under JA's rules names no set and replays to its end hash with and without sets: {ja_plays}. The recording made under MDA, with its sets not known, replays to its end hash in a level with sets: {unknown_plays}. A recording made under MDA in a level without sets was refused in a level with sets with {unset_refused:?}. In a level without sets it replayed to its end hash: {unset_plays}."
        ),
    )
}

/// The checks of saves and recordings, each by name.
pub(super) fn checks() -> Checks {
    vec![
        ("save_refit", save_refit()),
        ("ja_save_names_none", ja_save_names_none()),
        ("recording_refuses", recording_refuses()),
    ]
}
