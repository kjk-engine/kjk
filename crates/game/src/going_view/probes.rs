//! The probes of `going_view` (`kjk sim probes`, group "destroyed
//! placeables"): which of the module's placeables are hidden, frame after
//! frame, as the snapshots draw them.

use std::collections::BTreeSet;

use sim::probe::check;

use super::Fading;

/// The module's placeables in these checks: numbers 0, 1 and 2.
const MODULE: [u32; 3] = [0, 1, 2];

/// The placeables hidden after each frame, given the numbers each frame's
/// snapshot draws and whether that snapshot is of the scene's module. A
/// placeable counts as hidden once `placed_gone` names it, as the view
/// hides it then.
fn hidden_after(frames: &[(&[u32], bool)]) -> Vec<Vec<u32>> {
    let mut fading = Fading::default();
    frames
        .iter()
        .map(|&(present, same_module)| {
            if same_module {
                let present: BTreeSet<u32> = present.iter().copied().collect();
                let gone = fading.placed_gone(present, || MODULE.to_vec());
                fading.placed_hidden.extend(gone);
            }
            fading.placed_hidden.iter().copied().collect()
        })
        .collect()
}

/// A check's name, its frames (as `hidden_after` takes them) and the
/// placeables hidden after each frame.
type Case = (
    &'static str,
    &'static [(&'static [u32], bool)],
    Vec<Vec<u32>>,
);

pub(super) fn checks() -> sim::probe::Checks {
    let cases: [Case; 4] = [
        // A save loaded in which a script had destroyed placeable 1: its
        // module's first snapshot lacks it, and it is hidden at once.
        (
            "destroyed_before_the_scene",
            &[(&[0, 2], true), (&[0, 2], true)],
            vec![vec![1], vec![1]],
        ),
        // A script destroys placeable 2 while the scene is drawn.
        (
            "destroyed_in_the_scene",
            &[(&[0, 1, 2], true), (&[0, 1], true)],
            vec![vec![], vec![2]],
        ),
        // A guest draws the last module's snapshot over the new scene for
        // a while: nothing is hidden until a snapshot of this module comes.
        (
            "another_modules_snapshot",
            &[(&[], false), (&[], false), (&[0, 1, 2], true)],
            vec![vec![], vec![], vec![]],
        ),
        // Once the scene's module has been seen, a stale snapshot from
        // another module hides nothing.
        (
            "another_modules_snapshot_after_seen",
            &[(&[0, 1, 2], true), (&[], false), (&[0, 1, 2], true)],
            vec![vec![], vec![], vec![]],
        ),
    ];
    cases
        .into_iter()
        .map(|(name, frames, want)| {
            let got = hidden_after(frames);
            (
                name,
                check(
                    got == want,
                    format!("hidden after each frame {got:?}, want {want:?}"),
                ),
            )
        })
        .collect()
}
