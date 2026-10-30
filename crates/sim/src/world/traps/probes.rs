//! The probes of `world::traps` (`sim::traps::probes`; the approved test
//! `traps_lifecycle`): traps on the proving ground.

use crate::action::{Action, Verb};
use crate::combat::Sheet;
use crate::combat::sheet::factions::{HOSTILE_1, PLAYER};
use crate::input::Seat;
use crate::inventory::{Catalog, FIRST_ITEM, Item, PARTY};
use crate::module::{ObjectData, ObjectKind, Placement, Repute};
use crate::notice::Notice;
use crate::probe::check;
use crate::snapshot::trap;
use crate::traps::{Trap, TrapRow};
use crate::{StepReason, TickInput};

/// The probe's routines.
const CATALOG: &str = r#"
// 0
void SendMessageToPC(object oPlayer, string szMessage);
// 1
object GetFirstPC();
// 2
string IntToString(int nInteger);
// 3
int GetReflexAdjustedDamage(int nDamage, object oTarget, int nDC, int nSaveType, object oSaveVersus);
// 4
effect EffectPoison(int nPoisonType);
// 5
void ApplyEffectToObject(int nDurationType, effect eEffect, object oTarget, float fDuration);
// 6
object GetObjectByTag(string sTag, int nNth);
// 7
int GetIsTrapped(object oObject);
// 8
int GetTrapBaseType(object oTrapObject);
// 9
int GetTrapDetectDC(object oTrapObject);
// 10
int GetTrapDisarmDC(object oTrapObject);
// 11
int GetTrapOneShot(object oTrapObject);
// 12
int GetTrapDetectedBy(object oTrapObject, object oCreature);
// 13
int SetTrapDetectedBy(object oTrap, object oDetector);
// 14
void SetTrapDisabled(object oTrap);
// 15
object GetNearestTrapToObject(object oTarget, int nTrapDetected);
// 16
string GetTag(object oObject);
// 17
void SetCurrentStealthXP(int nCurrent);
// 18
int GetCurrentStealthXP();
// 19
void AwardStealthXP(object oTarget);
// 20
int GetStealthXPEnabled();
// 21
int GetIsEnemy(object oTarget, object oSource);
// 22
int GetIsFriend(object oTarget, object oSource);
// 23
int GetIsNeutral(object oTarget, object oSource);
"#;

/// Scripts that tell the player one word.
const SAYS: [(&str, &str); 3] = [
    ("probe_sprung", "sprung"),
    ("probe_disarmed", "disarmed"),
    ("probe_gate_sprung", "gate sprung"),
];

fn library() -> crate::script::Library {
    use fmt_odyssey::ncs::{Const, Op};
    use script_ncs::probes::Asm;
    let pc = Op::Action {
        routine: 1,
        args: 0,
    };
    let tell = Op::Action {
        routine: 0,
        args: 2,
    };
    let mut scripts: Vec<(String, fmt_odyssey::ncs::Ncs)> = SAYS
        .map(|(name, words)| {
            let mut asm = Asm::default();
            asm.op(Op::Const(Const::String(words.as_bytes().to_vec())))
                .op(pc.clone())
                .op(tell.clone())
                .op(Op::Retn);
            (name.to_string(), asm.build())
        })
        .to_vec();
    // Tells the player 30 Reflex-adjusted against DC 100 (no save), then
    // against DC -100 (always saved).
    let mut reflex = Asm::default();
    for dc in [100, -100] {
        reflex
            .op(pc.clone())
            .op(Op::Const(Const::Int(0)))
            .op(Op::Const(Const::Int(dc)))
            .op(pc.clone())
            .op(Op::Const(Const::Int(30)))
            .op(Op::Action {
                routine: 3,
                args: 5,
            })
            .op(Op::Action {
                routine: 2,
                args: 1,
            })
            .op(pc.clone())
            .op(tell.clone());
    }
    reflex.op(Op::Retn);
    scripts.push(("probe_reflex".into(), reflex.build()));
    // Poisons the player with `poison.2da` rows 3 (no save) and 4 (always
    // saved), for good.
    let mut poison = Asm::default();
    for row in [3, 4] {
        poison
            .op(Op::Const(Const::Float(0.0)))
            .op(pc.clone())
            .op(Op::Const(Const::Int(row)))
            .op(Op::Action {
                routine: 4,
                args: 1,
            })
            .op(Op::Const(Const::Int(2)))
            .op(Op::Action {
                routine: 5,
                args: 4,
            });
    }
    poison.op(Op::Retn);
    scripts.push(("probe_poison".into(), poison.build()));
    scripts.push(("probe_routines".into(), routines_script()));
    // Run as a trap: how the trap (OBJECT_SELF) regards the player, told
    // as GetIsEnemy, GetIsFriend, GetIsNeutral.
    let mut regard = Asm::default();
    for routine in [21, 22, 23] {
        regard
            .op(Op::Const(Const::Object(0)))
            .op(pc.clone())
            .op(Op::Action { routine, args: 2 })
            .op(Op::Action {
                routine: 2,
                args: 1,
            })
            .op(pc.clone())
            .op(tell.clone());
    }
    regard.op(Op::Retn);
    scripts.push(("probe_regard".into(), regard.build()));
    let talents = crate::script::Talents {
        poisons: courses(),
        ..Default::default()
    };
    crate::probe::library_with(CATALOG, scripts, talents).with_items(Catalog::new([
        crate::world::containers::plain("probe_key"),
        crate::inventory::ItemFacts {
            trap: Some(3),
            stacking: 99,
            ..crate::world::containers::plain("g_i_trapkit004")
        },
    ]))
}

/// A point of the proving ground's floor (metres, its own axes).
fn floor_at([x, y]: [f32; 2]) -> [f32; 3] {
    crate::determinism::ground_point(x, y, 0.0)
}

/// Where the probe's things stand.
const MINE_AT: [f32; 2] = [-2.0, -2.0];
const KEYED_AT: [f32; 2] = [-2.0, 2.0];
const ALLY_AT: [f32; 2] = [-6.0, -2.0];
const AWAY: [f32; 2] = [-4.0, 0.0];
const BY_GATE: [f32; 2] = [-1.2, 0.0];
/// Within reach of the mine (1.5 m off), out of its square; 5 m off.
const BY_MINE: [f32; 2] = [-3.5, -2.0];
const NEAR_MINE: [f32; 2] = [-7.0, -2.0];
const DEEP_AT: [f32; 2] = [-6.0, 2.0];
const BY_DEEP: [f32; 2] = [-6.0, 0.5];

