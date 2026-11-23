//! Movies (I20, `docs/SOUND.md`, "Movies"): what scripts play
//! (`PlayMovie`, sent as a screen, `script_screens`) or the console asks
//! for (`ui script movie <name>`), one after another, over everything:
//!
//! - black to the window's edges, from the first movie asked for to the
//!   end of the last one queued (nothing under it shows between them), the
//!   picture as large as the window allows, keeping its shape (a wide
//!   movie fills an ultrawide window's width), its sound unplaced
//!   at full gain;
//! - a Bink 1 movie (every KOTOR and TSL movie) decoded as it plays, on a
//!   thread of its own, a few frames ahead; any other read from the store,
//!   made there by `ffmpeg` the first time (`movie_verbs`); black while it
//!   loads;
//! - Escape, Space, Enter, a click or a pad's A, B or Start skips it, unless TSL's `QueueMovie`
//!   queued it unskippable; a skip of a queued one drops the rest of its
//!   queue run (`Queued`);
//! - while one is up (`Showing`), the host's world waits (`sim_host`: no
//!   ticks, `Waited`), no player's input plays, the host's or a guest's
//!   (`sim_host::PlayerInput`), every other sound is paused where it was (`sound`)
//!   and the music, ambience and sound objects stand down (`ambience`);
//! - its clock is real time; where the animation time is held (`anim`,
//!   `record`), that time less the game's, so a recording's frames and
//!   sound (`audio.wav`) keep in step while its world waits too.
//!
//! The menu starts with K1's logos (`leclogo`, `biologo`, `legal`).
//! A movie that is missing, or not Bink 1 when there is no `ffmpeg`, is
//! skipped, with a warning. A
//! headless run skips every one unless `KJK_MOVIES=1`; `KJK_MOVIES=0`
//! skips them in a window too (as xoreos's option to skip videos does; `played`).

use std::collections::VecDeque;
use std::sync::Mutex;
use std::sync::mpsc::{self, Receiver, TryRecvError};

use audio::{Bed, Stream};
use bevy::asset::RenderAssetUsages;
use bevy::camera::visibility::RenderLayers;
use bevy::camera::{OrthographicProjection, Projection, ScalingMode};
use bevy::ecs::system::SystemParam;
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
use movie::Movie;
use movie::player::Player;
use vfs::Game;

use crate::movie_verbs::Film;

use crate::lifecycle_view::{OverlayCamera, unlit};
use crate::material::OdysseyMaterial;
use crate::sim_host::{PresentedSnapshot, SceneSwap, Waited, shown_ms};
use crate::sound::Beds;

/// Its own layer, over the screens'.
const LAYER: usize = 11;
/// The keys that skip a movie (and a mouse button's press).
const SKIP: [KeyCode; 3] = [KeyCode::Escape, KeyCode::Space, KeyCode::Enter];
const PAD_SKIP: [bevy::input::gamepad::GamepadButton; 3] = {
    use bevy::input::gamepad::GamepadButton::*;
    [South, East, Start]
};

/// Movies asked for, in order.
#[derive(Resource, Default)]
pub struct Movies(pub VecDeque<Queued>);

/// A movie asked for: its resref, whether a skip ends it, and the queue
/// run it plays in (TSL's `PlayMovieQueue`; 0 alone): a skip of a skippable
/// one ends the rest of its run too, as KOTOR's player drops the rest of
/// its list; an unskippable one plays on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Queued {
    pub name: String,
    pub skippable: bool,
    pub run: u16,
}

impl Queued {
    /// A movie alone, skippable: `PlayMovie`'s, a module's, the logos.
    pub fn alone(name: &str) -> Queued {
        Queued {
            name: name.to_string(),
            skippable: true,
            run: 0,
        }
    }

    /// The movie a script screen asks for, if it is one.
    pub fn of(screen: &sim::notice::Screen) -> Option<Queued> {
        match *screen {
            sim::notice::Screen::Movie { skippable, run, .. } => Some(Queued {
                name: screen.movie_name()?,
                skippable,
                run,
            }),
            _ => None,
        }
    }
}

/// Whether a movie is up (loading or playing).
#[derive(Resource, Default)]
pub struct Showing(pub bool);

/// K1's opening logos, before the main menu (xoreos's order for the PC).
const OPENING: [&str; 3] = ["leclogo", "biologo", "legal"];

