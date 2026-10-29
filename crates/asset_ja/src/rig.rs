//! Jedi Academy's humanoid skeleton against KOTOR's: JA clips moved onto a
//! KOTOR skeleton (`retarget`), a JA hilt held in a KOTOR hand
//! (`hilt_in_hand`), and KOTOR meshes skinned to JA's skeleton (`rebind`).
//! These are the pieces behind the three skeleton probes of I10.
//!
//! Spaces. JA model space faces −Y with the model's left at +X (the
//! `_humanoid` base pose: the face bones sit at −Y, `lhumerus` at +X);
//! KOTOR's faces +Y with its right at +X (Carth's eyes at +Y, `rhand` at
//! +X). Both keep Z up, so JA to KOTOR is a half turn about Z (`TURN`).
//!
//! A JA bone's final matrix (see the crate docs) is the model-space change
//! from the base pose: its frame at any moment is final × base pose. So the
//! final's rotation `D` is how far the bone has turned, in model space,
//! since the base pose.
//!
//! A mapped KOTOR node takes the same model-space turn from a rest that is
//! first lined up with its JA bone:
//!
//! `global(t) = TURN · D(t) · TURN⁻¹ · align · rest_global`
//!
//! where `align` is the shortest rotation taking the KOTOR bone's direction
//! (towards the nodes named as its aim) onto the JA bone's base-pose
//! direction, and identity for nodes without an aim (the spine, neck, head
//! and feet, which both skeletons hold upright or flat). Unmapped nodes keep
//! their rest rotation under their parent. A node's key is
//! `parent_global⁻¹ · global`. Only the root travels: JA's pelvis movement,
//! scaled by the ratio of pelvis heights.

use std::collections::BTreeMap;

use asset_odyssey::anim::{AnimationIr, NodeTrack};
use asset_odyssey::model::{IDENTITY, Matrix, MeshIr, ModelIr, SkinIr};
use fmt_ja::ghoul2::Gla;
use glam::{Mat3, Quat, Vec3};

use crate::{Bolt, UNIT};

/// JA to KOTOR model space: a half turn about Z.
const TURN: Quat = Quat::from_xyzw(0.0, 0.0, 1.0, 0.0);

/// A JA bone and the KOTOR node that follows it, with the points each
/// bone aims at (averaged) to line the two up.
struct Pair {
    ja: &'static str,
    kotor: &'static str,
    aim: Option<(&'static [&'static str], &'static [&'static str])>,
}

const fn pair(ja: &'static str, kotor: &'static str) -> Pair {
    Pair {
        ja,
        kotor,
        aim: None,
    }
}

const fn aimed(
    ja: &'static str,
    kotor: &'static str,
    ja_aim: &'static [&'static str],
    kotor_aim: &'static [&'static str],
) -> Pair {
    Pair {
        ja,
        kotor,
        aim: Some((ja_aim, kotor_aim)),
    }
}

/// `_humanoid` bones to the KOTOR humanoid skeleton (`p_carthbb` and the
/// player bodies share it). JA's twist helpers (`*X`), fingers, face and
/// tails have no KOTOR node: JA's hands are fists in the base pose and its
/// finger bones barely move, so KOTOR's fingers take a KOTOR hand pose
/// instead (`Options::hold`), and the weapon hand turns to hold the hilt
/// (`Grip`). KOTOR's `rbicepl_g` and `hturn_g` have no JA bone and ride on
/// their parents. The neck maps to `neck_g`, not
/// `necklwr_g`: the head model animates from `neck_g` down, so a turn on
/// `necklwr_g` would move the body's copy but not the head's.
const HUMANOID: &[Pair] = &[
    pair("pelvis", "pelvis_g"),
    pair("upper_lumbar", "torso_g"),
    pair("thoracic", "torsoupr_g"),
    pair("cervical", "neck_g"),
    pair("cranium", "head_g"),
    aimed("rclavical", "rcollar_g", &["rhumerus"], &["rbicep_g"]),
    aimed("rhumerus", "rbicep_g", &["rradius"], &["rforearm_g"]),
    aimed("rradius", "rforearm_g", &["rhand"], &["rhand_g"]),
    aimed(
        "rhand",
        "rhand_g",
        &["r_d2_j1", "r_d4_j1"],
        &["rafngrb_g", "rdfngrb_g"],
    ),
    aimed("lclavical", "lcollar_g", &["lhumerus"], &["lbicep_g"]),
    aimed("lhumerus", "lbicep_g", &["lradius"], &["lforearm_g"]),
    aimed("lradius", "lforearm_g", &["lhand"], &["lhand_g"]),
    aimed(
        "lhand",
        "lhand_g",
        &["l_d2_j1", "l_d4_j1"],
        &["lafngrb_g", "ldfngrb_g"],
    ),
    aimed("rfemurYZ", "rthigh_g", &["rtibia"], &["rshin_g"]),
    aimed("rtibia", "rshin_g", &["rtalus"], &["rfoot_g"]),
    pair("rtalus", "rfoot_g"),
    aimed("lfemurYZ", "lthigh_g", &["ltibia"], &["lshin_g"]),
    aimed("ltibia", "lshin_g", &["ltalus"], &["lfoot_g"]),
    pair("ltalus", "lfoot_g"),
];

#[derive(Debug, Clone, Copy)]
struct Frame {
    rotation: Quat,
    position: Vec3,
}

impl Frame {
    const IDENTITY: Frame = Frame {
        rotation: Quat::IDENTITY,
        position: Vec3::ZERO,
    };

    fn then(self, local: Frame) -> Frame {
        Frame {
            rotation: (self.rotation * local.rotation).normalize(),
            position: self.position + self.rotation * local.position,
        }
    }

    fn inverse(self) -> Frame {
        let rotation = self.rotation.inverse();
        Frame {
            rotation,
            position: -(rotation * self.position),
        }
    }

    fn point(self, p: Vec3) -> Vec3 {
        self.position + self.rotation * p
    }

    fn of(m: &Matrix) -> Frame {
        let column = |c: usize| Vec3::new(m[0][c], m[1][c], m[2][c]).normalize_or_zero();
        Frame {
            rotation: Quat::from_mat3(&Mat3::from_cols(column(0), column(1), column(2)))
                .normalize(),
            position: Vec3::new(m[0][3], m[1][3], m[2][3]),
        }
    }

    fn matrix(self) -> Matrix {
        let r = Mat3::from_quat(self.rotation);
        let mut m = IDENTITY;
        for (row, cells) in m.iter_mut().take(3).enumerate() {
            for (column, cell) in cells.iter_mut().take(3).enumerate() {
                *cell = r.col(column)[row];
            }
            cells[3] = self.position[row];
        }
        m
    }
}

/// JA's skeleton: base-pose positions (metres) and an order with parents
/// first (a GLA can list a bone before its parent).
struct Ja<'a> {
    gla: &'a Gla,
    order: Vec<usize>,
}

