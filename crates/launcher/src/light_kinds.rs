//! `kjk light-kinds [k1|tsl|all]`: the kinds of light in every area of both
//! games, from the survey of `kjk room-lights`. It opens no window. It
//! writes `light-kinds.json`, `light-kinds.txt` and a picture of each
//! kind's texture (`textures/<game>-<texture>.png`) into `--out`.
//!
//! A level has thousands of lights and a few hundred kinds. A rule made for
//! a kind reaches every place the kind is used, and a mod's area that uses
//! the same lamp.
//!
//! - **A glowing surface's kind is its texture.** Every mesh of a room or a
//!   placeable that glows and looks like a light (`room_lights::Glow`) is
//!   of the kind its texture names.
//! - **A lamp at a glowing surface is of that surface's kind.** A light
//!   node within `AT_FIXTURE` metres of such a surface is the files' own
//!   light for that fixture.
//! - **Any other lamp.** A placeable's, a door's or a creature's is of the
//!   kind its model names. A room's is of the kind its flags name. A lamp
//!   whose radius is `SUN_RADIUS` or more is a sun.
//!
//! Each kind gives how often it is used and in how many areas, its size,
//! how many of its surfaces have a lamp of the files, what those lamps
//! are, how bright the stored lightmap is near it, and up to `EXAMPLES`
//! places to look at it, each with a camera.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use serde::Serialize;
use vfs::resolver::{Resolver, TextureQuality};
use vfs::{DirIndex, Discovery, Game};

use crate::room_light_fit::read_rgb;
use crate::room_lights::{self, AreaSurvey, GameSurvey, Glow, Lamp};

/// A lamp this near a glowing surface's middle is that fixture's light
/// (metres).
const AT_FIXTURE: f32 = 2.0;
/// A lamp with this radius or more is a sun (`room_light_fit`).
const SUN_RADIUS: f32 = 100.0;
/// How many places each kind gives to look at it.
const EXAMPLES: usize = 6;
/// How many of an area's lights of a kind an example lists.
const MATES: usize = 600;
/// How many of an area's lamps of a kind, as its texture shows them, an
/// example lists.
const BRIGHT_MATES: usize = 1200;
/// The longest side of a texture's picture (texels).
const PICTURE: usize = 128;

#[derive(Serialize)]
struct Kind {
    game: &'static str,
    /// What names the kind: a texture, a model, or a room lamp's flags.
    name: String,
    /// `glowing surface of a room`, `glowing surface of a placeable`,
    /// `lamp of a placeable`, `lamp of a door`, `lamp of a creature`,
    /// `lamp of a room at no glowing surface` or `sun`.
    group: String,
    /// The file of its texture's picture, for a glowing surface.
    picture: Option<String>,
    /// How many are placed, in how many areas.
    count: usize,
    areas: usize,
    /// The modules that use it most, with how many each has.
    modules: Vec<(String, usize)>,
    /// The models that carry it most (a placeable's, or the rooms').
    models: Vec<(String, usize)>,
    surface: Option<SurfaceFacts>,
    /// Its texture is bright by one colour only, as a red lamp's is. The
    /// survey found it by `Glow::by_colour`, after every other kind.
    by_colour: bool,
    lamps: Option<LampFacts>,
    /// The stored lightmap's brightness within 2 metres over the area's
    /// mean, at the median: above 1, the bake is brighter near it.
    near_bake: Option<f32>,
    examples: Vec<Example>,
}

#[derive(Serialize)]
struct SurfaceFacts {
    total_m2: f32,
    median_m2: f32,
    /// The share that lie flat, as a panel does.
    flat_share: f32,
    /// The share that face down, as a ceiling's panel does.
    facing_down_share: f32,
    lightmapped_share: f32,
    /// The share of the texture's texels that are bright.
    bright_share: Option<f32>,
    /// The share whose strongest channel is bright, for a kind found by
    /// its colour.
    strong_share: Option<f32>,
    /// How many meshes that are not set to glow carry the texture. Where
    /// they are many, the kind may need `always` in `fittings.toml`.
    unglowing: usize,
    /// The mean self-illumination colour.
    glow: [f32; 3],
    /// The share with a lamp of the files within `AT_FIXTURE`.
    with_lamp_share: f32,
}

