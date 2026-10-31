//! The probes of `remains` (`kjk sim probes`).

use super::super::scripts::Scripts;
use super::super::{Fighting, Id, Object, SimWorld};
use super::*;
use crate::action::Action;
use crate::input::Seat;
use crate::module::{Body, Placement};
use crate::probe::check;
use crate::snapshot::{EntityState, Kind, Snapshot, Spawned};
use crate::{StepReason, TickInput};

/// The remains' checks, on the proving ground with two creatures added
/// beside the entry and both killed: one wearing a rifle and carrying a
/// medpac (both droppable), a hide that does not drop and 25 gold, its
/// appearance taking it away 2 s after its death, fading over 0.6 s; one
/// whose death script keeps it (`SetIsDestroyable(FALSE)`). The first's
/// body fades and goes on time, in the world while it fades, a script's
/// `DestroyObject` on it mid-fade changing nothing (its fade goes on to
/// its end), its remains then standing where it lay: a
/// placeable with no tag, named and drawn as its bag's row, holding the
/// rifle, the medpac and the gold; the kept one stays, a `DestroyObject` on
/// it refused, and leaves nothing. A
/// save made then holds the remains and what they hold; the world loaded
/// from it opens them, takes all, closes them, and they go. The player
/// running through where the first lies leaves its body where it fell
/// (`walked_through`). Run twice, the two end in the same state. For the
/// approved test `corpse_remains`.
pub fn probes() -> Vec<(&'static str, Result<String, String>)> {
    let ran = match scenario() {
        Ok(ran) => ran,
        Err(why) => return vec![("remains_scenario", Err(why))],
    };
    let again = scenario().map(|again| again.last.hash());
    let due = ran.killed_ms + 2600;
    let body_until = ran
        .steps
        .iter()
        .rfind(|step| step.body.is_some())
        .map(|step| step.ms);
    let bag_from = ran.steps.iter().find(|step| step.bag).map(|step| step.ms);
    let faded = ran
        .steps
        .iter()
        .any(|step| step.body == Some(true) && !step.bag);
    let near = |ms: Option<u64>| ms.is_some_and(|ms| ms + 2 * u64::from(MSEC) >= due && ms <= due);
    let made = ran.made.as_ref();
    let took = |snapshot: &Snapshot| {
        let mut carried: Vec<String> = snapshot
            .goods
            .carried(crate::inventory::PARTY)
            .map(|(_, item)| item.template.clone())
            .collect();
        carried.sort();
        (carried, snapshot.goods.gold)
    };
    let bag_left = ran.last.entities.iter().any(|entity| {
        made.is_some_and(|(index, ..)| entity.kind == Kind::Object(ObjectKind::Placeable, *index))
    });
    vec![
        (
            "taken_after_delay_and_fade",
            check(
                near(body_until) && near(bag_from) && faded,
                format!(
                    "killed at {} ms: the body until {body_until:?}, faded {faded}, the remains \
                     from {bag_from:?}",
                    ran.killed_ms
                ),
            ),
        ),
        (
            "remains_hold_what_drops",
            check(
                made.is_some_and(|(_, spawned, _)| {
                    spawned.template == bag_template(2)
                        && spawned.holds
                            == [
                                ("rifle".to_string(), 1),
                                ("medpac".to_string(), 1),
                                (CREDITS.to_string(), 25),
                            ]
                }),
                format!("{:?}", made.map(|(_, spawned, _)| spawned)),
            ),
        ),
        (
            "remains_a_placeable",
            check(
                made.is_some_and(|(_, _, (facts, at))| {
                    facts.kind == ObjectKind::Placeable
                        && facts.tag.is_empty()
                        && facts.name == NAME
                        && facts.container
                        && facts.inventory.len() == 3
                        && (at[0] - ran.lies[0]).abs() < 0.01
                        && (at[1] - ran.lies[1]).abs() < 0.01
                }),
                format!(
                    "{:?} at {:?}, the body at {:?}",
                    made.map(|(_, _, (facts, _))| (&facts.name, &facts.tag, facts.kind)),
                    made.map(|(_, _, (_, at))| at),
                    ran.lies
                ),
            ),
        ),
        (
            "remains_targeted_where_they_lie",
            check(
                ran.targeted.is_some_and(|at| {
                    (at[0] - ran.lies[0]).abs() < 0.01 && (at[1] - ran.lies[1]).abs() < 0.01
                }),
                format!(
                    "the target block finds them at {:?}, the body at {:?}",
                    ran.targeted, ran.lies
                ),
            ),
        ),
        (
            "kept_stays",
            check(
                ran.kept && ran.bags == 1,
                format!(
                    "the kept body there {}, remains made {}",
                    ran.kept, ran.bags
                ),
            ),
        ),
        (
            "save_keeps_remains",
            check(ran.saved.is_ok(), format!("{:?}", ran.saved)),
        ),
        (
            "taken_all_and_gone",
            check(
                took(&ran.last) == (vec!["medpac".to_string(), "rifle".to_string()], 25)
                    && !bag_left,
                format!(
                    "the party {:?}, the remains left {bag_left}",
                    took(&ran.last)
                ),
            ),
        ),
        (
            "body_not_shoved",
            match walked_through() {
                Ok((passed, drift)) => check(
                    passed && drift < 0.01,
                    format!(
                        "the player walked past the body {passed}, the body moved {drift:.3} m \
                         while it lay"
                    ),
                ),
                Err(why) => Err(why),
            },
        ),
        (
            "same_every_run",
            check(
                again.as_ref().is_ok_and(|hash| *hash == ran.last.hash()),
                format!("{} then {again:?}", ran.last.hash().short()),
            ),
        ),
    ]
}

