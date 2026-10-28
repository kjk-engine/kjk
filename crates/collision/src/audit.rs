//! The containment audit: where a player can get that the level's
//! walkmesh never meant them to.
//!
//! KOTOR's characters never leave the walkmesh; a JA player jumps. The
//! audit finds every place a player's box can stand ("spots"), starts from
//! where the game puts players (the module's entry and waypoints), and
//! floods out with the moves each reach level allows, tracing every move
//! through the collision itself:
//!
//! - Spots: over a grid of columns (`cell` apart, around the walkmesh's
//!   bounds and `margin` beyond), a point trace down each column finds the
//!   surfaces a player could stand on (normal Z at least JA's 0.7); a spot
//!   is where the player's box, dropped onto one from a step above, rests.
//!   A walkmesh so large that the grid would pass `columns` (a swoop
//!   track, kilometres of open ground) is audited on wider columns: the
//!   smallest multiple of `cell` that fits, which the report names.
//! - In bounds: a spot with a walkable walkmesh face within 0.35 m up or
//!   down and 1 m across; every other spot is out of bounds: a perch when
//!   it is inside the level, outside when it is not (on the hull, through
//!   a gap: an escape). Inside is under a roof (the player's box cannot
//!   rise 10 m from it: on furniture under the room's ceiling) or over
//!   the walkable walkmesh (a walkable face within 2.5 m below the player's
//!   box: on a rock or a crate in a roofless yard, or on a rim it overhangs
//!   at the floor's edge, among the walls the walkmesh's open edges get,
//!   which stand half the box's width out and reach far above any jump;
//!   higher, on a hull or a roof over a deck, is outside: the walls leave
//!   the sky open over doorways and seams).
//! - Starts: where the game puts a player is always walkable walkmesh:
//!   KOTOR lands a creature on the walkable face under its place, else
//!   under one of four places a metre away (reone does the same). So a
//!   start is the spot the player's box rests on dropped onto that face: a
//!   cutscene waypoint over a roof starts on the floor below, one beyond a
//!   railing on the floor beside it.
//! - Walk (every level): to each of the 8 neighbouring columns, straight
//!   or stepping up a step (18 units) first, then down to whatever is below.
//!   A step is refused as `sim::motion` refuses it: one that settles with
//!   the box's middle over a `NONWALK` surface (a ledge KOTOR's walker
//!   never stands on); jumps may land there.
//! - Jump (level n): up the level's ledge reach, then along 16 bearings at
//!   that height as far as the level's jump carries, dropping at every
//!   column passed. A spot keeps the moves of every lower level.
//! - Leaks: a drop that finds nothing above the grid's floor (the player
//!   falls out of the world), or a move off the grid's edge.
//!
//! Reached out-of-bounds spots are grouped into regions (neighbouring
//! columns within a step of each other) level by level: a level's regions
//! join only the spots it and the levels below reach, and count only their
//! leaks, so a walk's perch beside a wall is not one region with the
//! ground beyond it that only a force jump reaches. Everything visits in a
//! fixed order, so a report is a pure function of the level.
//!
//! Threads share the work without changing it: the scan's columns are
//! independent, and scouts make each queued spot's moves ahead of the
//! flood to have their traces ready (a trace is a pure function of its
//! ends). The flood still makes every move itself, in order, taking a
//! scout's trace where it has one; so the report is the same for any
//! number of threads. Falls go through the level's drop index
//! ([`DropIndex`]), which answers as the tree does.
//!
//! The moves are a bound, not movement: a jump goes up, then across, then
//! down, which reaches some ledges a real arc would not; crouching and
//! slopes a player slides down are not modelled.

use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, AtomicU8, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Condvar, Mutex};

use crate::clip_file::Accept;
use crate::player::{
    BASE_SPEED, GRAVITY, HALF_WIDTH, SOLE_Z, STAND_HEIGHT, STEP_HEIGHT,
    WALKABLE_NORMAL_Z as WALK_NORMAL,
};
use crate::{CollisionWorld, Contents, DropIndex, Extents, Level, Trace, UNIT, trig};

/// How a level of reach moves: `ledge` is how high above the floor the
/// feet can get onto something, `distance` how far a jump carries.
#[derive(Debug, Clone, PartialEq)]
pub struct Reach {
    pub name: String,
    pub ledge: f32,
    pub distance: f32,
}

/// JA's step (`STEPSIZE`), metres, and the lift crouching in the air adds,
/// units.
pub const STEP: f32 = STEP_HEIGHT * UNIT;
const CROUCH_LIFT: f32 = 16.0;

impl Reach {
    /// A jump `height` units high (JA's `forceJumpHeight` by level: 32 for
    /// a plain jump, then 96, 192, 384): the feet reach `height` + a step +
    /// the crouch lift (the reference's own sums: 66, 130, 226, 418), and
    /// it carries at run speed for as long as a fall from `height` and back
    /// takes (2 √(2 height / gravity)).
    pub fn jump(name: &str, height: f32) -> Reach {
        let airtime = 2.0 * (2.0 * height / GRAVITY).sqrt();
        Reach {
            name: name.to_string(),
            ledge: (height + STEP_HEIGHT + CROUCH_LIFT) * UNIT,
            distance: BASE_SPEED * airtime * UNIT,
        }
    }

    /// Walking only.
    pub fn walk() -> Reach {
        Reach {
            name: "walk".to_string(),
            ledge: 0.0,
            distance: 0.0,
        }
    }

