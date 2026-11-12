//! `kjk_game`, the runtime `kjk` plays in: the Bevy app (window, rendering,
//! the play view, the screens, sound and movies), the sim's host, and the
//! sessions it hosts or joins. The command line, the verbs that check and
//! measure and the playthroughs are the binary's (`launcher`, library `kjk`),
//! built on this one so that a change to them rebuilds only them.

/// `println!` that exits quietly when stdout is a closed pipe (`kjk ls | head`)
/// instead of panicking. Exported for the binary's verbs.
#[macro_export]
macro_rules! outln {
    ($($arg:tt)*) => {{
        use std::io::Write as _;
        if let Err(error) = writeln!(std::io::stdout().lock(), $($arg)*) {
            if error.kind() == std::io::ErrorKind::BrokenPipe {
                std::process::exit(0);
            }
            panic!("writing to stdout: {error}");
        }
    }};
}

pub mod acting;
mod aim_fx;
pub mod alignment_look;
mod ambience;
pub mod arena;
mod armory;
mod bark_view;
pub mod battle_music;
mod blade_fx;
mod blaster_view;
mod body_marks;
pub mod by_hand;
pub mod camera_tune;
pub mod cast;
pub mod character;
pub mod clip_events;
mod combat_fx;
pub mod course;
pub mod crash;
pub mod dangly;
pub mod dialogue_view;
pub mod display;
mod dotenv;
pub mod draw_anim;
mod emitter_view;
mod executors;
mod face;
mod fade_view;
mod feat_view;
pub mod fight_rooms;
mod first_person;
pub mod flight_writer;
pub mod floaty_view;
mod floor_arena;
mod force_fx;
mod force_view;
mod form_view;
mod fx_model;
mod gait;
mod glow_shell;
mod going_view;
pub mod grass;
mod grenade_view;
mod ground_lean;
pub mod grunts;
mod gui_scene;
mod headgear;
mod heat_haze;
pub mod hitmarker_view;
pub mod holdings;
mod holster_view;
mod hud;
pub mod import_verbs;
pub mod installs;
mod ja_footsteps;
mod landing_fx;
pub mod launch_times;
mod leader_view;
pub mod lifecycle;
mod lifecycle_view;
mod lock_view;
mod loot;
mod made;
mod map_view;
pub mod material;
mod menu_figure;
mod merchant;
mod message_log;
#[cfg(feature = "probes")]
pub mod minigame_bot;
mod minigame_view;
pub mod model_fx;
pub mod modules;
mod motion_proxy;
pub mod movie_verbs;
mod movie_view;
pub mod net_feel;
pub mod net_join;
mod net_thread;
pub mod own_fx;
pub mod pad;
mod party_calls;
pub mod pazaak_view;
mod perf;
pub mod persona;
pub mod pick_view;
pub mod play;
pub mod play_view;
mod power_fx;
#[cfg(feature = "probes")]
mod probe_kit;
pub mod progress;
mod ragdolls;
pub mod report;
pub mod reticle_view;
mod rulebook_text;
pub use minigame_view::takes_view;
pub use ragdolls::doll_fits;
#[cfg(feature = "probes")]
pub mod held_probes;
mod particle_surfaces;
mod remote_view;
mod rig_ground;
pub mod rig_timeline;
#[cfg(feature = "probes")]
pub mod rise_probes;
pub mod rolloff;
pub mod runtime;
mod saber_flight;
mod saber_marks;
pub mod save_view;
pub mod scene;
mod scene_fx;
pub mod script_fx;
mod script_screens;
pub mod seat_tokens;
pub mod session_flow;
pub mod settings;
pub mod setup;
pub use settings::{key_bindings, key_rows};
mod severing;
pub mod shadow_tiles;
pub mod shadows;
mod shell;
mod shot_fx;
pub mod shots;
mod sim_blades;
pub mod sim_host;
mod sim_view;
mod sim_worker;
pub mod skeleton;
mod solo;
pub mod sound;
mod speed_blur;
pub mod stage;
mod stance;
mod stealth_view;
#[cfg(feature = "steam")]
mod steam_join;
pub mod store_verbs;
mod swallow;
pub mod target_view;
pub mod temporal;
mod terminal;
pub mod transcript;
mod ui_composite;
mod ui_input;
pub mod ui_material;
mod ui_sounds;
mod ui_view;
mod use_view;
pub mod video_effect;
mod view_rng;
pub mod viewer;
pub mod viewer_ja;
pub mod viewer_rig;
mod wardrobe;
mod warmup;
mod weapon_set_view;
mod weapon_sounds;
pub mod weapons;
pub mod weather;
mod wind;
mod window_icon;
pub mod workbench_view;

