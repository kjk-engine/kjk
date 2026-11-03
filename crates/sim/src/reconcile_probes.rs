//! Probes for `reconcile`: the number map over changed placement lists, the
//! roster's encoding, and saves loaded on the proving ground after its
//! placements changed.

use std::sync::Arc;

use script_ncs::probes::Check;

use crate::determinism::ground_point;
use crate::module::{ObjectData, ObjectKind, Placement};
use crate::reconcile::{Renumber, Report, decode, encode, renumber, roster};
use crate::script::party::Npc;
use crate::script::{Library, State};
use crate::snapshot::{EntityState, Kind, SimId, Snapshot};
use crate::wire::{Reader, Writer};
use crate::world::Level;
use crate::{Save, SimWorld};

/// A placement from `template/tag` shorthand: a creature, or a door when
/// the text starts `door:`.
fn placed(text: &str) -> Arc<ObjectData> {
    let (kind, rest) = match text.split_once(':') {
        Some(("door", rest)) => (ObjectKind::Door, rest),
        _ => (ObjectKind::Creature, text),
    };
    let (template, tag) = rest.split_once('/').unwrap_or((rest, ""));
    Arc::new(ObjectData {
        kind,
        tag: tag.into(),
        ..ObjectData::unknown(template, [0.0; 3], 0.0)
    })
}

fn list(texts: &[&str]) -> Vec<Arc<ObjectData>> {
    texts.iter().map(|text| placed(text)).collect()
}

fn check(name: &'static str, ok: bool, detail: String) -> Check {
    Check { name, ok, detail }
}

/// Every case of the number map, and the roster's round trip.
pub fn checks() -> Vec<Check> {
    let some = |numbers: &[u32]| numbers.iter().map(|&n| Some(n)).collect::<Vec<_>>();
    let cases: [(&'static str, &[&str], &[&str], Renumber); 5] = [
        (
            "unchanged is the identity",
            &["a/x", "b/y", "c/z"],
            &["a/x", "b/y", "c/z"],
            Renumber {
                map: some(&[0, 1, 2]),
                made_shift: 0,
                added: vec![],
            },
        ),
        (
            "one inserted at 2 of 5",
            &["a/1", "b/2", "c/3", "d/4", "e/5"],
            &["a/1", "b/2", "new/n", "c/3", "d/4", "e/5"],
            Renumber {
                map: some(&[0, 1, 3, 4, 5]),
                made_shift: 1,
                added: vec![2],
            },
        ),
        (
            "one removed at 1",
            &["a/1", "b/2", "c/3"],
            &["a/1", "c/3"],
            Renumber {
                map: vec![Some(0), None, Some(1)],
                made_shift: -1,
                added: vec![],
            },
        ),
        (
            "the n-th of a key is the n-th",
            &["a/x", "a/x", "door:d/x"],
            &["door:d/x", "a/x", "a/x"],
            Renumber {
                map: some(&[1, 2, 0]),
                made_shift: 0,
                added: vec![],
            },
        ),
        (
            "case does not matter, kind does",
            &["A/X", "door:a/x"],
            &["door:a/x", "a/x"],
            Renumber {
                map: some(&[1, 0]),
                made_shift: 0,
                added: vec![],
            },
        ),
    ];
    let mut out: Vec<Check> = cases
        .into_iter()
        .map(|(name, old, new, expected)| {
            let got = renumber(&roster(&list(old)), &list(new));
            check(name, got == expected, format!("{got:?}"))
        })
        .collect();

    let kept = roster(&list(&["a/x", "door:d/y"]));
    let rosters = [None, Some(kept), Some(Vec::new())];
    let round_trips = rosters.iter().all(|roster| {
        let mut bytes = Writer::default();
        encode(roster.as_deref(), &mut bytes);
        let mut input = Reader::new("roster", &bytes.0);
        decode(&mut input).is_ok_and(|read| read == *roster) && input.finish().is_ok()
    });
    out.push(check(
        "a roster round trips",
        round_trips,
        format!("{} rosters", rosters.len()),
    ));
    out
}

/// The proving ground, with a creature of the party's after its door and
/// trigger, and its placements as `change` leaves them.
fn ground(change: fn(&mut Vec<Placement>)) -> Level {
    let ground = crate::determinism::proving_ground();
    let mut data = (**ground.module().expect("the proving ground is a module")).clone();
    data.placements.push(creature(MEMBER, None));
    change(&mut data.placements);
    let library = Library::new(Default::default(), Default::default(), Default::default());
    ground.with_module(data).with_scripts(library)
}

/// A creature tagged `tag` (its template too), spawning with `spawn`.
fn creature(tag: &str, spawn: Option<&str>) -> Placement {
    Placement::Own(ObjectData {
        tag: tag.into(),
        scripts: spawn
            .map(|script| ("ScriptSpawn".to_string(), script.to_string()))
            .into_iter()
            .collect(),
        ..ObjectData::unknown(tag, ground_point(-4.0, 3.0, 0.0), 0.0)
    })
}

const MEMBER: &str = "p_member";
const LATE: &str = "n_late";
const LATE_SPAWN: &str = "k_late_spawn";
/// The door's local the save sets: numbers, index 9.
const LOCAL: (u8, i32, i32) = (1, 9, 42);

/// The save's objects by what they stand for, and its next id.
struct Before {
    gate: SimId,
    way_out: SimId,
    member: SimId,
    next_id: u32,
}

/// A save on `level`: its door open with a local, its creature in the
/// party.
fn save(level: &Level) -> Result<(Save, Before), String> {
    let mut world = SimWorld::new(crate::WorldConfig::one_seat(), level.clone())
        .map_err(|error| error.to_string())?;
    let mut save = Save {
        hz: crate::TICK_HZ,
        level: level.id(),
        snapshot: world.snapshot(),
        script: world.script_state().clone(),
    };
    let numbered = |number: u32| {
        save.snapshot
            .entities
            .iter()
            .find(|entity| matches!(entity.kind, Kind::Object(_, n) if n == number))
            .map(|entity| entity.id)
            .ok_or(format!("no object {number}"))
    };
    let before = Before {
        gate: numbered(0)?,
        way_out: numbered(1)?,
        member: numbered(2)?,
        next_id: save.snapshot.next_id,
    };
    for entity in &mut save.snapshot.entities {
        if let (true, Some(object)) = (entity.id == before.gate, entity.object.as_mut()) {
            object.state = 1;
        }
    }
    let (kind, index, value) = LOCAL;
    save.script
        .locals
        .insert((before.gate.0, kind, index), value);
    let party = &mut save.script.party;
    party.available.insert(
        0,
        Npc {
            template: MEMBER.into(),
            object: Some(before.member.0),
            kept: None,
            template_data: None,
        },
    );
    party.members.push(0);
    Ok((save, before))
}

/// How the changed module is reached.
#[derive(Clone, Copy)]
enum Path {
    /// The save loads in it.
    Load,
    /// The save loads in it as it was, leaves and comes back to it.
    Reenter,
}

/// What a report says but the ids it adds: trusted, moved, dropped,
/// added, snapped.
type Says = (bool, usize, &'static [&'static str], usize, usize);

