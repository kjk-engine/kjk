//! The authority's world stepped beside the frame (`sim_host`): a live
//! frame's ticks are handed to a thread of its own with the world, and the
//! world comes home with their snapshots (blocking `RunAndReturn` by the
//! next frame, or streaming per tick in persistent async `CheckedIn` mode
//! when `KJK_STEP_ASYNC` is enabled). Each step is the same call with the
//! same input, in the same order, so the world steps as it would have.

use std::ops::{Deref, DerefMut};
use std::sync::mpsc::{Receiver, Sender, TryRecvError, channel};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use sim::notice::Notice;
use sim::{Seat, SimWorld, Snapshot, StepReason, TickInput, step};

use crate::net_thread::Shared;
use crate::solo::Hosted;

/// The world, at home or away being stepped.
pub(crate) struct WorldSlot(Option<Box<SimWorld>>);

impl WorldSlot {
    pub(crate) fn new(world: SimWorld) -> WorldSlot {
        WorldSlot(Some(Box::new(world)))
    }

    /// Whether the world is here to read (not being stepped).
    pub(crate) fn home(&self) -> bool {
        self.0.is_some()
    }
}

impl Deref for WorldSlot {
    type Target = SimWorld;

    fn deref(&self) -> &SimWorld {
        self.0.as_deref().expect("the world is being stepped")
    }
}

impl DerefMut for WorldSlot {
    fn deref_mut(&mut self) -> &mut SimWorld {
        self.0.as_deref_mut().expect("the world is being stepped")
    }
}

/// A tick stepped away: its input, its snapshot and the notices it raised;
/// when hosting, its hash, and when a frame can be made of it the snapshot
/// split as the server keeps it (parts and check), made there too, so the
/// frame neither splits nor hashes it (issue 229). A solo host's tick that
/// its own client checks is split on a thread of its own instead
/// (`Prepare::check`).
pub(crate) struct Stepped {
    pub msec: u32,
    pub input: TickInput,
    pub snapshot: Arc<Snapshot>,
    pub notices: Vec<(Seat, Notice)>,
    pub for_net: Option<ForNet>,
    /// When the step thread pulled commands and stepped this tick.
    pub stepped_at: net::NetTime,
    /// New dialogue or transcript lines emitted during this step.
    pub talk: Vec<sim::script::dialogue::Line>,
    /// Wall duration spent stepping this tick on the worker thread, µs.
    pub step_us: u64,
    /// Whether the worker already published this tick to `Hosted::server`
    /// (co-op hosts in async queue mode).
    pub published: bool,
}

/// What the step thread made of a tick for the network. A host's tick is
/// hashed, and split when a frame can be made of it. A solo host's checked
/// tick is split on a thread of its own and not hashed.
pub(crate) struct ForNet {
    pub hash: Option<sim::SnapshotHash>,
    pub split: Option<net::codec::Stepped>,
}

/// What a step job makes beside the snapshot (`Prepare`).
#[derive(Clone, Copy, Default)]
pub(crate) struct Prepare {
    /// Its hash (a host's `hashes.txt`).
    pub hash: bool,
    /// Its parts too (a frame can be made of the tick).
    pub split: bool,
    /// Its parts, and its delta from the last tick checked, made on a
    /// thread of their own (`net::codec::Stepped::beside`). This is for a
    /// tick that a solo host's own client rebuilds to check the codec. A
    /// tick that is also split takes `split` instead.
    pub check: bool,
}

/// Live-hosted input pull performed right before stepping a batched tick so
/// incoming guest commands arriving between ticks of the same frame are seen.
pub(crate) struct LiveInput {
    pub hosted: Shared<Hosted>,
    pub paused_by: Option<Seat>,
    pub publish: Option<net::io::Waker>,
}

/// A tick to step away: its number, its input, how many milliseconds it
/// covers, and what to make beside its snapshot.
pub(crate) struct Order {
    pub tick: u64,
    pub input: TickInput,
    pub msec: u32,
    pub prepare: Prepare,
    pub live: Option<LiveInput>,
}

/// What the thread brings home with the world in blocking mode.
pub(crate) struct Away {
    /// The ticks it stepped, in order.
    pub stepped: Vec<Stepped>,
    /// The inputs of the ticks it did not step, in order: a tick before
    /// them holds the world (`ends`).
    pub unstepped: Vec<TickInput>,
}

