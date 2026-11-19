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
        ("straight out reaches the arc", straight_out()),
        ("the right stick picks from the arc", stick_arc_table()),
        ("closes_on_interruption", closes_on_interruption()),
        ("second_seat_ends_slow", second_seat_ends_slow()),
        ("the highlight holds near its edge", highlight_holds()),
        ("grenades are readied from the wheel", grenades_readied()),
        (
            "an item kind with several items stays",
            several_items_stay(),
        ),
        ("a mine is laid at a run", sim::traps::lay_while_running()),
        ("a mine stops the run", mine_stops_run()),
        ("picks are queued in order", picks_queued()),
        ("uses wait for a free hand", ready_table()),
        ("the centre uses nothing", centre_cancels()),
    ]
}

/// What a use sees while the character is free and the pick is not yet
/// shown.
const SEEN: Seen = Seen {
    confirmed: false,
    ready: true,
    now_ms: 0,
};

/// A use of `content` that the character can make, with the arc item
/// `pick` picked first.
fn due(content: Content, pick: Option<(Shelf, u16)>) -> Due {
    Due {
        content,
        choices: POWERS.to_vec(),
        usable: true,
        hand: 0,
        target: None,
        pick,
        sent: false,
        waited: 0.0,
        down: None,
        up: None,
        over: false,
    }
}

/// Force Valor, Force Armor and Force Speed: the powers the queue probe
/// picks, in the order it picks them. The force button casts all three.
const POWERS: [u16; 3] = [
    sim::combat::spells::FORCE_POWER_FORCE_VALOR,
    sim::combat::spells::FORCE_POWER_FORCE_ARMOR,
    sim::combat::spells::FORCE_POWER_SPEED_BURST,
];

