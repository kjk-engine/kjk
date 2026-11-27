//! An area's sun (`level_edit::area::SunLight`, `docs/LEVEL-EDIT.md`, "The
//! sun"): the light of a room's model that reaches `shadows::SUN_RADIUS`
//! or more, far over the level, whose shadows the sun's own tiles cast
//! (`shadows`). A level editor project may set it, in place of the game's.
//!
//! An area's sun as set is, in this order: the one a program set for it
//! (`set_sun`, the level editor's view before it saves), then the active
//! project's (`level_edit::active`). The scene's lights take it in place of
//! the game's sun, or as a sun of its own in an area that has none
//! (`scene`).

use std::collections::BTreeMap;

use bevy::math::Vec3;
use level_edit::area::SunLight;

use crate::scene::{placement, to_mat4};
use crate::viewer::Placed;

/// The suns a program sets for an area, by the area's name in lower case:
/// `None` sets none (the game's shows).
static SET: std::sync::RwLock<BTreeMap<String, Option<SunLight>>> =
    std::sync::RwLock::new(BTreeMap::new());

/// Gives the area `area` this sun in place of the active project's
/// (`Some`), or the project's again (`None`). The level editor sets the sun
/// it edits, so its view shows it before it is saved.
pub fn set_sun(area: &str, sun: Option<Option<SunLight>>) {
    let mut set = SET
        .write()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    match sun {
        Some(sun) => {
            set.insert(area.to_ascii_lowercase(), sun);
        }
        None => {
            set.remove(&area.to_ascii_lowercase());
        }
    }
}

/// The sun set for `game`'s area of `module`, if one is.
pub fn of_area(game: vfs::Game, module: &str) -> Option<SunLight> {
    if let Some(set) = SET
        .read()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .get(&module.to_ascii_lowercase())
    {
        return *set;
    }
    level_edit::active::kjk_of(game.key(), module).and_then(|kjk| kjk.sun)
}

/// The game's sun among `models` (game space): the first light of a model
/// that reaches `shadows::SUN_RADIUS` or more and lights more than its
/// ambient, as the scene gathers its lights.
pub fn found(models: &[Placed]) -> Option<SunLight> {
    for placed in models {
        let frame = placement(placed);
        for light in &placed.ir.lights {
            if light.radius < crate::shadows::SUN_RADIUS || light.ambient_only {
                continue;
            }
            let local = to_mat4(&placed.ir.nodes[light.node].world).transform_point3(Vec3::ZERO);
            return Some(SunLight {
                at: frame.transform_point3(local).to_array(),
                colour: light.color,
                multiplier: light.multiplier,
                radius: light.radius,
            });
        }
    }
    None
}
