//! A client's rewind as a job (issue 229): the world restored from an
//! authority snapshot and stepped again, with this seat's commands, to the
//! tick prediction had made when the job began. The job runs where the
//! client says (`Rebaser`): at once, in the caller (the probes, a session's
//! first frame), or on a thread of its own beside the frames, so a frame
//! never waits on it and the session's lock is never held across it. The
//! client lands its result at a frame's start (`client::land`): the few
//! ticks made meanwhile stepped on it, then it replaces the world that
//! predicted them. A result made for a world since replaced (a hold, an
//! install, a new session) carries an old generation and is dropped.
//!
//! The step is the same call with the same input either way, so where the
//! job runs changes when its result shows, never what it is.
//!
//! Only the ticks something will read get a whole snapshot: from
//! `Job::full_from` (the view's oldest own tick still drawn or followed)
//! on. The others keep this seat's body and the bolts in flight, which the
//! next authority snapshot is compared with; a snapshot costs about as much
//! as the step it records.

use std::sync::mpsc::{Receiver, Sender, TryRecvError, channel};
use std::sync::{Arc, Mutex, PoisonError};

use sim::{
    Command, EntityState, Kind, Level, Seat, SimId, SimWorld, Snapshot, StepReason, TickInput,
    TickRate, WorkedOut, step,
};

/// Bolts in flight by who fired them: creatures (only the host's minds
/// fire, so prediction has none of theirs but those it restored), and
/// players.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(super) struct Bolts {
    pub creatures: u32,
    pub players: u32,
}

impl Bolts {
    pub fn of(snapshot: &Snapshot) -> Bolts {
        Bolts::counted(
            snapshot
                .entities
                .iter()
                .filter(|entity| entity.missile.is_some()),
            |owner| snapshot.get(owner).map(|owner| owner.kind),
        )
    }

    /// The same count from a world, with no snapshot made.
    pub fn of_world(world: &mut SimWorld) -> Bolts {
        let missiles = world.missiles();
        Bolts::counted(missiles.iter(), |owner| {
            world.entity_state(owner).map(|owner| owner.kind)
        })
    }

    fn counted<'a>(
        missiles: impl Iterator<Item = &'a EntityState>,
        mut kind: impl FnMut(SimId) -> Option<Kind>,
    ) -> Bolts {
        let mut bolts = Bolts::default();
        for missile in missiles.filter_map(|entity| entity.missile) {
            match kind(missile.owner) {
                Some(Kind::Avatar(_)) => bolts.players += 1,
                _ => bolts.creatures += 1,
            }
        }
        bolts
    }
}

/// A worker thread's end: jobs go in, results come back (locked only so
/// the client can be shared as a resource is).
type Worker<J, R> = (Sender<J>, Mutex<Receiver<R>>);

/// A tick's input as predicted: this seat's command with the view lag it
/// was made with (the host rewinds its shots by it), and the others' last.
pub(super) fn tick_input(others: &TickInput, seat: Seat, own: Option<(Command, u16)>) -> TickInput {
    let mut input = others.clone();
    if let Some((command, lag)) = own {
        input.insert(seat, command);
        input.set_view_lag(seat, lag);
    }
    input
}

/// A rewind to make.
pub(super) struct Job {
    /// The world it is for (`Play::generation`).
    pub generation: u64,
    pub base: Arc<Snapshot>,
    pub level: Level,
    /// What the world it replaces worked out (`SimWorld::worked_out`),
    /// when that world was home.
    pub worked: Option<WorkedOut>,
    pub seat: Seat,
    /// This seat's body in `base`.
    pub own_id: Option<SimId>,
    /// The other seats' commands as the host last used them.
    pub others: TickInput,
    /// This seat's commands, with their view lags, for the ticks after
    /// `base` up to `to`.
    pub own: Vec<(u64, Command, u16)>,
    pub to: u64,
    pub rate: TickRate,
    /// Ticks from this one on get whole snapshots.
    pub full_from: u64,
}

/// A rewind made.
pub(super) struct Done {
    pub generation: u64,
    pub base: Arc<Snapshot>,
    pub others: TickInput,
    pub own_id: Option<SimId>,
    pub world: Box<SimWorld>,
    /// The tick the world stands at: `to`, or the base's when it was newer.
    pub made: u64,
    /// This seat's body and the bolts at each tick stepped.
    pub history: Vec<(u64, EntityState, Bolts)>,
    /// The ticks from `full_from` on, in order.
    pub snapshots: Vec<Arc<Snapshot>>,
}

