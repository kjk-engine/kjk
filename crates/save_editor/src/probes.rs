//! The editor's own checks, run by the approved test `save_edits` beside
//! `edit::probes`: each a name and what it found, or why it failed.

mod catalog;
mod locate;
mod session;

type Probe = (&'static str, Result<String, String>);

/// Every check, in order.
pub fn all() -> Vec<Probe> {
    vec![
        ("locate_saves_folder", locate::saves_folder()),
        ("locate_installs", locate::installs()),
        ("locate_game_of_save", locate::game_of_save()),
        ("catalog_search", catalog::search()),
        ("catalog_k1", catalog::install(vfs::Game::Kotor1)),
        ("catalog_tsl", catalog::install(vfs::Game::Kotor2)),
        ("catalog_tsl_own_items", catalog::tsl_own_items()),
        ("catalog_creature_k1", catalog::creature_k1()),
        ("session_undo_exact", session::undo_exact()),
        ("session_told_in_words", session::told_in_words()),
        ("session_feat_warned_k1", session::feat_warned_k1()),
        ("session_give_equipped", session::give_equipped()),
    ]
}
