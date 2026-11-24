//! Checks of what the capsule shadows send the shader, on capsules, boxes
//! and lights made here (no install). They cover the capsule a droid's box
//! stands for, the sphere that holds a character's capsules, which lights
//! cast a character's shadow, which placeables get a box, where a
//! placeable's box stands, and the sphere a point must lie in to trace any
//! character.

use std::f32::consts::FRAC_PI_2;

use bevy::prelude::*;

use super::{
    BoxTally, Line, Seen, ShadowBox, Traced, bounds, capsule_of, casting_lights, fill,
    outside_capsules, reach_of, sight_of, world_box,
};
use crate::material::{GpuLight, LIGHT_COMBAT};
use crate::probe_kit::{Checks, check};

/// The checks, for `capsule_shadows` in the approved tests.
pub fn probes() -> Checks {
    vec![
        ("box_as_capsule", box_as_capsule()),
        ("sphere_holds_capsules", sphere_holds_capsules()),
        ("lights_that_cast", lights_that_cast()),
        ("lamp_seen_either_way", lamp_seen_either_way()),
        ("placeables_boxed", placeables_boxed()),
        ("box_filled", box_filled()),
        ("box_placed", box_placed()),
        ("reach_holds_casters", reach_holds_casters()),
        ("held_light_casts_none", held_light_casts_none()),
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
    let mut traced = Traced::default();
    let mut cast = |fights: usize| {
        casting_lights(
            &lights,
            (Entity::PLACEHOLDER, Vec3::new(0.0, 1.0, 0.0), 1.0),
            (fights, &|_| 10.0),
            (None, &mut seen, 0),
            &mut traced,
        )
    };
    let (without, with) = (cast(0), cast(usize::MAX));
    check(
        without == 0b10 && with == 0b11010,
        format!("without fight lights {without:#b}, with them {with:#b}; wanted 0b10 and 0b11010"),
    )
}

/// A chest at the origin. A lamp sees it when the line from the lamp is
/// clear, or when the lamp hangs more than 2 m over the chest and the line
/// from 2 m over the chest under the lamp is clear. A high lamp behind a
/// strut that hides the point under it still sees the chest (the Star
/// Forge's deck). A lamp over a ceiling sees it from under the ceiling
/// (Leviathan's halls). A lamp behind a wall both ways does not, and
/// neither does a low lamp whose own line is blocked.
fn lamp_seen_either_way() -> Result<String, String> {
    let high = Vec3::new(4.0, 5.0, 0.0);
    let low = Vec3::new(4.0, 1.0, 0.0);
    // Each case: the lamp, whether the line from the lamp is blocked,
    // whether the line from under it is, and what is wanted.
    let cases = [
        (
            "high lamp, point under it hidden",
            high,
            false,
            true,
            Line::Clear,
        ),
        ("high lamp over a ceiling", high, true, false, Line::Clear),
        ("high lamp behind a wall", high, true, true, Line::Blocked),
        ("low lamp behind a wall", low, true, false, Line::Blocked),
    ];
    let wrong: Vec<String> = cases
        .iter()
        .filter_map(|&(name, at, lamp_blocked, under_blocked, want)| {
            let got = sight_of(&light(at, 10.0), Vec3::ZERO, |from| {
                let blocked = if from == at {
                    lamp_blocked
                } else {
                    under_blocked
                };
                if blocked { Line::Blocked } else { Line::Clear }
            });
            (got != want).then(|| format!("{name}: {got:?}, wanted {want:?}"))
        })
        .collect();
    check(
        wrong.is_empty(),
        format!("{} cases; wrong: {wrong:?}", cases.len()),
    )
}

/// The 12 triangles of a block from `low` to `high`.
fn block(low: Vec3, high: Vec3) -> Vec<[Vec3; 3]> {
    let corner = |i: usize| Vec3::select(BVec3::new(i & 1 != 0, i & 2 != 0, i & 4 != 0), high, low);
    // Each side as four corners in turn: x low and high, y, z.
    let sides = [
        [0, 2, 6, 4],
        [1, 3, 7, 5],
        [0, 1, 5, 4],
        [2, 3, 7, 6],
        [0, 1, 3, 2],
        [4, 5, 7, 6],
    ];
    sides
        .iter()
        .flat_map(|&[a, b, c, d]| {
            [
                [corner(a), corner(b), corner(c)],
                [corner(a), corner(c), corner(d)],
            ]
        })
        .collect()
}

/// A junk pile: a board 2 m square and 5 cm thick on the floor, with a
/// pole 5 cm thick standing 1.2 m tall in its middle.
fn pile() -> Vec<[Vec3; 3]> {
    let mut pile = block(Vec3::ZERO, Vec3::new(2.0, 2.0, 0.05));
    let pole = (Vec3::new(0.975, 0.975, 0.0), Vec3::new(1.025, 1.025, 1.2));
    pile.extend(block(pole.0, pole.1));
    pile
}

/// A crate 1 m on each side gets a box. A canister 0.25 m across is too
/// small. A hull 12 m long is too large. A body lying down, 1.8 m long and
/// 0.3 m high, is too flat. The junk pile (`pile`) is too sparse: from the
/// side, it fills little of its box.
fn placeables_boxed() -> Result<String, String> {
    let mut tally = BoxTally::default();
    let span = |x: f32, y: f32, z: f32| block(Vec3::ZERO, Vec3::new(x, y, z));
    let kept = [
        tally.of(&span(1.0, 1.0, 1.0)).is_some(),
        tally.of(&span(0.25, 0.25, 0.25)).is_some(),
        tally.of(&span(12.0, 3.0, 3.0)).is_some(),
        tally.of(&span(1.8, 0.6, 0.3)).is_some(),
        tally.of(&pile()).is_some(),
    ];
    let counts = (
        tally.boxed,
        tally.small,
        tally.large,
        tally.flat,
        tally.sparse,
    );
    check(
        kept == [true, false, false, false, false] && counts == (1, 1, 1, 1, 1),
        format!("kept {kept:?}, counts {counts:?}"),
    )
}

/// A block fills all of its box. A cylinder standing up fills about 78% of
/// its box from above (a circle in a square) and all of it from the side.
/// The junk pile (`pile`) fills its box from above, but from the side only
/// its board's bottom row of cells.
fn box_filled() -> Result<String, String> {
    let solid = fill(&block(Vec3::ZERO, Vec3::ONE), Vec3::ZERO, Vec3::ONE).min_element();
    let sides = 32;
    let rim = |k: usize| {
        let turn = std::f32::consts::TAU * k as f32 / sides as f32;
        Vec3::new(0.5 + 0.5 * turn.cos(), 0.5 + 0.5 * turn.sin(), 0.0)
    };
    let cylinder: Vec<[Vec3; 3]> = (0..sides)
        .flat_map(|k| {
            let (a, b) = (rim(k), rim(k + 1));
            let up = Vec3::Z * 2.0;
            [
                [Vec3::new(0.5, 0.5, 0.0), a, b],
                [Vec3::new(0.5, 0.5, 2.0), a + up, b + up],
                [a, b, b + up],
                [a, b + up, a + up],
            ]
        })
        .collect();
    let round = fill(&cylinder, Vec3::ZERO, Vec3::new(1.0, 1.0, 2.0)).min_element();
    let sparse = fill(&pile(), Vec3::ZERO, Vec3::new(2.0, 2.0, 1.2)).min_element();
    check(
        solid == 1.0 && (0.74..0.86).contains(&round) && sparse < 0.15,
        format!("block {solid}, cylinder {round}, pile {sparse}"),
    )
}

/// A box from (0, 0, 0) to (2, 1, 1) in its model's frame, under a root
/// at (10, 0, 0) turned a quarter round y. Its centre lands at
/// (10.5, 0.5, -1), its x axis along -z, and its half sizes stay 1, 0.5
/// and 0.5.
fn box_placed() -> Result<String, String> {
    let root = GlobalTransform::from(
        Transform::from_xyz(10.0, 0.0, 0.0).with_rotation(Quat::from_rotation_y(FRAC_PI_2)),
    );
    let placed = world_box(
        &ShadowBox {
            low: Vec3::ZERO,
            high: Vec3::new(2.0, 1.0, 1.0),
        },
        &root,
    );
    let near = |a: Vec3, b: Vec3| a.distance(b) < 1e-4;
    let halves = Vec3::new(placed.x.w, placed.y.w, placed.z.w);
    check(
        near(placed.centre.truncate(), Vec3::new(10.5, 0.5, -1.0))
            && near(placed.x.truncate(), Vec3::NEG_Z)
            && near(halves, Vec3::new(1.0, 0.5, 0.5)),
        format!(
            "centre {}, x axis {}, half sizes {halves}",
            placed.centre, placed.x
        ),
    )
}

/// The sphere holding every caster's reach holds each caster's sphere grown
/// by the reach, for casters 20 m apart. With no casters it is empty, so no
/// point lies in it.
fn reach_holds_casters() -> Result<String, String> {
    let spheres = [
        Vec4::new(0.0, 1.0, 0.0, 0.9),
        Vec4::new(20.0, 1.0, 0.0, 0.9),
        Vec4::new(5.0, 4.0, -3.0, 2.5),
    ];
    let reach = reach_of(spheres.iter().copied(), 6.0);
    let outside: Vec<_> = spheres
        .iter()
        .filter(|s| reach.truncate().distance(s.truncate()) + s.w + 6.0 > reach.w + 1e-4)
        .collect();
    let empty = reach_of(std::iter::empty(), 6.0);
    check(
        outside.is_empty() && empty == Vec4::ZERO,
        format!("reach {reach}, leaves out {outside:?}; with no casters {empty}"),
    )
}

/// A droid 1.2 m tall welds with a spark light 0.1 m off the skin of its
/// arm. That light does not cast the droid's shadow, as it would throw the
/// droid's own shadow wide from its skin. A lamp 2 m over its head still
/// casts.
fn held_light_casts_none() -> Result<String, String> {
    let capsules = [
        (Vec3::new(0.0, 0.3, 0.0), Vec3::new(0.0, 0.9, 0.0), 0.3),
        (Vec3::new(0.3, 0.8, 0.0), Vec3::new(0.7, 0.8, 0.0), 0.06),
    ];
    let (centre, radius) = bounds(&capsules);
    let lights = [
        light(Vec3::new(0.86, 0.8, 0.0), 3.0),
        light(Vec3::new(0.0, 3.2, 0.0), 5.0),
    ];
    let mut seen = Seen::default();
    let mut traced = Traced::default();
    let bits = casting_lights(
        &lights,
        (Entity::PLACEHOLDER, centre, radius),
        (usize::MAX, &|at| outside_capsules(&capsules, at)),
        (None, &mut seen, 0),
        &mut traced,
    );
    check(
        bits == 0b10 && traced.held == 1,
        format!(
            "lights casting {bits:#b}, held {}; wanted 0b10 and 1",
            traced.held
        ),
    )
}
