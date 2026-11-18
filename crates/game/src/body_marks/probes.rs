//! Checks of the body marks, on meshes and marks made here (no install).

use std::collections::VecDeque;

use bevy::asset::RenderAssetUsages;
use bevy::mesh::{Indices, PrimitiveTopology, VertexAttributeValues};
use bevy::prelude::*;
use sim::SimId;

use super::{
    BOLT_DEPTH, BOLT_LIFE_MS, BoltStages, DEPTH, Laid, Posed, SIZE, UNIT, bolt_tints, burn_place,
    cover, first_meeting, gore, head_weighted, make_room, mark_axes,
};
use crate::bolt_hits::MOST_BOLT_BURNS;
use crate::probe_kit::{Checks, check};

/// The checks, for `bolt_burns` in the approved tests.
pub fn probes() -> Checks {
    vec![
        ("gore_crosses_window", gore_crosses_window()),
        ("saber_off_skin", saber_off_skin()),
        ("bolt_tints_timing", bolt_tints_timing()),
        ("bolt_cap", bolt_cap()),
        ("bolt_cap_spares_saber", bolt_cap_spares_saber()),
        ("bolt_cap_per_body", bolt_cap_per_body()),
        ("trace_meets_front", trace_meets_front()),
        ("trace_meets_head_first", trace_meets_head_first()),
        ("head_mesh_unmarked", head_mesh_unmarked()),
        ("no_node_no_trail", no_node_no_trail()),
    ]
}

/// A mesh of the points `at` (game metres) and the triangles `triangles`.
fn mesh_of(at: &[Vec3], triangles: &[[u32; 3]]) -> Mesh {
    let mut mesh = Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::default(),
    );
    let positions: Vec<[f32; 3]> = at.iter().map(|point| point.to_array()).collect();
    mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, positions);
    mesh.insert_indices(Indices::U32(triangles.concat()));
    mesh
}

/// How many triangles `mesh` has, and the middle of each (game metres).
fn middles(mesh: &Mesh) -> Vec<Vec3> {
    let Some(VertexAttributeValues::Float32x3(positions)) =
        mesh.attribute(Mesh::ATTRIBUTE_POSITION)
    else {
        return Vec::new();
    };
    let corners: Vec<usize> = match mesh.indices() {
        Some(Indices::U32(list)) => list.iter().map(|&v| v as usize).collect(),
        Some(Indices::U16(list)) => list.iter().map(|&v| usize::from(v)).collect(),
        None => Vec::new(),
    };
    corners
        .as_chunks::<3>()
        .0
        .iter()
        .map(|triangle| {
            triangle
                .iter()
                .map(|&v| Vec3::from_array(positions[v]))
                .sum::<Vec3>()
                / 3.0
        })
        .collect()
}

/// A bolt's mark struck at the origin along +Y, with a triangle that has one
/// corner 5 units in front of the mark's depth window and two corners 6
/// units behind it. The triangle crosses the window, so it is kept. A
/// triangle with all three corners in front of the window is dropped.
fn gore_crosses_window() -> Result<String, String> {
    let ray = Vec3::Y;
    let axes = mark_axes(ray, 0.0, (3.0 * UNIT, 3.0 * UNIT));
    let (front, behind) = (BOLT_DEPTH.0 - 5.0, BOLT_DEPTH.1 + 6.0);
    let corner = |depth: f32, x: f32, z: f32| Vec3::new(x, depth, z) * UNIT;
    let crossing = [
        corner(front, 0.0, 1.0),
        corner(behind, -1.0, 0.0),
        corner(behind, 1.0, 0.0),
    ];
    let in_front = [
        corner(front, 0.0, 1.0),
        corner(front, -1.0, 0.0),
        corner(front, 1.0, 0.0),
    ];
    let kept = |corners: [Vec3; 3]| {
        let mesh = mesh_of(&corners, &[[0, 1, 2]]);
        gore(&mesh, (&corners, &[]), (Vec3::ZERO, ray), axes, BOLT_DEPTH).is_some()
    };
    let (crossing, in_front) = (kept(crossing), kept(in_front));
    check(
        crossing && !in_front,
        format!("the crossing triangle is kept {crossing}; the one in front is kept {in_front}"),
    )
}