#[derive(Serialize)]
struct LampFacts {
    count: usize,
    median_radius: f32,
    /// The mean colour times the multiplier.
    colour: [f32; 3],
    ambient_only_share: f32,
    shadow_share: f32,
    /// The share that light only people and objects (dynamic type 2).
    people_only_share: f32,
}

#[derive(Serialize, Clone)]
struct Example {
    module: String,
    area: String,
    room: Option<String>,
    position: [f32; 3],
    /// A camera that sees it: x, y, z, yaw and pitch in degrees.
    view: [f32; 5],
    /// Every lamp of the kind in this area, the example's own too (a mesh
    /// of several separate lamps gives one each): its place, the way it faces (zeros for a lamp), how wide it is in
    /// metres, the way it is longest and half its length that way. At most
    /// `MATES`.
    mates: Vec<[f32; 11]>,
    /// For a glowing surface, every lamp of the kind in this area as the
    /// texture shows them (`room_lights::Glow::lamps`): as a mate is, and
    /// then the colour of its bright texels. At most `BRIGHT_MATES`.
    bright_mates: Vec<[f32; 14]>,
    #[serde(skip)]
    near_texels: usize,
}

#[derive(Default)]
struct Gathered {
    group: String,
    texture: Option<String>,
    count: usize,
    areas: BTreeSet<String>,
    modules: BTreeMap<String, usize>,
    models: BTreeMap<String, usize>,
    glows: Vec<Glow>,
    by_colour: bool,
    unglowing: usize,
    with_lamp: usize,
    lamps: Vec<Lamp>,
    near: Vec<f32>,
    examples: Vec<Example>,
    /// The kind's lights by area: place, facing and width.
    placed: BTreeMap<String, Vec<[f32; 11]>>,
    /// The kind's lamps by area, as `Example::bright_mates` gives them.
    bright: BTreeMap<String, Vec<[f32; 14]>>,
}

impl Gathered {
    /// Adds a glowing surface of the kind `key`. `lit`: a lamp of the files
    /// is at it.
    fn glow(&mut self, area: &AreaSurvey, glow: &Glow, key: &(String, String), lit: bool) {
        let kind = self;
        kind.group.clone_from(&key.0);
        kind.texture.clone_from(&glow.texture);
        kind.seen(area, &glow.model);
        kind.with_lamp += usize::from(lit);
        kind.near.extend(glow.near_ratio);
        // Each separate piece of the mesh is a lamp of its own. The
        // camera looks at the largest (`room_lights::survey_area`).
        let focus = glow
            .parts
            .first()
            .map_or(glow.position, |part| [part[0], part[1], part[2]]);
        kind.example(area, &glow.room, focus, glow.view, glow.near_texels);
        if glow.parts.is_empty() {
            kind.placed(
                area,
                glow.position,
                glow.facing,
                glow.area_m2.sqrt(),
                glow.long,
            );
        }
        for &[x, y, z, fx, fy, fz, width, lx, ly, lz, half] in &glow.parts {
            kind.placed(area, [x, y, z], [fx, fy, fz], width, [lx, ly, lz, half]);
        }
        let bright = kind.bright.entry(area.area.clone()).or_default();
        let room = BRIGHT_MATES.saturating_sub(bright.len());
        bright.extend(glow.lamps.iter().take(room));
        kind.glows.push((*glow).clone());
    }

    fn seen(&mut self, area: &AreaSurvey, model: &str) {
        self.count += 1;
        self.areas.insert(area.area.clone());
        *self.modules.entry(area.module.clone()).or_default() += 1;
        *self.models.entry(model.to_string()).or_default() += 1;
    }

    fn example(
        &mut self,
        area: &AreaSurvey,
        room: &Option<String>,
        position: [f32; 3],
        view: Option<[f32; 5]>,
        near_texels: usize,
    ) {
        if let Some(view) = view {
            self.examples.push(Example {
                module: area.module.clone(),
                area: area.area.clone(),
                room: room.clone(),
                position,
                view,
                mates: Vec::new(),
                bright_mates: Vec::new(),
                near_texels,
            });
        }
    }

