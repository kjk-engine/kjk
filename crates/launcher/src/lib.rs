//! `kjk`, the command line (the binary, `kjk.rs`, calls [`run`]), the verbs
//! that check and measure, and the playthroughs; the window and everything
//! it plays is the game's (`kjk_game`, `crates/game`). The library has two
//! other doors, both for the approved tests: [`rig_measure`], the skeleton
//! probes' measures without a window (`timeline_jolt_ceilings`), and
//! `probes` (with the `probes` feature, on by default), the grass's, the
//! weather's and the emitters' checks.

// `outln!`, the game's `println!` for piped output.
#[macro_use]
extern crate kjk_game;

// The commands that take the most flags, each with its own: a new flag
// touches its command's file alone.
mod cli_host;
mod cli_join;
mod cli_play;
mod levels;
#[cfg(feature = "probes")]
pub mod probes;
mod report_verbs;
mod resources;
pub mod rig_measure;
mod rules_page;
mod show;
mod texture_export;

/// Declares modules built only with the `probes` feature.
macro_rules! probes {
    ($($name:ident),* $(,)?) => {
        $(#[cfg(feature = "probes")] mod $name;)*
    };
}

// The verbs that check and measure rather than play (`verbs`), and what
// only they use.
probes!(
    ai_verbs,
    chain,
    check,
    clips,
    collision_audit,
    collision_probes,
    collision_verbs,
    combat_verbs,
    creation_probes,
    damage_verbs,
    dialogue_verbs,
    duel_lag_verbs,
    duel_verbs,
    effects_verbs,
    encounter_verbs,
    equip_verbs,
    feats_verbs,
    fight_routines_verbs,
    force_verbs,
    formats,
    formats_ja,
    game_sweep,
    grenades_verbs,
    gui_verbs,
    impact_verbs,
    inventory_verbs,
    items_verbs,
    journal_verbs,
    lag_verbs,
    launch_report,
    levels_verbs,
    lifecycle_verbs,
    loot_verbs,
    materials,
    merchant_verbs,
    motion,
    move_verbs,
    nav_verbs,
    net_bandwidth_verb,
    net_codec_verb,
    net_feel_verb,
    net_margins_verb,
    net_session_verb,
    net_verbs,
    pace_verbs,
    party_verbs,
    playthrough,
    probe,
    routines_verbs,
    saber_verbs,
    scene_check,
    scene_sweep,
    scenes,
    script_verbs,
    security_verbs,
    sim_probes,
    sim_verbs,
    skeleton_probe,
    speaker_census,
    specials_verbs,
    stealth_verbs,
    sweep_verbs,
    tour,
    trace_verbs,
    traps_verbs,
    ute_encounter_probes,
    verbs,
    worked_verbs,
    workbench_probes,
    xp_verbs,
);

// The game's modules and session builders the CLI and the verbs name, by
// their paths here (`crate::scene`, `crate::discover`) as before the split;
// the second list only the verbs and probes name.
use kjk_game::{
    ByHandParty, HostSpec, JoinSpec, by_hand, discover, fingerprint_cache, games_manifest,
    host_session, import_verbs, installs, join_session, launch_times, lifecycle, modules,
    movie_verbs, net_join, netem_conditions, persona, play, play_view, rig_timeline, runtime,
    save_view, scene, session_flow, setup, store_verbs, viewer, viewer_ja, viewer_rig,
};
#[cfg(feature = "probes")]
use kjk_game::{
    acting, cast, character, grass, material, minigame_bot, model_fx, net_feel, own_fx,
    pazaak_view, sim_host, skeleton, sound, stage, transcript, weapons, weather, workbench_view,
};

// What the verbs share with `kjk play` (they take the root's names).
#[cfg(feature = "probes")]
use {
    cli_host::parse_seats,
    cli_play::{ByHand, parse_made, player_sheet},
};

use std::process::ExitCode;

use bevy::app::AppExit;
use clap::{Parser, Subcommand};
use vfs::{DirIndex, Game, Source};

#[derive(Parser)]
#[command(
    name = "kjk",
    about = "kjk: KOTOR worlds with Jedi Academy combat",
    version = concat!(env!("KJK_VERSION"), " (", env!("KJK_GIT_SHA"), ")")
)]
struct Cli {
    #[command(subcommand)]
    command: Option<Command>,
    /// Commands to run, e.g. 'wait 30f; screenshot first; quit'
    #[arg(long)]
    cmds: Option<String>,
    /// Run without a window, rendering into an off-screen image
    #[arg(long)]
    headless: bool,
    /// Open on the save with this id (`quick`, `0001`, a chain's `k1-03`,
    /// ...), as the main menu would load it
    #[arg(long)]
    load: Option<String>,
    /// Open on the main menu without the logos: a game left for it
    /// (`session_flow::restart_into_menu`)
    #[arg(long, hide = true)]
    back_to_menu: bool,
    /// Headless frame size, e.g. 1600x1200 (default 1280x720)
    #[arg(long, global = true, value_parser = parse_size)]
    size: Option<(u32, u32)>,
    /// Open the ways the games keep sealed that playtests go back through
    /// (`data/links`, e.g. the Starboard Section's door back to the Command
    /// Module); every machine of a session passes it alike
    #[arg(long, global = true)]
    playtest_links: bool,
    /// Play no sound (the settings' volumes are kept, not changed);
    /// `KJK_MUTE=1` does the same
    #[arg(long, global = true)]
    mute: bool,
}

