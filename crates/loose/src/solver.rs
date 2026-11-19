//! The XPBD solver: small fixed substeps of game time, one pass over the
//! constraints in each (Macklin and others, "Small Steps in Physics
//! Simulation", 2019).

use glam::{Quat, Vec3};
use ragdoll::closest_on_segment;

use crate::body::{Capsule, Collision, closest_between};
use crate::{Frame, Link, Point, Rig};

/// How the solver steps. The defaults are the ones the plan starts from.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Settings {
    /// Substeps per second of game time. Each substep is one pass over the
    /// constraints.
    pub rate: f32,
    /// The most substeps in one frame. A frame longer than this many
    /// substeps loses the rest of its time.
    pub most: u32,
    /// Passes over the shape's constraints in each substep. One is the
    /// small-steps method. A second pass halves the stretch of a tree in a
    /// fast flip and costs about a third more.
    pub passes: u32,
    /// Gravity in the world (metres per second squared, +Z up).
    pub gravity: Vec3,
    /// A root that moves farther than this in one frame (metres) has
    /// teleported, and every point goes back to its target pose.
    pub teleport: f32,
    /// The most that the root's movement through the world can move a point
    /// in one substep (metres).
    pub root_travel: f32,
    /// The most that the root's turn in the world can turn a point in one
    /// substep (radians).
    pub root_turn: f32,
    /// How much longer than its rest path a tether may grow, as a share of
    /// that path.
    pub tether_slack: f32,
    /// How far a free point may move in one substep (metres). It stops a
    /// point from passing through a thin capsule in a fast move.
    pub max_step: f32,
}

impl Default for Settings {
    fn default() -> Settings {
        Settings {
            rate: 240.0,
            most: 32,
            passes: 1,
            gravity: Vec3::new(0.0, 0.0, -9.81),
            teleport: 5.0,
            root_travel: 0.05,
            root_turn: 15f32.to_radians(),
            tether_slack: 0.02,
            max_step: 0.08,
        }
    }
}

/// What the solver reads each frame. Every slice has one entry per point.
#[derive(Debug, Clone, Copy)]
pub struct Pose<'a> {
    /// The character's root in the world.
    pub root: Frame,
    /// For a root point, the animated frame of the node it hangs on, in
    /// root space. Other points' entries are not read.
    pub attach: &'a [Frame],
    /// The animated local turn of the bone at each point. A tip has no bone
    /// and its entry is not read.
    pub turns: &'a [Quat],
    /// The body's capsules, posed from the animated skeleton, in root
    /// space ([`crate::body::Body::pose`]).
    pub capsules: &'a [Capsule],
    /// The floor's height in the world, if there is one.
    pub floor: Option<f32>,
}

/// What one frame's step did.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Report {
    pub substeps: u32,
    /// The points went back to their target pose: on the first frame, after
    /// a teleport, after time ran backwards or a frame over a second, or
    /// when a point was no longer a finite number.
    pub reset: bool,
}

/// The pose of the frame before, in its own root space.
#[derive(Debug, Clone)]
struct Then {
    root: Frame,
    targets: Vec<Vec3>,
    capsules: Vec<Capsule>,
}

/// The state of one body's loose points.
#[derive(Debug, Clone)]
pub struct Solver {
    settings: Settings,
    points: Vec<Point>,
    links: Vec<(Link, f32)>,
    children: Vec<Vec<usize>>,
    /// Each point's inverse mass: 0 for a pinned point.
    weight: Vec<f32>,
    /// The pinned point each point is tethered to, and the tether's length.
    tethers: Vec<(usize, f32)>,
    /// The distance from a point to its grandparent: the point, the
    /// grandparent, the rest length and the compliance. A child of a root
    /// has none, and swings freely about the root.
    bends: Vec<(usize, usize, f32, f32)>,
    /// Pairs of children of one point, kept at their rest distance so that
    /// one turn of the parent bone can aim at all of them.
    siblings: Vec<(usize, usize, f32, f32)>,
    collision: Option<Collision>,
    positions: Vec<Vec3>,
    previous: Vec<Vec3>,
    targets: Vec<Vec3>,
    /// The targets and capsules of one substep, reused.
    targets_now: Vec<Vec3>,
    capsules_now: Vec<Capsule>,
    then: Option<Then>,
    /// Game time not yet stepped, less than one substep.
    carry: f32,
}

