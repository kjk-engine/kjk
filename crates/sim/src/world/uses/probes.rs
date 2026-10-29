//! The probes of `uses` (`kjk sim probes`).

use crate::action::{Action, Verb};
use crate::input::Seat;
use crate::module::{ObjectKind, Opens};

/// A point of the proving ground's floor on its own axes
/// (`determinism::ground_point`).
fn floor_at([x, y]: [f32; 2]) -> [f32; 3] {
    crate::determinism::ground_point(x, y, 0.0)
}

/// The probe's scripts, each telling its subject's user something
/// (`SendMessageToPC(<getter>(), words)`), by catalog routine.
const CATALOG: &str = r#"
// 0
void SendMessageToPC(object oPlayer, string szMessage);
// 1
object GetLastOpenedBy();
// 2
object GetLastClosedBy();
// 3
object GetLastUsedBy();
// 4
object GetLastSpeaker();
// 5
object GetClickingObject();
"#;
const SAYS: [(&str, u16, &str); 8] = [
    ("probe_opened", 1, "opened"),
    ("probe_closed", 2, "closed"),
    ("probe_failed", 1, "failed"),
    ("probe_used", 3, "used"),
    ("probe_hidden", 3, "hidden"),
    ("probe_talk", 4, "talked"),
    ("probe_click", 5, "clicked"),
    ("probe_across", 4, "across"),
];

fn library() -> crate::script::Library {
    use fmt_odyssey::ncs::{Const, Op};
    let says = SAYS.map(|(name, getter, words)| {
        let mut asm = script_ncs::probes::Asm::default();
        asm.op(Op::Const(Const::String(words.as_bytes().to_vec())))
            .op(Op::Action {
                routine: getter,
                args: 0,
            })
            .op(Op::Action {
                routine: 0,
                args: 2,
            })
            .op(Op::Retn);
        (name.to_string(), asm.build())
    });
    crate::probe::library(CATALOG, says)
}

/// The proving ground's door, and the objects added after its trigger.
const GATE: u32 = 0;
const CHEST: u32 = 2;
const SAFE: u32 = 3;
const PANEL: u32 = 4;
const TALKER: u32 = 5;
const HIDDEN: u32 = 6;
/// K1's containers, which have no fail-to-open script: a locked one with
/// a use script (Taris's sealed strongbox), an open one (Dantooine's dead
/// settler, whose `OnUsed` writes the journal).
const STRONGBOX: u32 = 7;
const CASKET: u32 = 8;
/// A creature behind the wall, within a talk's reach of `BY_WALL`: one
/// across a counter (Dantooine's Administrator behind her desk).
const ACROSS: u32 = 9;

/// Where the player stands for a step, on the ground's own axes (metres
/// from the doorway's middle; x through the door, y along the wall).
const FAR: [f32; 2] = [-4.0, 0.0];
const NEAR: [f32; 2] = [-1.2, 0.0];
const IN_DOORWAY: [f32; 2] = [0.0, 0.0];
const BY_WALL: [f32; 2] = [-0.6, 3.0];
const PAST_WALL: [f32; 2] = [1.0, 2.0];
const BY_TALKER: [f32; 2] = [-2.5, 1.0];

