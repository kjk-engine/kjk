//! The robe solver stepped again on a record of play (`KJK_LOOSE_RECORD`),
//! without the game, and a count of the boot vertices that come out
//! through the back of the robe in each frame (`kjk loose-replay`).
//!
//! Two game runs of the same moves never pose the body the same way, so
//! their recordings cannot weigh a small change to the solver. A record
//! holds the animated skeleton that the solver read in each frame of one
//! run. Replayed, it gives the same count every time, and every build of
//! the solver meets the same strides.
//!
//! The body is skinned on the CPU as the game skins it, with the push out
//! of the legs that the vertex shader adds (`loose_skin`). A boot vertex
//! is one that the foot carries by half or more, and a robe vertex one that
//! the solved bones carry by half or more, as `KJK_LOOSE_PAINT` paints them.
//!
//! A boot vertex is through the robe's back when a ray from it toward the
//! robe's back meets no robe triangle, and a ray toward the front meets
//! one. A boot below the hem meets nothing either way, and a boot inside
//! the robe meets the robe both ways. A boot under a hem that flares up
//! behind is seen under the robe, not through it, so the boot must meet no
//! robe above it either. It must also meet no robe on one side at least,
//! so that a camera beside the body would see it. The back is the robe's
//! trailing side, away from the way the pelvis moves, as the side camera
//! sees it. The rays run across the body's up axis, from the pelvis to the
//! head. In a roll, where the body lies along the floor, a boot out of the
//! robe's open bottom is then below the hem and does not count. A frame in
//! which the pelvis hardly moves has no trailing side and counts nothing.
//!
//! The report also gives the solver's mean time for one step of the body,
//! without the skinning and the count.

use std::fmt::Write as _;
use std::path::Path;
use std::time::Instant;

use super::*;

pub struct Options<'a> {
    pub game: vfs::Game,
    /// The body model the record moved.
    pub body: &'a str,
    pub record: &'a Path,
    /// Where to write each frame's game time and count (CSV), if anywhere.
    pub frames: Option<&'a Path>,
    /// Where to write each frame's robe triangles, boot vertices, trailing
    /// side, up axis, solved points, targets and capsules in the world (one
    /// JSON line a frame), if anywhere.
    pub dump: Option<&'a Path>,
    /// A folder to write every frame's skinned body into, twice: as the
    /// animation poses it, and with the solved bones and the push out of
    /// the legs (`export`).
    pub export: Option<&'a Path>,
}

/// How long the pelvis's speed is averaged over to find the robe's
/// trailing side (seconds).
const SMOOTHING: f32 = 0.1;
/// The least speed of the pelvis across its up axis that gives the robe a
/// trailing side (metres per second).
const LEAST_SPEED: f32 = 0.5;

/// The boot vertices through the robe's back in a frame from which the
/// frame counts as one where the boot shows.
const SHOWS_FROM: usize = 3;

/// One skinned mesh's vertices that the measure needs.
struct Mesh {
    /// The joint matrices' rest inverses, one for each slot.
    binds: Vec<Mat4>,
    joints: Vec<usize>,
    /// Each boot vertex, by number.
    boots: Vec<usize>,
    /// The robe's triangles: those with a robe vertex.
    triangles: Vec<[usize; 3]>,
    /// The share of each vertex that the solved bones carry.
    loose: Vec<f32>,
    push: Option<Vec<[f32; 4]>>,
}