/// Whether a run plays movies: `KJK_MOVIES=1` always, `0` never, else in a
/// window and not headless (a headless run is a check or a capture of the
/// game, which the Endar Spire's 2-minute crawl would take over).
pub fn played(headless: bool) -> bool {
    match std::env::var("KJK_MOVIES").as_deref() {
        Ok("1") => true,
        Ok("0") => false,
        _ => !headless,
    }
}

/// The movies' systems; `opening` plays the game's logos first (the
/// menu's start); `play` false skips every one (`played`).
pub struct MovieViewPlugin {
    pub opening: bool,
    pub play: bool,
}

impl Plugin for MovieViewPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Movies>()
            .init_resource::<Showing>()
            .insert_resource(Theatre {
                play: self.play,
                ..Theatre::default()
            })
            // After the screens, which a movie keeps from the keys that
            // skip it.
            .add_systems(Update, show.after(crate::ui_view::Stepped));
        if self.opening {
            app.world_mut()
                .resource_mut::<Movies>()
                .0
                .extend(OPENING.map(Queued::alone));
        }
    }
}

/// A movie's pictures: a Bink decoded as it plays, or a movie `ffmpeg` made.
enum Pictures {
    Bink(Player),
    Stored(Movie),
}

impl Pictures {
    fn size(&self) -> (u32, u32) {
        match self {
            Pictures::Bink(player) => (player.width, player.height),
            Pictures::Stored(movie) => (movie.width, movie.height),
        }
    }

    fn seconds(&self) -> f64 {
        match self {
            Pictures::Bink(player) => player.seconds(),
            Pictures::Stored(movie) => movie.seconds(),
        }
    }

    fn frames(&self) -> usize {
        match self {
            Pictures::Bink(player) => player.frames(),
            Pictures::Stored(movie) => movie.frames(),
        }
    }

    fn frame_at(&self, seconds: f64) -> usize {
        match self {
            Pictures::Bink(player) => player.frame_at(seconds),
            Pictures::Stored(movie) => movie.frame_at(seconds),
        }
    }

    /// The newest picture at or before frame `due` that is not shown yet, as
    /// RGBA, and its frame. `wait` waits for frame `due` itself (`Player`).
    fn picture(
        &mut self,
        due: usize,
        shown: Option<usize>,
        wait: bool,
    ) -> Result<Option<(usize, Vec<u8>)>, String> {
        match self {
            Pictures::Bink(player) => player.picture(due, wait),
            Pictures::Stored(_) if shown == Some(due) => Ok(None),
            Pictures::Stored(movie) => {
                let jpeg = movie.frame(due).ok_or(format!("no frame {due}"))?;
                movie::picture(jpeg).map(|(_, _, rgba)| Some((due, rgba)))
            }
        }
    }
}

/// A movie read, and its sound (streamed as it plays).
type Loaded = Result<(Pictures, Option<Stream>), String>;

/// Quads on the movies' layer, and what they are drawn with.
#[derive(Default)]
struct Quads {
    entities: Vec<Entity>,
    meshes: Vec<Handle<Mesh>>,
    materials: Vec<Handle<OdysseyMaterial>>,
}

/// The stage the movies play on: its camera and the black to the window's
/// edges, raised for the first movie and struck after the last one queued,
/// so nothing under it (the main menu) shows between one movie and the next.
struct Stage {
    camera: Entity,
    backdrop: Quads,
}

/// The movie up: its picture, its sound, and where its clock started.
struct Screening {
    name: String,
    /// Whether a skip ends it, and its queue run (`Queued`).
    skippable: bool,
    run: u16,
    pictures: Option<Pictures>,
    sound: Option<Stream>,
    /// Locked only to share the view.
    loading: Option<Mutex<Receiver<Loaded>>>,
    /// Not read: it ends at once.
    failed: bool,
    /// Its picture's quad (taken away at its end).
    quads: Quads,
    picture: Option<Handle<Image>>,
    /// The clock (ms) at its first frame; the frame shown.
    started: Option<f64>,
    shown: Option<usize>,
}

#[derive(Resource, Default)]
struct Theatre {
    play: bool,
    stage: Option<Stage>,
    screening: Option<Screening>,
    /// Real time (ms) since the first movie, for a window's clock.
    real_ms: f64,
    played: Vec<String>,
}

