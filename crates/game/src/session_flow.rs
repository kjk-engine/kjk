//! Sessions the menu starts, inside the running app: a session intent
//! from the screens (`ui_view::UiIntents`) is prepared on a thread of its
//! own (the stage loaded; for a host, the port bound; for a joining
//! player, the host's answer first) while KOTOR's loading screen shows
//! what is happening. Once ready it is installed into the world the play
//! plugins wait in (they were added dormant): the scene and its camera,
//! the sim's host, the views, the HUD. A failure goes back to the main
//! menu with why.
//!
//! `kjk play`, `host` and `join` take the same way from their first frame
//! (`launch`), the only way a session comes in: its
//! session loads on a thread while the window already shows the loading
//! screen, its script waits until the session is in (`console::Hold`), and
//! a failure ends the run with why.

use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, Sender, TryRecvError, channel};

use bevy::prelude::*;
use ui::intent::{Intent, SessionIntent};
use ui::vm::SessionView;

use crate::play;
use crate::scene::SceneContent;
use crate::ui_view::{Ui, UiIntents};

/// A game left for the main menu: the program starts again, on it.
static TO_MENU: AtomicBool = AtomicBool::new(false);
/// The save this run loads at once (`kjk --load`).
static LOAD_AT_START: Mutex<Option<String>> = Mutex::new(None);
/// This run is a game left for the main menu (no logos before it).
static CAME_BACK: AtomicBool = AtomicBool::new(false);

fn take(slot: &Mutex<Option<String>>) -> Option<String> {
    slot.lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .take()
}

fn put(slot: &Mutex<Option<String>>, id: String) {
    *slot
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(id);
}

/// Whether the run ended to go back to the main menu.
pub fn to_menu() -> bool {
    TO_MENU.load(Ordering::Acquire)
}

/// The run's menu loads the save `id` at once, as if it was picked there.
pub fn load_at_start(id: String) {
    put(&LOAD_AT_START, id);
}

/// The run is a game left for the main menu (`kjk --back-to-menu`).
pub fn came_back() {
    CAME_BACK.store(true, Ordering::Release);
}

/// The run opens on a save or comes back from a game: the logos only open
/// the program's first menu.
pub fn skips_logos() -> bool {
    CAME_BACK.load(Ordering::Acquire)
        || LOAD_AT_START
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .is_some()
}

/// This program again, on its main menu, in place of this process (on
/// Windows, beside it as this one ends).
pub fn restart_into_menu() -> std::io::Error {
    let program = match std::env::current_exe() {
        Ok(program) => program,
        Err(error) => return error,
    };
    let mut command = std::process::Command::new(program);
    command.arg("--back-to-menu");
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

/// The module a new game of `campaign` starts in (none: K1's, as before
/// the menu asked).
fn new_game(campaign: Option<vfs::Game>) -> &'static str {
    ui::intent::new_game_module(campaign.unwrap_or(vfs::Game::Kotor1))
}

/// What a preparing thread sends.
enum Progress {
    Status(String),
    /// The character a save was made with.
    Made(ui::intent::CharacterDesc),
    Ready(Box<Result<play::Loaded, String>>),
}

/// The session being prepared, when one is; `launch` when it is the run's
/// own (`kjk play`'s), whose failure ends the run.
#[derive(Resource, Default)]
struct Flow {
    job: Option<Mutex<Receiver<Progress>>>,
    launch: bool,
}

/// A run's own session, loading while its window shows the loading screen
/// (`launch`), of its module when known.
pub struct Launch {
    receiver: Receiver<Progress>,
    module: Option<String>,
}

/// Where the launch's loading thread sends its session.
pub struct Launching(Sender<Progress>);

impl Launching {
    pub fn ready(self, result: Result<play::Loaded, String>) {
        let _ = self.0.send(Progress::Ready(Box::new(result)));
    }
}

/// A launch into `module`, when known: the loading thread's end, and the
/// flow's.
pub fn launch(module: Option<&str>) -> (Launching, Launch) {
    let (sender, receiver) = channel();
    let launch = Launch {
        receiver,
        module: module.map(str::to_string),
    };
    (Launching(sender), launch)
}

