//! Checks of the light probes on rooms, lightmaps and lamps made here (no
//! install). A probe over a lit floor takes the floor's light from above
//! and the floor's colour from below. A side facing a wall takes that
//! wall's colour. A lit doorway lights only the side that faces it. Two
//! rooms side by side never blend their light. A lamp's share is taken out
//! of the light from above. A model outside every room keeps the flat
//! ambient. A shadow takes the room's light from its strongest side.

use bevy::prelude::*;

use super::{AreaProbes, Fallback, Pixels, Probe, RoomProbes, Surface, bake_rooms, without_lamps};
use crate::material::{GpuLight, LIGHT_COMBAT};
use crate::probe_kit::{Checks, check};
use crate::viewer::RoomView;

/// The checks. They are proposed for the approved tests, not added there.
pub fn probes() -> Checks {
    vec![
        ("floor_above_and_below", floor_above_and_below()),
        ("wall_on_its_side", wall_on_its_side()),
        ("doorway_lights_its_side", doorway_lights_its_side()),
        ("no_blend_across_rooms", no_blend_across_rooms()),
        ("lamp_share_taken_out", lamp_share_taken_out()),
        ("outside_keeps_flat_ambient", outside_keeps_flat_ambient()),
        ("room_light_direction", room_light_direction()),
    ]
}

/// The source of the room's light a shadow takes (`room_light`). Light alike
/// on all four sides comes from straight above, from the wide source. A
/// doorway lighting the east side alone, with a dim floor, gives a source
/// to the east, lifted to the least elevation (0.6 radians) and narrow
/// (0.2 radians). Light from the east outweighing the west by 0.2, with
/// 0.3 from above, gives a source between the two, along (0.2, 0.3, 0),
/// a little narrower than the wide one. The side facing down counts for
/// nothing. No light at all comes from straight above.
fn room_light_direction() -> Result<String, String> {
    let even = super::room_light(&[
        Vec3::splat(0.2),
        Vec3::splat(0.2),
        Vec3::splat(0.5),
        Vec3::splat(0.1),
        Vec3::splat(0.2),
        Vec3::splat(0.2),
    ]);
    let doorway = super::room_light(&[
        Vec3::splat(0.6),
        Vec3::ZERO,
        Vec3::splat(0.1),
        Vec3::splat(0.1),
        Vec3::ZERO,
        Vec3::ZERO,
    ]);
    let between = super::room_light(&[
        Vec3::splat(0.4),
        Vec3::splat(0.2),
        Vec3::splat(0.3),
        Vec3::splat(0.1),
        Vec3::splat(0.2),
        Vec3::splat(0.2),
    ]);
    let dark = super::room_light(&[Vec3::ZERO; 6]);
    let close = |a: Vec4, b: Vec4| a.distance(b) < 0.01;
    let lowest = Vec4::new(0.6f32.cos(), 0.6f32.sin(), 0.0, 0.2);
    let tilted = Vec3::new(0.2, 0.3, 0.0).normalize().extend(0.0);
    check(
        close(even, Vec4::new(0.0, 1.0, 0.0, 0.45))
            && close(doorway, lowest)
            && close(between.truncate().extend(0.0), tilted)
            && between.w < 0.45
            && between.w > 0.3
            && close(dark, Vec4::new(0.0, 1.0, 0.0, 0.45)),
        format!("even {even}, doorway {doorway}, between {between}, dark {dark}"),
    )
}

/// The two triangles of the quad `a b c d`, its front turned toward
/// `front`, lit by lightmap `map` and drawn with texture `texture` (both
/// indices into the check's `Pixels`; no texture draws white).
fn quad(
    [a, b, c, d]: [Vec3; 4],
    front: Vec3,
    (map, texture): (Option<usize>, Option<usize>),
) -> [Surface; 2] {
    let turns = (b - a).cross(c - a).dot(front) >= 0.0;
    let [a, b, c, d] = if turns { [a, b, c, d] } else { [a, d, c, b] };
    let surface = |corners| Surface {
        corners,
        uv: [Vec2::splat(0.5); 3],
        lightmap: map,
        texture,
        texture_uv: [Vec2::splat(0.5); 3],
        glow: Vec3::ZERO,
    };
    [surface([a, b, c]), surface([a, c, d])]
}

