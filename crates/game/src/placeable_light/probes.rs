//! Checks of the placeables' baked light, on meshes and files made here
//! (no install): how a mesh is split before the bake, and how the game
//! reads the light a bake wrote for each placeable.

use std::collections::BTreeMap;

use asset_odyssey::model::MeshIr;

use super::{FILE, LONGEST_EDGE, read, subdivide};
use crate::probe_kit::{Checks, check};
use crate::stored_fill::StoredMap;

/// The checks, for `placeable_light_sets` in the approved tests.
pub fn probes() -> Checks {
    vec![
        (
            "subdivide_splits_every_edge_down_to_the_longest",
            subdivide_splits_every_edge_down_to_the_longest(),
        ),
        (
            "subdivide_leaves_a_small_triangle_alone",
            subdivide_leaves_a_small_triangle_alone(),
        ),
        (
            "read_takes_the_stored_texels_and_their_repeats",
            read_takes_the_stored_texels_and_their_repeats(),
        ),
        (
            "read_refuses_another_area_and_an_old_format",
            read_refuses_another_area_and_an_old_format(),
        ),
    ]
}

/// A mesh of one triangle with these corners, normals up and texture
/// coordinates from 0 to 1.
fn triangle(corners: [[f32; 3]; 3]) -> MeshIr {
    MeshIr {
        node: 0,
        positions: corners.to_vec(),
        normals: vec![[0.0, 0.0, 1.0]; 3],
        uv1: vec![[0.0, 0.0], [1.0, 0.0], [0.0, 1.0]],
        uv2: vec![[0.0, 0.0], [1.0, 0.0], [0.0, 1.0]],
        indices: vec![0, 1, 2],
        texture: None,
        lightmap: None,
        transparency_hint: 0,
        skinned: false,
        shadow: false,
        self_illum: [0.0; 3],
        alpha: 1.0,
        ambient: [0.0; 3],
        diffuse: [0.0; 3],
        uv_scroll: None,
        skin: None,
        dangly: None,
        referenced: false,
    }
}

fn length(p: [f32; 3], q: [f32; 3]) -> f32 {
    (0..3).map(|k| (p[k] - q[k]).powi(2)).sum::<f32>().sqrt()
}

/// A triangle 2 m on its short sides is split until no edge is longer
/// than `LONGEST_EDGE`. Every vertex keeps a normal and both texture
/// coordinates, the triangles still cover 2 square metres, and a new
/// vertex takes the mean of its edge's ends.
fn subdivide_splits_every_edge_down_to_the_longest() -> Result<String, String> {
    let mesh = triangle([[0.0, 0.0, 0.0], [2.0, 0.0, 0.0], [0.0, 2.0, 0.0]]);
    let split = subdivide(&mesh);
    check(
        split.indices.len() > mesh.indices.len(),
        format!("{} indices after the split", split.indices.len()),
    )?;
    let vertices = split.positions.len();
    check(
        split.normals.len() == vertices
            && split.uv1.len() == vertices
            && split.uv2.len() == vertices,
        format!(
            "{vertices} positions, {} normals, {} uv1 and {} uv2",
            split.normals.len(),
            split.uv1.len(),
            split.uv2.len()
        ),
    )?;
    for triangle in split.indices.as_chunks::<3>().0 {
        for i in 0..3 {
            let (a, b) = (triangle[i] as usize, triangle[(i + 1) % 3] as usize);
            let edge = length(split.positions[a], split.positions[b]);
            check(
                edge <= LONGEST_EDGE + 1e-5,
                format!("an edge of {edge} m is left"),
            )?;
        }
    }
    let area: f32 = (split.indices.as_chunks::<3>().0.iter())
        .map(|&[a, b, c]| {
            let (p, q, r) = (
                split.positions[a as usize],
                split.positions[b as usize],
                split.positions[c as usize],
            );
            ((q[0] - p[0]) * (r[1] - p[1]) - (r[0] - p[0]) * (q[1] - p[1])).abs() / 2.0
        })
        .sum();
    check(
        (area - 2.0).abs() < 1e-4,
        format!("the split covers {area} m2"),
    )?;
    let middle = (split.positions.iter())
        .position(|p| length(*p, [1.0, 0.0, 0.0]) < 1e-6)
        .ok_or("the long edge's middle is not a vertex")?;
    check(
        split.uv1[middle] == [0.5, 0.0],
        format!(
            "the long edge's middle has uv1 {:?}; {} triangles, {area} m2",
            split.uv1[middle],
            split.indices.len() / 3
        ),
    )
}

