//! `kjk sim skeleton`: the sim's skeleton (`sim::skeleton`) over saber
//! moves sampled every millisecond, against
//! - the renderer's Ghoul2 player (`asset_ja::g2`), which the sim turns
//!   frames and blends as (along arcs at full size, from the pose shown;
//!   QUIRKS.md Q-0080, Q-0081): it must meet it up to rounding, but where
//!   a change comes mid-blend twice running (the sim keeps one blend under
//!   another, the renderer the whole pose shown), and
//! - Ghoul2's own method worked in `f64` straight from the GLA (lerped
//!   matrices, what JA's server cuts with): how far the sim departs from it,
//!   and the renderer from it on frames with unit quaternions;
//!
//! by kind of moment (on a frame, between frames, blending), with where
//! they differ most; then the hash of every sim result (the value another
//! platform must reach) and what a bolt costs.

use std::process::ExitCode;
use std::time::Instant;

use asset_ja::g2::{Bones, Cue, Part, PartSet, Player};
use asset_ja::{SPECIAL_TRAILS, UNIT};
use bevy::math::{DVec3, Mat4, Quat, Vec3};
use fmt_ja::animation_cfg::{AnimationConfig, Sequence};
use fmt_ja::ghoul2::{Gla, unpack_bone};
use sim::math::sin_cos_deg;
use sim::skeleton::{
    self, BladeId, Bolt, BoltMatrix, Hand, LOOK_BONES, Look, Placement, Playback, Pose,
    SkeletonData, TORSO_BONE,
};

use crate::runtime::write_facts;
use crate::viewer_ja::{self, Files};

/// JA's push per unit of speed at its default 40 server frames a second.
const PUSH_PER_SPEED: f32 = 1.6 / 40.0;

/// The saber's first blade, in the right hand.
const FIRST_BLADE: BladeId = BladeId {
    hand: Hand::Right,
    saber: 0,
    blade: 0,
};

pub fn skeleton(discovery: &vfs::Discovery, saber: &str, step_ms: u32) -> ExitCode {
    match run(discovery, saber, step_ms.max(1)) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("sim skeleton: {error}");
            ExitCode::FAILURE
        }
    }
}

/// One timeline: its cues (ms, all whole), how long it runs, whether the
/// look and the model's yaw move.
struct Timeline<'c> {
    name: String,
    cues: Vec<Cue<'c>>,
    length_ms: i64,
    look: bool,
}

/// What every sample is measured with.
struct Probe<'g> {
    data: &'g SkeletonData,
    renderer: Renderer<'g>,
    reference: Reference<'g>,
}