use std::path::Path;

use vfs::Game;

/// The network conditions for this side: `--netem`, else `KJK_NETEM`, else
/// a perfect network.
pub fn netem_conditions(netem: Option<&str>) -> Result<net::netem::Conditions, String> {
    let conditions = match netem {
        Some(text) => net::netem::Conditions::parse(text).map(Some),
        None => net::netem::Conditions::from_env(),
    };
    conditions
        .map(|conditions| conditions.unwrap_or(net::netem::Conditions::default()))
        .map_err(|error| error.to_string())
}

/// The play options of a networked run on `stage`, its players as `sheet`
/// on the party's bodies (`play::ROSTER`, the same on every machine).
fn net_play_options<'a>(
    stage: &'a str,
    arrive: Option<&'a str>,
    moves: Option<&'a str>,
    seats: u8,
    sheet: sim::combat::Sheet,
) -> play::Options<'a> {
    play::Options {
        aim: None,
        stage,
        arrive,
        bodies: Vec::new(),
        moves,
        camera: None,
        quirks: false,
        trace: None,
        hostile: &[],
        allies: &[],
        seats,
        sheet,
        guests: sim::Guests::Avatars,
        sim_blades: false,
        globals: &[],
        scripts: &[],
        followers: &[],
        gold: None,
        xp: None,
        items: &[],
        talk: None,
        party: &[],
        save: None,
        inexact: None,
        story: None,
        replay: None,
        seat: 0,
        made: None,
    }
}

/// A hosted party set by hand (until saves carry one): who follows the
/// host, the party's gold, items and XP at the start, and script globals.
#[derive(Clone, Copy, Default)]
pub struct ByHandParty<'a> {
    pub followers: &'a [String],
    pub gold: Option<i32>,
    pub items: &'a [(String, u16)],
    pub xp: Option<i32>,
    /// Script globals set before the first step (`--global`).
    pub globals: &'a [(String, i32)],
}

/// What a hosted session is: its stage, its seats, where it listens, how
/// the network is shaped, its players' sheet, and its party by hand.
pub struct HostSpec<'a> {
    pub stage: &'a str,
    pub arrive: Option<&'a str>,
    pub seats: u8,
    /// What those who join play (`sim::Guests`).
    pub guests: sim::Guests,
    pub address: std::net::SocketAddr,
    pub sheet: sim::combat::Sheet,
    pub conditions: net::netem::Conditions,
    pub moves: Option<&'a str>,
    /// The host's scripted view turned to creatures with this tag.
    pub aim: Option<&'a str>,
    pub sim_blades: bool,
    /// A saved game to go on from (its module is the stage; the seats stay
    /// these, its guests taking its companions).
    pub save: Option<&'a sim::Save>,
    pub party: ByHandParty<'a>,
    /// Only players who prove this password join.
    pub password: Option<&'a str>,
    /// The character the host plays as, when one was made.
    pub made: Option<&'a ui::intent::CharacterDesc>,
    /// Steam friends join too (through Steam's relays, no port opened),
    /// where Steam is running.
    pub steam: bool,
    /// Where what takes a while is told (the loading screen's line).
    pub status: &'a (dyn Fn(String) + Sync),
}

