//! The running game: a window, or a headless run that renders into an image.
//! Either way a run gets its own folder under `kjk-artifacts/runs/` with a
//! log, a manifest and whatever its commands produce. A player's build (no
//! `probes`) keeps its runs in the user's own folder instead, the last
//! [`KEPT_RUNS`] of them: the logs a bug report wants, and nothing written
//! where the program was started from.

use std::path::{Path, PathBuf};
use std::sync::{Mutex, PoisonError};
use std::thread::JoinHandle;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use bevy::app::ScheduleRunnerPlugin;
use bevy::log::{BoxedLayer, LogPlugin, tracing_subscriber};
use bevy::prelude::*;
use bevy::render::renderer::RenderAdapterInfo;
use bevy::window::ExitCondition;
use bevy::winit::WinitPlugin;
use console::{CaptureTarget, Command, ConsolePlugin, DumpFacts, RunFolder};
use ui::assets::GuiAssets;

use crate::scene::{FrameSize, Headless, SceneContent, ScenePlugin};
use crate::viewer::LoadedScene;

pub struct RunOptions {
    pub commands: Vec<Command>,
    pub script: String,
    pub headless: bool,
    /// The headless frame size.
    pub size: (u32, u32),
    /// Facts about the machine for the manifest (games, fingerprints):
    /// gathered on a thread while the app runs (indexing every install
    /// took half a second and more before the app could start), and added
    /// to the manifest when the run ends.
    pub games: Box<dyn FnOnce() -> serde_json::Value + Send>,
    pub scene: Scene,
}

/// What the run shows.
pub enum Scene {
    Empty,
    Loaded(Box<LoadedScene>),
    /// The sim's placeholder world, stepped live (`kjk sim view`).
    #[cfg_attr(not(feature = "probes"), allow(dead_code))]
    Sim(Box<crate::sim_host::SimSetup>),
    /// A player on a stage, loading behind the window's loading screen
    /// (`kjk play`, `host`, `join`: `session_flow::launch`).
    Launch(crate::session_flow::Launch),
    /// The first-run setup (`setup`): the games found or chosen, then
    /// imported; `start`: the import at once.
    Setup {
        start: bool,
    },
}

