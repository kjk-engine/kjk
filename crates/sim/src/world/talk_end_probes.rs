//! The probes of a conversation's end and a locked door's refusal (`kjk
//! sim probes "talk ends"`), on the proving ground, no install, as K1's
//! exe has them.
//!
//! Ends: creatures and a placeable, each telling the player its tag from
//! its end-of-conversation script (a creature's `ScriptEndDialogu`, a
//! placeable's `OnEndDialogue`): a bystander, a prop, a dead body, a party
//! member and the conversation's owner. A conversation the owner owns,
//! ended by its last reply or with nothing to say, runs every living one's,
//! the party member's too, in the area's order; a one-liner runs its
//! owner's only; one left (cut short) none.
//!
//! Doors: the ground's door, locked, with no key, opened by the player.
//! Its `OnFailToOpen` that unlocks and opens it (as the Endar Spire's do)
//! leaves the player told nothing and the door open; one that does
//! nothing tells the player it is locked.
//!
//! Owners lost: a party member's conversation goes on while its owner
//! lies knocked out (it gets up again) and after its death; one whose
//! owner is destroyed is cut short.

use super::{Fighting, Id, Object, SimWorld};
use crate::StepReason;
use crate::action::{Action, Verb};
use crate::combat::Sheet;
use crate::combat::fighter::Life;
use crate::input::{Command, Seat, TickInput};
use crate::module::{ObjectData, ObjectKind, Opens, Placement};
use crate::notice::Notice;
use crate::probe::{Checks, check};
use crate::script::dialogue::{Pick, Start, answer};
use crate::script::dialogue_probes::talking;
use crate::snapshot::{Kind, Snapshot};
use fmt_odyssey::ncs::{Const, Op};
use script_ncs::probes::Asm;
use script_ncs::vm::ObjectId;
use std::collections::BTreeMap;

const MSEC: u32 = 8;
/// How long a pick may wait for its conversation's replies (steps): 8 s.
const ANSWER_STEPS: usize = 1000;

pub(super) const CATALOG: &str = r#"
// 0
void SendMessageToPC(object oPlayer, string szMessage);
// 1
object GetFirstPC();
// 2
string GetTag(object oObject);
// 3
void SetLocked(object oTarget, int bLocked);
// 4
void ActionOpenDoor(object oDoor);
// 5
void DestroyObject(object oDestroy, float fDelay=0.0f, int bNoFade = FALSE, float fDelayUntilFade = 0.0f, int nHideFeedback = 0);
"#;

/// What a hearer is besides its kind.
#[derive(Clone, Copy, PartialEq)]
enum Role {
    Living,
    /// Dead before the conversation: it hears nothing.
    Dead,
    /// In the party (`party::Party::join`).
    Party,
}

/// The hearers, in the area's order, the conversation's owner last.
const HEARERS: [(&str, ObjectKind, Role); 5] = [
    ("bystander", ObjectKind::Creature, Role::Living),
    ("prop", ObjectKind::Placeable, Role::Living),
    ("corpse", ObjectKind::Creature, Role::Dead),
    ("buddy", ObjectKind::Creature, Role::Party),
    ("owner", ObjectKind::Creature, Role::Living),
];

pub(super) fn call(routine: u16, args: u8) -> Op {
    Op::Action { routine, args }
}

/// `SendMessageToPC(GetFirstPC(), GetTag(OBJECT_SELF))`.
fn tells_tag() -> fmt_odyssey::ncs::Ncs {
    let mut asm = Asm::default();
    asm.op(Op::Const(Const::Object(0)))
        .op(call(2, 1))
        .op(call(1, 0))
        .op(call(0, 2))
        .op(Op::Retn);
    asm.build()
}

/// `SetLocked(OBJECT_SELF, FALSE); ActionOpenDoor(OBJECT_SELF)`.
fn opens_itself() -> fmt_odyssey::ncs::Ncs {
    let mut asm = Asm::default();
    asm.op(Op::Const(Const::Int(0)))
        .op(Op::Const(Const::Object(0)))
        .op(call(3, 2))
        .op(Op::Const(Const::Object(0)))
        .op(call(4, 1))
        .op(Op::Retn);
    asm.build()
}

/// `DestroyObject(OBJECT_SELF, 0.0, TRUE)`: at once, unfaded.
fn destroys_itself() -> fmt_odyssey::ncs::Ncs {
    let mut asm = Asm::default();
    asm.op(Op::Const(Const::Int(0)))
        .op(Op::Const(Const::Float(0.0)))
        .op(Op::Const(Const::Int(1)))
        .op(Op::Const(Const::Float(0.0)))
        .op(Op::Const(Const::Object(0)))
        .op(call(5, 5))
        .op(Op::Retn);
    asm.build()
}

