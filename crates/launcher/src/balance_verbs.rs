//! `kjk combat balance`: numbers for saber duel balance. Two fighters on
//! the grey-box arena's floor (`game::arena`, on the data of a module of
//! the game: K1's Endar Spire, TSL's Ebon Hawk), fought many times from
//! many seeds under each saber rule set (JA, MDA), the sim stepped on
//! the authority with nothing drawn; each fight ends at a death or the time
//! cap.
//!
//! - **Fighters** (`fighters`): side b is always a creature, side a a
//!   creature when the creature AI plays it (`--control ai`) or the seat's
//!   player when a bot does (`--control bot:<skill>`, `bot`). A template
//!   fighter is called in as the arena calls one (`ArenaOrder::Spawn`: its
//!   KOTOR sheet as the game makes it, killable); a player character
//!   (`guardian:12:medium`) is levelled as the game levels one, and as a
//!   creature rides a template's body (side b's, else K1's or TSL's own
//!   Dark Jedi) with its sheet put in. Side a is on the player's side, side
//!   b hostile, side a called in first so each creature's scripts perceive
//!   the other as a foe. Played by the AI, side a is the seat's companion:
//!   two creatures outside the party fight KOTOR's choreographed rounds
//!   (`combat::rounds`), not the blades this measures; the seat's player
//!   stands aside, neutral and unkillable.
//! - **KOTOR's reference** (`reference`): every attack KOTOR gives each
//!   side a round (both hands, Flurry's, Speed's), each one's chance to hit
//!   and damage, and the rounds it needs to kill the other, from the sheets
//!   fought with.
//! - **Powers**: the creatures cast as their minds choose; `--no-powers`
//!   holds every talent (powers, grenades, medpacs, feat picks) by the
//!   mind's own wait (`Mind::cast_after`), as the duel scenes do: a start
//!   state, not a rule changed. The bot never casts.
//! - **Measures** (`tally`): read off the snapshots and the blades' tap
//!   (`saber::duel::tap`, the probes' read-only record of what each blow
//!   meeting a blade decided); the sim steps as it does in play.
//!
//! Prints a table per rule set and writes every fight's record to
//! `facts.json` in the run folder (and `--out`), with seed 0's fight under
//! each rule set as `<rules>.kjkr` beside it (`kjk play --replay`).
//! `KJK_BALANCE_TRACE=<ms>` prints seed 0's fighters to stderr that often
//! (position, HP, saber move, meters, mind, command, last event).

mod bot;
mod fighters;
mod reference;
mod tally;

use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};

use serde_json::{Value, json};
use sim::action::{Action, ArenaOrder};
use sim::combat::saber::duel::tap;
use sim::combat::passive::Game;
use sim::combat::saber::{Kit, SaberRules, Style};
use sim::combat::sheet::{factions, flags};
use sim::combat::{Fighter, Sheet, force, mind};
use sim::module::ObjectKind;
use sim::motion::{Anim, MotionState};
use sim::{
    Command, Kind, Level, Recorder, Recording, Seat, SimId, SimWorld, Snapshot, StepReason,
    TickInput, TickRate, step,
};
use vfs::Discovery;

use bot::{Bot, SKILLS, Skill};
use fighters::Spec;
use tally::Tally;

#[derive(clap::Args, Debug, Clone)]
pub(crate) struct Options {
    /// `k1` or `tsl`
    #[arg(long, default_value = "k1")]
    game: String,
    /// The module whose data (templates, items, classes, scripts) the
    /// arena runs on: end_m01aa for K1, 001ebo for TSL without one
    #[arg(long)]
    module: Option<String>,
    /// Side a: a creature template (`g_darkjedi01`), or a player character
    /// `jedi:<guardian|consular|sentinel>:<level>:<fast|medium|strong|dual|staff>`
    /// with an optional base class for its first levels (`:soldier6`)
    #[arg(long)]
    a: String,
    /// Side b, a creature: as side a
    #[arg(long)]
    b: String,
    /// Who plays side a: `ai` (the creature AI) or `bot:<novice|average|expert>`
    #[arg(long, default_value = "ai")]
    control: String,
    /// The saber rule sets, each fought `--seeds` times
    #[arg(long, value_delimiter = ',', default_value = "ja,advanced")]
    rules: Vec<String>,
    /// Fights per rule set, seeds 0 up
    #[arg(long, default_value_t = 20)]
    seeds: u64,
    /// Sim seconds a fight lasts at most
    #[arg(long, default_value_t = 120)]
    seconds: u64,
    /// Also write the table, the facts and the replays here
    #[arg(long)]
    out: Option<PathBuf>,
    /// The creatures cast nothing (and use no item or feat talent)
    #[arg(long)]
    no_powers: bool,
}

/// Who plays side a.
#[derive(Debug, Clone, Copy)]
enum Control {
    Ai,
    Bot(Skill),
}

impl Control {
    fn parse(text: &str) -> Result<Control, String> {
        if text == "ai" {
            return Ok(Control::Ai);
        }
        text.strip_prefix("bot:")
            .and_then(|name| SKILLS.iter().find(|skill| skill.name == name))
            .map(|skill| Control::Bot(*skill))
            .ok_or(format!(
                "--control {text}: ai, bot:novice, bot:average or bot:expert"
            ))
    }

    fn name(self) -> String {
        match self {
            Control::Ai => "ai".into(),
            Control::Bot(skill) => format!("bot:{}", skill.name),
        }
    }
}

