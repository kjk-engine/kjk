//! A head that is a model of its own, hung on its body at `headhook`
//! (KOTOR's humans), plays its body's clips: the clip's tracks pose the
//! head's nodes at and under the head model's animation root (its neck,
//! face, jaw, brows and eyelids; the copy of the spine above stays at rest),
//! as reone plays a creature's clip on its attached head. So the head
//! sleeps, dies and wakes with its eyes as the clip keys its lids (`sleep`,
//! `dead`, a cutscene's stunt clips) and blinks where it does (`pause1`,
//! `listen`, `tlknorm`).

use std::sync::Arc;

use asset_odyssey::anim::AnimationIr;
use bevy::prelude::*;

use crate::scene::Animator;
use crate::viewer::Placed;

/// The model hung on body `body` (scene index) with a place of its own:
/// its head (a weapon hangs by a hand bolt instead).
pub(crate) fn hung_head(models: &[Placed], body: usize) -> Option<usize> {
    models
        .iter()
        .position(|placed| placed.attach.is_some() && placed.hook.map(|(on, _)| on) == Some(body))
}

/// A body's hung head: its animated nodes at rest, its body's clips bound
/// on them, and the clips it plays where the body plays none of KOTOR's (a
/// JA pose or a ragdoll has no face): standing, and dead (the lids shut).
pub(crate) struct Face {
    rest: Vec<(Entity, Vec3, Quat)>,
    clips: Vec<Animator>,
    idle: [Option<Animator>; 2],
    /// Where in its idle loop it is (0 to 1), so heads do not blink as one.
    phase: f32,
}

impl Face {
    /// The face of `head` with its body's `clips` and its standing and dead
    /// clips (`idle`) bound; none when the head has no animated node.
    pub(crate) fn new<'a>(
        (head, nodes): (&Placed, &[Entity]),
        clips: impl IntoIterator<Item = &'a Arc<AnimationIr>>,
        idle: [Option<&Arc<AnimationIr>>; 2],
        phase: f32,
    ) -> Option<Face> {
        let rest: Vec<(Entity, Vec3, Quat)> = (head.ir.nodes.iter().zip(nodes))
            .filter(|(node, _)| node.animated)
            .map(|(node, &entity)| {
                let [x, y, z, w] = node.rotation;
                (
                    entity,
                    Vec3::from_array(node.translation),
                    Quat::from_xyzw(x, y, z, w),
                )
            })
            .collect();
        if rest.is_empty() {
            return None;
        }
        let mut bound: Vec<Animator> = Vec::new();
        for clip in clips {
            if !bound.iter().any(|b| Arc::ptr_eq(b.clip(), clip))
                && let Some(animator) = crate::scene::animator(head, clip, nodes)
            {
                bound.push(animator);
            }
        }
        Some(Face {
            rest,
            clips: bound,
            idle: idle.map(|clip| clip.and_then(|clip| crate::scene::animator(head, clip, nodes))),
            phase,
        })
    }

    /// The face as it is under no clip of the body's: its standing clip
    /// (`alive`) or its dead one looped on the clock (`ms`), else its rest.
    pub(crate) fn stand(&self, (ms, alive): (f64, bool), transforms: &mut Query<&mut Transform>) {
        for &(entity, translation, rotation) in &self.rest {
            if let Ok(mut t) = transforms.get_mut(entity) {
                t.translation = translation;
                t.rotation = rotation;
            }
        }
        if let Some(idle) = &self.idle[usize::from(!alive)] {
            let length = idle.length();
            let at = (ms as f32 / 1000.0 + self.phase * length).rem_euclid(length);
            idle.pose(at, transforms);
        }
    }

    /// The body's clip `shown` laid over the face `weight` of the way,
    /// `seconds` in, as it is over the body.
    pub(crate) fn follow(
        &self,
        shown: &Animator,
        (seconds, weight): (f32, f32),
        transforms: &mut Query<&mut Transform>,
    ) {
        if let Some(clip) = self
            .clips
            .iter()
            .find(|b| Arc::ptr_eq(b.clip(), shown.clip()))
        {
            clip.pose_over(seconds, weight, transforms);
        }
    }
}