impl<'a> Ja<'a> {
    fn new(gla: &'a Gla) -> Ja<'a> {
        let mut order = Vec::with_capacity(gla.bones.len());
        let mut placed = vec![false; gla.bones.len()];
        while order.len() < gla.bones.len() {
            let before = order.len();
            for (bone, info) in gla.bones.iter().enumerate() {
                let ready = usize::try_from(info.parent)
                    .map_or(true, |parent| parent >= placed.len() || placed[parent]);
                if !placed[bone] && ready {
                    placed[bone] = true;
                    order.push(bone);
                }
            }
            if order.len() == before {
                // A cycle: take the rest as listed.
                order.extend((0..placed.len()).filter(|&bone| !placed[bone]));
                break;
            }
        }
        Ja { gla, order }
    }

    fn bone(&self, name: &str) -> Option<usize> {
        self.gla
            .bones
            .iter()
            .position(|bone| bone.name.eq_ignore_ascii_case(name))
    }

    fn base(&self, bone: usize) -> Vec3 {
        let m = &self.gla.bones[bone].base_pose;
        Vec3::new(m[0][3], m[1][3], m[2][3]) * UNIT
    }

    fn base_mean(&self, names: &[&str]) -> Option<Vec3> {
        let points: Option<Vec<Vec3>> = names
            .iter()
            .map(|name| self.bone(name).map(|b| self.base(b)))
            .collect();
        let points = points?;
        Some(points.iter().sum::<Vec3>() / points.len().max(1) as f32)
    }

    /// Every bone's final for bone locals (rotation, position), one per
    /// bone in bone order.
    fn finals_of(&self, locals: &[(Quat, Vec3)]) -> Vec<Frame> {
        let mut finals = vec![Frame::IDENTITY; self.gla.bones.len()];
        for &bone in &self.order {
            let local = locals
                .get(bone)
                .map_or(Frame::IDENTITY, |&(rotation, position)| Frame {
                    rotation: rotation.normalize(),
                    position,
                });
            let parent = usize::try_from(self.gla.bones[bone].parent)
                .ok()
                .filter(|&parent| parent < finals.len());
            finals[bone] = match parent {
                Some(parent) => finals[parent].then(local),
                None => local,
            };
        }
        finals
    }

    /// Every bone's final at key `key` of a clip made by `crate::combo`
    /// (one track per bone, in bone order).
    fn finals(&self, clip: &AnimationIr, key: usize) -> Vec<Frame> {
        let mut finals = vec![Frame::IDENTITY; self.gla.bones.len()];
        for &bone in &self.order {
            let local = clip.tracks.get(bone).map_or(Frame::IDENTITY, |track| {
                let [x, y, z, w] = track
                    .orientations
                    .get(key)
                    .map_or([0.0, 0.0, 0.0, 1.0], |k| k.1);
                Frame {
                    rotation: Quat::from_xyzw(x, y, z, w).normalize(),
                    position: Vec3::from_array(track.positions.get(key).map_or([0.0; 3], |k| k.1)),
                }
            });
            let parent = usize::try_from(self.gla.bones[bone].parent)
                .ok()
                .filter(|&parent| parent < finals.len());
            finals[bone] = match parent {
                Some(parent) => finals[parent].then(local),
                None => local,
            };
        }
        finals
    }
}

/// KOTOR nodes that point at their JA bone's aim instead of turning with
/// it, and the JA bone whose turn gives their roll: the thighs aim at JA's
/// knee and roll with the knee's hinge (the shin). JA's `*femurYZ` bones
/// carry twist about their own axis that the knee and foot do not show
/// (the medium style's attack and transition end poses differ by 160° of
/// thigh twist but 40° at the foot); turning with them would wring the
/// KOTOR thigh mid-blend.
/// JA's feet and KOTOR's, for keeping the supporting foot on the floor.
const FEET: &[(&str, &str)] = &[("ltalus", "lfoot_g"), ("rtalus", "rfoot_g")];

const HINGED: &[(&str, &str)] = &[("rthigh_g", "rtibia"), ("lthigh_g", "ltibia")];

/// How a hinged node is posed: the JA bones it aims at, the JA bone that
/// carries its hinge, and its aim and hinge in JA's base pose (KOTOR space).
struct Hinge {
    aim: Vec<usize>,
    hinge: usize,
    bind: Quat,
    bind_hinge: Vec3,
}

/// The rotation whose columns are the aim, the hinge made perpendicular to
/// it, and their cross product.
fn aim_frame(aim: Vec3, hinge: Vec3) -> Quat {
    let hinge = (hinge - aim * hinge.dot(aim)).normalize_or(Vec3::X);
    Quat::from_mat3(&Mat3::from_cols(aim, hinge, aim.cross(hinge))).normalize()
}

/// A KOTOR skeleton's rest pose and how each mapped node lines up with JA.
struct Kotor<'a> {
    ir: &'a ModelIr,
    rest: Vec<Frame>,
    local: Vec<Frame>,
    /// Node → (JA bone, align).
    mapped: BTreeMap<usize, (usize, Quat)>,
    hinged: BTreeMap<usize, Hinge>,
}

impl<'a> Kotor<'a> {
    fn new(ir: &'a ModelIr, ja: &Ja) -> Kotor<'a> {
        let rest: Vec<Frame> = ir.nodes.iter().map(|node| Frame::of(&node.world)).collect();
        let local = ir
            .nodes
            .iter()
            .map(|node| {
                let [x, y, z, w] = node.rotation;
                Frame {
                    rotation: Quat::from_xyzw(x, y, z, w).normalize(),
                    position: Vec3::from_array(node.translation),
                }
            })
            .collect();
        let node = |name: &str| {
            ir.nodes
                .iter()
                .position(|node| node.name.eq_ignore_ascii_case(name))
        };
        let mut mapped = BTreeMap::new();
        for pair in HUMANOID {
            let (Some(k), Some(b)) = (node(pair.kotor), ja.bone(pair.ja)) else {
                continue;
            };
            let align = pair
                .aim
                .and_then(|(ja_aim, kotor_aim)| {
                    let kotor_points: Option<Vec<Vec3>> = kotor_aim
                        .iter()
                        .map(|name| node(name).map(|n| rest[n].position))
                        .collect();
                    let kotor_points = kotor_points?;
                    let target = kotor_points.iter().sum::<Vec3>() / kotor_points.len() as f32;
                    let from = (target - rest[k].position).try_normalize()?;
                    let to = (TURN * (ja.base_mean(ja_aim)? - ja.base(b))).try_normalize()?;
                    Some(Quat::from_rotation_arc(from, to))
                })
                .unwrap_or(Quat::IDENTITY);
            mapped.insert(k, (b, align));
        }
        let mut hinged = BTreeMap::new();
        for &(kotor, hinge) in HINGED {
            let found = HUMANOID
                .iter()
                .find(|p| p.kotor.eq_ignore_ascii_case(kotor));
            let (Some(pair), Some(k), Some(hinge)) = (found, node(kotor), ja.bone(hinge)) else {
                continue;
            };
            let (Some((ja_aim, _)), Some(b)) = (pair.aim, ja.bone(pair.ja)) else {
                continue;
            };
            let aim: Option<Vec<usize>> = ja_aim.iter().map(|name| ja.bone(name)).collect();
            let (Some(aim), Some(target)) = (aim, ja.base_mean(ja_aim)) else {
                continue;
            };
            let Some(direction) = (TURN * (target - ja.base(b))).try_normalize() else {
                continue;
            };
            // The knee's hinge in the base pose: sideways (JA's +X, KOTOR's
            // −X after the half turn), square to the thigh.
            let bind_hinge = TURN * Vec3::X;
            hinged.insert(
                k,
                Hinge {
                    aim,
                    hinge,
                    bind: aim_frame(direction, bind_hinge),
                    bind_hinge,
                },
            );
        }
        Kotor {
            ir,
            rest,
            local,
            mapped,
            hinged,
        }
    }

    fn node(&self, name: &str) -> Option<usize> {
        self.ir
            .nodes
            .iter()
            .position(|node| node.name.eq_ignore_ascii_case(name))
    }

    /// Globals for a pose: mapped nodes turned as their JA bone's final,
    /// the rest under their parents. Nodes are listed parents first.
    #[allow(clippy::too_many_arguments)]
    fn globals(
        &self,
        ja: &Ja,
        finals: &[Frame],
        root: Option<usize>,
        root_offset: Vec3,
        grips: &[&Grip],
        hold: &BTreeMap<usize, Quat>,
        turned: &BTreeMap<usize, Quat>,
    ) -> Vec<Frame> {
        let mut globals: Vec<Frame> = Vec::with_capacity(self.rest.len());
        for (index, node) in self.ir.nodes.iter().enumerate() {
            let parent = node.parent.map_or(Frame::IDENTITY, |p| globals[p]);
            let mut local = self.local[index];
            if let Some(&rotation) = hold.get(&index) {
                local.rotation = rotation;
            }
            let mut global = parent.then(local);
            if let Some(&(bone, align)) = self.mapped.get(&index) {
                let turn = match self.hinged.get(&index) {
                    // Aimed at the JA aim point, rolled with the hinge bone.
                    Some(hinge) => {
                        let from = finals[bone].point(ja.base(bone));
                        let to = hinge
                            .aim
                            .iter()
                            .map(|&a| finals[a].point(ja.base(a)))
                            .sum::<Vec3>()
                            / hinge.aim.len().max(1) as f32;
                        let hinge_turn = TURN * finals[hinge.hinge].rotation * TURN.inverse();
                        match (TURN * (to - from)).try_normalize() {
                            Some(direction) => {
                                aim_frame(direction, hinge_turn * hinge.bind_hinge)
                                    * hinge.bind.inverse()
                            }
                            None => TURN * finals[bone].rotation * TURN.inverse(),
                        }
                    }
                    None => TURN * finals[bone].rotation * TURN.inverse(),
                };
                global.rotation = (turn * align * self.rest[index].rotation).normalize();
            }
            if let Some(grip) = grips.iter().find(|g| g.hand == index) {
                // Turned so the hilt in its weapon node points as JA's does.
                let hilt = TURN * finals[grip.bone].rotation * grip.bolt;
                global.rotation =
                    (hilt * grip.hilt.inverse() * self.local[grip.weapon].rotation.inverse())
                        .normalize();
            }
            if let Some(&rotation) = turned.get(&index) {
                global.rotation = rotation;
            }
            if Some(index) == root {
                global.position += root_offset;
            }
            globals.push(global);
        }
        globals
    }

    /// The node that carries root travel: the parent of the node that
    /// follows JA's pelvis.
    fn root(&self, ja: &Ja) -> Option<usize> {
        let pelvis = ja.bone("pelvis")?;
        let (&node, _) = self.mapped.iter().find(|(_, (bone, _))| *bone == pelvis)?;
        self.ir.nodes[node].parent
    }
}

/// JA's pelvis height over KOTOR's: root travel and grip offsets scale by it.
fn pelvis_scale(ja: &Ja, kotor: &Kotor) -> f32 {
    let ja_pelvis = ja.bone("pelvis").map(|b| ja.base(b).z);
    let kotor_pelvis = kotor.node("pelvis_g").map(|n| kotor.rest[n].position.z);
    match (ja_pelvis, kotor_pelvis) {
        (Some(ja), Some(kotor)) if ja > 0.0 && kotor > 0.0 => kotor / ja,
        _ => 1.0,
    }
}

/// Whether a KOTOR skeleton has every node the humanoid mapping moves
/// below the neck (a body whose head is a model of its own has no neck or
/// head): the bodies JA's animations can drive whole, the player bodies and
/// the full-body humanoids (the Sith soldiers) alike.
pub fn is_humanoid(kotor: &ModelIr) -> bool {
    let has = |name: &str| {
        kotor
            .nodes
            .iter()
            .any(|node| node.name.eq_ignore_ascii_case(name))
    };
    HUMANOID
        .iter()
        .filter(|pair| !matches!(pair.kotor, "neck_g" | "head_g"))
        .all(|pair| has(pair.kotor))
}

/// Which KOTOR nodes follow which JA bones, with each node's alignment in
/// degrees (for the journal).
pub fn mapping(kotor: &ModelIr, gla: &Gla) -> Vec<(String, String, f32)> {
    let ja = Ja::new(gla);
    let k = Kotor::new(kotor, &ja);
    k.mapped
        .iter()
        .map(|(&node, &(bone, align))| {
            (
                gla.bones[bone].name.clone(),
                kotor.nodes[node].name.clone(),
                align.angle_between(Quat::IDENTITY).to_degrees(),
            )
        })
        .collect()
}

/// A JA clip (`crate::combo`, one track per bone) as a clip of the KOTOR
/// model: an orientation track for each mapped node and a position track on
/// the root node, at the JA clip's key times. The root moves by the JA
/// pelvis's travel from its base pose (the clip's floor put at the base
/// pose's, `crate::ORIGIN_HEIGHT`), scaled, then up or down so the foot JA
/// holds lower sits at KOTOR's ankle height plus JA's lift of it (both feet
/// weighed by how low they are, so the correction is smooth).
pub fn retarget(kotor: &ModelIr, gla: &Gla, clip: &AnimationIr) -> AnimationIr {
    retarget_with(kotor, gla, clip, &Options::default())
}

/// What else a retargeted clip carries.
#[derive(Default)]
pub struct Options<'a> {
    /// Travel over the ground by the clip's seconds, in KOTOR model space
    /// (metres), for clips played on the move.
    pub ground: Option<&'a dyn Fn(f32) -> Vec3>,
    /// Hands holding JA hilts (one per hand).
    pub grips: Vec<&'a Grip>,
    /// Local rotations held on nodes (x, y, z, w): a KOTOR hand pose for the
    /// fingers.
    pub hold: Vec<(String, [f32; 4])>,
    /// How high the JA body's soles are in the base pose (metres; Kyle's
    /// stand 3.1 units up), so a foot's lift is measured from standing.
    /// KOTOR bodies stand on their origin.
    pub ja_sole: f32,
}

