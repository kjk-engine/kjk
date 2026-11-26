//! The probes of photo mode: its camera (the approved test `photo_camera`),
//! and how it holds the world, takes play's keys and calls a break in
//! co-op (`kjk sim probes`, group "photo mode").

use super::*;
use sim::probe::{Checks, check};
use sim::{Command, StepReason, TickInput, WorldConfig};

/// The frame the probes step by (seconds).
const DT: f32 = 1.0 / 60.0;

/// A camera at the origin looking along +X, level, at the play camera's
/// field of view, standing still.
fn level() -> Camera {
    Camera {
        at: Vec3::ZERO,
        angles: [0.0; 3],
        fov: crate::play_view::FOV_DEGREES,
        velocity: Vec3::ZERO,
        ease: 0.0,
    }
}

/// `camera` flown by `input` for `seconds`, a frame at a time.
fn flown(camera: Camera, input: Input, limits: &Limits, seconds: f32) -> Camera {
    let frames = (seconds / DT).round() as usize;
    (0..frames).fold(camera, |camera, _| step(camera, &input, limits, DT))
}

/// No limit near: the soles at the origin and no rooms.
const OPEN: Limits = Limits {
    area: None,
    soles: Vec3::ZERO,
};

/// A box of rooms 10 m across round the origin.
const ROOMS: Limits = Limits {
    area: Some((Vec3::splat(-5.0), Vec3::splat(5.0))),
    soles: Vec3::ZERO,
};

/// One case of the camera: its name, where the camera starts, the input
/// held, the limits, for how long (seconds), what is measured of the camera
/// after, the value wanted and how far off it may be.
type Case = (
    &'static str,
    Camera,
    Input,
    Limits,
    f32,
    fn(&Camera) -> f32,
    f32,
    f32,
);

