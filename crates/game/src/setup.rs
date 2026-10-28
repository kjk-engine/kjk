//! The first-run setup: before the main menu, the three games kjk
//! plays from found or chosen (KOTOR for the campaign, Jedi Academy for
//! movement, sabers and the Force, KOTOR II for the screens' look), then
//! KOTOR's shared data imported into the store (`import_verbs::prepare`).
//! It draws with Bevy's UI and its own font: no game's may be found yet.
//!
//! A folder is chosen with the platform's dialog, or typed or pasted; the
//! choice goes in the settings (`install.k1`, …), which `discover` reads
//! after the environment's variables. Mouse, keys and gamepads all drive it.
//! When it is done (`setup.done`), the program starts again on its main
//! menu (`restart`).

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Arc, Mutex, PoisonError};

use bevy::camera::CompositingSpace;
use bevy::ecs::system::NonSendMarker;
use bevy::input::gamepad::{Gamepad, GamepadButton};
use bevy::input_focus::AutoFocus;
use bevy::prelude::*;
use bevy::text::{EditableText, TextCursorStyle};
use vfs::{Discovery, Game, Source};

use crate::settings::{Settings, install_key};

/// The setting saying the setup was done.
const DONE: &str = "setup.done";

/// The games as the setup lists them: their names and what each brings.
const GAMES: [(Game, &str, &str); 3] = [
    (
        Game::Kotor1,
        "Star Wars: Knights of the Old Republic",
        "The campaign: its worlds, story and dialogue.",
    ),
    (
        Game::JediAcademy,
        "Star Wars Jedi Knight: Jedi Academy",
        "Movement, lightsabers and the Force.",
    ),
    (
        Game::Kotor2,
        "Star Wars: Knights of the Old Republic II",
        "The look of the menus.",
    ),
];

const BACKGROUND: Color = Color::srgb(0.035, 0.045, 0.06);
const PANEL: Color = Color::srgb(0.07, 0.085, 0.11);
const EDGE: Color = Color::srgb(0.16, 0.19, 0.24);
const ACCENT: Color = Color::srgb(0.36, 0.66, 0.92);
const TEXT: Color = Color::srgb(0.9, 0.92, 0.95);
const DIM: Color = Color::srgb(0.55, 0.6, 0.67);
const FOUND: Color = Color::srgb(0.45, 0.82, 0.52);
const MISSING: Color = Color::srgb(0.95, 0.68, 0.3);
const REFUSED: Color = Color::srgb(0.95, 0.45, 0.42);

/// How long "Ready" shows before the program starts again.
const READY_SECONDS: f32 = 1.0;

/// This run's setup finished: the program starts again (`restart`).
static FINISHED: AtomicBool = AtomicBool::new(false);

/// Whether the setup comes before the menu: a game is not found, or the
/// setup was never done.
pub fn needed(discovery: &Discovery) -> bool {
    Settings::load().value(DONE).is_none()
        || Game::ALL.iter().any(|game| discovery.get(*game).is_none())
}

/// Whether this run's setup finished.
pub fn finished() -> bool {
    FINISHED.load(Ordering::Acquire)
}

/// This program again with no arguments (its main menu), in place of this
/// process (on Windows, beside it as this one ends).
pub fn restart() -> std::io::Error {
    let program = match std::env::current_exe() {
        Ok(program) => program,
        Err(error) => return error,
    };
    let mut command = std::process::Command::new(program);
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt as _;
        command.exec()
    }
    #[cfg(not(unix))]
    {
        match command.spawn() {
            Ok(_) => std::process::exit(0),
            Err(error) => error,
        }
    }
}

/// The setup's screen. `start`: its import starts at once when every game
/// is found (`kjk setup --start`, for scripted runs).
#[derive(Default)]
pub struct SetupPlugin {
    pub start: bool,
}

impl Plugin for SetupPlugin {
    fn build(&self, app: &mut App) {
        let mut setup = Setup::new(crate::discover());
        if self.start {
            setup.pending.push(Act::Start);
        }
        app.insert_resource(ClearColor(BACKGROUND))
            .insert_resource(setup)
            .add_systems(
                Update,
                (pointer, navigate, act, follow, build, paint).chain(),
            );
    }
}

