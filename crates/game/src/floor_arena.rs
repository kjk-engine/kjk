//! The floors stage (`kjk play --stage floors`): a patch of each kind of
//! floor both games walk on, to hear and see what each sounds like. Each
//! patch is 6 m square, drawn with a floor texture of K1's or TSL's as it
//! lies in a module there, and solid as a floor of that material with the
//! ground its texture makes (`collision_odyssey::ground`): its footsteps,
//! its landings and its landing dust are those the module's floor plays.
//!
//! A row for each ground (sand, snow, dirt, mud, grass, stone, metal, wood,
//! rug, water), the far row sand; K1's patches west of the grey aisle the
//! player starts in, TSL's east. The patches near the player carry their
//! planet, module and texture over them (`labels`). The patches are the floor textures the
//! modules use most for each ground, one a planet (`kjk collision build`
//! lists them). The grey floor between them steps as stone. The line at
//! the top says the patch underfoot: its game, module, material, texture
//! and ground. It runs as the arena does
//! (`arena`): its Jedi, its keys, foes to call in.

use std::collections::BTreeMap;

use asset_odyssey::model::ModelIr;
use asset_odyssey::texture::TextureIr;
use collision::{Brush, BrushWorld, Ground, Surface};
use vfs::{Discovery, Game};

/// The stage's level name.
pub const NAME: &str = "floors";
/// A patch's side, and half the aisle down the middle (metres).
const PATCH: f32 = 6.0;
const AISLE: f32 = 1.5;
/// The space between the patches and the walls, and the walls' height.
const MARGIN: f32 = 3.0;
const WALL: f32 = 4.0;
/// Patches within this of the player carry their labels (metres).
const LABELLED: f32 = 14.0;
/// The grey floor between the patches sits this far below them, so the two
/// never draw over each other.
const UNDER: f32 = 0.01;
/// `surfacemat.2da`'s stone row.
const STONE: u16 = 4;

/// The rows, far to near.
const ROWS: [Ground; 10] = [
    Ground::Sand,
    Ground::Snow,
    Ground::Dirt,
    Ground::Mud,
    Ground::Grass,
    Ground::Stone,
    Ground::Metal,
    Ground::Wood,
    Ground::Rug,
    Ground::Water,
];