/// `retarget` with `Options`.
pub fn retarget_with(
    kotor: &ModelIr,
    gla: &Gla,
    clip: &AnimationIr,
    options: &Options,
) -> AnimationIr {
    let retargeter = Retargeter::new(kotor, gla, options);
    let ja = &retargeter.ja;
    let times: Vec<f32> = clip
        .tracks
        .first()
        .map(|track| track.orientations.iter().map(|key| key.0).collect())
        .unwrap_or_else(Vec::new);
    let mut tracks: BTreeMap<usize, NodeTrack> = BTreeMap::new();
    let mut off = OffHand::default();
    for (key, &time) in times.iter().enumerate() {
        let finals = ja.finals(clip, key);
        let ground = options.ground.map_or(Vec3::ZERO, |ground| ground(time));
        let posed = retargeter.frame(&finals, ground, (&[], &[]), (&mut off, time * 1000.0));
        for (node, q) in posed.rotations {
            tracks
                .entry(node)
                .or_insert_with(|| track(&kotor.nodes[node].name))
                .orientations
                .push((time, [q.x, q.y, q.z, q.w]));
        }
        if let Some((root, offset)) = posed.root {
            tracks
                .entry(root)
                .or_insert_with(|| track(&kotor.nodes[root].name))
                .positions
                .push((time, offset.to_array()));
        }
    }
    AnimationIr {
        name: clip.name.clone(),
        length: clip.length,
        transition: 0.0,
        root: None,
        tracks: tracks.into_values().collect(),
        settings: Vec::new(),
        events: Vec::new(),
    }
}

/// One retargeted moment: each posed node's local rotation, and the root
/// node's key (added, scaled by the model's animation scale, to its rest
/// position as the animator does).
#[derive(Debug, Clone, Default)]
pub struct Posed {
    pub rotations: Vec<(usize, Quat)>,
    pub root: Option<(usize, Vec3)>,
}

/// `retarget_with` one moment at a time, for a player animated live: the
/// skeletons are lined up once, then each JA pose (`g2::Player::pose`, one
/// local per GLA bone) becomes a KOTOR pose.
pub struct Retargeter<'a> {
    kotor: &'a ModelIr,
    ja: Ja<'a>,
    k: Kotor<'a>,
    scale: f32,
    pelvis: Option<usize>,
    root: Option<usize>,
    hold: BTreeMap<usize, Quat>,
    grips: Vec<&'a Grip>,
    ja_sole: f32,
    arms: Option<AimArms>,
    /// The nodes the pose takes at rest that carry the others (`carriers`).
    carriers: Vec<usize>,
    /// JA's left and right hand bones.
    hands: Option<[usize; 2]>,
}

