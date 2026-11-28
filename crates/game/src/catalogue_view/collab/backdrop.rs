//! The backdrop behind the screen, which a KOTOR panel's see-through parts
//! show. The third one is a still of the game world, made by a headless run
//! of kjk itself.

use std::collections::HashMap;
use std::path::Path;
use std::sync::{Mutex, mpsc};
use std::time::Instant;

use bevy::asset::RenderAssetUsages;
use bevy::camera::visibility::{NoFrustumCulling, RenderLayers};
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
use ui::draw::{DrawList, Quad};

use crate::ui_material::{UiMaterial, UiParams};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Backdrop {
    /// Black, as the catalogue always drew it.
    #[default]
    Plain,
    /// Diagonal stripes of two greys.
    Stripes,
    /// A frame of the game world, without the HUD.
    Play,
}

impl Backdrop {
    /// Every backdrop in the order the key cycles them.
    pub const ALL: [Backdrop; 3] = [Backdrop::Plain, Backdrop::Stripes, Backdrop::Play];

    pub fn name(self) -> &'static str {
        match self {
            Backdrop::Plain => "plain",
            Backdrop::Stripes => "stripes",
            Backdrop::Play => "play",
        }
    }

    pub fn named(name: &str) -> Option<Backdrop> {
        Backdrop::ALL
            .into_iter()
            .find(|backdrop| backdrop.name() == name)
    }

    pub fn next(self) -> Backdrop {
        let at = Backdrop::ALL
            .iter()
            .position(|each| *each == self)
            .unwrap_or(0);
        Backdrop::ALL[(at + 1) % Backdrop::ALL.len()]
    }

    /// What is drawn under the screens in a window of `window` logical
    /// pixels, if anything.
    pub fn list(self, window: (f32, f32)) -> Option<DrawList> {
        match self {
            Backdrop::Plain | Backdrop::Play => None,
            Backdrop::Stripes => Some(stripes(window)),
        }
    }
}

const DARK: [f32; 4] = [0.14, 0.14, 0.16, 1.0];
const LIGHT: [f32; 4] = [0.46, 0.46, 0.5, 1.0];

/// Stripes that run down to the right at 45 degrees, their width half their
/// spacing, 1/16 of the window's height apart.
fn stripes(window: (f32, f32)) -> DrawList {
    let spacing = (window.1 / 16.0).max(16.0);
    let reach = window.0 + window.1;
    let middle = [window.0 / 2.0, window.1 / 2.0];
    let across = std::f32::consts::FRAC_1_SQRT_2;
    let mut list = DrawList {
        size: window,
        ..DrawList::default()
    };
    list.quads
        .push(Quad::solid([0.0, 0.0, window.0, window.1], DARK));
    let mut offset = -reach / 2.0;
    while offset < reach / 2.0 {
        let centre = [middle[0] + offset * across, middle[1] - offset * across];
        let rect = [
            centre[0] - reach,
            centre[1] - spacing / 4.0,
            reach * 2.0,
            spacing / 2.0,
        ];
        list.quads.push(Quad {
            spin: std::f32::consts::FRAC_PI_4,
            ..Quad::solid(rect, LIGHT)
        });
        offset += spacing;
    }
    list
}

/// The stage a still of `game` is made on when the story's capture names
/// none: a place in the first area of each game that has light and floor.
fn default_stage(game: vfs::Game) -> &'static str {
    match game {
        vfs::Game::Kotor2 => "301nar",
        _ => "tar_m03ae",
    }
}

/// The stage a still behind a story is made on. The stage is the one
/// whose area the story's capture was taken in, read from the start of
/// the capture's name (`taris-arena`, `nar-shaddaa-target`), and the game's
/// default stage when the name is none of those.
pub fn stage_for(game: vfs::Game, capture: Option<&str>) -> &'static str {
    const AREAS: [(&str, &str); 6] = [
        ("endar-spire", "end_m01aa"),
        ("taris-apartment", "tar_m02af"),
        ("taris", "tar_m03ae"),
        ("nar-shaddaa", "301nar"),
        ("telos", "202tel"),
        ("floors-stage", "floors"),
    ];
    capture
        .and_then(|name| AREAS.iter().find(|(prefix, _)| name.starts_with(prefix)))
        .map_or_else(|| default_stage(game), |(_, stage)| stage)
}

/// A still as the screens' camera draws it: the picture's pixels, in the
/// device's size.
pub struct Picture {
    pub size: (u32, u32),
    pub rgba: Vec<u8>,
}

enum Job {
    Making(Mutex<mpsc::Receiver<Result<Picture, String>>>, Instant),
    Ready(Picture),
    Failed(String),
}

/// The stills of the game world, each made once and kept on disk. A still
/// is made by a thread that runs `kjk play` headless, so the catalogue
/// never waits for it.
#[derive(Default)]
pub struct Stills {
    jobs: HashMap<String, Job>,
}

/// How far a still is.
pub enum State<'a> {
    Making,
    Ready,
    Failed(&'a str),
}

impl Stills {
    pub fn ready(&self, key: &str) -> Option<&Picture> {
        match self.jobs.get(key)? {
            Job::Ready(picture) => Some(picture),
            _ => None,
        }
    }