/// Ticks and recovered unstepped inputs drained from an async checked-in queue.
pub(crate) struct Drained {
    pub stepped: Vec<Stepped>,
    pub unstepped: Vec<TickInput>,
    pub ended: bool,
}

enum StepCmd {
    /// Blocking single-batch path (default / co-op host / non-async).
    RunAndReturn {
        world: Box<SimWorld>,
        orders: Vec<Order>,
        installs: bool,
        reset_checked: bool,
    },
    /// Hand `SimWorld` to the step thread to keep across frames.
    CheckIn {
        world: Box<SimWorld>,
        installs: bool,
        epoch: u64,
        reset_checked: bool,
        paused_by: Option<Seat>,
    },
    /// Queue ticks for the checked-in world under `epoch`.
    Orders { epoch: u64, orders: Vec<Order> },
    /// Return the checked-in world after draining remaining in-flight orders.
    CheckOut,
}

enum StepEvent {
    Returned(Box<SimWorld>, Away),
    Tick {
        epoch: u64,
        stepped: Stepped,
        ended: bool,
    },
    Dropped {
        epoch: u64,
        count: usize,
        unstepped: Vec<TickInput>,
    },
    CheckedOut(Box<SimWorld>),
}

struct CheckedInWorld {
    world: Box<SimWorld>,
    installs: bool,
    epoch: u64,
    cut: bool,
    talk_sent: usize,
    paused_by: Option<Seat>,
}

enum Mode {
    Idle,
    Blocking,
    CheckedIn {
        epoch: u64,
        in_flight: usize,
        cut: bool,
    },
}

/// The thread that steps the world, and whether it has it now.
pub(crate) struct Stepper {
    jobs: Sender<StepCmd>,
    /// Locked only to be shared as a resource is: one system lands it.
    done: Mutex<Receiver<StepEvent>>,
    mode: Mode,
}

/// Whether the world waits after the tick that made `snapshot`, so the
/// ticks handed over with it are not stepped: the tick called a break, or
/// it asks for a transition and the session installs modules
/// (`installs`). `sim_host`'s `advance` stops its own loop at the same
/// ticks.
fn ends(snapshot: &Snapshot, installs: bool) -> bool {
    snapshot.paused_by.is_some() || (installs && snapshot.transition.is_some())
}

/// Steps `world` by `order` and makes what the order asks for beside the
/// snapshot. `checked` is the last tick split for a check. The next
/// checked tick's delta is packed from it.
fn step_order(
    world: &mut SimWorld,
    order: Order,
    checked: &mut Option<net::codec::Stepped>,
    talk_sent: &mut usize,
    paused_override: Option<Option<Seat>>,
) -> Stepped {
    let Order {
        tick,
        input,
        msec,
        prepare,
        live,
    } = order;
    let (input, split, publish_target, stepped_at) = match live {
        Some(live) => {
            let paused_by = paused_override.unwrap_or(live.paused_by);
            let mut hosted = crate::net_thread::lock(&live.hosted);
            let input = hosted.commands_for(tick, paused_by);
            let split = hosted.server().sends_on(tick);
            drop(hosted);
            (
                input,
                split,
                live.publish.map(|waker| (live.hosted, waker)),
                crate::solo::now(),
            )
        }
        None => (input, prepare.split, None, crate::solo::now()),
    };
    let started = Instant::now();
    let snapshot = Arc::new(step(world, tick, &input, msec, StepReason::Authority));
    let notices = world.take_notices();
    let talk_all = &world.script_report().talk;
    let talk = if talk_all.len() > *talk_sent {
        talk_all[*talk_sent..].to_vec()
    } else {
        Vec::new()
    };
    *talk_sent = talk_all.len();
    let mut for_net = match (prepare.hash, split, prepare.check) {
        (_, true, _) => {
            let (split, hash) = net::codec::Stepped::new_hashed(&snapshot);
            Some(ForNet {
                hash: Some(hash),
                split: Some(split),
            })
        }
        (true, false, _) => Some(ForNet {
            hash: Some(snapshot.hash()),
            split: None,
        }),
        (false, false, true) => {
            let split = net::codec::Stepped::beside(snapshot.clone(), checked.as_ref());
            *checked = Some(split.clone());
            Some(ForNet {
                hash: None,
                split: Some(split),
            })
        }
        (false, false, false) => None,
    };
    let mut published = false;
    if let Some((live_hosted, waker)) = publish_target {
        let mut hosted = crate::net_thread::lock(&live_hosted);
        match for_net.as_mut().and_then(|net| net.split.take()) {
            Some(split) => hosted.server_mut().stepped_split(stepped_at, &input, split),
            None => hosted
                .server_mut()
                .stepped_shared(stepped_at, &input, snapshot.clone()),
        }
        drop(hosted);
        waker.wake();
        published = true;
    }
    let step_us = started.elapsed().as_micros() as u64;
    Stepped {
        msec,
        input,
        snapshot,
        notices,
        for_net,
        stepped_at,
        talk,
        step_us,
        published,
    }
}

