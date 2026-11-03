//! A save against a module whose placements changed (`Snapshot::roster`).
//!
//! An object's number is its place in the module's list: the level's
//! placements in GIT order, then what play made (`crate::instance`). A data
//! change that adds or drops a placement moves every number after it, so a
//! save made before it points its entities, scripts and spawned objects at
//! the wrong ones. The roster is what the placements were when the module
//! was first installed; `renumber` says where each of them stands now, and
//! `reconcile` brings the save's objects onto the new numbers as it loads
//! (`Save::world`) and as a module left behind is entered again
//! (`install::assemble`).
//!
//! What holds an object's number, and what `apply` does with it:
//! - `EntityState::kind`, `Kind::Object(kind, number)`, of the snapshot's
//!   entities and of each module left (`Stashed::entities`): rewritten. A
//!   placement the data no longer has drops its entity, but for a party
//!   member's and one holding the party's items (a footlocker the player
//!   stored gear in), which stay where they stand as objects made in play.
//!   A save without a roster keeps its numbers, but for those naming an
//!   object of another kind now: dropped, a party member's kept likewise.
//! - `Snapshot::spawned` and `Stashed::spawned`, what play made: numbered
//!   after the placements in their order, so their numbers move with the
//!   placements' count (`Renumber::made_shift`); the list itself holds no
//!   number. A party member kept is added to its end.
//! - The world's own tables, made again from the two at every load and
//!   install: `world::Object::index`, `Instances`, the collision's door
//!   slots (`Level::states`). Nothing to rewrite.
//!
//! When the data moved (`Held::snap`), `onto_ground` puts characters the
//! collision no longer holds up back on the walkable mesh, for a save with a
//! roster and one without alike; a creature not of the party further off
//! stays where it is.
//!
//! Everything else names an object by its entity's `SimId`, which a script
//! sees as its `ObjectId` (`ObjectId(entity.id.0)`): a door's open state and
//! its lock (`ObjectState`, on the entity itself), the scripts' locals,
//! `last` (opener, user, damager, killer), action queues, deferred code and
//! events and their callers, `uncommandable`, a trigger's occupants, what
//! each creature perceives, rounds, what a module left keeps waiting
//! (`script::Left`), conversations' owners and speakers, the party table's
//! creatures (`Npc::object`), items' holders (a container's contents), the
//! scripts' visuals on objects (`OnObject`), a transition's door
//! (`Transition::via`) and the party's leader (`Snapshot::leading`). They
//! follow their objects as the entities keep their ids, so `apply` leaves
//! them; a dropped entity's are left as a destroyed object's are (its queue
//! goes at the next step, its locals and goods are not read again).

use std::collections::{BTreeMap, HashMap, HashSet};
use std::fmt;
use std::sync::Arc;

use collision::UNIT;

use crate::instance::Instances;
use crate::inventory::Items;
use crate::math::{self, Vec3};
use crate::module::{ObjectData, ObjectKind};
use crate::motion::{self, MotionState, motion_flags};
use crate::script::actions::{Action, Queued};
use crate::script::{self, Delayed, Library};
use crate::snapshot::{EntityState, Kind, SimId, Spawned};
use crate::wire::{DecodeError, Reader, Writer};
use crate::world::Level;

/// What a placement was, as the roster keeps it: enough to find it again in
/// a changed list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Origin {
    pub kind: ObjectKind,
    pub template: String,
    pub tag: String,
}

/// The origins of a module's placements, in their order.
pub fn roster(placed: &[Arc<ObjectData>]) -> Vec<Origin> {
    placed
        .iter()
        .map(|object| Origin {
            kind: object.kind,
            template: object.template.clone(),
            tag: object.tag.clone(),
        })
        .collect()
}

/// Where the objects of an old numbering stand in the new one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Renumber {
    /// The new number of each old placement, by its old number; `None`
    /// where the data no longer has it.
    pub map: Vec<Option<u32>>,
    /// What an object made in play, numbered `old >= map.len()`, is
    /// numbered now: `old + made_shift`.
    pub made_shift: i64,
    /// The new placements the old roster did not have, ascending.
    pub added: Vec<u32>,
}

/// A placement's identity: kind, template and tag without case.
fn key(kind: ObjectKind, template: &str, tag: &str) -> (ObjectKind, String, String) {
    (kind, template.to_lowercase(), tag.to_lowercase())
}

