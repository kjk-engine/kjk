//! Probes of the solver on rigs made here (no game data). The approved
//! test `loose_parts` runs them all.
//!
//! The five hard cases run on a chain and on a tree. They are a fast spin,
//! a flip, hanging upside down, a hard stop and a teleport. Each checks that
//! every point stays a finite number within its maximum distance, that no
//! length, tether or capsule is badly broken, and that two runs give the
//! same points bit for bit. The sweeps check that a chain never ends a
//! frame inside a capsule that moves through it, slowly or fast.
//!
//! Four probes hold the faults that once left a robe flared or bunched in
//! play. A capsule that passes over a pinned root must not throw the point
//! below it. A point outside its maximum distance must come back within one
//! substep, whatever the step limit. When the node the game places turns
//! and the bones turn back, so that the pelvis does not turn in the world,
//! the points must not swing. A fast spin of the pelvis inside a skeleton
//! that does not move must be felt only by the points' inertia share.

use std::time::Instant;

use glam::{Quat, Vec3};

use crate::body::{Body, Collision, Part};
use crate::{Frame, Link, Place, Point, Rig, Settings, Skeleton, Solver};

/// One probe: what it checks, what it measured, whether it passed.
#[derive(Debug, Clone)]
pub struct Probe {
    pub name: String,
    pub expect: &'static str,
    pub measured: String,
    pub pass: bool,
}

/// The frame time of every probe (seconds).
const FRAME: f32 = 1.0 / 60.0;
/// How far inside a capsule a point may end a frame and still pass
/// (metres).
const SLACK: f32 = 0.002;
/// How far past its maximum distance a point may end a frame (metres). The
/// collision comes after the maximum distance and can push a point past it.
const REACH: f32 = 0.02;
/// The most that a segment may be longer or shorter than at rest in any
/// frame, as a share of its length. The tree's flip reaches 11% for one
/// frame. The bones keep their lengths whatever the points do, so a
/// stretched segment only aims its bone a little wrong.
const STRETCH: f32 = 0.15;
/// The most that a point may be farther from its root than its tether, as
/// a share of the tether. The tree's flip reaches 5.4% for one frame.
const TETHER: f32 = 0.06;
/// Where the trunk node sits in the root node's space (metres).
const HIPS: Vec3 = Vec3::new(0.0, 0.0, 1.0);

/// The probes' skeleton has a root node, the trunk under it, a free node
/// under the root for a capsule that moves on its own, then the garment's
/// nodes.
const ROOT: usize = 0;
const TRUNK: usize = 1;
const FREE: usize = 2;
const GARMENT: usize = 3;

/// A rig and the rest frames of its garment nodes, each with its parent.
struct Garment {
    rig: Rig,
    nodes: Vec<(usize, Frame)>,
}

impl Garment {
    fn skeleton(&self) -> Skeleton {
        self.skeleton_on(ROOT)
    }

    /// The skeleton, solved in the space of `anchor`.
    fn skeleton_on(&self, anchor: usize) -> Skeleton {
        let mut parents = vec![None, Some(ROOT), Some(ROOT)];
        let mut rest = vec![
            Frame::IDENTITY,
            Frame::new(HIPS, Quat::IDENTITY),
            Frame::IDENTITY,
        ];
        for &(parent, frame) in &self.nodes {
            parents.push(Some(parent));
            rest.push(frame);
        }
        Skeleton {
            parents,
            rest,
            anchor,
        }
    }

    /// Each node's frame in its parent's, with the trunk and the free node
    /// where the probe puts them and the garment at rest.
    fn locals(&self, trunk: Frame, free: Frame, locals: &mut Vec<Frame>) {
        locals.clear();
        locals.extend([Frame::IDENTITY, trunk, free]);
        locals.extend(self.nodes.iter().map(|&(_, frame)| frame));
    }
}

/// A point with the values the probes use. It reaches the trunk.
fn point(place: Place) -> Point {
    Point {
        place,
        freedom: 1.0,
        max_distance: 0.0,
        stretch: 0.0,
        bend: 1e-3,
        drive: None,
        damping: 0.5,
        inertia: 0.5,
        thickness: 0.02,
        hang: 0.0,
        reach: vec![TRUNK],
    }
}

