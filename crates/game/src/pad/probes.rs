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

/// A pad's world as play has it: bevy's input and `SticksPlugin`, fed the
/// messages a controller sends.
struct PadWorld(App);

impl PadWorld {
    fn new() -> PadWorld {
        let mut app = App::new();
        app.add_plugins((bevy::input::InputPlugin, SticksPlugin));
        PadWorld(app)
    }

    fn plug(&mut self, pad: Entity, plugged: bool) {
        use bevy::input::gamepad::{GamepadConnection, GamepadConnectionEvent};
        let connection = if plugged {
            GamepadConnection::Connected {
                name: "probe".into(),
                vendor_id: None,
                product_id: None,
            }
        } else {
            GamepadConnection::Disconnected
        };
        self.0
            .world_mut()
            .write_message(GamepadConnectionEvent::new(pad, connection));
        self.0.update();
    }

    /// Moves `pad`'s right stick across to `x` for a frame; the right stick
    /// play then takes.
    fn across(&mut self, pad: Entity, x: f32) -> f32 {
        use bevy::input::gamepad::{RawGamepadAxisChangedEvent, RawGamepadEvent};
        self.0
            .world_mut()
            .write_message(RawGamepadEvent::Axis(RawGamepadAxisChangedEvent::new(
                pad,
                GamepadAxis::RightStickX,
                x,
            )));
        self.0.update();
        self.0
            .world()
            .get::<Sticks>(pad)
            .map_or(f32::NAN, |sticks| sticks.right.x)
    }
}

/// Through bevy's own pad messages: a throttle parked hard back from the
/// frame after it connects (bevy starts every axis at 0) turns nothing,
/// jittering; brought through the middle it counts; unplugged and plugged
/// in again parked it turns nothing again. A gamepad's stick, its first
/// change small, counts at once out to its edge.
fn parked_axis() -> Result<String, String> {
    let mut world = PadWorld::new();
    let throttle = world.0.world_mut().spawn_empty().id();
    let stick = world.0.world_mut().spawn_empty().id();
    world.plug(throttle, true);
    world.plug(stick, true);
    let parked = [-1.0, -0.98].map(|x| world.across(throttle, x));
    let used = [0.0, 0.6].map(|x| world.across(throttle, x));
    world.plug(throttle, false);
    world.plug(throttle, true);
    let replugged = world.across(throttle, 0.6);
    let moved = [0.1, 0.9].map(|x| world.across(stick, x));
    let near = |a: f32, b: f32| (a - b).abs() < 1e-3;
    check(
        parked == [0.0, 0.0]
            && near(used[0], 0.0)
            && near(used[1], 0.6)
            && replugged == 0.0
            && near(moved[0], 0.1)
            && near(moved[1], 0.9),
        format!(
            "throttle parked {parked:?}, through the middle {used:?}, replugged {replugged}; stick {moved:?}"
        ),
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