fn library() -> crate::script::Library {
    let dialogues: BTreeMap<String, fmt_odyssey::dlg::Dlg> = [
        ("probe_talk", talking(2, "")),
        ("probe_one_liner", talking(0, "")),
        ("probe_destroyed_talk", talking(2, "probe_destroy_self")),
        ("probe_nothing_to_say", {
            let mut dlg = talking(2, "");
            dlg.starts.clear();
            dlg
        }),
    ]
    .map(|(name, dlg)| (name.to_string(), dlg))
    .into();
    crate::probe::library(
        CATALOG,
        [
            ("probe_tell_tag", tells_tag()),
            ("probe_open_self", opens_itself()),
            ("probe_destroy_self", destroys_itself()),
            ("probe_nothing", {
                let mut asm = Asm::default();
                asm.op(Op::Retn);
                asm.build()
            }),
        ],
    )
    .with_dialogues(dialogues, BTreeMap::new())
}

/// The proving ground's entry moved by `dx`, `dy`.
pub(super) fn by_entry(dx: f32, dy: f32) -> [f32; 3] {
    let [x, y, z] = crate::determinism::proving_ground()
        .module()
        .map_or([0.0; 3], |module| module.entry.position);
    [x + dx, y + dy, z]
}

/// A world on the proving ground with `placed` added, `scripts` its
/// library, and its door's `OnFailToOpen` `door_fails` (locked, no key) if
/// any: the world and the first placed one's index.
pub(super) fn world_with(
    placed: Vec<ObjectData>,
    door_fails: Option<&str>,
    scripts: crate::script::Library,
) -> Result<(SimWorld, u32), String> {
    let ground = crate::determinism::proving_ground();
    let mut data = (**ground.module().ok_or("the ground has no module")?).clone();
    if let Some(script) = door_fails {
        let Some(Placement::Own(gate)) = data.placements.first_mut() else {
            return Err("the ground's first object is not its door".into());
        };
        gate.opens = Opens::Locked;
        gate.scripts = vec![("OnFailToOpen".into(), script.into())];
    }
    let first = data.placements.len() as u32;
    data.placements
        .extend(placed.into_iter().map(Placement::Own));
    let level = ground.with_module(data).with_scripts(scripts);
    let world =
        SimWorld::new(crate::WorldConfig::one_seat(), level).map_err(|error| error.to_string())?;
    Ok((world, first))
}

pub(super) fn id_of(world: &mut SimWorld, index: u32) -> Result<u32, String> {
    world
        .world
        .query::<(&Id, &Object)>()
        .iter(&world.world)
        .find(|(_, object)| object.index == index)
        .map(|(id, _)| id.0.0)
        .ok_or(format!("object {index} not placed"))
}

pub(super) fn step(world: &mut SimWorld, tick: u64, input: &TickInput) -> Snapshot {
    crate::step(world, tick, input, MSEC, StepReason::Authority)
}

/// What the player was told (`Notice::Said`), in order.
pub(super) fn told(world: &mut SimWorld, into: &mut Vec<String>) {
    into.extend(
        world
            .take_notices()
            .into_iter()
            .filter_map(|(_, notice)| match notice {
                Notice::Said(words) => Some(words),
                _ => None,
            }),
    );
}

/// How a conversation is brought to its end.
#[derive(Clone, Copy)]
enum Ending {
    /// Its last reply ("Goodbye.") taken.
    Reply,
    /// Left: cut short.
    Leave,
    /// A one-liner, ended as it is spoken.
    OneLiner,
    /// No entry to start from: it ends as it starts.
    NothingToSay,
}