impl Solver {
    /// A solver for `rig`, with collision against the body when `collision`
    /// is given ([`crate::body::contacts`]).
    pub fn new(
        rig: &Rig,
        settings: Settings,
        collision: Option<Collision>,
    ) -> Result<Solver, String> {
        rig.check()?;
        let points = rig.points.clone();
        let count = points.len();
        if let Some(collision) = &collision
            && (collision.contacts.len() != count
                || collision.backstops.len() != count
                || collision.segments.len() != count)
        {
            return Err("the collision does not have one entry per point".into());
        }
        let mut children = vec![Vec::new(); count];
        for (index, point) in points.iter().enumerate() {
            if let Some(parent) = point.parent {
                children[parent].push(index);
            }
        }
        let weight = points
            .iter()
            .map(|point| if point.pinned { 0.0 } else { point.freedom })
            .collect();
        let roots = rig.roots();
        let mut path = vec![0.0f32; count];
        for (index, point) in points.iter().enumerate() {
            if let Some(parent) = point.parent
                && roots[index] != index
            {
                path[index] = path[parent] + point.offset.length();
            }
        }
        let tethers = (0..count)
            .map(|index| (roots[index], path[index] * (1.0 + settings.tether_slack)))
            .collect();
        let mut bends = Vec::new();
        for (index, point) in points.iter().enumerate() {
            let Some(grandparent) = point.parent.and_then(|parent| points[parent].parent) else {
                continue;
            };
            bends.push((
                index,
                grandparent,
                point.at.distance(points[grandparent].at),
                point.bend,
            ));
        }
        let mut siblings = Vec::new();
        for kids in &children {
            for pair in kids.windows(2) {
                let (a, b) = (pair[0], pair[1]);
                let compliance = points[a].stretch.min(points[b].stretch);
                siblings.push((a, b, points[a].at.distance(points[b].at), compliance));
            }
        }
        let links = rig
            .links
            .iter()
            .map(|&link| (link, points[link.a].at.distance(points[link.b].at)))
            .collect();
        Ok(Solver {
            settings,
            links,
            children,
            weight,
            tethers,
            bends,
            siblings,
            collision,
            positions: vec![Vec3::ZERO; count],
            previous: vec![Vec3::ZERO; count],
            targets: vec![Vec3::ZERO; count],
            targets_now: vec![Vec3::ZERO; count],
            capsules_now: Vec::new(),
            then: None,
            carry: 0.0,
            points,
        })
    }

    pub fn points(&self) -> &[Point] {
        &self.points
    }

    pub fn settings(&self) -> &Settings {
        &self.settings
    }

    /// Where each point is, in root space.
    pub fn positions(&self) -> &[Vec3] {
        &self.positions
    }

    /// Where each point's target pose put it in the last frame, in root
    /// space.
    pub fn targets(&self) -> &[Vec3] {
        &self.targets
    }

    /// The pinned point that each point's tether holds it to, and the
    /// tether's length.
    pub fn tethers(&self) -> &[(usize, f32)] {
        &self.tethers
    }

    /// The links between chains, each with its rest length.
    pub fn links(&self) -> &[(Link, f32)] {
        &self.links
    }

    /// The contacts of each point with the capsules, if it collides.
    pub fn collision(&self) -> Option<&Collision> {
        self.collision.as_ref()
    }

    /// Forgets the points' motion: the next step puts them at their target
    /// pose.
    pub fn reset(&mut self) {
        self.then = None;
    }

