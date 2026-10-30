//! `kjk combat duel-lag`: lag compensation's measures (`docs/COMBAT.md`,
//! C18, C19). Two players duel on `kjk combat duel`'s
//! ring, both medium sabers at Jedi Defense 3: seat 1 on the host, seat 0
//! a remote player whose view lags L ms (0 to 350). Each runs `seconds`
//! from several seeds, compensated (the view lag in its commands: C18 and
//! C19) and not (JA's way: the authority meets the present):
//!
//! - **seat 1** fights to a rhythm: attack held 700 ms, a swing each way in
//!   turn, then 800 ms idle, facing seat 0 (blocking by state) and circling
//!   it, closing in beyond 64 units;
//! - **seat 0** fights to its own (500 ms on, 700 off) as it sees the world:
//!   seat 1 as the authority had it L ms before, which it aims at, and while
//!   it sees seat 1 attack it stands and lets go to block;
//! - **its client** predicts its own swing as a client does: from the
//!   authority snapshot it has (L less the interpolation delay, 3 ticks,
//!   before), re-stepped with its own commands and the others' last.
//!
//! The measures:
//! - **disagreement**: a swing of seat 0 *shows contact* when, in a tick
//!   it attacks (the move attacks and its animation has power), its
//!   predicted blade's sweep cuts seat 1's body as seat 0 saw it, by the
//!   authority's own test (`sim::seen_cuts`; I22 C20's contact sparks). The
//!   authority's verdict on the authority swing the client predicted (the
//!   latest of that move): **landed** (seat 1 among its contacts),
//!   **turned** (seat 1 blocked it by state or the blades clashed: a
//!   Parried event on seat 1 by seat 0) or **rejected** (neither, or no
//!   such swing: **never begun**, the client predicted a swing the
//!   authority's verdict on seat 0 kept from starting, which no rewind
//!   helps). The disagreement rate is rejected over shown; compensation is
//!   checked on the swings the authority began;
//! - **grace usage**: blows on seat 0 held for it (C19) and those that its
//!   late commands turned into blocks (gone before their hold ended);
//! - **rewind depth**: the ticks each of seat 0's sweeps is rewound (the
//!   view lag within `sim::REWIND_MS`), and how far seat 1 moved over them.
//!
//! - **bolt grace**: seat 1 shoots a rifle at seat 0 (Jedi Defense 2),
//!   which looks 90° aside for 400 ms, then at seat 1 for 400 ms; a bolt
//!   that meets it looking aside is held for its grace (C19), turned if its
//!   next commands (made seeing the world before the hit) face it back in
//!   time, else landing when the hold ends.
//!
//! Every run's recording replays. Then the rewinds' cost: end_m01aa with
//! 26 sword users fighting and seat 0 swinging among them, each sampled
//! tick stepped with its view 150 ms behind and on the host. Writes
//! `facts.json`.

use std::collections::{BTreeSet, VecDeque};
use std::process::ExitCode;
use std::sync::Arc;

use sim::combat::fighter::Outcome;
use sim::combat::saber::{Style, power};
use sim::combat::{Fighter, Sheet};
use sim::motion::{AnimTable, MotionState};
use sim::{Command, Level, Seat, SimId, SimRng, SimWorld, Snapshot, StepReason, TickInput, step};
use vfs::Discovery;

use crate::by_hand::with_defence;
use crate::duel_verbs::{
    Bout, Keys, SWINGS, end_m01aa, mean, median, seat_in, sword_users, timed, with_data,
};
use crate::saber_verbs::{ATTACK, Report, jedi, saber, weapons_for};
use crate::stage::ring;

/// The view lags measured (ms): a view lag is about the round trip and 80
/// ms more (`cargo xtask duo`'s net feel), so these stand for players up to
/// about 270 ms round trip.
const LAGS: [u16; 7] = [0, 80, 150, 200, 250, 300, 350];
/// The client's interpolation delay (ticks): its newest snapshot is this
/// much newer than what it shows of others.
const SHOWN_BEHIND: u64 = 3;
/// Idle duellists close in beyond this (units).
const CLOSE: f32 = 64.0;
/// How far back a client's swing is looked for among the authority's
/// (ticks: a second, longer than any swing).
const SWING_TICKS: u64 = 125;

