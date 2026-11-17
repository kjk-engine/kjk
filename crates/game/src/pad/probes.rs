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
        ("target cycle", target_cycle()),
        ("item step", item_step()),
        ("wheel bumpers", wheel_bumpers()),
        ("block on LB", block_on_lb()),
        ("stick after the wheel", stick_after_wheel()),
        ("control tokens", control_tokens()),
        ("keys held through a screen", held_through_screen()),
    ]
}

/// The frames of a key pressed while a screen is up and still held as the
/// screen closes: each frame's name, whether a screen is up, whether the
/// key is down, and whether it went down this frame.
const THROUGH_SCREEN: [(&str, bool, bool, bool); 5] = [
    ("pressed on a screen", true, true, true),
    ("held on the screen", true, true, false),
    ("the screen closed, still held", false, true, false),
    ("let go", false, false, false),
    ("pressed again", false, true, true),
];

/// W (Forward) held through a screen comes through to play on the first
/// frame after the screen closes, so the character moves. Bar slot 1's key
/// held the same way does nothing until it is let go and pressed again
/// (`gate_keys`).
fn held_through_screen() -> Result<String, String> {
    let cases = [
        (Action::Forward, [false, false, true, false, true]),
        (BAR[0], [false, false, false, false, true]),
    ];
    let open = |screen| {
        let taken = if screen { Taken::Screens } else { Taken::Play };
        open_keys(taken, KeyMap::defaults())
    };
    let wrong: Vec<String> = cases
        .iter()
        .filter_map(|(action, want)| {
            let key = bit(*action);
            let on = |on: bool| if on { key } else { 0 };
            let mut kept = 0;
            let seen = THROUGH_SCREEN.map(|(_, screen, down, went)| {
                gate_keys(open(screen), &mut kept, on(down), on(went)).0 == key
            });
            (seen != *want).then(|| format!("{action:?} held in play {seen:?}"))
        })
        .collect();
    check(
        wrong.is_empty(),
        format!(
            "{} keys over {} frames, wrong {wrong:?}",
            cases.len(),
            THROUGH_SCREEN.len()
        ),
    )
}

/// The games' text asks for the medical button with `PersonalMedicalAct`
/// and for the mines button with `PersonalMinesAct`. Each token names the
/// key of the bar slot that holds medpacs or mines. With no such slot, or no
/// bar known, it names the power wheel's key. The power button's token
/// always names the wheel's key, and the A button's names Use. The
/// minigames' shoot key names Attack, which shifts a swoop's gears.
fn control_tokens() -> Result<String, String> {
    let mut bar = Bar::default();
    bar.put(2, Content::Item(Shelf::Medpac));
    bar.put(4, Content::Mine);
    let empty = Bar::default();
    let cases = [
        ("PersonalMedicalAct", Some(bar), Some(Action::Bar3)),
        ("PersonalMinesAct", Some(bar), Some(Action::Bar5)),
        ("PersonalMedicalAct", Some(empty), Some(Action::Wheel)),
        ("PersonalMinesAct", None, Some(Action::Wheel)),
        ("PersonalPowerAct", Some(bar), Some(Action::Wheel)),
        ("abutton", None, Some(Action::Use)),
        ("MGshoot", None, Some(Action::Attack)),
        ("NoSuchToken", Some(bar), None),
    ];
    let wrong: Vec<_> = cases
        .iter()
        .filter_map(|(token, bar, wanted)| {
            let got = token_action(token, bar.as_ref());
            (got != *wanted).then(|| format!("{token}: {got:?}, wanted {wanted:?}"))
        })
        .collect();
    check(
        wrong.is_empty(),
        format!("{} tokens, wrong {wrong:?}", cases.len()),
    )
}

/// Through bevy's own pad messages: with the power wheel's key (RB) held
/// and the wheel open, LB steps the wheel on by one entry and the wheel's
/// key stays held. LB pressed while the wheel is open neither blocks nor
/// is a layer, so A pressed with it jumps and does not use the first bar
/// slot. The wheel's badges show nothing beside the back arrow and LB
/// beside the on arrow.
fn wheel_bumpers() -> Result<String, String> {
    use GamepadButton::South;
    let (mut world, pad) = PadWorld::sampling();
    world.button(pad, RB, true);
    world.0.insert_resource(crate::wheel_view::Wheel::opened());
    world.button(pad, SHIFT, true);
    let stepped = world.actions();
    let step = (
        stepped.wheel_step,
        stepped.held(Action::Wheel),
        stepped.held(Action::Block),
    );
    world.button(pad, South, true);
    let actions = world.actions();
    let layered = (actions.held(Action::Jump), actions.held(BAR[0]));
    let badges = wheel_badges(Some(&actions));
    let wanted = [String::new(), "kjkpad_xbox_lefttrigger".into()];
    check(
        step == (1, true, false) && layered == (true, false) && badges.as_ref() == Some(&wanted),
        format!(
            "(step, wheel key held, block held) for LB {step:?}, (jump, bar slot 1) for A with LB held {layered:?}, badges {badges:?}"
        ),
    )
}

