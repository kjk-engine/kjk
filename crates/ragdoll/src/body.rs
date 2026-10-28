//! A ragdoll's shape: which nodes of a skeleton become bodies, the capsules
//! that fill them and the joints between them, fitted to one skeleton and
//! its skin in their rest pose.

use glam::{Quat, Vec3};

use crate::Frame;

/// A skeleton in its rest pose (model space, +Z up).
pub struct Skeleton<'a> {
    pub nodes: &'a [Node],
    /// The skin's vertices (model space, rest pose), each with the node
    /// that moves it most.
    pub skin: &'a [(Vec3, usize)],
}

pub struct Node {
    pub name: String,
    pub parent: Option<usize>,
    pub rest: Frame,
}

/// Where a part belongs: parts of one ragdoll touch each other only across
/// groups, never the core (which the limbs hang on and overlap) and never
/// their own parent or children.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Group {
    Core,
    Head,
    ArmLeft,
    ArmRight,
    LegLeft,
    LegRight,
}

impl Group {
    fn touches(self, other: Group) -> bool {
        self != other && self != Group::Core && other != Group::Core
    }

    /// Which way is out for this group's side (body axes: x right).
    fn out(self) -> f32 {
        match self {
            Group::ArmLeft | Group::LegLeft => -1.0,
            Group::ArmRight | Group::LegRight => 1.0,
            Group::Core | Group::Head => 0.0,
        }
    }
}

/// What a body is made of: how heavy, how far its joints give and how
/// stiffly, how it meets the floor. A droid's metal barely bends at its
/// joints, and its servos lock where they stop.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Material {
    Flesh,
    Metal,
}

impl Material {
    /// kg/m³, for bodies fitted without a plan's masses.
    pub(crate) fn density(self) -> f32 {
        match self {
            Material::Flesh => 1000.0,
            Material::Metal => 1800.0,
        }
    }

    /// A fitted joint's swing and twist (degrees).
    pub(crate) fn give(self) -> (f32, f32) {
        match self {
            Material::Flesh => (45.0, 30.0),
            Material::Metal => (12.0, 5.0),
        }
    }

    /// The share of a joint's relative spin its friction takes each step,
    /// and the spin (rad/s) below which it holds fast.
    pub(crate) fn joint_friction(self) -> (f32, f32) {
        match self {
            Material::Flesh => (0.032, 0.6),
            Material::Metal => (0.2, 2.5),
        }
    }

    /// Friction and bounce against the level and other bodies.
    pub(crate) fn surface(self) -> (f32, f32) {
        match self {
            Material::Flesh => (0.85, 0.05),
            Material::Metal => (0.5, 0.2),
        }
    }
}

/// How to make a ragdoll of a kind of skeleton.
pub struct Plan {
    pub parts: &'static [PartPlan],
    /// The whole body's mass (kg).
    pub mass: f32,
    /// The nodes of the left and right hips: they say which way is right
    /// (and so forward) in the model.
    pub hips: (&'static str, &'static str),
    /// Nodes a skeleton may lack, each with the node that stands in for
    /// it there.
    pub stand_ins: &'static [(&'static str, &'static str)],
}

#[derive(Debug, Clone, Copy)]
pub struct PartPlan {
    pub name: &'static str,
    /// The skeleton node it moves.
    pub node: &'static str,
    /// The part it hangs on, by name.
    pub parent: Option<&'static str>,
    pub shape: Shape,
    pub joint: JointPlan,
    /// Its share of the body's mass.
    pub mass: f32,
    /// The capsule's radius at least and at most (metres); the most when
    /// the skin has nothing on the part.
    pub thickness: [f32; 2],
    pub group: Group,
}

