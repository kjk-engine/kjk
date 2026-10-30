//! The verbs that check and measure rather than play (`kjk sim`,
//! `kjk collision`, `kjk combat`, `kjk script`, ...): the `probes`
//! feature, on by default. A build without it (`--no-default-features`)
//! is the game alone.

use std::process::ExitCode;

use clap::Subcommand;
use vfs::Game;

use crate::*;

#[derive(Subcommand)]
pub(crate) enum Verb {
    /// Read every archive of an install and report what failed to parse.
    Check {
        /// k1, tsl or ja
        game: String,
    },
    /// Parse every copy of every data file and check write-back where supported.
    Formats {
        /// k1 or tsl
        game: String,
    },
    /// Render a tour of viewpoints headless into a contact sheet and compare
    /// with its previous run (default size 800x600).
    Tour {
        /// The tour file, e.g. tours/showcase.tour
        file: std::path::PathBuf,
        /// Only the stop, or the module, with this name
        #[arg(long)]
        only: Option<String>,
    },
    /// The whole-game check: every module of K1 and TSL (or one game)
    /// built, its scripts stepped headless, a frame rendered from its entry
    /// and its containment audited; a gallery page ranked worst first, bug
    /// lists, contact sheets and facts.json in one run folder (default
    /// frame size 640x360).
    Sweep {
        /// k1, tsl or all
        #[arg(default_value = "all")]
        game: String,
        /// Seconds of scripts stepped in each module
        #[arg(long, default_value_t = 10)]
        seconds: u64,
        /// Only the modules whose names start so
        #[arg(long)]
        only: Option<String>,
        /// Modules built, rendered and audited at once
        #[arg(long, default_value_t = 4)]
        jobs: usize,
        /// Stages to leave out, comma-separated: scripts, render, audit
        #[arg(long, default_value = "")]
        skip: String,
    },
    /// Which basic clips every creature finds through its supermodel chain.
    Clips {
        /// k1 or tsl
        game: String,
    },
    /// How every module's rooms, doors and placeables would be drawn:
    /// surface kinds, lighting paths, env maps, and what is not handled.
    Materials {
        /// k1 or tsl
        game: String,
    },
    /// KOTOR's screens without a window (I21): the UI's probes, one layout
    /// drawn to a PNG, a sweep of every layout, a contact sheet.
    Gui {
        #[command(subcommand)]
        verb: GuiVerb,
    },
    /// The sim funnel (I11): determinism, record and replay, saves, tick
    /// rates, and a live view that reads snapshots only.
    Sim {
        #[command(subcommand)]
        verb: SimVerb,
    },
    /// A module's collision (I12): what it is built from, trace probes,
    /// and the containment audit.
    Collision {
        #[command(subcommand)]
        verb: CollisionVerb,
    },
    /// Player movement (I13): JA's movement on the collision contract,
    /// measured and walked without a window.
    Move {
        #[command(subcommand)]
        verb: MoveVerb,
    },
    /// Module installs (I14): plans, commits, announcements, and runs that
    /// go between modules and count what is left behind.
    Lifecycle {
        #[command(subcommand)]
        verb: LifecycleVerb,
    },
    /// Combat (I23): a module's creatures as combat reads them, and a
    /// fight stepped without a window.
    Combat {
        #[command(subcommand)]
        verb: CombatVerb,
    },
    /// The network layer (I15): its probes and the snapshot codec's
    /// measures.
    Net {
        #[command(subcommand)]
        verb: NetVerb,
    },
    /// A module played headless goal by goal (I29): walking to what a goal
    /// names, using it, answering, fighting, on into the next module; a
    /// goal not met in time is a blocker (`playthrough.rs`)
    Playthrough {
        /// k1 or tsl
        game: String,
        module: String,
        /// The goals, `;` or new lines between them, or a file of them
        route: String,
        /// Arrive at this door or waypoint (by tag)
        #[arg(long)]
        arrive: Option<String>,
        /// The player's sheet, as `kjk play --class`
        #[arg(long, default_value = "soldier")]
        class: String,
        /// A script global set before the first step, NAME=NUMBER;
        /// repeatable
        #[arg(long = "global", value_parser = by_hand::parse_global)]
        globals: Vec<(String, i32)>,
        /// A script of the first module run for the player at the end of
        /// the first step, as `kjk play --run`; repeatable
        #[arg(long = "run")]
        scripts: Vec<String>,
        /// The first module's creatures with this tag in the party, as
        /// `kjk play --follow`; repeatable
        #[arg(long = "follow")]
        followers: Vec<String>,
        /// How long each goal may take (seconds of play)
        #[arg(long, default_value_t = 90)]
        goal_seconds: u64,
        /// The player never drops below 1 HP (the sheet's immortal flag): a
        /// walk that tests what the scripts do, not the fights' balance
        #[arg(long)]
        god: bool,
        /// The player's blows count ten times over: a playtest's fights end
        /// quickly (a test aid, as `--god`)
        #[arg(long)]
        mighty: bool,
        /// Starts from a checkpoint made with other level data than this
        /// build's (a dev aid while the chain is remade: the checkpoint's
        /// module takes this build's data, the other modules it left are
        /// met anew; not the run the chain would have made)
        #[arg(long)]
        stale: bool,
        /// Every conversation line heard out (by default the player clicks
        /// past each, as a player skips dialogue)
        #[arg(long)]
        listen: bool,
        /// The player's rank in a skill, NAME=RANK, as `kjk play --skill`;
        /// repeatable
        #[arg(long = "skill", value_parser = by_hand::parse_skill)]
        skills: Vec<(usize, u8)>,
        #[command(flatten)]
        blocks: Blocks,
        /// The folder the route's `save` goals write their checkpoints to
        /// (default kjk-artifacts/checkpoints/GAME)
        #[arg(long)]
        save_to: Option<std::path::PathBuf>,
        /// `chaos`: two phantom guests fly remote droids at random, every
        /// button of their kit pressed; the story kept as a plain run's
        #[arg(long, value_parser = ["chaos"])]
        droids: Option<String>,
        /// A build file (`playthrough::build`): the character a new game
        /// makes through character creation, and the plan its level-ups
        /// follow
        #[arg(long)]
        build: Option<std::path::PathBuf>,
    },
    /// A game's routes (`data/routes/<game>/`) played through as the chain
    /// they make, each from the checkpoint its header names, the branches
    /// beside the main way (`chain.rs`); logs, checkpoints and facts.json in
    /// one run folder, the checkpoints of each route met kept for the next
    /// run, an identical route's rerun taken from the cache
    Chain {
        /// k1 or tsl
        game: String,
        /// Routes by file name, each with every route after it; none, all
        routes: Vec<String>,
        /// The routes named (none: all) each alone, from the checkpoint kept
        /// for it (the last met run of the route before it saved it), all at
        /// once as the jobs allow; `--quick` with none named: every route
        /// so, a signal in minutes (the serial chain stays the merge gate)
        #[arg(long, visible_alias = "quick")]
        alone: bool,
        /// Playthroughs at once (default: half the cores)
        #[arg(long)]
        jobs: Option<usize>,
        #[command(flatten)]
        blocks: Blocks,
        /// How long each goal may take (default: the playthrough's)
        #[arg(long)]
        goal_seconds: Option<u64>,
        /// Every route with `--mighty`, beside its header's flags
        #[arg(long)]
        mighty: bool,
        /// Every route with this skill rank, NAME=RANK, as `playthrough
        /// --skill`; repeatable
        #[arg(long = "skill")]
        skills: Vec<String>,
        /// Without the game's chain flags (`data/routes/GAME/chain.flags`,
        /// k1's `--mighty --skill security=2`): each route with its
        /// header's flags and those given here only
        #[arg(long)]
        bare: bool,
        /// Every route played, none taken from the cache of identical runs
        /// (same binary, game data, start checkpoint, route and flags)
        #[arg(long)]
        no_cache: bool,
        /// Each route's command and whether the cache has its outcome;
        /// nothing played
        #[arg(long)]
        dry_run: bool,
        /// The checkpoints `data/routes/GAME/saves.list` names written as
        /// saves (k1-01, k1-02, ...) from a worktree too; the main checkout
        /// always writes them
        #[arg(long)]
        saves: bool,
        /// `chaos`: every route alone, from the plain chain's checkpoint,
        /// with two chaos droids; its story changes at every module left,
        /// against the last plain chain's, listed for review
        #[arg(long, value_parser = ["chaos"])]
        droids: Option<String>,
    },
    /// Conversations (I18): a module's DLGs, and conversations run headless
    Dialogue {
        #[command(subcommand)]
        verb: DialogueVerb,
    },
    /// Scripts (I16): the games' compiled scripts and the routines they call
    Script {
        #[command(subcommand)]
        verb: ScriptVerb,
    },
    /// The co-op remote droid: its duel census (every module's scripted
    /// rounds and whether a protection the droid's gate reads covers each)
    Remote {
        #[command(subcommand)]
        verb: RemoteVerb,
    },
    /// Launches `kjk play` headless a few times and tables where each
    /// launch's time went (its loading phases, and when it was loaded, built,
    /// spawned and warmed), for finding and watching load costs
    LaunchReport {
        /// The stage, as `play --stage` takes it
        #[arg(long, default_value = "end_m01aa")]
        stage: String,
        /// How many launches
        #[arg(long, default_value_t = 3)]
        runs: usize,
    },
    /// A sim package driven through its public API, one record a step,
    /// floats as bits (`docs/EQUIVALENCE.md`): from scenario files (or folders
    /// of them), or `random`, the package's seeded generator
    Trace {
        /// m1 movement, m2 saber move table, m3 weapon step (m4, m5 to come)
        pkg: String,
        /// Scenario files or folders, or `random`
        #[arg(required = true)]
        source: Vec<std::path::PathBuf>,
        /// The first seed of `random`
        #[arg(long, default_value_t = 1)]
        seed: u64,
        /// Seeds of `random` in a row from `--seed`
        #[arg(long, default_value_t = 1)]
        seeds: u64,
        /// Steps (queries) per seed of `random`
        #[arg(long, default_value_t = 10_000)]
        n: u64,
        /// The trace file written
        #[arg(long)]
        out: std::path::PathBuf,
        /// Also write the scenarios run as a scenario file (a random
        /// divergence kept as a named scenario)
        #[arg(long)]
        emit: Option<std::path::PathBuf>,
    },
    /// The first difference between two traces (expected, actual): its
    /// scenario, step, field and the ten records before it; fails on any
    TraceDiff {
        expected: std::path::PathBuf,
        actual: std::path::PathBuf,
    },
}