/// The camera's speeds, its ease, its zoom, tilt and pitch limits, and the
/// limits of where it may go: each case a camera flown and measured.
pub fn camera_probes() -> Checks {
    let forward = Input {
        forward: 1.0,
        ..Input::default()
    };
    let fast = Input {
        fast: true,
        ..forward
    };
    let diagonal = Camera {
        angles: [0.0, 45.0, 0.0],
        ..level()
    };
    let moving = Camera {
        velocity: Vec3::new(SPEED, 0.0, 0.0),
        ..level()
    };
    let flying_fast = Camera {
        velocity: Vec3::new(FAST, 0.0, 0.0),
        ..level()
    };
    let zoomed = Camera {
        fov: NARROWEST,
        ..level()
    };
    // At a speed for a second from rest, the first `EASE` seconds reach it
    // at an even rate, so the camera goes the speed less half of `EASE`'s
    // worth of it.
    let eased = |speed: f32| speed * (1.0 - EASE / 2.0);
    let far = Camera {
        at: Vec3::new(REACH + 10.0, 0.0, 0.0),
        ..level()
    };
    let cases: [Case; 18] = [
        (
            "speed",
            level(),
            forward,
            OPEN,
            1.0,
            |c| c.at.x,
            eased(SPEED),
            0.06,
        ),
        (
            "fast",
            level(),
            fast,
            OPEN,
            1.0,
            |c| c.at.x,
            eased(FAST),
            0.25,
        ),
        (
            "slow",
            level(),
            Input {
                slow: true,
                ..forward
            },
            OPEN,
            1.0,
            |c| c.at.x,
            eased(SLOW),
            0.02,
        ),
        (
            "straight up",
            level(),
            Input {
                up: 1.0,
                ..Input::default()
            },
            OPEN,
            1.0,
            |c| c.at.z,
            eased(SPEED),
            0.06,
        ),
        (
            "eases to a stop",
            moving,
            Input::default(),
            OPEN,
            1.0,
            |c| c.at.x,
            SPEED * EASE / 2.0,
            0.06,
        ),
        (
            "stopped after the ease",
            moving,
            Input::default(),
            OPEN,
            EASE + DT,
            |c| c.velocity.length(),
            0.0,
            1e-4,
        ),
        (
            "stopped after the ease from fast flight",
            flying_fast,
            Input::default(),
            OPEN,
            EASE + DT,
            |c| c.velocity.length(),
            0.0,
            1e-4,
        ),
        (
            "a shot's pitch a whole turn off is taken as it looks",
            Camera::of(([0.0; 3], [-322.9, 0.0, 0.0], 60.0)),
            Input::default(),
            OPEN,
            DT,
            |c| c.angles[0],
            37.1,
            1e-3,
        ),
        (
            "zoom in stops at its narrowest",
            level(),
            Input {
                zoom: 1000.0,
                ..Input::default()
            },
            OPEN,
            DT,
            |c| c.fov,
            NARROWEST,
            1e-4,
        ),
        (
            "zoom out stops at its widest",
            level(),
            Input {
                zoom: -1000.0,
                ..Input::default()
            },
            OPEN,
            DT,
            |c| c.fov,
            WIDEST,
            1e-4,
        ),
        (
            "tilt stops at its most",
            level(),
            Input {
                tilt: 1.0,
                ..Input::default()
            },
            OPEN,
            ROLL_MOST / TILT_SPEED + 1.0,
            |c| c.angles[2],
            ROLL_MOST,
            1e-4,
        ),
        (
            "pitch stops at the follow camera's limit",
            level(),
            Input {
                look: Vec2::new(0.0, 1000.0),
                ..Input::default()
            },
            OPEN,
            DT,
            |c| c.angles[0],
            crate::sim_host::pitch_limit(),
            1e-4,
        ),
        (
            "the look slows with the zoom",
            zoomed,
            Input {
                look: Vec2::new(10.0, 0.0),
                ..Input::default()
            },
            OPEN,
            DT,
            |c| c.angles[1],
            -10.0 * NARROWEST / crate::play_view::FOV_DEGREES,
            1e-4,
        ),
        (
            "stays within reach of the soles",
            Camera {
                at: Vec3::new(REACH - 0.5, 0.0, 0.0),
                ..diagonal
            },
            fast,
            OPEN,
            2.0,
            |c| c.at.length(),
            REACH,
            1e-3,
        ),
        (
            "slides along the reach",
            Camera {
                at: Vec3::new(REACH - 0.5, 0.0, 0.0),
                ..diagonal
            },
            fast,
            OPEN,
            2.0,
            // At least 10 m along the limit.
            |c| c.at.y.min(10.0),
            10.0,
            1e-4,
        ),
        (
            "a shot past the reach does not jump",
            far,
            Input::default(),
            OPEN,
            1.0,
            |c| c.at.x,
            REACH + 10.0,
            1e-4,
        ),
        (
            "goes no farther past the reach",
            far,
            fast,
            OPEN,
            1.0,
            |c| c.at.length(),
            REACH + 10.0,
            1e-3,
        ),
        (
            "slides along the rooms' box",
            Camera {
                at: Vec3::new(6.0, 0.0, 0.0),
                ..diagonal
            },
            fast,
            ROOMS,
            1.0,
            // On the box's side, grown by its margin, and on along it.
            |c| c.at.x + c.at.y.min(5.0),
            5.0 + AREA_MARGIN + 5.0,
            1e-4,
        ),
    ];
    cases
        .into_iter()
        .map(
            |(name, camera, input, limits, seconds, measure, wanted, off)| {
                let measured = measure(&flown(camera, input, &limits, seconds));
                (
                    name,
                    check(
                        (measured - wanted).abs() <= off,
                        format!("{measured:.4}, wanted {wanted:.4} within {off}"),
                    ),
                )
            },
        )
        .collect()
}