    /// Walking, a plain jump, and force jump levels 1 to 3.
    pub fn levels() -> Vec<Reach> {
        vec![
            Reach::walk(),
            Reach::jump("jump", 32.0),
            Reach::jump("force 1", 96.0),
            Reach::jump("force 2", 192.0),
            Reach::jump("force 3", 384.0),
        ]
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Config {
    pub cell: f32,
    pub margin: f32,
    /// How far below the walkmesh's lowest point, and above its highest,
    /// the grid reaches.
    pub below: f32,
    pub above: f32,
    /// The player's box (JA's standing box, 30 × 30 × 64 units).
    pub player: Extents,
    pub reaches: Vec<Reach>,
    pub bearings: usize,
    pub mask: Contents,
    /// Threads that trace (0: as many as the machine runs at once). The
    /// report is the same for any number.
    pub threads: usize,
    /// The most columns the grid has: over a walkmesh so large that `cell`
    /// would make more, the columns are the smallest multiple of `cell`
    /// wide that fits.
    pub columns: u64,
}

/// How far a spot may sit from a walkable face, up or down and across,
/// and still be in bounds: KOTOR keeps its walkmesh's edge back from the
/// walls, farther than a player's box (0.38 m across its half) keeps.
pub const IN_BOUNDS_UP: f32 = 0.35;
pub const IN_BOUNDS_ACROSS: f32 = 1.0;
/// A spot the player's box cannot rise this far from is under a roof.
const ROOF: f32 = 10.0;
/// How high over the walkable walkmesh an unroofed spot is still a perch:
/// a plain jump's reach (1.68 m) from a floor that falls away beneath
/// where it lands. A hull or a roof over a deck (the Endar Spire's, 2.7 m
/// and more) is outside.
const PERCH: f32 = 2.5;

impl Default for Config {
    fn default() -> Config {
        Config {
            cell: 0.5,
            margin: 20.0,
            below: 20.0,
            above: 15.0,
            player: Extents {
                mins: [-HALF_WIDTH * UNIT, -HALF_WIDTH * UNIT, SOLE_Z * UNIT],
                maxs: [HALF_WIDTH * UNIT, HALF_WIDTH * UNIT, STAND_HEIGHT * UNIT],
            },
            reaches: Reach::levels(),
            bearings: 16,
            mask: Contents::PLAYER_SOLID,
            threads: 0,
            columns: 1 << 22,
        }
    }
}

/// A place the player's box can stand.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Spot {
    pub column: u32,
    /// The box's origin.
    pub origin: [f32; 3],
    pub material: u16,
    pub in_bounds: bool,
    /// Inside the level (in bounds, under a roof, or over the walkable
    /// walkmesh): on top of something in a room or a yard, as against
    /// outside it.
    pub inside: bool,
    /// The lowest reach level that gets here.
    pub level: Option<u8>,
    /// The spot it was reached from (itself for a start).
    pub from: u32,
    /// When it was reached, in the flood's order.
    pub order: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LeakKind {
    /// Nothing below: the player falls out of the world.
    Void,
    /// The move leaves the audited grid.
    Edge,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Leak {
    pub kind: LeakKind,
    pub level: u8,
    /// The spot it happens from, and where the player went over.
    pub spot: u32,
    pub at: [f32; 3],
}

/// Reached out-of-bounds spots that touch, of those a level and the levels
/// below reach.
#[derive(Debug, Clone, PartialEq)]
pub struct Region {
    /// The highest level its spots are reached at: the level it is a
    /// region of.
    pub level: u8,
    pub spots: Vec<u32>,
    pub area: f32,
    pub min: [f32; 3],
    pub max: [f32; 3],
    /// Mean of its spots' feet.
    pub centroid: [f32; 3],
    /// Every spot inside the level: a perch, not an escape.
    pub inside: bool,
    /// Its first spot reached at its level, and the spot that reached it.
    pub entry: u32,
    pub from: u32,
    /// Leaks from its spots at its level or below.
    pub leaks: usize,
    /// The accept entry that covers it.
    pub accepted: Option<String>,
}

impl Region {
    /// Outside, or leaking out of the world.
    pub fn escapes(&self) -> bool {
        !self.inside || self.leaks > 0
    }
}

/// Counts for one reach level: everything reached with it (and the levels
/// below).
#[derive(Debug, Clone, PartialEq)]
pub struct LevelSummary {
    pub name: String,
    pub reached: usize,
    pub out_of_bounds: usize,
    pub regions: usize,
    /// Regions not accepted that are outside or leak: what clip brushes
    /// must close.
    pub escapes: usize,
    /// Regions not accepted that are inside and do not leak: standing on
    /// things in a room.
    pub perches: usize,
    pub leaks: usize,
}

#[derive(Debug, Clone)]
pub struct Report {
    pub config: Config,
    /// The grid: its corner, columns across (X) and along (Y).
    pub origin: [f32; 2],
    pub nx: u32,
    pub ny: u32,
    pub spots: Vec<Spot>,
    pub regions: Vec<Region>,
    pub leaks: Vec<Leak>,
    pub levels: Vec<LevelSummary>,
    pub traces: u64,
    /// Spots found by landing on them rather than by the column scan.
    pub landed: u64,
    /// The spots the flood started from.
    pub seeds: Vec<u32>,
    /// In-bounds spots nothing reached (walkmesh the starts do not connect
    /// to: behind doors that stay shut, say).
    pub unreached_in_bounds: usize,
}

impl Report {
    /// The feet of a spot.
    pub fn feet(&self, spot: u32) -> [f32; 3] {
        let s = &self.spots[spot as usize];
        [
            s.origin[0],
            s.origin[1],
            s.origin[2] + self.config.player.mins[2],
        ]
    }

    pub fn column_centre(&self, column: u32) -> [f32; 2] {
        let (ix, iy) = (column % self.nx, column / self.nx);
        [
            self.origin[0] + (ix as f32 + 0.5) * self.config.cell,
            self.origin[1] + (iy as f32 + 0.5) * self.config.cell,
        ]
    }
}

/// What the audit works on.
pub struct Input<'a> {
    pub world: &'a Level,
    /// The walkable walkmesh faces alone.
    pub walkmesh: &'a (dyn CollisionWorld + Sync),
    /// The walkable faces' bounds.
    pub bounds: ([f32; 3], [f32; 3]),
    pub accepts: &'a [Accept],
    /// Where the game puts players (feet): the module's entry, its
    /// waypoints. The flood starts from the spots under them.
    pub starts: &'a [[f32; 3]],
}

/// The grid and what the audit traces: read-only, so the threads that
/// trace ahead of the flood share it.
struct Grid<'a> {
    input: &'a Input<'a>,
    config: &'a Config,
    origin: [f32; 2],
    nx: u32,
    ny: u32,
    top: f32,
    bottom: f32,
    /// The level's brushes by column, for drops.
    drops: DropIndex,
}

impl Grid<'_> {
    /// A trace through the level; the player's box falling goes by the
    /// drop index, which answers the same.
    fn trace(&self, start: [f32; 3], end: [f32; 3], extents: &Extents) -> Trace {
        let world = self.input.world;
        let mask = self.config.mask;
        if *extents == self.config.player {
            world.trace_down(&self.drops, start, end, mask)
        } else {
            world.trace(start, end, extents, mask)
        }
    }

    fn column_at(&self, x: f32, y: f32) -> Option<u32> {
        let fx = (x - self.origin[0]) / self.config.cell;
        let fy = (y - self.origin[1]) / self.config.cell;
        if fx < 0.0 || fy < 0.0 {
            return None;
        }
        let (ix, iy) = (fx as u32, fy as u32);
        (ix < self.nx && iy < self.ny).then_some(iy * self.nx + ix)
    }

    fn centre(&self, column: u32) -> [f32; 2] {
        let (ix, iy) = (column % self.nx, column / self.nx);
        [
            self.origin[0] + (ix as f32 + 0.5) * self.config.cell,
            self.origin[1] + (iy as f32 + 0.5) * self.config.cell,
        ]
    }