/// A lightsaber's mark on a curved strip of 40 triangles: half a cylinder of
/// radius 0.15 m and height 0.06 m, facing −Y. The blade crosses its front
/// along +X. Some triangles are kept, but not all, and the middle of every
/// kept triangle lies within 5 cm of the strip's nearest vertex.
fn saber_off_skin() -> Result<String, String> {
    const SEGMENTS: u32 = 20;
    const RADIUS: f32 = 0.15;
    const HEIGHT: f32 = 0.06;
    let at: Vec<Vec3> = (0..=SEGMENTS)
        .flat_map(|i| {
            let angle = std::f32::consts::PI * i as f32 / SEGMENTS as f32;
            let (x, y) = (RADIUS * angle.cos(), -RADIUS * angle.sin());
            [Vec3::new(x, y, 0.0), Vec3::new(x, y, HEIGHT)]
        })
        .collect();
    let triangles: Vec<[u32; 3]> = (0..SEGMENTS)
        .flat_map(|i| {
            let v = 2 * i;
            [[v, v + 1, v + 2], [v + 1, v + 3, v + 2]]
        })
        .collect();
    let mesh = mesh_of(&at, &triangles);
    let ray = Vec3::X;
    let axes = mark_axes(ray, 0.0, (SIZE.0 * UNIT, SIZE.0 * UNIT));
    let hit = Vec3::new(0.0, -RADIUS, HEIGHT / 2.0);
    let kept = gore(&mesh, (&at, &[]), (hit, ray), axes, DEPTH)
        .map_or_else(Vec::new, |made| middles(&made));
    let farthest_cm = kept
        .iter()
        .map(|middle| {
            at.iter()
                .map(|vertex| vertex.distance(*middle))
                .fold(f32::MAX, f32::min)
        })
        .fold(0.0, f32::max)
        * 100.0;
    check(
        !kept.is_empty() && kept.len() < triangles.len() && farthest_cm <= 5.0,
        format!(
            "{} of {} triangles kept; the farthest middle is {farthest_cm:.1} cm off the strip",
            kept.len(),
            triangles.len()
        ),
    )
}

/// Which layer of a bolt burn a timing row reads.
#[derive(Clone, Copy, Debug)]
enum Layer {
    Burn,
    Embers,
    Flash,
}

/// What a timing row expects of its layer's tint.
#[derive(Clone, Copy, Debug)]
enum Expect {
    /// Near white: red and green above 0.9, blue above 0.7.
    White,
    /// Red: red above 0.5, green under half the red, blue under a fifth.
    Red,
    /// Brighter than nothing.
    Lit,
    /// No colour at all.
    Dark,
    /// The burn's alpha, within 0.01.
    Alpha(f32),
}

/// The tints of a bolt burn's three layers, row by row, `age` ms after the
/// hit, with the burn's life `BOLT_LIFE_MS` and no flicker.
fn bolt_tints_timing() -> Result<String, String> {
    use Expect::*;
    use Layer::*;
    let rows: [(Layer, f32, Expect); 11] = [
        (Flash, 0.0, White),
        (Flash, 300.0, Red),
        (Flash, 600.0, Dark),
        (Embers, 3400.0, Lit),
        (Embers, 3500.0, Dark),
        (Burn, 0.0, Alpha(1.0)),
        (Burn, 6000.0, Alpha(1.0)),
        (Burn, 7000.0, Alpha(0.5)),
        (Burn, 8000.0, Alpha(0.0)),
        (Flash, 8000.0, Dark),
        (Embers, 8000.0, Dark),
    ];
    let wrong: Vec<String> = rows
        .iter()
        .filter_map(|&(layer, age, expect)| {
            let tint = bolt_tints(age, BOLT_LIFE_MS - age, 1.0)[layer as usize];
            let rgb = tint.truncate();
            let right = match expect {
                White => rgb.x > 0.9 && rgb.y > 0.9 && rgb.z > 0.7,
                Red => rgb.x > 0.5 && rgb.y < 0.5 * rgb.x && rgb.z < 0.2 * rgb.x,
                Lit => rgb.max_element() > 0.0,
                Dark => rgb.max_element() <= 0.0,
                Alpha(alpha) => (tint.w - alpha).abs() <= 0.01,
            };
            (!right).then(|| format!("{layer:?} at {age} ms is {tint}, not {expect:?}"))
        })
        .collect();
    if wrong.is_empty() {
        check(
            true,
            format!(
                "all {} rows: the flash is white, then red, then gone at 600 ms; the embers \
                 glow until 3500 ms; the burn fades from 6000 ms to 8000 ms",
                rows.len()
            ),
        )
    } else {
        check(false, wrong.join("; "))
    }
}

/// A mark laid on `who` as number `id`, a bolt burn when `bolt` is set.
fn laid(who: u32, id: u32, bolt: bool) -> Laid {
    Laid {
        who: SimId(who),
        id,
        records: 1,
        until: f32::MAX,
        bolt: bolt.then(|| BoltStages {
            materials: [Handle::default(), Handle::default(), Handle::default()],
            laid_ms: id as f32,
        }),
    }
}

/// The marks `marks` laid one after another on body 1, numbered from 0. Each
/// is a bolt burn when it is `true`.
fn laid_on_one(marks: impl IntoIterator<Item = bool>) -> VecDeque<Laid> {
    marks
        .into_iter()
        .enumerate()
        .map(|(id, bolt)| laid(1, id as u32, bolt))
        .collect()
}