fn collect_unstepped(orders: impl Iterator<Item = Order>) -> (usize, Vec<TickInput>) {
    let mut count = 0usize;
    let mut unstepped = Vec::new();
    for order in orders {
        count += 1;
        if order.live.is_none() {
            unstepped.push(order.input);
        }
    }
    (count, unstepped)
}

impl Stepper {
    fn new() -> Stepper {
        let (jobs, take) = channel::<StepCmd>();
        let (give, done) = channel::<StepEvent>();
        std::thread::Builder::new()
            .name("sim step".into())
            .spawn(move || {
                let mut checked = None;
                let mut checked_in: Option<CheckedInWorld> = None;
                for cmd in take {
                    match cmd {
                        StepCmd::RunAndReturn {
                            mut world,
                            orders,
                            installs,
                            reset_checked,
                        } => {
                            if reset_checked {
                                checked = None;
                            }
                            let mut talk_sent = world.script_report().talk.len();
                            let mut stepped = Vec::with_capacity(orders.len());
                            let mut orders = orders.into_iter();
                            for order in orders.by_ref() {
                                let tick = step_order(
                                    &mut world,
                                    order,
                                    &mut checked,
                                    &mut talk_sent,
                                    None,
                                );
                                let last = ends(&tick.snapshot, installs);
                                stepped.push(tick);
                                if last {
                                    break;
                                }
                            }
                            let away = Away {
                                stepped,
                                unstepped: orders
                                    .filter(|order| order.live.is_none())
                                    .map(|order| order.input)
                                    .collect(),
                            };
                            if give.send(StepEvent::Returned(world, away)).is_err() {
                                break;
                            }
                        }
                        StepCmd::CheckIn {
                            world,
                            installs,
                            epoch,
                            reset_checked,
                            paused_by,
                        } => {
                            if reset_checked {
                                checked = None;
                            }
                            let talk_sent = world.script_report().talk.len();
                            checked_in = Some(CheckedInWorld {
                                world,
                                installs,
                                epoch,
                                cut: false,
                                talk_sent,
                                paused_by,
                            });
                        }
                        StepCmd::Orders { epoch, orders } => {
                            let Some(state) = checked_in.as_mut() else {
                                unreachable!("orders without a checked-in world");
                            };
                            if state.epoch != epoch || state.cut {
                                let (count, unstepped) = collect_unstepped(orders.into_iter());
                                if give
                                    .send(StepEvent::Dropped {
                                        epoch,
                                        count,
                                        unstepped,
                                    })
                                    .is_err()
                                {
                                    break;
                                }
                                continue;
                            }
                            let mut iter = orders.into_iter();
                            while let Some(order) = iter.next() {
                                let stepped = step_order(
                                    &mut state.world,
                                    order,
                                    &mut checked,
                                    &mut state.talk_sent,
                                    Some(state.paused_by),
                                );
                                state.paused_by = stepped.snapshot.paused_by;
                                let ended = ends(&stepped.snapshot, state.installs);
                                if ended {
                                    state.cut = true;
                                }
                                if give
                                    .send(StepEvent::Tick {
                                        epoch,
                                        stepped,
                                        ended,
                                    })
                                    .is_err()
                                {
                                    return;
                                }
                                if ended {
                                    let (count, unstepped) = collect_unstepped(iter);
                                    if count > 0
                                        && give
                                            .send(StepEvent::Dropped {
                                                epoch,
                                                count,
                                                unstepped,
                                            })
                                            .is_err()
                                    {
                                        return;
                                    }
                                    break;
                                }
                            }
                        }
                        StepCmd::CheckOut => {
                            let state = checked_in.take().expect("checked-in world to check out");
                            if give.send(StepEvent::CheckedOut(state.world)).is_err() {
                                break;
                            }
                        }
                    }
                }
            })
            .expect("the sim's step thread");
        Stepper {
            jobs,
            done: Mutex::new(done),
            mode: Mode::Idle,
        }
    }