fn says(report: &Report) -> (bool, usize, Vec<&str>, usize, usize) {
    let dropped = report.dropped.iter().map(String::as_str).collect();
    (
        report.trusted,
        report.moved,
        dropped,
        report.added.len(),
        report.snapped,
    )
}

/// What a case checks of the world it made, beyond its report.
type Holds = fn(&Before, &Snapshot, &State) -> bool;

struct Case {
    name: &'static str,
    path: Path,
    rostered: bool,
    change: fn(&mut Vec<Placement>),
    says: Says,
    holds: Holds,
}

/// The entity `id` in `snapshot`.
fn entity(snapshot: &Snapshot, id: SimId) -> Option<&EntityState> {
    snapshot.entities.iter().find(|entity| entity.id == id)
}

/// The entity numbered `number` of `kind` in `snapshot`.
fn numbered(snapshot: &Snapshot, kind: ObjectKind, number: u32) -> Option<&EntityState> {
    let want = Kind::Object(kind, number);
    snapshot.entities.iter().find(|entity| entity.kind == want)
}

/// The door, numbered `number`, still open with its local.
fn door_kept(before: &Before, snapshot: &Snapshot, state: &State, number: u32) -> bool {
    let (kind, index, value) = LOCAL;
    entity(snapshot, before.gate).is_some_and(|gate| {
        gate.kind == Kind::Object(ObjectKind::Door, number)
            && gate.object.is_some_and(|object| object.state == 1)
    }) && state.locals.get(&(before.gate.0, kind, index)) == Some(&value)
}