    /// Steps the points by `seconds` of game time toward `pose`.
    pub fn step(&mut self, seconds: f32, pose: &Pose) -> Report {
        let count = self.points.len();
        assert!(
            pose.attach.len() == count && pose.turns.len() == count,
            "a pose has one attach frame and one turn per point"
        );
        let mut targets = std::mem::take(&mut self.targets);
        self.target_pose(pose, &mut targets);
        self.targets = targets;
        let jumped = self.then.as_ref().is_none_or(|then| {
            then.root.translation.distance(pose.root.translation) > self.settings.teleport
        });
        let bad_time = !seconds.is_finite() || !(0.0..=1.0).contains(&seconds);
        if jumped || bad_time || pose.capsules.len() != self.capsule_count(pose) {
            self.start_over(pose);
            return Report {
                substeps: 0,
                reset: true,
            };
        }
        let rate = self.settings.rate;
        let leftover = self.carry;
        let due = ((leftover + seconds) * rate).floor().max(0.0);
        let substeps = due.min(self.settings.most as f32) as u32;
        let dropped = due > substeps as f32;
        self.carry = if dropped {
            0.0
        } else {
            (leftover + seconds - substeps as f32 / rate).clamp(0.0, 1.0 / rate)
        };
        let share = |k: u32| -> f32 {
            if dropped {
                (k as f32 / substeps as f32).clamp(0.0, 1.0)
            } else if seconds > 0.0 {
                ((k as f32 / rate - leftover) / seconds).clamp(0.0, 1.0)
            } else {
                1.0
            }
        };
        let then = self
            .then
            .take()
            .expect("a step after the first has the frame before");
        for k in 0..substeps {
            self.substep(&then, pose, (share(k), share(k + 1)));
        }
        let finite = self.positions.iter().all(|p| p.is_finite())
            && self.previous.iter().all(|p| p.is_finite());
        if !finite {
            self.start_over(pose);
            return Report {
                substeps,
                reset: true,
            };
        }
        self.remember(pose);
        Report {
            substeps,
            reset: false,
        }
    }

    /// The capsules a pose must give: as many as the frame before had, or
    /// any number on the first frame.
    fn capsule_count(&self, pose: &Pose) -> usize {
        self.then
            .as_ref()
            .map_or(pose.capsules.len(), |then| then.capsules.len())
    }

    /// Every point at its target, at rest.
    fn start_over(&mut self, pose: &Pose) {
        self.positions.clone_from(&self.targets);
        self.previous.clone_from(&self.targets);
        self.carry = 0.0;
        self.remember(pose);
    }

    fn remember(&mut self, pose: &Pose) {
        let mut then = self.then.take().unwrap_or_else(|| Then {
            root: pose.root,
            targets: Vec::new(),
            capsules: Vec::new(),
        });
        then.root = pose.root;
        then.targets.clone_from(&self.targets);
        then.capsules.clear();
        then.capsules.extend_from_slice(pose.capsules);
        self.then = Some(then);
    }

    /// Each point where the animated pose puts it, in root space.
    fn target_pose(&self, pose: &Pose, targets: &mut [Vec3]) {
        let mut turned: Vec<Quat> = Vec::with_capacity(self.points.len());
        for (index, point) in self.points.iter().enumerate() {
            let (parent_turn, at) = match point.parent {
                None => {
                    let attach = pose.attach[index];
                    (attach.rotation, attach.point(point.offset))
                }
                Some(parent) => (
                    turned[parent],
                    targets[parent] + turned[parent] * point.offset,
                ),
            };
            targets[index] = at;
            turned.push((parent_turn * pose.turns[index]).normalize());
        }
    }