impl<'a> Retargeter<'a> {
    /// `options.ground` is not used: `pose` takes the travel.
    pub fn new(kotor: &'a ModelIr, gla: &'a Gla, options: &Options<'a>) -> Retargeter<'a> {
        let ja = Ja::new(gla);
        let k = Kotor::new(kotor, &ja);
        let scale = pelvis_scale(&ja, &k);
        let pelvis = ja.bone("pelvis");
        let root = k.root(&ja);
        let hold: BTreeMap<usize, Quat> = options
            .hold
            .iter()
            .filter_map(|(name, [x, y, z, w])| {
                let node = k.node(name)?;
                (!k.mapped.contains_key(&node))
                    .then(|| (node, Quat::from_xyzw(*x, *y, *z, *w).normalize()))
            })
            .collect();
        let arms = AimArms::of(&k);
        let carriers = carriers(&k, &hold);
        let hands = ja
            .bone("lhand")
            .zip(ja.bone("rhand"))
            .map(<[usize; 2]>::from);
        Retargeter {
            arms,
            carriers,
            hands,
            kotor,
            ja,
            k,
            scale,
            pelvis,
            root,
            hold,
            grips: options.grips.clone(),
            ja_sole: options.ja_sole,
        }
    }

    /// How near JA's pose (`finals`) brings its hands (0 to 1): all the
    /// way with them `TWO_HANDED.0` apart or nearer, none from
    /// `TWO_HANDED.1`.
    fn off_hand(&self, finals: &[Frame]) -> Option<f32> {
        let [left, right] = self.hands?;
        let at = |bone: usize| finals[bone].point(self.ja.base(bone));
        let apart = at(left).distance(at(right));
        let (near, far) = TWO_HANDED;
        Some(smooth(far, near, apart))
    }

    /// The KOTOR model's node names, in its order.
    pub fn node_names(&self) -> Vec<&str> {
        self.kotor.nodes.iter().map(|n| n.name.as_str()).collect()
    }

    /// A JA pose (bone locals: rotation, position in metres) with the
    /// model moved by `ground` (KOTOR model space, metres), each gripping
    /// hand in `reach` reaching for its blade (`Reach`), each gun held by
    /// its arm's aim in `aims` (`Aim`), and the left hand on a lone hilt as
    /// `off` has it, eased on at `ms` (`OffHand`).
    pub fn pose(
        &self,
        locals: &[(Quat, Vec3)],
        ground: Vec3,
        (reach, aim): (&[Reach], &[Aim]),
        off: (&mut OffHand, f32),
    ) -> Posed {
        let finals = self.ja.finals_of(locals);
        self.frame(&finals, ground, (reach, aim), off)
    }

    fn frame(
        &self,
        finals: &[Frame],
        ground: Vec3,
        (reach, aims): (&[Reach], &[Aim]),
        (off, ms): (&mut OffHand, f32),
    ) -> Posed {
        let (ja, k, kotor) = (&self.ja, &self.k, self.kotor);
        let (scale, root, hold, grips) = (self.scale, self.root, &self.hold, &self.grips);
        // Clips stand the model on a floor `ORIGIN_HEIGHT` below its origin;
        // the base pose stands on the origin.
        let mut travel = self.pelvis.map_or(Vec3::ZERO, |b| {
            let floor = Vec3::Z * crate::ORIGIN_HEIGHT;
            TURN * (finals[b].point(ja.base(b)) + floor - ja.base(b)) * scale
        }) + ground;
        let none = BTreeMap::new();
        let mut globals = k.globals(ja, finals, root, travel, grips, hold, &none);
        // Keep the supporting foot at KOTOR's ankle height plus JA's lift
        // of it, scaled: the legs' proportions differ, so scaled pelvis travel
        // alone sinks or floats the feet. The feet share the correction by
        // how low JA holds each (a soft minimum over 3 cm), so it glides
        // rather than jumps when the weight changes feet.
        // Off the ground (a jump), the feet are held that much higher: JA's
        // clips jump in place.
        let height = ground.z;
        let feet: Vec<(f32, f32)> = FEET
            .iter()
            .filter_map(|&(ja_foot, kotor_foot)| {
                let (b, n) = (ja.bone(ja_foot)?, k.node(kotor_foot)?);
                let standing = ja.base(b).z - self.ja_sole;
                let lift =
                    (finals[b].point(ja.base(b)).z + crate::ORIGIN_HEIGHT - standing) * scale;
                Some((
                    lift,
                    k.rest[n].position.z + lift + height - globals[n].position.z,
                ))
            })
            .collect();
        if let Some(low) = feet.iter().map(|f| f.0).reduce(f32::min) {
            let weights: Vec<f32> = feet.iter().map(|f| (-(f.0 - low) / 0.03).exp()).collect();
            let total: f32 = weights.iter().sum();
            let dz = feet.iter().zip(&weights).map(|(f, w)| f.1 * w).sum::<f32>() / total;
            if dz.abs() > 1e-5 {
                travel.z += dz;
                globals = k.globals(ja, finals, root, travel, grips, hold, &none);
            }
        }
        let as_ja = globals.clone();
        let mut turned: BTreeMap<usize, Quat> = reach
            .iter()
            .filter_map(|reach| Some((grips.get(reach.grip)?, reach)))
            .flat_map(|(grip, reach)| reach_turns(&globals, grip, reach))
            .collect();
        if !turned.is_empty() {
            globals = k.globals(ja, finals, root, travel, grips, hold, &turned);
        }
        // One hilt in the right hand: the left hand holds it too where JA's
        // does, letting go as the hilt goes where the KOTOR body puts it
        // (`Reach::free`).
        let left = self
            .arms
            .and_then(|arms| Some((arms.left?.0, arms.left_weapon?)));
        let held = match (grips.as_slice(), left, self.off_hand(finals)) {
            ([grip], Some(left), Some(near)) => off_hand_hold((&as_ja, &globals), grip, left, near),
            _ => None,
        };
        let free = reach.iter().map(|reach| reach.free).fold(0.0, f32::max);
        let wanted = held.map_or(0.0, |(wanted, _)| wanted) * (1.0 - free.clamp(0.0, 1.0));
        let share = off.ease(wanted, ms);
        if let (Some((_, hold_at)), Some((arm, _))) = (held.filter(|_| share > 0.0), left)
            && let Some(turns) = hold_turns(&globals, arm, hold_at, share)
        {
            turned.extend(turns);
            globals = k.globals(ja, finals, root, travel, grips, hold, &turned);
        }
        // Each arm its own gun; the left hand under a lone right one. JA
        // holds no gun up in the left hand: a left gun beside a right one is
        // held where the right is, mirrored across the chest (`mirrored`).
        let lone = aims.len() == 1;
        let mut right_muzzle = None;
        let shown = |aim: &&Aim| aim.weight > 0.0 || aim.stow.is_some_and(|stow| stow.share > 0.0);
        for aim in aims.iter().filter(shown) {
            let Some(arms) = &self.arms else { break };
            let mut aim = *aim;
            if let (Side::Left, None, Some(muzzle)) = (aim.side, aim.hold, right_muzzle) {
                aim.hold = mirrored(&globals, arms, (muzzle, aim.target - muzzle));
            }
            let aim = &aim;
            turned.extend(aim_turns(&globals, arms, aim));
            globals = k.globals(ja, finals, root, travel, grips, hold, &turned);
            if aim.side == Side::Right {
                right_muzzle = Some(globals[arms.weapon].point(aim.muzzle));
            }
            if let (true, Side::Right, Some(support), Some(left)) =
                (lone, aim.side, aim.support, arms.left)
            {
                turned.extend(support_turns(&globals, arms, left, (aim, support)));
                globals = k.globals(ja, finals, root, travel, grips, hold, &turned);
            }
        }
        let parent_of = |node: usize| {
            kotor.nodes[node]
                .parent
                .map_or(Frame::IDENTITY, |p| globals[p])
        };
        let rotations = k
            .mapped
            .keys()
            .chain(hold.keys())
            .chain(&self.carriers)
            .map(|&node| (node, parent_of(node).inverse().then(globals[node]).rotation))
            .collect();
        // The animator adds `animation scale × key` to the rest position.
        let root = root.map(|root| {
            let offset = parent_of(root).rotation.inverse() * travel
                / kotor.animation_scale.max(f32::EPSILON);
            (root, offset)
        });
        Posed { rotations, root }
    }
}

/// The nodes a pose takes at rest under their parents (neither mapped nor
/// held) that a mapped node or a weapon node (`rhand`, `lhand`) hangs from,
/// and the weapon nodes themselves: `rbicepl_g` between the upper arm and
/// the forearm, `hturn_g`, the dummies over the pelvis. The pose sets them
/// as it took them, so a clip the body plays besides (a creature's idle)
/// cannot move the hands or the guns off where the pose put them.
fn carriers(k: &Kotor, hold: &BTreeMap<usize, Quat>) -> Vec<usize> {
    let nodes = &k.ir.nodes;
    let weapons = asset_odyssey::hooks::HANDS
        .iter()
        .filter_map(|name| k.node(name));
    let mut carriers = Vec::new();
    for start in k.mapped.keys().copied().chain(weapons) {
        let mut at = Some(start);
        while let Some(node) = at {
            if !k.mapped.contains_key(&node)
                && !hold.contains_key(&node)
                && !carriers.contains(&node)
            {
                carriers.push(node);
            }
            at = nodes[node].parent;
        }
    }
    carriers.sort_unstable();
    carriers
}

fn track(node: &str) -> NodeTrack {
    NodeTrack {
        node: node.to_string(),
        positions: Vec::new(),
        orientations: Vec::new(),
    }
}

/// A KOTOR hand holding a JA hilt the KOTOR way: KOTOR hangs a weapon at
/// the hand's weapon node (`rhand`), its grip at the node and its blade up
/// the node's +Z (the K1 hilts run so). The JA hilt is turned to match
/// (`hilt`: its blade direction onto +Z, rolled to sit as JA's hand holds it
/// in the base pose), and each moment the hand is turned so the hilt points
/// exactly as JA's does (`TURN · D(tag bone) · bolt`); the fingers take a
/// KOTOR grip pose (`Options::hold`). The hilt's place then follows the
/// KOTOR arm.
#[derive(Debug, Clone)]
pub struct Grip {
    /// KOTOR's hand bone (`rhand_g`) and weapon node (`rhand`).
    pub hand: usize,
    pub weapon: usize,
    /// The JA bone the hand bolt hangs on, and the bolt's rotation.
    bone: usize,
    bolt: Quat,
    /// Hilt space to the weapon node's space.
    hilt: Quat,
    /// KOTOR's upper arm and forearm (`rbicep_g`, `rforearm_g`), which bend
    /// for a reach.
    arm: Option<[usize; 2]>,
}

/// Where a gripping hand's blade should be (`Retargeter::pose`): the hand
/// turns so the blade of the hilt at its weapon node runs along
/// `direction` and moves `share` (0 to 1) of the way to where it leaves
/// the hilt at `muzzle` (KOTOR model space, metres), the upper arm and
/// forearm bending to carry it there, the wrist moving `most` metres at
/// most (`REACH` for the sim's blade).
#[derive(Debug, Clone, Copy)]
pub struct Reach {
    /// The grip's place among the retargeter's.
    pub grip: usize,
    pub muzzle: Vec3,
    pub direction: Vec3,
    /// Where the blade leaves the hilt and which way it runs, in the weapon
    /// node's space.
    pub origin: Vec3,
    pub along: Vec3,
    pub share: f32,
    pub most: f32,
    /// How much the place is the KOTOR body's own (0 to 1: a blade held
    /// before a first-person eye) rather than where JA's shorter arm put
    /// the sim's blade: the off hand lets a lone hilt go.
    pub free: f32,
}

/// How far a reach moves the wrist at most (metres): beyond it the hand
/// goes that far toward the blade and the gap shows.
pub const REACH: f32 = 0.3;

/// Which arm holds an aimed gun.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Side {
    Right,
    Left,
}