/// The proving ground's door (its first object), and what the probe adds
/// after its trigger.
const GATE: u32 = 0;
const MINE: u32 = 2;
const KEYED: u32 = 3;
const ALLY: u32 = 4;
const DEEP: u32 = 5;

/// `traps.2da` row 3 with the probe's `script`, set against `set_dc`.
fn rows(script: &str, set_dc: u8) -> Vec<TrapRow> {
    let mut rows = vec![TrapRow::default(); 4];
    rows[3] = TrapRow {
        label: "TRAP_FRAGMENTATION_MINE_MINOR".into(),
        script: script.into(),
        set_dc,
        detect_mod: 20,
        disarm_mod: 20,
        name: 1281,
        kit: "g_i_trapkit004".into(),
        model: "v_mnfrag".into(),
        sound: "cb_gr_fragment".into(),
    };
    rows
}

/// A trap of row 3, one-shot, DCs 20.
fn frag(key: &str) -> Trap {
    Trap {
        row: 3,
        detectable: true,
        disarmable: true,
        one_shot: true,
        detect_dc: 20,
        disarm_dc: 20,
        key: key.into(),
        ..Trap::default()
    }
}

/// A trap trigger at `at`: a metre square, the hostile faction's.
fn mine(tag: &str, at: [f32; 2], key: &str, hooks: &[(&str, &str)]) -> ObjectData {
    ObjectData {
        kind: ObjectKind::Trigger,
        tag: tag.into(),
        geometry: vec![
            [-0.5, -0.5, 0.0],
            [0.5, -0.5, 0.0],
            [0.5, 0.5, 0.0],
            [-0.5, 0.5, 0.0],
        ],
        faction: HOSTILE_1,
        trap: Some(frag(key)),
        scripts: hooks
            .iter()
            .map(|(hook, script)| (hook.to_string(), script.to_string()))
            .collect(),
        ..ObjectData::unknown(tag, floor_at(at), 0.0)
    }
}

/// `poison.2da` rows 3 (never saved against: DC 100) and 4 (always saved:
/// DC 1), as their courses.
fn courses() -> Vec<crate::combat::items::Course> {
    let mut rows = vec![crate::combat::items::Course::default(); 5];
    rows[3] = crate::combat::items::Course {
        points: 3,
        every_ms: 3000,
        for_ms: 30_000,
        dc: 100,
        ..Default::default()
    };
    rows[4] = crate::combat::items::Course {
        points: 4,
        dc: 1,
        ..rows[3]
    };
    rows
}

/// `world_for` with the soldier in one seat.
fn world(script: &str) -> Result<crate::world::SimWorld, String> {
    world_for(script, Sheet::dev_soldier(), 1)
}

/// The script the door runs when it springs in `routines`: it asks the
/// trap routines about the mine and the stealth XP routines, telling the
/// player each answer.
fn routines_script() -> fmt_odyssey::ncs::Ncs {
    use fmt_odyssey::ncs::{Const, Op};
    let call = |routine: u16, args: u8| Op::Action { routine, args };
    let pc = call(1, 0);
    let mine = [
        Op::Const(Const::Int(0)),
        Op::Const(Const::String(b"mine".to_vec())),
        call(6, 2),
    ];
    let mut asm = script_ncs::probes::Asm::default();
    // Tells the int on top of the stack.
    let tell = |asm: &mut script_ncs::probes::Asm| {
        asm.op(call(2, 1)).op(pc.clone()).op(call(0, 2));
    };
    // GetIsTrapped, GetTrapBaseType, ...DetectDC, ...DisarmDC, ...OneShot.
    for routine in [7, 8, 9, 10, 11] {
        for op in mine.clone() {
            asm.op(op);
        }
        asm.op(call(routine, 1));
        tell(&mut asm);
    }
    // Detected by the PC before and after SetTrapDetectedBy (its result
    // told between).
    for routine in [12, 13, 12] {
        asm.op(pc.clone());
        for op in mine.clone() {
            asm.op(op);
        }
        asm.op(call(routine, 2));
        tell(&mut asm);
    }
    // The nearest trap the PC detected: the mine.
    asm.op(Op::Const(Const::Int(1)))
        .op(pc.clone())
        .op(call(15, 2))
        .op(call(16, 1))
        .op(pc.clone())
        .op(call(0, 2));
    // Disabled, it is trapped no more.
    for op in mine.clone() {
        asm.op(op);
    }
    asm.op(call(14, 1));
    for op in mine.clone() {
        asm.op(op);
    }
    asm.op(call(7, 1));
    tell(&mut asm);
    // Stealth XP: 500 clamped to the area's 100; awarded; then none, off.
    asm.op(Op::Const(Const::Int(500))).op(call(17, 1));
    asm.op(call(18, 0));
    tell(&mut asm);
    asm.op(pc.clone()).op(call(19, 1));
    asm.op(call(18, 0));
    tell(&mut asm);
    asm.op(call(20, 0));
    tell(&mut asm);
    asm.op(Op::Retn);
    asm.build()
}

/// The proving ground with a mine (its row running `script`), a keyed
/// trap, a creature of the traps' faction, a trap no one disarms (DC 40)
/// and the door trapped; the player's faction at war with theirs; `seats`
/// players with `sheet`.
fn world_for(script: &str, sheet: Sheet, seats: u8) -> Result<crate::world::SimWorld, String> {
    world_set(script, sheet, seats, 15)
}

/// `world_for`, the mine's row set against `set_dc`.
fn world_set(
    script: &str,
    sheet: Sheet,
    seats: u8,
    set_dc: u8,
) -> Result<crate::world::SimWorld, String> {
    let ground = crate::determinism::proving_ground();
    let mut data = (**ground.module().ok_or("the ground has no module")?).clone();
    let Some(Placement::Own(gate)) = data.placements.first_mut() else {
        return Err("the ground's first object is not its door".into());
    };
    gate.faction = HOSTILE_1;
    gate.trap = Some(frag(""));
    let gate_script = if script == "probe_routines" {
        "probe_routines"
    } else {
        "probe_gate_sprung"
    };
    gate.scripts = vec![("OnTrapTriggered".into(), gate_script.into())];
    data.placements.extend(
        [
            mine("mine", MINE_AT, "", &[("OnDisarm", "probe_disarmed")]),
            mine(
                "keyed",
                KEYED_AT,
                "probe_key",
                &[("OnDisarm", "probe_disarmed")],
            ),
            ObjectData {
                sheet: Some(Sheet {
                    faction: HOSTILE_1,
                    ..Sheet::commoner()
                }),
                ..ObjectData::unknown("ally", floor_at(ALLY_AT), 0.0)
            },
            ObjectData {
                trap: Some(Trap {
                    disarm_dc: 40,
                    ..frag("")
                }),
                ..mine("deep", DEEP_AT, "", &[])
            },
        ]
        .map(Placement::Own),
    );
    data.traps = rows(script, set_dc);
    data.area_stealth = crate::module::AreaStealth {
        mod_listen: 0,
        xp_max: 100,
        xp_current: 60,
        xp_loss: 20,
        xp_enabled: true,
    };
    let war = [usize::from(PLAYER), usize::from(HOSTILE_1)];
    data.repute = Repute {
        rows: (0..7)
            .map(|a| {
                (0..7)
                    .map(|b| match (a, b) {
                        _ if a == b => 100,
                        _ if war.contains(&a) && war.contains(&b) => 0,
                        _ => 50,
                    })
                    .collect()
            })
            .collect(),
    };
    let level = ground.with_module(data).with_scripts(library());
    crate::world::SimWorld::new(
        crate::WorldConfig {
            seed: 1,
            seats,
            wanderers: 0,
            spawn: [0.0; 3],
            yaw: 0.0,
            sheet,
            guests: crate::Guests::Avatars,
        },
        level,
    )
    .map_err(|error| error.to_string())
}