/// A fighting rhythm: attack held `on` ms, then `off` ms idle, from
/// `phase` into its first period.
#[derive(Clone, Copy)]
struct Rhythm {
    on: u64,
    off: u64,
    phase: u64,
}

impl Rhythm {
    /// The keys at `ms`, the other `apart` units away: attack with the
    /// period's swing, or idle circling it (strafing one way, the next
    /// period the other), closing in beyond `CLOSE`.
    fn keys(&self, ms: u64, apart: f32) -> Keys {
        let at = ms + self.phase;
        let period = at / (self.on + self.off);
        if at % (self.on + self.off) >= self.on {
            let way = if period.is_multiple_of(2) { 127 } else { -127 };
            let closing = if apart > CLOSE { 127 } else { 0 };
            return Keys::default().with(closing, way);
        }
        let (_, forward, right) = SWINGS[(period % SWINGS.len() as u64) as usize];
        Keys::press(ATTACK).with(forward, right)
    }
}

/// Seat 0's swings as its client showed them and the authority judged.
#[derive(Default, Clone, Copy)]
struct Verdicts {
    /// Swings that showed contact, and of those: landed, turned, rejected
    /// (`unbegun` of them never begun by the authority).
    shown: u32,
    landed: u32,
    turned: u32,
    rejected: u32,
    unbegun: u32,
    /// Swings that landed without showing contact.
    unseen: u32,
}

impl Verdicts {
    fn add(&mut self, other: Verdicts) {
        self.shown += other.shown;
        self.landed += other.landed;
        self.turned += other.turned;
        self.rejected += other.rejected;
        self.unbegun += other.unbegun;
        self.unseen += other.unseen;
    }

    fn rate(&self) -> f64 {
        f64::from(self.rejected) / f64::from(self.shown.max(1))
    }

    /// The disagreement among the swings the authority began.
    fn begun_rate(&self) -> f64 {
        f64::from(self.rejected - self.unbegun) / f64::from((self.shown - self.unbegun).max(1))
    }
}

/// A swing of seat 0 on the authority: its move and restart bit, the tick
/// it began, whether its client showed contact in it, and the verdicts.
struct Swing {
    key: (u8, bool),
    start: u64,
    shown: bool,
    landed: bool,
    turned: bool,
}

/// A fighter's swing under way, as `Swing::key`.
fn key_of(fighter: &Fighter) -> (u8, bool) {
    (fighter.swing.saber_move as u8, fighter.swing.flip)
}

impl Swing {
    fn judge(&self, verdicts: &mut Verdicts) {
        if self.shown {
            verdicts.shown += 1;
            if self.landed {
                verdicts.landed += 1;
            } else if self.turned {
                verdicts.turned += 1;
            } else {
                verdicts.rejected += 1;
            }
        } else if self.landed {
            verdicts.unseen += 1;
        }
    }
}

/// One run's measures.
#[derive(Default, Clone, Copy)]
struct Measures {
    verdicts: Verdicts,
    /// Blows held for seat 0, turned to blocks, landed after their hold.
    held: u32,
    graced: u32,
    landed_late: u32,
    /// Seat 1's blows on seat 0 that hit, and that it blocked.
    hit: u32,
    blocked: u32,
    /// Ticks of seat 0's attacking sweeps, the ticks they were rewound in
    /// all, and seat 1's moves over them (units: sum and largest).
    sweeps: u32,
    rewound: u64,
    moved: f64,
    moved_max: f32,
    replayed: bool,
    end: Option<sim::SnapshotHash>,
}

impl Measures {
    fn add(&mut self, other: &Measures) {
        self.verdicts.add(other.verdicts);
        self.held += other.held;
        self.graced += other.graced;
        self.landed_late += other.landed_late;
        self.hit += other.hit;
        self.blocked += other.blocked;
        self.sweeps += other.sweeps;
        self.rewound += other.rewound;
        self.moved += other.moved;
        self.moved_max = self.moved_max.max(other.moved_max);
    }
}

