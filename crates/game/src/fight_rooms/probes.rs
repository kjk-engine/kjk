//! Checks of the rooms a fight's light reaches, on rooms made here (no
//! install): which rooms are linked, the masks' bits, and the room a light
//! is found in over stacked floors, over a pit and outside every room.

use bevy::prelude::*;

use super::{FightRooms, linked};
use crate::probe_kit::{Checks, check};
use crate::viewer::RoomView;

/// The checks, for `fight_light_rooms` in the approved tests.
pub fn probes() -> Checks {
    vec![
        ("linked_rooms", linked_rooms()),
        ("unseen_bits", unseen_bits()),
        ("past_the_mask", past_the_mask()),
        ("stacked_floors", stacked_floors()),
        ("over_a_pit", over_a_pit()),
        ("outside_every_room", outside_every_room()),
    ]
}

/// A room named `name` whose VIS lists `visible`, with a floor of two
/// triangles over the square from `low` to `high` (x and y) at height `z`.
/// No floor when `z` is `None`.
fn room(name: &str, visible: &[&str], square: Option<(Vec2, Vec2, f32)>) -> RoomView {
    let floor = square.map_or_else(Vec::new, |(low, high, z)| {
        let [a, b, c, d] = [
            [low.x, low.y, z],
            [high.x, low.y, z],
            [high.x, high.y, z],
            [low.x, high.y, z],
        ];
        vec![[a, b, c], [a, c, d]]
    });
    RoomView {
        name: name.into(),
        model: None,
        visible: visible.iter().map(|name| name.to_string()).collect(),
        floor,
        floor_error: None,
    }
}

/// A square floor from (x0, y0) to (x1, y1) at height z.
fn square(x0: f32, y0: f32, x1: f32, y1: f32, z: f32) -> Option<(Vec2, Vec2, f32)> {
    Some((Vec2::new(x0, y0), Vec2::new(x1, y1), z))
}

/// Room a lists b in its VIS; c lists nothing; d is only seen (no floor);
/// e has no floor because its walkmesh did not read.
fn rooms() -> Vec<RoomView> {
    let mut unread = room("e", &[], None);
    unread.floor_error = Some("unreadable".into());
    vec![
        room("a", &["B"], square(0.0, 0.0, 10.0, 10.0, 0.0)),
        room("b", &[], square(10.0, 0.0, 20.0, 10.0, 0.0)),
        room("c", &[], square(20.0, 0.0, 30.0, 10.0, 0.0)),
        room("d", &[], None),
        unread,
    ]
}

/// A link one way counts both ways, a VIS name matches in any case, a room
/// only seen is linked with every room, and a room whose floor did not read
/// is not taken for one only seen.
fn linked_rooms() -> Result<String, String> {
    let rooms = rooms();
    let pairs = [
        ((0, 0), true),
        ((0, 1), true),
        ((1, 0), true),
        ((0, 2), false),
        ((2, 0), false),
        ((2, 3), true),
        ((0, 4), false),
    ];
    let wrong: Vec<_> = pairs
        .iter()
        .filter(|((a, b), want)| linked(&rooms, *a, *b) != *want)
        .collect();
    check(wrong.is_empty(), format!("pairs judged wrong: {wrong:?}"))
}

/// A light in room a does not reach c or e, and reaches a, b and d.
fn unseen_bits() -> Result<String, String> {
    let rooms = rooms();
    let fight = FightRooms::new(&rooms, &vec![None; rooms.len()]);
    let mask = fight.unseen(0);
    let want = UVec4::new(1 << 2 | 1 << 4, 0, 0, 0);
    check(
        mask == want,
        format!("room a's mask {mask:?}, wanted {want:?}"),
    )
}

/// In an area of 130 rooms where no VIS lists another, a light in room 0
/// is kept from rooms 1 to 127 (the bits of x to w), and rooms 128 and 129
/// are past the mask, so it reaches them.
fn past_the_mask() -> Result<String, String> {
    let rooms: Vec<RoomView> = (0..130)
        .map(|i| {
            room(
                &format!("r{i}"),
                &[],
                square(i as f32, 0.0, i as f32 + 1.0, 1.0, 0.0),
            )
        })
        .collect();
    let fight = FightRooms::new(&rooms, &vec![None; rooms.len()]);
    let mask = fight.unseen(0);
    let want = UVec4::new(!1, u32::MAX, u32::MAX, u32::MAX);
    check(
        mask == want,
        format!("room 0's mask {mask:?}, wanted {want:?}"),
    )
}

/// A floor at 0 m under a floor at 3 m. A light 1.2 m over the lower floor
/// is in the lower room. A light 0.4 m under the upper floor counts as over
/// it (a bolt skimming a ramp). A light 1.2 m over the upper floor is in
/// the upper room.
fn stacked_floors() -> Result<String, String> {
    let rooms = vec![
        room("low", &[], square(0.0, 0.0, 10.0, 10.0, 0.0)),
        room("high", &[], square(0.0, 0.0, 10.0, 10.0, 3.0)),
    ];
    let fight = FightRooms::new(&rooms, &vec![None; rooms.len()]);
    let found = [1.2, 2.6, 4.2].map(|z| fight.room_of(&rooms, Vec3::new(5.0, 5.0, z)));
    let want = [Some(0), Some(1), Some(1)];
    check(
        found == want,
        format!("rooms found {found:?}, wanted {want:?}"),
    )
}

/// A hall whose model spans 0 to 20 m in x with a floor only at its ends,
/// over a cellar 10 m below its middle. A light in the hall's middle at
/// 1.2 m is over no floor near it. It is in the hall, the smallest room
/// whose bounds hold it, not in the cellar far below.
fn over_a_pit() -> Result<String, String> {
    let mut hall = room("hall", &[], square(0.0, 0.0, 5.0, 10.0, 0.0));
    hall.floor
        .extend(room("", &[], square(15.0, 0.0, 20.0, 10.0, 0.0)).floor);
    let rooms = vec![
        room("cellar", &[], square(0.0, 0.0, 20.0, 10.0, -10.0)),
        hall,
    ];
    let bounds = vec![
        Some((Vec3::new(0.0, 0.0, -10.0), Vec3::new(20.0, 10.0, 10.0))),
        Some((Vec3::new(0.0, 0.0, -1.0), Vec3::new(20.0, 10.0, 5.0))),
    ];
    let fight = FightRooms::new(&rooms, &bounds);
    let found = fight.room_of(&rooms, Vec3::new(10.0, 5.0, 1.2));
    check(
        found == Some(1),
        format!("room found {found:?}, wanted the hall (1)"),
    )
}

/// A light outside every room's floor and bounds is in no room.
fn outside_every_room() -> Result<String, String> {
    let rooms = rooms();
    let bounds = vec![Some((Vec3::ZERO, Vec3::new(10.0, 10.0, 5.0))); rooms.len()];
    let fight = FightRooms::new(&rooms, &bounds);
    let found = fight.room_of(&rooms, Vec3::new(-50.0, -50.0, 1.0));
    check(
        found.is_none(),
        format!("room found {found:?}, wanted none"),
    )
}
