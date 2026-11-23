//! The light a level's light grid (`fmt_ja::bsp::LightGrid`) gives a model
//! placed in the level, as Jedi Academy's renderer works it out for an
//! entity.
//!
//! The point is in one cell of the grid, and takes its light from the
//! eight grid points round it, each weighed by how near it is. A point
//! inside a wall gives nothing, and the others' weights then make up for
//! it. The ambient light is taken at [`AMBIENT_SHARE`], and
//! [`AMBIENT_FLOOR`] is added to it. The directed light comes from the
//! weighed mean of the points' directions.
//!
//! Two things follow the renderer as it is. A point past the grid's last
//! cell along one axis is the cell that follows in the grid's list, not
//! left out. And the place in the cell is not held to the grid's box, so
//! outside the box the nearest face's points are weighed as if the box
//! went on.
//!
//! Only the level's steady light is read: the first of each point's four
//! light styles.

use fmt_ja::bsp::{LightGrid, STYLE_NONE};

/// The share of the grid's ambient light a model takes (the renderer's
/// `r_ambientScale`).
pub const AMBIENT_SHARE: f32 = 0.5;
/// The ambient light, of 255, that every model has beside the grid's.
pub const AMBIENT_FLOOR: f32 = 32.0;
/// The ambient light, of 255, of a model in a level without a light grid,
/// before the floor is added.
pub const AMBIENT_WITHOUT_GRID: f32 = 150.0;

/// The light on a model: colours of 0 to 255.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ModelLight {
    pub ambient: [f32; 3],
    pub direct: [f32; 3],
    /// Toward where the directed light comes from; zero where there is
    /// none.
    pub from: [f32; 3],
}

impl ModelLight {
    /// The colour of a surface that faces along `normal`: the ambient
    /// light, and the directed light by how much the surface faces it.
    /// `normal` need not be of unit length. The renderer scales a model's
    /// axes by the model's scale, so a scaled model takes more or less of
    /// its directed light.
    pub fn shade(&self, normal: [f32; 3]) -> [u8; 3] {
        let facing: f32 = (0..3).map(|axis| normal[axis] * self.from[axis]).sum();
        std::array::from_fn(|channel| {
            let lit = self.ambient[channel] + facing.max(0.0) * self.direct[channel];
            lit.clamp(0.0, 255.0) as u8
        })
    }
}

/// The light at `at` (units).
pub fn light_at(grid: &LightGrid, at: [f32; 3]) -> ModelLight {
    if grid.cells.is_empty() {
        return ModelLight {
            ambient: [AMBIENT_WITHOUT_GRID + AMBIENT_FLOOR; 3],
            direct: [0.0; 3],
            from: [0.0; 3],
        };
    }
    let mut cell = [0usize; 3];
    let mut within = [0.0f32; 3];
    for axis in 0..3 {
        let along = (at[axis] - grid.origin[axis]) / grid.size[axis];
        within[axis] = along - along.floor();
        cell[axis] = (along.floor().max(0.0) as usize).min(grid.bounds[axis] - 1);
    }
    let steps = [1, grid.bounds[0], grid.bounds[0] * grid.bounds[1]];
    let first = cell[0] + steps[1] * cell[1] + steps[2] * cell[2];

    let (mut ambient, mut direct, mut from) = ([0.0f32; 3], [0.0f32; 3], [0.0f32; 3]);
    let mut weighed = 0.0;
    for corner in 0..8 {
        let mut weight = 1.0;
        let mut index = first;
        for axis in 0..3 {
            if corner & (1 << axis) != 0 {
                weight *= within[axis];
                index += steps[axis];
            } else {
                weight *= 1.0 - within[axis];
            }
        }
        let Some(point) = (grid.cells.get(index)).map(|&point| &grid.points[point as usize]) else {
            continue;
        };
        if point.styles[0] == STYLE_NONE {
            continue;
        }
        weighed += weight;
        for channel in 0..3 {
            ambient[channel] += weight * f32::from(point.ambient[0][channel]);
            direct[channel] += weight * f32::from(point.direct[0][channel]);
        }
        let turn = |byte: u8| f32::from(byte) * (std::f32::consts::TAU / 256.0);
        let (from_up, about_up) = (turn(point.from[0]), turn(point.from[1]));
        let toward = [
            about_up.cos() * from_up.sin(),
            about_up.sin() * from_up.sin(),
            from_up.cos(),
        ];
        for axis in 0..3 {
            from[axis] += weight * toward[axis];
        }
    }
    if weighed > 0.0 && weighed < 0.99 {
        ambient = ambient.map(|light| light / weighed);
        direct = direct.map(|light| light / weighed);
    }
    let length = from.iter().map(|part| part * part).sum::<f32>().sqrt();
    ModelLight {
        ambient: ambient.map(|light| (light * AMBIENT_SHARE + AMBIENT_FLOOR).min(255.0)),
        direct,
        from: if length > 0.0 {
            from.map(|part| part / length)
        } else {
            [0.0; 3]
        },
    }
}
