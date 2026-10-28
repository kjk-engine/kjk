//! TSL's rain and snow, held round the camera as the executable holds them
//! (EXE-NOTES `WEATHER_ROLL`, `WEATHER_HOLD`, `WEATHER_WIND`,
//! `WEATHER_INDOORS`).
//!
//! The weather is the area's roll for the 300 s the presented sim clock is
//! in (`asset_odyssey::weather`; the scene's clock in the viewers), alike on
//! every peer and outside the sim. While rain or snow falls and the player
//! stands outdoors, six copies of the area's `fx_rain` or `fx_snow` stand on
//! a hexagon of radius 4.5 m round the camera's ground point, turned with
//! the camera (one ahead, one behind, four at the sides 5 m higher), 8 m
//! over the player's feet, moved there every frame. Their particles are
//! folded into a 10 m square round the camera, so the camera never walks
//! out of the fall, and those of emitters the wind moves (`fx_rain`'s and
//! `fx_snow`'s all) drift along +X+Y at the area's `WindPower` in metres a
//! second; the executable's gusts on top of that are not copied [verify].
//!
//! In a room the area keeps dry (`DisableWeather`), found by the player's
//! feet, the copies are gone at once, no drop finishing its fall, and made
//! again on stepping out (the original's pop). When the weather stops, the
//! fall goes on for 6 s and is then gone at once: the executable's 6 s fade
//! scrubs a clip neither model has.
//!
//! While the weather has a chance of lightning and the player stands
//! outdoors, a check comes every 5 to 10 s (`LIGHTNING_INTERVAL`); one that
//! passes is distant thunder alone, or a flash: `fx_lightning01`'s light
//! 10 m over the player, a few metres aside, keyed by one of its three flash
//! clips while that plays (`LIGHTNING_CLIP`, `LIGHTNING_LIGHT`), with close
//! thunder. The light is a term every Odyssey material and the grass add
//! (`Flash`), rooms included, which the light list leaves to their
//! lightmaps [verify]; the executable's cut of the shadows to 40 % during a
//! flash is not copied (no dynamic shadows here). The thunder is TSL's
//! (`THUNDER`; K1 names none), heard while the area's ambient sound plays.

use std::sync::Arc;

use asset_odyssey::module::AreaWeather;
use asset_odyssey::msvc_rand::MsvcRand;
use asset_odyssey::weather::{Fall, ROLL_MS, Weather, area_key, draw, weather};
use audio::{Clip, Falloff};
use bevy::prelude::*;
use bevy::render::storage::ShaderBuffer;
use console::{AnimationTime, DumpDue, DumpFacts};
use fmt_odyssey::mdl::{
    CONTROLLER_LIGHT_COLOR as LIGHT_COLOUR, CONTROLLER_LIGHT_MULTIPLIER as LIGHT_MULTIPLIER,
    CONTROLLER_LIGHT_RADIUS as LIGHT_RADIUS, Controller, Node,
};
use vfs::Game;
use vfs::resolver::Resolver;

use crate::fx_model::{self, Drawing, Spawned, Spawning};
use crate::grass::GrassMaterial;
use crate::heat_haze::HeatHazeMaterial;
use crate::lifecycle_view::SceneCamera;
use crate::material::AreaLights;
use crate::model_fx::WeatherSpray;
#[cfg(feature = "probes")]
use crate::probe_kit::{Checks, check, not_installed};
use crate::scene::{
    self, ModelAssets, PlayerAt, PlayerRoom, SceneContent, SceneRoot, Spawner, game_to_bevy,
};
use crate::sim_host::PresentedSnapshot;
use crate::sound::{Added, Cue, Cues};

/// How far over the player's feet the copies stand (the side four 5 m
/// more, `rig_offsets`).
const LIFT_M: f32 = 8.0;
/// The side of the square the particles are folded into.
const SQUARE_M: f32 = 10.0;
/// How long the fall goes on after the weather stops.
const STOP_MS: f64 = 6000.0;
/// The model whose light a flash is, and that light's node.
const LIGHTNING_MODEL: &str = "fx_lightning01";
const LIGHTNING_LIGHT: &str = "auroralight01";
/// How high over the player's feet a flash's light stands.
const FLASH_LIFT_M: f32 = 10.0;
/// TSL's thunder by its pair: close (with a flash), then distant.
const THUNDER: [&str; 4] = [
    "as_wt_thundercl1",
    "as_wt_thundercl2",
    "as_wt_thunderds1",
    "as_wt_thunderds2",
];

pub struct WeatherPlugin;

impl Plugin for WeatherPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<WeatherNow>()
            .init_resource::<Held>()
            .init_resource::<Flash>()
            .add_systems(
                Update,
                forget
                    .after(scene::spawn_scene)
                    .run_if(resource_exists_and_changed::<SceneContent>),
            )
            .add_systems(
                PostUpdate,
                (
                    now,
                    storm,
                    light.run_if(resource_changed::<Flash>),
                    keep,
                    hold,
                )
                    .chain()
                    .after(crate::play_view::Drawn)
                    .before(bevy::transform::TransformSystems::Propagate),
            );
    }
}

/// The weather now: what falls, the lightning's chance and the wind
/// (`asset_odyssey::weather::Weather`), whether the player stands in a room
/// kept dry, and the lightning flashes so far.
#[derive(Resource, Default, Debug)]
pub(crate) struct WeatherNow {
    pub weather: Weather,
    pub indoors: bool,
    pub flashes: u32,
}

/// The lightning's light while a flash plays: where (Bevy world space), its
/// radius (m) and its colour (its keyed colour × multiplier, added to the
/// stored, gamma-encoded values the shaders work on); radius 0 when none.
#[derive(Resource, Default, PartialEq, Debug, Clone, Copy)]
pub(crate) struct Flash {
    pub at: Vec3,
    pub radius: f32,
    pub colour: Vec3,
}

