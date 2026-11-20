//! The XPBD solver: small fixed substeps of game time, one pass over the
//! constraints in each (Macklin and others, "Small Steps in Physics
//! Simulation", 2019).

use glam::{Quat, Vec3, Vec4};
use ragdoll::closest_on_segment;

use crate::body::{Capsule, Collision, Resting, closest_between, contacts};
use crate::trace::{self, Push, Record, Touch};
use crate::{Body, Frame, Place, Rig, Skeleton};

/// How the solver steps. The defaults are the ones the plan starts from.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Settings {
    /// Substeps per second of game time. A frame of 1/60 s takes 4 of the
    /// default 240, and a frame of 1/30 s takes 8.
    pub rate: f32,
    /// The most substeps in one frame. A longer frame loses the rest of its
    /// time.
    pub most: u32,
    /// How many times each substep goes over the shape's constraints. One
    /// is the small-steps method. A second pass halves the stretch of a tree
    /// in a fast flip, and it costs about a third more.
    pub passes: u32,
    /// Gravity in the world (metres per second squared, +Z up).
    pub gravity: Vec3,
    /// An anchor that moves farther than this in one frame (metres) has
    /// teleported. Every point then goes back to its target pose.
    pub teleport: f32,
    /// The most that the anchor's movement through the world can move a
    /// point in one substep (metres).
    pub anchor_travel: f32,
    /// The most that the anchor's turn in the world can turn a point in one
    /// substep (radians).
    pub anchor_turn: f32,
    /// How much longer than its rest path a tether may grow, as a share of
    /// that path.
    pub tether_slack: f32,
    /// How far a free point may move in one substep (metres). This stops a
    /// point from passing through a thin capsule in a fast move.
    pub max_step: f32,
    /// Whether each step records what each substep did, in `Solver::trace`
    /// (`trace`). It is a debugging aid.
    pub trace: bool,
}

/// What a debugging overlay draws: the points, their targets and the
/// capsules, in the anchor's space, and the anchor's place in the world.
#[derive(Debug, Clone)]
pub struct View {
    pub anchor: Frame,
    pub positions: Vec<Vec3>,
    pub targets: Vec<Vec3>,
    pub parents: Vec<Option<usize>>,
    /// Each capsule's ends and radius, and whether it is a leg's.
    pub capsules: Vec<([Vec3; 2], f32, bool)>,
    pub links: Vec<(usize, usize)>,
}

impl Default for Settings {
    fn default() -> Settings {
        Settings {
            rate: 240.0,
            most: 32,
            passes: 1,
            gravity: Vec3::new(0.0, 0.0, -9.81),
            teleport: 5.0,
            anchor_travel: 0.05,
            anchor_turn: 15f32.to_radians(),
            tether_slack: 0.02,
            max_step: 0.08,
            trace: false,
        }
    }
}

/// What one frame's step gives back.
#[derive(Debug, Clone, Copy)]
pub struct Step<'a> {
    /// Each loose bone's new local turn, by its skeleton node. A bone keeps
    /// its translation.
    pub turns: &'a [(usize, Quat)],
    pub substeps: u32,
    /// Whether the points went back to their target pose. This happens on
    /// the first frame and after a teleport. It also happens after time ran
    /// backwards, after a frame of more than a second, and when a point was
    /// no longer a finite number.
    pub reset: bool,
}

/// A distance kept between two points.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Distance {
    pub(crate) a: usize,
    pub(crate) b: usize,
    pub(crate) rest: f32,
    /// Metres per newton.
    compliance: f32,
}

/// A free point kept away from a bone of another tree of its garment: the
/// segment between the points `ends`. Without it, two chains of a robe can
/// pass through each other, and the skin between them folds through
/// itself. A robe that turns upside down would gather its hem into a point
/// above the body, where a real skirt keeps its hem a ring around it.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Apart {
    pub(crate) point: usize,
    pub(crate) ends: [usize; 2],
    /// The least distance kept (metres): `APART` of the rest distance.
    pub(crate) least: f32,
}

/// The share of its rest distance from another tree's bone that a point
/// keeps at least. Two chains of a robe may close up to half their rest
/// gap, and no more.
const APART: f32 = 0.5;
/// A point keeps away from the bones of another tree that are at most this
/// many times as far at rest as that tree's nearest bone. These are the
/// bones beside it, not those far above or below.
const APART_NEAR: f32 = 1.5;

/// One point, worked out against the skeleton.
#[derive(Debug, Clone, Copy)]
struct Resolved {
    parent: Option<usize>,
    /// The node whose joint the point is. A tip has none.
    node: Option<usize>,
    /// The node a root hangs on.
    attach: Option<usize>,
    /// Where the point sits in its parent bone's frame at rest.
    offset: Vec3,
    weight: f32,
    max_distance: f32,
    stretch: f32,
    drive: Option<f32>,
    damping: f32,
    inertia: f32,
    thickness: f32,
    hang: f32,
}

/// The frame before, in its own anchor space.
#[derive(Debug, Clone)]
struct Before {
    anchor: Frame,
    targets: Vec<Vec3>,
    capsules: Vec<Capsule>,
}

