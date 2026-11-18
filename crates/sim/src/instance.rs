//! Templates and instances (docs/IMPORT.md, part 2, slice 5). Every object
//! in an area is an instance: a template's facts and what is its own (where
//! it stands, its tag, what its placement changes). Three sources make
//! them, into one table per area:
//! - the module's placements, when the module is first installed, in the
//!   GIT's order (an encounter's additions after), so a placement's number
//!   is also its entity in the level's collision;
//! - scripts, as they run (`CreateObject`, `SpawnAvailableNPC`), from the
//!   level's templates;
//! - the party: its companions, carried into the next module.
//!
//! An object's number in the table is what `Kind::Object` names, and
//! numbers are never reused while the area lasts (its stay in the stash
//! included). The table is not state of its own: the placements are the
//! level's, what play made is the snapshot's (`Snapshot::spawned`, by
//! template), and the world, a client, a save and a view all build the
//! same table from the two (`Instances::of`), at the cost of looking up
//! what play made.

use std::sync::Arc;

use crate::module::{ObjectData, ObjectKind};
use crate::snapshot::{EntityState, Kind, Snapshot, Spawned};
use crate::world::{Level, TemplateSources};

/// Every object of an area, by number (see the module documentation).
#[derive(Debug, Clone, Default)]
pub struct Instances {
    /// The module's placements' facts (the level's): the table's first.
    placed: Arc<[Arc<ObjectData>]>,
    /// What play made after them, in the order made, as a snapshot carries
    /// it, and each one's facts (none for one made from a template the
    /// level lacks: it stands and fights as a commoner, with no hooks, tag
    /// or name).
    /// Both lists are shared, so a copy of a table costs nothing until
    /// one of them makes an instance.
    made: Arc<Vec<Spawned>>,
    made_facts: Arc<Vec<Option<Arc<ObjectData>>>>,
}

/// How many tables a thread keeps (`Instances::of`). Two cover the
/// snapshots of one frame on either side of an object being made.
const KEPT_TABLES: usize = 2;

thread_local! {
    /// The tables this thread made last, each with the template sources it
    /// was made from (`Level::template_sources`).
    static TABLES: std::cell::RefCell<Vec<(TemplateSources, Instances)>> =
        const { std::cell::RefCell::new(Vec::new()) };
}