/// The name the probe's bag rows have.
const NAME: &str = "Remains";
/// The steps' length (ms), and the step the two die in.
const MSEC: u32 = 8;
const KILL_STEP: u64 = 4;
/// The step a script destroys the fallen one, its body fading (2.3 s
/// after its death: taken away at 2 s, faded at 2.6 s).
const DESTROY_AGAIN_STEP: u64 = KILL_STEP + 2300 / MSEC as u64;
/// How many steps the scenario runs before saving.
const STEPS: u64 = 400;
/// How long the player runs through the fallen one (steps: 0.8 s), and
/// the step before its body is taken away (2 s after its death).
const WALK_STEPS: u64 = 100;
const TAKEN_STEP: u64 = KILL_STEP + 1900 / MSEC as u64;

/// A step as the scenario saw it: its time, the fallen body (`None` gone,
/// else whether it is fading out: a lifetime set), whether the remains
/// stand.
struct Step {
    ms: u64,
    body: Option<bool>,
    bag: bool,
}

/// What a run saw: when the two died (sim ms), where the fallen one lay
/// (metres), each step; the remains as made (their number, what snapshots
/// carry, their facts and place); whether the kept body stayed and how
/// many remains were made; the save made then, back the same and loaded;
/// the loaded world's last snapshot, the remains taken from and closed.
struct Ran {
    killed_ms: u64,
    lies: [f32; 3],
    steps: Vec<Step>,
    made: Option<(u32, Spawned, (ObjectData, [f32; 3]))>,
    /// Where the target block finds the remains (`target::candidates`).
    targeted: Option<[f32; 3]>,
    kept: bool,
    bags: usize,
    saved: Result<(), String>,
    last: Snapshot,
}

fn creature(snapshot: &Snapshot, index: u32) -> Option<&EntityState> {
    snapshot
        .entities
        .iter()
        .find(|entity| entity.kind == Kind::Object(ObjectKind::Creature, index))
}