/// Puts the player at `player` and the ally at `ally` (metres).
fn place(world: &mut crate::world::SimWorld, player: [f32; 2], ally: [f32; 2]) {
    use crate::world::{Avatar, Object, Player};
    let mut bodies = world
        .world
        .query::<(Option<&Avatar>, Option<&Object>, &mut Player)>();
    for (avatar, object, mut body) in bodies.iter_mut(&mut world.world) {
        let at = match (avatar, object) {
            (Some(_), None) => player,
            (None, Some(object)) if object.index == ALLY => ally,
            _ => continue,
        };
        let at = floor_at(at);
        body.0.origin[0] = at[0] / collision::UNIT;
        body.0.origin[1] = at[1] / collision::UNIT;
    }
}

/// Puts each seat's player where `at` says (metres), by seat.
fn place_seats(world: &mut crate::world::SimWorld, at: [[f32; 2]; 2]) {
    use crate::world::{Avatar, Player};
    let mut bodies = world.world.query::<(&Avatar, &mut Player)>();
    for (avatar, mut body) in bodies.iter_mut(&mut world.world) {
        let to = floor_at(at[usize::from(avatar.0.0.min(1))]);
        body.0.origin[0] = to[0] / collision::UNIT;
        body.0.origin[1] = to[1] / collision::UNIT;
    }
}

/// Sets object `index`'s trap bits.
fn mark(world: &mut crate::world::SimWorld, index: u32, bits: u8) {
    let mut objects = world.world.query::<&mut crate::world::Object>();
    for mut object in objects.iter_mut(&mut world.world) {
        if object.index == index {
            object.state.trap = bits;
        }
    }
}

/// Gives the party the key.
fn give_key(world: &mut crate::world::SimWorld) {
    let mut state = world.script_state().clone();
    let id = FIRST_ITEM + state.next_item;
    state.next_item += 1;
    state.items.insert(
        id,
        Item {
            template: "probe_key".into(),
            upgrades: 0,
            fitted: Vec::new(),
            holder: PARTY,
            slot: None,
            stack: 1,
            charges: 0,
            droppable: true,
            plot: false,
            identified: true,
            infinite: false,
            non_equippable: false,
        },
    );
    world.set_script_state(state);
}

/// What a run said and its snapshots' ends.
struct Ran {
    /// What was said, by phase.
    said: Vec<Vec<String>>,
    /// Whether each object stood at each phase's end, by index.
    standing: Vec<Vec<u32>>,
    last: crate::Snapshot,
}

/// The phases: the ally onto the mine; the player onto it; the player,
/// carrying the key, onto the keyed trap; the player opening the door.
/// `seen`: the party saw the mine before.
fn run(seen: bool) -> Result<Ran, String> {
    let mut world = world("probe_sprung")?;
    if seen {
        mark(&mut world, MINE, trap::PARTY_SEES);
    }
    let mut ran = Ran {
        said: Vec::new(),
        standing: Vec::new(),
        last: world.snapshot(),
    };
    let phases: [(&str, [f32; 2], [f32; 2]); 4] = [
        ("ally", AWAY, MINE_AT),
        ("player", MINE_AT, ALLY_AT),
        ("key", KEYED_AT, ALLY_AT),
        ("gate", BY_GATE, ALLY_AT),
    ];
    let mut tick = 0;
    for (name, player, ally) in phases {
        if name == "key" {
            give_key(&mut world);
        }
        let mut said = Vec::new();
        for step in 0..10 {
            tick += 1;
            place(&mut world, player, ally);
            let mut input = TickInput::default();
            if name == "gate" && step == 0 {
                input.act(
                    Seat(0),
                    Action::Use {
                        object: GATE,
                        verb: Verb::Open,
                    },
                );
            }
            ran.last = crate::step(&mut world, tick, &input, 8, StepReason::Authority);
            said.extend(
                world
                    .take_notices()
                    .into_iter()
                    .filter_map(|(_, notice)| match notice {
                        Notice::Said(words) => Some(words),
                        _ => None,
                    }),
            );
        }
        ran.said.push(said);
        ran.standing.push(
            ran.last
                .entities
                .iter()
                .filter_map(|entity| match entity.kind {
                    crate::Kind::Object(_, index) => Some(index),
                    _ => None,
                })
                .collect(),
        );
    }
    Ok(ran)
}

/// The player onto a mine whose row runs `script`: what was said, and the
/// player's fighter after.
fn spring_with(script: &str) -> Result<(Vec<String>, crate::combat::Fighter), String> {
    let mut world = world(script)?;
    let mut said = Vec::new();
    let mut last = world.snapshot();
    for tick in 1..=10 {
        place(&mut world, MINE_AT, ALLY_AT);
        last = crate::step(
            &mut world,
            tick,
            &TickInput::default(),
            8,
            StepReason::Authority,
        );
        said.extend(
            world
                .take_notices()
                .into_iter()
                .filter_map(|(_, notice)| match notice {
                    Notice::Said(words) => Some(words),
                    _ => None,
                }),
        );
    }
    let fighter = last
        .entities
        .iter()
        .find(|entity| matches!(entity.kind, crate::Kind::Avatar(_)))
        .and_then(|entity| entity.fighter)
        .ok_or("no player")?;
    Ok((said, fighter))
}

