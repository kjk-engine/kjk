//! The checks of a fighter's stance set at spawn, of a seat's picks and of
//! the session's rules switch. A fighter made under MDA takes its table
//! row's set, and under JA it takes none. A pick waits for the weapon
//! timer, as a queued style change does. A pick the rules refuse changes
//! nothing at all. Switching the rules to JA leaves no fighter in a set and
//! no part reading the lent frames, and switching back gives the rows'
//! sets again.

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
    for (rules, seats, sith) in [("mda", 1, 2), ("ja", 0, 0)] {
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
    let ticks = play(&duel("mda", true, "medium", 1, &then))?;
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
        ("seat 1 picks for seat 0's character", "mda", "pick 1 0 2"),
        ("seat 0 picks set 3 of 2", "mda", "pick 0 0 3"),
        ("seat 0 picks under JA's rules", "ja", "pick 0 0 2"),
        (
            "seat 0 picks for a creature out of the party",
            "mda",
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

fn pick_of_none() -> Result<String, String> {
    let ticks = play(&duel(
        "mda",
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

/// Whether a torso shows a set's held guard while it still blends in.
fn guard_blending(torso: &Shown, now: u64) -> bool {
    torso.plays != Played::OWN && !blended(torso.playback.blend, now)
}

/// Whether a torso plays JA's own sequence while its blend still reads a
/// set's lent frames.
fn swing_blending(torso: &Shown, now: u64) -> bool {
    torso.plays == Played::OWN && reads_lent(torso) && !blended(torso.playback.blend, now)
}

fn rules_switch() -> Result<String, String> {
    // Each row names the moment of the switch, the directives that lead up
    // to it, and what seat 0's torso shows on the tick before it.
    type Row = (&'static str, &'static str, fn(&Shown, u64) -> bool);
    let rows: [Row; 2] = [
        (
            "a held block's blend into a set's guard",
            "tick 2 h0=block",
            guard_blending,
        ),
        (
            "the first 150 ms of a swing out of a set's stance",
            "tick 1 b0=attack\ntick 2",
            swing_blending,
        ),
    ];
    let mut wrong = Vec::new();
    for (row, before, leads_up) in rows {
        let then = format!("{LIGHT}\ntick 20\n{before}\nrules ja\nrules mda");
        let ticks = play(&duel("mda", true, "medium", 1, &then))?;
        let [.., blending, ja, mda] = ticks.as_slice() else {
            return Err(format!(
                "In {row} the duel kept fewer than three snapshots."
            ));
        };
        let torso = parts(&fighter(blending, 0)?)?[1];
        if !leads_up(&torso, blending.time_ms) {
            wrong.push(format!(
                "Before the switch in {row}, seat 0's torso plays {:?} with the blend {:?}.",
                torso.plays, torso.playback.blend
            ));
        }
        for entity in &ja.entities {
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
        "A switch to JA during a held block's blend into a set's guard left every fighter in no set and every part on its own sequence and frames. A switch to JA in the first 150 ms of a swing out of a set's stance did the same. A switch back to MDA gave both seats their rows' set 1 each time.".into(),
    )
}

fn ja_switch_untouched() -> Result<String, String> {
    // The `stance` directive puts seat 0 in set 1 under JA's rules, which
    // no rule of the sim does. A switch to JA must leave even that alone.
    let then = format!("{LIGHT}\nrules ja\ntick 5");
    let ticks = play(&duel("ja", true, "medium", 1, &then))?;
    // The switch keeps one more snapshot of the tick it happens on.
    let pairs: Vec<(&Snapshot, &Snapshot)> = ticks
        .windows(2)
        .filter(|pair| pair[0].tick == pair[1].tick)
        .map(|pair| (&pair[0], &pair[1]))
        .collect();
    let [(before, after)] = pairs.as_slice() else {
        return Err(format!(
            "The duel kept {} pairs of snapshots of one tick, where the switch makes one.",
            pairs.len()
        ));
    };
    let set = fighter(after, 0)?.stance_set;
    let same = before.encode() == after.encode();
    let world = if same {
        "is byte for byte the world before it"
    } else {
        "differs from the world before it"
    };
    check(
        set == 1 && same,
        format!(
            "A switch to JA in a session under JA from its start left seat 0 in set {set}. The world after the switch {world}."
        ),
    )
}

/// The checks of spawning, picks and the rules switch, each by name.
pub(super) fn checks() -> Checks {
    vec![
        ("row_at_spawn", row_at_spawn()),
        ("pick_waits", pick_waits()),
        ("pick_refused", pick_refused()),
        ("pick_of_none", pick_of_none()),
        ("rules_switch", rules_switch()),
        ("ja_switch_untouched", ja_switch_untouched()),
    ]
}