/// A gun held up and aimed (`Retargeter::pose`), taken `weight` of the way
/// from the pose as JA has it: its arm's hand (`side`) turned so the barrel runs
/// from the muzzle to `target` (KOTOR model space), kicked up by `kick.0`
/// (radians), the gun upright; the wrist moved `forward` along the aim and
/// back by `kick.1` (metres), or with `hold` the arm carrying the muzzle
/// there (a first-person eye's gun, held up before it); with `support`, the
/// left palm under the gun there, its fingers round the far side.
#[derive(Debug, Clone, Copy)]
pub struct Aim {
    pub side: Side,
    pub target: Vec3,
    /// The gun in its hand's weapon node's space: its muzzle; its barrel
    /// (toward the muzzle) and up, unit; the point under it the other hand
    /// holds.
    pub muzzle: Vec3,
    pub barrel: Vec3,
    pub up: Vec3,
    pub support: Option<Vec3>,
    pub forward: f32,
    pub kick: (f32, f32),
    pub weight: f32,
    pub hold: Option<Vec3>,
    /// The hand on the gun as it sits in its holster, `share` of the way
    /// from the aim (a draw's or a holster's way there, `game::draw_anim`).
    pub stow: Option<Stowed>,
}

/// A gun in its holster: the body node it hangs from and its place under
/// it (rotation, position in metres), and how far (0 to 1) the hand is on
/// it rather than aiming.
#[derive(Debug, Clone, Copy)]
pub struct Stowed {
    pub node: usize,
    pub place: (Quat, Vec3),
    pub share: f32,
}

/// KOTOR's arms for an aim: each side's upper arm, forearm and hand bone,
/// the right hand's weapon node, and the left hand's finger bases and tips.
#[derive(Debug, Clone, Copy)]
struct AimArms {
    right: [usize; 3],
    weapon: usize,
    left: Option<([usize; 3], [[usize; 2]; 4])>,
    /// The left hand's weapon node (`lhand`).
    left_weapon: Option<usize>,
}

impl AimArms {
    fn of(k: &Kotor) -> Option<AimArms> {
        let side = |s: &str| {
            let [upper, fore, hand] =
                ["bicep_g", "forearm_g", "hand_g"].map(|part| k.node(&format!("{s}{part}")));
            Some([upper?, fore?, hand?])
        };
        let fingers: Option<Vec<[usize; 2]>> = ['a', 'b', 'c', 'd']
            .map(|finger| {
                let [base, tip] = ['b', 't'].map(|end| k.node(&format!("l{finger}fngr{end}_g")));
                Some([base?, tip?])
            })
            .into_iter()
            .collect();
        let fingers = fingers.and_then(|fingers| <[[usize; 2]; 4]>::try_from(fingers).ok());
        Some(AimArms {
            right: side("r")?,
            weapon: k.node("rhand")?,
            left: side("l").zip(fingers),
            left_weapon: k.node("lhand"),
        })
    }
}

/// A turn's frame: `forward` and `up` made square, as a rotation from X
/// forward and Z up.
fn basis(forward: Vec3, up: Vec3) -> Option<Quat> {
    let forward = forward.try_normalize()?;
    let side = up.cross(forward).try_normalize()?;
    let up = forward.cross(side);
    Some(Quat::from_mat3(&Mat3::from_cols(forward, side, up)).normalize())
}

/// The turns (globals) of `aim`'s arm: the hand turned so the barrel runs
/// along the kicked aim at the target with the gun upright, the arm bent
/// to carry the wrist forward and back, or the muzzle to its hold.
fn aim_turns(globals: &[Frame], arms: &AimArms, aim: &Aim) -> Vec<(usize, Quat)> {
    let ([upper, fore, hand], weapon) = match aim.side {
        Side::Right => (arms.right, arms.weapon),
        Side::Left => match (arms.left, arms.left_weapon) {
            (Some((left, _)), Some(weapon)) => (left, weapon),
            _ => return Vec::new(),
        },
    };
    let hand_frame = globals[hand];
    let gun = globals[weapon].rotation;
    // From where the muzzle will be: the pose's, or on its way to the hold.
    let muzzle = globals[weapon].point(aim.muzzle);
    let from = aim
        .hold
        .map_or(muzzle, |hold| muzzle.lerp(hold, aim.weight));
    let direction = (aim.target - from).normalize_or_zero();
    // Up is a turn about the aim's level side, never past `STEEPEST`.
    let side = direction.cross(Vec3::Z).try_normalize().unwrap_or(Vec3::X);
    let kick = aim
        .kick
        .0
        .min(STEEPEST - direction.z.clamp(-1.0, 1.0).asin())
        .max(0.0);
    let kicked = Quat::from_axis_angle(side, kick) * direction;
    let (Some(now), Some(wanted)) = (
        basis(gun * aim.barrel, gun * aim.up),
        basis(kicked, Vec3::Z),
    ) else {
        return Vec::new();
    };
    let turn = Quat::IDENTITY.slerp(wanted * now.inverse(), aim.weight);
    let wrist = hand_frame.position;
    let moved = match aim.hold {
        // The muzzle as the turned hand carries it, moved onto the hold.
        Some(hold) => {
            let carried = turn * (muzzle - wrist);
            wrist.lerp(hold - carried - direction * aim.kick.1, aim.weight)
        }
        None => wrist + direction * ((aim.forward - aim.kick.1) * aim.weight),
    };
    let mut turns = bend(globals, [upper, fore], wrist, moved).map_or_else(Vec::new, Vec::from);
    turns.push((hand, (turn * hand_frame.rotation).normalize()));
    match aim.stow.filter(|stow| stow.share > 0.0) {
        Some(stow) => {
            let holstered = stowed_turns(globals, ([upper, fore, hand], weapon), aim, stow);
            [upper, fore, hand]
                .into_iter()
                .map(|bone| {
                    let of = |turns: &[(usize, Quat)]| {
                        turns
                            .iter()
                            .find(|(at, _)| *at == bone)
                            .map_or(globals[bone].rotation, |&(_, rotation)| rotation)
                    };
                    let (aimed, held) = (of(&turns), of(&holstered));
                    (bone, aimed.slerp(held, stow.share).normalize())
                })
                .collect()
        }
        None => turns,
    }
}

/// The turns (globals) of an arm whose hand holds its gun where it sits in
/// its holster (`Stowed`): the hand turned so the gun in it lies as the
/// holstered one does, the arm bent to carry the muzzle onto its muzzle.
fn stowed_turns(
    globals: &[Frame],
    ([upper, fore, hand], weapon): ([usize; 3], usize),
    aim: &Aim,
    stow: Stowed,
) -> Vec<(usize, Quat)> {
    let (rotation, position) = stow.place;
    let holster = globals[stow.node].then(Frame { rotation, position });
    let gun = globals[weapon].rotation;
    let (Some(now), Some(wanted)) = (
        basis(gun * aim.barrel, gun * aim.up),
        basis(holster.rotation * aim.barrel, holster.rotation * aim.up),
    ) else {
        return Vec::new();
    };
    let turn = wanted * now.inverse();
    let wrist = globals[hand].position;
    let carried = turn * (globals[weapon].point(aim.muzzle) - wrist);
    let reach = holster.point(aim.muzzle) - carried;
    let mut turns = bend(globals, [upper, fore], wrist, reach).map_or_else(Vec::new, Vec::from);
    turns.push((hand, (turn * globals[hand].rotation).normalize()));
    turns
}

/// `point` mirrored across the upright plane along `way` (the aim) through
/// the middle between the shoulders (`AimArms`' upper arms): the guns side
/// by side along the aim, however JA's one-handed stance turns the chest.
fn mirrored(globals: &[Frame], arms: &AimArms, (point, way): (Vec3, Vec3)) -> Option<Vec3> {
    let (right, left) = (
        globals[arms.right[0]].position,
        globals[arms.left?.0[0]].position,
    );
    let across = way.cross(Vec3::Z).try_normalize()?;
    let middle = (left + right) / 2.0;
    Some(point - across * 2.0 * (point - middle).dot(across))
}

/// Where the palm's middle lies: a share of the way from the wrist to the
/// knuckles, and out from the bones toward the palm's face (metres).
const PALM: (f32, f32) = (0.5, 0.02);
/// The least finger curl (metres, tips off the line of the hand) that shows
/// which way the palm faces.
const CURL: f32 = 0.005;
/// The steepest a kick takes a barrel (radians from level).
const STEEPEST: f32 = 85.0 * std::f32::consts::PI / 180.0;

