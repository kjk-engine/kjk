//! The combat arena (`kjk play --stage arena`): a walled grey-box floor
//! 36 m square to try fights and Force powers on, with cover, pillars, a
//! ramp up to a ledge, a stack of crates and a pit to fall into (a body
//! under `PIT_FLOOR` dies; `sim::world::arena`). It runs on K1's data (the
//! scripts, creature templates, items and tables of `BASE_MODULE`'s plan)
//! with none of that module's own objects, area or scripts; `--stage
//! arena:MODULE` lends another module's (TSL's too), so its own creature
//! templates can be called in (`arena spawn TEMPLATE`).
//!
//! The player comes in as a level-20 Jedi who knows every K1 Force power
//! (`sheet`), carries every hand weapon K1 has (`weapons`), and is back on
//! its feet at the start `RESPAWN_MS` after going down. Keys
//! (`ArenaPlugin`): F2 calls in the chosen foe ahead of the player, F3
//! chooses the next in `ROSTER`, F4 turns god mode on or off, F6 takes
//! every foe away; a line at the top of the window says so.

use std::sync::Arc;

use bevy::prelude::*;
use collision::{Brush, BrushWorld};
use sim::action::{Action, ArenaOrder};
use sim::combat::Sheet;

/// The module whose plan lends the arena K1's data.
pub const BASE_MODULE: &str = "end_m01aa";
/// Half the floor's side (metres), and its walls' height.
const HALF: f32 = 18.0;
const WALL: f32 = 4.0;
/// The pit: its opening (x, y from and to), how deep its shaft goes, and
/// the height a body has fallen past when it dies there.
const PIT: [[f32; 2]; 2] = [[7.0, 5.0], [13.0, 11.0]];
const PIT_DEPTH: f32 = 20.0;
pub const PIT_FLOOR: f32 = -12.0;
/// Where the player starts and comes back, facing +Y.
pub const SPAWN: [f32; 3] = [0.0, -12.0, 0.0];
pub const YAW: f32 = 90.0;
pub const RESPAWN_MS: u64 = 3000;

/// What stands on the floor (min, max corners, metres): cover to crouch
/// behind, pillars, crates, and the ledge with its ramp (`RAMP`).
const SOLIDS: [([f32; 3], [f32; 3]); 11] = [
    // Low cover.
    ([-7.5, -3.0, 0.0], [-4.5, -2.5, 1.2]),
    ([4.5, -5.0, 0.0], [7.5, -4.5, 1.2]),
    ([-1.5, 3.0, 0.0], [1.5, 3.5, 1.2]),
    // Pillars.
    ([-10.5, 5.5, 0.0], [-9.5, 6.5, WALL]),
    ([-10.5, 11.5, 0.0], [-9.5, 12.5, WALL]),
    ([-4.5, 11.5, 0.0], [-3.5, 12.5, WALL]),
    ([2.5, 9.5, 0.0], [3.5, 10.5, WALL]),
    // Crates, two stacked.
    ([11.0, -11.0, 0.0], [12.2, -9.8, 1.2]),
    ([12.4, -11.0, 0.0], [13.6, -9.8, 1.2]),
    ([11.7, -11.0, 1.2], [12.9, -9.8, 2.4]),
    // The ledge.
    ([-HALF, -8.0, 0.0], [-11.0, 0.0, LEDGE]),
];
/// The ledge's height, and the ramp up to it from the south.
const LEDGE: f32 = 1.5;
const RAMP: ([f32; 3], [f32; 3]) = ([-HALF, -14.0, 0.0], [-11.0, -8.0, LEDGE]);

/// The arena's collision.
pub fn collision() -> BrushWorld {
    let cuboid = Brush::cuboid;
    let [[px0, py0], [px1, py1]] = PIT;
    let floor = [-HALF, -HALF, -1.0];
    let mut world = BrushWorld::new()
        // The floor, round the pit's opening.
        .with(cuboid(floor, [px0, HALF, 0.0]))
        .with(cuboid([px1, -HALF, -1.0], [HALF, HALF, 0.0]))
        .with(cuboid([px0, -HALF, -1.0], [px1, py0, 0.0]))
        .with(cuboid([px0, py1, -1.0], [px1, HALF, 0.0]))
        // The pit's shaft and its bottom.
        .with(cuboid([px0 - 0.5, py0, -PIT_DEPTH], [px0, py1, -1.0]))
        .with(cuboid([px1, py0, -PIT_DEPTH], [px1 + 0.5, py1, -1.0]))
        .with(cuboid([px0, py0 - 0.5, -PIT_DEPTH], [px1, py0, -1.0]))
        .with(cuboid([px0, py1, -PIT_DEPTH], [px1, py1 + 0.5, -1.0]))
        .with(cuboid([px0, py0, -PIT_DEPTH - 1.0], [px1, py1, -PIT_DEPTH]))
        // The walls.
        .with(cuboid([-HALF - 0.5, -HALF, 0.0], [-HALF, HALF, WALL]))
        .with(cuboid([HALF, -HALF, 0.0], [HALF + 0.5, HALF, WALL]))
        .with(cuboid([-HALF - 0.5, -HALF - 0.5, 0.0], [HALF + 0.5, -HALF, WALL]))
        .with(cuboid([-HALF - 0.5, HALF, 0.0], [HALF + 0.5, HALF + 0.5, WALL]))
        .with(Brush::ramp(RAMP.0, RAMP.1, 1, false));
    for (min, max) in SOLIDS {
        world = world.with(cuboid(min, max));
    }
    world
}