/// Steps the solver of `options.body` on the record and counts, in each
/// frame, the boot vertices through the robe's back. Returns the report.
pub fn replay(discovery: &vfs::Discovery, options: &Options) -> Result<String, String> {
    solve_in_this_run(true);
    tune();
    let resolver = crate::viewer::resolver(discovery, options.game, None)?;
    let mut cache = BTreeMap::new();
    let ir = crate::viewer::cached(&mut cache, &resolver, options.body)?
        .ok_or(format!("{}.mdl: not found", options.body))?;
    let chain = crate::viewer::chain_clips(&mut cache, &resolver, &ir)?;
    let clips: Vec<&AnimationIr> = chain.iter().collect();
    let model = Model::new(&ir, &clips).ok_or(format!("{}: no robe solver", ir.name))?;
    let found = finder::rig(&ir, &clips);
    let moved: Vec<usize> = simulated(&found).flat_map(bones_of).collect();
    let feet: Vec<usize> = (model.legs.iter().enumerate())
        .filter(|(index, _)| index % 3 == 2)
        .map(|(_, &(node, ..))| node)
        .collect();
    let meshes = meshes(&ir, &model, (&moved, &feet));
    let rest = crate::cast::rest_of(&ir);
    let parents: Vec<Option<usize>> = rest.iter().map(|(parent, ..)| *parent).collect();
    let pelvis = ir
        .node_index(asset_odyssey::hooks::PELVIS)
        .ok_or("no pelvis")?;
    let head = ir
        .node_index(asset_odyssey::hooks::HEAD_HOOK)
        .ok_or("no head hook")?;
    let text = std::fs::read_to_string(options.record)
        .map_err(|why| format!("{}: {why}", options.record.display()))?;
    let mut dump = match options.dump {
        Some(path) => {
            Some(std::fs::File::create(path).map_err(|why| format!("{}: {why}", path.display()))?)
        }
        None => None,
    };
    let mut export = match options.export {
        Some(dir) => Some(Export::new(dir, &ir, &meshes)?),
        None => None,
    };
    let mut solver = model.solver.clone();
    let mut body = None;
    let mut csv = String::from("ms,through,sunk\n");
    let (mut frames, mut shows, mut total) = (0usize, 0usize, 0usize);
    let (mut sunk, mut sunk_frames) = (0usize, 0usize);
    let (mut steps, mut solving) = (0usize, 0.0f64);
    let mut locals = Vec::new();
    let mut squeezes: Vec<f32> = Vec::new();
    let mut last_ms = None;
    let (mut moving, mut heading) = (None, Vec3::ZERO);
    for line in text.lines() {
        let value: serde_json::Value =
            serde_json::from_str(line).map_err(|why| format!("a record line: {why}"))?;
        if value["model"].as_str() != Some(&model.name) {
            continue;
        }
        let root = value["body"].as_u64();
        if *body.get_or_insert(root) != root {
            continue;
        }
        let numbers = |key: &str| -> Vec<f32> {
            (value[key].as_array().into_iter().flatten())
                .filter_map(|n| n.as_f64().map(|n| n as f32))
                .collect()
        };
        let frame = |n: &[f32]| {
            Frame::new(
                Vec3::new(n[0], n[1], n[2]),
                Quat::from_xyzw(n[3], n[4], n[5], n[6]),
            )
        };
        let placed = frame(&numbers("root"));
        locals.clear();
        locals.extend(
            numbers("locals")
                .as_chunks::<7>()
                .0
                .iter()
                .map(|n| frame(n)),
        );
        if locals.len() != parents.len() {
            return Err(format!(
                "the record has {} nodes and {} has {}",
                locals.len(),
                ir.name,
                parents.len()
            ));
        }
        let dt = value["dt"].as_f64().map(|dt| dt as f32);
        let floor = value["floor"].as_f64().map(|f| f as f32);
        let started = Instant::now();
        if dt.is_none() {
            solver.reset();
        }
        let animated = export
            .is_some()
            .then(|| world_of(&parents, &locals, placed));
        let step = solver.step(dt.unwrap_or(0.0), placed, &locals, floor);
        for &(node, turn) in step.turns {
            locals[node].rotation = turn;
        }
        solving += started.elapsed().as_secs_f64();
        steps += 1;
        squeezes.extend(
            (solver.links())
                .filter(|&(_, _, rest, _)| rest > 0.0)
                .map(|(_, _, rest, now)| (1.0 - now / rest).max(0.0)),
        );
        let world: Vec<Mat4> = walked(&parents, &locals)
            .iter()
            .map(|f| {
                let f = placed.then(*f);
                Mat4::from_rotation_translation(f.rotation, f.translation)
            })
            .collect();
        let pelvis_at = world[pelvis].w_axis.truncate();
        let ms = value["ms"].as_f64().unwrap_or(0.0);
        if let (Some(export), Some(animated)) = (export.as_mut(), animated.as_ref())
            && last_ms != Some(ms)
        {
            let up = (world[head].w_axis - world[pelvis].w_axis).truncate();
            export.frame(
                &ir,
                &meshes,
                (animated, &world),
                (ms, pelvis_at, up),
                &model.capsules,
            )?;
        }
        if let Some((at, then)) = moving.replace((pelvis_at, ms))
            && ms > then
        {
            let velocity = (pelvis_at - at) / ((ms - then) / 1000.0) as f32;
            let share = (((ms - then) / 1000.0) as f32 / SMOOTHING).min(1.0);
            heading = heading.lerp(velocity, share);
        }
        if last_ms.replace(ms) == Some(ms) {
            continue;
        }
        let up = (world[head].w_axis - world[pelvis].w_axis)
            .truncate()
            .normalize_or(Vec3::Z);
        let across = heading - up * heading.dot(up);
        let (robe, boots) = skinned(&ir, &meshes, &world, &model.capsules);
        let back = -across.normalize_or_zero();
        let through = match across.length() >= LEAST_SPEED {
            true => through(&robe, &boots, (back, up)),
            false => 0,
        };
        let in_body = {
            let view = solver.view();
            let capsules: Vec<([Vec3; 2], f32, bool)> = (view.capsules.iter())
                .map(|(ends, radius, leg)| (ends.map(|p| view.anchor.point(p)), *radius, *leg))
                .collect();
            (robe.iter().flatten())
                .filter(|&&p| {
                    capsules.iter().any(|&([a, b], radius, leg)| {
                        let from_axis = p.distance(ragdoll::closest_on_segment(p, a, b));
                        match leg {
                            true => from_axis < radius - SUNK_IN_LEG,
                            false => from_axis < radius * SUNK_IN_TRUNK,
                        }
                    })
                })
                .count()
        };
        sunk += in_body;
        sunk_frames += usize::from(in_body >= SUNK_FROM);
        if let Some(file) = dump.as_mut() {
            use std::io::Write;
            if frames == 0 {
                let loose: Vec<[f32; 3]> = (meshes.iter().flatten())
                    .flat_map(|mesh| mesh.triangles.iter().map(|t| t.map(|v| mesh.loose[v])))
                    .collect();
                writeln!(file, "{}", serde_json::json!({ "loose": loose }))
                    .map_err(|why| format!("the dump: {why}"))?;
            }
            let view = solver.view();
            let world = |p: &Vec3| view.anchor.point(*p).to_array();
            let line = serde_json::json!({
                "ms": ms,
                "back": back.to_array(),
                "up": up.to_array(),
                "boots": boots.iter().map(|p| p.to_array()).collect::<Vec<_>>(),
                "robe": robe.iter().map(|t| t.map(|p| p.to_array())).collect::<Vec<_>>(),
                "points": view.positions.iter().map(world).collect::<Vec<_>>(),
                "targets": view.targets.iter().map(world).collect::<Vec<_>>(),
                "parents": view.parents,
                "capsules": (view.capsules.iter())
                    .map(|(ends, radius, leg)| (world(&ends[0]), world(&ends[1]), radius, leg))
                    .collect::<Vec<_>>(),
            });
            writeln!(file, "{line}").map_err(|why| format!("the dump: {why}"))?;
        }
        let _ = writeln!(csv, "{ms:.2},{through},{in_body}");
        frames += 1;
        total += through;
        shows += usize::from(through >= SHOWS_FROM);
    }
    squeezes.sort_by(f32::total_cmp);
    let at = |share: f64| {
        squeezes
            .get((share * squeezes.len() as f64) as usize)
            .copied()
            .unwrap_or(0.0)
    };
    let squeeze = format!(
        "; links between chains squeezed: median {:.2}, 90% {:.2}, 99% {:.2}",
        at(0.5),
        at(0.9),
        at(0.99)
    );
    if let Some(path) = options.frames {
        std::fs::write(path, csv).map_err(|why| format!("{}: {why}", path.display()))?;
    }
    Ok(format!(
        "{}: {frames} frames, {shows} with {SHOWS_FROM} or more boot vertices through the robe's back, {total} in all; {sunk_frames} with {SUNK_FROM} or more robe corners sunk in the body, {sunk} in all; the solver took {:.1} µs a step",
        ir.name,
        solving * 1e6 / steps.max(1) as f64
    ) + &squeeze)
}