/// A walkable floor over the square from (0, 0) to (`side`, `side`) at
/// height 0, as a walkmesh has it.
fn floor(side: f32) -> Vec<[[f32; 3]; 3]> {
    vec![
        [[0.0, 0.0, 0.0], [side, 0.0, 0.0], [side, side, 0.0]],
        [[0.0, 0.0, 0.0], [side, side, 0.0], [0.0, side, 0.0]],
    ]
}

/// The square from (0, 0) to (`side`, `side`) at height `z`.
fn flat(side: f32, z: f32) -> [Vec3; 4] {
    [
        Vec3::new(0.0, 0.0, z),
        Vec3::new(side, 0.0, z),
        Vec3::new(side, side, z),
        Vec3::new(0.0, side, z),
    ]
}

/// A wall 5 m high across the line from `from` to `to` (x and y).
fn wall(from: Vec2, to: Vec2) -> [Vec3; 4] {
    [
        from.extend(0.0),
        to.extend(0.0),
        to.extend(5.0),
        from.extend(5.0),
    ]
}

/// The probe of `room` nearest `at`.
fn probe_near(rooms: &[RoomProbes], room: usize, at: Vec3) -> Option<Probe> {
    rooms[room]
        .probes
        .iter()
        .min_by(|a, b| a.at.distance(at).total_cmp(&b.at.distance(at)))
        .copied()
}

fn near(a: Vec3, b: Vec3) -> bool {
    a.distance(b) < 0.02
}

/// A floor lit 0.8 and drawn with a grey texture of 0.5. The probe 1 m over
/// its middle takes the floor's lightmap, 0.8, from above. Its side facing
/// down sees the floor as drawn, 0.8 times 0.5.
fn floor_above_and_below() -> Result<String, String> {
    let maps = [
        Pixels::flat(Vec3::splat(0.8)),
        Pixels::flat(Vec3::splat(0.5)),
    ];
    let surfaces = quad(flat(2.0, 0.0), Vec3::Z, (Some(0), Some(1)));
    let floor = floor(2.0);
    let rooms = bake_rooms(&[&floor[..]], &surfaces, &maps);
    let probe = probe_near(&rooms, 0, Vec3::new(1.0, 1.0, 1.0)).ok_or("no probe")?;
    let (up, down) = (probe.light[2], probe.light[3]);
    check(
        probe.lit
            && probe.at == Vec3::new(1.0, 1.0, 1.0)
            && near(up, Vec3::splat(0.8))
            && near(down, Vec3::splat(0.4)),
        format!(
            "{} probes; the middle one at {} takes {up} from above and {down} from below",
            rooms[0].probes.len(),
            probe.at
        ),
    )
}

/// A red wall 2 m west of the middle probe and a blue wall 2 m east of it,
/// on a grey floor. The probe's west side (Bevy's −x) faces the red wall
/// and takes mostly red. Its east side takes mostly blue. Rays leaning
/// down meet the grey floor on the way, so neither colour is pure.
fn wall_on_its_side() -> Result<String, String> {
    let red = Vec3::new(0.9, 0.1, 0.1);
    let blue = Vec3::new(0.1, 0.1, 0.9);
    let maps = [
        Pixels::flat(Vec3::splat(0.5)),
        Pixels::flat(red),
        Pixels::flat(blue),
    ];
    let surfaces: Vec<Surface> = [
        quad(flat(2.0, 0.0), Vec3::Z, (Some(0), None)),
        quad(
            wall(Vec2::new(-1.0, -1.0), Vec2::new(-1.0, 3.0)),
            Vec3::X,
            (Some(1), None),
        ),
        quad(
            wall(Vec2::new(3.0, -1.0), Vec2::new(3.0, 3.0)),
            Vec3::NEG_X,
            (Some(2), None),
        ),
    ]
    .into_iter()
    .flatten()
    .collect();
    let floor = floor(2.0);
    let rooms = bake_rooms(&[&floor[..]], &surfaces, &maps);
    let probe = probe_near(&rooms, 0, Vec3::new(1.0, 1.0, 1.0)).ok_or("no probe")?;
    let (east, west) = (probe.light[0], probe.light[1]);
    check(
        west.x > west.z + 0.4 && east.z > east.x + 0.4,
        format!(
            "west side {west} (want mostly the red wall's {red}), east side {east} (want mostly the blue wall's {blue})"
        ),
    )
}