/// Each frame of `block_on_lb`: its name, the button pressed or let go, and
/// whether Block, Attack and the third bar slot (LB with X) are then held.
const BLOCK_FRAMES: [(&str, GamepadButton, bool, [bool; 3]); 9] = {
    use GamepadButton::{LeftTrigger as Lb, RightTrigger2 as Rt, West as X};
    [
        ("LB pressed", Lb, true, [true, false, false]),
        (
            "the right trigger pressed with LB",
            Rt,
            true,
            [true, true, false],
        ),
        ("the right trigger let go", Rt, false, [true, false, false]),
        ("X pressed with LB", X, true, [false, false, true]),
        ("X let go, LB still held", X, false, [false, false, false]),
        ("LB let go", Lb, false, [false, false, false]),
        ("LB pressed again", Lb, true, [true, false, false]),
        ("the right trigger pressed", Rt, true, [true, true, false]),
        (
            "LB let go with the trigger held",
            Lb,
            false,
            [false, true, false],
        ),
    ]
};

/// Through bevy's own pad messages and play's reading of them (`sample`):
/// LB held alone blocks, and the right trigger pressed with it attacks while
/// the block stays held. Once LB has made a chord with X (the third bar
/// slot), it does not block again until it is let go and pressed again,
/// even after X is let go (`BLOCK_FRAMES`).
fn block_on_lb() -> Result<String, String> {
    let (mut world, pad) = PadWorld::sampling();
    let watched = [Action::Block, Action::Attack, BAR[2]];
    let wrong: Vec<String> = BLOCK_FRAMES
        .iter()
        .filter_map(|(name, button, down, wanted)| {
            world.button(pad, *button, *down);
            let actions = world.actions();
            let seen = watched.map(|action| actions.held(action));
            (seen != *wanted).then(|| format!("{name}: (block, attack, slot 3) {seen:?}"))
        })
        .collect();
    check(
        wrong.is_empty(),
        format!("{} frames, wrong {wrong:?}", BLOCK_FRAMES.len()),
    )
}

/// Through bevy's own pad messages: the right stick pushed to the side
/// while the power wheel is open moves the wheel's pointer. When the wheel
/// closes with the stick still pushed, the stick turns nothing. The stick
/// turns the camera again once it has been back in its middle.
fn stick_after_wheel() -> Result<String, String> {
    let (mut world, pad) = PadWorld::sampling();
    world.0.insert_resource(crate::wheel_view::Wheel::opened());
    let look = |world: &mut PadWorld, x: f32| {
        world.across(pad, x);
        // Play reads the stick on the frame after it moved.
        world.0.update();
        world.actions().aim.x
    };
    // A stick's first change is small, so that it does not read as parked.
    look(&mut world, 0.1);
    let open = look(&mut world, 0.9);
    world.0.insert_resource(crate::wheel_view::Wheel::default());
    let closed = [0.9, 0.5].map(|x| look(&mut world, x));
    let rested = look(&mut world, 0.0);
    let again = look(&mut world, 0.9);
    check(
        open > 0.5 && closed == [0.0, 0.0] && rested == 0.0 && again > 0.5,
        format!(
            "stick across with the wheel open {open}, after it closed {closed:?}, in the middle {rested}, across again {again}"
        ),
    )
}