/// The left arm's turns (globals) holding the gun at `support` (`Aim`): the
/// palm against it from below, the fingers across it and a little forward,
/// round its far side, the arm bent to carry the wrist there.
fn support_turns(
    globals: &[Frame],
    arms: &AimArms,
    (left, fingers): ([usize; 3], [[usize; 2]; 4]),
    (aim, support): (&Aim, Vec3),
) -> Vec<(usize, Quat)> {
    let [upper, fore, hand] = left;
    let gun = globals[arms.weapon];
    let (barrel, up) = (gun.rotation * aim.barrel, gun.rotation * aim.up);
    // The hand as the pose has it: along it to the knuckles, and the side
    // its fingers curl to, the palm's face.
    let mean = |end: usize| {
        fingers
            .iter()
            .map(|f| globals[f[end]].position)
            .sum::<Vec3>()
            / 4.0
    };
    let (knuckles, tips) = (mean(0), mean(1));
    let wrist = globals[hand].position;
    let along = knuckles - wrist;
    let square = |v: Vec3| v - along.normalize_or_zero() * v.dot(along.normalize_or_zero());
    // Fingers too straight to show which way they curl: the palm already
    // faces the gun's up, so only its fingers turn.
    let face = Some(square(tips - knuckles))
        .filter(|face| face.length() > CURL)
        .unwrap_or_else(|| square(up));
    let (Some(now), Some(wanted)) = (
        basis(along, face),
        basis(barrel * 0.5 + barrel.cross(up) * 0.85, up),
    ) else {
        return Vec::new();
    };
    let turn = Quat::IDENTITY.slerp(wanted * now.inverse(), aim.weight);
    let palm = along * PALM.0 + face.normalize_or_zero() * PALM.1;
    let target = gun.point(support) - turn * palm;
    let moved = wrist.lerp(target, aim.weight);
    let mut turns = bend(globals, [upper, fore], wrist, moved).map_or_else(Vec::new, Vec::from);
    turns.push((hand, (turn * globals[hand].rotation).normalize()));
    turns
}

/// How far apart (metres) JA's hands are when both hold one hilt (its
/// two-handed saber stance has them 0.12 apart), and from how far apart
/// the left has let go (a one-handed swing spreads them 0.2 to 0.8): the
/// grip eases over the stretch between, so the hand leaves the hilt as
/// smoothly as JA's leaves the other.
const TWO_HANDED: (f32, f32) = (0.14, 0.3);
/// How far below the right hand's grip the left one holds a hilt both
/// hold (metres along it): a fist's width at least, as far as the pose
/// has it below that, at most `LOWEST`.
const FIST: f32 = 0.085;
const LOWEST: f32 = 0.12;
/// The left hand takes the hilt only from below the right's grip (metres
/// along it: from none at the first to all at the second), its own hilt
/// running the same way (the cosine between them: none to all), so it is
/// never pulled across the right fist nor turned over onto it (a staff's
/// far end, a reversed hand).
const BELOW: (f32, f32) = (0.0, -0.04);
const SAME_WAY: (f32, f32) = (0.3, 0.7);
/// How far (metres) the hilt may be from where the pose has the left hand
/// for it to take it fully, and from how far it does not.
const IN_REACH: (f32, f32) = (0.08, 0.15);
/// Seconds the left hand takes to take a hilt or let it go.
const OFF_HAND_S: f32 = 0.15;

/// 0 at `from`, 1 at `to`, eased between (either way round).
fn smooth(from: f32, to: f32, x: f32) -> f32 {
    let t = ((x - from) / (to - from)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// How much (0 to 1) and where the left hand holds the right hand's hilt
/// (`grip`): as far as JA's hands are together (`near`,
/// `Retargeter::off_hand`), the hand is below the grip and the same way
/// round (`BELOW`, `SAME_WAY`), and the hilt is in reach of where the pose
/// has the hand (`IN_REACH`). Where: the left hand's weapon node (`lhand`,
/// where it would hold a hilt of its own) on the hilt's axis, `FIST` to
/// `LOWEST` below the right's grip, and turned so its own hilt would run
/// along this one, rolled as the pose before the right hand's reach had
/// it (`as_ja`); given as the hand bone's frame. K1's hilts run up their
/// weapon node's +Z.
fn off_hand_hold(
    (as_ja, globals): (&[Frame], &[Frame]),
    grip: &Grip,
    ([_, _, hand], weapon): ([usize; 3], usize),
    near: f32,
) -> Option<(f32, Frame)> {
    let (was, hilt) = (as_ja[grip.weapon], globals[grip.weapon]);
    // The left weapon node in the hilt's space, as the pose has it.
    let posed = was.inverse().then(as_ja[weapon]);
    let way = posed.rotation * Vec3::Z;
    let along = Quat::from_rotation_arc(way, Vec3::Z);
    let held = hilt.then(Frame {
        rotation: (along * posed.rotation).normalize(),
        position: Vec3::Z * posed.position.z.clamp(-LOWEST, -FIST),
    });
    // The hand that puts its weapon node there.
    let palm = globals[hand].inverse().then(globals[weapon]);
    let wanted = held.then(palm.inverse());
    let reach = wanted.position.distance(globals[hand].position);
    let fit = near
        * smooth(BELOW.0, BELOW.1, posed.position.z)
        * smooth(SAME_WAY.0, SAME_WAY.1, way.z)
        * smooth(IN_REACH.1, IN_REACH.0, reach);
    fit.is_finite().then_some((fit, wanted))
}

/// The left arm's turns (globals) taking the hand `share` of the way from
/// the pose to `wanted` (its frame): the hand turned, the arm bent to carry
/// the wrist.
fn hold_turns(
    globals: &[Frame],
    [upper, fore, hand]: [usize; 3],
    wanted: Frame,
    share: f32,
) -> Option<Vec<(usize, Quat)>> {
    let wrist = globals[hand].position;
    let moved = wrist.lerp(wanted.position, share);
    let mut turns = bend(globals, [upper, fore], wrist, moved)?.to_vec();
    let turn = globals[hand].rotation.slerp(wanted.rotation, share);
    turns.push((hand, turn.normalize()));
    Some(turns)
}

/// A body's left hand on a lone hilt over time (`Retargeter::pose`): how
/// much it holds, as of when (ms). It eases toward what each pose wants at
/// most a whole hold in `OFF_HAND_S`, so a hand never jumps onto the hilt
/// or off it as JA's pose changes from frame to frame.
#[derive(Debug, Clone, Copy, Default)]
pub struct OffHand {
    share: f32,
    ms: Option<f32>,
}

impl OffHand {
    /// Eased toward `wanted` at `ms`: the share held now.
    fn ease(&mut self, wanted: f32, ms: f32) -> f32 {
        let step = self
            .ms
            .map_or(1.0, |was| (ms - was).max(0.0) / 1000.0 / OFF_HAND_S);
        self.ms = Some(ms);
        self.share = if self.share < wanted {
            (self.share + step).min(wanted)
        } else {
            (self.share - step).max(wanted)
        };
        self.share
    }
}

/// The global turns that carry a grip's blade to its reach: the hand turned
/// onto the blade's direction, then the upper arm and forearm bent (two
/// bones, in the plane the elbow is in) so the wrist is where that hand
/// puts the blade's start on the muzzle.
fn reach_turns(globals: &[Frame], grip: &Grip, reach: &Reach) -> Vec<(usize, Quat)> {
    let Some([upper, fore]) = grip.arm else {
        return Vec::new();
    };
    let (hand, weapon) = (grip.hand, grip.weapon);
    // The weapon node from the hand, in the hand's space.
    let hand_frame = globals[hand];
    let weapon_local = hand_frame.inverse().then(globals[weapon]);
    let along = (globals[weapon].rotation * reach.along).normalize_or_zero();
    let aim = Quat::from_rotation_arc(along, reach.direction.normalize_or_zero());
    let hand_rotation = (aim * hand_frame.rotation).normalize();
    let weapon_frame = Frame {
        rotation: hand_rotation,
        position: Vec3::ZERO,
    }
    .then(weapon_local);
    let wanted = reach.muzzle - weapon_frame.point(reach.origin);
    let wrist = hand_frame.position;
    let wanted = wrist + (wanted - wrist).clamp_length_max(reach.most) * reach.share;
    // One solver, whatever the place: the arm keeps its own plane as the
    // wrist moves (`bend`), so the elbow never jumps between answers.
    let Some(arm) = bend(globals, [upper, fore], wrist, wanted) else {
        return Vec::new();
    };
    let mut turns = arm.to_vec();
    turns.push((hand, hand_rotation));
    turns
}

/// How strongly a nearly straight arm's elbow side leans down in `bend`
/// (metres: an arm bent by more than this keeps its own side).
const ELBOW_HANG: f32 = 0.02;

/// The upper arm and forearm turned (globals) so the wrist, now at `wrist`,
/// comes to `wanted`: two bones, bending in the plane the elbow is in.
/// `None` for an arm with no length or a place out of reach of numbers.
fn bend(
    globals: &[Frame],
    [upper, fore]: [usize; 2],
    wrist: Vec3,
    wanted: Vec3,
) -> Option<[(usize, Quat); 2]> {
    let (shoulder, elbow) = (globals[upper].position, globals[fore].position);
    let (a, b) = (elbow.distance(shoulder), wrist.distance(elbow));
    let to = wanted - shoulder;
    if !(a > 1e-3 && b > 1e-3 && to.is_finite()) {
        return None;
    }
    let d = to.length().clamp((a - b).abs() + 1e-4, a + b - 1e-4);
    let u = to.try_normalize()?;
    // The elbow's side: the arm's own plane, turned as the line from the
    // shoulder turns from the wrist to `wanted`, so it never depends on
    // where the elbow happens to sit against the new line (which flips it
    // as the line passes the elbow). A nearly straight arm has no plane:
    // its side fades toward the elbow hanging down, never jumping.
    let turn = Quat::from_rotation_arc((wrist - shoulder).try_normalize()?, u);
    let turned = turn * (elbow - shoulder);
    let down = Vec3::NEG_Z * ELBOW_HANG;
    let square = |v: Vec3| v - u * v.dot(u);
    let v = (square(turned) + square(down))
        .try_normalize()
        .unwrap_or_else(|| u.any_orthonormal_vector());
    let cos = ((a * a + d * d - b * b) / (2.0 * a * d)).clamp(-1.0, 1.0);
    let new_elbow = shoulder + a * (cos * u + (1.0 - cos * cos).sqrt() * v);
    let new_wrist = shoulder + u * d;
    let first = Quat::from_rotation_arc(
        (elbow - shoulder).normalize_or_zero(),
        (new_elbow - shoulder).normalize_or_zero(),
    );
    let second = Quat::from_rotation_arc(
        (first * (wrist - elbow)).normalize_or_zero(),
        (new_wrist - new_elbow).normalize_or_zero(),
    );
    Some([
        (upper, (first * globals[upper].rotation).normalize()),
        (fore, (second * first * globals[fore].rotation).normalize()),
    ])
}

/// Why a hand cannot hold a hilt (`Grip::new`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GripError {
    /// The KOTOR body has no such weapon node, or it hangs on nothing.
    NoWeapon(String),
    /// The hand bolt hangs on a bone the GLA does not have.
    BoltBone { bone: usize, bones: usize },
    /// The blade has no direction.
    NoBlade,
}

impl std::fmt::Display for GripError {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            GripError::NoWeapon(node) => write!(f, "{node}: not a weapon node in the body"),
            GripError::BoltBone { bone, bones } => {
                write!(f, "bolt on bone {bone}, past the GLA's {bones} bones")
            }
            GripError::NoBlade => write!(f, "the blade has no direction"),
        }
    }
}