/// Each point may go twice its path from the root away from its target, so
/// that a cape can swing all the way over.
fn reach_by_depth(garment: &mut Garment) {
    let mut path = vec![0.0f32; garment.rig.points.len()];
    for index in 0..garment.rig.points.len() {
        let (parent, length) = match garment.rig.points[index].place {
            Place::Joint(node) => {
                let parent_node = garment.nodes[node - GARMENT].0;
                let parent =
                    (0..index).find(|&p| garment.rig.points[p].place == Place::Joint(parent_node));
                (parent, garment.nodes[node - GARMENT].1.translation.length())
            }
            Place::Tip { point, offset } => (Some(point), offset.length()),
        };
        if let Some(parent) = parent {
            path[index] = path[parent] + length;
        }
        garment.rig.points[index].max_distance = 2.0 * path[index];
    }
}

/// A chain of five bones and a tip hanging behind the back, 0.2 m apart: a
/// cape. It hangs 3 cm to the right of the middle. A chain in the body's
/// middle plane can only move in that plane, which no garment does.
fn chain(right: f32) -> Garment {
    let top = Vec3::new(right, -0.14, 1.4);
    let down = Frame::new(Vec3::new(0.0, 0.0, -0.2), Quat::IDENTITY);
    let mut nodes = vec![(TRUNK, Frame::new(top - HIPS, Quat::IDENTITY))];
    for index in 1..5 {
        nodes.push((GARMENT + index - 1, down));
    }
    let mut points: Vec<Point> = (0..5).map(|i| point(Place::Joint(GARMENT + i))).collect();
    points.push(point(Place::Tip {
        point: 4,
        offset: down.translation,
    }));
    let mut garment = Garment {
        rig: Rig {
            points,
            links: Vec::new(),
        },
        nodes,
    };
    reach_by_depth(&mut garment);
    garment
}

/// A tree hanging behind the back. A root, one bone down, then two
/// branches of two bones and a tip each, linked side to side. The branching
/// is that of KOTOR 1 Malak's cape.
fn tree() -> Garment {
    let top = Vec3::new(0.0, -0.14, 1.4);
    let down = Vec3::new(0.0, 0.0, -0.2);
    let at = |v: Vec3| Frame::new(v, Quat::IDENTITY);
    let mut nodes = vec![(TRUNK, at(top - HIPS)), (GARMENT, at(down))];
    let mut points = vec![
        point(Place::Joint(GARMENT)),
        point(Place::Joint(GARMENT + 1)),
    ];
    let mut branches = [Vec::new(), Vec::new()];
    for (side, branch) in [-1.0f32, 1.0].into_iter().zip(&mut branches) {
        let first = GARMENT + nodes.len();
        nodes.push((GARMENT + 1, at(Vec3::new(0.12 * side, 0.0, -0.18))));
        nodes.push((first, at(down)));
        for node in [first, first + 1] {
            points.push(point(Place::Joint(node)));
            branch.push(points.len() - 1);
        }
        points.push(point(Place::Tip {
            point: points.len() - 1,
            offset: down,
        }));
        branch.push(points.len() - 1);
    }
    let links = branches[0]
        .iter()
        .zip(&branches[1])
        .map(|(&a, &b)| Link {
            a,
            b,
            compliance: 1e-4,
        })
        .collect();
    let mut garment = Garment {
        rig: Rig { points, links },
        nodes,
    };
    reach_by_depth(&mut garment);
    garment
}

/// A capsule of the probes' body, its ends in model space at rest.
fn part(
    node: usize,
    rest: Frame,
    ends: [Vec3; 2],
    radius: f32,
    parent: Option<usize>,
    trunk: bool,
) -> Part {
    let to_node = rest.inverse();
    Part {
        node,
        ends: ends.map(|end| to_node.point(end)),
        radius,
        parent,
        trunk,
        name: "",
        leg: !trunk,
    }
}

/// A trunk and two legs, all on the trunk node.
fn figure() -> Body {
    let rest = Frame::new(HIPS, Quat::IDENTITY);
    let leg = |x: f32| [Vec3::new(x, 0.0, 0.95), Vec3::new(x, 0.0, 0.1)];
    Body {
        parts: vec![
            part(
                TRUNK,
                rest,
                [Vec3::new(0.0, 0.0, 1.0), Vec3::new(0.0, 0.0, 1.45)],
                0.11,
                None,
                true,
            ),
            part(TRUNK, rest, leg(-0.1), 0.08, Some(0), false),
            part(TRUNK, rest, leg(0.1), 0.08, Some(0), false),
        ],
    }
}