    /// One substep, from share `from` to share `to` of the way from the
    /// frame before to this one.
    fn substep(&mut self, then: &Then, pose: &Pose, (from, to): (f32, f32)) {
        let h = 1.0 / self.settings.rate;
        let root_from = blend(then.root, pose.root, from);
        let root_to = blend(then.root, pose.root, to);
        let felt = self.limited(root_to.inverse().then(root_from));
        for index in 0..self.points.len() {
            let inertia = self.points[index].inertia;
            if self.weight[index] == 0.0 || inertia == 0.0 {
                continue;
            }
            for p in [&mut self.positions[index], &mut self.previous[index]] {
                *p += (felt.point(*p) - *p) * inertia;
            }
        }
        for (index, target) in self.targets_now.iter_mut().enumerate() {
            *target = then.targets[index].lerp(self.targets[index], to);
        }
        self.capsules_now.clear();
        self.capsules_now.extend(
            then.capsules
                .iter()
                .zip(pose.capsules)
                .map(|(a, b)| Capsule {
                    ends: [a.ends[0].lerp(b.ends[0], to), a.ends[1].lerp(b.ends[1], to)],
                    radius: a.radius + (b.radius - a.radius) * to,
                }),
        );
        let gravity = root_to.rotation.inverse() * self.settings.gravity;
        for index in 0..self.points.len() {
            let start = self.positions[index];
            if self.weight[index] == 0.0 {
                self.previous[index] = start;
                self.positions[index] = self.targets_now[index];
                continue;
            }
            let keep = (1.0 - self.points[index].damping * h).max(0.0);
            let velocity = (start - self.previous[index]) / h * keep + gravity * h;
            self.previous[index] = start;
            self.positions[index] = start + velocity * h;
        }
        for _ in 0..self.settings.passes.max(1) {
            self.constrain(h);
        }
        self.limit(root_to, pose.floor);
    }

    /// The root's movement in one substep, as the points feel it: no more
    /// than the settings let it move and turn.
    fn limited(&self, delta: Frame) -> Frame {
        let travel = delta
            .translation
            .clamp_length_max(self.settings.root_travel);
        let (axis, angle) = delta.rotation.to_axis_angle();
        let angle = if angle > std::f32::consts::PI {
            angle - std::f32::consts::TAU
        } else {
            angle
        };
        let turn = angle.clamp(-self.settings.root_turn, self.settings.root_turn);
        let rotation = if axis.is_finite() && axis.length_squared() > 0.5 {
            Quat::from_axis_angle(axis, turn)
        } else {
            Quat::IDENTITY
        };
        Frame::new(travel, rotation)
    }

    /// One pass over the constraints that keep the shape: lengths, bends,
    /// siblings, links, the pull toward the target, the tethers.
    fn constrain(&mut self, h: f32) {
        let scale = 1.0 / (h * h);
        self.lengths(scale);
        for s in 0..self.siblings.len() {
            let (a, b, rest, compliance) = self.siblings[s];
            self.distance((a, b), rest, compliance * scale, false);
        }
        for b in 0..self.bends.len() {
            let (point, grandparent, rest, compliance) = self.bends[b];
            self.distance((grandparent, point), rest, compliance * scale, false);
        }
        for l in 0..self.links.len() {
            let (link, rest) = self.links[l];
            self.distance((link.a, link.b), rest, link.compliance * scale, false);
        }
        for index in 0..self.points.len() {
            if let Some(drive) = self.points[index].drive {
                let target = self.targets_now[index];
                self.toward(index, target, 0.0, drive * scale);
            }
        }
        // The lengths again, after the bends and links have pulled on them:
        // one pass left a tree 11% longer or shorter in a flip.
        self.lengths(scale);
        for index in 0..self.points.len() {
            let (root, length) = self.tethers[index];
            if root != index {
                self.distance((root, index), length, 0.0, true);
            }
        }
    }

    /// Each point's distance to its parent.
    fn lengths(&mut self, scale: f32) {
        for index in 0..self.points.len() {
            let point = self.points[index];
            if let Some(parent) = point.parent {
                self.distance(
                    (parent, index),
                    point.offset.length(),
                    point.stretch * scale,
                    false,
                );
            }
        }
    }