fn run(discovery: &vfs::Discovery, saber: &str, step_ms: u32) -> Result<(), String> {
    let stack = viewer_ja::stack(discovery)?;
    let files = Files::new(&stack);
    let built = Instant::now();
    let data = crate::skeleton::skeleton_data(&files, &[saber])?;
    let built_ms = built.elapsed().as_secs_f64() * 1e3;
    let (glm, gla, gla_path) = viewer_ja::glm_and_gla(&files, crate::stage::JA_BODY)?;
    let cfg = viewer_ja::animation_cfg(&files, &gla_path)?;
    let (_, hilt) = viewer_ja::saber_and_hilt(&files, saber)?;
    let hand = asset_ja::bolt(&glm, "*r_hand").ok_or("*r_hand: no such tag")?;
    let blade = asset_ja::bolt(&hilt, "*blade1").ok_or("*blade1: no such tag")?;
    let first_blade = data
        .sabers()
        .first()
        .and_then(|s| s.blades.first())
        .ok_or("the saber has no blade")?;
    let length = first_blade.length;
    let probe = Probe {
        data: &data,
        renderer: Renderer {
            gla: &gla,
            hand: rows(&hand.matrix),
            hand_bone: hand.bone,
            blade: rows(&blade.matrix),
            length,
        },
        reference: Reference::new(&gla, &data, first_blade)?,
    };

    let digest = hex(&data.digest());
    let chain = |h: Hand| {
        data.chain(h)
            .iter()
            .map(|&b| data.bones()[usize::from(b)].name.clone())
            .collect::<Vec<_>>()
    };
    outln!(
        "skeleton data: {} bones, {} frames, {} pool entries, built in {built_ms:.0} ms; digest {digest}",
        data.bones().len(),
        data.frame_count(),
        data.pool_len()
    );
    outln!("  right hand: {}", chain(Hand::Right).join(" > "));
    outln!("  left hand:  {}", chain(Hand::Left).join(" > "));
    outln!(
        "  renderer's *r_hand on {} (rigid {}); saber {saber}, blade 1 {length} units",
        gla.bones[hand.bone].name,
        hand.rigid
    );

    let (timelines, skipped) = timelines(&gla, &cfg);
    if !skipped.is_empty() {
        outln!("  not in animation.cfg: {}", skipped.join(", "));
    }
    let mut stats = Stats::default();
    let mut hasher = blake3::Hasher::new();
    hasher.update(&data.digest());
    let mut poses = Vec::new();
    for timeline in &timelines {
        probe.sample(timeline, step_ms, &mut stats, &mut hasher, &mut poses)?;
    }
    let hash = hex(hasher.finalize().as_bytes());

    outln!(
        "{} timelines, {} samples every {step_ms} ms (units, degrees):",
        timelines.len(),
        stats.samples
    );
    outln!(
        "  the sim against Ghoul2's method in f64 (max) | against the renderer (mean, max) \
         | the renderer against the f64 method with unit quaternions (max)"
    );
    outln!(
        "  {:<22} {:>7} | {:>9} {:>9} | {:>9} {:>9} {:>9} {:>9} {:>8} | {:>9}",
        "moment",
        "samples",
        "hand",
        "tip",
        "hand mean",
        "hand max",
        "tip mean",
        "tip max",
        "dir max",
        "unit"
    );
    for class in Class::ALL {
        let c = &stats.classes[class as usize];
        if c.samples == 0 {
            continue;
        }
        outln!(
            "  {:<22} {:>7} | {:>9.6} {:>9.6} | {:>9.4} {:>9.4} {:>9.4} {:>9.4} {:>8.3} | {:>9.5}",
            class.name(),
            c.samples,
            c.exact_hand.max,
            c.exact_tip.max,
            c.hand.mean(),
            c.hand.max,
            c.tip.mean(),
            c.tip.max,
            c.direction_max,
            c.unit.max
        );
    }
    for class in Class::ALL {
        let c = &stats.classes[class as usize];
        if let Some(worst) = &c.tip.worst {
            outln!(
                "  renderer's farthest tip, {}: {:.3} at {worst}",
                class.name(),
                c.tip.max
            );
        }
    }
    let single = [Class::Frame, Class::Between, Class::Blend]
        .iter()
        .map(|&class| {
            let c = &stats.classes[class as usize];
            c.tip.max.max(c.hand.max)
        })
        .fold(0.0, f32::max);
    let ghoul2 = stats
        .classes
        .iter()
        .map(|c| c.exact_hand.max.max(c.exact_tip.max))
        .fold(0.0, f32::max);
    outln!(
        "  the sim is the renderer within {single:.4} units but mid-blend changes; Ghoul2's \
         lerped matrices are up to {ghoul2:.1} units from it"
    );

    let (hand_us, blade_us) = cost(&data, &poses);
    outln!(
        "cost ({} build): {hand_us:.2} us a hand bolt, {blade_us:.2} us a blade in the world",
        if cfg!(debug_assertions) {
            "debug"
        } else {
            "release"
        }
    );
    outln!(
        "determinism: {} poses, every hand and blade bolt hashed: {hash}",
        stats.samples
    );

    let class_facts: Vec<_> = Class::ALL
        .iter()
        .map(|&class| {
            let c = &stats.classes[class as usize];
            serde_json::json!({
                "moment": class.name(),
                "samples": c.samples,
                "sim_vs_f64": { "hand_max": c.exact_hand.max, "tip_max": c.exact_tip.max,
                                "tip_worst": c.exact_tip.worst },
                "renderer": {
                    "hand_mean": c.hand.mean(),
                    "hand_max": c.hand.max,
                    "hand_worst": c.hand.worst,
                    "tip_mean": c.tip.mean(),
                    "tip_max": c.tip.max,
                    "tip_worst": c.tip.worst,
                    "direction_max_degrees": c.direction_max,
                },
                "renderer_vs_unit_f64_max": c.unit.max,
            })
        })
        .collect();
    write_facts(serde_json::json!({
        "verb": "sim skeleton",
        "saber": saber,
        "blade_length": length,
        "step_ms": step_ms,
        "data": {
            "bones": data.bones().len(),
            "frames": data.frame_count(),
            "pool": data.pool_len(),
            "digest": digest,
            "right_chain": chain(Hand::Right),
            "left_chain": chain(Hand::Left),
        },
        "timelines": timelines.iter().map(|t| serde_json::json!({
            "name": t.name,
            "length_ms": t.length_ms,
            "look": t.look,
            "moves": t.cues.iter().map(|c| c.sequence.name.clone()).collect::<Vec<_>>(),
        })).collect::<Vec<_>>(),
        "skipped": skipped,
        "samples": stats.samples,
        "agreement": class_facts,
        "sim_vs_renderer_max": single,
        "sim_vs_f64_max": ghoul2,
        "cost_us": { "hand_bolt": hand_us, "blade_in_world": blade_us },
        "determinism_hash": hash,
    }));
    Ok(())
}