/// The capsule a part is filled with, and the way its bone runs (which
/// its joint swings and bends about).
#[derive(Debug, Clone, Copy)]
pub enum Shape {
    /// From its node to another, `past` metres beyond it.
    To { node: &'static str, past: f32 },
    /// Across, between two nodes, its bone running toward `aim`.
    Between {
        ends: (&'static str, &'static str),
        aim: &'static str,
    },
    /// From its node along a way of the body (`Way`), as far as its skin
    /// reaches, at least `length` metres.
    Along { way: Way, length: f32 },
}

#[derive(Debug, Clone, Copy)]
pub enum Way {
    Forward,
    Up,
}

/// What a part's joint allows (degrees). Leans and bends are given in body
/// axes: x out (to the part's own side), y forward, z up.
#[derive(Debug, Clone, Copy)]
pub enum JointPlan {
    /// Hangs on nothing.
    Root,
    /// Swings within `cone` of its rest way leaned toward `lean` (none:
    /// the rest way itself), twists within ±`twist`.
    Ball {
        cone: f32,
        twist: f32,
        lean: Option<[f32; 3]>,
    },
    /// Bends one way only, from straight up to `max`: its bone swings
    /// toward `toward`.
    Hinge { toward: [f32; 3], max: f32 },
}

/// One body of a ragdoll.
#[derive(Debug, Clone)]
pub struct Part {
    pub name: &'static str,
    pub node: usize,
    pub parent: Option<usize>,
    /// Its node's rest place (model space): the body's own frame.
    pub rest: Frame,
    /// Its shape, in the body's frame.
    pub solid: Solid,
    /// A share of the whole: the body's mass is the template's times this.
    pub mass: f32,
    pub joint: Option<Joint>,
    pub group: Group,
}

/// A body's shape in its own frame: a capsule (flesh, limbs), or a box
/// (a droid's plates and housings, which a capsule would let roll).
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Solid {
    Capsule {
        ends: [Vec3; 2],
        radius: f32,
    },
    Cuboid {
        centre: Vec3,
        turn: Quat,
        half: Vec3,
    },
}

impl Solid {
    pub fn centre(&self) -> Vec3 {
        match *self {
            Solid::Capsule { ends: [a, b], .. } => (a + b) * 0.5,
            Solid::Cuboid { centre, .. } => centre,
        }
    }

    /// The way it is longest.
    pub fn axis(&self) -> Vec3 {
        match *self {
            Solid::Capsule { ends: [a, b], .. } => (b - a).try_normalize().unwrap_or(Vec3::Z),
            Solid::Cuboid { turn, half, .. } => {
                let longest = if half.x >= half.y && half.x >= half.z {
                    Vec3::X
                } else if half.y >= half.z {
                    Vec3::Y
                } else {
                    Vec3::Z
                };
                turn * longest
            }
        }
    }

    pub fn volume(&self) -> f32 {
        match *self {
            Solid::Capsule {
                ends: [a, b],
                radius,
            } => std::f32::consts::PI * radius * radius * (a.distance(b) + 4.0 / 3.0 * radius),
            Solid::Cuboid { half, .. } => 8.0 * half.x * half.y * half.z,
        }
    }