/// What seat 0 does at a tick, from what it sees.
fn remote_keys(rhythm: &Rhythm, ms: u64, apart: f32, seen: &Fighter) -> Keys {
    if seen.saber.current_move.attack() {
        Keys::default()
    } else {
        rhythm.keys(ms, apart)
    }
}

/// How far apart two players stand (units).
fn apart(a: &MotionState, b: &MotionState) -> f32 {
    sim::math::length(sim::math::sub(a.origin, b.origin))
}

/// Whether `fighter` (standing as `ps`) sweeps to cut this tick: an
/// attack, or a special in its animation's power window.
fn cutting(anims: &AnimTable, ps: &MotionState, fighter: &Fighter) -> bool {
    let saber = &fighter.saber;
    power::attacking(saber.current_move, saber.blocked, fighter.weapon_wait_ms)
        && power::animation(anims, ps.torso.anim, ps.torso.timer, false) > 0
}

/// The snapshot of `tick` among `snapshots` from tick `first` on (the first
/// one before it).
fn at(snapshots: &[Snapshot], first: u64, tick: u64) -> &Snapshot {
    &snapshots[(tick.max(first) - first) as usize]
}

/// One run of a duel: `lag_ms` behind, compensated or not, from `seed`.
struct Duel<'a> {
    level: &'a Level,
    lag_ms: u16,
    compensated: bool,
    seed: u64,
    ms: u64,
}

