//! The authority's world stepped beside the frame (`sim_host`): a live
//! frame's ticks are handed to a thread of its own with the world, and the
//! world comes home with their snapshots by the next frame (or sooner, for
//! whatever must read it: a save, the session's end). The frame meanwhile
//! draws, so the world's steps cost it nothing; what the view shows is a
//! frame behind. Each step is the same call with the same input, in the
//! same order, so the world steps as it would have.
//!
//! A frame hands over all its ticks only when their commands do not depend
//! on the ticks before them in the frame (`sim_host`'s `advance` says
//! when). Otherwise it steps all but its last tick itself and hands over
//! the last.

use std::ops::{Deref, DerefMut};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Arc, Mutex};

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

/// What the thread brings home with the world.
pub(crate) struct Away {
    /// The ticks it stepped, in order.
    pub stepped: Vec<Stepped>,
    /// The inputs of the ticks it did not step, in order: a tick before
    /// them holds the world (`ends`).
    pub unstepped: Vec<TickInput>,
}

type Job = (Box<SimWorld>, Vec<Order>, bool);
type Done = (Box<SimWorld>, Away);

/// The thread that steps the world, and whether it has it now.
pub(crate) struct Stepper {
    jobs: Sender<Job>,
    /// Locked only to be shared as a resource is: one system lands it.
    done: Mutex<Receiver<Done>>,
    away: bool,
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
) -> Stepped {
    let Order {
        tick,
        input,
        msec,
        prepare,
        live,
    } = order;
    let (input, split, stepped_at) = match live {
        Some(live) => {
            let mut hosted = crate::net_thread::lock(&live.hosted);
            let input = hosted.commands_for(tick, live.paused_by);
            let split = hosted.server().sends_on(tick);
            drop(hosted);
            (input, split, crate::solo::now())
        }
        None => (input, prepare.split, crate::solo::now()),
    };
    let snapshot = Arc::new(step(world, tick, &input, msec, StepReason::Authority));
    let notices = world.take_notices();
    let for_net = match (prepare.hash, split, prepare.check) {
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
    Stepped {
        msec,
        input,
        snapshot,
        notices,
        for_net,
        stepped_at,
    }
}

impl Stepper {
    fn new() -> Stepper {
        let (jobs, take) = channel::<Job>();
        let (give, done) = channel::<Done>();
        std::thread::Builder::new()
            .name("sim step".into())
            .spawn(move || {
                let mut checked = None;
                for (mut world, orders, installs) in take {
                    let mut stepped = Vec::with_capacity(orders.len());
                    let mut orders = orders.into_iter();
                    for order in orders.by_ref() {
                        let tick = step_order(&mut world, order, &mut checked);
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
                    if give.send((world, away)).is_err() {
                        break;
                    }
                }
            })
            .expect("the sim's step thread");
        Stepper {
            jobs,
            done: Mutex::new(done),
            away: false,
        }
    }

    /// Steps `slot`'s world by `orders`, in order, on the thread, taking
    /// it from home until `land`. Each order's `prepare` hashes and splits
    /// its snapshot there too (a host's: `Stepped::hashed`). The thread
    /// stops after a tick the world waits after (`ends`; `installs` says
    /// whether the session installs modules) and brings the rest home
    /// unstepped.
    pub(crate) fn send(
        slot: &mut WorldSlot,
        stepper: &mut Option<Stepper>,
        orders: Vec<Order>,
        installs: bool,
    ) {
        let stepper = stepper.get_or_insert_with(Stepper::new);
        let world = slot.0.take().expect("the world is home to be sent");
        stepper
            .jobs
            .send((world, orders, installs))
            .expect("the sim's step thread runs");
        stepper.away = true;
    }

    /// The world home again with the ticks it was sent to step (waiting
    /// for them), if it was away.
    pub(crate) fn land(slot: &mut WorldSlot, stepper: &mut Option<Stepper>) -> Option<Away> {
        let stepper = stepper.as_mut().filter(|stepper| stepper.away)?;
        let (world, away) = (stepper.done.get_mut())
            .expect("the step thread's channel")
            .recv()
            .expect("the sim's step thread answers");
        stepper.away = false;
        slot.0 = Some(world);
        Some(away)
    }
}
