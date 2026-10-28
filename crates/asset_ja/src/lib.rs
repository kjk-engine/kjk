//! Jedi Academy's Ghoul2 models as the runtime draws them, built on the
//! Odyssey model IR so the same scene, skinning and animator serve both.
//!
//! The mapping follows the game's own bone transform: a bone's final matrix is
//! its parent's final matrix times the frame's uncompressed bone (the root:
//! the model's root matrix times its own), and that final matrix skins
//! model-space vertices directly. So:
//!
//! - node 0 is the model root; GLA bone `b` is node `b + 1`, parented as in
//!   the skeleton, with an identity rest transform (all identity is the base
//!   pose, since GLM vertices are stored in model space);
//! - each skinned mesh binds bone slot `b` to node `b + 1` with an identity
//!   inverse bind;
//! - a clip's key for a bone is that frame's uncompressed bone, absolute
//!   (rest is identity, so the animator's `rest + scale × key` is the key).
//!
//! Units: Quake units are taken as inches (`UNIT`), applied to vertices and
//! bone translations alike, which scales the whole pose uniformly.

use std::collections::BTreeMap;

use asset_odyssey::model::{IDENTITY, MeshIr, ModelIr, NodeIr, SkinIr};
use fmt_ja::ghoul2::{Gla, Glm, SURFACE_BOLT, SURFACE_OFF};

pub mod angles;
mod bolt;
pub mod events;
pub mod fx;
pub mod g2;
mod playback;
pub mod rig;
mod rules;

pub use bolt::{Bolt, bolt};
pub use playback::{chain, playback};
pub use rules::{
    FLIPPING, Ignition, SPECIAL_TRAILS, blade_lengths, blend_ms, is_death, move_hold_ms,
    saber_anim_speed, saber_trail_ms,
};

pub use collision::UNIT;

/// How far a player's origin stands above the floor (24 units), the bottom of the player's box. `_humanoid` clips pose the
/// model with its floor there (`model_root` carries a 24-unit drop), so an
/// animated model stands on the floor this far above its origin.
pub const ORIGIN_HEIGHT: f32 = -collision::player::SOLE_Z * UNIT;

/// Surface name → shader (or texture) path, from a `.skin` file's
/// `surface,shader` lines; `*off` hides a surface.
pub fn parse_skin(bytes: &[u8]) -> BTreeMap<String, String> {
    String::from_utf8_lossy(bytes)
        .lines()
        .filter_map(|line| {
            let (surface, shader) = line.split_once(',')?;
            let surface = surface.trim().to_ascii_lowercase();
            let shader = fmt_ja::game_path(shader.trim());
            (!surface.is_empty()).then_some((surface, shader))
        })
        .collect()
}

/// Why a surface is not drawn.
fn hidden(name: &str, flags: u32, skin: Option<&String>) -> bool {
    flags & (SURFACE_BOLT | SURFACE_OFF) != 0
        || name.starts_with('*')
        || name.ends_with("_off")
        || skin.is_some_and(|shader| shader == "*off")
}

/// The model in its base pose, LOD 0. `texture` on each mesh is the
/// surface's shader path from the skin (else the GLM's own), for the caller
/// to resolve.
pub fn model(glm: &Glm, gla: &Gla, skin: &BTreeMap<String, String>) -> ModelIr {
    let mut nodes = vec![NodeIr {
        name: glm.name.clone(),
        parent: None,
        translation: [0.0; 3],
        rotation: [0.0, 0.0, 0.0, 1.0],
        world: IDENTITY,
        animated: false,
    }];
    for bone in &gla.bones {
        let parent = usize::try_from(bone.parent).map_or(0, |parent| parent + 1);
        nodes.push(NodeIr {
            name: bone.name.clone(),
            parent: Some(parent),
            translation: [0.0; 3],
            rotation: [0.0, 0.0, 0.0, 1.0],
            world: IDENTITY,
            animated: true,
        });
    }
    let slots = gla.bones.len();
    let mut meshes = Vec::new();
    let mut refusals = Vec::new();
    for (info, surface) in glm
        .surfaces
        .iter()
        .zip(glm.lods.first().into_iter().flatten())
    {
        let name = info.name.to_ascii_lowercase();
        let shader = skin.get(&name);
        if hidden(&name, info.flags, shader) {
            continue;
        }
        if surface.positions.is_empty() || surface.triangles.is_empty() {
            refusals.push((info.name.clone(), asset_odyssey::model::Refusal::NoFaces));
            continue;
        }
        let (vertex_joints, vertex_weights) = surface
            .weights
            .iter()
            .map(|weights| {
                let mut j = [0u16; 4];
                let mut w = [0f32; 4];
                for (k, &(reference, weight)) in weights.iter().take(4).enumerate() {
                    let bone = surface
                        .bone_references
                        .get(usize::from(reference))
                        .and_then(|&bone| usize::try_from(bone).ok())
                        .filter(|&bone| bone < slots);
                    if let Some(bone) = bone {
                        j[k] = bone as u16;
                        w[k] = weight;
                    }
                }
                (j, w)
            })
            .unzip();
        let texture = shader
            .cloned()
            .unwrap_or_else(|| fmt_ja::game_path(&info.shader));
        meshes.push(MeshIr {
            node: 0,
            positions: surface
                .positions
                .iter()
                .map(|p| p.map(|v| v * UNIT))
                .collect(),
            normals: surface.normals.clone(),
            uv1: surface.uvs.clone(),
            uv2: Vec::new(),
            indices: surface.triangles.iter().flatten().copied().collect(),
            texture: (!texture.is_empty()).then_some(texture),
            lightmap: None,
            transparency_hint: 0,
            skinned: true,
            self_illum: [0.0; 3],
            alpha: 1.0,
            ambient: [1.0; 3],
            diffuse: [1.0; 3],
            uv_scroll: None,
            skin: Some(SkinIr {
                joints: (1..=slots).collect(),
                inverse_binds: vec![IDENTITY; slots],
                rest_inverse_binds: vec![IDENTITY; slots],
                vertex_joints,
                vertex_weights,
            }),
        });
    }
    ModelIr {
        name: glm.name.clone(),
        supermodel: None,
        nodes,
        meshes,
        lights: Vec::new(),
        emitters: Vec::new(),
        animations: Vec::new(),
        animation_scale: 1.0,
        affected_by_fog: false,
        refusals,
    }
}

