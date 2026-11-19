//! The solver's trace, a debugging aid. With `Settings::trace` on, each
//! substep leaves one JSON line in `Solver::trace`. The line holds the
//! anchor's real and felt movement and turn in the substep, and every
//! capsule. For each point it holds its place, its target, its speed, how
//! far the pull toward the target, the step limit, the maximum distance and
//! the bones' capsule test moved it, the capsules it touched, whether the
//! backstop or the floor moved it, and the stretch of its bone. The game
//! writes the lines to the file that `KJK_LOOSE_TRACE` names.

use glam::{Quat, Vec3};

use crate::Frame;
use crate::body::Capsule;

/// A bone pushed out of one capsule.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Push {
    pub(crate) capsule: usize,
    /// Where the bone's closest point to the capsule's axis lies, as a share
    /// of the way from the parent to the point.
    pub(crate) share: f32,
    pub(crate) depth: f32,
    /// How far the push moved the point and its parent (metres).
    pub(crate) point: f32,
    pub(crate) parent: f32,
}

/// A point that ended a substep near or inside one capsule.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Touch {
    pub(crate) capsule: usize,
    /// How far the point was from the capsule's surface before the push
    /// out (metres). Below 0 it was inside.
    pub(crate) gap: f32,
    /// Whether the point passed from one side of the capsule's axis to the
    /// other within its radius in this substep.
    pub(crate) crossed: bool,
    pub(crate) push: f32,
}

/// What one substep did to each point.
#[derive(Debug, Clone, Default)]
pub(crate) struct Record {
    pub(crate) speed: Vec<f32>,
    pub(crate) drive: Vec<f32>,
    pub(crate) max_step: Vec<f32>,
    pub(crate) max_distance: Vec<f32>,
    pub(crate) segment: Vec<f32>,
    pub(crate) pushes: Vec<Vec<Push>>,
    pub(crate) backstop: Vec<bool>,
    pub(crate) floor: Vec<f32>,
    pub(crate) touches: Vec<Vec<Touch>>,
    /// The capsules at the start of the substep.
    pub(crate) from: Vec<Capsule>,
    /// The points before the bones' capsule test.
    before: Vec<Vec3>,
}

impl Record {
    /// An empty record for `points` points.
    pub(crate) fn start(&mut self, points: usize) {
        for list in [
            &mut self.speed,
            &mut self.drive,
            &mut self.max_step,
            &mut self.max_distance,
            &mut self.segment,
            &mut self.floor,
        ] {
            list.clear();
            list.resize(points, 0.0);
        }
        self.backstop.clear();
        self.backstop.resize(points, false);
        self.pushes.iter_mut().for_each(Vec::clear);
        self.pushes.resize(points, Vec::new());
        self.touches.iter_mut().for_each(Vec::clear);
        self.touches.resize(points, Vec::new());
        self.from.clear();
    }

    /// The points before the bones' capsule test.
    pub(crate) fn before_bones(&mut self, positions: &[Vec3]) {
        self.before.clear();
        self.before.extend_from_slice(positions);
    }

    /// How far the bones' capsule test moved each point.
    pub(crate) fn after_bones(&mut self, positions: &[Vec3]) {
        for (moved, (was, now)) in self
            .segment
            .iter_mut()
            .zip(self.before.iter().zip(positions))
        {
            *moved = was.distance(*now);
        }
    }
}

/// The points at the end of a substep, as the trace line shows them.
pub(crate) struct Points<'a> {
    pub(crate) positions: &'a [Vec3],
    pub(crate) previous: &'a [Vec3],
    pub(crate) targets: &'a [Vec3],
    /// Each point's parent and its rest length from it.
    pub(crate) bones: &'a [Option<(usize, f32)>],
}

/// The substep's line, from share `from` to share `to` of the frame.
/// `delta` is the anchor's movement in the substep and `felt` what the
/// points felt of it.
pub(crate) fn line(
    record: &Record,
    (from, to): (f32, f32),
    (delta, felt): (Frame, Frame),
    points: &Points,
    capsules: &[Capsule],
) -> String {
    let degrees = |q: Quat| {
        let (_, angle) = q.to_axis_angle();
        let angle = if angle > std::f32::consts::PI {
            std::f32::consts::TAU - angle
        } else {
            angle
        };
        angle.to_degrees()
    };
    let v = |p: Vec3| format!("[{:.4},{:.4},{:.4}]", p.x, p.y, p.z);
    let mut each = Vec::with_capacity(points.positions.len());
    for (index, &at) in points.positions.iter().enumerate() {
        let target = points.targets[index];
        let stretch = match points.bones[index] {
            Some((parent, rest)) if rest > 1e-6 => {
                at.distance(points.positions[parent]) / rest - 1.0
            }
            _ => 0.0,
        };
        let touches: Vec<String> = record.touches[index]
            .iter()
            .map(|t| {
                format!(
                    "[{},{:.4},{},{:.4}]",
                    t.capsule,
                    t.gap,
                    u8::from(t.crossed),
                    t.push
                )
            })
            .collect();
        let pushes: Vec<String> = record.pushes[index]
            .iter()
            .map(|p| {
                format!(
                    "[{},{:.4},{:.4},{:.4},{:.4}]",
                    p.capsule, p.share, p.depth, p.point, p.parent
                )
            })
            .collect();
        each.push(format!(
            "{{\"i\":{index},\"pos\":{},\"tgt\":{},\"prev\":{},\"off\":{:.4},\"v\":{:.3},\"drive\":{:.5},\"maxd\":{:.4},\"maxs\":{:.4},\"seg\":{:.4},\"bs\":{},\"fl\":{:.4},\"len\":{:.4},\"c\":[{}],\"segs\":[{}]}}",
            v(at),
            v(target),
            v(points.previous[index]),
            at.distance(target),
            record.speed[index],
            record.drive[index],
            record.max_distance[index],
            record.max_step[index],
            record.segment[index],
            u8::from(record.backstop[index]),
            record.floor[index],
            stretch,
            touches.join(","),
            pushes.join(","),
        ));
    }
    let capsules: Vec<String> = capsules
        .iter()
        .map(|c| format!("[{},{},{:.4}]", v(c.ends[0]), v(c.ends[1]), c.radius))
        .collect();
    format!(
        "{{\"from\":{from:.4},\"to\":{to:.4},\"move\":{:.4},\"turn\":{:.3},\"felt_move\":{:.4},\"felt_turn\":{:.3},\"caps\":[{}],\"p\":[{}]}}",
        delta.translation.length(),
        degrees(delta.rotation),
        felt.translation.length(),
        degrees(felt.rotation),
        capsules.join(","),
        each.join(",")
    )
}