    /// A column's spots: where the player's box rests on each surface a
    /// point trace down its centre finds, top first, with their bounds.
    fn scan(&self, column: u32, traces: &mut u64) -> Vec<Spot> {
        let player = self.config.player;
        let [x, y] = self.centre(column);
        let mut z = self.top;
        let mut surfaces = Vec::new();
        for _ in 0..64 {
            *traces += 1;
            let hit = self.trace([x, y, z], [x, y, self.bottom], &Extents::POINT);
            if hit.fraction >= 1.0 {
                break;
            }
            if hit.plane.normal[2] >= WALK_NORMAL {
                surfaces.push(hit.end[2]);
            }
            // On through the surface's slab.
            z = hit.end[2] - 0.02;
        }
        let mut spots: Vec<Spot> = Vec::new();
        for surface in surfaces {
            let from = [x, y, surface - player.mins[2] + STEP];
            let to = [x, y, surface - player.mins[2] - 0.1];
            *traces += 1;
            let rest = self.trace(from, to, &player);
            if rest.start_solid || rest.fraction >= 1.0 || rest.plane.normal[2] < WALK_NORMAL {
                continue;
            }
            let duplicate = spots
                .iter()
                .any(|spot| (spot.origin[2] - rest.end[2]).abs() < 0.05);
            if !duplicate {
                spots.push(self.spot(column, &rest, traces));
            }
        }
        spots
    }

    /// The spot where the player's box rests after `rest`, not yet reached.
    fn spot(&self, column: u32, rest: &Trace, traces: &mut u64) -> Spot {
        let o = rest.end;
        let (in_bounds, inside) =
            self.bounds_of([o[0], o[1], o[2] + self.config.player.mins[2]], traces);
        Spot {
            column,
            origin: o,
            material: rest.surface.material,
            in_bounds,
            inside,
            level: None,
            from: u32::MAX,
            order: u32::MAX,
        }
    }

    /// Whether the player's box with feet here stands low over the walkable
    /// walkmesh (a walkable face within [`PERCH`] below its middle, a
    /// corner or an edge's middle): on a rock or a crate among the walls
    /// along its open edges. They stand half the box's width out, so a box
    /// may hang over the edge by that much, resting on a rim beside the
    /// floor. Higher is on a roof or a hull over the floor: outside.
    fn over_walkable(&self, feet: [f32; 3], traces: &mut u64) -> bool {
        let half = HALF_WIDTH * UNIT;
        let offsets = [-half, 0.0, half];
        offsets
            .iter()
            .flat_map(|&dx| offsets.iter().map(move |&dy| (dx, dy)))
            .any(|(dx, dy)| {
                *traces += 1;
                let (x, y) = (feet[0] + dx, feet[1] + dy);
                let below = self.input.walkmesh.trace(
                    [x, y, feet[2] + IN_BOUNDS_UP],
                    [x, y, feet[2] - PERCH],
                    &Extents::POINT,
                    Contents::SOLID,
                );
                below.fraction < 1.0
            })
    }

    /// Whether feet here are in bounds (a walkable face within
    /// [`IN_BOUNDS_ACROSS`] across and [`IN_BOUNDS_UP`] up or down), and
    /// whether they are inside (in bounds, over a walkable face, or under
    /// a roof).
    fn bounds_of(&self, feet: [f32; 3], traces: &mut u64) -> (bool, bool) {
        let mask = Contents::SOLID;
        let walkable = self.input.walkmesh;
        let mut near = false;
        'rings: for ring in 0..3 {
            let r = IN_BOUNDS_ACROSS * ring as f32 / 2.0;
            let samples = if ring == 0 { 1 } else { 8 };
            for k in 0..samples {
                let (sin, cos) = trig::sin_cos(k as f32 * (std::f32::consts::PI / 4.0));
                let (x, y) = (feet[0] + cos * r, feet[1] + sin * r);
                *traces += 1;
                let hit = walkable.trace(
                    [x, y, feet[2] + IN_BOUNDS_UP],
                    [x, y, feet[2] - IN_BOUNDS_UP],
                    &Extents::POINT,
                    mask,
                );
                if hit.fraction < 1.0 {
                    near = true;
                    break 'rings;
                }
            }
        }
        let inside = near || self.over_walkable(feet, traces) || {
            // The player's own box rising: a ceiling with gaps narrower
            // than the player (light wells, cracks between triangles) is
            // still a roof.
            *traces += 1;
            let player = self.config.player;
            let origin = [feet[0], feet[1], feet[2] - player.mins[2] + 0.01];
            let roof = self.trace(origin, [origin[0], origin[1], origin[2] + ROOF], &player);
            roof.fraction < 1.0 || roof.start_solid
        };
        (near, inside)
    }
}

/// Each column's spots: the scan's, numbered in column order (a range per
/// column), then those found by landing; and how many are not reached.
#[derive(Default)]
struct Columns {
    /// Column `c`'s scanned spots are `first[c]..first[c + 1]`.
    first: Vec<u32>,
    landed: Vec<Vec<u32>>,
    unreached: Vec<u32>,
}

impl Columns {
    fn spots(&self, column: u32) -> impl Iterator<Item = u32> + '_ {
        let c = column as usize;
        (self.first[c]..self.first[c + 1]).chain(self.landed[c].iter().copied())
    }

    /// Whether every spot of a column is reached (an empty one never is).
    fn done(&self, column: u32) -> bool {
        let c = column as usize;
        self.unreached[c] == 0 && (self.first[c + 1] > self.first[c] || !self.landed[c].is_empty())
    }
}

/// What a spot's moves ask of the flood: traces of the player's box,
/// whether a column is done, and what a drop lands on. The flood answers
/// them ([`Audit`]); so does a [`Scout`], tracing ahead of it.
trait Mover<'a> {
    fn grid(&self) -> &'a Grid<'a>;
    fn trace(&mut self, start: [f32; 3], end: [f32; 3]) -> Trace;
    /// A point's trace (never made ahead: the flood makes its own).
    fn point(&mut self, start: [f32; 3], end: [f32; 3]) -> Trace {
        let grid = self.grid();
        grid.input
            .world
            .trace(start, end, &Extents::POINT, grid.config.mask)
    }
    fn column_done(&self, column: u32) -> bool;
    /// Drops the player from `at` and returns what it lands on: `Ok(spot)`,
    /// `Err(true)` when it falls out of the world, `Err(false)` when it
    /// lands on nothing it can stand on.
    fn drop(&mut self, at: [f32; 3]) -> Result<u32, bool>;
}

/// A trace made ahead of the flood.
#[derive(Clone, Copy)]
struct Traced {
    start: [f32; 3],
    end: [f32; 3],
    trace: Trace,
}

