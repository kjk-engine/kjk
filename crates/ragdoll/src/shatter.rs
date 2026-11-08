//! Shattering a prop's mesh into pieces of itself, the same on every
//! machine: seeded points inside the meshes' bounds, each triangle to the
//! point nearest its middle, and each piece's open edge loops closed by a
//! fan (as `cut` closes a wound), textured from the mesh's own texture.
//! Meshes come in the model's frame, so pieces of a model made of several
//! nodes fit together. View-side only: nothing here reaches the sim.

use std::collections::BTreeSet;

use crate::loops::{self, cross, dot, length, sub};

/// One mesh of a model, in the model's frame. `capped` is false for a mesh
/// that is alpha-tested or two-sided (a leaf, a flag): its pieces stay open,
/// since a cap on a leaf would read as cardboard.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Plain {
    pub positions: Vec<[f32; 3]>,
    pub normals: Vec<[f32; 3]>,
    pub uvs: Vec<[f32; 2]>,
    pub indices: Vec<u32>,
    pub capped: bool,
}

/// A piece: for each source mesh (by its index) that has triangles in it,
/// its share; where its triangles' middle is; how big it is (its bounding
/// box, m³); and its points, for a convex collider.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Piece {
    pub meshes: Vec<(usize, Plain)>,
    pub middle: [f32; 3],
    pub volume: f32,
    pub hull: Vec<[f32; 3]>,
}

/// FNV-1a 64 of the lowercased name: the seed every machine cuts a model
/// with.
pub fn seed_of(name: &str) -> u64 {
    name.bytes().fold(0xcbf2_9ce4_8422_2325, |hash, byte| {
        (hash ^ u64::from(byte.to_ascii_lowercase())).wrapping_mul(0x0000_0100_0000_01b3)
    })
}

/// Splitmix64: seeded numbers, the same on every machine.
struct Dice(u64);

impl Dice {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9e37_79b9_7f4a_7c15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        z ^ (z >> 31)
    }

    /// 0 to 1 (1 never).
    fn unit(&mut self) -> f32 {
        (self.next() >> 40) as f32 / (1u64 << 24) as f32
    }
}

fn add(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    std::array::from_fn(|i| a[i] + b[i])
}

fn scale(a: [f32; 3], by: f32) -> [f32; 3] {
    a.map(|c| c * by)
}

fn lerp(a: [f32; 3], b: [f32; 3], t: f32) -> [f32; 3] {
    add(a, scale(sub(b, a), t))
}

fn unit(a: [f32; 3]) -> [f32; 3] {
    scale(a, 1.0 / length(a).max(1e-9))
}

fn triangles(mesh: &Plain) -> &[[u32; 3]] {
    mesh.indices.as_chunks::<3>().0
}

fn corners(mesh: &Plain, t: [u32; 3]) -> [[f32; 3]; 3] {
    t.map(|v| mesh.positions[v as usize])
}

fn centroid(c: [[f32; 3]; 3]) -> [f32; 3] {
    scale(add(add(c[0], c[1]), c[2]), 1.0 / 3.0)
}

/// The corners of the box around `points`; None for no points.
fn bounds<'a>(points: impl IntoIterator<Item = &'a [f32; 3]>) -> Option<([f32; 3], [f32; 3])> {
    points.into_iter().fold(None, |sofar, &p| {
        let (lo, hi) = sofar.unwrap_or((p, p));
        Some((
            std::array::from_fn(|i| lo[i].min(p[i])),
            std::array::from_fn(|i| hi[i].max(p[i])),
        ))
    })
}

