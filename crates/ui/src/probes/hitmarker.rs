//! The mark a landed blaster shot flashes (`screens::hud::hitmarker`):
//! four strokes, a kill's four more, shown for `MARK_SHOWN_MS`, fading.

use super::{Check, check, expect};
use crate::screens::hud::{MARK_SHOWN_MS, Mark, hitmarker};

const FRAME: (i32, i32) = (1920, 1080);

/// The lightest stroke's alpha in `quads` (each stroke's light over its
/// edge: the second of each pair).
fn alpha(quads: &[crate::draw::Quad]) -> f32 {
    quads
        .iter()
        .skip(1)
        .step_by(2)
        .map(|quad| quad.color[3])
        .fold(0.0, f32::max)
}

pub(super) fn all() -> Vec<Check> {
    let counts: Vec<(Mark, usize)> = [
        Mark::Hit,
        Mark::Strong,
        Mark::Turned,
        Mark::Kill { strong: false },
    ]
    .iter()
    .map(|&mark| (mark, hitmarker(FRAME, mark, 20.0, 0.0).len()))
    .collect();
    let counted = counts.iter().all(|&(mark, n)| {
        n == if matches!(mark, Mark::Kill { .. }) {
            16
        } else {
            8
        }
    });
    let fresh = alpha(&hitmarker(FRAME, Mark::Hit, 20.0, 0.0));
    let fading = alpha(&hitmarker(FRAME, Mark::Hit, 20.0, 100.0));
    let gone = hitmarker(FRAME, Mark::Hit, 20.0, MARK_SHOWN_MS + 1.0).len();
    vec![check(
        "hitmarker_fades",
        expect(
            &format!("quads {counts:?}; alpha new {fresh}, at 100 ms {fading}; after {gone} quads"),
            counted && fading < fresh && gone == 0,
        )
        .map(|()| "four strokes (a kill's eight), fading, gone after the flash".into()),
    )]
}