impl Traced {
    fn is(&self, start: [f32; 3], end: [f32; 3]) -> bool {
        let same = |a: [f32; 3], b: [f32; 3]| (0..3).all(|i| a[i].to_bits() == b[i].to_bits());
        same(self.start, start) && same(self.end, end)
    }
}

/// The traces a scout made for the queue entry being processed, in the
/// order it made them. The flood asks for the same ones in the same order,
/// less the drops into columns finished since; a trace not among them (a
/// column a landing reopened) it makes itself.
#[derive(Default)]
struct Ahead {
    traced: Vec<Traced>,
    next: usize,
}

impl Ahead {
    fn take(&mut self, start: [f32; 3], end: [f32; 3]) -> Option<Trace> {
        let at = self.traced[self.next..]
            .iter()
            .position(|t| t.is(start, end))?;
        self.next += at + 1;
        Some(self.traced[self.next - 1].trace)
    }
}

struct Audit<'a> {
    grid: &'a Grid<'a>,
    spots: Vec<Spot>,
    columns: Columns,
    traces: u64,
    /// Spots found by landing on them rather than by the column scan.
    landed: u64,
    ahead: Ahead,
}

impl Audit<'_> {
    /// Finds every column's spots.
    fn scan(&mut self, threads: usize) {
        let grid = self.grid;
        let count = grid.nx * grid.ny;
        // Runs of columns: their spots, and how many each column has.
        let runs = parallel(count.div_ceil(SCAN_RUN) as usize, threads, |run| {
            let (mut spots, mut sizes, mut traces) = (Vec::new(), Vec::new(), 0);
            for column in run as u32 * SCAN_RUN..((run as u32 + 1) * SCAN_RUN).min(count) {
                let found = grid.scan(column, &mut traces);
                sizes.push(found.len() as u32);
                spots.extend(found);
            }
            (spots, sizes, traces)
        });
        let columns = &mut self.columns;
        columns.first = Vec::with_capacity(count as usize + 1);
        columns.first.push(0);
        columns.unreached = Vec::with_capacity(count as usize);
        for (spots, sizes, traces) in runs {
            self.traces += traces;
            for size in sizes {
                let last = *columns.first.last().expect("starts with 0");
                columns.first.push(last + size);
                columns.unreached.push(size);
            }
            self.spots.extend(spots);
        }
        columns.landed = vec![Vec::new(); count as usize];
    }

    /// A new spot where the player's box rests after `rest`.
    fn add(&mut self, column: u32, rest: &Trace) -> u32 {
        let spot = self.grid.spot(column, rest, &mut self.traces);
        let index = self.spots.len() as u32;
        self.spots.push(spot);
        self.columns.landed[column as usize].push(index);
        self.columns.unreached[column as usize] += 1;
        index
    }

    /// Marks `spot` reached at `level`, the `order`th, from `from`.
    fn reach(&mut self, spot: u32, level: u8, from: u32, order: u32) {
        let s = &mut self.spots[spot as usize];
        s.level = Some(level);
        s.from = from;
        s.order = order;
        self.columns.unreached[s.column as usize] -= 1;
    }

    /// The spot a player put at `at` (the box's origin) stands on, as KOTOR
    /// lands a creature: on the walkable face under the place, else under
    /// the first of four places a metre away that has one; none when no
    /// such face holds the player's box in bounds.
    fn start(&mut self, at: [f32; 3]) -> Option<u32> {
        let grid = self.grid;
        for (dx, dy) in [(0.0, 0.0), (0.0, 1.0), (1.0, 0.0), (0.0, -1.0), (-1.0, 0.0)] {
            let (x, y) = (at[0] + dx, at[1] + dy);
            let floor = grid.input.walkmesh.trace(
                [x, y, at[2] + grid.config.player.mins[2]],
                [x, y, grid.bottom],
                &Extents::POINT,
                Contents::SOLID,
            );
            if floor.fraction >= 1.0 {
                continue;
            }
            // The box dropped onto it from half a step above.
            let on = [x, y, floor.end[2] - grid.config.player.mins[2] + 0.5 * STEP];
            if let Ok(spot) = self.drop(on)
                && self.spots[spot as usize].in_bounds
            {
                return Some(spot);
            }
        }
        None
    }

    /// The spot in `column` nearest `z` (origin), within a quarter metre.
    fn spot_near(&self, column: u32, z: f32) -> Option<u32> {
        self.columns
            .spots(column)
            .filter(|&s| (self.spots[s as usize].origin[2] - z).abs() < 0.25)
            .min_by(|&a, &b| {
                let da = (self.spots[a as usize].origin[2] - z).abs();
                let db = (self.spots[b as usize].origin[2] - z).abs();
                da.total_cmp(&db).then(a.cmp(&b))
            })
    }
}

impl<'a> Mover<'a> for Audit<'a> {
    fn grid(&self) -> &'a Grid<'a> {
        self.grid
    }

    fn trace(&mut self, start: [f32; 3], end: [f32; 3]) -> Trace {
        self.traces += 1;
        self.ahead
            .take(start, end)
            .unwrap_or_else(|| self.grid.trace(start, end, &self.grid.config.player))
    }

    fn column_done(&self, column: u32) -> bool {
        self.columns.done(column)
    }

    fn drop(&mut self, at: [f32; 3]) -> Result<u32, bool> {
        let grid = self.grid;
        let Some(column) = grid.column_at(at[0], at[1]) else {
            return Err(false);
        };
        let fall = self.trace(at, [at[0], at[1], grid.bottom]);
        if fall.start_solid {
            return Err(false);
        }
        if fall.fraction >= 1.0 {
            return Err(true);
        }
        if fall.plane.normal[2] < WALK_NORMAL {
            return Err(false);
        }
        match self.spot_near(column, fall.end[2]) {
            Some(spot) => Ok(spot),
            None => {
                // Resting where the scan found nothing: on an edge the
                // column's centre misses, say.
                self.landed += 1;
                Ok(self.add(column, &fall))
            }
        }
    }
}

/// Makes a queue entry's moves ahead of the flood, on another thread, to
/// have its traces ready: which columns are done it reads from the flood's
/// latest word ([`Lookahead::done`]), and its drops land nowhere.
struct Scout<'a> {
    grid: &'a Grid<'a>,
    done: &'a [AtomicU64],
    traced: Vec<Traced>,
}