impl Duel<'_> {
    /// Whether seat 0's blade, from `before` to `now` (its predicted
    /// snapshots), cuts seat 1 (`them`, standing as `seen`) as the
    /// authority's sweep would (`sim::seen_cuts`, what the client's contact
    /// sparks show).
    fn shows_contact(
        &self,
        before: &Snapshot,
        now: &Snapshot,
        them: SimId,
        seen: &MotionState,
    ) -> bool {
        let (me, was) = (seat_in(now, 0), seat_in(before, 0));
        let (Some(f), Some(ps), Some(old), Some(sheet)) =
            (me.fighter, me.player, was.fighter, me.sheet)
        else {
            return false;
        };
        // Two players: humanoids, met by JA's box.
        let body = sim::body(them, seen, None);
        !sim::seen_cuts(
            self.level,
            None,
            (&ps, &sheet),
            &old,
            &f,
            now.time_ms,
            std::slice::from_ref(&body),
        )
        .is_empty()
    }

    fn run(&self) -> Measures {
        let sheet = || {
            let mut sheet = with_defence(jedi(weapons_for(Style::Medium), 100), 3);
            sheet.max_hp = 5000;
            sheet
        };
        let mut bout = Bout::new(self.level, [sheet(), sheet()], 52.0, self.seed);
        if self.compensated {
            bout.lags[0] = self.lag_ms;
        }
        bout.light();
        let anims = self.level.anims().clone();
        let msec = u64::from(bout.rate.msec(bout.last.tick + 1));
        let behind = u64::from(self.lag_ms) / msec;
        let predicted = behind.saturating_sub(SHOWN_BEHIND);
        let rewind = u64::from(self.lag_ms).min(sim::REWIND_MS) / msec;
        let mut dice = SimRng::new(self.seed, 25);
        let rhythms = [
            Rhythm {
                on: 500,
                off: 700,
                phase: dice.below(1200) as u64,
            },
            Rhythm {
                on: 700,
                off: 800,
                phase: dice.below(1500) as u64,
            },
        ];
        let ids = [bout.seat(0).id, bout.seat(1).id];
        let first = bout.last.tick;
        let start_ms = bout.now();
        let mut snapshots: Vec<Snapshot> = vec![bout.last.clone()];
        let mut commands: Vec<[Command; 2]> = vec![[Command::default(); 2]];
        let mut m = Measures::default();
        let mut swings: Vec<Swing> = Vec::new();
        let mut orphans: BTreeSet<(u8, bool)> = BTreeSet::new();
        let mut held: BTreeSet<(u32, u64, u64)> = BTreeSet::new();
        while bout.now() < start_ms + self.ms {
            let tick = bout.last.tick + 1;
            // What seat 0 saw making this tick's command: at least the last
            // tick (in lockstep the view lag is 0 and it saw that).
            let them = seat_in(at(&snapshots, first, tick - behind.max(1)), 1);
            let since = bout.now() - start_ms;
            let (seen, present) = (them.player.expect("a player"), bout.ps(0));
            let keys = [
                remote_keys(
                    &rhythms[0],
                    since,
                    apart(&present, &seen),
                    &them.fighter.expect("a fighter"),
                ),
                rhythms[1].keys(since, apart(&present, &bout.ps(1))),
            ];
            let orders = [
                bout.command(0, keys[0], &seen),
                bout.command(1, keys[1], &present),
            ];
            bout.advance(orders);
            snapshots.push(bout.last.clone());
            commands.push(orders);
            let (prev, now) = self.predict(&snapshots, &commands, first, tick, predicted);
            // Seat 1 as the authority met seat 0's sweep: rewound, or now.
            let seen_now = seat_in(at(&snapshots, first, tick - behind), 1)
                .player
                .expect("a player");
            let shown = self.shows_contact(&prev, &now, ids[1], &seen_now);
            // The authority's swing of seat 0 and its verdicts; a contact
            // shown goes to the swing its client predicted (the latest
            // authority swing of that move begun within a second): a swing
            // the authority deflected or bounced is still the one shown.
            let f0 = bout.fighter(0);
            let key = key_of(&f0);
            if swings.last().is_none_or(|swing| swing.key != key) {
                swings.push(Swing {
                    key,
                    start: tick,
                    shown: false,
                    landed: false,
                    turned: false,
                });
            }
            if shown {
                let mine = key_of(&seat_in(&now, 0).fighter.expect("a fighter"));
                let swing = swings
                    .iter_mut()
                    .rev()
                    .take_while(|swing| swing.start + SWING_TICKS >= tick)
                    .find(|swing| swing.key == mine);
                match swing {
                    Some(swing) => swing.shown = true,
                    None => {
                        orphans.insert(mine);
                    }
                }
            }
            let swing = swings.last_mut().expect("one under way");
            swing.landed |= f0
                .swing
                .contacts
                .iter()
                .flatten()
                .any(|c| c.victim == ids[1]);
            for (seat, event) in bout.events() {
                let by = |id: usize| event.by == Some(ids[id]);
                match (seat, event.result) {
                    (1, Outcome::Parried) if by(0) => swing.turned = true,
                    (0, Outcome::Hit | Outcome::Critical) if by(1) => m.hit += 1,
                    (0, Outcome::Parried) if by(1) => m.blocked += 1,
                    _ => {}
                }
            }
            // Seat 0's sweeps, how far back, and how far seat 1 went.
            if cutting(&anims, &bout.ps(0), &f0) {
                m.sweeps += 1;
                if self.compensated && rewind > 0 {
                    m.rewound += rewind;
                    let then = seat_in(at(&snapshots, first, tick - rewind), 1)
                        .player
                        .expect("a player");
                    let moved = sim::math::length(sim::math::sub(bout.ps(1).origin, then.origin));
                    m.moved += f64::from(moved);
                    m.moved_max = m.moved_max.max(moved);
                }
            }
            // The blows held for seat 0 (C19): new ones, and those gone.
            let holding: BTreeSet<(u32, u64, u64)> = f0
                .grace
                .iter()
                .flatten()
                .map(|blow| (blow.by.0, blow.tick, blow.until()))
                .collect();
            m.held += holding.difference(&held).count() as u32;
            for &(_, _, until) in held.difference(&holding) {
                if tick < until {
                    m.graced += 1;
                } else {
                    m.landed_late += 1;
                }
            }
            held = holding;
        }
        for swing in &swings {
            swing.judge(&mut m.verdicts);
        }
        // A swing its client showed that the authority never began.
        m.verdicts.shown += orphans.len() as u32;
        m.verdicts.rejected += orphans.len() as u32;
        m.verdicts.unbegun += orphans.len() as u32;
        m.end = Some(bout.last.hash());
        m.replayed = bout.replays();
        m
    }

    /// Seat 0's client's prediction of ticks `tick - 1` and `tick`: from the
    /// authority snapshot `predicted` ticks back, its own commands as sent
    /// and seat 1's last known (none to predict: the authority's own).
    fn predict(
        &self,
        snapshots: &[Snapshot],
        commands: &[[Command; 2]],
        first: u64,
        tick: u64,
        predicted: u64,
    ) -> (Snapshot, Snapshot) {
        let index = |tick: u64| (tick.max(first) - first) as usize;
        let from = tick.saturating_sub(predicted).max(first);
        let mut before = at(snapshots, first, tick - 1).clone();
        let mut now = at(snapshots, first, from).clone();
        if from == tick {
            return (before, now);
        }
        let mut world = SimWorld::from_snapshot(&now, self.level.clone()).expect("restores");
        let theirs = commands[index(from)][1];
        for t in from + 1..=tick {
            let mut input = TickInput::default();
            input.insert(Seat(0), commands[index(t)][0]);
            input.set_view_lag(Seat(0), self.lag_ms);
            input.insert(Seat(1), theirs);
            let msec = sim::TickRate::default().msec(t);
            before = now;
            now = step(&mut world, t, &input, msec, StepReason::Predict);
        }
        (before, now)
    }
}