/// What an area's lightning plays: `fx_lightning01`'s light (its radius and
/// multiplier), its flash clips (`flash01` to `flash03`) and TSL's thunder,
/// decoded, by resref.
pub struct Lightning {
    pub radius: f32,
    pub multiplier: f32,
    pub flashes: Vec<FlashClip>,
    pub thunder: Vec<(String, Clip)>,
}

/// A flash clip: its length (s) and the colour keys of its light (time,
/// RGB).
#[derive(Debug, Clone, PartialEq)]
pub struct FlashClip {
    pub length: f32,
    pub colours: Vec<(f32, [f32; 3])>,
}

impl Lightning {
    /// Flash clip `clip` (1 to 3).
    fn clip(&self, clip: u8) -> Option<&FlashClip> {
        self.flashes.get(usize::from(clip).checked_sub(1)?)
    }
}

/// What an area with `weather` lightning plays; none without a chance of
/// it. The light is `fx_lightning01`'s; the thunder TSL's alone.
pub(crate) fn lightning(
    resolver: &Resolver,
    game: Game,
    weather: &AreaWeather,
) -> Option<Lightning> {
    if weather.lightning == 0 {
        return None;
    }
    let model = crate::viewer::parsed(resolver, LIGHTNING_MODEL)
        .ok()
        .flatten();
    let light = model
        .as_ref()
        .and_then(|model| named(&model.root, LIGHTNING_LIGHT));
    let rest = |kind: u32| {
        light
            .and_then(|light| controller(light, kind))
            .and_then(|controller| {
                crate::model_fx::rows(controller)
                    .first()?
                    .1
                    .first()
                    .copied()
            })
    };
    let flashes = (1..=3)
        .map_while(|clip| {
            let name = format!("flash{clip:02}");
            let animation = model
                .as_ref()?
                .animations
                .iter()
                .find(|animation| animation.name.eq_ignore_ascii_case(&name))?;
            let colours = named(&animation.root, LIGHTNING_LIGHT)
                .and_then(|light| controller(light, LIGHT_COLOUR))
                .map_or_else(Vec::new, crate::model_fx::rows)
                .into_iter()
                .filter_map(|(time, rgb)| Some((time, <[f32; 3]>::try_from(rgb.get(..3)?).ok()?)))
                .collect();
            Some(FlashClip {
                length: animation.length,
                colours,
            })
        })
        .collect();
    let thunder = THUNDER
        .iter()
        .filter(|_| game == Game::Kotor2)
        .filter_map(|resref| {
            let clip = crate::sound::kotor_file(resolver, resref)
                .ok_or_else(|| "not found".to_string())
                .and_then(|bytes| Clip::kotor(&bytes))
                .inspect_err(|why| warn!(target: "sound", "{resref}: {why}"))
                .ok()?;
            Some((resref.to_string(), clip))
        })
        .collect();
    Some(Lightning {
        radius: rest(LIGHT_RADIUS).unwrap_or(0.0),
        multiplier: rest(LIGHT_MULTIPLIER).unwrap_or(1.0),
        flashes,
        thunder,
    })
}

/// The node `name` at or under `node`.
fn named<'a>(node: &'a Node, name: &str) -> Option<&'a Node> {
    if node.name.eq_ignore_ascii_case(name) {
        return Some(node);
    }
    node.children.iter().find_map(|child| named(child, name))
}

fn controller(node: &Node, kind: u32) -> Option<&Controller> {
    node.controllers
        .iter()
        .find(|controller| controller.kind == kind)
}

/// The storm's clock: the next check, and the flash playing.
pub(crate) struct Storm {
    rand: MsvcRand,
    next_check_s: f32,
    /// Its clip (1 to 3), the time into it and its offset from the player
    /// (game space).
    flash: Option<(u8, f32, Vec3)>,
}

/// What a check that passes brings, with which of its pair of thunder
/// sounds (1 or 2) goes with it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum Strike {
    Flash { clip: u8, offset: Vec3, thunder: u8 },
    Distant { thunder: u8 },
}

impl Strike {
    /// The thunder that goes with it, by resref.
    fn thunder(self) -> String {
        match self {
            Strike::Flash { thunder, .. } => format!("as_wt_thundercl{thunder}"),
            Strike::Distant { thunder } => format!("as_wt_thunderds{thunder}"),
        }
    }
}

impl Storm {
    pub(crate) fn new(seed: u32) -> Storm {
        let mut storm = Storm {
            rand: MsvcRand(seed),
            next_check_s: 0.0,
            flash: None,
        };
        storm.arm();
        storm
    }

    /// The countdown to the next check: 5 to 10 s.
    fn arm(&mut self) {
        self.next_check_s = 5.0 + (self.rand.next() % 5000) as f32 / 1000.0;
    }

    /// Up to 9.9 m either way.
    fn aside(&mut self) -> f32 {
        let metres = (self.rand.next() % 100) as f32 / 10.0;
        if self.rand.next() & 1 == 1 {
            -metres
        } else {
            metres
        }
    }

    /// Advance `dt` seconds with lightning chance `chance` (0: none);
    /// a check every 5 + (rand % 5000) / 1000 s; `rand % 100 < chance` passes;
    /// then even `rand`: distant thunder only; odd: a flash with clip
    /// `rand % 3 + 1` and offset (±(rand % 100) / 10, ±(rand % 100) / 10, 10),
    /// each sign a further `rand & 1`. A flash re-arms the countdown when it ends.
    /// Last, the thunder's coin (`rand & 1`), drawn whether or not it is
    /// heard, so the storm goes alike with sound or without.
    pub(crate) fn step(
        &mut self,
        dt: f32,
        chance: i32,
        clip_length: impl Fn(u8) -> f32,
    ) -> Option<Strike> {
        if let Some((clip, time, _)) = &mut self.flash {
            *time += dt;
            if *time >= clip_length(*clip) {
                self.flash = None;
                self.arm();
            }
            return None;
        }
        if chance <= 0 {
            return None;
        }
        self.next_check_s -= dt;
        if self.next_check_s > 0.0 {
            return None;
        }
        self.arm();
        if (self.rand.next() % 100) as i32 >= chance {
            return None;
        }
        if self.rand.next() & 1 == 0 {
            return Some(Strike::Distant {
                thunder: self.coin(),
            });
        }
        let clip = (self.rand.next() % 3 + 1) as u8;
        let offset = Vec3::new(self.aside(), self.aside(), FLASH_LIFT_M);
        self.flash = Some((clip, 0.0, offset));
        Some(Strike::Flash {
            clip,
            offset,
            thunder: self.coin(),
        })
    }

