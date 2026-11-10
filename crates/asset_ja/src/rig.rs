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
//! and feet, which both skeletons hold upright or flat). KOTOR's two spine
//! nodes above the pelvis take their turn from JA's spine by height instead
//! (`spans`, `Curve::turn`): its joints sit a joint lower than JA's bones
//! named alike. Unmapped nodes keep
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

/// JA's spine from the hips up: each joint (the hips' middle, then the
/// lumbar and thoracic joints to the neck) and the bone carrying the stretch
/// above it. KOTOR's spine follows it by height (`Span`).
const JA_SPINE: &[(&[&str], &str)] = &[
    (&["rfemurYZ", "lfemurYZ"], "pelvis"),
    (&["lower_lumbar"], "lower_lumbar"),
    (&["upper_lumbar"], "upper_lumbar"),
    (&["thoracic"], "thoracic"),
    (&["cervical"], "cervical"),
];

/// KOTOR's spine nodes above the pelvis, bottom up, and the node that ends
/// the last one.
const KOTOR_SPINE: (&[&str], &str) = (&["torso_g", "torsoupr_g"], "neck_g");

/// JA's spine as the KOTOR spine follows it: per joint its bones (averaged)
/// and its height along the spine in the base pose (0 at the hips, 1 at the
/// neck), and per stretch between two joints the bone that carries it.
struct Curve {
    joints: Vec<(Vec<usize>, f32)>,
    bones: Vec<usize>,
}

impl Curve {
    fn new(ja: &Ja) -> Option<Curve> {
        let mut joints = Vec::with_capacity(JA_SPINE.len());
        let mut bones = Vec::with_capacity(JA_SPINE.len());
        for &(names, bone) in JA_SPINE {
            let at: Option<Vec<usize>> = names.iter().map(|name| ja.bone(name)).collect();
            let at = at?;
            let z = ja.base_mean(names)?.z;
            joints.push((at, z));
            bones.push(ja.bone(bone)?);
        }
        let (low, high) = (joints.first()?.1, joints.last()?.1);
        if high - low < 0.1 {
            return None;
        }
        for joint in &mut joints {
            joint.1 = (joint.1 - low) / (high - low);
        }
        bones.pop();
        Some(Curve { joints, bones })
    }

    /// Each joint's place (KOTOR model space) in a pose.
    fn points(&self, ja: &Ja, finals: &[Frame]) -> Vec<Vec3> {
        self.joints
            .iter()
            .map(|(bones, _)| {
                bones
                    .iter()
                    .map(|&b| TURN * finals[b].point(ja.base(b)))
                    .sum::<Vec3>()
                    / bones.len() as f32
            })
            .collect()
    }

    /// The place `t` of the way up the spine, between the joints around it.
    fn at(&self, points: &[Vec3], t: f32) -> Vec3 {
        let last = self.joints.len() - 1;
        let i = (1..last)
            .take_while(|&i| self.joints[i].1 <= t)
            .last()
            .unwrap_or(0);
        let (a, b) = (self.joints[i].1, self.joints[i + 1].1);
        let share = ((t - a) / (b - a).max(f32::EPSILON)).clamp(0.0, 1.0);
        points[i].lerp(points[i + 1], share)
    }

    /// How a stretch of the spine has turned from the base pose: the turns
    /// of the JA bones it overlaps, weighed by the overlap, then swung so
    /// the stretch points where JA's curve does over the same heights.
    fn turn(&self, ja: &Ja, finals: &[Frame], span: &Span) -> Quat {
        let mut mixed = Mix::default();
        for (i, &bone) in self.bones.iter().enumerate() {
            let (a, b) = (self.joints[i].1, self.joints[i + 1].1);
            let overlap = span.to.min(b) - span.from.max(a);
            if overlap > 0.0 {
                mixed.add(TURN * finals[bone].rotation * TURN.inverse(), overlap);
            }
        }
        let twist = mixed.quat();
        let points = self.points(ja, finals);
        match (self.at(&points, span.to) - self.at(&points, span.from)).try_normalize() {
            Some(pose) => Quat::from_rotation_arc(twist * span.base, pose) * twist,
            None => twist,
        }
    }
}

/// A weighted mean of turns (each kept on the first one's side).
#[derive(Default)]
struct Mix(Option<Quat>, glam::Vec4);

