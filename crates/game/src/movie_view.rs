//! Movies (I20, `docs/SOUND.md`, "Movies"): what scripts play
//! (`PlayMovie`, sent as a screen, `script_screens`) or the console asks
//! for (`ui script movie <name>`), one after another, over everything:
//!
//! - black to the window's edges, from the first movie asked for to the
//!   end of the last one queued (nothing under it shows between them), the
//!   picture fitted within 640 × 480 keeping its shape, its sound unplaced
//!   at full gain;
//! - read from the store, made there by `ffmpeg` the first time
//!   (`movie_verbs`), on a thread of its own; black meanwhile;
//! - Escape, Space, Enter or a click skips it, unless TSL's `QueueMovie`
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
//! Without `ffmpeg` (or the movie) it is skipped, with a warning. A
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
use vfs::Game;

use crate::lifecycle_view::{OverlayCamera, gui_quad, unlit};
use crate::material::OdysseyMaterial;
use crate::sim_host::{PresentedSnapshot, SceneSwap, Waited, shown_ms};
use crate::sound::Beds;

/// Its own layer, over the screens'.
const LAYER: usize = 11;
/// The keys that skip a movie (and a mouse button's press).
const SKIP: [KeyCode; 3] = [KeyCode::Escape, KeyCode::Space, KeyCode::Enter];

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

/// A movie read, and its sound (streamed as it plays).
type Loaded = Result<(Movie, Option<Stream>), String>;

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
    movie: Option<Movie>,
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

/// Reads `name` (making it with `ffmpeg` if the store has not got it) and
/// its sound, on a thread.
fn load(name: String, game: Game) -> Receiver<Loaded> {
    let (told, read) = mpsc::channel();
    std::thread::spawn(move || {
        let loaded = (|| {
            let resolver = crate::viewer::resolver(&crate::discover(), game, None)?;
            let store = store::Store::open();
            let (movie, _) = crate::movie_verbs::movie(store.as_ref(), game, &resolver, &name)?;
            let sound = movie
                .sound()
                .map(|(_, bytes)| Stream::file(bytes))
                .transpose()?;
            Ok((movie, sound))
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

/// What the picture is drawn with.
#[derive(SystemParam)]
struct Drawing<'w> {
    meshes: ResMut<'w, Assets<Mesh>>,
    materials: ResMut<'w, Assets<OdysseyMaterial>>,
    images: ResMut<'w, Assets<Image>>,
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
    drawing: Drawing,
    mut beds: ResMut<Beds>,
    target: Option<Res<console::CaptureTarget>>,
    mut facts: ResMut<console::DumpFacts>,
) {
    let Drawing {
        mut meshes,
        mut materials,
        mut images,
    } = drawing;
    let theatre = &mut *theatre;
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
    facts.0.insert(
        "movies".into(),
        serde_json::json!({ "played": theatre.played, "queued": movies.0.iter().map(|queued| &queued.name).collect::<Vec<_>>(), "play": theatre.play }),
    );
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
            movie: None,
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
            Ok((movie, sound)) => {
                let image = images.add(Image::new_fill(
                    Extent3d {
                        width: movie.width,
                        height: movie.height,
                        depth_or_array_layers: 1,
                    },
                    TextureDimension::D2,
                    &[0, 0, 0, 255],
                    TextureFormat::Rgba8Unorm,
                    RenderAssetUsages::default(),
                ));
                // Fitted within 640 × 480, its shape kept, centred.
                let scale = (640.0 / movie.width as f32).min(480.0 / movie.height as f32);
                let (width, height) = (movie.width as f32 * scale, movie.height as f32 * scale);
                let place = gui_quad(
                    [(640.0 - width) / 2.0, (480.0 - height) / 2.0, width, height],
                    1.0,
                );
                screening.quads.add(
                    &mut commands,
                    (&mut meshes, &mut materials),
                    unlit(Vec4::ONE, Some(image.clone())),
                    place,
                );
                screening.picture = Some(image);
                screening.movie = Some(movie);
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
    // An unskippable one plays on through a press; a skippable one's skip
    // drops the rest of its queue run with it.
    let skipped = skipped && screening.skippable;
    if skipped && screening.run != 0 {
        let run = screening.run;
        movies.0.retain(|queued| queued.run != run);
    }
    let mut over = skipped || screening.failed;
    if let Some(movie) = &screening.movie {
        let started = *screening.started.get_or_insert(clock);
        let seconds = (clock - started) / 1000.0;
        over |= seconds >= movie.seconds();
        let frame = movie.frame_at(seconds);
        if !over && screening.shown != Some(frame) {
            screening.shown = Some(frame);
            let picture = movie.frame(frame).map(movie::picture);
            if let (Some(Ok((_, _, pixels))), Some(mut image)) = (
                picture,
                screening.picture.as_ref().and_then(|h| images.get_mut(h)),
            ) {
                image.data = Some(pixels);
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
        facts.0.insert(
            "movie".into(),
            serde_json::json!({
                "name": screening.name,
                "size": [movie.width, movie.height],
                "frames": movie.frames(),
                "frame": screening.shown,
                "seconds": seconds,
            }),
        );
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
