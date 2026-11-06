//! Probes for `reconcile`: the number map over changed placement lists, the
//! roster's encoding, and saves loaded on the proving ground after its
//! placements changed.

use std::sync::Arc;

use script_ncs::probes::Check;

use crate::determinism::ground_point;
use crate::math::{Vec3, scale};
use crate::module::{ObjectData, ObjectKind, Placement};
use crate::reconcile::{Renumber, Report, decode, encode, onto_ground, renumber, roster};
use crate::script::actions::{Action, Queued};
use crate::script::party::Npc;
use crate::script::{Delayed, Library, State};
use crate::snapshot::{EntityState, Kind, SimId, Snapshot};
use crate::walk::on_mesh;
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
    out.extend(grounds());
    out
}

/// Where the waypoint of `mesh_ground` stands, in the room's frame.
const HOME: (f32, f32) = (-3.0, 0.0);

/// The proving ground with a waypoint at `HOME` and a navigation mesh of
/// half-metre squares over a 3 m square of its floor, x -7.5 to -4.5 and y
/// -2.5 to 0.5 in the room's frame; the rest of the floor is collision
/// alone.
fn mesh_ground() -> Level {
    const CELL: f32 = 0.5;
    let level = ground(|placements| {
        placements.push(Placement::Own(ObjectData {
            kind: ObjectKind::Waypoint,
            ..ObjectData::unknown("wp_home", ground_point(HOME.0, HOME.1, 0.0), 0.0)
        }));
    });
    let corner = |i: i32, j: i32| ground_point(-7.5 + i as f32 * CELL, -2.5 + j as f32 * CELL, 0.0);
    let triangles: Vec<[Vec3; 3]> = (0..6)
        .flat_map(|i| (0..6).map(move |j| (i, j)))
        .flat_map(|(i, j)| {
            let [a, b, c, d] =
                [(i, j), (i + 1, j), (i + 1, j + 1), (i, j + 1)].map(|(i, j)| corner(i, j));
            [[a, b, c], [a, c, d]]
        })
        .collect();
    let mut nav =
        collision::nav::NavMesh::build(&triangles, crate::walk::CLEARANCE * collision::UNIT);
    if let Some(floor) = level.collision().level() {
        nav.mark(floor);
    }
    level.with_nav(nav)
}

/// The origin (JA units) of a character standing at `feet` (metres).
fn origin_at(feet: Vec3) -> Vec3 {
    let [x, y, z] = scale(feet, 1.0 / collision::UNIT);
    [x, y, z - crate::motion::SOLE_Z]
}

/// Whether `entity` is an avatar or a creature, which `onto_ground` moves.
fn character(entity: &EntityState) -> bool {
    entity.player.is_some()
        && matches!(
            entity.kind,
            Kind::Avatar(_) | Kind::Object(ObjectKind::Creature, _)
        )
}

/// Where a body ends up.
#[derive(Clone, Copy, PartialEq)]
enum Ends {
    /// Untouched, bit for bit.
    Stays,
    /// Untouched, bit for bit, with a line saying it was left off the mesh.
    Left,
    /// On the mesh within `SNAP_M` of where it was, at rest.
    Nearby,
    /// At the module's nearest waypoint, at rest.
    Waypoint,
}

