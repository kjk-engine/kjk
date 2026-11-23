//! Baked light for the placeables that never move, under the baked trial
//! (`KJK_FITTED_LIGHTS=baked`).
//!
//! The light probes give a whole model one set of six colours, read 1 metre
//! above its origin. A crate in a dark corner then takes the light of the
//! room's middle, and a cylinder in the shade takes the light bounced off a
//! sunlit cliff over its whole side. The owner said of unk_m44ac on 21
//! November 2026 that the "plasteel cylinder in the shade is too bright",
//! and of 411dxn that the "crates in the corners are too bright, should have
//! same brightness as env".
//!
//! So the baker (`kjk detail-bake --placeable-light`) lights each vertex of
//! such a placeable as it lights a texel of a lightmap. A vertex takes the
//! fitted lights with their shadows, the sun with its shadow, the sky and
//! three bounces, at the scale of the room it stands in and through the
//! same curve. The placeable's own triangles and the other still
//! placeables block its rays. The baker writes the light into `FILE` in the
//! bake's set, one line for each mesh.
//!
//! The game gives each mesh that has a line its light as a vertex colour,
//! and the shader takes it in place of the probes' light, at the gain the
//! lightmaps get (`odyssey.wgsl`'s `VERTEX_LIGHT`).
//!
//! Each mesh of a placeable that has no skin and does not dangle takes the
//! bake's light (`bends`). A mesh that a clip moves, such as the lid of a
//! plasteel cylinder in unk_m44ac, is lit where it rests when the area
//! loads, and it keeps that light when it opens. In the first trial
//! such a lid kept the probes, and its edge showed a seam against the body
//! under it. A skinned mesh and one that dangles keep the probes, because
//! the bake cannot know where their vertices go. Characters and doors keep
//! the probes too.
//!
//! `motions` tells apart the meshes that keep still and those that a clip
//! moves, for the baker's report. The clip `damage` and the clips of a
//! placeable's death are not counted (`after_a_hit`). The first shakes the
//! placeable for a moment when it is hit, and the others break it apart
//! once, when it is destroyed.
//!
//! A placeable that holds one of the fitted lights, such as a terminal
//! whose screen glows, blocks no ray in the bake, so that its housing does
//! not shut in its own light. Its vertices take every light but its own.
//! Its own light would light it as brightly as a wall a few centimetres
//! from a lamp, and danm13's terminal came out a bright blue.
//!
//! The template's `Static` flag (`InstanceIr::is_static`) is not the test.
//! It says that the player cannot select the placeable, and the games use it
//! for scenery. A container that is not static may still never move, and its
//! light can be baked as well.
//!
//! `KJK_PLACEABLE_LIGHT=probes` puts every placeable back on the probes, to
//! compare pictures. A mesh with baked light takes none of the game's own
//! lamps, as a room's surface takes none. `KJK_MODEL_LAMPS=on` gives them
//! back at half, as step 1 of the probes has it. In unk_m44ac one of the
//! game's lamps adds 115 of 255 to the top of the plasteel cylinder in the
//! shade, which its baked light puts at 96 and the floor beside it at 77 to
//! 95 (22 November 2026). `KJK_MODEL_LAMPS=off` (`light_probes::lamps_off`)
//! drops the lamps from every model that the bake lights, characters among
//! them.

use std::collections::BTreeMap;
use std::path::Path;
use std::sync::Arc;

use asset_odyssey::anim::AnimationIr;
use asset_odyssey::model::{MeshIr, ModelIr};
use vfs::Game;
use vfs::resolver::Resolver;

/// The file in a bake's set that holds the placeables' light.
pub const FILE: &str = "placeables.txt";

/// A clip moves a node when it shifts it by more than this from its rest
/// place (metres).
const STILL_SHIFT: f32 = 0.01;
/// A clip turns a node when the cosine of half the angle between its key
/// and the rest is below this. The value is a turn of about 1.6 degrees.
const STILL_TURN: f32 = 0.9999;

/// Whether clip `name` is one the game plays only when the placeable is hit
/// or destroyed: `damage`, `die`, `dead` and the ways into `dead`.
fn after_a_hit(name: &str) -> bool {
    name == "damage" || name == "die" || name.contains("dead")
}

