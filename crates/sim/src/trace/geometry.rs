//! The test worlds movement scenarios walk: data made here, in JA units
//! (inches; the floor's top at z 0), turned into the collision's metres
//! when built. A library of named worlds (`world NAME`), and the shapes a
//! scenario may add (`box`, `ramp`, `prism`, `body`), each with places a
//! random scenario may start from.

use collision::{Body, Brush, BrushWorld, Contents, Plane, UNIT};

use super::scenario::{Line, float, int};
use crate::motion::{SOLE_Z, WALKABLE_NORMAL_Z};

/// A world being built: brushes and bodies, and where to start.
#[derive(Debug, Clone, Default)]
pub struct Geometry {
    pub brushes: Vec<Brush>,
    pub bodies: Vec<Body>,
    /// Places to start a random scenario: (min, max) of the origin, units.
    pub spawns: Vec<([f32; 3], [f32; 3])>,
}

fn metres(v: [f32; 3]) -> [f32; 3] {
    v.map(|c| c * UNIT)
}

/// Standing on a floor whose top is at `z`: the origin's height, the soles
/// an eighth of a unit over it (a box touching the floor starts in it).
pub fn stand_z(z: f32) -> f32 {
    z - SOLE_Z + 0.125
}

impl Geometry {
    pub fn world(&self) -> BrushWorld {
        BrushWorld {
            brushes: self.brushes.clone(),
        }
    }

    fn spawn(&mut self, min: [f32; 3], max: [f32; 3]) {
        self.spawns.push((min, max));
    }

    /// A box of `contents` (units).
    pub fn add_box(&mut self, min: [f32; 3], max: [f32; 3], contents: Contents) {
        let mut brush = Brush::cuboid(metres(min), metres(max));
        brush.contents = contents;
        self.brushes.push(brush);
    }

    pub fn solid(&mut self, min: [f32; 3], max: [f32; 3]) {
        self.add_box(min, max, Contents::SOLID);
    }

    /// A ramp over `min`..`max` rising along `axis` (0 x, 1 y).
    pub fn ramp(&mut self, min: [f32; 3], max: [f32; 3], axis: usize, down: bool) {
        self.brushes
            .push(Brush::ramp(metres(min), metres(max), axis, down));
    }

    /// An upright prism over the convex polygon `points` (x, y; counter-
    /// clockwise from above) from `low` to `high`, with its bounds as
    /// bevels.
    pub fn prism(&mut self, points: &[[f32; 2]], low: f32, high: f32) {
        let mut min = [f32::MAX, f32::MAX, low];
        let mut max = [f32::MIN, f32::MIN, high];
        for p in points {
            for axis in 0..2 {
                min[axis] = min[axis].min(p[axis]);
                max[axis] = max[axis].max(p[axis]);
            }
        }
        let mut brush = Brush::cuboid(metres(min), metres(max));
        for (i, a) in points.iter().enumerate() {
            let b = points[(i + 1) % points.len()];
            let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
            let length = (dx * dx + dy * dy).sqrt();
            if length == 0.0 {
                continue;
            }
            let normal = [dy / length, -dx / length, 0.0];
            let dist = (normal[0] * a[0] + normal[1] * a[1]) * UNIT;
            brush.planes.push(Plane { normal, dist });
        }
        self.brushes.push(brush);
    }

    /// A body (an actor's box, `Contents::BODY`) of `entity`.
    pub fn body(&mut self, entity: u32, min: [f32; 3], max: [f32; 3]) {
        self.bodies.push(Body {
            entity,
            min: metres(min),
            max: metres(max),
            contents: Contents::BODY,
        });
    }

    /// The library's world `name`.
    pub fn named(name: &str) -> Result<Geometry, String> {
        let mut g = Geometry::default();
        match name {
            "void" => g.spawn([-64.0, -64.0, 0.0], [64.0, 64.0, 512.0]),
            "flat" => g.flat(),
            "ramps" => g.ramps(),
            "stairs" => g.stairs(),
            "ledges" => g.ledges(),
            "ceilings" => g.ceilings(),
            "wedges" => g.wedges(),
            "walls" => g.walls(),
            "boxes" => g.boxes(),
            "crevices" => g.crevices(),
            "bevels" => g.bevels(),
            "course" => {
                g.flat();
                g.course();
            }
            _ => return Err(format!("no world {name} (one of {})", NAMES.join(", "))),
        }
        Ok(g)
    }

