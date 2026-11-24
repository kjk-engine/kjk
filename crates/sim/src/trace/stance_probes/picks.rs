//! The checks of a fighter's stance set at spawn, of a seat's picks and of
//! the session's rules switch. A fighter made under MDA takes its table
//! row's set, and under JA it takes none. A pick waits for the weapon
//! timer, as a queued style change does. A pick the rules refuse changes
//! nothing at all. The first seat may pick for another party member.
//! Switching the rules to JA leaves no fighter in a set and no part reading
//! the lent frames, in the area, departing or in a module left. Switching
//! back gives the rows' sets again. A switch to the rules a session already
//! plays by changes nothing.

use super::*;
use crate::module::ObjectKind;
use crate::skeleton::Blend;

/// A friendly creature tagged `g_sith`, out of the party, away from the
/// fighters. The made-up table gives a tag holding `sith` set 2.
const SITH: &str = "creature medium x=0 y=150 faction=friendly tag=g_sith";

/// A duel under `rules` with the made-up sets in which no stance is named:
/// two medium fighters facing each other and the creature `SITH` (`c0`).
/// `then` follows `begin`.
fn by_rows(rules: &str, then: &str) -> String {
    format!(
        "scenario probe\nrules {rules}\nseed 7\nstances synthetic\n\
         fighter 0 medium x=0 defence=2 tiers=2 fp=100 hp=400\n\
         fighter 1 medium x=45 yaw=180 defence=2 tiers=2 fp=100 hp=400\n\
         {SITH}\nbegin\n{then}\n"
    )
}

/// The creature's fighter in `snapshot`.
fn creature(snapshot: &Snapshot) -> Result<Fighter, String> {
    snapshot
        .entities
        .iter()
        .find(|entity| entity.kind == Kind::Object(ObjectKind::Creature, 0))
        .and_then(|entity| entity.fighter)
        .ok_or_else(|| format!("The creature has no fighter at tick {}.", snapshot.tick))
}

fn row_at_spawn() -> Result<String, String> {
    let mut wrong = Vec::new();
    for (rules, seats, sith) in [("advanced", 1, 2), ("ja", 0, 0)] {
        let ticks = play(&by_rows(rules, "tick 1"))?;
        let first = ticks.first().ok_or("The duel played no ticks.")?;
        let found = [
            fighter(first, 0)?.stance_set,
            fighter(first, 1)?.stance_set,
            creature(first)?.stance_set,
        ];
        if found != [seats, seats, sith] {
            wrong.push(format!(
                "Under {rules} the seats and the creature have the sets {found:?}, where [{seats}, {seats}, {sith}] was expected."
            ));
        }
    }
    none_wrong(
        wrong,
        "Under MDA both seats have set 1 and the creature tagged g_sith has set 2. Under JA all three have set 0.".into(),
    )
}

/// Whether a blend has blended in by `now`.
fn blended(blend: Option<Blend>, now: u64) -> bool {
    blend.is_none_or(|b| now as i64 >= b.start_ms + i64::from(b.length_ms))
}

