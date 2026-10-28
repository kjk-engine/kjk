//! The probes of KOTOR's UTE encounters (`kjk sim probes "ute encounters"`,
//! `crate::spawner`, `world::spawners`), with the executables' numbers and
//! the games' own encounters' data, on the proving ground, no install:
//! - the spawn lists: TSL's count, singles, guaranteed copies and draws
//!   (204tel's mercenaries, 106per's droid ambush, 506ond's palace guards);
//!   K1's points, party challenge and budget (tar_m03ab's Vulkar patrol,
//!   kas_m25aa's kinrath), its split and top-up; the same seed, the same
//!   lists;
//! - placement: a safe spot at the point, else the first of the rings round
//!   it; the rings round the encounter's own place;
//! - the step: who comes in starts it (hostile, alive, a player where
//!   `PlayerOnly` asks), one creature a step at TSL's dealt points (all on
//!   the second when the points outnumber the list) or K1's farthest,
//!   facing the point's way; `OnEntered` running while inactive and
//!   activating it; the spawn script seeing `GetIsEncounterCreature`;
//! - exhaustion (`OnExhausted`, inactive) and the 30 s reset with
//!   `Respawns` -1, none with `SpawnOption` 0; heartbeats while active;
//! - the routines (difficulty, resets, active), `GetEnteringObject` and
//!   `GetFirstInPersistentObject` on an encounter;
//! - the save: the list kept, the creatures' links not (QUIRKS.md Q-0261).

use super::{Fighting, Id, SimWorld};
use crate::combat::Sheet;
use crate::combat::fighter::Life;
use crate::combat::sheet::factions::{FRIENDLY_1, HOSTILE_1, NEUTRAL, PLAYER};
use crate::input::{Command, Seat, TickInput};
use crate::module::{ObjectData, ObjectKind, Placement, Repute};
use crate::probe::{Checks, check};
use crate::rng::SimRng;
use crate::snapshot::{Kind, Snapshot};
use crate::spawner::{self, Body, Creature, Encounter, Ground, Point};
use crate::{Level, StepReason, WorldConfig};
use fmt_odyssey::ncs::{Const, Ncs, Op};
use script_ncs::probes::Asm;
use std::collections::BTreeMap;
use std::sync::Arc;

const MSEC: u32 = 8;
const ENC: &str = "probe_enc";
const MERC: &str = "probe_merc";
const MERC_B: &str = "probe_merc_b";
/// The scripts: a creature's spawn script keeping `GetIsEncounterCreature`,
/// an encounter's `OnEntered` activating it, its `OnExhausted`,
/// `OnHeartbeat`, and the module's runs of the routines.
const SPAWNED: &str = "probe_enc_spawned";
const WAKE: &str = "probe_enc_wake";
const EXHAUSTED: &str = "probe_enc_exhausted";
const BEAT: &str = "probe_enc_beat";
const ROUTINES: &str = "probe_enc_routines";
const ENTERING: &str = "probe_enc_entering";

const CATALOG: &str = r#"
int OBJECT_TYPE_CREATURE = 1;
int PERSISTENT_ZONE_ACTIVE = 0;
// 0
void SetGlobalNumber(string sIdentifier, int nValue);
// 1
void SetEncounterActive(int nNewValue, object oEncounter=OBJECT_SELF);
// 2
int GetIsEncounterCreature(object oCreature=OBJECT_SELF);
// 3
void IncrementGlobalNumber(string sIdentifier, int nAmount);
// 4
object GetObjectByTag(string sTag, int nNth=0);
// 5
void SetEncounterDifficulty(int nEncounterDifficulty, object oEncounter=OBJECT_SELF);
// 6
int GetEncounterDifficulty(object oEncounter=OBJECT_SELF);
// 7
void SetEncounterSpawnsMax(int nNewValue, object oEncounter=OBJECT_SELF);
// 8
int GetEncounterSpawnsMax(object oEncounter=OBJECT_SELF);
// 9
void SetEncounterSpawnsCurrent(int nNewValue, object oEncounter=OBJECT_SELF);
// 10
int GetEncounterSpawnsCurrent(object oEncounter=OBJECT_SELF);
// 11
int GetEncounterActive(object oEncounter=OBJECT_SELF);
// 12
int GetIsPC(object oCreature);
// 13
object GetFirstInPersistentObject(object oPersistentObject=OBJECT_SELF, int nResidentObjectType=OBJECT_TYPE_CREATURE, int nPersistentZone=PERSISTENT_ZONE_ACTIVE);
// 14
object GetEnteringObject();
"#;

fn call(routine: u16, args: u8) -> Op {
    Op::Action { routine, args }
}

fn int(value: i32) -> Op {
    Op::Const(Const::Int(value))
}

fn text(value: &str) -> Op {
    Op::Const(Const::String(value.as_bytes().to_vec()))
}

const SELF: Op = Op::Const(Const::Object(0));

/// Global `name` set to what `value` leaves on the stack.
fn keep(asm: &mut Asm, name: &str, value: &[Op]) {
    for op in value {
        asm.op(op.clone());
    }
    asm.op(text(name)).op(call(0, 2));
}