fn sound_name(movie: &str) -> String {
    format!("movie/{movie}")
}

/// Reads `name` and its sound, on a thread. A Bink 1 movie starts decoding
/// there. Any other is made with `ffmpeg` if the store has not got it.
fn load(name: String, game: Game) -> Receiver<Loaded> {
    let (told, read) = mpsc::channel();
    std::thread::spawn(move || {
        let loaded = (|| {
            let resolver = crate::viewer::resolver(&crate::discover(), game, None)?;
            let store = store::Store::open();
            Ok(
                match crate::movie_verbs::film(store.as_ref(), game, &resolver, &name)? {
                    Film::Bink(bink) => {
                        let sound = movie::player::sound(&bink)?
                            .map(|wav| Stream::file(&wav))
                            .transpose()?;
                        (Pictures::Bink(Player::start(bink)), sound)
                    }
                    Film::Stored(movie) => {
                        let sound = movie
                            .sound()
                            .map(|(_, bytes)| Stream::file(bytes))
                            .transpose()?;
                        (Pictures::Stored(movie), sound)
                    }
                },
            )
        })();
        let _ = told.send(loaded);
    });
    read
}

/// What tells the movie's time and the game it is of.
#[derive(SystemParam)]
struct Clocks<'w> {
    time: Res<'w, Time<Real>>,
    held: Option<Res<'w, console::AnimationTime>>,
    presented: Option<Res<'w, PresentedSnapshot>>,
    waited: Option<Res<'w, Waited>>,
    swap: Option<Res<'w, SceneSwap>>,
}

/// What the picture is drawn with, and where.
#[derive(SystemParam)]
struct Drawing<'w, 's> {
    meshes: ResMut<'w, Assets<Mesh>>,
    materials: ResMut<'w, Assets<OdysseyMaterial>>,
    images: ResMut<'w, Assets<Image>>,
    /// The stage's camera, whose view has the window's shape.
    cameras: Query<'w, 's, &'static Camera>,
    places: Query<'w, 's, &'static mut Transform>,
}

/// The picture's place: as large as the view allows, its shape kept,
/// centred. The view is 480 units high and `aspect` times that wide, so
/// on an ultrawide window a wide movie fills the width.
fn fitted((width, height): (u32, u32), aspect: f32) -> Transform {
    let (width, height) = (width as f32, height as f32);
    let scale = (480.0 * aspect / width).min(480.0 / height);
    Transform::from_xyz(0.0, 0.0, 1.0).with_scale(Vec3::new(width * scale, height * scale, 1.0))
}