/// How far apart the two start (metres), and where a bystander seat
/// stands (metres on the arena's floor).
const APART_M: f32 = 4.0;
const BEHIND_M: f32 = 2.0;
const ASIDE_M: [f32; 2] = [8.0, -12.0];
/// The creature the AI's player characters ride when side b is no template.
fn carrier(game: &str) -> &'static str {
    if game == "tsl" {
        "g_darkjedi001"
    } else {
        "g_darkjedi01"
    }
}

/// A side as set up once for every fight.
#[derive(Clone)]
struct Fighting {
    spec: Spec,
    /// The template it is called in as (a player character's body).
    template: Option<String>,
    /// The sheet put in place of the template's (a player character's).
    sheet: Option<Sheet>,
    style: Option<Style>,
}

/// What every fight starts from.
struct Ground {
    level: Level,
    sides: [Fighting; 2],
    control: Control,
    no_powers: bool,
    cap_ms: u64,
    game: Game,
}

/// One fight's record, and its recording when it was kept.
struct Fought {
    record: Value,
    recording: Option<Recording>,
}

fn arena_level(discovery: &Discovery, game: &str, module: &str) -> Result<(Level, Option<fmt_odyssey::twoda::TwoDa>), String> {
    let installer = crate::scenes::installer(discovery, game)?;
    let prepared = installer.prepare_one(module, None)?;
    let anims = prepared.plan.level.anims().clone();
    let level = kjk_game::arena::level(&prepared.plan.level, anims)?;
    let classes = installer.base_resolver()?.two_da("classes");
    Ok((level, classes))
}

/// Each side as every fight takes it (see the module documentation).
fn sides(
    level: &Level,
    classes: Option<&fmt_odyssey::twoda::TwoDa>,
    game: &str,
    specs: [Spec; 2],
    control: Control,
) -> Result<[Fighting; 2], String> {
    let library = level.scripts().ok_or("the arena has no scripts")?;
    let body = specs[1]
        .template()
        .map_or(carrier(game).to_string(), str::to_string);
    let fighting = |spec: &Spec, creature: bool| -> Result<Fighting, String> {
        match spec {
            Spec::Template(template) => {
                let made = library
                    .creature(template)
                    .ok_or(format!("{template}: no such creature template"))?;
                // The bot's seat plays the template's own sheet.
                let sheet = (!creature)
                    .then(|| made.sheet.ok_or(format!("{template}: no sheet")))
                    .transpose()?;
                Ok(Fighting {
                    spec: spec.clone(),
                    template: creature.then(|| template.clone()),
                    sheet,
                    style: None,
                })
            }
            Spec::Character(build) => {
                let table = classes.ok_or("classes.2da: not read")?;
                let scores = fighters::scores(table, fighters::first_class(build));
                let sheet = fighters::sheet(build, library, scores)?;
                if creature && library.creature(&body).is_none() {
                    return Err(format!("{body}: no such creature template to carry {}", spec.name()));
                }
                Ok(Fighting {
                    spec: spec.clone(),
                    template: creature.then(|| body.clone()),
                    sheet: Some(sheet),
                    style: build.weapon.is_none().then_some(build.style),
                })
            }
        }
    };
    let a_creature = matches!(control, Control::Ai);
    Ok([fighting(&specs[0], a_creature)?, fighting(&specs[1], true)?])
}

/// `sheet` killable on `faction`'s side.
fn sided(mut sheet: Sheet, faction: u8) -> Sheet {
    sheet.faction = faction;
    sheet.flags &= !(flags::PLOT | flags::MIN_ONE_HP | flags::IMMORTAL);
    sheet
}

fn metres(m: f32) -> f32 {
    m / collision::UNIT
}

/// `world` stepped `ticks` ticks, the seat standing, `called` (templates)
/// called in on the first; the creatures standing that were not before.
fn call_in(world: &mut SimWorld, called: Option<&str>, ticks: u32) -> Vec<SimId> {
    let rate = TickRate::default();
    let creatures = |snapshot: &Snapshot| -> Vec<SimId> {
        snapshot
            .entities
            .iter()
            .filter(|e| matches!(e.kind, Kind::Object(ObjectKind::Creature, _)))
            .filter(|e| e.fighter.is_some_and(|f| f.alive()))
            .map(|e| e.id)
            .collect()
    };
    let before = creatures(&world.snapshot());
    for at in 0..ticks {
        let mut input = TickInput::default();
        input.insert(Seat(0), Command::default());
        if let Some(template) = called.filter(|_| at == 0) {
            input.act(Seat(0), Action::Arena(ArenaOrder::Spawn(template.to_string())));
        }
        let tick = world.tick() + 1;
        step(world, tick, &input, rate.msec(tick), StepReason::Authority);
    }
    let mut made = creatures(&world.snapshot());
    made.retain(|id| !before.contains(id));
    made
}

/// `world` made again from `snapshot` as `edit` leaves it, its scripts'
/// state and rules kept.
fn edited(
    world: &mut SimWorld,
    level: &Level,
    edit: impl FnOnce(&mut Snapshot) -> Result<(), String>,
) -> Result<SimWorld, String> {
    let mut snapshot = world.snapshot();
    edit(&mut snapshot)?;
    let script = world.script_state().clone();
    let rules = world.saber_rules();
    let mut world = SimWorld::from_snapshot(&snapshot, level.clone()).map_err(|e| e.to_string())?;
    world.set_script_state(script);
    world.set_saber_rules(rules);
    Ok(world)
}