/// The encounter, by tag (`GetObjectByTag(ENC, 0)`).
fn encounter_object() -> [Op; 3] {
    [int(0), text(ENC), call(4, 2)]
}

fn scripts() -> Vec<(&'static str, Ncs)> {
    let build = |fill: &dyn Fn(&mut Asm)| {
        let mut asm = Asm::default();
        fill(&mut asm);
        asm.op(Op::Retn);
        asm.build()
    };
    let enc = encounter_object();
    let with = |value: &[Op]| [enc.as_slice(), value].concat();
    vec![
        (
            SPAWNED,
            build(&|asm| keep(asm, "spawned_encounter", &[SELF, call(2, 1)])),
        ),
        (
            WAKE,
            build(&|asm| {
                asm.op(SELF).op(int(1)).op(call(1, 2));
            }),
        ),
        (EXHAUSTED, build(&|asm| keep(asm, "exhausted", &[int(1)]))),
        (
            BEAT,
            build(&|asm| {
                asm.op(int(1)).op(text("beats")).op(call(3, 2));
            }),
        ),
        (
            ROUTINES,
            build(&|asm| {
                // Difficulty row 3, then a row past the table; resets 7 and 2.
                for (set, get, value, name) in [
                    (5, 6, 3, "difficulty"),
                    (5, 6, 9, "difficulty_past"),
                    (7, 8, 7, "spawns_max"),
                    (9, 10, 2, "spawns_current"),
                ] {
                    for op in with(&[]) {
                        asm.op(op);
                    }
                    asm.op(int(value)).op(call(set, 2));
                    keep(asm, name, &with(&[call(get, 1)]));
                }
                keep(asm, "active", &with(&[call(11, 1)]));
                let first = [int(0), int(1)]
                    .into_iter()
                    .chain(enc.clone())
                    .chain([call(13, 3), call(12, 1)])
                    .collect::<Vec<_>>();
                keep(asm, "pc_inside", &first);
            }),
        ),
        (
            ENTERING,
            build(&|asm| {
                asm.op(call(14, 0)).op(call(12, 1));
                asm.op(text("entering_pc")).op(call(3, 2));
            }),
        ),
    ]
}

/// A creature template the encounters make: neutral to everyone (it
/// neither starts an encounter nor fights the player), keeping
/// `GetIsEncounterCreature` as it spawns.
fn template(name: &str) -> Arc<ObjectData> {
    let sheet = Sheet {
        faction: NEUTRAL,
        ..Sheet::commoner()
    };
    Arc::new(ObjectData {
        scripts: vec![("ScriptSpawn".into(), SPAWNED.into())],
        ..ObjectData::creature(name, name, [0.0; 3], sheet)
    })
}

/// A place on the proving ground's floor (its own frame, metres).
fn at(x: f32, y: f32) -> [f32; 3] {
    crate::determinism::ground_point(x, y, 0.0)
}

/// The proving ground's floor as a walkmesh.
fn floor() -> collision::nav::NavMesh {
    let corners = [[-7.5, -5.5], [7.5, -5.5], [7.5, 5.5], [-7.5, 5.5]].map(|[x, y]| at(x, y));
    collision::nav::NavMesh::build(
        &[
            [corners[0], corners[1], corners[2]],
            [corners[0], corners[2], corners[3]],
        ],
        crate::walk::CLEARANCE * collision::UNIT,
    )
}

fn creature(template: &str, cr: f32, single: bool, guaranteed: i32) -> Creature {
    Creature {
        template: template.into(),
        cr,
        single,
        guaranteed,
    }
}

/// An encounter of `creatures` (sorted as loaded), `rec..=max` of them,
/// at `points`; active, no reset, `PlayerOnly`.
fn ute(creatures: Vec<Creature>, (rec, max): (i32, i32), points: &[[f32; 3]]) -> Encounter {
    Encounter {
        active: true,
        reset: false,
        reset_time_s: 60,
        respawns: 0,
        spawn_option: 1,
        max_creatures: max,
        rec_creatures: rec,
        player_only: true,
        difficulty: 0,
        creatures: Encounter::sorted(creatures),
        points: points
            .iter()
            .enumerate()
            .map(|(n, &at)| Point {
                at,
                orientation: 0.25 * n as f32,
            })
            .collect(),
    }
}

/// 204tel's `merc_entry` (`mercenariesentry.ute`): six of four CR 1
/// mercenaries, the leader single.
fn mercenaries() -> Encounter {
    ute(
        vec![
            creature("203_merc01", 1.0, false, 0),
            creature("203_merc004", 1.0, true, 0),
            creature("203_merc002", 1.0, false, 0),
            creature("203_merc003", 1.0, false, 0),
        ],
        (6, 6),
        &[],
    )
}

/// The spawn points the world probes use, in the west half.
fn points() -> [[f32; 3]; 3] {
    [at(-6.0, -2.0), at(-2.0, 3.0), at(-1.5, -2.5)]
}

