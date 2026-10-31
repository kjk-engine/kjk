//! `kjk playthrough`: a module played headless, goal by goal, as a player
//! would: walking the navigation mesh (`sim::walk`) to what a goal names,
//! using it, answering conversations, fighting what is hostile, and going
//! on into the next module when a door leads out. A goal not met in its
//! time is a blocker: the run says where the player stood and what held
//! it, then goes on to the next goal; a goal found hopeless (no way to
//! its target for `NO_WAY_MS`, its target not in the module, the player
//! stranded off the navigation mesh) is a blocker at once. Its clock is
//! held only by progress: foes losing hit points, a conversation reaching a
//! line it had not, the way left growing shorter. The run folder gets the transcript,
//! the recording (`inputs.kjkr`, for `kjk play --replay`) and `facts.json`
//! (goals, blockers, the scripts' faults, gaps and missing scripts).
//!
//! A route is goals separated by `;` or new lines (`#` starts a comment):
//!
//! ```text
//! talk                  # a conversation with the player ends (`Talks`: one under
//!                       # way, else one that ended while the goal before ran)
//! pick 1,2              # the replies the next offers take (else the first)
//! say for free          # the next offer with a reply saying that takes it
//! say =22               # the next offer with a reply saying just that
//! to end_trask          # walk to the object with that tag (`TAG@2`: the area's
//!                       # second of that tag, counted whether or not the first
//!                       # is still there)
//! at 30.5 21            # walk to a place (metres)
//! use end_locker01      # walk up and use it: a container is emptied; anything else
//!                       # answered by its conversation, or the use the sim heard
//!                       # (`SimWorld::uses_heard`) once what it set going has
//!                       # played out (`State::busy`); a companion's fight far
//!                       # off fought first (`straggling`)
//! exit end_door10      # walk through a door that leads out, into its module
//!                       # (refused in a fight, or a door on the way shut: its
//!                       # foes fought first, as `fight 25`)
//! fight 15              # attack the nearest hostile within 15 m until none, and
//!                       # none the party can get at keeps it in combat (`Chase`)
//! disarm 52.4,-29.9     # disarm the mine the party sees nearest there (Demolitions),
//!                       # from where the player stands, until it is gone
//! use item:phasepulseps # what holds that item: remains searched (their creature
//!                       # fallen in a route before, no tag left), a foe fought
//! cast 270              # cast that power (spells.2da row) on the player from
//!                       # its power list, until its effect lasts on it
//! equip                 # wear what the party carries, each in a free slot it fits
//! equip ptar_sitharmor  # wear that item (a template), what its slot held off
//! unlock end_door02     # walk up and pick its lock (Security, the player's ranks);
//!                       # `unlock TAG g_i_secspike01`: with a spike the party carries
//! bash fl_spikes        # walk up and strike it with what is in hand until its lock
//!                       # breaks (`COMBAT.md`, "Bashing"; TSL's prologue footlocker)
//! leader 0              # companion NPC 0 leads (the portrait click's `Lead`); `leader player`
//! solo on               # solo mode (the companions stand where they are); `solo off`
//! stealth on            # the stealth button pressed for the body the player leads
//!                       # (Stealth ranks and a stealth unit worn); `stealth off`
//! credits               # stand until the game ends, won (`EndGame`)
//! party 7,0             # the next party selection screen takes NPCs 7 and 0
//! travel 4              # from the galaxy map, to planet 4 (`planetary.2da`)
//! levelup 0             # take a level in class row 0, the rest the game's picks
//! wait 5s               # stand
//! until END_TRASK_DLG=2 # stand until a script global has the value, the world
//!                       # settled (`Settle`), as `if` and `while` read theirs
//!                       # (`until in MODULE`, `until npc 8=1` too)
//! if Tar_SwoopStatus=0  # the goals to its `end` only if a global has the value (a truth: 0 or 1)
//! end                   # (a race lost: raced again)
//! while STA_MALAK_DEAD=0  # the goals to its `end` again and again while it holds,
//!                       # unless a round blocked or `QUIET_ROUNDS` in a row
//!                       # changed nothing (`Round`)
//! if in unk_m41aa       # the goals to its `end` only in that module
//! if npc 8=0            # ... only if companion NPC 8 is (1) or is not (0) available
//! while npc_pato.n13<10 # ... while the object's local number 13 reads under 10, as a
//!                       # script's GetLocalNumber does (`=`, `>` too; `TAG.b45=0`: a
//!                       # local boolean, read true as its raw bit is)

//! module end_m01ab      # stand until the player is in that module
//! gone hendarsfate      # stand until no object of that tag is there (the dead taken
//!                       # away: a script's `GetIsObjectValid(GetObjectByTag(..))`)
//! gather                # stand until the companions have caught up (up, within
//!                       # a follower's distance: `ai::FOLLOW_NEAR`)
//! put g_i_synthodor tar05_corpsepile  # walk up, open it, put one in (its disturbed script)
//! race                  # race the swoop minigame to its finish (`minigame_bot`)
//! turrets               # man the Ebon Hawk's gun until the fighters are down
//!                       # (Peragus' hangar: until its troopers are dead or
//!                       # aboard, the module left for the boarded Ebon Hawk)
//! pazaak                # stand until a pazaak match is played out (any match a
//!                       # script starts is played as `pazaak::Match::advice` says)
//! craft u_l_lens_01     # at the workbench (or lab station) a script showed, make one
//!                       # of that item of its table (TSL's `Action::Create`)
//! fit u_l_lens_01 saber # at the upgrade screen a script showed, fit that upgrade the
//!                       # party carries into the item (`equip`'s word)
//! global K_STAR_MAP=10  # a script global set (a number, or true/false)
//! gold 200              # the party's credits set
//! skill persuade=15     # the player's rank in a skill set (a build a route stands in)
//! goto tar_m03af tar03_wpmechanic  # the party moved to a module's waypoint (none:
//!                       # its entry)
//! save upper_city       # a checkpoint: the world as it stands, saved
//! ```
//!
//! `global`, `gold`, `skill` and `goto` stand in for what the engine or the driver
//! cannot play yet (a Persuade the recommended build never takes), a route's
//! marked workaround, not a story's step.
//!
//! A checkpoint goes to `kjk-artifacts/checkpoints/GAME/NAME.kjkv` (or the
//! folder `--save-to` names: `kjk chain`'s run folder); a run given `@NAME`
//! (or a `.kjkv` path) for its module starts from it, so a route is played
//! in segments, each from where the last one saved.

use std::cell::{OnceCell, RefCell};
use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::Arc;

use collision::UNIT;
use script_ncs::vm::ObjectId;
use serde_json::json;
use sim::action::Action;
use sim::combat::mind;
use sim::math::flat_distance as flat;
use sim::module::{ObjectData, ObjectKind};
use sim::notice::{Notice, Screen};
use sim::script::dialogue::{Line, Pick, answer};
use sim::walk::Walker;
use sim::{Command, Seat, Snapshot, TickInput, TickRate, buttons};
use vfs::{Discovery, Game};

use crate::lifecycle::Installer;
use crate::minigame_bot::{PRESS_MS, pressed};
mod route;
pub mod build;
pub(crate) use route::*;
mod here;
use here::*;
mod diagnose;
use diagnose::*;
mod chase;
use chase::*;
mod checks;
pub use checks::*;
mod gear;
use gear::*;
mod chaos;

/// How close a walk to an object comes (metres, seen from above) before
/// it is there (`to`). A use is made from within its click's reach, as the
/// sim's target block walks to it (`sim::target::Walk::near`).
const NEAR_M: f32 = 1.5;

/// How many times a `while` block goes round at most.
const MAX_ROUNDS: u32 = 50;
/// Rounds in a row that changed nothing before a `while` is left (Malak
/// stands off for a round while he walks to a captive Jedi to drain).
const QUIET_ROUNDS: u32 = 3;

/// How near (metres) a walk to a place (`to X,Y`) comes before it is there.
const PLACE_M: f32 = 0.25;

/// How far (metres) the player moves from where a use's target was checked
/// for sight before it is checked again.
const SIGHT_MOVED_M: f32 = 0.5;
/// How far (metres, across the floor) from where a creature lay dead the
/// remains its `use` loots may stand: its own, or, where several fell
/// together and a use before took its own bag's neighbour, another's
/// still standing (each use loots one, so all are looted).
const REMAINS_M: f32 = 3.0;
/// How long (ms) a `cast` waits on a cast before trying again: past the
/// longest conjure and cast time a power takes (`spells.2da`).
const CAST_AGAIN_MS: u64 = 3000;

/// What a sale keeps, by `baseitems.2da` label: security and programming
/// spikes (and parts, the latter's row).
const KEPT_BASE_ITEMS: [&str; 2] = ["Security_Spikes", "Programming_Spikes"];

/// The `baseitems.2da` rows of `KEPT_BASE_ITEMS` in `resolver`'s game.
fn kept_base_items(resolver: &vfs::resolver::Resolver) -> Result<Vec<u16>, String> {
    let table = resolver
        .two_da("baseitems")
        .ok_or("baseitems.2da: not in the install")?;
    KEPT_BASE_ITEMS
        .iter()
        .map(|label| {
            table
                .find("label", label)
                .and_then(|row| u16::try_from(row).ok())
                .ok_or(format!("baseitems.2da: no row labelled {label}"))
        })
        .collect()
}

/// A hostile this close (metres) is shot from where the player stands; one
/// farther is walked toward first. A blade or fists strike as a creature's
/// mind does (`melee_m`).
const SHOOT_M: f32 = 9.0;
/// How far (metres) a gun shoots at a foe seen but out of every way's
/// reach (`far_shot`).
const GUN_M: f32 = 25.0;

/// A blade's or fists' reach (metres, centre to centre), the sim's own for
/// its creatures (`mind::engage`): it steps in to the first and swings
/// within the second, pressing on into a body wider than a humanoid's (a
/// Mark IV droid's) until it stands against it.
fn melee_m() -> (f32, f32) {
    (mind::REACH * UNIT, mind::STRIKE * UNIT)
}

/// A target with no way to it this near (metres) is walked straight at.
const STRAIGHT_M: f32 = 12.0;

/// How far (metres) a goal's target moves from where its walk was planned
/// to before the walk is planned again.
const REPLAN_M: f32 = 3.5;
/// The same for a foe a blade goes for: a walk arrives within twice
/// `sim::walk::REACHED` (0.6 m) of its goal, so the foe is then within a
/// blade's reach.
const CHASE_M: f32 = 1.0;

/// How far (metres) a foe moves from where a walk to it was planned before
/// it is planned again: a blade's sooner (`CHASE_M`), a gun's at
/// `REPLAN_M`. Every chase of a foe goes by it (`Chase`).
fn chase_replan_m(ranged: bool) -> f32 {
    if ranged { REPLAN_M } else { CHASE_M }
}

/// The walk to `at` (metres): `walker` kept while its goal is within
/// `REPLAN_M` of it, else planned again by `plan`. A walk left from a chase
/// made for its foe, not for this target (Peragus's assassin droid fought
/// off at the shift console, the player then walking for good to where it
/// fell instead of back to the console a script had it strike).
fn walk_for(walker: &mut Option<Walker>, at: [f32; 3], plan: impl FnOnce() -> Walker) -> &mut Walker {
    if walker
        .as_ref()
        .is_some_and(|w| flat(w.goal.map(|v| v * UNIT), at) > REPLAN_M)
    {
        *walker = None;
    }
    walker.get_or_insert_with(plan)
}

/// How near (metres) a closed door is for walking to press use.
const DOOR_NEAR_M: f32 = 2.5;
/// How far ahead (metres) a step is checked for a transition out
/// (`exit_ahead`).
const EXIT_AHEAD_M: f32 = 1.0;
/// How near (metres) a step's way passes a door that leads out to be into it.
const EXIT_DOOR_M: f32 = 1.2;

/// How long (ms) a foe goes unhurt before the player tries its other
/// kind of weapon.
const SWITCH_MS: u64 = 15_000;

/// How near (metres) a foe holding up a walk is fought.
const BLOCKER_M: f32 = 3.0;

/// How often (ms) a walk's way left is measured, and how much shorter
/// (units: a metre) it must have grown to hold the goal's clock.
const WAY_MEASURE_MS: u64 = 1000;
const WAY_GAINED: f32 = 1.0 / UNIT;

/// How long (ms) a walk finds no way to its goal, what shuts the player in
/// unchanged and the goal no nearer, before the goal is hopeless; and how
/// long the player may stand off the navigation mesh, unable to step back
/// onto it.
const NO_WAY_MS: u64 = 10_000;

/// How often (ms) whether a way leads to a foe is asked again.
const REACH_EVERY_MS: u64 = 250;

/// How far (metres) a foe going for the party (a companion too) is gone
/// after by any goal while the party is held in combat.
const FIGHT_BACK_M: f32 = 30.0;

/// How far (metres) such a foe is gone after whatever it does; farther,
/// only one fighting at close quarters.
const FIGHT_NEAR_M: f32 = 15.0;

/// How far (metres) an exit refused in a fight looks for the foes to fight
/// first (as a `fight 25` goal).
const EXIT_FIGHT_M: f32 = 25.0;

/// How near (metres) a body holds up a walk, and how long (ms) the walk
/// backs off from it: out of a narrow way, so one walking it head on (an
/// Anchorhead commoner on its rounds) gets by.
const BODY_M: f32 = 1.2;
const BACKING_MS: u64 = 2500;

/// How near (metres) the route's first corner is for a walk stuck off its
/// line to step back to it, and for how long (ms).
const UNWEDGE_M: f32 = 1.5;
const UNWEDGE_MS: u64 = 1000;

/// How long a fight goal sees no foe before it is over (ms).
const QUIET_MS: u64 = 2000;
/// How long a fight goal waits for a first foe (ms).
const QUIET_FIRST_MS: u64 = 6000;

/// How long a line plays before the player clicks past it (ms), as a
/// player skips dialogue (`Pick::Skip`; `--listen` hears every line out).
const SKIP_AFTER_MS: u64 = 300;
/// How long a use waits at most for a scene its target's scripts play
/// out, and a condition for the world to settle (`Settle`).
const SCENE_WAIT_MS: u64 = 15_000;
/// How soon (ms) code a script left the player to run must be due for the
/// player to stand for it (a scene's last word, after its fade); and code
/// a used object's scripts deferred, for its use to be still under way.
const CUE_MS: u64 = 5_000;
/// How many steps in a row the world stands settled before a condition is
/// read (`Settle`): a script's module change is asked the step after the
/// conversation that set it off ends.
const SETTLED_STEPS: u32 = 2;
/// How many goals' time more progress may hold a goal's clock (ten in
/// all): a long fight going the party's way still ends.
const HELD_GOALS: u64 = 9;
/// How many stalls a walk with no body in its way takes before it backs off.
const LOST_STALLS: u8 = 8;

