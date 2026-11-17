//! The warning a script's call of a routine the engine does not carry out
//! gives. The VM goes on as before: the call returns its type's empty value
//! (an action call still takes its place in the queue), and an effect the
//! sim does not carry out is applied as nothing. Each such routine is
//! warned about once for the whole run, naming the routine and the script
//! that called it first, on the standard error. Mods' scripts call routines
//! the games' own never do, and the warning says which.

use std::collections::BTreeMap;
use std::sync::Mutex;

/// The routines warned about, and the warning each gave.
static WARNED: Mutex<BTreeMap<String, String>> = Mutex::new(BTreeMap::new());

/// Warns that `script` called `routine`, which does `what` here, unless
/// that routine was warned about before. Whether this call warned.
pub(crate) fn warn(routine: &str, script: &str, what: &str) -> bool {
    let mut warned = WARNED
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    if warned.contains_key(routine) {
        return false;
    }
    let line = format!(
        "warning: the script {script} called {routine}, which kjk does not carry out. {what}"
    );
    eprintln!("{line}");
    warned.insert(routine.to_owned(), line);
    true
}

/// The warning given about `routine`, if one was.
pub fn warning(routine: &str) -> Option<String> {
    WARNED
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .get(routine)
        .cloned()
}
