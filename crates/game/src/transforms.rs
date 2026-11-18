//! The transforms a character's posing reads and writes: the world's own,
//! or a copy of the ones it writes, kept apart while several characters are
//! posed at once on the compute threads (`Cast::draw`).
//!
//! A character's posing touches only its own body's and hung head's nodes,
//! so characters posed side by side never read what another writes. Each
//! one writes into its own `Staged` copy, read back by its later steps
//! (a clip laid over the pose, the head held level over it), and the copies
//! are written into the world once every character is posed.

use bevy::ecs::entity::EntityHashMap;
use bevy::prelude::*;

/// What posing reads and writes transforms through.
pub(crate) trait Transforms {
    /// The entity's transform, as written so far.
    fn read(&self, entity: Entity) -> Option<&Transform>;
    /// The entity's transform to change, marked changed.
    fn write(&mut self, entity: Entity) -> Option<&mut Transform>;
}

impl Transforms for Query<'_, '_, &mut Transform> {
    fn read(&self, entity: Entity) -> Option<&Transform> {
        self.get(entity).ok()
    }

    fn write(&mut self, entity: Entity) -> Option<&mut Transform> {
        self.get_mut(entity).ok().map(Mut::into_inner)
    }
}

/// One character's writes, over the world's transforms as they stood
/// before any character was posed this frame.
pub(crate) struct Staged<'q, W> {
    world: &'q W,
    written: EntityHashMap<Transform>,
}

impl<'q, W: Transforms> Staged<'q, W> {
    pub(crate) fn new(world: &'q W) -> Self {
        Staged {
            world,
            written: EntityHashMap::default(),
        }
    }

    /// The transforms written, to apply to the world.
    pub(crate) fn written(self) -> Written {
        Written(self.written)
    }
}

impl<W: Transforms> Transforms for Staged<'_, W> {
    fn read(&self, entity: Entity) -> Option<&Transform> {
        self.written
            .get(&entity)
            .or_else(|| self.world.read(entity))
    }

    fn write(&mut self, entity: Entity) -> Option<&mut Transform> {
        use bevy::platform::collections::hash_map::Entry;
        match self.written.entry(entity) {
            Entry::Occupied(written) => Some(written.into_mut()),
            Entry::Vacant(vacant) => {
                let now = *self.world.read(entity)?;
                Some(vacant.insert(now))
            }
        }
    }
}

/// The transforms a character's posing wrote (`Staged`).
pub(crate) struct Written(EntityHashMap<Transform>);

impl Written {
    /// Writes them into the world.
    pub(crate) fn apply(self, world: &mut Query<&mut Transform>) {
        for (entity, transform) in self.0 {
            if let Ok(mut now) = world.get_mut(entity) {
                *now = transform;
            }
        }
    }
}