/// How a probe moves the character at a time (seconds). It gives the root
/// node in the world and the trunk node's frame in the root node's space.
type Motion = fn(f32) -> (Frame, Frame);

/// The trunk turned by `turn` about a point `pivot` of the root's space.
fn turned_about(pivot: Vec3, turn: Quat) -> Frame {
    Frame::new(pivot + turn * (HIPS - pivot), turn)
}

fn upright() -> Frame {
    Frame::new(HIPS, Quat::IDENTITY)
}

/// Two turns a second about the vertical for 1.5 s, then still. That is
/// 1,440 degrees a second.
fn spin(t: f32) -> (Frame, Frame) {
    let turn = 4.0 * std::f32::consts::PI * t.min(1.5);
    (
        Frame::new(Vec3::ZERO, Quat::from_rotation_z(turn)),
        upright(),
    )
}

/// Running forward at 10 m/s, the body turns end over end once in 0.6 s,
/// starting at 0.5 s, about a point 1 m up.
fn flip(t: f32) -> (Frame, Frame) {
    let root = Frame::new(Vec3::Y * 10.0 * t.min(1.5), Quat::IDENTITY);
    let share = ((t - 0.5) / 0.6).clamp(0.0, 1.0);
    let turn = Quat::from_rotation_x(-std::f32::consts::TAU * share);
    (root, turned_about(Vec3::new(0.0, 0.0, 1.0), turn))
}

/// The body turns upside down in 0.4 s about a point 1.6 m up, as if it
/// hung by its feet, and stays so.
fn hang(t: f32) -> (Frame, Frame) {
    let share = (t / 0.4).clamp(0.0, 1.0);
    let turn = Quat::from_rotation_x(std::f32::consts::PI * share);
    (
        Frame::IDENTITY,
        turned_about(Vec3::new(0.0, 0.0, 1.6), turn),
    )
}

/// Running at 10 m/s for 1 s, then stopped dead in one frame.
fn stop(t: f32) -> (Frame, Frame) {
    (
        Frame::new(Vec3::Y * 10.0 * t.min(1.0), Quat::IDENTITY),
        upright(),
    )
}

/// Walking at 2 m/s, then 20 m away in one frame at 1 s.
fn teleport(t: f32) -> (Frame, Frame) {
    let jump = if t >= 1.0 - FRAME / 2.0 { 20.0 } else { 0.0 };
    (
        Frame::new(Vec3::X * (2.0 * t + jump), Quat::IDENTITY),
        upright(),
    )
}

/// What a run measured, frame by frame.
#[derive(Debug, Default, Clone)]
struct Run {
    /// Bits of every point at the end of every frame.
    hash: u64,
    finite: bool,
    /// The farthest a point went past its maximum distance (metres).
    past_reach: f32,
    /// The largest change of a segment's length, as a share of it.
    stretch: f32,
    /// The longest a tether grew past its length, as a share of it.
    tether: f32,
    /// The deepest a point ended a frame inside one of its capsules.
    inside: f32,
    resets: Vec<usize>,
    /// On the first reset after the first frame, the farthest a point was
    /// from its target.
    after_reset: Option<f32>,
    /// The last point's height in the world less the root point's, at the
    /// end.
    drop: f32,
    substeps: u32,
}

impl Run {
    fn new() -> Run {
        Run {
            finite: true,
            hash: 0xcbf2_9ce4_8422_2325,
            ..Run::default()
        }
    }
}

fn solver(garment: &Garment, body: Option<Body>) -> Solver {
    Solver::new(&garment.rig, &garment.skeleton(), body, Settings::default())
        .expect("the probe's rig is sound")
}

fn run(garment: &Garment, motion: Motion, seconds: f32) -> Run {
    let mut solver = solver(garment, Some(figure()));
    let mut locals = Vec::new();
    let mut measured = Run::new();
    let frames = (seconds / FRAME).round() as usize;
    for frame in 0..=frames {
        let (root, trunk) = motion(frame as f32 * FRAME);
        garment.locals(trunk, Frame::IDENTITY, &mut locals);
        let elapsed = if frame == 0 { 0.0 } else { FRAME };
        let step = solver.step(elapsed, root, &locals, Some(0.0));
        measured.substeps += step.substeps;
        if step.reset {
            measured.resets.push(frame);
            if frame > 0 && measured.after_reset.is_none() {
                measured.after_reset = Some(off_target(&solver));
            }
        }
        check(&solver, &mut measured);
        if frame == frames {
            let points = solver.positions();
            let world = |p: Vec3| root.point(p);
            measured.drop = world(points[points.len() - 1]).z - world(points[0]).z;
        }
    }
    measured
}

