//! Whether a player gets from one floor to another by a jump or a drop
//! (`link_lands`), moved as the containment audit moves one
//! (`audit`, which only checking builds have): up, across, then down. The
//! level editor checks its links between floors this way
//! (`nav::Link`).

use crate::player::{
    BASE_SPEED, GRAVITY, HALF_WIDTH, SOLE_Z, STAND_HEIGHT, STEP_HEIGHT,
    WALKABLE_NORMAL_Z as WALK_NORMAL,
};
use crate::{CollisionWorld, Contents, Extents, UNIT};

/// JA's step, metres.
const STEP: f32 = STEP_HEIGHT * UNIT;
/// The lift crouching in the air adds, units.
const CROUCH_LIFT: f32 = 16.0;

/// How high (metres) the feet get onto a ledge, and how far (metres) a jump
/// `height` units high carries (JA's `forceJumpHeight`: 32 for a plain
/// jump), as `audit::Reach::jump` reckons them.
pub fn jump_reach(height: f32) -> (f32, f32) {
    let airtime = 2.0 * (2.0 * height / GRAVITY).sqrt();
    (
        (height + STEP_HEIGHT + CROUCH_LIFT) * UNIT,
        BASE_SPEED * airtime * UNIT,
    )
}

/// A plain jump's height (units), the one every character makes.
pub const PLAIN_JUMP: f32 = 32.0;

/// The player's standing box (JA's, 30 × 30 × 64 units), metres.
pub fn player_box() -> Extents {
    Extents {
        mins: [-HALF_WIDTH * UNIT, -HALF_WIDTH * UNIT, SOLE_Z * UNIT],
        maxs: [HALF_WIDTH * UNIT, HALF_WIDTH * UNIT, STAND_HEIGHT * UNIT],
    }
}

/// How far (metres) under or over a link's end a jump or a drop may land
/// and still land there (`link_lands`), and how much higher than the
/// higher end its arc clears a lip.
const LANDS_WITHIN: f32 = 0.5;
const LIP: f32 = 0.3;

/// Whether a player standing with its feet at `from` (metres) gets to stand
/// at `to` by a jump `height` units high, or by stepping off and falling (`drop`), as
/// the audit moves a player: up as high as it needs (a drop rises a step
/// over the edge's lip), across at that height, then down. The box is the
/// player's standing box, and the solids are a player's. Why not, when it
/// does not: the landing too high or too far for the reach, something in
/// the way, nothing to land on, or a landing off the link's end.
pub fn link_lands(
    world: &dyn CollisionWorld,
    (from, to): ([f32; 3], [f32; 3]),
    height: f32,
    drop: bool,
) -> Result<(), String> {
    let (ledge, distance) = jump_reach(height);
    let player = player_box();
    let mask = Contents::PLAYER_SOLID;
    let origin = |feet: [f32; 3]| [feet[0], feet[1], feet[2] - SOLE_Z * UNIT + 0.01];
    let flat = ((to[0] - from[0]).powi(2) + (to[1] - from[1]).powi(2)).sqrt();
    let rise = to[2] - from[2];
    if drop && rise > STEP {
        return Err(format!(
            "a drop goes down, and its landing is {rise:.2} m higher than where it leaves"
        ));
    }
    if !drop && rise > ledge {
        return Err(format!(
            "the landing is {rise:.2} m higher, and a jump reaches {ledge:.2} m"
        ));
    }
    if !drop && flat > distance {
        return Err(format!(
            "the landing is {flat:.2} m away, and a jump carries {distance:.2} m"
        ));
    }
    let trace = |start: [f32; 3], end: [f32; 3]| world.trace(start, end, &player, mask);
    let start = origin(from);
    let lift = if drop { STEP } else { rise.max(0.0) + LIP };
    let up = trace(start, [start[0], start[1], start[2] + lift]);
    if up.start_solid {
        return Err("the player does not fit where the link leaves".into());
    }
    if up.fraction < 1.0 {
        return Err(format!(
            "a ceiling {:.2} m over where the link leaves stops the jump",
            up.end[2] - start[2]
        ));
    }
    let over = [to[0], to[1], up.end[2]];
    let across = trace(up.end, over);
    if across.fraction < 1.0 {
        return Err(format!(
            "something stands in the way {:.2} m along",
            across.fraction * flat
        ));
    }
    let landing = origin(to);
    let down = trace(over, [over[0], over[1], landing[2] - 2.0]);
    if down.fraction >= 1.0 {
        return Err("there is nothing to land on within 2 m under the link's end".into());
    }
    if down.plane.normal[2] < WALK_NORMAL {
        return Err("the landing is too steep to stand on".into());
    }
    let off = down.end[2] - landing[2];
    if off.abs() > LANDS_WITHIN {
        return Err(format!(
            "it lands {:.2} m {} the link's end",
            off.abs(),
            if off > 0.0 { "over" } else { "under" }
        ));
    }
    Ok(())
}
