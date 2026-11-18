//! Posing a character touches only its own body's and head's nodes, so a
//! character posed beside others never reads what another one writes.

use bevy::ecs::entity::EntityHashMap;
use bevy::platform::collections::hash_map::Entry;
use bevy::prelude::*;

pub(crate) trait Transforms {
    fn read(&self, entity: Entity) -> Option<&Transform>;
    fn write(&mut self, entity: Entity) -> Option<&mut Transform>;
    /// A transform left bit for bit as it was is not written, so it is not
    /// marked changed.
    fn set(&mut self, entity: Entity, to: impl FnOnce(&Transform) -> Transform);
}

impl Transforms for Query<'_, '_, &mut Transform> {
    fn read(&self, entity: Entity) -> Option<&Transform> {
        self.get(entity).ok()
    }

    fn write(&mut self, entity: Entity) -> Option<&mut Transform> {
        self.get_mut(entity).ok().map(Mut::into_inner)
    }

    fn set(&mut self, entity: Entity, to: impl FnOnce(&Transform) -> Transform) {
        if let Ok(mut now) = self.get_mut(entity) {
            let new = to(&now);
            if !same(&now, &new) {
                *now = new;
            }
        }
    }
}

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

    pub(crate) fn written(self) -> Written {
        let world = self.world;
        let changed = self.written.into_iter().filter(|(entity, transform)| {
            world.read(*entity).is_none_or(|now| !same(now, transform))
        });
        Written(changed.collect())
    }
}

impl<W: Transforms> Transforms for Staged<'_, W> {
    fn read(&self, entity: Entity) -> Option<&Transform> {
        self.written
            .get(&entity)
            .or_else(|| self.world.read(entity))
    }

    fn write(&mut self, entity: Entity) -> Option<&mut Transform> {
        match self.written.entry(entity) {
            Entry::Occupied(written) => Some(written.into_mut()),
            Entry::Vacant(vacant) => {
                let now = *self.world.read(entity)?;
                Some(vacant.insert(now))
            }
        }
    }

    fn set(&mut self, entity: Entity, to: impl FnOnce(&Transform) -> Transform) {
        match self.written.entry(entity) {
            Entry::Occupied(mut written) => {
                let new = to(written.get());
                written.insert(new);
            }
            Entry::Vacant(vacant) => {
                if let Some(now) = self.world.read(entity) {
                    let new = to(now);
                    if !same(now, &new) {
                        vacant.insert(new);
                    }
                }
            }
        }
    }
}

pub(crate) struct Written(Vec<(Entity, Transform)>);

impl Written {
    pub(crate) fn apply(self, world: &mut impl Transforms) {
        for (entity, transform) in self.0 {
            if let Some(now) = world.write(entity) {
                *now = transform;
            }
        }
    }
}

/// So 0.0 and -0.0 differ, and a NaN equals itself.
fn same(a: &Transform, b: &Transform) -> bool {
    bits(a) == bits(b)
}

fn bits(transform: &Transform) -> [u32; 10] {
    let [a, b, c] = transform.translation.to_array();
    let [d, e, f, g] = transform.rotation.to_array();
    let [h, i, j] = transform.scale.to_array();
    [a, b, c, d, e, f, g, h, i, j].map(f32::to_bits)
}