impl Plain {
    /// `triangles` of the mesh as a mesh of their own (a vertex with no
    /// normal or uv gets an up normal and uv 0).
    fn part(&self, triangles: &[[u32; 3]]) -> Plain {
        let mut part = Plain {
            capped: self.capped,
            ..Plain::default()
        };
        let mut index = std::collections::BTreeMap::new();
        for t in triangles {
            for &v in t {
                let local = *index.entry(v).or_insert_with(|| {
                    let at = v as usize;
                    part.push(
                        self.positions[at],
                        self.normals.get(at).copied().unwrap_or([0.0, 0.0, 1.0]),
                        self.uvs.get(at).copied().unwrap_or([0.0; 2]),
                    )
                });
                part.indices.push(local);
            }
        }
        part
    }

    fn push(&mut self, position: [f32; 3], normal: [f32; 3], uv: [f32; 2]) -> u32 {
        self.positions.push(position);
        self.normals.push(normal);
        self.uvs.push(uv);
        (self.positions.len() - 1) as u32
    }

    /// Closes each open loop by a fan from its middle, its uvs projected on
    /// the loop's best-fit plane at the mesh's own texel size around the
    /// rim's own place in the texture: the inside reads as more of the same
    /// stuff.
    fn cap(&mut self) {
        for lap in loops::open_loops(&self.positions, &self.indices) {
            let middle = loops::middle(&self.positions, &lap);
            let (pairs, normal) = loops::fan(&self.positions, &lap, middle);
            let across = if normal[0].abs() < 0.9 {
                [1.0, 0.0, 0.0]
            } else {
                [0.0, 1.0, 0.0]
            };
            let u = unit(cross(normal, across));
            let v = cross(normal, u);
            let rim_uv = |a: u32| self.uvs[a as usize];
            let uv_middle = lap
                .iter()
                .fold([0.0f32; 2], |sum, &a| {
                    [sum[0] + rim_uv(a)[0], sum[1] + rim_uv(a)[1]]
                })
                .map(|c| c / lap.len() as f32);
            let (texels, metres) = pairs.iter().fold((0.0f32, 0.0f32), |(t, m), &(a, b)| {
                let (ua, ub) = (rim_uv(a), rim_uv(b));
                let du = ((ua[0] - ub[0]).powi(2) + (ua[1] - ub[1]).powi(2)).sqrt();
                let at = |x: u32| self.positions[x as usize];
                (t + du, m + length(sub(at(a), at(b))))
            });
            let per_metre = if metres > 1e-6 { texels / metres } else { 1.0 };
            let project = |p: [f32; 3]| {
                let d = sub(p, middle);
                [
                    uv_middle[0] + dot(d, u) * per_metre,
                    uv_middle[1] + dot(d, v) * per_metre,
                ]
            };
            let centre = self.push(middle, normal, project(middle));
            let rim: std::collections::BTreeMap<u32, u32> = lap
                .iter()
                .map(|&a| {
                    let p = self.positions[a as usize];
                    (a, self.push(p, normal, project(p)))
                })
                .collect();
            for (a, b) in pairs {
                self.indices.extend([rim[&a], rim[&b], centre]);
            }
        }
    }
}

impl Piece {
    /// A piece of `meshes`: its middle the area-weighted middle of its
    /// triangles, its hull its distinct points.
    fn of(meshes: Vec<(usize, Plain)>) -> Piece {
        let (weighted, area) = meshes
            .iter()
            .flat_map(|(_, m)| triangles(m).iter().map(|&t| corners(m, t)))
            .fold(([0.0f32; 3], 0.0f32), |(sum, total), c| {
                let a = length(cross(sub(c[1], c[0]), sub(c[2], c[0]))) * 0.5;
                (add(sum, scale(centroid(c), a)), total + a)
            });
        let hull: Vec<[f32; 3]> = meshes
            .iter()
            .flat_map(|(_, m)| &m.positions)
            .map(|p| p.map(f32::to_bits))
            .collect::<BTreeSet<_>>()
            .into_iter()
            .map(|p| p.map(f32::from_bits))
            .collect();
        let (lo, hi) = bounds(&hull).unwrap_or(([0.0; 3], [0.0; 3]));
        let middle = if area > 1e-12 {
            scale(weighted, 1.0 / area)
        } else {
            lerp(lo, hi, 0.5)
        };
        let size = sub(hi, lo);
        Piece {
            meshes,
            middle,
            volume: size[0] * size[1] * size[2],
            hull,
        }
    }
}

