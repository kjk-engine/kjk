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
use std::sync::{Arc, Mutex, MutexGuard};
use std::thread::JoinHandle;
use std::time::Duration;

use net::io::Socket;
use net::netem::Netem;
use net::session::Client;
use net::{Datagram, NetTime, Peer, Transport};

/// The longest the thread sleeps: what the game queued waits at most this.
const MAX_SLEEP: Duration = Duration::from_millis(1);
/// The shortest: a deadline that passed and stays (nothing it can do
/// yet) never spins the thread.
const MIN_SLEEP: Duration = Duration::from_micros(50);

/// What a net thread pumps: datagrams in, datagrams out, and when it must
/// next look.
pub trait Endpoint: Send + 'static {
    fn receive(&mut self, now: NetTime, from: Peer, bytes: &[u8]);
    fn poll_transmit(&mut self, now: NetTime) -> Option<(Peer, Vec<u8>)>;
    fn next_deadline(&self) -> Option<NetTime>;
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

/// A running net thread.
pub struct NetThread {
    stop: Arc<AtomicBool>,
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
        let handle = std::thread::Builder::new()
            .name("kjk-net".into())
            .spawn(move || {
                let mut arrived: Vec<Datagram> = Vec::new();
                let mut leaving: Vec<(Peer, Vec<u8>)> = Vec::new();
                while !stopping.load(Ordering::Relaxed) {
                    while let Some(datagram) = socket.recv() {
                        arrived.push(datagram);
                    }
                    let deadline = {
                        let mut session = lock(&shared);
                        for datagram in arrived.drain(..) {
                            session.receive(datagram.at, datagram.from, &datagram.bytes);
                        }
                        let now = clock();
                        while let Some(out) = session.poll_transmit(now) {
                            leaving.push(out);
                        }
                        session.next_deadline()
                    };
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
            handle: Some(handle),
        })
    }

    /// Stops the thread; its socket back.
    pub fn stop(mut self) -> Option<Netem<Socket>> {
        self.halt()
    }

    fn halt(&mut self) -> Option<Netem<Socket>> {
        self.stop.store(true, Ordering::Relaxed);
        self.handle.take()?.join().ok()
    }
}

impl Drop for NetThread {
    fn drop(&mut self) {
        let _ = self.halt();
    }
}