    fn placed(
        &mut self,
        area: &AreaSurvey,
        at: [f32; 3],
        facing: [f32; 3],
        width: f32,
        long: [f32; 4],
    ) {
        let mates = self.placed.entry(area.area.clone()).or_default();
        if mates.len() < MATES {
            let [x, y, z] = at;
            let [fx, fy, fz] = facing;
            let [lx, ly, lz, half] = long;
            mates.push([x, y, z, fx, fy, fz, width, lx, ly, lz, half]);
        }
    }
}

pub fn run(discovery: &Discovery, game: &str, out: &Path) -> ExitCode {
    let games = match game {
        "all" => vec![Game::Kotor1, Game::Kotor2],
        key => match Game::from_key(key).filter(|game| *game != Game::JediAcademy) {
            Some(game) => vec![game],
            None => {
                eprintln!("light-kinds: use k1, tsl or all");
                return ExitCode::from(2);
            }
        },
    };
    let pictures = out.join("textures");
    if let Err(error) = std::fs::create_dir_all(&pictures) {
        eprintln!("{}: {error}", pictures.display());
        return ExitCode::FAILURE;
    }
    let mut kinds: Vec<Kind> = Vec::new();
    for game in games {
        let Some(install) = discovery.get(game) else {
            eprintln!("{}: not found (set {})", game.key(), game.env_var());
            return ExitCode::FAILURE;
        };
        let found = room_lights::survey_game(game, install).and_then(|survey| {
            let index = DirIndex::of_install(install).map_err(|error| error.to_string())?;
            let base =
                Resolver::base(game, &index, TextureQuality::High).map_err(|e| e.to_string())?;
            Ok(kinds_of(&survey, &base, &pictures))
        });
        match found {
            Ok(found) => {
                outln!("{}: {} kinds of light", game.key(), found.len());
                kinds.extend(found);
            }
            Err(error) => {
                eprintln!("{}: {error}", game.key());
                return ExitCode::FAILURE;
            }
        }
    }
    let json = out.join("light-kinds.json");
    let text = out.join("light-kinds.txt");
    let written = serde_json::to_string_pretty(&kinds)
        .map_err(|error| error.to_string())
        .and_then(|body| std::fs::write(&json, body).map_err(|error| error.to_string()))
        .and_then(|()| std::fs::write(&text, report(&kinds)).map_err(|error| error.to_string()));
    if let Err(error) = written {
        eprintln!("light-kinds: {error}");
        return ExitCode::FAILURE;
    }
    outln!("wrote {} and {}", json.display(), text.display());
    ExitCode::SUCCESS
}

/// The default folder: `target/light-kinds` under the current folder.
pub fn default_out() -> PathBuf {
    PathBuf::from("target").join("light-kinds")
}

fn distance(a: [f32; 3], b: [f32; 3]) -> f32 {
    (0..3)
        .map(|axis| (a[axis] - b[axis]).powi(2))
        .sum::<f32>()
        .sqrt()
}

fn median(values: &mut [f32]) -> Option<f32> {
    values.sort_by(f32::total_cmp);
    values.get(values.len() / 2).copied()
}

fn share(part: usize, whole: usize) -> f32 {
    if whole == 0 {
        0.0
    } else {
        part as f32 / whole as f32
    }
}

fn most(counts: &BTreeMap<String, usize>, keep: usize) -> Vec<(String, usize)> {
    let mut sorted: Vec<(String, usize)> = counts.iter().map(|(k, v)| (k.clone(), *v)).collect();
    sorted.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    sorted.truncate(keep);
    sorted
}

