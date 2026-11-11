//! Which rooms a fight's light reaches. A fight's light is a bolt, a
//! blade, a flash or a power (`material::LIGHT_COMBAT`). It lights only
//! the rooms linked with the room it is in, so it does not light the far
//! side of a wall. Room culling draws the rooms linked with the camera's
//! room by the same rule (`linked`).
//!
//! What a scene's rooms give the lights is worked out once, when the scene
//! is spawned (`FightRooms::new`): each room's mask of the rooms its lights
//! do not reach, and the bounds used to find a light's room quickly.

use bevy::prelude::*;

use crate::material::VIS_ROOMS;
use crate::viewer::RoomView;

/// How far above a light a floor may lie (metres) and still count as the
/// floor under it. A bolt skimming a ramp flies a little below the ramp's
/// higher end.
const FLOOR_ABOVE_LIGHT: f32 = 0.5;

/// How far below a light a floor may lie (metres) and still count as the
/// floor of the room the light is in. A light higher than this over a
/// floor is over a pit, a drop or ground nobody walks on. The room whose
/// bounds hold it is taken instead, not a room far below.
const FLOOR_BELOW_LIGHT: f32 = 4.0;

/// Whether a room is only seen. A room is only seen when the game ships
/// it no walkable floor and its walkmesh read without error. Skies,
/// backdrops and set pieces are such rooms. No VIS lists them.
pub(crate) fn seen_only(room: &RoomView) -> bool {
    room.floor.is_empty() && room.floor_error.is_none()
}

/// Whether room `from`'s VIS lists room `to`.
fn lists(rooms: &[RoomView], from: usize, to: usize) -> bool {
    rooms[from]
        .visible
        .iter()
        .any(|name| name.eq_ignore_ascii_case(&rooms[to].name))
}

/// Whether room `b` is linked with room `a`. A room is linked with itself
/// and with every room either of their VIS lists names. K1 makes every link
/// mutual, so a link one way counts both ways. A room that is only seen is
/// linked with every room, because no VIS lists it. Culling draws the rooms
/// linked with the camera's room. A fight's light lights the rooms linked
/// with its own.
pub(crate) fn linked(rooms: &[RoomView], a: usize, b: usize) -> bool {
    a == b || lists(rooms, a, b) || lists(rooms, b, a) || seen_only(&rooms[b])
}

/// What a scene's rooms give a fight's light, worked out once per scene.
#[derive(Default)]
pub(crate) struct FightRooms {
    /// For each room, the rooms a light in it does not reach, as
    /// `GpuLight::unseen`'s bits.
    unseen: Vec<UVec4>,
    /// Each room's walkable floor: its lowest and highest corner, game
    /// space. `None` for a room with no floor.
    floors: Vec<Option<(Vec3, Vec3)>>,
    /// Each room's model's lowest and highest corner, game space.
    bounds: Vec<Option<(Vec3, Vec3)>>,
}

impl FightRooms {
    /// The masks and bounds of `rooms`, whose models span `bounds`.
    pub(crate) fn new(rooms: &[RoomView], bounds: &[Option<(Vec3, Vec3)>]) -> FightRooms {
        let unseen = (0..rooms.len())
            .map(|here| {
                let mut bits = [0u32; 4];
                for room in 0..rooms.len().min(VIS_ROOMS) {
                    if !linked(rooms, here, room) {
                        bits[room / 32] |= 1 << (room % 32);
                    }
                }
                UVec4::from_array(bits)
            })
            .collect();
        let floors = rooms
            .iter()
            .map(|room| {
                room.floor
                    .iter()
                    .flatten()
                    .map(|corner| Vec3::from_array(*corner))
                    .fold(None, |span, corner| match span {
                        None => Some((corner, corner)),
                        Some((low, high)) => Some((corner.min(low), corner.max(high))),
                    })
            })
            .collect();
        FightRooms {
            unseen,
            floors,
            bounds: bounds.to_vec(),
        }
    }

    /// The rooms a fight's light in room `room` does not reach. A room the
    /// scene does not have reaches everything.
    pub(crate) fn unseen(&self, room: usize) -> UVec4 {
        self.unseen.get(room).copied().unwrap_or(UVec4::ZERO)
    }

    /// The room a fight's light at `at` (game space) is in. That is the
    /// room whose floor is under it: the highest floor at its x and y that
    /// lies no more than `FLOOR_ABOVE_LIGHT` above it and no more than
    /// `FLOOR_BELOW_LIGHT` below it. Over no such floor (a pit, a table, a
    /// ledge off the walkmesh), it is the smallest room whose model's
    /// bounds hold the light. Outside every room it is in none, and then
    /// it reaches every room.
    pub(crate) fn room_of(&self, rooms: &[RoomView], at: Vec3) -> Option<usize> {
        let mut best: Option<(usize, f32)> = None;
        for (index, room) in rooms.iter().enumerate() {
            let Some(Some((low, high))) = self.floors.get(index) else {
                continue;
            };
            let over = at.x >= low.x && at.x <= high.x && at.y >= low.y && at.y <= high.y;
            if !over || low.z > at.z + FLOOR_ABOVE_LIGHT || high.z < at.z - FLOOR_BELOW_LIGHT {
                continue;
            }
            for [a, b, c] in &room.floor {
                let Some(z) = crate::scene::height_at(*a, *b, *c, at.x, at.y) else {
                    continue;
                };
                let near = z <= at.z + FLOOR_ABOVE_LIGHT && z >= at.z - FLOOR_BELOW_LIGHT;
                if near && best.is_none_or(|(_, top)| z > top) {
                    best = Some((index, z));
                }
            }
        }
        if let Some((index, _)) = best {
            return Some(index);
        }
        let volume = |(low, high): (Vec3, Vec3)| (high - low).max(Vec3::ZERO).element_product();
        self.bounds
            .iter()
            .enumerate()
            .filter_map(|(index, bounds)| Some((index, (*bounds)?)))
            .filter(|(_, (low, high))| at.cmpge(*low).all() && at.cmple(*high).all())
            .min_by(|a, b| volume(a.1).total_cmp(&volume(b.1)))
            .map(|(index, _)| index)
    }
}

#[cfg(feature = "probes")]
pub mod probes;
