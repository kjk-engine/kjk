//! An area's map: how its picture (`lbl_map<area>`) shows the world, from
//! the ARE's `Map` struct, and the notes its waypoints put on it.
//!
//! Two world points and where they fall on the picture fix a scale for each
//! of the picture's axes. Which world axis runs across the picture depends
//! on its north: with north along world Y (`NorthAxis` 0 or 1) world X runs
//! across and world Y down; along world X (2 or 3) the two swap.

use std::f32::consts::{FRAC_PI_2, PI, TAU};

use fmt_odyssey::gff::{GffStruct, GffValue};

use crate::module::{Title, localized};

/// An area's `Map`: its picture's north, two world points (game space,
/// metres) and where each falls on the picture (fractions across the part
/// of it the game uses and down it), and the ARE's zoom and resolution.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AreaMap {
    /// `NorthAxis`: the world direction the picture's top faces, 0 to 3:
    /// +Y, −Y, +X, −X.
    pub north_axis: u8,
    /// `MapPt1`, `MapPt2`.
    pub map_points: [[f32; 2]; 2],
    /// `WorldPt1`, `WorldPt2`.
    pub world_points: [[f32; 2]; 2],
    /// `MapZoom`, `MapResX`.
    pub zoom: i32,
    pub res_x: i32,
}

/// One of the picture's axes: the fraction along it is world axis `world`
/// (0 X, 1 Y; metres) times `scale`, plus `offset`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MapAxis {
    pub world: u8,
    pub scale: f32,
    pub offset: f32,
}

/// Each north's direction as a yaw (radians anticlockwise from world +X).
const NORTH: [f32; 4] = [FRAC_PI_2, 3.0 * FRAC_PI_2, 0.0, PI];

impl AreaMap {
    /// The map `are` (an ARE's root) describes; `None` without one, with
    /// a north outside 0 to 3, or with points that fix no scale (the same
    /// world or map coordinate twice).
    pub fn read(are: &GffStruct) -> Option<AreaMap> {
        let Some(GffValue::Struct(map)) = are.get("Map") else {
            return None;
        };
        let north_axis = u8::try_from(map.uint("NorthAxis")?)
            .ok()
            .filter(|axis| *axis < 4)?;
        let point = |prefix: &str| {
            [
                map.float(&format!("{prefix}X")).unwrap_or(0.0),
                map.float(&format!("{prefix}Y")).unwrap_or(0.0),
            ]
        };
        let map_points = [point("MapPt1"), point("MapPt2")];
        let world_points = [point("WorldPt1"), point("WorldPt2")];
        let apart = |[one, two]: [[f32; 2]; 2]| (0..2).all(|axis| one[axis] != two[axis]);
        (apart(map_points) && apart(world_points)).then_some(AreaMap {
            north_axis,
            map_points,
            world_points,
            zoom: map.int("MapZoom").map(|v| v as i32).unwrap_or(0),
            res_x: map.int("MapResX").map(|v| v as i32).unwrap_or(0),
        })
    }

    /// Where the world point `world` (game space X and Y) falls on the
    /// picture: fractions across its used part and down it (outside 0..1
    /// beyond the picture).
    pub fn point(&self, world: [f32; 2]) -> [f32; 2] {
        self.axes()
            .map(|axis| world[usize::from(axis.world)] * axis.scale + axis.offset)
    }

    /// The picture's two axes, across then down, each from its world axis.
    pub fn axes(&self) -> [MapAxis; 2] {
        let [first, second] = self.world_points;
        let [one, two] = self.map_points;
        let (across, down) = if self.north_axis < 2 { (0, 1) } else { (1, 0) };
        let axis = |on: usize, from: usize| {
            let scale = (one[on] - two[on]) / (first[from] - second[from]);
            MapAxis {
                world: from as u8,
                scale,
                offset: one[on] - first[from] * scale,
            }
        };
        [axis(0, across), axis(1, down)]
    }

    /// How far an arrow drawn pointing up turns, clockwise on the picture
    /// and in radians (0 to 2π), to point along `yaw` (radians anticlockwise
    /// from world +X).
    pub fn arrow(&self, yaw: f32) -> f32 {
        (NORTH[usize::from(self.north_axis)] - yaw).rem_euclid(TAU)
    }
}

/// A waypoint's note on the map: its words and whether it shows yet.
#[derive(Debug, Clone, PartialEq)]
pub struct MapNote {
    pub text: Title,
    pub enabled: bool,
}

impl MapNote {
    /// The note `records` (an instance, then its template) give, each field
    /// from the first that has it; `None` unless `HasMapNote` is set.
    pub fn read(records: &[&GffStruct]) -> Option<MapNote> {
        let field = |label: &str| records.iter().find_map(|record| record.get(label));
        let flag = |label: &str| {
            field(label)
                .and_then(GffValue::uint)
                .is_some_and(|value| value != 0)
        };
        flag("HasMapNote").then(|| MapNote {
            text: records
                .iter()
                .map(|record| localized(record.get("MapNote")))
                .find(|text| *text != Title::None)
                .unwrap_or(Title::None),
            enabled: flag("MapNoteEnabled"),
        })
    }
}