/// `onto_ground` on every character of a world, all saved falling at a spot
/// of the room, `lift` metres over its floor: on the mesh, off it but on
/// the floor, and off it in the air by a metre and by five; the player's
/// avatar ends as the first `Ends` says, the creatures as the second, of
/// the party or not as the row says.
fn grounds() -> Vec<Check> {
    let cases = [
        (
            "a body on the mesh stays",
            (-6.0, -1.0),
            0.0,
            (Ends::Stays, Ends::Stays),
            false,
        ),
        (
            "a body off the mesh on the floor stays",
            (-3.5, -1.0),
            0.0,
            (Ends::Stays, Ends::Stays),
            false,
        ),
        (
            "a body falling 1 m off the mesh: onto it, at rest",
            (-3.5, -1.0),
            2.0,
            (Ends::Nearby, Ends::Nearby),
            false,
        ),
        (
            "a body falling 5 m off the mesh: the player to the waypoint, at rest; a creature not of the party left as it was",
            (-4.0, 5.5),
            2.0,
            (Ends::Waypoint, Ends::Left),
            false,
        ),
        (
            "a party member falling 5 m off the mesh: to the waypoint, at rest",
            (-4.0, 5.5),
            2.0,
            (Ends::Waypoint, Ends::Waypoint),
            true,
        ),
    ];
    cases
        .into_iter()
        .map(|(name, (x, y), lift, (avatar, creature), in_party)| {
            let level = mesh_ground();
            let Ok(mut world) = SimWorld::new(crate::WorldConfig::one_seat(), level.clone()) else {
                return check(name, false, "no world".into());
            };
            let mut entities = world.snapshot().entities;
            let origin = origin_at(ground_point(x, y, lift));
            for body in entities.iter_mut().filter(|body| character(body)) {
                body.position = scale(origin, collision::UNIT);
                body.velocity = [0.0, 0.0, -3.0];
                if let Some(player) = body.player.as_mut() {
                    player.origin = origin;
                    player.velocity = [0.0, 0.0, -120.0];
                    player.in_air_anim = true;
                    player.hold_ms = 250;
                    player.flags |= crate::motion::motion_flags::KNOCKBACK_HOLD;
                }
            }
            let members: Vec<(u32, String)> = entities
                .iter()
                .filter(|body| in_party && matches!(body.kind, Kind::Object(..)) && character(body))
                .map(|body| (body.id.0, MEMBER.to_string()))
                .collect();
            let ends = |body: &EntityState| match body.kind {
                Kind::Avatar(_) => avatar,
                _ => creature,
            };
            let before = entities.clone();
            let grounding = onto_ground(&level, &mut entities, &members);
            let lines = &grounding.lines;
            let bodies = entities.iter().filter(|body| character(body)).count();
            let creatures = bodies
                - entities
                    .iter()
                    .filter(|body| matches!(body.kind, Kind::Avatar(_)))
                    .count();
            let home = origin_at(ground_point(HOME.0, HOME.1, 0.0));
            let placed_alike = before.iter().zip(&entities).all(|(was, now)| {
                character(was) || (was.position == now.position && was.player == now.player)
            });
            let counted = |wanted: fn(Ends) -> bool| {
                before
                    .iter()
                    .filter(|body| character(body) && wanted(ends(body)))
                    .count()
            };
            let moved = counted(|ends| matches!(ends, Ends::Nearby | Ends::Waypoint));
            let left = counted(|ends| ends == Ends::Left);
            let ok = creatures >= 1
                && placed_alike
                && entities
                    .iter()
                    .zip(&before)
                    .filter(|(body, _)| character(body))
                    .all(|(body, was)| {
                        let Some(player) = body.player else {
                            return false;
                        };
                        let flat = collision::nav::flat_length(
                            scale(player.origin, collision::UNIT),
                            scale(origin, collision::UNIT),
                        );
                        let at_rest = body.velocity == [0.0; 3]
                            && player.velocity == [0.0; 3]
                            && player.ground.is_none()
                            && !player.in_air_anim
                            && player.hold_ms == 0
                            && player.flags & crate::motion::motion_flags::KNOCKBACK_HOLD == 0;
                        let named = |says: &str| {
                            let entity = format!("(entity {}) {says}", body.id.0);
                            lines.iter().any(|line| line.contains(&entity))
                        };
                        body.position == scale(player.origin, collision::UNIT)
                            && match ends(body) {
                                Ends::Stays => body == was,
                                Ends::Left => body == was && named("left off the mesh"),
                                Ends::Nearby => {
                                    at_rest && on_mesh(&level, player.origin) && flat <= 2.0
                                }
                                Ends::Waypoint => at_rest && player.origin == home,
                            }
                    })
                && grounding.moved == moved
                && lines.len() == moved + left;
            check(name, ok, format!("{lines:?}"))
        })
        .collect()
}