/// What a button does.
#[derive(Component, Clone, Copy, PartialEq, Eq, Debug)]
enum Act {
    /// The platform's folder dialog, for the game.
    Locate(Game),
    /// The folder typed or pasted.
    Type(Game),
    /// The typed folder taken, or left.
    Accept,
    Cancel,
    /// The import, then the menu.
    Start,
    Quit,
}

enum Phase {
    Choosing,
    Importing,
    Failed(String),
    /// Imported: seconds left before the program starts again.
    Ready(f32),
}

/// What the import's thread sends: a step begun, a step's detail, the end.
enum Step {
    Begin(String),
    Detail(String),
    Done(Result<(), String>),
}

#[derive(Resource)]
struct Setup {
    discovery: Discovery,
    phase: Phase,
    /// The game whose folder is being typed.
    editing: Option<Game>,
    /// The import's steps so far, and whether each is a detail.
    lines: Vec<(String, bool)>,
    job: Option<Mutex<Receiver<Step>>>,
    /// The focused button's place in `acts`.
    focus: usize,
    /// The screen is built again this frame.
    dirty: bool,
    /// Buttons pressed, to act on this frame.
    pending: Vec<Act>,
}

impl Setup {
    fn new(discovery: Discovery) -> Setup {
        Setup {
            discovery,
            phase: Phase::Choosing,
            editing: None,
            lines: Vec::new(),
            job: None,
            focus: 0,
            dirty: true,
            pending: Vec::new(),
        }
    }

    fn all_found(&self) -> bool {
        Game::ALL
            .iter()
            .all(|game| self.discovery.get(*game).is_some())
    }

    /// The buttons on the screen, in the order focus moves through them.
    fn acts(&self) -> Vec<Act> {
        if self.editing.is_some() {
            return vec![Act::Accept, Act::Cancel];
        }
        match self.phase {
            Phase::Choosing | Phase::Failed(_) => {
                let mut acts: Vec<Act> = GAMES
                    .iter()
                    .flat_map(|(game, _, _)| [Act::Locate(*game), Act::Type(*game)])
                    .collect();
                if self.all_found() {
                    acts.push(Act::Start);
                }
                acts.push(Act::Quit);
                acts
            }
            Phase::Importing | Phase::Ready(_) => Vec::new(),
        }
    }

    /// `path` taken as the player's folder for `game`, and the games found
    /// again.
    fn choose(&mut self, game: Game, path: &Path) {
        let mut settings = Settings::load();
        settings.apply(&install_key(game), &path.to_string_lossy());
        self.discovery = crate::discover();
        info!(target: "setup", game = game.key(), path = %path.display(), found = self.discovery.get(game).is_some(), "chosen");
        self.dirty = true;
    }

    fn chosen(game: Game) -> Option<String> {
        Settings::load()
            .value(&install_key(game))
            .map(str::to_string)
    }
}

/// The screen's root.
#[derive(Component)]
struct Root;

/// The folder being typed.
#[derive(Component)]
struct PathField;

fn text(value: impl Into<String>, size: f32, color: Color) -> impl Bundle {
    (
        Text::new(value),
        TextFont::from_font_size(size),
        TextColor(color),
    )
}

fn button(act: Act, label: &str) -> impl Bundle {
    (
        Button,
        act,
        Node {
            padding: UiRect::axes(px(18), px(9)),
            border: UiRect::all(px(2)),
            border_radius: BorderRadius::all(px(6)),
            justify_content: JustifyContent::Center,
            align_items: AlignItems::Center,
            ..default()
        },
        BackgroundColor(PANEL),
        BorderColor::all(EDGE),
        children![text(label, 17.0, TEXT)],
    )
}