impl<'a> Mover<'a> for Scout<'a> {
    fn grid(&self) -> &'a Grid<'a> {
        self.grid
    }

    fn trace(&mut self, start: [f32; 3], end: [f32; 3]) -> Trace {
        let trace = self.grid.trace(start, end, &self.grid.config.player);
        self.traced.push(Traced { start, end, trace });
        trace
    }

    fn column_done(&self, column: u32) -> bool {
        self.done[column as usize / 64].load(Ordering::Relaxed) & (1 << (column % 64)) != 0
    }

    fn drop(&mut self, at: [f32; 3]) -> Result<u32, bool> {
        if self.grid.column_at(at[0], at[1]).is_some() {
            self.trace(at, [at[0], at[1], self.grid.bottom]);
        }
        Err(false)
    }
}

/// The 8 neighbouring columns, in the order walks try them.
const NEIGHBOURS: [(i32, i32); 8] = [
    (1, 0),
    (1, 1),
    (0, 1),
    (-1, 1),
    (-1, 0),
    (-1, -1),
    (0, -1),
    (1, -1),
];

/// A queue entry's moves at `level` from a spot (its origin and column):
/// when `fresh`, its walks and every lower level's jump; then the level's
/// own jump.
fn spot_moves<'a>(
    m: &mut impl Mover<'a>,
    (origin, column): ([f32; 3], u32),
    fresh: bool,
    level: u8,
    out: &mut Vec<Move>,
) {
    let reaches = &m.grid().config.reaches;
    if fresh {
        for (dx, dy) in NEIGHBOURS {
            walk(m, origin, dx, dy, out);
        }
        for lower in reaches.iter().take(level as usize).skip(1) {
            jump(m, (origin, column), lower, out);
        }
    }
    if level > 0 {
        jump(m, (origin, column), &reaches[level as usize], out);
    }
}

/// Where a walk from `start` to the neighbouring column `(dx, dy)` gets.
fn walk<'a>(m: &mut impl Mover<'a>, start: [f32; 3], dx: i32, dy: i32, out: &mut Vec<Move>) {
    let grid = m.grid();
    let cell = grid.config.cell;
    let to = [
        start[0] + dx as f32 * cell,
        start[1] + dy as f32 * cell,
        start[2],
    ];
    if grid.column_at(to[0], to[1]).is_none() {
        out.push(Move::Leak(LeakKind::Edge, to));
        return;
    }
    let flat = m.trace(start, to);
    let over = if flat.fraction >= 1.0 && !flat.start_solid {
        to
    } else {
        let up = m.trace(start, [start[0], start[1], start[2] + STEP]);
        let raised = [to[0], to[1], up.end[2]];
        let across = m.trace(up.end, raised);
        if across.fraction < 1.0 || across.start_solid {
            return;
        }
        if settles_on_nonwalk(m, raised, up.end[2] - start[2]) {
            return;
        }
        raised
    };
    match m.drop(over) {
        Ok(landed) => out.push(Move::Spot(landed)),
        Err(true) => out.push(Move::Leak(LeakKind::Void, over)),
        Err(false) => {}
    }
}

/// Whether a step raised `rise` to `raised` settles back down within it
/// with the box's middle over a `NONWALK` surface (the face under its
/// soles' middle, within a step), which `sim::motion` refuses.
fn settles_on_nonwalk<'a>(m: &mut impl Mover<'a>, raised: [f32; 3], rise: f32) -> bool {
    let settle = m.trace(raised, [raised[0], raised[1], raised[2] - rise]);
    if settle.fraction >= 1.0 || !settle.contents.intersects(Contents::NONWALK) {
        return false;
    }
    let soles = settle.end[2] + m.grid().config.player.mins[2] + UNIT;
    let under = m.point(
        [settle.end[0], settle.end[1], soles],
        [settle.end[0], settle.end[1], soles - STEP],
    );
    under.fraction < 1.0 && under.contents.intersects(Contents::NONWALK)
}

/// Where a jump of `reach` from a spot (its origin and column) gets.
fn jump<'a>(
    m: &mut impl Mover<'a>,
    (start, column): ([f32; 3], u32),
    reach: &Reach,
    out: &mut Vec<Move>,
) {
    let grid = m.grid();
    let up = m.trace(start, [start[0], start[1], start[2] + reach.ledge]);
    if up.start_solid {
        return;
    }
    let apex = up.end;
    let cell = grid.config.cell;
    let bearings = grid.config.bearings;
    for bearing in 0..bearings {
        let angle = bearing as f32 * (2.0 * std::f32::consts::PI / bearings as f32);
        let (sin, cos) = trig::sin_cos(angle);
        let far = [
            apex[0] + cos * reach.distance,
            apex[1] + sin * reach.distance,
            apex[2],
        ];
        let across = m.trace(apex, far);
        let carried = across.fraction * reach.distance;
        let mut last = column;
        let mut k = 1;
        while k as f32 * cell <= carried {
            let d = k as f32 * cell;
            k += 1;
            let at = [apex[0] + cos * d, apex[1] + sin * d, apex[2]];
            let Some(column) = grid.column_at(at[0], at[1]) else {
                out.push(Move::Leak(LeakKind::Edge, at));
                break;
            };
            if column == last {
                continue;
            }
            last = column;
            if m.column_done(column) {
                continue;
            }
            match m.drop(at) {
                Ok(landed) => out.push(Move::Spot(landed)),
                Err(true) => out.push(Move::Leak(LeakKind::Void, at)),
                Err(false) => {}
            }
        }
    }
}

enum Move {
    Spot(u32),
    Leak(LeakKind, [f32; 3]),
}

/// Columns a scanning thread takes at a time.
const SCAN_RUN: u32 = 1024;
/// How far ahead of the flood its queue is traced, in entries.
const WINDOW: usize = 1024;

/// A queue entry as the scouts need it: the spot's origin and column, and
/// the moves to make from it.
#[derive(Clone, Copy, Default)]
struct Job {
    origin: [f32; 3],
    column: u32,
    fresh: bool,
    level: u8,
}

/// A slot of the window: empty, an entry posted, claimed (a scout or the
/// flood makes its moves), or traced.
const EMPTY: u8 = 0;
const POSTED: u8 = 1;
const CLAIMED: u8 = 2;
const TRACED: u8 = 3;

#[derive(Default)]
struct Slot {
    state: AtomicU8,
    job: Mutex<Job>,
    traced: Mutex<Vec<Traced>>,
}

/// The flood's queue, traced ahead of it by scout threads: the next
/// [`WINDOW`] entries, numbered in queue order, each in slot `number %
/// WINDOW`. Scouts claim posted entries in order; the flood takes each
/// entry's traces when it comes to it, waits for a scout still at work on
/// it, or claims it and makes its moves itself.
struct Lookahead {
    slots: Vec<Slot>,
    /// Entries posted so far.
    posted: AtomicU64,
    /// The next entry a scout tries to claim.
    next: AtomicU64,
    /// Which columns the flood has found done, a bit each: what the scouts
    /// skip drops into.
    done: Vec<AtomicU64>,
    over: AtomicBool,
    /// Scouts asleep for want of entries, and their bell.
    asleep: AtomicUsize,
    bell: (Mutex<()>, Condvar),
}

