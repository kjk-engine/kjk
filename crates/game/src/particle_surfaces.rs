//! What a bouncing particle meets (`model_fx::Surfaces`), as KOTOR's
//! emitter step traces a particle's way through the scene: the rooms
//! first, then every model's meshes but the emitter's own; the nearest
//! hit wins.
//!
//! Here the level is the sim's collision (rooms, doors, placeables, as a
//! view traces it with the doors' states), and each character's body
//! stands for its meshes as capsules between the nodes its hit locations
//! are drawn at (`cast::Hitboxes`): the trunk from the head to the waist,
//! each leg from the waist to the foot, each forearm from the arm to the
//! hand, `CAPSULES` thick [verify: the executable traces the meshes'
//! triangles; the views skin on the GPU, so none are posed here].

use bevy::prelude::*;
use collision::{Contents, Extents};
use sim::combat::Location;

use crate::cast::Hitboxes;
#[cfg(feature = "probes")]
use crate::probe_kit::{Checks, check};

/// A body's capsules: between which hit locations' nodes, how thick
/// (radius, metres) [verify].
const CAPSULES: [(Location, Location, f32); 5] = [
    (Location::Head, Location::Waist, 0.16),
    (Location::Waist, Location::FootLeft, 0.08),
    (Location::Waist, Location::FootRight, 0.08),
    (Location::ArmLeft, Location::HandLeft, 0.05),
    (Location::ArmRight, Location::HandRight, 0.05),
];
/// How many halvings find where a step entered a capsule.
const HALVINGS: usize = 8;

/// A capsule (game space, metres).
#[derive(Debug, Clone, Copy)]
pub struct Capsule {
    a: Vec3,
    b: Vec3,
    radius: f32,
}

impl Capsule {
    /// The point of its axis nearest `p`.
    fn axis_point(&self, p: Vec3) -> Vec3 {
        let ab = self.b - self.a;
        let t = (p - self.a).dot(ab) / ab.length_squared().max(f32::EPSILON);
        self.a + ab * t.clamp(0.0, 1.0)
    }

    fn holds(&self, p: Vec3) -> bool {
        p.distance_squared(self.axis_point(p)) <= self.radius * self.radius
    }

    /// Where the way from `from` to `to` enters it, and its normal there;
    /// none for a way that starts inside or never enters.
    fn entered(&self, from: Vec3, to: Vec3) -> Option<(f32, Vec3, Vec3)> {
        if self.holds(from) || !self.holds(to) {
            return None;
        }
        let (mut out, mut inside) = (0.0_f32, 1.0_f32);
        for _ in 0..HALVINGS {
            let mid = (out + inside) / 2.0;
            if self.holds(from.lerp(to, mid)) {
                inside = mid;
            } else {
                out = mid;
            }
        }
        let met = from.lerp(to, out);
        let normal = (met - self.axis_point(met)).normalize_or(-(to - from).normalize_or(Vec3::Z));
        Some((out, met, normal))
    }
}

/// The characters' bodies as drawn last (game space): their capsules.
pub fn bodies(hitboxes: &Hitboxes, globals: &Query<&GlobalTransform>) -> Vec<Capsule> {
    let to_game = crate::scene::game_to_bevy().inverse();
    let node = |spots: &[Option<Entity>], at: Location| {
        let entity = spots.get(at as usize).copied().flatten()?;
        Some(to_game * globals.get(entity).ok()?.translation())
    };
    hitboxes
        .0
        .values()
        .flat_map(|spots| {
            CAPSULES.iter().filter_map(move |&(from, to, radius)| {
                Some(Capsule {
                    a: node(spots, from)?,
                    b: node(spots, to)?,
                    radius,
                })
            })
        })
        .collect()
}

/// What the way from `from` to `to` (game space, metres) meets first: the
/// level as the sim traces it with `states` (`sim::Level::with`), or a
/// body's capsule.
pub fn meets(
    level: &sim::Level,
    states: Option<&[u8]>,
    bodies: &[Capsule],
    (from, to): (Vec3, Vec3),
) -> Option<(Vec3, Vec3)> {
    let trace = level.with(states, |world| {
        world.trace(
            from.to_array(),
            to.to_array(),
            &Extents::POINT,
            Contents::SOLID,
        )
    });
    let mut nearest = (trace.fraction < 1.0 && !trace.start_solid).then(|| {
        let normal = Vec3::from_array(trace.plane.normal);
        (trace.fraction, Vec3::from_array(trace.end), normal)
    });
    for body in bodies {
        if let Some(hit) = body.entered(from, to)
            && nearest.is_none_or(|(fraction, ..)| hit.0 < fraction)
        {
            nearest = Some(hit);
        }
    }
    nearest.map(|(_, met, normal)| (met, normal))
}

/// A way into a capsule meets it at its skin, the normal out from its
/// axis; one starting inside or passing beside it meets nothing.
#[cfg(feature = "probes")]
pub fn probes() -> Checks {
    let trunk = Capsule {
        a: Vec3::ZERO,
        b: Vec3::new(0.0, 0.0, 1.0),
        radius: 0.2,
    };
    let into = trunk.entered(Vec3::new(1.0, 0.0, 0.5), Vec3::new(0.0, 0.0, 0.5));
    let skin = into.is_some_and(|(_, met, normal)| {
        (met.x - 0.2).abs() < 0.01 && (normal - Vec3::X).length() < 1e-3
    });
    let inside = trunk.entered(Vec3::new(0.1, 0.0, 0.5), Vec3::new(0.5, 0.0, 0.5));
    let beside = trunk.entered(Vec3::new(1.0, 1.0, 0.5), Vec3::new(1.0, -1.0, 0.5));
    vec![
        ("capsule_met_at_its_skin", check(skin, format!("{into:?}"))),
        (
            "capsule_left_from_inside",
            check(inside.is_none(), format!("{inside:?}")),
        ),
        (
            "capsule_passed_beside",
            check(beside.is_none(), format!("{beside:?}")),
        ),
    ]
}