/// The floor's walkable faces as a navigation mesh: half-metre squares of
/// the floor that no solid stands on and that are not over the pit, the
/// ledge's top and the ramp up to it.
pub fn nav() -> collision::nav::NavMesh {
    const CELL: f32 = 0.5;
    let inside = |[x, y]: [f32; 2], (min, max): ([f32; 3], [f32; 3])| {
        x > min[0] && x < max[0] && y > min[1] && y < max[1]
    };
    let [[px0, py0], [px1, py1]] = PIT;
    let pit = ([px0, py0, 0.0], [px1, py1, 0.0]);
    let cells = (2.0 * HALF / CELL) as i32;
    let mut triangles = Vec::new();
    let mut quad = |[x0, y0, z0]: [f32; 3], [x1, y1, z1]: [f32; 3]| {
        let [a, b, c, d] = [[x0, y0, z0], [x1, y0, z0], [x1, y1, z1], [x0, y1, z1]];
        triangles.push([a, b, c]);
        triangles.push([a, c, d]);
    };
    for i in 0..cells {
        for j in 0..cells {
            let (x, y) = (-HALF + i as f32 * CELL, -HALF + j as f32 * CELL);
            let centre = [x + CELL / 2.0, y + CELL / 2.0];
            let blocked = SOLIDS
                .iter()
                .chain([&RAMP, &pit])
                .any(|solid| inside(centre, *solid));
            if !blocked {
                quad([x, y, 0.0], [x + CELL, y + CELL, 0.0]);
            }
        }
    }
    let (ledge_min, ledge_max) = SOLIDS[SOLIDS.len() - 1];
    quad(
        [ledge_min[0], ledge_min[1], LEDGE],
        [ledge_max[0], ledge_max[1], LEDGE],
    );
    // The ramp rises along +Y.
    let ([rx0, ry0, _], [rx1, ry1, _]) = RAMP;
    quad([rx0, ry0, 0.0], [rx1, ry1, LEDGE]);
    collision::nav::NavMesh::build(&triangles, sim::walk::CLEARANCE * collision::UNIT)
}

/// The arena's rules for the sim.
pub fn rules() -> sim::arena::Rules {
    sim::arena::Rules {
        respawn: SPAWN,
        yaw: YAW,
        pit: PIT_FLOOR,
        respawn_ms: RESPAWN_MS,
    }
}

/// `base` (`BASE_MODULE`'s level) as the arena: its scripts, templates and
/// tables kept, its objects, area and scripts gone, on the arena's floor.
pub fn level(base: &sim::Level, anims: Arc<sim::motion::AnimTable>) -> Result<sim::Level, String> {
    let mut data = base
        .module()
        .ok_or("the arena's base has no module")?
        .as_ref()
        .clone();
    data.placements.clear();
    data.module_scripts.clear();
    data.area_scripts.clear();
    data.map = None;
    data.entry = sim::module::Spot {
        position: SPAWN,
        yaw: YAW,
    };
    let library = base
        .scripts()
        .ok_or("the arena's base has no scripts")?
        .clone();
    Ok(sim::Level::new("arena", Arc::new(collision()), anims)
        .with_module(data)
        .with_nav(nav())
        .with_scripts(library)
        .with_arena(rules()))
}

/// The foes F2 calls in, by name and K1 template, in F3's order.
pub const ROSTER: [(&str, &str); 11] = [
    ("Sith Trooper", "g_sithtroop01"),
    ("Sith Trooper (elite)", "g_sithtroop05"),
    ("Dark Jedi", "g_darkjedi01"),
    ("Dark Jedi (veteran)", "g_darkjedi03"),
    ("Mandalorian", "g_mandalor01"),
    ("Black Vulkar thug", "g_blkvulkgang01"),
    ("War Droid", "g_wardroid01"),
    ("Assassin Droid", "g_assassindrd01"),
    ("Kath Hound", "g_kathhound01"),
    ("Rakghoul", "g_rakghoul01"),
    ("Rancor", "g_rancor01"),
];

/// The arena's player: the dev Jedi at level 20, a deep pool, every K1
/// power known and no other, proficient with every weapon it carries
/// (Proficiency All) and knowing the guns' feat attacks to their tops (Power
/// Blast, Rapid Shot, Sniper Shot: `combat::specials`), a saber in the right
/// hand and nothing in the left (`SABER` replaces it at the start, `play`).
pub fn sheet() -> Sheet {
    use sim::combat::sheet::{Rows, feats};
    let powers = sim::combat::spells::K1_FORCE_POWERS
        .iter()
        .fold(Rows::default(), |known, row| known.with(*row));
    let jedi = Sheet::dev_jedi();
    let gun_feats = [feats::POWER_BLAST, feats::RAPID_SHOT, feats::SNIPER_SHOT];
    let known = gun_feats
        .iter()
        .flatten()
        .fold(jedi.feats.with(feats::PROFICIENCY_ALL), |known, row| {
            known.with(*row)
        });
    Sheet {
        level: 20,
        bab: 20,
        max_hp: 200,
        max_fp: 300,
        powers,
        feats: known,
        weapons: [jedi.weapons[0], None],
        ..jedi
    }
}