/// How deep in a leg's capsule a corner of a robe triangle must be to count
/// as sunk in the body (metres).
const SUNK_IN_LEG: f32 = 0.03;
/// How deep in any other capsule, as a share of its radius from the axis.
/// Many robe vertices rest a little inside the trunk's capsules.
const SUNK_IN_TRUNK: f32 = 0.5;
/// The fewest sunk corners that make a frame count.
const SUNK_FROM: usize = 10;

/// Each node's matrix in the world, with the model placed at `placed`.
fn world_of(parents: &[Option<usize>], locals: &[Frame], placed: Frame) -> Vec<Mat4> {
    walked(parents, locals)
        .iter()
        .map(|f| {
            let f = placed.then(*f);
            Mat4::from_rotation_translation(f.rotation, f.translation)
        })
        .collect()
}

/// The skinned body written for an offline renderer or cloth solver
/// (`Options::export`). `mesh.json` holds each skinned mesh's triangles and
/// each vertex's share on the solved bones. `frames.bin` holds, for each
/// frame, its game time in milliseconds, the pelvis, the body's up axis,
/// then each mesh's vertices as the animation poses them and as the solved
/// bones and the push pose them, all as little-endian 32-bit floats.
struct Export {
    file: std::io::BufWriter<std::fs::File>,
}