/// Whether the placeables take their baked light, where the bake has it.
/// `KJK_PLACEABLE_LIGHT=probes` keeps them on the probes.
pub fn baked_light() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| std::env::var("KJK_PLACEABLE_LIGHT").map_or(true, |value| value != "probes"))
}

/// Whether a mesh with baked light takes none of the game's own lamps. It
/// takes none by default: the bake holds those lamps already, so they
/// would count twice. Characters and the other models on the probes keep
/// the lamps at half, as step 1 has it, because `light_probes::lamps_off`
/// stays false. The owner walked Peragus this way on 23 November 2026 and
/// said: "we might settle with this for now". `KJK_MODEL_LAMPS=on` gives
/// these meshes the lamps back at half, to compare pictures.
pub fn alone() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| std::env::var("KJK_MODEL_LAMPS").map_or(true, |value| value != "on"))
}

/// The clips the game may play on a placeable whose model is `ir`: the
/// model's own and those of its looks (`own_fx::look_clips`) that its
/// supermodel chain holds, read through `cache`.
pub fn clips(
    cache: &mut BTreeMap<String, Option<ModelIr>>,
    resolver: &Resolver,
    game: Game,
    ir: &ModelIr,
) -> BTreeMap<String, Arc<AnimationIr>> {
    let mut clips: BTreeMap<String, Arc<AnimationIr>> = (ir.animations.iter())
        .map(|clip| (clip.name.to_ascii_lowercase(), Arc::new(clip.clone())))
        .collect();
    for name in crate::own_fx::look_clips() {
        if clips.contains_key(&name) {
            continue;
        }
        match crate::viewer::clip(cache, resolver, game, ir, &name) {
            Ok(Some(found)) => {
                clips.insert(name, found);
            }
            Ok(None) => {}
            Err(why) => bevy::log::warn!("{}: its {name} clip is not read: {why}", ir.name),
        }
    }
    clips
}

/// Why the bake cannot light `mesh`: it has a skin, or it dangles. `None`
/// for a mesh whose vertices keep their places on its node, which the bake
/// lights where it rests.
pub fn bends(mesh: &MeshIr) -> Option<&'static str> {
    if mesh.skin.is_some() {
        Some("it has a skin")
    } else if mesh.dangly.is_some() {
        Some("it dangles")
    } else {
        None
    }
}

/// Which meshes of a model take the bake's light (`bends`).
pub fn lit_meshes(ir: &ModelIr) -> Vec<bool> {
    ir.meshes.iter().map(|mesh| bends(mesh).is_none()).collect()
}

/// How each mesh of a placeable whose model is `ir`, and on which the game
/// may play `clips`, moves: `None` for a mesh that keeps still, and for one
/// that moves the reason. A mesh moves when it has a skin, when it dangles,
/// or when a clip that is not one of `after_a_hit` shifts or turns its node
/// or a node above it.
pub fn motions(ir: &ModelIr, clips: &BTreeMap<String, Arc<AnimationIr>>) -> Vec<Option<String>> {
    let by_name: BTreeMap<String, usize> = (ir.nodes.iter().enumerate())
        .map(|(index, node)| (node.name.to_ascii_lowercase(), index))
        .collect();
    // The nodes a clip moves, each with the first clip that moves it.
    let mut moved: BTreeMap<usize, String> = BTreeMap::new();
    for (name, clip) in clips {
        if after_a_hit(name) {
            continue;
        }
        for track in &clip.tracks {
            let Some(&node) = by_name.get(&track.node.to_ascii_lowercase()) else {
                continue;
            };
            let shifted = (track.positions.iter())
                .any(|(_, offset)| offset.iter().map(|v| v * v).sum::<f32>().sqrt() > STILL_SHIFT);
            let rest = ir.nodes[node].rotation;
            let turned = (track.orientations.iter()).any(|(_, key)| {
                let cosine: f32 = (0..4).map(|k| key[k] * rest[k]).sum();
                cosine.abs() < STILL_TURN
            });
            if shifted || turned {
                moved
                    .entry(node)
                    .or_insert_with(|| format!("its clip {name} moves {}", track.node));
            }
        }
    }
    (ir.meshes.iter())
        .map(|mesh| {
            if let Some(why) = bends(mesh) {
                return Some(why.into());
            }
            let mut node = Some(mesh.node);
            let mut steps = 0;
            while let Some(at) = node.filter(|_| steps < ir.nodes.len()) {
                if let Some(why) = moved.get(&at) {
                    return Some(why.clone());
                }
                node = ir.nodes.get(at).and_then(|node| node.parent);
                steps += 1;
            }
            None
        })
        .collect()
}