impl Mix {
    fn add(&mut self, q: Quat, weight: f32) {
        let first = *self.0.get_or_insert(q);
        let q = if first.dot(q) < 0.0 { -q } else { q };
        self.1 += glam::Vec4::from(q) * weight;
    }

    fn quat(&self) -> Quat {
        match self.1.length() > f32::EPSILON {
            true => Quat::from_vec4(self.1.normalize()),
            false => Quat::IDENTITY,
        }
    }
}

/// The heights (0 at the hips, 1 at the neck) a KOTOR spine node spans, and
/// JA's curve's direction over them in the base pose.
struct Span {
    from: f32,
    to: f32,
    base: Vec3,
}

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
    /// JA's spine curve and the KOTOR spine nodes that follow it by height.
    spine: Option<(Curve, BTreeMap<usize, Span>)>,
    /// JA's lengths to KOTOR's (`pelvis_scale`).
    scale: f32,
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
                .filter(|_| !(pair.kotor.ends_with("collar_g") && off_spine(rest[k].position)))
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
        let spine = Curve::new(ja).and_then(|curve| {
            let spans = spans(&rest, &node, ja, &curve)?;
            Some((curve, spans))
        });
        let mut kotor = Kotor {
            ir,
            rest,
            local,
            mapped,
            hinged,
            spine,
            scale: 1.0,
        };
        kotor.scale = pelvis_scale(ja, &kotor);
        kotor
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
            // A hilt JA's tag bone carries away from its hand (a toss, the
            // protect's orbit, a taunt's flourish) goes with it, scaled.
            let carried = grips
                .iter()
                .find(|g| g.weapon == index)
                .map(|grip| (grip, grip.departure(finals) * self.scale));
            if let Some((_, away)) = carried {
                local.position += parent.rotation.inverse() * away;
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
                    None => match self
                        .spine
                        .as_ref()
                        .and_then(|(c, s)| Some((c, s.get(&index)?)))
                    {
                        Some((curve, span)) => curve.turn(ja, finals, span),
                        None => TURN * finals[bone].rotation * TURN.inverse(),
                    },
                };
                global.rotation = (turn * align * self.rest[index].rotation).normalize();
            }
            if let Some(grip) = grips.iter().find(|g| g.hand == index) {
                // Turned so the hilt in its weapon node points as JA's does;
                // as the hilt leaves the hand, the hand goes back to JA's arm.
                let held =
                    (grip.held(finals) * self.local[grip.weapon].rotation.inverse()).normalize();
                let gone = grip.gone(finals);
                global.rotation = held.slerp(global.rotation, gone).normalize();
            }
            if let Some((grip, away)) = carried
                && away.length() > 0.0
            {
                global.rotation = grip.held(finals);
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

/// Where each KOTOR spine node sits along its spine (0 at the hips' middle,
/// 1 at the neck), so it follows JA's spine over the same heights: K1's
/// `torso_g` starts where JA's `lower_lumbar` does and `torsoupr_g` where
/// `upper_lumbar` does, so taking the turns of the bones named alike
/// (`upper_lumbar`, `thoracic`) bent each a joint too high and hunched the
/// body. None without the thighs, every spine node or the neck.
fn spans(
    rest: &[Frame],
    node: &dyn Fn(&str) -> Option<usize>,
    ja: &Ja,
    curve: &Curve,
) -> Option<BTreeMap<usize, Span>> {
    let (nodes, end) = KOTOR_SPINE;
    let hips = (rest[node("rthigh_g")?].position.z + rest[node("lthigh_g")?].position.z) / 2.0;
    let neck = rest[node(end)?].position.z;
    if neck - hips < 0.1 {
        return None;
    }
    let height = |z: f32| ((z - hips) / (neck - hips)).clamp(0.0, 1.0);
    let base = curve.points(ja, &vec![Frame::IDENTITY; ja.gla.bones.len()]);
    let mut spans = BTreeMap::new();
    for (i, name) in nodes.iter().enumerate() {
        let n = node(name)?;
        let next = match nodes.get(i + 1) {
            Some(next) => rest[node(next)?].position.z,
            None => neck,
        };
        let (from, to) = (height(rest[n].position.z), height(next));
        let base = (curve.at(&base, to) - curve.at(&base, from)).try_normalize()?;
        spans.insert(n, Span { from, to, base });
    }
    Some(spans)
}

/// How far from the body's middle (metres, across) a collar may start and
/// still be lined up with JA's clavicle.
const OFF_SPINE: f32 = 0.03;

/// Whether a collar starts off the spine: K1's female skeleton's (every
/// woman's body, Bastila's too) starts 7.7 cm out and slopes 35° down to
/// the shoulder; JA's clavicle and K1's male collar start on the spine,
/// level. Lined up with JA's, such a collar turns level and lifts the
/// shoulder 3 to 7 cm into the neck, so it keeps its own rest under JA's
/// base pose and takes JA's turns from there.
fn off_spine(rest: Vec3) -> bool {
    rest.x.abs() > OFF_SPINE
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
    humanoid_names(kotor.nodes.iter().map(|node| node.name.as_str()))
}

/// `is_humanoid` from a model's node names alone (its MDL's name table,
/// `fmt_odyssey::mdl::Model::node_names`), for what reads no whole model.
pub fn humanoid_names<'a>(names: impl Iterator<Item = &'a str> + Clone) -> bool {
    HUMANOID
        .iter()
        .filter(|pair| !matches!(pair.kotor, "neck_g" | "head_g"))
        .all(|pair| {
            names
                .clone()
                .any(|name| name.eq_ignore_ascii_case(pair.kotor))
        })
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
        for (node, place) in posed.weapons {
            tracks
                .entry(node)
                .or_insert_with(|| track(&kotor.nodes[node].name))
                .positions
                .push((time, place.to_array()));
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
    /// Each gripping hand's weapon node's key, as the root's: none under
    /// the hand, or off it where JA's tag bone carries the hilt away.
    pub weapons: Vec<(usize, Vec3)>,
    /// JA's own joints in `JA_ARMS` order (KOTOR model space, metres, JA's
    /// floor on the model's), for traces beside the drawn arms.
    pub ja_arms: Vec<Vec3>,
}

/// JA's arm joints a `Posed` carries: each side's shoulder, elbow and wrist.
pub const JA_ARMS: [&str; 6] = [
    "rhumerus", "rradius", "rhand", "lhumerus", "lradius", "lhand",
];

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
    /// JA's `JA_ARMS` bones.
    arm_bones: Vec<usize>,
    /// KOTOR's hip joints (`lthigh_g`, `rthigh_g`): the pelvis's pivot.
    hips: Option<[usize; 2]>,
    /// A woman's body moves as K1's women do (`Woman`).
    woman: Option<Woman>,
}