/// Makes `job`: `None` when the snapshot does not restore in its level.
pub(super) fn run(job: Job) -> Option<Done> {
    let mut world = SimWorld::from_snapshot(&job.base, job.level).ok()?;
    if let Some(worked) = job.worked {
        world.use_worked_out(worked);
    }
    let base = job.base.tick;
    let made = job.to.max(base);
    let own = |tick: u64| {
        job.own
            .binary_search_by_key(&tick, |(at, ..)| *at)
            .ok()
            .map(|at| (job.own[at].1, job.own[at].2))
    };
    let mut history = Vec::with_capacity((made - base) as usize);
    let mut snapshots = Vec::new();
    for tick in base + 1..=made {
        let input = tick_input(&job.others, job.seat, own(tick));
        let msec = job.rate.msec(tick);
        if tick >= job.full_from {
            let snapshot = step(&mut world, tick, &input, msec, StepReason::Predict);
            if let Some(entity) = job.own_id.and_then(|id| snapshot.get(id)) {
                history.push((tick, *entity, Bolts::of(&snapshot)));
            }
            snapshots.push(Arc::new(snapshot));
        } else {
            world.advance_only(tick, &input, msec, StepReason::Predict);
            if let Some(entity) = job.own_id.and_then(|id| world.entity_state(id)) {
                history.push((tick, entity, Bolts::of_world(&mut world)));
            }
        }
    }
    Some(Done {
        generation: job.generation,
        base: job.base,
        others: job.others,
        own_id: job.own_id,
        world: Box::new(world),
        made,
        history,
        snapshots,
    })
}

/// The live world's ticks to step beside the frames (`Beside`): each with
/// its input and length.
pub(super) struct Steps {
    pub generation: u64,
    pub world: Box<SimWorld>,
    pub ticks: Vec<(u64, TickInput, u32)>,
}

/// Those ticks stepped: the world, and a whole snapshot of each (the view
/// draws and follows every one).
pub(super) struct Stepped {
    pub generation: u64,
    pub world: Box<SimWorld>,
    pub snapshots: Vec<Snapshot>,
}

fn step_all(mut job: Steps) -> Stepped {
    let snapshots = job
        .ticks
        .iter()
        .map(|(tick, input, msec)| step(&mut job.world, *tick, input, *msec, StepReason::Predict))
        .collect();
    Stepped {
        generation: job.generation,
        world: job.world,
        snapshots,
    }
}

/// The live world stepped beside the frames, as the host's is
/// (`game::sim_worker`): a frame hands it its new ticks, and the next
/// frame lands them, waiting for them if they are not done (they are a
/// tick or two: a frame's worth of time at the least). What the frame
/// shows of this seat is a frame behind, as the host's own is.
pub(super) struct Beside {
    on: RebaseOn,
    away: bool,
    ready: Option<Stepped>,
    thread: Option<Worker<Steps, Stepped>>,
    /// The thread died with the world (a panic in a step): the steps run
    /// in the caller from then on, and the world is made again.
    pub lost: bool,
}

impl Beside {
    pub fn new(on: RebaseOn) -> Beside {
        Beside {
            on,
            away: false,
            ready: None,
            thread: None,
            lost: false,
        }
    }

    /// The world is away being stepped.
    pub fn away(&self) -> bool {
        self.away
    }

    /// Steps `job`'s ticks (nothing may be away); in the caller, they are
    /// done on return, and land with the next `land`.
    pub fn send(&mut self, job: Steps) {
        debug_assert!(!self.away, "one step job at a time");
        self.away = true;
        if self.on == RebaseOn::Thread && !self.lost {
            if self.thread.is_none() {
                let (jobs, take) = channel::<Steps>();
                let (give, done) = channel::<Stepped>();
                let spawned = std::thread::Builder::new()
                    .name("net predict".into())
                    .spawn(move || {
                        for job in take {
                            if give.send(step_all(job)).is_err() {
                                break;
                            }
                        }
                    });
                if spawned.is_ok() {
                    self.thread = Some((jobs, Mutex::new(done)));
                }
            }
            match &self.thread {
                Some((jobs, _)) => match jobs.send(job) {
                    Ok(()) => return,
                    Err(returned) => {
                        self.lost = true;
                        self.thread = None;
                        self.ready = Some(step_all(returned.0));
                        return;
                    }
                },
                None => self.lost = true,
            }
        }
        self.ready = Some(step_all(job));
    }