/// The entity `id` of `snapshot`, to change.
fn entity_mut(snapshot: &mut Snapshot, id: SimId) -> Result<&mut sim::EntityState, String> {
    snapshot
        .entities
        .iter_mut()
        .find(|e| e.id == id)
        .ok_or(format!("entity {} is gone", id.0))
}

/// `side`'s fighter set at `origin` facing `yaw`: on its side, its sheet
/// (a player character's in place of its body's), its style, casting
/// nothing with `--no-powers`.
fn set_side(ground: &Ground, snapshot: &mut Snapshot, side: usize, id: SimId, origin: sim::math::Vec3, yaw: f32) -> Result<(), String> {
    let now = snapshot.time_ms;
    let entity = entity_mut(snapshot, id)?;
    let fighting = &ground.sides[side];
    let faction = [factions::PLAYER, factions::HOSTILE_1][side];
    entity.player = Some(MotionState::standing(origin, yaw));
    let sheet = sided(
        fighting.sheet.or(entity.sheet).ok_or("a fighter without a sheet")?,
        faction,
    );
    let mut fighter = match (entity.fighter, fighting.sheet) {
        (Some(fighter), None) => fighter,
        _ => {
            let mut fresh = Fighter::of(&sheet);
            fresh.force.regen_at = now + force::regen_every(sheet.pool());
            if let Some(made) = entity.fighter {
                fresh.mind = made.mind;
            }
            fresh
        }
    };
    fighter.faction = Some(faction);
    // Side a played by the AI is the seat's companion: in the party, so it
    // fights with JA's blades, as the game's Jedi companions do; two
    // creatures outside the party fight KOTOR's choreographed rounds
    // (`combat::rounds`) instead.
    if side == 0 && entity.kind != Kind::Avatar(Seat(0)) {
        fighter.mind.leader = Some(Seat(0));
    }
    if fighting.style.is_some() {
        fighter.saber.style = fighting.style;
    }
    if ground.no_powers && entity.kind != Kind::Avatar(Seat(0)) {
        fighter.mind.cast_after = u64::MAX;
    }
    entity.sheet = Some(sheet);
    entity.fighter = Some(fighter);
    Ok(())
}

/// A fight's world under `rules` from `seed` (see the module
/// documentation): side a set first (the seat's player, or a creature
/// called in and turned to the player's side), then side b called in, so
/// each creature's scripts perceive its foe as one; side a at the seat's
/// spawn less `BEHIND_M`, side b `APART_M` from it, face to face, a
/// bystander seat set aside. Its snapshot, the scripts' state, and the two
/// sides' ids.
fn start(ground: &Ground, rules: SaberRules, seed: u64) -> Result<(SimWorld, Snapshot, sim::ScriptVmState, [SimId; 2]), String> {
    let bot = matches!(ground.control, Control::Bot(_));
    let seat_sheet = match (&ground.sides[0].sheet, bot) {
        (Some(sheet), true) => sided(*sheet, factions::PLAYER),
        _ => Sheet {
            faction: factions::NEUTRAL,
            flags: flags::IMMORTAL,
            ..Sheet::dev_soldier()
        },
    };
    let config = sim::WorldConfig {
        seed,
        seats: 1,
        wanderers: 0,
        spawn: kjk_game::arena::SPAWN,
        yaw: kjk_game::arena::YAW,
        sheet: seat_sheet,
        guests: sim::Guests::Avatars,
    };
    let mut world = SimWorld::new(config, ground.level.clone()).map_err(|e| e.to_string())?;
    world.set_saber_rules(rules);
    let snapshot = world.snapshot();
    let seat = snapshot.own(Seat(0)).ok_or("no seat")?;
    let seat_id = seat.id;
    let here = seat.player.ok_or("the seat has no body")?.origin;
    let yaw = kjk_game::arena::YAW;
    let (sin, cos) = sim::math::sin_cos_deg(yaw);
    let ahead = [cos, sin, 0.0];
    let at_a = sim::math::mad(here, -metres(BEHIND_M), ahead);
    let at_b = sim::math::mad(at_a, metres(APART_M), ahead);
    let a = if bot {
        seat_id
    } else {
        let template = ground.sides[0].template.as_deref();
        let made = call_in(&mut world, template, 2);
        let &[a] = &made[..] else {
            return Err(format!("side a: {} creatures stand", made.len()));
        };
        world = edited(&mut world, &ground.level, |s| set_side(ground, s, 0, a, at_a, yaw))?;
        a
    };
    if bot {
        world = edited(&mut world, &ground.level, |s| set_side(ground, s, 0, a, at_a, yaw))?;
    }
    let made = call_in(&mut world, ground.sides[1].template.as_deref(), 2);
    let &[b] = &made[..] else {
        return Err(format!("side b: {} creatures stand", made.len()));
    };
    let mut world = edited(&mut world, &ground.level, |s| {
        set_side(ground, s, 1, b, at_b, yaw + 180.0)?;
        if !bot {
            let seat = entity_mut(s, seat_id)?;
            let aside = [metres(ASIDE_M[0]), metres(ASIDE_M[1]), here[2]];
            seat.player = Some(MotionState::standing(aside, yaw));
        }
        Ok(())
    })?;
    let snapshot = world.snapshot();
    let script = world.script_state().clone();
    Ok((world, snapshot, script, [a, b]))
}