/// Where a run stops: at its first blocked goal, by default (each goal
/// after it would spend `--goal-seconds`, and in a chain no route starts
/// after it), or not.
#[derive(clap::Args)]
pub(crate) struct Blocks {
    /// Play on past a blocked goal: every goal after it tried, and in a
    /// chain every route whose checkpoint was saved (by default a run stops
    /// at its first block)
    #[arg(long, overrides_with = "fail_fast")]
    keep_going: bool,
    /// The default (kept so older command lines run); undoes an earlier
    /// `--keep-going`
    #[arg(long, hide = true, overrides_with = "keep_going")]
    fail_fast: bool,
}

impl Blocks {
    pub(crate) fn fail_fast(&self) -> bool {
        self.fail_fast || !self.keep_going
    }
}

pub(crate) fn run(verb: Verb, size: Option<(u32, u32)>) -> ExitCode {
    match verb {
        Verb::Trace {
            pkg,
            source,
            seed,
            seeds,
            n,
            out,
            emit,
        } => {
            let options = sim::trace::Options {
                seed,
                seeds,
                n,
                emit,
            };
            trace_verbs::trace(&pkg, &source, &options, &out)
        }
        Verb::TraceDiff { expected, actual } => trace_verbs::diff(&expected, &actual),
        Verb::Check { game } => check::run(&discover(), &game),
        Verb::Formats { game } => formats::run(&discover(), &game),
        Verb::Materials { game } => materials::run(&discover(), &game),
        Verb::Clips { game } => clips::run(&discover(), &game),
        Verb::Sweep {
            game,
            seconds,
            only,
            jobs,
            skip,
        } => {
            let games = match game.as_str() {
                "all" => vec![Game::Kotor1, Game::Kotor2],
                key => match Game::from_key(key).filter(|game| *game != Game::JediAcademy) {
                    Some(game) => vec![game],
                    None => {
                        eprintln!("{key}: use k1, tsl or all");
                        return ExitCode::from(2);
                    }
                },
            };
            let skip: Vec<&str> = skip.split(',').map(str::trim).collect();
            if let Some(stage) = skip
                .iter()
                .find(|stage| !["", "scripts", "render", "audit"].contains(stage))
            {
                eprintln!("--skip {stage}: use scripts, render or audit");
                return ExitCode::from(2);
            }
            game_sweep::sweep(
                &discover(),
                &game_sweep::Options {
                    games,
                    seconds,
                    only,
                    size: size.unwrap_or((640, 360)),
                    jobs,
                    scripts: !skip.contains(&"scripts"),
                    render: !skip.contains(&"render"),
                    audit: !skip.contains(&"audit"),
                },
            )
        }
        Verb::Tour { file, only } => tour::run(
            &discover(),
            &file,
            size.unwrap_or((800, 600)),
            only.as_deref(),
        ),
        Verb::Gui { verb } => match verb {
            GuiVerb::Probes => gui_verbs::probes(),
            GuiVerb::Render {
                game,
                name,
                out,
                skinned,
            } => gui_verbs::render(
                &discover(),
                &game,
                &name,
                skinned,
                size.unwrap_or((1280, 960)),
                out,
            ),
            GuiVerb::Screen {
                game,
                name,
                steps,
                out,
                file,
                error,
            } => gui_verbs::screen(
                &discover(),
                (&game, &name),
                (&steps, error),
                size.unwrap_or((1280, 960)),
                (out, file.as_deref()),
            ),
            GuiVerb::Sweep { game } => gui_verbs::sweep(&discover(), game.as_deref()),
            GuiVerb::Sheet { game, out } => gui_verbs::sheet(&discover(), &game, out),
        },
        Verb::Sim { verb } => sim(verb, size.unwrap_or(scene::HEADLESS_SIZE)),
        Verb::Playthrough {
            game,
            module,
            route,
            arrive,
            class,
            globals,
            scripts,
            followers,
            goal_seconds,
            god,
            mighty,
            stale,
            listen,
            skills,
            blocks,
            save_to,
            droids,
            build,
        } => {
            let build = match build.as_deref().map(std::fs::read_to_string).transpose() {
                Ok(build) => build,
                Err(error) => {
                    eprintln!("--build: {error}");
                    return ExitCode::from(2);
                }
            };
            // A file of goals, or the goals themselves.
            let path = std::path::Path::new(&route);
            let route = match std::fs::read_to_string(path) {
                Ok(text) => text,
                Err(error) if route.contains('/') || route.ends_with(".txt") => {
                    eprintln!("{route}: {error}");
                    return ExitCode::from(2);
                }
                Err(_) => route,
            };
            let god = if god {
                sim::combat::sheet::flags::IMMORTAL
            } else {
                0
            } | if mighty {
                sim::combat::sheet::flags::MIGHTY
            } else {
                0
            };
            match player_sheet(&class).map(|sheet| sim::combat::Sheet {
                flags: sheet.flags | god,
                ..by_hand::with_skills(sheet, &skills)
            }) {
                Ok(sheet) => playthrough::run(
                    &discover(),
                    &playthrough::Options {
                        game,
                        module,
                        arrive,
                        route,
                        sheet,
                        globals,
                        scripts,
                        followers,
                        goal_seconds,
                        stale,
                        listen,
                        fail_fast: blocks.fail_fast(),
                        save_to,
                        droids: droids.is_some(),
                        build,
                    },
                ),
                Err(error) => {
                    eprintln!("{error}");
                    ExitCode::from(2)
                }
            }
        }
        Verb::Chain {
            game,
            routes,
            alone,
            jobs,
            blocks,
            goal_seconds,
            mighty,
            skills,
            bare,
            no_cache,
            dry_run,
            saves,
            droids,
        } => chain::run(
            &discover(),
            &chain::Options {
                game,
                routes,
                alone,
                jobs: jobs.unwrap_or_else(|| {
                    std::thread::available_parallelism().map_or(2, |cores| cores.get() / 2)
                }),
                fail_fast: blocks.fail_fast(),
                goal_seconds,
                mighty,
                skills,
                bare,
                cache: !no_cache,
                dry_run,
                saves,
                droids: droids.is_some(),
            },
        ),
        Verb::Dialogue { verb } => match verb {
            DialogueVerb::Show {
                game,
                module,
                dialog,
            } => dialogue_verbs::show(&discover(), &game, &module, dialog.as_deref()),
            DialogueVerb::Run {
                game,
                module,
                seconds,
                picks,
                arrive,
                globals,
                made,
                by_hand,
            } => {
                let ByHand {
                    skills,
                    items,
                    talk,
                    party,
                } = *by_hand;
                dialogue_verbs::run(
                    &discover(),
                    &game,
                    &module,
                    &dialogue_verbs::Run {
                        seconds,
                        picks,
                        arrive,
                        globals,
                        talk,
                        skills,
                        items,
                        made,
                        party,
                        read_ms: 0,
                    },
                )
            }
            DialogueVerb::Sweep { game, module } => {
                scene_sweep::sweep(&discover(), &game, &module, size)
            }
            DialogueVerb::Motion { game, modules } => {
                crate::motion::sweep(&discover(), &game, &modules)
            }
            DialogueVerb::Check {
                game,
                modules,
                jobs,
                report,
            } => scene_check::check(
                &discover(),
                &game,
                &modules,
                jobs.unwrap_or_else(|| {
                    std::thread::available_parallelism().map_or(4, |n| (n.get() / 2).max(1))
                }),
                report.as_deref(),
            ),
        },
        Verb::Remote {
            verb: RemoteVerb::Census { game },
        } => remote_census::census(&discover(), &game),
        Verb::LaunchReport { stage, runs } => launch_report::launch_report(&stage, runs),
        Verb::Script { verb } => match verb {
            ScriptVerb::Dis {
                game,
                script,
                module,
            } => script_verbs::dis(&discover(), &game, &script, module.as_deref()),
            ScriptVerb::Check { game } => script_verbs::check(&discover(), &game),
            ScriptVerb::Probes => script_verbs::probes(),
            ScriptVerb::Effects { game, module } => {
                effects_verbs::effects(&discover(), &game, &module)
            }
            ScriptVerb::Items { game, module } => {
                inventory_verbs::items(&discover(), &game, &module)
            }
            ScriptVerb::Loot { game, module } => loot_verbs::loot(&discover(), &game, &module),
            ScriptVerb::Party { game, module } => party_verbs::party(&discover(), &game, &module),
            ScriptVerb::Equip {
                game,
                module,
                container,
            } => equip_verbs::equip(&discover(), &game, &module, &container),
            ScriptVerb::Security {
                game,
                module,
                door,
                ranks,
            } => security_verbs::security(&discover(), &game, &module, door.as_deref(), ranks),
            ScriptVerb::Traps { game, module } => traps_verbs::traps(&discover(), &game, &module),
            ScriptVerb::Stores {
                game,
                module,
                opens,
            } => merchant_verbs::stores(&discover(), &game, &module, &opens),
            ScriptVerb::Journal { game, module } => {
                journal_verbs::journal(&discover(), &game, &module)
            }
            ScriptVerb::Levels { game, module } => {
                levels_verbs::levels(&discover(), &game, &module)
            }
            ScriptVerb::Xp { game, module } => xp_verbs::xp(&discover(), &game, &module),
            ScriptVerb::Routines { game, module } => {
                routines_verbs::routines(&discover(), &game, &module)
            }
            ScriptVerb::Fighting { game, module } => {
                fight_routines_verbs::fighting(&discover(), &game, &module)
            }
            ScriptVerb::Sweep {
                game,
                seconds,
                only,
            } => sweep_verbs::sweep(&discover(), &game, seconds, only.as_deref()),
            ScriptVerb::Impacts {
                game,
                levels,
                modifiers,
            } => {
                let casters: Vec<impact_verbs::Caster> = levels
                    .iter()
                    .zip(&modifiers)
                    .map(|(&level, &modifier)| impact_verbs::Caster { level, modifier })
                    .collect();
                impact_verbs::impacts(&discover(), &game, &casters)
            }
            ScriptVerb::Coverage {
                game,
                module,
                seconds,
                trace,
                walk,
                frames,
                jedi,
            } => {
                let walk = match walk.as_deref() {
                    None => None,
                    Some(&[x, y]) => Some([x, y]),
                    Some(_) => {
                        eprintln!("--walk takes x,y");
                        return ExitCode::FAILURE;
                    }
                };
                let options = script_verbs::Coverage {
                    seconds,
                    trace,
                    walk,
                    frames,
                    jedi,
                };
                script_verbs::coverage(&discover(), &game, &module, &options)
            }
            ScriptVerb::Run { game, only } => {
                script_verbs::run(&discover(), &game, only.as_deref())
            }
        },
        Verb::Move { verb } => match verb {
            MoveVerb::Run {
                stage,
                arrive,
                moves,
            } => {
                let discovery = discover();
                let (table, note) = stage::anims(&discovery);
                match stage::Stage::named(
                    &discovery,
                    &stage,
                    arrive.as_deref(),
                    std::sync::Arc::new(table),
                    false,
                    1,
                    1,
                ) {
                    Ok(stage) => move_verbs::run(&stage, &note, &moves),
                    Err(error) => {
                        eprintln!("{error}");
                        ExitCode::from(2)
                    }
                }
            }
            MoveVerb::Measure => move_verbs::measure(stage::anims(&discover())),
            MoveVerb::Trig => move_verbs::trig(),
            MoveVerb::Pace {
                game,
                only,
                seconds,
                follow,
            } => pace_verbs::pace(&discover(), &game, seconds, only.as_deref(), &follow),
        },
        Verb::Collision { verb } => collision(verb),
        Verb::Combat { verb } => match verb {
            CombatVerb::Roster { game, module } => {
                combat_verbs::roster(&discover(), &game, &module)
            }
            CombatVerb::Bodies { game } => combat_verbs::bodies(&discover(), &game),
            CombatVerb::Feats { game, module } => {
                feats_verbs::feats(&discover(), &game, &module)
            }
            CombatVerb::Fight {
                game,
                module,
                arrive,
                seconds,
                hostile,
                ally,
            } => combat_verbs::fight(
                &discover(),
                &game,
                &module,
                arrive.as_deref(),
                seconds,
                &hostile,
                &ally,
            ),
            CombatVerb::Force {
                game,
                module,
                target,
                drained,
            } => force_verbs::force(&discover(), &game, &module, &target, &drained),
            CombatVerb::Damage { game, module } => {
                damage_verbs::damage(&discover(), &game, &module)
            }
            CombatVerb::Grenades {
                game,
                module,
                target,
            } => grenades_verbs::grenades(&discover(), &game, &module, &target),
            CombatVerb::Items {
                game,
                module,
                target,
            } => items_verbs::items(&discover(), &game, &module, &target),
            CombatVerb::Worked => worked_verbs::worked(&discover()),
            CombatVerb::Stealth { game, module } => {
                stealth_verbs::stealth(&discover(), &game, &module)
            }
            CombatVerb::Specials {
                game,
                module,
                target,
            } => specials_verbs::specials(&discover(), &game, &module, &target),
            CombatVerb::Lag {
                game,
                module,
                arrive,
                runner,
            } => lag_verbs::lag(&discover(), &game, &module, Some(&arrive), &runner),
            CombatVerb::Encounters {
                game,
                module,
                players,
            } => encounter_verbs::encounters(&discover(), &game, &module, players),
            CombatVerb::Ai {
                game,
                module,
                arrive,
                seconds,
                hostile,
                companion,
            } => ai_verbs::ai(
                &discover(),
                &game,
                &module,
                arrive.as_deref(),
                seconds,
                &hostile,
                &companion,
            ),
            CombatVerb::Saber => saber_verbs::saber_moves(&discover()),
            CombatVerb::Duel { only } => duel_verbs::duel(&discover(), &only),
            CombatVerb::DuelLag { seconds, seeds } => {
                duel_lag_verbs::duel_lag(&discover(), seconds, seeds)
            }
        },
        Verb::Net { verb } => match verb {
            NetVerb::Probes => net_verbs::probes(),
            NetVerb::Feel { host, player } => net_feel_verb::feel_verb(&host, &player),
            NetVerb::Margins { seconds, rtt, only } => {
                net_margins_verb::margins(seconds, &rtt, only.as_deref())
            }
            NetVerb::Codec => net_codec_verb::codec(&discover()),
            NetVerb::Bandwidth {
                recording,
                every,
                back,
                sample,
            } => net_bandwidth_verb::bandwidth(&discover(), &recording, (every, &back, sample)),
            NetVerb::Session { only, soak_minutes } => {
                net_session_verb::session(&discover(), only.as_deref(), soak_minutes)
            }
        },
        Verb::Lifecycle { verb } => match verb {
            LifecycleVerb::Plan {
                game,
                module,
                arrive,
            } => lifecycle_verbs::plan_verb(&discover(), &game, &module, arrive.as_deref()),
            LifecycleVerb::Cycle {
                game,
                a,
                b,
                times,
                ticks,
            } => lifecycle_verbs::cycle_verb(&discover(), &game, &a, &b, times, ticks),
        },
    }
}