impl std::error::Error for GripError {}

impl Grip {
    /// `bolt` is the JA body's hand bolt (`*r_hand` or `*l_hand`); `blade`
    /// the first blade's direction in the hilt's space; `weapon` KOTOR's
    /// weapon node for that hand (`rhand` or `lhand`).
    pub fn new(
        kotor: &ModelIr,
        gla: &Gla,
        bolt: &Bolt,
        blade: [f32; 3],
        weapon: &str,
    ) -> Result<Grip, GripError> {
        if bolt.bone >= gla.bones.len() {
            return Err(GripError::BoltBone {
                bone: bolt.bone,
                bones: gla.bones.len(),
            });
        }
        let ja = Ja::new(gla);
        let k = Kotor::new(kotor, &ja);
        let no_weapon = || GripError::NoWeapon(weapon.to_string());
        let weapon = k.node(weapon).ok_or_else(no_weapon)?;
        let hand = kotor.nodes[weapon].parent.ok_or_else(no_weapon)?;
        let align = k.mapped.get(&hand).map_or(Quat::IDENTITY, |m| m.1);
        // The weapon node and the JA hilt in JA's base pose.
        let node = align * k.rest[hand].rotation * k.local[weapon].rotation;
        let bolt_rotation = Frame::of(&bolt.matrix).rotation;
        let rough = node.inverse() * TURN * bolt_rotation;
        let blade = Vec3::from_array(blade)
            .try_normalize()
            .ok_or(GripError::NoBlade)?;
        let hilt = (Quat::from_rotation_arc(rough * blade, Vec3::Z) * rough).normalize();
        let side = &kotor.nodes[weapon].name[..1];
        let arm = ["bicep_g", "forearm_g"].map(|part| k.node(&format!("{side}{part}")));
        Ok(Grip {
            hand,
            weapon,
            bone: bolt.bone,
            bolt: bolt_rotation,
            hilt,
            arm: arm[0].zip(arm[1]).map(|(upper, fore)| [upper, fore]),
        })
    }

    /// The hilt model's transform under the weapon node.
    pub fn hilt_local(&self) -> Matrix {
        Frame {
            rotation: self.hilt,
            position: Vec3::ZERO,
        }
        .matrix()
    }
}

/// Where a KOTOR node sits on JA's skeleton for `rebind`: the JA bone its
/// vertices follow, and the points that place it. The node moves by
/// `TURN · mean(JA points) · scale − mean(KOTOR points)` (the points default
/// to the bone and the node themselves). The torso anchors one bone lower
/// than it follows because KOTOR's torso nodes sit lower in the body than
/// JA's namesakes (Carth's `torsoupr_g` at 1.24 m, JA's `thoracic` at
/// 1.39 m once scaled); the pelvis rides on the hip joints (JA's `pelvis` is
/// at hip height, KOTOR's `pelvis_g` 14 cm above its thighs).
struct Seat {
    kotor: &'static str,
    bone: &'static str,
    ja: &'static [&'static str],
    kotor_points: &'static [&'static str],
}

const fn seat(kotor: &'static str, bone: &'static str) -> Seat {
    Seat {
        kotor,
        bone,
        ja: &[],
        kotor_points: &[],
    }
}

const SEATS: &[Seat] = &[
    Seat {
        kotor: "pelvis_g",
        bone: "pelvis",
        ja: &["lfemurYZ", "rfemurYZ"],
        kotor_points: &["lthigh_g", "rthigh_g"],
    },
    seat("torso_g", "lower_lumbar"),
    Seat {
        kotor: "torsoupr_g",
        bone: "thoracic",
        ja: &["upper_lumbar"],
        kotor_points: &[],
    },
    seat("necklwr_g", "cervical"),
    seat("neck_g", "cervical"),
    seat("head_g", "cranium"),
    seat("rcollar_g", "rclavical"),
    seat("rbicep_g", "rhumerus"),
    seat("rforearm_g", "rradius"),
    seat("rhand_g", "rhand"),
    seat("lcollar_g", "lclavical"),
    seat("lbicep_g", "lhumerus"),
    seat("lforearm_g", "lradius"),
    seat("lhand_g", "lhand"),
    seat("rthigh_g", "rfemurYZ"),
    seat("rshin_g", "rtibia"),
    seat("rfoot_g", "rtalus"),
    seat("lthigh_g", "lfemurYZ"),
    seat("lshin_g", "ltibia"),
    seat("lfoot_g", "ltalus"),
];