    /// Steps `slot`'s world by `orders`, in order, on the thread, taking
    /// it from home until `land` (blocking mode).
    pub(crate) fn send(
        slot: &mut WorldSlot,
        stepper: &mut Option<Stepper>,
        orders: Vec<Order>,
        installs: bool,
        reset_checked: bool,
    ) {
        let stepper = stepper.get_or_insert_with(Stepper::new);
        debug_assert!(matches!(stepper.mode, Mode::Idle));
        let world = slot.0.take().expect("the world is home to be sent");
        stepper
            .jobs
            .send(StepCmd::RunAndReturn {
                world,
                orders,
                installs,
                reset_checked,
            })
            .expect("the sim's step thread runs");
        stepper.mode = Mode::Blocking;
    }

    /// The world home again with the ticks it was sent to step (waiting
    /// for them), if it was away in blocking mode.
    pub(crate) fn land(slot: &mut WorldSlot, stepper: &mut Option<Stepper>) -> Option<Away> {
        let stepper = stepper
            .as_mut()
            .filter(|stepper| matches!(stepper.mode, Mode::Blocking))?;
        let (world, away) = match stepper
            .done
            .get_mut()
            .expect("the step thread's channel")
            .recv()
            .expect("the sim's step thread answers")
        {
            StepEvent::Returned(world, away) => (world, away),
            _ => unreachable!("blocking step returned non-blocking event"),
        };
        stepper.mode = Mode::Idle;
        slot.0 = Some(world);
        Some(away)
    }

    /// Checks `slot`'s world into the step thread so subsequent frames can
    /// stream orders without waiting to bring it home between frames.
    pub(crate) fn check_in(
        slot: &mut WorldSlot,
        stepper: &mut Option<Stepper>,
        installs: bool,
        epoch: u64,
        reset_checked: bool,
        paused_by: Option<Seat>,
    ) {
        let stepper = stepper.get_or_insert_with(Stepper::new);
        debug_assert!(matches!(stepper.mode, Mode::Idle));
        let world = slot.0.take().expect("the world is home to be checked in");
        stepper
            .jobs
            .send(StepCmd::CheckIn {
                world,
                installs,
                epoch,
                reset_checked,
                paused_by,
            })
            .expect("the sim's step thread runs");
        stepper.mode = Mode::CheckedIn {
            epoch,
            in_flight: 0,
            cut: false,
        };
    }

    pub(crate) fn is_checked_in(&self) -> bool {
        matches!(self.mode, Mode::CheckedIn { .. })
    }

    pub(crate) fn in_flight(&self) -> usize {
        match self.mode {
            Mode::CheckedIn { in_flight, .. } => in_flight,
            Mode::Idle | Mode::Blocking => 0,
        }
    }

    /// Queues `orders` on the already checked-in world under `epoch`.
    pub(crate) fn push_orders(&mut self, epoch: u64, orders: Vec<Order>) {
        let Mode::CheckedIn {
            epoch: active_gen,
            in_flight,
            ..
        } = &mut self.mode
        else {
            unreachable!("push_orders requires a checked-in world");
        };
        debug_assert_eq!(*active_gen, epoch);
        *in_flight += orders.len();
        self.jobs
            .send(StepCmd::Orders { epoch, orders })
            .expect("the sim's step thread runs");
    }