    /// The flash playing: its clip, the time into it and its offset.
    pub(crate) fn flash(&self) -> Option<(u8, f32, Vec3)> {
        self.flash
    }

    /// Which of a pair of thunder sounds plays (1 or 2): a coin.
    fn coin(&mut self) -> u8 {
        (self.rand.next() & 1) as u8 + 1
    }
}

/// One of the six weather copies' roots.
#[derive(Component)]
pub(crate) struct WeatherEmitter;

/// The copies out: the rig under the scene's root that their sprays hang
/// from (game space), and the copies.
struct Out {
    rig: Entity,
    copies: Vec<Spawned>,
}

/// What is out, when the weather stopped while it was, the clock now and
/// when it was drawn last (ms); the storm and when it was stepped last.
#[derive(Resource, Default)]
struct Held {
    out: Option<Out>,
    stopped_ms: Option<f64>,
    ms: f64,
    drawn_ms: Option<f64>,
    storm: Option<Storm>,
    stormed_ms: Option<f64>,
}

/// Where the six weather emitters stand round the camera's ground point,
/// turned by the camera's yaw (radians, game space, +Z up), before the
/// player-height lift: front, back and four sides 5 m higher.
///
/// The yaw is the camera's turn from looking along +Y (as models face), so
/// the first stands straight ahead. The executable's table: 0, ±4.5 and
/// ±3.897, ±2.25 on the hexagon, at heights 0 and 5.
pub(crate) fn rig_offsets(yaw: f32) -> [Vec3; 6] {
    let turn = Quat::from_rotation_z(yaw);
    [
        Vec3::new(0.0, 4.5, 0.0),
        Vec3::new(0.0, -4.5, 0.0),
        Vec3::new(3.897, 2.25, 5.0),
        Vec3::new(-3.897, 2.25, 5.0),
        Vec3::new(3.897, -2.25, 5.0),
        Vec3::new(-3.897, -2.25, 5.0),
    ]
    .map(|offset| turn * offset)
}

/// A weather particle's place folded into the 10 m square round `centre` (x and y).
pub(crate) fn wrapped(at: Vec3, centre: Vec2) -> Vec3 {
    let half = SQUARE_M / 2.0;
    let fold = |v: f32, c: f32| (v - c + half).rem_euclid(SQUARE_M) - half + c;
    Vec3::new(fold(at.x, centre.x), fold(at.y, centre.y), at.z)
}

/// The clock the weather goes by (ms): the presented sim's, else the
/// scene's.
fn clock_ms(presented: Option<&PresentedSnapshot>, time: &Time, held: &AnimationTime) -> f64 {
    presented.map_or_else(
        || f64::from(scene::animation_seconds(time, held)) * 1000.0,
        |presented| crate::sim_host::view_ms(&presented.0),
    )
}

/// A new scene: what was out went with the old one, and its storm. The
/// area's thunder goes into the bank.
fn forget(mut held: ResMut<Held>, scene: Res<SceneContent>, added: Option<ResMut<Added>>) {
    *held = Held::default();
    if let (Some(mut added), Some(lightning)) = (added, &scene.0.weather.lightning) {
        added.0.extend(
            lightning
                .thunder
                .iter()
                .map(|(resref, clip)| (thunder_name(resref), clip.clone())),
        );
    }
}

/// The bank's name for a thunder sound.
fn thunder_name(resref: &str) -> String {
    format!("weather/{resref}")
}

/// The lightning: the storm stepped while the weather has a chance of it
/// and the player stands outdoors; a strike counted, its thunder heard
/// while the area's ambient sound plays; and the flash's light over the
/// player while its clip plays.
fn storm(
    mut held: ResMut<Held>,
    (mut now, at): (ResMut<WeatherNow>, Res<PlayerAt>),
    (scene, presented): (Option<Res<SceneContent>>, Option<Res<PresentedSnapshot>>),
    (time, animation): (Res<Time>, Res<AnimationTime>),
    (mut flash, cues): (ResMut<Flash>, Option<ResMut<Cues>>),
) {
    let Some(scene) = scene else { return };
    let ms = clock_ms(presented.as_deref(), &time, &animation);
    let dt = held
        .stormed_ms
        .replace(ms)
        .filter(|was| *was <= ms)
        .map_or(0.0, |was| ((ms - was) / 1000.0) as f32);
    if now.indoors {
        flash.set_if_neq(Flash::default());
        return;
    }
    let lightning = scene.0.weather.lightning.as_deref();
    let length = |clip: u8| {
        lightning
            .and_then(|lightning| lightning.clip(clip))
            .map_or(0.0, |clip| clip.length)
    };
    let storm = held
        .storm
        .get_or_insert_with(|| Storm::new(area_key(&scene.0.weather.resref)));
    if let Some(strike) = storm.step(dt, now.weather.lightning, length) {
        if matches!(strike, Strike::Flash { .. }) {
            now.flashes += 1;
        }
        // The thunder routine's own check: the area's ambient sound playing
        // (always, without a sim to stop it).
        let ambient = presented.as_ref().is_none_or(|presented| {
            presented.0.current().area_sound.ambient.state != sim::sound::STOPPED
        });
        if let (true, Some(mut cues), Some(lightning)) = (ambient, cues, lightning)
            && !lightning.thunder.is_empty()
        {
            cues.0.push(Cue {
                falloff: Falloff::Everywhere,
                ..Cue::at(thunder_name(&strike.thunder()), Vec3::ZERO, ms as f32)
            });
        }
    }
    let lit = storm
        .flash()
        .zip(lightning)
        .and_then(|((clip, time, offset), lightning)| {
            let keys = &lightning.clip(clip)?.colours;
            let colour = crate::model_fx::sample_illum(keys, time)?;
            Some(Flash {
                at: game_to_bevy() * (at.0? + offset),
                radius: lightning.radius,
                colour: Vec3::from(colour) * lightning.multiplier,
            })
        });
    flash.set_if_neq(lit.unwrap_or(Flash::default()));
}