/// A step: where seat 0 stands, whether it is down, and its Uses.
type Step = ([f32; 2], bool, &'static [(u32, Verb)]);

const STEPS: [Step; 18] = [
    (FAR, false, &[(GATE, Verb::Open)]),
    (NEAR, false, &[(GATE, Verb::Open)]),
    (NEAR, false, &[(GATE, Verb::Open)]),
    (NEAR, false, &[(GATE, Verb::Close)]),
    (NEAR, false, &[(CHEST, Verb::Open)]),
    (NEAR, false, &[(SAFE, Verb::Open)]),
    (NEAR, false, &[(PANEL, Verb::Use)]),
    (NEAR, false, &[(TALKER, Verb::Talk)]),
    (BY_WALL, false, &[(HIDDEN, Verb::Use)]),
    (PAST_WALL, false, &[(HIDDEN, Verb::Use)]),
    (NEAR, false, &[(GATE, Verb::Open), (99, Verb::Open)]),
    (IN_DOORWAY, false, &[(GATE, Verb::Close)]),
    (
        BY_TALKER,
        false,
        &[(TALKER, Verb::Use), (SAFE, Verb::Use), (PANEL, Verb::Talk)],
    ),
    (NEAR, false, &[(TALKER, Verb::Talk)]),
    (NEAR, false, &[(STRONGBOX, Verb::Open)]),
    (NEAR, false, &[(CASKET, Verb::Open)]),
    (BY_WALL, false, &[(ACROSS, Verb::Talk)]),
    (NEAR, true, &[(PANEL, Verb::Use)]),
];

/// The step before which the talker dies.
const TALKER_DIES: usize = 13;

type Ran = (
    Vec<crate::Snapshot>,
    Vec<(usize, Seat, crate::notice::Notice)>,
    super::super::Level,
    Vec<Vec<super::super::UseHeard>>,
);

fn scenario() -> Result<Ran, String> {
    use crate::module::{ObjectData, Placement};
    use crate::{StepReason, TickInput, WorldConfig};

    let ground = crate::determinism::proving_ground();
    let mut data = (**ground.module().ok_or("the ground has no module")?).clone();
    let Some(Placement::Own(gate)) = data.placements.first_mut() else {
        return Err("the ground's first object is not its door".into());
    };
    gate.scripts = vec![
        ("OnOpen".into(), "probe_opened".into()),
        ("OnClosed".into(), "probe_closed".into()),
        ("OnClick".into(), "probe_click".into()),
    ];
    let thing = |kind, tag: &str, at, hook: &str, script: &str| ObjectData {
        kind,
        tag: tag.into(),
        useable: true,
        scripts: if hook.is_empty() {
            Vec::new()
        } else {
            vec![(hook.into(), script.into())]
        },
        ..ObjectData::unknown(tag, floor_at(at), 0.0)
    };
    let medpac = crate::inventory::Held {
        template: "medpac".into(),
        stack: 1,
        slot: None,
        droppable: true,
        infinite: false,
    };
    data.placements.extend(
        [
            ObjectData {
                container: true,
                inventory: vec![medpac],
                ..thing(ObjectKind::Placeable, "chest", [-2.0, -1.5], "", "")
            },
            ObjectData {
                container: true,
                opens: Opens::Locked,
                scripts: vec![
                    ("OnFailToOpen".into(), "probe_failed".into()),
                    ("OnUsed".into(), "probe_used".into()),
                ],
                ..thing(ObjectKind::Placeable, "safe", [-2.0, 1.5], "", "")
            },
            thing(
                ObjectKind::Placeable,
                "panel",
                [-1.2, -1.0],
                "OnUsed",
                "probe_used",
            ),
            thing(
                ObjectKind::Creature,
                "talker",
                [-3.5, 2.0],
                "ScriptDialogue",
                "probe_talk",
            ),
            thing(
                ObjectKind::Placeable,
                "hidden",
                [1.5, 3.0],
                "OnUsed",
                "probe_hidden",
            ),
            ObjectData {
                container: true,
                opens: Opens::Locked,
                ..thing(
                    ObjectKind::Placeable,
                    "strongbox",
                    [-2.4, 0.6],
                    "OnUsed",
                    "probe_used",
                )
            },
            ObjectData {
                container: true,
                ..thing(
                    ObjectKind::Placeable,
                    "casket",
                    [-2.4, -0.6],
                    "OnUsed",
                    "probe_used",
                )
            },
            thing(
                ObjectKind::Creature,
                "across",
                [1.0, 2.6],
                "ScriptDialogue",
                "probe_across",
            ),
        ]
        .map(Placement::Own),
    );
    let level = ground.with_module(data).with_scripts(library());
    let kept = level.clone();
    let mut world = super::super::SimWorld::new(
        WorldConfig {
            seed: 1,
            seats: 1,
            wanderers: 0,
            spawn: [0.0; 3],
            yaw: 0.0,
            sheet: crate::combat::Sheet::dev_soldier(),
            guests: crate::Guests::Avatars,
        },
        level,
    )
    .map_err(|error| error.to_string())?;
    let mut snapshots = Vec::new();
    let mut notices = Vec::new();
    let mut heard = Vec::new();
    for (step, (stand, down, uses)) in STEPS.iter().enumerate() {
        let mut players = world.world.query::<(
            &super::super::Avatar,
            &mut super::super::Player,
            &mut super::super::Fighting,
        )>();
        for (_, mut player, mut fighting) in players.iter_mut(&mut world.world) {
            let at = floor_at(*stand);
            player.0.origin[0] = at[0] / collision::UNIT;
            player.0.origin[1] = at[1] / collision::UNIT;
            if *down {
                fighting.0.life = crate::combat::Life::Dead;
            }
        }
        if step == TALKER_DIES {
            let mut creatures = world
                .world
                .query::<(&super::super::Object, &mut super::super::Fighting)>();
            for (object, mut fighting) in creatures.iter_mut(&mut world.world) {
                if object.index == TALKER {
                    fighting.0.life = crate::combat::Life::Dead;
                }
            }
        }
        let mut input = TickInput::default();
        for &(object, verb) in *uses {
            input.act(Seat(0), Action::Use { object, verb });
        }
        snapshots.push(crate::step(
            &mut world,
            step as u64 + 1,
            &input,
            8,
            StepReason::Authority,
        ));
        notices.extend(
            world
                .take_notices()
                .into_iter()
                .map(|(seat, notice)| (step, seat, notice)),
        );
        heard.push(world.uses_heard());
    }
    Ok((snapshots, notices, kept, heard))
}

/// The checks of using (see the module documentation), on the proving
/// ground with a container, a locked one, a panel, a creature that talks
/// and a panel behind the wall added. For the approved test `use_actions`.
pub fn probes() -> Vec<(&'static str, Result<String, String>)> {
    use crate::notice::Notice;
    let check = |ok: bool, what: String| if ok { Ok(what) } else { Err(what) };
    let round_trips = Verb::ALL.iter().all(|&verb| {
        let action = Action::Use { object: 7, verb };
        Action::from_bytes(&action.to_bytes()).as_ref() == Ok(&action)
    });
    let mut checks = vec![(
        "wire_round_trip",
        check(round_trips, format!("{:?}", Verb::ALL)),
    )];
    let (snapshots, notices, level, heard) = match scenario() {
        Ok(ran) => ran,
        Err(why) => {
            checks.push(("use_scenario", Err(why)));
            return checks;
        }
    };
    let again = scenario().map(|(snapshots, ..)| snapshots.last().map(crate::Snapshot::hash));
    let mut targets: Vec<u32> = snapshots.last().map_or(Vec::new(), |last| {
        crate::target::candidates(&level, last)
            .iter()
            .map(|candidate| candidate.index)
            .collect()
    });
    targets.sort();
    // The gate stands open at the end: no pick volume fills its doorway.
    let open_gate = snapshots.last().and_then(|last| {
        crate::target::candidates(&level, last)
            .into_iter()
            .find(|candidate| candidate.index == GATE)
            .map(|candidate| candidate.radius)
    });
    let state = |step: usize, index: u32| {
        snapshots.get(step).and_then(|snapshot| {
            snapshot
                .entities
                .iter()
                .find(|entity| matches!(entity.kind, crate::Kind::Object(_, i) if i == index))
                .and_then(|entity| entity.object)
        })
    };
    let said = |step: usize, words: &str| {
        notices
            .iter()
            .any(|(at, _, notice)| *at == step && *notice == Notice::Said(words.into()))
    };
    let said_ever = |words: &str| {
        notices
            .iter()
            .filter(|(_, _, notice)| *notice == Notice::Said(words.into()))
            .count()
    };
    let gate = |step| state(step, GATE).map(|object| object.state);
    // What the step heard used (`SimWorld::uses_heard`), by object number,
    // each by the player.
    let came = |step: usize| -> Vec<(u32, super::super::UseCame)> {
        let (Some(snapshot), Some(heard)) = (snapshots.get(step), heard.get(step)) else {
            return Vec::new();
        };
        let player = snapshot.own(Seat(0)).map(|own| own.id);
        heard
            .iter()
            .filter(|use_heard| Some(use_heard.by) == player)
            .filter_map(|use_heard| match snapshot.get(use_heard.object)?.kind {
                crate::Kind::Object(_, index) => Some((index, use_heard.came)),
                _ => None,
            })
            .collect()
    };
    let told_locked = |step: usize| {
        let locked = Notice::Line(super::super::locks::lines::LOCKED);
        notices.contains(&(step, Seat(0), locked))
    };
    checks.extend([
        (
            "out_of_reach",
            check(
                gate(0) == Some(0) && !said(0, "opened"),
                format!("gate {:?}", gate(0)),
            ),
        ),
        (
            "opens",
            check(
                gate(1) == Some(1) && said(1, "opened"),
                format!("gate {:?}", gate(1)),
            ),
        ),
        (
            "stale_verb",
            check(said_ever("opened") == 2, format!("{notices:?}")),
        ),
        (
            "click_open_door",
            check(
                said(2, "clicked") && said_ever("clicked") == 1,
                format!("{notices:?}"),
            ),
        ),
        (
            "closes",
            check(
                gate(3) == Some(0) && said(3, "closed"),
                format!("gate {:?}", gate(3)),
            ),
        ),
        (
            "container_opens",
            check(
                state(4, CHEST).is_some_and(|chest| chest.tenants & 1 != 0),
                format!("chest {:?}", state(4, CHEST)),
            ),
        ),
        (
            "locked_fails",
            check(
                said(5, "failed")
                    && told_locked(5)
                    && state(5, SAFE).is_some_and(|safe| safe.state == 0),
                format!("{notices:?}"),
            ),
        ),
        (
            "placeable_used",
            check(said(6, "used"), format!("{notices:?}")),
        ),
        (
            "talk_reach",
            check(said(7, "talked"), format!("{notices:?}")),
        ),
        (
            "wall_hides",
            check(
                !said(8, "hidden") && said(9, "hidden"),
                format!("{notices:?}"),
            ),
        ),
        (
            "unknown_object",
            check(gate(10) == Some(1), format!("gate {:?}", gate(10))),
        ),
        (
            "close_blocked",
            check(
                gate(11) == Some(1) && !said(11, "closed"),
                format!("gate {:?}", gate(11)),
            ),
        ),
        (
            "verb_must_fit",
            check(
                !said(12, "used") && !said(12, "talked") && !said(12, "failed"),
                format!("{notices:?}"),
            ),
        ),
        (
            "no_talking_to_the_dead",
            check(!said(13, "talked"), format!("{notices:?}")),
        ),
        (
            "locked_container_used",
            check(
                said(14, "used")
                    && !told_locked(14)
                    && state(14, STRONGBOX).is_some_and(|strongbox| strongbox.tenants == 0),
                format!("{:?} {notices:?}", state(14, STRONGBOX)),
            ),
        ),
        (
            "container_opens_used",
            check(
                said(15, "used") && state(15, CASKET).is_some_and(|casket| casket.tenants & 1 != 0),
                format!("{:?} {notices:?}", state(15, CASKET)),
            ),
        ),
        (
            "dead_refused",
            check(!said(17, "used"), format!("{notices:?}")),
        ),
        (
            "talk_needs_no_sight",
            check(said(16, "across"), format!("{notices:?}")),
        ),
        ("uses_heard", {
            use super::super::UseCame::{Clicked, Opened, Refused, Used};
            let heard: Vec<_> = [0, 1, 2, 4, 5, 6, 7, 8, 9].map(came).to_vec();
            check(
                heard
                    == [
                        vec![],
                        vec![(GATE, Opened)],
                        vec![(GATE, Clicked)],
                        vec![(CHEST, Opened)],
                        vec![(SAFE, Refused)],
                        vec![(PANEL, Used)],
                        vec![(TALKER, Used)],
                        vec![],
                        vec![(HIDDEN, Used)],
                    ],
                format!("{heard:?}"),
            )
        }),
        (
            "target_open_door_no_volume",
            check(open_gate == Some(0.0), format!("{open_gate:?}")),
        ),
        (
            "target_candidates",
            check(
                targets == [GATE, CHEST, SAFE, PANEL, HIDDEN, STRONGBOX, CASKET, ACROSS],
                format!("{targets:?}"),
            ),
        ),
        (
            "same_every_run",
            check(
                again
                    .as_ref()
                    .is_ok_and(|hash| *hash == snapshots.last().map(crate::Snapshot::hash)),
                format!("{again:?}"),
            ),
        ),
    ]);
    checks.extend(crate::target::probes());
    checks
}

/// The NPC probe's routines (`script_probes`).
const NPC_CATALOG: &str = r#"
// 0
void SendMessageToPC(object oPlayer, string szMessage);
// 1
object GetLastUsedBy();
// 2
string GetTag(object oObject);
// 3
object GetFirstPC();
// 4
object GetObjectByTag(string sTag, int nNth = 0);
// 5
void ActionUseObject(object oPlaceable);
// 6
void ActionInteractObject(object oPlaceable);
// 7
object GetLastOpenedBy();
// 8
void ActionOpenDoor(object oDoor);
// 9
void ActionLockObject(object oTarget);
"#;

/// Spawn scripts that use the object tagged so (by routine), the panel's
/// `OnUsed`, which tells the player its user's tag, the gate's `OnOpen`,
/// which tells its opener's, and heartbeats that open and lock their own
/// door.
fn npc_library() -> crate::script::Library {
    use fmt_odyssey::ncs::{Const, Op};
    let uses = |routine: u16, tag: &str| {
        let mut asm = script_ncs::probes::Asm::default();
        asm.op(Op::Const(Const::Int(0)))
            .op(Op::Const(Const::String(tag.as_bytes().to_vec())))
            .op(Op::Action {
                routine: 4,
                args: 2,
            })
            .op(Op::Action { routine, args: 1 })
            .op(Op::Retn);
        asm.build()
    };
    let tells = |last: u16| {
        let mut asm = script_ncs::probes::Asm::default();
        asm.op(Op::Action {
            routine: last,
            args: 0,
        })
        .op(Op::Action {
            routine: 2,
            args: 1,
        })
        .op(Op::Action {
            routine: 3,
            args: 0,
        })
        .op(Op::Action {
            routine: 0,
            args: 2,
        })
        .op(Op::Retn);
        asm.build()
    };
    let on_itself = |routine: u16| {
        let mut asm = script_ncs::probes::Asm::default();
        asm.op(Op::Const(Const::Object(0)))
            .op(Op::Action { routine, args: 1 })
            .op(Op::Retn);
        asm.build()
    };
    crate::probe::library(
        NPC_CATALOG,
        [
            ("probe_worker", uses(5, "panel")),
            ("probe_doorman", uses(6, "gate")),
            ("probe_used_by", tells(1)),
            ("probe_opened_by", tells(7)),
            ("probe_opens_itself", on_itself(8)),
            ("probe_locks_itself", on_itself(9)),
        ],
    )
}

/// How long the NPC probe runs at most (8 ms steps).
const NPC_STEPS: usize = 500;
/// How long the self-opening gate's run lasts (8 ms steps): past its
/// first heartbeat.
const SELF_OPEN_STEPS: usize = 1000;

/// The NPC probe's run: each step's gate state, what the panel and the
/// gate told the player and at which step, the last snapshot.
type NpcRan = (Vec<u8>, Vec<(usize, String)>, crate::Snapshot);

fn npc_scenario() -> Result<NpcRan, String> {
    ground_scenario(&[("OnOpen", "probe_opened_by")], None, true, NPC_STEPS)
}

/// The proving ground with its gate's `scripts` (and, given, whether its
/// lock may be locked again), the NPCs (`npcs`: the worker and the
/// doorman) or none, run for `steps`.
fn ground_scenario(
    scripts: &[(&str, &str)],
    lockable: Option<bool>,
    npcs: bool,
    steps: usize,
) -> Result<NpcRan, String> {
    use crate::module::{ObjectData, Placement};
    use crate::notice::Notice;
    use crate::{StepReason, TickInput, WorldConfig};

    let ground = crate::determinism::proving_ground();
    let mut data = (**ground.module().ok_or("the ground has no module")?).clone();
    let Some(Placement::Own(gate)) = data.placements.first_mut() else {
        return Err("the ground's first object is not its door".into());
    };
    gate.scripts = scripts
        .iter()
        .map(|(hook, script)| ((*hook).into(), (*script).into()))
        .collect();
    if let Some(lockable) = lockable {
        gate.lock = Some(crate::module::Lock {
            lockable,
            ..gate
                .lock
                .clone()
                .unwrap_or_else(crate::module::Lock::default)
        });
    }
    let thing = |kind, tag: &str, at, hook: &str, script: &str| ObjectData {
        kind,
        tag: tag.into(),
        useable: true,
        scripts: vec![(hook.into(), script.into())],
        ..ObjectData::unknown(tag, floor_at(at), 0.0)
    };
    let others = [
        thing(
            ObjectKind::Placeable,
            "panel",
            [-4.5, 3.0],
            "OnUsed",
            "probe_used_by",
        ),
        thing(
            ObjectKind::Creature,
            "worker",
            [-2.0, 4.5],
            "ScriptSpawn",
            "probe_worker",
        ),
        // Out of the door's reach. Its box is square to the world, not
        // the turned ground, so at v = -3.0 it would stand in the ramp.
        thing(
            ObjectKind::Creature,
            "doorman",
            [-3.5, -2.0],
            "ScriptSpawn",
            "probe_doorman",
        ),
    ];
    if npcs {
        data.placements.extend(others.map(Placement::Own));
    }
    // The floor as a navigation mesh, for the creatures' walks (the ground
    // has none of its own).
    let floor = [[-7.5, -5.5], [7.5, -5.5], [7.5, 5.5], [-7.5, 5.5]].map(floor_at);
    let nav = collision::nav::NavMesh::build(
        &[
            [floor[0], floor[1], floor[2]],
            [floor[0], floor[2], floor[3]],
        ],
        crate::walk::CLEARANCE * collision::UNIT,
    );
    let level = ground
        .with_module(data)
        .with_scripts(npc_library())
        .with_nav(nav);
    let mut world = super::super::SimWorld::new(
        WorldConfig {
            seed: 1,
            seats: 1,
            wanderers: 0,
            spawn: [0.0; 3],
            yaw: 0.0,
            sheet: crate::combat::Sheet::dev_soldier(),
            guests: crate::Guests::Avatars,
        },
        level,
    )
    .map_err(|error| error.to_string())?;
    let mut gates = Vec::new();
    let mut told = Vec::new();
    let mut last = None;
    for step in 0..steps {
        let snapshot = crate::step(
            &mut world,
            step as u64 + 1,
            &TickInput::default(),
            8,
            StepReason::Authority,
        );
        gates.push(
            snapshot
                .entities
                .iter()
                .find(|entity| matches!(entity.kind, crate::Kind::Object(_, GATE)))
                .and_then(|entity| entity.object)
                .map_or(u8::MAX, |object| object.state),
        );
        for (_, notice) in world.take_notices() {
            if let Notice::Said(words) = notice {
                told.push((step, words));
            }
        }
        last = Some(snapshot);
    }
    Ok((gates, told, last.ok_or("no steps")?))
}

/// The made-placeable probe's catalog.
const MAKE_CATALOG: &str = r#"
int OBJECT_TYPE_CREATURE = 1;
int OBJECT_TYPE_PLACEABLE = 64;
// 0
vector Vector(float x=0.0f, float y=0.0f, float z=0.0f);
// 1
location Location(vector vPosition, float fOrientation);
// 2
object CreateObject(int nObjectType, string sTemplate, location lLocation, int bUseAppearAnimation=FALSE);
"#;

/// How far ahead of the entry the made box's middle stands (metres, along
/// the world's x), and its half width.
const BOX_AHEAD: f32 = 1.5;
const BOX_HALF: f32 = 0.4;

/// The player walks from the entry along the world's x while a creature's
/// spawn script makes a placeable (`CreateObject`) square in its way, a
/// box of the template's walkmesh size: how far the player got (metres
/// from the entry), the last snapshot.
fn made_scenario() -> Result<(f32, crate::Snapshot), String> {
    use crate::module::{ObjectData, Placement};
    use crate::{Command, Seat, StepReason, TickInput, WorldConfig};
    use fmt_odyssey::ncs::{Const, Op};

    let ground = crate::determinism::proving_ground();
    let mut data = (**ground.module().ok_or("the ground has no module")?).clone();
    data.placements.push(Placement::Own(ObjectData {
        kind: ObjectKind::Creature,
        tag: "maker".into(),
        scripts: vec![("ScriptSpawn".into(), "probe_make".into())],
        ..ObjectData::unknown("maker", floor_at([-6.0, 4.5]), 0.0)
    }));
    let entry = floor_at([-4.0, 0.0]);
    let at = [entry[0] + BOX_AHEAD, entry[1], entry[2]];
    let mut make = script_ncs::probes::Asm::default();
    make.op(Op::Const(Const::Int(0)))
        .op(Op::Const(Const::Float(0.0)))
        .op(Op::Const(Const::Float(at[2])))
        .op(Op::Const(Const::Float(at[1])))
        .op(Op::Const(Const::Float(at[0])))
        .op(Op::Action {
            routine: 0,
            args: 3,
        })
        .op(Op::Action {
            routine: 1,
            args: 2,
        })
        .op(Op::Const(Const::String(b"probe_box".to_vec())))
        .op(Op::Const(Const::Int(64)))
        .op(Op::Action {
            routine: 2,
            args: 4,
        })
        .op(Op::Retn);
    let template = ObjectData {
        kind: ObjectKind::Placeable,
        geometry: vec![[-BOX_HALF, -BOX_HALF, 0.0], [BOX_HALF, BOX_HALF, 1.0]],
        ..ObjectData::unknown("probe_box", [0.0; 3], 0.0)
    };
    let library = crate::probe::library(MAKE_CATALOG, [("probe_make", make.build())])
        .with_placeables([("probe_box".to_string(), std::sync::Arc::new(template))].into());
    let level = ground.with_module(data).with_scripts(library);
    let mut world = super::super::SimWorld::new(
        WorldConfig {
            seed: 1,
            seats: 1,
            wanderers: 0,
            spawn: [0.0; 3],
            yaw: 0.0,
            sheet: crate::combat::Sheet::dev_soldier(),
            guests: crate::Guests::Avatars,
        },
        level,
    )
    .map_err(|error| error.to_string())?;
    // Its view turned to the world's x (a command's angles add to its
    // delta angles).
    let delta = world
        .snapshot()
        .entities
        .iter()
        .find_map(|entity| {
            entity
                .player
                .filter(|_| matches!(entity.kind, crate::Kind::Avatar(_)))
        })
        .ok_or("no player")?
        .view_offset;
    let mut input = TickInput::default();
    input.insert(
        Seat(0),
        Command {
            angles: [0, 0i16.wrapping_sub(delta[1]), 0],
            forward: 127,
            ..Command::default()
        },
    );
    let mut last = None;
    for step in 0..MADE_STEPS {
        last = Some(crate::step(
            &mut world,
            step as u64 + 1,
            &input,
            8,
            StepReason::Authority,
        ));
    }
    let last = last.ok_or("no steps")?;
    let player = last
        .entities
        .iter()
        .find(|entity| matches!(entity.kind, crate::Kind::Avatar(_)))
        .ok_or("no player")?;
    Ok((player.position[0] - entry[0], last))
}

/// How long the made-placeable probe walks (8 ms steps): 2 s, past the box
/// were it not there.
const MADE_STEPS: usize = 250;

/// The lasting-visual probe's catalog.
const GLOW_CATALOG: &str = r#"
int DURATION_TYPE_TEMPORARY = 1;
// 0
effect EffectVisualEffect(int nVisualEffectId, int nMissEffect=FALSE);
// 1
void ApplyEffectToObject(int nDurationType, effect eEffect, object oTarget, float fDuration=0.0f);
// 2
void ClearAllEffects();
"#;

/// The row the lasting-visual probe puts on (an energy shield's).
const GLOW_ROW: u16 = 2040;

/// A creature's spawn script puts a visual on itself for a minute, its
/// heartbeat clears its effects: the visuals' rows running after the spawn
/// and after the first heartbeat (`Effects::visuals`).
fn kept_visual_scenario() -> Result<(Vec<u16>, Vec<u16>), String> {
    use crate::module::{ObjectData, Placement};
    use crate::{StepReason, TickInput, WorldConfig};
    use fmt_odyssey::ncs::{Const, Op};

    let ground = crate::determinism::proving_ground();
    let mut data = (**ground.module().ok_or("the ground has no module")?).clone();
    data.placements.push(Placement::Own(ObjectData {
        kind: ObjectKind::Creature,
        tag: "glower".into(),
        scripts: vec![
            ("ScriptSpawn".into(), "probe_glow".into()),
            ("ScriptHeartbeat".into(), "probe_clear".into()),
        ],
        ..ObjectData::unknown("glower", floor_at([-6.0, 4.5]), 0.0)
    }));
    let mut glow = script_ncs::probes::Asm::default();
    glow.op(Op::Const(Const::Float(60.0)))
        .op(Op::Const(Const::Object(0)))
        .op(Op::Const(Const::Int(0)))
        .op(Op::Const(Const::Int(i32::from(GLOW_ROW))))
        .op(Op::Action {
            routine: 0,
            args: 2,
        })
        .op(Op::Const(Const::Int(1)))
        .op(Op::Action {
            routine: 1,
            args: 4,
        })
        .op(Op::Retn);
    let mut clear = script_ncs::probes::Asm::default();
    clear
        .op(Op::Action {
            routine: 2,
            args: 0,
        })
        .op(Op::Retn);
    let library = crate::probe::library(
        GLOW_CATALOG,
        [("probe_glow", glow.build()), ("probe_clear", clear.build())],
    );
    let level = ground.with_module(data).with_scripts(library);
    let mut world = super::super::SimWorld::new(WorldConfig::one_seat(), level)
        .map_err(|error| error.to_string())?;
    let input = TickInput::default();
    let mut rows = |steps: std::ops::Range<u64>| -> Result<Vec<u16>, String> {
        let mut last = None;
        for step in steps {
            last = Some(crate::step(
                &mut world,
                step,
                &input,
                8,
                StepReason::Authority,
            ));
        }
        let last = last.ok_or("no steps")?;
        Ok(last
            .entities
            .iter()
            .filter_map(|entity| entity.fighter.as_ref())
            .flat_map(|fighter| fighter.effects.visuals(last.time_ms).map(|(row, ..)| row))
            .collect())
    };
    let spawned = rows(1..20)?;
    // Past the first heartbeat (every 6 s, at its own moment).
    let cleared = rows(20..900)?;
    Ok((spawned, cleared))
}

/// The held-use probe's phases (8 ms steps): use held 320 ms (three of
/// JA's 100 ms repeats), let go, held again, let go, then held with a click
/// on the same panel in its first step.
const HELD: [(usize, bool, bool); 5] = [
    (40, true, false),
    (5, false, false),
    (5, true, false),
    (5, false, false),
    (5, true, true),
];

/// The player, facing a panel a metre off, presses use by `HELD`'s
/// phases: how many times the panel's `OnUsed` ran by the end of each.
fn held_scenario() -> Result<Vec<usize>, String> {
    use crate::module::{ObjectData, Placement};
    use crate::notice::Notice;
    use crate::{Command, StepReason, TickInput, WorldConfig};

    const PANEL_AT: u32 = 2;
    let ground = crate::determinism::proving_ground();
    let mut data = (**ground.module().ok_or("the ground has no module")?).clone();
    let panel = floor_at([-2.2, 0.0]);
    data.placements.push(Placement::Own(ObjectData {
        kind: ObjectKind::Placeable,
        tag: "panel".into(),
        useable: true,
        scripts: vec![("OnUsed".into(), "probe_used".into())],
        ..ObjectData::unknown("panel", panel, 0.0)
    }));
    let level = ground.with_module(data).with_scripts(library());
    let mut world = super::super::SimWorld::new(WorldConfig::one_seat(), level)
        .map_err(|error| error.to_string())?;
    let at = floor_at(NEAR);
    let mut players = world
        .world
        .query::<(&super::super::Avatar, &mut super::super::Player)>();
    for (_, mut player) in players.iter_mut(&mut world.world) {
        player.0.origin[0] = at[0] / collision::UNIT;
        player.0.origin[1] = at[1] / collision::UNIT;
        player.0.view_offset = [0; 3];
    }
    let eye = [at[0], at[1], at[2] + 1.6];
    let (pitch, yaw) = crate::combat::mind::aim(eye, [panel[0], panel[1], panel[2] + 0.5]);
    let (mut step, mut used, mut counts) = (0u64, 0, Vec::new());
    for (steps, held, click) in HELD {
        for nth in 0..steps {
            let mut input = TickInput::default();
            input.insert(
                Seat(0),
                Command {
                    angles: crate::moves::view_angles(pitch, yaw),
                    buttons: if held { crate::buttons::USE } else { 0 },
                    ..Command::default()
                },
            );
            if click && nth == 0 {
                input.act(
                    Seat(0),
                    Action::Use {
                        object: PANEL_AT,
                        verb: Verb::Use,
                    },
                );
            }
            step += 1;
            crate::step(&mut world, step, &input, 8, StepReason::Authority);
            used += world
                .take_notices()
                .iter()
                .filter(|(_, notice)| *notice == Notice::Said("used".into()))
                .count();
        }
        counts.push(used);
    }
    Ok(counts)
}

/// Where the key-use probe looks from `NEAR`: the door's middle, the panel
/// beside it (inside the use's cone past the door), and away along the wall.
#[derive(Clone, Copy)]
enum Aim {
    Door,
    Panel,
    Away,
}

/// The key-use probe's panel, beside the ground's door (`GATE`).
const BESIDE_DOOR: [f32; 2] = [-0.6, 0.6];

/// The player at `NEAR`, a panel with a use script beside the door, looks
/// by `aim`: what `SimWorld::key_use` says its use key would act on, then,
/// pressed once, whether the door opened and how often the panel's
/// `OnUsed` ran.
fn key_use_scenario(aim: Aim) -> Result<(Option<super::super::KeyUse>, bool, usize), String> {
    use crate::module::{ObjectData, Placement};
    use crate::notice::Notice;
    use crate::{Command, StepReason, TickInput, WorldConfig};

    let ground = crate::determinism::proving_ground();
    let mut data = (**ground.module().ok_or("the ground has no module")?).clone();
    let panel = floor_at(BESIDE_DOOR);
    data.placements.push(Placement::Own(ObjectData {
        kind: ObjectKind::Placeable,
        tag: "panel".into(),
        useable: true,
        scripts: vec![("OnUsed".into(), "probe_used".into())],
        ..ObjectData::unknown("panel", panel, 0.0)
    }));
    let level = ground.with_module(data).with_scripts(library());
    let mut world = super::super::SimWorld::new(WorldConfig::one_seat(), level)
        .map_err(|error| error.to_string())?;
    let at = floor_at(NEAR);
    let mut players = world
        .world
        .query::<(&super::super::Avatar, &mut super::super::Player)>();
    for (_, mut player) in players.iter_mut(&mut world.world) {
        player.0.origin[0] = at[0] / collision::UNIT;
        player.0.origin[1] = at[1] / collision::UNIT;
        player.0.view_offset = [0; 3];
    }
    let eye = [at[0], at[1], at[2] + 1.6];
    let looked = match aim {
        Aim::Door => {
            let door = floor_at([0.0, 0.0]);
            [door[0], door[1], door[2] + 1.0]
        }
        Aim::Panel => [panel[0], panel[1], panel[2] + 0.5],
        Aim::Away => {
            let along = floor_at([NEAR[0], -3.0]);
            [along[0], along[1], eye[2]]
        }
    };
    let (pitch, yaw) = crate::combat::mind::aim(eye, looked);
    let angles = crate::moves::view_angles(pitch, yaw);
    let said = world.key_use(Seat(0), angles);
    // One step's press, then let go while its scripts run.
    let (mut last, mut used) = (None, 0);
    for step in 1..=5 {
        let mut input = TickInput::default();
        input.insert(
            Seat(0),
            Command {
                angles,
                buttons: if step == 1 { crate::buttons::USE } else { 0 },
                ..Command::default()
            },
        );
        last = Some(crate::step(
            &mut world,
            step,
            &input,
            8,
            StepReason::Authority,
        ));
        used += world
            .take_notices()
            .iter()
            .filter(|(_, notice)| *notice == Notice::Said("used".into()))
            .count();
    }
    let opened = last
        .ok_or("no steps")?
        .entities
        .iter()
        .find(|entity| entity.kind == crate::Kind::Object(ObjectKind::Door, GATE))
        .and_then(|entity| entity.object)
        .is_some_and(|object| object.state != 0);
    Ok((said, opened, used))
}

/// The power-check probe's routines.
const SPELL_CATALOG: &str = r#"
// 0
void SendMessageToPC(object oPlayer, string szMessage);
// 1
object GetFirstPC();
// 2
int GetHasSpell(int nSpell, object oCreature=OBJECT_SELF);
// 3
string IntToString(int nInteger);
"#;

/// The power-check probe's power (a `spells.2da` row) and its cost.
const SPELL_ROW: u16 = 5;
const SPELL_COST: u16 = 10;

/// A creature that knows `SPELL_ROW` if `knows`, with `fp` FP, asks
/// `GetHasSpell` of it in its spawn script: what it said ("1" or "0").
fn has_spell_scenario(knows: bool, fp: u16) -> Result<Vec<String>, String> {
    use crate::combat::Sheet;
    use crate::module::{ObjectData, Placement};
    use crate::notice::Notice;
    use crate::{StepReason, TickInput, WorldConfig};
    use fmt_odyssey::ncs::{Const, Op};

    let ground = crate::determinism::proving_ground();
    let mut data = (**ground.module().ok_or("the ground has no module")?).clone();
    data.force_costs = vec![0; usize::from(SPELL_ROW) + 1];
    data.force_costs[usize::from(SPELL_ROW)] = SPELL_COST;
    let mut sheet = Sheet::dev_soldier();
    sheet.max_fp = fp;
    if knows {
        sheet.powers.insert(SPELL_ROW);
    }
    data.placements.push(Placement::Own(ObjectData {
        kind: ObjectKind::Creature,
        tag: "caster".into(),
        sheet: Some(sheet),
        scripts: vec![("ScriptSpawn".into(), "probe_has_spell".into())],
        ..ObjectData::unknown("caster", floor_at([-3.0, 3.0]), 0.0)
    }));
    let mut asks = script_ncs::probes::Asm::default();
    asks.op(Op::Const(Const::Object(0)))
        .op(Op::Const(Const::Int(i32::from(SPELL_ROW))))
        .op(Op::Action {
            routine: 2,
            args: 2,
        })
        .op(Op::Action {
            routine: 3,
            args: 1,
        })
        .op(Op::Action {
            routine: 1,
            args: 0,
        })
        .op(Op::Action {
            routine: 0,
            args: 2,
        })
        .op(Op::Retn);
    let library = crate::probe::library(SPELL_CATALOG, [("probe_has_spell", asks.build())]);
    let level = ground.with_module(data).with_scripts(library);
    let mut world = super::super::SimWorld::new(WorldConfig::one_seat(), level)
        .map_err(|error| error.to_string())?;
    let mut said = Vec::new();
    for step in 1..=20 {
        crate::step(
            &mut world,
            step,
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
    Ok(said)
}

/// The hearing probe's routines.
const HEARING_CATALOG: &str = r#"
// 0
void SendMessageToPC(object oPlayer, string szMessage);
// 1
object GetFirstPC();
// 2
void SpeakString(string sStringToSpeak, int nTalkVolume = TALKVOLUME_TALK);
// 3
void SetListening(object oObject, int bValue);
// 4
void SetListenPattern(object oObject, string sPattern, int nNumber = 0);
// 5
string GetTag(object oObject);
"#;

/// The hearing probe's listeners' hearing (metres, `ranges.2da`'s default
/// `secondaryrange`) and where they stand from the shouter: the far one off
/// the ground, past the 20 m the area's loader looks for a floor for it
/// (`install::settle`), so it stays there.
const HEARING_M: f32 = 20.0;
const LISTENERS: [(&str, f32); 2] = [("near", 5.0), ("far", 45.0)];

/// A creature silent-shouts (`TALKVOLUME_SILENT_SHOUT`) on its heartbeat;
/// two listening for it, of 20 m hearing, stand 5 m and 45 m off: the tags
/// of those who heard it.
fn hearing_scenario() -> Result<Vec<String>, String> {
    use crate::module::{ObjectData, Placement, Senses};
    use crate::notice::Notice;
    use crate::{StepReason, TickInput, WorldConfig};
    use fmt_odyssey::ncs::{Const, Op};

    const PATTERN: &[u8] = b"help";
    let ground = crate::determinism::proving_ground();
    let mut data = (**ground.module().ok_or("the ground has no module")?).clone();
    let creature = |tag: &str, at: [f32; 2], scripts: Vec<(String, String)>| ObjectData {
        kind: ObjectKind::Creature,
        tag: tag.into(),
        senses: Some(Senses {
            sight: HEARING_M,
            hearing: HEARING_M,
        }),
        scripts,
        ..ObjectData::unknown(tag, floor_at(at), 0.0)
    };
    data.placements.push(Placement::Own(creature(
        "shouter",
        [-6.0, -4.0],
        vec![("ScriptHeartbeat".into(), "probe_shout".into())],
    )));
    for (tag, off) in LISTENERS {
        data.placements.push(Placement::Own(creature(
            tag,
            [-6.0 + off, -4.0],
            vec![
                ("ScriptSpawn".into(), "probe_listen".into()),
                ("ScriptDialogue".into(), "probe_heard".into()),
            ],
        )));
    }
    let mut shout = script_ncs::probes::Asm::default();
    shout
        .op(Op::Const(Const::Int(4)))
        .op(Op::Const(Const::String(PATTERN.to_vec())))
        .op(Op::Action {
            routine: 2,
            args: 2,
        })
        .op(Op::Retn);
    let mut listen = script_ncs::probes::Asm::default();
    listen
        .op(Op::Const(Const::Int(1)))
        .op(Op::Const(Const::Object(0)))
        .op(Op::Action {
            routine: 3,
            args: 2,
        })
        .op(Op::Const(Const::Int(1)))
        .op(Op::Const(Const::String(PATTERN.to_vec())))
        .op(Op::Const(Const::Object(0)))
        .op(Op::Action {
            routine: 4,
            args: 3,
        })
        .op(Op::Retn);
    let mut heard = script_ncs::probes::Asm::default();
    heard
        .op(Op::Const(Const::Object(0)))
        .op(Op::Action {
            routine: 5,
            args: 1,
        })
        .op(Op::Action {
            routine: 1,
            args: 0,
        })
        .op(Op::Action {
            routine: 0,
            args: 2,
        })
        .op(Op::Retn);
    let library = crate::probe::library(
        HEARING_CATALOG,
        [
            ("probe_shout", shout.build()),
            ("probe_listen", listen.build()),
            ("probe_heard", heard.build()),
        ],
    );
    let level = ground.with_module(data).with_scripts(library);
    let mut world = super::super::SimWorld::new(WorldConfig::one_seat(), level)
        .map_err(|error| error.to_string())?;
    let mut tags = Vec::new();
    // Past the first heartbeat (every 6 s, at its own moment).
    for step in 1..=900 {
        crate::step(
            &mut world,
            step,
            &TickInput::default(),
            8,
            StepReason::Authority,
        );
        tags.extend(
            world
                .take_notices()
                .into_iter()
                .filter_map(|(_, notice)| match notice {
                    Notice::Said(words) => Some(words),
                    _ => None,
                }),
        );
    }
    tags.sort();
    tags.dedup();
    Ok(tags)
}

/// The airborne-move probe's routines.
const MOVE_CATALOG: &str = r#"
// 0
object GetObjectByTag(string sTag, int nNth = 0);
// 1
void ActionMoveToObject(object oMoveTo, int bRun=FALSE, float fRange=1.0f);
"#;

/// How long the airborne-move probe runs (8 ms steps): 4 s.
const AIRBORNE_STEPS: usize = 500;

/// A creature whose spawn script runs it to a post 3 m off
/// (`ActionMoveToObject`) starts 4 m up in the air (thrown, as a blow
/// throws Malak off the Star Forge's ledge): how far from the post it ends
/// (metres).
fn airborne_scenario() -> Result<f32, String> {
    use crate::module::{ObjectData, Placement};
    use crate::{StepReason, TickInput, WorldConfig};
    use fmt_odyssey::ncs::{Const, Op};

    const MOVER: u32 = 2;
    let ground = crate::determinism::proving_ground();
    let mut data = (**ground.module().ok_or("the ground has no module")?).clone();
    let post = floor_at([-6.0, 3.0]);
    data.placements.extend(
        [
            ObjectData {
                kind: ObjectKind::Creature,
                tag: "mover".into(),
                scripts: vec![("ScriptSpawn".into(), "probe_mover".into())],
                ..ObjectData::unknown("mover", floor_at([-3.0, 3.0]), 0.0)
            },
            ObjectData {
                kind: ObjectKind::Placeable,
                tag: "post".into(),
                ..ObjectData::unknown("post", post, 0.0)
            },
        ]
        .map(Placement::Own),
    );
    let mut mover = script_ncs::probes::Asm::default();
    mover
        .op(Op::Const(Const::Float(1.0)))
        .op(Op::Const(Const::Int(1)))
        .op(Op::Const(Const::Int(0)))
        .op(Op::Const(Const::String(b"post".to_vec())))
        .op(Op::Action {
            routine: 0,
            args: 2,
        })
        .op(Op::Action {
            routine: 1,
            args: 3,
        })
        .op(Op::Retn);
    let floor = [[-7.5, -5.5], [7.5, -5.5], [7.5, 5.5], [-7.5, 5.5]].map(floor_at);
    let nav = collision::nav::NavMesh::build(
        &[
            [floor[0], floor[1], floor[2]],
            [floor[0], floor[2], floor[3]],
        ],
        crate::walk::CLEARANCE * collision::UNIT,
    );
    let library = crate::probe::library(MOVE_CATALOG, [("probe_mover", mover.build())]);
    let level = ground.with_module(data).with_scripts(library).with_nav(nav);
    let mut world = super::super::SimWorld::new(WorldConfig::one_seat(), level)
        .map_err(|error| error.to_string())?;
    let mut bodies = world
        .world
        .query::<(&super::super::Object, &mut super::super::Player)>();
    for (object, mut player) in bodies.iter_mut(&mut world.world) {
        if object.index == MOVER {
            player.0.origin[2] += 4.0 / collision::UNIT;
            player.0.ground = None;
        }
    }
    let mut last = None;
    for step in 0..AIRBORNE_STEPS {
        last = Some(crate::step(
            &mut world,
            step as u64 + 1,
            &TickInput::default(),
            8,
            StepReason::Authority,
        ));
    }
    let last = last.ok_or("no steps")?;
    let at = last
        .entities
        .iter()
        .find(|entity| matches!(entity.kind, crate::Kind::Object(_, MOVER)))
        .ok_or("no mover")?
        .position;
    Ok((at[0] - post[0]).hypot(at[1] - post[1]))
}

/// The NPC checks: a creature's spawn script `ActionUseObject`s a panel
/// out of its reach, it walks there and the panel's `OnUsed` sees it as
/// its user; another's `ActionInteractObject` on the door, shut and out of
/// its reach, walks there and opens it, and the door's `OnOpen` runs once
/// with it as the opener; a placeable a spawn script makes stops the
/// player walking into it; a held use acts once, as KOTOR's click (again
/// only once let go; a click with it the same use); a scripted move given
/// in the air is judged once its mover lands, and taken; two runs end the
/// same. For the approved test
/// `use_scripts`.
pub fn script_probes() -> Vec<(&'static str, Result<String, String>)> {
    let check = |ok: bool, what: String| if ok { Ok(what) } else { Err(what) };
    let (gates, told, last) = match npc_scenario() {
        Ok(ran) => ran,
        Err(why) => return vec![("npc_scenario", Err(why))],
    };
    let made = made_scenario();
    let kept = kept_visual_scenario();
    let held = held_scenario();
    let keyed: Result<Vec<_>, String> = [Aim::Door, Aim::Panel, Aim::Away]
        .into_iter()
        .map(key_use_scenario)
        .collect();
    let airborne = airborne_scenario();
    let has_spell: Result<Vec<_>, String> = [(true, SPELL_COST), (true, 1), (false, SPELL_COST)]
        .into_iter()
        .map(|(knows, fp)| has_spell_scenario(knows, fp))
        .collect();
    let hearing = hearing_scenario();
    let again = npc_scenario().map(|(_, _, last)| last.hash());
    let itself = ground_scenario(
        &[
            ("OnHeartbeat", "probe_opens_itself"),
            ("OnOpen", "probe_opened_by"),
        ],
        None,
        false,
        SELF_OPEN_STEPS,
    );
    let locked: Result<Vec<_>, String> = [false, true]
        .into_iter()
        .map(|lockable| {
            let script = [("OnHeartbeat", "probe_locks_itself")];
            ground_scenario(&script, Some(lockable), false, SELF_OPEN_STEPS).map(|(_, _, last)| {
                last.entities
                    .iter()
                    .find(|entity| matches!(entity.kind, crate::Kind::Object(_, GATE)))
                    .and_then(|entity| entity.object)
                    .map(|object| object.lock == crate::snapshot::lock::LOCKED)
            })
        })
        .collect();
    let opened_at = gates.iter().position(|state| *state == 1);
    let told_by = |tag: &str| -> Vec<usize> {
        told.iter()
            .filter(|(_, words)| words == tag)
            .map(|(step, _)| *step)
            .collect()
    };
    let (used, opened) = (told_by("worker"), told_by("doorman"));
    vec![
        (
            "npc_uses_placeable",
            check(used.len() == 1 && used[0] > 10, format!("told {told:?}")),
        ),
        (
            "npc_opens_door",
            check(
                opened_at.is_some_and(|step| step > 10),
                format!("opened at step {opened_at:?}"),
            ),
        ),
        (
            "npc_door_on_open",
            check(
                opened.len() == 1 && opened_at.is_some_and(|step| opened[0] >= step),
                format!("opened at step {opened_at:?}, told {told:?}"),
            ),
        ),
        (
            "door_opening_itself_runs_on_open",
            match itself {
                Ok((gates, told, _)) => check(
                    gates.contains(&1) && told.iter().filter(|(_, by)| by == "gate").count() == 1,
                    format!("the gate open {}, told {told:?}", gates.contains(&1)),
                ),
                Err(why) => Err(why),
            },
        ),
        (
            "only_lockable_locks",
            match locked {
                Ok(locked) => check(
                    locked == [Some(false), Some(true)],
                    format!("locked after its heartbeat, unlockable and lockable: {locked:?}"),
                ),
                Err(why) => Err(why),
            },
        ),
        (
            "made_placeable_blocks",
            match made {
                Ok((along, _)) => check(
                    along > 0.3 && along < BOX_AHEAD - BOX_HALF,
                    format!(
                        "the player stopped {along:.2} m on, the box from {:.2}",
                        BOX_AHEAD - BOX_HALF
                    ),
                ),
                Err(why) => Err(why),
            },
        ),
        (
            "visual_kept_then_cleared",
            match kept {
                Ok((spawned, cleared)) => check(
                    spawned == [GLOW_ROW] && cleared.is_empty(),
                    format!("after the spawn {spawned:?}, after the heartbeat {cleared:?}"),
                ),
                Err(why) => Err(why),
            },
        ),
        (
            "held_use_once",
            match held {
                Ok(counts) => check(
                    counts == [1, 1, 2, 2, 3],
                    format!("OnUsed ran by each phase's end: {counts:?}"),
                ),
                Err(why) => Err(why),
            },
        ),
        (
            "key_use_foretold",
            match keyed {
                Ok(ran) => {
                    use super::super::KeyUse;
                    let panel_at = GATE + 2;
                    check(
                        ran == [
                            (Some(KeyUse::Object(GATE)), true, 0),
                            (Some(KeyUse::Object(panel_at)), false, 1),
                            (None, false, 0),
                        ],
                        format!(
                            "(said, door opened, panel used) aimed at door, panel, away: {ran:?}"
                        ),
                    )
                }
                Err(why) => Err(why),
            },
        ),
        (
            "has_spell_paid_for",
            match has_spell {
                Ok(said) => check(
                    said == [vec!["1".to_string()], vec!["0".into()], vec!["0".into()]],
                    format!("known and paid for, known with 1 FP, unknown: {said:?}"),
                ),
                Err(why) => Err(why),
            },
        ),
        (
            "shout_heard_within_hearing",
            match hearing {
                Ok(tags) => check(
                    tags == ["near"],
                    format!("heard a silent shout (5 m and 45 m off, 20 m hearing): {tags:?}"),
                ),
                Err(why) => Err(why),
            },
        ),
        (
            "move_waits_to_land",
            match airborne {
                Ok(off) => check(
                    off <= 1.5,
                    format!("the mover ended {off:.2} m from its post"),
                ),
                Err(why) => Err(why),
            },
        ),
        (
            "same_every_run",
            check(
                again.as_ref().is_ok_and(|hash| *hash == last.hash()),
                format!("{again:?}"),
            ),
        ),
    ]
}

/// How long a walk probe runs at most (8 ms steps).
const WALK_STEPS: usize = 1500;

/// The proving ground with `data` for its module, its floor walkable (the
/// walk's routes), the probe's scripts, and one seat playing `sheet`: the
/// level as the target block sees it, the world.
fn walk_world(
    data: crate::module::ModuleData,
    sheet: crate::combat::Sheet,
) -> Result<(super::super::Level, super::super::SimWorld), String> {
    let floor = [[-7.5, -5.5], [7.5, -5.5], [7.5, 5.5], [-7.5, 5.5]].map(floor_at);
    let nav = collision::nav::NavMesh::build(
        &[
            [floor[0], floor[1], floor[2]],
            [floor[0], floor[2], floor[3]],
        ],
        crate::walk::CLEARANCE * collision::UNIT,
    );
    let level = crate::determinism::proving_ground()
        .with_module(data)
        .with_scripts(library())
        .with_nav(nav);
    let kept = level.clone();
    let world = super::super::SimWorld::new(
        crate::WorldConfig {
            seed: 1,
            seats: 1,
            wanderers: 0,
            spawn: [0.0; 3],
            yaw: 0.0,
            sheet,
            guests: crate::Guests::Avatars,
        },
        level,
    )
    .map_err(|error| error.to_string())?;
    Ok((kept, world))
}

/// How seat 0's player chooses in a walk probe, as the target block does
/// (`game::target_view`): from `stand` (the ground's own axes; `None`, the
/// entry), running forward its first `run_in` steps (already moving when
/// it chooses), then `choice` on object `object` of `kind`: asked at once
/// where it stands near enough, else walked to and asked on arriving; with
/// `cancel_at`, its own forward key that step of the walk cancels it.
struct Choosing {
    stand: Option<[f32; 2]>,
    run_in: usize,
    object: u32,
    kind: ObjectKind,
    choice: crate::target::Choice,
    cancel_at: Option<usize>,
}

/// A walk probe's run: the step the choice was asked (none if it was not),
/// whether the player's own command cancelled the walk, the step the run
/// ended on (`WALK_STEPS` if it never did), the last snapshot.
struct Walked {
    asked: Option<usize>,
    cancelled: bool,
    ended: usize,
    last: crate::Snapshot,
}

/// Seat 0's player of `world` choosing (`Choosing`), each step's snapshot
/// and notices given to `done` with whether the choice was asked yet,
/// which ends the run.
fn walk_and_choose(
    kept: &super::super::Level,
    world: &mut super::super::SimWorld,
    how: &Choosing,
    mut done: impl FnMut(bool, &crate::Snapshot, &[(Seat, crate::notice::Notice)]) -> bool,
) -> Result<Walked, String> {
    use crate::target::{Stride, Walk};
    use crate::{StepReason, TickInput};

    if let Some(stand) = how.stand {
        let at = floor_at(stand);
        let mut players = world
            .world
            .query::<(&super::super::Avatar, &mut super::super::Player)>();
        for (_, mut player) in players.iter_mut(&mut world.world) {
            player.0.origin[0] = at[0] / collision::UNIT;
            player.0.origin[1] = at[1] / collision::UNIT;
        }
    }
    let mut last = crate::step(world, 1, &TickInput::default(), 8, StepReason::Authority);
    let mut walk: Option<Walk> = None;
    let (mut asked, mut cancelled) = (None, false);
    for step in 0..WALK_STEPS {
        let now = 8 * step as u64;
        let ps = last
            .entities
            .iter()
            .find(|entity| entity.kind == crate::Kind::Avatar(Seat(0)))
            .and_then(|entity| entity.player)
            .ok_or("no player")?;
        let state = |number: u32| {
            last.entities
                .iter()
                .find(|entity| matches!(entity.kind, crate::Kind::Object(_, i) if i == number))
                .and_then(|entity| entity.object)
                .unwrap_or(crate::ObjectState::default())
        };
        let to = crate::target::candidates(kept, &last)
            .iter()
            .find(|candidate| candidate.index == how.object)
            .map(|candidate| candidate.feet)
            .ok_or("the object is not a target")?;
        let mut input = TickInput::default();
        let ask = |input: &mut TickInput| -> Result<(), String> {
            let action = crate::target::ask(how.choice, how.object, how.kind)
                .ok_or("the choice asks the host nothing")?;
            input.act(Seat(0), action);
            Ok(())
        };
        if step < how.run_in {
            input.insert(
                Seat(0),
                crate::Command {
                    forward: 127,
                    ..crate::Command::default()
                },
            );
        } else if step == how.run_in {
            if Walk::near(&ps, to, how.choice, how.kind) {
                asked = Some(step);
                ask(&mut input)?;
            } else {
                walk = Some(Walk::new(
                    kept, &state, &ps, how.object, how.kind, to, how.choice, now,
                ));
            }
        }
        if let Some(walking) = walk.as_mut().filter(|_| asked.is_none() && !cancelled) {
            match walking.stride(kept, &state, &ps, how.kind, to, now) {
                Stride::Steer(command) => {
                    let mine = crate::Command {
                        forward: if how.cancel_at == Some(step) { 127 } else { 0 },
                        ..crate::Command::default()
                    };
                    let (taken, cancel) =
                        crate::target::steer(Some(command), mine, crate::target::WALK_BREAKS);
                    cancelled = cancel;
                    input.insert(Seat(0), taken);
                }
                Stride::Arrived => {
                    asked = Some(step);
                    ask(&mut input)?;
                }
                Stride::Unreachable => walk = None,
            }
        }
        last = crate::step(world, step as u64 + 2, &input, 8, StepReason::Authority);
        if done(asked.is_some(), &last, &world.take_notices()) {
            return Ok(Walked {
                asked,
                cancelled,
                ended: step,
                last,
            });
        }
    }
    Ok(Walked {
        asked,
        cancelled,
        ended: WALK_STEPS,
        last,
    })
}

/// The player walks from the entry to a panel 4.2 m off and uses it
/// (`target::Walk`); with `cancel_at`, its own forward key that step
/// cancels the walk. Whether the panel's `OnUsed` told the player, and the
/// run.
fn walk_scenario(cancel_at: Option<usize>) -> Result<(bool, Walked), String> {
    use crate::module::{ObjectData, Placement};
    use crate::notice::Notice;

    const PANEL_AT: u32 = 2;
    let ground = crate::determinism::proving_ground();
    let mut data = (**ground.module().ok_or("the ground has no module")?).clone();
    data.placements.push(Placement::Own(ObjectData {
        kind: ObjectKind::Placeable,
        tag: "panel".into(),
        useable: true,
        scripts: vec![("OnUsed".into(), "probe_used".into())],
        ..ObjectData::unknown("panel", floor_at([-1.0, -3.0]), 0.0)
    }));
    let (kept, mut world) = walk_world(data, crate::combat::Sheet::dev_soldier())?;
    let mut told = false;
    let how = Choosing {
        stand: None,
        run_in: 0,
        object: PANEL_AT,
        kind: ObjectKind::Placeable,
        choice: crate::target::Choice::Use,
        cancel_at,
    };
    let walked = walk_and_choose(&kept, &mut world, &how, |_, _, notices| {
        told |= notices
            .iter()
            .any(|(_, notice)| *notice == Notice::Said("used".into()));
        told
    })?;
    Ok((told, walked))
}

/// Security on the proving ground's gate, locked (`OpenLockDC` the
/// player's Security takes out of combat): where the player starts (the
/// ground's own axes), how many steps it runs forward first, and whether
/// the gate has scripts (`OnFailToOpen`, `OnUnlock`, `OnOpen`).
const SECURITY_RUNS: [(&str, [f32; 2], usize, bool); 7] = [
    ("near", [-1.4, 0.4], 0, true),
    ("straight", [-4.0, 0.0], 0, true),
    ("straight_plain", [-4.0, 0.0], 0, false),
    ("angled", [-3.0, 3.5], 0, true),
    ("angled_other_side", [-5.0, -4.0], 0, true),
    ("far", [-7.0, 1.5], 0, true),
    ("already_moving", [-7.0, -0.5], 40, true),
];

/// A Security run: the step it was asked, the step the gate stood open
/// (none if it never did), the farthest the player went from where it was
/// asked (metres), what its player was told.
#[derive(Debug)]
struct Unlocked {
    asked: Option<usize>,
    open: Option<usize>,
    slid: f32,
    said: Vec<crate::notice::Notice>,
}

/// One press of Security on the gate (`SECURITY_RUNS`), as the target block
/// does it: walked to where out of reach, asked on arriving.
fn security_scenario(stand: [f32; 2], run_in: usize, scripted: bool) -> Result<Unlocked, String> {
    use crate::module::{Lock, Placement};
    use crate::notice::Notice;

    let ground = crate::determinism::proving_ground();
    let mut data = (**ground.module().ok_or("the ground has no module")?).clone();
    let Some(Placement::Own(gate)) = data.placements.first_mut() else {
        return Err("the ground's first object is not its door".into());
    };
    gate.opens = Opens::Locked;
    gate.lock = Some(Lock {
        open_dc: rules::interaction::checks::TAKEN,
        ..Lock::default()
    });
    if scripted {
        gate.scripts = vec![
            ("OnFailToOpen".into(), "probe_failed".into()),
            ("OnUnlock".into(), "probe_opened".into()),
            ("OnOpen".into(), "probe_opened".into()),
        ];
    }
    let mut sheet = crate::combat::Sheet::dev_soldier();
    sheet.skills[crate::combat::sheet::skills::SECURITY] = 4;
    let (kept, mut world) = walk_world(data, sheet)?;
    let how = Choosing {
        stand: Some(stand),
        run_in,
        object: GATE,
        kind: ObjectKind::Door,
        choice: crate::target::Choice::Unlock { spike: None },
        cancel_at: None,
    };
    let feet = |snapshot: &crate::Snapshot| {
        snapshot
            .entities
            .iter()
            .find(|entity| entity.kind == crate::Kind::Avatar(Seat(0)))
            .map(|entity| entity.position)
    };
    let (mut from, mut slid, mut said, mut open) = (None, 0.0_f32, Vec::new(), false);
    let walked = walk_and_choose(&kept, &mut world, &how, |asked, last, notices| {
        said.extend(
            notices
                .iter()
                .filter(|(_, notice)| !matches!(notice, Notice::Sound(_)))
                .map(|(_, notice)| notice.clone()),
        );
        if from.is_none() && asked {
            from = feet(last);
        }
        if let (Some(from), Some(now)) = (from, feet(last)) {
            slid = slid.max(crate::math::flat_distance(from, now));
        }
        let gate = last
            .entities
            .iter()
            .find(|entity| matches!(entity.kind, crate::Kind::Object(_, GATE)))
            .and_then(|entity| entity.object);
        open = gate.is_some_and(|gate| gate.state != 0);
        open
    })?;
    Ok(Unlocked {
        asked: walked.asked,
        open: open.then_some(walked.ended),
        slid,
        said,
    })
}

/// The walk's checks: from the entry the player walks to a panel out of
/// reach, steered by `target::Walk` in place of its keys, and uses it on
/// arriving (its `OnUsed` tells it); its own forward key on the way
/// cancels the walk and nothing is used; two runs end the same. For the
/// approved test `use_walk`.
pub fn walk_probes() -> Vec<(&'static str, Result<String, String>)> {
    let check = |ok: bool, what: String| if ok { Ok(what) } else { Err(what) };
    let ((used, walked), (cancel_used, cancelled)) =
        match (walk_scenario(None), walk_scenario(Some(20))) {
            (Ok(walked), Ok(cancelled)) => (walked, cancelled),
            (Err(why), _) | (_, Err(why)) => return vec![("walk_scenario", Err(why))],
        };
    let again = walk_scenario(None).map(|(_, walked)| walked.last.hash());
    vec![
        (
            "walk_arrives_and_uses",
            check(
                walked.asked.is_some_and(|step| step > 20) && used,
                format!("arrived at step {:?}, used {used}", walked.asked),
            ),
        ),
        (
            "walk_cancelled",
            check(
                cancelled.cancelled && cancelled.asked.is_none() && !cancel_used,
                format!(
                    "cancelled {}, arrived {:?}, used {cancel_used}",
                    cancelled.cancelled, cancelled.asked
                ),
            ),
        ),
        (
            "walk_same_every_run",
            check(
                again.as_ref().is_ok_and(|hash| *hash == walked.last.hash()),
                format!("{again:?}"),
            ),
        ),
    ]
}

/// Security chosen once on a locked door (owner's playtest, 2026-10-25:
/// "sometimes needs two clicks"): from near, straight on, at an angle,
/// from afar, already running, with the door's scripts and without, one
/// press picks the lock and opens the door, walked to where out of reach;
/// the player stands within the use's own stay from where it asked (a walk
/// arriving at a run used to slide it past, breaking the use off
/// unheard). For the approved test `security_walk`.
pub fn security_walk_probes() -> Vec<(String, Result<String, String>)> {
    use crate::notice::Notice;
    SECURITY_RUNS
        .iter()
        .map(|&(name, stand, run_in, scripted)| {
            let ran = security_scenario(stand, run_in, scripted).and_then(|ran| {
                let what = format!("{ran:?}");
                let picked = ran
                    .said
                    .iter()
                    .any(|said| matches!(said, Notice::Check { success: true, .. }));
                let stood = ran.slid <= rules::interaction::skill_uses::STAY_M;
                if ran.asked.is_some() && ran.open.is_some() && picked && stood {
                    Ok(what)
                } else {
                    Err(what)
                }
            });
            (format!("security_one_press_{name}"), ran)
        })
        .collect()
}