/// The duels at each lag, compensated and not.
fn duels(level: &Level, seconds: u64, seeds: u64, report: &mut Report) -> serde_json::Value {
    outln!(
        "duels: seat 0 remote (its view L ms behind), seat 1 on the host; {seeds} seeds × {seconds} s each way"
    );
    let mut rows = Vec::new();
    let mut by_lag: Vec<(u16, Measures, Measures)> = Vec::new();
    let mut same_at_zero = true;
    for lag_ms in LAGS {
        let mut totals = [Measures::default(); 2];
        let mut replayed = true;
        for seed in 1..=seeds {
            let ends: Vec<Measures> = [true, false]
                .iter()
                .map(|&compensated| {
                    Duel {
                        level,
                        lag_ms,
                        compensated,
                        seed,
                        ms: seconds * 1000,
                    }
                    .run()
                })
                .collect();
            if lag_ms == 0 {
                same_at_zero &= ends[0].end == ends[1].end;
            }
            for (total, run) in totals.iter_mut().zip(&ends) {
                replayed &= run.replayed;
                total.add(run);
            }
        }
        for (name, m) in ["compensated", "uncompensated"].iter().zip(&totals) {
            let v = &m.verdicts;
            let depth = m.rewound as f64 / f64::from(m.sweeps.max(1));
            outln!(
                "  {lag_ms:>3} ms {name:<13}: {} swings showed contact: {} landed, {} turned, {} rejected ({} never begun; {:.1}% disagreement); {} landed unseen; blows on the remote: {} held, {} turned to blocks by grace, {} landed after their hold; {} hit, {} blocked; rewound {depth:.1} ticks a sweep, seat 1 moved {:.1} units on average (at most {:.1})",
                v.shown,
                v.landed,
                v.turned,
                v.rejected,
                v.unbegun,
                v.rate() * 100.0,
                v.unseen,
                m.held,
                m.graced,
                m.landed_late,
                m.hit,
                m.blocked,
                m.moved / f64::from(m.sweeps.max(1)),
                m.moved_max
            );
            rows.push(serde_json::json!({
                "lag_ms": lag_ms, "mode": name,
                "shown": v.shown, "landed": v.landed, "turned": v.turned,
                "rejected": v.rejected, "never_begun": v.unbegun, "disagreement": v.rate(), "unseen": v.unseen,
                "held": m.held, "graced": m.graced, "landed_after_hold": m.landed_late,
                "hit": m.hit, "blocked": m.blocked, "sweeps": m.sweeps,
                "rewind_ticks": depth, "moved_mean": m.moved / f64::from(m.sweeps.max(1)),
                "moved_max": m.moved_max,
            }));
        }
        report.check(
            format!("{lag_ms} ms: every run's recording replays"),
            replayed,
            format!("{} runs", seeds * 2),
        );
        by_lag.push((lag_ms, totals[0], totals[1]));
    }
    report.check(
        "0 ms: the view lag changes nothing (each seed ends the same either way)",
        same_at_zero,
        format!("{seeds} seeds"),
    );
    let at = |lag: u16| by_lag.iter().find(|(l, ..)| *l == lag).expect("measured");
    let base = at(0).1.verdicts.begun_rate();
    for (lag_ms, with, without) in &by_lag[1..] {
        let v = &with.verdicts;
        report.check(
            format!("{lag_ms} ms: compensated swings the authority began disagree no more than at 0 ms (+5 points), and all no more than uncompensated"),
            v.begun_rate() <= base + 0.05 && v.rate() <= without.verdicts.rate(),
            format!(
                "{:.1}% against {:.1}% at 0 ms ({:.1}% with those never begun) and {:.1}% uncompensated",
                v.begun_rate() * 100.0,
                base * 100.0,
                v.rate() * 100.0,
                without.verdicts.rate() * 100.0
            ),
        );
        report.check(
            format!("{lag_ms} ms: the remote player's grace turns held blows into blocks"),
            with.held > 0 && with.graced > 0 && without.held == 0,
            format!(
                "{} held, {} blocked by grace, {} landed after; uncompensated {} held",
                with.held, with.graced, with.landed_late, without.held
            ),
        );
    }
    let (with, without) = (&at(150).1, &at(150).2);
    report.check(
        "150 ms: uncompensated, the client shows contact the authority rejects",
        without.verdicts.rate() > with.verdicts.rate(),
        format!(
            "{:.1}% uncompensated against {:.1}% compensated",
            without.verdicts.rate() * 100.0,
            with.verdicts.rate() * 100.0
        ),
    );
    report.check(
        "0 ms: nothing held",
        at(0).1.held == 0,
        format!("{} held", at(0).1.held),
    );
    serde_json::json!(rows)
}

