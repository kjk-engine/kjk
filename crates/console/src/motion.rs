//! Camera moves on the game's time, for filming: a path of keyframes
//! (`campath FILE`), or a camera held relative to a creature (`camera
//! follow TAG …`). Both read the moment the view shows, the time `anim`
//! and `record` hold, so a move plays the same in slow motion as live.

use crate::script::CameraPose;

/// A path's keyframes: seconds from the path's start, each a pose.
#[derive(Debug, Clone, PartialEq)]
pub struct Path {
    keys: Vec<(f32, CameraPose)>,
}

/// A camera held at an offset from a creature (by tag, or `player`),
/// looking at a point `height` metres over another's soles (or its own).
#[derive(Debug, Clone, PartialEq)]
pub struct Follow {
    pub tag: String,
    /// Right, forward and up of the creature's facing (metres), or the
    /// world's x, y and z with `world`.
    pub offset: [f32; 3],
    pub world: bool,
    pub look: Option<String>,
    pub height: f32,
    /// Degrees a second the offset turns about the creature.
    pub orbit: f32,
    pub fov: Option<f32>,
}

/// A chest's height over the soles: where `camera follow` looks by default.
pub const CHEST_M: f32 = 1.3;

/// The move under way, and the moment (ms, the view's) it started at:
/// set by `campath … at S`, else the first frame the view sees it.
#[derive(Default, Debug)]
#[cfg_attr(feature = "runtime", derive(bevy::prelude::Resource))]
pub struct CameraMotion {
    pub moving: Option<Move>,
    pub start_ms: Option<f64>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Move {
    Path(Path),
    Follow(Follow),
}

impl Path {
    /// Lines of `t x y z yaw pitch [fov]` (`#` starts a comment), times
    /// rising; a key without a fov keeps the one before it.
    pub fn parse(text: &str) -> Result<Path, String> {
        let mut keys: Vec<(f32, CameraPose)> = Vec::new();
        for (n, line) in text.lines().enumerate() {
            let line = line.split('#').next().unwrap_or("").trim();
            if line.is_empty() {
                continue;
            }
            let bad = |why: &str| format!("line {}: {why}", n + 1);
            let numbers: Vec<f32> = line
                .split_whitespace()
                .map(|word| word.parse().ok().filter(|n: &f32| n.is_finite()))
                .collect::<Option<_>>()
                .ok_or_else(|| bad("numbers only: t x y z yaw pitch [fov]"))?;
            if !(6..=7).contains(&numbers.len()) {
                return Err(bad("t x y z yaw pitch [fov]"));
            }
            if keys.last().is_some_and(|(t, _)| numbers[0] <= *t) {
                return Err(bad("times rise from key to key"));
            }
            let fov = numbers
                .get(6)
                .copied()
                .or_else(|| keys.last().and_then(|(_, pose)| pose.fov));
            keys.push((
                numbers[0],
                CameraPose {
                    position: [numbers[1], numbers[2], numbers[3]],
                    yaw: numbers[4],
                    pitch: numbers[5],
                    fov,
                },
            ));
        }
        if keys.is_empty() {
            return Err("a path has at least one key".into());
        }
        // Each yaw within half a turn of the one before: the short way round.
        for i in 1..keys.len() {
            let before = keys[i - 1].1.yaw;
            let yaw = &mut keys[i].1.yaw;
            *yaw = before + (*yaw - before + 180.0).rem_euclid(360.0) - 180.0;
        }
        Ok(Path { keys })
    }

    /// The pose `seconds` into the path: a cubic through the keys (each
    /// key's slope the one between its neighbours), still at the first and
    /// last key, so it eases in and out; held before and after.
    pub fn pose(&self, seconds: f32) -> CameraPose {
        let keys = &self.keys;
        let last = keys.len() - 1;
        if last == 0 {
            return keys[0].1;
        }
        let i = keys[..last]
            .iter()
            .rposition(|(t, _)| *t <= seconds)
            .unwrap_or(0);
        let channels = |pose: &CameraPose| {
            let [x, y, z] = pose.position;
            [x, y, z, pose.yaw, pose.pitch, pose.fov.unwrap_or(0.0)]
        };
        let slope = |k: usize| -> [f32; 6] {
            if k == 0 || k == last {
                return [0.0; 6];
            }
            let (a, b) = (channels(&keys[k - 1].1), channels(&keys[k + 1].1));
            let span = keys[k + 1].0 - keys[k - 1].0;
            std::array::from_fn(|c| (b[c] - a[c]) / span)
        };
        let ((t0, p0), (t1, p1)) = (keys[i], keys[i + 1]);
        let dt = t1 - t0;
        let u = ((seconds - t0) / dt).clamp(0.0, 1.0);
        let (u2, u3) = (u * u, u * u * u);
        let h = [
            2.0 * u3 - 3.0 * u2 + 1.0,
            u3 - 2.0 * u2 + u,
            -2.0 * u3 + 3.0 * u2,
            u3 - u2,
        ];
        let (a, b, ma, mb) = (channels(&p0), channels(&p1), slope(i), slope(i + 1));
        let v: [f32; 6] = std::array::from_fn(|c| {
            h[0] * a[c] + h[1] * dt * ma[c] + h[2] * b[c] + h[3] * dt * mb[c]
        });
        CameraPose {
            position: [v[0], v[1], v[2]],
            yaw: v[3],
            pitch: v[4],
            fov: p0.fov.map(|_| v[5]),
        }
    }
}

impl Follow {
    /// The camera for a creature whose soles are at `soles` (metres),
    /// facing `facing` degrees, looking at `target`, `seconds` after the
    /// follow began.
    pub fn pose(&self, soles: [f32; 3], facing: f32, target: [f32; 3], seconds: f32) -> CameraPose {
        let [a, b, up] = self.offset;
        let (x, y) = if self.world {
            (a, b)
        } else {
            let (sin, cos) = facing.to_radians().sin_cos();
            // Right of the facing is the facing turned a quarter clockwise.
            (a * sin + b * cos, -a * cos + b * sin)
        };
        let (sin, cos) = (self.orbit * seconds).to_radians().sin_cos();
        let position = [
            soles[0] + x * cos - y * sin,
            soles[1] + x * sin + y * cos,
            soles[2] + up,
        ];
        let d: [f32; 3] = std::array::from_fn(|i| target[i] - position[i]);
        CameraPose {
            position,
            yaw: d[1].atan2(d[0]).to_degrees(),
            pitch: d[2].atan2(d[0].hypot(d[1])).to_degrees(),
            fov: self.fov,
        }
    }
}