/// KOTOR models skinned to JA's skeleton at JA's proportions, for a JA body
/// wearing a KOTOR outfit and head. `parts` are the body first, then models
/// hung on it: (model, its root's rest transform in the body's space, the
/// body node it hangs on). Each seated node (`SEATS`) turns as retargeting
/// lines it up (`align`) and moves onto JA's skeleton in its base pose,
/// scaled by `scale`; a model hung on the body moves its nodes as the
/// body's namesakes move. Other nodes ride on their parents. Every vertex is
/// carried by each node it weighs on and follows that node's JA bone (or
/// its nearest seated ancestor's). Returned meshes are in JA model space and
/// units, divided by `scale`, with bone slot `b` driving GLA bone `b` (as
/// `crate::model`'s).
pub fn rebind(gla: &Gla, parts: &[(&ModelIr, Matrix, Option<usize>)], scale: f32) -> Vec<MeshIr> {
    let ja = Ja::new(gla);
    let Some((body, _, _)) = parts.first() else {
        return Vec::new();
    };
    let rig = Kotor::new(body, &ja);
    // Node name → (JA bone, align, move).
    let mut seats: BTreeMap<String, (usize, Quat, Vec3)> = BTreeMap::new();
    for seat in SEATS {
        let (Some(node), Some(bone)) = (rig.node(seat.kotor), ja.bone(seat.bone)) else {
            continue;
        };
        let ja_points = if seat.ja.is_empty() {
            &[seat.bone][..]
        } else {
            seat.ja
        };
        let kotor_points: Option<Vec<Vec3>> = if seat.kotor_points.is_empty() {
            Some(vec![rig.rest[node].position])
        } else {
            seat.kotor_points
                .iter()
                .map(|name| rig.node(name).map(|n| rig.rest[n].position))
                .collect()
        };
        let (Some(target), Some(kotor_points)) = (ja.base_mean(ja_points), kotor_points) else {
            continue;
        };
        let from = kotor_points.iter().sum::<Vec3>() / kotor_points.len() as f32;
        let align = rig.mapped.get(&node).map_or(Quat::IDENTITY, |m| m.1);
        seats.insert(
            seat.kotor.to_ascii_lowercase(),
            (bone, align, TURN * target * scale - from),
        );
    }
    let posed = |ir: &ModelIr, root_rest: Frame, root_pose: Frame| {
        let mut rest: Vec<Frame> = Vec::with_capacity(ir.nodes.len());
        let mut pose: Vec<Frame> = Vec::with_capacity(ir.nodes.len());
        for node in &ir.nodes {
            let [x, y, z, w] = node.rotation;
            let local = Frame {
                rotation: Quat::from_xyzw(x, y, z, w).normalize(),
                position: Vec3::from_array(node.translation),
            };
            let (parent_rest, parent_pose) = node
                .parent
                .map_or((root_rest, root_pose), |p| (rest[p], pose[p]));
            let node_rest = parent_rest.then(local);
            let node_pose = match seats.get(&node.name.to_ascii_lowercase()) {
                Some(&(_, align, moved)) => Frame {
                    rotation: (align * node_rest.rotation).normalize(),
                    position: node_rest.position + moved,
                },
                None => parent_pose.then(local),
            };
            rest.push(node_rest);
            pose.push(node_pose);
        }
        (rest, pose)
    };
    let (body_rest, body_pose) = posed(body, Frame::IDENTITY, Frame::IDENTITY);
    let bones = gla.bones.len();
    let mut meshes = Vec::new();
    for (index, (ir, root, hook)) in parts.iter().enumerate() {
        let (rest, pose) = if index == 0 {
            (body_rest.clone(), body_pose.clone())
        } else {
            let root = Frame::of(root);
            let root_pose = hook.map_or(root, |h| {
                body_pose[h].then(body_rest[h].inverse().then(root))
            });
            posed(ir, root, root_pose)
        };
        // A node's JA bone: its own seat's, or its nearest seated ancestor's.
        let bone_of = |mut node: usize| loop {
            if let Some(&(bone, _, _)) = seats.get(&ir.nodes[node].name.to_ascii_lowercase()) {
                return Some(bone);
            }
            node = ir.nodes[node].parent?;
        };
        for mesh in &ir.meshes {
            let mesh_rest = rest[mesh.node];
            let mut positions = Vec::with_capacity(mesh.positions.len());
            let mut normals = Vec::with_capacity(mesh.positions.len());
            let mut vertex_joints = Vec::with_capacity(mesh.positions.len());
            let mut vertex_weights = Vec::with_capacity(mesh.positions.len());
            for (v, p) in mesh.positions.iter().enumerate() {
                let influences: Vec<(usize, f32)> = match &mesh.skin {
                    Some(skin) => (0..4)
                        .filter_map(|i| {
                            let weight = skin.vertex_weights.get(v)?[i];
                            let slot = usize::from(skin.vertex_joints.get(v)?[i]);
                            let joint = *skin.joints.get(slot)?;
                            (weight > 0.0).then_some((joint, weight))
                        })
                        .collect(),
                    None => vec![(mesh.node, 1.0)],
                };
                let total: f32 = influences.iter().map(|i| i.1).sum();
                let world = mesh_rest.point(Vec3::from_array(*p));
                let normal = mesh
                    .normals
                    .get(v)
                    .map_or(Vec3::Z, |n| mesh_rest.rotation * Vec3::from_array(*n));
                let (mut placed, mut turned) = (world, normal);
                let mut weights: BTreeMap<usize, f32> = BTreeMap::new();
                if total > 0.0 {
                    (placed, turned) = (Vec3::ZERO, Vec3::ZERO);
                    for &(node, weight) in &influences {
                        let share = weight / total;
                        let carry = pose[node].then(rest[node].inverse());
                        placed += carry.point(world) * share;
                        turned += carry.rotation * normal * share;
                        if let Some(bone) = bone_of(node) {
                            *weights.entry(bone).or_insert(0.0) += share;
                        }
                    }
                }
                positions.push((TURN.inverse() * placed / scale).to_array());
                normals.push((TURN.inverse() * turned).normalize_or(Vec3::Z).to_array());
                let mut strongest: Vec<(usize, f32)> = weights.into_iter().collect();
                strongest.sort_by(|a, b| b.1.total_cmp(&a.1));
                strongest.truncate(4);
                let sum: f32 = strongest.iter().map(|s| s.1).sum();
                let mut joints = [0u16; 4];
                let mut shares = [0f32; 4];
                for (i, &(bone, weight)) in strongest.iter().enumerate() {
                    joints[i] = bone as u16;
                    shares[i] = if sum > 0.0 { weight / sum } else { 0.0 };
                }
                vertex_joints.push(joints);
                vertex_weights.push(shares);
            }
            meshes.push(MeshIr {
                node: 0,
                positions,
                normals,
                skinned: true,
                skin: Some(SkinIr {
                    joints: (1..=bones).collect(),
                    inverse_binds: vec![IDENTITY; bones],
                    rest_inverse_binds: vec![IDENTITY; bones],
                    vertex_joints,
                    vertex_weights,
                }),
                ..mesh.clone()
            });
        }
    }
    meshes
}

/// World positions (model space, metres) of named nodes of a KOTOR model
/// playing `clip`, at each of the clip's key times, posed as the scene's
/// animator poses them (rest translation plus `animation scale × key`, the
/// key's rotation). Unknown names are skipped.
pub fn trace(model: &ModelIr, clip: &AnimationIr, names: &[&str]) -> Vec<(f32, Vec<Vec3>)> {
    let nodes: Vec<usize> = names
        .iter()
        .filter_map(|name| {
            model
                .nodes
                .iter()
                .position(|node| node.name.eq_ignore_ascii_case(name))
        })
        .collect();
    let track_of: Vec<Option<&NodeTrack>> = model
        .nodes
        .iter()
        .map(|node| {
            clip.tracks
                .iter()
                .find(|t| t.node.eq_ignore_ascii_case(&node.name))
        })
        .collect();
    let mut times: Vec<f32> = clip
        .tracks
        .iter()
        .flat_map(|t| {
            t.orientations
                .iter()
                .map(|k| k.0)
                .chain(t.positions.iter().map(|k| k.0))
        })
        .collect();
    times.sort_by(f32::total_cmp);
    times.dedup_by(|a, b| (*a - *b).abs() < 1e-5);
    times
        .into_iter()
        .map(|time| {
            let mut globals: Vec<Frame> = Vec::with_capacity(model.nodes.len());
            for (index, node) in model.nodes.iter().enumerate() {
                let [x, y, z, w] = node.rotation;
                let mut local = Frame {
                    rotation: Quat::from_xyzw(x, y, z, w).normalize(),
                    position: Vec3::from_array(node.translation),
                };
                if let Some(track) = track_of[index].filter(|_| node.animated) {
                    if let Some(p) = asset_odyssey::anim::sample_position(&track.positions, time) {
                        local.position += Vec3::from_array(p) * model.animation_scale;
                    }
                    if let Some([x, y, z, w]) =
                        asset_odyssey::anim::sample_orientation(&track.orientations, time)
                    {
                        local.rotation = Quat::from_xyzw(x, y, z, w);
                    }
                }
                let parent = node.parent.map_or(Frame::IDENTITY, |p| globals[p]);
                globals.push(parent.then(local));
            }
            (time, nodes.iter().map(|&n| globals[n].position).collect())
        })
        .collect()
}

/// Every node's world transform (model space) of a KOTOR model playing
/// `clip` at `time` seconds, posed as the scene's animator poses it.
pub fn pose_at(model: &ModelIr, clip: &AnimationIr, time: f32) -> Vec<Matrix> {
    let mut globals: Vec<Frame> = Vec::with_capacity(model.nodes.len());
    for node in &model.nodes {
        let [x, y, z, w] = node.rotation;
        let mut local = Frame {
            rotation: Quat::from_xyzw(x, y, z, w).normalize(),
            position: Vec3::from_array(node.translation),
        };
        let track = clip
            .tracks
            .iter()
            .find(|t| t.node.eq_ignore_ascii_case(&node.name));
        if let Some(track) = track.filter(|_| node.animated) {
            if let Some(p) = asset_odyssey::anim::sample_position(&track.positions, time) {
                local.position += Vec3::from_array(p) * model.animation_scale;
            }
            if let Some([x, y, z, w]) =
                asset_odyssey::anim::sample_orientation(&track.orientations, time)
            {
                local.rotation = Quat::from_xyzw(x, y, z, w);
            }
        }
        let parent = node.parent.map_or(Frame::IDENTITY, |p| globals[p]);
        globals.push(parent.then(local));
    }
    globals.into_iter().map(Frame::matrix).collect()
}