/// The death script that keeps its creature: `SetIsDestroyable(FALSE)`;
/// and one destroying the fallen one and the kept one
/// (`DestroyObject(GetObjectByTag(..))`), run as the fallen one's body
/// fades: the kept one is not destroyed (TSL's `CSWSCreature::EventHandler`
/// passes `EVENT_DESTROY_OBJECT` over for a creature not destroyable, so
/// 506OND's `a_end_game` destroys Tobin's body and finds him still there to
/// raise for Kreia's scene) [verify in K1's exe].
fn keeping_script() -> crate::script::Library {
    use fmt_odyssey::ncs::{Const, Op};
    const CATALOG: &str = r#"
// 0
void SetIsDestroyable(int bDestroyable, int bRaiseable = TRUE, int bSelectableWhenDead = FALSE);
// 1
void DestroyObject(object oDestroy, float fDelay=0.0f, int bNoFade = FALSE, float fDelayUntilFade = 0.0f);
// 2
object GetObjectByTag(string sTag, int nNth=0);
"#;
    let mut keep = script_ncs::probes::Asm::default();
    for value in [0, 1, 0] {
        keep.op(Op::Const(Const::Int(value)));
    }
    keep.op(Op::Action {
        routine: 0,
        args: 3,
    })
    .op(Op::Retn);
    let mut destroy = script_ncs::probes::Asm::default();
    for tag in [&b"fallen"[..], b"kept"] {
        destroy
            .op(Op::Const(Const::Int(0)))
            .op(Op::Const(Const::String(tag.to_vec())))
            .op(Op::Action {
                routine: 2,
                args: 2,
            })
            .op(Op::Action {
                routine: 1,
                args: 1,
            });
    }
    destroy.op(Op::Retn);
    crate::probe::library(
        CATALOG,
        [
            ("probe_keep", keep.build()),
            ("probe_destroy", destroy.build()),
        ],
    )
}

/// The proving ground with the two creatures added (`probes`), a world on
/// it, and the first one's placement.
fn ground() -> Result<(crate::Level, SimWorld, u32), String> {
    use crate::inventory::{Catalog, Held, ItemFacts};
    use crate::world::containers::plain;

    let ground = crate::determinism::proving_ground();
    let mut data = (**ground.module().ok_or("the ground has no module")?).clone();
    let [x, y, z] = data.entry.position;
    let held = |template: &str, slot: Option<u8>, droppable: bool| Held {
        template: template.into(),
        stack: 1,
        slot,
        droppable,
        infinite: false,
    };
    let body = Body {
        appearance: 0,
        size: 3,
        movement: 7,
        walk_mps: 1.7,
        run_mps: 5.4,
        head_arc_h: 0.0,
        bag: 2,
        destroy_ms: 2000,
        fade_ms: Some(600),
        bulk: None,
    };
    let fallen = ObjectData {
        inventory: vec![
            held("rifle", Some(4), true),
            held("medpac", None, true),
            held("hide", None, false),
        ],
        body: Some(body),
        ..ObjectData::creature(
            "fallen",
            "fallen",
            [x + 1.5, y, z],
            crate::combat::Sheet::commoner(),
        )
    };
    let kept = ObjectData {
        inventory: vec![held("medpac", None, true)],
        scripts: vec![("ScriptDeath".into(), "probe_keep".into())],
        ..ObjectData::creature(
            "kept",
            "kept",
            [x, y + 3.0, z],
            crate::combat::Sheet::commoner(),
        )
    };
    let first = data.placements.len() as u32;
    data.placements.extend([fallen, kept].map(Placement::Own));
    data.bags = [None, Some(3), Some(33)]
        .map(|appearance| BagRow {
            appearance,
            name: NAME.into(),
            corpse: false,
        })
        .to_vec();
    let credits = ItemFacts {
        base_item: crate::inventory::CREDITS,
        stacking: u16::MAX,
        ..plain(CREDITS)
    };
    let items = ["rifle", "medpac", "hide"]
        .map(plain)
        .into_iter()
        .chain([credits]);
    let level = ground
        .with_module(data)
        .with_scripts(keeping_script().with_items(Catalog::new(items)));
    let world = SimWorld::new(
        crate::WorldConfig {
            seed: 1,
            seats: 1,
            wanderers: 0,
            spawn: [0.0; 3],
            yaw: 0.0,
            sheet: crate::combat::Sheet::dev_soldier(),
            guests: crate::Guests::Avatars,
        },
        level.clone(),
    )
    .map_err(|error| error.to_string())?;
    Ok((level, world, first))
}

