//! A save against a module whose placements changed (`Snapshot::roster`).
//!
//! An object's number is its place in the module's list: the level's
//! placements in GIT order, then what play made (`crate::instance`). A data
//! change that adds or drops a placement moves every number after it, so a
//! save made before it points its entities, scripts and spawned objects at
//! the wrong ones. The roster is what the placements were when the module
//! was first installed; `renumber` says where each of them stands now, and
//! `reconcile` brings the save's objects onto the new numbers as it loads
//! (`Save::world`) and as a module left behind is entered again
//! (`install::assemble`).
//!
//! What holds an object's number, and what `apply` does with it:
//! - `EntityState::kind`, `Kind::Object(kind, number)`, of the snapshot's
//!   entities and of each module left (`Stashed::entities`): rewritten. A
//!   placement the data no longer has drops its entity, but for a party
//!   member's, which stays where it stands as an object made in play.
//! - `Snapshot::spawned` and `Stashed::spawned`, what play made: numbered
//!   after the placements in their order, so their numbers move with the
//!   placements' count (`Renumber::made_shift`); the list itself holds no
//!   number. A party member kept is added to its end.
//! - The world's own tables, made again from the two at every load and
//!   install: `world::Object::index`, `Instances`, the collision's door
//!   slots (`Level::states`). Nothing to rewrite.
//!
//! Everything else names an object by its entity's `SimId`, which a script
//! sees as its `ObjectId` (`ObjectId(entity.id.0)`): a door's open state and
//! its lock (`ObjectState`, on the entity itself), the scripts' locals,
//! `last` (opener, user, damager, killer), action queues, deferred code and
//! events and their callers, `uncommandable`, a trigger's occupants, what
//! each creature perceives, rounds, what a module left keeps waiting
//! (`script::Left`), conversations' owners and speakers, the party table's
//! creatures (`Npc::object`), items' holders (a container's contents), the
//! scripts' visuals on objects (`OnObject`), a transition's door
//! (`Transition::via`) and the party's leader (`Snapshot::leading`). They
//! follow their objects as the entities keep their ids, so `apply` leaves
//! them; a dropped entity's are left as a destroyed object's are (its queue
//! goes at the next step, its locals and goods are not read again).

use std::collections::{BTreeSet, HashMap, HashSet};
use std::fmt;
use std::sync::Arc;

use crate::instance::Instances;
use crate::module::{ObjectData, ObjectKind};
use crate::snapshot::{EntityState, Kind, SimId, Spawned};
use crate::wire::{DecodeError, Reader, Writer};
use crate::world::Level;

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
    let taken: HashSet<u32> = map.iter().flatten().copied().collect();
    Renumber {
        added: (0..new.len() as u32)
            .filter(|at| !taken.contains(at))
            .collect(),
        made_shift: new.len() as i64 - old.len() as i64,
        map,
    }
}

/// A module's saved objects, as a snapshot or a module left holds them.
pub struct Held<'a> {
    pub entities: &'a mut Vec<EntityState>,
    pub spawned: &'a mut Vec<Spawned>,
    pub roster: &'a mut Option<Vec<Origin>>,
    /// The world's next id, which the new placements' entities take.
    pub next_id: &'a mut u32,
}

/// What reconciling a module's saved objects did: one `save-migrate` line.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Report {
    pub module: String,
    /// Saved before rosters were kept: its numbers are trusted, checked
    /// only for their kind.
    pub trusted: bool,
    /// The entities whose number changed.
    pub moved: usize,
    /// What the dropped entities stood for.
    pub dropped: Vec<String>,
    /// The entities of the placements new in the data, which come alive as
    /// on a first arrival (`SimWorld::reconciled_with`).
    pub added: Vec<SimId>,
    /// The entities moved onto walkable ground.
    pub snapped: usize,
}

impl fmt::Display for Report {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.trusted {
            write!(
                f,
                "{}: no placements kept with the save, its numbers trusted",
                self.module
            )?;
        } else {
            write!(
                f,
                "{}: placements changed since the save: {} moved, {} added, {} snapped",
                self.module,
                self.moved,
                self.added.len(),
                self.snapped
            )?;
        }
        write!(f, ", {} dropped", self.dropped.len())?;
        if !self.dropped.is_empty() {
            write!(f, " ({})", self.dropped.join(", "))?;
        }
        Ok(())
    }
}

/// `held` brought onto `level`'s placements: renumbered by its roster
/// (`apply`), else its numbers trusted where they name an object of their
/// kind (`trust`). Its roster is `level`'s from then on.
pub fn reconcile(mut held: Held, level: &Level, party: &BTreeSet<SimId>) -> Report {
    let placed = level.placed();
    let mut report = match held.roster.take() {
        Some(old) => apply(&renumber(&old, placed), &old, level, &mut held, party),
        None => trust(level, &mut held),
    };
    report.module = level.id().name;
    *held.roster = Some(roster(placed));
    report
}

/// `held`'s objects numbered as `renumber` says (`old` the roster it was
/// made from): an entity whose placement is gone is dropped, but for a
/// party member's (in `party`), which stays as an object made in play; the
/// new placements stand as the module's first install stands them
/// (`install::anew`).
pub fn apply(
    renumber: &Renumber,
    old: &[Origin],
    level: &Level,
    held: &mut Held,
    party: &BTreeSet<SimId>,
) -> Report {
    let placed = level.placed().len() as u32;
    let mut report = Report::default();
    for mut entity in std::mem::take(held.entities) {
        if let Kind::Object(kind, number) = entity.kind {
            let now = match renumber.map.get(number as usize) {
                Some(&Some(now)) => now,
                Some(None) if party.contains(&entity.id) => {
                    let now = placed + held.spawned.len() as u32;
                    held.spawned.push(Spawned {
                        kind,
                        template: old[number as usize].template.clone(),
                        yaw: entity.player.map_or(0.0, |player| player.view[1]),
                        holds: Vec::new(),
                    });
                    now
                }
                Some(None) => {
                    report.dropped.push(describe(&old[number as usize]));
                    continue;
                }
                None => (i64::from(number) + renumber.made_shift) as u32,
            };
            report.moved += usize::from(now != number);
            entity.kind = Kind::Object(kind, now);
        }
        held.entities.push(entity);
    }
    let instances = Instances::of(level, held.spawned);
    let anew = crate::install::anew(
        level,
        &instances,
        &renumber.added,
        held.next_id,
        held.entities,
    );
    report.added = anew.iter().map(|entity| entity.id).collect();
    held.entities.extend(anew);
    report
}

/// `held`'s objects whose number names no object of their kind in `level`
/// dropped; the rest trusted, as a save from before rosters were kept
/// gives nothing to match them by.
fn trust(level: &Level, held: &mut Held) -> Report {
    let instances = Instances::of(level, held.spawned);
    let mut report = Report {
        trusted: true,
        ..Report::default()
    };
    held.entities.retain(|entity| match entity.kind {
        Kind::Object(kind, number) if instances.kind(number) != Some(kind) => {
            report.dropped.push(format!("{} {number}", kind.name()));
            false
        }
        _ => true,
    });
    report
}

/// A placement as the log names it: its kind, template and tag.
fn describe(origin: &Origin) -> String {
    let names: Vec<&str> = [origin.template.as_str(), origin.tag.as_str()]
        .into_iter()
        .filter(|name| !name.is_empty())
        .collect();
    format!("{} {}", origin.kind.name(), names.join(" "))
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