    /// The ticks sent, stepped, and the world with them (waiting for
    /// them); `None` with nothing away, or when the thread died with them.
    pub fn land(&mut self) -> Option<Stepped> {
        if !self.away {
            return None;
        }
        self.away = false;
        if let Some(stepped) = self.ready.take() {
            return Some(stepped);
        }
        let (_, done) = self.thread.as_ref()?;
        let received = done.lock().unwrap_or_else(PoisonError::into_inner).recv();
        match received {
            Ok(stepped) => Some(stepped),
            Err(_) => {
                self.lost = true;
                self.thread = None;
                None
            }
        }
    }
}

/// Where a client's rewinds run.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum RebaseOn {
    /// In the caller, at once: what the probes run, in virtual time.
    #[default]
    Caller,
    /// On threads of the client's own, beside its frames: the rewinds on
    /// one, the live world's new ticks on another (`Beside`).
    Thread,
}

/// The rewind under way, if any, and its result.
pub(super) struct Rebaser {
    on: RebaseOn,
    /// The job given and not landed yet.
    busy: bool,
    ready: Option<Option<Done>>,
    thread: Option<Worker<Job, Option<Done>>>,
    /// The thread died (a panic in a step): rewinds run in the caller
    /// from then on.
    pub lost: bool,
}

impl Rebaser {
    pub fn new(on: RebaseOn) -> Rebaser {
        Rebaser {
            on,
            busy: false,
            ready: None,
            thread: None,
            lost: false,
        }
    }

    /// A job is under way or its result not taken yet.
    pub fn busy(&self) -> bool {
        self.busy
    }

    /// Starts `job` (none may be under way); in the caller, it is done on
    /// return.
    pub fn start(&mut self, job: Job) {
        debug_assert!(!self.busy, "one rewind at a time");
        self.busy = true;
        if self.on == RebaseOn::Thread && !self.lost {
            let Some((jobs, _)) = self.thread() else {
                self.lost = true;
                self.ready = Some(run(job));
                return;
            };
            if let Err(returned) = jobs.send(job) {
                self.lost = true;
                self.thread = None;
                self.ready = Some(run(returned.0));
            }
            return;
        }
        self.ready = Some(run(job));
    }

    /// The thread, started the first time.
    fn thread(&mut self) -> Option<&Worker<Job, Option<Done>>> {
        if self.thread.is_none() {
            let (jobs, take) = channel::<Job>();
            let (give, done) = channel::<Option<Done>>();
            std::thread::Builder::new()
                .name("net rewind".into())
                .spawn(move || {
                    for job in take {
                        if give.send(run(job)).is_err() {
                            break;
                        }
                    }
                })
                .ok()?;
            self.thread = Some((jobs, Mutex::new(done)));
        }
        self.thread.as_ref()
    }

    /// The finished job's result, once: `Some(None)` when it failed (the
    /// snapshot did not restore). `None` while it is under way, or with
    /// none given.
    pub fn take(&mut self) -> Option<Option<Done>> {
        if !self.busy {
            return None;
        }
        if self.ready.is_none()
            && let Some((_, done)) = &self.thread
        {
            let received = done
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .try_recv();
            match received {
                Ok(result) => self.ready = Some(result),
                Err(TryRecvError::Empty) => return None,
                Err(TryRecvError::Disconnected) => {
                    // The thread died with the job: nothing comes, and
                    // the next rewinds run here.
                    self.lost = true;
                    self.thread = None;
                    self.busy = false;
                    return Some(None);
                }
            }
        }
        let result = self.ready.take()?;
        self.busy = false;
        Some(result)
    }

    /// Waits for the job under way, if any (a session's first frame, the
    /// probes' end).
    pub fn wait(&mut self) -> Option<Option<Done>> {
        if !self.busy {
            return None;
        }
        if self.ready.is_none()
            && let Some((_, done)) = &self.thread
        {
            let result = done
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .recv()
                .ok();
            self.busy = false;
            return match result {
                Some(result) => Some(result),
                None => {
                    self.lost = true;
                    self.thread = None;
                    Some(None)
                }
            };
        }
        self.take()
    }
}