/// A hand bolt's origin and a blade's tip and direction, units, in the
/// world at the origin (the model turned to its yaw).
struct Bolts<V> {
    hand: V,
    tip: V,
    direction: V,
}

/// The renderer's side: its player's skeleton, the `*r_hand` tag's base
/// matrix on its bone, the hilt's `*blade1` tag (metres), and the length.
struct Renderer<'g> {
    gla: &'g Gla,
    hand: Mat4,
    hand_bone: usize,
    blade: Mat4,
    length: f32,
}

impl Renderer<'_> {
    /// From a pose already turned to the yaw, in Ghoul2's model space
    /// (facing +X).
    fn bolts(&self, pose: &[(Quat, Vec3)]) -> Bolts<Vec3> {
        let line: Vec<usize> = std::iter::successors(Some(self.hand_bone), |&bone| {
            usize::try_from(self.gla.bones[bone].parent).ok()
        })
        .collect();
        let (q, t) = line
            .iter()
            .rev()
            .fold((Quat::IDENTITY, Vec3::ZERO), |(pq, pt), &bone| {
                let (q, t) = pose[bone];
                (pq * q, pt + pq * t)
            });
        let hand = Mat4::from_rotation_translation(q, t) * self.hand;
        let hilt = hand * self.blade;
        let direction = -hilt.x_axis.truncate().normalize();
        let tip = hilt.w_axis.truncate() + direction * self.length * UNIT;
        let ghoul2 = |v: Vec3| Vec3::new(-v.y, v.x, v.z);
        Bolts {
            hand: ghoul2(hand.w_axis.truncate() / UNIT),
            tip: ghoul2(tip / UNIT),
            direction: ghoul2(direction),
        }
    }
}

fn rows(m: &asset_odyssey::model::Matrix) -> Mat4 {
    Mat4::from_cols_array_2d(m).transpose()
}

type M64 = [[f64; 4]; 3];

/// Ghoul2's method in `f64`, read from the GLA rather than through the
/// sim's data; the frames and blend weights are the sim's playbacks'
/// (their timing is `docs/G2-SPEC.md`'s, which the renderer shares).
struct Reference<'g> {
    gla: &'g Gla,
    data: &'g SkeletonData,
    /// Each data bone's GLA bone, and whether the torso drives it.
    bones: Vec<(usize, bool)>,
    blade: [DVec3; 3],
    length: f64,
}