fn parse_size(text: &str) -> Result<(u32, u32), String> {
    let (width, height) = text
        .split_once('x')
        .ok_or("use WIDTHxHEIGHT, e.g. 1600x1200")?;
    let number = |part: &str| {
        part.parse::<u32>()
            .ok()
            .filter(|n| (16..=8192).contains(n))
            .ok_or(format!("{part}: a size from 16 to 8192"))
    };
    Ok((number(width)?, number(height)?))
}

#[derive(Subcommand)]
enum Command {
    /// Find the KOTOR 1, TSL and Jedi Academy installs and report them.
    Games,
    /// Turn every module (or one, dumped in full) into the module IR.
    Modules {
        /// k1 or tsl
        game: String,
        /// A module, e.g. end_m01aa; all modules when left out
        module: Option<String>,
    },
    /// List resources after precedence, with the layer each one comes from.
    Ls {
        /// k1, tsl or ja
        game: String,
        /// `resref.ext` pattern; `*` and `?` are wildcards
        pattern: Option<String>,
        /// Also load this module's archives, e.g. end_m01aa
        #[arg(long)]
        module: Option<String>,
        /// Only list resources that hide the same name in a lower layer, and
        /// compare the bytes of each hidden copy with the winner
        #[arg(long)]
        shadowed: bool,
    },
    /// Digest the game data of each install (or one), for comparing machines.
    Fingerprint {
        /// k1, tsl or ja; all found installs when left out
        game: Option<String>,
        /// Hash every file instead of trusting the per-file cache
        #[arg(long)]
        no_cache: bool,
    },
    /// Print a resolved GFF (as a tree) or 2DA (as rows).
    Show {
        /// k1 or tsl
        game: String,
        /// `resref.ext`, e.g. m01aa.are
        resource: String,
        #[arg(long)]
        module: Option<String>,
    },
    /// The wiki for playtesters: every rule the engine plays by (the
    /// rulebook, `data/rules/`) and the game's own weapons, items, powers,
    /// feats, skills and classes as the engine reads them, each with the
    /// rules that shape it, on one searchable page
    /// (`kjk-artifacts/rules/<game>/index.html`, and `codex.json`).
    Rules {
        /// k1 or tsl
        game: String,
        /// The folder to write into, else `kjk-artifacts/rules/<game>`
        #[arg(long)]
        out: Option<std::path::PathBuf>,
    },
    /// Open a module's entry area: every room at its layout position.
    Module {
        /// k1 or tsl
        game: String,
        /// Module, e.g. end_m01aa
        module: String,
        /// Commands to run, e.g. 'camera 20 20 1.5 90 0; screenshot; quit'
        #[arg(long)]
        cmds: Option<String>,
        #[arg(long)]
        headless: bool,
    },
    /// Open a model in the viewer (takes --cmds and --headless like a run).
    View {
        /// k1, tsl or ja
        game: String,
        /// Model resref, without extension (for ja: a path such as
        /// models/players/kyle/model.glm)
        model: String,
        #[arg(long)]
        module: Option<String>,
        /// ja: an animation.cfg sequence to loop, e.g. BOTH_STAND1, or
        /// several, comma-separated, played in turn
        #[arg(long)]
        clip: Option<String>,
        /// ja: a saber for the right hand, e.g. single_1
        #[arg(long)]
        saber: Option<String>,
        /// Commands to run, e.g. 'wait 30f; screenshot; quit'
        #[arg(long)]
        cmds: Option<String>,
        #[arg(long)]
        headless: bool,
    },
    /// A skeleton probe (I10): a JA saber combo on a KOTOR character.
    /// `a` retargets JA's clips onto KOTOR's skeleton, `b` skins the KOTOR
    /// character onto JA's skeleton, `c` plays a KOTOR clip before `a`.
    Rig {
        /// a, b or c
        probe: String,
        /// KOTOR body model
        #[arg(long, default_value = "p_carthbb")]
        body: String,
        /// KOTOR head model
        #[arg(long, default_value = "p_carthh")]
        head: String,
        /// JA sequences, comma-separated (default: a medium-style combo), or
        /// a timeline: `showcase` (draw, walk, run, a swing on the run, the
        /// combo, run back, jump, put away), `acrobatics` (crouch, rolls,
        /// force jump, flips), `specials`, `defense`, `force`, `staff`,
        /// `dual`, `reactions`, `melee`, `styles`, `gestures`, `look`
        /// (looking about, strafing, running every way)
        #[arg(long, default_value = viewer_rig::COMBO)]
        moves: String,
        /// JA saber, from ext_data/sabers
        #[arg(long, default_value = "single_1")]
        saber: String,
        /// For `c`: the KOTOR clip before the combo (its first seconds), or
        /// several, comma-separated, each played whole
        #[arg(long, default_value = "pause1")]
        kotor_clip: String,
        /// Write the feet, hands and head positions over the JA part (CSV)
        #[arg(long)]
        trace: Option<std::path::PathBuf>,
        /// For a and c: `kotor` to hold K1 hilts (the style picks them), or
        /// `ja` for the JA sabers' own
        #[arg(long, default_value = "kotor")]
        hilt: String,
        /// For K1 hilts: the blade colour (blue, red, green, yellow,
        /// purple), comma-separated per hand for two sabers
        #[arg(long, default_value = "blue")]
        color: String,
        /// Reproduce the jolting reference quirks (QUIRKS.md): playback's
        /// (Q-0003, Q-0004, Q-0010, Q-0017, Q-0028), deaths unblended
        /// (Q-0009), the thrown saber's jump back (Q-0007) and the look's
        /// (Q-0012 to Q-0014)
        #[arg(long)]
        quirks: bool,
        /// Commands to run, e.g. 'anim 0.5; screenshot; quit'
        #[arg(long)]
        cmds: Option<String>,
        #[arg(long)]
        headless: bool,
    },
    /// Decode a texture (tpc, else tga) and write its first mip as PNG.
    Texture {
        /// k1 or tsl
        game: String,
        /// Texture resref, without extension
        resref: String,
        #[arg(long)]
        module: Option<String>,
        /// Output folder; defaults to kjk-artifacts/textures/<game>
        #[arg(long)]
        out: Option<std::path::PathBuf>,
    },
    /// Play (I13): a KOTOR character on a stage, moving as a JA player,
    /// with JA's third-person camera. Mouse to look, WASD to move, space to
    /// jump, C or Ctrl to crouch (while running: roll), Shift to walk, E to
    /// use (open a door), the left button to attack, the right one for the
    /// weapon's other mode (a blaster's feat special), 1 and 2 to pick the
    /// hand, L to cycle the saber style, F to cast the selected power ([
    /// and ] to select), G to grip, R for lightning, Q to drain, T to
    /// charge a grenade and throw it on release, 3 for a medpac, 4 for a
    /// stim, 5 for a shield, H to holster the weapon, B to switch weapon
    /// sets, V for TSL's next Form. A door or
    /// trigger that leads out loads its module. The module's creatures move
    /// and fight as the sim has them.
    Play(Box<cli_play::PlayArgs>),
    /// Host a session others join over UDP: `kjk play` for more seats,
    /// listening on a port (`KJK_NETEM` or `--netem` shape what it sends).
    Host(Box<cli_host::HostArgs>),
    /// Join a host's session (`kjk host`) at `host[:port]`: its stage is
    /// loaded here, and this player takes a free seat.
    Join(cli_join::JoinArgs),
    /// The first-run setup: the three games found or chosen, then KOTOR's
    /// shared data imported (a plain `kjk` opens on it until it is done)
    Setup {
        /// Start the import at once when every game is found
        #[arg(long)]
        start: bool,
    },
    /// Imports an install's shared data into the store once (docs/IMPORT.md):
    /// every item and creature template and every voiced line's length;
    /// plans then take them
    Import {
        /// k1 or tsl
        game: String,
        /// Make every product again, even those already there
        #[arg(long)]
        force: bool,
        /// The module planned with and without the products, to compare
        #[arg(long, default_value = "end_m01aa")]
        module: String,
    },
    /// The games' movies (I20): Binks made ours by ffmpeg into the store
    Movie {
        #[command(subcommand)]
        verb: MovieVerb,
    },
    /// The store every worktree shares (docs/IMPORT.md): imported game data
    /// and other derived files
    Store {
        #[command(subcommand)]
        verb: StoreVerb,
    },
    /// A player's bug report (docs/BUG-REPORTS.md): what it says, and its
    /// flight recording replayed
    Report {
        #[command(subcommand)]
        verb: ReportVerb,
    },
    #[cfg(feature = "probes")]
    #[command(flatten)]
    Verb(verbs::Verb),
    /// Copy one resource out of the install, after precedence.
    Extract {
        /// k1, tsl or ja
        game: String,
        /// `resref.ext`, e.g. appearance.2da
        resource: String,
        #[arg(long)]
        module: Option<String>,
        /// Output folder; defaults to kjk-artifacts/extract/<game>
        #[arg(long)]
        out: Option<std::path::PathBuf>,
    },
}

