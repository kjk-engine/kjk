//! The lights of the games' own light fittings, placed by the game itself
//! (docs/LIGHTING-PLAN.md; a trial, `KJK_FITTED_LIGHTS=on` under Enhanced
//! lighting).
//!
//! A kind of fitting is a texture that glows. What each kind does is the
//! rule data of `data/lights/`: `k1.toml` and `tsl.toml` hold one entry for
//! each kind, and `fittings.toml` the lights placed by hand in one area.
//! `tools/light-kinds/kinds.py` says what an entry's fields mean.
//!
//! - **The lamps of a mesh** (`parts`). A mesh often holds several lamps.
//!   They are its pieces that do not touch, or the bright patches of its
//!   texture where the kind says so (`lamps = "bright"`).
//! - **A light on each lamp** (`lights`). It stands a little off the lamp,
//!   shines the way the lamp faces, and is a strip where the lamp is long.
//! - **The rooms' hidden lamps shine too.** A room's model holds lamps
//!   that light only characters today. Under the trial they light the
//!   rooms, each room's at one strength. That strength puts back the
//!   half of the room's baked light that the trial takes away, so a room with
//!   no fitting is about as bright as today. In a room with fittings the
//!   hidden lamps keep only `FILL_BESIDE` of that: the owner expects the
//!   glowing fittings to be what lights the room (17 November 2026).
//!
//! The survey of `kjk light-kinds` finds the lamps with the same code, so
//! the pictures the owner approved and the game agree.

use std::collections::{BTreeMap, BTreeSet, HashMap};

use asset_odyssey::texture::{Encoding, TextureIr};
use bevy::math::{Mat4, Vec2, Vec3};
use fmt_odyssey::dxt;

use crate::scene::{placement, to_mat4};
use crate::viewer::LoadedScene;

/// How the rooms are lit. A run starts with the mode `KJK_FITTED_LIGHTS`
/// names: unset or `off` is `Off`, `baked` is `Baked`, `lamps` is
/// `LampsOnly`, and anything else is `Live`. A program may change it while
/// it runs (`set_mode`, the level editor's lighting view); the next scene
/// drawn takes it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    /// The game's stored lightmaps alone.
    Off,
    /// The trial: the rooms keep half of their stored light, and the
    /// fitted lights give the rest.
    Live,
    /// The rooms keep none of their stored light: only the fitted lights
    /// show, to see what each one does.
    LampsOnly,
    /// The rooms show a bake of the fitted lights in place of their live
    /// light. The baked lightmaps are the ones `KJK_DETAIL_BAKE` names (or
    /// `viewer::set_bake_folder`), made by `kjk detail-bake --lights`. The
    /// rooms' lightmapped surfaces then show the baked lightmap whole and
    /// take no fitted light. Doors, placeables and characters are lit as
    /// under `Live`.
    Baked,
}

static MODE: std::sync::RwLock<Option<Mode>> = std::sync::RwLock::new(None);

/// The mode the rooms are lit in now.
pub fn mode() -> Mode {
    if let Some(mode) = *MODE
        .read()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
    {
        return mode;
    }
    let mode = match std::env::var("KJK_FITTED_LIGHTS").as_deref() {
        Err(_) | Ok("off") => Mode::Off,
        Ok("baked") => Mode::Baked,
        Ok("lamps") => Mode::LampsOnly,
        Ok(_) => Mode::Live,
    };
    set_mode(mode);
    mode
}

/// Lights the rooms in `mode` from the next scene drawn on.
pub fn set_mode(mode: Mode) {
    *MODE
        .write()
        .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(mode);
}

/// Whether the fitted lights shine (any mode but `Off`).
pub fn wanted() -> bool {
    mode() != Mode::Off
}

/// Whether the rooms show a bake of the fitted lights (`Mode::Baked`).
pub fn baked() -> bool {
    mode() == Mode::Baked
}

/// The value of `shadows.debug` the mode asks of the shader
/// (`odyssey.wgsl`): 7 for `Live`, 8 for `Baked`, 9 for `LampsOnly`, 0 for
/// `Off`.
pub fn shader_trial() -> u32 {
    match mode() {
        Mode::Off => 0,
        Mode::Live => 7,
        Mode::Baked => 8,
        Mode::LampsOnly => 9,
    }
}