/// The two creatures from placement `first` on, killed.
fn kill(world: &mut SimWorld, first: u32) {
    let mut creatures = world.world.query::<(&Object, &mut Fighting)>();
    for (_, mut fighting) in creatures
        .iter_mut(&mut world.world)
        .filter(|(object, _)| object.index >= first)
    {
        fighting.0.life = crate::combat::Life::Dead;
        fighting.0.hp = 0;
    }
}

/// The player, turned to +X, runs from the entry through where the fallen
/// one lies, 1.5 m on, for `WALK_STEPS` and stands while its body lies: a
/// corpse passes through bodies (JA's `PmoveSingle` takes `CONTENTS_BODY`
/// out of the dead's mask), so it is not shoved along (a Sith thug's on
/// Korriban, pushed 7 m by the player and its remains stood where it was
/// taken, far from where it fell). Whether the player got past it, and
/// how far the body moved while it lay (metres).
fn walked_through() -> Result<(bool, f32), String> {
    let (_, mut world, first) = ground()?;
    let delta = world
        .snapshot()
        .entities
        .iter()
        .find_map(|entity| {
            entity
                .player
                .filter(|_| matches!(entity.kind, Kind::Avatar(_)))
        })
        .ok_or("no player")?
        .view_offset;
    let mut lay: Option<[f32; 3]> = None;
    let mut drift = 0.0f32;
    let mut furthest = f32::MIN;
    for tick in 1..=TAKEN_STEP {
        if tick == KILL_STEP {
            kill(&mut world, first);
        }
        let mut input = TickInput::default();
        if (KILL_STEP..KILL_STEP + WALK_STEPS).contains(&tick) {
            input.insert(
                Seat(0),
                crate::Command {
                    angles: [0, 0i16.wrapping_sub(delta[1]), 0],
                    forward: 127,
                    ..crate::Command::default()
                },
            );
        }
        let snapshot = crate::step(&mut world, tick, &input, MSEC, StepReason::Authority);
        if let Some(player) = snapshot
            .entities
            .iter()
            .find(|entity| matches!(entity.kind, Kind::Avatar(_)))
        {
            furthest = furthest.max(player.position[0]);
        }
        let Some(body) = creature(&snapshot, first).filter(|_| tick > KILL_STEP) else {
            continue;
        };
        let at = *lay.get_or_insert(body.position);
        drift = drift.max(crate::math::length(crate::math::sub(body.position, at)));
    }
    let passed = lay.is_some_and(|lay| furthest > lay[0] + 1.0);
    Ok((passed, drift))
}