/// The farthest a point is from its target (metres).
fn off_target(solver: &Solver) -> f32 {
    solver
        .positions()
        .iter()
        .zip(solver.targets())
        .map(|(p, t)| p.distance(*t))
        .fold(0.0, f32::max)
}

/// Folds the frame's points into the run's measures.
fn check(solver: &Solver, run: &mut Run) {
    let positions = solver.positions();
    let targets = solver.targets();
    for (index, p) in positions.iter().enumerate() {
        run.finite &= p.is_finite();
        for v in p.to_array() {
            run.hash = (run.hash ^ u64::from(v.to_bits())).wrapping_mul(0x0100_0000_01b3);
        }
        run.past_reach = run
            .past_reach
            .max(p.distance(targets[index]) - solver.max_distance(index));
    }
    let (lengths, tethers) = solver.measures();
    for length in lengths {
        let now = positions[length.b].distance(positions[length.a]);
        run.stretch = run.stretch.max((now - length.rest).abs() / length.rest);
    }
    for tether in tethers {
        if tether.rest > 0.0 {
            let now = positions[tether.b].distance(positions[tether.a]);
            run.tether = run.tether.max((now - tether.rest) / tether.rest);
        }
    }
    if let Some(Collision { contacts, .. }) = solver.collision() {
        let capsules = solver.capsules();
        for (index, p) in positions.iter().enumerate() {
            for contact in &contacts[index] {
                let [a, b] = capsules[contact.capsule].ends;
                let depth = contact.radius - p.distance(ragdoll::closest_on_segment(*p, a, b));
                run.inside = run.inside.max(depth);
            }
        }
    }
}

/// The checks every hard case makes, on one rig.
fn hard_case(name: &str, garment: &Garment, motion: Motion, seconds: f32) -> (Probe, Run) {
    let first = run(garment, motion, seconds);
    let second = run(garment, motion, seconds);
    let pass = first.finite
        && first.past_reach <= REACH
        && first.stretch <= STRETCH
        && first.tether <= TETHER
        && first.inside <= SLACK
        && first.hash == second.hash
        && first.resets == [0];
    let probe = Probe {
        name: name.to_string(),
        expect: "every point finite and within 2 cm of its maximum distance, lengths within 15%, tethers within 6%, no point 2 mm inside a capsule, one reset at the start, two runs equal bit for bit",
        measured: format!(
            "finite {}, past reach {:.1} mm, stretch {:.1}%, tether {:.1}%, inside {:.1} mm, resets {:?}, runs equal {}, {} substeps",
            first.finite,
            first.past_reach * 1000.0,
            first.stretch * 100.0,
            first.tether * 100.0,
            first.inside * 1000.0,
            first.resets,
            first.hash == second.hash,
            first.substeps,
        ),
        pass,
    };
    (probe, first)
}

/// The five hard cases, on a chain and on a tree.
fn hard_cases() -> Vec<Probe> {
    let mut probes = Vec::new();
    for (shape, garment) in [("chain", chain(0.03)), ("tree", tree())] {
        probes.push(hard_case(&format!("{shape} spin"), &garment, spin, 2.5).0);
        probes.push(hard_case(&format!("{shape} flip"), &garment, flip, 2.0).0);
        let (mut probe, hung) =
            hard_case(&format!("{shape} hanging upside down"), &garment, hang, 2.5);
        probe.pass &= hung.drop < -0.5;
        probe.expect =
            "the hard-case checks, and the last point ends at least half a metre below the root";
        probe.measured = format!(
            "{}; last point {:.2} m from the root's height",
            probe.measured, hung.drop
        );
        probes.push(probe);
        probes.push(hard_case(&format!("{shape} hard stop"), &garment, stop, 2.0).0);
        let first = run(&garment, teleport, 2.0);
        let second = run(&garment, teleport, 2.0);
        let jump = (1.0 / FRAME).round() as usize;
        let after = first.after_reset.unwrap_or(f32::MAX);
        probes.push(Probe {
            name: format!("{shape} teleport"),
            expect: "a reset at the start and on the frame of the 20 m jump, every point at its target then, no other reset, the hard-case checks, two runs equal",
            measured: format!(
                "resets {:?}, off target after the jump {:.1} mm, finite {}, stretch {:.1}%, tether {:.1}%, runs equal {}",
                first.resets,
                after * 1000.0,
                first.finite,
                first.stretch * 100.0,
                first.tether * 100.0,
                first.hash == second.hash
            ),
            pass: first.resets == [0, jump]
                && after < 1e-5
                && first.finite
                && first.past_reach <= REACH
                && first.stretch <= STRETCH
                && first.tether <= TETHER
                && first.hash == second.hash,
        });
    }
    probes
}