/// The skeleton a `*default` GLM uses: one bone at the origin.
pub fn default_gla(name: &str) -> Gla {
    let identity = [
        [1.0, 0.0, 0.0, 0.0],
        [0.0, 1.0, 0.0, 0.0],
        [0.0, 0.0, 1.0, 0.0],
    ];
    Gla {
        name: name.to_string(),
        scale: 1.0,
        bones: vec![fmt_ja::ghoul2::Bone {
            name: "model_root".into(),
            flags: 0,
            parent: -1,
            base_pose: identity,
            base_pose_inverse: identity,
            children: Vec::new(),
        }],
        frame_count: 0,
        frames: Vec::new(),
        pool: Vec::new(),
    }
}

/// A saber from a `.sab` block: its hilt model and blade sizes in Quake
/// units. A saber the file leaves unsaid has one blade, length 32,
/// radius 3; a length below 4 becomes 4; `saberLength` sets every blade, and
/// `saberLength2`… then set their own (they must come after it).
#[derive(Debug, Clone, PartialEq)]
pub struct Saber {
    pub name: String,
    pub model: String,
    /// Each blade's length (`numBlades`, 1 to 8).
    pub lengths: Vec<f32>,
    pub radius: f32,
    /// What it sounds lighting, lit (a loop) and going out
    /// (`soundOn`, `soundLoop`, `soundOff`).
    pub sounds: [String; 3],
}

/// A saber's sounds unless its file names others.
pub const SABER_SOUNDS: [&str; 3] = [
    "sound/weapons/saber/enemy_saber_on.wav",
    "sound/weapons/saber/saberhum3.wav",
    "sound/weapons/saber/enemy_saber_off.wav",
];

pub fn saber(bytes: &[u8], name: &str) -> Option<Saber> {
    let root = fmt_ja::script::parse(bytes).ok()?;
    let block = root
        .children
        .iter()
        .find(|block| block.name.len() == 1 && block.name[0].eq_ignore_ascii_case(name))?;
    let value = |key: &str| {
        block.lines.iter().find_map(|line| {
            (line.first()?.eq_ignore_ascii_case(key))
                .then(|| line.get(1).cloned())
                .flatten()
        })
    };
    let length = |key: &str| {
        value(key)
            .and_then(|v| v.parse::<f32>().ok())
            .map(|v| v.max(4.0))
    };
    let count = value("numBlades")
        .and_then(|v| v.parse::<usize>().ok())
        .map_or(1, |n| n.clamp(1, 8));
    let first = length("saberLength").unwrap_or(32.0);
    let lengths = (1..=count)
        .map(|i| {
            if i == 1 {
                first
            } else {
                length(&format!("saberLength{i}")).unwrap_or(first)
            }
        })
        .collect();
    Some(Saber {
        name: name.to_ascii_lowercase(),
        model: fmt_ja::game_path(&value("saberModel")?),
        lengths,
        radius: value("saberRadius")
            .and_then(|v| v.parse().ok())
            .unwrap_or(3.0),
        sounds: [("soundOn", 0), ("soundLoop", 1), ("soundOff", 2)].map(|(key, at)| {
            value(key).map_or(SABER_SOUNDS[at].to_string(), |name| {
                fmt_ja::game_path(&name)
            })
        }),
    })
}