    fn apply_event(mode: &mut Mode, event: StepEvent, out: &mut Drained) {
        let Mode::CheckedIn {
            epoch: active_gen,
            in_flight,
            cut,
        } = mode
        else {
            unreachable!("async event while not checked in");
        };
        match event {
            StepEvent::Tick {
                epoch,
                stepped,
                ended,
            } if epoch == *active_gen => {
                *in_flight = in_flight.checked_sub(1).expect("in_flight underflow");
                if ended {
                    *cut = true;
                    out.ended = true;
                }
                out.stepped.push(stepped);
            }
            StepEvent::Dropped {
                epoch,
                count,
                unstepped,
            } if epoch == *active_gen => {
                *in_flight = in_flight.checked_sub(count).expect("in_flight underflow");
                out.unstepped.extend(unstepped);
            }
            StepEvent::Tick { .. } | StepEvent::Dropped { .. } => {}
            StepEvent::Returned(..) | StepEvent::CheckedOut(..) => {
                unreachable!("unexpected lifecycle event while checked in");
            }
        }
    }

    /// Collects every finished tick (and dropped batch) currently ready
    /// without blocking.
    pub(crate) fn try_drain(&mut self) -> Drained {
        let mut out = Drained {
            stepped: Vec::new(),
            unstepped: Vec::new(),
            ended: false,
        };
        if !matches!(self.mode, Mode::CheckedIn { .. }) {
            return out;
        }
        let rx = self.done.get_mut().expect("the step thread's channel");
        loop {
            match rx.try_recv() {
                Ok(event) => Self::apply_event(&mut self.mode, event, &mut out),
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => panic!("the sim's step thread exited"),
            }
        }
        out
    }

    /// Blocks until at least one event arrives, then drains any additional
    /// ready events without blocking.
    pub(crate) fn wait_one(&mut self) -> Drained {
        let mut out = Drained {
            stepped: Vec::new(),
            unstepped: Vec::new(),
            ended: false,
        };
        if self.in_flight() == 0 {
            return out;
        }
        let event = self
            .done
            .get_mut()
            .expect("the step thread's channel")
            .recv()
            .expect("the sim's step thread answers");
        Self::apply_event(&mut self.mode, event, &mut out);
        let more = self.try_drain();
        out.stepped.extend(more.stepped);
        out.unstepped.extend(more.unstepped);
        out.ended |= more.ended;
        out
    }

    /// Drains any remaining `Dropped` batches after an `ended` tick until
    /// `in_flight == 0`.
    pub(crate) fn drain_cut(&mut self) -> Vec<TickInput> {
        let mut out = Drained {
            stepped: Vec::new(),
            unstepped: Vec::new(),
            ended: false,
        };
        while self.in_flight() > 0 {
            let event = self
                .done
                .get_mut()
                .expect("the step thread's channel")
                .recv()
                .expect("the sim's step thread answers");
            Self::apply_event(&mut self.mode, event, &mut out);
        }
        debug_assert!(out.stepped.is_empty(), "cut generation should not step");
        out.unstepped
    }

    /// Checks the world back out of the step thread, waiting for all in-flight
    /// work to settle and returning any remaining ticks/unstepped inputs.
    pub(crate) fn check_out(slot: &mut WorldSlot, stepper: &mut Option<Stepper>) -> Drained {
        let mut out = Drained {
            stepped: Vec::new(),
            unstepped: Vec::new(),
            ended: false,
        };
        let Some(stepper) = stepper
            .as_mut()
            .filter(|stepper| matches!(stepper.mode, Mode::CheckedIn { .. }))
        else {
            return out;
        };
        stepper
            .jobs
            .send(StepCmd::CheckOut)
            .expect("the sim's step thread runs");
        loop {
            let event = stepper
                .done
                .get_mut()
                .expect("the step thread's channel")
                .recv()
                .expect("the sim's step thread answers");
            match event {
                StepEvent::CheckedOut(world) => {
                    debug_assert_eq!(stepper.in_flight(), 0);
                    stepper.mode = Mode::Idle;
                    slot.0 = Some(world);
                    return out;
                }
                other => Self::apply_event(&mut stepper.mode, other, &mut out),
            }
        }
    }
}