#[derive(Subcommand)]
pub(crate) enum GuiVerb {
    /// The UI's checks on layouts, fonts and screens built here.
    Probes,
    /// Draw a layout as the game would, every control at rest.
    Render {
        /// k1 or tsl
        game: String,
        /// The layout, without `.gui`
        name: String,
        /// Output folder; defaults to kjk-artifacts/gui/<game>
        #[arg(long)]
        out: Option<std::path::PathBuf>,
        /// The layout the game's screen draws: KOTOR 2's in its place.
        #[arg(long)]
        skinned: bool,
    },
    /// Draw a screen as play opens it on the game's own data (character
    /// creation's choices from its tables), after the steps given
    Screen {
        /// k1 or tsl
        game: String,
        /// The screen, as play opens it (`chargen:solo`, `options`, ...)
        name: String,
        /// Steps in turn, each `press TAG`, `type TEXT` or `scroll N` (wheel notches down at the window middle)
        #[arg(long = "step")]
        steps: Vec<String>,
        /// Output folder; defaults to kjk-artifacts/gui/<game>
        #[arg(long)]
        out: Option<std::path::PathBuf>,
        /// The PNG's name; the screen's by default
        #[arg(long)]
        file: Option<String>,
        /// Why a session did not start, as `message` shows it
        #[arg(long)]
        error: Option<String>,
    },
    /// Parse and paint every layout of each install (or one) and count
    /// what does not resolve.
    Sweep {
        /// k1 or tsl; both by default
        game: Option<String>,
    },
    /// Every layout of an install on one contact sheet.
    Sheet {
        /// k1 or tsl
        game: String,
        /// Output folder; defaults to kjk-artifacts/gui/<game>
        #[arg(long)]
        out: Option<std::path::PathBuf>,
    },
}