/// The rewinds' cost (see the module documentation): `TICKS_BACK` ticks
/// of history filled before each timed step.
fn rewind_cost(
    discovery: &Discovery,
    anims: &Arc<AnimTable>,
    report: &mut Report,
) -> serde_json::Value {
    const LAG_MS: u16 = 150;
    let ticks_back = usize::from(LAG_MS) / 8;
    outln!(
        "rewind cost: end_m01aa, 26 sword users and seat 0 swinging by the Dark Jedi, its view {LAG_MS} ms behind and on the host"
    );
    let Some(plan) = end_m01aa(discovery, anims, report) else {
        return serde_json::Value::Null;
    };
    let (data, _) = sword_users(&plan, 26);
    let level = with_data(&plan, &data, anims.clone());
    let config = sim::WorldConfig {
        seed: 1,
        seats: 1,
        ..sim::WorldConfig::one_seat()
    };
    let mut world = SimWorld::new(config, Level::arena()).expect("one seat");
    if let Err(error) = world.install(level.clone(), None) {
        report.check("end_m01aa installs", false, error.to_string());
        return serde_json::Value::Null;
    }
    let mut start = world.snapshot();
    let me = crate::scenes::seat0(&start);
    let Some(&target) = crate::scenes::tagged(&level, &start, "end_darkjedi01").first() else {
        report.check("the Dark Jedi stands in end_m01aa", false, "not found");
        return serde_json::Value::Null;
    };
    let them = crate::scenes::player(&start, target);
    let Some(spot) = crate::scenes::spot_near(&level, &them, 56.0) else {
        report.check("room by the Dark Jedi", false, "none");
        return serde_json::Value::Null;
    };
    let sheet: Sheet = jedi(weapons_for(Style::Medium), 100);
    let (_, yaw) = sim::combat::mind::aim(spot, them.origin);
    start.entities[me].player = Some(MotionState::standing(spot, yaw));
    start.entities[me].sheet = Some(sheet);
    start.entities[me].fighter = Some(Fighter::of(&sheet));
    let mut world = SimWorld::from_snapshot(&start, level.clone()).expect("restores");
    let rate = sim::TickRate::default();
    let mut last = start;
    // The ticks before this one: each one's snapshot before it, and input.
    let mut past: VecDeque<(Snapshot, TickInput)> = VecDeque::new();
    // Per sample: on the host, behind, and whether seat 0 was cutting.
    let mut samples: Vec<(f64, f64, bool)> = Vec::new();
    while rate.time_ms(last.tick) < 15_000 {
        let tick = last.tick + 1;
        let mut input = TickInput::default();
        input.insert(
            Seat(0),
            crate::scenes::aim(&last, me, target, (ATTACK, 0, 0)),
        );
        if past.len() == ticks_back && tick.is_multiple_of(2) {
            let warm: Vec<TickInput> = past.iter().map(|(_, input)| input.clone()).collect();
            let mut behind = input.clone();
            behind.set_view_lag(Seat(0), LAG_MS);
            // Each way first in turn, so neither has the warmer caches.
            let mut ways = [(&level, &input), (&level, &behind)];
            let turn = tick.is_multiple_of(4);
            if turn {
                ways.reverse();
            }
            let mut times = timed(&past[0].0, rate, &warm, &ways);
            if turn {
                times.reverse();
            }
            let ps = crate::scenes::player(&last, me);
            let swinging = cutting(anims, &ps, &crate::scenes::fighter(&last, me));
            samples.push((times[0], times[1], swinging));
        }
        past.push_back((last.clone(), input.clone()));
        if past.len() > ticks_back {
            past.pop_front();
        }
        last = step(
            &mut world,
            tick,
            &input,
            rate.msec(tick),
            StepReason::Authority,
        );
    }
    let pick = |cutting_only: bool, of: fn(&(f64, f64, bool)) -> f64| -> Vec<f64> {
        samples
            .iter()
            .filter(|sample| sample.2 || !cutting_only)
            .map(of)
            .collect()
    };
    let (on_host, behind) = (mean(&pick(false, |s| s.0)), mean(&pick(false, |s| s.1)));
    let while_cutting = pick(true, |s| s.1 - s.0);
    let (per_sweep, per_sweep_median) = (mean(&while_cutting), median(&while_cutting));
    outln!(
        "  a tick {on_host:.1} µs with seat 0's view on the host, {behind:.1} µs {LAG_MS} ms behind: the rewinds {:+.1} µs a tick (median {:+.1}), {per_sweep:+.1} µs (median {per_sweep_median:+.1}) in the {} of {} samples where seat 0's blade cut",
        behind - on_host,
        median(&pick(false, |s| s.1 - s.0)),
        while_cutting.len(),
        samples.len()
    );
    report.check(
        "the rewinds' cost was measured",
        !while_cutting.is_empty(),
        format!("{} samples, {} cutting", samples.len(), while_cutting.len()),
    );
    serde_json::json!({
        "lag_ms": LAG_MS, "host_us": on_host, "behind_us": behind,
        "rewinds_us": behind - on_host, "rewinds_cutting_us": per_sweep,
        "rewinds_cutting_median_us": per_sweep_median,
        "samples": samples.len(), "cutting_samples": while_cutting.len(),
    })
}