/// `meshes` cut into up to `count` pieces (empty cells dropped), seeded by
/// `seed`.
pub fn shatter(meshes: &[Plain], count: usize, seed: u64) -> Vec<Piece> {
    let used = meshes.iter().flat_map(|m| {
        triangles(m)
            .iter()
            .flatten()
            .map(|&v| &m.positions[v as usize])
    });
    let Some((lo, hi)) = bounds(used) else {
        return Vec::new();
    };
    let mut dice = Dice(seed);
    let points: Vec<[f32; 3]> = (0..count.max(1))
        .map(|_| std::array::from_fn(|i| lo[i] + (hi[i] - lo[i]) * dice.unit()))
        .collect();
    let mut cells = vec![vec![Vec::new(); meshes.len()]; points.len()];
    for (m, mesh) in meshes.iter().enumerate() {
        for &t in triangles(mesh) {
            let at = centroid(corners(mesh, t));
            let off = |p: &[f32; 3]| dot(sub(at, *p), sub(at, *p));
            let nearest = (0..points.len())
                .min_by(|&a, &b| off(&points[a]).total_cmp(&off(&points[b])))
                .unwrap_or(0);
            cells[nearest][m].push(t);
        }
    }
    cells
        .into_iter()
        .filter(|cell| cell.iter().any(|t| !t.is_empty()))
        .map(|cell| {
            let parts = cell
                .iter()
                .enumerate()
                .filter(|(_, t)| !t.is_empty())
                .map(|(m, t)| {
                    let mut part = meshes[m].part(t);
                    if part.capped {
                        part.cap();
                    }
                    (m, part)
                })
                .collect();
            Piece::of(parts)
        })
        .collect()
}

/// The point of triangle `c` nearest `p` (Ericson, Real-Time Collision
/// Detection 5.1.5).
fn nearest_on(c: [[f32; 3]; 3], p: [f32; 3]) -> [f32; 3] {
    let [a, b, cc] = c;
    let (ab, ac, ap) = (sub(b, a), sub(cc, a), sub(p, a));
    let (d1, d2) = (dot(ab, ap), dot(ac, ap));
    if d1 <= 0.0 && d2 <= 0.0 {
        return a;
    }
    let bp = sub(p, b);
    let (d3, d4) = (dot(ab, bp), dot(ac, bp));
    if d3 >= 0.0 && d4 <= d3 {
        return b;
    }
    let vc = d1 * d4 - d3 * d2;
    if vc <= 0.0 && d1 >= 0.0 && d3 <= 0.0 {
        return lerp(a, b, d1 / (d1 - d3));
    }
    let cp = sub(p, cc);
    let (d5, d6) = (dot(ab, cp), dot(ac, cp));
    if d6 >= 0.0 && d5 <= d6 {
        return cc;
    }
    let vb = d5 * d2 - d1 * d6;
    if vb <= 0.0 && d2 >= 0.0 && d6 <= 0.0 {
        return lerp(a, cc, d2 / (d2 - d6));
    }
    let va = d3 * d6 - d5 * d4;
    if va <= 0.0 && (d4 - d3) >= 0.0 && (d5 - d6) >= 0.0 {
        return lerp(b, cc, (d4 - d3) / ((d4 - d3) + (d5 - d6)));
    }
    let denom = 1.0 / (va + vb + vc);
    add(a, add(scale(ab, vb * denom), scale(ac, vc * denom)))
}

/// How near the triangles a hit's shards are cut from must be (m).
const SHARD_REACH: f32 = 0.25;
/// How far a shard's corners reach from its middle at most (m).
const SHARD_SIZE: f32 = 0.08;
/// How thick a shard's collider is (m).
const SHARD_THICKNESS: f32 = 0.01;