/// The seat's command: the bot's keys, looking at its foe's chest; a
/// bystander's nothing.
fn seat_command(bot: Option<&mut Bot>, last: &Snapshot, ids: [SimId; 2], tick_ms: f64) -> Command {
    let Some(bot) = bot else {
        return Command::default();
    };
    let (Some(me), Some(foe)) = (last.get(ids[0]), last.get(ids[1])) else {
        return Command::default();
    };
    let (Some(ps), Some(them), Some(mine), Some(theirs)) = (me.player, foe.player, me.fighter, foe.fighter) else {
        return Command::default();
    };
    let apart = sim::math::length(sim::math::sub(them.origin, ps.origin));
    let keys = bot.keys(last.time_ms, tick_ms, &mine, (ids[1], &theirs), apart);
    let (pitch, yaw) = mind::aim(mind::eye(&ps), mind::chest(&them));
    Command {
        forward: keys.forward,
        right: keys.right,
        buttons: keys.buttons,
        holds: keys.holds,
        ..mind::looking(&ps, pitch, yaw)
    }
}

/// KOTOR's reference for a fight (`reference`): each side against the
/// other with the sheets it started with, in the style it fought in, at
/// the cadence its swings came (the fight's time over its swings), with
/// Speed's extra attacks as many as ran on average, and the side KOTOR's
/// rules favour (fewer rounds to kill).
fn kotor(start: &Snapshot, last: &Snapshot, ids: [SimId; 2], tally: &Tally, ms: u64, game: Game) -> Value {
    // The sheets fought with: as the fight ended (a script may have armed a
    // creature after it was made), else as it began.
    let sheet = |id: SimId| {
        last.get(id)
            .and_then(|e| e.sheet)
            .or_else(|| start.get(id).and_then(|e| e.sheet))
    };
    let style = |id: SimId, sheet: &Sheet| {
        last.get(id)
            .and_then(|e| e.fighter)
            .and_then(|f| f.saber.style)
            .or_else(|| Kit::of(sheet).style_for(0, None))
            .unwrap_or(Style::Medium)
    };
    let (Some(a), Some(b)) = (sheet(ids[0]), sheet(ids[1])) else {
        return Value::Null;
    };
    let refs: Vec<reference::Reference> = [(ids[0], a, b), (ids[1], b, a)]
        .into_iter()
        .enumerate()
        .map(|(side, (id, me, foe))| {
            let swings = u64::from(tally.sides[side].swings.max(1));
            let cadence = u32::try_from(ms / swings).unwrap_or(u32::MAX);
            let ticks = tally.ticks.max(1) as f64;
            let speed = tally.sides[side].extra_attacks_sum as f64 / ticks;
            reference::reference(&me, style(id, &me), &foe, cadence, speed, game)
        })
        .collect();
    let (to_b, to_a) = (refs[0].kotor_rounds_to_kill, refs[1].kotor_rounds_to_kill);
    let winner = if to_b < to_a {
        "a"
    } else if to_a < to_b {
        "b"
    } else {
        "even"
    };
    json!({ "a": refs[0], "b": refs[1], "winner": winner })
}

/// How often `KJK_BALANCE_TRACE` prints seed 0's fighters when it names no period (sim ms).
const TRACE_MS: u64 = 1000;

/// A line of each side as it stands at `now` (`KJK_BALANCE_TRACE`): where,
/// its HP, its saber move and meters, its mind's task and target.
fn trace_line(rules: SaberRules, now: u64, last: &Snapshot, ids: [SimId; 2]) {
    let mut line = format!("TRACE {} {:>6} ms", rules.name(), now);
    for (side, id) in ["a", "b"].into_iter().zip(ids) {
        let Some(entity) = last.get(id) else {
            continue;
        };
        let (Some(f), Some(ps)) = (entity.fighter, entity.player) else {
            continue;
        };
        line += &format!(
            " | {side} at ({:.0},{:.0},{:.0}) hp {} {} on {} {:?} fp {} pts {} tired {} block {}{} perfect {} task {:?} target {:?} {} cmd {:#x}/{}/{} flags {:#x} stun {} pain {} wait {} steer {} queued {} moving {} acting {:?} combat {}",
            ps.origin[0],
            ps.origin[1],
            ps.origin[2],
            f.hp / 100,
            f.saber.current_move.name(),
            f.saber.on,
            f.saber.style,
            f.fp / 100,
            f.saber.duel.points,
            f.saber.duel.tired(),
            u8::from(f.saber.duel.block_held),
            u8::from(f.saber.duel.aimed),
            f.saber.duel.perfect_until.saturating_sub(last.time_ms),
            f.mind.task,
            f.target.map(|t| t.0),
            ps.torso.anim.name(),
            f.last_command.buttons,
            f.last_command.forward,
            f.last_command.holds,
            ps.flags,
            f.force.stunned_until.saturating_sub(last.time_ms),
            f.pain_until.saturating_sub(last.time_ms),
            f.weapon_wait_ms,
            f.mind.steered,
            f.mind.queued,
            f.mind.moving,
            f.acting.map(|acting| (acting.animation, acting.ms, acting.since)),
            f.combat_until.saturating_sub(last.time_ms),
        );
        if let Some(event) = f.newest().next() {
            line += &format!(
                " last {:?} {} by {:?} {:?} at tick {}",
                event.result,
                event.damage / 100,
                event.by.map(|by| by.0),
                event.weapon,
                event.tick
            );
        }
    }
    eprintln!("{line}");
}

