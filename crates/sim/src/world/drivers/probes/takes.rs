//! The probes of a seat taking a free companion (`Action::Play`) and of
//! the first seat's leader switch onto a guest's companion
//! (`Action::Lead`), on the party world of `super`: seat 0's own
//! character and the two companions `a` and `b`.

use super::*;
use crate::action::Action::{self, Lead, Play, SetLeader};

/// The checks, by name.
pub(super) fn takes() -> crate::probe::Checks {
    vec![
        ("take_free_companion", take_free()),
        ("take_played_refused", take_played()),
        ("lead_onto_guest_refused", lead_onto_guest()),
        ("two_takes_in_one_step", two_takes()),
        ("take_while_downed", take_downed()),
        ("take_from_a_droid", take_from_droid()),
        ("take_in_conversation_refused", take_in_conversation()),
        ("take_kept_across_a_drop", take_kept()),
    ]
}

/// One step of 8 ms: each of `seats` standing, and `actions` (seat,
/// action) sent with them.
fn acting(world: &mut SimWorld, seats: &[u8], actions: &[(u8, Action)]) -> Snapshot {
    let mut input = TickInput::default();
    for &seat in seats {
        input.insert(Seat(seat), command(0));
    }
    for (seat, action) in actions {
        input.act(Seat(*seat), action.clone());
    }
    let tick = world.tick() + 1;
    crate::step(world, tick, &input, 8, StepReason::Authority)
}

/// The bodies `seats` play in `snapshot`, in order.
fn bodies(snapshot: &Snapshot, seats: &[u8]) -> Vec<Option<SimId>> {
    seats
        .iter()
        .map(|seat| snapshot.body(Seat(*seat)).map(|entity| entity.id))
        .collect()
}

/// The seat driving `id` in `snapshot`, if one does.
fn driver(snapshot: &Snapshot, id: SimId) -> Option<Seat> {
    snapshot.get(id)?.fighter?.mind.driver
}

/// Seat 1 drives the first companion and takes the second, which nobody
/// plays: in that step it drives the second, and the first has no driver.
fn take_free() -> Result<String, String> {
    let (mut world, _, [a, b]) = party_world(Guests::Companions, 2)?;
    let before = bodies(&run(&mut world, &[(0, 0), (1, 0)]), &[1]);
    let after = acting(&mut world, &[0, 1], &[(1, Play(b))]);
    let (now, left) = (bodies(&after, &[1]), driver(&after, a));
    check(
        before == [Some(a)] && now == [Some(b)] && left.is_none(),
        format!("seat 1 drove {before:?}, then {now:?} (taking {b:?}); {a:?} driven by {left:?}"),
    )
}

/// Seats 1 and 2 drive the two companions. Seat 1's take of seat 2's
/// companion is refused, and so is its take of seat 0's own character
/// (the main character is never free).
fn take_played() -> Result<String, String> {
    let (mut world, _, [a, b]) = party_world(Guests::Companions, 3)?;
    let start = run(&mut world, &[(0, 0), (1, 0), (2, 0)]);
    let own = start.own(Seat(0)).map(|entity| entity.id);
    let own = own.ok_or("seat 0 has no own character")?;
    let played = bodies(&acting(&mut world, &[0, 1, 2], &[(1, Play(b))]), &[1, 2]);
    let main = bodies(
        &acting(&mut world, &[0, 1, 2], &[(1, Play(own))]),
        &[0, 1, 2],
    );
    check(
        played == [Some(a), Some(b)] && main == [Some(own), Some(a), Some(b)],
        format!("seats 1 and 2 after taking {b:?}: {played:?}; after taking {own:?}: {main:?}"),
    )
}