/// Up to `count` small pieces of `meshes` cut where a hit landed: chips of
/// the triangles within 0.25 m of `near` (the nearest ones if none), each a
/// small two-sided triangle of the mesh's own surface and texture around
/// the point of it nearest `near`. Uncapped; seeded by `seed`.
pub fn shards(meshes: &[Plain], near: [f32; 3], count: usize, seed: u64) -> Vec<Piece> {
    let mut found: Vec<(f32, usize, [u32; 3], [f32; 3])> = meshes
        .iter()
        .enumerate()
        .flat_map(|(m, mesh)| {
            triangles(mesh).iter().map(move |&t| {
                let at = nearest_on(corners(mesh, t), near);
                (length(sub(at, near)), m, t, at)
            })
        })
        .collect();
    found.sort_by(|a, b| a.0.total_cmp(&b.0));
    let within = found.iter().filter(|f| f.0 <= SHARD_REACH).count();
    found.truncate(if within > 0 { within } else { count });
    if found.is_empty() {
        return Vec::new();
    }
    let mut dice = Dice(seed);
    (0..count)
        .map(|i| {
            let (_, m, t, at) = found[i % found.len()];
            let mesh = &meshes[m];
            let c = corners(mesh, t);
            // A little into the triangle, so a chip at a corner is not a
            // sliver.
            let inward = sub(centroid(c), at);
            let middle = add(
                at,
                scale(
                    unit(inward),
                    length(inward).min(SHARD_SIZE * 0.5) * dice.unit(),
                ),
            );
            let normal = unit(cross(sub(c[1], c[0]), sub(c[2], c[0])));
            let uv = |v: u32| mesh.uvs.get(v as usize).copied().unwrap_or([0.0; 2]);
            let weights = c.map(|corner| {
                let reach = length(sub(corner, middle)).max(1e-6);
                (SHARD_SIZE / reach).min(1.0) * (0.5 + 0.5 * dice.unit())
            });
            // The chip's uv at its middle: the triangle's, by barycentrics
            // of `middle`.
            let bary = barycentric(c, middle);
            let uv_middle = [0, 1].map(|k| (0..3).map(|j| bary[j] * uv(t[j])[k]).sum::<f32>());
            let chip: [([f32; 3], [f32; 2]); 3] = std::array::from_fn(|j| {
                let w = weights[j];
                let corner_uv = uv(t[j]);
                (
                    lerp(middle, c[j], w),
                    [
                        uv_middle[0] + (corner_uv[0] - uv_middle[0]) * w,
                        uv_middle[1] + (corner_uv[1] - uv_middle[1]) * w,
                    ],
                )
            });
            let mut plain = Plain::default();
            for (facing, order) in [(normal, [0, 1, 2]), (scale(normal, -1.0), [0, 2, 1])] {
                for j in order {
                    let (p, uv) = chip[j];
                    let at = plain.push(p, facing, uv);
                    plain.indices.push(at);
                }
            }
            let back = scale(normal, -SHARD_THICKNESS);
            let hull = chip
                .iter()
                .map(|(p, _)| *p)
                .chain(chip.iter().map(|(p, _)| add(*p, back)))
                .collect();
            let (lo, hi) = bounds(&hull).unwrap_or(([0.0; 3], [0.0; 3]));
            let size = sub(hi, lo);
            Piece {
                meshes: vec![(m, plain)],
                middle: add(middle, scale(back, 0.5)),
                volume: size[0] * size[1] * size[2],
                hull,
            }
        })
        .collect()
}