impl<'g> Reference<'g> {
    fn new(
        gla: &'g Gla,
        data: &'g SkeletonData,
        blade: &skeleton::SaberBlade,
    ) -> Result<Reference<'g>, String> {
        let mut bones: Vec<(usize, bool)> = Vec::new();
        for bone in data.bones() {
            let index = gla
                .bones
                .iter()
                .position(|b| b.name == bone.name)
                .ok_or(format!("{}: not in the GLA", bone.name))?;
            let torso = bone.name.eq_ignore_ascii_case(TORSO_BONE)
                || bone.parent.is_some_and(|p| bones[usize::from(p)].1);
            bones.push((index, torso));
        }
        Ok(Reference {
            gla,
            data,
            bones,
            blade: blade.tag.map(|p| DVec3::from_array(p.map(f64::from))),
            length: f64::from(blade.length),
        })
    }

    fn key(&self, frame: i32, bone: usize, unit: bool) -> M64 {
        let frame = usize::try_from(frame)
            .ok()
            .filter(|&f| f < self.gla.frame_count)
            .unwrap_or(0);
        let entry = &self.gla.pool
            [self.gla.frames[frame * self.gla.bones.len() + self.bones[bone].0] as usize];
        let (q, t) = unpack_bone(entry);
        let [mut x, mut y, mut z, mut w] = q.map(f64::from);
        if unit {
            let n = (x * x + y * y + z * z + w * w).sqrt();
            [x, y, z, w] = [x / n, y / n, z / n, w / n];
        }
        let t = t.map(f64::from);
        [
            [
                1.0 - 2.0 * (y * y + z * z),
                2.0 * (x * y - w * z),
                2.0 * (x * z + w * y),
                t[0],
            ],
            [
                2.0 * (x * y + w * z),
                1.0 - 2.0 * (x * x + z * z),
                2.0 * (y * z - w * x),
                t[1],
            ],
            [
                2.0 * (x * z - w * y),
                2.0 * (y * z + w * x),
                1.0 - 2.0 * (x * x + y * y),
                t[2],
            ],
        ]
    }

    fn local(&self, playback: &Playback, now: i64, bone: usize, unit: bool) -> M64 {
        let frames = playback.frames(now);
        let t = f64::from(frames.lerp);
        let key = |frame| self.key(frame, bone, unit);
        let mut m = lerp64(&key(frames.next), &key(frames.current), t);
        if let Some((blend, amount)) = playback.blending(now) {
            let whole = blend.frame.trunc();
            let back = f64::from(blend.frame) - f64::from(whole);
            let captured = lerp64(&key(blend.next), &key(whole as i32), back);
            m = lerp64(&m, &captured, f64::from(amount));
        }
        m
    }

    fn bolts(&self, pose: &Pose, now: i64, yaw: f32, unit: bool) -> Bolts<DVec3> {
        let root: M64 = [
            [0.0, -1.0, 0.0, 0.0],
            [1.0, 0.0, 0.0, 0.0],
            [0.0, 0.0, 1.0, 0.0],
        ];
        let mut finals: Vec<M64> = Vec::with_capacity(self.bones.len());
        for (bone, info) in self.data.bones().iter().enumerate() {
            let playback = if self.bones[bone].1 {
                &pose.torso
            } else {
                &pose.legs
            };
            let local = self.local(playback, now, bone, unit);
            let parent = info.parent.map_or(&root, |p| &finals[usize::from(p)]);
            let mut m = mul64(parent, &local);
            let slot = LOOK_BONES
                .iter()
                .position(|name| info.name.eq_ignore_ascii_case(name));
            if let Some((look, slot)) = pose.look.zip(slot) {
                let [pitch, yaw, roll] = look[slot].map(f64::from);
                let base = info.base.map(|row| row.map(f64::from));
                let inverse = info.base_inverse.map(|row| row.map(f64::from));
                let turn = mul64(&base, &mul64(&angles64([pitch, roll, yaw]), &inverse));
                m = mul64(&m, &turn);
            }
            finals.push(m);
        }
        let tag = &self.data.hands()[Hand::Right as usize];
        let points = tag.vertices.each_ref().map(|vertex| {
            let mut total = 0.0;
            let count = vertex.influences.len();
            vertex
                .influences
                .iter()
                .enumerate()
                .map(|(k, &(bone, raw))| {
                    let weight = if k + 1 == count {
                        1.0 - total
                    } else {
                        total += f64::from(raw) / 1023.0;
                        f64::from(raw) / 1023.0
                    };
                    let p = DVec3::from_array(vertex.position.map(f64::from));
                    apply64(&finals[usize::from(bone)], p) * weight
                })
                .sum::<DVec3>()
        });
        let hand = tag64(&points);
        let blade = tag64(&self.blade.map(|p| apply64(&hand, p)));
        // The bolt query makes the rotation's rows unit; the blade runs
        // along the old −X.
        let rows = blade.map(|r| DVec3::new(r[0], r[1], r[2]).normalize());
        let direction = -DVec3::new(rows[0].x, rows[1].x, rows[2].x);
        let turn = |v: DVec3| {
            let (s, c) = f64::from(yaw).to_radians().sin_cos();
            DVec3::new(c * v.x - s * v.y, s * v.x + c * v.y, v.z)
        };
        let origin = |m: &M64| DVec3::new(m[0][3], m[1][3], m[2][3]);
        let direction = turn(direction);
        Bolts {
            hand: turn(origin(&hand)),
            tip: turn(origin(&blade)) + direction * self.length,
            direction,
        }
    }
}

fn mul64(n: &M64, m: &M64) -> M64 {
    std::array::from_fn(|r| {
        std::array::from_fn(|c| {
            let sum = n[r][0] * m[0][c] + n[r][1] * m[1][c] + n[r][2] * m[2][c];
            if c == 3 { sum + n[r][3] } else { sum }
        })
    })
}

fn lerp64(a: &M64, b: &M64, t: f64) -> M64 {
    std::array::from_fn(|r| std::array::from_fn(|c| a[r][c] * t + b[r][c] * (1.0 - t)))
}

