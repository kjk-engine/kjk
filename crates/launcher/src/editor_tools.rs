//! The two checking verbs the level editor runs (`docs/LEVEL-EDIT.md`):
//! `kjk detail-bake`, which its Bake buttons run, and `kjk collision
//! audit`, which its Audit button runs. They are built with the
//! `editor_tools` feature. A release builds the `kjk` of the level editor's
//! package with it, so that the editor of a release bakes and audits as a
//! build from source does (`xtask dist --level-editor`). The player's
//! `kjk` is built without it. The `probes` feature, which builds every
//! checking verb, takes them in too, and then `verbs` lists them among the
//! others.
//!
//! The feature also builds the two commands the editor's Try buttons run,
//! `kjk try-script` (`script_try`) and `kjk try-conversation`
//! (`talk_run`).

use std::process::ExitCode;

use crate::{collision_audit, collision_verbs, detail_bake, discover};

/// Which module a collision verb builds, and its clip brushes.
#[derive(clap::Args, Clone)]
pub(crate) struct CollisionModule {
    /// k1 or tsl
    pub(crate) game: String,
    /// Module, e.g. end_m01aa (its entry area is built); for `build`,
    /// `all` builds every module
    pub(crate) module: String,
    /// Clip brushes from this file instead of the built-in one
    #[arg(long)]
    clip: Option<std::path::PathBuf>,
    /// No clip brushes at all
    #[arg(long, conflicts_with = "clip")]
    no_clip: bool,
}

impl CollisionModule {
    pub(crate) fn clip(&self) -> collision_verbs::ClipChoice<'_> {
        match (&self.clip, self.no_clip) {
            (_, true) => collision_verbs::ClipChoice::None,
            (Some(path), _) => collision_verbs::ClipChoice::File(path),
            (None, false) => collision_verbs::ClipChoice::BuiltIn,
        }
    }
}


/// The doors a collision verb opens, by the word for them.
pub(crate) fn doors_of(text: &str) -> Option<collision_verbs::Doors> {
    Some(match text {
        "openable" => collision_verbs::Doors::Openable,
        "closed" => collision_verbs::Doors::Closed,
        "all" => collision_verbs::Doors::All,
        _ => return None,
    })
}

/// A zoom's place and radius, `x,y,radius` in metres.
pub(crate) fn parse_zoom(text: &str) -> Result<[f32; 3], String> {
    let numbers: Vec<f32> = text
        .split(',')
        .map(|part| {
            part.trim()
                .parse::<f32>()
                .map_err(|_| format!("{part}: a number"))
        })
        .collect::<Result<_, _>>()?;
    match numbers.as_slice() {
        &[x, y, r] if r > 0.0 => Ok([x, y, r]),
        _ => Err("use x,y,radius in metres".into()),
    }
}

