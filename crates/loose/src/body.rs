//! The character's body as the loose points meet it. It is the 14 ragdoll
//! capsules of `ragdoll::Template::fit`, posed every frame from the
//! animated skeleton, with a backstop on the trunk and a floor.
//!
//! Many garment vertices start inside a capsule in the rest pose. Most of
//! them are where a robe is tied to the body at the back and the lower
//! back. Each point therefore gets its own radius for each capsule it
//! meets. It is the capsule's radius plus the garment's thickness, or the
//! point's rest distance from the capsule's axis less 0.5 cm, whichever is
//! smaller. A point that starts inside a capsule then stays where it
//! started, and is never thrown out on the first frame.

use glam::Vec3;
use ragdoll::{Group, Solid, Template, closest_on_segment};

use crate::{Frame, Point};

/// A capsule: the segment between its two ends, and its radius.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Capsule {
    pub(crate) ends: [Vec3; 2],
    pub(crate) radius: f32,
}

/// One capsule that a point or a bone meets, and how far from the
/// capsule's axis it keeps.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Contact {
    /// The capsule's index in the body's parts.
    pub(crate) capsule: usize,
    pub(crate) radius: f32,
}

/// What each point collides with.
#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct Collision {
    pub(crate) contacts: Vec<Vec<Contact>>,
    /// A trunk capsule (pelvis, belly or chest) that keeps each point on
    /// its own side of the body. A point found more than half the radius
    /// deep in it, on the far side of its axis from the point's target, is
    /// put back on the target's side. It is not pushed out through the far
    /// side. A fast move that took a point into the body therefore never
    /// leaves it behind the body. A point that goes around the outside of
    /// the body is never held back. A cape does that when the body hangs
    /// upside down.
    pub(crate) backstops: Vec<Option<Contact>>,
    /// The capsules that the bone from each point's parent to the point
    /// meets. Without them, two points can stay outside a leg while the
    /// bone between them cuts through it.
    pub(crate) segments: Vec<Vec<Contact>>,
}

/// One capsule of the body, on its node.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Part {
    pub(crate) node: usize,
    /// The capsule's ends in its node's frame.
    pub(crate) ends: [Vec3; 2],
    pub(crate) radius: f32,
    pub(crate) parent: Option<usize>,
    /// The pelvis, the belly and the chest.
    pub(crate) trunk: bool,
    /// The ragdoll part's name, for the trace's log.
    pub(crate) name: &'static str,
    /// A thigh, a shin or a foot.
    pub(crate) leg: bool,
}

/// The body's capsules, each fixed to a node of the skeleton.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Body {
    pub(crate) parts: Vec<Part>,
}

/// How much less than its rest distance from a capsule's axis a point
/// keeps, when it starts closer than the capsule's radius (metres).
const INSIDE_MARGIN: f32 = 0.005;
/// A contact with a smaller radius than this keeps nothing out.
const SMALLEST: f32 = 0.002;
/// How many steps along the parts' tree a point's capsules may be from the
/// parts its skin weights reach. A KOTOR 2 robe's skin reaches only the
/// pelvis and the belly, and two steps take it down to the shins and the
/// upper arms.
const HOPS: usize = 2;

impl Body {
    /// The capsules of a fitted ragdoll. Parts that are boxes (a droid's
    /// plates) are left out.
    pub fn new(template: &Template) -> Body {
        let mut index_of = vec![None; template.parts.len()];
        let mut parts = Vec::new();
        for (index, part) in template.parts.iter().enumerate() {
            let Solid::Capsule { ends, radius } = part.solid else {
                continue;
            };
            index_of[index] = Some(parts.len());
            parts.push(Part {
                node: part.node,
                ends,
                radius,
                parent: part.parent.and_then(|parent| index_of[parent]),
                trunk: part.group == Group::Core,
                name: part.name,
                leg: matches!(part.group, Group::LegLeft | Group::LegRight),
            });
        }
        Body { parts }
    }

    /// The capsules where the skeleton's `frames` put their nodes.
    pub(crate) fn pose(&self, frames: impl Fn(usize) -> Frame, posed: &mut Vec<Capsule>) {
        posed.clear();
        posed.extend(self.parts.iter().map(|part| {
            let frame = frames(part.node);
            Capsule {
                ends: part.ends.map(|end| frame.point(end)),
                radius: part.radius,
            }
        }));
    }

    /// The part that owns node `node`: the part of the nearest node at or
    /// above it.
    fn part_of(&self, parents: &[Option<usize>], node: usize) -> Option<usize> {
        let mut at = node;
        loop {
            if let Some(part) = self.parts.iter().position(|part| part.node == at) {
                return Some(part);
            }
            at = (*parents.get(at)?)?;
        }
    }