fn apply64(m: &M64, p: DVec3) -> DVec3 {
    let row = |r: usize| m[r][0] * p.x + m[r][1] * p.y + m[r][2] * p.z + m[r][3];
    DVec3::new(row(0), row(1), row(2))
}

/// Quake angles as columns forward, left, up.
fn angles64([pitch, yaw, roll]: [f64; 3]) -> M64 {
    let (sp, cp) = pitch.to_radians().sin_cos();
    let (sy, cy) = yaw.to_radians().sin_cos();
    let (sr, cr) = roll.to_radians().sin_cos();
    let forward = [cp * cy, cp * sy, -sp];
    let left = [sr * sp * cy - cr * sy, sr * sp * sy + cr * cy, sr * cp];
    let up = [cr * sp * cy + sr * sy, cr * sp * sy - sr * cy, cr * cp];
    std::array::from_fn(|r| [forward[r], left[r], up[r], 0.0])
}

/// A tag's bolt from its placed vertices (`sim::skeleton`'s rule).
fn tag64(p: &[DVec3; 3]) -> M64 {
    let sides: [DVec3; 3] = std::array::from_fn(|j| p[(j + 1) % 3] - p[j]);
    let short = sides[2].normalize();
    let long = sides[0].normalize();
    let long = (long - short * long.dot(short)).normalize();
    let normal = sides[0].cross(sides[2]).normalize();
    std::array::from_fn(|r| [short[r], long[r], -normal[r], p[2][r]])
}

/// The look and the model's yaw at a moment: every look bone swaying on its
/// own periods, the model turning 45° a second.
fn look_at(now_ms: i64) -> (Look, f32) {
    let t = now_ms as f32;
    let sway = |period: f32, phase: f32, amplitude: f32| {
        sin_cos_deg((t / period) * 360.0 + phase).0 * amplitude
    };
    let look = std::array::from_fn(|bone| {
        let k = bone as f32;
        [
            sway(1300.0 + 90.0 * k, 17.0 * k, 12.0),
            sway(2100.0 - 70.0 * k, 40.0 + 23.0 * k, 30.0),
            sway(1700.0 + 50.0 * k, 80.0 + 11.0 * k, 6.0),
        ]
    });
    (look, (t * 0.045) % 360.0)
}

/// Kinds of moment: both parts on a frame and no blend, between frames,
/// blending, and blending from a change made mid-blend (where the sim
/// starts from the old animation's frames, not the pose shown).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Class {
    Frame,
    Between,
    Blend,
    BlendFromBlend,
}

impl Class {
    const ALL: [Class; 4] = [
        Class::Frame,
        Class::Between,
        Class::Blend,
        Class::BlendFromBlend,
    ];

    fn name(self) -> &'static str {
        match self {
            Class::Frame => "on a frame",
            Class::Between => "between frames",
            Class::Blend => "blending",
            Class::BlendFromBlend => "blending from a blend",
        }
    }
}

#[derive(Default)]
struct Measure {
    sum: f64,
    count: u64,
    max: f32,
    worst: Option<String>,
}

impl Measure {
    fn add(&mut self, value: f32, at: impl FnOnce() -> String) {
        self.sum += f64::from(value);
        self.count += 1;
        if value > self.max || self.worst.is_none() {
            self.max = value;
            self.worst = Some(at());
        }
    }

    fn mean(&self) -> f64 {
        if self.count == 0 {
            0.0
        } else {
            self.sum / self.count as f64
        }
    }
}

#[derive(Default)]
struct ClassStats {
    samples: u64,
    /// The sim against Ghoul2's method in `f64`.
    exact_hand: Measure,
    exact_tip: Measure,
    /// The renderer against the sim.
    hand: Measure,
    tip: Measure,
    direction_max: f32,
    /// The renderer against Ghoul2's method in `f64` with unit quaternions.
    unit: Measure,
}

#[derive(Default)]
struct Stats {
    samples: u64,
    classes: [ClassStats; 4],
}

/// The sim's side of the cues: each part's playback as the renderer's
/// `Player::set_part` sets it (without the Motion bone), and whether a
/// part's blend began mid-blend.
#[derive(Default)]
struct Mirror {
    legs: Option<Playback>,
    torso: Option<Playback>,
    from_blend: [bool; 2],
}