/// The nodes K1's women's style moves (`Retargeter::cocked`,
/// `Retargeter::tucked`). K1 has no female attacks, only a female ready
/// stance, but its women's own clips (`g2r1`, `walk`, `pause1`) against its
/// men's stand with the feet 8 to 10 cm nearer together, the elbows 3 to
/// 5 cm nearer the body, the hips cocked about 8° over the leg that bears
/// the weight, and swinging 16° in the walk (a man's 5°). JA's moves on the
/// female skeleton keep a man's width but for 1 to 2 cm, so a woman's body
/// takes these on top of them, view only.
#[derive(Debug, Clone, Copy)]
struct Woman {
    pelvis: usize,
    /// Left, right: thigh, shin, foot.
    legs: [[usize; 3]; 2],
    /// The spine's line, low to high (`torso_g`, `torsoupr_g`).
    spine: [usize; 2],
}

impl Woman {
    /// K1's female skeleton (its collars start off the spine, `off_spine`).
    fn of(k: &Kotor) -> Option<Woman> {
        let collar = k.node("lcollar_g")?;
        if !off_spine(k.rest[collar].position) {
            return None;
        }
        let leg = |s: &str| {
            let [thigh, shin, foot] =
                ["thigh_g", "shin_g", "foot_g"].map(|part| k.node(&format!("{s}{part}")));
            Some([thigh?, shin?, foot?])
        };
        Some(Woman {
            pelvis: k.node("pelvis_g")?,
            legs: [leg("l")?, leg("r")?],
            spine: [k.node("torso_g")?, k.node("torsoupr_g")?],
        })
    }
}

/// `Retargeter::cocked`'s pose: the travel moving the hips' middle back
/// where it was, the pelvis's turn (global), and each leg's ankle's place.
struct Cock {
    travel: Vec3,
    pelvis: (usize, Quat),
    ankles: [([usize; 3], Vec3); 2],
}