/// The state of one body's loose points.
#[derive(Debug, Clone)]
pub struct Solver {
    settings: Settings,
    points: Vec<Resolved>,
    parents: Vec<Option<usize>>,
    anchor: usize,
    body: Option<Body>,
    children: Vec<Vec<usize>>,
    lengths: Vec<Distance>,
    bends: Vec<Distance>,
    /// Pairs of children of one point, kept at their rest distance so that
    /// one turn of the parent bone can aim at all of them.
    siblings: Vec<Distance>,
    links: Vec<Distance>,
    apart: Vec<Apart>,
    /// The pinned point each point is tethered to, with the tether's length.
    tethers: Vec<Distance>,
    collision: Option<Collision>,
    positions: Vec<Vec3>,
    previous: Vec<Vec3>,
    targets: Vec<Vec3>,
    targets_now: Vec<Vec3>,
    /// Each point where the animated skeleton puts it, before the hang.
    posed: Vec<Vec3>,
    capsules: Vec<Capsule>,
    capsules_now: Vec<Capsule>,
    frames: Vec<Frame>,
    turns: Vec<(usize, Quat)>,
    before: Option<Before>,
    /// Game time not yet stepped, less than one substep.
    carry: f32,
    record: Record,
    trace: Vec<String>,
    anchor_world: Frame,
}

/// Each node's frame in model space, from each node's frame in its parent's.
fn walk(parents: &[Option<usize>], locals: &[Frame], frames: &mut Vec<Frame>) {
    frames.clear();
    for (node, &local) in locals.iter().enumerate() {
        let frame = match parents[node] {
            Some(parent) => frames[parent].then(local),
            None => local,
        };
        frames.push(frame);
    }
}

fn finite(values: &[f32]) -> bool {
    values
        .iter()
        .all(|value| value.is_finite() && *value >= 0.0)
}

impl Solver {
    /// A solver for `rig` on `skeleton`, colliding with `body` when given.
    /// It fails when a node comes before its parent, when a point names a
    /// node the skeleton lacks or comes before the point it hangs from, or
    /// when a value is not a finite number in its range.
    pub fn new(
        rig: &Rig,
        skeleton: &Skeleton,
        body: Option<Body>,
        settings: Settings,
    ) -> Result<Solver, String> {
        let nodes = skeleton.parents.len();
        if skeleton.rest.len() != nodes || skeleton.anchor >= nodes {
            return Err("the skeleton's lists do not match".into());
        }
        if (0..nodes).any(|node| skeleton.parents[node].is_some_and(|parent| parent >= node)) {
            return Err("a node comes before its parent".into());
        }
        let mut rest = Vec::new();
        walk(&skeleton.parents, &skeleton.rest, &mut rest);
        let mut point_of = vec![None; nodes];
        let mut points: Vec<Resolved> = Vec::with_capacity(rig.points.len());
        let mut at: Vec<Vec3> = Vec::with_capacity(rig.points.len());
        for (index, point) in rig.points.iter().enumerate() {
            let values = [
                point.freedom,
                point.max_distance,
                point.stretch,
                point.bend,
                point.drive.unwrap_or(0.0),
                point.damping,
                point.inertia,
                point.thickness,
                point.hang,
            ];
            if !finite(&values) || point.freedom > 1.0 || point.inertia > 1.0 || point.hang > 1.0 {
                return Err(format!("point {index} has a value out of its range"));
            }
            let (parent, node, attach, offset, place) = match point.place {
                Place::Joint(node) => {
                    if node >= nodes || point_of[node].is_some() {
                        return Err(format!(
                            "point {index} names node {node} twice or not at all"
                        ));
                    }
                    point_of[node] = Some(index);
                    let above = skeleton.parents[node];
                    let parent = above.and_then(|above| point_of[above]);
                    let attach = if parent.is_some() { None } else { above };
                    (
                        parent,
                        Some(node),
                        attach,
                        skeleton.rest[node].translation,
                        rest[node].translation,
                    )
                }
                Place::Tip {
                    point: bone,
                    offset,
                } => {
                    let Some(node) = points.get(bone).and_then(|p| p.node) else {
                        return Err(format!("point {index} is a tip after no earlier joint"));
                    };
                    if !offset.is_finite() {
                        return Err(format!("point {index} has an offset that is not finite"));
                    }
                    (Some(bone), None, None, offset, rest[node].point(offset))
                }
            };
            points.push(Resolved {
                parent,
                node,
                attach,
                offset,
                weight: if parent.is_some() { point.freedom } else { 0.0 },
                max_distance: point.max_distance,
                stretch: point.stretch,
                drive: point.drive,
                damping: point.damping,
                inertia: point.inertia,
                thickness: point.thickness,
                hang: point.hang,
            });
            at.push(place);
        }
        let count = points.len();
        let mut children = vec![Vec::new(); count];
        let mut lengths = Vec::new();
        let mut bends = Vec::new();
        let mut tethers = Vec::new();
        let mut root_of: Vec<usize> = Vec::with_capacity(count);
        let mut path = vec![0.0f32; count];
        for (index, point) in points.iter().enumerate() {
            let Some(parent) = point.parent else {
                root_of.push(index);
                continue;
            };
            children[parent].push(index);
            root_of.push(root_of[parent]);
            path[index] = path[parent] + point.offset.length();
            lengths.push(Distance {
                a: parent,
                b: index,
                rest: point.offset.length(),
                compliance: point.stretch,
            });
            if let Some(grandparent) = points[parent].parent {
                bends.push(Distance {
                    a: grandparent,
                    b: index,
                    rest: at[index].distance(at[grandparent]),
                    compliance: rig.points[index].bend,
                });
            }
            tethers.push(Distance {
                a: root_of[index],
                b: index,
                rest: path[index] * (1.0 + settings.tether_slack),
                compliance: 0.0,
            });
        }
        let mut siblings = Vec::new();
        for kids in &children {
            for pair in kids.windows(2) {
                let (a, b) = (pair[0], pair[1]);
                siblings.push(Distance {
                    a,
                    b,
                    rest: at[a].distance(at[b]),
                    compliance: points[a].stretch.min(points[b].stretch),
                });
            }
        }
        let mut links = Vec::new();
        for link in &rig.links {
            if link.a == link.b || link.a >= count || link.b >= count || !finite(&[link.compliance])
            {
                return Err(format!(
                    "the link from {} to {} is not sound",
                    link.a, link.b
                ));
            }
            links.push(Distance {
                a: link.a,
                b: link.b,
                rest: at[link.a].distance(at[link.b]),
                compliance: link.compliance,
            });
        }
        let apart = apart(&points, &links, &root_of, &at);
        let collision = body.as_ref().map(|body| {
            let mut posed = Vec::new();
            body.pose(|node| rest[node], &mut posed);
            let resting: Vec<Resting> = rig
                .points
                .iter()
                .enumerate()
                .map(|(index, point)| Resting {
                    point,
                    at: at[index],
                    parent_at: points[index].parent.map(|parent| at[parent]),
                    pinned: points[index].weight == 0.0,
                })
                .collect();
            contacts(&resting, body, &posed, &skeleton.parents)
        });
        Ok(Solver {
            settings,
            parents: skeleton.parents.clone(),
            anchor: skeleton.anchor,
            body,
            children,
            lengths,
            bends,
            siblings,
            links,
            apart,
            tethers,
            collision,
            positions: vec![Vec3::ZERO; count],
            previous: vec![Vec3::ZERO; count],
            targets: vec![Vec3::ZERO; count],
            targets_now: vec![Vec3::ZERO; count],
            posed: Vec::with_capacity(count),
            capsules: Vec::new(),
            capsules_now: Vec::new(),
            frames: Vec::with_capacity(nodes),
            turns: Vec::with_capacity(count),
            before: None,
            carry: 0.0,
            record: Record::default(),
            trace: Vec::new(),
            anchor_world: Frame::IDENTITY,
            points,
        })
    }