/// A body of one capsule on the free node, `ends` in the free node's frame.
fn on_free(ends: [Vec3; 2], radius: f32) -> Body {
    Body {
        parts: vec![Part {
            node: FREE,
            ends,
            radius,
            parent: None,
            trunk: false,
            name: "",
            leg: true,
        }],
    }
}

/// A shin that kicks back through a hanging chain at `speed` (metres a
/// second). The shin stands upright in the chain's plane, so the chain
/// cannot slide around it. The chain hangs from a root that does not move.
fn sweep(name: &str, speed: f32) -> Probe {
    let mut garment = chain(0.0);
    for point in &mut garment.rig.points {
        point.reach = vec![FREE];
    }
    let shin = on_free([Vec3::new(0.0, 0.0, -0.4), Vec3::new(0.0, 0.0, 0.4)], 0.07);
    let mut solver = solver(&garment, Some(shin));
    let mut locals = Vec::new();
    let mut measured = Run::new();
    let travel = 1.05;
    let stopped = ((travel / speed) / FRAME).ceil() as usize;
    let mut through = 0usize;
    for frame in 0..=stopped + 30 {
        let y = 0.6 - (speed * frame as f32 * FRAME).min(travel);
        let at = Frame::new(Vec3::new(0.0, y, 0.7), Quat::IDENTITY);
        garment.locals(upright(), at, &mut locals);
        solver.step(
            if frame == 0 { 0.0 } else { FRAME },
            Frame::IDENTITY,
            &locals,
            Some(0.0),
        );
        check(&solver, &mut measured);
        if frame == stopped {
            through = solver
                .positions()
                .iter()
                .filter(|p| (0.3..1.1).contains(&p.z) && p.y > y)
                .count();
        }
    }
    Probe {
        name: name.to_string(),
        expect: "no point ends a frame 2 mm inside the capsule, every point finite, and when the shin stops every point at its height is still behind it, so none passed through",
        measured: format!(
            "inside {:.1} mm, {through} points in front of the shin when it stops, finite {}",
            measured.inside * 1000.0,
            measured.finite,
        ),
        pass: measured.inside <= SLACK && measured.finite && through == 0,
    }
}

/// A chain swung hard through a still post. The trunk turns 69 degrees
/// about the vertical in two frames, so the lower half of the cape sweeps
/// sideways into a low post at about 5 m/s.
fn swing() -> Probe {
    let mut garment = chain(0.03);
    for point in &mut garment.rig.points {
        point.reach = vec![FREE];
    }
    let post = on_free([Vec3::new(0.0, 0.0, -0.3), Vec3::new(0.0, 0.0, 0.3)], 0.06);
    let still = Frame::new(Vec3::new(0.099, -0.099, 0.7), Quat::IDENTITY);
    let mut solver = solver(&garment, Some(post));
    let mut locals = Vec::new();
    let mut measured = Run::new();
    for frame in 0..=90 {
        let share = (frame as f32 / 2.0).clamp(0.0, 1.0);
        let trunk = turned_about(Vec3::ZERO, Quat::from_rotation_z(share * 1.2));
        garment.locals(trunk, still, &mut locals);
        solver.step(
            if frame == 0 { 0.0 } else { FRAME },
            Frame::IDENTITY,
            &locals,
            Some(0.0),
        );
        check(&solver, &mut measured);
    }
    Probe {
        name: "chain swung fast into a capsule".into(),
        expect: "no point ends a frame 2 mm inside the capsule, every point finite",
        measured: format!(
            "inside {:.1} mm, finite {}",
            measured.inside * 1000.0,
            measured.finite
        ),
        pass: measured.inside <= SLACK && measured.finite,
    }
}