impl Export {
    fn new(dir: &Path, ir: &ModelIr, meshes: &[Option<Mesh>]) -> Result<Export, String> {
        std::fs::create_dir_all(dir).map_err(|why| format!("{}: {why}", dir.display()))?;
        let list: Vec<serde_json::Value> = (meshes.iter().zip(&ir.meshes))
            .filter_map(|(mesh, ir_mesh)| {
                let mesh = mesh.as_ref()?;
                Some(serde_json::json!({
                    "node": ir_mesh.node,
                    "texture": ir_mesh.texture,
                    "vertices": ir_mesh.positions.len(),
                    "triangles": ir_mesh.indices.as_chunks::<3>().0,
                    "loose": mesh.loose,
                    "uvs": ir_mesh.uv1,
                }))
            })
            .collect();
        let json = serde_json::json!({ "model": ir.name, "meshes": list });
        std::fs::write(dir.join("mesh.json"), json.to_string())
            .map_err(|why| format!("mesh.json: {why}"))?;
        let file = std::fs::File::create(dir.join("frames.bin"))
            .map_err(|why| format!("frames.bin: {why}"))?;
        Ok(Export {
            file: std::io::BufWriter::new(file),
        })
    }

    fn frame(
        &mut self,
        ir: &ModelIr,
        meshes: &[Option<Mesh>],
        (animated, solved): (&[Mat4], &[Mat4]),
        (ms, pelvis, up): (f64, Vec3, Vec3),
        body: &[(usize, [Vec3; 2], f32)],
    ) -> Result<(), String> {
        use std::io::Write;
        let mut out: Vec<f32> = vec![ms as f32];
        out.extend(pelvis.to_array());
        out.extend(up.to_array());
        let capsules: Vec<([Vec3; 2], f32)> = (body.iter())
            .map(|&(node, ends, radius)| {
                (
                    ends.map(|end| solved[node].transform_point3(end)),
                    radius + crate::loose_skin::CLOTH_OFF_LEG,
                )
            })
            .collect();
        for (mesh, ir_mesh) in meshes.iter().zip(&ir.meshes) {
            let Some(mesh) = mesh else { continue };
            for (world, pushed) in [(animated, false), (solved, true)] {
                for p in skin_all(ir_mesh, mesh, world) {
                    let p = match (&mesh.push, pushed) {
                        (Some(push), true) => out_of_legs(p.0, &capsules, push[p.1]),
                        _ => p.0,
                    };
                    out.extend(p.to_array());
                }
            }
        }
        let bytes: Vec<u8> = out.iter().flat_map(|f| f.to_le_bytes()).collect();
        self.file
            .write_all(&bytes)
            .map_err(|why| format!("frames.bin: {why}"))
    }
}

