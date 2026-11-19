//! Probes of the solver on rigs made here (no game data). The approved
//! test `loose_parts` runs them all.
//!
//! The five hard cases of the plan run on a chain and on a tree. They are a
//! fast spin, a flip, hanging upside down, a hard stop and a teleport. Each
//! checks that every point stays a finite number within its maximum
//! distance, that no length, tether or capsule is badly broken, and that
//! two runs give the same points bit for bit. The sweeps check that a chain
//! never ends a frame inside a capsule that moves through it, slowly or
//! fast.

use std::time::Instant;

use glam::{Quat, Vec3};

use crate::body::{Body, Capsule, Collision, Part, contacts};
use crate::{Frame, Link, Point, Pose, Report, Rig, Settings, Solver};

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
/// How far past its maximum distance a point may end a frame (metres): the
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
/// Where the body's trunk node sits in the root's space (metres).
const HIPS: Vec3 = Vec3::new(0.0, 0.0, 1.0);

/// A free point with the values the probes use.
fn point(parent: Option<usize>, offset: Vec3, at: Vec3) -> Point {
    Point {
        parent,
        offset,
        at,
        pinned: parent.is_none(),
        freedom: 1.0,
        max_distance: 0.6,
        stretch: 0.0,
        bend: 1e-3,
        drive: None,
        damping: 0.5,
        inertia: 0.5,
        radius: 0.02,
    }
}

/// A chain of six points hanging behind the back, 0.2 m apart: a cape.
/// The root hangs on the trunk node, which sits at `HIPS`. It hangs 3 cm
/// to the right of the middle: a chain in the body's middle plane can only
/// move in that plane, which no garment does.
fn chain() -> Rig {
    let top = Vec3::new(0.03, -0.14, 1.4);
    let mut points = vec![point(None, top - HIPS, top)];
    for index in 1..6 {
        let at = top - Vec3::Z * 0.2 * index as f32;
        points.push(point(Some(index - 1), Vec3::new(0.0, 0.0, -0.2), at));
    }
    reach_by_depth(Rig {
        points,
        links: Vec::new(),
    })
}

/// Each point may go twice its path from the root away from its target, so
/// a cape can swing all the way over.
fn reach_by_depth(mut rig: Rig) -> Rig {
    let mut path = vec![0.0f32; rig.points.len()];
    for index in 0..rig.points.len() {
        if let Some(parent) = rig.points[index].parent {
            path[index] = path[parent] + rig.points[index].offset.length();
        }
        rig.points[index].max_distance = 2.0 * path[index];
    }
    rig
}

/// A tree hanging behind the back: a root, one bone down, then two
/// branches of three points each, linked side to side. The branching is
/// that of KOTOR 1 Malak's cape.
fn tree() -> Rig {
    let top = Vec3::new(0.0, -0.14, 1.4);
    let fork = top - Vec3::Z * 0.2;
    let mut points = vec![
        point(None, top - HIPS, top),
        point(Some(0), fork - top, fork),
    ];
    let mut branches = [Vec::new(), Vec::new()];
    for (side, branch) in [-1.0f32, 1.0].into_iter().zip(&mut branches) {
        let mut parent = 1;
        let mut at = fork;
        for depth in 0..3 {
            let step = if depth == 0 {
                Vec3::new(0.12 * side, 0.0, -0.18)
            } else {
                Vec3::new(0.0, 0.0, -0.2)
            };
            at += step;
            points.push(point(Some(parent), step, at));
            parent = points.len() - 1;
            branch.push(parent);
        }
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
    reach_by_depth(Rig { points, links })
}

/// A trunk and two legs, all on the trunk's node (node 0, at `HIPS`).
fn figure() -> Body {
    let rest = Frame::new(HIPS, Quat::IDENTITY);
    let part = |ends: [Vec3; 2], radius: f32, trunk: bool| Part {
        node: 0,
        ends: ends.map(|end| end - HIPS),
        radius,
        parent: if trunk { None } else { Some(0) },
        trunk,
        rest,
    };
    Body {
        parts: vec![
            part(
                [Vec3::new(0.0, 0.0, 1.0), Vec3::new(0.0, 0.0, 1.45)],
                0.11,
                true,
            ),
            part(
                [Vec3::new(-0.1, 0.0, 0.95), Vec3::new(-0.1, 0.0, 0.1)],
                0.08,
                false,
            ),
            part(
                [Vec3::new(0.1, 0.0, 0.95), Vec3::new(0.1, 0.0, 0.1)],
                0.08,
                false,
            ),
        ],
    }
}

/// How a probe moves the character: the root in the world, and the trunk
/// node's frame in the root's space, at a time (seconds).
type Motion = fn(f32) -> (Frame, Frame);

/// The trunk turned by `turn` about a point `pivot` of the root's space.
fn turned_about(pivot: Vec3, turn: Quat) -> Frame {
    Frame::new(pivot + turn * (HIPS - pivot), turn)
}

fn upright() -> Frame {
    Frame::new(HIPS, Quat::IDENTITY)
}

/// Two turns and a half a second about the vertical, for 1.5 s, then
/// still: 1,440 degrees a second.
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
        Frame::new(Vec3::ZERO, Quat::IDENTITY),
        turned_about(Vec3::new(0.0, 0.0, 1.6), turn),
    )
}