    /// The hard limits, last: the maximum distance from the target, the
    /// most a point moves in one substep, then the body and the floor.
    fn limit(&mut self, root: Frame, floor: Option<f32>) {
        for index in 0..self.points.len() {
            if self.weight[index] == 0.0 {
                continue;
            }
            let target = self.targets_now[index];
            let mut at = self.positions[index];
            at = target + (at - target).clamp_length_max(self.points[index].max_distance);
            let start = self.previous[index];
            self.positions[index] = start + (at - start).clamp_length_max(self.settings.max_step);
        }
        self.segments();
        let floor = floor.map(|height| {
            let up = root.rotation.inverse() * Vec3::Z;
            (up, height - root.translation.z)
        });
        for index in 0..self.points.len() {
            if self.weight[index] == 0.0 {
                continue;
            }
            let mut at = self.positions[index];
            if let Some(collision) = &self.collision {
                let target = self.targets_now[index];
                if let Some(contact) = collision.backstops[index]
                    && let Some(capsule) = self.capsules_now.get(contact.capsule)
                {
                    at = backstop(at, target, capsule, contact.radius);
                }
                for contact in &collision.contacts[index] {
                    if let Some(capsule) = self.capsules_now.get(contact.capsule) {
                        at = pushed_out(at, target, capsule, contact.radius);
                    }
                }
            }
            if let Some((up, height)) = floor {
                let above = at.dot(up) - height;
                let margin = self.points[index].radius.max(FLOOR_MARGIN);
                if above < margin {
                    at += up * (margin - above);
                }
            }
            self.positions[index] = at;
        }
    }

    /// Each bone kept out of the capsules its segment meets: the closest
    /// point of the segment to a capsule's axis is pushed out to the
    /// segment's radius, and the push is shared by the two ends by how near
    /// the point is to each and how freely each moves.
    fn segments(&mut self) {
        let Some(collision) = &self.collision else {
            return;
        };
        for index in 0..self.points.len() {
            let Some(parent) = self.points[index].parent else {
                continue;
            };
            for contact in &collision.segments[index] {
                let Some(capsule) = self.capsules_now.get(contact.capsule) else {
                    continue;
                };
                let (share, on_bone, on_axis) = closest_between(
                    (self.positions[parent], self.positions[index]),
                    (capsule.ends[0], capsule.ends[1]),
                );
                let out = on_bone - on_axis;
                let distance = out.length();
                if distance >= contact.radius {
                    continue;
                }
                let way = if distance > 1e-6 {
                    out / distance
                } else {
                    let target = self.targets_now[parent].lerp(self.targets_now[index], share);
                    (target - on_axis).try_normalize().unwrap_or(Vec3::Z)
                };
                let (near_parent, near_point) = (
                    self.weight[parent] * (1.0 - share),
                    self.weight[index] * share,
                );
                let sum = near_parent * (1.0 - share) + near_point * share;
                if sum < 1e-6 {
                    continue;
                }
                let push = way * ((contact.radius - distance) / sum);
                self.positions[parent] += push * near_parent;
                self.positions[index] += push * near_point;
            }
        }
    }

    /// The XPBD distance constraint between two points. A `tether` only
    /// pulls them together, never pushes them apart.
    fn distance(&mut self, (a, b): (usize, usize), rest: f32, compliance: f32, tether: bool) {
        let (wa, wb) = (self.weight[a], self.weight[b]);
        let sum = wa + wb + compliance;
        if wa + wb == 0.0 {
            return;
        }
        let between = self.positions[b] - self.positions[a];
        let length = between.length();
        if length < 1e-9 {
            return;
        }
        let error = length - rest;
        if tether && error <= 0.0 {
            return;
        }
        let way = between / length;
        let lambda = -error / sum;
        self.positions[a] -= way * (lambda * wa);
        self.positions[b] += way * (lambda * wb);
    }

