//! The movement test course (I13): boxes and ramps in a straight line to
//! walk through before the Endar Spire has collision (I12). Its collision
//! is a `collision::BrushWorld`; `stage` hands movement any
//! `CollisionWorld`, so the level collision replaces it with no other
//! change.
//!
//! Metres, Z up; the walk starts at the origin looking along +Y. In order:
//! stairs of 8 units up to a platform 40 units high, and its edge; a 30°
//! ramp up to 1.73 m and a drop from it (68 units: far enough to land with
//! an animation); a tunnel 43 units high (crouch through: standing takes
//! 64); open floor to roll on; two platforms 24 units high (too high to
//! step onto) with a 3 m gap between; steps of 16, 18 and 20 units side
//! by side (18 is JA's highest); walls along both sides. Off the line: a
//! 50° ramp (too steep to walk up) and a 44° one (just walkable).

use asset_odyssey::model::{IDENTITY, MeshIr, ModelIr, NodeIr};
use asset_odyssey::texture::{Encoding, TextureFacts, TextureIr};
use bevy::math::Vec3;
use collision::{Brush, BrushWorld, UNIT, trig};

/// The tangent of `degrees` from the portable trigonometry: the course is
/// the sim's collision, so its slopes are the same bits on every machine.
pub fn tan_deg(degrees: f32) -> f32 {
    let (sin, cos) = trig::sin_cos_deg(degrees);
    sin / cos
}

/// The course's collision.
pub fn course() -> BrushWorld {
    let cuboid = Brush::cuboid;
    let u = UNIT;
    let mut world = BrushWorld::new()
        // The ground, and walls along the sides and at the end.
        .with(cuboid([-9.0, -4.0, -1.0], [9.0, 64.0, 0.0]))
        .with(cuboid([-9.5, -4.0, 0.0], [-9.0, 64.0, 2.5]))
        .with(cuboid([9.0, -4.0, 0.0], [9.5, 64.0, 2.5]))
        .with(cuboid([-9.5, 64.0, 0.0], [9.5, 64.5, 2.5]))
        .with(cuboid([-9.5, -4.5, 0.0], [9.5, -4.0, 2.5]));
    // Stairs: five steps of 8 units, 35 cm deep, to a platform.
    for step in 0..5 {
        let y = 6.0 + 0.35 * step as f32;
        world = world.with(cuboid(
            [-2.0, y, 0.0],
            [2.0, 10.0, 8.0 * (step + 1) as f32 * u],
        ));
    }
    // A 30° ramp up to a platform, and the drop from its end.
    let rise = 3.0 * tan_deg(30.0);
    world = world
        .with(Brush::ramp([-2.0, 14.0, 0.0], [2.0, 17.0, rise], 1, false))
        .with(cuboid([-2.0, 17.0, 0.0], [2.0, 20.0, rise]));
    // Off the line, to the right: a 50° ramp and a 44° one.
    let steep = 2.0 * tan_deg(50.0);
    let walkable = 2.0 * tan_deg(44.0);
    world = world
        .with(Brush::ramp([4.0, 14.0, 0.0], [6.0, 16.0, steep], 1, false))
        .with(Brush::ramp(
            [6.5, 14.0, 0.0],
            [8.5, 16.0, walkable],
            1,
            false,
        ));
    // The tunnel: a roof 43 units up, walls either side.
    world = world
        .with(cuboid([-2.5, 28.0, 43.0 * u], [2.5, 32.0, 43.0 * u + 0.3]))
        .with(cuboid([-2.5, 28.0, 0.0], [-2.0, 32.0, 43.0 * u]))
        .with(cuboid([2.0, 28.0, 0.0], [2.5, 32.0, 43.0 * u]));
    // Two platforms 24 units high and the gap between.
    world = world
        .with(cuboid([-2.0, 43.0, 0.0], [2.0, 46.5, 24.0 * u]))
        .with(cuboid([-2.0, 49.5, 0.0], [2.0, 52.5, 24.0 * u]));
    // Steps of 16, 18 and 20 units side by side.
    for (x, rise) in [(-4.5f32, 16.0f32), (0.0, 18.0), (4.5, 20.0)] {
        world = world.with(cuboid([x - 1.5, 55.5, 0.0], [x + 1.5, 58.5, rise * u]));
    }
    world
}

/// The duel stage's collision: a floor 20 m square, walled.
pub fn duel() -> BrushWorld {
    let cuboid = Brush::cuboid;
    BrushWorld::new()
        .with(cuboid([-10.0, -10.0, -1.0], [10.0, 10.0, 0.0]))
        .with(cuboid([-10.5, -10.5, 0.0], [-10.0, 10.5, 2.5]))
        .with(cuboid([10.0, -10.5, 0.0], [10.5, 10.5, 2.5]))
        .with(cuboid([-10.0, -10.5, 0.0], [10.0, -10.0, 2.5]))
        .with(cuboid([-10.0, 10.0, 0.0], [10.0, 10.5, 2.5]))
}

/// The probes' ring as a model's brushes: their floor, 40 m square, no
/// walls (`stage::ring`).
pub fn ring() -> BrushWorld {
    let half = crate::stage::RING_HALF;
    BrushWorld::new().with(Brush::cuboid([-half, -half, -1.0], [half, half, 0.0]))
}