/// Keeps the Chrome trace writer alive until the app is dropped; dropping it
/// flushes `trace.json`.
#[derive(Resource)]
struct TraceGuard(#[allow(dead_code)] std::sync::Mutex<tracing_chrome::FlushGuard>);

#[derive(Resource)]
struct Manifest(serde_json::Value);

pub fn run(options: RunOptions) -> AppExit {
    let folder = match create_run_folder() {
        Ok(folder) => folder,
        Err(error) => {
            eprintln!("cannot create the run folder: {error}");
            return AppExit::error();
        }
    };
    eprintln!("run folder: {}", folder.display());
    crate::crash::install_hook(folder.clone(), crate::crash::facts());
    let player = crate::crash::PlayerRun::of(options.headless, &options.script);
    let mut backup = None;
    if player.0 {
        crate::crash::mark(&folder, crate::crash::PLAYER);
        backup = back_saves_up().err().map(crate::crash::BackupFailed);
    }

    let games = std::thread::spawn(options.games);
    let mut app = App::new();
    if let Some(backup) = backup {
        app.insert_resource(backup);
    }
    app.insert_resource(RunFolder(folder.clone()))
        .insert_resource(player)
        .insert_resource(Manifest(serde_json::json!({
            "run": folder.file_name().map(|name| name.to_string_lossy().into_owned()),
            "git": env!("KJK_GIT_SHA"),
            "profile": if cfg!(debug_assertions) { "debug" } else { "release" },
            "headless": options.headless,
            "script": options.script,
            "games": serde_json::Value::Null,
            "perf": perf_enabled(),
        })))
        .insert_resource(Headless(options.headless))
        .insert_resource(FrameSize(options.size.0, options.size.1));
    // The game's screens: the main menu in an empty window, over the play
    // in a game.
    let screens = match &options.scene {
        Scene::Empty => Some(Some("main_menu")),
        Scene::Launch(_) => Some(None),
        _ => None,
    };
    let menu = matches!(options.scene, Scene::Empty);
    let setup = match options.scene {
        Scene::Setup { start } => Some(crate::setup::SetupPlugin { start }),
        _ => None,
    };
    // A viewer whose scene has thunder to play hears it.
    let thunder = matches!(&options.scene, Scene::Loaded(scene)
        if scene.weather.lightning.as_ref().is_some_and(|lightning| !lightning.thunder.is_empty()));
    let mut launch = None;
    let sim = match options.scene {
        Scene::Empty | Scene::Setup { .. } => None,
        Scene::Launch(launching) => {
            launch = Some(launching);
            None
        }
        Scene::Loaded(scene) => {
            app.insert_resource(SceneContent(scene));
            None
        }
        Scene::Sim(setup) => Some(setup),
    };

    let log = LogPlugin {
        custom_layer: run_layers,
        ..default()
    };
    let mut laps = crate::launch_times::Laps::new("app");
    if options.headless {
        app.add_plugins(
            DefaultPlugins
                .set(log)
                .set(WindowPlugin {
                    primary_window: None,
                    exit_condition: ExitCondition::DontExit,
                    close_when_requested: false,
                    ..default()
                })
                .disable::<WinitPlugin>(),
        )
        .add_plugins(ScheduleRunnerPlugin::run_loop(Duration::from_secs_f64(
            1.0 / 60.0,
        )));
    } else {
        app.add_plugins(DefaultPlugins.set(log).set(WindowPlugin {
            primary_window: Some(Window {
                title: "kjk".into(),
                ..default()
            }),
            ..default()
        }))
        .add_plugins(crate::window_icon::WindowIconPlugin)
        .insert_resource(CaptureTarget::PrimaryWindow);
    }
    laps.lap("engine (window, render device)");
    app.add_plugins(ScenePlugin);
    if thunder {
        // Its mixer first, so the plugin opens no menu one.
        crate::sound::install(app.world_mut(), crate::sound::Bank::default());
        app.add_plugins(crate::sound::SoundPlugin)
            .insert_resource(crate::sound::SceneClock);
    }
    if let Some(setup) = setup {
        app.add_plugins(setup);
    }
    if let Some(setup) = sim {
        app.add_plugins((
            crate::sim_host::SimHostPlugin::new(*setup),
            crate::sim_view::SimViewPlugin,
        ));
    }
    // The menu and the loading screen fill the window's height; the sides
    // stay black.
    let black = menu || launch.is_some();
    // The menu's game in a window is the one friends join and are joined
    // from; a headless or scripted run never shows the player in Steam.
    #[cfg(feature = "steam")]
    if menu && !options.headless && options.script.is_empty() {
        app.add_plugins(crate::steam_join::SteamJoinPlugin);
    }
    if black {
        // The play waits, dormant, for a session the menu starts (or the
        // run's own, loading meanwhile).
        let flow = launch.map_or_else(
            crate::session_flow::SessionFlowPlugin::default,
            crate::session_flow::SessionFlowPlugin::launching,
        );
        app.add_plugins((
            crate::sim_host::SimHostPlugin::dormant(),
            crate::play_view::PlayViewPlugin,
            crate::combat_fx::CombatFxPlugin,
            crate::sound::SoundPlugin,
            crate::blade_fx::BladeFxPlugin,
            crate::body_marks::BodyMarksPlugin,
            crate::shot_fx::ShotFxPlugin,
            crate::power_fx::PowerFxPlugin,
            crate::force_view::ForceViewPlugin,
            crate::lifecycle_view::LifecycleViewPlugin,
            crate::fade_view::FadeViewPlugin,
            crate::video_effect::VideoEffectPlugin,
            crate::hud::HudPlugin,
            flow,
            (
                crate::ambience::AmbiencePlugin,
                crate::minigame_view::MinigameViewPlugin,
                crate::script_fx::ScriptFxPlugin,
                crate::blaster_view::BlasterViewPlugin,
                crate::grenade_view::GrenadeViewPlugin,
                // The game's logos open the menu (not a run's own session,
                // nor one opening on a save or back from a game); a headless
                // one is a check that wants the menu at once, and skips them
                // unless asked (`KJK_MOVIES=1`, `played`).
                crate::movie_view::MovieViewPlugin {
                    opening: menu && !crate::session_flow::skips_logos(),
                    play: crate::movie_view::played(options.headless),
                },
            ),
        ));
    }
    if let Some(start) = screens {
        app.add_plugins((
            crate::settings::SettingsPlugin,
            crate::display::DisplayPlugin,
            crate::loot::LootPlugin,
            (
                crate::lock_view::LockViewPlugin,
                crate::ui_sounds::UiSoundsPlugin,
            ),
            crate::use_view::UseViewPlugin,
            crate::target_view::TargetViewPlugin,
            (
                crate::bark_view::BarkViewPlugin,
                crate::arena::ArenaPlugin,
                crate::floaty_view::FloatyViewPlugin,
                crate::message_log::MessageLogPlugin,
            ),
            crate::party_calls::PartyCallsPlugin,
            (
                crate::leader_view::LeaderViewPlugin,
                crate::holster_view::HolsterViewPlugin,
                crate::weapon_set_view::WeaponSetViewPlugin,
                crate::form_view::FormViewPlugin,
            ),
            crate::merchant::MerchantPlugin,
            crate::pazaak_view::PazaakViewPlugin,
            crate::terminal::TerminalPlugin,
            crate::character::CharacterPlugin,
            crate::script_screens::ScriptScreensPlugin,
            (crate::save_view::SaveViewPlugin, crate::report::ReportPlugin),
        ));
        if black {
            app.insert_resource(ClearColor(Color::BLACK));
        }
        let assets = crate::launch_times::timed("app/game plugins/game screens", screen_assets)
            .inspect_err(|error| warn!("the game's screens: {error}"))
            .ok();
        app.add_plugins(crate::ui_view::UiViewPlugin::new(assets, start));
    }
    app.add_plugins((
        ConsolePlugin {
            commands: options.commands,
        },
        crate::launch_times::LaunchTimesPlugin,
    ))
    .add_systems(Startup, (log_render_adapter, write_manifest).chain());
    if menu {
        app.add_systems(Startup, crate::crash::notice);
    }
    laps.lap("game plugins");
    crate::launch_times::mark("app built");
    let exit = app.run();
    crate::crash::mark(&folder, crate::crash::ENDED);
    if let Ok(games) = games.join() {
        add_games(&folder.join("manifest.json"), games);
    }
    // The runner dropped the app, and with it the trace's writer, which
    // finished the file: the counters go in after.
    if perf_enabled() {
        crate::perf::finish(&folder.join("trace.json"));
    }
    exit
}

/// The game's screens' assets being opened on a thread (`open_screens`).
static SCREENS: Mutex<Option<JoinHandle<Result<GuiAssets, String>>>> = Mutex::new(None);

/// Starts opening the screens' assets of `campaign` (the game the run plays:
/// its tables, talk table and items; on the install the run shares,
/// `installs`) on a thread, so they open while a run loads.
pub fn open_screens(campaign: vfs::Game) {
    let opening = std::thread::spawn(move || campaign_screens(campaign));
    *SCREENS.lock().unwrap_or_else(PoisonError::into_inner) = Some(opening);
}

/// The screens' assets: those `open_screens` started, else opened now.
fn screen_assets() -> Result<GuiAssets, String> {
    let opening = SCREENS
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .take();
    match opening.map(JoinHandle::join) {
        Some(Ok(opened)) => opened,
        _ => campaign_screens(vfs::Game::Kotor1).or_else(|_| campaign_screens(vfs::Game::Kotor2)),
    }
}

/// The screens moved to `campaign`'s data (`GuiAssets::set_campaign`, on
/// its shared install): the menu's New Game of it, a session of it. What
/// character creation chose from is read again; an install that does not
/// open leaves them as they are.
pub fn set_campaign(ui: &mut crate::ui_view::Ui, campaign: vfs::Game) {
    let Some(assets) = ui.assets.as_mut().filter(|assets| assets.game != campaign) else {
        return;
    };
    let opened = crate::installs::open(&crate::discover(), campaign)
        .and_then(|install| Ok((install.index.clone(), install.base()?.clone())));
    match opened {
        Ok((index, base)) => {
            info!(target: "ui", campaign = campaign.key(), "the screens read its data");
            assets.set_campaign(campaign, index, base);
            ui.vm.chargen = None;
        }
        Err(why) => warn!(target: "ui", campaign = campaign.key(), %why, "the screens stay"),
    }
}

/// The screens' assets on `campaign`'s shared install (its index, base
/// resolver and talk table: the menu's is KOTOR 1's), skinned (`skinned`).
fn campaign_screens(campaign: vfs::Game) -> Result<GuiAssets, String> {
    let discovery = crate::discover();
    let install = crate::installs::open(&discovery, campaign)?;
    let base = install.base()?.clone();
    skinned(
        GuiAssets::with(install.game, install.index.clone(), base),
        &discovery,
    )
}

/// The campaign's screens in KOTOR 2's layouts and art: KOTOR 2 is
/// required.
pub fn skinned(assets: GuiAssets, discovery: &vfs::Discovery) -> Result<GuiAssets, String> {
    let base = crate::installs::open(discovery, vfs::Game::Kotor2)
        .and_then(|install| install.base().cloned())
        .map_err(|error| format!("the screens are KOTOR 2's, which is required: {error}"))?;
    Ok(assets.with_skin(vfs::Game::Kotor2, base))
}

/// Before a player's game first touches the saves with a new release, the
/// whole saves' folder kept in `save-backups` beside it
/// (`saves::backup_on_new_version`); not for saves elsewhere (`KJK_SAVES`).
fn back_saves_up() -> Result<(), String> {
    if std::env::var_os("KJK_SAVES").is_some() {
        return Ok(());
    }
    let (Some(root), Some(folder)) = (crate::save_view::folder(), crate::settings::folder()) else {
        return Ok(());
    };
    let backups = folder.join(saves::safety::BACKUPS);
    let made = saves::backup_on_new_version(&root, &backups, env!("KJK_VERSION"), env!("KJK_GIT_SHA"))?;
    if let Some(made) = made {
        eprintln!("the saves were kept in {}", made.display());
    }
    Ok(())
}

/// The games' facts into the manifest the run wrote.
fn add_games(path: &std::path::Path, games: serde_json::Value) {
    let Some(mut manifest) = std::fs::read(path)
        .ok()
        .and_then(|bytes| serde_json::from_slice::<serde_json::Value>(&bytes).ok())
    else {
        return;
    };
    manifest["games"] = games;
    let text = serde_json::to_string_pretty(&manifest).expect("json values serialize");
    if let Err(error) = std::fs::write(path, text) {
        eprintln!("cannot write {}: {error}", path.display());
    }
}

fn perf_enabled() -> bool {
    std::env::var("KJK_PERF").is_ok_and(|value| value == "1")
}

/// Where what the runs write goes (run folders, checkpoints, extracts,
/// caches), under the working folder.
pub fn artifacts() -> &'static Path {
    Path::new("kjk-artifacts")
}