#[allow(clippy::too_many_arguments)]
fn show(
    mut commands: Commands,
    mut movies: ResMut<Movies>,
    mut showing: ResMut<Showing>,
    mut theatre: ResMut<Theatre>,
    clocks: Clocks,
    (mut keys, mut buttons): (
        ResMut<ButtonInput<KeyCode>>,
        Option<ResMut<ButtonInput<MouseButton>>>,
    ),
    mut swallowed: ResMut<crate::swallow::Swallowed>,
    mut gamepads: Query<&mut bevy::input::gamepad::Gamepad>,
    drawing: Drawing,
    mut beds: ResMut<Beds>,
    target: Option<Res<console::CaptureTarget>>,
    mut facts: ResMut<console::DumpFacts>,
) {
    let Drawing {
        mut meshes,
        mut materials,
        mut images,
        cameras,
        mut places,
    } = drawing;
    let theatre = &mut *theatre;
    // The view's width over its height: the window's, or a recording's
    // frame's. 4:3 until the stage's camera has a view.
    let aspect = theatre
        .stage
        .as_ref()
        .and_then(|stage| cameras.get(stage.camera).ok())
        .and_then(Camera::logical_viewport_size)
        .filter(|size| size.y > 0.0)
        .map_or(4.0 / 3.0, |size| size.x / size.y);
    theatre.real_ms += clocks.time.delta_secs_f64() * 1000.0;
    let held = clocks.held.as_ref().is_some_and(|held| held.0.is_some());
    let clock = match &clocks.presented {
        Some(presented) if held => shown_ms(&presented.0, clocks.waited.as_deref()),
        _ => theatre.real_ms,
    };
    if !theatre.play {
        theatre
            .played
            .extend(movies.0.drain(..).map(|queued| queued.name));
    }
    if facts.wanted() {
        facts.0.insert(
            "movies".into(),
            serde_json::json!({ "played": theatre.played, "queued": movies.0.iter().map(|queued| &queued.name).collect::<Vec<_>>(), "play": theatre.play }),
        );
    }
    if theatre.screening.is_none()
        && let Some(Queued {
            name,
            skippable,
            run,
        }) = movies.0.pop_front()
    {
        let game = clocks
            .swap
            .as_ref()
            .and_then(|swap| swap.level.module().map(|module| module.game.clone()))
            .and_then(|game| Game::from_key(&game))
            .unwrap_or(Game::Kotor1);
        info!(target: "movie", %name, "plays");
        if theatre.stage.is_none() {
            let mut backdrop = Quads::default();
            // Black to the window's edges, whatever its shape.
            backdrop.add(
                &mut commands,
                (&mut meshes, &mut materials),
                unlit(Vec4::new(0.0, 0.0, 0.0, 1.0), None),
                Transform::from_xyz(0.0, 0.0, 0.0).with_scale(Vec3::new(8000.0, 960.0, 1.0)),
            );
            theatre.stage = Some(Stage {
                camera: stage(&mut commands, target.as_deref()),
                backdrop,
            });
        }
        theatre.screening = Some(Screening {
            loading: Some(Mutex::new(load(name.clone(), game))),
            name,
            skippable,
            run,
            pictures: None,
            sound: None,
            failed: false,
            quads: Quads::default(),
            picture: None,
            started: None,
            shown: None,
        });
    }
    showing.0 = theatre.screening.is_some();
    let Some(screening) = &mut theatre.screening else {
        return;
    };
    // Read: its picture's quad, and its sound.
    let received = screening.loading.as_ref().and_then(|loading| {
        match loading.lock().expect("the movie's channel").try_recv() {
            Ok(loaded) => Some(loaded),
            Err(TryRecvError::Empty) => None,
            Err(TryRecvError::Disconnected) => Some(Err("its reader stopped".into())),
        }
    });
    if let Some(loaded) = received {
        screening.loading = None;
        match loaded {
            Ok((pictures, sound)) => {
                let (width, height) = pictures.size();
                let image = images.add(Image::new_fill(
                    Extent3d {
                        width,
                        height,
                        depth_or_array_layers: 1,
                    },
                    TextureDimension::D2,
                    &[0, 0, 0, 255],
                    TextureFormat::Rgba8Unorm,
                    RenderAssetUsages::default(),
                ));
                screening.quads.add(
                    &mut commands,
                    (&mut meshes, &mut materials),
                    unlit(Vec4::ONE, Some(image.clone())),
                    fitted((width, height), aspect),
                );
                screening.picture = Some(image);
                screening.pictures = Some(pictures);
                screening.sound = sound;
            }
            Err(why) => {
                warn!(target: "movie", name = %screening.name, %why, "skipped");
                screening.failed = true;
            }
        }
    }
    // A press, not a release: the click that started it (a reply picked)
    // does not end it. What skips is taken, so nothing after this in the
    // frame (a conversation, the screens) sees it, nor play while it is
    // held (`swallow`).
    let mut skipped = false;
    for key in SKIP {
        if keys.clear_just_pressed(key) {
            skipped = true;
            swallowed.key(key, &keys);
        }
    }
    if let Some(buttons) = buttons.as_deref_mut() {
        skipped |= swallowed.pressed_buttons(buttons);
    }
    // A pad's A, B or Start; still held as the movie ends, `pad::sample`
    // takes it.
    for mut pad in &mut gamepads {
        for button in PAD_SKIP {
            skipped |= pad.digital_mut().clear_just_pressed(button);
        }
    }
    // An unskippable one plays on through a press; a skippable one's skip
    // drops the rest of its queue run with it.
    let skipped = skipped && screening.skippable;
    if skipped && screening.run != 0 {
        let run = screening.run;
        movies.0.retain(|queued| queued.run != run);
    }
    let mut over = skipped || screening.failed;
    if let Some(pictures) = &mut screening.pictures {
        // Fitted again each frame, so a resized window refits it.
        if let Some(mut place) = screening
            .quads
            .entities
            .first()
            .and_then(|&quad| places.get_mut(quad).ok())
        {
            *place = fitted(pictures.size(), aspect);
        }
        let started = *screening.started.get_or_insert(clock);
        let seconds = (clock - started) / 1000.0;
        over |= seconds >= pictures.seconds();
        let frame = pictures.frame_at(seconds);
        if !over {
            // A recording waits for each frame, so that its frames are the
            // same however fast the decoder is.
            match pictures.picture(frame, screening.shown, held) {
                Ok(Some((shown, pixels))) => {
                    screening.shown = Some(shown);
                    if let Some(mut image) =
                        screening.picture.as_ref().and_then(|h| images.get_mut(h))
                    {
                        image.data = Some(pixels);
                    }
                }
                Ok(None) => {}
                Err(why) => {
                    warn!(target: "movie", name = %screening.name, %why, "stops");
                    over = true;
                }
            }
        }
        if !over && let Some(sound) = &screening.sound {
            beds.0.push((
                sound_name(&screening.name),
                sound.clone(),
                Bed {
                    bus: audio::Bus::Movie,
                    gain: 1.0,
                    looping: false,
                    fade: 0.0,
                },
            ));
        }
        if facts.wanted() {
            facts.0.insert(
                "movie".into(),
                serde_json::json!({
                    "name": screening.name,
                    "size": pictures.size(),
                    "frames": pictures.frames(),
                    "frame": screening.shown,
                    "seconds": seconds,
                }),
            );
        }
    }
    if over {
        info!(target: "movie", name = %screening.name, skipped, "ends");
        screening
            .quads
            .remove(&mut commands, (&mut meshes, &mut materials));
        if let Some(image) = screening.picture.take() {
            images.remove(&image);
        }
        theatre.played.push(screening.name.clone());
        theatre.screening = None;
        showing.0 = !movies.0.is_empty();
        // The next movie goes up on the same black: the stage stays.
        if !showing.0
            && let Some(mut stage) = theatre.stage.take()
        {
            stage
                .backdrop
                .remove(&mut commands, (&mut meshes, &mut materials));
            commands.entity(stage.camera).despawn();
        }
        facts.0.remove("movie");
    }
}