/// The patches: game, the module whose floor it is, its walkmesh material
/// (a `surfacemat.2da` row) and texture.
const PATCHES: [(Game, &str, u32, &str); 53] = [
    (Game::Kotor1, "tat_m18ab", 1, "lta_flr01a"),
    (Game::Kotor1, "korr_m34aa", 1, "lrk_flr03"),
    (Game::Kotor1, "unk_m41aa", 1, "lun_sand01"),
    (Game::Kotor1, "manm28ab", 1, "lma_sand03"),
    (Game::Kotor1, "unk_m44ab", 1, "lrk_rock03"),
    (Game::Kotor1, "manm26mg", 1, "lmg_water01"),
    (Game::Kotor1, "tat_m17mg", 1, "lmg_tatline02"),
    (Game::Kotor1, "danm14ae", 1, "lda_ultimate1"),
    (Game::Kotor1, "kas_m25aa", 3, "lka_mud02"),
    (Game::Kotor1, "danm14ac", 3, "lda_grass07"),
    (Game::Kotor1, "unk_m41ad", 3, "lun_grss02"),
    (Game::Kotor1, "tar_m04aa", 3, "lts_unflr06"),
    (Game::Kotor1, "unk_m42aa", 4, "lrk_flr03a"),
    (Game::Kotor1, "danm15", 4, "lrk_rock03"),
    (Game::Kotor1, "korr_m39aa", 4, "lko_flr01"),
    (Game::Kotor1, "tat_m18ac", 4, "lta_clitran"),
    (Game::Kotor1, "lev_m40ab", 10, "lsi_floor01"),
    (Game::Kotor1, "tar_m03ab", 10, "lts_pwall01"),
    (Game::Kotor1, "manm26aa", 10, "lma_floor03"),
    (Game::Kotor1, "ebo_m12aa", 10, "leh_floor05"),
    (Game::Kotor1, "sta_m45ac", 10, "lsf_trim02"),
    (Game::Kotor1, "kas_m22ab", 5, "lka_wall01"),
    (Game::Kotor1, "ebo_m46ab", 9, "lsf_dream"),
    (Game::Kotor1, "korr_m34aa", 11, "lrk_flr03"),
    (Game::Kotor2, "710kor", 1, "lrk_flr03"),
    (Game::Kotor2, "231tel", 1, "tel_sand01"),
    (Game::Kotor2, "261tel", 9, "tel_snw"),
    (Game::Kotor2, "105per", 10, "tel_snw2"),
    (Game::Kotor2, "402dxn", 1, "dxn_grs6"),
    (Game::Kotor2, "106per", 1, "per_rk01"),
    (Game::Kotor2, "231tel", 1, "tel_dirt"),
    (Game::Kotor2, "902mal", 1, "mal_grnd01"),
    (Game::Kotor2, "501ond", 1, "ond_gs02"),
    (Game::Kotor2, "601dan", 3, "dan_grass07"),
    (Game::Kotor2, "231tel", 3, "tel_grass"),
    (Game::Kotor2, "401dxn", 3, "dxn_grs5"),
    (Game::Kotor2, "501ond", 3, "ond_gs02"),
    (Game::Kotor2, "506ond", 4, "ond_fl06"),
    (Game::Kotor2, "901mal", 4, "mal_grnd01"),
    (Game::Kotor2, "604dan", 4, "dan_fl04"),
    (Game::Kotor2, "411dxn", 4, "dxn_fl02"),
    (Game::Kotor2, "702kor", 4, "kor_flr01"),
    (Game::Kotor2, "151har", 10, "har_wl07"),
    (Game::Kotor2, "001ebo", 10, "leh_floor05"),
    (Game::Kotor2, "102per", 10, "per_fl05"),
    (Game::Kotor2, "851nih", 10, "nih_fl"),
    (Game::Kotor2, "232tel", 10, "tel_tr04"),
    (Game::Kotor2, "602dan", 9, "per_bed"),
    (Game::Kotor2, "950cor", 9, "mal_wl04"),
    (Game::Kotor2, "001ebo", 9, "lmi_bed01"),
    (Game::Kotor2, "305nar", 9, "nar_met3"),
    (Game::Kotor2, "410dxn", 11, "dxn_gras2"),
    (Game::Kotor2, "710kor", 11, "lrk_flr03"),
];

/// `surfacemat.2da`'s labels for the materials the patches use.
fn material_label(material: u32) -> &'static str {
    match material {
        1 => "Dirt",
        3 => "Grass",
        4 => "Stone",
        5 => "Wood",
        9 => "Carpet",
        10 => "Metal",
        11 => "Puddles",
        _ => "?",
    }
}

/// The planet a module lies on (or the ship it is aboard): K1's by the
/// module's first letters, TSL's by the letters after its number.
fn planet(game: Game, module: &str) -> &'static str {
    const K1: [(&str, &str); 10] = [
        ("tat", "Tatooine"),
        ("korr", "Korriban"),
        ("unk", "Unknown World"),
        ("man", "Manaan"),
        ("dan", "Dantooine"),
        ("tar", "Taris"),
        ("kas", "Kashyyyk"),
        ("lev", "Leviathan"),
        ("ebo", "Ebon Hawk"),
        ("sta", "Star Forge"),
    ];
    const TSL: [(&str, &str); 13] = [
        ("261tel", "Telos polar"),
        ("tel", "Telos"),
        ("per", "Peragus"),
        ("har", "Harbinger"),
        ("ebo", "Ebon Hawk"),
        ("nar", "Nar Shaddaa"),
        ("dxn", "Dxun"),
        ("ond", "Onderon"),
        ("dan", "Dantooine"),
        ("kor", "Korriban"),
        ("nih", "Ravager"),
        ("mal", "Malachor V"),
        ("cor", "Coruscant"),
    ];
    let found = match game {
        Game::Kotor1 => K1.iter().find(|(key, _)| module.starts_with(key)),
        _ => TSL
            .iter()
            .find(|(key, _)| module.ends_with(key) || module == *key),
    };
    found.map_or("?", |(_, planet)| planet)
}

