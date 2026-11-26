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
//! (`sheet`), carries every item K1 has (`gear`), and is back on
//! its feet at the start `RESPAWN_MS` after going down. Keys
//! (`ArenaPlugin`): F2 calls in the chosen foe ahead of the player, F3
//! chooses the next in `ROSTER`, F4 turns god mode on or off, F6 takes
//! every foe away; a panel under the minimap says so
//! (`ui::screens::arena_panel`).
//!
//! The sandbox has the Debug Mode terminal too: KOTOR 1's Rakatan computer
//! beside the arrival (`terminal`), which opens a page that changes a
//! party member's class, level, attributes, skills and alignment
//! (`crate::debug_terminal`). The console gives its orders as `arena
//! level`, `ability`, `skill`, `alignment` and `restore` (`console_order`).
//! The arena's player starts with one of every item (`gear`).

use std::sync::Arc;

use bevy::prelude::*;
use collision::{Brush, BrushWorld};
use sim::SimId;
use sim::action::{Action, ArenaOrder};
use sim::combat::Sheet;

/// The module whose plan lends the arena K1's data.
pub const BASE_MODULE: &str = "end_m01aa";
/// The combat sandbox (`kjk play --stage sandbox`, the main menu options'
/// Debug): the arena's Jedi, keys and panel in the Star Forge's viewing
/// platform (Malak's hall), a dark floor lit in pools by its pods, trim and
/// windows, so a fight's lights show; arriving at the player's own mark.
pub const SANDBOX: &str = "sandbox";
pub const SANDBOX_MODULE: &str = "sta_m45ad";
pub const SANDBOX_ARRIVAL: &str = "sta_way_player";
/// The Debug Mode terminal's name, shown when the player aims at it.
pub const TERMINAL_NAME: &str = "Debug Terminal";
/// Where the terminal stands from the player's arrival on the sandbox
/// (metres ahead and to the left of the way the player faces), and how far
/// it turns from facing the way the player faces (degrees).
const TERMINAL_AHEAD: f32 = -2.5;
const TERMINAL_LEFT: f32 = 3.0;
const TERMINAL_TURN: f32 = 180.0;

/// Where the Debug Mode terminal stands and the way it faces, for an
/// arrival at `at` facing `yaw` (`Stage::with_debug_terminal`).
pub fn terminal(at: [f32; 3], yaw: f32) -> ([f32; 3], f32) {
    let (sin, cos) = sim::math::sin_cos_deg(yaw);
    let [x, y, z] = at;
    let place = [
        x + cos * TERMINAL_AHEAD - sin * TERMINAL_LEFT,
        y + sin * TERMINAL_AHEAD + cos * TERMINAL_LEFT,
        z,
    ];
    (place, yaw + TERMINAL_TURN)
}

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
        .with(cuboid([px0, py0, -PIT_DEPTH - 1.0], [px1, py1, -PIT_DEPTH]));
    for (min, max) in WALLS {
        world = world.with(cuboid(min, max));
    }
    world = world.with(Brush::ramp(RAMP.0, RAMP.1, 1, false));
    for (min, max) in SOLIDS {
        world = world.with(cuboid(min, max));
    }
    world
}

/// The arena's walls (min, max corners, metres).
const WALLS: [([f32; 3], [f32; 3]); 4] = [
    ([-HALF - 0.5, -HALF, 0.0], [-HALF, HALF, WALL]),
    ([HALF, -HALF, 0.0], [HALF + 0.5, HALF, WALL]),
    ([-HALF - 0.5, -HALF - 0.5, 0.0], [HALF + 0.5, -HALF, WALL]),
    ([-HALF - 0.5, HALF, 0.0], [HALF + 0.5, HALF + 0.5, WALL]),
];

/// The boxes that hide what is behind them (min, max corners, metres):
/// the walls, what stands on the floor, and the box the ramp fills. A
/// camera placed to film a body checks its view against them.
pub fn sight_blocks() -> impl Iterator<Item = ([f32; 3], [f32; 3])> {
    WALLS.into_iter().chain(SOLIDS).chain([RAMP])
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
        terminal: None,
    }
}