/// Points that start inside a capsule at rest keep their place. No point
/// moves in the first half second of a body standing still.
fn start_inside() -> Probe {
    let garment = chain(0.03);
    let fat = Body {
        parts: vec![part(
            TRUNK,
            upright(),
            [Vec3::ZERO, Vec3::new(0.0, 0.0, 0.5)],
            0.2,
            None,
            true,
        )],
    };
    let mut solver = solver(&garment, Some(fat));
    let mut locals = Vec::new();
    let mut moved = 0.0f32;
    for frame in 0..=30 {
        garment.locals(upright(), Frame::IDENTITY, &mut locals);
        solver.step(
            if frame == 0 { 0.0 } else { FRAME },
            Frame::IDENTITY,
            &locals,
            Some(0.0),
        );
        moved = moved.max(off_target(&solver));
    }
    Probe {
        name: "points that start inside a capsule stay put".into(),
        expect: "a chain whose rest pose is 6 cm inside a capsule moves less than 1 cm in half a second",
        measured: format!("moved {:.1} mm", moved * 1000.0),
        pass: moved < 0.01,
    }
}

/// The solved points turned back into bones put each child where the
/// solver put it.
fn aimed() -> Probe {
    let garment = tree();
    let skeleton = garment.skeleton();
    let mut solver = solver(&garment, None);
    let mut locals = Vec::new();
    let mut worst = 0.0f32;
    for frame in 0..=60 {
        let (root, trunk) = flip(0.5 + frame as f32 * FRAME);
        garment.locals(trunk, Frame::IDENTITY, &mut locals);
        let turns = solver
            .step(if frame == 0 { 0.0 } else { FRAME }, root, &locals, None)
            .turns
            .to_vec();
        for (node, turn) in turns {
            locals[node].rotation = turn;
        }
        let mut frames: Vec<Frame> = Vec::new();
        for (node, &local) in locals.iter().enumerate() {
            frames.push(match skeleton.parents[node] {
                Some(parent) => frames[parent].then(local),
                None => local,
            });
        }
        let points = &garment.rig.points;
        let place = |index: usize| match points[index].place {
            Place::Joint(node) => frames[node].translation,
            Place::Tip { point, offset } => match points[point].place {
                Place::Joint(node) => frames[node].point(offset),
                Place::Tip { .. } => unreachable!("a tip hangs after a joint"),
            },
        };
        // Each bone with one child points straight at it. A fork aims
        // between its children.
        for (index, point) in points.iter().enumerate() {
            let parent = match point.place {
                Place::Tip { point, .. } => point,
                Place::Joint(node) => {
                    let Some(parent) = (0..index).find(|&p| {
                        Some(points[p].place) == skeleton.parents[node].map(Place::Joint)
                    }) else {
                        continue;
                    };
                    parent
                }
            };
            let siblings = (0..points.len())
                .filter(|&other| match points[other].place {
                    Place::Tip { point, .. } => point == parent,
                    Place::Joint(node) => {
                        Some(points[parent].place) == skeleton.parents[node].map(Place::Joint)
                    }
                })
                .count();
            if siblings > 1 {
                continue;
            }
            let head = place(parent);
            let was = (solver.positions()[index] - head).normalize();
            let now = (place(index) - head).normalize();
            worst = worst.max(was.angle_between(now));
        }
    }
    Probe {
        name: "bones aim at the solved points".into(),
        expect: "each bone with one child points at the child's solved place within 0.5 degrees, through a flip",
        measured: format!("worst {:.3} degrees", worst.to_degrees()),
        pass: worst.to_degrees() < 0.5,
    }
}

/// A capsule lying across the body comes down over the chain's pinned root
/// in half a second and stays there, its axis 2 cm below the root, so that
/// it cuts the first bone a tenth of the way down. This is the belly over a
/// robe's roots when the body bends. Without a bound on the bone's push,
/// such a contact threw the point below the root 1.7 m away.
fn over_the_root() -> Probe {
    let mut garment = chain(0.03);
    for point in &mut garment.rig.points {
        point.reach = vec![FREE];
    }
    let bar = on_free([Vec3::new(-0.4, 0.0, 0.0), Vec3::new(0.4, 0.0, 0.0)], 0.08);
    let mut solver = solver(&garment, Some(bar));
    let mut locals = Vec::new();
    let mut measured = Run::new();
    let mut farthest = 0.0f32;
    for frame in 0..=90 {
        let share = (frame as f32 * FRAME / 0.5).min(1.0);
        let at = Frame::new(Vec3::new(0.03, -0.14, 2.0 - 0.62 * share), Quat::IDENTITY);
        garment.locals(upright(), at, &mut locals);
        solver.step(
            if frame == 0 { 0.0 } else { FRAME },
            Frame::IDENTITY,
            &locals,
            Some(0.0),
        );
        check(&solver, &mut measured);
        farthest = farthest.max(off_target(&solver));
    }
    Probe {
        name: "capsule over a pinned root".into(),
        expect: "no point goes 10 cm from its target, every point finite, tethers within 6%",
        measured: format!(
            "farthest from its target {:.1} mm, tether {:.1}%, finite {}",
            farthest * 1000.0,
            measured.tether * 100.0,
            measured.finite
        ),
        pass: farthest < 0.1 && measured.finite && measured.tether <= TETHER,
    }
}

