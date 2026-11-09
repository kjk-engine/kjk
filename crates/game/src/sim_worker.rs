//! The authority's world stepped beside the frame (`sim_host`): a live
//! frame's last tick is handed to a thread of its own with the world, and
//! the world comes home with its snapshot by the next frame (or sooner, for
//! whatever must read it: a save, the session's end). The frame meanwhile
//! draws, so the world's step costs it nothing; what the view shows is a
//! tick behind, as a tick's interpolation is anyway. The step itself is the
//! same call with the same input, so the world steps as it would have.

use std::ops::{Deref, DerefMut};
use std::sync::Mutex;
use std::sync::mpsc::{Receiver, Sender, channel};

use sim::notice::Notice;
use sim::{Seat, SimWorld, Snapshot, StepReason, TickInput, step};

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
/// frame neither splits nor hashes it (issue 229).
pub(crate) struct Stepped {
    pub msec: u32,
    pub input: TickInput,
    pub snapshot: Snapshot,
    pub notices: Vec<(Seat, Notice)>,
    pub hashed: Option<Hashed>,
}

/// A host's tick, hashed and maybe split (`Stepped::hashed`).
pub(crate) struct Hashed {
    pub hash: sim::SnapshotHash,
    pub split: Option<net::codec::Stepped>,
}

/// What a step job makes beside the snapshot (`Prepare`).
#[derive(Clone, Copy, Default)]
pub(crate) struct Prepare {
    /// Its hash (a host's `hashes.txt`).
    pub hash: bool,
    /// Its parts too (a frame can be made of the tick).
    pub split: bool,
}

type Job = (Box<SimWorld>, u64, TickInput, u32, Prepare);
type Done = (Box<SimWorld>, Stepped);

/// The thread that steps the world, and whether it has it now.
pub(crate) struct Stepper {
    jobs: Sender<Job>,
    /// Locked only to be shared as a resource is: one system lands it.
    done: Mutex<Receiver<Done>>,
    away: bool,
}

impl Stepper {
    fn new() -> Stepper {
        let (jobs, take) = channel::<Job>();
        let (give, done) = channel::<Done>();
        std::thread::Builder::new()
            .name("sim step".into())
            .spawn(move || {
                for (mut world, tick, input, msec, prepare) in take {
                    let snapshot = step(&mut world, tick, &input, msec, StepReason::Authority);
                    let notices = world.take_notices();
                    let hashed = match (prepare.hash, prepare.split) {
                        (_, true) => {
                            let (split, hash) = net::codec::Stepped::new_hashed(&snapshot);
                            Some(Hashed {
                                hash,
                                split: Some(split),
                            })
                        }
                        (true, false) => Some(Hashed {
                            hash: snapshot.hash(),
                            split: None,
                        }),
                        (false, false) => None,
                    };
                    let stepped = Stepped {
                        msec,
                        input,
                        snapshot,
                        notices,
                        hashed,
                    };
                    if give.send((world, stepped)).is_err() {
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

    /// Steps `slot`'s world to `tick` with `input` on the thread, taking it
    /// from home until `land`; `prepare` hashes and splits its snapshot
    /// there too (a host's: `Stepped::hashed`).
    pub(crate) fn send(
        slot: &mut WorldSlot,
        stepper: &mut Option<Stepper>,
        job: (u64, TickInput, u32),
        prepare: Prepare,
    ) {
        let stepper = stepper.get_or_insert_with(Stepper::new);
        let world = slot.0.take().expect("the world is home to be sent");
        let (tick, input, msec) = job;
        stepper
            .jobs
            .send((world, tick, input, msec, prepare))
            .expect("the sim's step thread runs");
        stepper.away = true;
    }

    /// The world home again with the tick it was sent to step (waiting for
    /// it), if it was away.
    pub(crate) fn land(slot: &mut WorldSlot, stepper: &mut Option<Stepper>) -> Option<Stepped> {
        let stepper = stepper.as_mut().filter(|stepper| stepper.away)?;
        let (world, stepped) = (stepper.done.get_mut())
            .expect("the step thread's channel")
            .recv()
            .expect("the sim's step thread answers");
        stepper.away = false;
        slot.0 = Some(world);
        Some(stepped)
    }
}