/// What `k_trp_generic` asks of the engine beyond springing:
/// `GetReflexAdjustedDamage` (a Reflex save against the DC halves it) and
/// `EffectPoison` (a Fortitude save against the row's `dc_save` keeps it
/// off; else its points every period).
pub(crate) fn trap_routines() -> Vec<(&'static str, Result<String, String>)> {
    let reflex = spring_with("probe_reflex").map(|(said, _)| said);
    let poisoned = spring_with("probe_poison").map(|(_, fighter)| fighter.pack.poison);
    vec![
        (
            "reflex_adjusted_damage",
            check(
                reflex.as_ref().is_ok_and(|said| said == &["30", "15"]),
                format!("{reflex:?}"),
            ),
        ),
        (
            "effect_poison",
            check(
                poisoned.as_ref().is_ok_and(|poison| {
                    poison.is_some_and(|poison| {
                        poison.course.points == 3 && poison.course.every_ms == 3000
                    })
                }),
                format!("{poisoned:?}"),
            ),
        ),
    ]
}

/// The checks of springing (see the module documentation).
pub(crate) fn springing() -> Vec<(&'static str, Result<String, String>)> {
    let ran = match run(false) {
        Ok(ran) => ran,
        Err(why) => return vec![("trap_scenario", Err(why))],
    };
    let seen = run(true);
    let again = run(false).map(|again| again.last.hash());
    let said = |phase: usize, words: &str| ran.said[phase].iter().any(|said| said == words);
    let stands = |phase: usize, index: u32| ran.standing[phase].contains(&index);
    vec![
        (
            "spare_own_faction",
            check(
                !said(0, "sprung") && stands(0, MINE),
                format!("{:?}", ran.said[0]),
            ),
        ),
        (
            "springs_on_hostile",
            check(said(1, "sprung"), format!("{:?}", ran.said[1])),
        ),
        (
            "one_shot_gone",
            check(!stands(1, MINE), format!("{:?}", ran.standing[1])),
        ),
        (
            "key_disarms",
            check(
                said(2, "disarmed") && !said(2, "sprung") && !stands(2, KEYED),
                format!("{:?}", ran.said[2]),
            ),
        ),
        (
            "trapped_door_springs",
            check(said(3, "gate sprung"), format!("{:?}", ran.said[3])),
        ),
        (
            "detected_still_springs",
            check(
                seen.as_ref()
                    .is_ok_and(|seen| seen.said[1].iter().any(|words| words == "sprung")),
                format!("{:?}", seen.map(|seen| seen.said[1].clone())),
            ),
        ),
        (
            "trap_runs_alike",
            check(
                again.as_ref().ok() == Some(&ran.last.hash()),
                "two runs".into(),
            ),
        ),
    ]
}

/// The soldier with `awareness` and `demolitions` ranks.
fn skilled(awareness: u8, demolitions: u8) -> Sheet {
    use crate::combat::sheet::skills;
    let mut sheet = Sheet::dev_soldier();
    sheet.skills[skills::AWARENESS] = awareness;
    sheet.skills[skills::DEMOLITIONS] = demolitions;
    sheet
}

/// What a stretch of steps saw: every notice, and the last snapshot.
struct Stretch {
    notices: Vec<(Seat, Notice)>,
    last: crate::Snapshot,
}

/// `steps` steps with the player at `at` (the ally away), `acts` asked on
/// the first; from tick `from`.
fn stretch(
    world: &mut crate::world::SimWorld,
    from: u64,
    steps: u64,
    at: [f32; 2],
    acts: &[(Seat, Action)],
) -> Stretch {
    let mut notices = Vec::new();
    let mut last = None;
    for tick in from..from + steps {
        place(world, at, ALLY_AT);
        let mut input = TickInput::default();
        if tick == from {
            for (seat, action) in acts {
                input.act(*seat, action.clone());
            }
        }
        last = Some(crate::step(world, tick, &input, 8, StepReason::Authority));
        notices.extend(world.take_notices());
    }
    Stretch {
        notices,
        last: last.unwrap_or_else(|| world.snapshot()),
    }
}

/// Object `index`'s trap bits in `snapshot`; `None` once it is gone.
fn trap_bits(snapshot: &crate::Snapshot, index: u32) -> Option<u8> {
    snapshot
        .entities
        .iter()
        .find(|entity| matches!(entity.kind, crate::Kind::Object(_, i) if i == index))
        .and_then(|entity| entity.object)
        .map(|object| object.trap)
}

/// A skill check's line for `skill` came, and how it went.
fn checked(notices: &[(Seat, Notice)], skill: u32) -> Option<(Option<u8>, bool)> {
    notices.iter().find_map(|(_, notice)| match notice {
        Notice::Check {
            skill: line,
            roll,
            success,
            ..
        } if *line == skill => Some((*roll, *success)),
        _ => None,
    })
}

fn said(notices: &[(Seat, Notice)], words: &str) -> bool {
    notices
        .iter()
        .any(|(_, notice)| matches!(notice, Notice::Said(said) if said == words))
}

fn refused(notices: &[(Seat, Notice)], seat: u8) -> bool {
    notices.iter().any(|(to, notice)| {
        to.0 == seat && *notice == Notice::Line(crate::world::locks::lines::CANNOT_USE_SKILL)
    })
}