/// What the screen says of `game`'s install: where it is and how it was
/// found, or why not.
fn status(discovery: &Discovery, game: Game) -> (String, Color) {
    if let Some(install) = discovery.get(game) {
        let how = match &install.source {
            Source::SteamLibrary(_) => "found in Steam".to_string(),
            Source::GogDefault => "found in GOG".to_string(),
            Source::Env(var) => {
                let chosen =
                    Setup::chosen(game).is_some_and(|chosen| Path::new(&chosen) == install.root);
                if chosen {
                    "chosen".to_string()
                } else {
                    format!("set by {var}")
                }
            }
        };
        return (format!("{}  ({how})", install.root.display()), FOUND);
    }
    match discovery
        .refusals
        .iter()
        .rev()
        .find(|refusal| refusal.game == game)
    {
        Some(refusal) => (
            format!("{}: {}", refusal.path.display(), refusal.reason),
            REFUSED,
        ),
        None => (
            "Not found. Locate the folder it is installed in.".to_string(),
            MISSING,
        ),
    }
}

/// The screen built again from the state, when it changed.
fn build(
    mut commands: Commands,
    mut setup: ResMut<Setup>,
    roots: Query<Entity, With<Root>>,
    cameras: Query<(Entity, Has<CompositingSpace>), With<Camera2d>>,
) {
    // The scene's camera (`scene::spawn_camera`), named: headless it draws
    // into an image, which no camera is the UI's by default. It blends in
    // gamma space for KOTOR's screens; Bevy's UI wants its colours linear.
    let Some((camera, gamma)) = cameras.iter().next() else {
        return;
    };
    if gamma {
        commands.entity(camera).remove::<CompositingSpace>();
    }
    if !setup.dirty {
        return;
    }
    setup.dirty = false;
    let count = setup.acts().len();
    setup.focus = setup.focus.min(count.saturating_sub(1));
    for root in &roots {
        commands.entity(root).despawn();
    }
    let setup = &*setup;
    commands
        .spawn((
            Root,
            UiTargetCamera(camera),
            Node {
                width: percent(100),
                height: percent(100),
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                ..default()
            },
        ))
        .with_children(|screen| {
            screen
                .spawn(Node {
                    width: percent(100),
                    max_width: px(960),
                    flex_direction: FlexDirection::Column,
                    padding: UiRect::all(px(32)),
                    row_gap: px(14),
                    ..default()
                })
                .with_children(|page| {
                    page.spawn(text("kjk", 40.0, TEXT));
                    page.spawn(text(
                        "kjk plays from your own copies of three games. It only reads \
                         them: nothing in their folders is changed.",
                        17.0,
                        DIM,
                    ));
                    for (game, name, role) in GAMES {
                        game_row(page, setup, game, name, role);
                    }
                    footer(page, setup);
                });
        });
}

fn game_row(page: &mut ChildSpawnerCommands, setup: &Setup, game: Game, name: &str, role: &str) {
    let (status, colour) = status(&setup.discovery, game);
    let choosing = matches!(setup.phase, Phase::Choosing | Phase::Failed(_));
    page.spawn((
        Node {
            flex_direction: FlexDirection::Column,
            padding: UiRect::all(px(16)),
            row_gap: px(6),
            border: UiRect::all(px(1)),
            border_radius: BorderRadius::all(px(8)),
            ..default()
        },
        BackgroundColor(PANEL),
        BorderColor::all(EDGE),
    ))
    .with_children(|card| {
        card.spawn(Node {
            justify_content: JustifyContent::SpaceBetween,
            align_items: AlignItems::Center,
            column_gap: px(12),
            ..default()
        })
        .with_children(|top| {
            top.spawn(Node {
                flex_direction: FlexDirection::Column,
                row_gap: px(3),
                flex_shrink: 1.0,
                ..default()
            })
            .with_children(|words| {
                words.spawn(text(name, 21.0, TEXT));
                words.spawn(text(role, 15.0, DIM));
            });
            if choosing && setup.editing.is_none() {
                top.spawn(Node {
                    column_gap: px(8),
                    flex_shrink: 0.0,
                    ..default()
                })
                .with_children(|buttons| {
                    buttons.spawn(button(Act::Locate(game), "Locate..."));
                    buttons.spawn(button(Act::Type(game), "Type path"));
                });
            }
        });
        card.spawn(text(status, 15.0, colour));
        if setup.editing == Some(game) {
            let current = setup
                .discovery
                .get(game)
                .map(|install| install.root.display().to_string())
                .or_else(|| Setup::chosen(game))
                .unwrap_or_else(String::new);
            card.spawn((
                PathField,
                Node {
                    width: percent(100),
                    padding: UiRect::all(px(8)),
                    border: UiRect::all(px(2)),
                    border_radius: BorderRadius::all(px(4)),
                    ..default()
                },
                EditableText::new(current),
                TextLayout::no_wrap(),
                TextFont::from_font_size(17.0),
                TextColor(TEXT),
                TextCursorStyle::default(),
                BackgroundColor(BACKGROUND),
                BorderColor::all(ACCENT),
                AutoFocus,
            ));
            card.spawn(Node {
                column_gap: px(8),
                align_items: AlignItems::Center,
                ..default()
            })
            .with_children(|buttons| {
                buttons.spawn(button(Act::Accept, "Use this folder"));
                buttons.spawn(button(Act::Cancel, "Cancel"));
                buttons.spawn(text("Enter to use it, Escape to cancel", 14.0, DIM));
            });
        }
    });
}