/// The longest edge a triangle of a mesh with baked light keeps, in the
/// mesh's own metres (`subdivide`).
pub const LONGEST_EDGE: f32 = 0.5;
/// How many times `subdivide` halves the long edges, at the most. Six
/// rounds bring an edge of 32 metres down to half a metre.
const SPLIT_ROUNDS: usize = 6;

/// `mesh` with its long triangles split, so that no edge is longer than
/// `LONGEST_EDGE`. The baker lights the vertices of the split mesh, and
/// the game draws the same split mesh with that light.
///
/// The light is read at the vertices and blended across each triangle. On a
/// coarse mesh one vertex lights a whole fan of long triangles. The top of
/// 203tel's oval tables is such a fan around one middle vertex, and the
/// shade of the food on it darkened the whole top in streaks. After the
/// split the light is read every half metre or closer.
///
/// Each round halves every edge longer than `LONGEST_EDGE`. A triangle with
/// one long edge becomes two triangles, one with two long edges three, and
/// one with three long edges four. Whether an edge is split depends on its
/// length alone, so the two triangles that share an edge split it alike and
/// no crack opens between them. A new vertex takes the mean of its edge's
/// two ends: their place, their normal and their texture coordinates.
pub fn subdivide(mesh: &MeshIr) -> MeshIr {
    let mut out = mesh.clone();
    let count = out.positions.len();
    let has_normals = out.normals.len() == count;
    let has_uv1 = out.uv1.len() == count;
    let has_uv2 = out.uv2.len() == count;
    let long = |out: &MeshIr, a: u32, b: u32| {
        let (p, q) = (out.positions[a as usize], out.positions[b as usize]);
        (0..3).map(|k| (p[k] - q[k]) * (p[k] - q[k])).sum::<f32>() > LONGEST_EDGE * LONGEST_EDGE
    };
    for _ in 0..SPLIT_ROUNDS {
        let triangles: Vec<[u32; 3]> = (out.indices.as_chunks::<3>().0.iter())
            .filter(|triangle| triangle.iter().all(|&i| (i as usize) < out.positions.len()))
            .copied()
            .collect();
        if !triangles
            .iter()
            .any(|&[a, b, c]| long(&out, a, b) || long(&out, b, c) || long(&out, c, a))
        {
            break;
        }
        let mut middles: BTreeMap<(u32, u32), u32> = BTreeMap::new();
        let mut middle = |out: &mut MeshIr, a: u32, b: u32| -> u32 {
            *middles.entry((a.min(b), a.max(b))).or_insert_with(|| {
                let (a, b) = (a as usize, b as usize);
                let mean3 = |p: [f32; 3], q: [f32; 3]| -> [f32; 3] {
                    std::array::from_fn(|k| (p[k] + q[k]) / 2.0)
                };
                let mean2 = |p: [f32; 2], q: [f32; 2]| -> [f32; 2] {
                    std::array::from_fn(|k| (p[k] + q[k]) / 2.0)
                };
                out.positions
                    .push(mean3(out.positions[a], out.positions[b]));
                if has_normals {
                    let n = mean3(out.normals[a], out.normals[b]);
                    let length = (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt();
                    let n = if length > 1e-6 {
                        n.map(|v| v / length)
                    } else {
                        out.normals[a]
                    };
                    out.normals.push(n);
                }
                if has_uv1 {
                    out.uv1.push(mean2(out.uv1[a], out.uv1[b]));
                }
                if has_uv2 {
                    out.uv2.push(mean2(out.uv2[a], out.uv2[b]));
                }
                (out.positions.len() - 1) as u32
            })
        };
        let mut indices: Vec<u32> = Vec::with_capacity(out.indices.len() * 2);
        for v in triangles {
            let split: [bool; 3] = [0, 1, 2].map(|i| long(&out, v[i], v[(i + 1) % 3]));
            let m: [Option<u32>; 3] =
                [0, 1, 2].map(|i| split[i].then(|| middle(&mut out, v[i], v[(i + 1) % 3])));
            match split.iter().filter(|split| **split).count() {
                0 => indices.extend(v),
                1 => {
                    let i = (0..3).find(|&i| split[i]).unwrap_or(0);
                    let (a, b, c, mid) =
                        (v[i], v[(i + 1) % 3], v[(i + 2) % 3], m[i].unwrap_or(v[i]));
                    indices.extend([a, mid, c, mid, b, c]);
                }
                2 => {
                    // Edge j keeps whole. The corner between the two split
                    // edges is cut off, and the rest is cut in two.
                    let j = (0..3).find(|&j| !split[j]).unwrap_or(0);
                    let (a, b, c) = (v[j], v[(j + 1) % 3], v[(j + 2) % 3]);
                    let (bc, ca) = (m[(j + 1) % 3].unwrap_or(b), m[(j + 2) % 3].unwrap_or(c));
                    indices.extend([bc, c, ca, a, b, bc, a, bc, ca]);
                }
                _ => {
                    let [ab, bc, ca] = [0, 1, 2].map(|i| m[i].unwrap_or(v[i]));
                    indices.extend([v[0], ab, ca, ab, v[1], bc, ca, bc, v[2], ab, bc, ca]);
                }
            }
        }
        out.indices = indices;
    }
    out
}

/// One placeable's baked light: the light at each vertex of each of its
/// model's meshes, by the mesh's number, at the level the bake's lightmaps
/// store. A mesh the bake gave no light has none.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct BakedPlaceable {
    pub meshes: Vec<Option<Vec<[f32; 3]>>>,
}