/// Detecting, disarming and recovering (S5, S7, S8): every check on its
/// own world, so none leans on another's.
pub(crate) fn skill_uses() -> Vec<(&'static str, Result<String, String>)> {
    use crate::world::locks::lines;
    let disarm = |seat: u8, index: u32| (Seat(seat), Action::Disarm(index));
    let held = crate::traps::DISARM_MS / 8 + 5;
    let mut checks = Vec::new();
    let mut run = |name: &'static str, test: &dyn Fn() -> Result<(bool, String), String>| {
        checks.push((
            name,
            match test() {
                Ok((ok, what)) => check(ok, what),
                Err(why) => Err(why),
            },
        ));
    };
    // Awareness 4 at 5 m from a DC 20 mine finds it within a second (a
    // d10 of 6 or more in ten tries); the party sees it and is told.
    run("detect_party_sees", &|| {
        let mut world = world_for("probe_sprung", skilled(4, 0), 1)?;
        let seen = stretch(&mut world, 1, 125, NEAR_MINE, &[]);
        let bits = trap_bits(&seen.last, MINE);
        let told = seen
            .notices
            .iter()
            .any(|(_, notice)| matches!(notice, Notice::TrapSeen { name: 1281 }));
        let kept = world
            .script_state()
            .traps
            .values()
            .any(|kept| !kept.detected_by.is_empty());
        Ok((
            bits.is_some_and(|bits| bits & trap::PARTY_SEES != 0) && told && kept,
            format!("bits {bits:?}, told {told}, kept {kept}"),
        ))
    });
    // A creature of the trap's faction never looks for its own traps.
    run("own_faction_skips", &|| {
        let mut world = world_for("probe_sprung", skilled(0, 0), 1)?;
        let ran = stretch(&mut world, 1, 125, AWAY, &[]);
        let ally = ran
            .last
            .entities
            .iter()
            .find(|entity| matches!(entity.kind, crate::Kind::Object(_, ALLY)))
            .map(|entity| entity.id.0)
            .ok_or("no ally")?;
        let ally_found = world
            .script_state()
            .traps
            .values()
            .any(|kept| kept.detected_by.contains(&ally));
        Ok((!ally_found, format!("ally found one: {ally_found}")))
    });
    // Demolitions 2 out of combat takes 20: 22 against 20; 4.5 s later the
    // mine is gone and its OnDisarm ran.
    run("disarm_done", &|| {
        let mut world = world_for("probe_sprung", skilled(0, 2), 1)?;
        mark(&mut world, MINE, trap::PARTY_SEES);
        let done = stretch(&mut world, 1, held, BY_MINE, &[disarm(0, MINE)]);
        let line = checked(&done.notices, lines::DISARM_MINE);
        Ok((
            line == Some((None, true))
                && said(&done.notices, "disarmed")
                && trap_bits(&done.last, MINE).is_none(),
            format!("{line:?}, gone {}", trap_bits(&done.last, MINE).is_none()),
        ))
    });
    // Its user plays KOTOR's disarm animation for the use's 4.5 s.
    run("disarm_plays_animation", &|| {
        let mut world = world_for("probe_sprung", skilled(0, 2), 1)?;
        mark(&mut world, MINE, trap::PARTY_SEES);
        let begun = stretch(&mut world, 1, 5, BY_MINE, &[disarm(0, MINE)]);
        let acting = begun
            .last
            .entities
            .iter()
            .find(|entity| matches!(entity.kind, crate::Kind::Avatar(_)))
            .and_then(|entity| entity.fighter)
            .and_then(|fighter| fighter.acting);
        Ok((
            acting.is_some_and(|acting| {
                acting.animation == crate::traps::animations::DISARM
                    && u64::from(acting.ms) == crate::traps::DISARM_MS
            }),
            format!("{acting:?}"),
        ))
    });
    // Walking off before it is done ends it: no check, the mine stays.
    run("disarm_cancelled_by_moving", &|| {
        let mut world = world_for("probe_sprung", skilled(0, 2), 1)?;
        mark(&mut world, MINE, trap::PARTY_SEES);
        let begun = stretch(&mut world, 1, 100, BY_MINE, &[disarm(0, MINE)]);
        let after = stretch(&mut world, 101, held, AWAY, &[]);
        let line = checked(&after.notices, lines::DISARM_MINE)
            .or(checked(&begun.notices, lines::DISARM_MINE));
        Ok((
            line.is_none() && trap_bits(&after.last, MINE).is_some(),
            format!("{line:?}"),
        ))
    });
    // A mine the party has not found, and a soldier with no Demolitions:
    // refused.
    run("disarm_needs_detection", &|| {
        let mut world = world_for("probe_sprung", skilled(0, 2), 1)?;
        let tried = stretch(&mut world, 1, 10, BY_MINE, &[disarm(0, MINE)]);
        Ok((refused(&tried.notices, 0), format!("{:?}", tried.notices)))
    });
    run("disarm_needs_ranks", &|| {
        let mut world = world_for("probe_sprung", skilled(0, 0), 1)?;
        mark(&mut world, MINE, trap::PARTY_SEES);
        let tried = stretch(&mut world, 1, 10, BY_MINE, &[disarm(0, MINE)]);
        Ok((refused(&tried.notices, 0), format!("{:?}", tried.notices)))
    });
    // Recovering asks 30: Demolitions 12 takes 20 to 32; the mine goes
    // without its OnDisarm, the party holds its kit.
    run("recover_gives_kit", &|| {
        let mut world = world_for("probe_sprung", skilled(0, 12), 1)?;
        mark(&mut world, MINE, trap::PARTY_SEES);
        let done = stretch(
            &mut world,
            1,
            held,
            BY_MINE,
            &[(Seat(0), Action::Recover(MINE))],
        );
        let kit = world
            .script_state()
            .items
            .values()
            .any(|item| item.holder == PARTY && item.template == "g_i_trapkit004");
        let line = checked(&done.notices, lines::RECOVER_MINE);
        Ok((
            line == Some((None, true))
                && kit
                && !said(&done.notices, "disarmed")
                && trap_bits(&done.last, MINE).is_none(),
            format!("{line:?}, kit {kit}"),
        ))
    });
    // Two seats on one mine in one step: the lower seat's; the other is
    // refused.
    run("two_seats_lower_first", &|| {
        let mut world = world_for("probe_sprung", skilled(0, 2), 2)?;
        mark(&mut world, MINE, trap::PARTY_SEES);
        let done = stretch(
            &mut world,
            1,
            held,
            BY_MINE,
            &[disarm(0, MINE), disarm(1, MINE)],
        );
        let line = checked(&done.notices, lines::DISARM_MINE);
        Ok((
            refused(&done.notices, 1) && line == Some((None, true)),
            format!("{line:?}"),
        ))
    });
    // A DC past 35 never yields, and 20 + 1 falls more than 10 short of 40:
    // the trap springs on whoever tried.
    run("hopeless_disarm_springs", &|| {
        let mut world = world_for("probe_sprung", skilled(0, 1), 1)?;
        mark(&mut world, DEEP, trap::PARTY_SEES);
        let done = stretch(&mut world, 1, held, BY_DEEP, &[disarm(0, DEEP)]);
        let line = checked(&done.notices, lines::DISARM_MINE);
        Ok((
            line == Some((None, false)) && said(&done.notices, "sprung"),
            format!("{line:?}"),
        ))
    });
    checks
}