fn footer(page: &mut ChildSpawnerCommands, setup: &Setup) {
    let store = store::Store::open()
        .map(|store| store.root().display().to_string())
        .unwrap_or_else(|| "no store folder (set KJK_STORE)".to_string());
    match &setup.phase {
        Phase::Choosing | Phase::Failed(_) => {
            if let Phase::Failed(why) = &setup.phase {
                page.spawn(text(format!("The import stopped: {why}"), 16.0, REFUSED));
            }
            let note = if setup.all_found() {
                format!(
                    "Every game is found. Next, KOTOR's shared data is read once into {store} \
                     (a few seconds; later starts skip it)."
                )
            } else {
                "Locate each game that was not found. Steam and GOG installs are found by \
                 themselves."
                    .to_string()
            };
            page.spawn(text(note, 15.0, DIM));
            page.spawn(Node {
                column_gap: px(10),
                margin: UiRect::top(px(6)),
                ..default()
            })
            .with_children(|buttons| {
                if setup.editing.is_none() {
                    if setup.all_found() {
                        let label = if matches!(setup.phase, Phase::Failed(_)) {
                            "Try again"
                        } else {
                            "Import and play"
                        };
                        buttons.spawn(button(Act::Start, label));
                    }
                    buttons.spawn(button(Act::Quit, "Quit"));
                }
            });
            page.spawn(text(
                "Mouse, keyboard (arrows, Tab, Enter) or gamepad (D-pad, A).",
                13.0,
                DIM,
            ));
        }
        Phase::Importing | Phase::Ready(_) => {
            page.spawn(Node {
                flex_direction: FlexDirection::Column,
                row_gap: px(4),
                margin: UiRect::top(px(6)),
                ..default()
            })
            .with_children(|log| {
                for (line, detail) in &setup.lines {
                    if *detail {
                        log.spawn(text(format!("    {line}"), 13.0, DIM));
                    } else {
                        log.spawn(text(line.clone(), 16.0, TEXT));
                    }
                }
            });
            let (line, colour) = match setup.phase {
                Phase::Ready(_) => ("Ready. Starting the game...", FOUND),
                _ => ("Importing...", ACCENT),
            };
            page.spawn(text(line, 18.0, colour));
        }
    }
}

/// Mouse: a button hovered takes the focus; pressed, it acts.
fn pointer(mut setup: ResMut<Setup>, buttons: Query<(&Interaction, &Act), Changed<Interaction>>) {
    for (interaction, act) in &buttons {
        match interaction {
            Interaction::Pressed => setup.pending.push(*act),
            Interaction::Hovered => {
                if let Some(at) = setup.acts().iter().position(|known| known == act) {
                    setup.focus = at;
                }
            }
            Interaction::None => {}
        }
    }
}