/// The ground set up for an encounter: the module's `game` rules, the
/// encounter `facts` with `faction` and `hooks`, its outline a square round
/// the player's arrival unless `away` (then round the far corner), and the
/// characters `extra` placed (tag, faction, place, dead).
struct Setup {
    game: &'static str,
    facts: Encounter,
    faction: u8,
    hooks: Vec<(String, String)>,
    away: bool,
    extra: Vec<(&'static str, u8, [f32; 3], bool)>,
}

impl Setup {
    fn new(game: &'static str, facts: Encounter) -> Setup {
        Setup {
            game,
            facts,
            faction: HOSTILE_1,
            hooks: Vec::new(),
            away: false,
            extra: Vec::new(),
        }
    }

    fn hook(mut self, label: &str, script: &str) -> Setup {
        self.hooks.push((label.into(), script.into()));
        self
    }
}

struct Run {
    world: SimWorld,
    encounter: u32,
}

fn run(setup: Setup) -> Result<Run, String> {
    let ground = crate::determinism::proving_ground();
    let mut data = (**ground.module().ok_or("the ground has no module")?).clone();
    data.game = setup.game.into();
    data.repute = Repute::at_war([HOSTILE_1, PLAYER]);
    data.encounter_tables = spawner::Tables {
        difficulties: vec![0, 1, 2, 5, 9],
        fractional: vec![0.40, 0.30, 0.20, 0.15, -99999.0],
    };
    let (cx, cy) = if setup.away { (5.0, 4.0) } else { (-4.0, 0.0) };
    let position = at(cx, cy);
    let outline = [[-2.0, -2.5], [2.0, -2.5], [2.0, 2.5], [-2.0, 2.5]]
        .map(|[x, y]| {
            let p = at(cx + x, cy + y);
            [p[0] - position[0], p[1] - position[1], 0.0]
        })
        .to_vec();
    data.placements.push(Placement::Own(ObjectData {
        geometry: outline,
        faction: setup.faction,
        scripts: setup.hooks,
        encounter: Some(Box::new(setup.facts)),
        ..ObjectData::bare(ObjectKind::Encounter, ENC, position)
    }));
    for &(tag, faction, place, _) in &setup.extra {
        let sheet = Sheet {
            faction,
            ..Sheet::commoner()
        };
        data.placements
            .push(Placement::Own(ObjectData::creature(tag, tag, place, sheet)));
    }
    let creatures: BTreeMap<String, Arc<ObjectData>> = [MERC, MERC_B]
        .iter()
        .map(|name| (name.to_string(), template(name)))
        .collect();
    let library = crate::probe::library(CATALOG, scripts()).with_creatures(creatures);
    let level: Level = ground
        .with_module(data)
        .with_scripts(library)
        .with_nav(floor());
    let mut world =
        SimWorld::new(WorldConfig::one_seat(), level).map_err(|error| error.to_string())?;
    let start = world.snapshot();
    let objects = crate::Instances::in_snapshot(world.level(), &start);
    let tagged = |tag: &str| {
        start
            .entities
            .iter()
            .find(|e| objects.of_entity(e).is_some_and(|data| data.tag == tag))
            .map(|e| e.id.0)
    };
    let encounter = tagged(ENC).ok_or("no encounter")?;
    let dead: Vec<u32> = setup
        .extra
        .iter()
        .filter(|(.., dead)| *dead)
        .filter_map(|(tag, ..)| tagged(tag))
        .collect();
    let mut run = Run { world, encounter };
    run.kill(&dead);
    Ok(run)
}

impl Run {
    fn step(&mut self, msec: u32) -> Snapshot {
        let mut input = TickInput::default();
        input.insert(Seat(0), Command::default());
        let tick = self.world.tick() + 1;
        crate::step(&mut self.world, tick, &input, msec, StepReason::Authority)
    }

    fn steps(&mut self, n: usize, msec: u32) -> Snapshot {
        let mut last = self.world.snapshot();
        for _ in 0..n {
            last = self.step(msec);
        }
        last
    }

    /// The creatures encounters made standing now, in the order they were
    /// made: id, template, place (metres) and facing (degrees).
    fn made(&self, snapshot: &Snapshot) -> Vec<(u32, String, [f32; 3], f32)> {
        let objects = crate::Instances::in_snapshot(self.world.level(), snapshot);
        snapshot
            .entities
            .iter()
            .filter(|e| matches!(e.kind, Kind::Object(ObjectKind::Creature, _)))
            .filter_map(|e| {
                let data = objects.of_entity(e)?;
                [MERC, MERC_B].contains(&data.template.as_str()).then(|| {
                    let yaw = e.player.map_or(0.0, |player| player.view[1]);
                    (e.id.0, data.template.clone(), e.position, yaw)
                })
            })
            .collect()
    }

    /// `made` after `n` steps of `msec`.
    fn made_after(&mut self, n: usize, msec: u32) -> Vec<(u32, String, [f32; 3], f32)> {
        let snapshot = self.steps(n, msec);
        self.made(&snapshot)
    }