#[derive(Subcommand)]
enum MovieVerb {
    /// The movie container's checks, on frames made here
    Check,
    /// Make movies ours now (every one by default), kept in the store
    Import {
        /// k1 or tsl
        game: String,
        /// The movies, e.g. 01a legal; all of them when left out
        names: Vec<String>,
    },
}

#[derive(Subcommand)]
enum ReportVerb {
    /// The build, machine, note, breadcrumbs, crash, log's end, what the
    /// flight segments span, and whether its data is this install's
    Show { zip: std::path::PathBuf },
    /// Replays the last flight segment headless and says whether it
    /// matched
    Replay {
        zip: std::path::PathBuf,
        /// Stop at the last checkpoint at or before this tick
        #[arg(long)]
        to_tick: Option<u64>,
        /// Only check it; without, it also says how to watch it
        #[arg(long)]
        headless: bool,
        /// Replay even when this install's data is not the player's
        /// (another edition or a mod), up to where it diverges
        #[arg(long)]
        ignore_data: bool,
    },
}

#[derive(Subcommand)]
enum StoreVerb {
    /// Where it is (KJK_STORE, else the user's cache folder)
    Path,
    /// What each kind of file takes
    Du,
    /// Removes what no run used for a while
    Gc {
        #[arg(long, default_value_t = 30)]
        days: u64,
        /// Only count what would go
        #[arg(long)]
        dry_run: bool,
    },
    /// Its rules checked on a scratch root
    Check,
}

