//! A save against a module whose placements changed (`Snapshot::roster`).
//!
//! An object's number is its place in the module's list: the level's
//! placements in GIT order, then what play made (`crate::instance`). A data
//! change that adds or drops a placement moves every number after it, so a
//! save made before it points its entities, scripts and spawned objects at
//! the wrong ones. The roster is what the placements were when the module
//! was first installed; `renumber` says where each of them stands now.

use std::collections::HashMap;
use std::sync::Arc;

use crate::module::{ObjectData, ObjectKind};
use crate::wire::{DecodeError, Reader, Writer};

/// What a placement was, as the roster keeps it: enough to find it again in
/// a changed list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Origin {
    pub kind: ObjectKind,
    pub template: String,
    pub tag: String,
}

/// The origins of a module's placements, in their order.
pub fn roster(placed: &[Arc<ObjectData>]) -> Vec<Origin> {
    placed
        .iter()
        .map(|object| Origin {
            kind: object.kind,
            template: object.template.clone(),
            tag: object.tag.clone(),
        })
        .collect()
}

/// Where the objects of an old numbering stand in the new one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Renumber {
    /// The new number of each old placement, by its old number; `None`
    /// where the data no longer has it.
    pub map: Vec<Option<u32>>,
    /// What an object made in play, numbered `old >= map.len()`, is
    /// numbered now: `old + made_shift`.
    pub made_shift: i64,
    /// The new placements the old roster did not have, ascending.
    pub added: Vec<u32>,
}

/// A placement's identity: kind, template and tag without case.
fn key(kind: ObjectKind, template: &str, tag: &str) -> (ObjectKind, String, String) {
    (kind, template.to_lowercase(), tag.to_lowercase())
}

/// Matches the old placements to the new: the n-th of a key in `old` is the
/// n-th of it in `new`, so two alike keep their order.
pub fn renumber(old: &[Origin], new: &[Arc<ObjectData>]) -> Renumber {
    let mut found: HashMap<_, Vec<u32>> = HashMap::new();
    for (at, object) in new.iter().enumerate() {
        found
            .entry(key(object.kind, &object.template, &object.tag))
            .or_default()
            .push(at as u32);
    }
    let mut seen: HashMap<_, usize> = HashMap::new();
    let map: Vec<Option<u32>> = old
        .iter()
        .map(|origin| {
            let key = key(origin.kind, &origin.template, &origin.tag);
            let nth = seen.entry(key.clone()).or_default();
            let matched = found.get(&key).and_then(|at| at.get(*nth)).copied();
            *nth += 1;
            matched
        })
        .collect();
    let taken: std::collections::HashSet<u32> = map.iter().flatten().copied().collect();
    Renumber {
        added: (0..new.len() as u32)
            .filter(|at| !taken.contains(at))
            .collect(),
        made_shift: new.len() as i64 - old.len() as i64,
        map,
    }
}

/// Writes a roster, or its absence.
pub(crate) fn encode(roster: Option<&[Origin]>, out: &mut Writer) {
    let Some(roster) = roster else {
        out.u8(0);
        return;
    };
    out.u8(1);
    out.u32(roster.len() as u32);
    for origin in roster {
        out.u8(origin.kind.index());
        out.string(&origin.template);
        out.string(&origin.tag);
    }
}

/// Reads what `encode` wrote.
pub(crate) fn decode(input: &mut Reader) -> Result<Option<Vec<Origin>>, DecodeError> {
    let at = input.at();
    match input.u8()? {
        0 => Ok(None),
        1 => (0..input.u32()?)
            .map(|_| {
                let at = input.at();
                let kind = ObjectKind::ALL
                    .get(usize::from(input.u8()?))
                    .copied()
                    .ok_or(input.invalid("roster kind", at))?;
                Ok(Origin {
                    kind,
                    template: input.string()?,
                    tag: input.string()?,
                })
            })
            .collect::<Result<_, DecodeError>>()
            .map(Some),
        _ => Err(input.invalid("roster flag", at)),
    }
}
