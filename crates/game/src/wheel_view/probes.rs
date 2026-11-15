//! The probes of `wheel_view` (`kjk sim probes`, group "power_wheel").

use super::*;
use sim::probe::{Checks, check};

pub fn probes() -> Checks {
    vec![
        ("entry_at", entry_at_table()),
        ("time_scale", time_scale_table()),
        ("release_uses_entry", release_uses_entry()),
        (
            "release_with_nothing_uses_nothing",
            release_with_nothing_uses_nothing(),
        ),
        ("arc_choice_is_remembered", arc_choice_is_remembered()),
        ("closes_on_interruption", closes_on_interruption()),
        ("second_seat_ends_slow", second_seat_ends_slow()),
        ("the highlight holds near its edge", highlight_holds()),
    ]
}

/// The body the wheel opens for.
const ME: SimId = SimId(3);
/// Eight entries with no arc.
const PLAIN: [(usize, usize); 8] = [(0, 0); 8];
/// Eight entries. Entry 2 is a kind of item with an arc of 5 items, and the
/// character uses the third of them now.
const STIMS: [(usize, usize); 8] = [
    (0, 0),
    (0, 0),
    (5, 2),
    (0, 0),
    (0, 0),
    (0, 0),
    (0, 0),
    (0, 0),
];

/// A frame with the wheel's key `held`, the body alive and nothing else
/// up, with the mouse still.
fn frame(held: bool, arcs: &[(usize, usize)]) -> Frame<'_> {
    Frame {
        held,
        screen: false,
        talk: false,
        movie: false,
        body: Some((ME, true)),
        mouse: Vec2::ZERO,
        stick: Vec2::ZERO,
        steps: 0,
        arcs,
        seconds: 0.016,
    }
}

/// The pointer `length` H long at `degrees` clockwise from straight up.
fn toward(degrees: f32, length: f32) -> Vec2 {
    let turn = degrees.to_radians();
    Vec2::new(turn.sin(), turn.cos()) * length
}

/// A frame with the key held that moves the mouse so the pointer ends at
/// `to`.
fn pointing<'a>(wheel: &Wheel, to: Vec2, arcs: &'a [(usize, usize)]) -> Frame<'a> {
    Frame {
        mouse: to - wheel.pointer,
        ..frame(true, arcs)
    }
}

/// Opens the wheel and points at the entry at `degrees`. Returns the
/// releases seen, which should be none.
fn opened_at(wheel: &mut Wheel, degrees: f32, arcs: &[(usize, usize)]) -> Vec<Release> {
    let opened = wheel.frame(&frame(true, arcs));
    let pointed = wheel.frame(&pointing(wheel, toward(degrees, 0.2), arcs));
    opened.into_iter().chain(pointed).collect()
}

fn entry_at_table() -> Result<String, String> {
    let cases = [
        ("up", Vec2::new(0.0, 0.2), 8, Some(0)),
        ("right", Vec2::new(0.2, 0.0), 8, Some(2)),
        ("down", Vec2::new(0.0, -0.2), 8, Some(4)),
        ("left", Vec2::new(-0.2, 0.0), 8, Some(6)),
        ("inside the dead zone", Vec2::new(0.03, 0.03), 8, None),
        ("no entries", Vec2::new(0.0, 0.2), 0, None),
    ];
    let wrong: Vec<String> = cases
        .iter()
        .filter(|(_, pointer, entries, want)| entry_at(*pointer, *entries) != *want)
        .map(|(name, pointer, entries, _)| format!("{name}: {:?}", entry_at(*pointer, *entries)))
        .collect();
    check(
        wrong.is_empty(),
        format!("{} cases, wrong {wrong:?}", cases.len()),
    )
}

fn time_scale_table() -> Result<String, String> {
    let cases = [(true, 1, SLOW), (true, 2, 1.0), (false, 1, 1.0)];
    let seen = cases.map(|(open, seats, _)| time_scale(open, seats));
    check(
        seen.iter()
            .zip(&cases)
            .all(|(seen, (.., want))| seen == want),
        format!("open alone, open with two seats, shut alone: {seen:?}"),
    )
}

/// Pointing right at 8 entries and letting go uses entry 2.
fn release_uses_entry() -> Result<String, String> {
    let mut wheel = Wheel::default();
    let mut seen = opened_at(&mut wheel, 90.0, &PLAIN);
    seen.extend(wheel.frame(&frame(false, &PLAIN)));
    let want = [Release {
        at: 2,
        arc_at: None,
    }];
    check(seen == want, format!("releases {seen:?}"))
}

/// Opening the wheel and letting go with the pointer still at the centre
/// uses nothing.
fn release_with_nothing_uses_nothing() -> Result<String, String> {
    let mut wheel = Wheel::default();
    let seen: Vec<Release> = [true, true, false]
        .into_iter()
        .filter_map(|held| wheel.frame(&frame(held, &PLAIN)))
        .collect();
    check(seen.is_empty(), format!("releases {seen:?}"))
}