/// The command line, parsed on a thread of its own. Unoptimised, clap's
/// derive builds each subcommand tree in one frame, and they nest: `Command`
/// (~450 KB) holds `Verb` (~500 KB), which holds `ScriptVerb` (~450 KB).
/// That overflows Windows' 1 MB main thread before anything prints; macOS and
/// Linux give the main thread 8 MB. Release frames are a fraction of that.
fn parse_cli() -> Cli {
    const STACK: usize = 8 << 20;
    std::thread::Builder::new()
        .name("cli".into())
        .stack_size(STACK)
        .spawn(Cli::parse)
        .expect("the command line's thread starts")
        .join()
        .unwrap_or_else(|panic| std::panic::resume_unwind(panic))
}

/// Parses the command line and runs what it asks for.
pub fn run() -> ExitCode {
    launch_times::start();
    let cli = parse_cli();
    let size = cli.size.unwrap_or(scene::HEADLESS_SIZE);
    if cli.playtest_links {
        lifecycle::open_playtest_links();
    }
    if cli.mute {
        // An environment variable, so the processes this one starts (a
        // restart into the menu, a sweep's children) are silent too.
        // SAFETY: set before any thread is started.
        unsafe { std::env::set_var(kjk_game::sound::MUTE, "1") };
    }
    // The runs that play (a module, the menu, a session) read the installs:
    // opened on threads now, they are read while the rest starts.
    if matches!(
        cli.command,
        None | Some(
            Command::Module { .. } | Command::Play(_) | Command::Host(_) | Command::Join(_)
        )
    ) {
        installs::warm(&discover());
    }
    if let Some(id) = cli.load {
        if cli.command.is_some() || !saves::valid_id(&id) {
            eprintln!("--load {id}: a save's id (quick, 0001, k1-03, …), with no other command");
            return ExitCode::from(2);
        }
        session_flow::load_at_start(id);
    }
    if cli.back_to_menu {
        session_flow::came_back();
    }
    match cli.command {
        #[cfg(feature = "probes")]
        Some(Command::Verb(verb)) => verbs::run(verb, cli.size),
        // A plain start (no script, no save, not back from a game) whose
        // games are not all found, or that never had its setup, opens on
        // the setup.
        None if cli.cmds.is_none()
            && !cli.headless
            && !cli.back_to_menu
            && !session_flow::skips_logos()
            && setup::needed(&discover()) =>
        {
            play("", false, size, runtime::Scene::Setup { start: false })
        }
        None => play(
            cli.cmds.as_deref().unwrap_or(""),
            cli.headless,
            size,
            runtime::Scene::Empty,
        ),
        Some(Command::Setup { start }) => play(
            cli.cmds.as_deref().unwrap_or(""),
            cli.headless,
            size,
            runtime::Scene::Setup { start },
        ),
        Some(Command::Module {
            game,
            module,
            cmds,
            headless,
        }) => match viewer::load_module(&discover(), &game, &module) {
            Ok(loaded) => play(
                cmds.as_deref().unwrap_or(""),
                headless,
                size,
                runtime::Scene::Loaded(Box::new(loaded)),
            ),
            Err(error) => {
                eprintln!("{error}");
                ExitCode::FAILURE
            }
        },
        Some(Command::View {
            game,
            model,
            module,
            clip,
            saber,
            cmds,
            headless,
        }) => match if game.eq_ignore_ascii_case("ja") {
            viewer_ja::load(&discover(), &model, clip.as_deref(), saber.as_deref())
        } else {
            viewer::load_model(&discover(), &game, &model, module.as_deref())
        } {
            Ok(loaded) => play(
                cmds.as_deref().unwrap_or(""),
                headless,
                size,
                runtime::Scene::Loaded(Box::new(loaded)),
            ),
            Err(error) => {
                eprintln!("{error}");
                ExitCode::FAILURE
            }
        },
        Some(Command::Rig {
            probe,
            body,
            head,
            moves,
            saber,
            kotor_clip,
            trace,
            hilt,
            color,
            quirks,
            cmds,
            headless,
        }) => {
            let options = viewer_rig::Options {
                probe: &probe,
                body: &body,
                head: &head,
                moves: &moves,
                saber: &saber,
                kotor_clip: &kotor_clip,
                trace: trace.as_deref(),
                kotor_hilt: !hilt.eq_ignore_ascii_case("ja"),
                colors: color.split(',').map(str::trim).collect(),
                quirks,
            };
            match viewer_rig::load(&discover(), &options) {
                Ok(loaded) => play(
                    cmds.as_deref().unwrap_or(""),
                    headless,
                    size,
                    runtime::Scene::Loaded(Box::new(loaded)),
                ),
                Err(error) => {
                    eprintln!("{error}");
                    ExitCode::FAILURE
                }
            }
        }
        Some(Command::Games) => games(),
        Some(Command::Modules { game, module }) => {
            modules::run(&discover(), &game, module.as_deref())
        }
        Some(Command::Ls {
            game,
            pattern,
            module,
            shadowed,
        }) => resources::ls(
            &discover(),
            &game,
            pattern.as_deref(),
            module.as_deref(),
            shadowed,
        ),
        Some(Command::Fingerprint { game, no_cache }) => {
            fingerprint(&discover(), game.as_deref(), no_cache)
        }
        Some(Command::Show {
            game,
            resource,
            module,
        }) => show::run(&discover(), &game, &resource, module.as_deref()),
        Some(Command::Rules { game, out }) => rules_page::run(&discover(), &game, out),
        Some(Command::Texture {
            game,
            resref,
            module,
            out,
        }) => texture_export::run(&discover(), &game, &resref, module.as_deref(), out),
        Some(Command::Extract {
            game,
            resource,
            module,
            out,
        }) => resources::extract(&discover(), &game, &resource, module.as_deref(), out),
        Some(Command::Import {
            game,
            force,
            module,
        }) => import_verbs::import(&discover(), &game, force, &module),
        Some(Command::Movie { verb }) => match verb {
            MovieVerb::Check => movie_verbs::check(),
            MovieVerb::Import { game, names } => movie_verbs::import(&discover(), &game, &names),
        },
        Some(Command::Report { verb }) => match verb {
            ReportVerb::Show { zip } => report_verbs::show(&zip),
            ReportVerb::Replay {
                zip,
                to_tick,
                headless,
                ignore_data,
            } => report_verbs::replay(&zip, to_tick, headless, ignore_data),
        },
        Some(Command::Store { verb }) => match verb {
            StoreVerb::Path => store_verbs::path(),
            StoreVerb::Du => store_verbs::du(),
            StoreVerb::Gc { days, dry_run } => store_verbs::gc(days, dry_run),
            StoreVerb::Check => store_verbs::check(),
        },
        Some(Command::Play(args)) => cli_play::run(*args, size),
        Some(Command::Host(args)) => cli_host::run(*args, size),
        Some(Command::Join(args)) => cli_join::run(args, size),
    }
}

