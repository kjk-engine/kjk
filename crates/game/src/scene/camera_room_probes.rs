//! Checks of the rooms the camera is in for the room culling, on rooms
//! made here (no install): over a floor, against the wall between two
//! rooms, inside a sky's bounds only, outside every room, and with the
//! controlled character in another room than the camera.

use bevy::prelude::*;

use super::Culling;
use crate::probe_kit::{Checks, check};
use crate::viewer::RoomView;

/// The checks, for `camera_rooms` in the approved tests.
pub fn probes() -> Checks {
    vec![
        ("over_a_floor", over_a_floor()),
        ("against_a_wall", against_a_wall()),
        ("inside_a_sky_only", inside_a_sky_only()),
        ("outside_every_room", outside_every_room()),
        ("controlled_in_another_room", controlled_in_another_room()),
    ]
}

/// Room a's VIS lists b, b's lists a and c, and c's lists b. Each room has
/// a floor from 1 m to 9 m across its 10 m width in x (a 1 m strip under
/// each wall has no walkable floor) and spans 0 to 4 m in z. Room d is a
/// sky with no floor whose bounds hold the whole area.
fn rooms() -> (Vec<RoomView>, Culling) {
    let room = |name: &str, visible: &[&str], x0: f32| {
        let floor = if visible.is_empty() {
            Vec::new()
        } else {
            let [a, b, c, d] = [
                [x0 + 1.0, 1.0, 0.0],
                [x0 + 9.0, 1.0, 0.0],
                [x0 + 9.0, 9.0, 0.0],
                [x0 + 1.0, 9.0, 0.0],
            ];
            vec![[a, b, c], [a, c, d]]
        };
        RoomView {
            name: name.into(),
            model: None,
            visible: visible.iter().map(|name| name.to_string()).collect(),
            sees_all: false,
            floor,
            floor_error: None,
        }
    };
    let rooms = vec![
        room("a", &["b"], 0.0),
        room("b", &["a", "c"], 10.0),
        room("c", &["b"], 20.0),
        room("d", &[], 0.0),
    ];
    let span = |x0: f32| Some((Vec3::new(x0, 0.0, 0.0), Vec3::new(x0 + 10.0, 10.0, 4.0)));
    let culling = Culling {
        bounds: vec![
            span(0.0),
            span(10.0),
            span(20.0),
            Some((Vec3::splat(-500.0), Vec3::splat(500.0))),
        ],
        ..default()
    };
    (rooms, culling)
}

/// The rooms that show to an eye at `eye`, without fog, from the rooms
/// `culling` remembers the controlled character in.
fn shown(rooms: &[RoomView], culling: &Culling, eye: Vec3) -> Vec<usize> {
    let here = culling.shown_from(culling.eye_rooms(rooms, eye));
    (0..rooms.len())
        .filter(|&room| culling.room_visible(rooms, here.as_deref(), eye, f32::INFINITY, room))
        .collect()
}

/// An eye 1.6 m over room a's floor is in a. Rooms a and b show, and the
/// sky d, but not c, which only b sees.
fn over_a_floor() -> Result<String, String> {
    let (rooms, culling) = rooms();
    let eye = Vec3::new(5.0, 5.0, 1.6);
    let here = culling.eye_rooms(&rooms, eye);
    let shown = shown(&rooms, &culling, eye);
    check(
        here == Some(vec![0]) && shown == [0, 1, 3],
        format!("in {here:?}, shown {shown:?}; wanted in [0], shown [0, 1, 3]"),
    )
}

/// An eye against the wall between rooms b and c, 0.2 m short of it on b's
/// side, has no floor under it. It is in b and in c, whose bounds it is
/// within 0.5 m of. Every room shows, because b sees a. An eye against
/// room a's far wall, over no floor, is in a only, and c stays hidden.
fn against_a_wall() -> Result<String, String> {
    let (rooms, culling) = rooms();
    let between = Vec3::new(19.8, 5.0, 1.6);
    let here = culling.eye_rooms(&rooms, between);
    let shown_between = shown(&rooms, &culling, between);
    let far = Vec3::new(0.3, 5.0, 1.6);
    let here_far = culling.eye_rooms(&rooms, far);
    let shown_far = shown(&rooms, &culling, far);
    check(
        here == Some(vec![1, 2])
            && shown_between == [0, 1, 2, 3]
            && here_far == Some(vec![0])
            && shown_far == [0, 1, 3],
        format!(
            "between b and c in {here:?}, shown {shown_between:?}; \
             at a's far wall in {here_far:?}, shown {shown_far:?}; \
             wanted [1, 2] showing all four and [0] showing [0, 1, 3]"
        ),
    )
}

/// An eye 50 m above the area is within only the sky's bounds. The sky
/// has no floor and does not count, so the eye is in no room and every
/// room shows.
fn inside_a_sky_only() -> Result<String, String> {
    let (rooms, culling) = rooms();
    let eye = Vec3::new(-30.0, 5.0, 50.0);
    let here = culling.eye_rooms(&rooms, eye);
    let shown = shown(&rooms, &culling, eye);
    check(
        here.is_none() && shown == [0, 1, 2, 3],
        format!("in {here:?}, shown {shown:?}; wanted no room and all four shown"),
    )
}

/// An eye outside every room's floor and bounds, sky included, is in no
/// room, and every room shows.
fn outside_every_room() -> Result<String, String> {
    let (rooms, culling) = rooms();
    let eye = Vec3::new(-900.0, 0.0, 0.0);
    let here = culling.eye_rooms(&rooms, eye);
    let shown = shown(&rooms, &culling, eye);
    check(
        here.is_none() && shown == [0, 1, 2, 3],
        format!("in {here:?}, shown {shown:?}; wanted no room and all four shown"),
    )
}

/// The controlled character stands in room c, whose VIS lists only b. The
/// camera is 4 m away over room a's floor, and a lists only b. Rooms a, b
/// and the sky d show from the camera alone, but c would be missing around
/// the controlled character. With the controlled character's room counted,
/// c shows too. After the controlled character steps off every floor, c
/// still shows, because its room is kept. A camera outside every room still
/// shows all four rooms.
fn controlled_in_another_room() -> Result<String, String> {
    let (rooms, mut culling) = rooms();
    let eye = Vec3::new(5.0, 5.0, 1.6);
    let camera_only = shown(&rooms, &culling, eye);
    culling.see_controlled(Some(2), rooms.len());
    let with_controlled = shown(&rooms, &culling, eye);
    let off_floor = culling.see_controlled(None, rooms.len());
    let stale = culling.see_controlled(Some(9), rooms.len());
    let kept = shown(&rooms, &culling, eye);
    let outside = shown(&rooms, &culling, Vec3::new(-900.0, 0.0, 0.0));
    check(
        camera_only == [0, 1, 3]
            && with_controlled == [0, 1, 2, 3]
            && !off_floor
            && !stale
            && kept == [0, 1, 2, 3]
            && outside == [0, 1, 2, 3],
        format!(
            "camera alone showed {camera_only:?}, with the controlled character in c {with_controlled:?}, \
             kept {kept:?}, outside {outside:?}; wanted [0, 1, 3], then all four three times"
        ),
    )
}