/// Keys and gamepads: focus moves through the buttons and the focused one
/// acts; while a folder is typed, Enter takes it and Escape leaves it.
fn navigate(
    mut setup: ResMut<Setup>,
    keys: Res<ButtonInput<KeyCode>>,
    gamepads: Query<&Gamepad>,
    mut stick_was: Local<f32>,
) {
    if setup.editing.is_some() {
        let pad = |button| gamepads.iter().any(|pad| pad.just_pressed(button));
        if keys.just_pressed(KeyCode::Enter) || keys.just_pressed(KeyCode::NumpadEnter) {
            setup.pending.push(Act::Accept);
        } else if keys.just_pressed(KeyCode::Escape) || pad(GamepadButton::East) {
            setup.pending.push(Act::Cancel);
        } else if pad(GamepadButton::South) {
            setup.pending.push(Act::Accept);
        }
        return;
    }
    let acts = setup.acts();
    if acts.is_empty() {
        return;
    }
    let shift = keys.pressed(KeyCode::ShiftLeft) || keys.pressed(KeyCode::ShiftRight);
    let mut step: i32 = 0;
    let mut press = false;
    if keys.just_pressed(KeyCode::ArrowUp)
        || keys.just_pressed(KeyCode::ArrowLeft)
        || (shift && keys.just_pressed(KeyCode::Tab))
    {
        step = -1;
    } else if keys.just_pressed(KeyCode::ArrowDown)
        || keys.just_pressed(KeyCode::ArrowRight)
        || keys.just_pressed(KeyCode::Tab)
    {
        step = 1;
    }
    if keys.just_pressed(KeyCode::Enter)
        || keys.just_pressed(KeyCode::NumpadEnter)
        || keys.just_pressed(KeyCode::Space)
    {
        press = true;
    }
    let mut stick = 0.0f32;
    for pad in &gamepads {
        if pad.just_pressed(GamepadButton::DPadUp) || pad.just_pressed(GamepadButton::DPadLeft) {
            step = -1;
        }
        if pad.just_pressed(GamepadButton::DPadDown) || pad.just_pressed(GamepadButton::DPadRight) {
            step = 1;
        }
        if pad.just_pressed(GamepadButton::South) {
            press = true;
        }
        if stick.abs() < pad.left_stick().y.abs() {
            stick = pad.left_stick().y;
        }
    }
    // The stick moves the focus once each time it is pushed past half.
    let was = *stick_was;
    *stick_was = stick;
    if stick > 0.5 && was <= 0.5 {
        step = -1;
    } else if stick < -0.5 && was >= -0.5 {
        step = 1;
    }
    let count = acts.len() as i32;
    setup.focus = (setup.focus as i32 + step).rem_euclid(count) as usize;
    if press {
        let focused = acts[setup.focus];
        setup.pending.push(focused);
    }
}

/// The buttons pressed this frame acted on. On the main thread: the
/// platform's folder dialog wants it (macOS).
fn act(
    mut setup: ResMut<Setup>,
    fields: Query<&EditableText, With<PathField>>,
    headless: Res<crate::scene::Headless>,
    mut exit: MessageWriter<AppExit>,
    _main_thread: NonSendMarker,
) {
    let pending = std::mem::take(&mut setup.pending);
    for act in pending {
        info!(target: "setup", ?act, "pressed");
        match act {
            Act::Locate(game) => {
                if headless.0 {
                    continue;
                }
                let name = GAMES
                    .iter()
                    .find(|(known, _, _)| *known == game)
                    .map_or("the game", |(_, name, _)| *name);
                let mut dialog =
                    rfd::FileDialog::new().set_title(format!("The folder {name} is installed in"));
                if let Some(start) = start_folder(&setup.discovery, game) {
                    dialog = dialog.set_directory(start);
                }
                if let Some(folder) = dialog.pick_folder() {
                    setup.choose(game, &folder);
                }
            }
            Act::Type(game) => {
                setup.editing = Some(game);
                setup.focus = 0;
                setup.dirty = true;
            }
            Act::Accept => {
                let typed = fields.iter().next().map(|field| field.value().to_string());
                if let (Some(game), Some(typed)) = (setup.editing, typed) {
                    let typed = typed.trim().trim_matches('"');
                    if !typed.is_empty() {
                        setup.choose(game, Path::new(typed));
                    }
                }
                setup.editing = None;
                setup.dirty = true;
            }
            Act::Cancel => {
                setup.editing = None;
                setup.dirty = true;
            }
            Act::Start => {
                if !setup.all_found() || setup.job.is_some() {
                    continue;
                }
                let (sender, receiver) = channel();
                std::thread::spawn(move || {
                    let result = import(&sender);
                    let _ = sender.send(Step::Done(result));
                });
                setup.job = Some(Mutex::new(receiver));
                setup.lines.clear();
                setup.phase = Phase::Importing;
                setup.dirty = true;
            }
            Act::Quit => {
                exit.write(AppExit::Success);
            }
        }
    }
}