/// Seat 1 drives the first companion. Seat 0's leader switch onto it is
/// refused, by its body (`Lead`) and by its number (`SetLeader`): no one
/// leads, and seat 1 keeps it.
fn lead_onto_guest() -> Result<String, String> {
    let (mut world, _, [a, _]) = party_world(Guests::Companions, 2)?;
    run(&mut world, &[(0, 0), (1, 0)]);
    let by_body = acting(&mut world, &[0, 1], &[(0, Lead(a))]);
    let by_number = acting(&mut world, &[0, 1], &[(0, SetLeader(0))]);
    let after = run(&mut world, &[(0, 0), (1, 0)]);
    check(
        by_body.leading.is_none()
            && by_number.leading.is_none()
            && bodies(&after, &[1]) == [Some(a)],
        format!(
            "leading after Lead {:?}, after SetLeader {:?}; seat 1 drives {:?} ({a:?})",
            by_body.leading,
            by_number.leading,
            bodies(&after, &[1])
        ),
    )
}

/// Two asks for one companion in a step, decided in seat order:
/// - seats 1 and 2 drive the companions and seat 3 watches; seat 2 goes
///   away, so the second is kept for it and its mind plays it. Seats 1 and
///   3 both take the second in one step: seat 1 gets it, and seat 3 gets
///   the first, which seat 1 left;
/// - seat 1 drives the first companion; seat 0 leads through the second as
///   seat 1 takes it, in one step: seat 0 leads through it, and seat 1
///   keeps the first.
fn two_takes() -> Result<String, String> {
    let (mut world, _, [a, b]) = party_world(Guests::Companions, 4)?;
    let start = bodies(
        &run(&mut world, &[(0, 0), (1, 0), (2, 0), (3, 0)]),
        &[1, 2, 3],
    );
    let away = bodies(&run(&mut world, &[(0, 0), (1, 0), (3, 0)]), &[1, 2, 3]);
    let both = acting(&mut world, &[0, 1, 3], &[(1, Play(b)), (3, Play(b))]);
    let guests = bodies(&both, &[1, 3]);
    let (mut world, _, [a2, b2]) = party_world(Guests::Companions, 2)?;
    run(&mut world, &[(0, 0), (1, 0)]);
    let first = acting(&mut world, &[0, 1], &[(0, Lead(b2)), (1, Play(b2))]);
    let led = (first.leading, bodies(&first, &[1]));
    check(
        start == [Some(a), Some(b), None]
            && away == [Some(a), None, None]
            && guests == [Some(b), Some(a)]
            && led == (Some(b2), vec![Some(a2)]),
        format!(
            "seats 1 to 3 drove {start:?}, seat 2 away {away:?}; both taking {b:?}: seats 1 and 3 {guests:?}; \
             seat 0 leading through {b2:?} as seat 1 takes it: leading {:?}, seat 1 {:?}",
            led.0, led.1
        ),
    )
}

/// Seat 1's companion goes down. Seat 1 keeps it for 30 steps (nobody
/// moves a player off a downed character), then takes the second, which
/// stands.
fn take_downed() -> Result<String, String> {
    const WAITED: usize = 30;
    let (mut world, _, [a, b]) = party_world(Guests::Companions, 2)?;
    run(&mut world, &[(0, 0), (1, 0)]);
    poke(&mut world, a, |fighter, _| fighter.life = Life::Downed);
    let mut kept = true;
    for _ in 0..WAITED {
        kept &= bodies(&run(&mut world, &[(0, 0), (1, 0)]), &[1]) == [Some(a)];
    }
    let down = world.snapshot().get(a).and_then(|entity| entity.fighter);
    let down = down.is_some_and(|fighter| fighter.downed());
    let taken = bodies(&acting(&mut world, &[0, 1], &[(1, Play(b))]), &[1]);
    check(
        kept && down && taken == [Some(b)],
        format!(
            "seat 1 kept its downed {a:?} for {WAITED} steps: {kept} (still down {down}); then took {taken:?}"
        ),
    )
}

/// Steps `world`, seats `seats` standing, until the fade its module loads
/// under lifts (the droids wait it out, `remote::holds`).
fn settled(world: &mut SimWorld, seats: &[(u8, i8)]) -> Snapshot {
    let mut last = run(world, seats);
    for _ in 0..600 {
        if !last.fade.is_some_and(|fade| fade.out) {
            break;
        }
        last = run(world, seats);
    }
    last
}

