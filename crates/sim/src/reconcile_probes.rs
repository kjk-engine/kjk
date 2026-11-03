//! Probes for `reconcile`: the number map over changed placement lists, and
//! the roster's encoding.

use std::sync::Arc;

use script_ncs::probes::Check;

use crate::module::{ObjectData, ObjectKind};
use crate::reconcile::{Renumber, decode, encode, renumber, roster};
use crate::wire::{Reader, Writer};

/// A placement from `template/tag` shorthand: a creature, or a door when
/// the text starts `door:`.
fn placed(text: &str) -> Arc<ObjectData> {
    let (kind, rest) = match text.split_once(':') {
        Some(("door", rest)) => (ObjectKind::Door, rest),
        _ => (ObjectKind::Creature, text),
    };
    let (template, tag) = rest.split_once('/').unwrap_or((rest, ""));
    Arc::new(ObjectData {
        kind,
        tag: tag.into(),
        ..ObjectData::unknown(template, [0.0; 3], 0.0)
    })
}

fn list(texts: &[&str]) -> Vec<Arc<ObjectData>> {
    texts.iter().map(|text| placed(text)).collect()
}

fn check(name: &'static str, ok: bool, detail: String) -> Check {
    Check { name, ok, detail }
}

/// Every case of the number map, and the roster's round trip.
pub fn checks() -> Vec<Check> {
    let some = |numbers: &[u32]| numbers.iter().map(|&n| Some(n)).collect::<Vec<_>>();
    let cases: [(&'static str, &[&str], &[&str], Renumber); 5] = [
        (
            "unchanged is the identity",
            &["a/x", "b/y", "c/z"],
            &["a/x", "b/y", "c/z"],
            Renumber {
                map: some(&[0, 1, 2]),
                made_shift: 0,
                added: vec![],
            },
        ),
        (
            "one inserted at 2 of 5",
            &["a/1", "b/2", "c/3", "d/4", "e/5"],
            &["a/1", "b/2", "new/n", "c/3", "d/4", "e/5"],
            Renumber {
                map: some(&[0, 1, 3, 4, 5]),
                made_shift: 1,
                added: vec![2],
            },
        ),
        (
            "one removed at 1",
            &["a/1", "b/2", "c/3"],
            &["a/1", "c/3"],
            Renumber {
                map: vec![Some(0), None, Some(1)],
                made_shift: -1,
                added: vec![],
            },
        ),
        (
            "the n-th of a key is the n-th",
            &["a/x", "a/x", "door:d/x"],
            &["door:d/x", "a/x", "a/x"],
            Renumber {
                map: some(&[1, 2, 0]),
                made_shift: 0,
                added: vec![],
            },
        ),
        (
            "case does not matter, kind does",
            &["A/X", "door:a/x"],
            &["door:a/x", "a/x"],
            Renumber {
                map: some(&[1, 0]),
                made_shift: 0,
                added: vec![],
            },
        ),
    ];
    let mut out: Vec<Check> = cases
        .into_iter()
        .map(|(name, old, new, expected)| {
            let got = renumber(&roster(&list(old)), &list(new));
            check(name, got == expected, format!("{got:?}"))
        })
        .collect();

    let kept = roster(&list(&["a/x", "door:d/y"]));
    let rosters = [None, Some(kept), Some(Vec::new())];
    let round_trips = rosters.iter().all(|roster| {
        let mut bytes = Writer::default();
        encode(roster.as_deref(), &mut bytes);
        let mut input = Reader::new("roster", &bytes.0);
        decode(&mut input).is_ok_and(|read| read == *roster) && input.finish().is_ok()
    });
    out.push(check(
        "a roster round trips",
        round_trips,
        format!("{} rosters", rosters.len()),
    ));
    out
}