impl Mirror {
    fn set(&mut self, set: &PartSet, gla_frames: usize) {
        let Some((_, frame_ms)) = asset_ja::playback(set.sequence, gla_frames) else {
            return;
        };
        let sequence = set.sequence;
        let backward = sequence.frames_per_second < 0;
        let frame_lerp = if backward { -frame_ms } else { frame_ms } as i32;
        let wanted = skeleton::Sequence {
            first_frame: sequence.first_frame,
            frames: sequence.frame_count,
            frame_lerp,
            looping: sequence.loop_frame != -1,
        };
        let now = set.at_ms as i64;
        let name = sequence.name.as_str();
        let same = set.after.0 == set.after.1;
        let pick_up = |other: Option<Playback>| {
            let frame = other?.frame_at(now);
            let first = sequence.first_frame as f32;
            (!backward && (first..=first + sequence.frame_count as f32).contains(&frame))
                .then_some(frame)
        };
        let start = |old: Option<Playback>, blend: f32, begin| {
            let blend = blend as u32;
            let mid_blend = blend > 0
                && old.and_then(|p| p.blend).is_some_and(|b| {
                    let elapsed = now - b.start_ms;
                    elapsed > 0 && elapsed < i64::from(b.length_ms)
                });
            let new = Playback::set(old.as_ref(), &wanted, set.speed_scale, now, blend, begin);
            (Some(new), mid_blend)
        };
        if set.part != Part::Torso {
            let begin = (same && set.before.0 != name)
                .then(|| pick_up(self.torso))
                .flatten();
            (self.legs, self.from_blend[0]) = start(self.legs, set.blends.0, begin);
        }
        if set.part != Part::Legs {
            let begin = (same && set.before.1 != name)
                .then(|| pick_up(self.legs))
                .flatten();
            (self.torso, self.from_blend[1]) = start(self.torso, set.blends.1, begin);
        }
    }

    fn pose(&self, look: Option<Look>) -> Option<Pose> {
        let legs = self.legs?;
        Some(Pose {
            legs,
            torso: self.torso.unwrap_or(legs),
            look,
        })
    }

    fn class(&self, pose: &Pose, now: i64) -> Class {
        let parts = [pose.legs, pose.torso];
        let blending = |i: usize| parts[i].blending(now).is_some();
        if (0..2).any(|i| blending(i) && self.from_blend[i]) {
            Class::BlendFromBlend
        } else if (0..2).any(blending) {
            Class::Blend
        } else if parts.iter().any(|p| p.frames(now).lerp != 0.0) {
            Class::Between
        } else {
            Class::Frame
        }
    }
}