/// Five seats, three characters: seats 1 and 2 drive the companions, and
/// seats 3 and 4 fly droids. Seat 2 goes away, so its companion's mind
/// plays it. Seat 3 takes that companion: it drives it, and its droid
/// docks, while seat 4 flies on.
fn take_from_droid() -> Result<String, String> {
    let (mut world, _, [a, b]) = party_world(Guests::Companions, 5)?;
    let all = [(0, 0), (1, 0), (2, 0), (3, 0), (4, 0)];
    settled(&mut world, &all);
    let present = [(0, 0), (1, 0), (3, 0), (4, 0)];
    let flying = run(&mut world, &present);
    let droids = |snapshot: &Snapshot| [3, 4].map(|seat| snapshot.remote(Seat(seat)).is_some());
    acting(&mut world, &[0, 1, 3, 4], &[(3, Play(b))]);
    let after = run(&mut world, &present);
    check(
        bodies(&flying, &[1, 2, 3]) == [Some(a), None, None]
            && droids(&flying) == [true; 2]
            && bodies(&after, &[1, 3]) == [Some(a), Some(b)]
            && droids(&after) == [false, true],
        format!(
            "seats 1 to 3 played {:?}, droids of seats 3 and 4 {:?}; after seat 3 took {b:?}: seats 1 and 3 {:?}, droids {:?}",
            bodies(&flying, &[1, 2, 3]),
            droids(&flying),
            bodies(&after, &[1, 3]),
            droids(&after)
        ),
    )
}

/// Seat 1's companion holds a conversation with the second companion (the
/// one the conversation's scripts run for). Seat 1's take of the second is
/// refused while it lasts.
fn take_in_conversation() -> Result<String, String> {
    use crate::script::dialogue::Start;
    use script_ncs::vm::ObjectId;
    const DIALOG: &str = "probe_take_talk";
    let (level, numbers) = ground()?;
    let dialogues = [(
        DIALOG.to_string(),
        crate::script::dialogue_probes::talking(2, ""),
    )]
    .into();
    let scripts = library().with_dialogues(dialogues, std::collections::BTreeMap::new());
    let ground = (level.with_scripts(scripts), numbers);
    let (mut world, _, [a, b]) = party_on(ground, Guests::Companions, 2)?;
    let seats = [(0, 0), (1, 0)];
    settled(&mut world, &seats);
    let mut state = world.script_state().clone();
    state.talk.asked.push(Start {
        owner: ObjectId(b.0),
        pc: ObjectId(a.0),
        asker: ObjectId(a.0),
        dialog: DIALOG.into(),
        bark: false,
        cue: None,
    });
    world.set_script_state(state);
    run(&mut world, &seats);
    let talking = world.script_state().talk.holding(ObjectId(a.0)).is_some();
    let after = bodies(&acting(&mut world, &[0, 1], &[(1, Play(b))]), &[1]);
    check(
        talking && after == [Some(a)] && driver(&world.snapshot(), b).is_none(),
        format!(
            "seat 1's {a:?} in a conversation: {talking}; after taking {b:?} seat 1 drives {after:?}"
        ),
    )
}

/// Seat 1 takes the second companion and goes away for 10 steps. Back, it
/// drives the second again, not the first, which comes first in the
/// party's order.
fn take_kept() -> Result<String, String> {
    const AWAY: usize = 10;
    let (mut world, _, [_, b]) = party_world(Guests::Companions, 2)?;
    run(&mut world, &[(0, 0), (1, 0)]);
    let taken = bodies(&acting(&mut world, &[0, 1], &[(1, Play(b))]), &[1]);
    for _ in 0..AWAY {
        run(&mut world, &[(0, 0)]);
    }
    let away = driver(&world.snapshot(), b);
    run(&mut world, &[(0, 0), (1, 0)]);
    let back = bodies(&run(&mut world, &[(0, 0), (1, 0)]), &[1]);
    check(
        taken == [Some(b)] && away.is_none() && back == [Some(b)],
        format!(
            "seat 1 took {taken:?}; away, {b:?} driven by {away:?}; back, seat 1 drives {back:?}"
        ),
    )
}