/// A triangle whose edges are all shorter than `LONGEST_EDGE` is kept as
/// it is.
fn subdivide_leaves_a_small_triangle_alone() -> Result<String, String> {
    let mesh = triangle([[0.0, 0.0, 0.0], [0.3, 0.0, 0.0], [0.0, 0.3, 0.0]]);
    let split = subdivide(&mesh);
    check(
        split.positions == mesh.positions && split.indices == mesh.indices,
        format!(
            "positions {:?} and indices {:?}",
            split.positions, split.indices
        ),
    )
}

/// A scratch folder holding `text` as the set's `FILE`.
fn set_with(name: &str, text: &str) -> std::path::PathBuf {
    let folder = std::env::temp_dir().join(format!("kjk-placeables-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&folder);
    std::fs::create_dir_all(&folder).expect("the scratch folder is made");
    std::fs::write(folder.join(FILE), text).expect("the set's file is written");
    folder
}

/// The light of a vertex followed by a stored texel's word takes the
/// texel's fill, and `@` repeats the last texel. A line whose model is not
/// the placeable's, and a line with fewer vertices than it says, are not
/// kept.
fn read_takes_the_stored_texels_and_their_repeats() -> Result<String, String> {
    let text = format!(
        "# a comment\narea m13aa\nformat {}\nmap m13aa_01a_lm0 4 4\nmap m13aa_02a_lm0 8 8\n\
         mesh 3 plc_chair 1 3 0.1 0.2 0.3 0.4 0.5 0.6 @1:7 0.7 0.8 0.9 @\n\
         mesh 4 plc_other 0 1 1 1 1\n\
         mesh 3 plc_chair 0 2 0.5 0.5 0.5\n",
        crate::bake_fingerprint::FORMAT
    );
    let folder = set_with("format2", &text);
    let models = BTreeMap::from([(3u32, "PLC_CHAIR"), (4, "plc_table")]);
    let mut asked = Vec::new();
    let mut fill = |map: &StoredMap, texel: usize, light: [f32; 3]| {
        asked.push((map.name.clone(), texel));
        light.map(|v| v + 1.0)
    };
    let found = read(&folder, "M13AA", &models, &mut fill);
    let _ = std::fs::remove_dir_all(folder);
    // Object 4's model is not the one the file names. The second line
    // of object 3 says its mesh has two vertices but gives the light of
    // one. Neither line is kept.
    check(
        found.len() == 1,
        format!("{} placeables read, not 1", found.len()),
    )?;
    let chair = found.get(&3).ok_or("object 3 was not read")?;
    check(
        chair.meshes.len() == 2,
        format!("object 3 has {} meshes, not 2", chair.meshes.len()),
    )?;
    check(
        chair.meshes[0].is_none(),
        format!("object 3's first mesh is {:?}", chair.meshes[0]),
    )?;
    check(
        chair.meshes[1] == Some(vec![[0.1, 0.2, 0.3], [1.4, 1.5, 1.6], [1.7, 1.8, 1.9]]),
        format!("object 3's second mesh is {:?}", chair.meshes[1]),
    )?;
    check(
        asked
            == [
                ("m13aa_02a_lm0".to_string(), 7),
                ("m13aa_02a_lm0".to_string(), 7),
            ],
        format!("the fill was asked for {asked:?}"),
    )
}

/// A file of the old format, with no format line, is not read. A file of
/// the current format is read for its own area and not for another.
fn read_refuses_another_area_and_an_old_format() -> Result<String, String> {
    let old = "area m13aa\nmesh 3 plc_chair 0 1 0.1 0.2 0.3\n";
    let models = BTreeMap::from([(3u32, "plc_chair")]);
    let mut fill = |_: &StoredMap, _: usize, light: [f32; 3]| light;
    let folder = set_with("old", old);
    let from_old = read(&folder, "m13aa", &models, &mut fill).len();
    let current = format!(
        "area m13aa\nformat {}\nmesh 3 plc_chair 0 1 0.1 0.2 0.3\n",
        crate::bake_fingerprint::FORMAT
    );
    std::fs::write(folder.join(FILE), &current).expect("the set's file is written");
    let from_current = read(&folder, "m13aa", &models, &mut fill).len();
    let from_other = read(&folder, "m14aa", &models, &mut fill).len();
    let _ = std::fs::remove_dir_all(folder);
    check(
        from_old == 0 && from_current == 1 && from_other == 0,
        format!(
            "placeables read from the old format {from_old}, from the current {from_current}, for another area {from_other}"
        ),
    )
}