/// Photo mode on at `shot`, as `photo_keys` turns it on.
fn entered(shot: TalkShot) -> PhotoMode {
    PhotoMode {
        on: Some(Photo {
            camera: Camera::of(shot),
            start_fov: shot.2,
            speed: Speed::Stopped,
            hide_own: false,
            called_break: false,
            seen_break: false,
            idle: 0.0,
            generation: 0,
        }),
        ..PhotoMode::default()
    }
}

/// The ticks a world at `scale` is due over `seconds` of 60 Hz frames.
fn ticks_due(scale: f32, seconds: f32) -> usize {
    let mut clock = sim::Accumulator::new(sim::TickRate::default(), 0);
    let frames = (seconds / DT).round() as usize;
    (0..frames)
        .map(|_| {
            clock
                .advance(std::time::Duration::from_secs_f32(DT).mul_f32(scale))
                .len()
        })
        .sum()
}

/// Played alone, photo mode holds the world as it begins, the camera taking
/// the shot it began at, and runs it at a quarter of its speed at Quarter;
/// photo mode off, the view has its own shot again.
fn alone() -> Result<String, String> {
    let shot = ([1.0, 2.0, 3.0], [10.0, 20.0, 5.0], 50.0);
    let mut photo = entered(shot);
    let started = photo.shot() == Some(shot);
    let held = photo.holds_world() && photo.scale() == 1.0;
    let mut speeds = Vec::new();
    for speed in [Speed::Quarter, Speed::Normal] {
        if let Some(on) = photo.on.as_mut() {
            on.speed = speed;
        }
        speeds.push((photo.holds_world(), photo.scale()));
    }
    let (quarter, normal) = (ticks_due(Speed::Quarter.scale(), 5.0), ticks_due(1.0, 5.0));
    let a_quarter = (quarter as f32 - normal as f32 / 4.0).abs() <= 1.0;
    photo.on = None;
    let left = photo.shot().is_none() && !photo.holds_world() && photo.scale() == 1.0;
    check(
        started && held && speeds == [(false, 0.25), (false, 1.0)] && a_quarter && left,
        format!(
            "started at the shot {started}, held {held}, quarter and normal {speeds:?}, ticks due in 5 s {quarter} of {normal}, view's own shot after {left}"
        ),
    )
}

/// While photo mode is on, play gets no keys: a break, solo mode and the
/// attack held reach nothing, and the player's command is the empty one.
/// Photo mode keeps them for itself.
fn input() -> Result<String, String> {
    let mut actions = Actions::default();
    for action in [Action::Break, Action::Solo, Action::Attack, Action::Forward] {
        actions.set_pressed(action);
        actions.set_held(action, true);
    }
    let played = blank(&actions);
    let reaches = [Action::Break, Action::Solo, Action::Attack, Action::Forward]
        .iter()
        .filter(|action| played.held(**action) || played.pressed(**action))
        .count();
    let command = crate::sim_host::player_command(
        &played,
        crate::sim_host::Look {
            pitch: 0.0,
            yaw: 0.0,
        },
        (
            &crate::sim_host::Controls::default(),
            &crate::bar_view::BarUse::default(),
        ),
        || crate::sim_host::InHand {
            rules: sim::combat::saber::SaberRules::default(),
            blade: true,
            blaster: false,
            single_blade: false,
        },
    );
    let empty = command.buttons == 0
        && command.holds == 0
        && (command.forward, command.right, command.up) == (0, 0, 0);
    let kept = actions.pressed(Action::Break) && actions.held(Action::Attack);
    check(
        reaches == 0 && empty && kept,
        format!("{reaches} of play's actions reach it, empty command {empty}, kept {kept}"),
    )
}