#[derive(Default)]
pub struct SessionFlowPlugin {
    launch: Mutex<Option<Launch>>,
}

impl SessionFlowPlugin {
    /// The flow, installing `launch` once it is loaded.
    pub fn launching(launch: Launch) -> SessionFlowPlugin {
        SessionFlowPlugin {
            launch: Mutex::new(Some(launch)),
        }
    }
}

impl Plugin for SessionFlowPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Flow>()
            .add_systems(
                Update,
                start
                    .after(crate::ui_view::Stepped)
                    .after(crate::save_view::Acted),
            )
            .add_systems(Last, finish)
            .add_systems(Startup, |mut ui: ResMut<Ui>| {
                ui.vm.campaigns = crate::installs::campaigns(&crate::discover());
            });
        if let Some(id) = take(&LOAD_AT_START) {
            app.add_systems(Startup, move |mut intents: ResMut<UiIntents>| {
                intents.0.push(Intent::Session(SessionIntent::Solo {
                    save: Some(id.clone()),
                    character: None,
                    campaign: None,
                }));
            });
        }
        let launch = self
            .launch
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .take();
        if let Some(Launch { receiver, module }) = launch {
            app.insert_resource(Flow {
                job: Some(Mutex::new(receiver)),
                launch: true,
            })
            .insert_resource(console::Hold(true))
            .add_systems(Startup, move |mut ui: ResMut<Ui>| {
                show_loading(&mut ui, module.as_ref().map(|module| format!("load_{module}")));
            })
            // After the screen's camera is made, which it draws into.
            .add_systems(PostStartup, crate::warmup::prewarm);
        }
    }
}

/// The loading screen alone, with `image`.
fn show_loading(ui: &mut Ui, image: Option<String>) {
    ui.vm.session = SessionView {
        image,
        version: std::mem::take(&mut ui.vm.session.version),
        ..SessionView::default()
    };
    ui.stack.clear();
    ui.stack.open("loading", &ui.vm);
}