/// A chain allowed only 5 cm from its target goes through the flip one
/// substep per frame, with a step limit of 1 mm a substep. The flip moves
/// the targets far faster, so only the maximum distance keeps the points
/// near them.
fn hard_limit() -> Probe {
    let mut garment = chain(0.03);
    for point in &mut garment.rig.points {
        point.max_distance = point.max_distance.min(0.05);
    }
    let settings = Settings {
        max_step: 0.001,
        ..Settings::default()
    };
    let substep = 1.0 / settings.rate;
    let mut solver = Solver::new(&garment.rig, &garment.skeleton(), Some(figure()), settings)
        .expect("the probe's rig is sound");
    let mut locals = Vec::new();
    let mut measured = Run::new();
    let substeps = (1.5 / substep).round() as usize;
    for k in 0..=substeps {
        let (root, trunk) = flip(k as f32 * substep);
        garment.locals(trunk, Frame::IDENTITY, &mut locals);
        solver.step(if k == 0 { 0.0 } else { substep }, root, &locals, None);
        check(&solver, &mut measured);
    }
    Probe {
        name: "a point outside its maximum distance comes back at once".into(),
        expect: "after every substep of a flip, with a 1 mm step limit, every point is within 2 cm of its 5 cm maximum distance",
        measured: format!(
            "past reach {:.1} mm, finite {}",
            measured.past_reach * 1000.0,
            measured.finite
        ),
        pass: measured.past_reach <= REACH && measured.finite,
    }
}

/// How far from its target any point got, in metres, while `motion` moves
/// the character for `seconds`, solved in the space of `anchor`, with every
/// point's inertia share set to `inertia`.
fn farthest(motion: Motion, seconds: f32, anchor: usize, inertia: f32) -> f32 {
    let mut garment = chain(0.03);
    for point in &mut garment.rig.points {
        point.inertia = inertia;
    }
    let mut solver = Solver::new(
        &garment.rig,
        &garment.skeleton_on(anchor),
        Some(figure()),
        Settings::default(),
    )
    .expect("the probe's rig is sound");
    let mut locals = Vec::new();
    let mut farthest = 0.0f32;
    for frame in 0..=(seconds / FRAME).round() as usize {
        let (root, trunk) = motion(frame as f32 * FRAME);
        garment.locals(trunk, Frame::IDENTITY, &mut locals);
        solver.step(
            if frame == 0 { 0.0 } else { FRAME },
            root,
            &locals,
            Some(0.0),
        );
        farthest = farthest.max(off_target(&solver));
    }
    farthest
}

/// The placed node turns 165 degrees in one frame at 0.5 s, and the trunk
/// turns back by as much, so the body does not turn in the world. The cast
/// does this when a KOTOR clip takes over from a Jedi Academy move.
fn handoff(t: f32) -> (Frame, Frame) {
    let turn = if t >= 0.5 - FRAME / 2.0 {
        165f32.to_radians()
    } else {
        0.0
    };
    let back = Quat::from_rotation_z(-turn);
    (
        Frame::new(Vec3::ZERO, Quat::from_rotation_z(turn)),
        Frame::new(back * HIPS, back),
    )
}

/// The placed node stands still while the trunk spins 900 degrees a second
/// about the vertical for 1 s, as in a Jedi Academy spin.
fn spin_inside(t: f32) -> (Frame, Frame) {
    let turn = Quat::from_rotation_z(900f32.to_radians() * t.min(1.0));
    (Frame::IDENTITY, Frame::new(turn * HIPS, turn))
}