/// The use menu's choices on a trap (S7, S8) and the actions on the wire.
pub(crate) fn choices() -> Vec<(&'static str, Result<String, String>)> {
    use crate::target::{Choice, ask, choices};
    let facts = mine("mine", MINE_AT, "", &[]);
    let seen = crate::snapshot::ObjectState {
        trap: trap::PARTY_SEES,
        ..crate::snapshot::ObjectState::default()
    };
    let offer = |state, recover: Option<bool>| {
        let demolitions = recover.map(|recover| crate::target::Demolitions {
            recover,
            door_mine: false,
        });
        choices(ObjectKind::Trigger, &facts, state, true, None, demolitions)
    };
    let offered = offer(seen, Some(true));
    let no_kit = offer(seen, Some(false));
    let unseen = offer(crate::snapshot::ObjectState::default(), Some(true));
    // K1 lists neither for a character with no ranks in Demolitions.
    let untrained = offer(seen, None);
    let wire = [Action::Disarm(7), Action::Recover(7)]
        .iter()
        .all(|action| Action::from_bytes(&action.to_bytes()).as_ref() == Ok(action));
    // A walk to disarm or recover stops more than half a metre outside the
    // trap's circle (a run's stop slides on; `mine_walk` walks it) and
    // within the use's reach: it never walks onto the mine.
    let stands = [Choice::Disarm, Choice::Recover].map(|choice| {
        let stand = crate::target::stand_off(choice, ObjectKind::Trigger);
        stand > crate::traps::RADIUS_M + 0.5
            && stand < crate::target::reach(choice, ObjectKind::Trigger)
    });
    vec![
        (
            "trap_choices",
            check(
                offered == [Choice::Disarm, Choice::Recover]
                    && no_kit == [Choice::Disarm]
                    && unseen.is_empty()
                    && untrained.is_empty(),
                format!("{offered:?} {no_kit:?} {unseen:?} untrained {untrained:?}"),
            ),
        ),
        (
            "trap_asks",
            check(
                ask(Choice::Disarm, 7, ObjectKind::Trigger) == Some(Action::Disarm(7))
                    && ask(Choice::Recover, 7, ObjectKind::Trigger) == Some(Action::Recover(7))
                    && wire,
                format!("wire {wire}"),
            ),
        ),
        (
            "trap_walk_stands_off",
            check(stands == [true; 2], format!("{stands:?}")),
        ),
        ("skill_floats", skill_floats()),
    ]
}

/// What floats over a skill's user as its check ends, as K1's client
/// floats it (`locks::floats`, Q-0335).
fn skill_floats() -> Result<String, String> {
    use crate::world::locks::{floats, lines};
    let (disarm, security) = (lines::DISARM_MINE, lines::SECURITY);
    let (won, lost) = (Some(lines::SUCCESS), Some(lines::FAILURE));
    let (low, impossible) = (Some(lines::SKILL_TOO_LOW), Some(lines::IMPOSSIBLE));
    // The skill, the roll (none: taking 20), the DC, success, sprung; the
    // words floated.
    let cases = [
        ((disarm, None, 20, true, false), won),
        ((disarm, None, 30, false, false), low),
        ((disarm, None, 40, false, true), low),
        ((disarm, Some(5), 20, false, false), lost),
        ((disarm, Some(1), 30, false, true), None),
        ((security, None, 20, true, false), None),
        ((security, Some(3), 20, false, false), lost),
        ((security, None, 30, false, false), low),
        ((security, None, 60, false, false), impossible),
        ((lines::SET_MINE, None, 20, false, false), None),
    ];
    let wrong: Vec<usize> = (0..cases.len())
        .filter(|at| {
            let ((skill, roll, dc, success, sprung), floated) = cases[*at];
            floats(skill, roll, dc, success, sprung) != floated
        })
        .collect();
    check(wrong.is_empty(), format!("cases wrong: {wrong:?}"))
}

/// Where the player lays mines: clear of the probe's traps.
const LAY_AT: [f32; 2] = [-8.0, 0.0];

/// Gives the party `count` minor frag kits.
fn give_kits(world: &mut crate::world::SimWorld, count: u16) {
    let mut state = world.script_state().clone();
    let catalog = world
        .level()
        .scripts()
        .map(|library| library.items().clone());
    if let Some(catalog) = catalog {
        state.make_item(&catalog, "g_i_trapkit004", PARTY, count, None);
    }
    world.set_script_state(state);
}

/// `steps` steps at `at`, the lay key pressed on the first (held one step);
/// the ally at `ally`.
fn lay_stretch(
    world: &mut crate::world::SimWorld,
    from: u64,
    steps: u64,
    at: [f32; 2],
    ally: [f32; 2],
) -> Stretch {
    let mut notices = Vec::new();
    let mut last = None;
    for tick in from..from + steps {
        place(world, at, ally);
        let mut input = TickInput::default();
        if tick == from {
            input.insert(
                Seat(0),
                crate::Command {
                    buttons: crate::buttons::LAY_MINE,
                    ..crate::Command::default()
                },
            );
        }
        last = Some(crate::step(world, tick, &input, 8, StepReason::Authority));
        notices.extend(world.take_notices());
    }
    Stretch {
        notices,
        last: last.unwrap_or_else(|| world.snapshot()),
    }
}

/// The laid mines in `snapshot`: their numbers.
fn laid(world: &crate::world::SimWorld, snapshot: &crate::Snapshot) -> Vec<u32> {
    let instances = crate::Instances::in_snapshot(world.level(), snapshot);
    snapshot
        .entities
        .iter()
        .filter_map(|entity| match entity.kind {
            crate::Kind::Object(ObjectKind::Trigger, index) => Some(index),
            _ => None,
        })
        .filter(|index| {
            instances
                .made_from(*index)
                .is_some_and(|made| crate::traps::laid_row(&made.template).is_some())
        })
        .collect()
}