/// Probes for the persistent async step worker queue (`StepCmd` / `StepEvent`).
#[cfg(feature = "probes")]
pub fn queue_probes() -> sim::probe::Checks {
    use sim::action::{Action, Verb};
    use sim::determinism::{GROUND, pilot, proving_ground};
    use sim::probe::check;
    use sim::script::actions::{Action as ScriptAction, Queued};
    use sim::{Command, Level, WorldConfig};
    const TURN_DEGREES: f32 = 34.0;

    let setup = sim::determinism::Setup::pinned();
    let order = |tick: u64, input: TickInput| Order {
        tick,
        input,
        msec: setup.rate.msec(tick),
        prepare: Prepare::default(),
        live: None,
    };

    // 1. async_matches_sync: 200 ticks queued async match sync step hashes bit-for-bit.
    let mut sync_world = SimWorld::new(setup.world, Level::arena()).expect("sync world");
    let mut async_slot =
        WorldSlot::new(SimWorld::new(setup.world, Level::arena()).expect("async world"));
    let mut stepper = None;
    Stepper::check_in(&mut async_slot, &mut stepper, false, 1, true, None);
    let mut sync_hashes = Vec::with_capacity(200);
    let mut async_hashes = Vec::with_capacity(200);
    let mut next_tick = 1u64;
    while next_tick <= 200 {
        let end = (next_tick + 4).min(200);
        let mut batch = Vec::new();
        for tick in next_tick..=end {
            let input = pilot(setup.pilot, tick, setup.world.seats);
            let sync_snap = step(
                &mut sync_world,
                tick,
                &input,
                setup.rate.msec(tick),
                StepReason::Authority,
            );
            sync_hashes.push(sync_snap.hash());
            batch.push(order(tick, input));
        }
        if let Some(s) = stepper.as_mut() {
            s.push_orders(1, batch);
            let drained = s.wait_one();
            for item in drained.stepped {
                async_hashes.push(item.snapshot.hash());
            }
        }
        next_tick = end + 1;
    }
    let rest = Stepper::check_out(&mut async_slot, &mut stepper);
    for item in rest.stepped {
        async_hashes.push(item.snapshot.hash());
    }
    let sync_match = async_hashes.len() == 200
        && async_hashes == sync_hashes
        && async_slot.home()
        && async_slot.snapshot().hash() == sync_world.snapshot().hash();

    // 2. cut_on_break_returns_unstepped_actions: break on tick 1 stops ticks 2..=4 cleanly.
    let mut break_slot = WorldSlot::new(
        SimWorld::new(WorldConfig::one_seat(), Level::arena()).expect("break world"),
    );
    let mut break_stepper = None;
    Stepper::check_in(&mut break_slot, &mut break_stepper, false, 1, true, None);
    let mut break_in = TickInput::default();
    break_in.act(Seat(0), Action::Break);
    let mut u1 = TickInput::default();
    u1.act(
        Seat(0),
        Action::Use {
            object: 1,
            verb: Verb::Open,
        },
    );
    let mut u2 = TickInput::default();
    u2.act(
        Seat(0),
        Action::Use {
            object: 2,
            verb: Verb::Open,
        },
    );
    let mut u3 = TickInput::default();
    u3.act(Seat(0), Action::Holster);
    let mut break_ok = false;
    if let Some(s) = break_stepper.as_mut() {
        s.push_orders(
            1,
            vec![
                order(1, break_in),
                order(2, u1.clone()),
                order(3, u2.clone()),
                order(4, u3.clone()),
            ],
        );
        let first = s.wait_one();
        let more = s.drain_cut();
        let mut recovered = first.unstepped;
        recovered.extend(more);
        let tail = Stepper::check_out(&mut break_slot, &mut break_stepper);
        break_ok = first.ended
            && first.stepped.len() == 1
            && first.stepped[0].snapshot.paused_by == Some(Seat(0))
            && tail.stepped.is_empty()
            && recovered == vec![u1, u2, u3];
    }

    // 3. cut_on_transition_in_proving_ground: transition stops queued orders cleanly.
    let mut tr_world =
        SimWorld::new(WorldConfig::one_seat(), proving_ground()).expect("proving ground");
    let tr_snap0 = tr_world.snapshot();
    let player = tr_snap0
        .own(Seat(0))
        .and_then(|e| e.player)
        .expect("own player");
    let look_cmd = sim::combat::mind::looking(&player, 0.0, TURN_DEGREES);
    let walk_cmd = Command {
        forward: 127,
        ..look_cmd
    };
    let mut tr_slot = WorldSlot::new(tr_world);
    let mut tr_stepper = None;
    Stepper::check_in(&mut tr_slot, &mut tr_stepper, true, 1, true, None);
    let mut marker_tail = Vec::new();
    let mut tr_hit = false;
    let mut tr_recovered = Vec::new();
    for tick_base in (1u64..=600).step_by(8) {
        let mut batch = Vec::new();
        let mut expected_inputs = Vec::new();
        for offset in 0..8u64 {
            let tick = tick_base + offset;
            let mut input = TickInput::default();
            input.insert(Seat(0), walk_cmd);
            if offset == 0 {
                input.act(
                    Seat(0),
                    Action::Use {
                        object: sim::determinism::DOOR,
                        verb: Verb::Open,
                    },
                );
            } else {
                input.act(Seat(0), Action::Holster);
            }
            expected_inputs.push(input.clone());
            batch.push(order(tick, input));
        }
        if let Some(s) = tr_stepper.as_mut() {
            s.push_orders(1, batch);
            while s.in_flight() > 0 {
                let d = s.wait_one();
                for item in &d.stepped {
                    if item.snapshot.transition.is_some() {
                        tr_hit = true;
                        let step_idx = usize::try_from(item.snapshot.tick - tick_base).unwrap_or(0);
                        marker_tail = expected_inputs[step_idx + 1..].to_vec();
                    }
                }
                if d.ended {
                    tr_recovered.extend(d.unstepped);
                    tr_recovered.extend(s.drain_cut());
                    break;
                }
            }
        }
        if tr_hit {
            break;
        }
    }
    let _ = Stepper::check_out(&mut tr_slot, &mut tr_stepper);
    let tr_ok = tr_hit
        && !marker_tail.is_empty()
        && tr_recovered == marker_tail
        && tr_snap0.place.is_some_and(|p| p.module == GROUND);

    // 4. checkout_drains_in_flight_before_save: check_out drains every queued tick in order.
    let mut co_slot = WorldSlot::new(SimWorld::new(setup.world, Level::arena()).expect("co world"));
    let mut co_stepper = None;
    Stepper::check_in(&mut co_slot, &mut co_stepper, false, 1, true, None);
    let mut co_ticks = Vec::new();
    if let Some(s) = co_stepper.as_mut() {
        let batch = (1..=16u64)
            .map(|tick| order(tick, pilot(setup.pilot, tick, setup.world.seats)))
            .collect();
        s.push_orders(1, batch);
        let partial = s.try_drain();
        co_ticks.extend(partial.stepped.into_iter().map(|s| s.snapshot.tick));
    }
    let drained = Stepper::check_out(&mut co_slot, &mut co_stepper);
    co_ticks.extend(drained.stepped.into_iter().map(|s| s.snapshot.tick));
    let co_ok = co_slot.home()
        && co_ticks == (1..=16u64).collect::<Vec<_>>()
        && co_slot.snapshot().tick == 16;

    // 5. talk_streams_while_checked_in: dialogue lines stream monotonically via Stepped::talk.
    let talk_level =
        proving_ground().with_scripts(sim::probe::library_of(Vec::new(), Default::default()));
    let mut talk_world = SimWorld::new(WorldConfig::one_seat(), talk_level).expect("talk world");
    let speaker = talk_world.snapshot().own(Seat(0)).expect("own avatar").id.0;
    let mut state = talk_world.script_state().clone();
    state.actions.entry(speaker).or_default().push(Queued {
        action: ScriptAction::Bark { strref: 100 },
        started_ms: None,
    });
    talk_world.set_script_state(state);
    let mut talk_slot = WorldSlot::new(talk_world);
    let mut talk_stepper = None;
    Stepper::check_in(&mut talk_slot, &mut talk_stepper, false, 1, true, None);
    let mut streamed = Vec::new();
    if let Some(s) = talk_stepper.as_mut() {
        s.push_orders(
            1,
            vec![
                order(1, TickInput::default()),
                order(2, TickInput::default()),
            ],
        );
        while s.in_flight() > 0 {
            let d = s.wait_one();
            for item in d.stepped {
                streamed.extend(item.talk);
            }
        }
    }
    let tail = Stepper::check_out(&mut talk_slot, &mut talk_stepper);
    for item in tail.stepped {
        streamed.extend(item.talk);
    }
    let world_talk_len = talk_slot.script_report().talk.len();
    let talk_ok =
        streamed.len() == 1 && world_talk_len == 1 && streamed == talk_slot.script_report().talk;

    // 6. host_async_publishes_directly_and_cuts_on_break: worker pulls live commands,
    // publishes stepped snapshots directly to Hosted::server, and cuts on break.
    let arena = Level::arena();
    let mut host_world = SimWorld::new(WorldConfig::one_seat(), arena.clone()).expect("host world");
    let host_start = host_world.snapshot();
    let mut solo = crate::solo::Solo::new(
        &arena,
        net::session::LevelRequest::Stage(arena.id()),
        setup.rate,
        &host_start,
        None,
        &std::env::temp_dir(),
    )
    .expect("solo");
    for _ in 0..8 {
        solo.hold(false);
        solo.pump();
        if solo.playing() {
            break;
        }
    }
    let hosted = solo.shared_hosted();
    let live_order = |tick: u64| Order {
        tick,
        input: TickInput::default(),
        msec: setup.rate.msec(tick),
        prepare: Prepare::default(),
        live: Some(LiveInput {
            hosted: hosted.clone(),
            paused_by: None,
            publish: Some(net::io::Waker::new()),
        }),
    };
    let mut host_slot = WorldSlot::new(host_world);
    let mut host_stepper = None;
    Stepper::check_in(&mut host_slot, &mut host_stepper, false, 1, true, None);
    let mut host_t1_ok = false;
    let mut host_t2_cut_ok = false;
    if let Some(s) = host_stepper.as_mut() {
        s.push_orders(1, vec![live_order(1)]);
        let d1 = s.wait_one();
        let ring_after_t1 = crate::net_thread::lock(&hosted).server().held();
        host_t1_ok = d1.stepped.len() == 1
            && d1.stepped[0].published
            && d1.stepped[0].snapshot.tick == 1
            && d1.stepped[0].snapshot.paused_by.is_none()
            && ring_after_t1 == 2;
        if let Some(first) = d1.stepped.first() {
            solo.published_stepped(&first.snapshot, None);
        }
        solo.act(Action::Break);
        s.push_orders(1, vec![live_order(2), live_order(3), live_order(4)]);
        let d2 = s.wait_one();
        let more_unstepped = if d2.ended { s.drain_cut() } else { Vec::new() };
        let ring_after_t2 = crate::net_thread::lock(&hosted).server().held();
        host_t2_cut_ok = d2.ended
            && d2.stepped.len() == 1
            && d2.stepped[0].published
            && d2.stepped[0].snapshot.tick == 2
            && d2.stepped[0].snapshot.paused_by == Some(Seat(0))
            && d2.unstepped.is_empty()
            && more_unstepped.is_empty()
            && s.in_flight() == 0
            && ring_after_t2 == 3;
    }
    let host_tail = Stepper::check_out(&mut host_slot, &mut host_stepper);
    let host_ok = host_t1_ok
        && host_t2_cut_ok
        && host_tail.stepped.is_empty()
        && host_slot.home()
        && host_slot.snapshot().tick == 2;

    vec![
        (
            "async_matches_sync",
            check(
                sync_match,
                format!(
                    "async hashes={}, sync_end={}",
                    async_hashes.len(),
                    sync_world.snapshot().tick
                ),
            ),
        ),
        (
            "cut_on_break_returns_unstepped_actions",
            check(break_ok, format!("break cut ok={break_ok}")),
        ),
        (
            "cut_on_transition_in_proving_ground",
            check(
                tr_ok,
                format!(
                    "tr_hit={tr_hit}, recovered={}, expected={}",
                    tr_recovered.len(),
                    marker_tail.len()
                ),
            ),
        ),
        (
            "checkout_drains_in_flight_before_save",
            check(
                co_ok,
                format!("ticks={co_ticks:?}, home={}", co_slot.home()),
            ),
        ),
        (
            "talk_streams_while_checked_in",
            check(
                talk_ok,
                format!("streamed={}, report={world_talk_len}", streamed.len()),
            ),
        ),
        (
            "host_async_publishes_directly_and_cuts_on_break",
            check(
                host_ok,
                format!(
                    "t1={host_t1_ok}, t2_cut={host_t2_cut_ok}, home={}",
                    host_slot.home()
                ),
            ),
        ),
    ]
}
