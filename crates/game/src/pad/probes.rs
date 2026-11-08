//! The probes of `pad` (`kjk sim probes`, group "pad").

use super::*;
use sim::probe::check;

pub(super) fn checks() -> sim::probe::Checks {
    vec![
        ("stick rest", stick_rest()),
        ("parked axis", parked_axis()),
        ("stick step", stick_step()),
        ("keys over stick", keys_over_stick()),
        ("stick directed", stick_directed()),
        ("stick strafes", stick_strafes()),
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

/// An axis first heard parked off its middle (a throttle) reads as the
/// middle until it comes back there, then counts; one first heard in the
/// middle (a gamepad's stick) counts at once, out to its edge; an axis not
/// reported (a pad reconnecting) is heard afresh.
fn parked_axis() -> Result<String, String> {
    let heard = |readings: &[Option<f32>]| {
        let mut settle = Settle::default();
        readings
            .iter()
            .map(|reading| settle.read(*reading, 0.15))
            .collect::<Vec<f32>>()
    };
    let throttle = heard(&[Some(-1.0), Some(-0.98), Some(0.6), Some(0.05), Some(0.6)]);
    let stick = heard(&[Some(0.02), Some(0.5), Some(1.0), Some(1.0)]);
    let unplugged = heard(&[Some(0.0), None, Some(0.8)]);
    check(
        throttle == [0.0, 0.0, 0.0, 0.05, 0.6]
            && stick == [0.02, 0.5, 1.0, 1.0]
            && unplugged == [0.0, 0.0, 0.0],
        format!("throttle {throttle:?}, stick {stick:?}, unplugged {unplugged:?}"),
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

/// With a roll's, a flip's or an attack's button held the stick snaps to
/// the keys' eight ways: left with a little forward rolls left, forward
/// with a little side flips forward, a diagonal stays one; with none held
/// it moves as pushed.
fn stick_directed() -> Result<String, String> {
    let read = |x: f32, y: f32, held: Option<Action>| {
        let mut actions = Actions {
            stick: Vec2::new(x, y),
            ..Actions::default()
        };
        if let Some(action) = held {
            actions.held |= bit(action);
        }
        step(&actions, false)
    };
    let rolled = read(-0.95, 0.25, Some(Action::Crouch));
    let flipped = read(0.2, 0.95, Some(Action::Jump));
    let diagonal = read(0.7, 0.7, Some(Action::Attack));
    let free = read(-0.95, 0.25, None);
    check(
        rolled.0 == 0
            && rolled.1 < -100
            && flipped.1 == 0
            && flipped.0 > 100
            && diagonal.0 == diagonal.1
            && diagonal.0 > 100
            && free.0 > 0
            && free.1 < 0,
        format!("rolled {rolled:?}, flipped {flipped:?}, diagonal {diagonal:?}, free {free:?}"),
    )
}

/// Sideways and a little down strafes at the push's speed; further down
/// backpedals on a diagonal, straight down walks back, a little up keeps
/// its forward.
fn stick_strafes() -> Result<String, String> {
    let read = |x: f32, y: f32| {
        step(
            &Actions {
                stick: Vec2::new(x, y),
                ..Actions::default()
            },
            false,
        )
    };
    let strafe = read(0.95, -0.25);
    let diagonal = read(0.7, -0.7);
    let back = read(0.0, -1.0);
    let up = read(0.95, 0.25);
    check(
        strafe.0 == 0
            && strafe.1 > 100
            && diagonal.0 < 0
            && diagonal.1 > 0
            && back.0 < -100
            && up.0 > 0,
        format!("strafe {strafe:?}, diagonal {diagonal:?}, back {back:?}, up {up:?}"),
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