fn scenario() -> Result<Ran, String> {
    let (level, mut world, first) = ground()?;
    let id_of = |world: &mut SimWorld, index: u32| {
        world
            .world
            .query::<(&Id, &Object)>()
            .iter(&world.world)
            .find(|(_, object)| object.index == index)
            .map(|(id, _)| id.0.0)
    };
    let fallen_id = id_of(&mut world, first).ok_or("no fallen creature")?;
    world
        .world
        .resource_mut::<Scripts>()
        .state
        .add_gold(fallen_id, 25);
    let player = world
        .snapshot()
        .entities
        .iter()
        .find(|entity| matches!(entity.kind, Kind::Avatar(_)))
        .map(|entity| entity.id.0)
        .ok_or("no player")?;
    let mut steps = Vec::new();
    let mut killed_ms = 0;
    let mut lies = [0.0; 3];
    let mut last = None;
    for tick in 1..=STEPS {
        if tick == KILL_STEP {
            kill(&mut world, first);
        }
        if tick == DESTROY_AGAIN_STEP {
            let mut state = world.script_state().clone();
            state.pending.push(crate::script::Pending::new(
                "probe_destroy",
                script_ncs::vm::ObjectId(player),
            ));
            world.set_script_state(state);
        }
        let snapshot = crate::step(
            &mut world,
            tick,
            &TickInput::default(),
            MSEC,
            StepReason::Authority,
        );
        if tick == KILL_STEP {
            killed_ms = snapshot.time_ms;
        }
        let body = creature(&snapshot, first);
        if let Some(body) = body {
            lies = body.position;
        }
        steps.push(Step {
            ms: snapshot.time_ms,
            body: body.map(|body| body.life_ms.is_some()),
            bag: snapshot
                .spawned
                .iter()
                .any(|made| bag_row(&made.template).is_some()),
        });
        last = Some(snapshot);
    }
    let snapshot = last.ok_or("no steps")?;
    let instances = crate::Instances::in_snapshot(&level, &snapshot);
    let bags: Vec<u32> = (first + 2..)
        .take(snapshot.spawned.len())
        .filter(|index| {
            instances
                .made_from(*index)
                .is_some_and(|made| bag_row(&made.template).is_some())
        })
        .collect();
    let made = bags.first().and_then(|&index| {
        let entity = snapshot
            .entities
            .iter()
            .find(|entity| entity.kind == Kind::Object(ObjectKind::Placeable, index))?;
        let facts = instances.get(index)?.clone();
        Some((
            index,
            instances.made_from(index)?.clone(),
            (facts, entity.position),
        ))
    });
    let targeted = made.as_ref().and_then(|(index, ..)| {
        crate::target::candidates(&level, &snapshot, None)
            .into_iter()
            .find(|candidate| candidate.index == *index)
            .map(|candidate| candidate.feet)
    });
    let save = crate::Save {
        hz: crate::TICK_HZ,
        level: level.id(),
        snapshot: snapshot.clone(),
        script: world.script_state().clone(),
    };
    let saved = crate::Save::decode(&save.encode())
        .map_err(|error| error.to_string())
        .and_then(|back| {
            let same = back == save;
            let holds = back.script.items.values().any(|item| {
                made.as_ref().is_some_and(|(index, ..)| {
                    id_of_in(&back.snapshot, *index) == Some(item.holder)
                })
            });
            if same && holds {
                Ok(back)
            } else {
                Err(format!("back the same {same}, its remains holding {holds}"))
            }
        });
    let mut loaded = match &saved {
        Ok(back) => back.world(&level, 1, None)?,
        Err(_) => world,
    };
    let index = made.as_ref().map_or(u32::MAX, |(index, ..)| *index);
    let actions: [&[Action]; 4] = [
        &[Action::Open(index), Action::TakeAll(index)],
        &[Action::Close(index)],
        &[],
        &[],
    ];
    let mut last = snapshot.clone();
    for (n, actions) in actions.iter().enumerate() {
        let mut input = TickInput::default();
        for action in *actions {
            input.act(Seat(0), action.clone());
        }
        last = crate::step(
            &mut loaded,
            STEPS + 1 + n as u64,
            &input,
            MSEC,
            StepReason::Authority,
        );
    }
    Ok(Ran {
        killed_ms,
        lies: [
            lies[0],
            lies[1],
            lies[2] + crate::motion::SOLE_Z * collision::UNIT,
        ],
        steps,
        made,
        targeted,
        kept: creature(&snapshot, first + 1).is_some(),
        bags: bags.len(),
        saved: saved.map(|_| ()),
        last,
    })
}

/// The id of object `index` in `snapshot`.
fn id_of_in(snapshot: &Snapshot, index: u32) -> Option<u32> {
    snapshot
        .entities
        .iter()
        .find(|entity| matches!(entity.kind, Kind::Object(_, at) if at == index))
        .map(|entity| entity.id.0)
}
