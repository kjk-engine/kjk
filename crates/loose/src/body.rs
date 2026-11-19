//! The character's body as the loose points meet it: the 14 ragdoll
//! capsules of `ragdoll::Template::fit`, posed every frame from the
//! animated skeleton, with a backstop on the trunk and the floor.
//!
//! Many garment vertices start inside a capsule in the rest pose, mostly
//! where a robe is tied to the body at the back and the lower back
//! (`docs/research/2026-11-19-robe-capsules-and-stress-scenes.md`). Each
//! point therefore gets its own radius for each capsule it meets. It is the
//! capsule's radius plus the point's own thickness, or the point's rest
//! distance from the capsule's axis less 0.5 cm, whichever is smaller. A
//! point that starts inside a capsule then stays where it started, and is
//! never thrown out on the first frame.

use glam::Vec3;
use ragdoll::{Group, Solid, Template, closest_on_segment};

use crate::{Frame, Rig};

/// A capsule: the segment between its two ends, and its radius.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Capsule {
    pub ends: [Vec3; 2],
    pub radius: f32,
}

/// One capsule that a point meets, and how far from its axis the point
/// keeps.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Contact {
    /// The capsule's index in [`Body::parts`] and in each posed list.
    pub capsule: usize,
    pub radius: f32,
}

/// What each point of a rig collides with, made once per body model.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Collision {
    /// The capsules each point is kept out of.
    pub contacts: Vec<Vec<Contact>>,
    /// A trunk capsule (pelvis, belly or chest) that keeps each point on
    /// its own side of the body. A point found more than half the radius
    /// deep in it, on the far side of its axis from the point's target, is
    /// put back on the target's side, and not pushed out through the far
    /// side. A fast move that took
    /// a point into the body therefore never leaves it behind the body. A
    /// point that goes around the outside of the body, as a cape does when
    /// the body hangs upside down, is never held back.
    pub backstops: Vec<Option<Contact>>,
    /// The capsules that the segment from each point's parent to the point
    /// is kept out of. Without them, two points can stay outside a leg
    /// while the bone between them cuts through it. A root has none.
    pub segments: Vec<Vec<Contact>>,
}

/// One capsule of the body, on its node.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Part {
    pub node: usize,
    /// The capsule's ends in its node's frame.
    pub ends: [Vec3; 2],
    pub radius: f32,
    /// The part it hangs on, as an index into [`Body::parts`].
    pub parent: Option<usize>,
    /// A part of the trunk: the pelvis, the belly or the chest.
    pub trunk: bool,
    /// Its node's rest frame in model space.
    pub rest: Frame,
}

/// The body's capsules.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Body {
    pub parts: Vec<Part>,
}

/// How much less than its rest distance from a capsule's axis a point
/// keeps, when it starts closer than the capsule's radius (metres).
const INSIDE_MARGIN: f32 = 0.005;
/// A contact with a smaller radius than this keeps nothing out.
const SMALLEST: f32 = 0.002;

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
                rest: part.rest,
            });
        }
        Body { parts }
    }

    /// The capsules where the nodes are: `node` gives each node's frame in
    /// the space wanted (the root space, for the solver).
    pub fn pose(&self, node: impl Fn(usize) -> Frame, posed: &mut Vec<Capsule>) {
        posed.clear();
        posed.extend(self.parts.iter().map(|part| {
            let frame = node(part.node);
            Capsule {
                ends: part.ends.map(|end| frame.point(end)),
                radius: part.radius,
            }
        }));
    }

    /// The capsules in the rest pose, in model space.
    pub fn rest(&self) -> Vec<Capsule> {
        self.parts
            .iter()
            .map(|part| Capsule {
                ends: part.ends.map(|end| part.rest.point(end)),
                radius: part.radius,
            })
            .collect()
    }

    /// The part that owns node `node`: the part of the nearest node at or
    /// above it. `parents` is the skeleton's parent of each node.
    pub fn part_of(&self, parents: &[Option<usize>], node: usize) -> Option<usize> {
        let mut at = node;
        loop {
            if let Some(part) = self.parts.iter().position(|part| part.node == at) {
                return Some(part);
            }
            at = (*parents.get(at)?)?;
        }
    }

    /// The parts within `hops` steps of `seeds` along the parts' tree.
    fn around(&self, seeds: &[usize], hops: usize) -> Vec<usize> {
        let mut found: Vec<usize> = seeds
            .iter()
            .copied()
            .filter(|&s| s < self.parts.len())
            .collect();
        found.sort_unstable();
        found.dedup();
        let mut edge = found.clone();
        for _ in 0..hops {
            let mut next = Vec::new();
            for &part in &edge {
                let neighbours = self.parts[part].parent.into_iter().chain(
                    self.parts
                        .iter()
                        .enumerate()
                        .filter(|(_, p)| p.parent == Some(part))
                        .map(|(i, _)| i),
                );
                for neighbour in neighbours {
                    if !found.contains(&neighbour) {
                        found.push(neighbour);
                        next.push(neighbour);
                    }
                }
            }
            edge = next;
        }
        found
    }
}