/// A dark corridor (a floor and a wall behind it lit 0.05) with a lit
/// doorway 3 m ahead of the probe: a wall facing it, lit 0.9, as the hall
/// seen through a door. The probe's side facing the doorway (north, the
/// game's +y, Bevy's −z) takes its light. The side facing back down the
/// corridor (south) stays dark. A character standing there is lit on the
/// side toward the door only.
fn doorway_lights_its_side() -> Result<String, String> {
    let maps = [
        Pixels::flat(Vec3::splat(0.05)),
        Pixels::flat(Vec3::splat(0.9)),
    ];
    let surfaces: Vec<Surface> = [
        quad(flat(2.0, 0.0), Vec3::Z, (Some(0), None)),
        quad(
            wall(Vec2::new(-4.0, 4.0), Vec2::new(6.0, 4.0)),
            Vec3::NEG_Y,
            (Some(1), None),
        ),
        quad(
            wall(Vec2::new(-4.0, -2.0), Vec2::new(6.0, -2.0)),
            Vec3::Y,
            (Some(0), None),
        ),
    ]
    .into_iter()
    .flatten()
    .collect();
    let floor = floor(2.0);
    let rooms = bake_rooms(&[&floor[..]], &surfaces, &maps);
    let probe = probe_near(&rooms, 0, Vec3::new(1.0, 1.0, 1.0)).ok_or("no probe")?;
    let (south, north) = (probe.light[4], probe.light[5]);
    check(
        north.x > 0.6 && south.x < 0.1,
        format!("the side facing the doorway {north}, the side facing back {south}"),
    )
}

/// A room whose VIS lists `visible`, with a floor.
fn room(name: &str, visible: &[&str]) -> RoomView {
    RoomView {
        name: name.into(),
        model: None,
        visible: visible.iter().map(|name| name.to_string()).collect(),
        floor: floor(2.0),
        floor_error: None,
    }
}

/// A grid of probes 1 m apart over `low` to `high` (x and y), 1 m up, all
/// holding `light`.
fn probes_over(low: Vec2, high: Vec2, light: Vec3) -> RoomProbes {
    let mut probes = Vec::new();
    let mut x = low.x;
    while x <= high.x {
        let mut y = low.y;
        while y <= high.y {
            probes.push(Probe {
                at: Vec3::new(x, y, 1.0),
                light: [light; 6],
                lit: true,
            });
            y += 1.0;
        }
        x += 1.0;
    }
    RoomProbes::new(probes)
}

fn area(rooms: Vec<RoomProbes>) -> AreaProbes {
    AreaProbes {
        rooms,
        ..AreaProbes::default()
    }
}