/// How many of its runs' folders a player's build keeps.
pub const KEPT_RUNS: usize = 10;

/// Where every run's folder is made: under the working folder, or in a
/// player's build the user's own folder's `logs`.
pub fn runs() -> PathBuf {
    if cfg!(feature = "probes") {
        return artifacts().join("runs");
    }
    crate::settings::folder().map_or_else(|| artifacts().join("runs"), |folder| folder.join("logs"))
}

/// A player's build's oldest runs removed, so `keep` are left beside the
/// one about to be made, and the recent crashes (`crash::kept`). Their
/// names sort by their start.
fn prune_runs(runs: &Path, keep: usize) {
    let folders = crate::crash::run_folders(runs);
    let kept = crate::crash::kept(folders.clone(), keep, SystemTime::now());
    for folder in folders.iter().filter(|folder| !kept.contains(folder)) {
        let _ = std::fs::remove_dir_all(folder);
    }
}

/// A folder named after the start time (UTC) and the process id.
pub fn create_run_folder() -> std::io::Result<PathBuf> {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |since| since.as_secs());
    let (days, seconds) = (now / 86_400, now % 86_400);
    let (year, month, day) = saves::civil_from_days(days as i64);
    let name = format!(
        "{year:04}{month:02}{day:02}-{:02}{:02}{:02}-{}",
        seconds / 3600,
        seconds / 60 % 60,
        seconds % 60,
        std::process::id()
    );
    let runs = runs();
    if !cfg!(feature = "probes") {
        prune_runs(&runs, KEPT_RUNS - 1);
    }
    let folder = runs.join(name);
    std::fs::create_dir_all(&folder)?;
    Ok(folder)
}