#[cfg(feature = "probes")]
thread_local! {
    /// How many tables `Instances::of` made on this thread rather than
    /// took from the kept ones, for the probes.
    pub(crate) static MADE_TABLES: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

impl Instances {
    /// `level`'s placements, then `made` (the snapshot's list) from the
    /// level's templates.
    ///
    /// The list grows by every object play makes in the area and never
    /// shrinks, so a visited area's table can hold hundreds (the Ebon
    /// Hawk's scripts make the party and its gizka again on each visit).
    /// The views ask for the table many times a frame. So the thread keeps
    /// the last tables it made, and a table asked for again from the same
    /// level's templates and an equal list is that one, shared.
    pub fn of(level: &Level, made: &[Spawned]) -> Instances {
        let sources = level.template_sources();
        let kept = TABLES.with_borrow(|tables| {
            (tables.iter())
                .find(|(from, table)| *from == sources && table.made.as_slice() == made)
                .map(|(_, table)| table.clone())
        });
        if let Some(table) = kept {
            return table;
        }
        #[cfg(feature = "probes")]
        MADE_TABLES.with(|count| count.set(count.get() + 1));
        let mut instances = Instances {
            placed: level.placed().clone(),
            made: Arc::new(Vec::with_capacity(made.len())),
            made_facts: Arc::new(Vec::with_capacity(made.len())),
        };
        for spawned in made {
            instances.make(level, spawned.clone());
        }
        TABLES.with_borrow_mut(|tables| {
            // A table of another level is let go at once.
            tables.retain(|(from, _)| *from == sources);
            if tables.len() >= KEPT_TABLES {
                tables.remove(0);
            }
            tables.push((sources, instances.clone()));
        });
        instances
    }

    /// The table of the area `snapshot` is in, made in `level`.
    ///
    /// A kept table that holds the snapshot's own list is the table
    /// wanted. The list cannot have changed while both hold it, because a
    /// shared list is copied before anything is added to it. Otherwise the
    /// table is found as `of` finds it. That table then holds the
    /// snapshot's list, so the next snapshot with the same list finds it
    /// at once.
    pub fn in_snapshot(level: &Level, snapshot: &Snapshot) -> Instances {
        let list = &snapshot.spawned;
        let kept = TABLES.with_borrow(|tables| {
            (tables.iter())
                .find(|(from, table)| Arc::ptr_eq(&table.made, list) && from.are_of(level))
                .map(|(_, table)| table.clone())
        });
        if let Some(table) = kept {
            return table;
        }
        let mut table = Instances::of(level, list);
        let made = std::mem::replace(&mut table.made, list.clone());
        TABLES.with_borrow_mut(|tables| {
            if let Some((_, kept)) =
                (tables.iter_mut()).find(|(_, kept)| Arc::ptr_eq(&kept.made, &made))
            {
                kept.made = list.clone();
            }
        });
        table
    }

    /// An instance `made` in play, from `level`'s templates: its number. A
    /// placeable's facts face its way (a creature's turns with its body)
    /// and hold what it was made holding, where that is its own.
    pub fn make(&mut self, level: &Level, made: Spawned) -> u32 {
        let number = (self.placed.len() + self.made.len()) as u32;
        let facts = level
            .template(made.kind, &made.template)
            .map(|facts| match made.kind {
                ObjectKind::Creature => facts,
                _ => Arc::new(ObjectData {
                    yaw: made.yaw,
                    inventory: if made.holds.is_empty() {
                        facts.inventory.clone()
                    } else {
                        crate::world::remains::held(&made.holds)
                    },
                    ..(*facts).clone()
                }),
            });
        Arc::make_mut(&mut self.made).push(made);
        Arc::make_mut(&mut self.made_facts).push(facts);
        number
    }

    /// Instance `number`: its kind and facts.
    fn entry(&self, number: u32) -> Option<(ObjectKind, Option<&Arc<ObjectData>>)> {
        match self.placed.get(number as usize) {
            Some(facts) => Some((facts.kind, Some(facts))),
            None => {
                let at = self.made_at(number)?;
                Some((self.made[at].kind, self.made_facts[at].as_ref()))
            }
        }
    }

    /// Where instance `number` is in `made`, if play made it.
    fn made_at(&self, number: u32) -> Option<usize> {
        let at = (number as usize).checked_sub(self.placed.len())?;
        (at < self.made.len()).then_some(at)
    }

    /// What instance `number` was made from, if play made it: a view loads
    /// these as they come, where the module's own came with its scene.
    pub fn made_from(&self, number: u32) -> Option<&Spawned> {
        Some(&self.made[self.made_at(number)?])
    }

    /// Instance `number`'s facts.
    pub fn get(&self, number: u32) -> Option<&ObjectData> {
        self.facts(number).map(Arc::as_ref)
    }

    /// Instance `number`'s facts, shared (an entity keeps them at hand).
    pub fn facts(&self, number: u32) -> Option<&Arc<ObjectData>> {
        self.entry(number)?.1
    }

    /// Instance `number`'s tag: its facts', else, for one play made from a
    /// template this level lacks (a companion followed in from another
    /// module), the template it was made from, which names it as its tag
    /// does.
    pub fn tag(&self, number: u32) -> Option<&str> {
        match self.get(number) {
            Some(facts) => Some(facts.tag.as_str()),
            None => self.made_from(number).map(|made| made.template.as_str()),
        }
    }

    /// What instance `number` is, if the table has it.
    pub fn kind(&self, number: u32) -> Option<ObjectKind> {
        Some(self.entry(number)?.0)
    }

    /// The facts of the object `entity` is, if it is one.
    pub fn of_entity(&self, entity: &EntityState) -> Option<&ObjectData> {
        match entity.kind {
            Kind::Object(_, number) => self.get(number),
            _ => None,
        }
    }

    /// Every instance with facts, by number.
    pub fn iter(&self) -> impl Iterator<Item = (u32, &ObjectData)> {
        let made = self.made_facts.iter().map(Option::as_ref);
        self.placed
            .iter()
            .map(Some)
            .chain(made)
            .enumerate()
            .filter_map(|(number, facts)| Some((number as u32, facts?.as_ref())))
    }

    /// The module's placements, by number: what comes alive on the module's
    /// first visit (its items, its creatures' spawn scripts).
    pub fn placements(&self) -> impl Iterator<Item = (u32, &ObjectData)> {
        (0..).zip(self.placed.iter().map(Arc::as_ref))
    }

    /// What play made, in the order made (`Snapshot::spawned`).
    pub fn made(&self) -> &[Spawned] {
        &self.made
    }

    /// The same list as `made`, shared rather than copied. A snapshot
    /// carries this list as it is (`Snapshot::spawned`).
    pub fn made_shared(&self) -> &Arc<Vec<Spawned>> {
        &self.made
    }
}