impl Quads {
    /// A quad on the movies' layer.
    fn add(
        &mut self,
        commands: &mut Commands,
        (meshes, materials): (&mut Assets<Mesh>, &mut Assets<OdysseyMaterial>),
        material: OdysseyMaterial,
        place: Transform,
    ) {
        let mesh = meshes.add(Rectangle::new(1.0, 1.0));
        let material = materials.add(material);
        self.meshes.push(mesh.clone());
        self.materials.push(material.clone());
        self.entities.push(
            commands
                .spawn((
                    Mesh3d(mesh),
                    MeshMaterial3d(material),
                    place,
                    RenderLayers::layer(LAYER),
                ))
                .id(),
        );
    }

    /// Every quad despawned, and what it was drawn with dropped.
    fn remove(
        &mut self,
        commands: &mut Commands,
        (meshes, materials): (&mut Assets<Mesh>, &mut Assets<OdysseyMaterial>),
    ) {
        for entity in self.entities.drain(..) {
            commands.entity(entity).despawn();
        }
        for mesh in self.meshes.drain(..) {
            meshes.remove(&mesh);
        }
        for material in self.materials.drain(..) {
            materials.remove(&material);
        }
    }
}

/// The movie's camera, over every other, drawing where the run's frames go
/// (a headless run's image).
fn stage(commands: &mut Commands, target: Option<&console::CaptureTarget>) -> Entity {
    let target = match target {
        Some(console::CaptureTarget::Image(image)) => {
            Some(bevy::camera::RenderTarget::Image(image.clone().into()))
        }
        _ => None,
    };
    let mut camera = commands.spawn((
        Camera3d::default(),
        crate::scene::over_world(20),
        Projection::Orthographic(OrthographicProjection {
            scaling_mode: ScalingMode::FixedVertical {
                viewport_height: 480.0,
            },
            ..OrthographicProjection::default_3d()
        }),
        crate::scene::encoded_view(),
        Transform::from_xyz(0.0, 0.0, 100.0),
        RenderLayers::layer(LAYER),
        OverlayCamera,
    ));
    if let Some(target) = target {
        camera.insert(target);
    }
    camera.id()
}