/// Running at 10 m/s for 1 s, then stopped dead in one frame.
fn stop(t: f32) -> (Frame, Frame) {
    let root = Frame::new(Vec3::Y * 10.0 * t.min(1.0), Quat::IDENTITY);
    (root, upright())
}

/// Walking at 2 m/s, then 20 m away in one frame at 1 s.
fn teleport(t: f32) -> (Frame, Frame) {
    let jump = if t >= 1.0 - FRAME / 2.0 { 20.0 } else { 0.0 };
    let root = Frame::new(Vec3::X * (2.0 * t + jump), Quat::IDENTITY);
    (root, upright())
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
    /// The frame after the first reset past the first frame: the largest
    /// distance of a point from its target.
    after_reset: Option<f32>,
    /// World height of the last point less the root's, at the end.
    drop: f32,
    substeps: u32,
}

fn run(rig: &Rig, motion: Motion, seconds: f32, collide: bool) -> Run {
    let body = figure();
    let reach: Vec<Vec<usize>> = rig.points.iter().map(|_| vec![0]).collect();
    let collision = collide.then(|| contacts(rig, &body, &reach, 6));
    let mut solver =
        Solver::new(rig, Settings::default(), collision).expect("the probe's rig is sound");
    let count = rig.points.len();
    let turns = vec![Quat::IDENTITY; count];
    let mut capsules = Vec::new();
    let mut measured = Run {
        finite: true,
        hash: 0xcbf2_9ce4_8422_2325,
        ..Run::default()
    };
    let frames = (seconds / FRAME).round() as usize;
    for frame in 0..=frames {
        let t = frame as f32 * FRAME;
        let (root, trunk) = motion(t);
        let attach = vec![trunk; count];
        if collide {
            body.pose(|_| trunk, &mut capsules);
        }
        let pose = Pose {
            root,
            attach: &attach,
            turns: &turns,
            capsules: &capsules,
            floor: Some(0.0),
        };
        let report: Report = solver.step(if frame == 0 { 0.0 } else { FRAME }, &pose);
        measured.substeps += report.substeps;
        if report.reset {
            measured.resets.push(frame);
            if frame > 0 && measured.after_reset.is_none() {
                let off = solver
                    .positions()
                    .iter()
                    .zip(solver.targets())
                    .map(|(p, t)| p.distance(*t))
                    .fold(0.0, f32::max);
                measured.after_reset = Some(off);
            }
        }
        check(&solver, &capsules, &mut measured);
        let _ = solver.turns(&pose);
        if frame == frames {
            let world = |p: Vec3| root.point(p);
            measured.drop = world(solver.positions()[count - 1]).z - world(solver.positions()[0]).z;
        }
    }
    measured
}

