//! Combat grunts: the lines of a character's sound set (`fmt_odyssey::ssf::
//! slots`) the K1 executable has it say as it fights, its own client's
//! choice, so drawn here (`view_rng`) and heard as the scripts' lines are
//! (`sim::notice::Sound::line`, `ambience`):
//!
//! - **attack**: as a melee attack begins, the executable's at its attack
//!   clip's start: a scene's swing (a round's, its `Acting` a new
//!   `blows::SWING`) or JA's own (its saber or melee move entering an
//!   attack, `swung`): one of AttackGrunt 1–3 at random; a gun's shot says
//!   nothing;
//! - **pain**: a landed blow on one still standing (a hit, critical or
//!   automatic hit event with damage, its hit points above 0): one of
//!   PainGrunt 1–2 at random.
//!
//! Each is said always by the player's own character, else one time in
//! five. Two flags of the executable's attack that skip both are not read
//! [verify]. The death cry (`Dead`) is cried by every creature that dies,
//! a script's death too (`combat_fx`).

use std::collections::BTreeMap;

use bevy::prelude::*;
use fmt_odyssey::ssf::slots;
use sim::combat::Outcome;
use sim::combat::saber::SaberState;
use sim::script::effects::blows;
use sim::{SimId, Snapshot};

use crate::ambience::Heard;
use crate::sim_host::{OwnSeat, PresentedSnapshot, SceneSwap};

/// What has been seen: the scene swap, the snapshot last looked at (its
/// game ms), each character's next event unseen and its saber or melee
/// weapon as last seen.
#[derive(Default)]
pub struct Seen {
    generation: Option<u32>,
    looked_ms: Option<u64>,
    events: BTreeMap<SimId, u16>,
    sabers: BTreeMap<SimId, SaberState>,
}

/// Whether a saber or melee weapon that was `was` began an attack as `now`
/// (JA's move into an attack, or the next of a chain).
fn swung(was: &SaberState, now: &SaberState) -> bool {
    now.current_move.attack() && (!was.current_move.attack() || now.chain != was.chain)
}

/// Whether a grunt is said: always by the player's own (`own`), else when
/// `roll` comes up one in five.
fn says(own: bool, roll: u32) -> bool {
    own || roll.is_multiple_of(5)
}

/// One of `lines` by `roll`.
fn pick(lines: &[usize], roll: u32) -> usize {
    lines[roll as usize % lines.len()]
}

/// The grunts new in `snapshot` (`me`: the player's own character), as
/// (character, sound-set slot): the swings begun since the snapshot last
/// looked at (none on the first look after a scene swap: a swing already
/// playing then was not seen begin), the blows landed since. A snapshot
/// looked at already brings none.
fn new_grunts(snapshot: &Snapshot, me: Option<SimId>, seen: &mut Seen) -> Vec<(SimId, usize)> {
    let mut said = Vec::new();
    if seen.looked_ms == Some(snapshot.time_ms) {
        return said;
    }
    let after = seen
        .looked_ms
        .replace(snapshot.time_ms)
        .unwrap_or(snapshot.time_ms);
    let roll = |id: SimId, salt: u32, line: u32| crate::view_rng::mix32_salted(id.0 ^ line, salt);
    for entity in &snapshot.entities {
        let Some(fighter) = &entity.fighter else {
            continue;
        };
        let own = me == Some(entity.id);
        let round = fighter.acting.filter(|acting| {
            acting.since > after
                && blows::of(acting.animation).is_some_and(|(kind, _)| kind == blows::SWING)
        });
        let ja = seen
            .sabers
            .insert(entity.id, fighter.saber)
            .is_some_and(|was| swung(&was, &fighter.saber));
        let swing = round
            .map(|acting| acting.since)
            .or(ja.then_some(snapshot.time_ms));
        if let Some(since) = swing {
            let at = since as u32;
            if says(own, roll(entity.id, at, 1)) {
                said.push((
                    entity.id,
                    pick(&slots::ATTACK_GRUNTS, roll(entity.id, at, 2)),
                ));
            }
        }
        let Some(last) = seen.events.insert(entity.id, fighter.next_event) else {
            continue;
        };
        let fresh = fighter.next_event.wrapping_sub(last);
        if fresh > u16::MAX / 2 || !fighter.alive() || fighter.hp <= 0 {
            continue;
        }
        let hurt = fighter.newest().take(usize::from(fresh)).find(|event| {
            event.damage > 0
                && matches!(
                    event.result,
                    Outcome::Hit | Outcome::Critical | Outcome::Automatic
                )
        });
        if let Some(event) = hurt {
            let at = u32::from(event.sequence);
            if says(own, roll(entity.id, at, 3)) {
                said.push((entity.id, pick(&slots::PAIN_GRUNTS, roll(entity.id, at, 4))));
            }
        }
    }
    said
}

/// The grunts of the presented snapshot, heard as sound-set lines.
pub fn grunts(
    presented: Res<PresentedSnapshot>,
    swap: Option<Res<SceneSwap>>,
    seat: Option<Res<OwnSeat>>,
    mut heard: ResMut<Heard>,
    mut seen: Local<Seen>,
) {
    let generation = swap.map(|swap| swap.generation);
    if seen.generation != generation {
        *seen = Seen {
            generation,
            ..Seen::default()
        };
    }
    let current = presented.0.current();
    let me = crate::cast::me(current, seat.map_or(sim::Seat(0), |seat| seat.0));
    for (id, slot) in new_grunts(current, me, &mut seen) {
        heard.0.push(sim::notice::Sound::line(id.0, slot));
    }
}

/// The rules' probes (`kjk sim probes --only grunts`).
#[cfg(feature = "probes")]
pub fn probes() -> sim::probe::Checks {
    use sim::probe::check;
    let rolls = 0..1000u32;
    let others = rolls.clone().filter(|roll| says(false, *roll)).count();
    let own = rolls.clone().filter(|roll| says(true, *roll)).count();
    let attacks: Vec<usize> = (0..3)
        .map(|roll| pick(&slots::ATTACK_GRUNTS, roll))
        .collect();
    let pains: Vec<usize> = (0..2).map(|roll| pick(&slots::PAIN_GRUNTS, roll)).collect();
    vec![
        (
            "the player's own always, others one in five",
            check(
                own == 1000 && others == 200,
                format!("own {own}/1000, others {others}/1000"),
            ),
        ),
        ("a melee attack begun grunts, JA's too", {
            use sim::combat::saber::moves::Move;
            let state = |saber_move, chain| SaberState {
                current_move: saber_move,
                chain,
                ..SaberState::default()
            };
            let ready = state(Move::READY, 0);
            let first = state(Move::A_T2B, 1);
            let chained = state(Move::A_TL2BR, 2);
            let back = state(Move::R_T2B, 2);
            let begun = [
                swung(&ready, &first),
                swung(&first, &first),
                swung(&first, &chained),
                swung(&first, &back),
                swung(&back, &ready),
            ];
            check(
                begun == [true, false, true, false, false],
                format!("into an attack, held, chained, returning, readied: {begun:?}"),
            )
        }),
        (
            "attack grunts 1-3, pain grunts 1-2",
            check(
                attacks == [9, 10, 11] && pains == [12, 13],
                format!("attack slots {attacks:?}, pain slots {pains:?}"),
            ),
        ),
    ]
}