fn kinds_of(survey: &GameSurvey, base: &Resolver, pictures: &Path) -> Vec<Kind> {
    let mut gathered: BTreeMap<(String, String), Gathered> = BTreeMap::new();
    for area in &survey.areas {
        // The glowing surfaces that look like lights, each under its
        // texture.
        let fixtures: Vec<(&Glow, (String, String))> = (area.glows.iter())
            .filter(|glow| glow.lamp_like && !glow.backdrop)
            .map(|glow| {
                let group = format!("glowing surface of a {}", glow.owner);
                let name = glow
                    .texture
                    .clone()
                    .unwrap_or_else(|| "no texture".to_string());
                (glow, (group, name))
            })
            .collect();
        let mut lit = vec![false; fixtures.len()];
        // The lamps: each at its nearest fixture, or of a kind of its own.
        for lamp in &area.lamps {
            let nearest = (fixtures.iter().enumerate())
                .map(|(at, (glow, _))| (at, distance(glow.position, lamp.position)))
                .filter(|(_, apart)| *apart <= AT_FIXTURE)
                .min_by(|a, b| a.1.total_cmp(&b.1));
            let sun = lamp.radius >= SUN_RADIUS;
            if let (Some((at, _)), false) = (nearest, sun) {
                lit[at] = true;
                let kind = gathered.entry(fixtures[at].1.clone()).or_default();
                kind.lamps.push(lamp.clone());
                continue;
            }
            let (group, name) = if sun {
                ("sun".to_string(), "sun".to_string())
            } else if lamp.owner == "room" {
                (
                    "lamp of a room at no glowing surface".to_string(),
                    format!(
                        "{}, {}, {}",
                        if lamp.ambient_only {
                            "ambient only"
                        } else {
                            "direct"
                        },
                        if lamp.shadow {
                            "casts shadows"
                        } else {
                            "no shadows"
                        },
                        if lamp.dynamic_type == 2 {
                            "lights people only"
                        } else {
                            "lights all"
                        },
                    ),
                )
            } else {
                (format!("lamp of a {}", lamp.owner), lamp.model.clone())
            };
            let kind = gathered.entry((group.clone(), name)).or_default();
            kind.group = group;
            kind.seen(area, &lamp.model);
            kind.lamps.push(lamp.clone());
            kind.near.extend(lamp.near_ratio);
            kind.example(area, &lamp.room, lamp.position, lamp.view, lamp.near_texels);
            kind.placed(area, lamp.position, [0.0; 3], 0.0, [0.0; 4]);
        }
        for ((glow, key), lit) in fixtures.iter().zip(lit) {
            gathered
                .entry(key.clone())
                .or_default()
                .glow(area, glow, key, lit);
        }
    }
    // The lamps found by their colour, where their texture is no kind yet.
    // A kind gathered above stays as it was approved.
    let known: BTreeSet<(String, String)> = gathered.keys().cloned().collect();
    for area in &survey.areas {
        for glow in area.glows.iter().filter(|glow| glow.by_colour) {
            let Some(texture) = glow.texture.clone() else {
                continue;
            };
            let key = (format!("glowing surface of a {}", glow.owner), texture);
            if known.contains(&key) {
                continue;
            }
            let kind = gathered.entry(key.clone()).or_default();
            kind.by_colour = true;
            kind.glow(area, glow, &key, false);
        }
    }
    for ((_, name), kind) in &mut gathered {
        if kind.texture.is_some() {
            kind.unglowing = (survey.areas.iter())
                .filter_map(|area| area.unglowing.get(name))
                .sum();
        }
    }

    let mut kinds: Vec<Kind> = Vec::new();
    for ((_, name), mut kind) in gathered {
        if kind.count == 0 {
            continue;
        }
        let picture = kind.texture.as_deref().and_then(|texture| {
            let file = format!("{}-{texture}.png", survey.game);
            picture_of(base, texture, &pictures.join(&file)).then_some(file)
        });
        let surface = (!kind.glows.is_empty()).then(|| {
            let n = kind.glows.len();
            let mut sizes: Vec<f32> = kind.glows.iter().map(|glow| glow.area_m2).collect();
            let flat = |glow: &&Glow| distance(glow.facing, [0.0; 3]) > 0.5;
            let mut glow_colour = [0.0f32; 3];
            for glow in &kind.glows {
                for (sum, v) in glow_colour.iter_mut().zip(glow.self_illum) {
                    *sum += v / n as f32;
                }
            }
            SurfaceFacts {
                total_m2: sizes.iter().sum(),
                median_m2: median(&mut sizes).unwrap_or(0.0),
                flat_share: share(kind.glows.iter().filter(flat).count(), n),
                facing_down_share: share(
                    (kind.glows.iter())
                        .filter(|glow| flat(glow) && glow.facing[2] < -0.5)
                        .count(),
                    n,
                ),
                lightmapped_share: share(kind.glows.iter().filter(|g| g.lightmapped).count(), n),
                bright_share: kind.glows.iter().find_map(|glow| glow.bright_share),
                strong_share: kind.glows.iter().find_map(|glow| glow.strong_share),
                unglowing: kind.unglowing,
                glow: glow_colour,
                with_lamp_share: share(kind.with_lamp, n),
            }
        });
        let lamps = (!kind.lamps.is_empty()).then(|| {
            let n = kind.lamps.len();
            let mut radii: Vec<f32> = kind.lamps.iter().map(|lamp| lamp.radius).collect();
            let mut colour = [0.0f32; 3];
            for lamp in &kind.lamps {
                for (sum, v) in colour.iter_mut().zip(lamp.color) {
                    *sum += v * lamp.multiplier / n as f32;
                }
            }
            let count =
                |test: fn(&Lamp) -> bool| share(kind.lamps.iter().filter(|l| test(l)).count(), n);
            LampFacts {
                count: n,
                median_radius: median(&mut radii).unwrap_or(0.0),
                colour,
                ambient_only_share: count(|lamp| lamp.ambient_only),
                shadow_share: count(|lamp| lamp.shadow),
                people_only_share: count(|lamp| lamp.dynamic_type == 2),
            }
        });
        // The places to look at it: those with the most lightmap near
        // them, one an area.
        kind.examples
            .sort_by_key(|example| std::cmp::Reverse(example.near_texels));
        let mut areas = BTreeSet::new();
        let examples: Vec<Example> = (kind.examples.iter())
            .filter(|example| areas.insert(example.area.clone()))
            .take(EXAMPLES)
            .cloned()
            .map(|mut example| {
                example.mates = kind
                    .placed
                    .get(&example.area)
                    .map_or_else(Vec::new, Clone::clone);
                example.bright_mates = kind
                    .bright
                    .get(&example.area)
                    .map_or_else(Vec::new, Clone::clone);
                example
            })
            .collect();
        kinds.push(Kind {
            game: survey.game,
            name,
            group: kind.group,
            picture,
            count: kind.count,
            areas: kind.areas.len(),
            modules: most(&kind.modules, 5),
            models: most(&kind.models, 5),
            surface,
            by_colour: kind.by_colour,
            lamps,
            near_bake: median(&mut kind.near),
            examples,
        });
    }
    kinds.sort_by(|a, b| b.count.cmp(&a.count).then_with(|| a.name.cmp(&b.name)));
    kinds
}

