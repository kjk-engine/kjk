//! The editor's own checks, run by the approved test `save_edits` beside
//! `edit::probes`: each a name and what it found, or why it failed.

mod catalog;
mod locate;

type Probe = (&'static str, Result<String, String>);

/// Every check, in order.
pub fn all() -> Vec<Probe> {
    vec![
        ("locate_saves_folder", locate::saves_folder()),
        ("locate_installs", locate::installs()),
        ("catalog_search", catalog::search()),
        ("catalog_k1", catalog::install(vfs::Game::Kotor1)),
        ("catalog_tsl", catalog::install(vfs::Game::Kotor2)),
        ("catalog_tsl_own_items", catalog::tsl_own_items()),
    ]
}