/// A patch laid out: what it is, the ground it makes and where it lies
/// (min and max x, y; metres).
struct Laid {
    game: Game,
    module: &'static str,
    material: u32,
    texture: &'static str,
    ground: Ground,
    min: [f32; 2],
    max: [f32; 2],
}

impl Laid {
    /// Its texture's name in the model: the game's, as both games have
    /// textures of the same name.
    fn key(&self) -> String {
        format!("{}/{}", self.game.key(), self.texture)
    }
}

/// Every patch in its row (its ground's) and column (in its game's order
/// within the row, outward from the aisle).
fn laid() -> Vec<Laid> {
    // By game (K1 or not) and ground.
    let mut columns: BTreeMap<(bool, Ground), usize> = BTreeMap::new();
    PATCHES
        .iter()
        .map(|&(game, module, material, texture)| {
            let ground = collision_odyssey::ground::of(material, texture);
            let row = ROWS
                .iter()
                .position(|&g| g == ground)
                .unwrap_or(ROWS.len() - 1);
            let column = columns.entry((game == Game::Kotor1, ground)).or_insert(0);
            let out = AISLE + PATCH * *column as f32;
            *column += 1;
            let (x0, x1) = match game {
                Game::Kotor1 => (-out - PATCH, -out),
                _ => (out, out + PATCH),
            };
            let y1 = PATCH * ROWS.len() as f32 / 2.0 - PATCH * row as f32;
            Laid {
                game,
                module,
                material,
                texture,
                ground,
                min: [x0, y1 - PATCH],
                max: [x1, y1],
            }
        })
        .collect()
}

/// Half the floor's side: the patches' reach and a margin.
fn half() -> f32 {
    let reach = laid()
        .iter()
        .flat_map(|patch| [patch.min, patch.max])
        .flatten()
        .fold(0.0f32, |reach, v| reach.max(v.abs()));
    reach + MARGIN
}

/// The stage's collision: the grey floor and walls, and each patch solid
/// as its floor.
pub fn collision() -> BrushWorld {
    let cuboid = Brush::cuboid;
    let half = half();
    // The grey floor steps as stone, so leaving a patch is heard.
    let mut grey = cuboid([-half, -half, -1.0], [half, half, -UNDER]);
    grey.surface = Surface {
        material: STONE,
        ground: Ground::Stone,
    };
    let mut world = BrushWorld::new()
        .with(grey)
        .with(cuboid([-half - 0.5, -half, 0.0], [-half, half, WALL]))
        .with(cuboid([half, -half, 0.0], [half + 0.5, half, WALL]))
        .with(cuboid(
            [-half - 0.5, -half - 0.5, 0.0],
            [half + 0.5, -half, WALL],
        ))
        .with(cuboid(
            [-half - 0.5, half, 0.0],
            [half + 0.5, half + 0.5, WALL],
        ));
    for patch in laid() {
        let mut brush = cuboid(
            [patch.min[0], patch.min[1], -1.0],
            [patch.max[0], patch.max[1], 0.0],
        );
        brush.surface = Surface {
            material: patch.material as u16,
            ground: patch.ground,
        };
        world = world.with(brush);
    }
    world
}

/// The floor as a navigation mesh: one square.
pub fn nav() -> collision::nav::NavMesh {
    let half = half();
    let [a, b, c, d] = [
        [-half, -half, 0.0],
        [half, -half, 0.0],
        [half, half, 0.0],
        [-half, half, 0.0],
    ];
    collision::nav::NavMesh::build(
        &[[a, b, c], [a, c, d]],
        sim::walk::CLEARANCE * collision::UNIT,
    )
}