/// The placeholder world a sim verb starts from, and its scripted commands.
#[derive(clap::Args, Clone, Copy)]
pub(crate) struct SimWorldArgs {
    /// Seeds the world's random streams
    #[arg(long, default_value_t = 1)]
    seed: u64,
    /// Seeds the scripted commands
    #[arg(long, default_value_t = 7)]
    pilot: u64,
    /// Seats (avatars), each driven by scripted commands (at most 64)
    #[arg(long, default_value_t = 4, value_parser = parse_seats)]
    seats: u8,
    /// Wanderers at the start
    #[arg(long, default_value_t = 32)]
    wanderers: u32,
    /// Ticks per second (default: sim::TICK_HZ)
    #[arg(long, default_value_t = sim::TICK_HZ, value_parser = parse_hz)]
    hz: u32,
}

impl SimWorldArgs {
    fn setup(self) -> sim::determinism::Setup {
        sim::determinism::Setup {
            world: sim::WorldConfig {
                seed: self.seed,
                seats: self.seats,
                wanderers: self.wanderers,
                spawn: [0.0; 3],
                yaw: 90.0,
                sheet: sim::combat::Sheet::dev_soldier(),
                guests: sim::Guests::Avatars,
            },
            rate: sim::TickRate::new(self.hz).expect("checked by parse_hz"),
            pilot: self.pilot,
        }
    }
}

/// Which module a collision verb builds, and its clip brushes.
#[derive(clap::Args, Clone)]
pub(crate) struct CollisionModule {
    /// k1 or tsl
    game: String,
    /// Module, e.g. end_m01aa (its entry area is built); for `build`,
    /// `all` builds every module
    module: String,
    /// Clip brushes from this file instead of the built-in one
    #[arg(long)]
    clip: Option<std::path::PathBuf>,
    /// No clip brushes at all
    #[arg(long, conflicts_with = "clip")]
    no_clip: bool,
}

impl CollisionModule {
    fn clip(&self) -> collision_verbs::ClipChoice<'_> {
        match (&self.clip, self.no_clip) {
            (_, true) => collision_verbs::ClipChoice::None,
            (Some(path), _) => collision_verbs::ClipChoice::File(path),
            (None, false) => collision_verbs::ClipChoice::BuiltIn,
        }
    }
}

#[derive(Subcommand)]
pub(crate) enum CollisionVerb {
    /// Build the level and report what went into it.
    Build {
        #[command(flatten)]
        module: CollisionModule,
    },
    /// Walk routes of the navigation mesh (I27) for real: a player steered
    /// by `sim::walk` through move_body, from a drawn face to another
    Walk {
        /// k1 or tsl
        game: String,
        /// Module, e.g. end_m01aa
        module: String,
        /// How many walks
        #[arg(long, default_value_t = 50)]
        routes: usize,
        /// The draw's seed
        #[arg(long, default_value_t = 1)]
        seed: u64,
    },
    /// The navigation mesh (I27): bake it from the walkable faces, route
    /// between drawn points of its largest island and check every route.
    Nav {
        #[command(flatten)]
        module: CollisionModule,
        /// How many routes
        #[arg(long, default_value_t = 500)]
        routes: usize,
        /// The draw's seed
        #[arg(long, default_value_t = 1)]
        seed: u64,
    },
    /// The containment audit: where a player gets off the walkmesh by
    /// walking and by each jump level; writes facts.json and audit.png.
    Audit {
        #[command(flatten)]
        module: CollisionModule,
        /// Grid cell, metres
        #[arg(long, default_value_t = 0.5)]
        cell: f32,
        /// Which doors are open: `openable` (not static, not a module
        /// transition), `closed` or `all`
        #[arg(long, default_value = "openable")]
        doors: String,
        /// Regions to print
        #[arg(long, default_value_t = 40)]
        show: usize,
        /// Also draw zoom.png around x,y,radius (metres)
        #[arg(long, value_parser = parse_zoom)]
        zoom: Option<[f32; 3]>,
        /// Where the flood starts: `starts` (the module's entry and
        /// waypoints) or `walkmesh` (every in-bounds spot: stricter)
        #[arg(long, default_value = "starts")]
        seeds: String,
    },
    /// One trace through the module's collision.
    Trace {
        #[command(flatten)]
        module: CollisionModule,
        /// Start, x,y,z metres (the traced shape's origin)
        #[arg(value_parser = parse_point)]
        from: [f32; 3],
        /// End, x,y,z metres
        #[arg(value_parser = parse_point)]
        to: [f32; 3],
        /// point, player (JA's standing box), body (that box round across, as a body moves), crouch or capsule
        #[arg(long, default_value = "player")]
        shape: String,
        /// Which doors are open: openable, closed or all
        #[arg(long, default_value = "closed")]
        doors: String,
        /// Trace again N times with both ends moved by dx,dy,dz each time:
        /// dx,dy,dz,N
        #[arg(long, value_parser = parse_repeat)]
        repeat: Option<([f32; 3], u32)>,
    },
    /// Probes with known answers: the clip on made-up levels (and against
    /// BoxWorld), then, given a module, traces through its collision.
    Probes {
        /// k1 or tsl
        game: Option<String>,
        /// Module, e.g. end_m01aa
        module: Option<String>,
        /// Clip brushes from this file instead of the built-in one
        #[arg(long)]
        clip: Option<std::path::PathBuf>,
        /// No clip brushes at all
        #[arg(long, conflicts_with = "clip")]
        no_clip: bool,
    },
}

