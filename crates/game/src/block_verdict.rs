//! The verdict on each blow that an enemy lands on the held block of the
//! body the player controls, under the Advanced rules. The game log gets
//! one line for each block. The HUD does not show the verdict.
//!
//! The verdict comes from the presented snapshot alone, so a co-op guest
//! sees the verdict on its own body's blocks. A new `Parried` event of the
//! body, while its block button is held, is a block. The block was perfect
//! when the body's last perfect block (`Duel::perfect_at`) is new. Otherwise
//! the body's perfect block window says why it was not.
//!
//! - The window was still open. Letting go of attack does not close the
//!   window, so this holds both for attack held and for a quick click of
//!   attack. Only the aim can have failed, so the verdict is "Wrong side".
//! - The player held attack, or pressed it in the last second, and the
//!   window had closed. The window closes when its time runs out, or when
//!   the block ends. The verdict is "Too early".
//! - The player had not pressed attack. The verdict is "Blocked" at first.
//!   When the player presses attack within the late grace after the blow
//!   (`perfect::LATE_GRACE_MS`), the sim turns the block into a perfect
//!   block, and the verdict becomes "Perfect block". With targeted blocks
//!   on, a press inside the grace that the sim does not make perfect was
//!   aimed at the wrong side, and the verdict becomes "Wrong side". When
//!   the player presses attack after the grace but within 300 ms after the
//!   blow, the verdict becomes "Too late".

use std::fmt;

use sim::combat::Outcome;
use sim::{SimId, Snapshot};
use ui::vm::{BlockReticleView, BlockVerdict, Side, Swing};

/// How long after a blow a press of attack still counts as a late attempt
/// at a perfect block (ms).
pub const LATE_MS: u64 = 300;
/// How long after a blow a press of attack still makes the block perfect
/// (ms). The sim applies this rule, and the verdict only reads it.
const GRACE_MS: u64 = rules::duel::perfect::LATE_GRACE_MS;
/// How long before a blow a press of attack, already let go, still counts
/// as an early attempt at a perfect block (ms).
pub const EARLY_MS: u64 = 1000;

/// What the tracker last saw of the body the player controls.
#[derive(Clone, Copy)]
struct Seen {
    id: SimId,
    tick: u64,
    next_event: u16,
    /// The block button was held (`Duel::block_held`).
    held: bool,
    perfect_at: u64,
}

/// One block as the game log writes it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Said {
    pub verdict: BlockVerdict,
    /// The side of the aim and the threat's swing that the arcs showed just
    /// before the blow. None without the arcs.
    pub aim: Option<Side>,
    pub swing: Option<Swing>,
    /// How long the perfect block window was, when the last press of attack
    /// opened one and the block did not end after the press.
    pub window_ms: Option<u64>,
    /// How long before the blow attack was last pressed.
    pub since_press_ms: Option<u64>,
    /// How long after the blow attack was pressed, for a late attempt.
    pub late_ms: Option<u64>,
}

impl fmt::Display for Said {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let reason = match self.verdict {
            BlockVerdict::Perfect => None,
            BlockVerdict::TooEarly => Some("too_early"),
            BlockVerdict::TooLate => Some("too_late"),
            BlockVerdict::WrongSide => Some("wrong_side"),
            BlockVerdict::Blocked => Some("no_attempt"),
        };
        match reason {
            None => write!(f, "block: perfect")?,
            Some(reason) => write!(f, "block: ordinary reason={reason}")?,
        }
        if let Some(aim) = self.aim {
            write!(f, " aim={aim:?}")?;
        }
        match self.swing {
            Some(Swing::Side(side)) => write!(f, " swing={side:?}")?,
            Some(Swing::Any) => write!(f, " swing=Any")?,
            None => {}
        }
        let numbers = [
            ("window_ms", self.window_ms),
            ("since_press_ms", self.since_press_ms),
            ("late_ms", self.late_ms),
        ];
        for (name, value) in numbers {
            if let Some(value) = value {
                write!(f, " {name}={value}")?;
            }
        }
        Ok(())
    }
}

/// A block with no attempt, waiting 300 ms for a late press of attack.
#[derive(Clone, Copy)]
struct Pending {
    blow_at: u64,
    said: Said,
}

/// The verdicts of the body the player controls.
#[derive(bevy::prelude::Resource, Default)]
pub struct Verdicts {
    seen: Option<Seen>,
    pending: Option<Pending>,
}