/// The flash's light, when it changes: into the area's shared light list
/// (`AREA_LIGHTS`, which every scene material reads; a 3D view's stage keeps
/// its own and is not flashed), and into the grass's few materials.
fn light(
    flash: Res<Flash>,
    area: Option<ResMut<AreaLights>>,
    buffers: Option<ResMut<Assets<ShaderBuffer>>>,
    grass: Option<ResMut<Assets<GrassMaterial>>>,
) {
    let (at, colour) = (flash.at.extend(flash.radius), flash.colour.extend(0.0));
    if let (Some(mut area), Some(mut buffers)) = (area, buffers) {
        area.0.flash_at = at;
        area.0.flash_colour = colour;
        area.upload(&mut buffers);
    }
    for (_, material) in grass
        .into_iter()
        .flat_map(|assets| assets.into_inner().iter_mut())
    {
        material.params.flash_at = at;
        material.params.flash_colour = colour;
    }
}

/// The weather the area's roll gives for the 300 s the clock is in, and
/// whether the player stands in a room the area keeps dry.
fn now(
    scene: Option<Res<SceneContent>>,
    presented: Option<Res<PresentedSnapshot>>,
    room: Res<PlayerRoom>,
    (time, animation): (Res<Time>, Res<AnimationTime>),
    (mut now, mut held): (ResMut<WeatherNow>, ResMut<Held>),
) {
    let Some(scene) = scene else { return };
    held.ms = clock_ms(presented.as_deref(), &time, &animation);
    let epoch = held.ms as u64 / ROLL_MS;
    now.weather = weather(&scene.0.weather.area, draw(&scene.0.weather.resref, epoch));
    now.indoors = room
        .0
        .is_some_and(|room| scene.0.weather.dry_rooms.get(room) == Some(&true));
}

/// Makes the six copies when rain or snow falls outdoors and none are out,
/// and takes them away indoors at once, or 6 s after the weather stops.
fn keep(
    mut held: ResMut<Held>,
    (now, scene): (Res<WeatherNow>, Option<Res<SceneContent>>),
    roots: Query<Entity, With<SceneRoot>>,
    (mut assets, mut hazes): (ModelAssets, ResMut<Assets<HeatHazeMaterial>>),
    mut commands: Commands,
) {
    let Some(scene) = scene else { return };
    let held = &mut *held;
    let ms = held.ms;
    let falling = scene
        .0
        .weather
        .model
        .as_ref()
        .filter(|model| model.fall == now.weather.fall);
    if falling.is_some() {
        held.stopped_ms = None;
    } else if held.out.is_some() {
        held.stopped_ms.get_or_insert(ms);
    }
    let over = held
        .stopped_ms
        .is_some_and(|stopped| ms - stopped >= STOP_MS);
    if now.indoors || over {
        if let Some(out) = held.out.take() {
            commands.entity(out.rig).despawn();
        }
        held.stopped_ms = None;
    } else if held.out.is_none()
        && let (Some(model), Ok(root)) = (falling, roots.single())
    {
        held.out = Some(make(
            model,
            root,
            &scene.0,
            (&mut assets, &mut hazes),
            &mut commands,
        ));
    }
}

/// Moves the copies round the camera and draws them, their particles kept
/// to the square round it and pushed by the wind, and puts the weather in
/// the dump.
fn hold(
    mut held: ResMut<Held>,
    (now, at, room): (Res<WeatherNow>, Res<PlayerAt>, Res<PlayerRoom>),
    cameras: Query<Entity, SceneCamera>,
    mut posing: ParamSet<(Query<&mut Transform>, Drawing)>,
    (mut facts, dump_due): (ResMut<DumpFacts>, Res<DumpDue>),
) {
    let held = &mut *held;
    let ms = held.ms;
    let dt = held
        .drawn_ms
        .replace(ms)
        .filter(|was| *was <= ms)
        .map_or(0.0, |was| ((ms - was) / 1000.0) as f32);
    let mut particles = 0;
    if let Some(out) = held.out.as_mut() {
        let mut transforms = posing.p0();
        let eye = cameras
            .iter()
            .next()
            .and_then(|camera| transforms.get(camera).ok())
            .copied();
        // The camera's point and turn from looking along +Y, in game space.
        let (centre, yaw) = eye.map_or((Vec2::ZERO, 0.0), |eye| {
            let to_game = game_to_bevy().inverse();
            let ahead = to_game * (eye.rotation * Vec3::NEG_Z);
            (
                (to_game * eye.translation).truncate(),
                f32::atan2(-ahead.x, ahead.y),
            )
        });
        let base = centre.extend(at.0.map_or(0.0, |feet| feet.z) + LIFT_M);
        let wind = Vec2::ONE.normalize() * now.weather.wind as f32;
        for (copy, offset) in out.copies.iter_mut().zip(rig_offsets(yaw)) {
            if let Ok(mut transform) = transforms.get_mut(copy.root) {
                transform.translation = base + offset;
            }
            fx_model::animate(copy, &[], (ms, dt), Vec::new(), &mut transforms);
            for spray in &mut copy.sprays {
                spray.weather = Some(WeatherSpray { centre, wind });
            }
        }
        let facing = fx_model::facing(eye.map_or(Quat::IDENTITY, |eye| eye.rotation));
        let mut drawing = posing.p1();
        for copy in &mut out.copies {
            drawing.effects(copy, None, (ms, dt), facing);
            particles += copy
                .sprays
                .iter()
                .map(|spray| spray.particles())
                .sum::<usize>();
        }
    }
    if dump_due.0 {
        let weather = now.weather;
        facts.0.insert(
            "weather".into(),
            serde_json::json!({
                "fall": format!("{:?}", weather.fall),
                "lightning": weather.lightning,
                "wind": weather.wind,
                "running": weather.fall != Fall::None || weather.lightning != 0,
                "indoors": now.indoors,
                "player_room": room.0,
                "player_at": at.0.map(|at| at.to_array()),
                "emitters": held.out.as_ref().map_or(0, |out| out.copies.len()),
                "particles": particles,
                "flashes": now.flashes,
            }),
        );
    }
}