/// Every vertex of a skinned mesh placed by the nodes at `world`, with its
/// number.
fn skin_all(
    ir_mesh: &asset_odyssey::model::MeshIr,
    mesh: &Mesh,
    world: &[Mat4],
) -> Vec<(Vec3, usize)> {
    let skin = ir_mesh.skin.as_ref().expect("an exported mesh is skinned");
    let joint: Vec<Mat4> = (mesh.joints.iter().zip(&mesh.binds))
        .map(|(&node, bind)| world[node] * *bind)
        .collect();
    (0..ir_mesh.positions.len())
        .map(|vertex| {
            let mut placed = Mat4::ZERO;
            for (&slot, &weight) in skin.vertex_joints[vertex]
                .iter()
                .zip(&skin.vertex_weights[vertex])
            {
                if let Some(m) = joint.get(usize::from(slot)) {
                    placed += *m * weight;
                }
            }
            (
                placed.transform_point3(Vec3::from_array(ir_mesh.positions[vertex])),
                vertex,
            )
        })
        .collect()
}

/// Each node's frame in model space, from each node's frame in its
/// parent's.
fn walked(parents: &[Option<usize>], locals: &[Frame]) -> Vec<Frame> {
    let mut frames: Vec<Frame> = Vec::with_capacity(locals.len());
    for (node, &local) in locals.iter().enumerate() {
        let frame = match parents[node] {
            Some(parent) => frames[parent].then(local),
            None => local,
        };
        frames.push(frame);
    }
    frames
}

/// The robe and boot vertices of each skinned mesh of `ir`, painted as
/// `KJK_LOOSE_PAINT` paints them from the bones `moved` and the feet `feet`.
fn meshes(ir: &ModelIr, model: &Model, (moved, feet): (&[usize], &[usize])) -> Vec<Option<Mesh>> {
    ir.meshes
        .iter()
        .enumerate()
        .map(|(index, mesh)| {
            let skin = mesh.skin.as_ref()?;
            let share = |vertex: usize, nodes: &[usize]| {
                let (Some(slots), Some(weights)) = (
                    skin.vertex_joints.get(vertex),
                    skin.vertex_weights.get(vertex),
                ) else {
                    return 0.0;
                };
                (slots.iter().zip(weights))
                    .filter(|&(&slot, _)| {
                        (skin.joints.get(usize::from(slot)))
                            .is_some_and(|node| nodes.contains(node))
                    })
                    .map(|(_, &weight)| weight)
                    .sum::<f32>()
            };
            let robe: Vec<usize> = (0..mesh.positions.len())
                .filter(|&v| share(v, moved) >= 0.5)
                .collect();
            let boots: Vec<usize> = (0..mesh.positions.len())
                .filter(|&v| share(v, moved) < 0.5 && share(v, feet) >= 0.5)
                .collect();
            let triangles = (mesh.indices.as_chunks::<3>().0.iter())
                .map(|t| t.map(|v| v as usize))
                .filter(|t| t.iter().any(|v| robe.binary_search(v).is_ok()))
                .collect();
            Some(Mesh {
                binds: (skin.inverse_binds.iter())
                    .map(crate::scene::to_mat4)
                    .collect(),
                joints: skin.joints.clone(),
                boots,
                triangles,
                loose: (0..mesh.positions.len()).map(|v| share(v, moved)).collect(),
                push: pushing()
                    .then(|| model.pushes.get(index).cloned().flatten())
                    .flatten(),
            })
        })
        .collect()
}