/// The verdict on a block that was not perfect, from the body's perfect
/// block window at the blow. `aimed` says that attack was held, `open` that
/// the window was still open, and `since_press` how long ago attack was last
/// pressed. Letting go of attack does not close the window, so a click of
/// attack whose window is still open at the blow failed only on the aim.
/// None: the player made no attempt so far.
pub fn ordinary(aimed: bool, open: bool, since_press: Option<u64>) -> Option<BlockVerdict> {
    match (aimed, open) {
        (_, true) => Some(BlockVerdict::WrongSide),
        (true, false) => Some(BlockVerdict::TooEarly),
        _ if since_press.is_some_and(|ms| ms <= EARLY_MS) => Some(BlockVerdict::TooEarly),
        _ => None,
    }
}

impl Verdicts {
    /// Reads `snapshot` for the body that `seat` controls, with `arcs` the
    /// arcs shown just before it. The blocks settled by it, for the log.
    pub fn step(
        &mut self,
        snapshot: &Snapshot,
        seat: sim::Seat,
        arcs: Option<BlockReticleView>,
    ) -> Vec<Said> {
        let own = (snapshot.body(seat))
            .filter(|_| snapshot.saber_rules.duel())
            .and_then(|own| Some((own.id, own.fighter?)));
        let Some((id, fighter)) = own else {
            *self = Self::default();
            return Vec::new();
        };
        let duel = fighter.saber.duel;
        let now = snapshot.time_ms;
        let before = self.seen.filter(|seen| seen.id == id);
        if before.is_some_and(|seen| seen.tick == snapshot.tick) {
            return Vec::new();
        }
        self.seen = Some(Seen {
            id,
            tick: snapshot.tick,
            next_event: fighter.next_event,
            held: duel.block_held,
            perfect_at: duel.perfect_at,
        });
        let mut said = Vec::new();
        if let Some(pending) = self.pending {
            // The sim makes a late press perfect on the step of the press or
            // the step after it, so a press inside the grace waits one
            // snapshot for the perfect block.
            let made_perfect = duel.perfect_at > pending.blow_at;
            let settled = made_perfect || now > duel.aimed_at;
            let late = (duel.aimed_at.checked_sub(pending.blow_at))
                .filter(|&ms| ms > 0 && ms <= LATE_MS && settled);
            if let Some(late_ms) = late {
                let verdict = match late_ms {
                    _ if made_perfect => BlockVerdict::Perfect,
                    ms if ms > GRACE_MS => BlockVerdict::TooLate,
                    _ if snapshot.targeted() => BlockVerdict::WrongSide,
                    _ => BlockVerdict::TooLate,
                };
                said.push(Said {
                    verdict,
                    late_ms: Some(late_ms),
                    ..pending.said
                });
                self.pending = None;
            } else if now > pending.blow_at + LATE_MS {
                said.push(pending.said);
                self.pending = None;
            }
        }
        let Some(before) = before else {
            return said;
        };
        let fresh = fighter.next_event.wrapping_sub(before.next_event);
        let parried = (fighter.events.iter().flatten()).any(|event| {
            event.sequence.wrapping_sub(before.next_event) < fresh
                && event.result == Outcome::Parried
        });
        if !parried || !(before.held || duel.block_held) || fighter.saber.locked(now) {
            return said;
        }
        // A new block settles the one still waiting for a late press.
        said.extend(self.pending.take().map(|pending| pending.said));
        let perfect = duel.perfect_at != 0 && duel.perfect_at != before.perfect_at;
        let since_press = (duel.aimed_at != 0).then(|| now.saturating_sub(duel.aimed_at));
        let attempt = match perfect {
            true => Some(BlockVerdict::Perfect),
            false => ordinary(duel.aimed, now < duel.perfect_until, since_press),
        };
        let blow = Said {
            verdict: attempt.unwrap_or(BlockVerdict::Blocked),
            aim: arcs.and_then(|arcs| arcs.turned),
            swing: arcs.and_then(|arcs| arcs.swing),
            window_ms: (duel.perfect_until > duel.aimed_at)
                .then(|| duel.perfect_until - duel.aimed_at),
            since_press_ms: since_press.filter(|_| attempt.is_some()),
            late_ms: None,
        };
        match attempt {
            Some(_) => said.push(blow),
            None => {
                self.pending = Some(Pending {
                    blow_at: now,
                    said: blow,
                })
            }
        }
        said
    }
}