impl Probe<'_> {
    fn sample(
        &self,
        timeline: &Timeline,
        step_ms: u32,
        stats: &mut Stats,
        hasher: &mut blake3::Hasher,
        poses: &mut Vec<(Pose, i64)>,
    ) -> Result<(), String> {
        let data = self.data;
        let mut player = Player::new(self.renderer.gla);
        let parts = Bones::of(&player);
        let look_bones = LOOK_BONES.map(|name| player.bone(name));
        let mut mirror = Mirror::default();
        let (mut legs, mut body) = ("", "");
        let mut next = 0;
        for now in (0..=timeline.length_ms).step_by(step_ms as usize) {
            while let Some(cue) = timeline.cues.get(next).filter(|c| c.at_ms as i64 <= now) {
                let name = cue.sequence.name.as_str();
                let after = match cue.part {
                    Part::Both => (name, name),
                    Part::Legs => (name, body),
                    Part::Torso => (legs, name),
                };
                let set = PartSet {
                    part: cue.part,
                    sequence: cue.sequence,
                    at_ms: cue.at_ms,
                    blends: (cue.blend_ms, cue.blend_ms),
                    speed_scale: cue.speed_scale,
                    before: (legs, body),
                    after,
                };
                player.set_part(&set, parts);
                mirror.set(&set, self.renderer.gla.frame_count);
                (legs, body) = after;
                next += 1;
            }
            let (look, yaw) = if timeline.look {
                let (look, yaw) = look_at(now);
                (Some(look), yaw)
            } else {
                (None, 0.0)
            };
            let Some(pose) = mirror.pose(look) else {
                continue;
            };

            let mut shown = player.pose(now as f32);
            if let Some(look) = look {
                for (bone, angles) in look_bones.iter().zip(look) {
                    if let Some(bone) = *bone {
                        player.turn(&mut shown, bone, angles);
                    }
                }
            }
            if let Some(root) = parts.root {
                let turn = Quat::from_rotation_z(yaw.to_radians());
                shown[root] = (turn * shown[root].0, turn * shown[root].1);
            }
            let rendered = self.renderer.bolts(&shown);
            let exact = self.reference.bolts(&pose, now, yaw, false);
            let unit = self.reference.bolts(&pose, now, yaw, true);

            let at_origin = Placement {
                origin: [0.0; 3],
                yaw,
            };
            let right = bolt(data, &pose, now, Bolt::Hand(Hand::Right))?;
            let hand = Vec3::from_array(skeleton::world(&right, &at_origin).origin);
            let blade = skeleton::blade(data, &pose, now, &at_origin, FIRST_BLADE)
                .ok_or("the saber has no blade")?;
            let tip = Vec3::from_array(blade.tip);

            let class = mirror.class(&pose, now);
            let at = || format!("{} {now} ms ({legs} / {body})", timeline.name);
            let far = |a: Vec3, b: DVec3| (a.as_dvec3().distance(b)) as f32;
            let c = &mut stats.classes[class as usize];
            c.samples += 1;
            c.exact_hand.add(far(hand, exact.hand), at);
            c.exact_tip.add(far(tip, exact.tip), at);
            c.hand.add(hand.distance(rendered.hand), at);
            c.tip.add(tip.distance(rendered.tip), at);
            let cosine = Vec3::from_array(blade.direction)
                .dot(rendered.direction)
                .clamp(-1.0, 1.0);
            c.direction_max = c.direction_max.max(cosine.acos().to_degrees());
            c.unit.add(
                far(rendered.hand, unit.hand).max(far(rendered.tip, unit.tip)),
                at,
            );
            stats.samples += 1;

            // What the determinism hash covers: both hands in the model's
            // space, and the blade in the world with the server's push.
            let t = now as f32;
            let (sin, cos) = sin_cos_deg(t * 0.02);
            let velocity = [300.0 * cos, 250.0 * sin, 40.0 * sin * cos];
            let placement =
                Placement::ahead([t * 0.01, -t * 0.02, 3.0], velocity, PUSH_PER_SPEED, yaw);
            let left = bolt(data, &pose, now, Bolt::Hand(Hand::Left))?;
            let blade = bolt(data, &pose, now, Bolt::Blade(FIRST_BLADE))?;
            for matrix in [right, left, skeleton::world(&blade, &placement)] {
                for bits in matrix.bits() {
                    hasher.update(&bits.to_le_bytes());
                }
            }
            poses.push((pose, now));
        }
        Ok(())
    }
}

fn bolt(data: &SkeletonData, pose: &Pose, now: i64, which: Bolt) -> Result<BoltMatrix, String> {
    skeleton::bolt(data, pose, now, which).ok_or(format!("{which:?}: not in the data"))
}

/// Mean microseconds of a right-hand bolt, and of the first blade placed in
/// the world, over the poses sampled.
fn cost(data: &SkeletonData, poses: &[(Pose, i64)]) -> (f64, f64) {
    let placement = Placement {
        origin: [10.0, 20.0, 30.0],
        yaw: 33.0,
    };
    let time = |f: &dyn Fn(&Pose, i64)| {
        let started = Instant::now();
        for _ in 0..3 {
            for (pose, now) in poses {
                f(std::hint::black_box(pose), std::hint::black_box(*now));
            }
        }
        started.elapsed().as_secs_f64() * 1e6 / (3 * poses.len().max(1)) as f64
    };
    let hand = time(&|pose, now| {
        std::hint::black_box(skeleton::bolt(data, pose, now, Bolt::Hand(Hand::Right)));
    });
    let blade = time(&|pose, now| {
        std::hint::black_box(skeleton::blade(data, pose, now, &placement, FIRST_BLADE));
    });
    (hand, blade)
}