fn pick_waits() -> Result<String, String> {
    // The made-up swing and its return take about 330 ticks, so the duel
    // runs 500 ticks after the pick.
    let then = format!("{LIGHT}\ntick 1 b0=attack\ntick 3\npick 0 0 2\ntick 500");
    let ticks = play(&duel("advanced", true, "medium", 1, &then))?;
    let seat = ticks
        .iter()
        .map(|snapshot| fighter(snapshot, 0))
        .collect::<Result<Vec<_>, _>>()?;
    let queued = seat
        .iter()
        .position(|f| f.stance_queued == Some(2))
        .ok_or("The pick of set 2 was never queued.")?;
    let taken = seat
        .iter()
        .position(|f| f.stance_set == 2)
        .ok_or("Set 2 was never taken.")?;
    if queued == 0 || taken <= queued {
        return Err(format!(
            "The pick was queued at tick {} and taken at tick {}.",
            ticks[queued].tick, ticks[taken].tick
        ));
    }
    // A tick's saber step begins with the weapon timer the tick before it
    // left. The pick waits while that timer runs.
    let waited = (queued..taken).all(|i| {
        seat[i].stance_set == 1
            && seat[i].stance_queued == Some(2)
            && seat[i - 1].weapon_wait_ms > 0
    });
    let ready = seat[taken - 1].weapon_wait_ms <= 0 && seat[taken].stance_queued.is_none();
    let torso = |f: &Fighter| parts(f).map(|part| part[1]);
    let stance = in_set(2, 0);
    let first = (taken..seat.len())
        .find(|&i| torso(&seat[i]).is_ok_and(|part| part.plays == stance))
        .ok_or("The torso never played set 2's stance.")?;
    let (start, end) = (torso(&seat[first])?, torso(&seat[seat.len() - 1])?);
    let blend = start.playback.blend;
    check(
        waited
            && ready
            && blend.is_some_and(|b| b.length_ms == 150)
            && end.plays == stance
            && blended(end.playback.blend, ticks[seat.len() - 1].time_ms),
        format!(
            "The pick was queued at tick {} with the weapon timer at {} ms and taken at tick {}, after the timer stood at {} ms. The torso played {stance:?} from tick {} with the blend {blend:?}, and at the end it plays {:?}.",
            ticks[queued].tick,
            seat[queued - 1].weapon_wait_ms,
            ticks[taken].tick,
            seat[taken - 1].weapon_wait_ms,
            ticks[first].tick,
            end.plays
        ),
    )
}

fn pick_refused() -> Result<String, String> {
    let rows = [
        (
            "seat 1 picks for seat 0's character",
            "advanced",
            "pick 1 0 2",
        ),
        ("seat 0 picks set 3 of 2", "advanced", "pick 0 0 3"),
        ("seat 0 picks under JA's rules", "ja", "pick 0 0 2"),
        (
            "seat 0 picks for a creature out of the party",
            "advanced",
            "pick 0 c0 1",
        ),
    ];
    // The creature's table row gives it set 2, so the pick for it asks for
    // set 1. A pick that was taken would change its set.
    let mut wrong = Vec::new();
    for (row, rules, pick) in rows {
        let run = |pick: &str| play(&by_rows(rules, &format!("tick 1\n{pick}tick 20")));
        let (with, without) = (run(&format!("{pick}\n"))?, run("")?);
        let same = with.len() == without.len()
            && with
                .iter()
                .zip(&without)
                .all(|(a, b)| a.encode() == b.encode());
        if !same {
            wrong.push(format!(
                "When {row}, the duel differs from the same duel without the pick."
            ));
        }
    }
    none_wrong(
        wrong,
        format!(
            "All {} refused picks left the duel byte for byte as it was without them.",
            rows.len()
        ),
    )
}

fn pick_for_party() -> Result<String, String> {
    // The creature tagged g_sith joins the party, led by seat 1. Seat 0 is
    // not that creature's seat, and as the first seat it may pick for any
    // party member. The creature's table row gives it set 2, so seat 0
    // picks set 1.
    let ticks = play(&by_rows(
        "advanced",
        "tick 1\nparty c0 1\npick 0 c0 1\ntick 20",
    ))?;
    let first = creature(ticks.first().ok_or("The duel played no ticks.")?)?;
    let picked = ticks
        .iter()
        .map(creature)
        .collect::<Result<Vec<_>, _>>()?
        .iter()
        .any(|f| f.stance_queued == Some(1) || f.stance_set == 1);
    check(
        first.stance_set == 2 && picked,
        format!(
            "The creature began in set {}. Seat 0's pick of set 1 for it, while seat 1 leads it, was queued or taken: {picked}.",
            first.stance_set
        ),
    )
}

fn pick_of_none() -> Result<String, String> {
    let ticks = play(&duel(
        "advanced",
        true,
        "medium",
        2,
        "tick 1\npick 0 0 0\ntick 5",
    ))?;
    let first = fighter(ticks.first().ok_or("The duel played no ticks.")?, 0)?;
    let last = fighter(ticks.last().ok_or("The duel played no ticks.")?, 0)?;
    check(
        first.stance_set == 2 && last.stance_set == 1 && last.stance_queued.is_none(),
        format!(
            "Seat 0 began in set {} and picked set 0. It ends in set {} with {:?} queued, and its table row gives set 1.",
            first.stance_set, last.stance_set, last.stance_queued
        ),
    )
}