/// Barycentric weights of `p` (on the triangle's plane) in triangle `c`.
fn barycentric(c: [[f32; 3]; 3], p: [f32; 3]) -> [f32; 3] {
    let (v0, v1, v2) = (sub(c[1], c[0]), sub(c[2], c[0]), sub(p, c[0]));
    let (d00, d01, d11) = (dot(v0, v0), dot(v0, v1), dot(v1, v1));
    let (d20, d21) = (dot(v2, v0), dot(v2, v1));
    let denom = d00 * d11 - d01 * d01;
    if denom.abs() < 1e-12 {
        return [1.0, 0.0, 0.0];
    }
    let v = (d11 * d20 - d01 * d21) / denom;
    let w = (d00 * d21 - d01 * d20) / denom;
    [1.0 - v - w, v, w]
}

/// The first point along the segment `from`–`to` where it crosses one of
/// `triangles`, and that triangle's normal turned to face `from`.
pub fn first_crossing(
    from: [f32; 3],
    to: [f32; 3],
    triangles: &[[[f32; 3]; 3]],
) -> Option<([f32; 3], [f32; 3])> {
    // Möller–Trumbore, both faces, the nearest crossing along the segment.
    let way = sub(to, from);
    triangles
        .iter()
        .filter_map(|&[a, b, c]| {
            let (e1, e2) = (sub(b, a), sub(c, a));
            let p = cross(way, e2);
            let det = dot(e1, p);
            if det.abs() < 1e-12 {
                return None;
            }
            let s = sub(from, a);
            let u = dot(s, p) / det;
            let q = cross(s, e1);
            let v = dot(way, q) / det;
            let t = dot(e2, q) / det;
            let inside = (0.0..=1.0).contains(&u) && v >= 0.0 && u + v <= 1.0;
            (inside && (0.0..=1.0).contains(&t)).then(|| {
                let normal = unit(cross(e1, e2));
                let facing = if dot(normal, way) > 0.0 {
                    scale(normal, -1.0)
                } else {
                    normal
                };
                (t, add(from, scale(way, t)), facing)
            })
        })
        .min_by(|a, b| a.0.total_cmp(&b.0))
        .map(|(_, at, normal)| (at, normal))
}

/// Presses a dent into a mesh: each vertex within `radius` of `centre`
/// moves in by `depth × (1 − (d / radius)²)`, `d` its distance from
/// `centre`, all against one way (the mean of their normals), so the
/// copies of a vertex split at a hard edge move together and no seam opens.
pub fn dent(
    positions: &mut [[f32; 3]],
    normals: &[[f32; 3]],
    centre: [f32; 3],
    radius: f32,
    depth: f32,
) {
    let within = |p: &[f32; 3]| length(sub(*p, centre)) < radius;
    let sum = (positions.iter().zip(normals))
        .filter(|(p, _)| within(p))
        .fold([0.0; 3], |sum, (_, &n)| add(sum, n));
    let size = length(sum);
    if size < 1e-6 {
        return;
    }
    let way = scale(sum, 1.0 / size);
    for p in positions.iter_mut().filter(|p| within(p)) {
        let d = length(sub(*p, centre));
        let fall = 1.0 - (d / radius) * (d / radius);
        *p = sub(*p, scale(way, depth * fall));
    }
}

#[cfg(feature = "probes")]
pub use probes::probes;

#[cfg(feature = "probes")]
mod probes {
    use super::*;