fn doors_of(text: &str) -> Option<collision_verbs::Doors> {
    Some(match text {
        "openable" => collision_verbs::Doors::Openable,
        "closed" => collision_verbs::Doors::Closed,
        "all" => collision_verbs::Doors::All,
        _ => return None,
    })
}

#[derive(Subcommand)]
pub(crate) enum CombatVerb {
    /// Every creature of a module: sheet, faction, weapons, hostility.
    Roster {
        /// k1 or tsl
        game: String,
        /// Module, e.g. end_m01aa
        module: String,
    },
    /// Every appearance.2da row's body as shots, blades and the crosshair
    /// meet it: JA's box for a humanoid, else its model's box at rest, beside
    /// the model's drawn box.
    Bodies {
        /// k1 or tsl
        game: String,
    },
    /// Passive feats on the game's own characters: the party templates'
    /// HP, saves, skills, to hit in each hand, defence and what they may
    /// equip, and levels taken with Toughness.
    Feats {
        /// k1 or tsl
        game: String,
        /// Module whose plan is read, e.g. end_m01aa
        #[arg(default_value = "end_m01aa")]
        module: String,
    },
    /// Install a module for one player and let it fight: the player's seat
    /// shoots back at whoever targets it; tally the hits and deaths, and
    /// replay the recording.
    Fight {
        /// k1 or tsl
        game: String,
        /// Module, e.g. end_m01aa
        module: String,
        /// Door or waypoint tag to arrive at
        #[arg(long)]
        arrive: Option<String>,
        /// Sim seconds to step
        #[arg(long, default_value_t = 30)]
        seconds: u64,
        /// Turn the creatures with this tag hostile first (repeatable)
        #[arg(long)]
        hostile: Vec<String>,
        /// Turn the creatures with this tag to the player's side (repeatable)
        #[arg(long)]
        ally: Vec<String>,
    },
    /// Lag compensation (I25): a player shoots at a runner, aiming where it
    /// was L ms before, with and without its view lag in the input
    Lag {
        /// k1 or tsl
        game: String,
        /// Module, e.g. end_m01aa
        #[arg(default_value = "end_m01aa")]
        module: String,
        /// Door or waypoint tag to arrive at
        #[arg(long, default_value = "wp_end_weld01_02")]
        arrive: String,
        /// The creature that runs (a neutral one)
        #[arg(long, default_value = "end_bridgerep1")]
        runner: String,
    },
    /// Encounter overrides (I28): a module's creatures before and with its
    /// overrides for one to four players
    Encounters {
        /// k1 or tsl
        game: String,
        /// Module, e.g. end_m01ab
        module: String,
        /// Up to how many players
        #[arg(long, default_value_t = 4)]
        players: u8,
    },
    /// Minds (I27): a player walks toward the hostiles with companions;
    /// every mind's decisions are logged and checked
    Ai {
        /// k1 or tsl
        game: String,
        /// Module, e.g. end_m01aa
        #[arg(default_value = "end_m01aa")]
        module: String,
        /// Door or waypoint tag to arrive at
        #[arg(long)]
        arrive: Option<String>,
        /// Sim seconds to step
        #[arg(long, default_value_t = 30)]
        seconds: u64,
        /// Turn the creatures with this tag hostile (repeatable)
        #[arg(long)]
        hostile: Vec<String>,
        /// Make the creatures with this tag the player's companions
        /// (repeatable)
        #[arg(long)]
        companion: Vec<String>,
    },
    /// The Force (I26): a dev Jedi casts each power at a creature turned
    /// hostile, one recorded scene per power, checked against the rules
    Force {
        /// k1 or tsl
        game: String,
        /// Module, e.g. end_m01aa
        #[arg(default_value = "end_m01aa")]
        module: String,
        /// The creature the powers are cast at
        #[arg(long, default_value = "sithsoldier")]
        target: String,
        /// A Force user to drain
        #[arg(long, default_value = "end_darkjedi01")]
        drained: String,
    },
    /// The damage pipeline's type step (I22 section 1 step 9): immunity,
    /// vulnerability and resistance by damage type, bonuses against a
    /// race, measured over seeded rolls and on a module's guarded creatures
    Damage {
        /// k1 or tsl
        game: String,
        /// Module, e.g. end_m01ab
        #[arg(default_value = "end_m01ab")]
        module: String,
    },
    /// Grenades (I22 section 7): KOTOR's kinds thrown as JA's thermal
    /// detonator, their blasts, saves and secondary effects, one recorded
    /// scene each against a creature turned hostile
    Grenades {
        /// k1 or tsl
        game: String,
        /// Module, e.g. end_m01aa
        #[arg(default_value = "end_m01aa")]
        module: String,
        /// The creature the grenades are thrown at
        #[arg(long, default_value = "sithsoldier")]
        target: String,
    },
    /// Stealth and Awareness against a module's own creature: seen plain
    /// at 10 m, unseen in stealth mode (Stealth 8, a stealth unit worn),
    /// KOTOR's perception cadence, the opener's sneak attack
    Stealth {
        /// k1 or tsl
        game: String,
        #[arg(default_value = "tar_m09aa")]
        module: String,
    },
    /// KOTOR's consumables (I22 sections 5 and 7): medpacs and repair kits,
    /// antidotes, squad stims, energy shields, stims, in the arena and
    /// against a creature turned hostile
    Items {
        /// k1 or tsl
        game: String,
        /// Module, e.g. end_m01aa
        #[arg(default_value = "end_m01aa")]
        module: String,
        /// A gunner to shoot at the shield, and to use a medpac
        #[arg(long, default_value = "end_gensith")]
        target: String,
    },
    /// I22 section 11's worked Endar Spire fights recomputed through the
    /// pipeline from the modules' own creatures
    Worked,
    /// The second bar (stamina, C7) and the feat specials (I22 section 4):
    /// Power Blast, Rapid and Multi Shot, Sniper Shot and sneak attack,
    /// one recorded scene each against a creature turned hostile
    Specials {
        /// k1 or tsl
        game: String,
        /// Module, e.g. end_m01aa
        #[arg(default_value = "end_m01aa")]
        module: String,
        /// The creature shot at
        #[arg(long, default_value = "sithsoldier")]
        target: String,
    },
    /// Sabers (I24): a character per style driven through JA's saber moves
    /// (attack held each way, the rig's combos, every special,
    /// knockdowns, weapon changes, style cycling), checked against the
    /// rulebook and the rig's timelines
    Saber,
    /// Blades (I24): two players' duels on a floor (cuts and the C5
    /// budget, blocks at each defence level, bolts deflected and reflected,
    /// clashes, locks, throws, kicks) checked against the rules, and the
    /// blades' cost per tick in end_m01aa with 2, 8 and 26 sword users.
    /// Writes the bouts it keeps as `*.kjkr` beside its facts (`kjk play
    /// --replay` draws them)
    Duel {
        /// Only these sections (e.g. `blocks,blade_bolts`); all without
        #[arg(long, value_delimiter = ',')]
        only: Vec<String>,
    },
    /// Duels under latency (I25): a remote player's swings as its client
    /// showed them against the authority's verdicts (C18), the blows held
    /// for its grace (C19) and the rewinds, at 0, 80 and 150 ms
    DuelLag {
        /// Sim seconds each duel lasts
        #[arg(long, default_value_t = 30)]
        seconds: u64,
        /// Duels from this many seeds at each lag, each way
        #[arg(long, default_value_t = 4)]
        seeds: u64,
    },
}