/// The rig under `root` and the six copies of the area's weather model on
/// it, each root marked `WeatherEmitter`.
fn make(
    model: &Arc<crate::viewer::WeatherModel>,
    root: Entity,
    scene: &crate::viewer::LoadedScene,
    (assets, hazes): (&mut ModelAssets, &mut Assets<HeatHazeMaterial>),
    commands: &mut Commands,
) -> Out {
    let rig = commands
        .spawn((Transform::IDENTITY, Visibility::default()))
        .id();
    commands.entity(root).add_child(rig);
    let (images, stores) = assets.split();
    let mut spawning = Spawning {
        spawner: Spawner::new(&model.textures, scene.area_light(), images),
        stores,
        hazes,
        textures: &model.textures,
        area: scene.area_light(),
        lit: scene.lit,
        root: rig,
        seed: 1,
        commands,
    };
    let copies = rig_offsets(0.0)
        .map(|_| {
            let copy = spawning.spawn(model.model.copy(), false);
            spawning.commands.entity(copy.root).insert(WeatherEmitter);
            copy
        })
        .into();
    Out { rig, copies }
}

/// Whether every point of `want` is in `got` (within 0.001), as many of each.
#[cfg(feature = "probes")]
fn same_set(got: &[Vec3], want: &[Vec3]) -> bool {
    let mut left = got.to_vec();
    got.len() == want.len()
        && want.iter().all(|w| {
            left.iter()
                .position(|g| g.distance(*w) < 0.001)
                .map(|k| left.swap_remove(k))
                .is_some()
        })
}

/// The hexagon, the fold, the copies gone indoors and after the weather
/// stops, and the weather the games' own areas have.
#[cfg(feature = "probes")]
pub fn probes() -> Checks {
    let mut checks = vec![
        ("weather_rig_hexagon", hexagon_probe()),
        ("weather_wrap", wrap_probe()),
    ];
    checks.extend(held_probes());
    checks.push(("weather_scenes", scene_probe()));
    checks.extend([
        ("storm_intervals", storm_intervals_probe()),
        ("storm_never", storm_never_probe()),
    ]);
    checks.extend(flash_probes());
    checks.push(("lightning_scenes", lightning_scene_probe()));
    checks
}

/// A flash clip's length in the storm probes: `flash02`'s for clip 2, 2 s
/// for the others.
#[cfg(feature = "probes")]
fn probe_length(clip: u8) -> f32 {
    if clip == 2 { 5.0 / 3.0 } else { 2.0 }
}

/// The strikes of `Storm::new(seed)` stepped at 0.1 s for `seconds` with
/// `chance`: when (s) and what.
#[cfg(feature = "probes")]
fn strikes(seed: u32, chance: i32, seconds: u32) -> Vec<(f32, Strike)> {
    let mut storm = Storm::new(seed);
    (1..=seconds * 10)
        .filter_map(|step| {
            let strike = storm.step(0.1, chance, probe_length)?;
            Some((step as f32 / 10.0, strike))
        })
        .collect()
}

/// Chance 100 for 600 s: every gap between strikes (from a flash's end) is
/// 5 to 10 s, give or take two steps; both kinds come; every flash's clip
/// is 1 to 3 and its offset within 9.9 m aside and 10 m up.
#[cfg(feature = "probes")]
fn storm_intervals_probe() -> Result<String, String> {
    let struck = strikes(7, 100, 600);
    let ends: Vec<f32> = struck
        .iter()
        .map(|(at, strike)| match strike {
            Strike::Flash { clip, .. } => at + probe_length(*clip),
            Strike::Distant { .. } => *at,
        })
        .collect();
    let gaps: Vec<f32> = struck
        .iter()
        .skip(1)
        .zip(&ends)
        .map(|((at, _), end)| at - end)
        .collect();
    let (shortest, longest) = gaps.iter().fold((f32::MAX, f32::MIN), |(low, high), gap| {
        (low.min(*gap), high.max(*gap))
    });
    let flashes: Vec<(u8, Vec3)> = struck
        .iter()
        .filter_map(|(_, strike)| match strike {
            Strike::Flash { clip, offset, .. } => Some((*clip, *offset)),
            Strike::Distant { .. } => None,
        })
        .collect();
    let distant = struck.len() - flashes.len();
    let placed = flashes.iter().all(|(clip, offset)| {
        (1..=3).contains(clip)
            && offset.x.abs() <= 9.9 + 1e-4
            && offset.y.abs() <= 9.9 + 1e-4
            && offset.z == FLASH_LIFT_M
    });
    let clips: std::collections::BTreeSet<u8> = flashes.iter().map(|(clip, _)| *clip).collect();
    check(
        struck.len() > 40
            && shortest >= 5.0 - 1e-3
            && longest <= 10.0 + 0.2 + 1e-3
            && !flashes.is_empty()
            && distant > 0
            && placed
            && clips.len() == 3,
        format!(
            "{} strikes ({} flashes, clips {clips:?}; {distant} distant), gaps {shortest:.2} to {longest:.2} s, offsets in bounds {placed}",
            struck.len(),
            flashes.len()
        ),
    )
}