/// The timelines: each style's attacks chained with the cgame's blends and
/// without, two styles' again with the look; specials; backward sequences;
/// fast and strong transitions at their speeds; running legs under a
/// swinging torso; and changes made mid-blend.
fn timelines<'c>(gla: &Gla, cfg: &'c AnimationConfig) -> (Vec<Timeline<'c>>, Vec<String>) {
    let mut skipped = Vec::new();
    let mut find = |name: &str| {
        let found = cfg
            .sequences
            .iter()
            .find(|s| s.name.eq_ignore_ascii_case(name));
        if found.is_none() {
            skipped.push(name.to_string());
        }
        found
    };
    let mut out = Vec::new();
    let mut chained = |name: String, moves: &[&'c Sequence], blend: Option<f32>, look: bool| {
        if moves.is_empty() {
            return;
        }
        let with: Vec<(&Sequence, f32)> = moves
            .iter()
            .enumerate()
            .map(|(i, s)| {
                let before = i.checked_sub(1).map(|j| moves[j].name.as_str());
                let blend = blend.unwrap_or_else(|| asset_ja::blend_ms(before, &s.name, false));
                (*s, blend)
            })
            .collect();
        if let Some((cues, length)) = asset_ja::chain(gla, &with) {
            out.push(Timeline {
                name,
                cues,
                length_ms: length as i64,
                look,
            });
        }
    };
    for style in 1..=7 {
        let prefix = format!("BOTH_A{style}_");
        let attacks: Vec<&Sequence> = cfg
            .sequences
            .iter()
            .filter(|s| {
                let name = s.name.to_ascii_uppercase();
                name.starts_with(&prefix)
                    && asset_ja::saber_trail_ms(&name) == 40.0
                    && !SPECIAL_TRAILS.iter().any(|(special, _)| *special == name)
            })
            .collect();
        chained(format!("A{style} attacks"), &attacks, None, false);
        chained(
            format!("A{style} attacks, no blends"),
            &attacks,
            Some(0.0),
            false,
        );
        if style == 2 || style == 7 {
            chained(format!("A{style} attacks, looking"), &attacks, None, true);
        }
    }
    let specials: Vec<&Sequence> = [
        "BOTH_A1_SPECIAL",
        "BOTH_A2_SPECIAL",
        "BOTH_A3_SPECIAL",
        "BOTH_A6_SABERPROTECT",
        "BOTH_A7_SOULCAL",
        "BOTH_A2_STABBACK1",
        "BOTH_ATTACK_BACK",
        "BOTH_CROUCHATTACKBACK1",
        "BOTH_JUMPFLIPSLASHDOWN1",
        "BOTH_BUTTERFLY_LEFT",
        "BOTH_A7_KICK_F",
    ]
    .iter()
    .filter_map(|name| find(name))
    .collect();
    chained("specials".into(), &specials, None, false);
    let backward: Vec<&Sequence> = cfg
        .sequences
        .iter()
        .filter(|s| s.frames_per_second < 0 && s.frame_count > 1)
        .take(4)
        .collect();
    chained("backward sequences".into(), &backward, None, true);

    // Cues set by hand: (ms, part, name, speed).
    let mut cued = |name: &str, cues: &[(f32, Part, &str, f32)], length_ms: i64| {
        let cues: Vec<Cue> = cues
            .iter()
            .filter_map(|&(at_ms, part, anim, speed_scale)| {
                Some(Cue {
                    at_ms,
                    part,
                    sequence: find(anim)?,
                    blend_ms: 100.0,
                    speed_scale,
                })
            })
            .collect();
        (!cues.is_empty()).then(|| Timeline {
            name: name.into(),
            cues,
            length_ms,
            look: true,
        })
    };
    let by_hand = [
        cued(
            "fast and strong transitions",
            &[
                (0.0, Part::Both, "BOTH_T1_BR_BL", 1.5),
                (300.0, Part::Both, "BOTH_T1__L__R", 1.5),
                (600.0, Part::Both, "BOTH_T1_TL_BR", 1.5),
                (900.0, Part::Both, "BOTH_T3_BR_TL", 0.75),
                (1500.0, Part::Both, "BOTH_T3__R__L", 0.75),
                (2100.0, Part::Both, "BOTH_T3_TR_BL", 0.75),
            ],
            2800,
        ),
        cued(
            "running legs, swinging torso",
            &[
                (0.0, Part::Legs, "BOTH_RUN1", 1.0),
                (0.0, Part::Torso, "BOTH_STAND2", 1.0),
                (250.0, Part::Torso, "BOTH_A2_TL_BR", 1.0),
                (800.0, Part::Torso, "BOTH_A2_BR_TL", 1.0),
                (1400.0, Part::Torso, "BOTH_RUN1", 1.0),
                (1900.0, Part::Legs, "BOTH_STAND1", 1.0),
                (2300.0, Part::Both, "BOTH_A2_T__B_", 1.0),
            ],
            3200,
        ),
        cued(
            "changes mid-blend",
            &[
                (0.0, Part::Both, "BOTH_A1_TL_BR", 1.0),
                (60.0, Part::Both, "BOTH_A1_BR_TL", 1.0),
                (110.0, Part::Both, "BOTH_A1_T__B_", 1.0),
                (170.0, Part::Both, "BOTH_A1__L__R", 1.0),
                (500.0, Part::Both, "BOTH_A1_TR_BL", 1.0),
            ],
            1000,
        ),
    ];
    out.extend(by_hand.into_iter().flatten());
    (out, skipped)
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}