    /// One JSON line for each substep of the last step, when the settings
    /// ask for a trace (`Settings::trace`). Each step starts the list
    /// again.
    pub fn trace(&self) -> &[String] {
        &self.trace
    }

    /// How many points it moves.
    pub fn len(&self) -> usize {
        self.points.len()
    }

    pub fn is_empty(&self) -> bool {
        self.points.is_empty()
    }

    /// Forgets the points' motion. The next step puts them at their target
    /// pose.
    pub fn reset(&mut self) {
        self.before = None;
    }

    /// Steps the points by `seconds` of game time. `model` is the model's
    /// place in the world, and `locals` is each skeleton node's animated
    /// frame in its parent's frame. The floor is a height in the world.
    pub fn step(
        &mut self,
        seconds: f32,
        model: Frame,
        locals: &[Frame],
        floor: Option<f32>,
    ) -> Step<'_> {
        assert_eq!(
            locals.len(),
            self.parents.len(),
            "a step has one frame for each node of the skeleton"
        );
        let mut frames = std::mem::take(&mut self.frames);
        walk(&self.parents, locals, &mut frames);
        let anchor = model.then(frames[self.anchor]);
        self.anchor_world = anchor;
        self.trace.clear();
        let to_anchor = frames[self.anchor].inverse();
        let down = (anchor.rotation.inverse() * self.settings.gravity).normalize_or_zero();
        self.target_pose(&frames, to_anchor, down);
        let mut capsules = std::mem::take(&mut self.capsules);
        if let Some(body) = &self.body {
            body.pose(|node| to_anchor.then(frames[node]), &mut capsules);
        }
        let (substeps, reset) = self.advance(seconds, anchor, &capsules, floor);
        self.capsules = capsules;
        self.aim(&frames, to_anchor, locals);
        self.frames = frames;
        Step {
            turns: &self.turns,
            substeps,
            reset,
        }
    }

    /// The substeps that `seconds` brings due, and whether the points went
    /// back to their targets instead.
    fn advance(
        &mut self,
        seconds: f32,
        anchor: Frame,
        capsules: &[Capsule],
        floor: Option<f32>,
    ) -> (u32, bool) {
        let jumped = self.before.as_ref().is_none_or(|before| {
            before.anchor.translation.distance(anchor.translation) > self.settings.teleport
        });
        let bad_time = !seconds.is_finite() || !(0.0..=1.0).contains(&seconds);
        if jumped || bad_time {
            self.start_over(anchor, capsules);
            return (0, true);
        }
        let rate = self.settings.rate;
        let leftover = self.carry;
        let due = ((leftover + seconds) * rate).floor().max(0.0);
        let substeps = due.min(self.settings.most.max(1) as f32) as u32;
        let dropped = due > substeps as f32;
        self.carry = if dropped {
            0.0
        } else {
            (leftover + seconds - substeps as f32 / rate).clamp(0.0, 1.0 / rate)
        };
        let share = |k: u32| -> f32 {
            if dropped {
                k as f32 / substeps as f32
            } else if seconds > 0.0 {
                ((k as f32 / rate - leftover) / seconds).clamp(0.0, 1.0)
            } else {
                1.0
            }
        };
        let before = self
            .before
            .take()
            .expect("a step after the first has the frame before");
        for k in 0..substeps {
            self.substep(&before, (anchor, capsules, floor), (share(k), share(k + 1)));
        }
        let finite = self
            .positions
            .iter()
            .chain(&self.previous)
            .all(|p| p.is_finite());
        self.before = Some(before);
        if !finite {
            self.start_over(anchor, capsules);
            return (substeps, true);
        }
        self.remember(anchor, capsules);
        (substeps, false)
    }

    /// Every point at its target, at rest.
    fn start_over(&mut self, anchor: Frame, capsules: &[Capsule]) {
        self.positions.clone_from(&self.targets);
        self.previous.clone_from(&self.targets);
        self.carry = 0.0;
        self.remember(anchor, capsules);
    }

    fn remember(&mut self, anchor: Frame, capsules: &[Capsule]) {
        let before = self.before.get_or_insert_with(|| Before {
            anchor,
            targets: Vec::new(),
            capsules: Vec::new(),
        });
        before.anchor = anchor;
        before.targets.clone_from(&self.targets);
        before.capsules.clear();
        before.capsules.extend_from_slice(capsules);
    }

    /// Each point where the animated skeleton puts it, in anchor space,
    /// then each free point's bone turned by its share of the way toward
    /// `down`, from the top of each tree.
    fn target_pose(&mut self, frames: &[Frame], to_anchor: Frame, down: Vec3) {
        self.posed.clear();
        for point in &self.points {
            self.posed.push(match (point.node, point.parent) {
                (Some(node), _) => to_anchor.then(frames[node]).translation,
                (None, Some(bone)) => {
                    let node = self.points[bone].node.expect("a tip hangs after a joint");
                    to_anchor.then(frames[node]).point(point.offset)
                }
                (None, None) => unreachable!("a point is a joint or a tip"),
            });
        }
        for (index, point) in self.points.iter().enumerate() {
            self.targets[index] = match point.parent.filter(|_| point.weight > 0.0) {
                Some(parent) => {
                    let bone = self.posed[index] - self.posed[parent];
                    self.targets[parent] + hung(bone, down, point.hang)
                }
                None => self.posed[index],
            };
        }
    }

    /// One substep, from share `from` to share `to` of the way from the
    /// frame before to this one.
    fn substep(
        &mut self,
        before: &Before,
        (anchor, capsules, floor): (Frame, &[Capsule], Option<f32>),
        (from, to): (f32, f32),
    ) {
        let h = 1.0 / self.settings.rate;
        let anchor_from = blend(before.anchor, anchor, from);
        let anchor_to = blend(before.anchor, anchor, to);
        let delta = anchor_to.inverse().then(anchor_from);
        let felt = self.limited(delta);
        let tracing = self.settings.trace;
        if tracing {
            self.record.start(self.points.len());
            self.record.from.extend_from_slice(&self.capsules_now);
        }
        for index in 0..self.points.len() {
            let inertia = self.points[index].inertia;
            if self.points[index].weight == 0.0 || inertia == 0.0 {
                continue;
            }
            for p in [&mut self.positions[index], &mut self.previous[index]] {
                *p += (felt.point(*p) - *p) * inertia;
            }
        }
        for (index, target) in self.targets_now.iter_mut().enumerate() {
            *target = before.targets[index].lerp(self.targets[index], to);
        }
        self.capsules_now.clear();
        if before.capsules.len() == capsules.len() {
            self.capsules_now
                .extend(before.capsules.iter().zip(capsules).map(|(a, b)| Capsule {
                    ends: [a.ends[0].lerp(b.ends[0], to), a.ends[1].lerp(b.ends[1], to)],
                    radius: a.radius + (b.radius - a.radius) * to,
                }));
        }
        let gravity = anchor_to.rotation.inverse() * self.settings.gravity;
        for index in 0..self.points.len() {
            let start = self.positions[index];
            if self.points[index].weight == 0.0 {
                self.previous[index] = start;
                self.positions[index] = self.targets_now[index];
                continue;
            }
            let keep = (1.0 - self.points[index].damping * h).max(0.0);
            let velocity = (start - self.previous[index]) / h * keep + gravity * h;
            if tracing {
                self.record.speed[index] = velocity.length();
            }
            self.previous[index] = start;
            self.positions[index] = start + velocity * h;
        }
        for _ in 0..self.settings.passes.max(1) {
            self.constrain(h);
        }
        self.limit(anchor_to, floor);
        if tracing {
            let bones: Vec<Option<(usize, f32)>> = (self.points.iter())
                .map(|p| p.parent.map(|parent| (parent, p.offset.length())))
                .collect();
            let points = trace::Points {
                positions: &self.positions,
                previous: &self.previous,
                targets: &self.targets_now,
                bones: &bones,
            };
            let line = trace::line(
                &self.record,
                (from, to),
                (delta, felt),
                &points,
                &self.capsules_now,
            );
            self.trace.push(line);
        }
    }

    pub fn view(&self) -> View {
        let body = self.body.as_ref();
        View {
            anchor: self.anchor_world,
            positions: self.positions.clone(),
            targets: self.targets.clone(),
            parents: self.points.iter().map(|p| p.parent).collect(),
            capsules: self
                .capsules
                .iter()
                .enumerate()
                .map(|(i, c)| (c.ends, c.radius, body.is_some_and(|b| b.parts[i].leg)))
                .collect(),
            links: self.links.iter().map(|l| (l.a, l.b)).collect(),
        }
    }

    /// The points, their values and what each meets, for the log.
    pub fn describe(&self) -> String {
        let mut out = String::new();
        let names = |i: usize| {
            self.body
                .as_ref()
                .map_or(String::new(), |b| b.parts[i].name.to_string())
        };
        for (i, p) in self.points.iter().enumerate() {
            out += &format!(
                "point {i}: node {:?} parent {:?} weight {:.2} max_distance {:.3} drive {:?} damping {:.2} inertia {:.2} thickness {:.3} rest_len {:.3}\n",
                p.node,
                p.parent,
                p.weight,
                p.max_distance,
                p.drive,
                p.damping,
                p.inertia,
                p.thickness,
                p.offset.length()
            );
            if let Some(c) = &self.collision {
                let list = |l: &Vec<crate::body::Contact>| {
                    l.iter()
                        .map(|k| format!("{}({:.3})", names(k.capsule), k.radius))
                        .collect::<Vec<_>>()
                        .join(" ")
                };
                out += &format!(
                    "   contacts: {}\n   segments: {}\n   backstop: {}\n",
                    list(&c.contacts[i]),
                    list(&c.segments[i]),
                    c.backstops[i].map_or("none".into(), |k| format!(
                        "{}({:.3})",
                        names(k.capsule),
                        k.radius
                    ))
                );
            }
        }
        for l in &self.links {
            out += &format!(
                "link {}-{} rest {:.3} compliance {}\n",
                l.a, l.b, l.rest, l.compliance
            );
        }
        for l in &self.bends {
            out += &format!(
                "bend {}-{} rest {:.3} compliance {}\n",
                l.a, l.b, l.rest, l.compliance
            );
        }
        for l in &self.siblings {
            out += &format!("sibling {}-{} rest {:.3}\n", l.a, l.b, l.rest);
        }
        for a in &self.apart {
            out += &format!(
                "apart {} from {}-{} least {:.3}\n",
                a.point, a.ends[0], a.ends[1], a.least
            );
        }
        out
    }

    /// The anchor's movement in one substep, as the points feel it. It
    /// moves and turns them no more than the settings allow.
    fn limited(&self, delta: Frame) -> Frame {
        let travel = delta
            .translation
            .clamp_length_max(self.settings.anchor_travel);
        let (axis, angle) = delta.rotation.to_axis_angle();
        let angle = if angle > std::f32::consts::PI {
            angle - std::f32::consts::TAU
        } else {
            angle
        };
        let turn = angle.clamp(-self.settings.anchor_turn, self.settings.anchor_turn);
        let rotation = if axis.is_finite() && axis.length_squared() > 0.5 {
            Quat::from_axis_angle(axis, turn)
        } else {
            Quat::IDENTITY
        };
        Frame::new(travel, rotation)
    }

    /// One pass over the constraints that keep the shape. They are the
    /// lengths, the bends, the siblings, the links, the pull toward the
    /// target and the tethers.
    fn constrain(&mut self, h: f32) {
        let scale = 1.0 / (h * h);
        for list in [&self.lengths, &self.siblings, &self.bends, &self.links] {
            for &distance in list {
                keep_distance(&mut self.positions, &self.points, distance, scale);
            }
        }
        for index in 0..self.apart.len() {
            keep_apart(&mut self.positions, &self.points, self.apart[index]);
        }
        for index in 0..self.points.len() {
            if let Some(drive) = self.points[index].drive {
                let target = self.targets_now[index];
                let was = self.positions[index];
                self.toward(index, target, drive * scale);
                if self.settings.trace {
                    self.record.drive[index] += was.distance(self.positions[index]);
                }
            }
        }
        // One pass left a tree 11% longer or shorter in a flip, after the
        // bends and links had pulled on its lengths.
        for &distance in &self.lengths {
            keep_distance(&mut self.positions, &self.points, distance, scale);
        }
        for index in 0..self.tethers.len() {
            self.tether(self.tethers[index]);
        }
    }

    /// The limits come last. First the most a point moves in one substep,
    /// then the hard limits, which the step limit never undoes. They are
    /// the tethers and the maximum distance from the target, so a point
    /// outside either comes back at once. Then the bones against the
    /// capsules, and each point against the backstop, the capsules and the
    /// floor. A point that hangs from a pinned root ends at its bone's
    /// length from the root, so a push from a capsule only turns its bone.
    /// A capsule over the root would otherwise push that point out along
    /// the bone, past its tether. Such a point is pushed out and put back at
    /// its length a few times (`ROOT_PASSES`), so that it ends outside a
    /// capsule next to the root.
    fn limit(&mut self, anchor: Frame, floor: Option<f32>) {
        let tracing = self.settings.trace;
        for index in 0..self.points.len() {
            if self.points[index].weight == 0.0 {
                continue;
            }
            let start = self.previous[index];
            let was = self.positions[index];
            self.positions[index] = start + (was - start).clamp_length_max(self.settings.max_step);
            if tracing {
                self.record.max_step[index] = was.distance(self.positions[index]);
            }
        }
        for index in 0..self.tethers.len() {
            self.tether(self.tethers[index]);
        }
        for index in 0..self.points.len() {
            if self.points[index].weight == 0.0 {
                continue;
            }
            let target = self.targets_now[index];
            let was = self.positions[index];
            self.positions[index] =
                target + (was - target).clamp_length_max(self.points[index].max_distance);
            if tracing {
                self.record.max_distance[index] = was.distance(self.positions[index]);
            }
        }
        if tracing {
            self.record.before_bones(&self.positions);
        }
        self.segments();
        if tracing {
            self.record.after_bones(&self.positions);
        }
        let floor = floor.map(|height| {
            let up = anchor.rotation.inverse() * Vec3::Z;
            (up, height - anchor.translation.z)
        });
        for index in 0..self.points.len() {
            if self.points[index].weight == 0.0 {
                continue;
            }
            let mut at = self.positions[index];
            if let Some(collision) = &self.collision {
                let target = self.targets_now[index];
                if let Some(contact) = collision.backstops[index]
                    && let Some(capsule) = self.capsules_now.get(contact.capsule)
                {
                    let was = at;
                    at = backstop(at, target, capsule, contact.radius);
                    if tracing {
                        self.record.backstop[index] = was != at;
                    }
                }
                for contact in &collision.contacts[index] {
                    if let Some(capsule) = self.capsules_now.get(contact.capsule) {
                        let was = at;
                        at = pushed_out(at, target, capsule, contact.radius);
                        if tracing {
                            let touch = touch(
                                (self.previous[index], was, at),
                                (contact.capsule, contact.radius),
                                (self.record.from.get(contact.capsule), capsule),
                            );
                            self.record.touches[index].extend(touch);
                        }
                    }
                }
            }
            if let Some((up, height)) = floor {
                let above = at.dot(up) - height;
                let margin = self.points[index].thickness.max(FLOOR_MARGIN);
                if above < margin {
                    at += up * (margin - above);
                    if tracing {
                        self.record.floor[index] = margin - above;
                    }
                }
            }
            if let Some(parent) = self.points[index].parent
                && self.points[parent].weight == 0.0
            {
                let root = self.positions[parent];
                let length = self.points[index].offset.length();
                let target = self.targets_now[index];
                for pass in 0..=ROOT_PASSES {
                    if let Some(way) = (at - root).try_normalize() {
                        at = root + way * length;
                    }
                    let Some(collision) = self.collision.as_ref().filter(|_| pass < ROOT_PASSES)
                    else {
                        break;
                    };
                    let was = at;
                    for contact in &collision.contacts[index] {
                        if let Some(capsule) = self.capsules_now.get(contact.capsule) {
                            at = pushed_out(at, target, capsule, contact.radius);
                        }
                    }
                    if was == at {
                        break;
                    }
                }
            }
            self.positions[index] = at;
        }
    }

    /// Each bone kept out of the capsules it meets. The bone's closest
    /// point to a capsule's axis is pushed out to the bone's radius. The two
    /// ends share the push by how near that point is to each, and by how
    /// freely each moves. Neither end moves farther than the depth. Without
    /// that bound, a contact next to a pinned root would throw the free end
    /// by the depth divided by its tiny share of the bone, metres away in
    /// one substep. Such a contact is then only partly cleared, and the free
    /// point's own capsule test does the rest.
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
                let near_parent = self.points[parent].weight * (1.0 - share);
                let near_point = self.points[index].weight * share;
                let sum = (near_parent * (1.0 - share) + near_point * share)
                    .max(near_parent)
                    .max(near_point);
                if sum < 1e-6 {
                    continue;
                }
                let push = way * ((contact.radius - distance) / sum);
                self.positions[parent] += push * near_parent;
                self.positions[index] += push * near_point;
                if self.settings.trace {
                    self.record.pushes[index].push(Push {
                        capsule: contact.capsule,
                        share,
                        depth: contact.radius - distance,
                        point: (push * near_point).length(),
                        parent: (push * near_parent).length(),
                    });
                }
            }
        }
    }

    /// A tether only pulls a point toward its root, never pushes it away.
    fn tether(&mut self, tether: Distance) {
        if self.positions[tether.b].distance(self.positions[tether.a]) > tether.rest {
            keep_distance(&mut self.positions, &self.points, tether, 0.0);
        }
    }

    /// The XPBD pull of a point toward a place that does not move.
    fn toward(&mut self, index: usize, place: Vec3, compliance: f32) {
        let weight = self.points[index].weight;
        let between = self.positions[index] - place;
        let length = between.length();
        if weight == 0.0 || length < 1e-9 {
            return;
        }
        let lambda = -length / (weight + compliance);
        self.positions[index] += between / length * (lambda * weight);
    }

    /// Each loose bone, from the top of each tree down, turns to point at
    /// its children's solved places. It keeps the twist about its own length
    /// that the animation gave it.
    fn aim(&mut self, frames: &[Frame], to_anchor: Frame, locals: &[Frame]) {
        self.turns.clear();
        let count = self.points.len();
        let mut world: Vec<Quat> = Vec::with_capacity(count);
        let mut heads: Vec<Vec3> = Vec::with_capacity(count);
        for index in 0..count {
            let point = self.points[index];
            let Some(node) = point.node else {
                world.push(Quat::IDENTITY);
                heads.push(self.positions[index]);
                continue;
            };
            let local = locals[node];
            let (above, head) = match (point.parent, point.attach) {
                (Some(parent), _) => (
                    world[parent],
                    heads[parent] + world[parent] * local.translation,
                ),
                (None, attach) => {
                    let frame =
                        attach.map_or(Frame::IDENTITY, |attach| to_anchor.then(frames[attach]));
                    (frame.rotation, frame.point(local.translation))
                }
            };
            let animated = (above * local.rotation).normalize();
            let mut swing = Vec4::ZERO;
            let mut first: Option<Quat> = None;
            for &child in &self.children[index] {
                let offset = match self.points[child].node {
                    Some(node) => locals[node].translation,
                    None => self.points[child].offset,
                };
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
                swing += Vec4::from(arc) * offset.length();
            }
            let new = match swing.try_normalize() {
                Some(swing) => (Quat::from_vec4(swing) * animated).normalize(),
                None => animated,
            };
            if !self.children[index].is_empty() {
                self.turns.push((node, (above.inverse() * new).normalize()));
            }
            world.push(new);
            heads.push(head);
        }
    }

    #[cfg(feature = "probes")]
    pub(crate) fn positions(&self) -> &[Vec3] {
        &self.positions
    }

    #[cfg(feature = "probes")]
    pub(crate) fn targets(&self) -> &[Vec3] {
        &self.targets
    }

    /// Each point's length from its parent, and each point's tether.
    #[cfg(feature = "probes")]
    pub(crate) fn measures(&self) -> (&[Distance], &[Distance]) {
        (&self.lengths, &self.tethers)
    }

    #[cfg(feature = "probes")]
    pub(crate) fn max_distance(&self, index: usize) -> f32 {
        self.points[index].max_distance
    }

    #[cfg(feature = "probes")]
    pub(crate) fn collision(&self) -> Option<&Collision> {
        self.collision.as_ref()
    }

    #[cfg(feature = "probes")]
    pub(crate) fn capsules(&self) -> &[Capsule] {
        &self.capsules
    }
}