fn run(case: &Case) -> Result<(Before, SimWorld, Level), String> {
    let (base, changed) = (ground(|_| {}), ground(case.change));
    let (mut save, before) = save(&base)?;
    if !case.rostered {
        save.snapshot.roster = None;
    }
    let world = match case.path {
        Path::Load => save.world(&changed, 1, None)?,
        Path::Reenter => {
            let mut world = save.world(&base, 1, None)?;
            let elsewhere = crate::determinism::proving_ground_named("probe_elsewhere");
            for level in [elsewhere, changed.clone()] {
                world
                    .install(level, None)
                    .map_err(|error| error.to_string())?;
            }
            world
        }
    };
    Ok((before, world, changed))
}

/// Saves loaded, and a module left entered again, after its placements
/// changed: one case per rule of the design's 3c.
pub fn loads() -> Vec<Check> {
    let first = |placements: &mut Vec<Placement>| {
        let spot = ObjectData {
            kind: ObjectKind::Waypoint,
            ..ObjectData::unknown("wp_new", ground_point(-2.0, 0.0, 0.0), 0.0)
        };
        placements.insert(0, Placement::Own(spot));
    };
    let cases = [
        Case {
            name: "a placement inserted first: the door's state and local follow it",
            path: Path::Load,
            rostered: true,
            change: first,
            says: (false, 3, &[], 1, 0),
            holds: |before, snapshot, state| {
                door_kept(before, snapshot, state, 1)
                    && numbered(snapshot, ObjectKind::Waypoint, 0)
                        .is_some_and(|spot| spot.id.0 >= before.next_id)
            },
        },
        Case {
            name: "a placement inserted first, in a module left: its door follows",
            path: Path::Reenter,
            rostered: true,
            change: first,
            says: (false, 2, &[], 1, 0),
            holds: |before, snapshot, state| {
                door_kept(before, snapshot, state, 1)
                    && entity(snapshot, before.way_out)
                        .is_some_and(|out| out.kind == Kind::Object(ObjectKind::Trigger, 2))
            },
        },
        Case {
            name: "placements removed: the trigger dropped, the party's creature kept",
            path: Path::Load,
            rostered: true,
            change: |placements| {
                placements.truncate(1);
            },
            says: (false, 1, &["trigger way_out"], 0, 0),
            holds: |before, snapshot, state| {
                door_kept(before, snapshot, state, 0)
                    && entity(snapshot, before.way_out).is_none()
                    && entity(snapshot, before.member)
                        .is_some_and(|member| member.kind == Kind::Object(ObjectKind::Creature, 1))
                    && snapshot
                        .spawned
                        .last()
                        .is_some_and(|made| made.template == MEMBER)
            },
        },
        Case {
            name: "a placement new in the data: placed and spawned as on first arrival",
            path: Path::Load,
            rostered: true,
            change: |placements| placements.push(creature(LATE, Some(LATE_SPAWN))),
            says: (false, 0, &[], 1, 0),
            holds: |before, snapshot, state| {
                door_kept(before, snapshot, state, 0)
                    && numbered(snapshot, ObjectKind::Creature, 3).is_some_and(|late| {
                        late.id.0 >= before.next_id
                            && late.fighter.is_some()
                            && state.pending.iter().any(|pending| {
                                pending.script == LATE_SPAWN && pending.caller.0 == late.id.0
                            })
                    })
            },
        },
        Case {
            name: "a save without a roster, its data the same: trusted",
            path: Path::Load,
            rostered: false,
            change: |_| {},
            says: (true, 0, &[], 0, 0),
            holds: |before, snapshot, state| door_kept(before, snapshot, state, 0),
        },
        Case {
            name: "a save without a roster, a placement inserted first: other kinds dropped",
            path: Path::Load,
            rostered: false,
            change: first,
            says: (true, 0, &["door 0", "trigger 1", "creature 2"], 0, 0),
            holds: |before, snapshot, _| {
                entity(snapshot, before.gate).is_none()
                    && numbered(snapshot, ObjectKind::Waypoint, 0).is_none()
            },
        },
    ];
    cases
        .iter()
        .map(|case| match run(case) {
            Ok((before, mut world, changed)) => {
                let snapshot = world.snapshot();
                let report = world.reconciled();
                let (trusted, moved, dropped, added, snapped) = case.says;
                let ok = report.map(says)
                    == Some((trusted, moved, dropped.to_vec(), added, snapped))
                    && snapshot.roster.as_deref() == Some(&roster(changed.placed())[..])
                    && (case.holds)(&before, &snapshot, world.script_state());
                check(case.name, ok, format!("{report:?}"))
            }
            Err(error) => check(case.name, false, error),
        })
        .collect()
}