/// Six bolt burns on one body, then a seventh: the first goes to make room.
fn bolt_cap() -> Result<String, String> {
    let marks = laid_on_one([true; MOST_BOLT_BURNS]);
    let fewer = laid_on_one([true; MOST_BOLT_BURNS - 1]);
    let (full, short) = (make_room(&marks, SimId(1)), make_room(&fewer, SimId(1)));
    check(
        full == Some(0) && short.is_none(),
        format!(
            "with {MOST_BOLT_BURNS} bolt burns the one to go is {full:?}; with {} it is {short:?}",
            MOST_BOLT_BURNS - 1
        ),
    )
}

/// A body with 16 lightsaber marks laid first and six bolt burns after them:
/// the first bolt burn goes, never a lightsaber mark. A body with lightsaber
/// marks only gives none up.
fn bolt_cap_spares_saber() -> Result<String, String> {
    const SABER_MARKS: usize = 16;
    let both = laid_on_one(
        std::iter::repeat_n(false, SABER_MARKS).chain(std::iter::repeat_n(true, MOST_BOLT_BURNS)),
    );
    let sabers = laid_on_one([false; SABER_MARKS]);
    let (mixed, saber_only) = (make_room(&both, SimId(1)), make_room(&sabers, SimId(1)));
    check(
        mixed == Some(SABER_MARKS as u32) && saber_only.is_none(),
        format!(
            "with {SABER_MARKS} lightsaber marks and {MOST_BOLT_BURNS} bolt burns the one to go is \
             {mixed:?}; with lightsaber marks only it is {saber_only:?}"
        ),
    )
}

/// Six bolt burns on body 1 make no room on body 2.
fn bolt_cap_per_body() -> Result<String, String> {
    let marks = laid_on_one([true; MOST_BOLT_BURNS]);
    let other = make_room(&marks, SimId(2));
    check(
        other.is_none(),
        format!("with {MOST_BOLT_BURNS} bolt burns on another body, the one to go is {other:?}"),
    )
}

/// The synthetic body's trunk: a box 0.4 m wide (x), 0.25 m deep (y) and
/// 0.6 m tall (z), standing on the origin and facing −Y. Its sides are cut
/// at 0.25 m and 0.5 m. Every vertex is skinned to the trunk's joint (0),
/// except the top ring at the neck, which is skinned to the head's joint
/// (1).
const TRUNK_RINGS: [f32; 4] = [0.0, 0.25, 0.5, 0.6];
const TRUNK_HALF: Vec2 = Vec2::new(0.2, 0.125);
/// The synthetic body's head: a box 0.2 m on each side across, from 0.65 m
/// to 0.9 m up, as a separate mesh hung under the head's node.
const HEAD_HALF: f32 = 0.1;
const HEAD_SPAN: (f32, f32) = (0.65, 0.9);

/// The four corners of a box's ring at height `z`, half `half` across.
fn ring(half: Vec2, z: f32) -> [Vec3; 4] {
    [
        Vec3::new(-half.x, -half.y, z),
        Vec3::new(half.x, -half.y, z),
        Vec3::new(half.x, half.y, z),
        Vec3::new(-half.x, half.y, z),
    ]
}

/// The triangles of a box made of `rings` rings of four corners, one ring
/// after another, with its bottom and top closed.
fn box_triangles(rings: u32) -> Vec<[u32; 3]> {
    let sides = (0..rings - 1).flat_map(|r| {
        (0..4).flat_map(move |k| {
            let (a, b) = (r * 4 + k, r * 4 + (k + 1) % 4);
            let (c, d) = (a + 4, b + 4);
            [[a, b, d], [a, d, c]]
        })
    });
    let top = (rings - 1) * 4;
    sides
        .chain([
            [0, 2, 1],
            [0, 3, 2],
            [top, top + 1, top + 2],
            [top, top + 2, top + 3],
        ])
        .collect()
}