#[cfg(feature = "probes")]
fn storm_never_probe() -> Result<String, String> {
    let struck = strikes(7, 0, 600);
    check(
        struck.is_empty(),
        format!("{} strikes at chance 0", struck.len()),
    )
}

/// A made lightning: radius 20, multiplier 100, each clip's colour rising
/// from black to (0.01, 0.005, 0.002) at 0.5 s and back by its end; the
/// four thunder sounds silent.
#[cfg(feature = "probes")]
fn made_lightning() -> Lightning {
    let silent = || {
        let mut bytes = b"KJKA".to_vec();
        for word in [Clip::VERSION, 22050, 0] {
            bytes.extend(word.to_le_bytes());
        }
        Clip::from_encoded(&bytes).expect("an empty clip reads")
    };
    Lightning {
        radius: 20.0,
        multiplier: 100.0,
        flashes: (1..=3)
            .map(|clip| {
                let length = probe_length(clip);
                FlashClip {
                    length,
                    colours: vec![
                        (0.0, [0.0; 3]),
                        (0.5, [0.01, 0.005, 0.002]),
                        (length, [0.0; 3]),
                    ],
                }
            })
            .collect(),
        thunder: THUNDER
            .iter()
            .map(|resref| (resref.to_string(), silent()))
            .collect(),
    }
}

/// A probe frame: its `Flash`, the storm's flash (clip, time, offset), and
/// the shared light list's flash place and colour (those uploaded).
#[cfg(feature = "probes")]
type Frame = (Flash, Option<(u8, f32, Vec3)>, Vec4, Vec4);

/// An app with the storm and the light over an area of lightning chance
/// 100, the player at (1, 2, 3), stepped at 0.1 s for 30 s, outdoors or
/// in: each frame's `Flash` with the storm's flash, the shared light list's
/// flash, and the thunder asked for.
#[cfg(feature = "probes")]
struct Stepped {
    frames: Vec<Frame>,
    flashes: u32,
    cues: Vec<String>,
    added: Vec<String>,
}

#[cfg(feature = "probes")]
fn stepped(indoors: bool) -> Stepped {
    let mut app = App::new();
    app.add_plugins(MinimalPlugins)
        .init_resource::<Assets<ShaderBuffer>>()
        .init_resource::<Assets<GrassMaterial>>()
        .init_resource::<AreaLights>()
        .init_resource::<Held>()
        .init_resource::<Flash>()
        .init_resource::<Cues>()
        .init_resource::<Added>()
        .insert_resource(PlayerAt(Some(Vec3::new(1.0, 2.0, 3.0))))
        .insert_resource(AnimationTime(Some(0.0)))
        .insert_resource(WeatherNow {
            weather: Weather {
                lightning: 100,
                ..Weather::default()
            },
            indoors,
            flashes: 0,
        })
        .insert_resource(SceneContent(Box::new(crate::viewer::LoadedScene {
            weather: crate::viewer::SceneWeather {
                resref: "made".into(),
                lightning: Some(Arc::new(made_lightning())),
                ..Default::default()
            },
            ..Default::default()
        })))
        .add_systems(
            Update,
            (
                forget.run_if(resource_exists_and_changed::<SceneContent>),
                storm,
                light.run_if(resource_changed::<Flash>),
            )
                .chain(),
        );
    let _ = app
        .world_mut()
        .resource_mut::<Assets<ShaderBuffer>>()
        .insert(
            &crate::material::AREA_LIGHTS,
            ShaderBuffer::from(crate::material::LightList::default()),
        );
    let mut frames = Vec::new();
    let mut cues = Vec::new();
    let mut added = Vec::new();
    for step in 0..=300 {
        app.world_mut().resource_mut::<AnimationTime>().0 = Some(step as f32 / 10.0);
        app.update();
        let world = app.world_mut();
        // The list as uploaded: the buffer's bytes are the shared list's.
        let list = world.resource::<AreaLights>().0;
        let uploaded = world
            .resource::<Assets<ShaderBuffer>>()
            .get(&crate::material::AREA_LIGHTS)
            .is_some_and(|buffer| buffer.data == ShaderBuffer::from(list).data);
        let playing = world
            .resource::<Held>()
            .storm
            .as_ref()
            .and_then(Storm::flash);
        frames.push((
            *world.resource::<Flash>(),
            playing,
            if uploaded { list.flash_at } else { Vec4::NAN },
            list.flash_colour,
        ));
        cues.extend(world.resource_mut::<Cues>().0.drain(..).map(|cue| cue.name));
        added.extend(
            world
                .resource_mut::<Added>()
                .0
                .drain(..)
                .map(|(name, _)| name),
        );
    }
    Stepped {
        frames,
        flashes: app.world().resource::<WeatherNow>().flashes,
        cues,
        added,
    }
}

