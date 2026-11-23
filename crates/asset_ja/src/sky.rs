//! A Jedi Academy level's sky, as a box round the viewer.
//!
//! A level's sky surfaces seal its open space, and the game draws the sky
//! on them as if from very far away: six pictures on the faces of a box
//! round the viewer (a sky shader's `skyParms`, its first word naming the
//! pictures). The box moves with the viewer, so the sky never comes nearer.
//!
//! Here the box is a model of its own ([`model`]), named with [`SUFFIX`].
//! The scene keeps such a model round the camera (`game::sky_box`). The
//! box is larger than the level, so all of the level is drawn in front of
//! it, and the sky shows only where the level is open.
//!
//! Each picture faces the viewer upright and the right way round. The
//! pictures are named by suffix:
//!
//! | suffix | faces | its right runs along | its top runs along |
//! |---|---|---|---|
//! | `rt` | +X | -Y | +Z |
//! | `lf` | -X | +Y | +Z |
//! | `bk` | +Y | +X | +Z |
//! | `ft` | -Y | -X | +Z |
//! | `up` | +Z | -Y | -X |
//! | `dn` | -Z | -Y | +X |

use asset_odyssey::model::{IDENTITY, MeshIr, ModelIr, NodeIr};

/// The end of a sky model's name.
pub const SUFFIX: &str = "#sky";

/// The six pictures' suffixes, in the order of [`FACES`].
pub const SIDES: [&str; 6] = ["rt", "lf", "bk", "ft", "up", "dn"];

/// Each side's direction from the viewer, the direction its picture's
/// right runs along, and the direction its picture's top runs along.
const FACES: [([f32; 3], [f32; 3], [f32; 3]); 6] = [
    ([1.0, 0.0, 0.0], [0.0, -1.0, 0.0], [0.0, 0.0, 1.0]),
    ([-1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]),
    ([0.0, 1.0, 0.0], [1.0, 0.0, 0.0], [0.0, 0.0, 1.0]),
    ([0.0, -1.0, 0.0], [-1.0, 0.0, 0.0], [0.0, 0.0, 1.0]),
    ([0.0, 0.0, 1.0], [0.0, -1.0, 0.0], [-1.0, 0.0, 0.0]),
    ([0.0, 0.0, -1.0], [0.0, -1.0, 0.0], [1.0, 0.0, 0.0]),
];

/// The sky's box: a model named `level` and [`SUFFIX`], whose six sides
/// are `half` metres from its middle, each drawn with the texture `pictures`
/// names for it (in the order of [`SIDES`]). The sides face in, are drawn
/// at full brightness, cast no shadow and take no fog.
pub fn model(level: &str, pictures: [String; 6], half: f32) -> ModelIr {
    let name = format!("{level}{SUFFIX}");
    let meshes = (FACES.iter().zip(pictures))
        .map(|(&(facing, right, top), picture)| {
            let at = |r: f32, t: f32| -> [f32; 3] {
                std::array::from_fn(|axis| (facing[axis] + r * right[axis] + t * top[axis]) * half)
            };
            // Seen from the middle, these wind counter-clockwise: the
            // scene's front.
            let positions = vec![at(-1.0, -1.0), at(1.0, -1.0), at(1.0, 1.0), at(-1.0, 1.0)];
            let inward = facing.map(|part| -part);
            MeshIr {
                node: 0,
                positions,
                normals: vec![inward; 4],
                uv1: vec![[0.0, 1.0], [1.0, 1.0], [1.0, 0.0], [0.0, 0.0]],
                uv2: Vec::new(),
                indices: vec![0, 1, 2, 0, 2, 3],
                texture: Some(picture),
                lightmap: None,
                transparency_hint: 0,
                skinned: false,
                shadow: false,
                self_illum: [1.0; 3],
                alpha: 1.0,
                ambient: [1.0; 3],
                diffuse: [1.0; 3],
                uv_scroll: None,
                skin: None,
                dangly: None,
            }
        })
        .collect();
    ModelIr {
        name: name.clone(),
        supermodel: None,
        nodes: vec![NodeIr {
            name,
            parent: None,
            translation: [0.0; 3],
            rotation: [0.0, 0.0, 0.0, 1.0],
            world: IDENTITY,
            animated: false,
        }],
        meshes,
        lights: Vec::new(),
        emitters: Vec::new(),
        animations: Vec::new(),
        animation_scale: 1.0,
        affected_by_fog: false,
        refusals: Vec::new(),
        folded: Vec::new(),
    }
}

/// How far the sky's sides must be from the viewer, in metres, for all of
/// the level `bsp` to be drawn in front of them wherever the viewer is in
/// it: the length of the world's bounds from corner to corner.
pub fn reach(bsp: &fmt_ja::bsp::Bsp) -> f32 {
    let Some(world) = bsp.models.first() else {
        return 100.0;
    };
    let length: f32 = (0..3)
        .map(|axis| (world.max[axis] - world.min[axis]).powi(2))
        .sum::<f32>()
        .sqrt();
    (length * collision::UNIT).max(100.0)
}
