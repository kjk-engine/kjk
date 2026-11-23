//! Checks of a bake's fingerprint, on fingerprints made here (no install):
//! its text reads back, and the verdict follows the parts that differ.

use super::{FORMAT, Fingerprint, Verdict, parse, text, verdict};
use crate::probe_kit::{Checks, check};

/// The checks, for `lighting_fingerprints` in the approved tests.
pub fn probes() -> Checks {
    vec![
        ("the_text_reads_back", the_text_reads_back()),
        ("a_verdict_follows_the_parts", a_verdict_follows_the_parts()),
    ]
}

/// A fingerprint written as the text of its file reads back the same.
fn the_text_reads_back() -> Result<String, String> {
    let print = Fingerprint {
        area: "101per".into(),
        format: FORMAT,
        rooms: "0123456789abcdef0123456789abcdef".into(),
        placeables: "fedcba9876543210fedcba9876543210".into(),
    };
    let read = parse(&text(&print));
    check(
        read.as_ref() == Some(&print),
        format!("{print:?} read back as {read:?}"),
    )
}

/// The same fingerprint gives the whole bake. Other placeables give its
/// lightmaps alone. Other rooms, or another format, refuse it.
fn a_verdict_follows_the_parts() -> Result<String, String> {
    let stamp = Fingerprint {
        area: "a".into(),
        format: FORMAT,
        rooms: "r".into(),
        placeables: "p".into(),
    };
    let same = verdict(&stamp, &stamp);
    check(same == Verdict::Whole, format!("the same gives {same:?}"))?;
    let moved = Fingerprint {
        placeables: "q".into(),
        ..stamp.clone()
    };
    let moved = verdict(&stamp, &moved);
    check(
        moved == Verdict::LightmapsOnly,
        format!("other placeables give {moved:?}"),
    )?;
    let rebuilt = Fingerprint {
        rooms: "s".into(),
        ..stamp.clone()
    };
    let rebuilt = verdict(&stamp, &rebuilt);
    check(
        matches!(rebuilt, Verdict::Refused(_)),
        format!("other rooms give {rebuilt:?}"),
    )?;
    let older = Fingerprint {
        format: FORMAT + 1,
        ..stamp.clone()
    };
    let older = verdict(&stamp, &older);
    check(
        matches!(older, Verdict::Refused(_)),
        format!("another format gives {older:?}"),
    )
}