/// One fight under `rules` from `seed`, recorded when `record`.
fn fight(ground: &Ground, rules: SaberRules, seed: u64, record: bool) -> Result<Fought, String> {
    let (mut world, start, script, ids) = start(ground, rules, seed)?;
    let rate = TickRate::default();
    let tick_ms = 1000.0 / f64::from(rate.hz());
    let mut recorder = record.then(|| {
        Recorder::new(rate.hz(), start.clone(), ground.level.id(), 100, world.stance_ids()).with_script(script.clone())
    });
    let mut bot = match ground.control {
        Control::Bot(skill) => Some(Bot::new(skill, rules, seed)),
        Control::Ai => None,
    };
    let fighter = |snapshot: &Snapshot, id: SimId| snapshot.get(id).and_then(|e| e.fighter);
    let pool = |snapshot: &Snapshot, id: SimId| {
        snapshot
            .get(id)
            .and_then(|e| e.sheet)
            .map_or(1, |sheet| sheet.pool())
    };
    let (Some(fa), Some(fb)) = (fighter(&start, ids[0]), fighter(&start, ids[1])) else {
        return Err("a side has no fighter".into());
    };
    let mut tally = Tally::new(ids, [&fa, &fb]);
    let pools = [pool(&start, ids[0]), pool(&start, ids[1])];
    // `KJK_BALANCE_TRACE=<ms>`: seed 0 printed every so often (1000 by default).
    let trace = std::env::var("KJK_BALANCE_TRACE")
        .ok()
        .filter(|_| record)
        .map(|every| every.parse::<u64>().unwrap_or(TRACE_MS).max(1));
    let mut next_trace = 0;
    tap::listen();
    let mut last = start.clone();
    let mut ended = None;
    while last.time_ms < start.time_ms + ground.cap_ms {
        let command = seat_command(bot.as_mut(), &last, ids, tick_ms);
        let mut input = TickInput::default();
        input.insert(Seat(0), command);
        let tick = last.tick + 1;
        let msec = rate.msec(tick);
        last = step(&mut world, tick, &input, msec, StepReason::Authority);
        if let Some(recorder) = recorder.as_mut() {
            recorder.record(msec, &input, &last);
        }
        let (Some(fa), Some(fb)) = (fighter(&last, ids[0]), fighter(&last, ids[1])) else {
            break;
        };
        let now = last.time_ms - start.time_ms;
        if let Some(every) = trace
            && now >= next_trace
        {
            trace_line(rules, now, &last, ids);
            next_trace = now + every;
        }
        let torso = |id: SimId| last.get(id).and_then(|e| e.player).map_or(Anim::BOTH_STAND1, |ps| ps.torso.anim);
        tally.tick(
            now,
            [(&fa, pools[0], torso(ids[0])), (&fb, pools[1], torso(ids[1]))],
            &tap::take(),
        );
        if !fa.alive() || !fb.alive() {
            ended = Some((now, fa.alive(), fb.alive()));
            break;
        }
    }
    tap::stop();
    tally.finish();
    let ms = last.time_ms - start.time_ms;
    let winner = match ended {
        Some((_, true, false)) => "a",
        Some((_, false, true)) => "b",
        Some(_) => "both",
        None => "none",
    };
    let sides: Vec<Value> = tally
        .sides
        .iter()
        .zip(&ground.sides)
        .map(|(side, fighting)| {
            let mut value = json!(side);
            value["name"] = fighting.spec.name().into();
            value
        })
        .collect();
    let kotor = kotor(&start, &last, ids, &tally, ms, ground.game);
    let record = json!({
        "rules": rules.name(),
        "seed": seed,
        "winner": winner,
        "ms": ms,
        "ticks": tally.ticks,
        "sides": sides,
        "locks": tally.locks,
        "bot": bot.map(|bot| json!(bot.tries)),
        "kotor": kotor,
    });
    Ok(Fought {
        record,
        recording: recorder.map(|recorder| recorder.finish(&last)),
    })
}

/// Every fight of `ground` under each of `rules`, seeds `0..seeds`, on
/// every core.
fn fights(ground: &Ground, rules: &[SaberRules], seeds: u64) -> Vec<(SaberRules, u64, Result<Fought, String>)> {
    let jobs: Vec<(SaberRules, u64)> = rules
        .iter()
        .flat_map(|&rules| (0..seeds).map(move |seed| (rules, seed)))
        .collect();
    let next = AtomicUsize::new(0);
    let done = Mutex::new(Vec::new());
    let workers = std::thread::available_parallelism().map_or(4, |n| n.get());
    std::thread::scope(|scope| {
        for _ in 0..workers.min(jobs.len()) {
            scope.spawn(|| {
                loop {
                    let at = next.fetch_add(1, Ordering::Relaxed);
                    let Some(&(rules, seed)) = jobs.get(at) else {
                        break;
                    };
                    let fought = fight(ground, rules, seed, seed == 0);
                    if let Ok(mut done) = done.lock() {
                        done.push((rules, seed, fought));
                    }
                }
            });
        }
    });
    let mut done = done.into_inner().unwrap_or_else(|poisoned| poisoned.into_inner());
    done.sort_by_key(|(rules, seed, _)| (*rules, *seed));
    done
}

/// A sorted list's value at fraction `p` (nearest rank).
fn quantile(sorted: &[f64], p: f64) -> f64 {
    if sorted.is_empty() {
        return f64::NAN;
    }
    let at = ((sorted.len() - 1) as f64 * p).round() as usize;
    sorted[at]
}

