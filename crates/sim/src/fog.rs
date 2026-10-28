//! Fog of war (`docs/UI.md`): what the party has
//! explored of each area's map, kept as KOTOR 1's saves keep it (the GIT's
//! `AreaMap`: `AreaMapResX` by `AreaMapResY` cells over the picture, a bit
//! each, row by row from the top left, a byte's first its lowest). A cell
//! is explored once a party member comes within `REVEAL_M` of it, and by
//! `ExploreAreaForPlayer` and `RevealMap`. The grids are the party's, the
//! same for every seat, kept per module across module changes: in the
//! snapshot, so in saves.

use crate::wire::{DecodeError, Reader, Writer};

/// How near (metres) a party member comes to a cell to explore it.
pub const REVEAL_M: f32 = rules::world::map::REVEAL_M;

/// The grid's columns where the ARE gives no `MapResX`, and the most it may
/// give.
const COLUMNS: u8 = 32;
const MOST_COLUMNS: u8 = 64;

/// One of the picture's axes: the fraction along it is world axis `world`
/// (0 X, 1 Y; metres) times `scale`, plus `offset`
/// (`asset_odyssey::map::MapAxis`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Axis {
    pub world: u8,
    pub scale: f32,
    pub offset: f32,
}

/// An area's map as the sim keeps it: where a world point falls on its
/// picture (fractions across the part of it the map uses, and down), and
/// the grid explored cells are counted in.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Chart {
    /// Across, then down.
    pub axes: [Axis; 2],
    /// Columns and rows.
    pub cells: [u8; 2],
}

impl Chart {
    /// The chart of `axes` with a grid `res_x` (the ARE's `MapResX`) cells
    /// across, 32 where it gives none, and as many rows as fit cells as tall
    /// as wide on KOTOR 1's 440 by 256 picture: the Endar Spire's 15 by 8,
    /// as its saves keep it [verify: KOTOR 2's].
    pub fn new(axes: [Axis; 2], res_x: i32) -> Chart {
        let columns = u8::try_from(res_x)
            .ok()
            .filter(|columns| *columns > 0)
            .unwrap_or(COLUMNS)
            .min(MOST_COLUMNS);
        let rows = (u32::from(columns) * 256 / 440).max(1) as u8;
        Chart {
            axes,
            cells: [columns, rows],
        }
    }

    /// Where world point `at` (metres) falls on the picture.
    pub fn point(&self, at: [f32; 2]) -> [f32; 2] {
        self.axes
            .map(|axis| at[usize::from(axis.world)] * axis.scale + axis.offset)
    }

    /// The cell holding picture point `point`, if one does.
    fn cell(&self, point: [f32; 2]) -> Option<[usize; 2]> {
        let mut cell = [0; 2];
        for axis in 0..2 {
            let at = (point[axis] * f32::from(self.cells[axis])).floor();
            if !(0.0..f32::from(self.cells[axis])).contains(&at) {
                return None;
            }
            cell[axis] = at as usize;
        }
        Some(cell)
    }

    /// The cells whose nearest point is within `reach` metres of world
    /// point `at`.
    fn near(&self, at: [f32; 2], reach: f32) -> Vec<[usize; 2]> {
        let point = self.point(at);
        // On each axis: the cells the reach spans, and how far (metres)
        // each lies from the point.
        let spans = [0, 1].map(|axis| {
            let (scale, count) = (self.axes[axis].scale.abs(), f32::from(self.cells[axis]));
            if scale == 0.0 || !scale.is_finite() {
                return Vec::new();
            }
            let (from, to) = (point[axis] - reach * scale, point[axis] + reach * scale);
            let first = (from * count).floor().max(0.0);
            let last = (to * count).floor().min(count - 1.0);
            if first > last {
                return Vec::new();
            }
            (first as usize..=last as usize)
                .map(|cell| {
                    let (low, high) = (cell as f32 / count, (cell + 1) as f32 / count);
                    (
                        cell,
                        (point[axis] - point[axis].clamp(low, high)).abs() / scale,
                    )
                })
                .collect()
        });
        let mut near = Vec::new();
        for &(row, down) in &spans[1] {
            for &(column, across) in &spans[0] {
                if across * across + down * down <= reach * reach {
                    near.push([column, row]);
                }
            }
        }
        near
    }

    pub(crate) fn digest_into(&self, out: &mut Writer) {
        for axis in &self.axes {
            out.u8(axis.world);
            out.f32(axis.scale);
            out.f32(axis.offset);
        }
        out.u8(self.cells[0]);
        out.u8(self.cells[1]);
    }
}

/// What the party has explored of one module's area.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Explored {
    pub module: String,
    /// Columns and rows.
    pub cells: [u8; 2],
    /// A bit a cell, row by row (`fog`).
    bits: Vec<u8>,
}

impl Explored {
    fn new(module: &str, cells: [u8; 2]) -> Explored {
        let count = usize::from(cells[0]) * usize::from(cells[1]);
        Explored {
            module: module.to_string(),
            cells,
            bits: vec![0; count.div_ceil(8)],
        }
    }

