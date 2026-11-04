//! The editor's own checks, run by the approved test `save_edits` beside
//! `edit::probes`: each a name and what it found, or why it failed.

type Probe = (&'static str, Result<String, String>);

/// Every check, in order.
pub fn all() -> Vec<Probe> {
    vec![]
}