    /// A floor 4096 units across, its top at 0.
    fn flat(&mut self) {
        self.solid([-2048.0, -2048.0, -64.0], [2048.0, 2048.0, 0.0]);
        let z = stand_z(0.0);
        self.spawn([-1024.0, -1024.0, z], [1024.0, 1024.0, z]);
        self.spawn([-1024.0, -1024.0, z + 1.0], [1024.0, 1024.0, z + 200.0]);
    }

    /// Ramps along +x, one a lane 192 units apart in y, at rises over a
    /// run of 256: gentle to the walkable limit (both sides of it) to near
    /// a wall, each with a plateau at its top and a ramp down after it.
    fn ramps(&mut self) {
        self.flat_floor();
        // The walkable limit: the rise over the run whose normal's z is
        // WALKABLE_NORMAL_Z.
        let limit = (1.0 / (WALKABLE_NORMAL_Z * WALKABLE_NORMAL_Z) - 1.0).sqrt();
        let slopes = [
            0.1,
            0.27,
            0.58,
            0.9,
            limit * 0.995,
            limit,
            limit * 1.005,
            1.2,
            1.73,
            3.7,
        ];
        let run = 256.0;
        for (lane, slope) in slopes.iter().enumerate() {
            let y = -960.0 + lane as f32 * 192.0;
            let rise = slope * run;
            let (y0, y1) = (y, y + 128.0);
            self.ramp([0.0, y0, 0.0], [run, y1, rise], 0, false);
            self.solid([run, y0, 0.0], [run + 128.0, y1, rise]);
            self.ramp(
                [run + 128.0, y0, 0.0],
                [2.0 * run + 128.0, y1, rise],
                0,
                true,
            );
            let z = stand_z(0.0);
            self.spawn([-160.0, y0 + 16.0, z], [-32.0, y1 - 16.0, z]);
            let top = stand_z(rise);
            self.spawn([run + 16.0, y0 + 16.0, top], [run + 112.0, y1 - 16.0, top]);
            // Over the slope, a little above it.
            self.spawn(
                [32.0, y0 + 16.0, stand_z(rise) + 8.0],
                [run - 32.0, y1 - 16.0, stand_z(rise) + 64.0],
            );
        }
    }

    fn flat_floor(&mut self) {
        self.solid([-2048.0, -2048.0, -64.0], [2048.0, 2048.0, 0.0]);
    }

    /// Flights of stairs along +x, a lane each, by rise and tread: the
    /// step limit and either side of it, low kerbs and too-high ledges,
    /// narrow and wide treads.
    fn stairs(&mut self) {
        self.flat_floor();
        let flights: [(f32, f32); 10] = [
            (4.0, 16.0),
            (8.0, 16.0),
            (12.0, 8.0),
            (16.0, 24.0),
            (17.9, 16.0),
            (18.0, 16.0),
            (18.1, 16.0),
            (20.0, 32.0),
            (24.0, 16.0),
            (9.0, 3.0),
        ];
        for (lane, (rise, tread)) in flights.iter().enumerate() {
            let y = -960.0 + lane as f32 * 192.0;
            for step in 0..8 {
                let x0 = step as f32 * tread;
                let top = (step + 1) as f32 * rise;
                self.solid([x0, y, 0.0], [x0 + tread + 0.0, y + 128.0, top]);
            }
            let end = 8.0 * tread;
            self.solid([end, y, 0.0], [end + 96.0, y + 128.0, 8.0 * rise]);
            let z = stand_z(0.0);
            self.spawn([-128.0, y + 16.0, z], [-24.0, y + 112.0, z]);
            let top = stand_z(8.0 * rise);
            self.spawn([end + 16.0, y + 16.0, top], [end + 80.0, y + 112.0, top]);
        }
    }

    /// Platforms over the floor, by height, to walk or run off.
    fn ledges(&mut self) {
        self.flat_floor();
        let heights = [
            8.0, 18.0, 19.0, 32.0, 48.0, 64.0, 128.0, 256.0, 512.0, 1024.0,
        ];
        for (lane, height) in heights.iter().enumerate() {
            let y = -960.0 + lane as f32 * 192.0;
            self.solid([-256.0, y, 0.0], [0.0, y + 160.0, *height]);
            let top = stand_z(*height);
            self.spawn([-200.0, y + 16.0, top], [-16.0, y + 144.0, top]);
        }
        let z = stand_z(0.0);
        self.spawn([32.0, -960.0, z], [400.0, 960.0, z]);
    }