    fn at(&self, [column, row]: [usize; 2]) -> usize {
        row * usize::from(self.cells[0]) + column
    }

    /// Whether the cell `column` across and `row` down is explored.
    pub fn explored(&self, column: usize, row: usize) -> bool {
        let at = self.at([column, row]);
        self.bits
            .get(at / 8)
            .is_some_and(|byte| byte & 1 << (at % 8) != 0)
    }

    /// Every cell explored (`ExploreAreaForPlayer`'s check).
    pub fn whole(&self) -> bool {
        let [columns, rows] = self.cells.map(usize::from);
        (0..rows).all(|row| (0..columns).all(|column| self.explored(column, row)))
    }

    fn explore(&mut self, cell: [usize; 2]) {
        let at = self.at(cell);
        if let Some(byte) = self.bits.get_mut(at / 8) {
            *byte |= 1 << (at % 8);
        }
    }
}

/// How a script explores the area (`RevealMap`, `ExploreAreaForPlayer`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Reveal {
    All,
    /// The cells within `radius` cells of the one world point `at` (metres)
    /// falls in, itself among them.
    Around {
        at: [f32; 2],
        radius: u32,
    },
}

/// `module`'s grid in `grids` (by module), if the party explored any of it.
pub fn of<'g>(grids: &'g [Explored], module: &str) -> Option<&'g Explored> {
    grids
        .binary_search_by(|grid| grid.module.as_str().cmp(module))
        .ok()
        .map(|at| &grids[at])
}

/// `module`'s grid in `grids` (by module), made unexplored where there is
/// none or the chart's grid is another size.
fn grid<'g>(grids: &'g mut Vec<Explored>, module: &str, chart: &Chart) -> &'g mut Explored {
    let at = match grids.binary_search_by(|grid| grid.module.as_str().cmp(module)) {
        Ok(at) => {
            if grids[at].cells != chart.cells {
                grids[at] = Explored::new(module, chart.cells);
            }
            at
        }
        Err(at) => {
            grids.insert(at, Explored::new(module, chart.cells));
            at
        }
    };
    &mut grids[at]
}

/// A party member at world point `at` (metres) in `module` explores the
/// cells within `REVEAL_M` of it.
pub(crate) fn explore(grids: &mut Vec<Explored>, module: &str, chart: &Chart, at: [f32; 2]) {
    let near = chart.near(at, REVEAL_M);
    if near.is_empty() {
        return;
    }
    let grid = grid(grids, module, chart);
    for cell in near {
        grid.explore(cell);
    }
}

/// A script's `how` explores `module`'s cells.
pub(crate) fn reveal(grids: &mut Vec<Explored>, module: &str, chart: &Chart, how: Reveal) {
    let [columns, rows] = chart.cells.map(usize::from);
    let cells: Vec<[usize; 2]> = match how {
        Reveal::All => (0..rows)
            .flat_map(|row| (0..columns).map(move |column| [column, row]))
            .collect(),
        Reveal::Around { at, radius } => {
            let Some([column, row]) = chart.cell(chart.point(at)) else {
                return;
            };
            let radius = radius as usize;
            let span = |middle: usize, count: usize| {
                middle.saturating_sub(radius)..=(middle + radius).min(count - 1)
            };
            span(row, rows)
                .flat_map(|y| span(column, columns).map(move |x| [x, y]))
                .filter(|&[x, y]| {
                    x.abs_diff(column).pow(2) + y.abs_diff(row).pow(2) <= radius * radius
                })
                .collect()
        }
    };
    let grid = grid(grids, module, chart);
    for cell in cells {
        grid.explore(cell);
    }
}

pub(crate) fn encode(grids: &[Explored], out: &mut Writer) {
    out.u16(u16::try_from(grids.len()).expect("fewer areas than 65536"));
    for grid in grids {
        out.string(&grid.module);
        out.u8(grid.cells[0]);
        out.u8(grid.cells[1]);
        out.bytes(&grid.bits);
    }
}

/// The grids `encode` wrote: by module, each once, every bit past its
/// cells clear (one state, one encoding).
pub(crate) fn decode(input: &mut Reader) -> Result<Vec<Explored>, DecodeError> {
    let mut grids: Vec<Explored> = Vec::new();
    for _ in 0..input.u16()? {
        let at = input.at();
        let module = input.string()?;
        let cells = [input.u8()?, input.u8()?];
        if cells.contains(&0) || grids.last().is_some_and(|last| last.module >= module) {
            return Err(input.invalid("explored area", at));
        }
        let mut grid = Explored::new(&module, cells);
        let length = grid.bits.len();
        grid.bits = input.take(length)?.to_vec();
        let count = usize::from(cells[0]) * usize::from(cells[1]);
        if count % 8 != 0 && grid.bits[length - 1] >> (count % 8) != 0 {
            return Err(input.invalid("explored cells", at));
        }
        grids.push(grid);
    }
    Ok(grids)
}