/// Writes texture `name` as a PNG no larger than `PICTURE` a side.
fn picture_of(base: &Resolver, name: &str, file: &Path) -> bool {
    let Some(rgb) = read_rgb(base, name, PICTURE) else {
        return false;
    };
    let mut picture = image::RgbImage::new(rgb.width as u32, rgb.height as u32);
    for (at, texel) in rgb.texels.iter().enumerate() {
        let (x, y) = ((at % rgb.width) as u32, (at / rgb.width) as u32);
        picture.put_pixel(x, y, image::Rgb(texel.map(|v| (v * 255.0).round() as u8)));
    }
    picture.save(file).is_ok()
}

fn report(kinds: &[Kind]) -> String {
    let mut out = String::new();
    for game in ["k1", "tsl"] {
        let of_game: Vec<&Kind> = kinds.iter().filter(|kind| kind.game == game).collect();
        if of_game.is_empty() {
            continue;
        }
        let _ = writeln!(out, "{game}: {} kinds of light", of_game.len());
        let _ = writeln!(
            out,
            "{:<34} {:<38} {:>6} {:>5} {:>8} {:>6} {:>6} {:>7}",
            "name", "group", "count", "areas", "m2 each", "lamp", "bake", "radius"
        );
        for kind in of_game {
            let number = |v: Option<f32>| v.map_or("-".to_string(), |v| format!("{v:.2}"));
            let _ = writeln!(
                out,
                "{:<34} {:<38} {:>6} {:>5} {:>8} {:>6} {:>6} {:>7}",
                kind.name,
                kind.group,
                kind.count,
                kind.areas,
                number(kind.surface.as_ref().map(|s| s.median_m2)),
                number(kind.surface.as_ref().map(|s| s.with_lamp_share)),
                number(kind.near_bake),
                number(kind.lamps.as_ref().map(|l| l.median_radius)),
            );
        }
        out.push('\n');
    }
    out
}