/// Bolt grace (see the module documentation): `seconds` from one seed at
/// each lag, with the lag in seat 0's commands and without.
fn bolt_grace(level: &Level, seconds: u64, report: &mut Report) -> serde_json::Value {
    outln!(
        "bolt grace: seat 1 shoots a rifle at seat 0 (defence 2, 300 units away), which looks aside 400 ms, then at it 400 ms"
    );
    let rifle = sim::combat::Weapon::plain(
        20,
        1,
        8,
        (1, 2),
        sim::combat::sheet::damage_types::ENERGY,
        [sim::combat::sheet::feats::PROFICIENCY_BLASTER_RIFLE, 0, 0],
    );
    let mut rows = Vec::new();
    for lag in LAGS {
        for compensated in [true, false] {
            let mut target = with_defence(jedi([Some(saber(8)), None], 100), 2);
            target.max_hp = 2000;
            let mut shooter = jedi([Some(rifle), None], 0);
            shooter
                .feats
                .insert(sim::combat::sheet::feats::PROFICIENCY_BLASTER_RIFLE);
            shooter.max_hp = 2000;
            let mut bout = Bout::new(level, [target, shooter], 300.0, 5);
            bout.light();
            bout.lags = [if compensated { lag } else { 0 }, 0];
            let shooter_id = bout.seat(1).id;
            let (mut held, mut turned, mut hit) = (0usize, 0usize, 0usize);
            let mut holding: BTreeSet<u32> = BTreeSet::new();
            let end = bout.now() + seconds * 1000;
            while bout.now() < end {
                let aside = (bout.now() / 400).is_multiple_of(2);
                let me = bout.ps(0);
                let them = bout.ps(1);
                let look = bout.command(
                    0,
                    Keys::default().turned(if aside { 90.0 } else { 0.0 }),
                    &them,
                );
                let shoot = bout.command(1, Keys::press(ATTACK), &me);
                bout.advance([look, shoot]);
                for (seat, event) in bout.events() {
                    match (seat, event.result) {
                        (0, Outcome::Hit | Outcome::Critical) if event.by == Some(shooter_id) => {
                            hit += 1
                        }
                        _ => {}
                    }
                }
                let now: BTreeSet<u32> = bout
                    .last
                    .entities
                    .iter()
                    .filter(|e| e.missile.is_some_and(|m| m.held.is_some()))
                    .map(|e| e.id.0)
                    .collect();
                held += now.difference(&holding).count();
                // A held bolt still flying, now its target's: turned by grace.
                turned += holding
                    .difference(&now)
                    .filter(|id| {
                        bout.last.entities.iter().any(|e| {
                            e.id.0 == **id && e.missile.is_some_and(|m| m.owner != shooter_id)
                        })
                    })
                    .count();
                holding = now;
            }
            let absorbed_or_hit = held.saturating_sub(turned);
            let way = if compensated {
                "compensated"
            } else {
                "uncompensated"
            };
            outln!(
                "  {lag:>4} ms {way:<13}: {held:>3} bolts held, {turned:>2} turned by grace, {absorbed_or_hit:>3} landed after their hold; {hit} hits on seat 0"
            );
            rows.push(serde_json::json!({
                "lag_ms": lag, "compensated": compensated, "held": held,
                "turned_by_grace": turned, "hits": hit,
            }));
            let replays = bout.replays();
            if !compensated || lag == 0 {
                report.check(
                    format!("{lag} ms {way}: no bolt held"),
                    held == 0 && replays,
                    format!("{held} held, replays {replays}"),
                );
            } else {
                report.check(
                    format!("{lag} ms compensated: bolts on the remote player are held, and grace turns some"),
                    held > 0 && turned > 0 && replays,
                    format!("{held} held, {turned} turned, replays {replays}"),
                );
            }
        }
    }
    serde_json::json!(rows)
}

