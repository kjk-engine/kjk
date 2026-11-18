//! The transforms that posing a character reads and writes.
//!
//! A character is posed either on the world's transforms or on a copy of
//! its own. The copy is used when several characters are posed at once on
//! the compute threads (`Cast::draw`).
//!
//! Posing a character touches only the nodes of its own body and of the
//! head hung on it. So a character posed beside others never reads what
//! another one writes. Each character writes into its own copy (`Staged`).
//! Its later steps read their own writes back from that copy, such as a
//! clip laid over the pose or the head held level over it. The copies are
//! written into the world once every character is posed.

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

    /// Gives back the transforms this character wrote.
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
    /// Writes the character's transforms into the world. A transform that
    /// ends bit for bit as it stood is left alone. It is then not marked
    /// changed, and Bevy does not propagate its subtree again.
    pub(crate) fn apply(self, world: &mut Query<&mut Transform>) {
        for (entity, transform) in self.0 {
            if let Ok(mut now) = world.get_mut(entity)
                && !same(&now, &transform)
            {
                *now = transform;
            }
        }
    }
}

/// Whether two transforms are the same bit for bit. So 0.0 and -0.0
/// differ, and a NaN equals itself.
pub(crate) fn same(a: &Transform, b: &Transform) -> bool {
    bits(a) == bits(b)
}

fn bits(transform: &Transform) -> [u32; 10] {
    let [a, b, c] = transform.translation.to_array();
    let [d, e, f, g] = transform.rotation.to_array();
    let [h, i, j] = transform.scale.to_array();
    [a, b, c, d, e, f, g, h, i, j].map(f32::to_bits)
}