    fn kill(&mut self, who: &[u32]) {
        let mut fighters = self.world.world.query::<(&Id, &mut Fighting)>();
        for (id, mut fighting) in fighters.iter_mut(&mut self.world.world) {
            if who.contains(&id.0.0) {
                fighting.0.hp = 0;
                fighting.0.life = Life::Dead;
            }
        }
    }

    fn spawn(&self) -> Option<&spawner::Spawn> {
        self.world.script_state().encounters.get(&self.encounter)
    }

    fn global(&self, name: &str) -> i32 {
        self.world
            .script_state()
            .numbers
            .get(name)
            .copied()
            .unwrap_or(0)
    }
}

/// Whether `a` and `b` are the same place seen from above (to 5 cm).
fn same_spot(a: [f32; 3], b: [f32; 3]) -> bool {
    (a[0] - b[0]).abs() < 0.05 && (a[1] - b[1]).abs() < 0.05
}

/// A check run on its own.
type Case = fn() -> Result<String, String>;

pub fn probes() -> Checks {
    let cases: [(&'static str, Case); 20] = [
        ("tsl_mercenaries", tsl_mercenaries),
        ("tsl_droid_ambush", tsl_droid_ambush),
        ("tsl_palace_guards", tsl_palace_guards),
        ("k1_points", k1_points),
        ("k1_vulkar_budget", k1_vulkar_budget),
        ("k1_kinrath_count", k1_kinrath_count),
        ("k1_split_and_top_up", k1_split_and_top_up),
        ("safe_place_rings", safe_place_rings),
        ("entering_spawns_one_a_step", entering_spawns),
        ("tsl_points_outnumber_the_list", cursor_quirk),
        ("k1_farthest_point", farthest_point),
        ("on_entered_while_inactive", on_entered_while_inactive),
        ("who_starts_it", who_starts_it),
        ("exhausts_and_resets", exhausts_and_resets),
        (
            "no_reset_without_spawn_option",
            no_reset_without_spawn_option,
        ),
        ("heartbeat_while_active", heartbeat_while_active),
        ("routines", routines),
        ("entering_object", entering_object),
        ("save_keeps_the_list_not_the_links", save_drops_links),
        (
            "spawn_script_sees_its_encounter",
            spawn_script_sees_its_encounter,
        ),
    ];
    cases.into_iter().map(|(name, run)| (name, run())).collect()
}

fn names(facts: &Encounter, picks: &[usize]) -> Vec<String> {
    picks
        .iter()
        .map(|&i| facts.creatures[i].template.clone())
        .collect()
}

/// 204tel's mercenaries: six, the single leader first and once, the rest
/// drawn from the three others; the same seed, the same six.
fn tsl_mercenaries() -> Result<String, String> {
    let facts = mercenaries();
    let list = |seed| {
        names(
            &facts,
            &spawner::tsl_list(&facts, &mut SimRng::new(seed, 0)),
        )
    };
    let (one, again, other) = (list(7), list(7), list(8));
    let rest_ok = one[1..]
        .iter()
        .all(|name| ["203_merc01", "203_merc002", "203_merc003"].contains(&name.as_str()));
    let leaders = one.iter().filter(|name| *name == "203_merc004").count();
    check(
        one.len() == 6 && one[0] == "203_merc004" && leaders == 1 && rest_ok && one == again,
        format!("seed 7 {one:?} (again {again:?}), seed 8 {other:?}"),
    )
}

/// 106per's droid ambush: one mine droid and two spyders (their
/// `GuaranteedCount`s fill the three).
fn tsl_droid_ambush() -> Result<String, String> {
    let facts = ute(
        vec![
            creature("g_droid_mine_024", 2.0, false, 1),
            creature("c_drdspyder001", 2.0, false, 2),
        ],
        (3, 3),
        &[],
    );
    let list = names(&facts, &spawner::tsl_list(&facts, &mut SimRng::new(1, 0)));
    check(
        list == ["g_droid_mine_024", "c_drdspyder001", "c_drdspyder001"],
        format!("{list:?}"),
    )
}

/// 506ond's `ls_vaklu_enc`: by challenge the grenadier (single), the
/// gunner and the speaker (one each guaranteed), the droid (single): the
/// two singles first, then one of each guaranteed: no draw left.
fn tsl_palace_guards() -> Result<String, String> {
    let facts = ute(
        vec![
            creature("mil_droid_mk1", 6.0, true, 1),
            creature("va_sold_gren", 7.0, true, 1),
            creature("va_sold_gun", 7.0, false, 1),
            creature("va_speaker_002", 7.0, false, 1),
        ],
        (4, 4),
        &[],
    );
    let list = names(&facts, &spawner::tsl_list(&facts, &mut SimRng::new(3, 0)));
    check(
        list == [
            "va_sold_gren",
            "mil_droid_mk1",
            "va_sold_gun",
            "va_speaker_002",
        ],
        format!("{list:?}"),
    )
}

/// K1's points (`fractionalcr.2da`'s rows under 1, 1.5 to the whole part
/// to 50, beyond by whole steps) and the party challenge.
fn k1_points() -> Result<String, String> {
    let fractional = [0.40, 0.30, 0.20, 0.15, -99999.0];
    let points = [
        (0.5, 1.0),
        (0.35, 2.0 / 3.0),
        (0.25, 4.0 / 9.0),
        (0.16, 8.0 / 27.0),
        (0.1, 16.0 / 81.0),
        (0.0, 1.0),
        (2.7, 2.25),
        (3.0, 3.375),
        (-1.0, 1.0 / 1.5),
    ];
    let got_points: Vec<f32> = points
        .iter()
        .map(|(cr, _)| spawner::points(*cr, &fractional))
        .collect();
    let points_ok = points
        .iter()
        .zip(&got_points)
        .all(|((_, want), got)| (want - got).abs() < 1e-6);
    // One level 4 party member; one at level 1; three at 8; three at 12;
    // a quarter of the points at 1 (0) and at 1/2 (−1.2: −1).
    let party = [
        (5.0625, 1),
        (1.5, -1),
        (76.890625, 7),
        (389.23828, 11),
        (4.0, 0),
        (2.0, -1),
    ];
    let got_party: Vec<i32> = party.iter().map(|(p, _)| spawner::party_cr(*p)).collect();
    let party_ok = party
        .iter()
        .zip(&got_party)
        .all(|((_, want), got)| want == got);
    check(
        points_ok && party_ok,
        format!("points {got_points:?}, party challenges {got_party:?}"),
    )
}

/// tar_m03ab's Vulkar patrol (`ptar_vulkaraptptl`: two to three, the CR 3
/// leader single, difficulty row 3: 5). A level 4 party member alone: area
/// points 5.0625, party challenge 1, pool points(5) = 7.59375: the leader
/// free, then three... two drawn from the CR 2 pair until the room is out.
/// At level 1: pool points(3) = 3.375: the leader and one.
fn k1_vulkar_budget() -> Result<String, String> {
    let fractional = [0.40, 0.30, 0.20, 0.15, -99999.0];
    let facts = Encounter {
        difficulty: 5,
        reset: true,
        reset_time_s: 30,
        respawns: -1,
        ..ute(
            vec![
                creature("tar03_blackv040", 2.0, false, 0),
                creature("tar03_blackv041", 2.0, false, 0),
                creature("tar03_blackv042", 3.0, true, 0),
            ],
            (2, 3),
            &[],
        )
    };
    let pool = |level: f32| {
        let near = spawner::points(level, &fractional);
        spawner::points(
            (spawner::party_cr(near) + facts.difficulty - 1) as f32,
            &fractional,
        )
    };
    let pick = |level| {
        names(
            &facts,
            &spawner::k1_list(&facts, &fractional, pool(level), 0, &mut SimRng::new(5, 0)),
        )
    };
    let (four, one) = (pick(4.0), pick(1.0));
    let pair = |list: &[String]| list[1..].iter().all(|name| name != "tar03_blackv042");
    check(
        (pool(4.0) - 7.59375).abs() < 1e-5
            && (pool(1.0) - 3.375).abs() < 1e-5
            && four.len() == 3
            && one.len() == 2
            && four[0] == "tar03_blackv042"
            && one[0] == "tar03_blackv042"
            && pair(&four)
            && pair(&one),
        format!(
            "level 4 (pool {}): {four:?}; level 1 (pool {}): {one:?}",
            pool(4.0),
            pool(1.0)
        ),
    )
}

/// kas_m25aa's kinrath (three to six of one CR 10, difficulty 2): three
/// level 8 party members (pool points(8) = 25.6: none fits, the top-up
/// makes three), three at 12 (pool points(12) = 129.7: two, split refused
/// at the cheapest, topped up to three), three at 20 (six, the most); the
/// same seed, the same lists.
fn k1_kinrath_count() -> Result<String, String> {
    let fractional = [0.40, 0.30, 0.20, 0.15, -99999.0];
    let facts = Encounter {
        difficulty: 2,
        player_only: false,
        ..ute(
            vec![creature("kas25_kinrath02", 10.0, false, 0)],
            (3, 6),
            &[],
        )
    };
    let count = |level: f32, seed| {
        let near = 3.0 * spawner::points(level, &fractional);
        let pool = spawner::points(
            (spawner::party_cr(near) + facts.difficulty - 1) as f32,
            &fractional,
        );
        spawner::k1_list(&facts, &fractional, pool, 0, &mut SimRng::new(seed, 0)).len()
    };
    let counts = [count(8.0, 1), count(12.0, 1), count(20.0, 1)];
    let again = [count(8.0, 1), count(12.0, 1), count(20.0, 1)];
    check(
        counts == [3, 3, 6] && counts == again,
        format!("levels 8, 12, 20: {counts:?} (again {again:?})"),
    )
}

/// A CR 4 creature's points (5.0625) for pool, four wanted: the greedy pick
/// takes it, the split trades it for three CR 1s (1.5 each), the top-up
/// adds the fourth.
fn k1_split_and_top_up() -> Result<String, String> {
    let facts = ute(
        vec![
            creature("big", 4.0, false, 0),
            creature("small", 1.0, false, 0),
        ],
        (4, 6),
        &[],
    );
    let list = names(
        &facts,
        &spawner::k1_list(&facts, &[], 5.0625, 0, &mut SimRng::new(2, 0)),
    );
    check(list == ["small"; 4], format!("{list:?}"))
}

/// The safe place for a body at a spawn point: the point itself while it
/// is clear; with a body on it the first spot of the first ring (a metre
/// back on both axes); round the encounter's place the same.
fn safe_place_rings() -> Result<String, String> {
    let nav = floor();
    let point = at(-3.0, 1.0);
    let sees = |_: [f32; 3], _: [f32; 3]| true;
    let clear = Ground {
        nav: Some(&nav),
        bodies: Vec::new(),
        sees: &sees,
    };
    let taken = Ground {
        bodies: vec![Body {
            soles: point,
            radius: 0.4,
        }],
        ..clear
    };
    let radius = 0.4;
    let free = clear.safe_location(point, radius);
    let ringed = taken.safe_location(point, radius);
    let origin = taken.out_of_sight(point, radius);
    let first_ring = [point[0] - 1.0, point[1] - 1.0, 0.0];
    let near =
        |found: Option<[f32; 3]>, want: [f32; 3]| found.is_some_and(|got| same_spot(got, want));
    check(
        near(free, point) && near(ringed, first_ring) && near(origin, first_ring),
        format!("clear {free:?}, taken {ringed:?}, round the origin {origin:?} (point {point:?})"),
    )
}

/// TSL: the player arriving in it starts it; one creature a step, each at
/// its own point (three points, three creatures), facing the point's way
/// (`90° − orientation`), counted alive.
fn entering_spawns() -> Result<String, String> {
    let facts = ute(
        vec![
            creature(MERC, 2.0, false, 1),
            creature(MERC_B, 2.0, false, 2),
        ],
        (3, 3),
        &points(),
    );
    let wanted: Vec<f32> = facts.points.iter().map(Point::yaw).collect();
    let mut run = run(Setup::new("tsl", facts))?;
    let mut counts = Vec::new();
    let mut last = run.world.snapshot();
    for _ in 0..5 {
        last = run.step(MSEC);
        counts.push(run.made(&last).len());
    }
    let made = run.made(&last);
    let placed = made
        .iter()
        .zip(points())
        .all(|((_, _, at, _), point)| same_spot(*at, point));
    let faced = made
        .iter()
        .zip(&wanted)
        .all(|((.., yaw), want)| (yaw - want).abs() < 0.5);
    let order: Vec<&str> = made.iter().map(|(_, name, ..)| name.as_str()).collect();
    let alive = run.spawn().map_or(-1, |spawn| spawn.number_spawned);
    check(
        counts == [0, 1, 2, 3, 3]
            && placed
            && faced
            && order == [MERC, MERC_B, MERC_B]
            && alive == 3,
        format!(
            "made by step {counts:?}, {order:?} at their points {placed}, facing {faced} ({wanted:?}), alive {alive}"
        ),
    )
}

/// TSL: two creatures, three points: no share for each point, so both take
/// the second (the first moved off it to the first ring spot).
fn cursor_quirk() -> Result<String, String> {
    let facts = ute(vec![creature(MERC, 2.0, false, 2)], (2, 2), &points());
    let mut run = run(Setup::new("tsl", facts))?;
    let last = run.steps(4, MSEC);
    let made = run.made(&last);
    let second = points()[1];
    let ring = [second[0] - 1.0, second[1] - 1.0, 0.0];
    let ok = made.len() == 2 && same_spot(made[0].2, second) && same_spot(made[1].2, ring);
    check(
        ok,
        format!(
            "{:?} (second point {second:?})",
            made.iter().map(|m| m.2).collect::<Vec<_>>()
        ),
    )
}

/// K1: every creature at the point farthest from whoever entered.
fn farthest_point() -> Result<String, String> {
    let near = at(-3.0, -1.0);
    let far = at(-5.5, -2.5);
    let facts = ute(vec![creature(MERC, 0.5, false, 0)], (2, 2), &[near, far]);
    let mut run = run(Setup::new("k1", facts))?;
    let last = run.steps(4, MSEC);
    let made = run.made(&last);
    let ring = [far[0] - 1.0, far[1] - 1.0, 0.0];
    let ok = made.len() == 2 && same_spot(made[0].2, far) && same_spot(made[1].2, ring);
    check(
        ok,
        format!(
            "{:?} (far point {far:?})",
            made.iter().map(|m| m.2).collect::<Vec<_>>()
        ),
    )
}

/// An inactive encounter: entered, nothing; with an `OnEntered` that
/// activates it (106per's `t_spawndrd`), it spawns at once, the player
/// still inside, and once only.
fn on_entered_while_inactive() -> Result<String, String> {
    let quiet = Encounter {
        active: false,
        ..ute(vec![creature(MERC, 2.0, false, 3)], (3, 3), &[])
    };
    let mut asleep = run(Setup::new("tsl", quiet.clone()))?;
    let none = asleep.made_after(8, MSEC).len();
    let mut woken = run(Setup::new("tsl", quiet).hook("OnEntered", WAKE))?;
    let made = woken.made_after(12, MSEC).len();
    let started = woken
        .spawn()
        .is_some_and(|spawn| spawn.started && spawn.active);
    check(
        none == 0 && made == 3 && started,
        format!("without the script {none}, with it {made} (started and active {started})"),
    )
}

/// Who starts an encounter: a living creature hostile to its faction in
/// it: the player (`PlayerOnly`) yes, but not with the encounter of a
/// faction at peace with it; away from the player a hostile creature
/// that is no player starts it only without `PlayerOnly`, a dead one never.
fn who_starts_it() -> Result<String, String> {
    let facts = || ute(vec![creature(MERC, 2.0, false, 1)], (1, 1), &[]);
    let count = |setup: Setup| -> Result<usize, String> {
        let mut run = run(setup)?;
        let last = run.steps(6, MSEC);
        Ok(run.made(&last).len())
    };
    let friendly = Setup {
        faction: FRIENDLY_1,
        ..Setup::new("tsl", facts())
    };
    let stranger = |player_only: bool, dead: bool| Setup {
        away: true,
        extra: vec![("probe_stranger", PLAYER, at(5.0, 4.0), dead)],
        ..Setup::new(
            "tsl",
            Encounter {
                player_only,
                ..facts()
            },
        )
    };
    let counts = [
        count(Setup::new("tsl", facts()))?,
        count(friendly)?,
        count(stranger(true, false))?,
        count(stranger(false, false))?,
        count(stranger(false, true))?,
    ];
    check(
        counts == [1, 0, 0, 1, 0],
        format!(
            "player {}, at peace {}, stranger with PlayerOnly {}, without {}, dead {}",
            counts[0], counts[1], counts[2], counts[3], counts[4]
        ),
    )
}

/// Kills every creature made, then steps once.
fn kill_all(run: &mut Run) {
    let snapshot = run.world.snapshot();
    let ids: Vec<u32> = run.made(&snapshot).iter().map(|m| m.0).collect();
    run.kill(&ids);
    run.step(MSEC);
}

/// tar_m03ab's patrol rules (K1, `Reset`, 30 s, `Respawns` -1): made,
/// killed, it signals `OnExhausted` and turns inactive; 30 s on it is
/// active again, one reset counted, and spawns anew (the player inside).
fn exhausts_and_resets() -> Result<String, String> {
    let facts = Encounter {
        reset: true,
        reset_time_s: 30,
        respawns: -1,
        difficulty: 5,
        ..ute(
            vec![
                creature(MERC, 2.0, false, 0),
                creature(MERC_B, 3.0, true, 0),
            ],
            (2, 3),
            &[],
        )
    };
    let mut run = run(Setup::new("k1", facts).hook("OnExhausted", EXHAUSTED))?;
    let first = run.made_after(6, MSEC).len();
    kill_all(&mut run);
    let exhausted = run.global("exhausted");
    let inactive = run
        .spawn()
        .is_some_and(|spawn| !spawn.active && !spawn.started);
    run.steps(290, 100);
    let early = run.spawn().is_some_and(|spawn| spawn.active);
    run.steps(15, 100);
    let reset = run
        .spawn()
        .map(|spawn| (spawn.active, spawn.current_spawns));
    run.steps(6, MSEC);
    let alive = run.spawn().map_or(0, |spawn| spawn.number_spawned);
    check(
        first >= 2
            && exhausted == 1
            && inactive
            && !early
            && reset == Some((true, 1))
            && alive >= 2,
        format!(
            "made {first}, exhausted {exhausted} (inactive {inactive}), active at 29 s {early}, at 30.5 s {reset:?}, alive again {alive}"
        ),
    )
}

/// 402dxn's `enc_junglecannok` rules (`Reset`, `ResetTime` 1, but
/// `SpawnOption` 0): exhausted, it stays so.
fn no_reset_without_spawn_option() -> Result<String, String> {
    let facts = Encounter {
        reset: true,
        reset_time_s: 1,
        respawns: 10,
        spawn_option: 0,
        ..ute(vec![creature(MERC, 2.0, false, 2)], (2, 2), &[])
    };
    let mut run = run(Setup::new("tsl", facts))?;
    run.steps(5, MSEC);
    kill_all(&mut run);
    run.steps(30, 100);
    let spawn = run
        .spawn()
        .map(|spawn| (spawn.active, spawn.current_spawns));
    check(
        spawn == Some((false, 0)),
        format!("after 3 s: active and resets {spawn:?}"),
    )
}

/// The encounter's heartbeat: none while inactive; active, the first
/// stamp skipped, then one each 6 s.
fn heartbeat_while_active() -> Result<String, String> {
    let facts = Encounter {
        active: false,
        ..ute(vec![creature(MERC, 2.0, false, 1)], (1, 1), &[])
    };
    let mut run = run(Setup::new("tsl", facts).hook("OnHeartbeat", BEAT))?;
    run.steps(130, 100);
    let asleep = run.global("beats");
    let mut state = run.world.script_state().clone();
    if let Some(spawn) = state.encounters.get_mut(&run.encounter) {
        spawn.active = true;
    }
    run.world.set_script_state(state);
    run.steps(125, 100);
    let awake = run.global("beats");
    check(
        asleep == 0 && awake == 2,
        format!("inactive 13 s: {asleep}; active 12.5 s: {awake}"),
    )
}

/// The routines on an encounter: difficulty row 3 is 5 and a row past the
/// table changes nothing; resets most and done; active; the player in it
/// by `GetFirstInPersistentObject`.
fn routines() -> Result<String, String> {
    let facts = ute(vec![creature(MERC, 2.0, false, 1)], (1, 1), &[]);
    let mut run = run(Setup::new("tsl", facts))?;
    run.step(MSEC);
    let mut state = run.world.script_state().clone();
    state
        .pending
        .push(crate::script::Pending::new(ROUTINES, crate::script::MODULE));
    run.world.set_script_state(state);
    run.step(MSEC);
    let got: Vec<i32> = [
        "difficulty",
        "difficulty_past",
        "spawns_max",
        "spawns_current",
        "active",
        "pc_inside",
    ]
    .iter()
    .map(|name| run.global(name))
    .collect();
    check(got == [5, 5, 7, 2, 1, 1], format!("{got:?}"))
}

/// `GetEnteringObject` in the encounter's `OnEntered`: the player once
/// (counted; the creature it then makes over its outline comes in too, and
/// is none).
fn entering_object() -> Result<String, String> {
    let facts = ute(vec![creature(MERC, 2.0, false, 1)], (1, 1), &[]);
    let mut run = run(Setup::new("tsl", facts).hook("OnEntered", ENTERING))?;
    run.steps(3, MSEC);
    let entered = run.global("entering_pc");
    let last = run.spawn().and_then(|spawn| spawn.last_entered).is_some();
    check(
        entered == 1 && last,
        format!("entering is a PC {entered}, last entered kept {last}"),
    )
}

/// A world saved with half its list made loads with the rest of the list
/// and those alive counted, but no creature linked (`GetIsEncounterCreature`
/// 0 for them); the rest made, and killed, it does not exhaust: the first
/// three are still counted alive.
fn save_drops_links() -> Result<String, String> {
    let mut run = run(Setup::new("tsl", mercenaries_here()))?;
    run.steps(4, MSEC);
    let before = run
        .spawn()
        .map(|spawn| (spawn.list.len(), spawn.number_spawned));
    let level = run.world.level().clone();
    let save = crate::Save {
        hz: crate::TICK_HZ,
        level: level.id(),
        snapshot: run.world.snapshot(),
        script: run.world.script_state().clone(),
    };
    let loaded = crate::Save::decode(&save.encode()).map_err(|error| error.to_string())?;
    let kept = loaded
        .script
        .encounters
        .get(&run.encounter)
        .map(|spawn| (spawn.list.len(), spawn.number_spawned));
    let links = loaded.script.encounter_creatures.len();
    let world = loaded.world(&level, 1, None)?;
    let mut again = Run { world, ..run };
    let loaded_snapshot = again.world.snapshot();
    let early: Vec<u32> = again.made(&loaded_snapshot).iter().map(|m| m.0).collect();
    let all = again.made_after(5, MSEC);
    let late: Vec<u32> = all
        .iter()
        .map(|m| m.0)
        .filter(|id| !early.contains(id))
        .collect();
    again.kill(&late);
    again.step(MSEC);
    let after = again
        .spawn()
        .map(|spawn| (spawn.active, spawn.number_spawned));
    check(
        before == Some((3, 3))
            && kept == before
            && links == 0
            && all.len() == 6
            && after == Some((true, 3)),
        format!(
            "saved (left, alive) {before:?}, loaded {kept:?} with {links} links; made {}; the late three killed: (active, alive) {after:?}",
            all.len()
        ),
    )
}

/// The mercenaries' numbers with templates the probe has: six of three,
/// the leader single.
fn mercenaries_here() -> Encounter {
    ute(
        vec![
            creature(MERC, 1.0, false, 0),
            creature(MERC_B, 1.0, true, 0),
        ],
        (6, 6),
        &[],
    )
}

/// The spawn script of a creature an encounter made sees it as one
/// (`GetIsEncounterCreature`, as `k_inc_generic`'s spawn routine asks).
fn spawn_script_sees_its_encounter() -> Result<String, String> {
    let facts = ute(vec![creature(MERC, 2.0, false, 1)], (1, 1), &[]);
    let mut run = run(Setup::new("tsl", facts))?;
    run.steps(4, MSEC);
    let seen = run.global("spawned_encounter");
    let linked = run.world.script_state().encounter_creatures.len();
    check(
        seen == 1 && linked == 1,
        format!("in its spawn script {seen}, linked {linked}"),
    )
}