/// A use of the grenades from the wheel: its name, the arc item picked
/// before it, the actions its first frame sends, and whether the use is
/// over after that frame.
type GrenadeUse = (&'static str, Option<u16>, Vec<Action>, bool);

/// Letting go on the grenades' entry readies the grenade the slot uses (or
/// puts away the one readied), as the slot's key does. A grenade picked
/// from the arc is readied by its pick alone, with no press after it,
/// because a press would put it away again. No use holds a button down.
fn grenades_readied() -> Result<String, String> {
    let thermal = 89;
    let cases: [GrenadeUse; 2] = [
        (
            "the grenades' entry",
            None,
            vec![Action::Grenade(sim::action::GrenadeOrder::Toggle)],
            false,
        ),
        (
            "a grenade picked from the arc",
            Some(thermal),
            vec![Action::PickItem { row: thermal }],
            true,
        ),
    ];
    let wrong: Vec<String> = cases
        .iter()
        .filter_map(|(name, pick, want, done)| {
            let mut due = due(
                Content::Item(Shelf::Grenade),
                pick.map(|row| (Shelf::Grenade, row)),
            );
            let (sent, held) = due.step(SEEN, 0.01);
            let right = sent == *want && held.buttons == 0 && due.done() == *done;
            (!right).then(|| format!("{name}: sent {sent:?}, held {held:?}, done {}", due.done()))
        })
        .collect();
    check(
        wrong.is_empty(),
        format!("{} cases, wrong {wrong:?}", cases.len()),
    )
}

/// The dev soldier with grenades and medpacs on its bar. The grenades stay
/// on the wheel while it carries two or more different grenades, and are
/// left off when it carries one. The medpacs, of which it carries one
/// kind, are left off both times.
fn several_items_stay() -> Result<String, String> {
    let mut sheet = sim::combat::Sheet::dev_soldier();
    sheet.bar.put(0, Content::Item(Shelf::Grenade));
    sheet.bar.put(1, Content::Item(Shelf::Medpac));
    let cases = [(true, true), (false, false)];
    let wrong: Vec<String> = cases
        .iter()
        .filter_map(|(several, want)| {
            let listed = Rules::with(None, false, |rules| {
                let several = |shelf| *several && shelf == Shelf::Grenade;
                bar::wheel(&sheet, rules, 0, &several)
            });
            let grenades = listed.contains(&Content::Item(Shelf::Grenade));
            let medpacs = listed.contains(&Content::Item(Shelf::Medpac));
            (grenades != *want || medpacs)
                .then(|| format!("several grenades {several}: listed {listed:?}"))
        })
        .collect();
    check(
        wrong.is_empty(),
        format!("{} cases, wrong {wrong:?}", cases.len()),
    )
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

/// Every entry in one group.
const ONE_GROUP: [u8; 8] = [0; 8];

/// A frame with the wheel's key `held`, the body alive and nothing else
/// up, with the mouse still. Every entry is in one group.
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
        groups: &ONE_GROUP,
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

/// Eight entries in one group stand 45 degrees apart. Eight entries in two
/// groups of four have a gap between the groups, so entry 4 stands past
/// straight down and entry 3 before it.
fn entry_at_table() -> Result<String, String> {
    let even = Ring::new(&ONE_GROUP);
    let split = Ring::new(&[0, 0, 0, 0, 1, 1, 1, 1]);
    let cases = [
        ("up", toward(0.0, 0.2), &even, Some(0)),
        ("right", toward(90.0, 0.2), &even, Some(2)),
        ("down", toward(180.0, 0.2), &even, Some(4)),
        ("left", toward(270.0, 0.2), &even, Some(6)),
        ("inside the dead zone", Vec2::new(0.03, 0.03), &even, None),
        ("no entries", toward(0.0, 0.2), &Ring::new(&[]), None),
        (
            "last of the first group",
            toward(120.0, 0.2),
            &split,
            Some(3),
        ),
        (
            "first of the second group",
            toward(175.0, 0.2),
            &split,
            Some(4),
        ),
    ];
    let wrong: Vec<String> = cases
        .iter()
        .filter(|(_, pointer, ring, want)| entry_at(*pointer, ring) != *want)
        .map(|(name, pointer, ring, _)| format!("{name}: {:?}", entry_at(*pointer, ring)))
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
    let ring = Ring::new(&ONE_GROUP);
    let out = toward(90.0 + ring.arc_step(), ring.arc());
    seen.extend(wheel.frame(&pointing(&wheel, out, &STIMS)));
    seen.extend(wheel.frame(&frame(false, &STIMS)));
    let want = [Release {
        at: 2,
        arc_at: Some(3),
    }];
    let row = 42;
    let mut due = due(Content::Item(Shelf::Stim), Some((Shelf::Stim, row)));
    let first = due.step(SEEN, 0.01);
    let second = due.step(
        Seen {
            confirmed: true,
            ..SEEN
        },
        0.01,
    );
    let picked_first = first == (vec![Action::PickItem { row }], BarUse::default());
    let used_after = second.0.is_empty() && second.1.buttons == Shelf::Stim.button();
    check(
        on_ring == Some(2) && seen == want && picked_first && used_after,
        format!(
            "arc item on the ring {on_ring:?}, releases {seen:?}, first frame {first:?}, once picked {second:?}"
        ),
    )
}

/// Moving the pointer straight out from the stims (entry 2), from the ring
/// to the arc, keeps the stims highlighted the whole way. Once in the arc,
/// it picks the arc item straight outside the entry.
fn straight_out() -> Result<String, String> {
    let mut wheel = Wheel::default();
    opened_at(&mut wheel, 90.0, &STIMS);
    let ring = Ring::new(&ONE_GROUP);
    let lengths = [
        ring.radius,
        ring.arc_in() - 0.01,
        ring.arc_in() + 0.01,
        ring.arc(),
    ];
    let seen = lengths.map(|length| {
        wheel.frame(&pointing(&wheel, toward(90.0, length), &STIMS));
        (wheel.at, wheel.in_arc, wheel.arc_at)
    });
    let in_ring = (Some(2), false, Some(2));
    let in_arc = (Some(2), true, Some(2));
    check(
        seen == [in_ring, in_ring, in_arc, in_arc],
        format!(
            "(highlight, in the arc, arc item) at the ring, short of the arc, in it, at its middle: {seen:?}"
        ),
    )
}

/// One frame of a player's hand on the wheel, with its key held. Each turn
/// is counted in arc items' angles (`Ring::arc_step`) clockwise from the
/// stims' entry, which stands straight right. Each length is a fraction of
/// the arc's radius.
#[derive(Clone, Copy)]
enum Hand {
    /// The right stick, turned and pushed this far. A push of 0 is the
    /// stick at rest.
    Stick(f32, f32),
    /// The mouse, moving the pointer to this turn and length.
    Mouse(f32, f32),
    /// LB pressed once, with the stick held where it was.
    Step,
}

/// A run of frames on the stims' entry, then the key let go: its name, the
/// frames, and whether the pointer should end in the arc and on which arc
/// item.
type StickCase = (&'static str, &'static [Hand], bool, usize);

/// The stims (entry 2) have an arc of 5 items, and the character uses the
/// third of them (item 2) now. A full push of the right stick reaches the
/// arc, and a push of 80% stays on the ring. A flick of the stick keeps the
/// item in use, wherever along the entry it points. Turning the stick along
/// the rim then moves the choice one item for each arc item's angle, and
/// stops at the arc's ends. LB steps on by one item, and the stick's next
/// turn counts on from there. Letting go of the stick a frame before the
/// key keeps the choice. The mouse picks the item its pointer points at.
fn stick_arc_table() -> Result<String, String> {
    use Hand::{Mouse, Step, Stick};
    const FULL: Hand = Stick(0.0, 1.0);
    let cases: [StickCase; 10] = [
        (
            "a light push",
            &[Stick(0.0, 0.8), Stick(0.0, 0.8)],
            false,
            2,
        ),
        ("a full push", &[FULL, FULL], true, 2),
        (
            "a flick an item clockwise",
            &[Stick(1.0, 1.0), Stick(1.0, 1.0)],
            true,
            2,
        ),
        (
            "half an item along",
            &[FULL, FULL, Stick(0.5, 1.0)],
            true,
            2,
        ),
        ("one item along", &[FULL, FULL, Stick(1.0, 1.0)], true, 3),
        (
            "past the last item",
            &[FULL, FULL, Stick(5.0, 1.0)],
            true,
            4,
        ),
        (
            "past the first item",
            &[FULL, FULL, Stick(-5.0, 1.0)],
            true,
            0,
        ),
        (
            "LB and then one item along",
            &[FULL, FULL, Step, Stick(1.0, 1.0)],
            true,
            4,
        ),
        (
            "the stick let go first",
            &[FULL, FULL, Stick(1.0, 1.0), Stick(0.0, 0.0)],
            true,
            3,
        ),
        (
            "the mouse one item along",
            &[Mouse(0.0, 0.8), Mouse(1.0, 1.0)],
            true,
            3,
        ),
    ];
    let ring = Ring::new(&ONE_GROUP);
    let degrees = |turn: f32| 90.0 + turn * ring.arc_step();
    let wrong: Vec<String> = cases
        .iter()
        .filter_map(|(name, hands, in_arc, item)| {
            let mut wheel = Wheel::default();
            wheel.frame(&frame(true, &STIMS));
            let mut stick = Vec2::ZERO;
            for hand in *hands {
                let mut held = frame(true, &STIMS);
                match *hand {
                    Stick(turn, push) => stick = toward(degrees(turn), push),
                    Mouse(turn, length) => {
                        held.mouse = toward(degrees(turn), length * ring.arc()) - wheel.pointer;
                    }
                    Step => held.steps = 1,
                }
                held.stick = stick;
                wheel.frame(&held);
            }
            let ended = wheel.in_arc;
            let release = wheel.frame(&frame(false, &STIMS));
            let want = Release {
                at: 2,
                arc_at: Some(*item),
            };
            (ended != *in_arc || release != Some(want))
                .then(|| format!("{name}: in the arc {ended}, released {release:?}"))
        })
        .collect();
    check(
        wrong.is_empty(),
        format!("{} cases, wrong {wrong:?}", cases.len()),
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

/// The wheel open in a session of one seat slows the clock. In a session
/// of two seats, the clock runs at full speed while the wheel is open. The
/// number of seats is set when the session starts.
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

/// How long the queue probe's fake sim keeps the hand out after a cast
/// (sim ms), and how long each of its steps is.
const HAND_OUT_MS: u64 = 600;
const STEP_MS: u64 = 8;

/// The player picks Force Valor from the wheel. While the character casts
/// it, the player picks Force Armor and then Force Speed. The probe's fake
/// sim steps once every frame, sees the command of the frame before, casts
/// on each new press of the force button, and keeps the hand out for
/// `HAND_OUT_MS` after each cast. All three powers are cast, in the order
/// picked, each only after the hand of the one before is down again.
fn picks_queued() -> Result<String, String> {
    let mut wheel = Wheel::default();
    let picked_at = [0, 3, 6];
    let mut casts: Vec<(u16, u64)> = Vec::new();
    let (mut now, mut hand_until, mut was_down) = (0u64, 0u64, false);
    let mut command = BarUse::default();
    for frame in 0..400 {
        // The sim's step, on the command of the frame before.
        let down = command.buttons & sim::buttons::FORCE_POWER != 0;
        if down && !was_down && now >= hand_until {
            casts.push((command.force.unwrap_or(0), now));
            hand_until = now + HAND_OUT_MS;
        } else if down && !was_down {
            casts.push((0, now));
        }
        was_down = down;
        now += STEP_MS;
        if let Some(nth) = picked_at.iter().position(|at| *at == frame) {
            wheel.queue(due(Content::Power(POWERS[nth]), None));
        }
        command = BarUse::default();
        let Some(front) = wheel.queue.front_mut() else {
            continue;
        };
        let seen = Seen {
            ready: now >= hand_until,
            now_ms: now,
            ..SEEN
        };
        let (_, used) = front.step(seen, 0.016);
        if front.done() {
            wheel.queue.pop_front();
        }
        command = used;
    }
    let rows: Vec<u16> = casts.iter().map(|(row, _)| *row).collect();
    let apart = casts
        .windows(2)
        .all(|pair| pair[1].1 >= pair[0].1 + HAND_OUT_MS);
    check(
        rows == POWERS && apart && wheel.queue.is_empty(),
        format!(
            "casts (row, sim ms) {casts:?}, queue left {}",
            wheel.queue.len()
        ),
    )
}

/// A Force power, a mine and an item other than a grenade wait while the
/// character's hand is out. A feat attack, stealth mode and the grenades
/// do not wait.
fn ready_table() -> Result<String, String> {
    let free = sim::combat::Fighter::fresh(100, 100);
    let mut busy = free;
    busy.force.hand = Some((sim::combat::force::Gesture::Push, 1000));
    let cases = [
        (Content::Power(POWERS[0]), false),
        (Content::Mine, false),
        (Content::Item(Shelf::Medpac), false),
        (Content::Item(Shelf::Grenade), true),
        (Content::Stealth, true),
    ];
    let wrong: Vec<String> = cases
        .iter()
        .filter(|(content, while_busy)| {
            ready(*content, &busy, 0) != *while_busy || !ready(*content, &free, 0)
        })
        .map(|(content, _)| format!("{content:?}"))
        .collect();
    check(
        wrong.is_empty(),
        format!("{} cases, wrong {wrong:?}", cases.len()),
    )
}

/// A mine's use keeps the character still. The player runs with the
/// forward key held and lays a mine: the command leaves out the forward
/// key. Without a mine being laid, the forward key runs as before.
fn mine_stops_run() -> Result<String, String> {
    let mut actions = crate::pad::Actions::default();
    actions.set_held(Key::Forward, true);
    let mine = crate::bar_view::slot_use(Content::Mine, &[], true, true, true, 0, None);
    let forward = |bar: &BarUse| {
        let hand = || crate::sim_host::InHand {
            rules: sim::combat::saber::SaberRules::default(),
            blade: false,
        };
        let look = crate::sim_host::Look {
            pitch: 0.0,
            yaw: 0.0,
        };
        let controls = crate::sim_host::Controls::default();
        crate::sim_host::player_command(&actions, look, (&controls, bar), hand).forward
    };
    let seen = [forward(&mine), forward(&BarUse::default())];
    check(
        mine.still && seen == [0, 127],
        format!(
            "the mine holds still {}, forward with a mine and without {seen:?}",
            mine.still
        ),
    )
}

/// How the player's hand comes back to the wheel's centre after pointing
/// at an entry.
#[derive(Clone, Copy, Debug)]
enum Back {
    /// The mouse moves the pointer this far from the centre (H).
    Mouse(f32),
    /// The right stick is pushed this far (from 0 to 1).
    Stick(f32),
}

/// The player points at entry 2 and then brings the pointer back towards
/// the centre. Inside the centre the highlight clears and letting go uses
/// nothing. Just outside it, the entry stays highlighted and is used.
fn centre_cancels() -> Result<String, String> {
    let cases = [
        (Back::Mouse(DEAD_OUT - 0.01), None),
        (Back::Mouse(0.0), None),
        (Back::Mouse(DEAD_IN + 0.02), Some(2)),
        (Back::Stick(STICK_CENTRE - 0.05), None),
        (Back::Stick(0.6), Some(2)),
    ];
    let wrong: Vec<String> = cases
        .iter()
        .filter_map(|(back, want)| {
            let mut wheel = Wheel::default();
            opened_at(&mut wheel, 90.0, &PLAIN);
            let mut held = frame(true, &PLAIN);
            match *back {
                Back::Mouse(length) => held.mouse = toward(90.0, length) - wheel.pointer,
                Back::Stick(push) => held.stick = toward(90.0, push),
            }
            wheel.frame(&held);
            let lit = wheel.at;
            let released = wheel.frame(&frame(false, &PLAIN)).map(|release| release.at);
            (lit != *want || released != *want)
                .then(|| format!("{back:?}: highlight {lit:?}, released {released:?}"))
        })
        .collect();
    check(
        wrong.is_empty(),
        format!("{} cases, wrong {wrong:?}", cases.len()),
    )
}