/// `kjk play`'s options on `stage`, the new game's module, alone.
fn solo_options(stage: &'static str) -> Result<play::Options<'static>, String> {
    Ok(play::Options {
        aim: None,
        stage,
        arrive: None,
        bodies: play::bodies("", "")?,
        moves: None,
        camera: None,
        quirks: false,
        trace: None,
        hostile: &[],
        allies: &[],
        seats: 1,
        sheet: sim::combat::Sheet::dev_soldier(),
        guests: sim::Guests::Avatars,
        sim_blades: false,
        replay: None,
        seat: 0,
        globals: &[],
        scripts: &[],
        followers: &[],
        gold: None,
        xp: None,
        items: &[],
        talk: None,
        party: &[],
        save: None,
        made: None,
    })
}

/// Prepares `intent` on a thread of its own; what it does goes to
/// `sender`, then the session (or why not).
fn prepare(intent: SessionIntent) -> Receiver<Progress> {
    let (sender, receiver) = channel();
    std::thread::spawn(move || {
        let discovery = crate::discover();
        let status = |text: String| {
            let _ = sender.send(Progress::Status(text));
        };
        let conditions = crate::netem_conditions(None);
        // A save's world, read here; the character it was made with goes
        // to the flow (`character::Made`).
        let read = |id: Option<String>| -> Result<Option<sim::Save>, String> {
            let Some(id) = id else {
                return Ok(None);
            };
            let (save, meta) = crate::save_view::read(&id)?;
            let origin =
                crate::save_view::folder().and_then(|root| saves::copies::origin(&root, &id));
            status(match origin {
                Some(copy) => format!(
                    "Loading {}, a copy from {} (build {})",
                    meta.name, copy.from, copy.build
                ),
                None => format!("Loading {}", meta.name),
            });
            if let Some(made) = meta.character {
                let _ = sender.send(Progress::Made(made));
            }
            Ok(Some(save))
        };
        let result = match intent {
            SessionIntent::Solo {
                save,
                character,
                campaign,
            } => read(save).and_then(|save| {
                let stage = new_game(campaign);
                if save.is_none() {
                    status(format!("Loading {stage}"));
                }
                let options = play::Options {
                    save: save.as_ref(),
                    made: character.as_ref().map(crate::persona::Made::Desc),
                    ..solo_options(stage)?
                };
                play::load(&discovery, &options)
            }),
            SessionIntent::Host {
                save,
                seats,
                port,
                character,
                campaign,
            } => read(save).and_then(|save| {
                status(format!("Hosting on port {port} for {seats} players"));
                crate::host_session(
                    &discovery,
                    crate::HostSpec {
                        stage: new_game(campaign),
                        arrive: None,
                        // A save's guests take its companions (I31): the
                        // seats are the host's to choose.
                        seats,
                        guests: sim::Guests::Companions,
                        address: std::net::SocketAddr::new(
                            std::net::Ipv6Addr::UNSPECIFIED.into(),
                            port,
                        ),
                        sheet: sim::combat::Sheet::dev_soldier(),
                        conditions: conditions?,
                        moves: None,
                        aim: None,
                        sim_blades: false,
                        save: save.as_ref(),
                        party: crate::ByHandParty::default(),
                        password: None,
                        made: character.as_ref(),
                        steam: true,
                    },
                )
            }),
            SessionIntent::Join { address, name } => {
                status(format!("Connecting to {address}"));
                conditions.and_then(|conditions| {
                    crate::join_session(
                        &discovery,
                        crate::JoinSpec {
                            host: &address,
                            name: &name,
                            password: None,
                            conditions,
                            moves: None,
                            sim_blades: false,
                            plan_delay: std::time::Duration::ZERO,
                            trace: None,
                        },
                    )
                })
            }
            SessionIntent::ToMenu => Err("not a session".into()),
        };
        let _ = sender.send(Progress::Ready(Box::new(result)));
    });
    receiver
}

/// Takes the screens' session intents: one is prepared at a time.
fn start(
    mut ui: ResMut<Ui>,
    mut intents: ResMut<UiIntents>,
    mut flow: ResMut<Flow>,
    mut made: ResMut<crate::character::Made>,
    mut exit: MessageWriter<AppExit>,
    (playing, here): (
        Option<Res<crate::sim_host::OwnSeat>>,
        Option<Res<crate::sim_host::WorldHere>>,
    ),
    mut reload: ResMut<crate::sim_host::ReloadRequest>,
) {
    let mut rest = Vec::new();
    for intent in std::mem::take(&mut intents.0) {
        match intent {
            Intent::Session(SessionIntent::ToMenu) => {
                info!(target: "session", "back to the main menu");
                TO_MENU.store(true, Ordering::Release);
                exit.write(AppExit::Success);
            }
            // During play a save is loaded in place, as the reload after
            // the party fell is (`sim_host::ReloadRequest`): its module
            // planned behind the loading screen, the world made from the
            // save at the commit; the program and its window go on. Alone
            // only (a host's guests would be dropped: it loads from the
            // main menu). The screens' Load, quick load and the console's
            // `ui load` all come here.
            Intent::Session(SessionIntent::Solo { save: Some(id), .. }) if playing.is_some() => {
                let refused = match here.as_deref() {
                    Some(here) if here.alone => None,
                    Some(_) => Some("A hosted game loads from the main menu."),
                    None => Some("Only the host loads a save."),
                };
                if let Some(why) = refused {
                    warn!(target: "session", id, "only a game played alone loads during play");
                    ui.vm.feedback.push(why.into());
                } else {
                    info!(target: "session", id, "into a save, in place");
                    // The character the save was made with, as a load from
                    // the menu takes it (`Progress::Made`).
                    let meta = crate::save_view::folder().and_then(|root| {
                        saves::list(&root)
                            .saves
                            .into_iter()
                            .find(|(of, _)| *of == id)
                    });
                    if let Some((_, meta)) = meta {
                        made.0 = meta.character;
                    }
                    ui.stack.clear();
                    reload.0 = Some(id);
                }
            }
            Intent::Session(session) if flow.job.is_none() => {
                info!(target: "session", ?session, "starting");
                // A guest keeps what the host gives: nothing made here.
                made.0 = match &session {
                    SessionIntent::Solo { character, .. }
                    | SessionIntent::Host { character, .. } => character.clone(),
                    _ => None,
                };
                let image = match &session {
                    SessionIntent::Solo { save, campaign, .. }
                    | SessionIntent::Host { save, campaign, .. } => {
                        let module = save.as_ref().and_then(|id| {
                            let save = ui.vm.saves.iter().find(|save| save.id == *id)?;
                            Some(save.module.as_str())
                        });
                        Some(format!("load_{}", module.unwrap_or(new_game(*campaign))))
                    }
                    _ => None,
                };
                show_loading(&mut ui, image);
                flow.job = Some(Mutex::new(prepare(session)));
            }
            Intent::Session(session) => {
                warn!(target: "session", ?session, "a session is already starting");
            }
            other => rest.push(other),
        }
    }
    intents.0 = rest;
}

/// The session prepared, installed; or the main menu again, with why not.
fn finish(world: &mut World) {
    let Some(job) = world.resource::<Flow>().job.as_ref() else {
        return;
    };
    let mut ready = None;
    let mut statuses = Vec::new();
    let mut character = None;
    loop {
        let message = job.lock().expect("the session job's lock").try_recv();
        match message {
            Ok(Progress::Status(text)) => statuses.push(text),
            Ok(Progress::Made(made)) => character = Some(made),
            Ok(Progress::Ready(result)) => {
                ready = Some(*result);
                break;
            }
            Err(TryRecvError::Empty) => break,
            Err(TryRecvError::Disconnected) => {
                ready = Some(Err("the session's thread ended without a session".into()));
                break;
            }
        }
    }
    if let Some(status) = statuses.pop() {
        world.resource_mut::<Ui>().vm.session.status = status;
    }
    if character.is_some() {
        world.resource_mut::<crate::character::Made>().0 = character;
    }
    let Some(result) = ready else {
        return;
    };
    let launch = std::mem::take(&mut world.resource_mut::<Flow>().launch);
    world.resource_mut::<Flow>().job = None;
    match result.and_then(|loaded| install(world, loaded)) {
        Ok(()) => world.resource_mut::<console::Hold>().0 = false,
        Err(why) if launch => {
            // The run was for this session: it ends, saying why, as when it
            // loaded before its window.
            eprintln!("{why}");
            world.write_message(AppExit::error());
        }
        Err(why) => {
            warn!(target: "session", %why, "the session did not start");
            let mut ui = world.resource_mut::<Ui>();
            let ui = &mut *ui;
            ui.vm.session.error = Some(why);
            ui.stack.clear();
            ui.stack.open("main_menu", &ui.vm);
            ui.stack.open("message", &ui.vm);
        }
    }
}

/// A prepared session put into the world the play plugins wait in.
fn install(world: &mut World, loaded: play::Loaded) -> Result<(), String> {
    let play::Loaded {
        scene,
        sim,
        view,
        effects,
        sounds,
        blades,
        weapons,
        powers,
        hud,
    } = loaded;
    info!(target: "session", "installed");
    // The screens read the session's campaign: a TSL save loaded from the
    // menu, a TSL host joined, as a TSL new game does.
    let campaign = sim
        .level
        .module()
        .and_then(|module| vfs::Game::from_key(&module.game));
    if let Some(campaign) = campaign {
        crate::runtime::set_campaign(&mut world.resource_mut::<Ui>(), campaign);
    }
    crate::scene::replace_camera(world, &scene);
    world.insert_resource(SceneContent(Box::new(scene)));
    crate::sim_host::install(world, sim)?;
    crate::play_view::install(world, view);
    crate::combat_fx::install(world, effects);
    crate::sound::install(world, sounds);
    crate::blade_fx::install(world, blades);
    world.insert_resource(weapons);
    crate::force_view::install(world, powers);
    crate::hud::install(world, hud);
    world.resource_mut::<Ui>().stack.clear();
    Ok(())
}