/// `base` (`BASE_MODULE`'s level) as the arena: its scripts, templates and
/// tables kept, its objects, area and scripts gone, on the arena's floor.
pub fn level(base: &sim::Level, anims: Arc<sim::motion::AnimTable>) -> Result<sim::Level, String> {
    on_floor(base, anims, "arena", collision(), nav())
}

/// `base` as an arena named `name` on `world`, walked by `nav` (the
/// arena's own floor, or the floors stage's: `floor_arena`).
pub fn on_floor(
    base: &sim::Level,
    anims: Arc<sim::motion::AnimTable>,
    name: &str,
    world: BrushWorld,
    nav: collision::nav::NavMesh,
) -> Result<sim::Level, String> {
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
    Ok(sim::Level::new(name, Arc::new(world), anims)
        .with_module(data)
        .with_nav(nav)
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

/// The arena's player on `level`'s data (`sheet`), knowing every KOTOR 2
/// power its scripts cast as well (`kotor2_powers`): in a KOTOR 1 level the
/// ones KOTOR 2 lends KOTOR 1, in a KOTOR 2 level all of KOTOR 2's, so each
/// can be tried there.
pub fn sheet_for(level: &sim::Level) -> Sheet {
    let mut sheet = sheet();
    if let Some(library) = level.scripts() {
        sheet.powers = with_level_powers(sheet.powers, library);
    }
    sheet
}

/// `powers` with every KOTOR 2 power `library` casts (`kotor2_powers`), and
/// without the rows its table has no power at (`Library::lacks_power`), so
/// the bar and the wheel show no blank power.
pub fn with_level_powers(
    powers: sim::combat::sheet::Powers,
    library: &sim::script::Library,
) -> sim::combat::sheet::Powers {
    let mut powers = kotor2_powers(library)
        .into_iter()
        .fold(powers, |known, row| known.with(row));
    for row in powers.rows().collect::<Vec<_>>() {
        if library.lacks_power(row) {
            powers.remove(row);
        }
    }
    powers
}

/// The KOTOR 2 Force powers `library` casts: the rows past KOTOR 1's own
/// that a radial menu offers (`forcehostile` or `forcefriendly`) and that
/// KOTOR 2's power script casts (Force Barrier's), and that a screen can
/// show (`Library::lacks_power`). The droids' items have scripts of their
/// own, and the Enemy Battle Meditation rows, which only creatures use,
/// have no icon.
pub fn kotor2_powers(library: &sim::script::Library) -> Vec<u16> {
    use sim::combat::spells::{FORCE_POWER_FORCE_BARRIER, K1_SPELL_ROWS};
    let Some(script) = library
        .impact(FORCE_POWER_FORCE_BARRIER)
        .filter(|s| !s.is_empty())
    else {
        return Vec::new();
    };
    (K1_SPELL_ROWS..sim::combat::sheet::Powers::ROWS)
        .filter(|&row| {
            library.impact(row) == Some(script)
                && library.spell(row).menu != sim::script::Menu::None
                && !library.lacks_power(row)
        })
        .collect()
}

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

/// The saber a two-saber arena player (`--class duelist@dual`) holds in
/// its left hand: another template, so wearing it leaves `SABER` in the right.
pub const LEFT_SABER: &str = "g_w_lghtsbr02";

/// The double-bladed one, for a sheet that holds a staff (`--class
/// duelist@staff`).
pub const STAFF: &str = "g_w_dblsbr001";

/// The sabers the arena's player is handed at the start, as its sheet
/// holds them (`--class duelist@staff`, `duelist@dual`): the staff, `SABER`
/// and `LEFT_SABER` in each hand, else `SABER` in the right.
pub fn sabers(sheet: &Sheet) -> Vec<(&'static str, u8)> {
    use sim::inventory::slots::{LEFT_WEAPON, RIGHT_WEAPON};
    let saber = |hand: usize| {
        sheet.weapons[hand].filter(|w| matches!(w.delivery, sim::combat::Delivery::Saber(_)))
    };
    match (saber(0), saber(1)) {
        (Some(staff), None) if staff.base_item == STAFF_ROW => vec![(STAFF, RIGHT_WEAPON)],
        (Some(_), Some(_)) => vec![(SABER, RIGHT_WEAPON), (LEFT_SABER, LEFT_WEAPON)],
        _ => vec![(SABER, RIGHT_WEAPON)],
    }
}

/// `baseitems.2da`'s double-bladed lightsaber.
const STAFF_ROW: u16 = 9;

/// How many of each grenade, mine, medical item and stimulant the arena's
/// player starts with.
const STACK: u16 = rules::world::arena::terminal::STACK;

/// Every item the arena's player carries (`library`'s item templates but
/// plot items, by template): what a body wears on its head, body, hands,
/// arms, belt or as an implant, every hand weapon, every grenade and mine,
/// and every medical item and stimulant. It has one of each thing it
/// wears or holds, two of a gun the left hand takes too (a pistol, for a
/// pair), and `STACK` of each grenade, mine, medical item and stimulant.
pub fn gear(library: &sim::script::Library) -> Vec<(String, u16)> {
    use sim::combat::items::{Shelf, use_of};
    use sim::inventory::slots::{
        BELT, BODY, HANDS, HEAD, IMPLANT, LEFT_ARM, LEFT_WEAPON, RIGHT_ARM, RIGHT_WEAPON,
    };
    let worn = [HEAD, BODY, HANDS, LEFT_ARM, RIGHT_ARM, IMPLANT, BELT];
    library
        .items()
        .iter()
        .filter(|item| !item.plot)
        .filter_map(|item| {
            let fits = |slot: u8| item.slots & 1 << slot != 0;
            let shelf = item
                .spell
                .and_then(|spell| use_of(spell.row))
                .map(|of| of.shelf());
            let used = matches!(shelf, Some(Shelf::Grenade | Shelf::Medpac | Shelf::Stim));
            let gun = item.weapon.is_some_and(|weapon| weapon.delivery.ranged());
            let pistol = gun && fits(LEFT_WEAPON);
            let count = if item.slots == 0 && (used || item.trap.is_some()) {
                STACK
            } else if worn.into_iter().any(fits) || item.weapon.is_some() && fits(RIGHT_WEAPON) {
                1 + u16::from(pistol)
            } else {
                return None;
            };
            Some((item.template.clone(), count))
        })
        .collect()
}

/// The arena's keys and its panel.
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
    (mut ui, map): (
        Option<ResMut<crate::ui_view::Ui>>,
        Option<Res<crate::settings::KeyMap>>,
    ),
) {
    let Some(swap) = swap.filter(|swap| swap.level.arena_rules().is_some()) else {
        return;
    };
    // The console's orders, as the keys and the Debug Mode terminal give
    // them (`console_order`).
    let me = presented
        .as_ref()
        .zip(own.as_ref())
        .and_then(|(presented, own)| crate::cast::me(presented.0.current(), own.0));
    let asked = requests.as_mut().map_or_else(Vec::new, |requests| {
        std::mem::take(&mut requests.0)
            .into_iter()
            .filter_map(|words| console_order(&words, me, &swap.level))
            .collect()
    });
    if let Some(outgoing) = outgoing.as_mut() {
        outgoing.0.extend(asked.into_iter().map(Action::Arena));
    }
    let god = presented
        .as_ref()
        .zip(own.as_ref())
        .is_some_and(|(presented, own)| {
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
    // The floors stage says the floor underfoot instead.
    let at = presented.as_ref().zip(me).and_then(|(presented, me)| {
        let current = presented.0.current();
        current.get(me).map(|entity| entity.position)
    });
    let Some(ui) = ui.as_mut() else {
        return;
    };
    if swap.level.name() == crate::floor_arena::NAME {
        ui.vm.arena = Some(crate::floor_arena::line(at.unwrap_or([0.0; 3])).into_bytes());
    } else {
        // The terminal's row names the key that uses it, on a map that has one.
        let map = map
            .as_deref()
            .unwrap_or(crate::settings::KeyMap::defaults());
        let terminal = (swap.level.arena_rules())
            .is_some_and(|rules| rules.terminal.is_some())
            .then(|| map.shown(crate::settings::Action::Use));
        ui.vm.arena_panel = Some(panel(ROSTER[chosen.foe].0, god, terminal));
    }
}

/// The console's names of the attributes (`Ability` order) and the skills
/// (`skills.2da` rows).
const ABILITY_NAMES: [&str; 6] = [
    "strength",
    "dexterity",
    "constitution",
    "intelligence",
    "wisdom",
    "charisma",
];
const SKILL_NAMES: [&str; 8] = [
    "computer",
    "demolitions",
    "stealth",
    "awareness",
    "persuade",
    "repair",
    "security",
    "treat",
];

/// The console's arena order `words` (`arena spawn TEMPLATE`, `clear`,
/// `god`, `down N`), and the Debug Mode terminal's orders for the player's
/// own body `me`: `level CLASS N` (total level N, with `CLASS2 N2` that
/// many levels of a second class), `ability NAME N`, `skill NAME N`,
/// `alignment N` and `restore` (to `level`'s
/// starting character). A name may also be its number.
fn console_order(words: &[String], me: Option<SimId>, level: &sim::Level) -> Option<ArenaOrder> {
    let words: Vec<&str> = words.iter().map(String::as_str).collect();
    let named = |names: &[&str], word: &str| {
        let lower = word.to_ascii_lowercase();
        (names.iter().position(|name| *name == lower))
            .and_then(|at| u8::try_from(at).ok())
            .or_else(|| word.parse().ok())
    };
    Some(match words.as_slice() {
        ["spawn", template] => ArenaOrder::Spawn(template.to_string()),
        ["clear"] => ArenaOrder::Clear,
        ["god"] => ArenaOrder::God,
        ["down", n] => ArenaOrder::Down(n.parse().ok()?),
        ["level", class, n, second @ ..] => ArenaOrder::Rebuild {
            who: me?,
            class: class.parse().ok()?,
            level: n.parse().ok()?,
            second: match second {
                [] => None,
                [other, levels] => Some((other.parse().ok()?, levels.parse().ok()?)),
                _ => return None,
            },
        },
        ["ability", name, n] => ArenaOrder::Ability {
            who: me?,
            ability: named(&ABILITY_NAMES, name)?,
            score: n.parse().ok()?,
        },
        ["skill", name, n] => ArenaOrder::Skill {
            who: me?,
            skill: named(&SKILL_NAMES, name)?,
            ranks: n.parse().ok()?,
        },
        ["alignment", n] => ArenaOrder::Alignment {
            who: me?,
            good_evil: n.parse().ok()?,
        },
        ["restore"] => ArenaOrder::Restore {
            who: me?,
            sheet: Box::new(sheet_for(level)),
        },
        _ => return None,
    })
}

/// The panel's title and rows (`ui::screens::arena_panel`): `foe` the one
/// F2 calls in, `god` whether the player's body is in god mode, and the
/// key that uses the Debug Mode terminal on a map that has one.
fn panel(foe: &str, god: bool, terminal: Option<Vec<u8>>) -> ui::vm::ArenaPanelView {
    let row = |key: &[u8], words: &str, state: &str, on: bool| ui::vm::ArenaRow {
        key: key.into(),
        words: words.into(),
        state: state.into(),
        on,
    };
    let terminal = terminal.map(|key| row(&key, ui::strings::terminal::PANEL, "", false));
    ui::vm::ArenaPanelView {
        title: b"COMBAT DEBUG".to_vec(),
        rows: [
            row(b"F2", "Spawn", foe, true),
            row(b"F3", "Next foe", "", false),
            row(b"F4", "God mode", if god { "ON" } else { "off" }, god),
            row(b"F6", "Clear foes", "", false),
            row(b"[ ]", "Pick power", "", false),
            row(b"F", "Cast power", "", false),
        ]
        .into_iter()
        .chain(terminal)
        .collect(),
    }
}