impl Lookahead {
    fn new(columns: u32) -> Lookahead {
        Lookahead {
            slots: (0..WINDOW).map(|_| Slot::default()).collect(),
            posted: AtomicU64::new(0),
            next: AtomicU64::new(0),
            done: (0..columns.div_ceil(64))
                .map(|_| AtomicU64::new(0))
                .collect(),
            over: AtomicBool::new(false),
            asleep: AtomicUsize::new(0),
            bell: (Mutex::new(()), Condvar::new()),
        }
    }

    fn slot(&self, number: u64) -> &Slot {
        &self.slots[(number % WINDOW as u64) as usize]
    }

    fn mark(&self, column: u32, done: bool) {
        let bit = 1 << (column % 64);
        let word = &self.done[column as usize / 64];
        if done {
            word.fetch_or(bit, Ordering::Relaxed);
        } else {
            word.fetch_and(!bit, Ordering::Relaxed);
        }
    }

    /// Posts entry `number` (the window has room: its slot is empty).
    fn post(&self, number: u64, job: Job) {
        let slot = self.slot(number);
        *slot.job.lock().expect("no scout panics holding it") = job;
        slot.state.store(POSTED, Ordering::Release);
        self.posted.store(number + 1, Ordering::Release);
    }

    fn ring(&self) {
        if self.asleep.load(Ordering::Relaxed) > 0 {
            let _bell = self.bell.0.lock().expect("no scout panics holding it");
            self.bell.1.notify_all();
        }
    }

    /// Entry `number`'s traces: a scout's, waited for if one is at work
    /// on it; none when the flood claims it (and makes its moves itself).
    fn take(&self, number: u64) -> Vec<Traced> {
        let slot = self.slot(number);
        if slot
            .state
            .compare_exchange(POSTED, CLAIMED, Ordering::Acquire, Ordering::Relaxed)
            .is_ok()
        {
            slot.state.store(EMPTY, Ordering::Release);
            self.next.fetch_max(number + 1, Ordering::Relaxed);
            return Vec::new();
        }
        let mut spins = 0u32;
        while slot.state.load(Ordering::Acquire) != TRACED {
            spins += 1;
            if spins < 64 {
                std::hint::spin_loop();
            } else {
                std::thread::yield_now();
            }
        }
        let traced = std::mem::take(&mut *slot.traced.lock().expect("no scout panics holding it"));
        slot.state.store(EMPTY, Ordering::Release);
        traced
    }

    fn finish(&self) {
        self.over.store(true, Ordering::Release);
        let _bell = self.bell.0.lock().expect("no scout panics holding it");
        self.bell.1.notify_all();
    }

    /// A scout: claims posted entries in order and traces their moves.
    fn scout(&self, grid: &Grid) {
        let mut idle = 0u32;
        while !self.over.load(Ordering::Acquire) {
            let number = self.next.load(Ordering::Relaxed);
            if number >= self.posted.load(Ordering::Acquire) {
                idle += 1;
                if idle < 64 {
                    std::hint::spin_loop();
                } else {
                    let bell = self.bell.0.lock().expect("no scout panics holding it");
                    self.asleep.fetch_add(1, Ordering::Relaxed);
                    if self.next.load(Ordering::Relaxed) >= self.posted.load(Ordering::Acquire)
                        && !self.over.load(Ordering::Acquire)
                    {
                        let wait = std::time::Duration::from_millis(1);
                        drop(self.bell.1.wait_timeout(bell, wait));
                    }
                    self.asleep.fetch_sub(1, Ordering::Relaxed);
                }
                continue;
            }
            idle = 0;
            if self
                .next
                .compare_exchange_weak(number, number + 1, Ordering::Relaxed, Ordering::Relaxed)
                .is_err()
            {
                continue;
            }
            // The slot may hold a later entry by now (the flood went past
            // this one): whichever it holds is traced for it.
            let slot = self.slot(number);
            if slot
                .state
                .compare_exchange(POSTED, CLAIMED, Ordering::Acquire, Ordering::Relaxed)
                .is_err()
            {
                continue;
            }
            let job = *slot.job.lock().expect("no scout panics holding it");
            let mut scout = Scout {
                grid,
                done: &self.done,
                traced: Vec::new(),
            };
            let from = (job.origin, job.column);
            spot_moves(&mut scout, from, job.fresh, job.level, &mut Vec::new());
            *slot.traced.lock().expect("no scout panics holding it") = scout.traced;
            slot.state.store(TRACED, Ordering::Release);
        }
    }
}

/// `work` for every index below `count`, on `threads` threads, the results
/// in index order.
fn parallel<T: Send>(count: usize, threads: usize, work: impl Fn(usize) -> T + Sync) -> Vec<T> {
    if threads <= 1 || count <= 1 {
        return (0..count).map(work).collect();
    }
    let next = AtomicUsize::new(0);
    let mut results: Vec<Option<T>> = (0..count).map(|_| None).collect();
    std::thread::scope(|scope| {
        let workers: Vec<_> = (0..threads)
            .map(|_| {
                scope.spawn(|| {
                    let mut done = Vec::new();
                    loop {
                        let index = next.fetch_add(1, Ordering::Relaxed);
                        if index >= count {
                            break;
                        }
                        done.push((index, work(index)));
                    }
                    done
                })
            })
            .collect();
        for worker in workers {
            for (index, result) in worker.join().expect("an audit thread panicked") {
                results[index] = Some(result);
            }
        }
    });
    results
        .into_iter()
        .map(|result| result.expect("every index is worked"))
        .collect()
}

/// The flood: its queue, the scouts tracing it ahead, and what it found.
struct Flood<'a, 'g> {
    audit: &'a mut Audit<'g>,
    lookahead: Option<&'a Lookahead>,
    /// Spots, whether each is newly reached, and the level of the moves
    /// to make from it.
    queue: VecDeque<(u32, bool, u8)>,
    /// Entries taken and posted so far: an entry's number is its place in
    /// the queue.
    taken: u64,
    posted: u64,
    order: u32,
    leaks: &'a mut Vec<Leak>,
    leaked: &'a mut Vec<u8>,
}

impl Flood<'_, '_> {
    /// Queues `spots` for `level`'s moves.
    fn push(&mut self, spots: &[u32], fresh: bool, level: u8) {
        self.queue
            .extend(spots.iter().map(|&spot| (spot, fresh, level)));
        if let Some(lookahead) = self.lookahead {
            for &spot in spots {
                let column = self.audit.spots[spot as usize].column;
                lookahead.mark(column, self.audit.columns.done(column));
            }
        }
        self.post();
    }

