//! Checks of what the capsule shadows send the shader, on capsules and
//! lights made here (no install). They cover the capsule a droid's box
//! stands for, the sphere that holds a character's capsules, and which
//! lights cast a character's shadow.

use bevy::prelude::*;

use super::{Seen, bounds, capsule_of, casting_lights};
use crate::material::{GpuLight, LIGHT_COMBAT};
use crate::probe_kit::{Checks, check};

/// The checks, for `capsule_shadows` in the approved tests.
pub fn probes() -> Checks {
    vec![
        ("box_as_capsule", box_as_capsule()),
        ("sphere_holds_capsules", sphere_holds_capsules()),
        ("lights_that_cast", lights_that_cast()),
    ]
}

/// A box 0.2 m by 0.8 m by 0.4 m becomes a capsule along its 0.8 m side.
/// The capsule is 0.15 m wide (the mean of the other two half sides), and
/// its ends are drawn in by that much, so it is 0.8 m long in all.
fn box_as_capsule() -> Result<String, String> {
    let (a, b, radius) = capsule_of(&ragdoll::Solid::Cuboid {
        centre: Vec3::new(1.0, 2.0, 3.0),
        turn: Quat::IDENTITY,
        half: Vec3::new(0.1, 0.4, 0.2),
    });
    let want = (Vec3::new(1.0, 1.75, 3.0), Vec3::new(1.0, 2.25, 3.0), 0.15);
    let near = |x: Vec3, y: Vec3| x.distance(y) < 1e-4;
    check(
        near(a, want.0) && near(b, want.1) && (radius - want.2).abs() < 1e-4,
        format!("capsule {a} to {b}, {radius} wide, wanted {want:?}"),
    )
}

/// The sphere `bounds` gives holds both ends of every capsule, with its
/// radius about them.
fn sphere_holds_capsules() -> Result<String, String> {
    let capsules = [
        (Vec3::new(0.0, 0.1, 0.0), Vec3::new(0.0, 0.9, 0.0), 0.1),
        (Vec3::new(0.2, 1.0, 0.0), Vec3::new(0.2, 1.6, 0.0), 0.15),
        (Vec3::new(-0.6, 1.4, 0.3), Vec3::new(-0.2, 1.4, 0.3), 0.05),
    ];
    let (centre, radius) = bounds(&capsules);
    let outside: Vec<_> = capsules
        .iter()
        .filter(|&&(a, b, r)| {
            centre.distance(a) + r > radius + 1e-4 || centre.distance(b) + r > radius + 1e-4
        })
        .collect();
    check(
        outside.is_empty(),
        format!("sphere at {centre}, {radius} wide, leaves out {outside:?}"),
    )
}

/// A light at `at` lighting out to `radius`.
fn light(at: Vec3, radius: f32) -> GpuLight {
    GpuLight {
        position_radius: at.extend(radius),
        color_multiplier: Vec4::ONE,
        ..default()
    }
}

/// A character at the origin, 1 m wide. Light 0 lights only the ambient.
/// Light 1 is a lamp in reach. Light 2 is a lamp out of reach. Light 3 is a
/// fight's light in reach. Light 4 is a bolt whose line reaches the
/// character, though its middle is out of reach. Without fight lights, the
/// lamp in reach alone casts. With them, the fight's light and the bolt
/// cast too.
fn lights_that_cast() -> Result<String, String> {
    let mut ambient = light(Vec3::new(0.0, 2.0, 0.0), 5.0);
    ambient.kind.x = 1;
    let mut fight = light(Vec3::new(2.0, 1.0, 0.0), 3.0);
    fight.kind.z = LIGHT_COMBAT;
    let mut bolt = light(Vec3::new(4.5, 1.0, 0.0), 3.0);
    bolt.kind.z = LIGHT_COMBAT;
    bolt.way = Vec4::new(1.0, 0.0, 0.0, 0.75);
    let lights = [
        ambient,
        light(Vec3::new(0.0, 3.0, 0.0), 5.0),
        light(Vec3::new(9.0, 3.0, 0.0), 5.0),
        fight,
        bolt,
    ];
    let mut seen = Seen::default();
    let mut counts = (0, 0);
    let mut cast = |fights: bool| {
        casting_lights(
            &lights,
            (Entity::PLACEHOLDER, Vec3::new(0.0, 1.0, 0.0), 1.0),
            fights,
            (None, &mut seen, 0),
            (&mut counts.0, &mut counts.1),
        )
    };
    let (without, with) = (cast(false), cast(true));
    check(
        without == 0b10 && with == 0b11010,
        format!("without fight lights {without:#b}, with them {with:#b}; wanted 0b10 and 0b11010"),
    )
}