    /// Tunnels along +x, by how high their roof is over the floor: under
    /// a crouch, at it, between it and standing, and above.
    fn ceilings(&mut self) {
        self.flat_floor();
        let crouch = crate::motion::CROUCH_HEIGHT - SOLE_Z;
        let stand = crate::motion::STAND_HEIGHT - SOLE_Z;
        let roofs = [
            crouch - 2.0,
            crouch,
            crouch + 0.5,
            crouch + 8.0,
            stand - 1.0,
            stand,
            stand + 0.2,
            stand + 1.0,
            stand + 12.0,
            stand + 40.0,
        ];
        for (lane, roof) in roofs.iter().enumerate() {
            let y = -960.0 + lane as f32 * 192.0;
            // Walls either side 64 apart, a roof over them.
            self.solid([0.0, y, 0.0], [512.0, y + 16.0, roof + 32.0]);
            self.solid([0.0, y + 80.0, 0.0], [512.0, y + 96.0, roof + 32.0]);
            self.solid([0.0, y, *roof], [512.0, y + 96.0, roof + 32.0]);
            let z = stand_z(0.0);
            self.spawn([-96.0, y + 32.0, z], [-24.0, y + 64.0, z]);
            self.spawn([64.0, y + 32.0, z], [448.0, y + 64.0, z]);
        }
    }

    /// Corners where two to five planes meet, at many angles: walls
    /// rotated about a point, pockets, and a corner with a ramp in it.
    fn wedges(&mut self) {
        self.flat_floor();
        let angles = [15.0f32, 30.0, 45.0, 60.0, 90.0, 120.0, 150.0, 170.0];
        for (k, angle) in angles.iter().enumerate() {
            let cx = -768.0 + (k % 4) as f32 * 512.0;
            let cy = if k < 4 { -512.0 } else { 512.0 };
            // One wall along -x from the corner, the other turned by the
            // angle: a wedge opening toward +x.
            self.wall([cx, cy], 180.0, 192.0, 160.0);
            self.wall([cx, cy], 180.0 + angle, 192.0, 160.0);
            let (s, c) = crate::math::sin_cos_deg(180.0 + angle / 2.0);
            let z = stand_z(0.0);
            let at = [cx + c * 96.0, cy + s * 96.0];
            self.spawn(
                [at[0] - 24.0, at[1] - 24.0, z],
                [at[0] + 24.0, at[1] + 24.0, z],
            );
        }
        // A pocket of three walls and a low roof (five planes at its back).
        self.solid([0.0, -40.0, 0.0], [16.0, 40.0, 96.0]);
        self.solid([-128.0, -56.0, 0.0], [16.0, -40.0, 96.0]);
        self.solid([-128.0, 40.0, 0.0], [16.0, 56.0, 96.0]);
        self.solid([-128.0, -56.0, 70.0], [16.0, 56.0, 96.0]);
        self.ramp([-64.0, -40.0, 0.0], [0.0, 40.0, 24.0], 0, false);
        let z = stand_z(0.0);
        self.spawn([-112.0, -16.0, z], [-80.0, 16.0, z]);
    }

    /// A wall `length` long and 16 thick from `from` (x, y) heading `yaw`
    /// degrees, `height` tall.
    fn wall(&mut self, from: [f32; 2], yaw: f32, length: f32, height: f32) {
        let (s, c) = crate::math::sin_cos_deg(yaw);
        let (dx, dy) = (c * length, s * length);
        // Its left side, 16 units across.
        let (nx, ny) = (-s * 16.0, c * 16.0);
        let a = from;
        let b = [from[0] + dx, from[1] + dy];
        let d = [from[0] + nx, from[1] + ny];
        let e = [b[0] + nx, b[1] + ny];
        // Counter-clockwise from above: a, b, e, d.
        self.prism(&[a, b, e, d], 0.0, height);
    }

    /// Walls at many slants in the open, to slide along at every angle of
    /// incidence.
    fn walls(&mut self) {
        self.flat_floor();
        for k in 0..12 {
            let yaw = k as f32 * 7.5 + 3.0;
            let x = -900.0 + (k % 6) as f32 * 320.0;
            let y = if k < 6 { -400.0 } else { 400.0 };
            self.wall([x, y], yaw, 256.0, 128.0);
            let z = stand_z(0.0);
            self.spawn([x - 96.0, y - 160.0, z], [x + 96.0, y - 32.0, z]);
        }
    }