    /// Posts the queue's entries within the window to the scouts.
    fn post(&mut self) {
        let Some(lookahead) = self.lookahead else {
            return;
        };
        let end = self.taken + self.queue.len().min(WINDOW) as u64;
        let first = self.posted;
        while self.posted < end {
            let (spot, fresh, level) = self.queue[(self.posted - self.taken) as usize];
            let s = &self.audit.spots[spot as usize];
            let job = Job {
                origin: s.origin,
                column: s.column,
                fresh,
                level,
            };
            lookahead.post(self.posted, job);
            self.posted += 1;
        }
        // Asleep scouts are woken for every 32 entries posted.
        if self.posted / 32 > first / 32 {
            lookahead.ring();
        }
    }

    /// Makes the moves of every entry in the queue, and of every entry
    /// they queue, in order.
    fn drain(&mut self) {
        let mut moves = Vec::new();
        let mut reached = Vec::new();
        while let Some((spot, fresh, level)) = self.queue.pop_front() {
            let traced = self
                .lookahead
                .map_or_else(Vec::new, |lookahead| lookahead.take(self.taken));
            self.taken += 1;
            let audit = &mut *self.audit;
            let landed = audit.spots.len();
            audit.ahead = Ahead { traced, next: 0 };
            let s = &audit.spots[spot as usize];
            let from = (s.origin, s.column);
            spot_moves(audit, from, fresh, level, &mut moves);
            if let Some(lookahead) = self.lookahead {
                // A landing leaves its column with a spot not reached.
                for index in landed..audit.spots.len() {
                    lookahead.mark(audit.spots[index].column, false);
                }
            }
            for found in moves.drain(..) {
                match found {
                    Move::Spot(next) => {
                        if audit.spots[next as usize].level.is_none() {
                            audit.reach(next, level, spot, self.order);
                            self.order += 1;
                            reached.push(next);
                        }
                    }
                    Move::Leak(kind, at) => {
                        let bit = match kind {
                            LeakKind::Void => 1,
                            LeakKind::Edge => 2,
                        };
                        if self.leaked.len() <= spot as usize {
                            self.leaked.resize(audit.spots.len(), 0);
                        }
                        if self.leaked[spot as usize] & bit == 0 {
                            self.leaked[spot as usize] |= bit;
                            self.leaks.push(Leak {
                                kind,
                                level,
                                spot,
                                at,
                            });
                        }
                    }
                }
            }
            self.push(&reached, true, level);
            reached.clear();
        }
        self.audit.ahead = Ahead::default();
    }
}

/// The drop index's columns, metres: a few of the grid's, fewer over a
/// large grid.
const DROP_CELL: f32 = 1.0;
const DROP_COLUMNS: u32 = 1 << 20;

/// The level's drop index over the grid.
fn drop_index(level: &Level, config: &Config, origin: [f32; 2], [nx, ny]: [u32; 2]) -> DropIndex {
    let (width, depth) = (nx as f32 * config.cell, ny as f32 * config.cell);
    let mut cell = DROP_CELL.max(config.cell);
    let size = |cell: f32| [(width / cell) as u32 + 1, (depth / cell) as u32 + 1];
    while size(cell)[0] * size(cell)[1] > DROP_COLUMNS {
        cell *= 2.0;
    }
    level.drop_index(&config.player, origin, cell, size(cell))
}

/// The audit's grid over `bounds`: the config with its cell widened to fit
/// [`Config::columns`], the grid's corner, and its columns across and along.
fn grid_over(bounds: ([f32; 3], [f32; 3]), config: &Config) -> (Config, [f32; 2], [u32; 2]) {
    let (low, high) = bounds;
    let size = |cell: f32| {
        let n = |axis: usize| ((high[axis] - low[axis] + 2.0 * config.margin) / cell) as u32 + 1;
        [n(0), n(1)]
    };
    let columns = |cell: f32| size(cell).iter().map(|&n| u64::from(n)).product::<u64>();
    let cell = (1..)
        .map(|n| config.cell * n as f32)
        .find(|&cell| columns(cell) <= config.columns)
        .expect("a wide enough cell makes one column");
    let origin = [low[0] - config.margin, low[1] - config.margin];
    (
        Config {
            cell,
            ..config.clone()
        },
        origin,
        size(cell),
    )
}

/// The audit's grid with nothing scanned or reached: a frame to draw the
/// level in where it is not audited.
pub fn frame(bounds: ([f32; 3], [f32; 3]), config: &Config) -> Report {
    let (config, origin, [nx, ny]) = grid_over(bounds, config);
    Report {
        levels: Vec::new(),
        config,
        origin,
        nx,
        ny,
        spots: Vec::new(),
        regions: Vec::new(),
        leaks: Vec::new(),
        traces: 0,
        landed: 0,
        seeds: Vec::new(),
        unreached_in_bounds: 0,
    }
}

