//! Clip-brush overrides: our own data, one text file per module, that adds
//! invisible walls (`PLAYER_CLIP`) where the containment audit finds a
//! player can leave a level KOTOR never built for jumping, and accepts the
//! places it finds that are fine to reach.
//!
//! One entry per line, `#` to the end of a line is a comment, numbers are
//! metres in KOTOR's world space (+Z up):
//!
//! ```text
//! box    <name> <x0> <y0> <z0> <x1> <y1> <z1>          # opposite corners
//! wall   <name> <x0> <y0> <x1> <y1> <z0> <z1> [<thickness>]
//! accept <name> <x> <y> <z> <radius>
//! ```
//!
//! A `wall` is an upright slab along the segment from (x0, y0) to (x1, y1),
//! from z0 to z1, `thickness` thick (0.1 by default) centred on the
//! segment. An `accept` marks the audit's out-of-bounds places within
//! `radius` of the point as meant (standing on a crate, say): the audit
//! still lists them, as accepted. Names are for reports and must be unique.

use crate::brush::normalize;
use crate::{Contents, LevelBuilder, Tag};

#[derive(Debug, Clone, PartialEq)]
pub struct ClipFile {
    pub brushes: Vec<ClipBrush>,
    pub accepts: Vec<Accept>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ClipBrush {
    pub name: String,
    pub line: usize,
    pub shape: ClipShape,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ClipShape {
    Box {
        min: [f32; 3],
        max: [f32; 3],
    },
    Wall {
        from: [f32; 2],
        to: [f32; 2],
        z: [f32; 2],
        thickness: f32,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub struct Accept {
    pub name: String,
    pub line: usize,
    pub point: [f32; 3],
    pub radius: f32,
}

impl Accept {
    pub fn covers(&self, point: [f32; 3]) -> bool {
        let d: [f32; 3] = std::array::from_fn(|i| point[i] - self.point[i]);
        d[0] * d[0] + d[1] * d[1] + d[2] * d[2] <= self.radius * self.radius
    }
}

/// A line that does not read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClipError {
    pub line: usize,
    pub what: String,
}

impl std::fmt::Display for ClipError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "line {}: {}", self.line, self.what)
    }
}

impl ClipFile {
    pub fn parse(text: &str) -> Result<ClipFile, ClipError> {
        let mut file = ClipFile {
            brushes: Vec::new(),
            accepts: Vec::new(),
        };
        let mut names: Vec<String> = Vec::new();
        for (at, raw) in text.lines().enumerate() {
            let line = at + 1;
            let error = |what: String| ClipError { line, what };
            let words: Vec<&str> = raw
                .split('#')
                .next()
                .unwrap_or("")
                .split_whitespace()
                .collect();
            let Some((&keyword, rest)) = words.split_first() else {
                continue;
            };
            let Some((&name, numbers)) = rest.split_first() else {
                return Err(error(format!("{keyword} needs a name")));
            };
            if names.iter().any(|seen| seen == name) {
                return Err(error(format!("{name} is named twice")));
            }
            names.push(name.to_string());
            let numbers: Vec<f32> = numbers
                .iter()
                .map(|word| {
                    word.parse::<f32>()
                        .ok()
                        .filter(|value| value.is_finite())
                        .ok_or_else(|| error(format!("{word:?} is not a number")))
                })
                .collect::<Result<_, _>>()?;
            let name = name.to_string();
            match (keyword, numbers.as_slice()) {
                ("box", &[x0, y0, z0, x1, y1, z1]) => file.brushes.push(ClipBrush {
                    name,
                    line,
                    shape: ClipShape::Box {
                        min: [x0.min(x1), y0.min(y1), z0.min(z1)],
                        max: [x0.max(x1), y0.max(y1), z0.max(z1)],
                    },
                }),
                ("wall", &[x0, y0, x1, y1, z0, z1, ref thickness @ ..]) if thickness.len() <= 1 => {
                    let thickness = thickness.first().copied().unwrap_or(0.1);
                    if thickness <= 0.0 || (x0 == x1 && y0 == y1) {
                        return Err(error("a wall needs a length and a thickness".into()));
                    }
                    file.brushes.push(ClipBrush {
                        name,
                        line,
                        shape: ClipShape::Wall {
                            from: [x0, y0],
                            to: [x1, y1],
                            z: [z0.min(z1), z0.max(z1)],
                            thickness,
                        },
                    });
                }
                ("accept", &[x, y, z, radius]) if radius > 0.0 => file.accepts.push(Accept {
                    name,
                    line,
                    point: [x, y, z],
                    radius,
                }),
                ("box" | "wall" | "accept", _) => {
                    return Err(error(format!("{keyword} takes other numbers")));
                }
                _ => return Err(error(format!("unknown entry {keyword:?}"))),
            }
        }
        Ok(file)
    }

    /// Adds every brush as `PLAYER_CLIP`. Returns how many were added.
    pub fn add_to(&self, builder: &mut LevelBuilder) -> usize {
        let tag = Tag::level(Contents::PLAYER_CLIP, 0);
        self.brushes
            .iter()
            .filter(|brush| brush.shape.add_to(builder, tag))
            .count()
    }
}

impl ClipShape {
    pub fn add_to(&self, builder: &mut LevelBuilder, tag: Tag) -> bool {
        match *self {
            ClipShape::Box { min, max } => builder.aabb(min, max, tag),
            ClipShape::Wall {
                from,
                to,
                z,
                thickness,
            } => {
                let Some(along) = normalize([to[0] - from[0], to[1] - from[1], 0.0]) else {
                    return false;
                };
                let half = thickness * 0.5;
                let side = [-along[1] * half, along[0] * half];
                let mut points = Vec::with_capacity(8);
                for end in [from, to] {
                    for sign in [1.0, -1.0] {
                        for height in z {
                            points.push([end[0] + sign * side[0], end[1] + sign * side[1], height]);
                        }
                    }
                }
                let across = [-along[1], along[0], 0.0];
                let faces = [
                    across,
                    across.map(|c| -c),
                    along,
                    along.map(|c| -c),
                    [0.0, 0.0, 1.0],
                    [0.0, 0.0, -1.0],
                ];
                // Points: (end, side, height) bits 2, 1, 0; an edge joins
                // two that differ in one.
                let mut edges = Vec::with_capacity(12);
                for a in 0..8usize {
                    for bit in [1, 2, 4] {
                        if a & bit == 0 {
                            edges.push((a, a | bit));
                        }
                    }
                }
                builder.convex(&points, &faces, &edges, tag)
            }
        }
    }
}
