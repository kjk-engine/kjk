//! The area's wind, as the executables' wind manager blows it (TSL's
//! `WindManager`; K1 has the same): what sways the danglymeshes (`dangly`),
//! the areas' tree leaves above all.
//!
//! The ARE's `WindPower` (0, 1 or 2) sets a steady wind along +X+Y of that
//! many metres a second, and how far a gust may turn it: up to 100° (150°)
//! about the up axis and 3° (5°) about the X axis. The gusts come from two
//! 16 × 16 noise fields: each cell the sum of five layers of blocks (16, 8,
//! 4, 2 and 1 cells wide), each block one draw of the C library's `rand`,
//! `rand % (500 / (16 / width)) / 1000`, so a cell is 0 to 0.963. The wind
//! blends from the first field to the second over 2 s, then the second
//! becomes the first and a new second is drawn.
//!
//! At a point, the cell `|4 × trunc(x)| mod 16`, `|4 × trunc(y)| mod 16`
//! (game space, metres) gives the blended gust `g`: the wind is the steady
//! one × `g` × the frame's time, turned by `(2g − 1) × ½` of the up-axis
//! limit (as a half angle) and the same of the X limit from the cell with
//! x and y swapped. A still area (`WindPower` 0) has none.
//!
//! Not done: the point sources the executables add (`AddPointSourceWind`).
//! The fields are seeded with 1 (the C library's own start) [verify: the
//! executables' `rand` has run on by then].

use asset_odyssey::msvc_rand::MsvcRand;
use bevy::prelude::*;

use crate::scene::SceneContent;

/// A field's side, in cells.
const CELLS: usize = 16;
/// The widest layer's block draws below this; each narrower layer below
/// this ÷ (16 ÷ its width).
const DRAW_RANGE: u32 = 500;
/// A draw over this is a cell's share.
const DRAW_SCALE: f32 = 1000.0;
/// The cells per metre a point's whole metres step.
const CELLS_PER_M: i32 = 4;
/// The seconds one field takes to blend into the next.
const BLEND_S: f32 = 2.0;
/// Per `WindPower` (0, 1, 2): the steady wind (m/s), and the most a gust
/// turns it about the up axis and about X (degrees).
const STRENGTH: [f32; 3] = [0.0, 1.0, 2.0];
const TURN_UP_DEG: [f32; 3] = [0.0, 100.0, 150.0];
const TURN_X_DEG: [f32; 3] = [0.0, 3.0, 5.0];

type Field = [[f32; CELLS]; CELLS];

pub struct WindPlugin;

impl Plugin for WindPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Wind>().add_systems(
            Update,
            blow_in.run_if(resource_exists_and_changed::<SceneContent>),
        );
    }
}

/// The scene's wind: its steady part and its two noise fields.
#[derive(Resource)]
pub struct Wind {
    /// Game space, metres a second.
    steady: Vec3,
    /// The most a gust turns it about the up axis and X (radians).
    turn: [f32; 2],
    fields: [Field; 2],
    /// Seconds into the blend from the first field to the second.
    blended: f32,
    rand: MsvcRand,
}

impl Default for Wind {
    fn default() -> Wind {
        Wind::of_power(0)
    }
}

impl Wind {
    /// The wind an ARE's `WindPower` blows (0 to 2; anything else is 0).
    pub fn of_power(power: i32) -> Wind {
        let power = usize::try_from(power)
            .ok()
            .filter(|&p| p < STRENGTH.len())
            .unwrap_or(0);
        let mut rand = MsvcRand(1);
        let fields = [noise(&mut rand), noise(&mut rand)];
        Wind {
            steady: Vec2::ONE.normalize().extend(0.0) * STRENGTH[power],
            turn: [
                TURN_UP_DEG[power].to_radians(),
                TURN_X_DEG[power].to_radians(),
            ],
            fields,
            blended: 0.0,
            rand,
        }
    }

    pub fn still(&self) -> bool {
        self.steady == Vec3::ZERO
    }