/// The stage as a model to draw, and its textures: the grey checks, and
/// each patch's top in its texture, read from its game's install (a
/// texture not read keeps the checks, and is noted).
pub fn model(
    discovery: &Discovery,
    world: &BrushWorld,
    notes: &mut Vec<String>,
) -> (ModelIr, Vec<(String, TextureIr)>) {
    let laid = laid();
    let mut textures = crate::course::check_textures();
    let mut read: BTreeMap<String, bool> = BTreeMap::new();
    for patch in &laid {
        let key = patch.key();
        if read.contains_key(&key) {
            continue;
        }
        let texture = crate::installs::open(discovery, patch.game)
            .and_then(|install| install.resolver(Some(patch.module)))
            .ok()
            .and_then(|resolver| crate::viewer::texture(&resolver, patch.texture));
        match texture {
            Some(texture) => {
                textures.push((key.clone(), texture));
                read.insert(key, true);
            }
            None => {
                notes.push(format!(
                    "floors: {key} not read; its patch keeps the checks"
                ));
                read.insert(key, false);
            }
        }
    }
    let mut checks: Vec<_> = textures
        .iter()
        .take(3)
        .map(|(name, _)| crate::course::mesh(name))
        .collect();
    let mut tops: BTreeMap<String, asset_odyssey::model::MeshIr> = BTreeMap::new();
    // The patches are the brushes after the floor and the four walls.
    for (k, brush) in world.brushes.iter().enumerate() {
        let patch = k.checked_sub(5).and_then(|i| laid.get(i));
        for i in 0..brush.planes.len() {
            let (which, axes) = crate::course::facing(brush, i);
            let top = which == 0 && brush.planes[i].normal[2] > 0.0;
            match patch.filter(|patch| top && read.get(&patch.key()) == Some(&true)) {
                Some(patch) => {
                    let key = patch.key();
                    let mesh = tops
                        .entry(key.clone())
                        .or_insert_with(|| crate::course::mesh(&key));
                    crate::course::add_face(mesh, brush, i, axes);
                }
                None => crate::course::add_face(&mut checks[which], brush, i, axes),
            }
        }
    }
    checks.extend(tops.into_values());
    (crate::course::model_of(NAME, checks), textures)
}

/// The line at the top for a player standing at `at` (metres): the patch
/// underfoot, or how the stage is laid out.
pub fn line(at: [f32; 3]) -> String {
    let under = laid().into_iter().find(|patch| {
        (patch.min[0]..patch.max[0]).contains(&at[0])
            && (patch.min[1]..patch.max[1]).contains(&at[1])
    });
    match under {
        Some(patch) => format!(
            "FLOORS   {} {} {}   {} floor, {}   sounds like {:?}",
            patch.game.key().to_uppercase(),
            planet(patch.game, patch.module),
            patch.module,
            material_label(patch.material),
            patch.texture,
            patch.ground,
        ),
        None => "FLOORS   a row per ground (far: sand, near: water); K1 west, TSL east   F2 spawn   F4 god mode".to_string(),
    }
}

/// The labels over the patches within `LABELLED` of `at` (metres): at each
/// patch's middle, its game and planet over its module and texture.
pub fn labels(at: [f32; 3]) -> Vec<([f32; 3], [String; 2])> {
    laid()
        .into_iter()
        .filter_map(|patch| {
            let middle = [
                (patch.min[0] + patch.max[0]) / 2.0,
                (patch.min[1] + patch.max[1]) / 2.0,
                0.0,
            ];
            let near = (middle[0] - at[0]).hypot(middle[1] - at[1]) < LABELLED;
            let game = patch.game.key().to_uppercase();
            near.then(|| {
                let planet = planet(patch.game, patch.module);
                let words = [
                    format!("{game} {planet}"),
                    format!("{} {}", patch.module, patch.texture),
                ];
                (middle, words)
            })
        })
        .collect()
}