/// The XPBD distance constraint between two points.
fn keep_distance(positions: &mut [Vec3], points: &[Resolved], distance: Distance, scale: f32) {
    let Distance {
        a,
        b,
        rest,
        compliance,
    } = distance;
    let (wa, wb) = (points[a].weight, points[b].weight);
    let between = positions[b] - positions[a];
    let length = between.length();
    if wa + wb == 0.0 || length < 1e-9 {
        return;
    }
    let way = between / length;
    let lambda = -(length - rest) / (wa + wb + compliance * scale);
    positions[a] -= way * (lambda * wa);
    positions[b] += way * (lambda * wb);
}

/// The free points kept away from the bones of the other trees of their
/// garment (`Apart`). The trees of a garment are those that links join,
/// directly or through other trees. `root_of` is each point's root, which
/// names its tree, and `at` each point's rest place.
fn apart(points: &[Resolved], links: &[Distance], root_of: &[usize], at: &[Vec3]) -> Vec<Apart> {
    let mut garment: Vec<usize> = (0..points.len()).collect();
    let find = |garment: &mut Vec<usize>, mut tree: usize| {
        while garment[tree] != tree {
            garment[tree] = garment[garment[tree]];
            tree = garment[tree];
        }
        tree
    };
    for link in links {
        let (a, b) = (
            find(&mut garment, root_of[link.a]),
            find(&mut garment, root_of[link.b]),
        );
        garment[a.max(b)] = a.min(b);
    }
    let roots: Vec<usize> = (0..points.len()).filter(|&p| root_of[p] == p).collect();
    let bones_of = |root: usize| -> Vec<[usize; 2]> {
        (0..points.len())
            .filter(|&index| root_of[index] == root)
            .filter_map(|index| Some([points[index].parent?, index]))
            .collect()
    };
    let mut out = Vec::new();
    for &own in &roots {
        for &other in &roots {
            if own == other || find(&mut garment, own) != find(&mut garment, other) {
                continue;
            }
            let bones = bones_of(other);
            for point in (0..points.len()).filter(|&p| root_of[p] == own && points[p].weight > 0.0)
            {
                let rest: Vec<f32> = bones
                    .iter()
                    .map(|&[q0, q1]| {
                        at[point].distance(closest_on_segment(at[point], at[q0], at[q1]))
                    })
                    .collect();
                let nearest = rest.iter().copied().fold(f32::MAX, f32::min);
                for (&ends, &rest) in bones.iter().zip(&rest) {
                    if rest <= nearest * APART_NEAR && rest > 0.0 {
                        out.push(Apart {
                            point,
                            ends,
                            least: rest * APART,
                        });
                    }
                }
            }
        }
    }
    out
}