    type Checks = Vec<(&'static str, Result<String, String>)>;

    fn check(ok: bool, what: String) -> Result<String, String> {
        if ok { Ok(what) } else { Err(what) }
    }

    /// A unit cube from `at`, each face its own four vertices (split along
    /// uv seams, as KOTOR's meshes are), facing out.
    fn cube(at: [f32; 3], capped: bool) -> Plain {
        let mut mesh = Plain {
            capped,
            ..Plain::default()
        };
        for axis in 0..3 {
            for side in [0.0f32, 1.0] {
                let (u, v) = ((axis + 1) % 3, (axis + 2) % 3);
                let corner = |a: f32, b: f32| {
                    let mut p = at;
                    p[axis] += side;
                    p[u] += a;
                    p[v] += b;
                    p
                };
                let mut normal = [0.0; 3];
                normal[axis] = if side > 0.0 { 1.0 } else { -1.0 };
                let base = mesh.positions.len() as u32;
                for (a, b) in [(0.0, 0.0), (1.0, 0.0), (1.0, 1.0), (0.0, 1.0)] {
                    mesh.positions.push(corner(a, b));
                    mesh.normals.push(normal);
                    mesh.uvs.push([a, b]);
                }
                let quad = if side > 0.0 {
                    [0, 1, 2, 0, 2, 3]
                } else {
                    [0, 2, 1, 0, 3, 2]
                };
                mesh.indices.extend(quad.map(|i| base + i));
            }
        }
        mesh
    }

    type Corners = [[u32; 3]; 3];

    fn triangle_keys(mesh: &Plain) -> Vec<Corners> {
        let (triangles, _) = mesh.indices.as_chunks::<3>();
        triangles
            .iter()
            .map(|t| t.map(|v| mesh.positions[v as usize].map(f32::to_bits)))
            .collect()
    }

    /// `t` as its lowest-first turn, so a triangle reads one way however it
    /// is started.
    fn turned(t: Corners) -> Corners {
        (0..3)
            .map(|i| [t[i], t[(i + 1) % 3], t[(i + 2) % 3]])
            .min()
            .unwrap_or(t)
    }

    fn closed(mesh: &Plain) -> bool {
        let edges: BTreeSet<([u32; 3], [u32; 3])> = triangle_keys(mesh)
            .into_iter()
            .flat_map(|t| [(t[0], t[1]), (t[1], t[2]), (t[2], t[0])])
            .collect();
        edges.iter().all(|&(a, b)| edges.contains(&(b, a)))
    }

    fn covers_once() -> Result<String, String> {
        let cube = cube([0.0; 3], true);
        let pieces = shatter(std::slice::from_ref(&cube), 6, seed_of("plc_chair1"));
        let source: Vec<Corners> = triangle_keys(&cube).into_iter().map(turned).collect();
        let found: Vec<usize> = source
            .iter()
            .map(|t| {
                pieces
                    .iter()
                    .flat_map(|p| &p.meshes)
                    .flat_map(|(_, m)| triangle_keys(m))
                    .filter(|k| turned(*k) == *t)
                    .count()
            })
            .collect();
        let open = pieces
            .iter()
            .filter(|p| !p.meshes.iter().all(|(_, m)| closed(m)))
            .count();
        check(
            !pieces.is_empty() && found.iter().all(|&n| n == 1) && open == 0,
            format!(
                "{} pieces; each triangle found {found:?} times (want 1 each); \
                 {open} pieces open (want 0)",
                pieces.len()
            ),
        )
    }

    fn same_everywhere() -> Result<String, String> {
        let cube = [cube([0.0; 3], true)];
        let seed = seed_of("plc_chair1");
        let (a, b) = (shatter(&cube, 6, seed), shatter(&cube, 6, seed));
        let cased = seed_of("PLC_Chair1") == seed;
        let other = shatter(&cube, 6, seed_of("plc_chair2")) != a;
        check(
            !a.is_empty() && a == b && cased && other,
            format!(
                "same seed same pieces {}, case-blind seed {cased}, another seed cuts \
                 otherwise {other}",
                a == b
            ),
        )
    }

    fn model_frame() -> Result<String, String> {
        let meshes = [cube([0.0; 3], true), cube([2.0, 0.0, 0.0], true)];
        let pieces = shatter(&meshes, 6, seed_of("plc_two"));
        let xs: Vec<f32> = pieces.iter().map(|p| p.middle[0]).collect();
        let (low, high) = xs
            .iter()
            .fold((f32::MAX, f32::MIN), |(lo, hi), &x| (lo.min(x), hi.max(x)));
        check(
            low < 1.0 && high > 2.0,
            format!("pieces' middles span x {low:.2}..{high:.2} (want under 1 to over 2)"),
        )
    }

    fn uncapped_leaf() -> Result<String, String> {
        let leaf = Plain {
            positions: vec![[0.0; 3], [1.0, 0.0, 0.0], [1.0, 1.0, 0.0], [0.0, 1.0, 0.0]],
            normals: vec![[0.0, 0.0, 1.0]; 4],
            uvs: vec![[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]],
            indices: vec![0, 1, 2, 0, 2, 3],
            capped: false,
        };
        let pieces = shatter(&[leaf], 4, seed_of("plc_leaf"));
        let triangles: usize = pieces
            .iter()
            .flat_map(|p| &p.meshes)
            .map(|(_, m)| m.indices.len() / 3)
            .sum();
        check(
            !pieces.is_empty() && triangles == 2,
            format!(
                "{} pieces holding {triangles} triangles (want 2)",
                pieces.len()
            ),
        )
    }

    fn shards_near() -> Result<String, String> {
        let corner = [1.0, 1.0, 1.0];
        let pieces = shards(&[cube([0.0; 3], true)], corner, 5, 7);
        let far = pieces
            .iter()
            .flat_map(|p| &p.meshes)
            .flat_map(|(_, m)| &m.positions)
            .map(|&p| length(sub(p, corner)))
            .fold(0.0f32, f32::max);
        check(
            pieces.len() == 5 && far <= 0.25,
            format!(
                "{} shards (want 5), farthest point {far:.3} m from the corner (want ≤ 0.25)",
                pieces.len()
            ),
        )
    }

    fn debris_settles() -> Result<String, String> {
        let mut world = crate::World::new();
        let floor = [[-5.0, -5.0], [5.0, -5.0], [5.0, 5.0], [-5.0, 5.0]]
            .map(|[x, y]| glam::Vec3::new(x, y, 0.0));
        world.add_faces([
            (0, [floor[0], floor[1], floor[2]]),
            (1, [floor[0], floor[2], floor[3]]),
        ]);
        let hull: Vec<[f32; 3]> = (0..8)
            .map(|i| [0, 1, 2].map(|axis| if i >> axis & 1 == 1 { 0.1 } else { -0.1 }))
            .collect();
        let rotation = [0.0, 0.0, 0.0, 1.0];
        let Some(id) = world.debris(
            &hull,
            [0.0, 0.0, 1.0],
            rotation,
            [0.5, 0.0, 0.0],
            [3.0, 2.0, 1.0],
        ) else {
            return Err("a cube's hull made no piece".into());
        };
        let mut rested = None;
        for step in 0..180 {
            world.step(1.0 / 60.0);
            if world.resting_debris(id) {
                rested = Some(step as f32 / 60.0);
                break;
            }
        }
        let lowest = world.debris_pose(id).map(|(at, turn)| {
            let turn = glam::Quat::from_array(turn);
            hull.iter()
                .map(|&p| (glam::Vec3::from_array(at) + turn * glam::Vec3::from_array(p)).z)
                .fold(f32::MAX, f32::min)
        });
        world.remove_debris(id);
        let gone = world.debris_pose(id).is_none();
        check(
            rested.is_some() && lowest.is_some_and(|z| z.abs() <= 0.02) && gone,
            format!(
                "rested after {rested:?} s (want within 3), lowest point z {lowest:?} \
                 (want within 0.02 of 0), gone once removed {gone}"
            ),
        )
    }

    fn crossing() -> Result<String, String> {
        let quad = [
            [[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [1.0, 1.0, 0.0]],
            [[0.0, 0.0, 0.0], [1.0, 1.0, 0.0], [0.0, 1.0, 0.0]],
        ];
        let through = first_crossing([0.5, 0.25, 1.0], [0.5, 0.25, -1.0], &quad);
        let under = first_crossing([0.5, 0.25, -1.0], [0.5, 0.25, 1.0], &quad);
        let misses = first_crossing([2.0, 2.0, 1.0], [2.0, 2.0, -1.0], &quad);
        let good = |found: Option<([f32; 3], [f32; 3])>, up: f32| {
            found.is_some_and(|(at, normal)| at[2].abs() < 1e-5 && normal[2] == up)
        };
        check(
            good(through, 1.0) && good(under, -1.0) && misses.is_none(),
            format!(
                "down through {through:?} (want z 0, normal up), up through {under:?} \
                 (normal down), beside {misses:?} (want None)"
            ),
        )
    }

    fn dented() -> Result<String, String> {
        // A flat 5 × 5 grid a metre across, facing up.
        let mut grid: Vec<[f32; 3]> = (0..25)
            .map(|i| [(i % 5) as f32 * 0.25, (i / 5) as f32 * 0.25, 0.0])
            .collect();
        let normals = vec![[0.0, 0.0, 1.0]; 25];
        dent(&mut grid, &normals, [0.5, 0.5, 0.0], 0.5, 0.04);
        let (centre, rim, far) = (grid[12][2], grid[2][2], grid[0][2]);
        check(
            (centre + 0.04).abs() < 1e-6 && rim == 0.0 && far == 0.0,
            format!(
                "the middle went to z {centre} (want -0.04), one at the radius {rim}, a corner {far} (want 0)"
            ),
        )
    }

    /// A dent by a cube's corner keeps the copies of each split vertex
    /// together: no seam opens.
    fn dent_seams() -> Result<String, String> {
        let cube = cube([0.0; 3], true);
        let mut positions = cube.positions.clone();
        dent(&mut positions, &cube.normals, [1.0, 1.0, 0.8], 0.5, 0.08);
        let moved = (positions.iter().zip(&cube.positions))
            .filter(|(a, b)| a != b)
            .count();
        let mut torn = 0;
        for a in 0..positions.len() {
            for b in a + 1..positions.len() {
                let was = length(sub(cube.positions[a], cube.positions[b]));
                if was < 1e-6 && length(sub(positions[a], positions[b])) > 1e-5 {
                    torn += 1;
                }
            }
        }
        check(
            moved > 0 && torn == 0,
            format!(
                "{moved} vertices pressed (want some), {torn} split copies torn apart (want 0)"
            ),
        )
    }

    /// A piece whose hull is flat (one face of a box) still has a body and
    /// falls.
    fn debris_flat_falls() -> Result<String, String> {
        let mut world = crate::World::new();
        let hull = [
            [0.0, 0.0, 0.0],
            [0.3, 0.0, 0.0],
            [0.3, 0.3, 0.0],
            [0.0, 0.3, 0.0],
        ];
        let Some(id) = world.debris(
            &hull,
            [0.0, 0.0, 5.0],
            [0.0, 0.0, 0.0, 1.0],
            [0.0; 3],
            [0.0; 3],
        ) else {
            return Err("a flat hull made no piece".into());
        };
        for _ in 0..30 {
            world.step(1.0 / 60.0);
        }
        let z = world.debris_pose(id).map(|(at, _)| at[2]);
        check(
            z.is_some_and(|z| z < 4.0),
            format!("after half a second its z is {z:?} (want under 4: falling from 5)"),
        )
    }

    /// The probes.
    pub fn probes() -> Checks {
        vec![
            ("prop_stage_dent", dented()),
            ("prop_stage_dent_seams", dent_seams()),
            ("debris_flat_falls", debris_flat_falls()),
            ("prop_hit_crossing", crossing()),
            ("debris_settles", debris_settles()),
            ("shatter_covers_once", covers_once()),
            ("shatter_same_everywhere", same_everywhere()),
            ("shatter_model_frame", model_frame()),
            ("shatter_uncapped_leaf", uncapped_leaf()),
            ("shards_near_point", shards_near()),
        ]
    }
}