/// A hosted session ready to run: listening, its stage loaded, its
/// credentials made.
pub fn host_session(
    discovery: &vfs::Discovery,
    spec: HostSpec<'_>,
) -> Result<play::Loaded, String> {
    if !(1..=sim::MAX_SEATS).contains(&spec.seats) {
        return Err(format!("seats: 1 to {}", sim::MAX_SEATS));
    }
    let udp = net::io::Udp::bind(spec.address, solo::clock())
        .map_err(|error| format!("listen on {}: {error}", spec.address))?;
    // A host no IPv4 player can reach would wait for players who all time
    // out (issue 340).
    if let Some(why) = udp.ipv4_refused() {
        return Err(format!(
            "Port {} is in use by another program for IPv4, so players could not join by an IPv4 address. Close the other program or host on another port. ({why})",
            spec.address.port()
        ));
    }
    let socket = net::io::Socket::udp(udp);
    let socket = if spec.steam {
        beside_steam(socket)
    } else {
        socket
    };
    let ByHandParty {
        followers,
        gold,
        items,
        xp,
        globals,
    } = spec.party;
    let play_options = play::Options {
        aim: spec.aim,
        sim_blades: spec.sim_blades,
        guests: spec.guests,
        save: spec.save,
        followers,
        gold,
        items,
        xp,
        globals,
        made: spec.made.map(persona::Made::Desc),
        ..net_play_options(spec.stage, spec.arrive, spec.moves, spec.seats, spec.sheet)
    };
    let mut loaded = play::load(discovery, &play_options)?;
    // Players must have this build and the data the world is built from:
    // JA's (the animations) and the module's game's.
    let module_game = loaded
        .sim
        .lifecycle
        .as_ref()
        .and_then(|lifecycle| lifecycle.first.as_ref())
        .and_then(|first| Game::from_key(&first.game));
    let games: Vec<Game> = std::iter::once(Game::JediAcademy)
        .chain(module_game)
        .collect();
    let credentials = solo::credentials("host", world_data(discovery, &games, spec.status)?);
    if solo::dirty() {
        outln!(
            "a dirty build ({}): only the same dirty build can join",
            credentials.build
        );
    }
    if spec.password.is_some() {
        outln!("players need the password to join");
    }
    // Said once the stage is loaded and the credentials made, so whoever
    // waits for it (`cargo xtask duo`, `refusals`) joins a host that answers.
    outln!(
        "{} seats; guests play {}",
        spec.seats,
        match spec.guests {
            sim::Guests::Avatars => "a player each",
            sim::Guests::Companions => "the party's companions",
        }
    );
    outln!(
        "listening on {}{}",
        socket
            .udp_end()
            .and_then(net::io::Udp::address)
            .map_or_else(|| spec.address.to_string(), |a| a.to_string()),
        if socket.udp_end().is_some_and(net::io::Udp::split_stack) {
            " (and IPv4 beside it)"
        } else {
            ""
        }
    );
    loaded.sim.net = sim_host::Net::Host(Box::new(solo::Listen {
        socket: net::netem::Netem::new(socket, spec.conditions),
        credentials,
        join_key: spec.password.map(net::handshake::JoinKey::new),
    }));
    Ok(loaded)
}

/// `socket` with Steam beside it, friends told it hosts; as it was where
/// Steam is not running (or this build has none).
fn beside_steam(socket: net::io::Socket) -> net::io::Socket {
    #[cfg(feature = "steam")]
    match net::io::Steam::open(solo::clock()) {
        Ok(mut steam) => {
            steam.advertise();
            outln!(
                "Steam friends join from their friends list (or {})",
                net::steam_address(steam.id())
            );
            return socket.with_steam(steam);
        }
        Err(why) => outln!("{why}: players join by address"),
    }
    #[cfg(not(feature = "steam"))]
    outln!("no Steam in this build: players join by address");
    socket
}