/// Runs `script` in a window that opens at once on the loading screen (of
/// `module`, when known) while `load` makes the session on a thread
/// (`session_flow::launch`): the script starts once it is in, and a session
/// that does not load ends the run with why.
fn launch(
    script: &str,
    headless: bool,
    size: (u32, u32),
    module: Option<&str>,
    load: impl FnOnce() -> Result<play::Loaded, String> + Send,
) -> ExitCode {
    let (launching, launched) = session_flow::launch(module);
    std::thread::scope(|scope| {
        scope.spawn(|| launching.ready(load()));
        play(script, headless, size, runtime::Scene::Launch(launched))
    })
}

fn play(script: &str, headless: bool, size: (u32, u32), scene: runtime::Scene) -> ExitCode {
    let commands = match console::parse(script) {
        Ok(commands) => commands,
        Err(error) => {
            eprintln!("--cmds: {error}");
            return ExitCode::from(2);
        }
    };
    let options = runtime::RunOptions {
        commands,
        script: script.to_string(),
        headless,
        size,
        games: Box::new(|| games_manifest(&discover())),
        scene,
    };
    let exit = runtime::run(options);
    if setup::finished() && !headless {
        let error = setup::restart();
        eprintln!("on to the main menu: {error}");
    }
    if session_flow::to_menu() && !headless {
        let error = session_flow::restart_into_menu();
        eprintln!("back to the main menu: {error}");
    }
    match exit {
        AppExit::Success => ExitCode::SUCCESS,
        AppExit::Error(code) => ExitCode::from(code.get()),
    }
}