/// Through bevy's own pad messages and play's reading of them (`sample`):
/// while bar slot 1's chord (LB with A) holds medpacs with kinds to choose
/// among, the D-pad's right steps on and its left steps back. Slot 1 stays
/// held after LB is let go. Neither step moves the target's action, though
/// the D-pad's left and right alone move it otherwise.
fn item_step() -> Result<String, String> {
    use GamepadButton::{DPadLeft, DPadRight, South};
    let (mut world, pad) = PadWorld::sampling();
    let slot = crate::settings::BAR[0];
    world
        .0
        .insert_resource(crate::pick_view::Picking::choosing_medpacs(slot));
    world.button(pad, SHIFT, true);
    world.button(pad, South, true);
    world.button(pad, SHIFT, false);
    let mut step = |button| {
        world.button(pad, button, true);
        let actions = world.actions();
        world.button(pad, button, false);
        (actions.pick_step, actions.held(slot), actions.cycle)
    };
    let seen = [step(DPadRight), step(DPadLeft)];
    check(
        seen == [(1, true, 0), (-1, true, 0)],
        format!("(step, slot 1 held, target cycle) on the right and the left: {seen:?}"),
    )
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
pub(crate) struct PadWorld(App);

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

    /// A pad's world that also reads play's `Actions` each frame (`sample`),
    /// with the default chords, and one pad plugged in.
    pub(crate) fn sampling() -> (PadWorld, Entity) {
        let mut world = PadWorld::new();
        world
            .0
            .insert_resource(crate::scene::Headless(false))
            .init_resource::<Actions>()
            .init_resource::<PadPresses>()
            .init_resource::<Time>()
            .add_systems(PreUpdate, sample.after(bevy::input::InputSystems));
        let pad = world.0.world_mut().spawn_empty().id();
        world.plug(pad, true);
        (world, pad)
    }

    /// Play's actions as the last frame read them.
    pub(crate) fn actions(&self) -> Actions {
        *self.0.world().resource::<Actions>()
    }

    /// Presses or lets go `button` on `pad` for a frame.
    pub(crate) fn button(&mut self, pad: Entity, button: GamepadButton, down: bool) {
        use bevy::input::gamepad::{RawGamepadButtonChangedEvent, RawGamepadEvent};
        let value = if down { 1.0 } else { 0.0 };
        self.0.world_mut().write_message(RawGamepadEvent::Button(
            RawGamepadButtonChangedEvent::new(pad, button, value),
        ));
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

/// Each default chord reads back by its name. Each bumper alone, and RB
/// with LB, read as chords. LB with itself, a chord with RB as the layer
/// (which older files keep), and nonsense read as none.
fn chord_names() -> Result<String, String> {
    let wrong: Vec<String> = PAD
        .iter()
        .chain(&SHARED)
        .map(|(_, chord)| *chord)
        .filter(|chord| Chord::named(&chord.name()) != Some(*chord))
        .map(Chord::name)
        .collect();
    let read = [
        ("LeftTrigger", Some(alone(SHIFT))),
        ("RightTrigger", Some(alone(RB))),
        ("LB+RightTrigger", Some(shifted(RB))),
        ("LB+LeftTrigger", None),
        ("RB+South", None),
        ("Nonsense", None),
    ];
    let misnamed: Vec<&str> = read
        .iter()
        .filter(|(name, wanted)| Chord::named(name) != *wanted)
        .map(|(name, _)| *name)
        .collect();
    check(
        wrong.is_empty() && misnamed.is_empty(),
        format!("defaults misread {wrong:?}, names read wrongly {misnamed:?}"),
    )
}

/// No two actions share a chord but where their keys do (or a screen takes
/// the chord's other meaning), and none is the target's cycle. No LB chord
/// is on the attack's button, so the attack still works with LB held.
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
    }
    if let Some(attack) = map
        .chord(UNDER_BLOCK)
        .filter(|attack| map.on_lb(attack.button))
    {
        clashes.push(format!("an LB chord on the attack's {}", attack.name()));
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

/// Through bevy's own pad messages and play's reading of them (`sample`): the
/// D-pad's right alone moves the target's chosen action on, and its left
/// alone moves it back, the same on every object. With LB held the D-pad's
/// right moves nothing. Once that pad has played, the target block's hint
/// names those two buttons as its own. While the keys play there is no
/// hint.
fn target_cycle() -> Result<String, String> {
    use GamepadButton::{DPadLeft, DPadRight};
    let (mut world, pad) = PadWorld::sampling();
    let keys_hint = cycle_badges(world.0.world().get_resource::<Actions>());
    let cycle = |world: &mut PadWorld, shift: bool, button: GamepadButton| {
        if shift {
            world.button(pad, SHIFT, true);
        }
        world.button(pad, button, true);
        let moved = world.actions().cycle;
        world.button(pad, button, false);
        if shift {
            world.button(pad, SHIFT, false);
        }
        moved
    };
    let moved = [
        cycle(&mut world, false, DPadRight),
        cycle(&mut world, false, DPadLeft),
        cycle(&mut world, true, DPadRight),
    ];
    let pad_hint = cycle_badges(world.0.world().get_resource::<Actions>());
    let wanted = ["kjkpad_xbox_dpadleft", "kjkpad_xbox_dpadright"].map(String::from);
    check(
        moved == [1, -1, 0] && keys_hint.is_none() && pad_hint.as_ref() == Some(&wanted),
        format!("moved {moved:?}; hint with the keys {keys_hint:?}, with the pad {pad_hint:?}"),
    )
}