/// How many steps along the parts' tree a point's capsules may be from the
/// parts its skin weights reach. Two steps take a robe that the pelvis
/// moves down to the shins.
const HOPS: usize = 2;

/// The contacts of each point of `rig` with `body`. `reach` gives, for each
/// point, the parts that its skin weights reach: the parts that own the
/// bones moving the same vertices as the point's bone (`Body::part_of`).
/// Each point meets at most `most` capsules. They are the nearest ones at
/// rest among the parts within two steps of the ones it reaches. A point
/// that reaches no part may meet any of them.
pub fn contacts(rig: &Rig, body: &Body, reach: &[Vec<usize>], most: usize) -> Collision {
    let rest = body.rest();
    let all: Vec<usize> = (0..body.parts.len()).collect();
    let mut collision = Collision::default();
    for (index, point) in rig.points.iter().enumerate() {
        let seeds = reach.get(index).map_or(&[][..], Vec::as_slice);
        let candidates = if seeds.is_empty() {
            all.clone()
        } else {
            body.around(seeds, HOPS)
        };
        let surface = |capsule: usize| {
            let Capsule {
                ends: [a, b],
                radius,
            } = rest[capsule];
            point.at.distance(closest_on_segment(point.at, a, b)) - radius
        };
        let contact = |capsule: usize| {
            let Capsule {
                ends: [a, b],
                radius,
            } = rest[capsule];
            let axis = point.at.distance(closest_on_segment(point.at, a, b));
            Contact {
                capsule,
                radius: (radius + point.radius).min(axis - INSIDE_MARGIN),
            }
        };
        let mut near = candidates;
        near.sort_by(|&a, &b| surface(a).total_cmp(&surface(b)).then(a.cmp(&b)));
        near.truncate(most);
        near.sort_unstable();
        let segment = |capsule: usize| {
            let parent = rig.points[point.parent?].at;
            let Capsule {
                ends: [a, b],
                radius,
            } = rest[capsule];
            let (_, on_bone, on_axis) = closest_between((parent, point.at), (a, b));
            Some(Contact {
                capsule,
                radius: (radius + point.radius).min(on_bone.distance(on_axis) - INSIDE_MARGIN),
            })
        };
        let segments: Vec<Contact> = near
            .iter()
            .filter_map(|&capsule| segment(capsule))
            .filter(|c| c.radius > SMALLEST)
            .collect();
        let mut kept: Vec<Contact> = near
            .into_iter()
            .map(contact)
            .filter(|c| c.radius > SMALLEST)
            .collect();
        if point.parent.is_none() || point.pinned {
            kept.clear();
        }
        let backstop = (!point.pinned)
            .then(|| {
                (0..body.parts.len())
                    .filter(|&part| body.parts[part].trunk)
                    .min_by(|&a, &b| surface(a).total_cmp(&surface(b)))
            })
            .flatten()
            .map(contact)
            .filter(|c| c.radius > SMALLEST);
        collision.contacts.push(kept);
        collision.backstops.push(backstop);
        collision.segments.push(segments);
    }
    collision
}

/// The closest points of the segments `p` and `q`: the share of the way
/// along `p` of its closest point, then the two points.
pub(crate) fn closest_between((p0, p1): (Vec3, Vec3), (q0, q1): (Vec3, Vec3)) -> (f32, Vec3, Vec3) {
    let (dp, dq, r) = (p1 - p0, q1 - q0, p0 - q0);
    let (a, e, f) = (dp.length_squared(), dq.length_squared(), dq.dot(r));
    const TINY: f32 = 1e-12;
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