/// The run's log file, and with `KJK_PERF=1` a Chrome/Perfetto trace, as
/// extra layers beside the console output.
fn run_layers(app: &mut App) -> Option<BoxedLayer> {
    let folder = app.world().resource::<RunFolder>().0.clone();
    let mut layers: Vec<BoxedLayer> = Vec::new();
    match std::fs::File::create(folder.join("kjk.log")) {
        Ok(file) => layers.push(Box::new(
            tracing_subscriber::fmt::layer()
                .with_ansi(false)
                .with_writer(std::sync::Mutex::new(file)),
        )),
        Err(error) => eprintln!("cannot create kjk.log: {error}"),
    }
    if perf_enabled() {
        let (chrome, guard) = tracing_chrome::ChromeLayerBuilder::new()
            .file(folder.join("trace.json"))
            .include_args(true)
            .build();
        app.insert_resource(TraceGuard(std::sync::Mutex::new(guard)));
        crate::perf::start();
        layers.push(Box::new(chrome));
    }
    Some(Box::new(layers))
}

fn log_render_adapter(adapter: Res<RenderAdapterInfo>, mut facts: ResMut<DumpFacts>) {
    info!(
        backend = ?adapter.backend,
        adapter = %adapter.name,
        driver = %adapter.driver_info,
        "render adapter"
    );
    crate::crash::set_adapter(&format!(
        "{} {:?} ({})",
        adapter.name, adapter.backend, adapter.driver_info
    ));
    facts.0.insert(
        "render_adapter".into(),
        serde_json::json!({
            "backend": format!("{:?}", adapter.backend),
            "name": adapter.name,
            "driver": adapter.driver_info,
        }),
    );
}