    /// Crates, a table top KOTOR's walker never stands on, invisible walls
    /// of both kinds, and bodies (actors' boxes) standing about and lying
    /// low to walk into and stand on.
    fn boxes(&mut self) {
        self.flat_floor();
        self.solid([64.0, -32.0, 0.0], [128.0, 32.0, 16.0]);
        self.solid([64.0, 64.0, 0.0], [128.0, 128.0, 40.0]);
        self.add_box(
            [160.0, -32.0, 0.0],
            [224.0, 32.0, 12.0],
            Contents(Contents::SOLID.0 | Contents::NONWALK.0),
        );
        self.add_box(
            [160.0, 64.0, 0.0],
            [224.0, 128.0, 30.0],
            Contents(Contents::SOLID.0 | Contents::NONWALK.0),
        );
        self.add_box(
            [-128.0, -256.0, 0.0],
            [-112.0, 256.0, 256.0],
            Contents::PLAYER_CLIP,
        );
        self.add_box(
            [-256.0, -256.0, 0.0],
            [-240.0, 256.0, 256.0],
            Contents::WALK_CLIP,
        );
        let half = crate::motion::HALF_WIDTH;
        let stand = crate::motion::STAND_HEIGHT - SOLE_Z;
        for (k, (x, y, top)) in [
            (0.0, 160.0, stand),
            (40.0, 200.0, stand),
            (-64.0, 192.0, 16.0),
            (256.0, 0.0, stand),
        ]
        .iter()
        .enumerate()
        {
            self.body(
                (k as u32 + 1) | crate::BODY,
                [x - half, y - half, 0.0],
                [x + half, y + half, *top],
            );
        }
        let z = stand_z(0.0);
        self.spawn([-64.0, -128.0, z], [320.0, 320.0, z]);
        self.spawn(
            [64.0, -32.0, stand_z(16.0) + 1.0],
            [128.0, 32.0, stand_z(16.0) + 40.0],
        );
    }

    /// V-shaped clefts of two steep slopes meeting below, too steep to
    /// stand on and too narrow to slide down; and a pit.
    fn crevices(&mut self) {
        self.flat_floor();
        for (lane, (width, depth)) in [(32.0, 64.0), (48.0, 96.0), (96.0, 96.0), (24.0, 128.0)]
            .iter()
            .enumerate()
        {
            let y = -600.0 + lane as f32 * 320.0;
            self.ramp([-width, y, 0.0], [0.0, y + 256.0, *depth], 0, true);
            self.ramp([0.0, y, 0.0], [*width, y + 256.0, *depth], 0, false);
            self.spawn(
                [-width * 0.5, y + 32.0, stand_z(*depth * 0.5)],
                [width * 0.5, y + 224.0, stand_z(*depth) + 32.0],
            );
        }
        // A pit 96 deep, 64 across.
        self.solid([400.0, -400.0, 0.0], [432.0, 400.0, 96.0]);
        self.solid([496.0, -400.0, 0.0], [528.0, 400.0, 96.0]);
        self.spawn(
            [440.0, -300.0, stand_z(0.0) + 4.0],
            [488.0, 300.0, stand_z(96.0)],
        );
    }

    /// Raised floors a few units up behind a steep bevel (too steep a
    /// climb to step, low enough to stride onto), by height and bevel, and
    /// with what may refuse the stride: a wall just behind, a top KOTOR's
    /// walker never stands on, a body's top, a sloping top.
    fn bevels(&mut self) {
        self.flat_floor();
        let lanes: [(f32, f32); 10] = [
            (8.0, 5.8),
            (8.0, 4.0),
            (10.0, 7.0),
            (11.0, 7.0),
            (8.0, 5.8),
            (8.0, 5.8),
            (8.0, 5.8),
            (6.0, 3.0),
            (1.5, 1.0),
            (8.0, 5.8),
        ];
        for (lane, (height, run)) in lanes.iter().enumerate() {
            let y = -960.0 + lane as f32 * 192.0;
            let (y0, y1) = (y, y + 128.0);
            self.ramp([0.0, y0, 0.0], [*run, y1, *height], 0, false);
            match lane {
                4 => {
                    self.solid([*run, y0, 0.0], [400.0, y1, *height]);
                    self.solid([run + 3.0, y0, 0.0], [run + 24.0, y1, 96.0]);
                }
                // A plain strip, then a top the walker never stands on:
                // the box lands on the strip, its middle over the rest.
                5 => {
                    self.solid([*run, y0, 0.0], [run + 12.0, y1, *height]);
                    self.add_box(
                        [run + 12.0, y0, 0.0],
                        [400.0, y1, *height],
                        Contents(Contents::SOLID.0 | Contents::NONWALK.0),
                    );
                }
                // A body's top a unit over the bevel's.
                6 => self.body(
                    (lane as u32) | crate::BODY,
                    [*run, y0, 0.0],
                    [400.0, y1, height + 1.0],
                ),
                9 => self.ramp([*run, y0, 0.0], [400.0, y1, height + 30.0], 0, false),
                _ => self.solid([*run, y0, 0.0], [400.0, y1, *height]),
            }
            let z = stand_z(0.0);
            self.spawn([-96.0, y0 + 16.0, z], [-24.0, y1 - 16.0, z]);
        }
    }

