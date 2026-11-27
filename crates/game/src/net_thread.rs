//! D3's net thread: a session's socket and link pump, off the game's
//! thread. The session (a host's `Server`, a player's `Client`) sits
//! behind a lock the game takes for its own work (a tick's commands, a
//! snapshot, a frame's prediction); the thread takes it the moment
//! datagrams arrive, so they are stamped as they arrive and acks and
//! resends leave on time instead of waiting for the next frame. The
//! network's conditions (`Netem`) release on time too.
//!
//! The thread sleeps on the socket until a datagram comes, the session's
//! next deadline, the conditions' next release, or `MAX_SLEEP`, whichever
//! is first: what the game queues between leaves within that.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Arc, Mutex, MutexGuard};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use net::io::Socket;
use net::netem::Netem;
use net::session::{Client, EncodeJob, EncodedJob};
use net::{Datagram, NetTime, Peer, Transport};

/// The longest the thread sleeps: what the game queued waits at most this.
const MAX_SLEEP: Duration = Duration::from_millis(1);
/// The shortest: a deadline that passed and stays (nothing it can do
/// yet) never spins the thread.
const MIN_SLEEP: Duration = Duration::from_micros(50);

/// What a net thread pumps: datagrams in, datagrams out, optional off-lock
/// snapshot encoding jobs, and when it must next look.
pub trait Endpoint: Send + 'static {
    fn receive(&mut self, now: NetTime, from: Peer, bytes: &[u8]);
    fn poll_transmit(&mut self, now: NetTime) -> Option<(Peer, Vec<u8>)>;
    fn next_deadline(&self) -> Option<NetTime>;
    fn poll_encode_job(&mut self, _now: NetTime) -> Option<EncodeJob> {
        None
    }
    fn finish_encode(&mut self, _now: NetTime, _done: EncodedJob) {}
    fn parallel_encode(&self) -> bool {
        false
    }
}

impl Endpoint for Client {
    fn receive(&mut self, now: NetTime, from: Peer, bytes: &[u8]) {
        Client::receive(self, now, from, bytes);
    }

    fn poll_transmit(&mut self, now: NetTime) -> Option<(Peer, Vec<u8>)> {
        Client::poll_transmit(self, now)
    }

    fn next_deadline(&self) -> Option<NetTime> {
        Client::next_deadline(self)
    }
}

/// A session shared by the game and its net thread.
pub type Shared<E> = Arc<Mutex<E>>;

/// The session, locked (a thread that panicked holding it left it as it
/// was: the session's state machines have no half-done steps to fear).
pub fn lock<E>(shared: &Mutex<E>) -> MutexGuard<'_, E> {
    shared
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

#[cfg(windows)]
struct WinTimerGuard;

#[cfg(windows)]
impl WinTimerGuard {
    fn enter() -> Self {
        // SAFETY: paired `timeBeginPeriod(1)` / `timeEndPeriod(1)` for the net thread lifetime.
        unsafe {
            windows_sys::Win32::Media::timeBeginPeriod(1);
        }
        Self
    }
}

#[cfg(windows)]
impl Drop for WinTimerGuard {
    fn drop(&mut self) {
        // SAFETY: pairs with `timeBeginPeriod(1)` in `WinTimerGuard::enter`.
        unsafe {
            windows_sys::Win32::Media::timeEndPeriod(1);
        }
    }
}

struct KeyframeWorker {
    jobs: Sender<EncodeJob>,
    done: Receiver<EncodedJob>,
    handle: Option<JoinHandle<()>>,
}

impl KeyframeWorker {
    fn start(waker: net::io::Waker) -> Option<Self> {
        let (jobs_tx, jobs_rx) = channel::<EncodeJob>();
        let (done_tx, done_rx) = channel::<EncodedJob>();
        let handle = std::thread::Builder::new()
            .name("kjk-keyframe".into())
            .spawn(move || {
                for job in jobs_rx {
                    let started = crate::perf::enabled().then(Instant::now);
                    let done = job.run();
                    if let Some(started) = started {
                        crate::perf::counter(
                            "net.encode",
                            &[("encode_ms", started.elapsed().as_secs_f64() * 1000.0)],
                        );
                    }
                    if done_tx.send(done).is_err() {
                        break;
                    }
                    waker.wake();
                }
            })
            .ok()?;
        Some(Self {
            jobs: jobs_tx,
            done: done_rx,
            handle: Some(handle),
        })
    }
}