/// A point pushed off a segment to its least distance. The point and the
/// segment's two ends share the push by how freely each moves, the ends by
/// how near each is to the segment's closest point.
fn keep_apart(positions: &mut [Vec3], points: &[Resolved], apart: Apart) {
    let Apart {
        point,
        ends: [q0, q1],
        least,
    } = apart;
    let (a, b) = (positions[q0], positions[q1]);
    let ab = b - a;
    let t = ((positions[point] - a).dot(ab) / ab.length_squared().max(1e-12)).clamp(0.0, 1.0);
    let between = positions[point] - (a + ab * t);
    let distance = between.length();
    if distance >= least || distance < 1e-9 {
        return;
    }
    let (wp, w0, w1) = (points[point].weight, points[q0].weight, points[q1].weight);
    let sum = wp + w0 * (1.0 - t) * (1.0 - t) + w1 * t * t;
    if sum < 1e-9 {
        return;
    }
    let push = between / distance * ((least - distance) / sum);
    positions[point] += push * wp;
    positions[q0] -= push * (w0 * (1.0 - t));
    positions[q1] -= push * (w1 * t);
}

/// How many more times a point under a pinned root is pushed out of its
/// capsules and put back at its bone's length. Each time turns its bone
/// further out of a capsule near the root, such as the trunk's over a
/// robe's waist.
const ROOT_PASSES: usize = 3;
/// How far a point keeps above the floor at least (metres).
const FLOOR_MARGIN: f32 = 0.01;
/// How deep in a trunk capsule, as a share of its radius, a point on the
/// far side from its target must be to count as having passed through.
const THROUGH: f32 = 0.5;