/// Outdoors: while a flash plays its light is 20 m round the player + its
/// offset (Bevy space) in the colour its clip keys × 100, the shared light
/// list carrying the same, uploaded; none after; each flash counted, with close thunder,
/// and the distant strikes with distant thunder. Indoors: nothing in 30 s.
#[cfg(feature = "probes")]
fn flash_probes() -> Checks {
    let lightning = made_lightning();
    let outdoors = stepped(false);
    let mut lit = 0;
    let faithful = outdoors.frames.iter().all(|(flash, playing, at, colour)| {
        let want = playing.map_or(Flash::default(), |(clip, t, offset)| {
            let keys = &lightning.clip(clip).expect("three clips").colours;
            Flash {
                at: game_to_bevy() * (Vec3::new(1.0, 2.0, 3.0) + offset),
                radius: 20.0,
                colour: Vec3::from(crate::model_fx::sample_illum(keys, t).expect("keyed")) * 100.0,
            }
        });
        lit += usize::from(flash.radius > 0.0);
        flash.at.distance(want.at) < 1e-4
            && flash.radius == want.radius
            && flash.colour.distance(want.colour) < 1e-5
            && *at == flash.at.extend(flash.radius)
            && *colour == flash.colour.extend(0.0)
    });
    let starts = outdoors
        .frames
        .windows(2)
        .filter(|pair| pair[0].1.is_none() && pair[1].1.is_some())
        .count() as u32;
    let close = outdoors
        .cues
        .iter()
        .filter(|cue| cue.contains("thundercl"))
        .count() as u32;
    let distant = outdoors
        .cues
        .iter()
        .filter(|cue| cue.contains("thunderds"))
        .count();
    let named = outdoors
        .cues
        .iter()
        .all(|cue| THUNDER.iter().any(|resref| *cue == thunder_name(resref)));
    let banked = outdoors.added.len() == THUNDER.len();
    let lights = check(
        faithful
            && starts > 0
            && lit > 0
            && outdoors.flashes == starts
            && close == starts
            && distant > 0
            && named
            && banked,
        format!(
            "{} flashes ({lit} frames lit, true to the keys: {faithful}), thunder {close} close {distant} distant, named {named}, banked {banked}",
            outdoors.flashes
        ),
    );
    let indoors = stepped(true);
    let dark = check(
        indoors.flashes == 0
            && indoors.cues.is_empty()
            && indoors.frames.iter().all(|(flash, ..)| flash.radius == 0.0),
        format!(
            "indoors: {} flashes, {} thunder",
            indoors.flashes,
            indoors.cues.len()
        ),
    );
    vec![("flash_lights", lights), ("flash_indoors", dark)]
}

/// TSL's `fx_lightning01`: radius 20, multiplier 100, three flash clips of
/// 2, 1.67 and 2 s keying its colour, and the four thunder sounds; K1
/// plays no thunder; an area without lightning none of it.
#[cfg(feature = "probes")]
fn lightning_scene_probe() -> Result<String, String> {
    let discovery = crate::discover();
    let mut seen = Vec::new();
    let weather = AreaWeather {
        lightning: 100,
        ..AreaWeather::default()
    };
    for game in [Game::Kotor2, Game::Kotor1] {
        if discovery.get(game).is_none() {
            seen.push(format!("{game:?} {}", not_installed(game)));
            continue;
        }
        let resolver = crate::viewer::resolver(&discovery, game, None)?;
        if lightning(&resolver, game, &AreaWeather::default()).is_some() {
            return Err(format!("{game:?}: lightning without a chance of it"));
        }
        let got = lightning(&resolver, game, &weather).ok_or("no lightning")?;
        let lengths: Vec<f32> = got.flashes.iter().map(|clip| clip.length).collect();
        let keyed = got.flashes.iter().all(|clip| clip.colours.len() > 2);
        let thunder: Vec<&str> = got.thunder.iter().map(|(name, _)| name.as_str()).collect();
        // Each clip's brightest key.
        let peaks: Vec<[f32; 3]> = got
            .flashes
            .iter()
            .filter_map(|clip| {
                clip.colours
                    .iter()
                    .map(|(_, rgb)| *rgb)
                    .max_by(|a, b| a[0].total_cmp(&b[0]))
            })
            .collect();
        let what = format!(
            "{game:?}: radius {}, multiplier {}, clips {lengths:?} keyed {keyed} (peaks {peaks:?}), thunder {thunder:?}",
            got.radius, got.multiplier
        );
        let light = got.radius == 20.0 && got.multiplier == 100.0;
        let ok = match game {
            Game::Kotor2 => {
                light
                    && keyed
                    && lengths.len() == 3
                    && (lengths[1] - 5.0 / 3.0).abs() < 1e-3
                    && thunder == THUNDER
            }
            // K1's model is the same if it has one; no thunder either way.
            _ => thunder.is_empty() && (got.flashes.is_empty() || light && keyed),
        };
        if !ok {
            return Err(what);
        }
        seen.push(what);
    }
    Ok(seen.join("; "))
}

#[cfg(feature = "probes")]
fn hexagon_probe() -> Result<String, String> {
    let ahead = [
        Vec3::new(0.0, 4.5, 0.0),
        Vec3::new(0.0, -4.5, 0.0),
        Vec3::new(3.897, 2.25, 5.0),
        Vec3::new(-3.897, 2.25, 5.0),
        Vec3::new(3.897, -2.25, 5.0),
        Vec3::new(-3.897, -2.25, 5.0),
    ];
    // A quarter turn: (x, y) to (−y, x).
    let quarter = [
        Vec3::new(-4.5, 0.0, 0.0),
        Vec3::new(4.5, 0.0, 0.0),
        Vec3::new(-2.25, 3.897, 5.0),
        Vec3::new(-2.25, -3.897, 5.0),
        Vec3::new(2.25, 3.897, 5.0),
        Vec3::new(2.25, -3.897, 5.0),
    ];
    let (zero, turned) = (rig_offsets(0.0), rig_offsets(std::f32::consts::FRAC_PI_2));
    check(
        same_set(&zero, &ahead) && same_set(&turned, &quarter),
        format!("{zero:?} / {turned:?}"),
    )
}

#[cfg(feature = "probes")]
fn wrap_probe() -> Result<String, String> {
    let folded = wrapped(Vec3::new(12.0, -7.0, 3.0), Vec2::ZERO);
    let inside = Vec3::new(1.5, -4.0, 2.0);
    let kept = wrapped(inside, Vec2::ZERO);
    let centres = [Vec2::ZERO, Vec2::new(103.7, -48.2), Vec2::new(-5.0, 5.0)];
    let within = centres.iter().all(|&centre| {
        (-40..=40).all(|i| {
            let at = Vec3::new(i as f32 * 3.7 - 11.0, i as f32 * -2.9 + 60.0, 1.0);
            let got = wrapped(at, centre);
            (got.truncate() - centre).abs().max_element() <= SQUARE_M / 2.0 && got.z == at.z
        })
    });
    check(
        folded.distance(Vec3::new(2.0, 3.0, 3.0)) < 1e-4 && kept.distance(inside) < 1e-4 && within,
        format!("(12, −7, 3) to {folded:?}, {inside:?} to {kept:?}, all within ±5: {within}"),
    )
}