/// Matches the old placements to the new: the n-th of a key in `old` is the
/// n-th of it in `new`, so two alike keep their order.
pub fn renumber(old: &[Origin], new: &[Arc<ObjectData>]) -> Renumber {
    let mut found: HashMap<_, Vec<u32>> = HashMap::new();
    for (at, object) in new.iter().enumerate() {
        found
            .entry(key(object.kind, &object.template, &object.tag))
            .or_default()
            .push(at as u32);
    }
    let mut seen: HashMap<_, usize> = HashMap::new();
    let map: Vec<Option<u32>> = old
        .iter()
        .map(|origin| {
            let key = key(origin.kind, &origin.template, &origin.tag);
            let nth = seen.entry(key.clone()).or_default();
            let matched = found.get(&key).and_then(|at| at.get(*nth)).copied();
            *nth += 1;
            matched
        })
        .collect();
    let taken: HashSet<u32> = map.iter().flatten().copied().collect();
    Renumber {
        added: (0..new.len() as u32)
            .filter(|at| !taken.contains(at))
            .collect(),
        made_shift: new.len() as i64 - old.len() as i64,
        map,
    }
}

/// A module's saved objects, as a snapshot or a module left holds them.
pub struct Held<'a> {
    pub entities: &'a mut Vec<EntityState>,
    pub spawned: &'a mut Vec<Spawned>,
    pub roster: &'a mut Option<Vec<Origin>>,
    /// The world's next id, which the new placements' entities take.
    pub next_id: &'a mut u32,
    /// Whether the level's data moved since the objects were saved
    /// (`Restamp`), which is when bodies it left unsupported are put back
    /// on the ground (`onto_ground`).
    pub snap: bool,
}

/// What reconciling a module's saved objects did: one `save-migrate` line.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Report {
    pub module: String,
    /// Saved before rosters were kept: its numbers are trusted, checked
    /// only for their kind.
    pub trusted: bool,
    /// The entities whose number changed.
    pub moved: usize,
    /// What the dropped entities stood for.
    pub dropped: Vec<String>,
    /// The entities of the placements new in the data, which come alive as
    /// on a first arrival (`SimWorld::reconciled_with`).
    pub added: Vec<SimId>,
    /// The entities moved onto walkable ground.
    pub snapped: usize,
    /// One line for each of them, who and from where to where, and for each
    /// creature left off the mesh (`onto_ground`).
    pub snaps: Vec<String>,
}

impl fmt::Display for Report {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.trusted {
            write!(
                f,
                "{}: no placements kept with the save, its numbers trusted",
                self.module
            )?;
        } else {
            write!(
                f,
                "{}: placements changed since the save: {} moved, {} added, {} snapped",
                self.module,
                self.moved,
                self.added.len(),
                self.snapped
            )?;
        }
        write!(f, ", {} dropped", self.dropped.len())?;
        if !self.dropped.is_empty() {
            write!(f, " ({})", self.dropped.join(", "))?;
        }
        Ok(())
    }
}

/// The party, as reconciling keeps what is its: its members' creatures,
/// each with its template (`install::members`), and every item in play,
/// to find what it put away.
pub struct Party<'a> {
    pub members: &'a [(u32, String)],
    pub items: &'a Items,
}

impl Party<'_> {
    /// The template of the member whose creature is `id`.
    fn member(&self, id: SimId) -> Option<&str> {
        self.members
            .iter()
            .find(|(member, _)| *member == id.0)
            .map(|(_, template)| template.as_str())
    }

    /// Whether `entity`, made from `template`, holds anything its
    /// template's own inventory does not have. Nothing marks an item the
    /// party put into a container (`State::put_in` moves its holder), so an
    /// item that is not among the template's goods is taken for the
    /// party's; what a script gave it counts too, which keeps a little more
    /// than needed rather than lose the party's gear.
    fn stored_in(
        &self,
        entity: &EntityState,
        kind: ObjectKind,
        template: &str,
        level: &Level,
    ) -> bool {
        let facts = level.template(kind, template);
        let own: Vec<String> = facts
            .iter()
            .flat_map(|facts| &facts.inventory)
            .map(|held| held.template.to_lowercase())
            .collect();
        self.items
            .held_by(entity.id.0)
            .any(|(_, item)| !own.contains(&item.template.to_lowercase()))
    }
}