/// Pointing at the stims (entry 2) opens their arc on the stim in use, the
/// third. Moving the pointer out into the arc one step clockwise picks the
/// fourth, and letting go uses it. The use sends the pick first, waits for
/// the snapshot to show it, and then holds the stims' button.
fn arc_choice_is_remembered() -> Result<String, String> {
    let mut wheel = Wheel::default();
    let mut seen = opened_at(&mut wheel, 90.0, &STIMS);
    let on_ring = wheel.arc_at;
    seen.extend(wheel.frame(&pointing(&wheel, toward(90.0 + ARC_STEP, 0.33), &STIMS)));
    seen.extend(wheel.frame(&frame(false, &STIMS)));
    let want = [Release {
        at: 2,
        arc_at: Some(3),
    }];
    let row = 42;
    let mut due = Due {
        content: Content::Item(Shelf::Stim),
        choices: Vec::new(),
        usable: true,
        hand: 0,
        target: None,
        pick: Some((Shelf::Stim, row)),
        sent: false,
        waited: 0.0,
        tap: None,
    };
    let first = due.step(false, 0.01);
    let second = due.step(true, 0.01);
    let picked_first = first == (vec![Action::PickItem { row }], BarUse::default());
    let used_after = second.0.is_empty() && second.1.buttons == Shelf::Stim.button();
    check(
        on_ring == Some(2) && seen == want && picked_first && used_after,
        format!(
            "arc item on the ring {on_ring:?}, releases {seen:?}, first frame {first:?}, once picked {second:?}"
        ),
    )
}

/// What comes up while the wheel is open, by name.
type Interruption = (&'static str, fn(&mut Frame));

const INTERRUPTIONS: [Interruption; 5] = [
    ("a screen", |frame| frame.screen = true),
    ("a conversation", |frame| frame.talk = true),
    ("a movie", |frame| frame.movie = true),
    ("a leader change", |frame| {
        frame.body = Some((SimId(4), true))
    }),
    ("the character goes down", |frame| {
        frame.body = Some((ME, false));
    }),
];

/// The wheel open on entry 2 closes when each interruption comes up, with
/// its key still held. It uses nothing, then or when the key is let go, and
/// the clock is back at full speed.
fn closes_on_interruption() -> Result<String, String> {
    let wrong: Vec<String> = INTERRUPTIONS
        .iter()
        .filter_map(|(name, interrupt)| {
            let mut wheel = Wheel::default();
            let mut seen = opened_at(&mut wheel, 90.0, &PLAIN);
            let mut interrupted = frame(true, &PLAIN);
            interrupt(&mut interrupted);
            seen.extend(wheel.frame(&interrupted));
            let shut = !wheel.open;
            seen.extend(wheel.frame(&frame(true, &PLAIN)));
            seen.extend(wheel.frame(&frame(false, &PLAIN)));
            let scale = time_scale(wheel.open, 1);
            let right = shut && !wheel.open && seen.is_empty() && scale == 1.0;
            (!right).then(|| format!("{name}: shut {shut}, releases {seen:?}, clock {scale}"))
        })
        .collect();
    check(
        wrong.is_empty(),
        format!("{} interruptions, wrong {wrong:?}", INTERRUPTIONS.len()),
    )
}

/// The wheel open in a session of one seat slows the clock. Once the
/// session has a second seat, the clock runs at full speed while the wheel
/// stays open.
fn second_seat_ends_slow() -> Result<String, String> {
    let mut wheel = Wheel::default();
    wheel.frame(&frame(true, &PLAIN));
    let seen = [time_scale(wheel.open, 1), time_scale(wheel.open, 2)];
    check(
        wheel.open && seen == [SLOW, 1.0],
        format!(
            "open {}, clock alone then with two seats {seen:?}",
            wheel.open
        ),
    )
}

/// With 8 entries, entry 0's sector ends 22.5 degrees right of straight
/// up. A pointer 4 degrees past that edge keeps entry 0 highlighted, and a
/// pointer 6 degrees past it highlights entry 1.
fn highlight_holds() -> Result<String, String> {
    let mut wheel = Wheel::default();
    opened_at(&mut wheel, 0.0, &PLAIN);
    let mut at = |degrees: f32| {
        wheel.frame(&pointing(&wheel, toward(degrees, 0.2), &PLAIN));
        wheel.at
    };
    let seen = [at(0.0), at(22.5 + 4.0), at(22.5 + 6.0)];
    check(
        seen == [Some(0), Some(0), Some(1)],
        format!("straight up, 4 and 6 degrees past the edge: {seen:?}"),
    )
}