/// The world data (`Fingerprint::world`) of each of `games` this machine
/// has, for a session's credentials: its world groups' files alone
/// (`world_fingerprint`). The fingerprint cache is used: the first time
/// hashes them (seconds on a fast disk, minutes on a slow one, which
/// `status` tells as it goes), later only what changed.
fn world_data(
    discovery: &vfs::Discovery,
    games: &[Game],
    status: &dyn Fn(String),
) -> Result<Vec<net::handshake::GameFingerprint>, String> {
    games
        .iter()
        .filter_map(|&game| discovery.get(game))
        .map(|install| {
            let started = std::time::Instant::now();
            let game = install.game;
            let opened = installs::open(discovery, game)
                .map_err(|error| format!("{}: {error}", install.data_root.display()))?;
            // Told in tenths, so the loading screen shows it moving.
            let mut told = None;
            let mut progress = |done: u64, total: u64| {
                let tenth = (done * 10).checked_div(total).unwrap_or(10);
                if told != Some(tenth) {
                    told = Some(tenth);
                    bevy::log::info!(target: "session", game = game.key(), done, total, "checking the game data");
                    let name = match game {
                        Game::Kotor1 => "KOTOR",
                        Game::Kotor2 => "KOTOR 2",
                        Game::JediAcademy => "Jedi Academy",
                    };
                    status(format!(
                        "Checking the {name} game data (only the first time): {}%",
                        tenth * 10
                    ));
                }
            };
            let print = vfs::fingerprint::world_fingerprint(
                game,
                &opened.index,
                &fingerprint_cache(game),
                &mut progress,
            )
            .map_err(|error| format!("fingerprinting {}: {error}", game.key()))?;
            if print.hashed > 0 {
                bevy::log::info!(
                    target: "session",
                    game = game.key(),
                    files = print.hashed,
                    seconds = started.elapsed().as_secs_f64(),
                    "hashed the game data (cached from now on)"
                );
                outln!(
                    "{}: hashed {} files of its data in {:.1} s (cached from now on)",
                    game.key(),
                    print.hashed,
                    started.elapsed().as_secs_f64()
                );
            }
            let world = print.world();
            Ok(net::handshake::GameFingerprint {
                game: game.key().to_string(),
                digest: world.digest,
                groups: world.groups,
            })
        })
        .collect()
}

/// Who joins whom, and how.
pub struct JoinSpec<'a> {
    pub host: &'a str,
    pub name: &'a str,
    /// The host's join password, proved in the handshake.
    pub password: Option<&'a str>,
    pub conditions: net::netem::Conditions,
    pub moves: Option<&'a str>,
    pub sim_blades: bool,
    pub plan_delay: std::time::Duration,
    pub trace: Option<&'a std::path::Path>,
    /// A player's game: it joins with the seat token it kept for this host
    /// and keeps the one it is given (`seat_tokens`).
    pub remember: bool,
    /// Where what takes a while is told (the loading screen's line).
    pub status: &'a (dyn Fn(String) + Sync),
}

/// A joined session ready to run: the host answered, its stage loaded
/// here (the connection kept answered meanwhile).
pub fn join_session(
    discovery: &vfs::Discovery,
    spec: JoinSpec<'_>,
) -> Result<play::Loaded, String> {
    // Every game this machine has: the host's check names the ones its
    // world needs.
    let games: Vec<Game> = discovery
        .installs
        .iter()
        .map(|install| install.game)
        .collect();
    let credentials = solo::credentials(spec.name, world_data(discovery, &games, spec.status)?);
    (spec.status)(format!("Connecting to {}", spec.host));
    if solo::dirty() {
        outln!(
            "a dirty build ({}): only the same dirty build can host",
            credentials.build
        );
    }
    let mut joining = net_join::peer(spec.host)
        .and_then(|address| {
            let key = spec.password.map(net::handshake::JoinKey::new);
            net_join::connect(address, credentials, key, spec.conditions, spec.remember)
        })
        .map_err(|error| format!("join: {error}"))?;
    let (stage, arrive, seats, players) = match &joining.request {
        net::session::LevelRequest::Module(announcement) => {
            let install = net::session::InstallRequest::of(announcement);
            let request = net_join::planned(&install);
            let players = Some(request.players);
            (request.module, request.arrival, request.seats, players)
        }
        net::session::LevelRequest::Stage(id) => (id.name.clone(), None, joining.seats, None),
    };
    outln!("joined as seat {}: loading {stage}", joining.seat.0);
    joining.plan_delay = spec.plan_delay;
    // The net thread keeps the connection answered while the stage loads;
    // the host's snapshots carry the players' sheets.
    let options = play::Options {
        aim: None,
        trace: spec.trace,
        sim_blades: spec.sim_blades,
        ..net_play_options(
            &stage,
            arrive.as_deref(),
            spec.moves,
            seats.max(1),
            sim::combat::Sheet::dev_soldier(),
        )
    };
    let mut loaded = play::load_for(discovery, &options, players)?;
    loaded.sim.net = sim_host::Net::Join(Box::new(joining));
    Ok(loaded)
}