/// A number field of every record's side `side` (`path` into it).
fn field(records: &[&Value], side: usize, path: &[&str]) -> Vec<f64> {
    records
        .iter()
        .map(|record| {
            let mut value = &record["sides"][side];
            for key in path {
                value = &value[*key];
            }
            value.as_f64().unwrap_or(0.0)
        })
        .collect()
}

/// A side's counts averaged per fight, its blocks by kind and what it took.
const COUNTS: [&str; 14] = [
    "swings",
    "hits",
    "blows_stopped",
    "disarmed",
    "knockdowns",
    "kicks_thrown",
    "kicks_landed",
    "kicked_absorbed",
    "kicked_floored",
    "saber_breaks",
    "power_casts",
    "bolts_turned",
    "hp_start",
    "hp_end",
];
const BLOCKS: [&str; 7] = [
    "creature",
    "held",
    "aimed",
    "perfect",
    "unguarded",
    "state",
    "clash",
];
const TOOK: [&str; 9] = [
    "bounce",
    "attack_bounce",
    "heavy_stagger",
    "stagger",
    "knockaway",
    "knocked_aside",
    "parry",
    "weary_parry",
    "disarm_roll",
];

fn mean(values: &[f64]) -> f64 {
    if values.is_empty() {
        return f64::NAN;
    }
    values.iter().sum::<f64>() / values.len() as f64
}

/// The summary of one rule set's fights, and its lines of the table.
fn summary(rules: SaberRules, records: &[&Value]) -> (Value, Vec<String>) {
    let n = records.len();
    let won = |who: &str| records.iter().filter(|r| r["winner"] == who).count();
    let mut ttk: Vec<f64> = records
        .iter()
        .filter(|r| r["winner"] == "a" || r["winner"] == "b")
        .map(|r| r["ms"].as_f64().unwrap_or(0.0) / 1000.0)
        .collect();
    ttk.sort_by(f64::total_cmp);
    let seconds: Vec<f64> = records
        .iter()
        .map(|r| r["ms"].as_f64().unwrap_or(0.0) / 1000.0)
        .collect();
    let ticks: Vec<f64> = records.iter().map(|r| r["ticks"].as_f64().unwrap_or(1.0)).collect();
    let per_fight = |side: usize, path: &[&str]| mean(&field(records, side, path));
    let side_summary = |side: usize| {
        let lost = field(records, side, &["hp_lost"]);
        let hp_per_s = lost.iter().zip(&seconds).map(|(l, s)| l / 100.0 / s.max(0.001)).collect::<Vec<_>>();
        let hits = field(records, side, &["hits"]).iter().sum::<f64>();
        let damage = field(records, side, &["damage"]).iter().sum::<f64>();
        let share = |path: &[&str]| {
            let per_tick: Vec<f64> = field(records, side, path).iter().zip(&ticks).map(|(v, t)| v / t.max(1.0)).collect();
            mean(&per_tick)
        };
        let means = |group: &str, keys: &[&str]| -> Value {
            keys.iter()
                .map(|key| (key.to_string(), json!(per_fight(side, &[group, key]))))
                .collect::<serde_json::Map<_, _>>()
                .into()
        };
        let mut value = json!({});
        for key in COUNTS {
            value[key] = json!(per_fight(side, &[key]));
        }
        value["hp_lost_per_s"] = json!(mean(&hp_per_s));
        value["damage_per_hit"] = json!(if hits > 0.0 { damage / hits / 100.0 } else { 0.0 });
        value["blocks"] = means("blocks", &BLOCKS);
        value["took"] = means("took", &TOOK);
        value["tired_share"] = json!(share(&["tired_ticks"]));
        value["force_pct"] = json!(share(&["force_pct_sum"]));
        value["points"] = json!(share(&["points_sum"]));
        value["power_fp"] = json!(per_fight(side, &["power_fp"]) / 100.0);
        value["other_hp"] = json!(per_fight(side, &["other_damage"]) / 100.0);
        value
    };
    let locks: Vec<&Value> = records
        .iter()
        .flat_map(|r| r["locks"].as_array().into_iter().flatten())
        .collect();
    let lock_count = |key: &str, value: &str| locks.iter().filter(|l| l[key] == value).count();
    let summary = json!({
        "rules": rules.name(),
        "fights": n,
        "wins": { "a": won("a"), "b": won("b"), "both": won("both"), "none": won("none") },
        "ttk_s": {
            "mean": mean(&ttk), "median": quantile(&ttk, 0.5),
            "p10": quantile(&ttk, 0.1), "p90": quantile(&ttk, 0.9), "decided": ttk.len(),
        },
        "locks": {
            "per_fight": locks.len() as f64 / n.max(1) as f64,
            "won_a": lock_count("winner", "a"), "won_b": lock_count("winner", "b"),
            "even": lock_count("winner", "even"),
            "kills": lock_count("result", "kill"), "downs": lock_count("result", "down"),
            "disarms": lock_count("result", "disarm"),
        },
        "sides": [side_summary(0), side_summary(1)],
        "kotor": kotor_summary(records),
    });
    let lines = table(&summary);
    (summary, lines)
}