/// Where the folder dialog opens for `game`: its folder, else the folder of
/// another game found (the same store's library, likely), else none.
fn start_folder(discovery: &Discovery, game: Game) -> Option<PathBuf> {
    if let Some(install) = discovery.get(game) {
        return Some(install.root.clone());
    }
    discovery
        .installs
        .first()
        .and_then(|install| install.root.parent().map(Path::to_path_buf))
}

/// The import's steps as they come; at its end, Ready (the setup done) or
/// why not; Ready's time up, the program starts again.
fn follow(mut setup: ResMut<Setup>, time: Res<Time>, mut exit: MessageWriter<AppExit>) {
    if let Phase::Ready(left) = &mut setup.phase {
        *left -= time.delta_secs();
        if *left <= 0.0 {
            FINISHED.store(true, Ordering::Release);
            exit.write(AppExit::Success);
        }
        return;
    }
    let Some(job) = &setup.job else {
        return;
    };
    let steps: Vec<Step> = job
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .try_iter()
        .collect();
    for step in steps {
        setup.dirty = true;
        match step {
            Step::Begin(line) => setup.lines.push((line, false)),
            Step::Detail(line) => setup.lines.push((line, true)),
            Step::Done(Ok(())) => {
                setup.job = None;
                Settings::load().apply(DONE, "1");
                info!(target: "setup", "done");
                setup.phase = Phase::Ready(READY_SECONDS);
            }
            Step::Done(Err(why)) => {
                setup.job = None;
                warn!(target: "setup", %why, "the import stopped");
                setup.discovery = crate::discover();
                setup.phase = Phase::Failed(why);
            }
        }
    }
}

/// The import, on its thread: the other two games opened (a broken install
/// is told now, not in the menu), then KOTOR's shared data made into the
/// store.
fn import(sender: &Sender<Step>) -> Result<(), String> {
    let begin = |line: &str| {
        let _ = sender.send(Step::Begin(line.to_string()));
    };
    let detail = |line: String| {
        let _ = sender.send(Step::Detail(line));
    };
    let discovery = crate::discover();
    begin("Reading Jedi Academy's files");
    let ja = crate::installs::open(&discovery, Game::JediAcademy)?;
    detail(format!("{} files", ja.index.len()));
    begin("Reading KOTOR II's files");
    let k2 = crate::installs::open(&discovery, Game::Kotor2)?;
    k2.base()?;
    detail(format!("{} files", k2.index.len()));
    begin("Reading KOTOR's files and its templates and voices (the first time takes longest)");
    let anims = Arc::new(sim::motion::AnimTable::stand_in());
    let installer = crate::lifecycle::Installer::without_import(&discovery, Game::Kotor1, anims)?;
    crate::import_verbs::prepare(&installer, false, &mut |line| detail(line))?;
    Ok(())
}

/// The buttons painted for the focus and the pointer.
fn paint(
    setup: Res<Setup>,
    mut buttons: Query<(&Act, &Interaction, &mut BackgroundColor, &mut BorderColor)>,
) {
    let acts = setup.acts();
    let focused = acts.get(setup.focus).copied();
    for (act, interaction, mut background, mut border) in &mut buttons {
        let focus = Some(*act) == focused;
        *border = BorderColor::all(if focus { ACCENT } else { EDGE });
        background.0 = match interaction {
            Interaction::Pressed => ACCENT.darker(0.35),
            _ if focus => PANEL.lighter(0.06),
            _ => PANEL,
        };
    }
}