/// The placeables' light in the bake's set `folder` for area `area`, by the
/// placeable's number among the area's instances. A line is kept only when
/// its model is `models` names for that number. An empty map when the set
/// has no `FILE` or holds another area's.
///
/// Each line of the file is a mesh: `mesh`, the instance's number, its
/// model's name, the mesh's number, how many vertices it has, and then the
/// light of each vertex as r g b. The first line that is not a comment is
/// `area` and the area's name.
pub fn read(
    folder: &Path,
    area: &str,
    models: &BTreeMap<u32, &str>,
) -> BTreeMap<u32, BakedPlaceable> {
    let mut found: BTreeMap<u32, BakedPlaceable> = BTreeMap::new();
    let Ok(text) = std::fs::read_to_string(folder.join(FILE)) else {
        return found;
    };
    let mut lines = text.lines().filter(|line| !line.starts_with('#'));
    let ours = lines
        .next()
        .and_then(|line| line.strip_prefix("area "))
        .is_some_and(|name| name.trim().eq_ignore_ascii_case(area));
    if !ours {
        return found;
    }
    for line in lines {
        let mut words = line.split_whitespace();
        if words.next() != Some("mesh") {
            continue;
        }
        let (Some(object), Some(model), Some(mesh), Some(count)) = (
            words.next().and_then(|word| word.parse::<u32>().ok()),
            words.next(),
            words.next().and_then(|word| word.parse::<usize>().ok()),
            words.next().and_then(|word| word.parse::<usize>().ok()),
        ) else {
            continue;
        };
        if !models
            .get(&object)
            .is_some_and(|name| name.eq_ignore_ascii_case(model))
        {
            continue;
        }
        let numbers: Vec<f32> = words.filter_map(|word| word.parse().ok()).collect();
        if numbers.len() != 3 * count {
            continue;
        }
        let light: Vec<[f32; 3]> = numbers.as_chunks::<3>().0.to_vec();
        let placeable = found.entry(object).or_default();
        if placeable.meshes.len() <= mesh {
            placeable.meshes.resize(mesh + 1, None);
        }
        placeable.meshes[mesh] = Some(light);
    }
    found
}