/// The trace's note of a point pushed from `was` to `at` by a capsule it
/// keeps `radius` from, having started the substep at `start`. A point is
/// noted when it came within 3 cm of the capsule, or when it passed through
/// the capsule's axis since the capsule's place at the start (`from`).
fn touch(
    (start, was, at): (Vec3, Vec3, Vec3),
    (index, radius): (usize, f32),
    (from, capsule): (Option<&Capsule>, &Capsule),
) -> Option<Touch> {
    let [a, b] = capsule.ends;
    let out = was - closest_on_segment(was, a, b);
    let gap = out.length() - radius;
    let crossed = from.is_some_and(|from| {
        let before = start - closest_on_segment(start, from.ends[0], from.ends[1]);
        let (_, on_path, on_axis) = closest_between((start, was), (a, b));
        before.dot(out) < 0.0 && on_path.distance(on_axis) < radius
    });
    (gap < 0.03 || crossed).then_some(Touch {
        capsule: index,
        gap,
        crossed,
        push: was.distance(at),
    })
}

/// `at` moved out of `capsule` to `radius` from its axis. It goes straight
/// out, or toward `target` when it sits on the axis.
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
/// the capsule on the far side. A fast move took it through the body.
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

/// `bone` turned by `share` of the angle between it and `down`. A bone that
/// points straight up turns about any axis across it.
fn hung(bone: Vec3, down: Vec3, share: f32) -> Vec3 {
    let Some(way) = bone.try_normalize() else {
        return bone;
    };
    if share == 0.0 || down == Vec3::ZERO {
        return bone;
    }
    let angle = way.angle_between(down);
    let axis = way
        .cross(down)
        .try_normalize()
        .unwrap_or_else(|| way.any_orthonormal_vector());
    Quat::from_axis_angle(axis, angle * share) * bone
}

/// The frame `share` of the way from `a` to `b`.
fn blend(a: Frame, b: Frame, share: f32) -> Frame {
    Frame::new(
        a.translation.lerp(b.translation, share),
        a.rotation.slerp(b.rotation, share),
    )
}