/// In a conversation, the Photo mode key and a pad's Photo mode chord
/// come through, and the conversation keeps play's other actions. With
/// photo mode on, in a conversation too, photo mode has every action.
fn talk_keys() -> Result<String, String> {
    use crate::pad::{Taken, bit, open_keys};
    let photo = bit(Action::PhotoMode);
    let key = open_keys(Taken::Talk, KeyMap::defaults()).is_some_and(|open| open & photo != 0);
    let pad = Taken::Talk.keeps_pad(Action::PhotoMode) && !Taken::Talk.keeps_pad(Action::Jump);
    let on = Taken::Talk.with_photo(true);
    let all = on == Taken::Photo
        && on.keeps_pad(Action::Jump)
        && open_keys(on, KeyMap::defaults()) == Some(u64::MAX)
        && Taken::Screens.with_photo(true) == Taken::Screens;
    check(
        key && pad && all,
        format!("key {key}, pad {pad}, photo mode has every action {all}"),
    )
}

/// A key held as photo mode ends stays out of play until it is let go, as
/// after a screen: the attack held from photo mode does not attack. The
/// movement keys, Crouch among them, go on moving. Pressed again, the attack
/// attacks.
fn held_after() -> Result<String, String> {
    use crate::pad::{Taken, bit, gate_keys, open_keys};
    let (attack, moving) = (
        bit(Action::Attack),
        bit(Action::Forward) | bit(Action::Crouch),
    );
    let mut kept = 0;
    let frames = [
        (Taken::Photo, attack | moving, attack),
        (Taken::Play, attack | moving, 0),
        (Taken::Play, moving, 0),
        (Taken::Play, attack | moving, attack),
    ]
    .map(|(taken, held, down)| {
        gate_keys(open_keys(taken, KeyMap::defaults()), &mut kept, held, down)
    });
    let wanted = [
        (attack | moving, attack),
        (moving, 0),
        (moving, 0),
        (attack | moving, attack),
    ];
    check(
        frames == wanted,
        format!("held and pressed {frames:?}, wanted {wanted:?}"),
    )
}

/// Photo mode holds the cursor while it is on. Left in a conversation, it
/// lets the cursor go, so the player can pick a reply. Left in play, it
/// leaves the cursor to play, which holds it.
fn cursor() -> Result<String, String> {
    let frames = |talking: bool| {
        let mut was = false;
        [true, false, false].map(|photo| crate::sim_host::photo_cursor(photo, &mut was, talking))
    };
    let (talk, play) = (frames(true), frames(false));
    check(
        talk == [Some(true), Some(false), None] && play == [Some(true), None, None],
        format!("in a conversation {talk:?}, in play {play:?}"),
    )
}

/// Two seats on the arena, stepped with the calls photo mode makes.
struct Pair {
    world: sim::SimWorld,
}

impl Pair {
    fn new() -> Result<Pair, String> {
        let config = WorldConfig {
            seats: 2,
            ..WorldConfig::one_seat()
        };
        let world =
            sim::SimWorld::new(config, sim::Level::arena()).map_err(|error| error.to_string())?;
        Ok(Pair { world })
    }

    /// One step, each seat standing, with `calls` made by their seats.
    fn step(&mut self, calls: &[(u8, Option<SimAction>)]) -> Snapshot {
        let mut input = TickInput::default();
        for seat in 0..2 {
            input.insert(Seat(seat), Command::default());
        }
        for (seat, call) in calls
            .iter()
            .filter_map(|(seat, call)| Some((seat, call.clone()?)))
        {
            input.act(Seat(*seat), call);
        }
        let tick = self.world.tick() + 1;
        sim::step(&mut self.world, tick, &input, 8, StepReason::Authority)
    }
}

