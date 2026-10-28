//! The area's map in the world: its waypoints' notes, which scripts show
//! and hide (`SetMapPinEnabled`, `snapshot::pin`), and the party exploring
//! it (`crate::fog`): each step every party member, a seat's player or a
//! companion following or driven, explores the cells near it.

use bevy_ecs::prelude::*;

use super::{Avatar, Fighting, Module, Player, Static};

#[cfg(feature = "probes")]
mod probes;
#[cfg(feature = "probes")]
pub use probes::probes;

pub(super) fn explore(
    level: Res<Static>,
    mut area: ResMut<Module>,
    members: Query<(&Player, Option<&Avatar>, Option<&Fighting>)>,
) {
    let Some(module) = level.0.module() else {
        return;
    };
    let Some(chart) = module.map else {
        return;
    };
    for (player, avatar, fighting) in &members {
        // A seat's player, or a companion following or driven.
        let member = match fighting {
            Some(fighting) => {
                super::in_party(avatar, &fighting.0) || fighting.0.mind.driver.is_some()
            }
            None => avatar.is_some(),
        };
        if member {
            let [x, y, _] = crate::math::scale(player.0.origin, collision::UNIT);
            crate::fog::explore(&mut area.explored, &module.name, &chart, [x, y]);
        }
    }
}
