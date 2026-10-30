//! The kit's gate (`gate`): whom a droid's bolt and blast may touch, when
//! its dart holds off, and whom what it does is credited to.

use super::super::super::drivers::probes::{party_world, run, scripts_of};
use super::super::gate::{Fair, Gate, Target};
use crate::Guests;
use crate::combat::FriendlyFire;
use crate::combat::ai::Task;
use crate::combat::fighter::Life;
use crate::combat::sheet::{factions, flags};
use crate::input::Seat;
use crate::module::Repute;
use crate::motion::MotionState;
use crate::probe::{Checks, check};
use crate::snapshot::SimId;
use crate::world::party::{Combatant, Rules};

pub(super) fn probes() -> Checks {
    vec![
        ("fair_cases", cases()),
        ("dueling_member", dueling()),
        ("credit_follows_switch", switched()),
        ("credit_in_the_world", credited()),
    ]
}

const NOW: u64 = 60_000;
const LEADER: SimId = SimId(0);
const FOE: SimId = SimId(1);

fn repute() -> Repute {
    Repute::at_war([factions::HOSTILE_1, factions::PLAYER])
}

fn rules(repute: &Repute) -> Rules<'_> {
    Rules {
        repute: Some(repute.with(&crate::module::UNADJUSTED)),
        friendly_fire: FriendlyFire::Off,
        reach: Rules::REACH,
    }
}

/// The party's leader, standing alone.
fn leader() -> Target {
    Target {
        who: Combatant {
            id: LEADER,
            at: [0.0; 3],
            party: true,
            faction: factions::PLAYER,
            life: Life::Alive,
            target: None,
            task: Task::Idle,
            combat_until: 0,
            struck_by: None,
            plot: false,
        },
        flags: 0,
        talks: false,
    }
}

/// A hostile 10 m off, fighting the leader.
fn foe() -> Target {
    let mut foe = leader();
    foe.who = Combatant {
        id: FOE,
        at: [400.0, 0.0, 0.0],
        party: false,
        faction: factions::HOSTILE_1,
        target: Some(LEADER),
        ..foe.who
    };
    foe
}

fn with(edit: impl Fn(&mut Target)) -> Target {
    let mut foe = foe();
    edit(&mut foe);
    foe
}

/// What the gate says of the foe beside the leader.
fn judged(foe: Target) -> Fair {
    let repute = repute();
    Gate::new(vec![leader(), foe], Some(LEADER), rules(&repute), NOW).fair(FOE)
}

fn cases() -> Result<String, String> {
    let cases = [
        (
            "neutral",
            with(|t| t.who.faction = factions::NEUTRAL),
            Fair::NotHostile,
        ),
        (
            "friendly",
            with(|t| t.who.faction = factions::PLAYER),
            Fair::NotHostile,
        ),
        (
            "hostile, not fighting",
            with(|t| t.who.target = None),
            Fair::NotFighting,
        ),
        ("hostile, fighting", foe(), Fair::Yes),
        ("dead", with(|t| t.who.life = Life::Dead), Fair::NotFighting),
        (
            "min1hp",
            with(|t| t.flags = flags::MIN_ONE_HP),
            Fair::Protected,
        ),
        ("plot", with(|t| t.who.plot = true), Fair::Protected),
        (
            "immortal",
            with(|t| t.flags = flags::IMMORTAL),
            Fair::Protected,
        ),
        (
            "in a conversation",
            with(|t| t.talks = true),
            Fair::Protected,
        ),
    ];
    let wrong: Vec<String> = cases
        .into_iter()
        .filter_map(|(name, foe, wanted)| {
            let got = judged(foe);
            (got != wanted).then(|| format!("{name}: {got:?}, not {wanted:?}"))
        })
        .collect();
    check(wrong.is_empty(), format!("9 cases, wrong: {wrong:?}"))
}

/// The leader fighting a Min1HP foe duels; fighting a plain one it does
/// not, nor does a member no protected foe fights.
fn dueling() -> Result<String, String> {
    let repute = repute();
    let gate = |foe: Target| Gate::new(vec![leader(), foe], Some(LEADER), rules(&repute), NOW);
    let duel = gate(with(|t| t.flags = flags::MIN_ONE_HP)).dueling(LEADER);
    let plain = gate(foe()).dueling(LEADER);
    let elsewhere = gate(with(|t| {
        t.flags = flags::MIN_ONE_HP;
        t.who.target = Some(SimId(7));
    }))
    .dueling(LEADER);
    check(
        duel && !plain && !elsewhere,
        format!("against Min1HP {duel}, a plain foe {plain}, Min1HP fighting another {elsewhere}"),
    )
}

/// `lead` (the leash's and the credit's leader): seat 0's avatar; after
/// `SwitchPlayerCharacter` the body that avatar moved to; the companion a
/// portrait click lends.
fn switched() -> Result<String, String> {
    let still = MotionState::standing([0.0; 3], 0.0);
    let (pc, t3, carth) = (SimId(1), SimId(2), SimId(3));
    let led = |lent, seat_0_on: SimId| {
        super::super::lead(
            lent,
            [pc, t3, carth]
                .into_iter()
                .map(|id| (id, &still, (id == seat_0_on).then_some(Seat(0)))),
        )
        .map(|(id, ..)| id)
    };
    let (alone, after_switch, lent) = (led(None, pc), led(None, t3), led(Some(carth), pc));
    check(
        alone == Some(pc) && after_switch == Some(t3) && lent == Some(carth),
        format!("alone {alone:?}, switched to T3 {after_switch:?}, Carth lent {lent:?}"),
    )
}

/// In a world: seat 0's body, then the companion it lends the lead to.
fn credited() -> Result<String, String> {
    let (mut world, _, [a, _]) = party_world(Guests::Companions, 1)?;
    let before = run(&mut world, &[(0, 0)]);
    let own = before.body(Seat(0)).map(|entity| entity.id);
    let first = world.remote_credit();
    scripts_of(&mut world).state.party.leader = Some(0);
    run(&mut world, &[(0, 0)]);
    let lent = world.remote_credit();
    check(
        own.is_some() && first == own && lent == Some(a),
        format!("seat 0's body {own:?}: credit {first:?}, lent to {a:?}: {lent:?}"),
    )
}