/// An area's lightmaps baked anew from the game's fitted lights, at four
/// times their stored size, on the graphics card. Writes one PNG for each
/// lightmap into two sets: a, the live look with shadows and bounced light,
/// and b, the lamps alone, which the lighting download ships. The game
/// shows b with KJK_FITTED_LIGHTS=baked.
#[derive(clap::Args)]
pub(crate) struct DetailBakeArgs {
    /// The module, for example tar_m03ae
    pub(crate) module: String,
    /// k1 or tsl; both are searched when left out
    #[arg(long)]
    pub(crate) game: Option<String>,
    /// The fitted lights to bake from: a file the game writes when
    /// KJK_FITTED_DUMP names it. Every bake needs it
    #[arg(long, required_unless_present_any = ["fingerprint", "stamp"])]
    pub(crate) lights: Option<std::path::PathBuf>,
    /// The bake always runs on the graphics card. The flag is still
    /// taken, so older command lines keep working
    #[arg(long)]
    pub(crate) gpu: bool,
    /// Print what the texels near a place see of each
    /// light: "x y z radius" in metres
    #[arg(long)]
    pub(crate) probe: Option<String>,
    /// The levels the lamps-alone set b aims at: a file
    /// with one line for each room, its name and its mean stored level
    #[arg(long)]
    pub(crate) levels: Option<std::path::PathBuf>,
    /// The area's sun and sky for set b, nine numbers:
    /// the way to the sun "x y z", the stored level a surface facing
    /// the sun shows of it "r g b", and the same under the whole sky.
    /// Six more numbers give a second sun: its way and its colour.
    /// One last number is half the suns' width in radians, for sharp
    /// shadows
    #[arg(long, allow_hyphen_values = true)]
    pub(crate) sun: Option<String>,
    /// Print the ways to the sun that fit the stored
    /// lightmaps best
    #[arg(long)]
    pub(crate) sun_fit: bool,
    /// A room's level is its level where the game lights
    /// it today: each texel counts by its stored level
    #[arg(long)]
    pub(crate) weigh: bool,
    /// No room takes the even light: a room its lamps and
    /// the sun cannot bring to its level stays darker
    #[arg(long)]
    pub(crate) no_even: bool,
    /// A room its lamps cannot bring to its level takes its
    /// own stored light for the rest, and not an even light
    #[arg(long)]
    pub(crate) fill_stored: bool,
    /// Each texel of the lamps' set shows at least this
    /// share of its stored linear light in the game (`floor.txt`)
    #[arg(long)]
    pub(crate) floor: Option<f32>,
    /// Every room with a lightmapped mesh is baked, not
    /// only the rooms with a walkable face, as under a sun
    #[arg(long)]
    pub(crate) every_room: bool,
    /// A room with no walkable face shows its stored light in
    /// the lamps' set, and not twice it
    #[arg(long)]
    pub(crate) stored_backdrop: bool,
    /// The least a room's lamps are scaled by (0.5 when
    /// left out)
    #[arg(long)]
    pub(crate) least: Option<f32>,
    /// The most a room's lamps are scaled by (6 when left
    /// out)
    #[arg(long)]
    pub(crate) most: Option<f32>,
    /// With --sun, the meshes that send the sun's and the sky's light
    /// down into their room, by their node's name, with spaces between
    #[arg(long)]
    pub(crate) skylights: Option<String>,
    /// The placeables block the rays where they rest, so they
    /// cast their shadows into the lightmaps. Skinned and dangling meshes
    /// do not
    #[arg(long)]
    pub(crate) placeables: bool,
    /// Each vertex of a placeable's mesh that is not skinned
    /// and does not dangle is lit as a texel is, into placeables.txt of
    /// the lamps' set. It implies --placeables
    #[arg(long)]
    pub(crate) placeable_light: bool,
    /// Bake nothing. Print the fingerprint of each of the module's
    /// areas in the game files as they are now: the format, and the
    /// digests of the rooms and of the placeables. The game compares it
    /// with fingerprint.txt in a bake's folder
    #[arg(long)]
    pub(crate) fingerprint: bool,
    /// Bake nothing. Write fingerprint.txt into this folder of
    /// lightmaps baked earlier, from the game files as they are now.
    /// This is right only when the game files are still the ones the
    /// bake was made from. Over other files the game would then show a
    /// bake that does not fit them
    #[arg(long)]
    pub(crate) stamp: Option<std::path::PathBuf>,
    /// The folder the files are written to
    #[arg(long, default_value_os_t = detail_bake::default_out())]
    pub(crate) out: std::path::PathBuf,
}