/// In co-op, photo mode calls a break as it begins and resumes it as it
/// ends. A break that seat 0 called first stays, and so does nothing when
/// the host ended the photo break. In combat the break is asked for as
/// anywhere else, and leaving before it shows resumes nothing at once.
fn coop() -> Result<String, String> {
    let mut pair = Pair::new()?;
    let start = pair.step(&[]);
    // Seat 1 enters photo mode, then leaves it.
    let call = break_on_enter(&start);
    let called = call.is_some();
    let paused = pair.step(&[(1, call)]);
    let (resume, owed) = break_on_leave(called, true, &paused, Seat(1));
    let resumed = pair.step(&[(1, resume.clone())]);
    let own = (paused.paused_by, resumed.paused_by, owed);
    // Seat 0's break first: seat 1's photo mode leaves it alone.
    let held = pair.step(&[(0, Some(SimAction::Break))]);
    let call = break_on_enter(&held);
    let left = break_on_leave(call.is_some(), false, &held, Seat(1));
    let after = pair.step(&[(1, left.0.clone())]);
    let others = (call.clone(), left, after.paused_by);
    let ended = pair.step(&[(0, Some(SimAction::Resume))]);
    // Seat 1's photo break, seen, then ended by the host: leaving calls
    // nothing and owes nothing.
    let call = break_on_enter(&ended);
    let photo_break = pair.step(&[(1, call)]);
    let host_ended = pair.step(&[(0, Some(SimAction::Resume))]);
    let host = break_on_leave(true, true, &host_ended, Seat(1));
    // In combat, before the break shows, leaving resumes nothing at once.
    // The resume owed is forgotten after `OWED_WAIT` seconds with no break.
    let mut fighting = host_ended.clone();
    fighting.in_combat = true;
    let combat = (
        break_on_enter(&fighting),
        break_on_leave(true, false, &fighting, Seat(1)),
        owed_resume(OWED_WAIT, &fighting, Seat(1)),
    );
    check(
        own == (Some(Seat(1)), None, false)
            && resume == Some(SimAction::Resume)
            && others == (None, (None, false), Some(Seat(0)))
            && photo_break.paused_by == Some(Seat(1))
            && host == (None, false)
            && combat == (Some(SimAction::Break), (None, true), (None, false)),
        format!(
            "own break {own:?}, seat 0's break {others:?}, host ended {:?} then {host:?}, combat {combat:?}",
            photo_break.paused_by
        ),
    )
}

/// In co-op, a player who leaves photo mode before the snapshot shows the
/// break it called is owed a resume. The resume is sent once the break
/// shows, and the world runs again. A resume owed while another seat's
/// break holds is forgotten, and that break stays.
fn quick_break() -> Result<String, String> {
    let mut pair = Pair::new()?;
    let start = pair.step(&[]);
    // Seat 1 enters and leaves photo mode within the same snapshot.
    let call = break_on_enter(&start);
    let left = break_on_leave(call.is_some(), false, &start, Seat(1));
    let paused = pair.step(&[(1, call)]);
    let waiting = owed_resume(0.0, &start, Seat(1));
    let sent = owed_resume(0.1, &paused, Seat(1));
    let resumed = pair.step(&[(1, sent.0.clone())]);
    // Seat 0's break holds while seat 1's resume is owed.
    let held = pair.step(&[(0, Some(SimAction::Break))]);
    let other = owed_resume(0.1, &held, Seat(1));
    let after = pair.step(&[(1, other.0.clone())]);
    check(
        left == (None, true)
            && waiting == (None, true)
            && paused.paused_by == Some(Seat(1))
            && sent == (Some(SimAction::Resume), false)
            && resumed.paused_by.is_none()
            && other == (None, false)
            && after.paused_by == Some(Seat(0)),
        format!(
            "left {left:?}, waiting {waiting:?}, paused by {:?}, sent {sent:?}, then paused by {:?}, owed under seat 0's break {other:?}, then paused by {:?}",
            paused.paused_by, resumed.paused_by, after.paused_by
        ),
    )
}

/// The probes of photo mode's time, keys and co-op break.
pub fn probes() -> Checks {
    vec![
        ("photo mode alone", alone()),
        ("photo mode input", input()),
        ("photo mode coop", coop()),
        ("photo mode quick break", quick_break()),
        ("photo mode cursor", cursor()),
        ("photo mode key in a conversation", talk_keys()),
        ("photo mode keys held after", held_after()),
    ]
}