fn facing_handoff() -> Probe {
    let off = farthest(handoff, 1.5, TRUNK, 0.5);
    Probe {
        name: "a turn of the placed node that the bones undo".into(),
        expect: "with the trunk as the anchor, no point moves 1 cm from its target",
        measured: format!("farthest from its target {:.1} mm", off * 1000.0),
        pass: off < 0.01,
    }
}

fn spin_felt_by_inertia() -> Probe {
    let none = farthest(spin_inside, 1.5, TRUNK, 0.0);
    let half = farthest(spin_inside, 1.5, TRUNK, 0.5);
    let placed = farthest(spin_inside, 1.5, ROOT, 0.0);
    Probe {
        name: "a spin of the trunk is felt by the inertia share".into(),
        expect: "with the trunk as the anchor, a point with no inertia stays within 1 cm of its target and one with half swings at least 5 cm away",
        measured: format!(
            "inertia 0: {:.1} mm, inertia 0.5: {:.0} mm; solved in the placed node's space, inertia 0: {:.0} mm",
            none * 1000.0,
            half * 1000.0,
            placed * 1000.0
        ),
        pass: none < 0.01 && half > 0.05,
    }
}

/// Every probe of the solver.
pub fn all() -> Vec<Probe> {
    let mut probes = hard_cases();
    probes.push(sweep("capsule kicks through a chain at 1 m/s", 1.0));
    probes.push(sweep("capsule kicks through a chain at 6 m/s", 6.0));
    probes.push(swing());
    probes.push(start_inside());
    probes.push(aimed());
    probes.push(over_the_root());
    probes.push(hard_limit());
    probes.push(facing_handoff());
    probes.push(spin_felt_by_inertia());
    probes
}

/// What one body's solve costs on this machine.
#[derive(Debug, Clone, Copy)]
pub struct Cost {
    pub points: usize,
    pub capsules: usize,
    /// Nanoseconds per point per substep, with collision.
    pub per_point_substep_ns: f64,
    /// Microseconds per body per frame at 60 frames a second (4 substeps),
    /// with the skeleton's walk and the turns back to bones.
    pub per_body_frame_us: f64,
}

/// Times a body of four trees like the probes' tree (32 points, about
/// twice the 15 of a KOTOR 2 robe) through a flip with the three capsules
/// of the probes, over `frames` frames of 1/60 s.
pub fn cost(frames: usize) -> Cost {
    let one = tree();
    let mut garment = Garment {
        rig: Rig::default(),
        nodes: Vec::new(),
    };
    for copy in 0..4 {
        let turn = Quat::from_rotation_z(copy as f32 * std::f32::consts::FRAC_PI_2);
        let node_base = garment.nodes.len();
        let point_base = garment.rig.points.len();
        for &(parent, frame) in &one.nodes {
            let parent = if parent == TRUNK {
                TRUNK
            } else {
                parent + node_base
            };
            garment
                .nodes
                .push((parent, Frame::new(turn * frame.translation, frame.rotation)));
        }
        for point in &one.rig.points {
            let mut point = point.clone();
            point.place = match point.place {
                Place::Joint(node) => Place::Joint(node + node_base),
                Place::Tip { point, offset } => Place::Tip {
                    point: point + point_base,
                    offset: turn * offset,
                },
            };
            garment.rig.points.push(point);
        }
        for link in &one.rig.links {
            garment.rig.links.push(Link {
                a: link.a + point_base,
                b: link.b + point_base,
                ..*link
            });
        }
    }
    let body = figure();
    let capsules = body.parts.len();
    let mut solver = solver(&garment, Some(body));
    let mut locals = Vec::new();
    let mut substeps = 0u64;
    let started = Instant::now();
    for frame in 0..frames {
        let (root, trunk) = flip((frame % 120) as f32 * FRAME);
        garment.locals(trunk, Frame::IDENTITY, &mut locals);
        if frame % 120 == 0 {
            solver.reset();
        }
        let step = solver.step(FRAME, root, &locals, Some(0.0));
        substeps += u64::from(step.substeps);
        std::hint::black_box(step.turns);
    }
    let seconds = started.elapsed().as_secs_f64();
    let points = garment.rig.points.len();
    Cost {
        points,
        capsules,
        per_point_substep_ns: seconds * 1e9 / (substeps.max(1) as f64 * points as f64),
        per_body_frame_us: seconds * 1e6 / frames.max(1) as f64,
    }
}