#[derive(Subcommand)]
pub(crate) enum NetVerb {
    /// The network core's probes: reliable delivery under loss, RTT, the
    /// handshake and its refusals, decoders against corrupt bytes.
    Probes,
    /// The snapshot codec over recorded runs: every snapshot rebuilt from
    /// every baseline, delta sizes, and corrupt frames.
    Codec,
    /// Whole sessions in virtual time: a host and clients over a lossy
    /// virtual network, prediction, installs, a join, a soak, and a KOTOR
    /// module when installed.
    Session {
        /// Only the sessions whose name contains this
        #[arg(long)]
        only: Option<String>,
        /// Virtual minutes of the soak
        #[arg(long, default_value_t = 10)]
        soak_minutes: u64,
    },
    /// What the clocks' margins buy: sessions in virtual time at each round
    /// trip, jitter and loss, with each set of margins
    Margins {
        /// Virtual seconds each session lasts
        #[arg(long, default_value_t = 60)]
        seconds: u64,
        /// Round trips, ms
        #[arg(long, value_delimiter = ',', default_value = "100,200")]
        rtt: Vec<u64>,
        /// Only the margins whose name contains this
        #[arg(long)]
        only: Option<String>,
    },
    /// The net-feel report (markdown) of a host's and a player's
    /// `net.json`
    Feel {
        host: std::path::PathBuf,
        player: std::path::PathBuf,
    },
    /// What a host's snapshots cost a client, from one of its recordings:
    /// KB/s by baseline age, by entity kind and by record field group
    Bandwidth {
        /// A host's `inputs.kjkr`
        recording: std::path::PathBuf,
        /// A snapshot every this many ticks
        #[arg(long, default_value_t = 2)]
        every: u64,
        /// Baselines this many ticks back (the client's confirmation's age)
        #[arg(long, value_delimiter = ',', default_value = "2,10,20")]
        back: Vec<u64>,
        /// Split the cost on every this many sent snapshots
        #[arg(long, default_value_t = 10)]
        sample: u64,
    },
}

#[derive(Subcommand)]
pub(crate) enum DialogueVerb {
    /// The conversations a module's level carries, or one as a tree with
    /// its text
    Show {
        /// k1 or tsl
        game: String,
        module: String,
        /// A DLG's resref, e.g. end_trask01
        dialog: Option<String>,
    },
    /// A module stepped headless for one player, its conversations
    /// running; the player answers each offer once its line has played.
    /// The transcript goes to the run folder
    Run {
        /// k1 or tsl
        game: String,
        module: String,
        #[arg(long, default_value_t = 120)]
        seconds: u64,
        /// The replies to take, by place among those offered (from 1), in
        /// order; the first when the list runs out
        #[arg(long, value_delimiter = ',')]
        picks: Vec<u8>,
        /// Arrive at this door or waypoint (by tag)
        #[arg(long)]
        arrive: Option<String>,
        /// A script global set before the first step, NAME=NUMBER;
        /// repeatable
        #[arg(long = "global", value_parser = by_hand::parse_global)]
        globals: Vec<(String, i32)>,
        /// The player made by quick creation, CLASS:PORTRAIT[:NAME], as
        /// `kjk play --made`
        #[arg(long, value_parser = parse_made)]
        made: Option<(u8, String, String)>,
        #[command(flatten)]
        by_hand: Box<ByHand>,
    },
    /// Every conversation holding the world that a K1 module's headless
    /// runs start (one run from its entry, or one for each line
    /// `data/dialogue/k1.sweep` gives it), each filmed by `kjk play` from
    /// its first line until three lines after its first reply set (12 to
    /// 60 s), on one page (`index.html` in the run folder) with its DLG and
    /// first words
    Sweep {
        /// k1
        game: String,
        module: String,
    },
    /// Every scripted move the sweep's runs of each module make (all the
    /// game's modules when none is named), watched step by step: a body
    /// crossing the floor without a moving clip (a slide) or turning back
    /// and forth, stopping and starting (a jitter); `motion.txt` in the
    /// run folder
    Motion {
        /// k1 or tsl
        game: String,
        modules: Vec<String>,
    },
    /// Every conversation of a game's modules (K1's or TSL's) played headless (the sweep's
    /// runs, then one for each object's conversation they did not reach),
    /// each line checked: animations resolve, voices are heard, shots
    /// frame someone, blows give feedback, weapons have models, the dead
    /// stay down, jumps land, conversations end, bodies stand on the
    /// floor, and the census gaps reached; `report.json` and `SUMMARY.md`
    /// in the run folder
    Check {
        /// k1
        game: String,
        /// The modules; every module of the install when none
        modules: Vec<String>,
        /// Modules checked at once (each in its own kjk); half the cores
        /// by default
        #[arg(long)]
        jobs: Option<usize>,
        /// Check the one module here and write its JSON to this file (the
        /// children's part)
        #[arg(long, hide = true)]
        report: Option<std::path::PathBuf>,
    },
}

#[derive(Subcommand)]
pub(crate) enum RemoteVerb {
    /// Every module's own scripts read for a scripted round (a creature
    /// turned hostile, the fight ended by script), and whether the module
    /// protects its fighter as the droid's gate reads it (Min1HP, plot);
    /// the rest listed [verify]
    Census {
        /// k1 or tsl
        game: String,
    },
}