fn fingerprint(discovery: &vfs::Discovery, game: Option<&str>, no_cache: bool) -> ExitCode {
    let games: Vec<Game> = match game {
        None => Game::ALL.to_vec(),
        Some(key) => match Game::from_key(key) {
            Some(game) => vec![game],
            None => {
                eprintln!("unknown game {key:?}: use k1, tsl or ja");
                return ExitCode::from(2);
            }
        },
    };
    let mut failed = false;
    for game in games {
        let Some(install) = discovery.get(game) else {
            outln!("{}: not found (set {})", game.key(), game.env_var());
            continue;
        };
        let started = std::time::Instant::now();
        let cache = fingerprint_cache(game);
        let result = DirIndex::build(&install.data_root).and_then(|index| {
            vfs::fingerprint::fingerprint(game, &index, (!no_cache).then_some(cache.as_path()))
        });
        match result {
            Ok(print) => {
                outln!("{} {}", game.key(), print.digest);
                outln!(
                    "  {} files, {} bytes; {} hashed, {} from cache; {:.1} s",
                    print.files,
                    print.bytes,
                    print.hashed,
                    print.cached,
                    started.elapsed().as_secs_f64()
                );
                for (group, (digest, count)) in &print.groups {
                    outln!("  {group}: {} ({count} files)", &digest[..16]);
                }
            }
            Err(error) => {
                outln!("{}: {error}", game.key());
                failed = true;
            }
        }
    }
    if failed {
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    }
}

