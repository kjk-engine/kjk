//! A thread that does one kind of work beside the frame. It starts at its
//! first job and then waits for the next one, so a host that checks every
//! tick does not start a new thread for each.

use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::mpsc::{Receiver, Sender, TryRecvError, channel};
use std::sync::{Mutex, PoisonError};

type Job = Box<dyn FnOnce() + Send>;

pub(crate) struct Worker {
    name: &'static str,
    jobs: Mutex<Option<Sender<Job>>>,
}

/// A job's result, once the worker has finished it. It is `None` when the
/// job panicked.
#[derive(Debug)]
pub(crate) struct Queued<T> {
    // A `Receiver` alone is not `Sync`, and a client must be.
    result: Mutex<Receiver<T>>,
    landed: Option<Option<T>>,
}

impl Worker {
    pub(crate) const fn new(name: &'static str) -> Worker {
        Worker {
            name,
            jobs: Mutex::new(None),
        }
    }

    /// Queues `work` behind the jobs already given. It returns `None` when
    /// no thread could be started, and the caller then does the work itself.
    pub(crate) fn run<T: Send + 'static>(
        &self,
        work: impl FnOnce() -> T + Send + 'static,
    ) -> Option<Queued<T>> {
        let (done, result) = channel();
        let job: Job = Box::new(move || {
            let _ = done.send(work());
        });
        let mut jobs = self.jobs.lock().unwrap_or_else(PoisonError::into_inner);
        if jobs.is_none() {
            *jobs = start(self.name);
        }
        jobs.as_ref()?.send(job).ok()?;
        Some(Queued {
            result: Mutex::new(result),
            landed: None,
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