/// Laying mines (S9).
pub(crate) fn laying() -> Vec<(&'static str, Result<String, String>)> {
    use crate::world::locks::lines;
    let lay_ms = crate::traps::LAY_MS / 8 + 5;
    let mut checks = Vec::new();
    let mut run = |name: &'static str, test: &dyn Fn() -> Result<(bool, String), String>| {
        checks.push((
            name,
            match test() {
                Ok((ok, what)) => check(ok, what),
                Err(why) => Err(why),
            },
        ));
    };
    // Demolitions 3 out of combat takes 20: 23 against 15; the mine at the
    // player's feet, seen by the party, its DCs 20 + 23, the kit used.
    run("lay_mine", &|| {
        let mut world = world_for("probe_sprung", skilled(0, 3), 1)?;
        give_kits(&mut world, 1);
        let done = lay_stretch(&mut world, 1, lay_ms, LAY_AT, ALLY_AT);
        let mines = laid(&world, &done.last);
        let bits = mines
            .first()
            .and_then(|index| trap_bits(&done.last, *index));
        let kept = world
            .script_state()
            .traps
            .values()
            .find_map(|kept| kept.laid.zip(kept.creator));
        let kit_left = world
            .script_state()
            .items
            .values()
            .any(|item| item.template == "g_i_trapkit004");
        let line = checked(&done.notices, lines::SET_MINE);
        Ok((
            mines.len() == 1
                && bits.is_some_and(|bits| bits & trap::PARTY_SEES != 0)
                && kept.is_some_and(|(laid, _)| laid.detect_dc == 43 && laid.disarm_dc == 43)
                && !kit_left
                && line == Some((None, true)),
            format!("{mines:?} bits {bits:?} kept {kept:?} kit left {kit_left} {line:?}"),
        ))
    });
    // A hostile walking onto it springs it; so after a snapshot and the
    // scripts' state are restored (a save's load).
    run("laid_springs_on_hostile", &|| {
        let mut world = world_for("probe_sprung", skilled(0, 3), 1)?;
        give_kits(&mut world, 1);
        lay_stretch(&mut world, 1, lay_ms, LAY_AT, ALLY_AT);
        let walked = stretch_with(&mut world, 1 + lay_ms, 10, AWAY, LAY_AT);
        Ok((
            said(&walked.notices, "sprung"),
            format!("{:?}", walked.notices),
        ))
    });
    run("laid_survives_snapshot", &|| {
        let mut world = world_for("probe_sprung", skilled(0, 3), 1)?;
        give_kits(&mut world, 1);
        let done = lay_stretch(&mut world, 1, lay_ms, LAY_AT, ALLY_AT);
        let state = world.script_state().clone();
        let mut again = crate::world::SimWorld::from_snapshot(&done.last, world.level().clone())
            .map_err(|error| error.to_string())?;
        again.set_script_state(state);
        let walked = stretch_with(&mut again, 1 + lay_ms, 10, AWAY, LAY_AT);
        Ok((
            said(&walked.notices, "sprung"),
            format!("{:?}", walked.notices),
        ))
    });
    // Fifteen laid, the sixteenth refused.
    run("lay_cap", &|| {
        let mut world = world_for("probe_sprung", skilled(0, 3), 1)?;
        give_kits(&mut world, 16);
        let mut tick = 1;
        for _ in 0..crate::traps::LAID_MOST {
            lay_stretch(&mut world, tick, lay_ms, LAY_AT, ALLY_AT);
            tick += lay_ms;
        }
        let refused_now = lay_stretch(&mut world, tick, lay_ms, LAY_AT, ALLY_AT);
        let mines = laid(&world, &refused_now.last).len();
        Ok((
            mines == crate::traps::LAID_MOST && refused(&refused_now.notices, 0),
            format!("{mines} laid"),
        ))
    });
    // A row set against 40: Demolitions 3 takes 20 to 23, short by 17 out
    // of combat: nothing laid, the kit kept (S9).
    run("lay_failed_keeps_kit", &|| {
        let mut world = world_set("probe_sprung", skilled(0, 3), 1, 40)?;
        give_kits(&mut world, 1);
        let done = lay_stretch(&mut world, 1, lay_ms, LAY_AT, ALLY_AT);
        let kept = world
            .script_state()
            .items
            .values()
            .any(|item| item.template == "g_i_trapkit004");
        let line = checked(&done.notices, lines::SET_MINE);
        Ok((
            laid(&world, &done.last).is_empty() && kept && line == Some((None, false)),
            format!("kit kept {kept}, {line:?}"),
        ))
    });
    // No kit: refused, nothing laid.
    run("lay_no_kit", &|| {
        let mut world = world_for("probe_sprung", skilled(0, 3), 1)?;
        let done = lay_stretch(&mut world, 1, lay_ms, LAY_AT, ALLY_AT);
        Ok((
            refused(&done.notices, 0) && laid(&world, &done.last).is_empty(),
            format!("{:?}", done.notices),
        ))
    });
    checks
}

/// `stretch`, the ally at `ally`.
fn stretch_with(
    world: &mut crate::world::SimWorld,
    from: u64,
    steps: u64,
    at: [f32; 2],
    ally: [f32; 2],
) -> Stretch {
    let mut notices = Vec::new();
    let mut last = None;
    for tick in from..from + steps {
        place(world, at, ally);
        last = Some(crate::step(
            world,
            tick,
            &TickInput::default(),
            8,
            StepReason::Authority,
        ));
        notices.extend(world.take_notices());
    }
    Stretch {
        notices,
        last: last.unwrap_or_else(|| world.snapshot()),
    }
}

/// The trap and stealth XP routines (S15), asked by the door's script as
/// the player opens it.
pub(crate) fn routines() -> Vec<(&'static str, Result<String, String>)> {
    let ran = (|| -> Result<(Vec<String>, i32), String> {
        let mut world = world("probe_routines")?;
        let mut said = Vec::new();
        let mut xp = 0;
        for tick in 1..=10 {
            place(&mut world, BY_GATE, ALLY_AT);
            let mut input = TickInput::default();
            if tick == 1 {
                input.act(
                    Seat(0),
                    Action::Use {
                        object: GATE,
                        verb: Verb::Open,
                    },
                );
            }
            crate::step(&mut world, tick, &input, 8, StepReason::Authority);
            for (_, notice) in world.take_notices() {
                match notice {
                    Notice::Said(words) => said.push(words),
                    Notice::Experience(gained) => xp += gained,
                    _ => {}
                }
            }
        }
        Ok((said, xp))
    })();
    let expected = [
        "1", "3", "20", "20", "1", "0", "1", "1", "mine", "0", "100", "0", "0",
    ];
    vec![(
        "trap_and_stealth_routines",
        check(
            ran.as_ref()
                .is_ok_and(|(said, xp)| said == &expected && *xp == 100),
            format!("{ran:?}"),
        ),
    )]
}

/// The words the player was told, in order.
fn words(notices: &[(Seat, Notice)]) -> Vec<String> {
    notices
        .iter()
        .filter_map(|(_, notice)| match notice {
            Notice::Said(words) => Some(words.clone()),
            _ => None,
        })
        .collect()
}