/// `held` brought onto `level`'s placements: renumbered by its roster
/// (`apply`), else its numbers trusted where they name an object of their
/// kind (`trust`). Its roster is `level`'s from then on.
pub fn reconcile(mut held: Held, level: &Level, party: &Party) -> Report {
    let placed = level.placed();
    let mut report = match held.roster.take() {
        Some(old) => apply(&renumber(&old, placed), &old, level, &mut held, party),
        None => trust(level, &mut held, party),
    };
    if held.snap {
        // What `apply` just placed stands as a first install stands it.
        let settled = held.entities.len() - report.added.len();
        let grounding = onto_ground(level, &mut held.entities[..settled], party.members);
        report.snapped = grounding.moved;
        report.snaps = grounding.lines;
    }
    report.module = level.id().name;
    *held.roster = Some(roster(placed));
    report
}

/// How far (metres) a body off the walkable mesh is moved to the nearest
/// walkable point; a player or party member further off goes to the
/// module's nearest waypoint (spec 3c of the save design).
const SNAP_M: f32 = 2.0;

/// What `onto_ground` did: one line for each character it moved or left
/// off the mesh, and how many it moved.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Grounding {
    pub moved: usize,
    pub lines: Vec<String>,
}

/// The characters of `entities` (avatars and creatures) the level's
/// collision no longer holds up (`Level::held_up`: in the air or in a wall,
/// as a change in the nav or the collision left them) put back on the
/// walkable mesh, and at rest: onto `walk::back_on`'s nearest walkable point
/// when it lies within `SNAP_M`, else, for an avatar or a creature of the
/// party's `members` (`install::members`), onto the nearest waypoint of the
/// module's placements (none: where it stands). Any other creature further
/// off stays where it is: it stands where the game's own scripts put it,
/// while a stranded player or companion must be recoverable. The mesh says
/// only where, never whether: a body off it but held up (on a bed, a
/// placeable, or in a place with no mesh) stays.
pub fn onto_ground(
    level: &Level,
    entities: &mut [EntityState],
    members: &[(u32, String)],
) -> Grounding {
    let mut grounding = Grounding::default();
    for entity in entities {
        let (who, recovered) = match entity.kind {
            Kind::Avatar(seat) => (format!("avatar {}", seat.0), true),
            Kind::Object(ObjectKind::Creature, number) => (
                format!("creature {number}"),
                members.iter().any(|(member, _)| *member == entity.id.0),
            ),
            _ => continue,
        };
        let Some(player) = entity.player.as_mut() else {
            continue;
        };
        if level.held_up(player.origin) || crate::walk::on_mesh(level, player.origin) {
            continue;
        }
        let from = entity.position;
        let near = crate::walk::back_on(level, player.origin)
            .filter(|&to| flat_m(to, player.origin) <= SNAP_M);
        let Some(to) = near.or_else(|| {
            recovered
                .then(|| nearest_waypoint(level, player.origin))
                .flatten()
        }) else {
            if !recovered {
                grounding.lines.push(format!(
                    "{who} (entity {}) left off the mesh, more than {SNAP_M} m from it: ({:.2}, {:.2}, {:.2}) m",
                    entity.id.0, from[0], from[1], from[2],
                ));
            }
            continue;
        };
        grounding.lines.push(format!(
            "{who} (entity {}) snapped onto the ground: ({:.2}, {:.2}, {:.2}) to ({:.2}, {:.2}, {:.2}) m",
            entity.id.0,
            from[0],
            from[1],
            from[2],
            to[0] * UNIT,
            to[1] * UNIT,
            to[2] * UNIT,
        ));
        grounding.moved += 1;
        let rest = MotionState::standing(to, 0.0);
        player.origin = to;
        player.velocity = rest.velocity;
        player.ground = rest.ground;
        player.in_air_anim = rest.in_air_anim;
        player.hold_ms = rest.hold_ms;
        player.flags &= !(motion_flags::LAND_HOLD | motion_flags::KNOCKBACK_HOLD);
        entity.position = math::scale(to, UNIT);
        entity.velocity = [0.0; 3];
    }
    grounding
}

/// The distance (metres, seen from above) between two origins (units).
fn flat_m(a: Vec3, b: Vec3) -> f32 {
    collision::nav::flat_length(math::scale(a, UNIT), math::scale(b, UNIT))
}

