//! A thread that does one kind of work beside the frame. It starts at its
//! first job and then waits for the next one, so a host that checks every
//! tick does not start a new thread for each.

use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::mpsc::{Receiver, Sender, TryRecvError, channel};
use std::sync::{Arc, Condvar, Mutex, PoisonError};

/// An opaque handle allowing a caller to wait for an in-flight background
/// job without holding a lock on the session.
#[derive(Clone, Debug)]
pub struct PendingSignal {
    inner: Arc<(Mutex<u64>, Condvar)>,
    target: u64,
}

impl PendingSignal {
    /// Blocks until the signalled job finishes (or its worker panics).
    pub fn wait(self) {
        let (lock, cv) = &*self.inner;
        let mut guard = lock.lock().unwrap_or_else(PoisonError::into_inner);
        while *guard < self.target {
            guard = cv.wait(guard).unwrap_or_else(PoisonError::into_inner);
        }
    }
}

#[derive(Clone, Default, Debug)]
pub(crate) struct WaitSignal(Arc<(Mutex<u64>, Condvar)>);

impl WaitSignal {
    pub(crate) fn new() -> Self {
        Self(Arc::new((Mutex::new(0), Condvar::new())))
    }

    pub(crate) fn pending(&self, target: u64) -> PendingSignal {
        PendingSignal {
            inner: self.0.clone(),
            target,
        }
    }

    pub(crate) fn finish(&self, target: u64) {
        let (lock, cv) = &*self.0;
        let mut guard = lock.lock().unwrap_or_else(PoisonError::into_inner);
        if *guard < target {
            *guard = target;
        }
        cv.notify_all();
    }
}

pub(crate) struct NotifyOnDrop {
    pub signal: WaitSignal,
    pub target: u64,
}

impl Drop for NotifyOnDrop {
    fn drop(&mut self) {
        self.signal.finish(self.target);
    }
}

type Job = Box<dyn FnOnce() + Send>;

pub(crate) struct Worker {
    name: &'static str,
    jobs: Mutex<(Option<Sender<Job>>, u64, Option<WaitSignal>)>,
}

/// A job's result, once the worker has finished it. It is `None` when the
/// job panicked.
#[derive(Debug)]
pub(crate) struct Queued<T> {
    // A `Receiver` alone is not `Sync`, and a client must be.
    result: Mutex<Receiver<T>>,
    landed: Option<Option<T>>,
    signal: PendingSignal,
}

impl Worker {
    pub(crate) const fn new(name: &'static str) -> Worker {
        Worker {
            name,
            jobs: Mutex::new((None, 0, None)),
        }
    }

    /// Queues `work` behind the jobs already given. It returns `None` when
    /// no thread could be started, and the caller then does the work itself.
    pub(crate) fn run<T: Send + 'static>(
        &self,
        work: impl FnOnce() -> T + Send + 'static,
    ) -> Option<Queued<T>> {
        let (done, result) = channel();
        let mut state = self.jobs.lock().unwrap_or_else(PoisonError::into_inner);
        if state.0.is_none() {
            state.0 = start(self.name);
        }
        state.1 += 1;
        let target = state.1;
        let signal = state.2.get_or_insert_with(WaitSignal::new).clone();
        let pending = signal.pending(target);
        let job: Job = Box::new(move || {
            let _notify = NotifyOnDrop { signal, target };
            let value = work();
            let _ = done.send(value);
        });
        state.0.as_ref()?.send(job).ok()?;
        Some(Queued {
            result: Mutex::new(result),
            landed: None,
            signal: pending,
        })
    }
}

fn start(name: &str) -> Option<Sender<Job>> {
    let (jobs, given) = channel::<Job>();
    std::thread::Builder::new()
        .name(name.into())
        .spawn(move || {
            // A panicking job must not take the jobs queued after it down.
            for job in given {
                let _ = catch_unwind(AssertUnwindSafe(job));
            }
        })
        .ok()?;
    Some(jobs)
}

impl<T> Queued<T> {
    pub(crate) fn is_finished(&mut self) -> bool {
        if self.landed.is_none() {
            let result = self
                .result
                .get_mut()
                .unwrap_or_else(PoisonError::into_inner);
            match result.try_recv() {
                Ok(result) => self.landed = Some(Some(result)),
                Err(TryRecvError::Disconnected) => self.landed = Some(None),
                Err(TryRecvError::Empty) => {}
            }
        }
        self.landed.is_some()
    }

    pub(crate) fn pending(&self) -> PendingSignal {
        self.signal.clone()
    }

    pub(crate) fn wait(self) -> Option<T> {
        match self.landed {
            Some(landed) => landed,
            None => (self.result.into_inner())
                .unwrap_or_else(PoisonError::into_inner)
                .recv()
                .ok(),
        }
    }
}