/// Room A (probes over x 0 to 2, all white) and room B (over x 3 to 5,
/// all black) stand side by side, and neither VIS lists the other. A model
/// in room A 0.4 m from the wall between, within reach of B's nearest
/// probes, takes A's white alone. Were B's VIS to list A, a model in A where
/// A has no probe near would take B's light.
fn no_blend_across_rooms() -> Result<String, String> {
    let probes = area(vec![
        probes_over(Vec2::ZERO, Vec2::new(2.0, 2.0), Vec3::ONE),
        probes_over(Vec2::new(3.0, 0.0), Vec2::new(5.0, 2.0), Vec3::ZERO),
    ]);
    let apart = [room("a", &[]), room("b", &[])];
    let at = Vec3::new(2.6, 1.0, 1.0);
    let in_a = probes.light_at(&apart, Some(0), at);
    let linked = [room("a", &[]), room("b", &["a"])];
    let past_a = Vec3::new(3.8, 1.0, 1.0);
    let lent = probes.light_at(&linked, Some(0), past_a);
    let unlinked = probes.light_at(&apart, Some(0), past_a);
    check(
        in_a == Ok([Vec3::ONE; 6])
            && lent == Ok([Vec3::ZERO; 6])
            && unlinked == Err(Fallback::NoneNear),
        format!(
            "in room A by the wall {in_a:?}; past A's probes, B linked {lent:?}, B apart {unlinked:?}"
        ),
    )
}

/// Light 0.5 on every side, under a white lamp 2 m over the point the
/// light is read at (radius 10, multiplier 0.2). The lamp adds
/// 0.2 × 10² / 12² ≈ 0.139 to a surface facing up, so that much is taken
/// out of the light from above, which is read off the floor's lightmap.
/// The other five sides are what the probe sees, which holds no lamp, and
/// keep 0.5. A fight's light at the same place takes nothing out, and neither
/// does a lamp that darkens (multiplier −0.2).
fn lamp_share_taken_out() -> Result<String, String> {
    let lamp = GpuLight {
        position_radius: Vec4::new(0.0, 3.0, 0.0, 10.0),
        color_multiplier: Vec4::new(1.0, 1.0, 1.0, 0.2),
        ..default()
    };
    let fight = GpuLight {
        kind: UVec4::new(0, 0, LIGHT_COMBAT, 0),
        ..lamp
    };
    let dark = GpuLight {
        color_multiplier: Vec4::new(1.0, 1.0, 1.0, -0.2),
        ..lamp
    };
    let at = (Vec3::ZERO, Vec3::Y);
    let lit = without_lamps([Vec3::splat(0.5); 6], &[lamp], at, false);
    let fought = without_lamps([Vec3::splat(0.5); 6], &[fight], at, false);
    let darkened = without_lamps([Vec3::splat(0.5); 6], &[dark], at, false);
    let share = 0.2 * 100.0 / 144.0;
    let others_kept = [0, 1, 3, 4, 5]
        .iter()
        .all(|&side| near(lit[side], Vec3::splat(0.5)));
    check(
        near(lit[2], Vec3::splat(0.5 - share))
            && others_kept
            && fought == [Vec3::splat(0.5); 6]
            && darkened == [Vec3::splat(0.5); 6],
        format!(
            "with the lamp {lit:?}; with a fight's light {fought:?}; with a darkening lamp {darkened:?}"
        ),
    )
}

/// A model in no room keeps the flat ambient, and so does one in an area
/// with no probes, or in a room whose probes met no lightmap.
fn outside_keeps_flat_ambient() -> Result<String, String> {
    let rooms = [room("a", &[])];
    let probes = area(vec![probes_over(
        Vec2::ZERO,
        Vec2::new(2.0, 2.0),
        Vec3::ONE,
    )]);
    let outside = probes.light_at(&rooms, None, Vec3::new(1.0, 1.0, 1.0));
    let none = area(Vec::new()).light_at(&rooms, Some(0), Vec3::new(1.0, 1.0, 1.0));
    let floor = floor(2.0);
    let dark = area(bake_rooms(&[&floor[..]], &[], &[]));
    let unlit = dark.light_at(&rooms, Some(0), Vec3::new(1.0, 1.0, 1.0));
    check(
        outside == Err(Fallback::Outside)
            && none == Err(Fallback::NoProbes)
            && unlit == Err(Fallback::NoneNear),
        format!(
            "outside every room {outside:?}; no probes {none:?}; probes without light {unlit:?}"
        ),
    )
}