    /// The same shape in a frame where this one's frame is `place`.
    pub(crate) fn moved(self, place: Frame) -> Solid {
        match self {
            Solid::Capsule { ends, radius } => Solid::Capsule {
                ends: ends.map(|end| place.point(end)),
                radius,
            },
            Solid::Cuboid { centre, turn, half } => Solid::Cuboid {
                centre: place.point(centre),
                turn: place.rotation * turn,
                half,
            },
        }
    }
}

/// A part's joint to its parent: its frame on each body (they meet at
/// rest, but for a ball's lean) with X along the swing's centre or the
/// hinge's axis, and its limits (radians).
#[derive(Debug, Clone, Copy)]
pub struct Joint {
    pub on_parent: Frame,
    pub on_child: Frame,
    pub kind: JointKind,
}

impl Joint {
    /// The joint of a body at `rest` on its parent's body at `parent`
    /// (rest places, model space), at `anchor` (model space): X along
    /// `axis` on the child, and along `axis` turned by `centre` on the
    /// parent (a ball's swing is measured from there).
    pub(crate) fn at(
        (parent, rest): (Frame, Frame),
        anchor: Vec3,
        (axis, centre): (Vec3, Quat),
        kind: JointKind,
    ) -> Joint {
        let basis = Quat::from_rotation_arc(Vec3::X, axis);
        Joint {
            on_parent: Frame::new(
                parent.inverse().point(anchor),
                parent.rotation.inverse() * centre * basis,
            ),
            on_child: Frame::new(
                rest.inverse().point(anchor),
                rest.rotation.inverse() * basis,
            ),
            kind,
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub enum JointKind {
    Ball { cone: f32, twist: f32 },
    Hinge { max: f32 },
}

/// A ragdoll's parts fitted to one skeleton, parents first.
#[derive(Debug, Clone)]
pub struct Template {
    pub parts: Vec<Part>,
    /// The whole body's mass (kg): each part has its share.
    pub mass: f32,
    pub material: Material,
    /// Bit `j` of `touches[i]`: parts `i` and `j` collide.
    touches: Vec<u64>,
}

/// Of the capsule fitted to a part's skin, the share of its vertices left
/// outside.
const OUTSIDE: f32 = 0.35;
/// How much of its radius a limb's capsule is drawn in at each end, so
/// neighbours overlap less at their joints.
const TUCK: f32 = 0.5;

impl Template {
    /// The plan's parts fitted to `skeleton`.
    pub fn fit(plan: &Plan, skeleton: &Skeleton) -> Result<Template, String> {
        let named = |name: &str| {
            skeleton
                .nodes
                .iter()
                .position(|n| n.name.eq_ignore_ascii_case(name))
        };
        let find = |name: &str| {
            named(name)
                .or_else(|| {
                    let (_, stand_in) =
                        plan.stand_ins.iter().find(|(lacked, _)| *lacked == name)?;
                    named(stand_in)
                })
                .ok_or_else(|| format!("the skeleton has no {name}"))
        };
        let origin = |node: usize| skeleton.nodes[node].rest.translation;
        let (left, right) = (origin(find(plan.hips.0)?), origin(find(plan.hips.1)?));
        let right_way = (right - left)
            .with_z(0.0)
            .try_normalize()
            .ok_or("the hips meet")?;
        let forward = Vec3::Z.cross(right_way);
        let body = |v: [f32; 3], out: f32| right_way * v[0] * out + forward * v[1] + Vec3::Z * v[2];

        let nodes = plan
            .parts
            .iter()
            .map(|p| find(p.node))
            .collect::<Result<Vec<_>, _>>()?;
        let skin = skin_by_part(skeleton, &nodes);
        let mut parts: Vec<Part> = Vec::with_capacity(plan.parts.len());
        for (index, (p, &node)) in plan.parts.iter().zip(&nodes).enumerate() {
            let parent = match p.parent {
                Some(name) => Some(
                    plan.parts[..index]
                        .iter()
                        .position(|q| q.name == name)
                        .ok_or_else(|| format!("{}: its parent {name} comes later", p.name))?,
                ),
                None => None,
            };
            let rest = skeleton.nodes[node].rest;
            let start = rest.translation;
            let points = &skin[index];
            let (a, b, aim, tuck) = match p.shape {
                Shape::To { node: to, past } => {
                    let end = origin(find(to)?);
                    let way = (end - start)
                        .try_normalize()
                        .ok_or("a bone has no length")?;
                    (start, end + way * past, way, true)
                }
                Shape::Between { ends, aim } => {
                    let way = (origin(find(aim)?) - start)
                        .try_normalize()
                        .ok_or("a bone has no length")?;
                    (origin(find(ends.0)?), origin(find(ends.1)?), way, false)
                }
                Shape::Along { way, length } => {
                    let way = match way {
                        Way::Forward => forward,
                        Way::Up => Vec3::Z,
                    };
                    let reach = points
                        .iter()
                        .map(|&q| (q - start).dot(way))
                        .fold(length, f32::max);
                    (start, start + way * reach, way, true)
                }
            };
            let radius = fit_radius(points, (a, b), p.thickness);
            let (a, b) = if tuck { tucked(a, b, radius) } else { (a, b) };
            let joint = match parent {
                None => None,
                Some(parent) => {
                    let (axis, kind, centre) = match p.joint {
                        JointPlan::Root => {
                            return Err(format!("{}: hangs on a part but has no joint", p.name));
                        }
                        JointPlan::Ball { cone, twist, lean } => {
                            let centre = lean.map_or(aim, |lean| {
                                body(lean, p.group.out()).try_normalize().unwrap_or(aim)
                            });
                            let kind = JointKind::Ball {
                                cone: cone.to_radians(),
                                twist: twist.to_radians(),
                            };
                            (aim, kind, Quat::from_rotation_arc(aim, centre))
                        }
                        JointPlan::Hinge { toward, max } => {
                            let axis = aim
                                .cross(body(toward, p.group.out()))
                                .try_normalize()
                                .ok_or_else(|| {
                                    format!("{}: its hinge bends along its bone", p.name)
                                })?;
                            let kind = JointKind::Hinge {
                                max: max.to_radians(),
                            };
                            (axis, kind, Quat::IDENTITY)
                        }
                    };
                    Some(Joint::at(
                        (parts[parent].rest, rest),
                        start,
                        (axis, centre),
                        kind,
                    ))
                }
            };
            parts.push(Part {
                name: p.name,
                node,
                parent,
                rest,
                solid: Solid::Capsule {
                    ends: [a, b].map(|end| rest.inverse().point(end)),
                    radius,
                },
                mass: p.mass,
                joint,
                group: p.group,
            });
        }
        Ok(Template::new(parts, plan.mass, Material::Flesh))
    }

    /// A template of `parts`, which touch each other as their groups say.
    pub(crate) fn new(parts: Vec<Part>, mass: f32, material: Material) -> Template {
        // Which parts touch is a bit each; a collider names its part in 8.
        assert!(parts.len() <= 64, "a ragdoll has at most 64 parts");
        let touches = (0..parts.len())
            .map(|i| {
                (0..parts.len())
                    .filter(|&j| {
                        let kin = parts[i].parent == Some(j) || parts[j].parent == Some(i);
                        !kin && parts[i].group.touches(parts[j].group)
                    })
                    .fold(0u64, |bits, j| bits | 1 << j)
            })
            .collect();
        Template {
            parts,
            mass,
            material,
            touches,
        }
    }

    /// Whether two parts of one ragdoll collide.
    pub fn touches(&self, a: usize, b: usize) -> bool {
        self.touches
            .get(a)
            .is_some_and(|bits| b < 64 && bits & 1 << b != 0)
    }
}

/// The skin's vertices by part: each vertex goes to the part of the nearest
/// node at or above the one that moves it.
pub(crate) fn skin_by_part(skeleton: &Skeleton, parts: &[usize]) -> Vec<Vec<Vec3>> {
    let owner: Vec<Option<usize>> = (0..skeleton.nodes.len())
        .map(|mut node| {
            loop {
                if let Some(part) = parts.iter().position(|&p| p == node) {
                    return Some(part);
                }
                node = skeleton.nodes[node].parent?;
            }
        })
        .collect();
    let mut skin = vec![Vec::new(); parts.len()];
    for &(point, node) in skeleton.skin {
        if let Some(part) = owner.get(node).copied().flatten() {
            skin[part].push(point);
        }
    }
    skin
}

/// The radius that leaves `OUTSIDE` of `points` beyond the segment's
/// capsule, within `thickness`.
fn fit_radius(points: &[Vec3], (a, b): (Vec3, Vec3), thickness: [f32; 2]) -> f32 {
    if points.len() < 8 {
        return thickness[1];
    }
    let mut distances: Vec<f32> = points.iter().map(|&p| segment_distance(p, a, b)).collect();
    distances.sort_by(f32::total_cmp);
    let at = ((distances.len() - 1) as f32 * (1.0 - OUTSIDE)) as usize;
    distances[at].clamp(thickness[0], thickness[1])
}

pub(crate) fn segment_distance(p: Vec3, a: Vec3, b: Vec3) -> f32 {
    let ab = b - a;
    let t = ((p - a).dot(ab) / ab.length_squared().max(1e-12)).clamp(0.0, 1.0);
    p.distance(a + ab * t)
}

/// `a`..`b` drawn in by `TUCK` of `radius` at each end, never past its
/// middle.
fn tucked(a: Vec3, b: Vec3, radius: f32) -> (Vec3, Vec3) {
    let length = a.distance(b);
    let tuck = (radius * TUCK).min(length * 0.5);
    let way = (b - a) / length.max(1e-6);
    (a + way * tuck, b - way * tuck)
}
