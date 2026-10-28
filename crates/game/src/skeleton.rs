//! The sim's skeleton data (`sim::skeleton::SkeletonData`) from the JA
//! install: the `_humanoid` bones on the chains the hand tags of
//! `stage::JA_BODY` and the `Motion` bone hang from, their frames as the GLA stores them (the
//! pool cut down to the entries those bones use), the `*r_hand` and
//! `*l_hand` tags, and each named saber's blade tags and lengths.

use std::collections::BTreeMap;

use fmt_ja::ghoul2::{Glm, Surface};
use sim::skeleton::{
    Bone, MOTION_BONE, Saber, SaberBlade, SkeletonData, SkeletonParts, Tag, TagVertex,
};

use crate::stage::JA_BODY;
use crate::viewer_ja::{self, Files};

/// The hand tags, in `sim::skeleton::Hand`'s order.
const HAND_TAGS: [&str; 2] = ["*r_hand", "*l_hand"];

pub fn skeleton_data(files: &Files, sabers: &[&str]) -> Result<SkeletonData, String> {
    let (glm, gla, gla_path) = viewer_ja::glm_and_gla(files, JA_BODY)?;
    let parent = |bone: usize| {
        gla.bones
            .get(bone)
            .and_then(|b| usize::try_from(b.parent).ok())
    };
    let [right, left] = HAND_TAGS.map(|name| {
        surface(&glm, name).and_then(|surface| hand_tag(name, surface, gla.bones.len()))
    });
    let hands = [right?, left?];

    // The bones the tags hang on and their ancestors (by depth, so parents
    // come first), and each one's place among them.
    let ancestry = |bone: usize| {
        let line: Vec<usize> = std::iter::successors(Some(bone), |&b| parent(b))
            .take(gla.bones.len() + 1)
            .collect();
        if line.len() > gla.bones.len() {
            return Err(format!("{gla_path}: bone {bone}'s parents loop"));
        }
        Ok(line)
    };
    let motion = gla
        .bones
        .iter()
        .position(|bone| bone.name.eq_ignore_ascii_case(MOTION_BONE));
    let mut depths = BTreeMap::new();
    let ends = hands
        .iter()
        .flat_map(|tag| &tag.vertices)
        .flat_map(|vertex| vertex.influences.iter().map(|&(bone, _)| usize::from(bone)))
        .chain(motion);
    for end in ends {
        let line = ancestry(end)?;
        for (up, &bone) in line.iter().enumerate() {
            depths.insert(bone, line.len() - up);
        }
    }
    let mut order: Vec<usize> = depths.keys().copied().collect();
    order.sort_by_key(|bone| (depths[bone], *bone));
    let chain: BTreeMap<usize, u16> = order
        .iter()
        .enumerate()
        .map(|(index, &bone)| (bone, index as u16))
        .collect();
    let bones = order
        .iter()
        .map(|&bone| {
            let info = &gla.bones[bone];
            Bone {
                name: info.name.clone(),
                parent: parent(bone).and_then(|p| chain.get(&p).copied()),
                base: info.base_pose,
                base_inverse: info.base_pose_inverse,
            }
        })
        .collect();
    let hands = hands.map(|tag| Tag {
        vertices: tag.vertices.map(|vertex| TagVertex {
            influences: vertex
                .influences
                .iter()
                .map(|&(bone, weight)| (chain[&usize::from(bone)], weight))
                .collect(),
            ..vertex
        }),
    });

    let mut pool = Vec::new();
    let mut used: BTreeMap<u32, u32> = BTreeMap::new();
    let mut keys = Vec::with_capacity(gla.frame_count * order.len());
    for frame in 0..gla.frame_count {
        for &bone in &order {
            let index = gla.frames[frame * gla.bones.len() + bone];
            let key = *used.entry(index).or_insert_with(|| {
                pool.push(gla.pool[index as usize]);
                (pool.len() - 1) as u32
            });
            keys.push(key);
        }
    }

    let sabers = sabers
        .iter()
        .map(|name| saber(files, name))
        .collect::<Result<_, _>>()?;
    SkeletonData::new(SkeletonParts {
        bones,
        frame_count: u32::try_from(gla.frame_count).map_err(|e| e.to_string())?,
        keys,
        pool,
        hands,
        sabers,
        motion: motion.and_then(|bone| chain.get(&bone).copied()),
    })
    .map_err(|error| format!("{JA_BODY}: {error}"))
}