    /// The parts within `hops` steps of `seeds` along the parts' tree, and
    /// the rest of each leg below a leg part among them. A leg swings
    /// anywhere under a skirt in a kick or a cartwheel, so a point that can
    /// meet a thigh can meet the shin and the foot below it.
    fn around(&self, seeds: &[usize], hops: usize) -> Vec<usize> {
        let mut found: Vec<usize> = seeds.to_vec();
        found.sort_unstable();
        found.dedup();
        let mut edge = found.clone();
        for _ in 0..hops {
            let mut next = Vec::new();
            for &part in &edge {
                let children =
                    (0..self.parts.len()).filter(|&i| self.parts[i].parent == Some(part));
                for neighbour in self.parts[part].parent.into_iter().chain(children) {
                    if !found.contains(&neighbour) {
                        found.push(neighbour);
                        next.push(neighbour);
                    }
                }
            }
            edge = next;
        }
        let mut at = 0;
        while at < found.len() {
            let part = found[at];
            for below in 0..self.parts.len() {
                if self.parts[below].leg
                    && self.parts[below].parent == Some(part)
                    && !found.contains(&below)
                {
                    found.push(below);
                }
            }
            at += 1;
        }
        found.sort_unstable();
        found
    }
}

/// A point as the collision sees it: its rest place, its parent's rest
/// place, whether it is pinned, and the point itself.
pub(crate) struct Resting<'a> {
    pub(crate) point: &'a Point,
    pub(crate) at: Vec3,
    pub(crate) parent_at: Option<Vec3>,
    pub(crate) pinned: bool,
}

/// What each point meets. `rest` is the capsules in the rest pose and
/// `parents` the skeleton's parent of each node.
pub(crate) fn contacts(
    points: &[Resting],
    body: &Body,
    rest: &[Capsule],
    parents: &[Option<usize>],
) -> Collision {
    let mut collision = Collision::default();
    for resting in points {
        let seeds: Vec<usize> = resting
            .point
            .reach
            .iter()
            .filter_map(|&node| body.part_of(parents, node))
            .collect();
        let candidates = if seeds.is_empty() {
            (0..body.parts.len()).collect()
        } else {
            body.around(&seeds, HOPS)
        };
        let thick = resting.point.thickness;
        let from_axis = |capsule: usize, at: Vec3| {
            let [a, b] = rest[capsule].ends;
            at.distance(closest_on_segment(at, a, b))
        };
        let keep = |capsule: usize, from_axis: f32| Contact {
            capsule,
            radius: (rest[capsule].radius + thick).min(from_axis - INSIDE_MARGIN),
        };
        let surface = |capsule: usize| from_axis(capsule, resting.at) - rest[capsule].radius;
        let useful = |contact: &Contact| contact.radius > SMALLEST;
        let (contacts, segments, backstop) = if resting.pinned {
            (Vec::new(), Vec::new(), None)
        } else {
            let contacts = candidates
                .iter()
                .map(|&capsule| keep(capsule, from_axis(capsule, resting.at)))
                .filter(useful)
                .collect();
            let segments = resting.parent_at.map_or(Vec::new(), |parent| {
                candidates
                    .iter()
                    .map(|&capsule| {
                        let [a, b] = rest[capsule].ends;
                        let (_, on_bone, on_axis) = closest_between((parent, resting.at), (a, b));
                        keep(capsule, on_bone.distance(on_axis))
                    })
                    .filter(useful)
                    .collect()
            });
            let backstop = (0..body.parts.len())
                .filter(|&part| body.parts[part].trunk)
                .min_by(|&a, &b| surface(a).total_cmp(&surface(b)))
                .map(|capsule| keep(capsule, from_axis(capsule, resting.at)))
                .filter(useful);
            (contacts, segments, backstop)
        };
        collision.contacts.push(contacts);
        collision.segments.push(segments);
        collision.backstops.push(backstop);
    }
    collision
}

/// The closest points of the segments `p` and `q`: the share of the way
/// along `p` of its closest point, then the two points.
pub(crate) fn closest_between((p0, p1): (Vec3, Vec3), (q0, q1): (Vec3, Vec3)) -> (f32, Vec3, Vec3) {
    const TINY: f32 = 1e-12;
    let (dp, dq, r) = (p1 - p0, q1 - q0, p0 - q0);
    let (a, e, f) = (dp.length_squared(), dq.length_squared(), dq.dot(r));
    let (s, t) = if a <= TINY && e <= TINY {
        (0.0, 0.0)
    } else if a <= TINY {
        (0.0, (f / e).clamp(0.0, 1.0))
    } else {
        let c = dp.dot(r);
        if e <= TINY {
            ((-c / a).clamp(0.0, 1.0), 0.0)
        } else {
            let b = dp.dot(dq);
            let denominator = a * e - b * b;
            let s = if denominator > TINY {
                ((b * f - c * e) / denominator).clamp(0.0, 1.0)
            } else {
                0.0
            };
            let t = (b * s + f) / e;
            if t < 0.0 {
                ((-c / a).clamp(0.0, 1.0), 0.0)
            } else if t > 1.0 {
                (((b - c) / a).clamp(0.0, 1.0), 1.0)
            } else {
                (s, t)
            }
        }
    };
    (s, p0 + dp * s, q0 + dq * t)
}
