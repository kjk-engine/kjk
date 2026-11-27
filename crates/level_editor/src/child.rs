//! The `kjk` runs the editor starts and waits for: a bake, an audit, a try
//! of a script or a conversation. Each runs on a thread of its own while
//! the editor keeps working. The editor keeps them here so that it stops
//! them when it closes (`stop_all`). A run left going would bake or step on
//! for a result that no editor is there to take.
//!
//! Play and walk mode are not kept here. They open a window of their own,
//! which the person closes.

use std::io::Read;
use std::process::{Child, Command, Output, Stdio};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

/// The runs going now.
static RUNNING: Mutex<Vec<Arc<Mutex<Child>>>> = Mutex::new(Vec::new());

/// How often a run is asked whether it has ended.
const EVERY: Duration = Duration::from_millis(50);

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// Everything `pipe` gives until it closes, read on a thread of its own, so
/// a run that writes much to one pipe does not wait on the other.
fn read_all<R: Read + Send + 'static>(pipe: Option<R>) -> std::thread::JoinHandle<Vec<u8>> {
    std::thread::spawn(move || {
        let mut bytes = Vec::new();
        if let Some(mut pipe) = pipe {
            let _ = pipe.read_to_end(&mut bytes);
        }
        bytes
    })
}

/// Runs `command` to its end and gives what it wrote, as `Command::output`
/// does. The run is kept while it goes, so `stop_all` can stop it: its
/// status is then the one of a run that was stopped.
pub fn output(mut command: Command) -> std::io::Result<Output> {
    command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = command.spawn()?;
    let stdout = read_all(child.stdout.take());
    let stderr = read_all(child.stderr.take());
    let child = Arc::new(Mutex::new(child));
    lock(&RUNNING).push(child.clone());
    let status = loop {
        match lock(&child).try_wait() {
            Ok(Some(status)) => break Ok(status),
            Ok(None) => std::thread::sleep(EVERY),
            Err(error) => break Err(error),
        }
    };
    lock(&RUNNING).retain(|kept| !Arc::ptr_eq(kept, &child));
    Ok(Output {
        status: status?,
        stdout: stdout.join().unwrap_or_else(|_| Vec::new()),
        stderr: stderr.join().unwrap_or_else(|_| Vec::new()),
    })
}

/// Stops every run still going. The editor calls it as it closes.
pub fn stop_all() {
    for child in lock(&RUNNING).drain(..) {
        let _ = lock(&child).kill();
    }
}