/// The hearers placed, the owner's conversation started and ended by
/// `ending`: the tags their end scripts told, in order.
fn ends(ending: Ending) -> Result<Vec<String>, String> {
    let placed = HEARERS
        .iter()
        .enumerate()
        .map(|(n, &(tag, kind, _))| {
            let at = by_entry(1.5, n as f32 - 1.0);
            let hook = match kind {
                ObjectKind::Placeable => "OnEndDialogue",
                _ => "ScriptEndDialogu",
            };
            let base = match kind {
                ObjectKind::Placeable => ObjectData {
                    kind,
                    tag: tag.into(),
                    ..ObjectData::unknown(tag, at, 0.0)
                },
                _ => ObjectData::creature(tag, tag, at, Sheet::commoner()),
            };
            ObjectData {
                scripts: vec![(hook.into(), "probe_tell_tag".into())],
                ..base
            }
        })
        .collect();
    let (mut world, first) = world_with(placed, None, library())?;
    let mut state = world.script_state().clone();
    for (n, &(tag, _, role)) in (first..).zip(&HEARERS) {
        let id = id_of(&mut world, n)?;
        match role {
            Role::Living => {}
            Role::Dead => {
                let mut fighters = world.world.query::<(&Id, &mut Fighting)>();
                for (own, mut fighting) in fighters.iter_mut(&mut world.world) {
                    if own.0.0 == id {
                        fighting.0.hp = 0;
                        fighting.0.life = Life::Dead;
                    }
                }
            }
            Role::Party => {
                let slot = crate::script::party::Npc {
                    template: tag.into(),
                    object: None,
                    kept: None,
                    template_data: None,
                };
                state.party.available.insert(0, slot);
                if !state.party.join(0, id) {
                    return Err(format!("{tag} did not join"));
                }
            }
        }
    }
    world.set_script_state(state);
    let owner = id_of(&mut world, first + HEARERS.len() as u32 - 1)?;
    let mut tick = 1;
    let player = step(&mut world, tick, &TickInput::default())
        .body(Seat(0))
        .map(|entity| entity.id.0)
        .ok_or("no player")?;
    let dialog = match ending {
        Ending::OneLiner => "probe_one_liner",
        Ending::NothingToSay => "probe_nothing_to_say",
        Ending::Reply | Ending::Leave => "probe_talk",
    };
    let mut state = world.script_state().clone();
    state.talk.asked.push(Start {
        owner: ObjectId(owner),
        pc: ObjectId(player),
        asker: ObjectId(player),
        dialog: dialog.into(),
        bark: false,
    });
    world.set_script_state(state);
    let mut said = Vec::new();
    // A seat's first answer byte only starts its count: a still one first.
    let mut still = TickInput::default();
    still.insert(Seat(0), Command::default());
    tick += 1;
    step(&mut world, tick, &still);
    told(&mut world, &mut said);
    let pick = match ending {
        Ending::Reply => Some(Pick::Reply(1)),
        Ending::Leave => Some(Pick::Leave),
        Ending::OneLiner | Ending::NothingToSay => None,
    };
    // The pick held (one answer, however many steps carry it) until the
    // replies are up and it is taken.
    let mut input = TickInput::default();
    if let Some(pick) = pick {
        input.insert(
            Seat(0),
            Command {
                reply: answer(0, pick),
                ..Command::default()
            },
        );
    }
    for _ in 0..ANSWER_STEPS {
        tick += 1;
        let snapshot = step(&mut world, tick, &input);
        told(&mut world, &mut said);
        if snapshot.talk.conversations.is_empty() {
            return Ok(said);
        }
    }
    Err(format!(
        "the conversation did not end: {:?}; told {said:?}",
        world.snapshot().talk.conversations
    ))
}

/// How the owner of a conversation is lost while it plays.
#[derive(Clone, Copy)]
enum Lost {
    /// A party member knocked out (`Fighter::fall`): it gets up again.
    KnockedOut,
    /// Dead for good.
    Dead,
    /// Not of the party, destroyed by its first line's script
    /// (`DestroyObject(OBJECT_SELF)`, as TSL's `a_next_scene` takes VisasCut).
    Destroyed,
}

/// A dead creature with a conversation queued for it to start
/// (`ActionStartConversation`) starts it, though the dead act on nothing
/// else: TSL's Visquis, dead, starts Kreia's scene (`a_start_kreend`).
/// Whether the conversation began, and whether a walk queued after it was
/// left waiting.
fn dead_starts_its_conversation() -> Result<(bool, bool), String> {
    use crate::script::actions::{Action as Queue, Queued};
    let at = by_entry(1.5, 0.0);
    let placed = vec![ObjectData::creature(
        "buddy",
        "buddy",
        at,
        Sheet::commoner(),
    )];
    let (mut world, first) = world_with(placed, None, library())?;
    let owner = id_of(&mut world, first)?;
    let mut tick = 1;
    let player = step(&mut world, tick, &TickInput::default())
        .body(Seat(0))
        .map(|entity| entity.id.0)
        .ok_or("no player")?;
    let mut fighters = world.world.query::<(&Id, &mut Fighting)>();
    for (own, mut fighting) in fighters.iter_mut(&mut world.world) {
        if own.0.0 == owner {
            fighting.0.hp = 0;
            fighting.0.life = Life::Dead;
        }
    }
    let mut state = world.script_state().clone();
    let queued = |action| Queued {
        action,
        started_ms: None,
    };
    state.actions.insert(
        owner,
        vec![
            queued(Queue::Converse {
                with: ObjectId(player),
                dialog: "probe_talk".into(),
                asker: None,
            }),
            queued(Queue::Wait { ms: 1000 }),
        ],
    );
    world.set_script_state(state);
    let mut still = TickInput::default();
    still.insert(Seat(0), Command::default());
    let mut began = false;
    for _ in 0..5 {
        tick += 1;
        began |= !step(&mut world, tick, &still).talk.conversations.is_empty();
    }
    let waiting = world
        .script_state()
        .actions
        .get(&owner)
        .is_some_and(|queue| queue.iter().any(|q| matches!(q.action, Queue::Wait { .. })));
    Ok((began, waiting))
}

