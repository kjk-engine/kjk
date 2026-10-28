//! The clip-brush override files built into this binary, from
//! `data/clip/<game>/<module>.clip` (`collision::clip_file` has the format).
//! They are our own data; building them in means every machine on the same
//! commit clips a module the same, which the join handshake's build check
//! already covers.

include!(concat!(env!("OUT_DIR"), "/overrides.rs"));

/// The override file for `module` of `game` (`k1`, `tsl`), if there is one.
pub fn text(game: &str, module: &str) -> Option<&'static str> {
    FILES
        .iter()
        .find(|(g, m, _)| g.eq_ignore_ascii_case(game) && m.eq_ignore_ascii_case(module))
        .map(|&(_, _, text)| text)
}

/// Every built-in file: (game, module).
pub fn all() -> impl Iterator<Item = (&'static str, &'static str)> {
    FILES.iter().map(|&(game, module, _)| (game, module))
}