/// The hips' cock over the leg bearing the weight (radians).
const HIP_COCK: f32 = 8.0 * std::f32::consts::PI / 180.0;
/// The most the hips roll with the cock added (radians); JA's own roll past
/// it stays.
const HIP_MOST: f32 = 12.0 * std::f32::consts::PI / 180.0;
/// How much nearer under the hips (metres) one foot is than the other for
/// the cock to be about three quarters over it.
const WEIGHT_SPREAD: f32 = 0.06;
/// A lifted foot bears less: its height (metres) counts this many times
/// as distance out from under the hips.
const LIFT_WEIGHS: f32 = 3.0;
/// The cock fades out as the lower foot leaves the ground (metres): whole
/// up to the first, none from the second.
const GROUNDED: (f32, f32) = (0.05, 0.15);
/// How far each foot comes in (metres), and at most what share of its
/// distance out from under the hips.
const NARROW: (f32, f32) = (0.035, 0.25);
/// How far each elbow comes in toward the spine (metres), never nearer it
/// than the second.
const ELBOW_IN: (f32, f32) = (0.035, 0.13);
/// The most the arm turns about its shoulder-wrist line to tuck the elbow
/// (radians): past it the forearm's roll against the hand shows.
const ELBOW_TURN_MOST: f32 = 0.35;