/// Runs the audit.
pub fn run(input: &Input, config: &Config) -> Report {
    let (low, high) = input.bounds;
    let (config, origin, [nx, ny]) = grid_over(input.bounds, config);
    let config = &config;
    let threads = match config.threads {
        0 => std::thread::available_parallelism().map_or(1, |n| n.get()),
        n => n,
    };
    let grid = Grid {
        input,
        config,
        origin,
        nx,
        ny,
        top: high[2] + config.above,
        bottom: low[2] - config.below,
        drops: drop_index(input.world, config, origin, [nx, ny]),
    };
    let mut audit = Audit {
        grid: &grid,
        spots: Vec::new(),
        columns: Columns::default(),
        traces: 0,
        landed: 0,
        ahead: Ahead::default(),
    };
    audit.scan(threads);

    let mut leaks = Vec::new();
    // Each spot's leaks are listed once per kind.
    let mut leaked = vec![0u8; audit.spots.len()];
    let mut order = 0u32;
    // The spots the starts rest on, on the walkable walkmesh; every
    // in-bounds spot when none does.
    let mut seeds = Vec::new();
    for start in input.starts {
        let lift = 0.5 - config.player.mins[2];
        if let Some(spot) = audit.start([start[0], start[1], start[2] + lift])
            && !seeds.contains(&spot)
        {
            seeds.push(spot);
        }
    }
    if seeds.is_empty() {
        seeds = (0..audit.spots.len() as u32)
            .filter(|&s| audit.spots[s as usize].in_bounds)
            .collect();
    }
    for &index in &seeds {
        audit.reach(index, 0, index, order);
        order += 1;
    }
    let lookahead = Lookahead::new(nx * ny);
    std::thread::scope(|scope| {
        for _ in 1..threads {
            scope.spawn(|| lookahead.scout(&grid));
        }
        let mut flood = Flood {
            audit: &mut audit,
            lookahead: (threads > 1).then_some(&lookahead),
            queue: VecDeque::new(),
            taken: 0,
            posted: 0,
            order,
            leaks: &mut leaks,
            leaked: &mut leaked,
        };
        flood.push(&seeds, true, 0);
        for level in 0..config.reaches.len() as u8 {
            if level > 0 {
                // Spots reached before get this level's jump; new ones get
                // everything.
                let reached: Vec<u32> = (0..flood.audit.spots.len() as u32)
                    .filter(|&s| flood.audit.spots[s as usize].level.is_some())
                    .collect();
                flood.push(&reached, false, level);
            }
            flood.drain();
        }
        lookahead.finish();
    });

    // Each level's regions; the report lists those a level adds or grows.
    let mut regions = Vec::new();
    let levels = config
        .reaches
        .iter()
        .enumerate()
        .map(|(level, reach)| {
            let level = level as u8;
            let within = |l: Option<u8>| l.is_some_and(|l| l <= level);
            let at_level = level_regions(&audit, &leaks, input.accepts, level);
            let summary = LevelSummary {
                name: reach.name.clone(),
                reached: audit.spots.iter().filter(|s| within(s.level)).count(),
                out_of_bounds: audit
                    .spots
                    .iter()
                    .filter(|s| within(s.level) && !s.in_bounds)
                    .count(),
                regions: at_level.len(),
                escapes: at_level
                    .iter()
                    .filter(|r| r.accepted.is_none() && r.escapes())
                    .count(),
                perches: at_level
                    .iter()
                    .filter(|r| r.accepted.is_none() && !r.escapes())
                    .count(),
                leaks: leaks.iter().filter(|leak| leak.level <= level).count(),
            };
            regions.extend(at_level.into_iter().filter(|r| r.level == level));
            summary
        })
        .collect();
    Report {
        config: config.clone(),
        origin,
        nx,
        ny,
        unreached_in_bounds: audit
            .spots
            .iter()
            .filter(|s| s.in_bounds && s.level.is_none())
            .count(),
        seeds,
        spots: audit.spots,
        regions,
        leaks,
        levels,
        traces: audit.traces,
        landed: audit.landed,
    }
}

fn find(parent: &mut [u32], mut x: u32) -> u32 {
    while parent[x as usize] != x {
        parent[x as usize] = parent[parent[x as usize] as usize];
        x = parent[x as usize];
    }
    x
}

/// The regions of the out-of-bounds spots reached at `level` or below, and
/// their leaks at those levels. A region's level is its spots' highest: the
/// level that made it as it is (a lower one's region it did not grow is
/// that level's too).
fn level_regions(audit: &Audit, leaks: &[Leak], accepts: &[Accept], level: u8) -> Vec<Region> {
    let spots = &audit.spots;
    let (nx, ny) = (audit.grid.nx, audit.grid.ny);
    let out = |s: &Spot| s.level.is_some_and(|l| l <= level) && !s.in_bounds;
    let mut parent: Vec<u32> = (0..spots.len() as u32).collect();
    for (index, spot) in spots.iter().enumerate() {
        if !out(spot) {
            continue;
        }
        let (ix, iy) = (spot.column % nx, spot.column / nx);
        for (dx, dy) in [(1i32, 0i32), (0, 1), (1, 1), (-1, 1)] {
            let (jx, jy) = (ix as i32 + dx, iy as i32 + dy);
            if jx < 0 || jy < 0 || jx as u32 >= nx || jy as u32 >= ny {
                continue;
            }
            for other in audit.columns.spots(jy as u32 * nx + jx as u32) {
                let o = &spots[other as usize];
                if out(o) && (o.origin[2] - spot.origin[2]).abs() <= STEP + 0.1 {
                    let (a, b) = (find(&mut parent, index as u32), find(&mut parent, other));
                    let (low, high) = (a.min(b), a.max(b));
                    parent[high as usize] = low;
                }
            }
        }
    }
    let mut groups: std::collections::BTreeMap<u32, Vec<u32>> = std::collections::BTreeMap::new();
    for index in 0..spots.len() as u32 {
        if out(&spots[index as usize]) {
            let root = find(&mut parent, index);
            groups.entry(root).or_insert_with(Vec::new).push(index);
        }
    }
    // Leaks by the region (its root) of the spot they happen from.
    let mut leaked: std::collections::BTreeMap<u32, usize> = std::collections::BTreeMap::new();
    for leak in leaks.iter().filter(|leak| leak.level <= level) {
        if out(&spots[leak.spot as usize]) {
            *leaked.entry(find(&mut parent, leak.spot)).or_default() += 1;
        }
    }
    let cell = audit.grid.config.cell;
    let lift = audit.grid.config.player.mins[2];
    let mut regions: Vec<Region> = groups
        .into_iter()
        .map(|(root, members)| {
            let feet = |s: u32| {
                let o = spots[s as usize].origin;
                [o[0], o[1], o[2] + lift]
            };
            let mut min = [f32::INFINITY; 3];
            let mut max = [f32::NEG_INFINITY; 3];
            let mut sum = [0.0f32; 3];
            for &s in &members {
                let f = feet(s);
                for i in 0..3 {
                    min[i] = min[i].min(f[i]);
                    max[i] = max[i].max(f[i]);
                    sum[i] += f[i];
                }
            }
            let n = members.len() as f32;
            let centroid = sum.map(|v| v / n);
            let level = members
                .iter()
                .filter_map(|&s| spots[s as usize].level)
                .max()
                .expect("a region's spots are reached");
            // Its first spot reached at its level: how that level got in.
            let entry = *members
                .iter()
                .filter(|&&s| spots[s as usize].level == Some(level))
                .min_by_key(|&&s| spots[s as usize].order)
                .expect("a region has spots at its level");
            let leaks = leaked.get(&root).copied().unwrap_or(0);
            let accepted = accepts
                .iter()
                .find(|accept| {
                    let covered = members.iter().filter(|&&s| accept.covers(feet(s))).count();
                    covered * 2 >= members.len()
                })
                .map(|accept| accept.name.clone());
            Region {
                level,
                area: n * cell * cell,
                inside: members.iter().all(|&s| spots[s as usize].inside),
                from: spots[entry as usize].from,
                spots: members,
                min,
                max,
                centroid,
                entry,
                leaks,
                accepted,
            }
        })
        .collect();
    regions.sort_by(|a, b| {
        a.level
            .cmp(&b.level)
            .then(b.spots.len().cmp(&a.spots.len()))
            .then(a.entry.cmp(&b.entry))
    });
    regions
}