    /// A mixed course on the flat: a ramp, stairs, a ledge, a tunnel, a
    /// corner and a crate, close enough to run between.
    fn course(&mut self) {
        self.ramp([200.0, -64.0, 0.0], [456.0, 64.0, 128.0], 0, false);
        self.solid([456.0, -64.0, 0.0], [600.0, 64.0, 128.0]);
        for step in 0..6 {
            let x = 200.0 + step as f32 * 20.0;
            self.solid([x, 128.0, 0.0], [x + 20.0, 256.0, (step + 1) as f32 * 16.0]);
        }
        self.solid([-300.0, -64.0, 48.0], [-200.0, 64.0, 80.0]);
        self.solid([-300.0, -80.0, 0.0], [-200.0, -64.0, 80.0]);
        self.solid([-300.0, 64.0, 0.0], [-200.0, 80.0, 80.0]);
        self.wall([0.0, -300.0], 200.0, 160.0, 96.0);
        self.wall([0.0, -300.0], 250.0, 160.0, 96.0);
        self.solid([-64.0, 300.0, 0.0], [64.0, 364.0, 32.0]);
    }

    /// A scenario's own shape (`box`, `ramp`, `prism`, `body`).
    pub fn add(&mut self, line: &Line) -> Result<(), String> {
        let a = line.args();
        let n = |i: usize| -> Result<f32, String> {
            a.get(i)
                .ok_or_else(|| "too few numbers".to_string())
                .and_then(|w| float(w))
        };
        let v = |i: usize| -> Result<[f32; 3], String> { Ok([n(i)?, n(i + 1)?, n(i + 2)?]) };
        match line.verb() {
            "box" => {
                let contents = match a.get(6).map(String::as_str) {
                    None | Some("solid") => Contents::SOLID,
                    Some("nonwalk") => Contents(Contents::SOLID.0 | Contents::NONWALK.0),
                    Some("clip") => Contents::PLAYER_CLIP,
                    Some("walkclip") => Contents::WALK_CLIP,
                    Some(other) => return Err(format!("no contents {other}")),
                };
                self.add_box(v(0)?, v(3)?, contents);
            }
            "ramp" => {
                let axis = match a.get(6).map(String::as_str) {
                    Some("x") => 0,
                    Some("y") => 1,
                    _ => return Err("ramp: axis x or y".to_string()),
                };
                let down = a.get(7).is_some_and(|w| w == "down");
                self.ramp(v(0)?, v(3)?, axis, down);
            }
            "prism" => {
                let (low, high) = (n(0)?, n(1)?);
                let mut points = Vec::new();
                let mut i = 2;
                while i + 1 < a.len() {
                    points.push([n(i)?, n(i + 1)?]);
                    i += 2;
                }
                if points.len() < 3 {
                    return Err("prism: three points at least".to_string());
                }
                self.prism(&points, low, high);
            }
            "body" => {
                let entity: u32 = int(a.first().ok_or("body: entity")?)?;
                self.body(entity, v(1)?, v(4)?);
            }
            "spawn" => self.spawn(v(0)?, v(3)?),
            _ => return Err(format!("not a shape: {}", line.place())),
        }
        Ok(())
    }
}

/// The library's worlds.
pub const NAMES: &[&str] = &[
    "void", "flat", "ramps", "stairs", "ledges", "ceilings", "wedges", "walls", "boxes",
    "crevices", "bevels", "course",
];