/// Several things happening to one trap in one step, and the trap's own
/// faction as its script sees it.
pub(crate) fn races() -> Vec<(&'static str, Result<String, String>)> {
    let mut checks = Vec::new();
    let mut run = |name: &'static str, test: &dyn Fn() -> Result<(bool, String), String>| {
        checks.push((
            name,
            match test() {
                Ok((ok, what)) => check(ok, what),
                Err(why) => Err(why),
            },
        ));
    };
    // Two players coming into a one-shot mine in one step: it springs once.
    run("one_spring_per_step", &|| {
        let mut world = world_for("probe_sprung", Sheet::dev_soldier(), 2)?;
        let ran = stretch(&mut world, 1, 10, MINE_AT, &[]);
        let sprung = words(&ran.notices)
            .iter()
            .filter(|words| *words == "sprung")
            .count();
        Ok((sprung == 1, format!("sprung {sprung} times")))
    });
    // A recover done in the very step another player walks on: the
    // recover's (the kit, no spring), not both.
    run("recover_beats_walker", &|| {
        let mut world = world_for("probe_sprung", skilled(0, 12), 2)?;
        mark(&mut world, MINE, trap::PARTY_SEES);
        let mut notices = Vec::new();
        for tick in 1..=crate::traps::DISARM_MS / 8 + 20 {
            let due = world
                .script_state()
                .skill_uses
                .get(&0)
                .is_some_and(|used| used.until <= tick * 8);
            place_seats(&mut world, [BY_MINE, if due { MINE_AT } else { AWAY }]);
            let mut input = TickInput::default();
            if tick == 1 {
                input.act(Seat(0), Action::Recover(MINE));
            }
            crate::step(&mut world, tick, &input, 8, StepReason::Authority);
            notices.extend(world.take_notices());
        }
        let kit = world
            .script_state()
            .items
            .values()
            .any(|item| item.holder == PARTY && item.template == "g_i_trapkit004");
        let sprung = words(&notices).contains(&"sprung".to_string());
        Ok((kit && !sprung, format!("kit {kit}, sprung {sprung}")))
    });
    // The mine's script asks how the trap regards the player: its faction
    // is at war with the player's.
    run("regard_trap_faction", &|| {
        let mut world = world("probe_regard")?;
        let ran = stretch(&mut world, 1, 10, MINE_AT, &[]);
        let told = words(&ran.notices);
        Ok((told == ["1", "0", "0"], format!("{told:?}")))
    });
    // A mine the player laid is the player's faction's as its script runs
    // (sprung by a hostile): the player its friend.
    run("regard_laid_mine", &|| {
        let mut world = world_for("probe_regard", skilled(0, 3), 1)?;
        give_kits(&mut world, 1);
        let lay_ms = crate::traps::LAY_MS / 8 + 5;
        lay_stretch(&mut world, 1, lay_ms, LAY_AT, ALLY_AT);
        let walked = stretch_with(&mut world, 1 + lay_ms, 10, AWAY, LAY_AT);
        let told = words(&walked.notices);
        Ok((told == ["0", "1", "0"], format!("{told:?}")))
    });
    checks
}

/// Where a mine as K1 places them (an octagon of `traps::RADIUS_M`, the
/// hostile faction's) stands on the proving ground for the walk probes,
/// and its number (after the ground's two objects).
const WALKED_MINE_AT: [f32; 2] = [-3.5, -2.0];
const WALKED_MINE: u32 = 2;

/// The walks to disarm the mine (owner's playtest, 2026-10-30: "interacting
/// with mines is fiddly and sometimes leads to you detonating it"): where
/// the player starts, how many steps it runs forward first.
const MINE_WALKS: [(&str, [f32; 2], usize); 5] = [
    ("straight", [-3.5, 4.0], 0),
    ("angled", [-7.0, 3.5], 0),
    ("from_the_side", [-7.0, -2.0], 0),
    ("already_running", [-7.0, 1.0], 40),
    ("beside_it", [-3.5, 0.9], 0),
];

/// A walk to disarm the mine: the nearest the player came to its middle
/// (metres), whether it went off, and how the check went (`checked`).
struct MineWalk {
    nearest: f32,
    sprung: bool,
    check: Option<(Option<u8>, bool)>,
}

/// One choice of Disarm on the mine, as the target block does it (walked
/// to where out of reach, asked on arriving).
fn mine_walk(stand: [f32; 2], run_in: usize) -> Result<MineWalk, String> {
    use crate::world::locks::lines;
    use crate::world::uses::probes::{Choosing, walk_and_choose, walk_world};
    let ground = crate::determinism::proving_ground();
    let mut data = (**ground.module().ok_or("the ground has no module")?).clone();
    let octagon = (0..8)
        .map(|at| {
            let (sin, cos) = crate::math::sin_cos_deg(45.0 * at as f32);
            [
                crate::traps::RADIUS_M * cos,
                crate::traps::RADIUS_M * sin,
                0.0,
            ]
        })
        .collect();
    data.placements.push(Placement::Own(ObjectData {
        geometry: octagon,
        ..mine("mine", WALKED_MINE_AT, "", &[])
    }));
    data.traps = rows("", 15);
    let war = [usize::from(PLAYER), usize::from(HOSTILE_1)];
    data.repute = Repute {
        rows: (0..7)
            .map(|a| {
                (0..7)
                    .map(|b| match (a, b) {
                        _ if a == b => 100,
                        _ if war.contains(&a) && war.contains(&b) => 0,
                        _ => 50,
                    })
                    .collect()
            })
            .collect(),
    };
    let (kept, mut world) = walk_world(data, skilled(0, 4))?;
    mark(&mut world, WALKED_MINE, trap::PARTY_SEES);
    let how = Choosing {
        stand: Some(stand),
        run_in,
        object: WALKED_MINE,
        kind: ObjectKind::Trigger,
        choice: crate::target::Choice::Disarm,
        cancel_at: None,
    };
    let middle = floor_at(WALKED_MINE_AT);
    let (mut nearest, mut sprung, mut check) = (f32::MAX, false, None);
    walk_and_choose(&kept, &mut world, &how, |_, last, notices| {
        if let Some(me) = last
            .entities
            .iter()
            .find(|entity| entity.kind == crate::Kind::Avatar(Seat(0)))
        {
            nearest = nearest.min(crate::math::flat_distance(me.position, middle));
        }
        let told: Vec<(Seat, Notice)> = notices.to_vec();
        sprung |= told
            .iter()
            .any(|(_, n)| *n == Notice::Line(lines::MINE_TRIGGERED));
        check = check.or(checked(&told, lines::DISARM_MINE));
        sprung || check.is_some()
    })?;
    Ok(MineWalk {
        nearest,
        sprung,
        check,
    })
}

/// The walks to disarm a K1 mine from every side and at a run: each stops
/// outside its circle, never setting it off, and disarms it. For the
/// approved test `mine_walk` (awaiting the owner).
pub fn walk_probes() -> Vec<(String, Result<String, String>)> {
    MINE_WALKS
        .iter()
        .map(|&(name, stand, run_in)| {
            let ran = mine_walk(stand, run_in).and_then(
                |MineWalk {
                     nearest,
                     sprung,
                     check,
                 }| {
                    let what = format!("nearest {nearest:.2} m, sprung {sprung}, check {check:?}");
                    let clear = nearest > crate::traps::RADIUS_M;
                    if clear && !sprung && check == Some((None, true)) {
                        Ok(what)
                    } else {
                        Err(what)
                    }
                },
            );
            (format!("mine_walk_{name}"), ran)
        })
        .collect()
}