/// What a run starts from and how long each goal may take.
pub struct Options {
    pub game: String,
    /// A module, or `@NAME` (a `.kjkv` path) for a checkpoint.
    pub module: String,
    pub arrive: Option<String>,
    pub route: String,
    pub sheet: sim::combat::Sheet,
    pub globals: Vec<(String, i32)>,
    pub scripts: Vec<String>,
    pub followers: Vec<String>,
    pub goal_seconds: u64,
    /// A checkpoint made with other level data is taken anyway (`--stale`).
    pub stale: bool,
    /// Every line heard out, none clicked past (`--listen`).
    pub listen: bool,
    /// The run ends at the first goal blocked (the default; `--keep-going`
    /// goes on, spending `goal_seconds` on each goal after it).
    pub fail_fast: bool,
    /// Where `save` goals write their checkpoints (`--save-to`; by default
    /// beside the others, `checkpoint`): a chain's run folder.
    pub save_to: Option<PathBuf>,
    /// Two phantom guests fly remote droids at random, every button of
    /// their kit pressed (`--droids chaos`, `chaos`).
    pub droids: bool,
    /// A build's text (`--build`, `build::parse`): the character a new
    /// game makes, and the plan every level taken follows.
    pub build: Option<String>,
}

/// Plays `options.route` from `options.module`; see the module's notes.
pub fn run(discovery: &Discovery, options: &Options) -> ExitCode {
    let go =
        || -> Result<(serde_json::Value, Vec<String>, Logs, sim::Recording, bool), String> {
            let game = Game::from_key(&options.game).ok_or("use k1 or tsl")?;
            let goals = parse(&options.route)?;
            let (anims, _) = crate::stage::anims(discovery);
            let installer = Installer::new(discovery, game, Arc::new(anims))?;
            let kept = kept_base_items(installer.base_resolver()?)?;
            let build = options
                .build
                .as_deref()
                .map(|text| {
                    let resolver = installer.base_resolver()?;
                    let table = |name: &str| resolver.two_da(name).ok_or(format!("{name}.2da: not read"));
                    build::parse(text, &table("feat")?, &table("spells")?)
                })
                .transpose()?;
            let rate = TickRate::default();
            let world = match options.module.strip_prefix('@') {
                Some(name) => {
                    let mut world = resume(
                        &installer,
                        &rate,
                        &checkpoint(&options.game, name),
                        options.stale,
                    )?;
                    // God mode holds on a loaded world too.
                    world.add_player_flags(
                        options.sheet.flags
                            & (sim::combat::sheet::flags::IMMORTAL
                                | sim::combat::sheet::flags::MIGHTY),
                    );
                    world
                }
                None => {
                    let request = installer.request(&options.module, options.arrive.as_deref(), 1);
                    let prepared = installer.prepare(&request, false)?;
                    let config = sim::WorldConfig {
                        seed: 1,
                        seats: 1,
                        wanderers: 0,
                        spawn: [0.0; 3],
                        yaw: 0.0,
                        sheet: options.sheet,
                        guests: sim::Guests::Avatars,
                    };
                    let mut world = sim::SimWorld::new(config, sim::Level::arena())
                        .map_err(|e| e.to_string())?;
                    prepared
                        .plan
                        .commit(&mut world)
                        .map_err(|e| e.to_string())?;
                    // A new game: the build's character made, as character
                    // creation makes it (god mode holding on its sheet).
                    if let Some(build) = &build {
                        let made = crate::persona::Made::Desc(&build.made);
                        crate::persona::start(&mut world, installer.base_resolver()?, 1, Some(made))?;
                        world.add_player_flags(
                            options.sheet.flags
                                & (sim::combat::sheet::flags::IMMORTAL
                                    | sim::combat::sheet::flags::MIGHTY),
                        );
                    }
                    world
                }
            };
            let mut world =
                crate::stage::with_followers(world, &options.followers, Some(&installer))?;
            crate::by_hand::set_globals(&mut world, &options.globals);
            crate::by_hand::queue_scripts(&mut world, &options.scripts)?;
            if options.droids {
                world.set_seats_held(chaos::HELD);
            }
            let mut watch = chaos::Watch::new(options.droids);
            let mut last = world.snapshot();
            let mut recorder = sim::Recorder::new(rate.hz(), last.clone(), world.level().id(), 100)
                .with_script(world.script_state().clone());
            let mut motion = crate::motion::Motion::new();
            let mut names = crate::transcript::speakers(&last, world.level());
            // The transcript, written out module by module (each with its
            // own conversations), and how many of the report's lines it has.
            let mut lines: Vec<String> = Vec::new();
            let mut transcribed = 0;
            let mut picks: VecDeque<u8> = VecDeque::new();
            // Replies asked for by what they say: an offer takes the first
            // of them one of its replies says, else answers as without.
            let mut says: VecDeque<String> = VecDeque::new();
            // The companions the next party selection takes, if asked.
            let mut chosen: Option<[u8; 2]> = None;
            let mut answered = BTreeSet::new();
            let mut skipped = BTreeSet::new();
            // Replies taken, by conversation: an offer not given a pick
            // takes the first not taken yet, and when all were (a hub of
            // questions), its last, which leaves.
            let mut replied: BTreeSet<(u32, u32)> = BTreeSet::new();
            let mut reply = 0u8;
            let mut tick = last.tick;
            let mut outcomes = Vec::new();
            let mut said: Vec<String> = Vec::new();
            let goal_ms = options.goal_seconds * 1000;
            let began_wall = std::time::Instant::now();
            let module_of = |snapshot: &Snapshot| {
                snapshot
                    .place
                    .as_ref()
                    .map_or_else(String::new, |place| place.module.clone())
            };
            let mut down = false;
            // Actions for the next step: the screen closed, a leader made.
            let mut acts: Vec<Action> = Vec::new();
            // The player's conversations as they end (`Talks`), and whether
            // the world stands settled (`Settle`).
            let mut talks = Talks::default();
            // The triggers whose scene the player set off (`Scenes`).
            let mut scenes = Scenes::default();
            let mut settle = Settle::default();
            // When the last goal that took steps began: a `talk` takes a
            // conversation that ended since (`Talks::claim`).
            let mut stepped_from = last.time_ms;
            // The door or trigger the party last left a module by (its tag):
            // an `exit` naming it, with nothing so tagged where the party is
            // now, was already taken (a walk up to the Undercity's lift that
            // went on into it).
            let mut left_through: Option<String> = None;
            // Whether the party last left by an exit it went through (a door
            // or a trigger that leads out), not a script's start of a module.
            let mut left_by_exit = false;
            // What `use` goals met so far used, by tag (lowercase).
            let mut used_tags: BTreeSet<String> = BTreeSet::new();
            // The area's creatures seen dead, by number (`note_fallen`).
            let mut fallen: BTreeMap<u32, Fell> = BTreeMap::new();
            // A galaxy map shown and not yet travelled from (the planets it
            // offers): a `travel` picks only from an open map (a use answered
            // before the screen came), as its Accept would.
            let mut map_shown: Option<u32> = None;
            // The bench a script last showed (`ShowUpgradeScreen`,
            // `ShowChemicalUpgradeScreen`): a `craft` or `fit` presses its
            // buttons, until a conversation or another module closes it; and
            // what the sim refused there since (its line).
            let mut bench_shown: Option<Screen> = None;
            let mut bench_refused: Option<String> = None;
            // The credits rolled (`StartCreditSequence`), and the game's end
            // after them (`EndGame` without its death screen): won.
            let mut credits_rolled = false;
            let mut game_won = false;
            // The game ended without credits (Manaan's sentence): the run's
            // end.
            let mut game_over = false;
            // Goals of an `if` block whose condition failed, still to skip.
            let mut skip = 0usize;
            // The goal next; how often each `while` has gone round, and
            // where its round began (the outcomes then, what it may change).
            let mut next = 0usize;
            let mut rounds: BTreeMap<usize, u32> = BTreeMap::new();
            let mut quiet_rounds: BTreeMap<usize, u32> = BTreeMap::new();
            let mut round_from: BTreeMap<usize, (usize, Round)> = BTreeMap::new();
            while next < goals.len() {
                let at_goal = next;
                let (text, goal) = &goals[next];
                next += 1;
                if skip > 0 {
                    skip -= 1;
                    outln!("{:>8.2}s  {:<8} {text}", last.time_ms as f64 / 1000.0, "skipped");
                    continue;
                }
                // A `while` goes round again, unless its round blocked or
                // `QUIET_ROUNDS` in a row changed nothing (no global, no
                // creature's hit points): it would go the same way again.
                if let Goal::Again(at) = goal {
                    let round = rounds.entry(*at).or_default();
                    *round += 1;
                    let quiet = quiet_rounds.entry(*at).or_default();
                    match round_from.get(at) {
                        Some((_, was)) if *was == round_of(&world, &last, module_of(&last)) => *quiet += 1,
                        _ => *quiet = 0,
                    }
                    let left = match round_from.get(at) {
                        Some((from, _)) if outcomes[*from..].iter().any(|o: &Outcome| !o.met) => {
                            Some("a round blocked")
                        }
                        _ if *quiet >= QUIET_ROUNDS => Some("its rounds changed nothing"),
                        _ if *round >= MAX_ROUNDS => Some("its rounds ran out"),
                        _ => None,
                    };
                    match left {
                        Some(why) => outln!(
                            "{:>8.2}s  {:<8} {text} ({why})",
                            last.time_ms as f64 / 1000.0,
                            "left"
                        ),
                        None => next = *at,
                    }
                    continue;
                }
                let started_ms = last.time_ms;
                let started_wall = std::time::Instant::now();
                let started_tick = tick;
                let module = module_of(&last);
                let mut walker: Option<Walker> = None;
                // A fight goal's foe, and a foe fought back by any goal.
                let mut chase = Chase::default();
                let mut back = Chase::default();
                // The way to a foe fought back.
                let mut back_walker: Option<Walker> = None;
                let mut reach = Reach::default();
                // Where a use's target was last checked for sight from its
                // reach (metres); a stand round it in sight is walked to
                // first.
                let mut sighted: Option<[f32; 3]> = None;
                let mut stand_for_sight = false;
                let mut pressed_at = 0u64;
                let mut pressed_ever = false;
                // The scripts' talk lines before the goal's first press.
                let mut talk_at_press: Option<usize> = None;
                // The use the sim heard of the goal's target, by the player,
                // since the press (`SimWorld::uses_heard`).
                let mut heard: Option<sim::UseCame> = None;
                // The target's entity this step, for the use heard.
                let mut target_id: Option<sim::SimId> = None;
                // The target was there when the goal began (a trigger its
                // own script takes away on entering it).
                let mut seen_target = false;
                // Until when a walk held up by a body backs off.
                let mut backing_until = 0u64;
                // Closed doors the sim heard refuse the player's press, and
                // those that stayed shut once their scripts had played out
                // (a fail-to-open script that only says why: Manaan's
                // flooded rooms), not planned through again this goal.
                let mut refusals: BTreeSet<u32> = BTreeSet::new();
                let mut refused: BTreeSet<u32> = BTreeSet::new();
                // Triggers that turned the player back (Tatooine's dune
                // edge): walls the walks plan round, and for choosing among
                // a goal's alternatives (`find_any`).
                let mut turned_back = TurnedBack::default();
                let mut made = sim::walk::MadeWalls::default();
                // A travel picked (met the step after, its notice sent).
                let mut travelled = false;
                // Since when the player has stood for code of its own due.
                let mut cued_since: Option<u64> = None;
                // Which of the driver's own rules last held the walk still
                // (for the block report), and when.
                let mut stilled: Option<(&str, u64)> = None;
                // Which way it steps aside, the other each time.
                let mut backing_side = 127i8;
                // Until when a walk stuck off its corner's line steps back
                // to that corner, and where it is (metres).
                let mut unwedge_until = 0u64;
                let mut unwedge = [0.0f32; 3];
                // Unwedges since the walk last backed off: each plans the
                // walk afresh, so its stalls never mount to a backing off;
                // two that did not free it count as lost.
                let mut unwedges = 0u8;
                let mut taken = false;
                // A container's opens asked this goal (`loot_step`), and the
                // container they were of (`pick_remains`).
                let mut opens = 0u8;
                let mut looted: Option<u32> = None;
                let mut met = false;
                let mut held = Vec::new();
                // The goal's clock, held by progress only: foes losing hit
                // points, a conversation reaching a line it had not, the way
                // left growing shorter, a minigame's scripts keeping count.
                let mut clock_ms = started_ms;
                let mut nodes: BTreeSet<(u32, u32)> = BTreeSet::new();
                let mut counted: Option<BTreeMap<String, i32>> = None;
                let mut foe_hp: BTreeMap<sim::SimId, i32> = BTreeMap::new();
                let mut quiet_since: Option<u64> = None;
                let mut fought = false;
                // An exit's foes fought until none is in reach: then out
                // (how many doors had stayed shut by then).
                let mut fought_out: Option<usize> = None;
                // The shortest way left to a walk's goal so far (units), and
                // when it was last measured.
                let mut way_left: Option<f32> = None;
                let mut way_measured = 0u64;
                // Since when no way has led to the walk's goal, what shut
                // the player in then, and how near the goal it stood; since
                // when the target has not been in the module; since when the
                // player has stood off the navigation mesh.
                let mut no_way: Option<(u64, Shut, f32)> = None;
                let mut absent_since: Option<u64> = None;
                let mut off_mesh_since: Option<u64> = None;
                // The conversation a `talk` saw under way.
                let mut seen_talk: Option<u32> = None;
                // The death screen shown (a turret fight lost): the game is
                // over.
                let mut over = false;
                // The minigame's module, once the goal is in it.
                let mut played_in: Option<String> = None;
                // A pazaak match seen under way.
                let mut tabled = false;
                // A `craft` or `fit` pressed at its bench: for a `craft`,
                // how many of the item the party carried then.
                let mut benched: Option<u32> = None;
                if let Goal::Party(picks) = goal {
                    chosen = Some(*picks);
                    met = true;
                }
                if let Goal::Say(words) = goal {
                    says.push_back(words.clone());
                    met = true;
                }
                if let Goal::Pick(more) = goal {
                    picks.extend(more);
                    met = true;
                }
                if let Goal::LevelUp(class) = goal {
                    acts.push(Action::LevelUp(sim::levels::Choices {
                        class: *class,
                        ..sim::levels::Choices::default()
                    }));
                    met = true;
                } else if let Some(class) = level_due(&world, &last) {
                    // A level the party's XP allows, taken at once as a
                    // player would, in the class taken last: the build's
                    // plan's picks, else the game's. One a goal.
                    let planned = build.as_ref().and_then(|build| planned(&world, &last, class, &build.plan));
                    acts.push(Action::LevelUp(planned.unwrap_or(sim::levels::Choices {
                        class,
                        ..sim::levels::Choices::default()
                    })));
                }
                // The companions' levels too, each the game's picks.
                for (who, choices) in companion_levels_due(&world, &last) {
                    acts.push(Action::LevelUpOn { who, choices });
                }
                if let Goal::Solo(on) = goal {
                    acts.push(Action::Solo(*on));
                }
                if let Goal::Leader(npc) = goal {
                    // As the HUD's portrait click asks it: by the member's
                    // body (a companion with no creature here, by number).
                    let body = match *npc {
                        u8::MAX => last.own(Seat(0)).map(|own| own.id),
                        npc => world
                            .script_state()
                            .party
                            .available
                            .get(&npc)
                            .and_then(|known| known.object)
                            .map(sim::SimId),
                    };
                    acts.push(body.map_or(Action::SetLeader(*npc), Action::Lead));
                }
                if let Goal::Global(name, value) = goal {
                    let mut state = world.script_state().clone();
                    match value {
                        Ok(number) => sim::script::set_global(&mut state.numbers, name, *number),
                        Err(truth) => sim::script::set_global(&mut state.booleans, name, *truth),
                    };
                    world.set_script_state(state);
                    met = true;
                }
                if let Goal::Gold(gold) = goal {
                    let mut state = world.script_state().clone();
                    state.gold = *gold;
                    world.set_script_state(state);
                    met = true;
                }
                if let Goal::Skill(row, rank) = goal {
                    world.set_player_skill(*row, *rank);
                    met = true;
                }
                if let Goal::Goto(module_name, tag) = goal {
                    let request = installer.request_from(&last, module_name, tag.as_deref());
                    let prepared = installer.prepare(&request, false)?;
                    let announcement = prepared
                        .plan
                        .commit(&mut world)
                        .map_err(|e| e.to_string())?;
                    recorder.install(announcement);
                    last = world.snapshot();
                    names = crate::transcript::speakers(&last, world.level());
                    fallen.clear();
                    met = true;
                }
                // A checkpoint is only a good run's: one after a goal not
                // met would start the next segment wrong.
                let bad = outcomes.iter().any(|o: &Outcome| !o.met);
                if let (Goal::Save(name), true) = (goal, bad) {
                    held.push(format!(
                        "save {name}: not saved, a goal before it was not met"
                    ));
                } else if let (Goal::Save(name), true) = (goal, last.minigame.is_some()) {
                    held.push(format!("save {name}: not saved during a minigame"));
                } else if let Goal::Save(name) = goal {
                    // What the goals before it asked goes out first.
                    if !acts.is_empty() {
                        let mut input = TickInput::default();
                        for action in acts.drain(..) {
                            input.act(Seat(0), action);
                        }
                        last = step_on(&mut world, &mut tick, &rate, (&mut recorder, &mut motion), (input, &mut watch, &last));
                    }
                    let path = options.save_to.as_ref().map_or_else(
                        || checkpoint(&options.game, name),
                        |folder| folder.join(format!("{name}.kjkv")),
                    );
                    let save = sim::Save {
                        hz: rate.hz(),
                        level: world.level().id(),
                        snapshot: last.clone(),
                        script: world.script_state().clone(),
                    };
                    // Whole or not at all: a chain starts the route after
                    // it as soon as the file is there.
                    match crate::chain::write_whole(&path, &save.encode()) {
                        Ok(()) => met = true,
                        Err(error) => held.push(format!("{}: {error}", path.display())),
                    }
                }
                if let Goal::Equip(named) = goal {
                    let named = &named
                        .as_deref()
                        .map(|named| named_item(&world, &last, named).unwrap_or_else(|| named.to_string()));
                    let wear = equip(&world, &last, named.as_deref());
                    // Worn already (a scene armed it): nothing to do.
                    if named
                        .as_deref()
                        .is_some_and(|named| wears(&world, &last, named))
                    {
                        met = true;
                    } else if wear.is_empty() {
                        held.push(format!(
                            "equip: the party carries {} to wear",
                            named.as_deref().unwrap_or("nothing")
                        ));
                    } else {
                        acts.extend(wear);
                        met = named.is_none();
                    }
                }
                // A goal refused at once (nothing to wear) ends with why.
                while !met && held.is_empty() {
                    let avatar = last.own(Seat(0)).copied().ok_or("no player")?;
                    // The body the player moves: a companion leading, else
                    // the seat's (`Snapshot::body`: the one it is switched
                    // to, `SwitchPlayerCharacter`), else its own.
                    let me = sim::lent(&world.script_state().party)
                        .and_then(|body| last.get(body).copied())
                        .or_else(|| last.body(Seat(0)).copied())
                        .unwrap_or(avatar);
                    let player = me.player.ok_or("the player has no movement")?;
                    // The downed wait to get up (I30 P11); the whole party
                    // down is the end of the run, as death was.
                    if last.lost.is_some() || avatar.fighter.is_some_and(|f| f.dead()) {
                        held = vec!["the party is down".into()];
                        down = true;
                        break;
                    }
                    if game_over {
                        held = vec!["the game is over (EndGame)".into()];
                        break;
                    }
                    let now = last.time_ms;
                    // A conversation with the player, or with the companion
                    // it moves (one a script started with the leader), or
                    // the one holding every seat, the player's to answer
                    // whichever body leads now (Bao-Dur's at the old
                    // military base's first shield, begun as he led and
                    // still under way once the Exile led again).
                    let talking = last
                        .talk
                        .holding(ObjectId(avatar.id.0))
                        .or_else(|| last.talk.holding(ObjectId(me.id.0)))
                        .or_else(|| last.talk.party())
                        .cloned();
                    if let Some(talk) = &talking {
                        settle.partner = Some(talk.owner.0);
                    }
                    let party: Vec<u32> = {
                        let table = &world.script_state().party;
                        [me.id.0, avatar.id.0]
                            .into_iter()
                            .chain(
                                table
                                    .members
                                    .iter()
                                    .filter_map(|npc| table.available.get(npc)?.object),
                            )
                            .collect()
                    };
                    let watched: Vec<u32> = party.iter().copied().chain(settle.partner).collect();
                    settle.note(
                        tick,
                        settled(&last, world.script_state(), talking.is_some(), &watched),
                    );
                    // Doors the sim heard refuse a press that stayed shut
                    // once nothing their scripts set going is under way (a
                    // fail-to-open script's conversation, its deferred code):
                    // planned as locked from now on.
                    let level = world.level().clone();
                    for index in std::mem::take(&mut refusals) {
                        let door = last
                            .entities
                            .iter()
                            .find(|e| matches!(e.kind, sim::Kind::Object(_, i) if i == index));
                        let locked = door.and_then(|e| e.object).is_some_and(|state| {
                            state.state == 0
                                && level
                                    .placed()
                                    .get(index as usize)
                                    .is_some_and(|placed| state.locked(placed.opens))
                        });
                        let busy = talking.is_some()
                            || door.is_some_and(|e| world.script_state().busy(e.id.0, now, CUE_MS));
                        if locked && busy {
                            refusals.insert(index);
                        } else if locked {
                            refused.insert(index);
                        }
                    }
                    let here = Here::new(&level, &last, &refused)
                        .walled(&turned_back.walled)
                        .made(made.of_snapshot(&level, &last));
                    if turned_back.note(level.name(), me.position, talking.is_some(), || {
                        holding(&here, me.position)
                    }) {
                        // Planned again, round it, from the next step.
                        turned_back.wall(&here);
                        walker = None;
                    }
                    note_fallen(&mut fallen, here.snapshot, &world.script_state().corpses);
                    let owner_of = |talk: &sim::script::dialogue::Conversation| {
                        last.get(sim::SimId(talk.owner.0))
                            .and_then(|e| match e.kind {
                                sim::Kind::Object(_, index) => Some(index),
                                _ => None,
                            })
                    };
                    talks.note(talking.as_ref().map(|talk| (talk.id, owner_of(talk))), now);
                    let trigger_of = |id: ObjectId| match last.get(sim::SimId(id.0))?.kind {
                        sim::Kind::Object(ObjectKind::Trigger, index) => Some(index),
                        _ => None,
                    };
                    scenes.note(level.name(), &world.script_report().talk, me.id, trigger_of, now);
                    let talked_with = talking.as_ref().and_then(owner_of);
                    let mut command = mind::looking(&player, player.view[0], player.view[1]);
                    let mut input = TickInput::default();
                    // Each offer is answered once its line has played.
                    if let Some(talk) = &talking
                        && talk.until_ms == u64::MAX
                        && !talk.replies.is_empty()
                        && answered.insert((talk.id, talk.shown_ms))
                    {
                        let last = talk.replies.len() - 1;
                        // The first words asked for that a reply here says.
                        let texts: Vec<String> = talk
                            .replies
                            .iter()
                            .map(|r| {
                                reply_text(&installer, &world, &talk.dialog, *r).to_lowercase()
                            })
                            .collect();
                        let said = says.iter().enumerate().find_map(|(at, words)| {
                            texts
                                .iter()
                                .position(|text| match words.strip_prefix('=') {
                                    // `say =2`: that reply and no other
                                    // (a terminal's numbers: not "22").
                                    Some(exact) => text.trim() == exact,
                                    None => text.contains(words.as_str()),
                                })
                                .map(|nth| (at, nth))
                        });
                        let said = said.map(|(at, nth)| {
                            says.remove(at);
                            nth
                        });
                        let nth = said.unwrap_or_else(|| {
                            picks.pop_front().map_or_else(
                                || {
                                    talk.replies
                                        .iter()
                                        .position(|r| !replied.contains(&(talk.id, *r)))
                                        .unwrap_or(last)
                                },
                                |n| usize::from(n - 1).min(last),
                            )
                        });
                        replied.insert((talk.id, talk.replies[nth]));
                        reply = answer(reply, Pick::Reply(nth as u8));
                    }
                    // A line that plays out on its own is clicked past (where
                    // the conversation lets it be), as a player skips them.
                    if let Some(talk) = &talking
                        && !options.listen
                        && talk.until_ms != u64::MAX
                        && now >= talk.shown_ms + SKIP_AFTER_MS
                        && skipped.insert((talk.id, talk.shown_ms))
                    {
                        reply = answer(reply, Pick::Skip);
                    }
                    // Use pressed and let go with a closed door at hand (it
                    // opens), the press made only where the key would open
                    // that door (`key_meets`, below).
                    let own = match goal {
                        Goal::Exit(Target::Tag(tag, nth)) | Goal::Use(Target::Tag(tag, nth)) => {
                            find(&here, tag, *nth).map(|(index, _)| index)
                        }
                        _ => None,
                    };
                    let door =
                        closed_door_near(&here, me.position, matches!(goal, Goal::Exit(_)), own);
                    let pulse =
                        buttons::USE * u16::from(pressed(now) && door.is_some());
                    // What this step's use key is pressed for: the door, or
                    // the goal's own target (its press, below).
                    let mut pressed_for = door.filter(|_| pulse != 0).map(|(index, _)| index);
                    // A body standing in the walk's way, gone round.
                    if let Some(w) = walker.as_mut() {
                        let bodies: Vec<[f32; 3]> = last
                            .entities
                            .iter()
                            .filter(|e| e.id != me.id && e.fighter.is_some_and(|f| f.alive()))
                            .filter(|e| !sim::bodies_pass(&level, true, sim::party_member(e)))
                            .filter_map(|e| e.player.map(|ps| ps.origin))
                            .collect();
                        w.around(&level, &player, &bodies);
                    }
                    // The party in combat (`Snapshot::in_combat`): the nearest
                    // of those keeping it so (`keepers`, not plot) the player
                    // can get at. A fight, and an exit refused in it, reach as
                    // far as that one, as a player goes to help (a Sith gunner
                    // at a companion across the Leviathan's hangar deck while
                    // the player waited at the storage door).
                    let at_hand: Vec<Keeper> = last
                        .in_combat
                        .then(|| keepers(&here, &world, me.position))
                        .into_iter()
                        .flatten()
                        .filter(|keeper| {
                            !keeper.plot
                                && reach.gettable(&here, &player, me.position, (keeper.id, keeper.at), now)
                        })
                        .collect();
                    let keeping = at_hand
                        .iter()
                        .map(|keeper| flat(keeper.at, me.position) + 1.0)
                        .min_by(f32::total_cmp);
                    let stretched = |radius: f32| keeping.map_or(radius, |d| d.max(radius));
                    // A use not yet pressed while a companion fights out
                    // of the party (`straggling`): that fight first, once.
                    let straggled = (matches!(goal, Goal::Use(_)) && !pressed_ever && fought_out.is_none())
                        .then(|| {
                            let bodies: Vec<(sim::SimId, [f32; 3])> = mates(&world, &last)
                                .iter()
                                .map(|mate| (mate.body.id, mate.body.position))
                                .collect();
                            straggling(&at_hand, &bodies, me.position)
                        })
                        .flatten();
                    // An exit the party may not take in a fight
                    // (`Snapshot::in_combat`: rakghouls at the Undercity's
                    // sewer door, a droid at the Vulkars' lift), or one whose
                    // way a door stayed shut on while foes stand near
                    // (Korriban's valley, shut until the academy's three
                    // fall, one of them hanging back; the Star Forge's turret
                    // door, shut while turrets that never saw the party
                    // stand): its foes are fought first, as a fight goal
                    // fights them (those it can get at), then out; again for
                    // each door shut since. Once through, the exit is taken:
                    // no fight goes on in the next module (after deck two's
                    // turret door the player fought on in the command centre,
                    // away from the trigger of its frozen door's scene).
                    let fight_out =
                        Goal::Fight(straggled.unwrap_or(0.0).max(stretched(EXIT_FIGHT_M)));
                    let shut_since = refused.len() > fought_out.unwrap_or(0);
                    let fight_first = (straggled.is_some()
                        || fights_first(goal, fought_out.is_some(), last.in_combat, shut_since))
                        && module_of(&last) == module
                        && matches!(fight_out, Goal::Fight(r)
                            if !hostiles(&here, &world, me.position, r).is_empty());
                    // A goal's target that is not there, nor anything else
                    // answers it (`absent`), is hopeless once the world has
                    // settled, no script's deferred code is due soon and a
                    // heartbeat's time has passed (a spawn script's turn): a
                    // route naming what is not in the module.
                    let mut absent = false;
                    // A target there that cannot answer the goal as it is
                    // placed (an unlock of a thing not locked, a use of a
                    // placeable not useable; an open door answers an unlock),
                    // hopeless as `absent` is: the
                    // Harbinger's engine-deck route named the first of seven
                    // `MilLowFootLker`, a bare prop, and waited out its
                    // goal's time walking about it.
                    let mut cannot: Option<&'static str> = None;
                    // Whether the goal walks to its target this step.
                    let mut walked = false;
                    let walks = matches!(
                        goal,
                        Goal::To(_)
                            | Goal::Use(_)
                            | Goal::Exit(_)
                            | Goal::Unlock(..)
                            | Goal::Bash(_)
                            | Goal::Mine(_)
                            | Goal::Put(..)
                    );
                    match if fight_first { &fight_out } else { goal } {
                        Goal::Pick(_)
                        | Goal::Say(_)
                        | Goal::Again(_)
                        | Goal::Party(_)
                        | Goal::Equip(None)
                        | Goal::LevelUp(_)
                        | Goal::Save(_)
                        | Goal::Global(..)
                        | Goal::Gold(_)
                        | Goal::Skill(..)
                        | Goal::Goto(..) => {}
                        // Picked once what the map's use plays out (its
                        // first use's tutorial) has ended.
                        Goal::Travel(planet) => {
                            if talking.is_none() && travelled {
                                met = true;
                            } else if talking.is_none()
                                && let Some(selectable) = map_shown.take()
                            {
                                if !Screen::has_planet(selectable, usize::from(*planet)) {
                                    held = vec![format!("planet {planet} is not on offer")];
                                    break;
                                }
                                acts.push(Action::Travel(*planet));
                                travelled = true;
                            }
                        }
                        // Met once the party table has it (the next goal
                        // moves the body it gives).
                        Goal::Leader(npc) => {
                            met = world.script_state().party.leader
                                == Some(*npc).filter(|npc| *npc != u8::MAX);
                        }
                        Goal::Solo(on) => met = world.script_state().party.solo == *on,
                        // Pressed once out of a conversation (as the HUD's
                        // stealth button is), held a step; not met a second
                        // later, refused (the game's line is in the
                        // transcript: no ranks, no unit worn, in combat).
                        Goal::Stealth(on) => {
                            met = me.fighter.is_some_and(|fighter| fighter.covert.stealth == *on);
                            if !met && talking.is_none() {
                                match benched {
                                    None => {
                                        benched = Some(0);
                                        pressed_at = now;
                                        command.buttons |= buttons::STEALTH;
                                    }
                                    Some(_) if now > pressed_at + 1000 => {
                                        held = vec![format!("stealth {}: refused", if *on { "on" } else { "off" })];
                                        break;
                                    }
                                    Some(_) => {}
                                }
                            }
                        }
                        // As a player disarms a mine it sees from beside it:
                        // again after each try (`sim::traps::DISARM_MS`), a
                        // failure or a blast, until none is left there.
                        Goal::Disarm(at) => match mine_near(&here, *at) {
                            None => met = true,
                            Some(index) => {
                                if now >= pressed_at + sim::traps::DISARM_MS + 4 * PRESS_MS {
                                    input.act(Seat(0), Action::Disarm(index));
                                    pressed_at = now.max(1);
                                }
                            }
                        },
                        // As a player casts from the power list: again after
                        // each cast's time, until the power's effect lasts on
                        // the player (one it cannot pay for is never cast).
                        Goal::Cast(row) => match under(&last, *row) {
                            Some((_, true)) => met = true,
                            Some((me, false)) if now >= pressed_at + CAST_AGAIN_MS => {
                                input.act(Seat(0), Action::Cast { row: *row, target: me });
                                pressed_at = now.max(1);
                            }
                            _ => {}
                        },
                        Goal::Credits => met = game_won,
                        // One item named: put on again each step until worn
                        // (armour is refused while a fight goes on: the Sand
                        // People's robes taken off the fallen on the dunes).
                        Goal::Equip(Some(named)) => {
                            let named = named_item(&world, &last, named).unwrap_or_else(|| named.clone());
                            met = wears(&world, &last, &named);
                            if !met {
                                acts.extend(equip(&world, &last, Some(&named)));
                            }
                        }
                        // One a step, its purchase seen before the next;
                        // once the world has settled (a store a script opens
                        // a moment after its line, as Kodin's does).
                        Goal::Buy(template, count) => {
                            met = gear::carried(&last, template) >= *count;
                            if !met && talking.is_none() && settle.settled() {
                                match gear::purchase(&last, template) {
                                    Ok(buy) => acts.extend(buy),
                                    Err(why) => held.push(why),
                                }
                            }
                        }
                        // One item a step, its sale seen before the next.
                        Goal::Sell(gold) => {
                            met = last.goods.gold >= *gold;
                            if !met && talking.is_none() {
                                acts.extend(sale(&world, &last, &kept));
                            }
                        }
                        // The conversation it saw under way, once ended, or
                        // one that ended while the goal before ran (`Talks`).
                        Goal::Talk => match &talking {
                            Some(talk) => {
                                seen_talk.get_or_insert(talk.id);
                            }
                            None => met = talks.claim(seen_talk, stepped_from),
                        },
                        Goal::Wait(ms) => met = now >= started_ms + ms,
                        // Read once the world has settled (`Settle`), as a
                        // player looks once the screen has changed (a lift's
                        // conversation ends a step or two before its script
                        // starts the next module); after a scene's time, as
                        // it then stands.
                        Goal::If(condition, block) => {
                            if settle.settled() || now >= started_ms + SCENE_WAIT_MS {
                                if !condition.holds(world.script_state(), &module_of(&last), &here) {
                                    skip = *block;
                                }
                                round_from.insert(
                                    at_goal,
                                    (outcomes.len() + 1, round_of(&world, &last, module_of(&last))),
                                );
                                met = true;
                            }
                        }
                        // Once the world has settled too (after a scene's
                        // time, once no conversation plays).
                        Goal::Until(condition) => {
                            met = condition.holds(world.script_state(), &module_of(&last), &here)
                                && (settle.settled()
                                    || talking.is_none() && now >= started_ms + SCENE_WAIT_MS);
                        }
                        Goal::Gone(tag, nth) => met = find(&here, tag, *nth).is_none(),
                        Goal::Gather => met = gathered(&world, &last, me.position),
                        Goal::Module(name) => met = module_of(&last).eq_ignore_ascii_case(name),
                        // Pressed once, on the bench a script showed: made
                        // once the party carries one more, or refused (too
                        // few components, the skill not reached).
                        Goal::Craft(template) => {
                            if let Some(had) = benched {
                                met = gear::carried(&last, template) > had;
                                if let (false, Some(why)) = (met, bench_refused.take()) {
                                    held = vec![format!("craft: {why}")];
                                    break;
                                }
                            } else if let Some(Screen::Workbench(bench)) = &bench_shown
                                && talking.is_none()
                            {
                                match gear::creation(&world, bench, template) {
                                    Ok(create) => {
                                        bench_refused = None;
                                        benched = Some(gear::carried(&last, template));
                                        acts.push(create);
                                    }
                                    Err(why) => {
                                        held = vec![why];
                                        break;
                                    }
                                }
                            } else if settle.settled() && talking.is_none() {
                                held = vec!["craft: no workbench or lab station open".into()];
                                break;
                            }
                        }
                        // Pressed once, on an upgrade screen a script showed
                        // for the item (or for any): met once fitted.
                        Goal::Fit(upgrade, named) => match gear::fitted(&world, &last, named, upgrade) {
                            Some((_, true)) => met = true,
                            Some((item, false)) if benched.is_none() => {
                                let open = match &bench_shown {
                                    Some(Screen::Upgrade { item: only }) => only.is_none_or(|only| only == item),
                                    Some(Screen::Workbench(bench)) => {
                                        bench.upgrade && bench.item.is_none_or(|only| only == item)
                                    }
                                    _ => false,
                                };
                                if open && talking.is_none() {
                                    match gear::fitting(&world, item, upgrade) {
                                        Ok(fit) => {
                                            bench_refused = None;
                                            benched = Some(0);
                                            acts.push(fit);
                                        }
                                        Err(why) => {
                                            held = vec![why];
                                            break;
                                        }
                                    }
                                } else if settle.settled() && talking.is_none() {
                                    held = vec![format!("fit: no upgrade screen open for {named}")];
                                    break;
                                }
                            }
                            Some(_) => {
                                if let Some(why) = bench_refused.take() {
                                    held = vec![format!("fit: {why}")];
                                    break;
                                }
                            }
                            None => {
                                held = vec![format!("fit: the party has no {named}")];
                                break;
                            }
                        },
                        Goal::Pazaak => {
                            tabled |= last.pazaak_of(Seat(0)).is_some();
                            met = tabled && last.pazaak_of(Seat(0)).is_none();
                        }
                        // Played by the bots from what the snapshot shows,
                        // met when the minigame's script leaves its module
                        // (the goal may start before it is entered).
                        Goal::Race | Goal::Turrets => {
                            if let (Some(course), Some(rules)) =
                                (last.minigame.as_ref(), level.minigame())
                            {
                                played_in.get_or_insert_with(|| module_of(&last));
                                let gear = sim::script::global(
                                    &world.script_state().numbers,
                                    crate::minigame_bot::GEAR,
                                )
                                .copied();
                                let bot = if matches!(goal, Goal::Race) {
                                    "race"
                                } else {
                                    "turrets"
                                };
                                if let Some(played) = crate::minigame_bot::drive(
                                    bot,
                                    command,
                                    (Some(rules), Some(course)),
                                    gear,
                                    now,
                                    &crate::minigame_bot::area_foes(&last),
                                ) {
                                    command = played;
                                }
                            }
                            met = played_in
                                .as_ref()
                                .is_some_and(|minigame| module_of(&last) != *minigame);
                            if over {
                                held = vec!["the game is over (the death screen)".into()];
                                break;
                            }
                        }
                        Goal::To(target)
                        | Goal::Use(target)
                        | Goal::Exit(target)
                        | Goal::Unlock(target, _)
                        | Goal::Bash(target)
                        | Goal::Mine(target)
                        | Goal::Put(_, target) => {
                            let lay = match target {
                                Target::Tag(tag, nth) => fell(&here, &fallen, tag, *nth),
                                Target::Place(_) | Target::Holding(_) => None,
                            };
                            // One dead whose body the engine has yet to make
                            // its remains is waited for: the remains nearest
                            // where it lay are another's (Tatooine's three Dark
                            // Jedi, fallen side by side: the first's own, its
                            // datapad and saber, were left behind).
                            let laid_out = matches!(goal, Goal::Use(_))
                                && lay.is_some_and(|lay| {
                                    world.script_state().corpses.contains_key(&lay.id.0)
                                });
                            let found = match target {
                                Target::Tag(..) if laid_out => None,
                                Target::Tag(tag, nth) => {
                                    let found = find_any(&here, tag, *nth, &player);
                                    // One dead is used by its remains, once
                                    // they stand where it lay.
                                    matches!(goal, Goal::Use(_))
                                        .then_some(lay)
                                        .flatten()
                                        .and_then(|lay| {
                                            remains_at(&here, world.script_state(), lay.at, looted)
                                        })
                                        .or(found)
                                        // One dead and taken away is walked to
                                        // where it lay (the Leviathan's
                                        // technician, come to a fight before
                                        // the walk to him).
                                        .or_else(|| {
                                            matches!(goal, Goal::To(_))
                                                .then_some(lay)
                                                .flatten()
                                                .map(|lay| (u32::MAX, lay.at))
                                        })
                                }
                                Target::Holding(template) => holder_of(&here, world.script_state(), template),
                                Target::Place([x, y]) => Some((u32::MAX, [*x, *y, me.position[2]])),
                            };
                            seen_target |= found.is_some();
                            target_id = found
                                .and_then(|(index, _)| here.entity(index))
                                .map(|e| e.id);
                            // Out: the door is the last module's, or the
                            // one the goal before went out by.
                            let gone_by = found.is_none()
                                && matches!(target, Target::Tag(tag, _)
                                    if left_through.as_deref().is_some_and(|by| by.eq_ignore_ascii_case(tag)));
                            if matches!(goal, Goal::Exit(_)) && (module_of(&last) != module || gone_by) {
                                met = true;
                            } else if matches!(goal, Goal::To(_)) && gone_by {
                                // A trigger the walk before already went out
                                // by (the Unknown World's way aboard, taken
                                // as the beach's fight ended).
                                met = true;
                            } else if matches!(goal, Goal::To(_))
                                && let Some(scene) =
                                    found.and_then(|(index, _)| scenes.take_before(index, started_ms))
                            {
                                // A trigger whose own scene the player set
                                // off during a goal before (`Scenes`): the
                                // `talk` after it takes that scene.
                                talks.ended.push_back(Talk {
                                    ended_ms: now,
                                    ..scene
                                });
                                met = true;
                            } else if found.is_none()
                                && matches!(goal, Goal::Use(_))
                                && !laid_out
                                && (pressed_ever
                                    || taken
                                    || module_of(&last) != module
                                    || matches!(target, Target::Tag(tag, _)
                                        if used_tags.contains(&tag.to_ascii_lowercase())))
                            {
                                // Or used by a goal before and gone since
                                // (remains emptied: the Unknown World's
                                // scholar, looted where he fell).
                                // Gone once used (its script took it: the
                                // Vulkars' accelerator, given as an item).
                                // Or left behind with its module, the walk
                                // there having set off its scene (Kashyyyk's
                                // repulsor field: pressed on the way, its
                                // conversation takes the party down).
                                met = true;
                            } else if let (None, Goal::Use(_), Some(lay)) = (found, goal, lay) {
                                // Fallen and taken away leaving no remains
                                // (it held nothing that drops: a Dark Jedi on
                                // Tatooine's dunes), once the engine has
                                // come for its body (`sim::remains`). One
                                // that left remains is done once this use
                                // emptied them.
                                met = lay.used(taken, &world.script_state().corpses);
                            } else if found.is_none()
                                && matches!(goal, Goal::Use(_))
                                && seen_target
                                && module_of(&last) != module
                                && !left_by_exit
                            {
                                // Left behind with its module, the party
                                // taken on by the story before it was used
                                // (Jolee's word past Czerka's field on
                                // Kashyyyk, taking the party down as the
                                // player walked to the field).
                                met = true;
                            } else if found.is_none()
                                && matches!(goal, Goal::To(_))
                                && (seen_target
                                    || matches!(target, Target::Tag(tag, nth) if sprung(&here, tag, *nth)))
                            {
                                // Gone once entered: a trigger its script
                                // destroys as it fires (Jolee's on Kashyyyk
                                // jumps the party and takes itself away), or
                                // a mine the party set off before the goal
                                // (Peragus's fuel-line charge by the stash,
                                // met on the walk to the stash).
                                met = true;
                            } else if found.is_none() {
                                absent = true;
                            } else if matches!(goal, Goal::Use(_))
                                && found.is_some_and(|(index, _)| {
                                    talks.ended_with(index, stepped_from)
                                        && !here.facts(index).is_some_and(|data| data.container)
                                        && here
                                            .entity(index)
                                            .is_some_and(|e| e.fighter.is_some_and(|f| f.alive()))
                                })
                            {
                                // Its conversation played out in the goal
                                // before (Bastila's after the swoop's
                                // fight): the use is done. Not one dead
                                // since (the chieftain's remains to loot),
                                // nor a container, opened still (Korriban's
                                // Lonna Vash, whose body's scene a trigger
                                // plays before she is searched).
                                met = true;
                            } else if matches!(goal, Goal::Unlock(..))
                                && found.is_some_and(|(index, _)| {
                                    here.facts(index).is_some_and(|data| {
                                        data.opens == sim::module::Opens::Locked
                                            && object_of(&here, index).lock == sim::lock::UNLOCKED
                                    })
                                })
                            {
                                // Placed locked and unlocked in play already
                                // (broken open by a blow in the fight round
                                // it, Korriban's academy footlockers): its
                                // lock is done with.
                                met = true;
                            } else if let Some((index, at)) = found {
                                let data = here.facts(index);
                                // A blow is an attack, made at the thing
                                // itself, not a use.
                                let at = match goal {
                                    Goal::Bash(_) => at,
                                    _ => Here::use_point(me.position, at, data.as_deref()),
                                };
                                let d = flat(at, me.position);
                                cannot = data.as_ref().and_then(|data| {
                                    let state = object_of(&here, index);
                                    match goal {
                                        Goal::Unlock(..)
                                            if matches!(data.kind, ObjectKind::Door | ObjectKind::Placeable)
                                                && !state.locked(data.opens)
                                                && !(data.kind == ObjectKind::Door && state.state != 0) =>
                                        {
                                            Some("it is not locked (Security on it is refused)")
                                        }
                                        Goal::Use(_) if data.kind == ObjectKind::Placeable && !data.useable => {
                                            Some("it is a placeable that is not useable")
                                        }
                                        _ => None,
                                    }
                                });
                                // What the harness's click asks of it (as E,
                                // the target block's first choice), and so
                                // how near it is made from.
                                let choice = data.as_ref().and_then(|data| {
                                    click(goal, data, object_of(&here, index))
                                });
                                // A target that moved on (a creature walking
                                // back to its post), or a place that did (where
                                // one of a tag fell, moved by the next of it
                                // to fall: the sewers' two Gamorrean leaders):
                                // planned again. (A walk's goal stands up to
                                // 3.2 m short of its target, `approach`.)
                                if walker
                                    .as_ref()
                                    .is_some_and(|w| flat(w.goal.map(|v| v * UNIT), at) > REPLAN_M)
                                {
                                    walker = None;
                                }
                                // Pressed at from within reach but out of its
                                // sight (a desk's side, a frame's edge between):
                                // a stand round it in sight (`sighted_stand`),
                                // checked again from each new spot (a fight on
                                // the way that pushed the player off it: foes
                                // at Hrakert's airlock).
                                // The sim's own check of the use, from where
                                // the player stands (`SimWorld::may_use`).
                                let verb = choice.zip(data.as_ref()).and_then(|(choice, data)| {
                                    match sim::target::ask(choice, index, data.kind)? {
                                        Action::Use { verb, .. } => Some(verb),
                                        _ => None,
                                    }
                                });
                                if matches!(goal, Goal::Use(_))
                                    && sighted.is_none_or(|from| flat(from, me.position) > SIGHT_MOVED_M)
                                    && !(stand_for_sight && walker.as_ref().is_some_and(|w| !w.arrived(&player)))
                                    && pressed_ever
                                    && index != u32::MAX
                                    && let Some((choice, data)) = choice.zip(data.as_ref())
                                    && d <= sim::target::reach(choice, data.kind)
                                    && let Some(verb) = verb
                                {
                                    sighted = Some(me.position);
                                    if !world.may_use(&player, index, verb)
                                        && let Some(stand) =
                                            sighted_stand(&here, &player, at, &mut |body| world.may_use(body, index, verb))
                                    {
                                        walker = Some(stand);
                                        stand_for_sight = true;
                                    }
                                }
                                // Come to the stand short of its point (a
                                // walk arrives within a margin) where the use
                                // is still refused: the last step onto it.
                                let usable = !stand_for_sight
                                    || index == u32::MAX
                                    || verb.is_none_or(|verb| world.may_use(&player, index, verb));
                                let walker = walker.get_or_insert_with(|| match (verb, choice.zip(data.as_ref())) {
                                    (Some(sim::action::Verb::Talk), Some((choice, data))) => {
                                        here.approach_talk(&player, at, sim::target::reach(choice, data.kind))
                                    }
                                    _ => here.approach(&player, at),
                                });
                                // No way there (a locked door's doorway, a
                                // body in the way) but near: straight at it.
                                // Arrived short of it: the last stretch straight.
                                let arrived = walker.arrived(&player);
                                let routed = !arrived && here.leads(player.origin, walker.goal);
                                let round = routed && here.goes_round(player.origin, walker.goal);
                                let mut walking = || Command {
                                    buttons: {
                                        walked = true;
                                        pulse
                                    },
                                    ..if stand_for_sight && arrived && !usable {
                                        sim::walk::toward(&player, walker.goal)
                                    } else if routed || d > STRAIGHT_M {
                                        walker.command_round(
                                            &level,
                                            &player,
                                            now,
                                            &|i| here.to_try(i),
                                            &|face| round && here.walls(face),
                                            false,
                                        )
                                    } else {
                                        Command {
                                            forward: 127,
                                            ..facing(&player, at)
                                        }
                                    }
                                };
                                // Within its click's reach: near as the target
                                // block walks up (`Walk::near`), or, the walk
                                // come as near as it can (across a counter:
                                // Greeta Holda answers from 3.4 m), within the
                                // reach itself.
                                let near = choice.zip(data.as_ref()).map_or(d <= NEAR_M, |(choice, data)| {
                                    sim::target::Walk::near(
                                        &player,
                                        sim::target::Spot::plain(at, choice, data.kind),
                                    )
                                        || arrived && d <= sim::target::reach(choice, data.kind)
                                });
                                let alive = |e: &sim::EntityState| e.fighter.is_some_and(|f| f.alive());
                                // In its fight, or in a scene of its
                                // scripts (moves queued, code deferred).
                                let busy = now < started_ms + SCENE_WAIT_MS
                                    && here.entity(index).is_some_and(|e| {
                                        e.kind == sim::Kind::Object(ObjectKind::Creature, index)
                                            && alive(e)
                                            && (e.fighter.is_some_and(|f| f.combat_until > now)
                                                || world.script_state().delayed.iter().any(|d| {
                                                    d.caller.0 == e.id.0 && d.due_ms > now
                                                })
                                                || world.script_state().actions.get(&e.id.0).is_some_and(|q| {
                                                    q.iter().any(|a| {
                                                        matches!(a.action, sim::script::actions::Action::Move { .. })
                                                    })
                                                }))
                                    });
                                let creature = data
                                    .as_ref()
                                    .is_some_and(|d| d.kind == ObjectKind::Creature)
                                    && here.entity(index).is_some_and(alive);
                                // A door opened (whoever opened it) has
                                // answered a use; a placeable's state is its
                                // look (`sim::look`), which a script may turn
                                // on as the player comes (Dantooine's
                                // terminals), no answer.
                                let opened = index != u32::MAX
                                    && data.as_ref().is_some_and(|d| d.kind == ObjectKind::Door)
                                    && object_of(&here, index).state
                                        != data.as_ref().map_or(0, |d| d.state);
                                // A creature started talking with the player
                                // on its own (a guard who stops you), or a
                                // locked door its press failed to open
                                // speaking (its `OnFailToOpen` conversation:
                                // Visquis's sealed door, whose scene hands the
                                // player to Mira far from it): the use is
                                // answered.
                                // A placeable's own scene is not its use (the
                                // galaxy map's tutorial, played as the party
                                // walks up, opens no planets).
                                // An unlocked door's own scene is no answer
                                // (Peragus's prison door, Kreia speaking
                                // through it as the player comes).
                                let sealed = index != u32::MAX
                                    && data.as_ref().is_some_and(|d| {
                                        d.kind == ObjectKind::Door && object_of(&here, index).locked(d.opens)
                                    });
                                let spoke = (creature || sealed) && talking.as_ref().is_some_and(|talk| {
                                    [talk.owner, talk.speaker].iter().any(|id| {
                                        last.get(sim::SimId(id.0)).is_some_and(|e| {
                                            matches!(e.kind, sim::Kind::Object(_, i) if i == index)
                                        })
                                    })
                                });
                                match goal {
                                    // A trigger: inside its outline; one that
                                    // leads out, at it (inside is out: the
                                    // Dune Sea's way on, walked up to so the
                                    // Sand People there are fought first).
                                    Goal::To(_) => {
                                        command = walking();
                                        met = match data.as_ref() {
                                            Some(t) if t.kind == ObjectKind::Trigger && t.exit().is_none() => {
                                                level.in_trigger(t, me.position)
                                            }
                                            // A place: on it (a route steps
                                            // tile by tile: the Unknown
                                            // World's floor puzzle).
                                            _ if index == u32::MAX => d <= PLACE_M,
                                            _ => d <= NEAR_M || walker.arrived(&player),
                                        };
                                    }
                                    // Out through it: walked up, then into it.
                                    Goal::Exit(_) => {
                                        command = if d > 2.5 {
                                            walking()
                                        } else {
                                            Command {
                                                forward: 127,
                                                buttons: pulse,
                                                ..facing(&player, at)
                                            }
                                        };
                                        met = module_of(&last) != module;
                                    }
                                    Goal::Unlock(..) | Goal::Bash(_) | Goal::Mine(_) | Goal::Put(..)
                                        if !near =>
                                    {
                                        command = walking()
                                    }
                                    // A mine set on the door, again while
                                    // none stands set on it, until it
                                    // stands open (blasted).
                                    Goal::Mine(_) => {
                                        command = facing(&player, at);
                                        let set = here.entity(index).is_some_and(|e| {
                                            world.script_state().door_mines.contains_key(&e.id.0)
                                        });
                                        if !set
                                            && now >= pressed_at + sim::traps::LAY_MS + 4 * PRESS_MS
                                        {
                                            input.act(Seat(0), Action::Mine(index));
                                            pressed_at = now.max(1);
                                        }
                                        met = object_of(&here, index).state != 0;
                                    }
                                    // Struck with what is in hand, as a
                                    // player attacks it, until it breaks
                                    // (unlocked).
                                    Goal::Bash(_) => {
                                        // A blade or a baton steps in while it
                                        // swings, as a fight's chase presses on
                                        // into a wide body (`Chase::strike`),
                                        // until the thing's own collision holds
                                        // it: a walk to pick its lock ends up
                                        // to 2 m off, out of a blow's reach
                                        // (Peragus's locker between lockers).
                                        // Nothing in hand: the strongest weapon
                                        // the party carries drawn first, as a
                                        // player arms to break a door (fists
                                        // took a minute and more on Dantooine's
                                        // stuck Enclave door, a blade seconds).
                                        let unarmed = me.sheet.is_some_and(|sheet| {
                                            matches!(
                                                sheet.main_weapon().delivery,
                                                sim::combat::sheet::Delivery::Unarmed
                                            )
                                        });
                                        if unarmed {
                                            // A blade if one is trained for,
                                            // else a gun: its bolts strike too.
                                            acts.extend(
                                                other_weapon(&world, &last, true)
                                                    .or_else(|| other_weapon(&world, &last, false)),
                                            );
                                        }
                                        let step_in = !ranged(&me) && d > melee_m().0;
                                        command = Command {
                                            buttons: buttons::ATTACK,
                                            forward: if step_in { 127 } else { 0 },
                                            ..aiming(&player, &me, strike_at(&here, None, at))
                                        };
                                        met = object_of(&here, index).lock == sim::lock::UNLOCKED;
                                    }
                                    // Opened, the item put in, closed.
                                    Goal::Put(template, _) => {
                                        command = facing(&player, at);
                                        let item = last
                                            .goods
                                            .carried(sim::inventory::PARTY)
                                            .find(|(_, item)| {
                                                item.template.eq_ignore_ascii_case(template)
                                            })
                                            .map(|(id, _)| id);
                                        match (taken, item) {
                                            (false, Some(item)) => {
                                                input.act(Seat(0), Action::Open(index));
                                                input.act(
                                                    Seat(0),
                                                    Action::Put {
                                                        container: index,
                                                        item,
                                                    },
                                                );
                                                taken = true;
                                            }
                                            (false, None) => {
                                                held.push(format!(
                                                    "put: the party carries no {template}"
                                                ));
                                            }
                                            (true, _) => {
                                                input.act(Seat(0), Action::Close(index));
                                                met = true;
                                            }
                                        }
                                    }
                                    // A creature still in its fight, or in a
                                    // scene its scripts play out (its moves
                                    // and deferred code), is left to finish
                                    // first, from where the player stands (a
                                    // player waits, out of its way: Bastila
                                    // after Brejik's fall goes for her sword).
                                    Goal::Use(_) if busy => command = facing(&player, at),
                                    Goal::Use(_) if !near || (stand_for_sight && !(arrived && usable)) => {
                                        command = walking();
                                        met = opened || spoke;
                                    }
                                    // Security tried, again while it fails.
                                    Goal::Unlock(_, spike) => {
                                        command = facing(&player, at);
                                        // The spike as the Security screen
                                        // picks it: one the party carries.
                                        let spike = spike.as_deref().and_then(|template| {
                                            last.goods
                                                .carried(sim::inventory::PARTY)
                                                .find(|(_, item)| {
                                                    item.template.eq_ignore_ascii_case(template)
                                                })
                                                .map(|(id, _)| id)
                                        });
                                        // Again once a try's time is up.
                                        if now >= pressed_at + sim::locks::UNLOCK_MS + 4 * PRESS_MS
                                        {
                                            input.act(
                                                Seat(0),
                                                Action::Unlock {
                                                    object: index,
                                                    spike,
                                                },
                                            );
                                            pressed_at = now.max(1);
                                        }
                                        // A door open is unlocked too; a
                                        // placeable's state is its look
                                        // (TSL's med bin stands closed at 2).
                                        let door = object_of(&here, index);
                                        let is_door = here.entity(index).is_some_and(|e| {
                                            e.kind == sim::Kind::Object(ObjectKind::Door, index)
                                        });
                                        met = door.lock == sim::lock::UNLOCKED
                                            || (is_door && door.state != 0);
                                    }
                                    _ => {
                                        command = facing(&player, at);
                                        let dead = here.entity(index).is_some_and(|e| {
                                            e.kind == sim::Kind::Object(ObjectKind::Creature, index)
                                                && e.fighter.is_some_and(|f| f.dead())
                                        });
                                        // A foe still standing (one come over
                                        // after the fight's end): attacked, as
                                        // a player's click on a foe does.
                                        let foe = hostiles(&here, &world, at, 1.0)
                                            .first()
                                            .is_some_and(|(id, _)| Some(*id) == target_id);
                                        if foe {
                                            command = Command {
                                                buttons: buttons::ATTACK,
                                                ..aiming(&player, &me, strike_at(&here, target_id, at))
                                            };
                                        } else if dead {
                                            // Its remains still to come
                                            // (`sim::remains`): waited for.
                                        } else if data.as_ref().is_some_and(|d| d.container) {
                                            // Opened by the player's use (its
                                            // `OnUsed` runs, locked or not) and
                                            // emptied (`loot_step`).
                                            let holds = holds_items(&here, world.script_state(), index);
                                            match loot_step(holds, opens, now, pressed_at) {
                                                Loot::Close => {
                                                    input.act(Seat(0), Action::Close(index));
                                                    met = true;
                                                }
                                                Loot::Open => {
                                                    input.act(
                                                        Seat(0),
                                                        Action::Use {
                                                            object: index,
                                                            verb: sim::action::Verb::Open,
                                                        },
                                                    );
                                                    input.act(Seat(0), Action::TakeAll(index));
                                                    pressed_at = now.max(1);
                                                    // An open refused from
                                                    // here (out of sight)
                                                    // walks to a stand it
                                                    // sees it from, as a
                                                    // press does (above).
                                                    pressed_ever = true;
                                                    opens = opens.saturating_add(1);
                                                    looted = Some(index);
                                                    taken = true;
                                                }
                                                Loot::Wait => {}
                                            }
                                        } else {
                                            // A conversation playing before any
                                            // press, or a trigger's (one walked
                                            // into on the way), is not this
                                            // use's answer: a player clicks
                                            // nothing until it ends.
                                            let foreign = talking.is_some()
                                                && (!pressed_ever
                                                    || talked_with.is_some_and(|by| {
                                                        by != index
                                                            && here
                                                                .facts(by)
                                                                .is_some_and(|d| d.kind == ObjectKind::Trigger)
                                                    }));
                                            if pressed_at == 0 && !foreign {
                                                pressed_at = now.max(1);
                                                pressed_ever = true;
                                                talk_at_press.get_or_insert(world.script_report().talk.len());
                                                // And KOTOR's click on it: what
                                                // the key's trace misses (a
                                                // basket a script made,
                                                // Kashyyyk's lift), as E asks it.
                                                acts.extend(choice.zip(data.as_ref()).and_then(|(choice, data)| {
                                                    sim::target::ask(choice, index, data.kind)
                                                }));
                                            }
                                            if now < pressed_at + PRESS_MS {
                                                command.buttons |= buttons::USE;
                                                pressed_for = Some(index);
                                            }
                                            if now >= pressed_at + 4 * PRESS_MS {
                                                pressed_at = 0;
                                            }
                                            // Its own conversation refused since
                                            // the press (its user already
                                            // talking: a trigger's, walked into
                                            // on the way) means the use never
                                            // happened, as a KOTOR click a
                                            // conversation cancels: pressed
                                            // again once the other one ends.
                                            let conversation = data.as_ref().map_or("", |d| d.conversation.as_str());
                                            let refused_talk = talk_at_press.is_some_and(|from| {
                                                !conversation.is_empty()
                                                    && world.script_report().talk.get(from..).is_some_and(|lines| {
                                                        lines.iter().any(|line| {
                                                            matches!(line, Line::Refused { dialog, .. } if dialog.eq_ignore_ascii_case(conversation))
                                                        })
                                                    })
                                            });
                                            if refused_talk {
                                                pressed_at = 0;
                                                talk_at_press = None;
                                                heard = None;
                                            }
                                            // One with a conversation of its own
                                            // is answered by that one (its own,
                                            // or begun by its use script), not by
                                            // another that plays meanwhile.
                                            let answered = talking.as_ref().is_some_and(|talk| {
                                                !foreign
                                                    && (conversation.is_empty()
                                                        || talked_with == Some(index)
                                                        || talk.dialog.eq_ignore_ascii_case(conversation))
                                            });
                                            // Or the sim heard it used
                                            // (`SimWorld::uses_heard`) and what
                                            // the use set going has played out
                                            // (`State::busy`: nothing queued on
                                            // it, no conversation asked with
                                            // it, none of its deferred code due
                                            // soon). A door is answered by
                                            // opening or speaking only.
                                            let door = data.as_ref().is_some_and(|d| d.kind == ObjectKind::Door);
                                            let played_out = !refused_talk
                                                && match heard {
                                                    Some(sim::UseCame::Used) => !door,
                                                    Some(sim::UseCame::Refused) | None => false,
                                                    Some(_) => true,
                                                }
                                                && target_id.is_none_or(|id| {
                                                    !world.script_state().busy(id.0, now, CUE_MS)
                                                });
                                            // Or a line over its head: a
                                            // creature that only barks.
                                            let barked = last.talk.barks.iter().any(|bark| {
                                                last.get(sim::SimId(bark.who.0)).is_some_and(|e| {
                                                    e.kind == sim::Kind::Object(ObjectKind::Creature, index)
                                                })
                                            });
                                            // Or its own conversation of one
                                            // line, said since the press: begun
                                            // and over within a step, never
                                            // seen playing (Peragus's sealed
                                            // hatch, `a_compdlg`'s hint).
                                            let one_line = talk_at_press.zip(target_id).is_some_and(|(from, id)| {
                                                world.script_report().talk.get(from..).is_some_and(|lines| {
                                                    lines.iter().any(|line| {
                                                        matches!(line, Line::Bark { who, conversation: true, .. } if who.0 == id.0)
                                                    })
                                                })
                                            });
                                            met = answered
                                                || played_out
                                                || barked
                                                || one_line
                                                || opened
                                                || module_of(&last) != module;
                                        }
                                    }
                                }
                            }
                        }
                        Goal::Fight(radius) => {
                            let reach_m = stretched(*radius);
                            // The foe fought, else the nearest, the player can
                            // get at (`Reach::gettable`, `Chase::pick`), never
                            // one behind a locked door, nor one standing in a
                            // way out (a guard of the Sith base's entrance on
                            // Manaan): the party never steps into one but by
                            // an `exit`.
                            let in_way_out = |at: [f32; 3]| {
                                here.objects().iter().any(|(_, data)| {
                                    data.kind == ObjectKind::Trigger
                                        && data.exit().is_some()
                                        && level.in_trigger(data, at)
                                })
                            };
                            let foe = chase.pick(me.position, hostiles(&here, &world, me.position, reach_m), |foe| {
                                !in_way_out(foe.1)
                                    && !chase.given_up(&here, &player, foe.0)
                                    && reach.gettable(&here, &player, me.position, *foe, now)
                            });
                            // A medpac below a third of its health, pressed
                            // and let go.
                            let low = me.fighter.zip(me.sheet).is_some_and(|(fighter, sheet)| {
                                fighter.hp * 3 < i32::from(sheet.max_hp) * 100
                            });
                            if low && pressed(now) {
                                command.buttons |= buttons::MEDPAC;
                            }
                            let coming = talk_coming(&world.script_state().actions, &party);
                            match foe {
                                // A scene's conversation still playing, or
                                // coming: its end may start the fight (a
                                // raid's "Attack!"), so no quiet yet.
                                None if talking.is_some() || coming => quiet_since = None,
                                // No foe for a while (a script turns some
                                // hostile a step or more after its line),
                                // longer before any foe (a scene's speakers
                                // turn on you a moment after it); and from
                                // the floor: in the air (knocked or jumping)
                                // no way leads anywhere, so no foe is
                                // reachable. The party still held in combat
                                // by one it can get at goes on to it
                                // (`stretched`).
                                None => {
                                    let since = *quiet_since.get_or_insert(now);
                                    let quiet = if fought { QUIET_MS } else { QUIET_FIRST_MS };
                                    met = now >= since + quiet && player.ground.is_some();
                                }
                                Some(foe) => {
                                    quiet_since = None;
                                    fought = true;
                                    let (strike, stepping) = chase.strike(
                                        &mut walker, &here, &world, &mut reach, &player, &me, foe,
                                        now, &mut acts,
                                    );
                                    command = Command {
                                        buttons: command.buttons
                                            | strike.buttons
                                            | if stepping { pulse } else { 0 },
                                        ..strike
                                    };
                                }
                            }
                        }
                    }
                    // Fighting through a refused exit is not the exit: once
                    // no foe is in reach, on out (foes out of reach still near
                    // marked the exit taken: the Leviathan's storage door).
                    if met && fight_first {
                        met = false;
                        fought_out = Some(refused.len());
                    }
                    if met {
                        // What the goal asked on its last step still goes
                        // out, on the next: a container used is closed, and
                        // its `OnClosed` runs as the screen's close makes it
                        // (TSL's sensor droid box starts its tutorial so).
                        acts.extend(input.actions().iter().map(|(_, action)| action.clone()));
                        break;
                    }
                    // The goal's clock is held only by progress (up to
                    // `HELD_GOALS` goals' time more): a conversation reaching
                    // a line it had not yet; foes at hand losing hit points to the party (a
                    // long fight going its way; a walk fought through a
                    // waylay: Tatooine's dark Jedi on the way to the gate),
                    // each by its own, a shield worn down as its hit points
                    // (Hrakert's heavy sentry droid); the way left to a walk's
                    // goal growing shorter (below).
                    let hold = now.min(started_ms + HELD_GOALS * goal_ms);
                    if let Some(talk) = &talking
                        && nodes.insert((talk.id, talk.entry))
                    {
                        clock_ms = hold;
                    }
                    // A minigame ends when its scripts say, the player in its
                    // seat meanwhile, as KOTOR's is: a global they set changing
                    // is progress (Peragus' turret counts the troopers that
                    // board the Ebon Hawk, `107PER_MG_EBON`, until all 25 have).
                    if matches!(goal, Goal::Race | Goal::Turrets) {
                        let numbers = &world.script_state().numbers;
                        if counted.as_ref() != Some(numbers) {
                            if counted.is_some() {
                                clock_ms = hold;
                            }
                            counted = Some(numbers.clone());
                        }
                    }
                    let near_m = match goal {
                        Goal::Fight(radius) => stretched(*radius),
                        _ if fight_first => stretched(EXIT_FIGHT_M),
                        _ => BLOCKER_M,
                    };
                    let foes = hostiles(&here, &world, me.position, near_m);
                    let fought_back = back.foe.and_then(|id| Some((id, last.get(id)?.position)));
                    for (id, _) in foes.into_iter().chain(fought_back) {
                        let Some(f) = last.get(id).and_then(|e| e.fighter) else {
                            continue;
                        };
                        let hp = f.hp + f.pack.shield.map_or(0, |s| i32::from(s.left) * 100);
                        if foe_hp.insert(id, hp).is_some_and(|was| hp < was) {
                            clock_ms = hold;
                        }
                    }
                    // A foe seen at hand that has fallen since, or is gone,
                    // is progress too: one blow can kill (`--mighty`), and
                    // the dead are no foes to lose hit points again (the
                    // Unknown World's Elders and their guards, one after
                    // another, ran the fight past its time).
                    let felled: Vec<sim::SimId> = foe_hp
                        .keys()
                        .filter(|id| last.get(**id).and_then(|e| e.fighter).is_none_or(|f| !f.alive()))
                        .copied()
                        .collect();
                    if !felled.is_empty() {
                        clock_ms = hold;
                        for id in felled {
                            foe_hp.remove(&id);
                        }
                    }
                    // A walk the player stands still for while a conversation
                    // goes on (a scene's script may move it, as Zaalbar's on
                    // Kashyyyk's landing port does) plans afresh after: the
                    // talk's seconds are not stalls to back off from.
                    if talking.is_some() {
                        walker = None;
                        backing_until = 0;
                    }
                    // Once a second: the way left to the walk's goal (shorter
                    // holds the clock: a long way round, out of Manaan's Sith
                    // base past its flooded rooms), and whether any way leads
                    // there. None for `NO_WAY_MS`, what shuts the player in
                    // unchanged (no door opened, none refused) and the goal
                    // no nearer, is hopeless.
                    if !walked || fight_first || back_walker.is_some() || talking.is_some() {
                        no_way = None;
                    }
                    if now >= way_measured + WAY_MEASURE_MS
                        && let Some(w) = walker.as_ref()
                        && talking.is_none()
                    {
                        way_measured = now;
                        let near = flat(w.goal.map(|v| v * UNIT), me.position);
                        match here.route(player.origin, w.goal, true) {
                            Some(corners) => {
                                no_way = None;
                                let left = corners
                                    .iter()
                                    .fold((0.0, player.origin), |(length, from), corner| {
                                        (length + flat(from, *corner), *corner)
                                    })
                                    .0;
                                if left + WAY_GAINED < way_left.unwrap_or(f32::MAX) {
                                    way_left = Some(left);
                                    clock_ms = hold;
                                }
                            }
                            None if walked && !fight_first && back_walker.is_none() => {
                                let shut: Shut =
                                    sim::walk::frontier(&level, player.origin, &|i| here.to_try(i))
                                        .iter()
                                        .map(|(at, entity)| (*entity, at.map(|v| v.round() as i32)))
                                        .collect();
                                match &no_way {
                                    Some((_, was, nearest))
                                        if *was == shut && near > nearest - 1.0 => {}
                                    _ => no_way = Some((now, shut, near)),
                                }
                            }
                            None => {}
                        }
                    }
                    // Off the navigation mesh (`sim::walk::on_mesh`), on the
                    // floor, the goal making no progress since.
                    let on_mesh = sim::walk::on_mesh(&level, player.origin);
                    if on_mesh
                        || player.ground.is_none()
                        || matches!(goal, Goal::Race | Goal::Turrets)
                        || off_mesh_since.is_some_and(|since| clock_ms > since)
                    {
                        off_mesh_since = None;
                    } else {
                        off_mesh_since.get_or_insert(now);
                    }
                    if absent || cannot.is_some() {
                        absent_since.get_or_insert(now);
                    } else {
                        absent_since = None;
                    }
                    // Hopeless early only where nothing may yet change it
                    // before the goal's time would run out: the world settled,
                    // out of combat, and no script's deferred code due till
                    // then (a door a scene unlocks later, a spawn on a delay).
                    let still = settle.settled()
                        && !last.in_combat
                        && !world
                            .script_state()
                            .delayed
                            .iter()
                            .any(|d| d.due_ms > now && d.due_ms <= clock_ms + goal_ms);
                    let hopeless = if still
                        && no_way.as_ref().is_some_and(|(since, ..)| now >= since + NO_WAY_MS)
                    {
                        Some("no way there, nothing that shuts the player in changed and the goal no nearer")
                    } else if still
                        && cannot.is_some()
                        && absent_since.is_some_and(|since| now >= since + sim::HEARTBEAT_MS)
                    {
                        cannot
                    } else if still
                        && absent_since.is_some_and(|since| now >= since + sim::HEARTBEAT_MS)
                    {
                        Some("what it names is not in the module, the world settled and no script's code due before the goal's time is up")
                    } else if still && off_mesh_since.is_some_and(|since| now >= since + NO_WAY_MS) {
                        Some("the player is stranded off the navigation mesh")
                    } else {
                        None
                    };
                    if hopeless.is_some() || now >= clock_ms + goal_ms {
                        held = diagnose(&here, &world, &me, goal, walker.as_ref());
                        if let Some(why) = hopeless {
                            held.insert(0, format!("hopeless: {why}"));
                        }
                        if !refused.is_empty() {
                            let placed = level.placed();
                            let tags: Vec<String> = refused
                                .iter()
                                .map(|index| {
                                    let state = object_of(&here, *index);
                                    placed.get(*index as usize).map_or(format!("#{index}"), |o| {
                                        format!(
                                            "{} at ({:.2}, {:.2}), lock {}, state {}",
                                            o.tag, o.position[0], o.position[1], state.lock, state.state
                                        )
                                    })
                                })
                                .collect();
                            held.push(format!("doors pressed that stayed shut (not planned through): {}", tags.join(", ")));
                        }
                        if let Some((why, at)) = stilled.filter(|(_, at)| now < at + 2000) {
                            held.push(format!("held still by the driver: {why} ({:.1} s ago)", (now - at) as f64 / 1000.0));
                        }
                        break;
                    }
                    // A walk held by a closed door on its way (toward the
                    // corner it makes for, no farther): turned to the door and
                    // into it, use pressed, as a player clicks it.
                    let ahead = |w: &Walker, door: [f32; 3]| {
                        w.waypoint.is_some_and(|corner| {
                            let corner = corner.map(|v| v * UNIT);
                            let (to_door, to_corner) = (
                                [door[0] - me.position[0], door[1] - me.position[1]],
                                [corner[0] - me.position[0], corner[1] - me.position[1]],
                            );
                            to_door[0] * to_corner[0] + to_door[1] * to_corner[1] > 0.0
                                && flat(door, me.position) <= flat(corner, me.position)
                        })
                    };
                    if let (Some(w), Some((_, door))) = (walker.as_ref(), door)
                        && w.stalls > 0
                        && command.forward != 0
                        && ahead(w, door)
                    {
                        command = Command {
                            forward: 127,
                            buttons: command.buttons | pulse,
                            ..facing(&player, door)
                        };
                    }
                    // A walk held up with a foe at hand (a rakghoul in the
                    // way), or any goal with one at hand going for the party
                    // (kath hounds at the grove's way out: no leaving in a
                    // fight): fought, as a player would, by the fight goal's
                    // rules (`Chase`), then on. Held in combat, one going for
                    // the party farther off is gone to and fought too (insane
                    // Selkath shooting across Hrakert's airlock from 6 m: no
                    // leaving while they live). Going for the party: the
                    // player, or a companion (Mandalorians on Kashyyyk's
                    // companions while the player waits at the way out).
                    let set_on = |id: sim::SimId| {
                        last.get(id)
                            .and_then(|e| e.fighter)
                            .and_then(|f| f.target)
                            .is_some_and(|t| party.contains(&t.0))
                    };
                    // Farther off only while the goal stands (a walking
                    // goal arrived; one that walks nowhere): a walk on its
                    // way is not led off after every foe in sight (the
                    // Gamorreans' camp), and only after one a way leads to
                    // (not a droid behind Davik's cage).
                    let standing = if walks {
                        walker.as_ref().is_some_and(|w| w.arrived(&player))
                    } else {
                        true
                    };
                    let reach_back = if last.in_combat && (standing || back.foe.is_some()) {
                        FIGHT_BACK_M
                    } else {
                        BLOCKER_M
                    };
                    let foe_back = (!matches!(goal, Goal::Fight(_)) && talking.is_none())
                        .then(|| hostiles(&here, &world, me.position, reach_back))
                        .and_then(|foes| back.pick(me.position, foes,|(id, at)| {
                            let d = flat(*at, me.position);
                            let near = d <= BLOCKER_M;
                            // Beyond `FIGHT_NEAR_M` only one at close
                            // quarters with the one it goes for (a
                            // companion's fight), not a gunner on a dune.
                            // Or one at a companion from near it, a gunner
                            // too (troopers at Carth by the Leviathan's
                            // hangar lift, the party waited for there).
                            let engaged = d <= FIGHT_NEAR_M
                                || last
                                    .get(*id)
                                    .and_then(|e| e.fighter)
                                    .and_then(|f| f.target)
                                    .and_then(|t| last.get(t))
                                    .is_some_and(|t| {
                                        let reach =
                                            if t.id == me.id { BLOCKER_M } else { FIGHT_NEAR_M };
                                        flat(t.position, *at) <= reach
                                    });
                            !back.given_up(&here, &player, *id)
                                && ((set_on(*id)
                                    && engaged
                                    && (near || reach.ways(&here, &player, (*id, *at), now).1))
                                    || near && walker.as_ref().is_some_and(|w| w.stalls > 2))
                        }));
                    // A door or placeable a script's attack has the player
                    // strike (`SimWorld::assigned_strike`): the sim strikes
                    // it once the player stands within its reach, the
                    // player's legs being its seat's. KOTOR's character walks
                    // up to it by itself, so the harness walks there,
                    // whatever the goal (TSL's Peragus shift console,
                    // "[Destroy the console]").
                    let assigned = world
                        .assigned_strike(me.id)
                        .filter(|_| talking.is_none())
                        .and_then(|id| last.get(id))
                        .filter(|e| {
                            matches!(
                                e.kind,
                                sim::Kind::Object(ObjectKind::Door | ObjectKind::Placeable, _)
                            )
                        })
                        .map(|e| e.position)
                        .filter(|at| flat(*at, me.position) > sim::script::DOOR_REACH);
                    match foe_back {
                        None => {
                            back = Chase::default();
                            match assigned {
                                None => back_walker = None,
                                Some(at) => {
                                    let w = walk_for(&mut back_walker, at, || here.approach(&player, at));
                                    let walk = if !w.arrived(&player) && here.leads(player.origin, w.goal) {
                                        w.command_eager(here.level, &player, now, &|index| here.to_try(index))
                                    } else {
                                        Command {
                                            forward: 127,
                                            ..facing(&player, at)
                                        }
                                    };
                                    command = Command {
                                        buttons: command.buttons | pulse,
                                        ..walk
                                    };
                                    walker = None;
                                }
                            }
                        }
                        Some(foe) => {
                            let (strike, stepping) = back.strike(
                                &mut back_walker, &here, &world, &mut reach, &player, &me, foe, now,
                                &mut acts,
                            );
                            command = Command {
                                buttons: command.buttons
                                    | strike.buttons
                                    | if stepping { pulse } else { 0 },
                                ..strike
                            };
                            // The goal's own way is planned afresh after.
                            if stepping {
                                walker = None;
                            }
                        }
                    }
                    // A walk held up by a body that is no foe (a wandering
                    // kath hound face to face in a gully): back off aside a
                    // moment, then plan again, as a player steps round.
                    // (Or a placeable: Dantooine's rubble on a route's corner;
                    // or nothing, a walk stalled on and on: a fight's chase
                    // stepped up onto a steep lip above Kashyyyk's walkway,
                    // that the way down from slides along.)
                    let body_in_way = last.entities.iter().any(|e| {
                        e.id != me.id
                            && e.id != avatar.id
                            && (e.fighter.is_some_and(|f| f.alive())
                                || matches!(e.kind, sim::Kind::Object(ObjectKind::Placeable, _)))
                            && flat(e.position, me.position) <= BODY_M
                    });
                    if let Some(w) = walker.as_ref()
                        && (w.stalls > 2 && (body_in_way || unwedges >= 2) || w.stalls > LOST_STALLS)
                        && now >= backing_until + BACKING_MS * 2
                    {
                        unwedges = 0;
                        backing_until = now + BACKING_MS;
                        backing_side = -backing_side;
                    }
                    if now < backing_until {
                        command.forward = -64;
                        command.right = backing_side;
                        if now + u64::from(rate.msec(tick)) >= backing_until {
                            walker = None;
                        }
                    }
                    // A walk stalled again and again with no body in its way,
                    // the route's first corner close by and not the one it
                    // makes for (a nook a backing off or a blow put it in,
                    // off that corner's line: Endar Spire's footlocker room):
                    // straight at that corner a moment, then planned again,
                    // as a player steps back out.
                    if let Some(w) = walker.as_ref()
                        && w.stalls > 2
                        && !body_in_way
                        && now >= backing_until
                        && now >= unwedge_until + UNWEDGE_MS * 2
                        && unwedges < 2
                        && let Some(corner) = here
                            .route(player.origin, w.goal, false)
                            .and_then(|corners| corners.first().copied())
                            .map(|corner| corner.map(|v| v * UNIT))
                        && flat(corner, me.position) <= UNWEDGE_M
                        && w.waypoint
                            .is_some_and(|aim| flat(aim.map(|v| v * UNIT), corner) > UNWEDGE_M)
                    {
                        unwedge = corner;
                        unwedge_until = now + UNWEDGE_MS;
                        unwedges += 1;
                    }
                    if now < unwedge_until {
                        command = Command {
                            forward: 127,
                            buttons: command.buttons,
                            ..facing(&player, unwedge)
                        };
                        if now + u64::from(rate.msec(tick)) >= unwedge_until {
                            walker = None;
                        }
                    }
                    // Off the navigation mesh (on a ledge a blow put it on,
                    // resting on furniture no step lands on: the Star Forge's
                    // second deck; no way leads anywhere from there, no foe
                    // is reached): back onto it first, as KOTOR's walkmesh
                    // never lets one off. On a raised piece of it its walk's
                    // goal is not on (the Ebon Hawk's medical bed, whose
                    // steps let T3-M4 up as a corridor too narrow for its box
                    // turned its walk across the bed, and the Exile lying
                    // there held it): down off it on the goal's side, at
                    // once (no way leads from there to the goal). Off the
                    // mesh, only with no walk, or one stalled again and
                    // again (a walk under way goes its way).
                    let stranded = || walker.as_ref().is_none_or(|w| w.stalls > 2);
                    let lifted = || {
                        walker
                            .as_ref()
                            .and_then(|w| sim::walk::down_to(&level, player.origin, w.goal))
                    };
                    if talking.is_none()
                        && !matches!(goal, Goal::Race | Goal::Turrets)
                        && let Some((spot, why)) = if on_mesh {
                            lifted().map(|spot| (spot, "on a raised floor its goal is not on, stepping down"))
                        } else {
                            sim::walk::back_on(&level, player.origin)
                                .filter(|_| stranded())
                                .map(|spot| (spot, "off the navigation mesh, stepping back on"))
                        }
                    {
                        command = Command {
                            buttons: command.buttons,
                            ..sim::walk::toward(&player, spot)
                        };
                        walker = None;
                        stilled = Some((why, now));
                    }
                    // Standing idle against one going somewhere (no foe, not
                    // of the party: Malak on his way along the platform's
                    // narrow ledge to a captive Jedi): out of its way, as a
                    // player steps aside.
                    if talking.is_none()
                        && matches!(
                            goal,
                            Goal::Wait(_)
                                | Goal::Fight(_)
                                | Goal::Until(..)
                                | Goal::Gone(..)
                                | Goal::Gather
                                | Goal::Credits
                        )
                        && command.forward == 0
                        && command.right == 0
                        && command.buttons & buttons::ATTACK == 0
                        && let Some(other) = last.entities.iter().find(|e| {
                            e.id != me.id
                                && e.id != avatar.id
                                && e.fighter.is_some_and(|f| f.alive() && f.mind.leader.is_none())
                                && flat(e.position, me.position) <= BODY_M
                                && hostiles(&here, &world, me.position, BODY_M).iter().all(|(id, _)| *id != e.id)
                        })
                    {
                        command = Command {
                            forward: -64,
                            buttons: command.buttons,
                            ..facing(&player, other.position)
                        };
                        stilled = Some(("stepping out of the way of one passing", now));
                    }
                    // Code a script left the player to run in a moment (a
                    // scene's last line, the screen faded: Kashyyyk's guard
                    // sends the party to Chuundar a second after the party
                    // screen closes) is waited for, as a player waits out
                    // the dark, up to a scene's time.
                    let state = world.script_state();
                    let cued = [me.id.0, avatar.id.0]
                        .iter()
                        .any(|id| state.due_soon(*id, now, CUE_MS));
                    let since = *cued_since.get_or_insert(now);
                    if !cued {
                        cued_since = None;
                    } else if now < since + SCENE_WAIT_MS {
                        command.forward = 0;
                        command.right = 0;
                        stilled = Some(("code of its own due (a scene's)", now));
                    }
                    // Out of the module only by an `exit`, and by its own way
                    // out (a fight's chase of a foe fleeing to the map's edge
                    // stops short of it; a walk across the Dune Sea passes the
                    // way back to Anchorhead by its arrival). A walk to a way
                    // out takes it too (`to k36_trg_k34`, the Shyrack cave's
                    // mouth on Korriban: held 3.6 m short).
                    let way_out = match goal {
                        Goal::Exit(Target::Tag(tag, nth)) | Goal::To(Target::Tag(tag, nth)) => {
                            find(&here, tag, *nth).map(|(index, _)| index)
                        }
                        _ => None,
                    };
                    let ahead = (!matches!(goal, Goal::Race | Goal::Turrets))
                        .then(|| {
                            exit_ahead(
                                &here,
                                me.position,
                                player.velocity,
                                player.view[1],
                                &command,
                                way_out,
                            )
                        })
                        .flatten();
                    if let Some(ahead) = ahead {
                        (command.forward, command.right) = match ahead {
                            Ahead::Command { slides: false } => (0, 0),
                            Ahead::Command { slides: true } | Ahead::Slides => {
                                braking(player.velocity, player.view[1])
                            }
                        };
                        stilled = Some(("a way out ahead it is not to take", now));
                    }
                    command.reply = reply;
                    // The use key acts on what it is pressed for or is not
                    // pressed: JA's key uses whatever its look meets first,
                    // and a press for Manaan's pressure door used the water
                    // button beside it, the water sent on twice. One already
                    // spent (held, it acted) acts on nothing more.
                    let spent = player.flags & sim::motion::motion_flags::USE_SPENT != 0;
                    if command.buttons & buttons::USE != 0
                        && !spent
                        && !pressed_for.is_some_and(|index| key_meets(&mut world, &last, &command, index))
                    {
                        command.buttons &= !buttons::USE;
                    }
                    // A pazaak match is played as a player would.
                    acts.extend(
                        last.pazaak_of(Seat(0))
                            .and_then(sim::pazaak::Match::advice)
                            .map(Action::Pazaak),
                    );
                    input.insert(Seat(0), command);
                    for action in acts.drain(..) {
                        input.act(Seat(0), action);
                    }
                    let before = module_of(&last);
                    let was_tabled = last.pazaak_of(Seat(0)).is_some();
                    last = step_on(&mut world, &mut tick, &rate, (&mut recorder, &mut motion), (input, &mut watch, &last));
                    // What the sim heard used by the player this step: the
                    // goal's target, and doors that refused it.
                    for use_heard in world.uses_heard() {
                        if !party[..2].contains(&use_heard.by.0) {
                            continue;
                        }
                        if Some(use_heard.object) == target_id && pressed_ever {
                            heard = Some(use_heard.came);
                        }
                        if use_heard.came == sim::UseCame::Refused
                            && let Some(sim::Kind::Object(ObjectKind::Door, index)) =
                                last.get(use_heard.object).map(|e| e.kind)
                        {
                            refusals.insert(index);
                        }
                    }
                    let at = format!("{:>8.2}s", last.time_ms as f64 / 1000.0);
                    if was_tabled && last.pazaak_of(Seat(0)).is_none() {
                        let state = world.script_state();
                        let result = if state.pazaak_won { "won" } else { "lost" };
                        said.push(format!("{at} [pazaak {result}: credits {}]", state.gold));
                    }
                    for (_, notice) in world.take_notices() {
                        match notice {
                            Notice::Said(text) => said.push(format!("{at} {text}")),
                            Notice::Line(strref) => {
                                let line = installer.text(strref).unwrap_or_else(|| format!("line {strref}"));
                                if bench_shown.is_some() {
                                    bench_refused = Some(line.clone());
                                }
                                said.push(format!("{at} {line}"));
                            }
                            Notice::Check { .. }
                            | Notice::TrapSeen { .. }
                            | Notice::Experience(_)
                            | Notice::LevelRefused
                            | Notice::Gone { .. }
                            | Notice::NotOpen
                            | Notice::OutOfReach
                            | Notice::Empty => said.push(format!("{at} {notice:?}")),
                            // Closed as a player would, taking whom the
                            // route asked for, else the party as it is.
                            Notice::Show(Screen::GalaxyMap {
                                planet,
                                available,
                                selectable,
                            }) => {
                                map_shown = Some(available & selectable);
                                said.push(format!("{at} [the galaxy map, at planet {planet}]"));
                            }
                            Notice::Show(Screen::PartySelection(_)) => {
                                acts.push(Action::ChooseParty(chosen.take()));
                                said.push(format!("{at} [the party selection screen, closed]"));
                            }
                            Notice::Show(screen @ (Screen::Workbench(_) | Screen::Upgrade { .. })) => {
                                if let Screen::Workbench(bench) = &screen {
                                    let pool = if bench.lab { "chemicals" } else { "components" };
                                    said.push(format!("{at} [the {} open: {} {pool}]", bench.list, bench.stock));
                                }
                                bench_shown = Some(screen);
                            }
                            refused @ (Notice::CannotUse | Notice::CannotUpgrade) if bench_shown.is_some() => {
                                said.push(format!("{at} {refused:?}"));
                                bench_refused = Some(format!("{refused:?}"));
                            }
                            Notice::Show(Screen::Pazaak) => {
                                said.push(format!("{at} [a pazaak match]"));
                            }
                            Notice::Show(screen) if screen.rolls_credits() => {
                                credits_rolled = true;
                                let movie = screen.movie_name().map_or_else(String::new, |name| format!(" ({name})"));
                                said.push(format!("{at} [the credits{movie}]"));
                            }
                            Notice::Show(Screen::End) if credits_rolled => {
                                game_won = true;
                                said.push(format!("{at} [the end: the game won]"));
                            }
                            Notice::Show(Screen::End) => {
                                game_over = true;
                                said.push(format!("{at} [the end: the game over, not won]"));
                            }
                            Notice::Show(Screen::Death) => {
                                over = true;
                                said.push(format!("{at} [the death screen]"));
                            }
                            _ => {}
                        }
                    }
                    if let Some(transition) = last.transition.clone() {
                        bench_shown = None;
                        let via = last
                            .get(transition.via)
                            .and_then(|entity| Here::new(&level, &last, &refused).facts_of(entity));
                        left_by_exit = via.as_ref().is_some_and(|data| data.exit().is_some());
                        left_through = via.map(|data| data.tag.clone());
                        transcribed =
                            transcribe(&installer, &world, (&names, &last), transcribed, &mut lines);
                        let request = installer.request_from(
                            &last,
                            &transition.module,
                            Some(&transition.tag),
                        );
                        let prepared = installer.prepare(&request, false)?;
                        let announcement = prepared
                            .plan
                            .commit(&mut world)
                            .map_err(|e| e.to_string())?;
                        recorder.install(announcement);
                        last = world.snapshot();
                        names = crate::transcript::speakers(&last, world.level());
                        fallen.clear();
                        walker = None;
                        outln!(
                            "{:>8.2}s  {before} -> {} (at {})",
                            last.time_ms as f64 / 1000.0,
                            module_of(&last),
                            transition.tag
                        );
                    }
                }
                if met && let Goal::Use(Target::Tag(tag, _)) = goal {
                    used_tags.insert(tag.to_ascii_lowercase());
                }
                let outcome = Outcome {
                    goal: text.clone(),
                    module,
                    started_ms,
                    ended_ms: last.time_ms,
                    wall_ms: started_wall.elapsed().as_millis(),
                    met,
                    held,
                };
                if tick > started_tick {
                    stepped_from = started_ms;
                }
                outln!(
                    "{:>8.2}s  {:<8} {} ({:.1} s)",
                    outcome.ended_ms as f64 / 1000.0,
                    if outcome.met { "ok" } else { "BLOCKED" },
                    outcome.goal,
                    (outcome.ended_ms - outcome.started_ms) as f64 / 1000.0
                );
                for line in &outcome.held {
                    outln!("            {line}");
                }
                let stop = down || (options.fail_fast && !outcome.met);
                outcomes.push(outcome);
                if stop {
                    break;
                }
            }
            // Actions the last goals left (a leader made, a level asked
            // for) go out on one more step.
            if !acts.is_empty() {
                let mut input = TickInput::default();
                for action in acts.drain(..) {
                    input.act(Seat(0), action);
                }
                last = step_on(&mut world, &mut tick, &rate, (&mut recorder, &mut motion), (input, &mut watch, &last));
            }
            let report = world.script_report();
            transcribe(&installer, &world, (&names, &last), transcribed, &mut lines);
            lines.extend(said.iter().map(|s| format!("{s} (to the player)")));
            let blockers: Vec<&Outcome> = outcomes.iter().filter(|o| !o.met).collect();
            outln!(
                "playthrough: {} goals, {} met, {} blocked; {:.1} s of play; faults {}, runaways {}, aborted {}, gaps {}, missing scripts {}",
                outcomes.len(),
                outcomes.len() - blockers.len(),
                blockers.len(),
                last.time_ms as f64 / 1000.0,
                report.faults.len(),
                report.runaways.len(),
                report.aborted.len(),
                report.gaps.len(),
                report.missing.len()
            );
            for fault in &report.faults {
                outln!("  fault: {fault}");
            }
            for runaway in &report.runaways {
                outln!("  runaway: {runaway}");
            }
            for aborted in report.aborted.iter().collect::<BTreeSet<_>>() {
                outln!("  aborted (the game's own error): {aborted}");
            }
            let mut gaps: Vec<(&String, &usize)> = report.gaps.iter().collect();
            gaps.sort_by(|a, b| b.1.cmp(a.1).then(a.0.cmp(b.0)));
            outln!(
                "  gaps: {}",
                gaps.iter()
                    .map(|(name, count)| format!("{name} {count}"))
                    .collect::<Vec<_>>()
                    .join(", ")
            );
            let goals_json: Vec<serde_json::Value> = outcomes
                .iter()
                .map(|o| {
                    json!({
                        "goal": o.goal, "module": o.module, "started_ms": o.started_ms,
                        "ended_ms": o.ended_ms, "wall_ms": o.wall_ms, "met": o.met, "held": o.held,
                    })
                })
                .collect();
            let facts = json!({
                "verb": "playthrough",
                "game": options.game,
                "module": options.module,
                "arrive": options.arrive,
                "goals": goals_json,
                "blocked": blockers.len(),
                "ms": last.time_ms,
                "wall_ms": began_wall.elapsed().as_millis(),
                "faults": report.faults,
                "runaways": report.runaways,
                "aborted": report.aborted,
                "gaps": report.gaps,
                "missing": report.missing,
                "runs": report.runs,
                "globals": world.script_state().numbers,
                "flags": world.script_state().booleans,
                "party": goods(&world, &last),
                "motion": motion.facts(),
                "droids": watch.finish(&world),
            });
            let all_met = blockers.is_empty();
            let mut trace = report.trace_lines().join("\n");
            trace.push('\n');
            use crate::motion::Episode;
            let mut moves: String = motion
                .kept()
                .map(Episode::line)
                .chain(motion.trace.iter().cloned())
                .map(|line| line + "\n")
                .collect();
            moves.insert_str(0, &format!(
                "{} moving steps, {} sliding, {} episodes\n",
                motion.moving_steps,
                motion.sliding_steps,
                motion.kept().count()
            ));
            Ok((facts, lines, (trace, moves), recorder.finish(&last), all_met))
        };
    match go() {
        Ok((facts, lines, (trace, moves), recording, all_met)) => {
            if let Some(folder) = crate::runtime::write_facts(facts) {
                let mut body = lines.join("\n");
                body.push('\n');
                for (name, bytes) in [
                    ("transcript.txt", body.into_bytes()),
                    ("trace.txt", trace.into_bytes()),
                    ("motion.txt", moves.into_bytes()),
                    ("inputs.kjkr", recording.encode()),
                ] {
                    let path = folder.join(name);
                    if let Err(error) = std::fs::write(&path, bytes) {
                        eprintln!("{}: {error}", path.display());
                    }
                }
                eprintln!("transcript and recording: {}", folder.display());
            }
            if all_met {
                ExitCode::SUCCESS
            } else {
                ExitCode::FAILURE
            }
        }
        Err(error) => {
            eprintln!("{error}");
            ExitCode::from(2)
        }
    }
}