/// The synthetic body: its trunk and its head, posed, with the meshes they
/// are made of.
fn body() -> (Vec<Posed>, Assets<Mesh>) {
    let mut meshes = Assets::<Mesh>::default();
    let trunk_at: Vec<Vec3> = TRUNK_RINGS
        .iter()
        .flat_map(|&z| ring(TRUNK_HALF, z))
        .collect();
    let neck = 4 * (TRUNK_RINGS.len() - 1);
    let joints: Vec<[u16; 4]> = (0..trunk_at.len())
        .map(|v| [u16::from(v >= neck), 0, 0, 0])
        .collect();
    let weights = vec![[1.0, 0.0, 0.0, 0.0]; trunk_at.len()];
    let mut trunk = mesh_of(&trunk_at, &box_triangles(TRUNK_RINGS.len() as u32));
    trunk.insert_attribute(
        Mesh::ATTRIBUTE_JOINT_INDEX,
        VertexAttributeValues::Uint16x4(joints.clone()),
    );
    trunk.insert_attribute(Mesh::ATTRIBUTE_JOINT_WEIGHT, weights.clone());
    let head_at: Vec<Vec3> = [HEAD_SPAN.0, HEAD_SPAN.1]
        .iter()
        .flat_map(|&z| ring(Vec2::splat(HEAD_HALF), z))
        .collect();
    let head = mesh_of(&head_at, &box_triangles(2));
    let posed = vec![
        Posed {
            entity: Entity::PLACEHOLDER,
            mesh: meshes.add(trunk),
            skin: None,
            head_vertex: head_weighted(&joints, &weights, &[false, true]),
            at: trunk_at,
            head_mesh: false,
        },
        Posed {
            entity: Entity::PLACEHOLDER,
            mesh: meshes.add(head),
            skin: None,
            head_vertex: vec![false; head_at.len()],
            at: head_at,
            head_mesh: true,
        },
    ];
    (posed, meshes)
}

/// A line through the trunk's middle along +Y meets the trunk's front face,
/// at y = −0.125 m, not its back.
fn trace_meets_front() -> Result<String, String> {
    let (posed, meshes) = body();
    let line = (Vec3::new(0.0, -2.0, 0.3), Vec3::Y);
    let met = first_meeting(&posed, &meshes, line, 4.0);
    let front = met.is_some_and(|(point, head)| (point.y + TRUNK_HALF.y).abs() < 1e-4 && !head);
    check(front, format!("the line meets {met:?}"))
}

/// A line straight down through the head and then the trunk meets the head
/// first, so the bolt leaves no burn.
fn trace_meets_head_first() -> Result<String, String> {
    let (posed, meshes) = body();
    let line = (Vec3::new(0.02, 0.01, 2.0), Vec3::NEG_Z);
    let met = first_meeting(&posed, &meshes, line, 4.0);
    let place = burn_place(&posed, &meshes, line, Vec3::new(0.0, 0.0, 0.3));
    let head_first = met.is_some_and(|(point, head)| head && (point.z - HEAD_SPAN.1).abs() < 1e-4);
    check(
        head_first && place.is_none(),
        format!("the line meets {met:?}; the burn goes at {place:?}"),
    )
}

/// A burn at the neck, 0.1 m across either way, covers no triangle of the
/// head's mesh and no trunk triangle with a corner skinned to the head's
/// joint. It still covers trunk triangles below the neck.
fn head_mesh_unmarked() -> Result<String, String> {
    let (posed, meshes) = body();
    let ray = Vec3::Y;
    let axes = mark_axes(ray, 0.0, (0.1, 0.1));
    let at = Vec3::new(0.0, -TRUNK_HALF.y, 0.58);
    let covered = cover(posed, &meshes, (at, ray), axes, BOLT_DEPTH, true);
    let head_meshes = covered.iter().filter(|(posed, _)| posed.head_mesh).count();
    let head_corners: usize = covered
        .iter()
        .map(
            |(_, made)| match made.attribute(Mesh::ATTRIBUTE_JOINT_INDEX) {
                Some(VertexAttributeValues::Uint16x4(joints)) => {
                    joints.iter().filter(|joint| joint[0] == 1).count()
                }
                _ => 0,
            },
        )
        .sum();
    let triangles: usize = covered.iter().map(|(_, made)| middles(made).len()).sum();
    check(
        head_meshes == 0 && head_corners == 0 && triangles > 0,
        format!(
            "{triangles} trunk triangles covered; {head_meshes} head meshes and {head_corners} \
             corners skinned to the head covered"
        ),
    )
}

/// With no hit location node and no bolt's trail, the burn aims at the
/// chest. Here the line passes 3 m beside the body and meets nothing, so the
/// burn goes on the trunk vertex nearest the chest point that is not the
/// head's. The chest point is put near the neck, so that a head vertex and a
/// vertex skinned to the head are nearer to it than any other.
fn no_node_no_trail() -> Result<String, String> {
    let (posed, meshes) = body();
    let chest = Vec3::new(0.0, -0.05, 0.62);
    let line = (Vec3::new(3.0, -2.0, 0.3), Vec3::Y);
    let place = burn_place(&posed, &meshes, line, chest);
    let expected = posed[0]
        .at
        .iter()
        .zip(&posed[0].head_vertex)
        .filter(|(_, head)| !**head)
        .map(|(point, _)| *point)
        .min_by(|a, b| a.distance(chest).total_cmp(&b.distance(chest)));
    check(
        place.is_some() && place == expected,
        format!(
            "the burn goes at {place:?}; the nearest trunk vertex not the head's is {expected:?}"
        ),
    )
}