impl Drop for KeyframeWorker {
    fn drop(&mut self) {
        let (dummy_tx, _) = channel();
        self.jobs = dummy_tx;
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}

struct EncodePool {
    jobs: Option<Sender<EncodeJob>>,
    done: Receiver<EncodedJob>,
    handles: Vec<JoinHandle<()>>,
}

impl EncodePool {
    fn start(workers: usize, waker: net::io::Waker) -> Option<Self> {
        let (jobs_tx, jobs_rx) = channel::<EncodeJob>();
        let jobs_rx = Arc::new(Mutex::new(jobs_rx));
        let (done_tx, done_rx) = channel::<EncodedJob>();
        let mut handles = Vec::with_capacity(workers);
        for index in 0..workers {
            let rx = Arc::clone(&jobs_rx);
            let tx = done_tx.clone();
            let worker_waker = waker.clone();
            let handle = std::thread::Builder::new()
                .name(format!("kjk-encode-{index}"))
                .spawn(move || {
                    loop {
                        let job = {
                            let Ok(guard) = rx.lock() else {
                                break;
                            };
                            guard.recv()
                        };
                        let Ok(job) = job else {
                            break;
                        };
                        let started = crate::perf::enabled().then(Instant::now);
                        let done = job.run();
                        if let Some(started) = started {
                            crate::perf::counter(
                                "net.encode",
                                &[("encode_ms", started.elapsed().as_secs_f64() * 1000.0)],
                            );
                        }
                        if tx.send(done).is_err() {
                            break;
                        }
                        worker_waker.wake();
                    }
                })
                .ok()?;
            handles.push(handle);
        }
        Some(Self {
            jobs: Some(jobs_tx),
            done: done_rx,
            handles,
        })
    }
}

impl Drop for EncodePool {
    fn drop(&mut self) {
        self.jobs = None;
        for handle in self.handles.drain(..) {
            let _ = handle.join();
        }
    }
}

/// A running net thread.
pub struct NetThread {
    stop: Arc<AtomicBool>,
    waker: net::io::Waker,
    handle: Option<JoinHandle<Netem<Socket>>>,
}

impl NetThread {
    /// Starts pumping `shared` through `socket`, stamping with `clock`.
    pub fn spawn<E: Endpoint>(
        mut socket: Netem<Socket>,
        shared: Shared<E>,
        clock: net::io::Clock,
    ) -> std::io::Result<NetThread> {
        let stop = Arc::new(AtomicBool::new(false));
        let stopping = stop.clone();
        let waker = socket.inner().waker();
        let thread_waker = waker.clone();
        let handle = std::thread::Builder::new()
            .name("kjk-net".into())
            .spawn(move || {
                #[cfg(windows)]
                let _timer = WinTimerGuard::enter();
                let mut arrived: Vec<Datagram> = Vec::new();
                let mut leaving: Vec<(Peer, Vec<u8>)> = Vec::new();
                let mut encode_jobs: Vec<EncodeJob> = Vec::new();
                let mut done_encoded: Vec<EncodedJob> = Vec::new();
                let mut keyframe_worker: Option<KeyframeWorker> = None;
                let mut encode_pool: Option<EncodePool> = None;
                while !stopping.load(Ordering::Relaxed) {
                    while let Some(datagram) = socket.recv() {
                        arrived.push(datagram);
                    }
                    if let Some(worker) = &keyframe_worker {
                        while let Ok(done) = worker.done.try_recv() {
                            done_encoded.push(done);
                        }
                    }
                    if let Some(pool) = &encode_pool {
                        while let Ok(done) = pool.done.try_recv() {
                            done_encoded.push(done);
                        }
                    }
                    let use_pool;
                    let mut deadline = {
                        let perf_t = crate::perf::enabled().then(Instant::now);
                        let mut session = lock(&shared);
                        use_pool = session.parallel_encode();
                        for datagram in arrived.drain(..) {
                            session.receive(datagram.at, datagram.from, &datagram.bytes);
                        }
                        let now = clock();
                        for done in done_encoded.drain(..) {
                            session.finish_encode(now, done);
                        }
                        while let Some(job) = session.poll_encode_job(now) {
                            encode_jobs.push(job);
                        }
                        if encode_jobs.is_empty() || use_pool {
                            while let Some(out) = session.poll_transmit(now) {
                                leaving.push(out);
                            }
                        }
                        let deadline = session.next_deadline();
                        drop(session);
                        if let Some(t) = perf_t {
                            crate::perf::counter(
                                "net.thread",
                                &[("mutex_hold_us", t.elapsed().as_secs_f64() * 1e6)],
                            );
                        }
                        deadline
                    };
                    if !encode_jobs.is_empty() {
                        for job in encode_jobs.drain(..) {
                            if job.is_keyframe() {
                                if keyframe_worker.is_none() {
                                    keyframe_worker = KeyframeWorker::start(thread_waker.clone());
                                }
                                match &keyframe_worker {
                                    Some(worker) => match worker.jobs.send(job) {
                                        Ok(()) => continue,
                                        Err(returned) => {
                                            keyframe_worker = None;
                                            done_encoded.push(returned.0.run());
                                        }
                                    },
                                    None => done_encoded.push(job.run()),
                                }
                            } else if use_pool {
                                if encode_pool.is_none() {
                                    encode_pool = EncodePool::start(4, thread_waker.clone());
                                }
                                match encode_pool.as_ref().and_then(|pool| pool.jobs.as_ref()) {
                                    Some(jobs) => match jobs.send(job) {
                                        Ok(()) => continue,
                                        Err(returned) => {
                                            encode_pool = None;
                                            done_encoded.push(returned.0.run());
                                        }
                                    },
                                    None => done_encoded.push(job.run()),
                                }
                            } else {
                                let started = crate::perf::enabled().then(Instant::now);
                                let done = job.run();
                                if let Some(started) = started {
                                    crate::perf::counter(
                                        "net.encode",
                                        &[("encode_ms", started.elapsed().as_secs_f64() * 1000.0)],
                                    );
                                }
                                done_encoded.push(done);
                            }
                        }
                        if !done_encoded.is_empty() {
                            let perf_t = crate::perf::enabled().then(Instant::now);
                            let mut session = lock(&shared);
                            let now = clock();
                            for done in done_encoded.drain(..) {
                                session.finish_encode(now, done);
                            }
                            while let Some(out) = session.poll_transmit(now) {
                                leaving.push(out);
                            }
                            deadline = session.next_deadline();
                            drop(session);
                            if let Some(t) = perf_t {
                                crate::perf::counter(
                                    "net.thread",
                                    &[("mutex_hold_us", t.elapsed().as_secs_f64() * 1e6)],
                                );
                            }
                        }
                    }
                    let now = clock();
                    socket.poll(now);
                    for (to, bytes) in leaving.drain(..) {
                        socket.send(to, &bytes);
                    }
                    socket.poll(now);
                    let wake = [deadline, socket.next_due()]
                        .into_iter()
                        .flatten()
                        .min()
                        .map_or(MAX_SLEEP, |at| {
                            Duration::from_micros(at.since(now)).clamp(MIN_SLEEP, MAX_SLEEP)
                        });
                    socket.inner().wait(wake);
                }
                socket
            })?;
        Ok(NetThread {
            stop,
            waker,
            handle: Some(handle),
        })
    }

    /// Wakes the net thread immediately so outbound work leaves without
    /// waiting for the sleep interval.
    pub fn wake(&self) {
        self.waker.wake();
    }

    /// Stops the thread; its socket back.
    pub fn stop(mut self) -> Option<Netem<Socket>> {
        self.halt()
    }

    fn halt(&mut self) -> Option<Netem<Socket>> {
        self.stop.store(true, Ordering::Relaxed);
        self.waker.wake();
        self.handle.take()?.join().ok()
    }
}

impl Drop for NetThread {
    fn drop(&mut self) {
        let _ = self.halt();
    }
}