/// Runs `kjk detail-bake`.
pub(crate) fn detail_bake(args: DetailBakeArgs) -> ExitCode {
    let DetailBakeArgs {
        module,
        game,
        lights,
        gpu: _,
        probe,
        levels,
        sun,
        sun_fit,
        weigh,
        no_even,
        fill_stored,
        floor,
        every_room,
        stored_backdrop,
        least,
        most,
        skylights,
        placeables,
        placeable_light,
        fingerprint,
        stamp,
        out,
    } = args;
    if fingerprint || stamp.is_some() {
        return detail_bake::fingerprint(&discover(), &module, game.as_deref(), stamp.as_deref());
    }
    let probe = match probe.as_deref().map(|text| {
        text.split_whitespace()
            .map(str::parse::<f32>)
            .collect::<Result<Vec<f32>, _>>()
    }) {
        None => None,
        Some(Ok(v)) if v.len() == 4 => {
            Some((bevy::math::Vec3::new(v[0], v[1], v[2]), v[3]))
        }
        Some(_) => {
            eprintln!("--probe: give x y z radius");
            return ExitCode::from(2);
        }
    };
    let sun = match sun.as_deref().map(|text| {
        text.split_whitespace()
            .map(str::parse::<f32>)
            .collect::<Result<Vec<f32>, _>>()
    }) {
        None => None,
        Some(Ok(v)) if matches!(v.len(), 9 | 10 | 15 | 16) => {
            let three = |at: usize| bevy::math::Vec3::new(v[at], v[at + 1], v[at + 2]);
            // A last number on its own is half the suns' width.
            let width = (v.len() % 3 == 1).then(|| v[v.len() - 1]);
            let v = &v[..v.len() - usize::from(width.is_some())];
            Some(detail_bake::Sun {
                toward: three(0).normalize_or_zero(),
                colour: three(3),
                sky: three(6),
                second: (v.len() == 15).then(|| (three(9).normalize_or_zero(), three(12))),
                width,
            })
        }
        Some(_) => {
            eprintln!("--sun: give nine numbers, the way to the sun, its colour and the sky's, six more for a second sun, and one last for half the suns' width");
            return ExitCode::from(2);
        }
    };
    let Some(lights) = lights else {
        eprintln!("detail-bake: give the fitted lights with --lights");
        return ExitCode::from(2);
    };
    let options = detail_bake::GpuOptions {
        probe,
        levels,
        sun,
        sun_fit,
        weigh,
        no_even,
        fill_stored,
        floor,
        every_room,
        stored_backdrop,
        least,
        most,
        skylights: (skylights.iter())
            .flat_map(|names| names.split_whitespace())
            .map(str::to_string)
            .collect(),
        placeables: placeables || placeable_light,
        placeable_light,
    };
    detail_bake::run(&discover(), &module, game.as_deref(), &lights, &options, &out)
}

/// The containment audit: where a player gets off the walkmesh by
/// walking and by each jump level; writes facts.json and audit.png.
#[derive(clap::Args)]
pub(crate) struct AuditArgs {
    #[command(flatten)]
    pub(crate) module: CollisionModule,
    /// Grid cell, metres
    #[arg(long, default_value_t = 0.5)]
    pub(crate) cell: f32,
    /// Which doors are open: `openable` (not static, not a module
    /// transition), `closed` or `all`
    #[arg(long, default_value = "openable")]
    pub(crate) doors: String,
    /// Regions to print
    #[arg(long, default_value_t = 40)]
    pub(crate) show: usize,
    /// Also draw zoom.png around x,y,radius (metres)
    #[arg(long, value_parser = parse_zoom)]
    pub(crate) zoom: Option<[f32; 3]>,
    /// Where the flood starts: `starts` (the module's entry and
    /// waypoints) or `walkmesh` (every in-bounds spot: stricter)
    #[arg(long, default_value = "starts")]
    pub(crate) seeds: String,
}

/// Runs `kjk collision audit`.
pub(crate) fn audit(args: AuditArgs) -> ExitCode {
    let AuditArgs {
            module,
            cell,
            doors,
            show,
            zoom,
            seeds,
    } = args;
    let from_walkmesh = match seeds.as_str() {
        "starts" => false,
        "walkmesh" => true,
        other => {
            eprintln!("--seeds {other}: use starts or walkmesh");
            return ExitCode::from(2);
        }
    };
    let Some(doors) = doors_of(&doors) else {
        eprintln!("--doors {doors}: use openable, closed or all");
        return ExitCode::from(2);
    };
    collision_audit::run(
        &discover(),
        &module.game,
        &module.module,
        module.clip(),
        &collision_audit::Options {
            cell,
            doors,
            show,
            zoom,
            from_walkmesh,
        },
    )
}

/// `kjk collision` in a build with the editor's tools alone (no `probes`):
/// the containment audit.
#[derive(clap::Subcommand)]
pub(crate) enum CollisionTools {
    /// The containment audit: where a player gets off the walkmesh by
    /// walking and by each jump level; writes facts.json and audit.png.
    Audit(AuditArgs),
}