/// The origin (units) of a character standing on the waypoint of `level`'s
/// placements nearest `from` (an origin, units).
fn nearest_waypoint(level: &Level, from: Vec3) -> Option<Vec3> {
    level
        .placed()
        .iter()
        .filter(|object| object.kind == ObjectKind::Waypoint)
        .map(|object| {
            let [x, y, z] = math::scale(object.position, 1.0 / UNIT);
            [x, y, z - motion::SOLE_Z]
        })
        .min_by(|&a, &b| flat_m(a, from).total_cmp(&flat_m(b, from)))
}

/// `held`'s objects numbered as `renumber` says (`old` the roster it was
/// made from): an entity whose placement is gone is dropped, but for a
/// party member's and one holding the party's items (`Party::stored_in`),
/// which stay where they stand as objects made in play (`made_in_play`);
/// an object of a placement that is not a creature stands where its
/// placement does; the new placements stand as the module's first install
/// stands them (`install::anew`).
pub fn apply(
    renumber: &Renumber,
    old: &[Origin],
    level: &Level,
    held: &mut Held,
    party: &Party,
) -> Report {
    let mut report = Report::default();
    for mut entity in std::mem::take(held.entities) {
        if let Kind::Object(kind, number) = entity.kind {
            let now = match renumber.map.get(number as usize) {
                Some(&Some(now)) => now,
                Some(None) => {
                    let origin = &old[number as usize];
                    let kept = party.member(entity.id).is_some()
                        || party.stored_in(&entity, kind, &origin.template, level);
                    if !kept {
                        report.dropped.push(describe(origin));
                        continue;
                    }
                    made_in_play(&entity, kind, &origin.template, level, held.spawned)
                }
                None => (i64::from(number) + renumber.made_shift) as u32,
            };
            report.moved += usize::from(now != number);
            entity.kind = Kind::Object(kind, now);
            // An object of a placement, not a creature, stands where the
            // placement does: no script of either game moves one, and a
            // stash may know it only by its id and state (a story
            // restore's, `saves::Story::restore`), not where it stood.
            if let Some(facts) = level.placed().get(now as usize)
                && kind != ObjectKind::Creature
            {
                entity.position = facts.position;
            }
        }
        held.entities.push(entity);
    }
    let instances = Instances::of(level, held.spawned);
    let anew = crate::install::anew(
        level,
        &instances,
        &renumber.added,
        held.next_id,
        held.entities,
    );
    report.added = anew.iter().map(|entity| entity.id).collect();
    held.entities.extend(anew);
    report
}

/// `held`'s objects whose number names no object of their kind in `level`
/// dropped, but for a party member's, which stays as an object made in
/// play from the party table's template; the rest trusted, as a save from
/// before rosters were kept gives nothing to match them by.
fn trust(level: &Level, held: &mut Held, party: &Party) -> Report {
    let instances = Instances::of(level, held.spawned);
    let mut report = Report {
        trusted: true,
        ..Report::default()
    };
    for mut entity in std::mem::take(held.entities) {
        if let Kind::Object(kind, number) = entity.kind
            && instances.kind(number) != Some(kind)
        {
            let Some(template) = party.member(entity.id) else {
                report.dropped.push(format!("{} {number}", kind.name()));
                continue;
            };
            let now = made_in_play(&entity, kind, template, level, held.spawned);
            report.moved += 1;
            entity.kind = Kind::Object(kind, now);
        }
        held.entities.push(entity);
    }
    report
}

/// `entity`, a `kind` made from `template`, kept as an object made in
/// play where it stands: added to `spawned`, its new number.
fn made_in_play(
    entity: &EntityState,
    kind: ObjectKind,
    template: &str,
    level: &Level,
    spawned: &mut Vec<Spawned>,
) -> u32 {
    let number = (level.placed().len() + spawned.len()) as u32;
    spawned.push(Spawned {
        kind,
        template: template.to_string(),
        yaw: entity.player.map_or(0.0, |player| player.view[1]),
        holds: Vec::new(),
    });
    number
}

/// The scripts whose saved continuations (`Delayed`, queued
/// `Action::Code`) were deferred from other bytes than `library`'s now: code
/// points into a compiled script, so one whose script changed cannot be
/// resumed. Continuations saved before digests were kept are trusted, and
/// so is one whose script the library lacks (`drop_absent_code`). Sorted,
/// each once.
pub fn stale_code(library: &Library, script: &script::State) -> Vec<String> {
    stale(library, &script.delayed, &script.actions)
}