/// What the harness's click on an object asks for a goal, as E takes the
/// target block's first choice (`sim::target::choices`): a creature is
/// talked to, a lock picked for `unlock`, a container opened for `put`.
/// None for what takes no click (a trigger).
fn click(goal: &Goal, data: &ObjectData, state: sim::ObjectState) -> Option<sim::target::Choice> {
    use sim::target::Choice;
    match (goal, data.kind) {
        // A lock struck is walked up to as one picked: within a blade's
        // reach of it.
        (Goal::Unlock(..) | Goal::Bash(_), _) => Some(Choice::Unlock { spike: None }),
        (Goal::Mine(_), _) => Some(Choice::Mine),
        (Goal::Put(..), _) => Some(Choice::Open),
        (_, ObjectKind::Creature) => Some(Choice::Talk),
        (_, ObjectKind::Door | ObjectKind::Placeable) => {
            sim::target::choices(data.kind, data, state, false, None, None)
                .first()
                .copied()
        }
        _ => None,
    }
}

/// A chase's walk to a foe that shifted beyond `chase_replan_m` since it was
/// planned: made afresh while the walk makes headway, kept while it stalls
/// (no headway for half a stall's time, or getting round one), so its
/// sidesteps and skipped corners run (a kiting gunner's fresh plan every
/// second kept the player pressed into a corner of Hrakert's corridors).
fn replans(
    chase_once: &dyn Fn(sim::combat::Sheet, f32, Prepare<'_>) -> Chased,
) -> sim::probe::Checks {
    let d = 8.0;
    let shift = CHASE_M + 2.0;
    [
        ("chase_replans_a_shifted_foe_with_headway", 0u8, true),
        ("chase_keeps_its_plan_while_stalling", 1, false),
    ]
    .into_iter()
    .map(|(name, stalls, replans)| {
        let planned = std::cell::Cell::new([0.0f32; 3]);
        let after = chase_once(
            sim::combat::Sheet::dev_jedi(),
            d,
            &|chase, walker, (id, at), player| {
                let was = [at[0], at[1] + shift, at[2]];
                let goal = [was[0] / UNIT, was[1] / UNIT, player.origin[2]];
                let mut walk = Walker::new(goal);
                walk.stalls = stalls;
                walk.since_ms = 0;
                planned.set(walk.goal);
                *walker = Some(walk);
                chase.planned = Some((id, was));
            },
        );
        let result = match after.and_then(|(.., walker)| walker) {
            None => Err("no player on the arena".to_string()),
            Some(walker) => {
                let replanned = walker.goal != planned.get();
                let detail = format!(
                    "a foe {shift:.1} m from where the walk was planned to, {stalls} stalls: planned afresh {replanned}"
                );
                if replanned == replans {
                    Ok(detail)
                } else {
                    Err(detail)
                }
            }
        };
        (name, result)
    })
    .collect()
}

/// The report's conversation lines from `from` on, as the running
/// module's conversations and speakers have them (the player's tokens the
/// first seat's in `last`), added to `lines`: how many the report has.
fn transcribe(
    installer: &Installer,
    world: &sim::SimWorld,
    (names, last): (&std::collections::BTreeMap<u32, String>, &Snapshot),
    from: usize,
    lines: &mut Vec<String>,
) -> usize {
    let talk = &world.script_report().talk;
    if let Some(library) = world.level().scripts() {
        let text = |strref| installer.text(strref);
        let mut tokens = crate::transcript::tokens(&talk[..from.min(talk.len())]);
        let player = sim::persona::of(&last.personas, Seat(0));
        lines.extend(talk.iter().skip(from).flat_map(|line| {
            crate::transcript::transcript(&text, library, names, (&mut tokens, player), line)
        }));
    }
    talk.len()
}

/// What reply `reply` of the running module's `dialog` says.
fn reply_text(installer: &Installer, world: &sim::SimWorld, dialog: &str, reply: u32) -> String {
    let talk = |strref| installer.text(strref);
    world
        .level()
        .scripts()
        .and_then(|library| library.dialogue(dialog))
        .and_then(|dlg| dlg.replies.get(reply as usize))
        .map_or_else(String::new, |node| crate::transcript::text(&talk, node))
}

/// A run's script trace and its moves' report (`motion.txt`).
type Logs = (String, String);

/// One step of the authority with `input`, recorded and its moves watched.
fn step_on(
    world: &mut sim::SimWorld,
    tick: &mut u64,
    rate: &TickRate,
    (recorder, motion): (&mut sim::Recorder, &mut crate::motion::Motion),
    (mut input, watch, last): (TickInput, &mut chaos::Watch, &Snapshot),
) -> Snapshot {
    watch.before(world, last, &mut input);
    *tick += 1;
    let snapshot = sim::step(
        world,
        *tick,
        &input,
        rate.msec(*tick),
        sim::StepReason::Authority,
    );
    recorder.record(rate.msec(*tick), &input, &snapshot);
    motion.watch(&snapshot, world.level(), rate.msec(*tick));
    watch.after(last, &snapshot);
    snapshot
}