fn games() -> ExitCode {
    let discovery = discover();
    for game in Game::ALL {
        let Some(install) = discovery.get(game) else {
            outln!("{}: not found (set {})", game.key(), game.env_var());
            continue;
        };
        let source = match &install.source {
            Source::Env(var) => format!("env {var}"),
            Source::SteamLibrary(library) => format!("steam library {}", library.display()),
            Source::GogDefault => "gog default".to_string(),
        };
        outln!("{}: {}", game.key(), install.root.display());
        outln!("    source: {source}");
        outln!("    store: {:?}", install.store);
        outln!("    data root: {}", install.data_root.display());
        outln!("    markers: {}", install.markers.join(" "));
        match DirIndex::build(&install.data_root) {
            Ok(index) => {
                outln!("    files: {}", index.len());
                for collision in index.collisions() {
                    outln!(
                        "    case collision: {} (kept {}, ignored {})",
                        collision.key,
                        collision.kept.display(),
                        collision.ignored.display()
                    );
                }
            }
            Err(error) => outln!("    files: cannot index: {error}"),
        }
    }
    for refusal in &discovery.refusals {
        outln!(
            "refused {} at {} ({:?}): {}",
            refusal.game.key(),
            refusal.path.display(),
            refusal.source,
            refusal.reason
        );
    }
    ExitCode::SUCCESS
}