/// A party member's conversation with the player (TSL's Atton starting
/// Peragus' escape), its owner `lost` as its first line plays; then its
/// last reply taken: whether it was still under way after the fall, and
/// whether it ended cut short.
fn owner_lost(lost: Lost) -> Result<(bool, bool), String> {
    let at = by_entry(1.5, 0.0);
    let placed = vec![ObjectData::creature(
        "buddy",
        "buddy",
        at,
        Sheet::commoner(),
    )];
    let (mut world, first) = world_with(placed, None, library())?;
    let owner = id_of(&mut world, first)?;
    let dialog = match lost {
        Lost::Destroyed => "probe_destroyed_talk",
        Lost::KnockedOut | Lost::Dead => {
            let mut state = world.script_state().clone();
            let slot = crate::script::party::Npc {
                template: "buddy".into(),
                object: None,
                kept: None,
                template_data: None,
            };
            state.party.available.insert(0, slot);
            if !state.party.join(0, owner) {
                return Err("buddy did not join".into());
            }
            world.set_script_state(state);
            "probe_talk"
        }
    };
    let mut tick = 1;
    let player = step(&mut world, tick, &TickInput::default())
        .body(Seat(0))
        .map(|entity| entity.id.0)
        .ok_or("no player")?;
    let mut state = world.script_state().clone();
    state.talk.asked.push(Start {
        owner: ObjectId(owner),
        pc: ObjectId(player),
        asker: ObjectId(player),
        dialog: dialog.into(),
        bark: false,
    });
    world.set_script_state(state);
    let mut still = TickInput::default();
    still.insert(Seat(0), Command::default());
    tick += 1;
    let started = step(&mut world, tick, &still);
    let Some(id) = started.talk.conversations.first().map(|c| c.id) else {
        return Err("the conversation did not start".into());
    };
    let now = started.time_ms;
    let mut fighters = world.world.query::<(&Id, &mut Fighting)>();
    for (own, mut fighting) in fighters.iter_mut(&mut world.world) {
        if own.0.0 == owner {
            // The party's to the world too: a companion of the player's.
            fighting.0.mind = crate::combat::ai::Mind::companion(Seat(0));
            match lost {
                Lost::KnockedOut => fighting.0.fall(now),
                Lost::Dead => {
                    fighting.0.hp = 0;
                    fighting.0.life = Life::Dead;
                }
                Lost::Destroyed => {}
            }
        }
    }
    for _ in 0..10 {
        tick += 1;
        step(&mut world, tick, &still);
    }
    let on = world
        .snapshot()
        .talk
        .conversations
        .iter()
        .any(|c| c.id == id);
    let mut input = TickInput::default();
    input.insert(
        Seat(0),
        Command {
            reply: answer(0, Pick::Reply(1)),
            ..Command::default()
        },
    );
    for _ in 0..ANSWER_STEPS {
        tick += 1;
        if step(&mut world, tick, &input).talk.conversations.is_empty() {
            break;
        }
    }
    let ended = |aborted: bool| {
        world.script_report().talk.iter().any(|line| {
            matches!(line, crate::script::dialogue::Line::Ended { id: of, aborted: cut, .. }
                if *of == id && *cut == aborted)
        })
    };
    match (ended(false), ended(true)) {
        (_, true) => Ok((on, true)),
        (true, false) => Ok((on, false)),
        _ => Err(format!(
            "the conversation did not end: {:?}",
            world.snapshot().talk.conversations
        )),
    }
}

