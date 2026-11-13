//! Checks of the light probes on rooms, lightmaps and lamps made here (no
//! install). A probe over a lit floor under a dark ceiling takes the
//! floor's light from above and the ceiling's from below. A wall's colour
//! reaches only the side of a probe that faces the wall. Two rooms side by
//! side never blend their light. A lamp's share is taken out of the light
//! it would add to. A model outside every room keeps the flat ambient.

use bevy::prelude::*;

use super::{AreaProbes, Fallback, Pixels, Probe, RoomProbes, Surface, bake_rooms, without_lamps};
use crate::material::{GpuLight, LIGHT_COMBAT};
use crate::probe_kit::{Checks, check};
use crate::viewer::RoomView;

/// The checks. They are proposed for the approved tests, not added there.
pub fn probes() -> Checks {
    vec![
        ("lit_floor_dark_ceiling", lit_floor_dark_ceiling()),
        ("wall_on_its_side", wall_on_its_side()),
        ("no_blend_across_rooms", no_blend_across_rooms()),
        ("lamp_share_taken_out", lamp_share_taken_out()),
        ("outside_keeps_flat_ambient", outside_keeps_flat_ambient()),
    ]
}

/// The two triangles of the quad `a b c d`, its front the way the corners
/// turn (`(b − a) × (c − a)`), lit by lightmap `map`.
fn quad([a, b, c, d]: [Vec3; 4], map: Option<usize>) -> [Surface; 2] {
    let surface = |corners| Surface {
        corners,
        uv: [Vec2::splat(0.5); 3],
        lightmap: map,
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

/// The floor's quad at height `z`, facing up, or down when `down`.
fn flat(side: f32, z: f32, down: bool) -> [Vec3; 4] {
    let [a, b, c, d] = [
        Vec3::new(0.0, 0.0, z),
        Vec3::new(side, 0.0, z),
        Vec3::new(side, side, z),
        Vec3::new(0.0, side, z),
    ];
    if down { [a, d, c, b] } else { [a, b, c, d] }
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

/// A floor lit 0.8 under a ceiling lit 0.1, 3 m above. The probe 1 m over
/// the middle takes 0.8 on its side facing up and 0.1 on its side facing
/// down: the floor holds the light from above, the ceiling the light from
/// below.
fn lit_floor_dark_ceiling() -> Result<String, String> {
    let maps = [
        Pixels::flat(Vec3::splat(0.8)),
        Pixels::flat(Vec3::splat(0.1)),
    ];
    let surfaces: Vec<Surface> = [
        quad(flat(2.0, 0.0, false), Some(0)),
        quad(flat(2.0, 3.0, true), Some(1)),
    ]
    .into_iter()
    .flatten()
    .collect();
    let floor = floor(2.0);
    let rooms = bake_rooms(&[&floor[..]], &surfaces, &maps);
    let probe = probe_near(&rooms, 0, Vec3::new(1.0, 1.0, 1.0)).ok_or("no probe")?;
    let (up, down) = (probe.light[2], probe.light[3]);
    check(
        probe.lit
            && probe.at == Vec3::new(1.0, 1.0, 1.0)
            && near(up, Vec3::splat(0.8))
            && near(down, Vec3::splat(0.1)),
        format!(
            "{} probes; the middle one at {} takes {up} from above and {down} from below",
            rooms[0].probes.len(),
            probe.at
        ),
    )
}

/// A red wall facing east 1 m west of a 2 m floor, and a blue wall facing
/// west 1 m east of it. The middle probe's east side (Bevy's +x) takes the
/// red wall it faces, and its west side the blue one.
fn wall_on_its_side() -> Result<String, String> {
    let red = Vec3::new(0.9, 0.1, 0.1);
    let blue = Vec3::new(0.1, 0.1, 0.9);
    let maps = [
        Pixels::flat(Vec3::splat(0.5)),
        Pixels::flat(red),
        Pixels::flat(blue),
    ];
    let west_wall = [
        Vec3::new(-1.0, -1.0, 0.0),
        Vec3::new(-1.0, 3.0, 0.0),
        Vec3::new(-1.0, 3.0, 3.0),
        Vec3::new(-1.0, -1.0, 3.0),
    ];
    let [a, b, c, d] = west_wall.map(|p| p + Vec3::X * 4.0);
    let east_wall = [a, d, c, b];
    let surfaces: Vec<Surface> = [
        quad(flat(2.0, 0.0, false), Some(0)),
        quad(west_wall, Some(1)),
        quad(east_wall, Some(2)),
    ]
    .into_iter()
    .flatten()
    .collect();
    let floor = floor(2.0);
    let rooms = bake_rooms(&[&floor[..]], &surfaces, &maps);
    let probe = probe_near(&rooms, 0, Vec3::new(1.0, 1.0, 1.0)).ok_or("no probe")?;
    let (east, west) = (probe.light[0], probe.light[1]);
    check(
        near(east, red) && near(west, blue),
        format!(
            "east side {east} (want the red wall's {red}), west side {west} (want the blue wall's {blue})"
        ),
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
/// out of the up side. The other five sides face away or across and keep
/// 0.5. A fight's light at the same place takes nothing out.
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
    let at = (Vec3::ZERO, Vec3::Y);
    let lit = without_lamps([Vec3::splat(0.5); 6], &[lamp], at, false);
    let fought = without_lamps([Vec3::splat(0.5); 6], &[fight], at, false);
    let share = 0.2 * 100.0 / 144.0;
    let others_kept = [0, 1, 3, 4, 5]
        .iter()
        .all(|&side| near(lit[side], Vec3::splat(0.5)));
    check(
        near(lit[2], Vec3::splat(0.5 - share)) && others_kept && fought == [Vec3::splat(0.5); 6],
        format!("with the lamp {lit:?}; with a fight's light {fought:?}"),
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