/// Each found install with its fingerprint, when the fingerprint cache
/// already covers it (a run never hashes gigabytes before it starts), and
/// how long opening it took (`installs`).
pub fn games_manifest(discovery: &vfs::Discovery) -> serde_json::Value {
    let games: Vec<serde_json::Value> = discovery
        .installs
        .iter()
        .map(|found| {
            let install = installs::open(discovery, found.game).ok();
            let opened = install.as_ref().map(|install| {
                serde_json::json!({ "index_ms": install.index_ms, "base_ms": install.base_ms() })
            });
            serde_json::json!({
                "game": found.game.key(),
                "root": found.root.display().to_string(),
                "fingerprint": install.as_ref().and_then(|install| install.fingerprint()).map(|print| &print.digest),
                "opened": opened,
            })
        })
        .collect();
    serde_json::Value::Array(games)
}

/// The install fingerprints' cache, in the store every worktree shares (else
/// this checkout's artifacts).
pub fn fingerprint_cache(game: Game) -> std::path::PathBuf {
    let name = format!("{}.tsv", game.key());
    match store_verbs::opened() {
        Ok(store) => store.named("cache/fingerprint", &name),
        Err(_) => crate::runtime::artifacts()
            .join("cache/fingerprint")
            .join(name),
    }
}

/// K1's install indexed, where this machine has it: what the probes on
/// K1's own data open (`k1_open`).
#[cfg(feature = "probes")]
pub fn k1_index() -> Option<vfs::DirIndex> {
    discover()
        .get(vfs::Game::Kotor1)
        .and_then(|install| vfs::DirIndex::of_install(install).ok())
}

/// K1's resources in `index`, with `module`'s archives where one is named.
#[cfg(feature = "probes")]
pub fn k1_open(
    index: &vfs::DirIndex,
    module: Option<&str>,
) -> Result<vfs::resolver::Resolver, String> {
    use vfs::resolver::{Resolver, TextureQuality};
    Resolver::open(vfs::Game::Kotor1, index, module, TextureQuality::High)
        .map_err(|error| format!("{module:?}: {error:?}"))
}

/// K1's install, without a module, where this machine has it.
#[cfg(feature = "probes")]
pub fn k1_resolver() -> Option<vfs::resolver::Resolver> {
    k1_index().and_then(|index| k1_open(&index, None).ok())
}

/// The installs this machine has. Variables come from the environment first,
/// then the folders the player chose in the setup (`setup`, kept in the
/// settings), then `.env` in the working directory, then `default_var`.
pub fn discover() -> vfs::Discovery {
    let chosen = settings::chosen_installs();
    let env_file = dotenv::load(Path::new(".env"));
    vfs::discover(|name| {
        std::env::var(name)
            .ok()
            .or_else(|| chosen.get(name).cloned())
            .or_else(|| env_file.get(name).cloned())
            .or_else(|| default_var(name))
    })
}

/// What a variable nothing sets is. The Steam Workshop items are the
/// player's build's (`cargo xtask dist`, without `probes`): a build with the
/// verbs that check plays the game folders alone, as the approved tests do,
/// so a chain's or sweep's answer does not depend on what a machine is
/// subscribed to. `KJK_WORKSHOP=on` gives such a build the items.
fn default_var(name: &str) -> Option<String> {
    (cfg!(feature = "probes") && name == vfs::WORKSHOP_VAR).then(|| "off".to_string())
}