    /// The XPBD distance constraint between a point and a place that does
    /// not move.
    fn toward(&mut self, index: usize, place: Vec3, rest: f32, compliance: f32) {
        let weight = self.weight[index];
        if weight == 0.0 {
            return;
        }
        let between = self.positions[index] - place;
        let length = between.length();
        if length < 1e-9 {
            return;
        }
        let error = length - rest;
        let lambda = -error / (weight + compliance);
        self.positions[index] += between / length * (lambda * weight);
    }

    /// Each bone's new local turn, with the solved points: each bone, from
    /// the top of each tree down, turns to point at its children's solved
    /// places. It keeps the twist about its own length that the animation
    /// gave it, and its translation does not change. A tip's entry is its
    /// turn in `pose`.
    pub fn turns(&self, pose: &Pose) -> Vec<Quat> {
        let count = self.points.len();
        let mut turns = Vec::with_capacity(count);
        let mut world: Vec<Quat> = Vec::with_capacity(count);
        let mut heads: Vec<Vec3> = Vec::with_capacity(count);
        for (index, point) in self.points.iter().enumerate() {
            let (above, head) = match point.parent {
                None => {
                    let attach = pose.attach[index];
                    (attach.rotation, attach.point(point.offset))
                }
                Some(parent) => (world[parent], heads[parent] + world[parent] * point.offset),
            };
            let animated = (above * pose.turns[index]).normalize();
            let mut swing = glam::Vec4::ZERO;
            let mut first: Option<Quat> = None;
            for &child in &self.children[index] {
                let offset = self.points[child].offset;
                let (Some(was), Some(now)) = (
                    (animated * offset).try_normalize(),
                    (self.positions[child] - head).try_normalize(),
                ) else {
                    continue;
                };
                let mut arc = Quat::from_rotation_arc(was, now);
                let reference = *first.get_or_insert(arc);
                if arc.dot(reference) < 0.0 {
                    arc = -arc;
                }
                swing += glam::Vec4::from(arc) * offset.length();
            }
            let new = match swing.try_normalize() {
                Some(swing) => (Quat::from_vec4(swing) * animated).normalize(),
                None => animated,
            };
            turns.push((above.inverse() * new).normalize());
            world.push(new);
            heads.push(head);
        }
        turns
    }
}

/// How far a point keeps above the floor at least (metres).
const FLOOR_MARGIN: f32 = 0.01;
/// How deep in a trunk capsule, as a share of its radius, a point on the
/// far side from its target must be to count as having passed through.
const THROUGH: f32 = 0.5;

/// `at` outside `capsule`, at `radius` from its axis: pushed straight out,
/// or toward `target` when it sits on the axis.
fn pushed_out(at: Vec3, target: Vec3, capsule: &Capsule, radius: f32) -> Vec3 {
    let [a, b] = capsule.ends;
    let closest = closest_on_segment(at, a, b);
    let out = at - closest;
    let distance = out.length();
    if distance >= radius {
        return at;
    }
    let way = if distance > 1e-6 {
        out / distance
    } else {
        (target - closest).try_normalize().unwrap_or(Vec3::Z)
    };
    closest + way * radius
}

/// `at` put back on `target`'s side of a trunk capsule when it is deep in
/// the capsule on the far side: a fast move took it through the body.
fn backstop(at: Vec3, target: Vec3, capsule: &Capsule, radius: f32) -> Vec3 {
    let [a, b] = capsule.ends;
    let closest = closest_on_segment(at, a, b);
    let out = at - closest;
    if out.length() >= radius * THROUGH {
        return at;
    }
    let Some(side) = (target - closest_on_segment(target, a, b)).try_normalize() else {
        return at;
    };
    if out.dot(side) < 0.0 {
        closest + side * radius
    } else {
        at
    }
}

/// The frame `share` of the way from `a` to `b`.
fn blend(a: Frame, b: Frame, share: f32) -> Frame {
    Frame::new(
        a.translation.lerp(b.translation, share),
        a.rotation.slerp(b.rotation, share),
    )
}