/// The text of a rule file of `data/lights/` (`name`, such as
/// `fittings.toml`): the one in the folder `KJK_LIGHT_DATA` names, where it
/// names one that holds it, else the copy built into the program. The
/// level editor saves its lights to such a folder, and play started from it
/// reads them there.
fn data_text(name: &str, built_in: &'static str) -> std::borrow::Cow<'static, str> {
    std::env::var_os("KJK_LIGHT_DATA")
        .filter(|folder| !folder.is_empty())
        .and_then(|folder| std::fs::read_to_string(std::path::Path::new(&folder).join(name)).ok())
        .map_or(
            std::borrow::Cow::Borrowed(built_in),
            std::borrow::Cow::Owned,
        )
}

/// The lights placed by hand that a program sets for an area in place of
/// `fittings.toml`'s (`set_placed`), by the area's name in lower case.
static PLACED: std::sync::RwLock<BTreeMap<String, Vec<Light>>> =
    std::sync::RwLock::new(BTreeMap::new());

/// Gives the area `area` these lights placed by hand in place of the ones
/// `fittings.toml` holds for it, or (`None`) the file's again. The level
/// editor sets the lights it edits, so its view shows them before they are
/// saved.
pub fn set_placed(area: &str, lights: Option<Vec<Light>>) {
    let mut placed = PLACED
        .write()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    match lights {
        Some(lights) => {
            placed.insert(area.to_ascii_lowercase(), lights);
        }
        None => {
            placed.remove(&area.to_ascii_lowercase());
        }
    }
}

impl Light {
    /// A light placed by hand in the level editor (`level_edit::lights`).
    pub fn of_placed(light: &level_edit::PlacedLight) -> Light {
        Light {
            at: Vec3::from_array(light.at),
            colour: Vec3::from_array(light.colour),
            radius: light.radius,
            way: Vec3::from_array(light.way),
            half: light.half,
            aim: Vec3::from_array(light.aim),
            cone: light.cone,
        }
    }

    /// The light as the level editor shows it.
    pub fn placed(&self) -> level_edit::PlacedLight {
        level_edit::PlacedLight {
            at: self.at.to_array(),
            colour: self.colour.to_array(),
            radius: self.radius,
            aim: self.aim.to_array(),
            cone: self.cone,
            way: self.way.to_array(),
            half: self.half,
            texture: String::new(),
            reason: String::new(),
        }
    }
}

/// A light of a fitting, in game space.
#[derive(Debug, Clone, PartialEq)]
pub struct Light {
    pub at: Vec3,
    pub colour: Vec3,
    pub radius: f32,
    /// The way a strip runs and half its length (metres); zeros for a
    /// light at one place.
    pub way: Vec3,
    pub half: f32,
    /// The way it shines and how narrow (`material::GpuLight::aim`); zeros
    /// for a light that shines all ways.
    pub aim: Vec3,
    pub cone: f32,
}

/// What a kind of fitting does (an entry of `data/lights/<game>.toml`).
#[derive(Clone)]
struct Kind {
    form: String,
    colour: Vec3,
    reach: f32,
    strength: f32,
    cone: f32,
    line: bool,
    gather: bool,
    off: Option<f32>,
    bright: bool,
    longest: Option<f32>,
    tint: bool,
    /// It shines where its mesh does not glow too.
    always: bool,
    /// A large lamp of it shines stronger than a small one (`GROW_WIDTH`).
    grow: bool,
    /// It shines although little of its texture is bright (a terminal's
    /// small displays).
    dark: bool,
}

/// A game's kinds by texture and whether it is on a placeable. `None` is a
/// kind that is no lamp.
type Kinds = BTreeMap<(String, bool), Option<Kind>>;

const TABLES: [&str; 2] = [
    include_str!("../../../data/lights/k1.toml"),
    include_str!("../../../data/lights/tsl.toml"),
];
const FITTINGS: &str = include_str!("../../../data/lights/fittings.toml");

/// How far off its surface a fitting's light stands (metres).
const OFF: f32 = 0.25;
/// A lamp longer than this from its middle shines as a strip, which ends
/// `STRIP_SHORT` metres before the lamp does.
const STRIP_FROM: f32 = 1.0;
const STRIP_SHORT: f32 = 0.3;
/// A fitting whose facing is shorter than this (its square) faces no one
/// way, so its light has no cone.
const FACES: f32 = 0.25;
/// The tiny pieces of a gathered kind within this of each other (metres)
/// share one light, which stands `GATHER_UP` metres above their middle.
const GATHER: f32 = 3.0;
const GATHER_UP: f32 = 0.6;
/// A mesh larger than this is a sky or a backdrop (square metres).
const SKY_M2: f32 = 2000.0;
/// A texel brighter than this counts as bright, and a texture with at
/// least `BRIGHT_SHARE` of its texels bright looks like a light
/// (`kjk room-lights`).
const BRIGHT: f32 = 0.75;
const BRIGHT_SHARE: f32 = 0.1;
/// The longest side at which a texture's brightness is read (texels).
const BRIGHTNESS_SIDE: usize = 64;
/// A hidden lamp of one strong colour (its strongest channel more than
/// twice the next) counts at this much, or its room turns that colour.
const STRONG_COLOUR: f32 = 0.35;
/// How much of the baked light taken away the hidden lamps put back. It
/// was fitted on the Harbinger (2.8) and the Dantooine enclave (3.1).
const FILL_GAIN: f32 = 3.0;
/// The least and the most an area's hidden lamps are strengthened.
const FILL_RANGE: (f32, f32) = (0.05, 2.0);
/// How much of their strength the hidden lamps keep in a room that has
/// light fittings.
const FILL_BESIDE: f32 = 0.3;
/// How bright the lights may make a room (`level`), as a stored level of
/// light where 1 is full white. Over all its baked surfaces the room may
/// show `LEVEL_OVER` times what it shows today, or `LEVEL_LIFT` where that
/// is more: a dark room may be lifted, and a bright one stays as bright as
/// it is. The brightest twentieth of its surfaces may show `LEVEL_HOT`, and
/// the brightest hundredth `LEVEL_PEAK` times that. On the Harbinger, which
/// the owner walked and approved, the rooms show up to 0.55, 1.06 and
/// 1.35, so nothing changes there.
const LEVEL_OVER: f32 = 1.1;
const LEVEL_LIFT: f32 = 0.56;
const LEVEL_HOT: f32 = 1.1;
const LEVEL_PEAK: f32 = 1.24;
/// A room that is bright today may show less on its brightest surfaces:
/// `LEVEL_HOT_BRIGHT` where it shows `LEVEL_BRIGHT.1` today, and
/// `LEVEL_HOT` where it shows `LEVEL_BRIGHT.0` or less, which every room
/// of the Harbinger does. The Endar Spire's rooms show 0.26 to 0.55 today,
/// and their walls are near white beside a lamp already. With `LEVEL_HOT`
/// a wall beside a row of lamps showed twice what it shows today, and the
/// owner said of it: "this side is too bright, the opposite wall is not as
/// bright". The two numbers are this branch's judgement from pictures of
/// six places there. The owner has not seen them.
const LEVEL_HOT_BRIGHT: f32 = 0.7;
const LEVEL_BRIGHT: (f32, f32) = (0.38, 0.54);
/// A light no stronger than this is faint (a screen, a sign, a room's
/// hidden lamp). `level` leaves it as it is where the faint lights alone
/// keep the room's brightest surfaces under `LEVEL_FAINT_PART` of what
/// they may show, so that every fitting still sheds a little light in a
/// room whose lamps are made much weaker. Where they do not (a bridge
/// full of screens), they are made weaker by one share too, and the lamps
/// have the rest.
const LEVEL_FAINT: f32 = 0.65;
const LEVEL_FAINT_PART: f32 = 0.8;
/// A piece of a room's surface is at most this large where the level is
/// measured (square metres), and the brightest pieces are these shares of
/// the surfaces.
const LEVEL_PIECE: f32 = 0.5;
const LEVEL_HOT_SHARE: f32 = 0.05;
const LEVEL_PEAK_SHARE: f32 = 0.01;
/// A length cut from a long lamp that bends (`lengths`) shines at this
/// much of its kind's strength. The lengths lie end to end and each
/// lights the wall beside its neighbours too, so together at full strength
/// they would light the hall more than one long lamp does.
const CUT_SHARE: f32 = 0.6;
/// A lamp of a kind that grows (`grow`) shines at its kind's strength
/// when its size is no more than this, as a tube or two are. Its size is
/// the square root of its glowing area (metres). A larger one, a panel of
/// several tubes, shines stronger by its size, up to `GROW_MOST` times.
const GROW_WIDTH: f32 = 0.7;
const GROW_MOST: f32 = 1.5;
/// How far under a hidden lamp its light is measured (metres).
const FILL_UNDER: f32 = 1.5;

/// A lamp of a room that the files do not set to glow is switched on by
/// the stored lightmap on it. The games light such a lamp with the lightmap
/// alone, so it looks lit and sheds no light. It shines from `ON_FROM` of
/// full white on its brighter surfaces, and at its kind's whole strength
/// from `ON_FULL`. The owner walked Peragus on 18 November 2026 and said of
/// such lamps: "these light panels only emit light at the top of them" and
/// "its actual panels dont emit any light at all, we should make them glow".
const ON_FROM: f32 = 0.3;
const ON_FULL: f32 = 0.6;
/// The share of a lamp's surfaces that are darker than the level it is
/// judged by. Its housing lies in shadow, and its tubes do not.
const ON_ABOVE: f32 = 0.75;
/// A switched lamp shines at this share of its kind's strength at the most,
/// and its brightest texels glow this much over their baked light. At its
/// kind's whole strength and a full glow its frame showed as white as its
/// tubes, and the owner said: "it just looks too bright and the frame of the
/// lamp is equally bright like the panel". It was 0.6 until the Harbinger's
/// ceiling lamps filled in the shadow under a terminal, and the owner said
/// on 18 November 2026: "I'm missing the shadow from the terminal". At a
/// quarter the shadow shows as it did before these lamps shone.
const SWITCHED_SHARE: f32 = 0.25;
pub const SWITCHED_GLOW: f32 = 0.35;
/// A glowing texture that the rule data does not name is a screen or a
/// panel. Each bright patch of it sheds a faint light of the patch's own
/// colour, this strong and this far (metres), in a cone this narrow. The
/// owner said of Peragus's consoles and displays: "this can emit some light
/// from its panels" and "give em a lil light from the panels".
const UNNAMED_STRENGTH: f32 = 0.5;
const UNNAMED_REACH: f32 = 3.0;
const UNNAMED_CONE: f32 = 0.2;
/// An unnamed glowing piece wider than this is a wall or a backdrop, and
/// sheds nothing (metres, the square root of its area).
const UNNAMED_WIDEST: f32 = 8.0;
/// An unnamed glowing piece no wider than this sheds light although no
/// texel of it is bright: a beacon or a crystal (metres).
const UNNAMED_SMALL: f32 = 1.0;
/// A door's glowing mesh sheds light only where its bright texels are this
/// strongly coloured (the strongest channel less the weakest): a force
/// field. A door of grey metal glows in the files in place of baked light.
const FIELD_CHROMA: f32 = 0.5;

/// What a glowing texture that the rule data does not name does.
fn unnamed() -> Kind {
    Kind {
        form: "faint".to_string(),
        colour: Vec3::ONE,
        reach: UNNAMED_REACH,
        strength: UNNAMED_STRENGTH,
        cone: UNNAMED_CONE,
        line: false,
        gather: false,
        off: None,
        bright: true,
        longest: None,
        tint: true,
        always: false,
        grow: false,
        dark: true,
    }
}

/// How much of its kind's strength a room's lamp that the files do not set
/// to glow shines at, from 0 to 1 (`ON_FROM`). `stored` is the lightmap the
/// game's data stores for the mesh. A mesh that glows, a mesh of no lamp
/// and a faint kind's mesh give 0.
pub fn switched_on(
    mesh: &asset_odyssey::model::MeshIr,
    stored: impl FnOnce() -> Option<TextureIr>,
) -> f32 {
    if !wanted() || mesh.skin.is_some() || mesh.self_illum.iter().any(|&v| v > 0.0) {
        return 0.0;
    }
    let Some(texture) = mesh.texture.as_deref() else {
        return 0.0;
    };
    let key = (texture.to_ascii_lowercase(), false);
    let lamp = (kinds().iter())
        .any(|game| matches!(game.get(&key), Some(Some(kind)) if kind.form != "faint"));
    if !lamp || mesh.uv2.len() != mesh.positions.len() {
        return 0.0;
    }
    let Some((width, height, map)) = stored().and_then(|stored| texels(&stored, 128)) else {
        return 0.0;
    };
    let map = LampTexture::new(width, height, map);
    // Each triangle's size and the stored light at its middle.
    let mut pieces: Vec<(f32, f32)> = Vec::new();
    for &[a, b, c] in mesh.indices.as_chunks::<3>().0 {
        let corner = |i: u32| {
            let at = Vec3::from_array(*mesh.positions.get(i as usize)?);
            Some((at, Vec2::from_array(mesh.uv2[i as usize])))
        };
        let (Some(a), Some(b), Some(c)) = (corner(a), corner(b), corner(c)) else {
            continue;
        };
        let size = 0.5 * (b.0 - a.0).cross(c.0 - a.0).length();
        if size > 1e-8 {
            let level = brightness(&map.at(((a.1 + b.1 + c.1) / 3.0).to_array()));
            pieces.push((size, level));
        }
    }
    let area: f32 = pieces.iter().map(|piece| piece.0).sum();
    if area <= 0.0 {
        return 0.0;
    }
    pieces.sort_by(|a, b| a.1.total_cmp(&b.1));
    let (mut under, mut level) = (0.0, 0.0);
    for (size, at) in &pieces {
        under += size;
        level = *at;
        if under >= ON_ABOVE * area {
            break;
        }
    }
    let t = ((level - ON_FROM) / (ON_FULL - ON_FROM)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// Each game's kinds, read once (from `KJK_LIGHT_DATA` or the copy built
/// in), with the entries a program set since (`set_kinds`).
static KINDS: std::sync::RwLock<Option<std::sync::Arc<[Kinds; 2]>>> = std::sync::RwLock::new(None);

fn kinds() -> std::sync::Arc<[Kinds; 2]> {
    if let Some(kinds) = KINDS
        .read()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .as_ref()
    {
        return kinds.clone();
    }
    let read = std::sync::Arc::new(
        [("k1.toml", TABLES[0]), ("tsl.toml", TABLES[1])]
            .map(|(name, text)| read_kinds(&data_text(name, text)).unwrap_or(Kinds::new())),
    );
    *KINDS
        .write()
        .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(read.clone());
    read
}

/// Puts the `[[kind]]` entries of `text` (in the layout of
/// `data/lights/<game>.toml`) in place of `game`'s (`k1` or `tsl`) entries
/// for the same kinds, in the scenes drawn from now on. The level editor
/// sets a kind it edits, so its view shows the change before it is saved.
pub fn set_kinds(game: &str, text: &str) -> Result<(), String> {
    let index = match game {
        "k1" => 0,
        "tsl" => 1,
        _ => return Err(format!("{game} is not k1 or tsl")),
    };
    let entries = read_kinds(text).ok_or("the kinds do not read")?;
    let mut all = (*kinds()).clone();
    all[index].extend(entries);
    *KINDS
        .write()
        .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(std::sync::Arc::new(all));
    Ok(())
}

fn number(item: Option<&toml_edit::Item>) -> Option<f32> {
    let item = item?;
    (item.as_float().map(|v| v as f32)).or_else(|| item.as_integer().map(|v| v as f32))
}

fn three(item: Option<&toml_edit::Item>) -> Option<Vec3> {
    let array = item?.as_array()?;
    let at = |index: usize| {
        let value = array.get(index)?;
        (value.as_float().map(|v| v as f32)).or_else(|| value.as_integer().map(|v| v as f32))
    };
    Some(Vec3::new(at(0)?, at(1)?, at(2)?))
}

fn read_kinds(text: &str) -> Option<Kinds> {
    let doc = toml_edit::Document::parse(text).ok()?;
    let mut kinds = Kinds::new();
    for table in doc.get("kind")?.as_array_of_tables()?.iter() {
        let text = |key: &str| table.get(key).and_then(toml_edit::Item::as_str);
        let flag = |key: &str| (table.get(key).and_then(toml_edit::Item::as_bool)).unwrap_or(false);
        let Some(texture) = text("texture") else {
            continue;
        };
        let key = (
            texture.to_ascii_lowercase(),
            text("on") == Some("placeable"),
        );
        let lamp = (table.get("lamp").and_then(toml_edit::Item::as_bool)).unwrap_or(true);
        let kind = lamp
            .then(|| {
                Some(Kind {
                    form: text("form")?.to_string(),
                    colour: three(table.get("colour"))?,
                    reach: number(table.get("reach"))?,
                    strength: number(table.get("strength"))?,
                    cone: number(table.get("cone")).unwrap_or(0.0),
                    line: flag("line"),
                    gather: flag("gather"),
                    off: number(table.get("off")),
                    bright: text("lamps") == Some("bright"),
                    longest: number(table.get("longest")),
                    tint: flag("tint"),
                    always: flag("always"),
                    grow: flag("grow"),
                    dark: flag("dark"),
                })
            })
            .flatten();
        kinds.insert(key, kind);
    }
    Some(kinds)
}

/// The module a scene's name names. Play's scene is named for the
/// player's body and the module (`play pmbal in danm13`, `play::compose`);
/// a module's own scene is named for the module alone.
fn module_of(scene: &str) -> &str {
    scene.rsplit(" in ").next().unwrap_or(scene)
}

/// The lights placed by hand in the area `area` (`[[placed]]` in
/// `fittings.toml`).
fn placed(area: &str) -> Vec<Light> {
    if let Some(set) = PLACED
        .read()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .get(&area.to_ascii_lowercase())
    {
        return set.clone();
    }
    // A level editor project's new map keeps its own (`level_edit`).
    if let Some(lights) = level_edit::active::map_lights(area) {
        return lights.iter().map(Light::of_placed).collect();
    }
    let text = data_text("fittings.toml", FITTINGS);
    let Ok(doc) = toml_edit::Document::parse(text.as_ref()) else {
        return Vec::new();
    };
    let Some(tables) = (doc.get("placed")).and_then(toml_edit::Item::as_array_of_tables) else {
        return Vec::new();
    };
    (tables.iter())
        .filter(|table| {
            (table.get("area").and_then(toml_edit::Item::as_str))
                .is_some_and(|name| name.eq_ignore_ascii_case(area))
        })
        .filter_map(|table| {
            Some(Light {
                at: three(table.get("at"))?,
                colour: three(table.get("colour"))?,
                radius: number(table.get("radius"))?,
                way: three(table.get("way")).unwrap_or(Vec3::ZERO),
                half: number(table.get("half")).unwrap_or(0.0),
                aim: three(table.get("aim")).unwrap_or(Vec3::ZERO),
                cone: number(table.get("cone")).unwrap_or(0.0),
            })
        })
        .collect()
}

/// A texture's colours (0 to 1, as stored) at its first level no wider or
/// taller than `side`, row by row, with its width and height.
fn texels(texture: &TextureIr, side: usize) -> Option<(usize, usize, Vec<[f32; 3]>)> {
    let size = |level: usize| {
        (
            (texture.width as usize >> level).max(1),
            (texture.height as usize >> level).max(1),
        )
    };
    let level = (0..texture.mips.len())
        .find(|&level| size(level).0.max(size(level).1) <= side)
        .or(texture.mips.len().checked_sub(1))?;
    let (width, height) = size(level);
    let data = &texture.mips[level];
    let rgba = match texture.encoding {
        Encoding::Rgba8 => (data.len() >= width * height * 4).then(|| data.clone())?,
        Encoding::Bc1 | Encoding::Bc1Holes => dxt::decode(data, width, height, false)?,
        Encoding::Bc3 => dxt::decode(data, width, height, true)?,
    };
    let texels: Vec<[f32; 3]> = (rgba.as_chunks::<4>().0.iter())
        .take(width * height)
        .map(|[r, g, b, _]| [*r, *g, *b].map(|v| f32::from(v) / 255.0))
        .collect();
    (texels.len() == width * height && width > 0 && height > 0).then_some((width, height, texels))
}

fn brightness(texel: &[f32; 3]) -> f32 {
    0.2126 * texel[0] + 0.7152 * texel[1] + 0.0722 * texel[2]
}

/// A texel's strongest channel. A red or an orange lamp is bright by it
/// and dim by `brightness`.
fn strongest(texel: &[f32; 3]) -> f32 {
    texel[0].max(texel[1]).max(texel[2])
}

/// The lights of one kind's fittings (`tools/light-kinds/shine.py`,
/// `lights_of`). A fitting is as `parts` gives a lamp.
fn lights_of(kind: &Kind, fittings: &[[f32; 14]], cut: &[bool], out: &mut Vec<Light>) {
    let colour = kind.colour * kind.strength;
    if kind.gather {
        let mut groups: Vec<Vec<Vec3>> = Vec::new();
        for fitting in fittings {
            let at = Vec3::new(fitting[0], fitting[1], fitting[2]);
            match (groups.iter_mut()).find(|group| group[0].distance(at) < GATHER) {
                Some(group) => group.push(at),
                None => groups.push(vec![at]),
            }
        }
        out.extend(groups.iter().map(|group| Light {
            at: group.iter().sum::<Vec3>() / group.len() as f32 + Vec3::Z * GATHER_UP,
            colour,
            radius: kind.reach,
            way: Vec3::ZERO,
            half: 0.0,
            aim: Vec3::ZERO,
            cone: 0.0,
        }));
        return;
    }
    let off = kind.off.unwrap_or(OFF);
    for (fitting, cut) in fittings.iter().zip(cut) {
        let [x, y, z, fx, fy, fz, _, lx, ly, lz, half, red, green, blue] = *fitting;
        let (facing, own) = (Vec3::new(fx, fy, fz), Vec3::new(red, green, blue));
        let faces = kind.cone > 0.0 && facing.length_squared() > FACES;
        let (way, half) = if kind.line {
            (Vec3::new(lx, ly, lz), half)
        } else if half > STRIP_FROM && kind.form != "down" {
            (Vec3::new(lx, ly, lz), half - STRIP_SHORT)
        } else {
            (Vec3::ZERO, 0.0)
        };
        let grown = if kind.grow {
            (fitting[6] / GROW_WIDTH).clamp(1.0, GROW_MOST)
        } else {
            1.0
        } * if *cut { CUT_SHARE } else { 1.0 };
        out.push(Light {
            at: Vec3::new(x, y, z) + facing * off,
            colour: grown
                * if kind.tint && own.max_element() > 0.0 {
                    own * kind.strength
                } else {
                    colour
                },
            radius: kind.reach,
            way,
            half,
            aim: if faces { facing } else { Vec3::ZERO },
            cone: if faces { kind.cone } else { 0.0 },
        });
    }
}

/// How much of a fight's light is left at a distance (`odyssey.wgsl`,
/// `fight_falloff`).
pub(crate) fn falloff(distance: f32, radius: f32) -> f32 {
    if distance >= radius {
        return 0.0;
    }
    let edge = (1.0 - (distance / radius).powi(4)).max(0.0);
    edge * edge / (1.0 + (distance / (0.45 * radius)).powi(2))
}

/// The fitted lights of a scene: a light on each lamp of each kind the
/// rule data knows, the lights placed by hand in its area, and the rooms'
/// hidden lamps (the module doc).
pub fn lights(loaded: &LoadedScene) -> Vec<Light> {
    explained(loaded)
        .into_iter()
        .map(|(light, _, _)| light)
        .collect()
}

/// `lights`, each with where it comes from: its kind's texture for a
/// fitting's light, `placed` for one placed by hand, `hidden lamp` for a
/// room's hidden lamp; and whether its fitting is on a placeable (the
/// kind's `on`). The level editor shows them.
pub fn explained(loaded: &LoadedScene) -> Vec<(Light, String, bool)> {
    let doors: BTreeSet<usize> = loaded.doors.iter().map(|door| door.model).collect();
    // The doors are among them. A door sheds light only as a force field
    // (`FIELD_CHROMA`).
    let placeables: BTreeSet<usize> = (loaded.instance_models.values().copied())
        .chain(doors.iter().copied())
        .collect();
    let room_of = |model: usize| (loaded.rooms.iter()).position(|room| room.model == Some(model));
    // The glowing meshes of rooms and placeables: the model, the mesh,
    // and whether it is on a placeable.
    let all = kinds();
    let [k1, tsl] = &*all;
    // A kind that shines where its mesh does not glow (`always`).
    let always = |texture: &str, on: bool| {
        let key = (texture.to_ascii_lowercase(), on);
        [k1, tsl]
            .iter()
            .any(|game| matches!(game.get(&key), Some(Some(kind)) if kind.always))
    };
    // How much each lamp of a room that does not glow in the files shines
    // (`switched_on`), by its model and mesh.
    let mut switched: BTreeMap<(usize, usize), f32> = BTreeMap::new();
    for (model, placed) in loaded.models.iter().enumerate() {
        if room_of(model).is_none() {
            continue;
        }
        for (index, mesh) in placed.ir.meshes.iter().enumerate() {
            let share = switched_on(mesh, || {
                let name = mesh.lightmap.as_ref()?;
                crate::viewer::stored_lightmap(name).or_else(|| loaded.textures.get(name).cloned())
            });
            if share > 0.0 {
                switched.insert((model, index), share);
            }
        }
    }
    let glowing: Vec<(usize, usize, bool)> = (loaded.models.iter().enumerate())
        .filter(|(model, _)| room_of(*model).is_some() || placeables.contains(model))
        .flat_map(|(model, placed)| {
            let on = placeables.contains(&model);
            let switched = &switched;
            (placed.ir.meshes.iter().enumerate())
                .filter(move |(index, mesh)| {
                    mesh.skin.is_none()
                        && mesh.texture.as_deref().is_some_and(|texture| {
                            mesh.self_illum.iter().any(|&v| v > 0.0)
                                || always(texture, on)
                                || switched.contains_key(&(model, *index))
                        })
                })
                .map(move |(mesh, _)| (model, mesh, on))
        })
        .collect();
    let name = |model: usize, mesh: usize| {
        (loaded.models[model].ir.meshes[mesh].texture.as_deref())
            .unwrap_or("")
            .to_ascii_lowercase()
    };
    // The scene has no word of which game it is from, and a few textures
    // are in both games' data with other entries. It is of the game whose
    // data knows more of its glowing meshes.
    let known = |game: &Kinds| {
        (glowing.iter())
            .filter(|(model, mesh, on)| game.contains_key(&(name(*model, *mesh), *on)))
            .count()
    };
    let game = if known(tsl) > known(k1) { tsl } else { k1 };

    let mut lamp_textures: BTreeMap<String, Option<(LampTexture, f32)>> = BTreeMap::new();
    let mut found = Vec::new();
    // What each light is from, for `KJK_FITTED_DUMP`.
    let mut sources: Vec<String> = Vec::new();
    // Whether each light's fitting is on a placeable.
    let mut on_placeable: Vec<bool> = Vec::new();
    // The room each light is in, where it is from a room's own mesh.
    let mut rooms: Vec<Option<usize>> = Vec::new();
    // The rooms that have a fitting's light.
    let mut lit_rooms: BTreeSet<usize> = BTreeSet::new();
    let unnamed = unnamed();
    for &(model, mesh_index, on) in &glowing {
        let texture = name(model, mesh_index);
        let door = doors.contains(&model);
        // A texture the data does not name is a screen or a panel
        // (`unnamed`). A door is never a named kind. A texture the data
        // calls no lamp sheds nothing on a door either: the wooden gates
        // of the Sand People's huts are orange enough to pass as a force
        // field.
        let (kind, named) = match game.get(&(texture.clone(), on)) {
            Some(Some(kind)) if !door => (kind, true),
            Some(None) => continue,
            None => (&unnamed, false),
            Some(Some(_)) => (&unnamed, false),
        };
        let placed = &loaded.models[model];
        let mesh = &placed.ir.meshes[mesh_index];
        let node: Mat4 = placement(placed) * to_mat4(&placed.ir.nodes[mesh.node].world);
        let textured = mesh.uv1.len() == mesh.positions.len();
        let (mut corners, mut corner_uv, mut area) = (Vec::new(), Vec::new(), 0.0);
        for &[a, b, c] in mesh.indices.as_chunks::<3>().0 {
            let corner = |i: u32| {
                (mesh.positions.get(i as usize))
                    .map(|p| node.transform_point3(Vec3::from_array(*p)))
            };
            let (Some(pa), Some(pb), Some(pc)) = (corner(a), corner(b), corner(c)) else {
                continue;
            };
            if textured {
                corner_uv.extend([a, b, c].map(|i| Vec2::from_array(mesh.uv1[i as usize])));
            }
            corners.extend([pa, pb, pc]);
            area += 0.5 * (pb - pa).cross(pc - pa).length();
        }
        // What looks like a light, as the survey has it: no sky or
        // backdrop, and lightmapped or bright enough.
        let room = room_of(model);
        let floorless = room.is_some_and(|room| {
            loaded.rooms[room].floor.is_empty() && loaded.rooms[room].floor_error.is_none()
        });
        if floorless || area > SKY_M2 {
            continue;
        }
        let read = lamp_textures.entry(texture).or_insert_with(|| {
            let ir = loaded.textures.get(mesh.texture.as_ref()?)?;
            let (width, height, colours) = texels(ir, LAMP_TEXTURE)?;
            let (_, _, small) = texels(ir, BRIGHTNESS_SIDE)?;
            let bright = small.iter().filter(|texel| strongest(texel) > BRIGHT);
            let share = bright.count() as f32 / small.len() as f32;
            Some((LampTexture::new(width, height, colours), share))
        });
        let lightmapped = (mesh.lightmap.as_ref()).is_some_and(|name| {
            mesh.uv2.len() == mesh.positions.len() && loaded.textures.contains_key(name)
        });
        let bright = (read.as_ref()).is_some_and(|(_, share)| *share >= BRIGHT_SHARE);
        if !(lightmapped || bright || kind.dark) {
            continue;
        }
        let texture = read.as_ref().map(|(texture, _)| texture);
        let (mut pieces, mut lamps) = parts(&corners, &corner_uv, texture, false);
        // A long piece may bend. Its lengths are then the lamps
        // (`lengths`), and the lamps that were not there before are cut.
        let place =
            |fitting: &[f32]| [fitting[0], fitting[1], fitting[2]].map(|v| (v * 100.0) as i32);
        let mut uncut: Option<BTreeSet<[i32; 3]>> = None;
        if pieces.iter().any(|piece| piece[10] > BEND_FROM) {
            uncut = Some(
                (pieces.iter().map(|piece| place(piece)))
                    .chain(lamps.iter().map(|lamp| place(lamp)))
                    .collect(),
            );
            (pieces, lamps) = parts(&corners, &corner_uv, texture, true);
        }
        let mut fittings: Vec<[f32; 14]> = if kind.bright {
            lamps
        } else {
            (pieces.iter())
                .map(|piece| {
                    let mut fitting = [0.0; 14];
                    fitting[..11].copy_from_slice(piece);
                    fitting
                })
                .collect()
        };
        if let Some(longest) = kind.longest {
            fittings.retain(|fitting| 2.0 * fitting[10] <= longest);
        }
        if !named {
            // A small piece with no bright texel, a beacon or a crystal,
            // has its texture's own colour. So has a door's force field,
            // whose dim layers add up to a bright one.
            let small = |fitting: &[f32; 14]| {
                fitting[11].max(fitting[12]).max(fitting[13]) <= 0.0
                    && (door || fitting[6] <= UNNAMED_SMALL)
            };
            let dim = fittings.iter().any(small);
            let whole = (dim.then_some(mesh.texture.as_ref()).flatten())
                .and_then(|name| texels(loaded.textures.get(name)?, BRIGHTNESS_SIDE))
                .map(|(_, _, texels)| {
                    let sum: Vec3 = texels.iter().map(|texel| Vec3::from_array(*texel)).sum();
                    sum / sum.max_element().max(1e-6)
                })
                .filter(|colour| colour.is_finite());
            if let Some(whole) = whole {
                for fitting in &mut fittings {
                    if small(fitting) {
                        fitting[11..14].copy_from_slice(&whole.to_array());
                    }
                }
            }
            // Only its bright patches shine, and no wall or backdrop does.
            // A door shines only as a force field.
            fittings.retain(|fitting| {
                let own = Vec3::new(fitting[11], fitting[12], fitting[13]);
                own.max_element() > 0.0
                    && fitting[6] <= UNNAMED_WIDEST
                    && (!door || own.max_element() - own.min_element() >= FIELD_CHROMA)
            });
        }
        let cut: Vec<bool> = (fittings.iter())
            .map(|fitting| (uncut.as_ref()).is_some_and(|uncut| !uncut.contains(&place(fitting))))
            .collect();
        if !fittings.is_empty() {
            // A placeable's fitting lights the room it stands in.
            let at = Vec3::new(fittings[0][0], fittings[0][1], fittings[0][2]);
            lit_rooms.extend(room.or_else(|| room_over(loaded, at)));
        }
        let before = found.len();
        lights_of(kind, &fittings, &cut, &mut found);
        if let Some(share) = switched.get(&(model, mesh_index)) {
            for light in &mut found[before..] {
                light.colour *= *share * SWITCHED_SHARE;
            }
        }
        sources.extend((before..found.len()).map(|_| name(model, mesh_index)));
        on_placeable.resize(found.len(), on);
        rooms.resize(found.len(), room);
    }
    found.extend(placed(module_of(&loaded.name)));
    sources.resize(found.len(), "placed".to_string());
    let fittings = found.len();
    found.extend(fill(loaded, &lit_rooms));
    sources.resize(found.len(), "hidden lamp".to_string());
    // Any other light is in the room whose floor lies under it.
    let known = rooms.len();
    rooms.extend(found[known..].iter().map(|_| None));
    for (light, room) in found.iter().zip(&mut rooms) {
        *room = room.or_else(|| room_over(loaded, light.at));
    }
    level(loaded, &mut found, &rooms, fittings);

    if let Ok(path) = std::env::var("KJK_FITTED_DUMP") {
        dump(loaded, &found, &sources, &switched, &path);
    }
    on_placeable.resize(found.len(), false);
    (found.into_iter().zip(sources).zip(on_placeable))
        .map(|((light, source), on)| (light, source, on))
        .collect()
}

/// What `light` gives a surface at `at` that faces `normal`, by its
/// strongest channel, as `odyssey.wgsl`'s `fitted_lights` works it out.
fn gives(light: &Light, at: Vec3, normal: Option<Vec3>) -> f32 {
    let away = light.at - at;
    let to = away + light.way * (-away.dot(light.way)).clamp(-light.half, light.half);
    let distance = to.length();
    if distance >= light.radius {
        return 0.0;
    }
    let toward = to / distance.max(1e-4);
    let facing = normal.map_or(1.0, |normal| 0.3 + 0.7 * normal.dot(toward).max(0.0));
    let cone = if light.cone > 0.0 {
        let out = -light.aim.dot(toward);
        let t = ((out - (light.cone - 0.5)) / 0.6).clamp(0.0, 1.0);
        0.33 + 0.67 * t * t * (3.0 - 2.0 * t)
    } else {
        1.0
    };
    light.colour.max_element() * falloff(distance, light.radius) * facing * cone
}

/// Keeps each room about as bright as it is today. It works out what
/// each piece of a room's baked surfaces shows with the lights on
/// (`shown`, from the piece's own baked light), and holds the room to
/// `LEVEL_OVER`, `LEVEL_LIFT`, `LEVEL_HOT` and `LEVEL_PEAK`. Where a room
/// shows more, its lights are made weaker by one share, so the lamps keep
/// their pools. The faint lights have a share of their own
/// (`LEVEL_FAINT`). The rooms' hidden lamps, the lights from `hidden` on,
/// have the third share: what the fittings leave. A large room's many hidden
/// lamps were faint lights at first, and together they took the whole of the
/// faint lights' part. Its terminals then shed nothing, and the owner said
/// on 18 November 2026: "no light from this terminal for some reason".
///
/// The owner's rule is that a room's general level stays about as it is,
/// and that a hot spot reads as wrong. Each kind's strength was approved
/// in a picture of a few fittings. A room with hundreds of them, as the
/// Endar Spire's halls have, was washed out white. He walked it on 17
/// November 2026 with a first, looser form of this rule and said of its
/// halls and door openings: "too bright". He also said its screens and
/// panels gave too little light, because that form made them weaker too.
fn level(loaded: &LoadedScene, lights: &mut [Light], rooms: &[Option<usize>], hidden: usize) {
    let mut kept: Vec<(f32, f32, f32)> = vec![(1.0, 1.0, 1.0); loaded.rooms.len()];
    let mut maps: HashMap<Option<String>, Option<LampTexture>> = HashMap::new();
    let mut said: Vec<String> = Vec::new();
    for (room, view) in loaded.rooms.iter().enumerate() {
        let Some(model) = view.model else {
            continue;
        };
        // What the lights give each piece of the room's baked surfaces:
        // how large the piece is, and how much it gets.
        let placed = &loaded.models[model];
        let frame = placement(placed);
        let mut pieces: Vec<(f32, f32, f32, f32, f32)> = Vec::new();
        for mesh in (placed.ir.meshes.iter()).filter(|mesh| mesh.lightmap.is_some()) {
            let map = maps.entry(mesh.lightmap.clone()).or_insert_with(|| {
                // A baked lightmap drawn in the stored one's place is not
                // the light the rooms have today.
                let name = mesh.lightmap.as_ref()?;
                let stored = crate::viewer::stored_lightmap(name);
                let (width, height, map) =
                    texels(stored.as_ref().or(loaded.textures.get(name))?, 128)?;
                Some(LampTexture::new(width, height, map))
            });
            let Some(map) = map
                .as_ref()
                .filter(|_| mesh.uv2.len() == mesh.positions.len())
            else {
                continue;
            };
            let node = frame * to_mat4(&placed.ir.nodes[mesh.node].world);
            let corner = |i: u32| {
                let at = node.transform_point3(Vec3::from_array(*mesh.positions.get(i as usize)?));
                let normal = (mesh.normals.get(i as usize))
                    .map_or(Vec3::ZERO, |n| node.transform_vector3(Vec3::from_array(*n)));
                Some((at, Vec2::from_array(mesh.uv2[i as usize]), normal))
            };
            for &[a, b, c] in mesh.indices.as_chunks::<3>().0 {
                let (Some(a), Some(b), Some(c)) = (corner(a), corner(b), corner(c)) else {
                    continue;
                };
                let mut whole = vec![[a, b, c]];
                while let Some([a, b, c]) = whole.pop() {
                    let front = (b.0 - a.0).cross(c.0 - a.0);
                    let size = 0.5 * front.length();
                    if size > LEVEL_PIECE && whole.len() < 4096 {
                        let half = |p: (Vec3, Vec2, Vec3), q: (Vec3, Vec2, Vec3)| {
                            ((p.0 + q.0) / 2.0, (p.1 + q.1) / 2.0, p.2 + q.2)
                        };
                        let (ab, bc, ca) = (half(a, b), half(b, c), half(c, a));
                        whole.extend([[a, ab, ca], [ab, b, bc], [ca, bc, c], [ab, bc, ca]]);
                        continue;
                    }
                    if size <= 1e-6 {
                        continue;
                    }
                    // A mesh without normals is lit from every side alike,
                    // as the shader lights it.
                    let (middle, normal) =
                        ((a.0 + b.0 + c.0) / 3.0, (a.2 + b.2 + c.2).try_normalize());
                    let local = brightness(&map.at(((a.1 + b.1 + c.1) / 3.0).to_array()));
                    // What the faint lights give it, what the other
                    // fittings do, and what the hidden lamps do.
                    let (mut faint, mut strong, mut fill) = (0.0, 0.0, 0.0);
                    for (index, light) in lights.iter().enumerate() {
                        let given = gives(light, middle, normal);
                        if index >= hidden {
                            fill += given;
                        } else if light.colour.max_element() <= LEVEL_FAINT {
                            faint += given;
                        } else {
                            strong += given;
                        }
                    }
                    pieces.push((size, faint, strong, fill, local));
                }
            }
        }
        let area: f32 = pieces.iter().map(|piece| piece.0).sum();
        if area <= 0.0 {
            continue;
        }
        // What the room shows today, over all its surfaces.
        let today = pieces.iter().map(|piece| piece.0 * piece.4).sum::<f32>() / area;
        // The room's level with the strong lights at `share` of their
        // strength: over all its surfaces, the level its brightest
        // twentieth is over, and the level its brightest hundredth is over.
        let levels = |low: f32, share: f32, last: f32| {
            let mut each: Vec<(f32, f32)> = (pieces.iter())
                .map(|(size, faint, strong, fill, local)| {
                    let given = low * faint + share * strong + last * fill;
                    (*size, shown(*local, given))
                })
                .collect();
            let all = each.iter().map(|(size, level)| size * level).sum::<f32>() / area;
            each.sort_by(|a, b| b.1.total_cmp(&a.1));
            let (mut over, mut hot, mut peak) = (0.0, 0.0, None);
            for (size, level) in &each {
                over += size;
                hot = *level;
                if over >= LEVEL_PEAK_SHARE * area {
                    peak.get_or_insert(hot);
                }
                if over >= LEVEL_HOT_SHARE * area {
                    break;
                }
            }
            (all, hot, peak.unwrap_or(hot))
        };
        let bright = (today - LEVEL_BRIGHT.0) / (LEVEL_BRIGHT.1 - LEVEL_BRIGHT.0);
        let hot_most = LEVEL_HOT + (LEVEL_HOT_BRIGHT - LEVEL_HOT) * bright.clamp(0.0, 1.0);
        let fits = |low: f32, share: f32, last: f32, part: f32| {
            let (all, hot, peak) = levels(low, share, last);
            all <= (LEVEL_OVER * today).max(LEVEL_LIFT)
                && hot <= part * hot_most
                && peak <= part * LEVEL_PEAK * hot_most
        };
        // The most of 0 to 1 that `fits`.
        let most = |fits: &dyn Fn(f32) -> bool| {
            if fits(1.0) {
                return 1.0;
            }
            let (mut low, mut high) = (0.0f32, 1.0f32);
            for _ in 0..12 {
                let middle = (low + high) / 2.0;
                if fits(middle) {
                    low = middle;
                } else {
                    high = middle;
                }
            }
            low
        };
        // The faint lights first, alone, in their part of what the room
        // may show. Then the other fittings with them, and the hidden
        // lamps last.
        let low = most(&|low| fits(low, 0.0, 0.0, LEVEL_FAINT_PART));
        let share = most(&|share| fits(low, share, 0.0, 1.0));
        let last = most(&|last| fits(low, share, last, 1.0));
        kept[room] = (low, share, last);
        if low < 1.0 || share < 1.0 || last < 1.0 {
            said.push(format!(
                "{} {share:.2} (faint {low:.2}, hidden {last:.2})",
                view.name
            ));
        }
    }
    for (index, (light, room)) in lights.iter_mut().zip(rooms).enumerate() {
        if let Some(room) = room {
            let (low, share, last) = kept[*room];
            light.colour *= if index >= hidden {
                last
            } else if light.colour.max_element() > LEVEL_FAINT {
                share
            } else {
                low
            };
        }
    }
    if !said.is_empty() {
        bevy::log::info!(
            "fitted lights: rooms whose lights are made weaker, and what they keep: {}",
            said.join(", ")
        );
    }
}

/// The stored level of light a surface shows, where its baked light is
/// `baked` and the fitted lights give it `given`
/// (`odyssey.wgsl`): half of the baked light, and the lights' sum read as a
/// stored level, added as linear light.
fn shown(baked: f32, given: f32) -> f32 {
    ((baked / 2.0).powf(2.2) + (0.8 * given).powf(2.2)).powf(1.0 / 2.2)
}

/// `KJK_FITTED_DUMP=<file>`: every textured mesh of the rooms and
/// placeables (its model, texture, glow, middle and size) and every fitted
/// light (what it is from, its place, colour and reach), a line each, to
/// find why a fitting gives no light. Each light also has a line that
/// starts with `fitted`, which `kjk detail-bake --lights` reads. It holds,
/// in full precision and in game space, the light's place (3 numbers), its
/// colour (3), its radius, the way a strip runs (3), half the strip's
/// length, the way the light shines (3) and its cone. The lights are
/// written after `level` has made them weaker, so they are the lights the
/// shader's grid is built from. A room's lamp that shines although the
/// files do not set it to glow (`switched_on`) has a line that starts with
/// `switched`: its model, the number of its mesh in the model, and how much
/// it shines. The bake lets such a lamp glow as the game does.
fn dump(
    loaded: &LoadedScene,
    lights: &[Light],
    sources: &[String],
    switched: &BTreeMap<(usize, usize), f32>,
    path: &str,
) {
    use std::fmt::Write;
    let mut text = String::new();
    for ((model, mesh), share) in switched {
        let _ = writeln!(
            text,
            "switched {} {mesh} {share}",
            loaded.models[*model].ir.name
        );
    }
    for placed in &loaded.models {
        let frame = placement(placed);
        for mesh in &placed.ir.meshes {
            let Some(texture) = mesh.texture.as_deref() else {
                continue;
            };
            let node = frame * to_mat4(&placed.ir.nodes[mesh.node].world);
            let corners =
                (mesh.positions.iter()).map(|p| node.transform_point3(Vec3::from_array(*p)));
            let low = corners.clone().fold(Vec3::MAX, Vec3::min);
            let high = corners.fold(Vec3::MIN, Vec3::max);
            let _ = writeln!(
                text,
                "mesh {} {texture} glow {:?} lightmap {:?} middle {:.1?} size {:.1?}",
                placed.ir.name,
                mesh.self_illum,
                mesh.lightmap,
                ((low + high) / 2.0).to_array(),
                (high - low).to_array()
            );
        }
    }
    for (light, source) in lights.iter().zip(sources) {
        let _ = writeln!(
            text,
            "light {source} at {:.1?} colour {:.2?} reach {:.1} half {:.1} cone {:.1}",
            light.at.to_array(),
            light.colour.to_array(),
            light.radius,
            light.half,
            light.cone
        );
    }
    for light in lights {
        let numbers = [
            light.at.to_array(),
            light.colour.to_array(),
            [light.radius, 0.0, 0.0],
            light.way.to_array(),
            [light.half, 0.0, 0.0],
            light.aim.to_array(),
            [light.cone, 0.0, 0.0],
        ];
        let line: Vec<String> = (numbers.iter().zip([3, 3, 1, 3, 1, 3, 1]))
            .flat_map(|(values, count)| values[..count].iter().map(|v| format!("{v}")))
            .collect();
        let _ = writeln!(text, "fitted {}", line.join(" "));
    }
    if let Err(error) = std::fs::write(path, text) {
        bevy::log::warn!("KJK_FITTED_DUMP: {path}: {error}");
    }
}

/// The room whose floor lies under `at` (game space).
fn room_over(loaded: &LoadedScene, at: Vec3) -> Option<usize> {
    let side =
        |a: &[f32; 3], b: &[f32; 3]| (b[0] - a[0]) * (at.y - a[1]) - (b[1] - a[1]) * (at.x - a[0]);
    (loaded.rooms.iter()).position(|room| {
        room.floor.iter().any(|[a, b, c]| {
            let sides = [side(a, b), side(b, c), side(c, a)];
            sides.iter().all(|v| *v >= 0.0) || sides.iter().all(|v| *v <= 0.0)
        })
    })
}

/// The rooms' hidden lamps as lights that reach the rooms
/// (`tools/light-kinds/fill.py`). Each room has its own strength, which
/// puts back the half of that room's baked light. One strength for the
/// whole area made a room with many far-reaching lamps, the Harbinger's
/// medical bay, twice as bright as it is today (the owner, 17 November
/// 2026: "this area is too bright").
fn fill(loaded: &LoadedScene, lit_rooms: &BTreeSet<usize>) -> Vec<Light> {
    // Each room's lamps that are not ambient only: its room, its place,
    // its radius and its colour as it lights a room.
    let mut lamps: Vec<(usize, Vec3, f32, Vec3)> = Vec::new();
    for (room, view) in loaded.rooms.iter().enumerate() {
        let Some(placed) = view.model.map(|model| &loaded.models[model]) else {
            continue;
        };
        let frame = placement(placed);
        for light in placed.ir.lights.iter().filter(|light| !light.ambient_only) {
            let at =
                (frame * to_mat4(&placed.ir.nodes[light.node].world)).transform_point3(Vec3::ZERO);
            let mut sorted = light.color;
            sorted.sort_by(f32::total_cmp);
            let strong = sorted[2] > 2.0 * sorted[1];
            let power = Vec3::from_array(light.color)
                * light.multiplier
                * if strong { STRONG_COLOUR } else { 1.0 };
            lamps.push((room, at, light.radius, power));
        }
    }
    let mut found = Vec::new();
    let mut said: Vec<String> = Vec::new();
    for (room, view) in loaded.rooms.iter().enumerate() {
        let own: Vec<&(usize, Vec3, f32, Vec3)> =
            lamps.iter().filter(|lamp| lamp.0 == room).collect();
        let Some(placed) = view.model.map(|model| &loaded.models[model]) else {
            continue;
        };
        if own.is_empty() {
            continue;
        }
        // The mean brightness of the room's lightmaps.
        let names: BTreeSet<&String> = (placed.ir.meshes.iter())
            .filter_map(|mesh| mesh.lightmap.as_ref())
            .collect();
        let (mut sum, mut count) = (0.0, 0usize);
        for name in names {
            let read = (loaded.textures.get(name)).and_then(|ir| texels(ir, BRIGHTNESS_SIDE));
            if let Some((_, _, map)) = read {
                sum += map.iter().map(brightness).sum::<f32>();
                count += map.len();
            }
        }
        if count == 0 {
            continue;
        }
        let mean = sum / count as f32;
        // What every lamp of the area gives under a usual lamp of the room.
        let mut under: Vec<f32> = (own.iter())
            .map(|(_, at, ..)| {
                let below = *at - Vec3::Z * FILL_UNDER;
                (lamps.iter())
                    .map(|(_, other, radius, power)| {
                        power.max_element() * falloff(below.distance(*other), *radius)
                    })
                    .sum()
            })
            .collect();
        under.sort_by(f32::total_cmp);
        let middle = under[under.len() / 2];
        if middle <= 0.0 {
            continue;
        }
        let kept = if lit_rooms.contains(&room) {
            FILL_BESIDE
        } else {
            1.0
        };
        let scale = (FILL_GAIN * mean / 2.0 / middle).clamp(FILL_RANGE.0, FILL_RANGE.1) * kept;
        said.push(format!("{} {scale:.2}", view.name));
        found.extend(own.iter().map(|(_, at, radius, power)| Light {
            at: *at,
            colour: *power * scale,
            radius: *radius,
            way: Vec3::ZERO,
            half: 0.0,
            aim: Vec3::ZERO,
            cone: 0.0,
        }));
    }
    bevy::log::info!(
        "fitted lights: {} hidden lamps, each room's strength: {}",
        found.len(),
        said.join(", ")
    );
    found
}

/// How many pieces of a glowing mesh are kept, and how many lamps.
const PARTS: usize = 48;
const LAMPS: usize = 96;
/// A texel whose strongest channel is at least this (0 to 1, as stored) is
/// part of a lamp's bright face. The trial shader keeps a surface's own
/// glow from about here up (`dull_glow` in `odyssey.wgsl`).
const LAMP_TEXEL: f32 = 0.75;
const LAMP_TOP: f32 = 0.88;
/// The longest side at which a lamp's texture is read (texels).
pub const LAMP_TEXTURE: usize = 128;
/// A welded piece with at least this share of its face bright is one lamp
/// as it is.
const WHOLE: f32 = 0.9;
/// A welded piece with less than this share of its face bright is taken
/// as it is too: its few bright texels are no lamp.
const DIM: f32 = 0.1;
/// How many places of one welded piece are tried for bright texels, and
/// how many for a first rough look.
const SPOTS: usize = 30_000;
const ROUGH_SPOTS: usize = 1_500;
/// The least distance between those places (metres).
const SPOT_FINEST: f32 = 0.01;
/// A bright patch smaller than this share of its piece's largest is a
/// speck (a rivet, a status light) and no lamp.
const SPECK: f32 = 0.15;
/// A lamp whose facing rises by at least `TILTED_UP` of its length and by
/// less than `STRAIGHT_UP` shines level (`parts`).
const TILTED_UP: f32 = 0.6;
const STRAIGHT_UP: f32 = 0.95;
/// How far a place may lie from where its texture coordinates put it, as
/// a share of the repeat's size, and still be in the same repeat as the
/// repeat's first place. A curved face bends away a little.
const SAME_REPEAT: f32 = 0.25;
/// Bright patches of one piece whose middles are nearer than this are one
/// lamp (metres). The tubes of one fitting are a few decimetres apart.
/// Lamps that light pools of their own are a metre apart or more.
const ONE_LAMP: f32 = 0.6;
/// A lamp at least this many times longer than thick is a tube, and a
/// tube is no thicker than `TUBE_THICK` metres.
const SLENDER: f32 = 2.0;
const TUBE_THICK: f32 = 0.5;
/// Two tubes that lie end to end are one lamp when the gap between them is
/// at most this share of their thickness, and at most `JOIN_GAP` metres: a
/// clamp at a tube's middle cuts it in two.
const JOIN_THICK: f32 = 0.4;
const JOIN_GAP: f32 = 0.08;

/// A lamp's texture and how bright a texel of it must be to be part of
/// the lamp's bright face: its strongest channel at least `LAMP_TEXEL`,
/// and at least `LAMP_TOP` of the texture's brightest texels (the top
/// fiftieth), so a white tube on a light grey panel is told from the panel.
pub struct LampTexture {
    width: usize,
    height: usize,
    texels: Vec<[f32; 3]>,
    bright: f32,
}

impl LampTexture {
    /// A texture of `width` by `height` texels, row by row, each 0 to 1
    /// as stored.
    pub fn new(width: usize, height: usize, texels: Vec<[f32; 3]>) -> LampTexture {
        let mut tops: Vec<f32> = (texels.iter())
            .map(|texel| texel[0].max(texel[1]).max(texel[2]))
            .collect();
        tops.sort_by(f32::total_cmp);
        let top = tops.get(tops.len() * 49 / 50).copied().unwrap_or(1.0);
        LampTexture {
            width,
            height,
            texels,
            bright: LAMP_TEXEL.max(LAMP_TOP * top),
        }
    }

    /// The texel at `uv`, nearest, repeating.
    fn at(&self, uv: [f32; 2]) -> [f32; 3] {
        let x = ((uv[0] * self.width as f32).floor() as i64).rem_euclid(self.width as i64);
        let y = ((uv[1] * self.height as f32).floor() as i64).rem_euclid(self.height as i64);
        self.texels[y as usize * self.width + x as usize]
    }
}

/// A place on a mesh's face whose texel is bright.
struct Spot {
    at: Vec3,
    /// Its texture coordinates, and how far a step of 1 in each moves it on
    /// the face (zeros where the face's texture coordinates are a line).
    uv: Vec2,
    across: [Vec3; 2],
    /// The face's front there, of length 1.
    front: Vec3,
    /// How much of the face it stands for (square metres).
    area: f32,
    colour: Vec3,
}

/// One lamp of a mesh, as `Glow::parts` gives it.
struct Found {
    area: f32,
    centre: Vec3,
    facing: Vec3,
    /// The way it is longest and half its length that way.
    way: Vec3,
    half: f32,
    /// How far it reaches across that way (metres).
    thick: f32,
    colour: Vec3,
}

impl Found {
    fn part(&self) -> [f32; 14] {
        [
            self.centre.x,
            self.centre.y,
            self.centre.z,
            self.facing.x,
            self.facing.y,
            self.facing.z,
            self.area.sqrt(),
            self.way.x,
            self.way.y,
            self.way.z,
            self.half,
            self.colour.x,
            self.colour.y,
            self.colour.z,
        ]
    }
}

/// How far `points` reach from the line through `centre` along `way`,
/// both sides together.
fn thickness(points: &[Vec3], centre: Vec3, way: Vec3) -> f32 {
    2.0 * points
        .iter()
        .map(|point| {
            let off = *point - centre;
            (off - way * off.dot(way)).length()
        })
        .fold(0.0, f32::max)
}

/// A welded piece as one lamp: what `parts` gave before it read textures.
fn whole(piece: &[Vec3]) -> Option<Found> {
    let (mut area, mut centre, mut facing) = (0.0f32, Vec3::ZERO, Vec3::ZERO);
    for triangle in piece.as_chunks::<3>().0 {
        let front = (triangle[1] - triangle[0]).cross(triangle[2] - triangle[0]);
        let size = 0.5 * front.length();
        area += size;
        centre += size * (triangle[0] + triangle[1] + triangle[2]) / 3.0;
        facing += 0.5 * front;
    }
    if area <= 0.0 {
        return None;
    }
    let (centre, facing) = (centre / area, facing / area);
    let [lx, ly, lz, half] = longest(piece, centre);
    let way = Vec3::new(lx, ly, lz);
    Some(Found {
        area,
        centre,
        facing,
        way,
        half,
        thick: thickness(piece, centre, way),
        colour: Vec3::ZERO,
    })
}

/// Bright texels' colours as a light's colour: the strongest channel is 1.
/// `sum` is their colours added up, each weighed by how coloured it is, so
/// a tube's white middle does not wash out its red rim.
fn light_colour(sum: Vec3) -> Vec3 {
    let top = sum.max_element();
    if top > 0.0 { sum / top } else { Vec3::ZERO }
}

/// The bright places of a welded piece, given its triangles' corners and
/// texture coordinates three by three, and how far apart the places are
/// (metres). About `budget` places are tried at most. Where that allows,
/// they lie closer than half a texel.
fn bright_spots(
    piece: &[Vec3],
    uv: &[Vec2],
    texture: &LampTexture,
    budget: usize,
) -> (Vec<Spot>, f32) {
    let bright = texture.bright;
    let size = Vec2::new(texture.width as f32, texture.height as f32);
    let (mut total, mut texel) = (0.0f32, f32::MAX);
    for (corners, uv) in piece.as_chunks::<3>().0.iter().zip(uv.as_chunks::<3>().0) {
        let area = 0.5
            * (corners[1] - corners[0])
                .cross(corners[2] - corners[0])
                .length();
        let texels = 0.5
            * ((uv[1] - uv[0]) * size)
                .perp_dot((uv[2] - uv[0]) * size)
                .abs();
        total += area;
        if area > 1e-8 && texels > 1e-6 {
            texel = texel.min((area / texels).sqrt());
        }
    }
    let step = (if texel == f32::MAX { 0.05 } else { 0.4 * texel })
        .max(SPOT_FINEST)
        .max((total / budget as f32).sqrt());
    let mut spots = Vec::new();
    for (corners, uv) in piece.as_chunks::<3>().0.iter().zip(uv.as_chunks::<3>().0) {
        // Rows of places along the triangle's longest side, up to the
        // corner across from it.
        let side = |k: usize| (corners[(k + 1) % 3] - corners[k]).length_squared();
        let first = (0..3)
            .max_by(|a, b| side(*a).total_cmp(&side(*b)))
            .unwrap_or(0);
        let [a, b, c] = [first, (first + 1) % 3, (first + 2) % 3];
        let (along, up) = (corners[b] - corners[a], corners[c] - corners[a]);
        let front = along.cross(up);
        let area = 0.5 * front.length();
        let base = along.length();
        if area <= 1e-10 || base <= 0.0 {
            continue;
        }
        let front = front.normalize();
        let (du, dv) = (uv[b] - uv[a], uv[c] - uv[a]);
        let turn = du.perp_dot(dv);
        let moves = if turn.abs() > 1e-9 {
            [
                (along * dv.y - up * du.y) / turn,
                (up * du.x - along * dv.x) / turn,
            ]
        } else {
            [Vec3::ZERO; 2]
        };
        let rows = ((2.0 * area / base / step).ceil() as usize).max(1);
        let across = (base / step).ceil();
        for row in 0..rows {
            let (low, high) = (row as f32 / rows as f32, (row + 1) as f32 / rows as f32);
            let t = 0.5 * (low + high);
            let count = ((across * (1.0 - t)).ceil() as usize).max(1);
            let each = area * ((1.0 - low).powi(2) - (1.0 - high).powi(2)) / count as f32;
            for at in 0..count {
                let s = (at as f32 + 0.5) / count as f32 * (1.0 - t);
                let at_uv = uv[a] + (uv[b] - uv[a]) * s + (uv[c] - uv[a]) * t;
                let colour = Vec3::from_array(texture.at(at_uv.to_array()));
                if colour.max_element() >= bright {
                    spots.push(Spot {
                        at: corners[a] + along * s + up * t,
                        uv: at_uv,
                        across: moves,
                        front,
                        area: each,
                        colour,
                    });
                }
            }
        }
    }
    (spots, step)
}

/// The lamps among a piece's bright places: places nearer each other than
/// one and a half `step` are one lamp. Specks are left out.
fn patches(spots: &[Spot], step: f32) -> Vec<Found> {
    let reach = 1.5 * step;
    let cell = |at: Vec3| (at / reach).floor().as_ivec3().to_array();
    let mut cells: HashMap<[i32; 3], Vec<usize>> = HashMap::new();
    for (index, spot) in spots.iter().enumerate() {
        cells.entry(cell(spot.at)).or_default().push(index);
    }
    let mut parent: Vec<usize> = (0..spots.len()).collect();
    for (index, spot) in spots.iter().enumerate() {
        let [x, y, z] = cell(spot.at);
        for near in 0..27 {
            let key = [x + near % 3 - 1, y + near / 3 % 3 - 1, z + near / 9 - 1];
            let Some(others) = cells.get(&key) else {
                continue;
            };
            for &other in others {
                if other > index && spots[other].at.distance_squared(spot.at) <= reach * reach {
                    let (a, b) = (find(&mut parent, index), find(&mut parent, other));
                    parent[b] = a;
                }
            }
        }
    }
    // A lamp's texture shows one fitting, so the patches of one repeat of
    // it are one lamp: the four tubes of a panel. Two faces may show the
    // same repeat at two places, so a place joins the repeat's first place
    // only where it lies as the texture coordinates say it should.
    let mut repeats: HashMap<[i32; 2], Vec<usize>> = HashMap::new();
    for (index, spot) in spots.iter().enumerate() {
        let firsts = repeats
            .entry(spot.uv.floor().as_ivec2().to_array())
            .or_default();
        let same = firsts.iter().copied().find(|&first| {
            let first = &spots[first];
            let [u, v] = first.across;
            let moved = spot.uv - first.uv;
            let off = spot.at - (first.at + u * moved.x + v * moved.y);
            off.length() <= SAME_REPEAT * u.length().max(v.length())
        });
        match same {
            Some(first) => {
                let (a, b) = (find(&mut parent, first), find(&mut parent, index));
                parent[b] = a;
            }
            None => firsts.push(index),
        }
    }
    let mut groups: BTreeMap<usize, Vec<usize>> = BTreeMap::new();
    for index in 0..spots.len() {
        groups
            .entry(find(&mut parent, index))
            .or_default()
            .push(index);
    }
    // Patches whose middles are near each other are one lamp too: the
    // dots of a grille whose texture repeats every few centimetres.
    let middles: Vec<(usize, Vec3)> = (groups.iter())
        .map(|(root, group)| {
            let area: f32 = group.iter().map(|&index| spots[index].area).sum();
            let middle: Vec3 = (group.iter())
                .map(|&index| spots[index].area * spots[index].at)
                .sum();
            (*root, middle / area)
        })
        .collect();
    for (at, (root, middle)) in middles.iter().enumerate() {
        for (other, other_middle) in &middles[at + 1..] {
            if middle.distance(*other_middle) < ONE_LAMP {
                let (a, b) = (find(&mut parent, *root), find(&mut parent, *other));
                parent[b] = a;
            }
        }
    }
    let mut groups: BTreeMap<usize, Vec<usize>> = BTreeMap::new();
    for index in 0..spots.len() {
        groups
            .entry(find(&mut parent, index))
            .or_default()
            .push(index);
    }
    let mut found: Vec<Found> = (groups.values())
        .map(|group| {
            let (mut area, mut centre, mut facing) = (0.0f32, Vec3::ZERO, Vec3::ZERO);
            let (mut colour, mut places) = (Vec3::ZERO, Vec::with_capacity(group.len()));
            for &index in group {
                let spot = &spots[index];
                area += spot.area;
                centre += spot.area * spot.at;
                facing += spot.area * spot.front;
                let chroma = spot.colour.max_element() - spot.colour.min_element();
                colour += spot.area * (chroma * chroma + 0.02) * spot.colour;
                places.push(spot.at);
            }
            let (centre, facing) = (centre / area, facing / area);
            let [lx, ly, lz, half] = longest(&places, centre);
            let way = Vec3::new(lx, ly, lz);
            Found {
                area,
                centre,
                facing,
                way,
                // The outermost places stand half a step inside the edge.
                half: half + 0.5 * step,
                thick: thickness(&places, centre, way) + step,
                colour: light_colour(colour),
            }
        })
        .collect();
    let largest = found.iter().map(|lamp| lamp.area).fold(0.0, f32::max);
    found.retain(|lamp| lamp.area >= SPECK * largest);
    found
}

/// Joins tubes that lie end to end (`JOIN_THICK`).
fn join_tubes(found: &mut Vec<Found>) {
    // Half a tube is at least as long as thick, and no tube is thicker
    // than `TUBE_THICK`.
    let tube = |lamp: &Found| {
        lamp.thick > 0.0 && lamp.thick <= TUBE_THICK && 2.0 * lamp.half >= lamp.thick
    };
    loop {
        let mut pair = None;
        'search: for a in 0..found.len() {
            for b in a + 1..found.len() {
                let (first, second) = (&found[a], &found[b]);
                if !tube(first) || !tube(second) {
                    continue;
                }
                let apart = second.centre - first.centre;
                let Some(way) = apart.try_normalize() else {
                    continue;
                };
                let thick = first.thick.max(second.thick);
                let gap = apart.length() - first.half - second.half;
                if 2.0 * (first.half + second.half) >= SLENDER * thick
                    && way.dot(first.way).abs() >= 0.9
                    && way.dot(second.way).abs() >= 0.9
                    && gap >= -0.25 * thick
                    && gap <= (JOIN_THICK * thick).min(JOIN_GAP)
                {
                    pair = Some((a, b, way, apart.length()));
                    break 'search;
                }
            }
        }
        let Some((a, b, way, apart)) = pair else {
            return;
        };
        let second = found.swap_remove(b);
        let first = &mut found[a];
        let area = first.area + second.area;
        let half = 0.5 * (first.half + apart + second.half);
        first.centre += way * (half - first.half);
        first.facing = (first.facing * first.area + second.facing * second.area) / area;
        first.colour = (first.colour * first.area + second.colour * second.area) / area;
        first.thick = first.thick.max(second.thick);
        (first.area, first.way, first.half) = (area, way, half);
    }
}

/// The pieces and the lamps of a mesh, given its triangles' corners three
/// by three, and their texture coordinates and the texture where the mesh
/// has them.
///
/// Triangles that share a corner (to the millimetre) are one piece. The
/// pieces are as `Glow::parts` says, the largest first.
///
/// The lamps are as `Glow::lamps` says. A piece whose texture is bright all
/// over, or hardly at all, is one lamp. In any other piece each bright
/// patch of the face is a lamp, at the middle of its bright texels and
/// facing as they do. The patches of one repeat of the texture are one
/// lamp, and so are patches nearer than `ONE_LAMP`: a panel of four tubes
/// is one lamp, and a band that repeats its texture has one lamp for each
/// repeat. Tubes cut in two by a clamp are joined. A lamp that faces
/// more up than out shines level.
///
/// With `follow` a long piece that bends, such as a strip along a curved
/// hall, is cut into lengths of `BEND_LENGTH` first, and each is a piece.
/// The game asks for that. The survey does not, so its numbers stay as
/// the owner approved them.
pub fn parts(
    corners: &[Vec3],
    uv: &[Vec2],
    texture: Option<&LampTexture>,
    follow: bool,
) -> (Vec<[f32; 11]>, Vec<[f32; 14]>) {
    let mut keys: HashMap<[i32; 3], usize> = HashMap::new();
    let mut parent: Vec<usize> = Vec::new();
    let mut of_corner = Vec::with_capacity(corners.len());
    for corner in corners {
        let key = (*corner * 1000.0).round().as_ivec3().to_array();
        let next = keys.len();
        let index = *keys.entry(key).or_insert(next);
        if index == parent.len() {
            parent.push(index);
        }
        of_corner.push(index);
    }
    for triangle in of_corner.as_chunks::<3>().0 {
        let root = find(&mut parent, triangle[0]);
        for &other in &triangle[1..] {
            let other = find(&mut parent, other);
            parent[other] = root;
        }
    }
    let textured = uv.len() == corners.len();
    let mut pieces: BTreeMap<usize, (Vec<Vec3>, Vec<Vec2>)> = BTreeMap::new();
    for (at, of) in of_corner.as_chunks::<3>().0.iter().enumerate() {
        let root = find(&mut parent, of[0]);
        let piece = pieces.entry(root).or_default();
        piece.0.extend_from_slice(&corners[3 * at..3 * at + 3]);
        if textured {
            piece.1.extend_from_slice(&uv[3 * at..3 * at + 3]);
        }
    }
    let mut found: Vec<Found> = Vec::new();
    let mut wholes: Vec<(f32, [f32; 11])> = Vec::new();
    let cut: Vec<(Vec<Vec3>, Vec<Vec2>)> = if follow {
        pieces
            .into_values()
            .flat_map(|(piece, uv)| lengths(piece, uv))
            .collect()
    } else {
        pieces.into_values().collect()
    };
    for (piece, uv) in &cut {
        let Some(mut lamp) = whole(piece) else {
            continue;
        };
        let [x, y, z, fx, fy, fz, width, lx, ly, lz, half, ..] = lamp.part();
        wholes.push((lamp.area, [x, y, z, fx, fy, fz, width, lx, ly, lz, half]));
        if let (Some(texture), true) = (texture, textured) {
            // A rough look first: most pieces are bright all over or dark.
            let (spots, _) = bright_spots(piece, uv, texture, ROUGH_SPOTS);
            let bright: f32 = spots.iter().map(|spot| spot.area).sum();
            if bright >= DIM * lamp.area && bright < WHOLE * lamp.area {
                let (spots, step) = bright_spots(piece, uv, texture, SPOTS);
                found.extend(patches(&spots, step));
                continue;
            }
            // Its colour is its bright texels', whatever its form.
            lamp.colour = light_colour(
                (spots.iter())
                    .map(|spot| {
                        let chroma = spot.colour.max_element() - spot.colour.min_element();
                        spot.area * (chroma * chroma + 0.02) * spot.colour
                    })
                    .sum(),
            );
        }
        found.push(lamp);
    }
    wholes.sort_by(|a, b| b.0.total_cmp(&a.0));
    wholes.truncate(PARTS);
    join_tubes(&mut found);
    for lamp in &mut found {
        // A lamp set into a wall that faces more up than out, but not
        // straight up, shines level: a light held off it the way it faces
        // would stand in the lip of the wall above it.
        let level = Vec3::new(lamp.facing.x, lamp.facing.y, 0.0);
        let up = lamp.facing.z / lamp.facing.length().max(1e-6);
        if (TILTED_UP..STRAIGHT_UP).contains(&up) {
            lamp.facing = level.normalize_or_zero() * lamp.facing.length();
        }
    }
    found.sort_by(|a, b| b.area.total_cmp(&a.area));
    found.truncate(LAMPS);
    (
        wholes.into_iter().map(|(_, part)| part).collect(),
        found.iter().map(Found::part).collect(),
    )
}

/// A piece longer than this from its middle (metres) is looked at for a
/// bend. It bends when the middle of one of its lengths lies farther than
/// `BEND_OFF` metres from the straight line through the piece. Each length
/// is at most `BEND_LENGTH` metres.
const BEND_FROM: f32 = 3.0;
const BEND_OFF: f32 = 0.3;
const BEND_LENGTH: f32 = 3.0;

/// A welded piece cut into lengths where it is long and bends (`parts`,
/// `follow`). Any other piece comes back whole. A light stands on the
/// straight line through its lamp, and the straight line through a strip
/// that follows a curved hall leaves the hall. The Harbinger's curved hall
/// had lights 17 to 23 m long that lit odd wedges of its wall, which the
/// owner took for baked shadows (17 November 2026).
fn lengths(piece: Vec<Vec3>, uv: Vec<Vec2>) -> Vec<(Vec<Vec3>, Vec<Vec2>)> {
    let Some(lamp) = whole(&piece) else {
        return vec![(piece, uv)];
    };
    if lamp.half <= BEND_FROM {
        return vec![(piece, uv)];
    }
    let count = (2.0 * lamp.half / BEND_LENGTH).ceil() as usize;
    let textured = uv.len() == piece.len();
    let mut cut: Vec<(Vec<Vec3>, Vec<Vec2>)> = vec![Default::default(); count];
    for (at, triangle) in piece.as_chunks::<3>().0.iter().enumerate() {
        let middle = (triangle[0] + triangle[1] + triangle[2]) / 3.0;
        let along = (middle - lamp.centre).dot(lamp.way);
        let bin = ((along + lamp.half) / (2.0 * lamp.half) * count as f32) as usize;
        let length = &mut cut[bin.min(count - 1)];
        length.0.extend_from_slice(triangle);
        if textured {
            length.1.extend_from_slice(&uv[3 * at..3 * at + 3]);
        }
    }
    cut.retain(|(corners, _)| !corners.is_empty());
    let bends = cut.iter().any(|(corners, _)| {
        let middle = corners.iter().sum::<Vec3>() / corners.len() as f32;
        let off = middle - lamp.centre;
        (off - lamp.way * off.dot(lamp.way)).length() > BEND_OFF
    });
    if bends { cut } else { vec![(piece, uv)] }
}

/// The way `corners` spread most about `centre` (a unit vector) and half
/// their length that way. The way is found by multiplying a guess by the
/// corners' spread a few times, which turns it to the longest axis.
pub fn longest(corners: &[Vec3], centre: Vec3) -> [f32; 4] {
    let mut way = Vec3::new(1.0, 0.7, 0.4).normalize();
    for _ in 0..12 {
        let turned: Vec3 = (corners.iter())
            .map(|corner| (*corner - centre) * (*corner - centre).dot(way))
            .sum();
        if turned.length_squared() < 1e-12 {
            return [0.0; 4];
        }
        way = turned.normalize();
    }
    let (least, most) = corners
        .iter()
        .fold((f32::MAX, f32::MIN), |(low, high), corner| {
            let along = (*corner - centre).dot(way);
            (low.min(along), high.max(along))
        });
    if least > most {
        return [0.0; 4];
    }
    [way.x, way.y, way.z, 0.5 * (most - least)]
}

fn find(parent: &mut [usize], mut at: usize) -> usize {
    while parent[at] != at {
        parent[at] = parent[parent[at]];
        at = parent[at];
    }
    at
}

/// The fitted lights as the shader reads them (`odyssey.wgsl`, `fitted`):
/// a grid over the area's floor plan, built once when the area loads. Each
/// cell lists the lights that reach it, so a surface reads only the lights
/// near it, every light of the area is always lit, and nothing depends on
/// where the camera is.
pub const GRID: bevy::asset::Handle<bevy::render::storage::ShaderBuffer> =
    bevy::asset::uuid_handle!("7b1e4c92-3a6d-4f58-9c07-d2e5a8f1b634");
/// How many 32-bit words the grid's buffer holds. `odyssey.wgsl`'s
/// `fitted` is a quarter as many `vec4<u32>`.
pub const GRID_WORDS: usize = 1 << 21;
/// Where the rooms' links start (words), after the two heads: four words
/// for each of `VIS_ROOMS` rooms. Then the cells start.
const LINKS_AT: usize = 8;
const CELLS_AT: usize = LINKS_AT + 4 * crate::material::VIS_ROOMS;
/// How many words a light takes in a cell's list: two `vec4`.
const ENTRY_WORDS: usize = 8;
/// The strongest colour a list can hold. A channel has 10 bits.
const COLOUR_MOST: f32 = 4.0;
/// The smallest cell (metres) and the most cells along one side.
const CELL: f32 = 1.0;
const CELLS_ACROSS: usize = 256;
/// The most lights one cell lists: its count has 12 bits.
const CELL_LIGHTS: usize = 4095;
/// A light's room where it is in none, and so reaches every room.
const NO_ROOM: u32 = 255;
/// A light is left out of a cell where it gives less than this
/// (`odyssey.wgsl`, `FAINT`).
const FAINT: f32 = 0.002;

/// A fitted light as the grid holds it, in Bevy world space
/// (`material::GpuLight`'s fields of the same names).
#[derive(Clone, Copy)]
pub struct GridLight {
    pub position_radius: bevy::math::Vec4,
    pub colour: Vec3,
    pub way: bevy::math::Vec4,
    pub aim: bevy::math::Vec4,
    /// The room it is in, which decides the rooms it reaches.
    pub room: Option<usize>,
}

/// The grid with no light in it.
pub fn no_grid() -> Vec<u8> {
    vec![0; GRID_WORDS * 4]
}

/// How far from a light it still gives a surface `FAINT` or more.
fn shown_to(light: &GridLight) -> f32 {
    let radius = light.position_radius.w;
    let strongest = light.colour.max_element();
    (1..=32)
        .rev()
        .map(|step| radius * step as f32 / 32.0)
        .find(|distance| strongest * falloff(*distance * 31.0 / 32.0, radius) >= FAINT)
        .unwrap_or(0.0)
}

/// A direction as three signed bytes in the low bytes of a word, which
/// `odyssey.wgsl` reads back with `unpack4x8snorm`.
fn bytes_of(way: Vec3) -> u32 {
    let byte = |v: f32| ((v.clamp(-1.0, 1.0) * 127.0).round() as i8) as u8 as u32;
    let way = way.normalize_or_zero();
    byte(way.x) | (byte(way.y) << 8) | (byte(way.z) << 16)
}

/// The square of how far the line from `from` to `to` is from the
/// rectangle between `low` and `high`. It is 0 where they meet.
fn apart(from: Vec2, to: Vec2, low: Vec2, high: Vec2) -> f32 {
    // The part of the line inside the rectangle, if any.
    let along = to - from;
    let (mut enters, mut leaves) = (0.0f32, 1.0f32);
    for axis in 0..2 {
        if along[axis] == 0.0 {
            if from[axis] < low[axis] || from[axis] > high[axis] {
                enters = f32::MAX;
            }
            continue;
        }
        let one = (low[axis] - from[axis]) / along[axis];
        let other = (high[axis] - from[axis]) / along[axis];
        enters = enters.max(one.min(other));
        leaves = leaves.min(one.max(other));
    }
    if enters <= leaves {
        return 0.0;
    }
    // Apart, they are nearest at an end of the line or at a corner of the
    // rectangle.
    let to_rectangle = |point: Vec2| point.distance_squared(point.clamp(low, high));
    let to_line = |point: Vec2| {
        let share = ((point - from).dot(along) / along.length_squared().max(1e-12)).clamp(0.0, 1.0);
        point.distance_squared(from + along * share)
    };
    [
        low,
        high,
        Vec2::new(low.x, high.y),
        Vec2::new(high.x, low.y),
    ]
    .into_iter()
    .map(to_line)
    .chain([to_rectangle(from), to_rectangle(to)])
    .fold(f32::MAX, f32::min)
}

/// The grid's buffer for `lights`. `linked` has, for each room, a bit for
/// each room whose lights reach it. The buffer holds, in this order:
/// - two heads: how many columns and rows, where the lists start, the
///   grid's corner and one over a cell's width;
/// - the rooms' links;
/// - a word for each cell: where its list starts and how long it is;
/// - the cells' lists. A light in a list is two `vec4` side by side. The
///   first is its place and how far around it anything is lit. A surface
///   outside that reads no more of the light. The second is the rest,
///   packed: its colour (10 bits a channel, up to `COLOUR_MOST`), its
///   radius, the way a strip runs with its cone, and the way it shines
///   with its room.
///
/// A cell lists a light that shows anywhere over it. A strip shows
/// around its line, not around its middle, so it is left out of the cells
/// off its ends' sides.
///
/// A light is written into every list that holds it. A surface then
/// reads memory that lies together, which the graphics card does much
/// faster than reads all over a list of lights (measured on the Harbinger,
/// 17 November 2026).
///
/// A cell is `CELL` wide, or wider in an area too large for
/// `CELLS_ACROSS` of them or with too many lights for the buffer.
pub fn grid(lights: &[GridLight], linked: &[bevy::math::UVec4]) -> Vec<u8> {
    let mut words = vec![0u32; GRID_WORDS];
    let bytes = |words: &[u32]| words.iter().flat_map(|word| word.to_le_bytes()).collect();
    // What each light covers of the floor plan (x and z): its place, how
    // far it shows with a strip's half length added, and from its place to
    // a strip's end.
    let covers: Vec<(Vec2, f32, Vec2)> = (lights.iter())
        .map(|light| {
            let at = Vec2::new(light.position_radius.x, light.position_radius.z);
            let way = light.way.truncate().normalize_or_zero();
            let shown = shown_to(light);
            let reach = if shown > 0.0 {
                shown + light.way.w
            } else {
                0.0
            };
            (at, reach, Vec2::new(way.x, way.z) * light.way.w)
        })
        .collect();
    let shown: Vec<usize> = (0..lights.len())
        .filter(|index| covers[*index].1 > 0.0)
        .collect();
    if shown.is_empty() {
        return bytes(&words);
    }
    // Each light's two `vec4`.
    let packed: Vec<[u32; ENTRY_WORDS]> = (lights.iter())
        .map(|light| {
            let place = (light.position_radius.truncate())
                .extend(light.position_radius.w + light.way.w)
                .to_array()
                .map(f32::to_bits);
            let channel = |v: f32| ((v / COLOUR_MOST).clamp(0.0, 1.0) * 1023.0).round() as u32;
            let colour = channel(light.colour.x)
                | (channel(light.colour.y) << 10)
                | (channel(light.colour.z) << 20);
            let cone = (light.aim.w.clamp(0.0, 1.0) * 127.0).round() as u32;
            let room = light
                .room
                .map_or(NO_ROOM, |room| (room as u32).min(NO_ROOM));
            [
                place[0],
                place[1],
                place[2],
                place[3],
                colour,
                light.position_radius.w.to_bits(),
                bytes_of(light.way.truncate()) | (cone << 24),
                bytes_of(light.aim.truncate()) | (room << 24),
            ]
        })
        .collect();
    let low = (shown.iter())
        .map(|index| covers[*index].0 - covers[*index].1)
        .fold(Vec2::MAX, Vec2::min);
    let high = (shown.iter())
        .map(|index| covers[*index].0 + covers[*index].1)
        .fold(Vec2::MIN, Vec2::max);
    let span = (high - low).max_element();
    let mut cell = CELL.max(span / CELLS_ACROSS as f32);
    loop {
        let columns = ((high.x - low.x) / cell).ceil().max(1.0) as usize;
        let rows = ((high.y - low.y) / cell).ceil().max(1.0) as usize;
        let mut lists: Vec<Vec<u32>> = vec![Vec::new(); columns * rows];
        for &index in &shown {
            let (at, reach, end) = covers[index];
            let shows = reach - lights[index].way.w;
            let first = ((at - reach - low) / cell).floor().max(Vec2::ZERO);
            let last = ((at + reach - low) / cell).floor();
            for row in first.y as usize..=(last.y as usize).min(rows - 1) {
                for column in first.x as usize..=(last.x as usize).min(columns - 1) {
                    let corner = low + Vec2::new(column as f32, row as f32) * cell;
                    let list = &mut lists[row * columns + column];
                    if apart(at - end, at + end, corner, corner + cell) < shows * shows
                        && list.len() < CELL_LIGHTS
                    {
                        list.push(index as u32);
                    }
                }
            }
        }
        let listed: usize = lists.iter().map(Vec::len).sum();
        let lists_at = (CELLS_AT + columns * rows).next_multiple_of(4);
        if lists_at + ENTRY_WORDS * listed > GRID_WORDS || listed >= 1 << 20 {
            if cell > 64.0 {
                bevy::log::warn!(
                    "fitted lights: {} lights are too many for the grid",
                    lights.len()
                );
                return no_grid();
            }
            cell *= 1.5;
            continue;
        }
        words[0] = columns as u32;
        words[1] = rows as u32;
        words[2] = (lists_at / 4) as u32;
        words[4] = low.x.to_bits();
        words[5] = low.y.to_bits();
        words[6] = (1.0 / cell).to_bits();
        for (room, links) in linked.iter().take(crate::material::VIS_ROOMS).enumerate() {
            words[LINKS_AT + 4 * room..LINKS_AT + 4 * room + 4].copy_from_slice(&links.to_array());
        }
        let mut next = 0usize;
        for (at, list) in lists.iter().enumerate() {
            words[CELLS_AT + at] = ((next as u32) << 12) | list.len() as u32;
            for &index in list {
                let to = lists_at + ENTRY_WORDS * next;
                words[to..to + ENTRY_WORDS].copy_from_slice(&packed[index as usize]);
                next += 1;
            }
        }
        let longest = lists.iter().map(Vec::len).max().unwrap_or(0);
        bevy::log::info!(
            "fitted lights: a grid of {columns} by {rows} cells of {cell:.1} m, {listed} entries, {:.1} lights a lit cell, {longest} at most",
            listed as f32 / lists.iter().filter(|list| !list.is_empty()).count().max(1) as f32
        );
        return bytes(&words);
    }
}