/// Folds the frame's points into the run's measures.
fn check(solver: &Solver, capsules: &[Capsule], run: &mut Run) {
    let positions = solver.positions();
    let targets = solver.targets();
    for (index, (p, point)) in positions.iter().zip(solver.points()).enumerate() {
        run.finite &= p.is_finite();
        for v in p.to_array() {
            run.hash = (run.hash ^ u64::from(v.to_bits())).wrapping_mul(0x0100_0000_01b3);
        }
        run.past_reach = run
            .past_reach
            .max(p.distance(targets[index]) - point.max_distance);
        if let Some(parent) = point.parent {
            let rest = point.offset.length();
            let now = p.distance(positions[parent]);
            run.stretch = run.stretch.max((now - rest).abs() / rest);
        }
        let (root, length) = solver.tethers()[index];
        if root != index && length > 0.0 {
            let over = (p.distance(positions[root]) - length) / length;
            run.tether = run.tether.max(over);
        }
        if let Some(Collision { contacts, .. }) = solver.collision() {
            for contact in &contacts[index] {
                let [a, b] = capsules[contact.capsule].ends;
                let depth = contact.radius - p.distance(ragdoll::closest_on_segment(*p, a, b));
                run.inside = run.inside.max(depth);
            }
        }
    }
}

/// The checks every hard case makes, on one rig.
fn hard_case(name: &str, rig: &Rig, motion: Motion, seconds: f32) -> (Probe, Run) {
    let first = run(rig, motion, seconds, true);
    let second = run(rig, motion, seconds, true);
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
    for (shape, rig) in [("chain", chain()), ("tree", tree())] {
        let (probe, _) = hard_case(&format!("{shape} spin"), &rig, spin, 2.5);
        probes.push(probe);
        let (probe, _) = hard_case(&format!("{shape} flip"), &rig, flip, 2.0);
        probes.push(probe);
        let (mut probe, hung) = hard_case(&format!("{shape} hanging upside down"), &rig, hang, 2.5);
        // Upside down, the cloth falls past the root toward the floor.
        let length: f32 = rig.points.iter().skip(1).map(|p| p.offset.length()).sum();
        let falls = hung.drop < -0.5 * length.min(1.0);
        probe.pass &= falls;
        probe.expect =
            "the hard-case checks, and the last point ends at least half a metre below the root";
        probe.measured = format!(
            "{}; last point {:.2} m from the root's height",
            probe.measured, hung.drop
        );
        probes.push(probe);
        let (probe, _) = hard_case(&format!("{shape} hard stop"), &rig, stop, 2.0);
        probes.push(probe);
        let first = run(&rig, teleport, 2.0, true);
        let second = run(&rig, teleport, 2.0, true);
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

/// A shin that kicks back through a hanging chain at `speed` (metres a
/// second). The shin stands upright in the chain's plane, so the chain
/// cannot slide around it. The chain hangs from a root that does not move.
fn sweep(name: &str, speed: f32) -> Probe {
    let mut rig = chain();
    for point in &mut rig.points {
        point.at.x = 0.0;
    }
    rig.points[0].offset.x = 0.0;
    let leg = Body {
        parts: vec![Part {
            node: 0,
            ends: [Vec3::new(0.0, 0.0, -0.4), Vec3::new(0.0, 0.0, 0.4)],
            radius: 0.07,
            parent: None,
            trunk: false,
            rest: Frame::new(Vec3::new(0.0, 0.6, 0.7), Quat::IDENTITY),
        }],
    };
    let reach: Vec<Vec<usize>> = rig.points.iter().map(|_| vec![0]).collect();
    let collision = contacts(&rig, &leg, &reach, 6);
    let mut solver =
        Solver::new(&rig, Settings::default(), Some(collision)).expect("the probe's rig is sound");
    let count = rig.points.len();
    let attach = vec![upright(); count];
    let turns = vec![Quat::IDENTITY; count];
    let mut capsules = Vec::new();
    let mut measured = Run {
        finite: true,
        ..Run::default()
    };
    let travel = 1.05;
    let stopped = ((travel / speed) / FRAME).ceil() as usize;
    let frames = stopped + 30;
    let mut pushed = 0.0f32;
    let mut through = 0usize;
    for frame in 0..=frames {
        let t = frame as f32 * FRAME;
        let y = 0.6 - (speed * t).min(travel);
        let at = Frame::new(Vec3::new(0.0, y, 0.7), Quat::IDENTITY);
        leg.pose(|_| at, &mut capsules);
        let pose = Pose {
            root: Frame::IDENTITY,
            attach: &attach,
            turns: &turns,
            capsules: &capsules,
            floor: Some(0.0),
        };
        solver.step(if frame == 0 { 0.0 } else { FRAME }, &pose);
        check(&solver, &capsules, &mut measured);
        let behind = solver
            .positions()
            .iter()
            .zip(solver.targets())
            .map(|(p, t)| t.y - p.y)
            .fold(0.0, f32::max);
        pushed = behind;
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
            "inside {:.1} mm, {through} points in front of the shin when it stops, finite {}, {:.0} cm the farthest behind its rest place at the end",
            measured.inside * 1000.0,
            measured.finite,
            pushed * 100.0
        ),
        pass: measured.inside <= SLACK && measured.finite && through == 0,
    }
}

/// A chain swung hard through a still capsule: the trunk turns 69 degrees
/// about the vertical in two frames, so the lower half of the cape sweeps
/// sideways into a low post at about 5 m/s.
fn swing() -> Probe {
    const POST: Vec3 = Vec3::new(0.099, -0.099, 0.7);
    let rig = chain();
    let post = Body {
        parts: vec![Part {
            node: 1,
            ends: [Vec3::new(0.0, 0.0, -0.3), Vec3::new(0.0, 0.0, 0.3)],
            radius: 0.06,
            parent: None,
            trunk: false,
            rest: Frame::new(POST, Quat::IDENTITY),
        }],
    };
    let reach: Vec<Vec<usize>> = rig.points.iter().map(|_| vec![0]).collect();
    let collision = contacts(&rig, &post, &reach, 6);
    let mut solver =
        Solver::new(&rig, Settings::default(), Some(collision)).expect("the probe's rig is sound");
    let count = rig.points.len();
    let turns = vec![Quat::IDENTITY; count];
    let mut capsules = Vec::new();
    let mut measured = Run {
        finite: true,
        ..Run::default()
    };
    for frame in 0..=90 {
        let t = frame as f32 * FRAME;
        let share = (t / (2.0 * FRAME)).clamp(0.0, 1.0);
        let trunk = turned_about(Vec3::ZERO, Quat::from_rotation_z(share * 1.2));
        let attach = vec![trunk; count];
        let still = Frame::new(POST, Quat::IDENTITY);
        post.pose(|_| still, &mut capsules);
        let pose = Pose {
            root: Frame::IDENTITY,
            attach: &attach,
            turns: &turns,
            capsules: &capsules,
            floor: Some(0.0),
        };
        solver.step(if frame == 0 { 0.0 } else { FRAME }, &pose);
        check(&solver, &capsules, &mut measured);
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

/// Points that start inside a capsule at rest keep their place: no point
/// moves at the first frames of a body standing still.
fn start_inside() -> Probe {
    let rig = chain();
    let fat = Body {
        parts: vec![Part {
            node: 0,
            ends: [Vec3::new(0.0, 0.0, 0.0), Vec3::new(0.0, 0.0, 0.5)],
            radius: 0.2,
            parent: None,
            trunk: true,
            rest: upright(),
        }],
    };
    let reach: Vec<Vec<usize>> = rig.points.iter().map(|_| vec![0]).collect();
    let collision = contacts(&rig, &fat, &reach, 6);
    let mut solver =
        Solver::new(&rig, Settings::default(), Some(collision)).expect("the probe's rig is sound");
    let count = rig.points.len();
    let attach = vec![upright(); count];
    let turns = vec![Quat::IDENTITY; count];
    let mut capsules = Vec::new();
    fat.pose(|_| upright(), &mut capsules);
    let mut moved = 0.0f32;
    for frame in 0..=30 {
        let pose = Pose {
            root: Frame::IDENTITY,
            attach: &attach,
            turns: &turns,
            capsules: &capsules,
            floor: Some(0.0),
        };
        solver.step(if frame == 0 { 0.0 } else { FRAME }, &pose);
        moved = solver
            .positions()
            .iter()
            .zip(solver.targets())
            .map(|(p, t)| p.distance(*t))
            .fold(moved, f32::max);
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
    let rig = tree();
    let mut solver =
        Solver::new(&rig, Settings::default(), None).expect("the probe's rig is sound");
    let count = rig.points.len();
    let turns = vec![Quat::IDENTITY; count];
    let mut worst = 0.0f32;
    for frame in 0..=60 {
        let (root, trunk) = flip(0.5 + frame as f32 * FRAME);
        let attach = vec![trunk; count];
        let pose = Pose {
            root,
            attach: &attach,
            turns: &turns,
            capsules: &[],
            floor: None,
        };
        solver.step(if frame == 0 { 0.0 } else { FRAME }, &pose);
        let new = solver.turns(&pose);
        // The bones walked with their new turns from the attach frame.
        let mut world = Vec::with_capacity(count);
        let mut heads: Vec<Vec3> = Vec::with_capacity(count);
        for (index, point) in rig.points.iter().enumerate() {
            let (above, head) = match point.parent {
                None => (attach[index].rotation, attach[index].point(point.offset)),
                Some(parent) => (world[parent], heads[parent] + world[parent] * point.offset),
            };
            world.push(above * new[index]);
            heads.push(head);
        }
        // Each bone with one child points straight at it; a fork aims
        // between its children.
        for (index, point) in rig.points.iter().enumerate() {
            let Some(parent) = point.parent else { continue };
            let siblings = rig
                .points
                .iter()
                .filter(|p| p.parent == Some(parent))
                .count();
            if siblings > 1 {
                continue;
            }
            let was = (solver.positions()[index] - heads[parent]).normalize();
            let now = (heads[index] - heads[parent]).normalize();
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

/// Every probe of the solver.
pub fn all() -> Vec<Probe> {
    let mut probes = hard_cases();
    probes.push(sweep("capsule kicks through a chain at 1 m/s", 1.0));
    probes.push(sweep("capsule kicks through a chain at 6 m/s", 6.0));
    probes.push(swing());
    probes.push(start_inside());
    probes.push(aimed());
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
    /// with the turns back to bones.
    pub per_body_frame_us: f64,
}

/// Times a body of four trees like the probes' tree (32 points, about
/// twice the 15 of a KOTOR 2 robe) through a flip with the three capsules
/// of the probes, over `frames` frames of 1/60 s.
pub fn cost(frames: usize) -> Cost {
    let one = tree();
    let mut rig = Rig::default();
    for copy in 0..4 {
        let base = rig.points.len();
        let turn = Quat::from_rotation_z(copy as f32 * std::f32::consts::FRAC_PI_2);
        for point in &one.points {
            let mut point = *point;
            point.parent = point.parent.map(|p| p + base);
            point.at = HIPS + turn * (point.at - HIPS);
            point.offset = turn * point.offset;
            rig.points.push(point);
        }
        for link in &one.links {
            rig.links.push(Link {
                a: link.a + base,
                b: link.b + base,
                ..*link
            });
        }
    }
    let body = figure();
    let reach: Vec<Vec<usize>> = rig.points.iter().map(|_| vec![0]).collect();
    let collision = contacts(&rig, &body, &reach, 6);
    let mut solver =
        Solver::new(&rig, Settings::default(), Some(collision)).expect("the probe's rig is sound");
    let count = rig.points.len();
    let turns = vec![Quat::IDENTITY; count];
    let mut attach = vec![upright(); count];
    let mut capsules = Vec::new();
    let mut substeps = 0u64;
    let started = Instant::now();
    for frame in 0..frames {
        let t = (frame % 120) as f32 * FRAME;
        let (root, trunk) = flip(t);
        attach.fill(trunk);
        body.pose(|_| trunk, &mut capsules);
        let pose = Pose {
            root,
            attach: &attach,
            turns: &turns,
            capsules: &capsules,
            floor: Some(0.0),
        };
        if frame % 120 == 0 {
            solver.reset();
        }
        substeps += u64::from(solver.step(FRAME, &pose).substeps);
        std::hint::black_box(solver.turns(&pose));
    }
    let seconds = started.elapsed().as_secs_f64();
    Cost {
        points: count,
        capsules: body.parts.len(),
        per_point_substep_ns: seconds * 1e9 / (substeps.max(1) as f64 * count as f64),
        per_body_frame_us: seconds * 1e6 / frames.max(1) as f64,
    }
}