/// A made weather model: no meshes and no emitters, enough to count.
#[cfg(feature = "probes")]
fn made_weather() -> crate::viewer::WeatherModel {
    use asset_odyssey::model::ModelIr;
    let ir = ModelIr {
        name: "fx_made".into(),
        supermodel: None,
        nodes: Vec::new(),
        meshes: Vec::new(),
        lights: Vec::new(),
        emitters: Vec::new(),
        animations: Vec::new(),
        animation_scale: 1.0,
        affected_by_fog: false,
        refusals: Vec::new(),
    };
    crate::viewer::WeatherModel {
        fall: Fall::Rain,
        model: fx_model::Model {
            placed: crate::viewer::Placed::at(ir, [0.0; 3]),
            rotating: false,
            clips: Default::default(),
            fx: Arc::default(),
            chunks: Arc::default(),
            floor: 0.0,
        },
        textures: Default::default(),
    }
}

/// An app with the weather's systems over a rainy area of two rooms, the
/// second kept dry, its clock held: the copies out in the wet room, gone
/// in the dry one, back on stepping out; and, the rain stopped, still out
/// 5.9 s on and gone at 6 s.
#[cfg(feature = "probes")]
fn held_probes() -> Checks {
    let mut app = crate::probe_kit::scene_app();
    app.init_resource::<WeatherNow>()
        .init_resource::<Held>()
        .init_resource::<PlayerRoom>()
        .init_resource::<PlayerAt>()
        .insert_resource(SceneContent(Box::new(crate::viewer::LoadedScene {
            weather: crate::viewer::SceneWeather {
                area: AreaWeather {
                    rain: 100,
                    ..Default::default()
                },
                resref: "made".into(),
                dry_rooms: vec![false, true],
                model: Some(Arc::new(made_weather())),
                ..Default::default()
            },
            lit: true,
            ..Default::default()
        })))
        .add_systems(Update, (now, keep, hold).chain());
    app.world_mut()
        .spawn((SceneRoot, Transform::IDENTITY, Visibility::default()));
    // The player in `room` at `seconds`: the copies out after a frame.
    let step = |app: &mut App, room: usize, seconds: f32| {
        app.world_mut().resource_mut::<PlayerRoom>().0 = Some(room);
        app.world_mut().resource_mut::<AnimationTime>().0 = Some(seconds);
        app.update();
        let world = app.world_mut();
        world
            .query_filtered::<(), With<WeatherEmitter>>()
            .iter(world)
            .count()
    };
    let (out, dry, back) = (
        step(&mut app, 0, 0.0),
        step(&mut app, 1, 0.5),
        step(&mut app, 0, 1.0),
    );
    let indoors = check(
        (out, dry, back) == (6, 0, 6),
        format!("wet {out}, dry {dry}, wet again {back}"),
    );
    app.world_mut()
        .resource_mut::<SceneContent>()
        .0
        .weather
        .area
        .rain = 0;
    let (stopped, late, gone) = (
        step(&mut app, 0, 2.0),
        step(&mut app, 0, 7.9),
        step(&mut app, 0, 8.0),
    );
    let stops = check(
        (stopped, late, gone) == (6, 6, 0),
        format!("stopped {stopped}, 5.9 s on {late}, 6 s on {gone}"),
    );
    vec![
        ("weather_indoors_despawns", indoors),
        ("weather_stops_after_6s", stops),
    ]
}

/// The weather of TSL's Dxun landing (rain, some rooms dry and some wet)
/// and Telos's polar area (snow), each with its model; K1's Dantooine
/// courtyard no chance of any.
#[cfg(feature = "probes")]
fn scene_probe() -> Result<String, String> {
    use vfs::Game;
    let discovery = crate::discover();
    let mut seen = Vec::new();
    for (game, key, module, fall) in [
        (Game::Kotor2, "tsl", "401dxn", Fall::Rain),
        (Game::Kotor2, "tsl", "261tel", Fall::Snow),
        (Game::Kotor1, "k1", "danm14aa", Fall::None),
    ] {
        if discovery.get(game).is_none() {
            seen.push(format!("{module} {}", not_installed(game)));
            continue;
        }
        let scene = crate::viewer::load_module(&discovery, key, module)?;
        let got = weather(&scene.weather.area, 0).fall;
        // Each emitter drawn from a node of its own below the model's root
        // (`fx_rain`'s shares the root's name).
        let model = scene.weather.model.as_ref().map(|model| {
            let (emitters, ir) = (&model.model.fx.emitters, &model.model.placed.ir);
            let windy = emitters.iter().filter(|emitter| emitter.wind).count();
            let own = emitters
                .iter()
                .filter(|emitter| {
                    crate::model_fx::emitter_node(ir, &emitter.node).is_some_and(|node| node > 0)
                })
                .count();
            (model.fall, emitters.len(), windy, own)
        });
        let dry = scene.weather.dry_rooms.iter().filter(|dry| **dry).count();
        let wet = scene.weather.dry_rooms.len() - dry;
        let ok = match fall {
            // Its wind blows (Dantooine's `WindPower` is 1): only the
            // chances are none.
            Fall::None => {
                let area = scene.weather.area;
                (area.rain, area.snow, area.lightning) == (0, 0, 0) && model.is_none()
            }
            fall => {
                got == fall
                    && model.is_some_and(|(drawn, emitters, windy, own)| {
                        drawn == fall && emitters > 0 && windy == emitters && own == emitters
                    })
                    && (fall != Fall::Rain || dry > 0 && wet > 0)
            }
        };
        let what = format!(
            "{module}: {got:?}, model (fall, emitters, windy, on their own nodes) {model:?}, rooms {dry} dry {wet} wet"
        );
        if !ok {
            return Err(what);
        }
        seen.push(what);
    }
    Ok(seen.join("; "))
}