/// Texture names: floors, and sides facing X and Y, each a shade of the
/// same checks so edges read.
const TEXTURES: [(&str, u8); 3] = [
    ("kjk_course_top", 96),
    ("kjk_course_x", 64),
    ("kjk_course_y", 78),
];

/// The course as a model to draw (every brush face, with 1 m checks in
/// world space) and its textures.
pub fn model(world: &BrushWorld) -> (ModelIr, Vec<(String, TextureIr)>) {
    let mut meshes: Vec<MeshIr> = TEXTURES
        .iter()
        .map(|(name, _)| MeshIr {
            node: 0,
            positions: Vec::new(),
            normals: Vec::new(),
            uv1: Vec::new(),
            uv2: Vec::new(),
            indices: Vec::new(),
            texture: Some((*name).into()),
            lightmap: None,
            transparency_hint: 0,
            skinned: false,
            self_illum: [0.0; 3],
            alpha: 1.0,
            ambient: [1.0; 3],
            diffuse: [1.0; 3],
            uv_scroll: None,
            skin: None,
        })
        .collect();
    for brush in &world.brushes {
        for (i, plane) in brush.planes.iter().enumerate() {
            let polygon = face(brush, i);
            if polygon.len() < 3 {
                continue;
            }
            let n = Vec3::from_array(plane.normal);
            let (mesh, axes) = if n.z.abs() >= n.x.abs().max(n.y.abs()) {
                (0, (0, 1))
            } else if n.x.abs() >= n.y.abs() {
                (1, (1, 2))
            } else {
                (2, (0, 2))
            };
            let mesh = &mut meshes[mesh];
            let base = mesh.positions.len() as u32;
            for p in &polygon {
                mesh.positions.push(p.to_array());
                mesh.normals.push(n.to_array());
                // Half a texture repeat a metre: one check a metre.
                mesh.uv1.push([p[axes.0] * 0.5, p[axes.1] * 0.5]);
            }
            for k in 1..polygon.len() as u32 - 1 {
                mesh.indices.extend([base, base + k, base + k + 1]);
            }
        }
    }
    let ir = ModelIr {
        name: "course".into(),
        supermodel: None,
        nodes: vec![NodeIr {
            name: "course".into(),
            parent: None,
            translation: [0.0; 3],
            rotation: [0.0, 0.0, 0.0, 1.0],
            world: IDENTITY,
            animated: false,
        }],
        meshes: meshes
            .into_iter()
            .filter(|m| !m.indices.is_empty())
            .collect(),
        lights: Vec::new(),
        emitters: Vec::new(),
        animations: Vec::new(),
        animation_scale: 1.0,
        affected_by_fog: false,
        refusals: Vec::new(),
    };
    let textures = TEXTURES
        .iter()
        .map(|(name, grey)| ((*name).to_string(), checks(*grey)))
        .collect();
    (ir, textures)
}

/// Face `i` of a convex brush: a big square on its plane cut by every
/// other plane, wound counter-clockwise seen from outside.
fn face(brush: &Brush, i: usize) -> Vec<Vec3> {
    let plane = brush.planes[i];
    let n = Vec3::from_array(plane.normal);
    let centre = n * plane.dist;
    let helper = if n.z.abs() < 0.9 { Vec3::Z } else { Vec3::X };
    let u = n.cross(helper).normalize() * 1000.0;
    let v = n.cross(u).normalize() * 1000.0;
    let mut polygon = vec![
        centre - u - v,
        centre + u - v,
        centre + u + v,
        centre - u + v,
    ];
    if n.dot((polygon[1] - polygon[0]).cross(polygon[2] - polygon[0])) < 0.0 {
        polygon.reverse();
    }
    for (j, other) in brush.planes.iter().enumerate() {
        if j == i {
            continue;
        }
        let m = Vec3::from_array(other.normal);
        if m.dot(n) > 0.9999 {
            // The same plane twice: keep the first.
            if j < i {
                return Vec::new();
            }
            continue;
        }
        let side = |p: Vec3| p.dot(m) - other.dist;
        let mut clipped = Vec::with_capacity(polygon.len() + 1);
        for k in 0..polygon.len() {
            let (a, b) = (polygon[k], polygon[(k + 1) % polygon.len()]);
            let (da, db) = (side(a), side(b));
            if da <= 0.0 {
                clipped.push(a);
            }
            if (da < 0.0 && db > 0.0) || (da > 0.0 && db < 0.0) {
                clipped.push(a + (b - a) * (da / (da - db)));
            }
        }
        polygon = clipped;
        if polygon.len() < 3 {
            return Vec::new();
        }
    }
    polygon
}

/// 64 × 64 pixels: two checks a repeat, around `grey`.
fn checks(grey: u8) -> TextureIr {
    let size = 64u32;
    let mut pixels = Vec::with_capacity((size * size * 4) as usize);
    for y in 0..size {
        for x in 0..size {
            let light = (x < size / 2) ^ (y < size / 2);
            let g = if light { grey } else { grey - 18 };
            pixels.extend([g, g, g.saturating_add(6), 255]);
        }
    }
    TextureIr {
        width: size,
        height: size,
        encoding: Encoding::Rgba8,
        mips: vec![pixels],
        cube: false,
        frames: 1,
        facts: TextureFacts::default(),
        faces: Vec::new(),
        frame_mips: Vec::new(),
    }
}