/// The verdicts in the staged fights of targeted blocks
/// (`targeted::staged`), for the body of seat 1, which holds block while
/// seat 0 swings. In the fight where seat 1 turned its camera to the left,
/// the first block is perfect. In the fight where seat 1 has not turned its
/// camera, the window is open at the blow but the aim has no side, so the
/// first verdict is "Wrong side". Against the spin, which has no side, the
/// first block is perfect. A guest's decoded snapshots give the same
/// verdicts.
///
/// The ordinary block of the unturned fight is read again with seat 1's
/// presses of attack changed in its snapshots. With attack held but the
/// window already closed, the verdict is "Too early". With attack first
/// pressed 100 ms after the blow, it is "Too late". With attack never
/// pressed, it is "Blocked".
#[cfg(feature = "probes")]
pub fn probe() -> Result<String, String> {
    use sim::trace::duelling_probes::targeted::{Stage, staged};
    let verdicts = |snapshots: &[Snapshot]| -> Vec<(Said, u64)> {
        let mut verdicts = Verdicts::default();
        (snapshots.iter())
            .flat_map(|s| {
                let said = verdicts.step(s, sim::Seat(1), None);
                said.into_iter().map(|said| (said, s.time_ms))
            })
            .collect()
    };
    let rows = [
        ("turned left", Stage::TurnedLeft, BlockVerdict::Perfect),
        ("unturned", Stage::Unturned, BlockVerdict::WrongSide),
        ("spin", Stage::Spin, BlockVerdict::Perfect),
    ];
    let mut wrong = Vec::new();
    let mut got = Vec::new();
    // The ordinary block of the unturned fight, with seat 1's presses of
    // attack changed in every snapshot: held with the window already
    // closed, first pressed 100 ms after the blow, and never pressed.
    let unturned = staged(Stage::Unturned)?;
    let blow = verdicts(&unturned).first().map_or(0, |(_, at)| *at);
    type Press = fn(&mut sim::combat::saber::duel::Duel, u64, u64);
    let presses: [(&str, Press, BlockVerdict); 3] = [
        (
            "the window closed",
            |duel, _, _| duel.perfect_until = duel.aimed_at,
            BlockVerdict::TooEarly,
        ),
        (
            "attack 100 ms after the blow",
            |duel, time, blow| {
                duel.aimed = time >= blow + 100;
                duel.aimed_at = if duel.aimed { blow + 100 } else { 0 };
                duel.perfect_until = 0;
            },
            BlockVerdict::TooLate,
        ),
        (
            "no attack",
            |duel, _, _| {
                (duel.aimed, duel.aimed_at, duel.perfect_until) = (false, 0, 0);
            },
            BlockVerdict::Blocked,
        ),
    ];
    for (name, press, want) in presses {
        let changed: Vec<Snapshot> = (unturned.iter().cloned())
            .map(|mut s| {
                let (time, id) = (s.time_ms, s.body(sim::Seat(1)).map(|own| own.id));
                let own = s.entities.iter_mut().find(|e| Some(e.id) == id);
                if let Some(fighter) = own.and_then(|own| own.fighter.as_mut()) {
                    press(&mut fighter.saber.duel, time, blow);
                }
                s
            })
            .collect();
        let first = verdicts(&changed).first().map(|(said, _)| *said);
        got.push(format!("{name}: {first:?}"));
        if first.map(|said| said.verdict) != Some(want) {
            wrong.push(format!("{name}: {first:?}, expected {want:?}"));
        }
    }
    for (name, stage, want) in rows {
        let snapshots = staged(stage)?;
        let guest: Vec<Snapshot> = (snapshots.iter())
            .map(|s| Snapshot::decode(&s.encode()).map_err(|e| format!("{e:?}")))
            .collect::<Result<_, _>>()?;
        let (host_said, guest_said) = (verdicts(&snapshots), verdicts(&guest));
        let first = host_said.first().map(|(said, _)| said.verdict);
        got.push(format!("{name}: {:?}", host_said.first()));
        if host_said != guest_said {
            wrong.push(format!("{name}: the guest's verdicts differ"));
        }
        let Some(verdict) = first else {
            wrong.push(format!("{name}: no block"));
            continue;
        };
        if verdict != want {
            wrong.push(format!("{name}: {verdict:?}, expected {want:?}"));
        }
    }
    match wrong.is_empty() {
        true => Ok(got.join("; ")),
        false => Err(format!("{}; got {}", wrong.join("; "), got.join("; "))),
    }
}