/// The door locked with `fails` its `OnFailToOpen`, the player opening it
/// from a step before it: whether it was told it is locked, and whether
/// the door stands open after.
fn refused(fails: &str) -> Result<(bool, bool), String> {
    let (mut world, _) = world_with(Vec::new(), Some(fails), library())?;
    let near = crate::determinism::ground_point(-1.2, 0.0, 0.0);
    let mut players = world.world.query::<(&super::Avatar, &mut super::Player)>();
    for (_, mut player) in players.iter_mut(&mut world.world) {
        player.0.origin[0] = near[0] / collision::UNIT;
        player.0.origin[1] = near[1] / collision::UNIT;
    }
    let mut input = TickInput::default();
    input.act(
        Seat(0),
        Action::Use {
            object: 0,
            verb: Verb::Open,
        },
    );
    let locked_line = Notice::Line(super::locks::lines::LOCKED);
    let mut locked = false;
    let mut last = step(&mut world, 1, &input);
    for tick in 1..=60 {
        locked |= world
            .take_notices()
            .iter()
            .any(|(seat, notice)| *seat == Seat(0) && *notice == locked_line);
        last = step(&mut world, tick + 1, &TickInput::default());
    }
    let open = last
        .entities
        .iter()
        .find(|entity| entity.kind == Kind::Object(ObjectKind::Door, 0))
        .and_then(|entity| entity.object)
        .is_some_and(|object| object.state != 0);
    Ok((locked, open))
}

pub fn probes() -> Checks {
    let all: Vec<String> = HEARERS
        .iter()
        .filter(|(.., role)| *role != Role::Dead)
        .map(|(tag, ..)| tag.to_string())
        .collect();
    let ended = |ending, want: &[String], what: &str| match ends(ending) {
        Ok(said) => check(said == want, format!("{what}: told {said:?}")),
        Err(why) => Err(why),
    };
    let opened = refused("probe_open_self");
    let shut = refused("probe_nothing");
    vec![
        (
            "end_heard_by_the_area",
            ended(Ending::Reply, &all, "ended by its last reply"),
        ),
        (
            "nothing_to_say_heard_by_the_area",
            ended(Ending::NothingToSay, &all, "nothing to say"),
        ),
        (
            "one_liner_end_heard_by_its_owner",
            ended(Ending::OneLiner, &["owner".to_string()], "a one-liner"),
        ),
        (
            "cut_short_end_heard_by_none",
            ended(Ending::Leave, &[], "left"),
        ),
        (
            "owner_knocked_out_talks_on",
            match owner_lost(Lost::KnockedOut) {
                Ok((on, cut)) => check(
                    on && !cut,
                    format!("under way after the fall {on}, cut short {cut}"),
                ),
                Err(why) => Err(why),
            },
        ),
        // The conversation is its owner's object's: TSL's exe keeps it on the
        // owner (`CSWSObject::StartDialog` sets it there, the main loop runs
        // the owner's `UpdateDialog`). A dead owner is still there, so
        // `miraend` goes on past the death its own first line's script deals
        // (`a_305end_setup0`, Visquis).
        (
            "owner_dead_talks_on",
            match owner_lost(Lost::Dead) {
                Ok((on, cut)) => check(
                    on && !cut,
                    format!("under way after the death {on}, cut short {cut}"),
                ),
                Err(why) => Err(why),
            },
        ),
        // A destroyed one is not: `DestroyObject`'s event deletes the creature
        // at once, whatever it is doing (`CSWSCreature::EventHandler` asks only
        // that it is destroyable and not the player's or the party's), and
        // its destructor the conversation with it. So Visas's joining talk
        // ends at the Handmaiden's line, whose `a_next_scene` takes VisasCut.
        (
            "owner_destroyed_cut_short",
            match owner_lost(Lost::Destroyed) {
                Ok((on, cut)) => check(
                    !on && cut,
                    format!("under way after the destruction {on}, cut short {cut}"),
                ),
                Err(why) => Err(why),
            },
        ),
        (
            "dead_starts_its_conversation",
            match dead_starts_its_conversation() {
                Ok((began, waiting)) => check(
                    began && waiting,
                    format!("the conversation began {began}, the walk after it waits {waiting}"),
                ),
                Err(why) => Err(why),
            },
        ),
        (
            "door_opening_itself_says_nothing",
            match opened {
                Ok((locked, open)) => check(
                    !locked && open,
                    format!("told it is locked {locked}, open after {open}"),
                ),
                Err(why) => Err(why),
            },
        ),
        (
            "door_staying_locked_says_so",
            match shut {
                Ok((locked, open)) => check(
                    locked && !open,
                    format!("told it is locked {locked}, open after {open}"),
                ),
                Err(why) => Err(why),
            },
        ),
    ]
}