/// The fights' KOTOR references averaged by side, and the side KOTOR's
/// rules favour by them (fewer rounds to kill).
fn kotor_summary(records: &[&Value]) -> Value {
    let side = |name: &str| -> Value {
        let mut keys: serde_json::Map<_, _> = [
            "p",
            "hit",
            "round",
            "per_blow",
            "landed",
            "cadence_ms",
            "foe_hp",
            "kotor_rounds_to_kill",
            "kjk_landed_to_kill",
        ]
        .iter()
        .map(|key| {
            let values: Vec<f64> = records
                .iter()
                .filter_map(|r| r["kotor"][name][key].as_f64())
                .collect();
            (key.to_string(), json!(mean(&values)))
        })
        .collect();
        // The round's attacks over every fight (the style fought in and
        // Speed's part differ): the same attack's counts added up, over the
        // fights.
        let mut attacks: Vec<Value> = Vec::new();
        for attack in records
            .iter()
            .filter_map(|r| r["kotor"][name]["attacks"].as_array())
            .flatten()
        {
            let same = |known: &&mut Value| {
                ["hand", "p", "hit"]
                    .iter()
                    .all(|key| known[key] == attack[key])
            };
            let count = attack["count"].as_f64().unwrap_or(0.0) / records.len().max(1) as f64;
            match attacks.iter_mut().find(same) {
                Some(known) => {
                    known["count"] = json!(known["count"].as_f64().unwrap_or(0.0) + count);
                }
                None => {
                    let mut new = attack.clone();
                    new["count"] = json!(count);
                    attacks.push(new);
                }
            }
        }
        keys.insert("attacks".into(), attacks.into());
        keys.into()
    };
    let (a, b) = (side("a"), side("b"));
    let rolls = |side: &Value| side["kotor_rounds_to_kill"].as_f64().unwrap_or(f64::NAN);
    let winner = match (rolls(&a), rolls(&b)) {
        (to_b, to_a) if to_b < to_a => "a",
        (to_b, to_a) if to_a < to_b => "b",
        _ => "even",
    };
    json!({ "a": a, "b": b, "winner": winner })
}

/// A rule set's lines of the table.
fn table(s: &Value) -> Vec<String> {
    let f = |v: &Value| v.as_f64().unwrap_or(f64::NAN);
    let k = &s["kotor"];
    let kotor = format!(
        "  kotor p a {:.2} b {:.2} | hit a {:.1} b {:.1} | round a {:.1} b {:.1} | per blow a {:.1} b {:.1} | landed (p×hit×pace) a {:.1} b {:.1} | rounds to kill a→b {:.1} b→a {:.1} | landed to kill a→b {:.1} b→a {:.1} | favours {}",
        f(&k["a"]["p"]),
        f(&k["b"]["p"]),
        f(&k["a"]["hit"]),
        f(&k["b"]["hit"]),
        f(&k["a"]["round"]),
        f(&k["b"]["round"]),
        f(&k["a"]["per_blow"]),
        f(&k["b"]["per_blow"]),
        f(&k["a"]["landed"]),
        f(&k["b"]["landed"]),
        f(&k["a"]["kotor_rounds_to_kill"]),
        f(&k["b"]["kotor_rounds_to_kill"]),
        f(&k["a"]["kjk_landed_to_kill"]),
        f(&k["b"]["kjk_landed_to_kill"]),
        k["winner"].as_str().unwrap_or("?"),
    );
    let mut lines = vec![format!(
        "{:<3} n {:>3} | wins a {:>3} b {:>3} cap {:>3} | ttk s mean {:>5.1} med {:>5.1} p10 {:>5.1} p90 {:>5.1} | locks/fight {:.2} (a {} b {} even {}; kill {} down {} disarm {})",
        s["rules"].as_str().unwrap_or("?"),
        s["fights"],
        s["wins"]["a"],
        s["wins"]["b"],
        s["wins"]["none"],
        f(&s["ttk_s"]["mean"]),
        f(&s["ttk_s"]["median"]),
        f(&s["ttk_s"]["p10"]),
        f(&s["ttk_s"]["p90"]),
        f(&s["locks"]["per_fight"]),
        s["locks"]["won_a"],
        s["locks"]["won_b"],
        s["locks"]["even"],
        s["locks"]["kills"],
        s["locks"]["downs"],
        s["locks"]["disarms"],
    )];
    lines.push(
        "      hp/s  swing   hit dmg/hit stopd | blocks cre held  aim perf ungd  st clash | took bnc abnc heavy stag kaway aside disarm | dsrm  kd kick land abs flr brk | fatig force  pts cast fp/pw"
            .into(),
    );
    for (side, name) in ["a", "b"].into_iter().enumerate() {
        let v = &s["sides"][side];
        let b = &v["blocks"];
        let t = &v["took"];
        lines.push(format!(
            "  {name}  {:>5.2} {:>6.1} {:>5.1} {:>7.1} {:>5.1} |      {:>4.1} {:>4.1} {:>4.1} {:>4.1} {:>4.1} {:>3.1} {:>5.1} |      {:>3.1} {:>4.1} {:>5.1} {:>4.1} {:>5.1} {:>5.1} {:>6.2} | {:>4.2} {:>3.1} {:>4.1} {:>4.1} {:>3.1} {:>3.1} {:>3.1} | {:>5.2} {:>5.1} {:>4.0} {:>4.1} {:>5.1}",
            f(&v["hp_lost_per_s"]),
            f(&v["swings"]),
            f(&v["hits"]),
            f(&v["damage_per_hit"]),
            f(&v["blows_stopped"]),
            f(&b["creature"]),
            f(&b["held"]),
            f(&b["aimed"]),
            f(&b["perfect"]),
            f(&b["unguarded"]),
            f(&b["state"]),
            f(&b["clash"]),
            f(&t["bounce"]),
            f(&t["attack_bounce"]),
            f(&t["heavy_stagger"]),
            f(&t["stagger"]),
            f(&t["knockaway"]),
            f(&t["knocked_aside"]),
            f(&t["disarm_roll"]),
            f(&v["disarmed"]),
            f(&v["knockdowns"]),
            f(&v["kicks_thrown"]),
            f(&v["kicks_landed"]),
            f(&v["kicked_absorbed"]),
            f(&v["kicked_floored"]),
            f(&v["saber_breaks"]),
            f(&v["tired_share"]),
            f(&v["force_pct"]),
            f(&v["points"]),
            f(&v["power_casts"]),
            f(&v["power_fp"]),
        ));
    }
    lines.push(kotor);
    lines
}