#[derive(Subcommand)]
pub(crate) enum ScriptVerb {
    /// The listing of one compiled script, its routines named
    Dis {
        /// k1 or tsl
        game: String,
        /// The script's resref, e.g. k_sp1_generic
        script: String,
        #[arg(long)]
        module: Option<String>,
    },
    /// Decode every compiled script of an install, write each back, and
    /// check every routine call against the game's nwscript.nss
    Check {
        /// k1 or tsl
        game: String,
    },
    /// A module installed headless for one player and stepped, its scripts
    /// running: what ran, the routines called (bound and gaps), faults, the
    /// trace
    Coverage {
        /// k1 or tsl
        game: String,
        module: String,
        #[arg(long, default_value_t = 30)]
        seconds: u64,
        /// The whole trace, not its first lines
        #[arg(long)]
        trace: bool,
        /// The player walks to this point (metres, game space: x,y)
        #[arg(long, value_delimiter = ',')]
        walk: Option<Vec<f32>>,
        /// Write who perceives whom every 200 ms to DIR/frames.json
        #[arg(long)]
        frames: Option<std::path::PathBuf>,
        /// The player is the dev Jedi, not the soldier
        #[arg(long)]
        jedi: bool,
    },
    /// The VM's probes: programs written here, checked (what the approved
    /// test script_vm asserts)
    Probes,
    /// The effects engine checked end to end: two scripts made here apply
    /// effects to the player in a module, and its fighter is read
    Effects {
        /// k1 or tsl
        game: String,
        #[arg(default_value = "end_m01aa")]
        module: String,
    },
    /// Items checked end to end: the first visit's stock, the party's
    /// inventory and gold, a creature equipping, destroying, a save
    Items {
        /// k1 or tsl
        game: String,
        #[arg(default_value = "end_m01aa")]
        module: String,
    },
    /// What a module's creatures leave in their remains: a listed item
    /// only when its list marks it `Dropable`, what their spawn scripts
    /// made (the games' random treasure) always
    Loot {
        /// k1 or tsl
        game: String,
        #[arg(default_value = "end_m01aa")]
        module: String,
    },
    /// Taking and equipping checked end to end: a container emptied into
    /// the party's items, a body item and a weapon worn by the player's
    /// actions, what `GetItemInSlot` then finds, taken off, a save
    Equip {
        /// k1 or tsl
        game: String,
        #[arg(default_value = "end_m01aa")]
        module: String,
        /// The container taken from, by tag
        #[arg(long, default_value = "end_locker01")]
        container: String,
    },
    /// Locks and Security checked end to end on a module's own locked
    /// door: used shut, Security refused without ranks, taking 20 out of
    /// combat (a spike's bonus when 20 is not enough), the door opening,
    /// locked again by a script, a d20 in combat; a lock only its key
    /// opens, a key, a locked container, where the module has them; a save
    Security {
        /// k1 or tsl
        game: String,
        #[arg(default_value = "end_m01aa")]
        module: String,
        /// The locked door tried, by tag (default: the module's first a
        /// key is not required for)
        #[arg(long)]
        door: Option<String>,
        /// The player's ranks in Security
        #[arg(long, default_value_t = 4)]
        ranks: u8,
    },
    /// Traps and Demolitions checked end to end on a module's own mines
    /// with its own scripts: a mine's facts, found with Awareness,
    /// disarmed, recovered into its kit, sprung by the player and not by
    /// its own faction, the kit laid, a save with it, a Sith springing it
    Traps {
        /// k1 or tsl
        game: String,
        #[arg(default_value = "tar_m09aa")]
        module: String,
    },
    /// Stores checked end to end: their terms and stock, the game's own
    /// script opening one, buying and selling with the party's gold,
    /// leaving, a save
    Stores {
        /// k1 or tsl
        game: String,
        #[arg(default_value = "tar_m02aa")]
        module: String,
        /// The module's script that opens a store (`OpenStore`)
        #[arg(long, default_value = "k_ptar_larrimsto")]
        opens: String,
    },
    /// The party table checked end to end: companions made available,
    /// joining (the party is three), leaving, a save
    Party {
        /// k1 or tsl
        game: String,
        #[arg(default_value = "end_m01aa")]
        module: String,
    },
    /// The journal checked end to end: the install's quests read, a quest
    /// set forward, refused going back, allowed back, another set and
    /// removed, a save
    Journal {
        /// k1 or tsl
        game: String,
        #[arg(default_value = "end_m01aa")]
        module: String,
    },
    /// Levels checked end to end: the classes read, the game's XP boost,
    /// a companion's level waiting then asked, the player's level asked (picks
    /// refused, then taken), a save
    Levels {
        /// k1 or tsl
        game: String,
        #[arg(default_value = "end_m01aa")]
        module: String,
    },
    /// Experience checked end to end: the XP tables read, XP given, set,
    /// kept apart outside the party, plot and quest XP, a hostile
    /// creature's death, the notices, a save
    Xp {
        /// k1 or tsl
        game: String,
        #[arg(default_value = "end_m01aa")]
        module: String,
    },
    /// The routines the whole-game sweep found most reached, checked end
    /// to end: globals raised and lowered, names, personal space, a
    /// talk-table line, the party leader, a creature made hostile, kept at
    /// one HP, its heartbeat taken away and its combat cancelled, a save
    Routines {
        /// k1 or tsl
        game: String,
        #[arg(default_value = "end_m01aa")]
        module: String,
    },
    /// The routines the combat AI calls most, checked end to end: the most
    /// damaging weapons equipped, the last attack action, no power's
    /// target, following the leader (none outside the party), fleeing,
    /// the damage dealt
    Fighting {
        /// k1 or tsl
        game: String,
        #[arg(default_value = "end_m01aa")]
        module: String,
    },
    /// Every module installed and stepped with its scripts running: the
    /// gaps ranked by how many modules reach them, the faults, the modules
    /// that do not install
    Sweep {
        /// k1 or tsl
        game: String,
        #[arg(long, default_value_t = 10)]
        seconds: u64,
        /// Only the modules whose names start so
        #[arg(long)]
        only: Option<String>,
    },
    /// What each Force power does to a target, read off the game's impact
    /// script (`k_sp1_generic`) run per power on a recording host, for
    /// each caster level and ability modifier given
    Impacts {
        /// k1 or tsl
        game: String,
        #[arg(long, value_delimiter = ',', default_values_t = [5, 15])]
        levels: Vec<i32>,
        #[arg(long, value_delimiter = ',', default_values_t = [1, 4])]
        modifiers: Vec<i32>,
    },
    /// Run every compiled script of an install with stand-in routines (each
    /// returns nothing), and the code it defers: the VM's faults counted
    Run {
        /// k1 or tsl
        game: String,
        /// Only scripts whose name contains this
        #[arg(long)]
        only: Option<String>,
    },
}

#[derive(Subcommand)]
pub(crate) enum LifecycleVerb {
    /// Plan a module, commit it as a host, and install it again as a
    /// client from the host's announcement alone.
    Plan {
        /// k1 or tsl
        game: String,
        /// Module, e.g. end_m01ab
        module: String,
        /// Arrive at this door or waypoint (by tag)
        #[arg(long)]
        arrive: Option<String>,
    },
    /// Install two modules in turn, a player walking about in each, and
    /// check nothing is left over: entity counts per visit, the whole run
    /// again, its replay, a save loaded from its module alone.
    Cycle {
        /// k1 or tsl
        game: String,
        a: String,
        b: String,
        /// How many times to go to `b` and back
        #[arg(long, default_value_t = 5)]
        times: u32,
        /// Ticks stepped in each module
        #[arg(long, default_value_t = 250)]
        ticks: u64,
    },
}

#[derive(Subcommand)]
pub(crate) enum MoveVerb {
    /// Walk a stage with scripted input and write the track; a door or
    /// trigger that leads out loads its module and the walk goes on there.
    Run {
        /// The stage: `course`, `duel` (two players facing off), or a KOTOR 1
        /// module such as `end_m01aa`
        #[arg(long, default_value = "course")]
        stage: String,
        /// On a module stage: arrive at this door or waypoint (by tag)
        #[arg(long)]
        arrive: Option<String>,
        /// Scripted input (see `moves.rs`), or a named script: `course`
        #[arg(long, default_value = "course")]
        moves: String,
    },
    /// Speeds, jumps, steps, slopes, crouching, rolls and drops, measured.
    Measure,
    /// The sim's sine and cosine against f64 over every command angle.
    Trig,
    /// How fast every module's creatures walk and run out of a fight,
    /// beside KOTOR's speeds for them (`creaturespeed.2da`): each module
    /// stepped headless with its scripts running, its windows off KOTOR's
    /// counted
    Pace {
        /// k1 or tsl
        game: String,
        /// Only the modules starting so, e.g. tar_m02
        #[arg(long)]
        only: Option<String>,
        /// Seconds each module is stepped
        #[arg(long, default_value_t = 30)]
        seconds: u64,
        /// The creatures with this tag follow the player as companions
        /// (`kjk play --follow`): their follow measured; repeatable
        #[arg(long)]
        follow: Vec<String>,
    },
}

#[derive(Subcommand)]
pub(crate) enum SimVerb {
    /// Step the same inputs several ways (fresh world, reordered input, a
    /// save and load, a replay, rollbacks) and compare snapshot hashes.
    Determinism {
        #[arg(long, default_value_t = 10_000)]
        ticks: u64,
        /// The final hash this machine must reach (hex, or a prefix of it):
        /// the same run on every platform pins one value
        #[arg(long)]
        expect: Option<String>,
        #[command(flatten)]
        world: SimWorldArgs,
    },
    /// Step scripted commands and write them as a recording.
    Record {
        file: std::path::PathBuf,
        #[arg(long, default_value_t = 10_000)]
        ticks: u64,
        #[command(flatten)]
        world: SimWorldArgs,
    },
    /// Play a recording back and check it reaches the hashes it recorded.
    Replay { file: std::path::PathBuf },
    /// Step scripted commands, save, and print the hash a load must reach.
    Save {
        file: std::path::PathBuf,
        #[arg(long, default_value_t = 5_000)]
        ticks: u64,
        /// Ticks to step after saving
        #[arg(long, default_value_t = 5_000)]
        then: u64,
        #[command(flatten)]
        world: SimWorldArgs,
    },
    /// Load a save, check it encodes back to the same bytes, and step on.
    Load {
        file: std::path::PathBuf,
        /// Ticks to step after loading
        #[arg(long, default_value_t = 5_000)]
        then: u64,
        /// Seeds the scripted commands (as when saving)
        #[arg(long, default_value_t = 7)]
        pilot: u64,
        #[arg(long, default_value_t = 4, value_parser = parse_seats)]
        seats: u8,
    },
    /// The sim's probes with known answers, group by group (`sim_probes`),
    /// no install needed, each check printed; fails when any does:
    /// containers, drivers (seats to bodies), coop talk, co-op items,
    /// scaling, encounter count, falls (falling, reviving, losing), party
    /// (the party's calls)
    Probes {
        /// Only these groups or cases, comma-separated
        #[arg(long)]
        only: Option<String>,
    },
    /// Time candidate tick rates: step cost, milliseconds per tick, and
    /// ticks per display frame.
    Rates {
        /// Rates to compare, comma-separated
        #[arg(long, default_value = "20,30,40,50,60,125")]
        hz: String,
        /// Entity counts, comma-separated
        #[arg(long, default_value = "8,256,2048")]
        entities: String,
        /// Sim time per measurement, seconds
        #[arg(long, default_value_t = 10)]
        seconds: u64,
    },
    /// The sim's skeleton against the renderer's Ghoul2 player over saber
    /// moves (hand bolt, blade tip), its determinism hash, and a bolt's
    /// cost (use --release).
    Skeleton {
        /// The saber in the right hand (a `.sab` name)
        #[arg(long, default_value = "single_1")]
        saber: String,
        /// Milliseconds between samples
        #[arg(long, default_value_t = 1)]
        step: u32,
    },
    /// Run the sim live and draw it: the host steps a fixed tick from wall
    /// time; the view reads the presented snapshots only. WASD moves seat
    /// 0, space scatters it. Writes the session to inputs.kjkr.
    View {
        /// Play this recording instead of live input
        #[arg(long)]
        replay: Option<std::path::PathBuf>,
        #[command(flatten)]
        world: SimWorldArgs,
        /// Commands to run, e.g. 'wait 5s; dump; screenshot; quit'
        #[arg(long)]
        cmds: Option<String>,
        #[arg(long)]
        headless: bool,
    },
}