/// Whether a part's playback or blend reads a frame the made-up sets lend.
fn reads_lent(part: &Shown) -> bool {
    part.reads_lent(OWN_FRAMES)
}

/// Whether seat 0's torso shows a set's held guard while it still blends
/// in.
fn guard_blending(snapshot: &Snapshot) -> Result<bool, String> {
    let torso = parts(&fighter(snapshot, 0)?)?[1];
    Ok(torso.plays != Played::OWN && !blended(torso.playback.blend, snapshot.time_ms))
}

/// Whether seat 0's torso plays JA's own sequence while its blend still
/// reads a set's lent frames.
fn swing_blending(snapshot: &Snapshot) -> Result<bool, String> {
    let torso = parts(&fighter(snapshot, 0)?)?[1];
    Ok(torso.plays == Played::OWN
        && reads_lent(&torso)
        && !blended(torso.playback.blend, snapshot.time_ms))
}

/// Every entity out of the area in `snapshot`: those departing, then those
/// in the modules left.
fn out_of_area(snapshot: &Snapshot) -> impl Iterator<Item = &crate::EntityState> {
    snapshot
        .departing
        .iter()
        .map(|going| &going.entity)
        .chain(snapshot.stash.iter().flat_map(|left| &left.entities))
}

/// Whether the creature out of the area is in a set.
fn away_in_set(snapshot: &Snapshot) -> Result<bool, String> {
    Ok(out_of_area(snapshot)
        .filter_map(|entity| entity.fighter)
        .any(|f| f.stance_set != 0))
}

fn rules_switch() -> Result<String, String> {
    // Each row names the moment of the switch, the duel it happens in, the
    // directives that lead up to it, and what holds on the tick before it.
    // The last two rows take the creature tagged g_sith, in set 2, out of
    // the area.
    type Row = (
        &'static str,
        fn(&str) -> String,
        &'static str,
        fn(&Snapshot) -> Result<bool, String>,
    );
    let in_set_one: fn(&str) -> String = |then| duel("advanced", true, "medium", 1, then);
    let with_creature: fn(&str) -> String = |then| by_rows("advanced", then);
    let rows: [Row; 4] = [
        (
            "a held block's blend into a set's guard",
            in_set_one,
            "tick 2 h0=block",
            guard_blending,
        ),
        (
            "the first 150 ms of a swing out of a set's stance",
            in_set_one,
            "tick 1 b0=attack\ntick 2",
            swing_blending,
        ),
        (
            "a duel with a creature in a module left",
            with_creature,
            "leave c0 module\ntick 1",
            away_in_set,
        ),
        (
            "a duel with a creature departing",
            with_creature,
            "leave c0 departing\ntick 1",
            away_in_set,
        ),
    ];
    let mut wrong = Vec::new();
    for (row, script, before, leads_up) in rows {
        let then = format!("{LIGHT}\ntick 20\n{before}\nrules ja\nrules advanced");
        let ticks = play(&script(&then))?;
        let [.., blending, ja, mda] = ticks.as_slice() else {
            return Err(format!(
                "In {row} the duel kept fewer than three snapshots."
            ));
        };
        if !leads_up(blending)? {
            let torso = parts(&fighter(blending, 0)?)?[1];
            wrong.push(format!(
                "Before the switch in {row}, seat 0's torso plays {:?} with the blend {:?}, and the creature out of the area is in a set: {}.",
                torso.plays,
                torso.playback.blend,
                away_in_set(blending)?
            ));
        }
        for entity in ja.entities.iter().chain(out_of_area(ja)) {
            let Some(f) = entity.fighter else {
                continue;
            };
            let shown = [f.pose.legs, f.pose.torso];
            let played = shown.iter().flatten().any(|part| part.plays != Played::OWN);
            let lent = shown.iter().flatten().any(reads_lent);
            if f.stance_set != 0 || f.stance_queued.is_some() || played || lent {
                wrong.push(format!(
                    "After the switch to JA in {row}, entity {} has set {} with {:?} queued, and its parts show {:?}.",
                    entity.id.0,
                    f.stance_set,
                    f.stance_queued,
                    shown.map(|part| part.map(|part| (part.plays, part.playback)))
                ));
            }
        }
        let back = [fighter(mda, 0)?.stance_set, fighter(mda, 1)?.stance_set];
        if back != [1, 1] {
            wrong.push(format!(
                "Back under MDA after {row}, the seats have the sets {back:?}, where their rows give [1, 1]."
            ));
        }
    }
    none_wrong(
        wrong,
        "A switch to JA during a held block's blend into a set's guard left every fighter in no set and every part on its own sequence and frames. A switch to JA in the first 150 ms of a swing out of a set's stance did the same. A switch to JA did the same for a creature in a module left and for a creature departing. A switch back to MDA gave both seats their rows' set 1 each time.".into(),
    )
}