    /// The clock on by `dt` seconds: past the blend, the second field
    /// becomes the first and a new second is drawn.
    pub fn tick(&mut self, dt: f32) {
        self.blended += dt;
        if self.blended > BLEND_S {
            self.fields.swap(0, 1);
            self.fields[1] = noise(&mut self.rand);
            self.blended = 0.0;
        }
    }

    /// The wind at `at` (game space) over a frame of `dt` seconds (game
    /// space): how far it pushes.
    pub fn at(&self, at: Vec3, dt: f32) -> Vec3 {
        if self.still() {
            return Vec3::ZERO;
        }
        let cell = |metres: f32| {
            ((metres as i32).wrapping_mul(CELLS_PER_M).unsigned_abs() as usize) % CELLS
        };
        let (x, y) = (cell(at.x), cell(at.y));
        let share = self.blended / BLEND_S;
        let gust = |row: usize, column: usize| {
            (1.0 - share) * self.fields[0][row][column] + share * self.fields[1][row][column]
        };
        let (up, across) = (gust(x, y), gust(y, x));
        // Half angles: the turn is twice each.
        let half = |gust: f32, most: f32| (2.0 * gust * most - most) * 0.5;
        let (a, b) = (half(up, self.turn[0]), half(across, self.turn[1]));
        let turn = Quat::from_xyzw(
            a.cos() * b.sin(),
            a.sin() * b.sin(),
            a.sin() * b.cos(),
            a.cos() * b.cos(),
        );
        turn * (self.steady * up * dt)
    }
}

/// A new noise field: five layers of blocks, the widest first, each block
/// one draw, top rows first and left to right.
fn noise(rand: &mut MsvcRand) -> Field {
    let mut field = [[0.0; CELLS]; CELLS];
    let mut width = CELLS;
    while width >= 1 {
        let range = DRAW_RANGE / (CELLS / width) as u32;
        for row in (0..CELLS).step_by(width) {
            for column in (0..CELLS).step_by(width) {
                let draw = (rand.next() % range) as f32 / DRAW_SCALE;
                for cells in &mut field[row..row + width] {
                    for cell in &mut cells[column..column + width] {
                        *cell += draw;
                    }
                }
            }
        }
        width /= 2;
    }
    field
}

/// A new scene's wind, from its ARE.
fn blow_in(scene: Res<SceneContent>, mut wind: ResMut<Wind>) {
    *wind = Wind::of_power(scene.0.weather.area.wind);
}

/// The wind without an app: none in a still area, a steady one along +X+Y
/// turned by at most its limits, and cells within 0 to 0.963.
#[cfg(feature = "probes")]
pub fn probes() -> crate::probe_kit::Checks {
    use crate::probe_kit::check;
    let still = Wind::of_power(0);
    let calm = check(
        still.at(Vec3::new(3.0, 7.0, 0.0), 1.0) == Vec3::ZERO,
        "power 0".into(),
    );

    let mut wind = Wind::of_power(1);
    // Every layer's largest draw at once: 0.963.
    let most_cell = [1, 2, 4, 8, 16]
        .iter()
        .map(|parts| (DRAW_RANGE / parts - 1) as f32 / DRAW_SCALE)
        .sum::<f32>();
    let cells_in = wind
        .fields
        .iter()
        .flatten()
        .flatten()
        .all(|&cell| (0.0..=most_cell).contains(&cell));
    let limit = (TURN_UP_DEG[1] + TURN_X_DEG[1]).to_radians() + 1e-3;
    let mut worst = 0.0f32;
    for step in 0..200 {
        wind.tick(0.05);
        let push = wind.at(Vec3::new(step as f32 * 0.37, step as f32 * 0.91, 0.0), 1.0);
        if push != Vec3::ZERO {
            worst = worst.max(push.angle_between(wind.steady));
        }
    }
    let turned = check(
        cells_in && worst <= limit,
        format!(
            "cells within 0..{most_cell}: {cells_in}, the most it turned {:.1}°",
            worst.to_degrees()
        ),
    );
    vec![
        ("wind_still_area", calm),
        ("wind_turns_within_limits", turned),
    ]
}