/// `stale_code` of what the scripts keep waiting in `module`, a module the
/// party left (`script::State::left`).
pub fn stale_left_code(library: &Library, script: &script::State, module: &str) -> Vec<String> {
    script
        .left
        .get(&script::key(module))
        .map_or_else(Vec::new, |left| {
            stale(library, &left.delayed, &left.actions)
        })
}

/// Drops the saved continuations whose script `library` lacks, the code
/// the party carried from the module it came from (`State::leave_module`
/// keeps the party's own): run, it would do nothing, as no program is
/// found. One line for each, naming `module` and the script.
pub fn drop_absent_code(
    library: &Library,
    script: &mut script::State,
    module: &str,
) -> Vec<String> {
    absent(library, &mut script.delayed, &mut script.actions, module)
}

/// `drop_absent_code` of what the scripts keep waiting in `module`, a
/// module the party left.
pub fn drop_absent_left_code(
    library: &Library,
    script: &mut script::State,
    module: &str,
) -> Vec<String> {
    script
        .left
        .get_mut(&script::key(module))
        .map_or_else(Vec::new, |left| {
            absent(library, &mut left.delayed, &mut left.actions, module)
        })
}

/// Whether code `code` deferred from `script` is saved with a digest but
/// `library` holds no such script.
fn lacks(library: &Library, script: &str, code: Option<[u8; 32]>) -> bool {
    code.is_some() && library.script_digest(script).is_none()
}

fn absent(
    library: &Library,
    delayed: &mut Vec<Delayed>,
    actions: &mut BTreeMap<u32, Vec<Queued>>,
    module: &str,
) -> Vec<String> {
    let mut dropped = Vec::new();
    let mut drop_if = |script: &str, code: Option<[u8; 32]>| {
        let gone = lacks(library, script, code);
        if gone {
            dropped.push(format!(
                "{module}: deferred code of {script} dropped, a script the module lacks"
            ));
        }
        !gone
    };
    delayed.retain(|delayed| drop_if(&delayed.script, delayed.code));
    // A queue left empty by its code dropped goes too.
    actions.retain(|_, queue| {
        let held = queue.len();
        queue.retain(|queued| match &queued.action {
            Action::Code { script, code, .. } => drop_if(script, *code),
            _ => true,
        });
        held == 0 || !queue.is_empty()
    });
    dropped
}

fn stale(
    library: &Library,
    delayed: &[Delayed],
    actions: &BTreeMap<u32, Vec<Queued>>,
) -> Vec<String> {
    let queued = actions
        .values()
        .flatten()
        .filter_map(|queued| match &queued.action {
            Action::Code { script, code, .. } => Some((script, *code)),
            _ => None,
        });
    let mut stale: Vec<String> = delayed
        .iter()
        .map(|delayed| (&delayed.script, delayed.code))
        .chain(queued)
        .filter(|(script, code)| {
            code.is_some_and(|code| library.script_digest(script).is_some_and(|now| now != code))
        })
        .map(|(script, _)| script.clone())
        .collect();
    stale.sort();
    stale.dedup();
    stale
}

/// A placement as the log names it: its kind, template and tag.
fn describe(origin: &Origin) -> String {
    let names: Vec<&str> = [origin.template.as_str(), origin.tag.as_str()]
        .into_iter()
        .filter(|name| !name.is_empty())
        .collect();
    format!("{} {}", origin.kind.name(), names.join(" "))
}

/// Writes a roster, or its absence.
pub(crate) fn encode(roster: Option<&[Origin]>, out: &mut Writer) {
    let Some(roster) = roster else {
        out.u8(0);
        return;
    };
    out.u8(1);
    out.u32(roster.len() as u32);
    for origin in roster {
        out.u8(origin.kind.index());
        out.string(&origin.template);
        out.string(&origin.tag);
    }
}

/// Reads what `encode` wrote.
pub(crate) fn decode(input: &mut Reader) -> Result<Option<Vec<Origin>>, DecodeError> {
    let at = input.at();
    match input.u8()? {
        0 => Ok(None),
        1 => (0..input.u32()?)
            .map(|_| {
                let at = input.at();
                let kind = ObjectKind::ALL
                    .get(usize::from(input.u8()?))
                    .copied()
                    .ok_or(input.invalid("roster kind", at))?;
                Ok(Origin {
                    kind,
                    template: input.string()?,
                    tag: input.string()?,
                })
            })
            .collect::<Result<_, DecodeError>>()
            .map(Some),
        _ => Err(input.invalid("roster flag", at)),
    }
}
