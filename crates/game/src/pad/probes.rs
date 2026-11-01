//! The probes of `pad` (`kjk sim probes`, group "pad").

use super::*;
use sim::probe::check;

pub(super) fn checks() -> sim::probe::Checks {
    vec![
        ("stick rest", stick_rest()),
        ("stick step", stick_step()),
        ("keys over stick", keys_over_stick()),
        ("chord names", chord_names()),
        ("chords apart", chords_apart()),
        ("pad names", pad_names()),
    ]
}

/// The dead middle is still, the edge is full, the push between is scaled.
fn stick_rest() -> Result<String, String> {
    let still = rest(Vec2::new(0.1, 0.05), 0.15);
    let edge = rest(Vec2::new(0.0, 0.97), 0.15).y;
    let half = rest(Vec2::new(0.55, 0.0), 0.15).x;
    check(
        still == Vec2::ZERO && (edge - 1.0).abs() < 1e-6 && (half - 0.5).abs() < 1e-6,
        format!("middle {still}, edge {edge}, half-way {half}"),
    )
}

/// A light push walks slowly, the walking legs below `WALK_AXIS`; the edge
/// runs.
fn stick_step() -> Result<String, String> {
    let pushed = |y: f32| {
        step(
            &Actions {
                stick: Vec2::new(0.0, y),
                ..Actions::default()
            },
            false,
        )
    };
    let (light, half, full, back) = (pushed(0.01).0, pushed(0.5).0, pushed(1.0).0, pushed(-1.0));
    check(
        (light, half, full, back) == (26, 76, 127, (-127, 0)) && walks(light, 0) && !walks(full, 0),
        format!("light {light}, half {half}, full {full}, back {back:?}"),
    )
}

/// A movement key held beats the stick, at the walk's 64 with walk held.
fn keys_over_stick() -> Result<String, String> {
    let mut actions = Actions {
        stick: Vec2::new(1.0, 0.0),
        ..Actions::default()
    };
    actions.held |= bit(Action::Forward);
    let axes = step(&actions, true);
    check(axes == (64, 0), format!("{axes:?}"))
}

/// Each default chord reads back by its name; the shift button and
/// nonsense name none.
fn chord_names() -> Result<String, String> {
    let wrong: Vec<String> = PAD
        .iter()
        .chain(&SHARED)
        .map(|(_, chord)| *chord)
        .filter(|chord| Chord::named(&chord.name()) != Some(*chord))
        .map(Chord::name)
        .collect();
    let refused = Chord::named("LB+LeftTrigger").is_none() && Chord::named("Nonsense").is_none();
    check(
        wrong.is_empty() && refused,
        format!("misread {wrong:?}, shift and nonsense refused: {refused}"),
    )
}

/// No two actions share a chord but where their keys do (or a screen takes
/// the chord's other meaning), none is
/// the target's cycle, and none is the shift button.
fn chords_apart() -> Result<String, String> {
    let shared = crate::settings::SHARES;
    let map = PadMap::default();
    let mut clashes = Vec::new();
    for (a, x) in &map.0 {
        for (b, y) in map.0.iter().filter(|(b, y)| a < *b && x == *y) {
            if !shared
                .iter()
                .any(|pair| pair.contains(a) && pair.contains(b))
            {
                clashes.push(format!("{a:?} and {b:?} on {}", y.name()));
            }
        }
        if CYCLE.iter().any(|(chord, _)| chord == x) {
            clashes.push(format!("{a:?} on the cycle"));
        }
        if x.button == SHIFT {
            clashes.push(format!("{a:?} on {}", x.name()));
        }
    }
    check(
        clashes.is_empty(),
        format!("{} chords; clashes {clashes:?}", map.0.len()),
    )
}

/// Each kind of pad names the shifted A by its own buttons.
fn pad_names() -> Result<String, String> {
    let chord = shifted(GamepadButton::South);
    let names = [
        PadKind::Xbox,
        PadKind::PlayStation,
        PadKind::Nintendo,
        PadKind::Deck,
    ]
    .map(|kind| kind.chord(chord));
    check(
        names == ["LB+A", "L1+Cross", "L+B", "L1+A"],
        format!("{names:?}"),
    )
}