/// The saber the arena's player holds at the start: an item, so the equip
/// screen takes it off and swaps it as any other (K1's blue lightsaber).
pub const SABER: &str = "g_w_lghtsbr01";

/// Every hand weapon K1 has (`library`'s item templates a right hand
/// holds, but plot items), by template: what the arena's player carries,
/// one of each, two of a gun the left hand takes too (a pistol, for a pair).
pub fn weapons(library: &sim::script::Library) -> Vec<(String, u16)> {
    use sim::inventory::slots::{LEFT_WEAPON, RIGHT_WEAPON};
    library
        .items()
        .iter()
        .filter(|item| item.weapon.is_some() && item.slots & 1 << RIGHT_WEAPON != 0 && !item.plot)
        .map(|item| {
            let gun = item.weapon.is_some_and(|weapon| weapon.delivery.ranged());
            let pistol = gun && item.slots & 1 << LEFT_WEAPON != 0;
            (item.template.clone(), 1 + u16::from(pistol))
        })
        .collect()
}

/// The arena's keys and its line at the top of the window.
pub struct ArenaPlugin;

/// The foe F2 calls in (`ROSTER`'s index).
#[derive(Resource, Default)]
struct Chosen {
    foe: usize,
}

impl Plugin for ArenaPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Chosen>().add_systems(Update, keys);
    }
}

/// What the arena's keys read of play: the level, what is shown of it and
/// whose view it is.
type Shown<'w> = (
    Option<Res<'w, crate::sim_host::SceneSwap>>,
    Option<Res<'w, crate::sim_host::PresentedSnapshot>>,
    Option<Res<'w, crate::sim_host::OwnSeat>>,
);

/// Where the arena's orders go: the actions sent to the sim, and the
/// console's orders taken.
type Orders<'w> = (
    Option<ResMut<'w, crate::loot::Outgoing>>,
    Option<ResMut<'w, console::ArenaRequests>>,
);

/// F2 to F6 (see the module documentation), only on the arena's level;
/// the line says whether the player's body is in god mode now.
fn keys(
    keys: Option<Res<ButtonInput<KeyCode>>>,
    (swap, presented, own): Shown,
    mut chosen: ResMut<Chosen>,
    (mut outgoing, mut requests): Orders,
    mut ui: Option<ResMut<crate::ui_view::Ui>>,
) {
    let Some(swap) = swap.filter(|swap| swap.level.arena_rules().is_some()) else {
        return;
    };
    // The console's orders (`arena spawn TEMPLATE`, `arena clear`, `arena
    // god`), as the keys give them; `arena down N` (no key).
    let asked = requests.as_mut().map_or_else(Vec::new, |requests| {
        std::mem::take(&mut requests.0)
            .into_iter()
            .filter_map(|words| match words.as_slice() {
                [spawn, template] if spawn == "spawn" => Some(ArenaOrder::Spawn(template.clone())),
                [clear] if clear == "clear" => Some(ArenaOrder::Clear),
                [god] if god == "god" => Some(ArenaOrder::God),
                [down, n] if down == "down" => n.parse().ok().map(ArenaOrder::Down),
                _ => None,
            })
            .collect()
    });
    if let Some(outgoing) = outgoing.as_mut() {
        outgoing.0.extend(asked.into_iter().map(Action::Arena));
    }
    let god = presented.zip(own).is_some_and(|(presented, own)| {
        crate::cast::body_sheet(presented.0.current(), own.0, &swap.level)
            .is_some_and(|(_, sheet)| sheet.flags & sim::combat::sheet::flags::IMMORTAL != 0)
    });
    if let Some(keys) = keys {
        let mut order = |order: ArenaOrder| {
            if let Some(outgoing) = outgoing.as_mut() {
                outgoing.0.push(Action::Arena(order));
            }
        };
        if keys.just_pressed(KeyCode::F2) {
            order(ArenaOrder::Spawn(ROSTER[chosen.foe].1.to_string()));
        }
        if keys.just_pressed(KeyCode::F3) {
            chosen.foe = (chosen.foe + 1) % ROSTER.len();
        }
        if keys.just_pressed(KeyCode::F4) {
            order(ArenaOrder::God);
        }
        if keys.just_pressed(KeyCode::F6) {
            order(ArenaOrder::Clear);
        }
    }
    if let Some(ui) = ui.as_mut() {
        let line = format!(
            "ARENA   F2 spawn {}   F3 next foe   F4 god mode {}   F6 clear   [ ] pick power, F cast",
            ROSTER[chosen.foe].0,
            if god { "ON" } else { "off" },
        );
        ui.vm.arena = Some(line.into_bytes());
    }
}