impl<'a> Retargeter<'a> {
    /// `options.ground` is not used: `pose` takes the travel.
    pub fn new(kotor: &'a ModelIr, gla: &'a Gla, options: &Options<'a>) -> Retargeter<'a> {
        let ja = Ja::new(gla);
        let k = Kotor::new(kotor, &ja);
        let scale = k.scale;
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
        let arm_bones = JA_ARMS.iter().filter_map(|name| ja.bone(name)).collect();
        let hips = k
            .node("lthigh_g")
            .zip(k.node("rthigh_g"))
            .map(<[usize; 2]>::from);
        let woman = Woman::of(&k);
        Retargeter {
            woman,
            hips,
            arms,
            arm_bones,
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
        let (near, far) = TWO_HANDED;
        Some(smooth(far, near, self.apart(finals)?))
    }

    /// How far apart (metres) JA's pose (`finals`) has its hands.
    fn apart(&self, finals: &[Frame]) -> Option<f32> {
        let [left, right] = self.hands?;
        let at = |bone: usize| finals[bone].point(self.ja.base(bone));
        Some(at(left).distance(at(right)))
    }

    /// The left arm's turns (globals) putting its wrist where JA's is from
    /// the right wrist, as far as JA's hands are together (`off_hand`).
    fn spaced(&self, finals: &[Frame], globals: &[Frame]) -> BTreeMap<usize, Quat> {
        let (Some(arms), Some([left, right]), Some(near)) =
            (self.arms, self.hands, self.off_hand(finals))
        else {
            return BTreeMap::new();
        };
        let Some(([upper, fore, hand], _)) = arms.left.filter(|_| near > 0.0) else {
            return BTreeMap::new();
        };
        let at = |b: usize| TURN * finals[b].point(self.ja.base(b));
        let wrist = globals[hand].position;
        let wanted = globals[arms.right[2]].position + (at(left) - at(right));
        bend(globals, [upper, fore], wrist, wrist.lerp(wanted, near))
            .map_or_else(BTreeMap::new, BTreeMap::from)
    }

    /// A woman's hips cocked over the leg that bears the weight (`HIP_COCK`)
    /// about their middle, and where each leg then bends its ankle: where
    /// it was but nearer the other (`NARROW`). The weight is on the foot
    /// nearer under the hips and lower; standing it cocks the hips, walking
    /// it swings them as the weight changes feet.
    fn cocked(&self, globals: &[Frame], travel: Vec3, ground: Vec3) -> Option<Cock> {
        let woman = self.woman?;
        let [left, right] = woman.legs;
        let hip = |leg: [usize; 3]| globals[leg[0]].position;
        let ankle = |leg: [usize; 3]| globals[leg[2]].position;
        let middle = (hip(left) + hip(right)) / 2.0;
        let across = hip(right) - hip(left);
        let side = across.with_z(0.0).try_normalize()?;
        let out = |leg: [usize; 3], way: f32| way * (ankle(leg) - middle).dot(side);
        let lift =
            |leg: [usize; 3]| (ankle(leg).z - self.k.rest[leg[2]].position.z - ground.z).max(0.0);
        let bears = |leg: [usize; 3], way: f32| out(leg, way).max(0.0) + LIFT_WEIGHS * lift(leg);
        // Above 0: the right foot bears the weight, its hip goes up.
        let weight = ((bears(left, -1.0) - bears(right, 1.0)) / WEIGHT_SPREAD).tanh();
        let grounded = smooth(GROUNDED.1, GROUNDED.0, lift(left).min(lift(right)));
        let roll = (across.z / across.length().max(f32::EPSILON))
            .clamp(-1.0, 1.0)
            .asin();
        let most = HIP_MOST.max(roll.abs());
        let added = (roll + HIP_COCK * weight * grounded).clamp(-most, most) - roll;
        let turn = Quat::from_axis_angle(side.cross(Vec3::Z), added);
        let pelvis = globals[woman.pelvis].position;
        let moved = travel + middle + turn * (pelvis - middle) - pelvis;
        let ankles = [(left, -1.0), (right, 1.0)].map(|(leg, way)| {
            let narrow = (out(leg, way) * NARROW.1).clamp(0.0, NARROW.0);
            (leg, ankle(leg) - side * way * narrow)
        });
        Some(Cock {
            travel: moved,
            pelvis: (
                woman.pelvis,
                (turn * globals[woman.pelvis].rotation).normalize(),
            ),
            ankles,
        })
    }

    /// A woman's elbows turned in toward the spine (`ELBOW_IN`) about each
    /// arm's shoulder-wrist line, so the wrist and the hand stay: the arms'
    /// turns (globals).
    fn tucked(&self, globals: &[Frame]) -> Vec<(usize, Quat)> {
        let (Some(woman), Some(arms)) = (self.woman, self.arms) else {
            return Vec::new();
        };
        let [low, high] = woman.spine.map(|n| globals[n].position);
        let Some(spine) = (high - low).try_normalize() else {
            return Vec::new();
        };
        [Some(arms.right), arms.left.map(|(left, _)| left)]
            .into_iter()
            .flatten()
            .filter_map(|[upper, fore, hand]| {
                let (shoulder, elbow) = (globals[upper].position, globals[fore].position);
                let line = (globals[hand].position - shoulder).try_normalize()?;
                let square = |v: Vec3| v - line * v.dot(line);
                let bent = square(elbow - shoulder);
                let inward = low + spine * (elbow - low).dot(spine) - elbow;
                let toward = square(inward);
                let (reach, along) = (bent.length(), toward.length());
                if reach < 1e-3 || along < 1e-3 {
                    return None;
                }
                let cos = bent.dot(toward) / (reach * along);
                let apart = line.dot(bent.cross(toward)).atan2(bent.dot(toward));
                // Faded as the arm straightens (no elbow to tuck) and as the
                // elbow points away from the spine (which way is in flips).
                let wanted = (inward.length() - ELBOW_IN.1).clamp(0.0, ELBOW_IN.0)
                    * smooth(0.01, 0.05, reach)
                    * smooth(-0.9, -0.3, cos);
                let step = (wanted / reach).min(apart.abs()).min(ELBOW_TURN_MOST);
                let turn = Quat::from_axis_angle(line, step * apart.signum());
                Some([
                    (upper, (turn * globals[upper].rotation).normalize()),
                    (fore, (turn * globals[fore].rotation).normalize()),
                ])
            })
            .flatten()
            .collect()
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
        // JA's pelvis turns about the hip joints; KOTOR's `pelvis_g` sits 13
        // (a woman's) to 19 cm (a man's) above them, so the same tilt swings
        // KOTOR's hips, and the feet, sideways. The root moves across so the
        // hips' middle goes where JA's travel puts the pelvis.
        if let Some([left, right]) = self.hips {
            let rest = (k.rest[left].position + k.rest[right].position) / 2.0;
            let now = (globals[left].position + globals[right].position) / 2.0;
            let off = rest + travel - now;
            travel += Vec3::new(off.x, off.y, 0.0);
            globals = k.globals(ja, finals, root, travel, grips, hold, &none);
        }
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
        // A woman's hips cock over the leg bearing the weight, the feet a
        // little nearer together.
        let mut turned = BTreeMap::new();
        if let Some(Cock {
            travel: moved,
            pelvis,
            ankles,
        }) = self.cocked(&globals, travel, ground)
        {
            travel = moved;
            turned.insert(pelvis.0, pelvis.1);
            globals = k.globals(ja, finals, root, travel, grips, hold, &turned);
            for ([thigh, shin, foot], wanted) in ankles {
                turned.extend(
                    bend(&globals, [thigh, shin], globals[foot].position, wanted)
                        .into_iter()
                        .flatten(),
                );
            }
            globals = k.globals(ja, finals, root, travel, grips, hold, &turned);
        }
        // With JA's hands together (a two-handed hold), the left wrist keeps
        // JA's own spacing from the right one, not the spacing KOTOR's longer
        // arms or a woman's collars give it.
        turned.extend(self.spaced(finals, &globals));
        if !turned.is_empty() {
            globals = k.globals(ja, finals, root, travel, grips, hold, &turned);
        }
        let as_ja = globals.clone();
        turned.extend(
            reach
                .iter()
                .filter_map(|reach| Some((grips.get(reach.grip)?, reach)))
                .flat_map(|(grip, reach)| reach_turns(&globals, grip, reach)),
        );
        if !turned.is_empty() {
            globals = k.globals(ja, finals, root, travel, grips, hold, &turned);
        }
        // One hilt in the right hand: the left hand holds it too where JA's
        // does, letting go as the hilt goes where the KOTOR body puts it
        // (`Reach::free`).
        let left = self
            .arms
            .and_then(|arms| Some((arms.left?.0, arms.left_weapon?)));
        let held = match (grips.as_slice(), left, self.apart(finals)) {
            ([grip], Some(left), Some(apart)) if grip.staff => {
                let near = smooth(STAFF_HANDS.1, STAFF_HANDS.0, apart);
                staff_hold((&as_ja, &globals), grip, left, near)
            }
            ([grip], Some(left), Some(apart)) => {
                let near = smooth(TWO_HANDED.1, TWO_HANDED.0, apart);
                off_hand_hold((&as_ja, &globals), grip, left, near)
            }
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
        // A woman's elbows nearer her body, the wrists where they were.
        let tucks = self.tucked(&globals);
        if !tucks.is_empty() {
            turned.extend(tucks);
            globals = k.globals(ja, finals, root, travel, grips, hold, &turned);
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
        let floor = Vec3::Z * crate::ORIGIN_HEIGHT;
        let ja_arms = self
            .arm_bones
            .iter()
            .map(|&b| TURN * (finals[b].point(ja.base(b)) + floor) + ground)
            .collect();
        let weapons = grips
            .iter()
            .map(|grip| {
                let local = parent_of(grip.weapon).inverse().then(globals[grip.weapon]);
                let moved = local.position - k.local[grip.weapon].position;
                (grip.weapon, moved / kotor.animation_scale.max(f32::EPSILON))
            })
            .collect();
        Posed {
            rotations,
            root,
            weapons,
            ja_arms,
        }
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
    /// A double-bladed hilt (`Grip::staff`): the left hand takes it beside
    /// the right wherever JA's takes JA's longer one.
    staff: bool,
    /// JA's hand bone on the same side, which carries the tag bone, and
    /// where the bolt sits in JA's base pose (metres).
    ja_hand: Option<usize>,
    bolt_at: Vec3,
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
    /// How much the hand turns the blade onto `direction` (0 to 1): at 0 the
    /// blade keeps the drawn pose's own turn, at 1 it points the sim's way.
    pub turn: f32,
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
/// there (a first-person eye's gun, held up before it, its kick up turning
/// the gun about the wrist); with `support`, the left palm under the gun
/// there, its fingers round the far side.
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
        // The muzzle as the hand turned but for the kick carries it, moved
        // onto the hold: the kick lifts it from there.
        Some(hold) => {
            let steady = basis(direction, Vec3::Z).map_or(turn, |wanted| {
                Quat::IDENTITY.slerp(wanted * now.inverse(), aim.weight)
            });
            let carried = steady * (muzzle - wrist);
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
/// From how far (metres) JA's tag bone has carried the hilt off its hand
/// the hilt starts to leave the KOTOR hand, and by how far it has wholly:
/// JA's dual and staff swings shift it 5 to 15 cm, which KOTOR's own grip
/// keeps in the hand; its throws and orbits carry it 0.6 to 2.7 m.
const DEPART: (f32, f32) = (0.12, 0.35);
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

/// How far apart (metres) JA's hands are when both hold its staff (0.35
/// to 0.43 in its attacks), and from how far apart the left has let go (its
/// one-handed stance and spins hold them 1 to 1.2 m apart).
const STAFF_HANDS: (f32, f32) = (0.45, 0.6);
/// How far along a K1 double-bladed hilt from its middle (metres) the left
/// fist may sit: the blades start 18.5 cm out, and a fist is `FIST` wide.
const STAFF_SPAN: (f32, f32) = (FIST, 0.185 - FIST / 2.0);
/// How far off the staff's line (metres) the pose may have the left hand
/// for it to take the staff fully, and from how far it does not.
const STAFF_LINE: (f32, f32) = (0.15, 0.3);

/// `off_hand_hold` for a double-bladed hilt held by its middle: K1's is
/// shorter than JA's (its blades start 0.37 m apart, JA's 0.58), so where
/// JA's hands hold JA's 35 to 43 cm apart the left hand takes K1's on the
/// same side of the right fist, as near the far blade as a fist goes
/// (`STAFF_SPAN`), as far as JA's hands are together (`near`) and the pose
/// has the left hand along the staff's line (`STAFF_LINE`); turned with
/// its own hilt along the staff, the way round nearer the pose's.
fn staff_hold(
    (as_ja, globals): (&[Frame], &[Frame]),
    grip: &Grip,
    ([_, _, hand], weapon): ([usize; 3], usize),
    near: f32,
) -> Option<(f32, Frame)> {
    let (was, hilt) = (as_ja[grip.weapon], globals[grip.weapon]);
    let posed = was.inverse().then(as_ja[weapon]);
    let way = posed.rotation * Vec3::Z;
    let along = Quat::from_rotation_arc(way, Vec3::Z * way.z.signum());
    let side = posed.position.z.signum();
    let at = posed.position.z.abs().clamp(STAFF_SPAN.0, STAFF_SPAN.1) * side;
    let held = hilt.then(Frame {
        rotation: (along * posed.rotation).normalize(),
        position: Vec3::Z * at,
    });
    let palm = globals[hand].inverse().then(globals[weapon]);
    let wanted = held.then(palm.inverse());
    let off_line = posed.position.truncate().length();
    let fit = near * smooth(STAFF_LINE.1, STAFF_LINE.0, off_line);
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
    let mut aim = Quat::from_rotation_arc(along, reach.direction.normalize_or_zero());
    // A full turn is the arc itself, untouched by the blend.
    if reach.turn < 1.0 {
        aim = Quat::IDENTITY.slerp(aim, reach.turn.max(0.0));
    }
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
        let ja_hand = ja.bone(&format!("{side}hand"));
        Ok(Grip {
            staff: false,
            ja_hand,
            bolt_at: Frame::of(&bolt.matrix).position,
            hand,
            weapon,
            bone: bolt.bone,
            bolt: bolt_rotation,
            hilt,
            arm: arm[0].zip(arm[1]).map(|(upper, fore)| [upper, fore]),
        })
    }

    /// The grip of a double-bladed hilt, held by its middle.
    pub fn staff(self) -> Grip {
        Grip {
            staff: true,
            ..self
        }
    }

    /// The weapon node's turn (KOTOR model space) that points the hilt as
    /// JA's tag bone points JA's.
    fn held(&self, finals: &[Frame]) -> Quat {
        (TURN * finals[self.bone].rotation * self.bolt * self.hilt.inverse()).normalize()
    }

    /// How far JA's tag bone has carried the hilt from its hand (KOTOR
    /// model space, JA's metres): the bolt's place against where the hand
    /// would carry it from the base pose. JA's hilts ride their hand but for a few moves that throw
    /// or orbit the saber (`BOTH_VICTORY_STAFF`'s toss,
    /// `BOTH_A6_SABERPROTECT`, the taunts and show-offs); a shift under
    /// `DEPART.0` counts as none.
    fn departure(&self, finals: &[Frame]) -> Vec3 {
        let off = self.off(finals);
        off * smooth(DEPART.0, DEPART.1, off.length())
    }

    /// How far (0 to 1) the hilt has left the hand.
    fn gone(&self, finals: &[Frame]) -> f32 {
        smooth(DEPART.0, DEPART.1, self.off(finals).length())
    }

    /// The bolt's place against where JA's hand would carry it.
    fn off(&self, finals: &[Frame]) -> Vec3 {
        self.ja_hand.map_or(Vec3::ZERO, |hand| {
            TURN * (finals[self.bone].point(self.bolt_at) - finals[hand].point(self.bolt_at))
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