/// The proving ground, with a creature of the party's and a footlocker
/// after its door and trigger, and its placements as `change` leaves them.
fn ground(change: fn(&mut Vec<Placement>)) -> Level {
    let ground = crate::determinism::proving_ground();
    let mut data = (**ground.module().expect("the proving ground is a module")).clone();
    data.placements.push(creature(MEMBER, None));
    data.placements.push(Placement::Own(ObjectData {
        kind: ObjectKind::Placeable,
        tag: "locker".into(),
        container: true,
        ..ObjectData::unknown(LOCKER, ground_point(-6.0, -4.0, 0.0), 0.0)
    }));
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
const LOCKER: &str = "plc_footlker";
/// The item the party stored in the footlocker.
const STORED: u32 = crate::inventory::FIRST_ITEM + 100;
const LATE: &str = "n_late";
const LATE_SPAWN: &str = "k_late_spawn";
/// The door's local the save sets: numbers, index 9.
const LOCAL: (u8, i32, i32) = (1, 9, 42);

/// The save's objects by what they stand for, and its next id.
struct Before {
    gate: SimId,
    way_out: SimId,
    member: SimId,
    locker: SimId,
    next_id: u32,
}

/// A save on `level`: its door open with a local, its creature in the
/// party, an item of the party's stored in its footlocker.
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
        locker: numbered(3)?,
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
    save.script.items.insert(
        STORED,
        crate::inventory::Item {
            template: "g_w_blaster01".into(),
            holder: before.locker.0,
            slot: None,
            stack: 1,
            charges: 0,
            droppable: true,
            plot: false,
            identified: true,
            infinite: false,
            non_equippable: false,
            upgrades: 0,
            fitted: Vec::new(),
        },
    );
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

/// The entity `id` kept as the last object made in play: a `kind`
/// numbered `number`, made from `template`.
fn kept_as_made(
    snapshot: &Snapshot,
    id: SimId,
    kind: ObjectKind,
    number: u32,
    template: &str,
) -> bool {
    entity(snapshot, id).is_some_and(|kept| kept.kind == Kind::Object(kind, number))
        && snapshot
            .spawned
            .last()
            .is_some_and(|made| made.kind == kind && made.template == template)
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
            says: (false, 4, &[], 1, 0),
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
            says: (false, 3, &[], 1, 0),
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
                placements.drain(1..3);
            },
            says: (false, 1, &["trigger way_out"], 0, 0),
            holds: |before, snapshot, state| {
                door_kept(before, snapshot, state, 0)
                    && entity(snapshot, before.way_out).is_none()
                    && kept_as_made(snapshot, before.member, ObjectKind::Creature, 2, MEMBER)
            },
        },
        Case {
            name: "a footlocker gone from the data, holding the party's items: kept with them",
            path: Path::Load,
            rostered: true,
            change: |placements| {
                placements.remove(3);
                placements.remove(1);
            },
            says: (false, 2, &["trigger way_out"], 0, 0),
            holds: |before, snapshot, state| {
                kept_as_made(snapshot, before.locker, ObjectKind::Placeable, 2, LOCKER)
                    && state
                        .items
                        .held_by(before.locker.0)
                        .any(|(item, _)| item == STORED)
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
                    && numbered(snapshot, ObjectKind::Creature, 4).is_some_and(|late| {
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
            name: "a save without a roster, a placement inserted first: other kinds dropped, the party's creature kept",
            path: Path::Load,
            rostered: false,
            change: first,
            says: (true, 1, &["door 0", "trigger 1", "placeable 3"], 0, 0),
            holds: |before, snapshot, _| {
                entity(snapshot, before.gate).is_none()
                    && numbered(snapshot, ObjectKind::Waypoint, 0).is_none()
                    && kept_as_made(snapshot, before.member, ObjectKind::Creature, 5, MEMBER)
            },
        },
    ];
    let placements = cases.iter().map(|case| match run(case) {
        Ok((before, mut world, changed)) => {
            let snapshot = world.snapshot();
            let report = world.reconciled();
            let (trusted, moved, dropped, added, snapped) = case.says;
            let ok = report.map(says) == Some((trusted, moved, dropped.to_vec(), added, snapped))
                && snapshot.roster.as_deref() == Some(&roster(changed.placed())[..])
                && (case.holds)(&before, &snapshot, world.script_state());
            check(case.name, ok, format!("{report:?}"))
        }
        Err(error) => check(case.name, false, error),
    });
    placements.chain(deferred_code()).collect()
}

/// The script every deferred continuation of the code cases stems from.
const CODE_SCRIPT: &str = "k_deferred";

/// A library holding `CODE_SCRIPT` of `length` instructions: another length,
/// other bytes.
fn scripted(length: usize) -> Library {
    let mut asm = script_ncs::probes::Asm::default();
    for _ in 0..length {
        asm.op(fmt_odyssey::ncs::Op::Retn);
    }
    let routines = script_ncs::catalog::Catalog::parse("").expect("an empty catalog parses");
    let programs = std::collections::BTreeMap::from([(CODE_SCRIPT.to_string(), asm.build())]);
    Library::new(routines, programs, Default::default())
}

/// Where a save keeps deferred code.
#[derive(Clone, Copy)]
enum Waits {
    /// `Delayed`, in the state.
    Delayed,
    /// A queued `Action::Code`.
    Queued,
    /// A `Delayed` of a module left, entered again.
    Left,
}

/// The script digest a continuation was saved with.
#[derive(Clone, Copy)]
enum Saved {
    /// Saved before digests were kept.
    Nothing,
    /// The digest of the script as it is now.
    Current,
    /// The digest of the script as it was.
    Changed,
    /// The digest of a script the level lacks: code carried from another
    /// module.
    Absent,
}

/// `level` with its module named in capitals, as KOTOR's module names
/// often are while the scripts' state keys them in lower case.
fn capitals(level: Level) -> Level {
    let mut data = (**level.module().expect("the proving ground is a module")).clone();
    data.name = data.name.to_uppercase();
    level.with_module(data)
}

/// The save's continuation of `CODE_SCRIPT`, as a `Delayed` carrying `code`.
fn continuation(code: Option<[u8; 32]>) -> Delayed {
    Delayed {
        due_ms: 500,
        order: 0,
        caller: script_ncs::vm::ObjectId(1),
        script: CODE_SCRIPT.into(),
        deferred: script_ncs::vm::Deferred {
            resume: 16,
            globals: Vec::new(),
            locals: Vec::new(),
        },
        spell: None,
        talk: None,
        cue: None,
        run: None,
        ai: false,
        code,
    }
}

/// Loads a save holding deferred code of `CODE_SCRIPT` in `waits`, saved
/// with `saved`, into a level whose script is `scripted(1)` (none for
/// `Saved::Absent`); its module named in capitals.
fn load_with_code(waits: Waits, saved: Saved) -> Result<SimWorld, String> {
    let code = match saved {
        Saved::Nothing => None,
        Saved::Current | Saved::Absent => scripted(1).script_digest(CODE_SCRIPT),
        Saved::Changed => scripted(2).script_digest(CODE_SCRIPT),
    };
    let base = capitals(ground(|_| {}));
    let changed = match saved {
        Saved::Absent => base.clone(),
        _ => base.clone().with_scripts(scripted(1)),
    };
    let (mut save, before) = save(&base)?;
    let held = continuation(code);
    match waits {
        Waits::Delayed => save.script.delayed.push(held),
        Waits::Queued => {
            let Delayed {
                script, deferred, ..
            } = held;
            let action = Action::Code {
                script,
                deferred,
                ai: false,
                code,
            };
            let queued = Queued {
                action,
                started_ms: None,
            };
            save.script
                .actions
                .entry(before.gate.0)
                .or_default()
                .push(queued);
        }
        Waits::Left => {
            let mut world = save.world(&base, 1, None)?;
            let module = base.place().ok_or("the ground is a module")?.module;
            let elsewhere = crate::determinism::proving_ground_named("probe_elsewhere");
            world
                .install(elsewhere, None)
                .map_err(|error| error.to_string())?;
            let mut state = world.script_state().clone();
            // Keyed as `State::leave_module` keys it.
            let key = crate::script::key(&module);
            state.left.entry(key).or_default().delayed.push(held);
            world.set_script_state(state);
            world
                .install(changed, None)
                .map_err(|error| error.to_string())?;
            return Ok(world);
        }
    }
    save.world(&changed, 1, None)
}

/// Whether `world` keeps none of `CODE_SCRIPT`'s code, waiting or queued,
/// and logs one line for each continuation dropped.
fn code_dropped(world: &SimWorld) -> bool {
    let state = world.script_state();
    let queued = state.actions.values().flatten().any(
        |queued| matches!(&queued.action, Action::Code { script, .. } if script == CODE_SCRIPT),
    );
    let waiting = state
        .delayed
        .iter()
        .any(|delayed| delayed.script == CODE_SCRIPT);
    let lines = world.dropped_code();
    !queued && !waiting && lines.len() == 1 && lines[0].contains(CODE_SCRIPT)
}

/// Saved continuations of a script that changed (#181) or that the level
/// lacks load without them, each logged; those of the same bytes, and those
/// saved before digests were kept, load with them. A module left is found
/// whatever the case of its name.
fn deferred_code() -> Vec<Check> {
    let rows = [
        (
            "deferred code of a changed script",
            Waits::Delayed,
            Saved::Changed,
        ),
        (
            "deferred code of the same script",
            Waits::Delayed,
            Saved::Current,
        ),
        (
            "deferred code saved before digests",
            Waits::Delayed,
            Saved::Nothing,
        ),
        (
            "deferred code of a script the level lacks",
            Waits::Delayed,
            Saved::Absent,
        ),
        (
            "queued code of a changed script",
            Waits::Queued,
            Saved::Changed,
        ),
        (
            "queued code of the same script",
            Waits::Queued,
            Saved::Current,
        ),
        (
            "queued code of a script the level lacks",
            Waits::Queued,
            Saved::Absent,
        ),
        (
            "a module left, its code changed",
            Waits::Left,
            Saved::Changed,
        ),
        (
            "a module left, its code the same",
            Waits::Left,
            Saved::Current,
        ),
        ("a module left, its script gone", Waits::Left, Saved::Absent),
    ];
    rows.into_iter()
        .map(|(name, waits, saved)| {
            let outcome = load_with_code(waits, saved);
            let ok = match (&outcome, saved) {
                (Ok(world), Saved::Changed | Saved::Absent) => code_dropped(world),
                (Ok(world), _) => world.dropped_code().is_empty(),
                (Err(_), _) => false,
            };
            let seen = outcome.map(|world| world.dropped_code().to_vec());
            check(name, ok, format!("{seen:?}"))
        })
        .collect()
}