fn write_manifest(manifest: Res<Manifest>, folder: Res<RunFolder>, facts: Res<DumpFacts>) {
    let mut manifest = manifest.0.clone();
    manifest["render_adapter"] = facts.0["render_adapter"].clone();
    let path = folder.0.join("manifest.json");
    let text = serde_json::to_string_pretty(&manifest).expect("json values serialize");
    if let Err(error) = std::fs::write(&path, text) {
        error!("cannot write {}: {error}", path.display());
    }
}

/// Writes `facts` to `facts.json` in a new run folder, and says where;
/// the folder.
pub fn write_facts(facts: serde_json::Value) -> Option<std::path::PathBuf> {
    let folder = match create_run_folder() {
        Ok(folder) => folder,
        Err(error) => {
            eprintln!("cannot create the run folder: {error}");
            return None;
        }
    };
    write_facts_in(&folder, facts);
    Some(folder)
}

/// Writes `facts` to `facts.json` in `folder`, and says where.
pub fn write_facts_in(folder: &Path, mut facts: serde_json::Value) {
    facts["git"] = env!("KJK_GIT_SHA").into();
    let path = folder.join("facts.json");
    let text = serde_json::to_string_pretty(&facts).expect("json values serialize");
    match std::fs::write(&path, text) {
        Ok(()) => eprintln!("facts: {}", path.display()),
        Err(error) => eprintln!("{}: {error}", path.display()),
    }
}

/// A probe's checks, and its scenes' replays.
#[derive(Default)]
pub struct Report {
    checks: Vec<(String, bool)>,
    replays: Vec<(&'static str, bool)>,
}

impl Report {
    /// A line of the report, and whether it holds.
    pub fn check(&mut self, pass: bool, what: String) {
        outln!("    [{}] {what}", if pass { "ok" } else { "FAIL" });
        self.checks.push((what, pass));
    }

    /// Whether scene `name`'s recording replayed to its end.
    #[cfg(feature = "probes")]
    pub fn replayed(&mut self, name: &'static str, same: bool) {
        self.replays.push((name, same));
    }

    /// The last check (every replay the same, when there were scenes), the
    /// count, and `facts` with the checks written to `facts.json`.
    pub fn finish(mut self, mut facts: serde_json::Value) -> std::process::ExitCode {
        if !self.replays.is_empty() {
            let replayed = self.replays.iter().all(|(_, same)| *same);
            let each = self
                .replays
                .iter()
                .map(|(name, same)| format!("{name}: {}", if *same { "same" } else { "DIFFERENT" }))
                .collect::<Vec<_>>()
                .join(", ");
            self.check(
                replayed,
                format!("every scene's recording replays to its end ({each})"),
            );
        }
        let failed = self.checks.iter().filter(|(_, pass)| !pass).count();
        outln!("  {} checks, {failed} failed", self.checks.len());
        facts["checks"] = self
            .checks
            .iter()
            .map(|(what, pass)| serde_json::json!({"what": what, "pass": pass}))
            .collect();
        facts["failed"] = failed.into();
        write_facts(facts);
        if failed == 0 {
            std::process::ExitCode::SUCCESS
        } else {
            std::process::ExitCode::FAILURE
        }
    }
}