/// `kjk combat duel-lag`.
pub fn duel_lag(discovery: &Discovery, seconds: u64, seeds: u64) -> ExitCode {
    let (anims, note) = crate::stage::anims(discovery);
    outln!("combat duel-lag: {note}");
    if anims.skeleton().is_none() {
        eprintln!("the duels need JA's skeleton and sabers");
        return ExitCode::FAILURE;
    }
    let anims = Arc::new(anims);
    let level = ring(&anims);
    let mut report = Report::default();
    let mut facts = serde_json::json!({
        "verb": "combat duel-lag", "seconds": seconds, "seeds": seeds,
        "duels": duels(&level, seconds, seeds, &mut report),
        "bolt_grace": bolt_grace(&level, seconds.min(20), &mut report),
        "rewind_cost": rewind_cost(discovery, &anims, &mut report),
    });
    let failed = report.checks.iter().filter(|c| !c.pass).count();
    outln!(
        "combat duel-lag: {} checks, {failed} failed",
        report.checks.len()
    );
    facts["checks"] = serde_json::json!(
        report
            .checks
            .iter()
            .map(|c| serde_json::json!({"name": c.name, "pass": c.pass, "seen": c.seen}))
            .collect::<Vec<_>>()
    );
    crate::runtime::write_facts(facts);
    if failed == 0 {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}