/// Whether seat 0 is in set 1.
fn in_set_one(snapshot: &Snapshot) -> Result<bool, String> {
    Ok(fighter(snapshot, 0)?.stance_set == 1)
}

/// Whether seat 0's torso plays one of JA's own parries.
fn plays_ja_parry(snapshot: &Snapshot) -> Result<bool, String> {
    let torso = parts(&fighter(snapshot, 0)?)?[1];
    Ok((1..16).contains(&torso.plays.0))
}

fn same_rules_untouched() -> Result<String, String> {
    // Each row names the session, its duel, and what holds of seat 0 after
    // a switch to the rules the session already plays by. In the first row
    // the `stance` directive puts seat 0 in set 1 under JA's rules, which no
    // rule of the sim does. A switch to JA must leave even that alone. In
    // the second row seat 0 holds its block under MDA in a level without
    // sets, so its torso plays JA's own parry.
    type Row = (&'static str, String, fn(&Snapshot) -> Result<bool, String>);
    let rows: [Row; 2] = [
        (
            "under JA from its start with seat 0 in set 1",
            duel(
                "ja",
                true,
                "medium",
                1,
                &format!("{LIGHT}\nrules ja\ntick 5"),
            ),
            in_set_one,
        ),
        (
            "under MDA in a level without sets with seat 0's block held",
            duel(
                "advanced",
                false,
                "medium",
                0,
                &format!("{LIGHT}\ntick 20 h0=block\nrules advanced\ntick 5 h0=block"),
            ),
            plays_ja_parry,
        ),
    ];
    let mut wrong = Vec::new();
    for (row, script, holds) in &rows {
        let ticks = play(script)?;
        // The switch keeps one more snapshot of the tick it happens on.
        let pairs: Vec<(&Snapshot, &Snapshot)> = ticks
            .windows(2)
            .filter(|pair| pair[0].tick == pair[1].tick)
            .map(|pair| (&pair[0], &pair[1]))
            .collect();
        let [(before, after)] = pairs.as_slice() else {
            return Err(format!(
                "In a session {row}, the duel kept {} pairs of snapshots of one tick, where the switch makes one.",
                pairs.len()
            ));
        };
        let same = before.encode() == after.encode();
        if !same || !holds(after).unwrap_or(false) {
            let f = fighter(after, 0)?;
            wrong.push(format!(
                "In a session {row}, the world after the switch is byte for byte the world before it: {same}. Seat 0 is in set {} and its parts play {:?}.",
                f.stance_set,
                plays(&f).ok()
            ));
        }
    }
    none_wrong(
        wrong,
        "A switch to JA in a session under JA from its start left seat 0 in set 1, and the world after it is byte for byte the world before it. A switch to MDA in a session under MDA in a level without sets left seat 0's torso on JA's own parry, and the world after it is byte for byte the world before it.".into(),
    )
}

/// The checks of spawning, picks and the rules switch, each by name.
pub(super) fn checks() -> Checks {
    vec![
        ("row_at_spawn", row_at_spawn()),
        ("pick_waits", pick_waits()),
        ("pick_refused", pick_refused()),
        ("pick_for_party", pick_for_party()),
        ("pick_of_none", pick_of_none()),
        ("rules_switch", rules_switch()),
        ("same_rules_untouched", same_rules_untouched()),
    ]
}