    /// The still of `stage` at `pixels`, started when it is not yet, and
    /// the state it is in. `key` names the still on disk and in memory.
    pub fn of(&mut self, stage: &str, pixels: (u32, u32), cache: &Path) -> (String, State<'_>) {
        let key = format!("{stage}-{}x{}", pixels.0, pixels.1);
        if !self.jobs.contains_key(&key) {
            let (send, receive) = mpsc::channel();
            let (stage, file) = (stage.to_string(), cache.join(format!("{key}.png")));
            std::thread::spawn(move || {
                let _ = send.send(make(&stage, pixels, &file));
            });
            self.jobs.insert(
                key.clone(),
                Job::Making(Mutex::new(receive), Instant::now()),
            );
        }
        if let Some(Job::Making(receive, started)) = self.jobs.get(&key)
            && let Ok(done) = receive
                .lock()
                .map_or(Err(mpsc::TryRecvError::Empty), |receive| receive.try_recv())
        {
            let seconds = started.elapsed().as_secs_f32();
            let job = match done {
                Ok(picture) => {
                    info!(target: "catalogue", still = key, seconds, "backdrop still ready");
                    Job::Ready(picture)
                }
                Err(why) => {
                    warn!(target: "catalogue", still = key, "no backdrop still: {why}");
                    Job::Failed(why)
                }
            };
            self.jobs.insert(key.clone(), job);
        }
        let state = match &self.jobs[&key] {
            Job::Making(..) => State::Making,
            Job::Ready(_) => State::Ready,
            Job::Failed(why) => State::Failed(why),
        };
        (key, state)
    }
}

/// The picture of `file`, made first by a run of this program when the file
/// is not there.
fn make(stage: &str, pixels: (u32, u32), file: &Path) -> Result<Picture, String> {
    if !file.exists() {
        render(stage, pixels, file)?;
    }
    let decoded = image::open(file)
        .inspect_err(|_| {
            let _ = std::fs::remove_file(file);
        })
        .map_err(|error| format!("{}: {error}", file.display()))?
        .to_rgba8();
    Ok(Picture {
        size: decoded.dimensions(),
        rgba: decoded.into_raw(),
    })
}

/// A headless `kjk play` on `stage`, its HUD hidden, whose picture of the
/// world is moved to `file`. The run's own folder is made apart and taken
/// away when the picture is out of it.
fn render(stage: &str, pixels: (u32, u32), file: &Path) -> Result<(), String> {
    let why = |path: &Path, error: std::io::Error| format!("{}: {error}", path.display());
    let folder = file.parent().ok_or("the cache has no folder")?;
    std::fs::create_dir_all(folder).map_err(|error| why(folder, error))?;
    let folder = std::fs::canonicalize(folder).map_err(|error| why(folder, error))?;
    let work = folder.join(format!("making-{stage}-{}", std::process::id()));
    std::fs::create_dir_all(&work).map_err(|error| why(&work, error))?;
    let program = std::env::current_exe().map_err(|error| error.to_string())?;
    let status = std::process::Command::new(program)
        .current_dir(&work)
        .args([
            "play",
            "--stage",
            stage,
            "--headless",
            "--mute",
            "--no-splash",
        ])
        .arg("--size")
        .arg(format!("{}x{}", pixels.0, pixels.1))
        .args([
            "--cmds",
            "wait world; ui hotkey hide_interface; wait 240f; screenshot still; quit",
        ])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map_err(|error| format!("kjk play: {error}"))?;
    let made = std::fs::read_dir(work.join("kjk-artifacts").join("runs"))
        .into_iter()
        .flatten()
        .flatten()
        .map(|run| run.path().join("still.png"))
        .find(|picture| picture.exists());
    let result = match made {
        Some(picture) if status.success() => {
            let part = file.with_extension("png.part");
            std::fs::copy(&picture, &part)
                .and_then(|_| std::fs::rename(&part, file))
                .map_err(|error| why(file, error))
        }
        _ => {
            return Err(format!(
                "kjk play on {stage} ended with {status}, its folder is {}",
                work.display()
            ));
        }
    };
    let _ = std::fs::remove_dir_all(&work);
    result
}

/// The quad that shows a still under the screens, and the still it shows.
#[derive(Default)]
pub struct Shown {
    key: String,
    window: Vec2,
    entity: Option<Entity>,
}

/// `want`, the still behind the screens, shown by a quad of `window` logical
/// pixels at the back of the screens' layer, or no quad when there is none.
/// Whether the quad came or went or changed.
pub fn show(
    shown: &mut Shown,
    want: Option<(&str, &Picture)>,
    (window, layer): (Vec2, usize),
    (commands, meshes, materials, images): (
        &mut Commands,
        &mut Assets<Mesh>,
        &mut Assets<UiMaterial>,
        &mut Assets<Image>,
    ),
) -> bool {
    let Some((key, picture)) = want else {
        let had = shown.entity.take();
        if let Some(entity) = had {
            commands.entity(entity).despawn();
            shown.key.clear();
        }
        return had.is_some();
    };
    if shown.entity.is_some() && shown.key == key && shown.window == window {
        return false;
    }
    if let Some(entity) = shown.entity.take() {
        commands.entity(entity).despawn();
    }
    let (width, height) = picture.size;
    let image = images.add(Image::new(
        Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        picture.rgba.clone(),
        TextureFormat::Rgba8Unorm,
        RenderAssetUsages::RENDER_WORLD,
    ));
    let entity = commands
        .spawn((
            Mesh3d(meshes.add(Rectangle::new(1.0, 1.0))),
            MeshMaterial3d(materials.add(UiMaterial {
                params: UiParams {
                    size: Vec2::new(width as f32, height as f32),
                    shape: 0,
                    view: 1,
                    grey: 0,
                },
                texture: Some(image),
                additive: false,
            })),
            Transform {
                translation: Vec3::new(0.0, 0.0, -1.0),
                scale: Vec3::new(window.x, window.y, 1.0),
                ..Transform::IDENTITY
            },
            RenderLayers::layer(layer),
            NoFrustumCulling,
        ))
        .id();
    shown.key = key.to_string();
    shown.window = window;
    shown.entity = Some(entity);
    true
}