fn surface<'g>(glm: &'g Glm, name: &str) -> Result<&'g Surface, String> {
    let index = glm
        .surfaces
        .iter()
        .position(|surface| surface.name.eq_ignore_ascii_case(name))
        .ok_or(format!("{name}: no such tag"))?;
    glm.lods
        .first()
        .and_then(|lod| lod.get(index))
        .ok_or(format!("{name}: no LOD 0 surface"))
}

/// A tag's first three vertices, their influences as GLA bones and the
/// file's 10-bit weights (the parser's `w / 1023` turned back; the last
/// weight is what the others leave, so its own value is not needed).
fn hand_tag(name: &str, surface: &Surface, bones: usize) -> Result<Tag, String> {
    let vertex = |i: usize| -> Result<TagVertex, String> {
        let (Some(&position), Some(weights)) = (surface.positions.get(i), surface.weights.get(i))
        else {
            return Err(format!("{name}: fewer than three vertices"));
        };
        let influences = weights
            .iter()
            .enumerate()
            .map(|(k, &(reference, weight))| {
                let bone = surface
                    .bone_references
                    .get(usize::from(reference))
                    .and_then(|&bone| usize::try_from(bone).ok())
                    .filter(|&bone| bone < bones)
                    .ok_or(format!("{name}: vertex {i} names no bone"))?;
                let raw = (weight * 1023.0).round().clamp(0.0, 1023.0) as u16;
                if k + 1 < weights.len() && f32::from(raw) / 1023.0 != weight {
                    return Err(format!(
                        "{name}: vertex {i} weight {weight} is not n / 1023"
                    ));
                }
                Ok((bone as u16, raw))
            })
            .collect::<Result<Vec<_>, String>>()?;
        Ok(TagVertex {
            position,
            influences,
        })
    };
    Ok(Tag {
        vertices: [vertex(0)?, vertex(1)?, vertex(2)?],
    })
}

/// A saber's blades: each `*blade<n>` tag's first three vertices, which
/// must hang on the hilt's one bone, and the blade's length.
fn saber(files: &Files, name: &str) -> Result<Saber, String> {
    let (saber, hilt) = viewer_ja::saber_and_hilt(files, name)?;
    if !hilt.animation.eq_ignore_ascii_case("*default") {
        return Err(format!(
            "{}: a hilt with its own skeleton ({}) is not supported",
            saber.model, hilt.animation
        ));
    }
    let blades = saber
        .lengths
        .iter()
        .enumerate()
        .map(|(i, &length)| {
            let tag = format!("*blade{}", i + 1);
            let surface = surface(&hilt, &tag)?;
            let on_root = |weights: &[(u8, f32)]| match weights {
                [(reference, _)] => {
                    surface.bone_references.get(usize::from(*reference)) == Some(&0)
                }
                _ => false,
            };
            let point = |v: usize| -> Result<[f32; 3], String> {
                match (surface.positions.get(v), surface.weights.get(v)) {
                    (Some(&p), Some(weights)) if on_root(weights) => Ok(p),
                    _ => Err(format!(
                        "{}: {tag} vertex {v} is not on the hilt",
                        saber.model
                    )),
                }
            };
            Ok(SaberBlade {
                tag: [point(0)?, point(1)?, point(2)?],
                length,
            })
        })
        .collect::<Result<_, String>>()?;
    Ok(Saber {
        name: saber.name,
        blades,
    })
}