/// The robe's triangles and the boot vertices, skinned with the nodes at
/// `world` and pushed out of the body capsules `body` as the vertex shader
/// pushes them.
fn skinned(
    ir: &ModelIr,
    meshes: &[Option<Mesh>],
    world: &[Mat4],
    body: &[(usize, [Vec3; 2], f32)],
) -> (Vec<[Vec3; 3]>, Vec<Vec3>) {
    let capsules: Vec<([Vec3; 2], f32)> = (body.iter())
        .map(|&(node, ends, radius)| {
            (
                ends.map(|end| world[node].transform_point3(end)),
                radius + crate::loose_skin::CLOTH_OFF_LEG,
            )
        })
        .collect();
    let mut robe = Vec::new();
    let mut boots = Vec::new();
    for (mesh, ir_mesh) in meshes.iter().zip(&ir.meshes) {
        let Some(mesh) = mesh else { continue };
        let skin = ir_mesh.skin.as_ref().expect("a measured mesh is skinned");
        let joint: Vec<Mat4> = (mesh.joints.iter().zip(&mesh.binds))
            .map(|(&node, bind)| world[node] * *bind)
            .collect();
        let mut at: Vec<Option<Vec3>> = vec![None; ir_mesh.positions.len()];
        let mut place = |vertex: usize| -> Vec3 {
            *at[vertex].get_or_insert_with(|| {
                let mut placed = Mat4::ZERO;
                let slots = skin.vertex_joints[vertex].iter();
                for (&slot, &weight) in slots.zip(&skin.vertex_weights[vertex]) {
                    if let Some(m) = joint.get(usize::from(slot)) {
                        placed += *m * weight;
                    }
                }
                let p = placed.transform_point3(Vec3::from_array(ir_mesh.positions[vertex]));
                match &mesh.push {
                    Some(push) => out_of_legs(p, &capsules, push[vertex]),
                    None => p,
                }
            })
        };
        robe.extend(mesh.triangles.iter().map(|t| t.map(&mut place)));
        boots.extend(mesh.boots.iter().map(|&v| place(v)));
    }
    (robe, boots)
}

/// How many of the boot vertices `boots` are through the back of the robe
/// `robe`, with the back toward `back` and the body's up axis `up`.
fn through(robe: &[[Vec3; 3]], boots: &[Vec3], (back, up): (Vec3, Vec3)) -> usize {
    let meets = |from: Vec3, way: Vec3| robe.iter().any(|t| ray_meets(from, way, t));
    let side = up.cross(back);
    (boots.iter())
        .filter(|&&p| {
            !meets(p, back)
                && meets(p, -back)
                && !meets(p, up)
                && (!meets(p, side) || !meets(p, -side))
        })
        .count()
}

/// `loose_legs.wgsl`'s `out_of_legs`, on the CPU. `capsules` are the
/// body's, the legs first, with `loose_skin::CLOTH_OFF_LEG` in the radius.
fn out_of_legs(at: Vec3, capsules: &[([Vec3; 2], f32)], push: [f32; 4]) -> Vec3 {
    let mut p = at;
    if push[0] <= 0.0 {
        return p;
    }
    let inside = (push[0] + 0.5) as u32 - 1;
    for (i, &([a, b], radius)) in capsules.iter().enumerate() {
        let rests = match i.checked_sub(crate::loose_skin::LEG_CAPSULES) {
            None => push[1],
            Some(bit) if inside >> bit & 1 == 1 => continue,
            Some(_) => 0.0,
        };
        let radius = radius - rests;
        let ab = b - a;
        let t = ((p - a).dot(ab) / ab.length_squared().max(1e-8)).clamp(0.0, 1.0);
        let out = p - (a + ab * t);
        let distance = out.length();
        if distance < radius && distance >= 1e-5 {
            p += out / distance * (radius - distance);
        }
    }
    p
}

/// Whether the ray from `from` toward `way` meets the triangle `t`.
fn ray_meets(from: Vec3, way: Vec3, t: &[Vec3; 3]) -> bool {
    let (e1, e2) = (t[1] - t[0], t[2] - t[0]);
    let p = way.cross(e2);
    let det = e1.dot(p);
    if det.abs() < 1e-9 {
        return false;
    }
    let inv = 1.0 / det;
    let s = from - t[0];
    let u = s.dot(p) * inv;
    if !(0.0..=1.0).contains(&u) {
        return false;
    }
    let q = s.cross(e1);
    let v = way.dot(q) * inv;
    if v < 0.0 || u + v > 1.0 {
        return false;
    }
    e2.dot(q) * inv > 0.0
}