fn collision(verb: CollisionVerb) -> ExitCode {
    match verb {
        CollisionVerb::Build { module } => {
            collision_verbs::build(&discover(), &module.game, &module.module, module.clip())
        }
        CollisionVerb::Walk {
            game,
            module,
            routes,
            seed,
        } => nav_verbs::walk(&discover(), &game, &module, routes, seed),
        CollisionVerb::Nav {
            module,
            routes,
            seed,
        } => nav_verbs::nav(
            &discover(),
            &module.game,
            &module.module,
            module.clip(),
            routes,
            seed,
        ),
        CollisionVerb::Audit {
            module,
            cell,
            doors,
            show,
            zoom,
            seeds,
        } => {
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
        CollisionVerb::Trace {
            module,
            from,
            to,
            shape,
            doors,
            repeat,
        } => {
            let Some(doors) = doors_of(&doors) else {
                eprintln!("--doors {doors}: use openable, closed or all");
                return ExitCode::from(2);
            };
            collision_probes::trace(
                &discover(),
                &module.game,
                &module.module,
                module.clip(),
                &collision_probes::TraceArgs {
                    from,
                    to,
                    shape: &shape,
                    doors,
                    repeat,
                },
            )
        }
        CollisionVerb::Probes {
            game,
            module,
            clip,
            no_clip,
        } => {
            let choice = match (&clip, no_clip) {
                (_, true) => collision_verbs::ClipChoice::None,
                (Some(path), _) => collision_verbs::ClipChoice::File(path),
                (None, false) => collision_verbs::ClipChoice::BuiltIn,
            };
            let target = match (&game, &module) {
                (Some(game), Some(module)) => Some((game.as_str(), module.as_str())),
                (None, None) => None,
                _ => {
                    eprintln!("probes: give both a game and a module, or neither");
                    return ExitCode::from(2);
                }
            };
            collision_probes::probes(&discover(), target, choice)
        }
    }
}

fn sim(verb: SimVerb, size: (u32, u32)) -> ExitCode {
    match verb {
        SimVerb::Determinism {
            ticks,
            expect,
            world,
        } => sim_verbs::determinism(&world.setup(), ticks, expect.as_deref()),
        SimVerb::Record { file, ticks, world } => sim_verbs::record(&world.setup(), ticks, &file),
        SimVerb::Replay { file } => sim_verbs::replay(&discover(), &file),
        SimVerb::Save {
            file,
            ticks,
            then,
            world,
        } => sim_verbs::save(&world.setup(), ticks, then, &file),
        SimVerb::Load {
            file,
            then,
            pilot,
            seats,
        } => sim_verbs::load(&discover(), &file, then, pilot, seats),
        SimVerb::Rates {
            hz,
            entities,
            seconds,
        } => {
            let rates: Option<Vec<sim::TickRate>> = hz
                .split(',')
                .map(|part| part.trim().parse().ok().and_then(sim::TickRate::new))
                .collect();
            let counts: Option<Vec<u32>> = entities
                .split(',')
                .map(|part| part.trim().parse().ok())
                .collect();
            match (rates, counts) {
                (Some(rates), Some(counts)) => sim_verbs::rates(&rates, &counts, seconds, 7),
                _ => {
                    eprintln!("--hz and --entities take comma-separated numbers");
                    ExitCode::from(2)
                }
            }
        }
        SimVerb::Probes { only } => sim_probes::probes(only.as_deref()),
        SimVerb::Skeleton { saber, step } => skeleton_probe::skeleton(&discover(), &saber, step),
        SimVerb::View {
            replay,
            world,
            cmds,
            headless,
        } => {
            let recording = match replay.as_deref().map(|path| {
                std::fs::read(path)
                    .map_err(|error| error.to_string())
                    .and_then(|bytes| {
                        sim::Recording::from_file(&bytes).map_err(|error| error.to_string())
                    })
                    .map_err(|error| format!("{}: {error}", path.display()))
            }) {
                Some(Err(error)) => {
                    eprintln!("{error}");
                    return ExitCode::FAILURE;
                }
                Some(Ok(recording)) => Some(recording),
                None => None,
            };
            // A recording replays in its own level, and through the modules
            // it installed.
            let (level, start, lifecycle) = match &recording {
                Some(recording) => match sim_verbs::replay_setup(&discover(), recording) {
                    Ok((level, start, lifecycle)) => (level, Some(start), lifecycle),
                    Err(error) => {
                        eprintln!("replay refused: {error}");
                        return ExitCode::FAILURE;
                    }
                },
                None => (sim::Level::arena(), None, None),
            };
            play(
                cmds.as_deref().unwrap_or(""),
                headless,
                size,
                runtime::Scene::Sim(Box::new(sim_host::SimSetup {
                    setup: world.setup(),
                    level,
                    driver: sim_host::Driver::Pilot,
                    aim: None,
                    recording,
                    world: start,
                    lifecycle,
                    net: sim_host::Net::Solo,
                    seat: sim::Seat(0),
                })),
            )
        }
    }
}

fn parse_repeat(text: &str) -> Result<([f32; 3], u32), String> {
    let parts: Vec<&str> = text.split(',').map(str::trim).collect();
    let [x, y, z, n] = parts.as_slice() else {
        return Err("use dx,dy,dz,N".into());
    };
    let number = |part: &str| part.parse::<f32>().map_err(|_| format!("{part}: a number"));
    let n = n
        .parse::<u32>()
        .ok()
        .filter(|n| *n <= 10_000)
        .ok_or(format!("{n}: a count up to 10000"))?;
    Ok(([number(x)?, number(y)?, number(z)?], n))
}

fn parse_point(text: &str) -> Result<[f32; 3], String> {
    let numbers: Vec<f32> = text
        .split(',')
        .map(|part| {
            part.trim()
                .parse::<f32>()
                .map_err(|_| format!("{part}: a number"))
        })
        .collect::<Result<_, _>>()?;
    match numbers.as_slice() {
        &[x, y, z] => Ok([x, y, z]),
        _ => Err("use x,y,z in metres".into()),
    }
}

fn parse_zoom(text: &str) -> Result<[f32; 3], String> {
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

fn parse_hz(text: &str) -> Result<u32, String> {
    text.parse::<u32>()
        .ok()
        .filter(|hz| sim::TickRate::new(*hz).is_some())
        .ok_or(format!("{text}: ticks per second from 1 to 1000"))
}