/// `kjk combat balance`.
pub fn balance(discovery: &Discovery, options: &Options) -> ExitCode {
    match run(discovery, options) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("combat balance: {error}");
            ExitCode::FAILURE
        }
    }
}

fn run(discovery: &Discovery, options: &Options) -> Result<(), String> {
    let control = Control::parse(&options.control)?;
    let rules = options
        .rules
        .iter()
        .map(|name| SaberRules::named(name).ok_or(format!("--rules {name}: ja or advanced")))
        .collect::<Result<Vec<_>, _>>()?;
    let specs = [Spec::parse(&options.a)?, Spec::parse(&options.b)?];
    let game = options.game.as_str();
    let module = options
        .module
        .clone()
        .unwrap_or(if game == "tsl" { "001ebo" } else { kjk_game::arena::BASE_MODULE }.to_string());
    let (level, classes) = arena_level(discovery, game, &module)?;
    let sides = sides(&level, classes.as_ref(), game, specs, control)?;
    let ground = Ground {
        level,
        sides,
        control,
        no_powers: options.no_powers,
        cap_ms: options.seconds * 1000,
        game: if game == "tsl" { Game::Tsl } else { Game::K1 },
    };
    let matchup = format!(
        "{} ({}) vs {}{}",
        ground.sides[0].spec.name(),
        control.name(),
        ground.sides[1].spec.name(),
        if options.no_powers { ", no powers" } else { "" }
    );
    let mut lines = vec![format!(
        "combat balance: {game} ({module}), {matchup}, {} seeds, cap {} s",
        options.seeds, options.seconds
    )];
    for (side, fighting) in ground.sides.iter().enumerate() {
        if let Some(sheet) = fighting.sheet {
            lines.push(format!(
                "  {}: level {}, HP {}, FP {}, BAB {}, defence {}, Jedi Defense {}, feats {:?}, powers {:?}",
                ["a", "b"][side],
                sheet.level,
                sheet.max_hp,
                sheet.max_fp,
                sheet.bab,
                sheet.defence(),
                sheet.feats.count(&sim::combat::sheet::feats::JEDI_DEFENSE),
                sheet.feats.rows().collect::<Vec<_>>(),
                sheet.powers.rows().collect::<Vec<_>>(),
            ));
        }
    }
    let started = std::time::Instant::now();
    let fought = fights(&ground, &rules, options.seeds);
    let mut records = Vec::new();
    let mut summaries = Vec::new();
    let mut recordings = Vec::new();
    let mut errors = Vec::new();
    for (rules, seed, result) in fought {
        match result {
            Ok(fought) => {
                if let Some(recording) = fought.recording {
                    recordings.push((rules.name().to_string(), recording));
                }
                records.push(fought.record);
            }
            Err(error) => errors.push(format!("{} seed {seed}: {error}", rules.name())),
        }
    }
    for &rules in &rules {
        let of: Vec<&Value> = records.iter().filter(|r| r["rules"] == rules.name()).collect();
        if of.is_empty() {
            continue;
        }
        let (summary, table) = summary(rules, &of);
        lines.extend(table);
        summaries.push(summary);
    }
    lines.push(format!(
        "{} fights in {:.1} s{}",
        records.len(),
        started.elapsed().as_secs_f64(),
        if errors.is_empty() {
            String::new()
        } else {
            format!("; {} failed: {}", errors.len(), errors.join("; "))
        }
    ));
    for line in &lines {
        outln!("{line}");
    }
    let facts = json!({
        "verb": "combat balance",
        "game": game,
        "module": module,
        "matchup": matchup,
        "a": ground.sides[0].spec.name(),
        "b": ground.sides[1].spec.name(),
        "control": control.name(),
        "no_powers": options.no_powers,
        "seeds": options.seeds,
        "cap_s": options.seconds,
        "summaries": summaries,
        "fights": records,
        "errors": errors,
        "table": lines,
    });
    let mut folders: Vec<PathBuf> = crate::runtime::write_facts(facts.clone()).into_iter().collect();
    if let Some(out) = &options.out {
        std::fs::create_dir_all(out).map_err(|e| format!("{}: {e}", out.display()))?;
        crate::runtime::write_facts_in(out, facts);
        folders.push(out.clone());
    }
    for folder in &folders {
        let _ = std::fs::write(folder.join("table.txt"), lines.join("\n") + "\n");
        crate::duel_verbs::write_bouts(folder, &recordings);
    }
    if records.is_empty() {
        return Err("no fight ran".into());
    }
    Ok(())
}
