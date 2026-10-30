//! What the player is looking at, each frame in play (`sim::target`): the
//! crosshair's ray from the camera (`play_view::Aim`), the presented
//! snapshot's targetable objects, the level's collision with the presented
//! doors. No target while a screen or a conversation is up. It fills the
//! HUD's target block (`ui::vm::TargetView`: KOTOR's own icons and words,
//! Open 365, Use 366, Dialog 371, Security 255) and the crosshair; the
//! mouse wheel moves the chosen action. The dumps carry it (`target`).
//!
//! The use key (E; the console's `ui act`) does the chosen action (T16,
//! T17): within its reach at once, else the player walks there first
//! (`sim::target::Walk`), steered in place of the keys (`Autopilot`,
//! `steer`), and does it on arriving. A move, jump or attack of the
//! player's own, the target going, a screen or a conversation ends the
//! walk; one that stalls or runs out of time tells the player it cannot
//! reach.
//!
//! A downed party member offers Revive (Treat Injury's icon and word, 257):
//! on arriving the player holds use facing it without a key
//! (`sim::target::Reviving`) until it stands; its own move, jump or action,
//! a hit, or the revive breaking off on the host ends the hold (E again
//! changes nothing, as during a walk).
//! The block shows the revive's progress in its bar, "Reviving" beneath;
//! the downed player's own block shows its reviver and the same bar.

use bevy::input::mouse::MouseWheel;
use bevy::prelude::*;
use console::{DumpDue, DumpFacts};
use sim::module::ObjectKind;
use sim::target::{Choice, Hold, Looker, Reviving, Stride, Walk};
use ui::draw::Textures;
use ui::skin::Source;
use ui::vm::{TargetAction, TargetView};

use crate::loot::{Outgoing, Told, words};
use crate::play_view::Aim;
use crate::sim_host::{OwnSeat, PresentedSnapshot, SceneSwap};
use crate::ui_view::Ui;

/// The command a walk to a target or a revive's hold steers the player by,
/// while one is under way, and what of the player's own cancels it
/// (`sim::target::steer`); `cancelled` set when it did.
#[derive(Resource, Default)]
pub struct Autopilot {
    pub command: Option<sim::Command>,
    pub breaks: u16,
    pub cancelled: bool,
}

/// The use key pressed by the console (`ui act`), and the action chosen
/// first if it named one (`ui act N`, from 0, as the wheel moves it).
#[derive(Resource, Default)]
pub struct Acted(pub bool, pub Option<usize>);

/// The console's `ui target TAG`: the target held on the nearest object so
/// tagged (or made from that template: remains, `*bag2`), as if the
/// crosshair were on it (`ui target` lets it go).
#[derive(Resource, Default)]
pub struct Pinned(pub Option<String>);

/// The target, what may be done to it and which is chosen (the wheel moves
/// it, a new target resets it; `docs/UI.md`), and the walk there under way.
#[derive(Resource, Default)]
pub struct Targeted {
    pub object: Option<u32>,
    pub choices: Vec<Choice>,
    pub selected: usize,
    hold: Hold,
    walk: Option<Walk>,
    reviving: Option<Reviving>,
}

impl Targeted {
    /// Ends the walk and the revive's hold under way.
    fn stop(&mut self) {
        self.walk = None;
        self.reviving = None;
    }
}

/// KOTOR's words for a choice (talk-table strrefs).
mod lines {
    pub const OPEN: u32 = 365;
    pub const USE: u32 = 366;
    pub const DIALOG: u32 = 371;
    pub const SECURITY: u32 = 255;
    pub const TREAT_INJURY: u32 = 257;
    pub const DISARM_MINE: u32 = sim::locks::lines::DISARM_MINE;
    pub const RECOVER_MINE: u32 = sim::locks::lines::RECOVER_MINE;
    pub const SET_MINE: u32 = sim::locks::lines::SET_MINE;
}

/// A choice's icon, by the kind of its object (an item's its own).
fn icon(choice: Choice, kind: ObjectKind) -> &'static str {
    match (choice, kind) {
        (Choice::Open, ObjectKind::Door) => "i_opendoor",
        (Choice::Open, _) => "i_openplace",
        (Choice::Use, _) => "i_useplace",
        (Choice::Talk, _) => "i_dialog",
        (Choice::Unlock { .. }, _) => "isk_security",
        (Choice::Revive | Choice::Item { .. }, _) => "isk_treatinj",
        (Choice::Disarm, _) => "i_disablemine",
        (Choice::Recover, _) => "i_recovermine",
        (Choice::Mine, _) => "isk_demolitions",
    }
}

/// A body's sheet without its effects: its own, else its module's.
fn sheet_of(
    snapshot: &sim::Snapshot,
    level: &sim::Level,
    body: &sim::EntityState,
) -> Option<sim::combat::Sheet> {
    body.sheet.or_else(|| {
        sim::Instances::in_snapshot(level, snapshot)
            .of_entity(body)
            .and_then(|facts| facts.sheet)
    })
}

/// The items `body` wears that it may use on a foe (a droid's utility
/// items: `sim::script::items::spell_for`, the host's own rule), with a use
/// left: their choices, and their templates by id.
fn worn_on_foes(
    snapshot: &sim::Snapshot,
    level: &sim::Level,
    body: &sim::EntityState,
) -> Vec<(Choice, String)> {
    let (Some(library), Some(sheet)) = (level.scripts(), sheet_of(snapshot, level, body)) else {
        return Vec::new();
    };
    snapshot
        .goods
        .items
        .iter()
        .filter(|(_, item)| item.holder == body.id.0 && item.slot.is_some())
        .filter_map(|(id, item)| {
            let facts = library.items().get(&item.template)?;
            let row = sim::script::items::spell_for(library, facts, sheet.race, false)?;
            let choice = Choice::Item {
                item: *id,
                reach_dm: library.spell(row).reach_dm,
            };
            (item.uses(facts) > 0).then(|| (choice, item.template.clone()))
        })
        .collect()
}

/// A body's name: a creature's, else "Player N" for a seat's avatar.
fn name_of(entity: &sim::EntityState, instances: &sim::Instances) -> String {
    instances
        .of_entity(entity)
        .map(|facts| facts.name.clone())
        .filter(|name| !name.is_empty())
        .unwrap_or_else(|| match entity.kind {
            sim::Kind::Avatar(seat) => format!("Player {}", u16::from(seat.0) + 1),
            _ => String::new(),
        })
}

/// A creature's vitality, whole points over its most.
fn vitality(
    entity: &sim::EntityState,
    facts: Option<&sim::module::ObjectData>,
) -> Option<(i32, i32)> {
    let sheet = entity.sheet.or(facts.and_then(|facts| facts.sheet))?;
    Some((entity.fighter?.whole_hp(), i32::from(sheet.max_hp)))
}

/// How far along the revive of `body` is, if one is under way.
fn revive_done(body: &sim::EntityState, now_ms: u64) -> Option<f32> {
    let revive = body.fighter?.fallen?.revive?;
    Some(revive.done(now_ms))
}

/// The downed player's own block: its reviver's name and how far along
/// the revive is (I30 P11).
fn revived(
    snapshot: &sim::Snapshot,
    seat: sim::Seat,
    instances: &sim::Instances,
) -> Option<TargetView> {
    let me = snapshot.body(seat)?;
    let revive = me.fighter?.fallen?.revive?;
    let by = snapshot.get(revive.by)?;
    Some(TargetView {
        name: name_of(by, instances).into_bytes(),
        progress: Some(revive.done(snapshot.time_ms)),
        ..TargetView::default()
    })
}

/// The command the player's tick takes (`sim::target::steer`): `base`
/// (the keys', the script's), or a walk's or a revive's hold in its place;
/// `look` turns with it so the view stays where it ends.
pub(crate) fn steer(
    autopilot: Option<&mut Autopilot>,
    base: sim::Command,
    look: &mut crate::sim_host::Look,
) -> sim::Command {
    let Some(pilot) = autopilot else {
        return base;
    };
    let (command, cancelled) = sim::target::steer(pilot.command, base, pilot.breaks);
    if cancelled {
        pilot.cancelled = true;
        pilot.command = None;
    } else if pilot.command.is_some() {
        look.pitch = sim::math::short_to_degrees(command.angles[0]);
        look.yaw = sim::math::short_to_degrees(command.angles[1]);
    }
    command
}

pub struct TargetViewPlugin;

impl Plugin for TargetViewPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Targeted>()
            .init_resource::<Autopilot>()
            .init_resource::<Acted>()
            .init_resource::<Pinned>()
            .add_systems(PostUpdate, target.after(crate::play_view::Drawn));
    }
}

/// What the target block shows of a target: its kind, name, vitality and
/// choices.
type Shown = (ObjectKind, String, Option<(i32, i32)>, Vec<Choice>, bool);

#[allow(clippy::type_complexity, clippy::too_many_arguments)]
fn target(
    presented: Option<Res<PresentedSnapshot>>,
    (own, swap, aim): (Option<Res<OwnSeat>>, Option<Res<SceneSwap>>, Res<Aim>),
    mut ui: ResMut<Ui>,
    (mut wheel, keys, map): (
        MessageReader<MouseWheel>,
        Option<Res<ButtonInput<KeyCode>>>,
        Option<Res<crate::settings::KeyMap>>,
    ),
    (mut autopilot, mut acted, mut outgoing, mut told): (
        ResMut<Autopilot>,
        ResMut<Acted>,
        ResMut<Outgoing>,
        ResMut<Told>,
    ),
    time: Res<Time<Real>>,
    mut targeted: ResMut<Targeted>,
    (mut facts, dump_due, pinned): (ResMut<DumpFacts>, Res<DumpDue>, Res<Pinned>),
    mut made: Local<sim::walk::MadeWalls>,
) {
    let turned: f32 = wheel.read().map(|event| event.y).sum();
    let map = map.map_or_else(crate::settings::KeyMap::default, |map| map.clone());
    let pressed = std::mem::take(&mut acted.0)
        || keys.is_some_and(|keys| map.pressed_now(&keys, crate::settings::Action::Use));
    if std::mem::take(&mut autopilot.cancelled) {
        targeted.stop();
    }
    autopilot.command = None;
    let looking = aim.0.filter(|_| !ui.stack.is_open());
    ui.vm.crosshair = looking.is_some();
    // The seat's body while it stands (`Snapshot::standing`: its avatar, or
    // the companion it drives, I31).
    let me = presented
        .as_ref()
        .zip(own.as_ref())
        .and_then(|(presented, own)| presented.0.current().standing(own.0))
        .and_then(|entity| Some((entity, entity.player?)));
    let (Some(presented), Some(swap), Some(look), Some((me, ps))) =
        (&presented, &swap, looking, me)
    else {
        targeted.hold = Hold::default();
        targeted.object = None;
        targeted.choices.clear();
        targeted.stop();
        // Down, the player's block shows who revives it.
        let snapshot = presented.as_ref().map(|presented| presented.0.current());
        ui.vm.target = snapshot
            .zip(own.as_ref())
            .zip(swap.as_ref())
            .filter(|_| !ui.stack.is_open())
            .and_then(|((snapshot, own), swap)| {
                let instances = sim::Instances::in_snapshot(&swap.level, snapshot);
                revived(snapshot, own.0, &instances)
            });
        if dump_due.0
            && let Some(progress) = ui.vm.target.as_ref().and_then(|view| view.progress)
        {
            facts.0.insert(
                "target".into(),
                serde_json::json!({ "object": null, "revived": progress }),
            );
        }
        return;
    };
    let snapshot = presented.0.current();
    let level = &swap.level;
    let feet = sim::math::scale(
        [
            ps.origin[0],
            ps.origin[1],
            ps.origin[2] + sim::motion::SOLE_Z,
        ],
        collision::UNIT,
    );
    let candidates = sim::target::candidates(level, snapshot);
    let yaw = ps.view[1].to_radians();
    let looker = Looker {
        feet,
        facing: [yaw.cos(), yaw.sin()],
    };
    let states = level.states_in(snapshot);
    let picked = match pinned.0.as_deref() {
        Some(tag) => {
            let instances = sim::Instances::in_snapshot(level, snapshot);
            let tagged = |index: u32| {
                sim::target::entity(snapshot, index)
                    .and_then(|entity| instances.of_entity(entity))
                    .is_some_and(|facts| {
                        facts.tag.eq_ignore_ascii_case(tag)
                            || facts.template.eq_ignore_ascii_case(tag)
                    })
            };
            candidates
                .iter()
                .filter(|candidate| tagged(candidate.index))
                .min_by(|a, b| {
                    let away = |c: &&sim::target::Candidate| {
                        sim::math::length(sim::math::sub(c.feet, feet))
                    };
                    away(a).total_cmp(&away(b))
                })
                .map(|candidate| candidate.index)
        }
        None => sim::target::pick(level, states.as_deref(), &look, &looker, &candidates),
    };
    let now = time.elapsed().as_millis() as u64;
    // A walk or a revive keeps its target, wherever the view turns.
    let walking = targeted.walk.as_ref().map(|walk| walk.object);
    let reviving = targeted.reviving.as_ref().map(|hold| hold.object);
    let object = walking
        .or(reviving)
        .or_else(|| targeted.hold.update(picked, now));
    if object != targeted.object {
        targeted.object = object;
        targeted.selected = 0;
    }
    let instances = sim::Instances::in_snapshot(level, snapshot);
    let entity_of = |number: u32| sim::target::entity(snapshot, number);
    let catalog = level.scripts().map(|library| library.items());
    // The best security spike the party carries.
    let spike = catalog.and_then(|catalog| {
        snapshot
            .goods
            .carried(sim::inventory::PARTY)
            .filter_map(|(id, item)| Some((catalog.get(&item.template)?.tools, id, item)))
            .filter(|(tools, ..)| *tools > 0)
            .max_by_key(|(tools, ..)| *tools)
            .map(|(_, id, item)| (id, item.template.clone()))
    });
    // TSL's mine on a door, where the party carries a kit.
    let mine = level.module().is_some_and(|module| module.game == "tsl")
        && catalog.is_some_and(|catalog| {
            snapshot
                .goods
                .carried(sim::inventory::PARTY)
                .any(|(_, item)| {
                    catalog
                        .get(&item.template)
                        .is_some_and(|facts| facts.trap.is_some())
                })
        });
    // Security, for a body with ranks in it.
    let security = sheet_of(snapshot, level, me)
        .is_some_and(|sheet| sheet.skills[sim::combat::sheet::skills::SECURITY] > 0)
        .then(|| sim::target::Security {
            spike: spike.as_ref().map(|(id, _)| *id),
        });
    let shown: Option<Shown> = object.and_then(entity_of).and_then(|entity| {
        if sim::target::revivable(entity) {
            return Some((
                ObjectKind::Creature,
                name_of(entity, &instances),
                vitality(entity, instances.of_entity(entity)),
                sim::target::body_choices(entity),
                false,
            ));
        }
        let facts = instances.of_entity(entity)?;
        let sheet = entity.sheet.or(facts.sheet);
        let fighter = entity.fighter.as_ref();
        let hostile = fighter.zip(sheet).is_some_and(|(fighter, sheet)| {
            let faction = fighter.faction.unwrap_or(sheet.faction);
            level.module().is_some_and(|module| {
                module
                    .repute
                    .hostile(faction, sim::combat::sheet::factions::PLAYER)
            })
        });
        // A door's or container's once blows have told on it (bashing,
        // `sim::objects`), over what it was placed with: KOTOR's doors are
        // placed with more than their most (`CurrentHP` 60, `HP` 20).
        let state = entity.object.unwrap_or(sim::ObjectState::default());
        let hp = match facts.kind {
            ObjectKind::Creature => vitality(entity, Some(facts)),
            _ => (state.damage > 0 && state.bashable(facts))
                .then(|| state.hit_points(facts))
                .flatten()
                .zip(facts.hit_points)
                .map(|((now, most), (placed, _))| (now, most.max(placed))),
        };
        // A trap: its row's (a kit to recover; its name), hostile by its
        // faction.
        let row = facts
            .trap
            .as_ref()
            .and_then(|trap| level.module()?.traps.get(usize::from(trap.row)).cloned());
        let hostile = hostile
            || (row.is_some()
                && level.module().is_some_and(|module| {
                    module
                        .repute
                        .hostile(facts.faction, sim::combat::sheet::factions::PLAYER)
                }));
        let choices = sim::target::choices(
            facts.kind,
            facts,
            state,
            hostile,
            security,
            (row.as_ref().is_some_and(|row| !row.kit.is_empty()), mine),
        );
        let name = match &row {
            Some(row) if row.name != u32::MAX => ui
                .assets
                .as_ref()
                .and_then(|assets| assets.text(Source::Campaign, row.name))
                .map_or_else(String::new, |text| fmt_odyssey::code_page(&text)),
            _ => facts.name.clone(),
        };
        Some((facts.kind, name, hp, choices, hostile))
    });
    // A foe: the items the body wears that it may use on one (a droid's).
    let worn = worn_on_foes(snapshot, level, me);
    let shown = shown.map(|(kind, name, hp, mut choices, hostile)| {
        if hostile && kind == ObjectKind::Creature {
            choices.extend(worn.iter().map(|(choice, _)| *choice));
        }
        (kind, name, hp, choices, hostile)
    });
    let at = object.and_then(|index| {
        candidates
            .iter()
            .find(|candidate| candidate.index == index)
            .map(|candidate| candidate.feet)
    });
    let (Some(index), Some((kind, name, hp, choices, hostile)), Some(to)) = (object, shown, at)
    else {
        targeted.choices.clear();
        targeted.stop();
        ui.vm.target = None;
        if dump_due.0 {
            facts.0.insert(
                "target".into(),
                serde_json::json!({ "object": null, "candidates": candidates.len() }),
            );
        }
        return;
    };
    if turned != 0.0 && !choices.is_empty() {
        let step = if turned > 0.0 { -1 } else { 1 };
        let count = choices.len() as i64;
        targeted.selected = (targeted.selected as i64 + step).rem_euclid(count) as usize;
    }
    if let Some(chosen) = acted.1.take() {
        targeted.selected = chosen;
    }
    targeted.selected = targeted.selected.min(choices.len().saturating_sub(1));
    // The use key: the chosen action at once, or the walk there.
    let doors = |number: u32| {
        entity_of(number)
            .and_then(|entity| entity.object)
            .unwrap_or(sim::ObjectState::default())
    };
    // The walk keeps off the placeables play made, as the sim's do.
    let state = sim::walk::Walled {
        doors: &doors,
        walled: made.of_snapshot(level, snapshot),
    };
    // A walk is timed on the sim's clock, not the frame's.
    let sim_ms = snapshot.time_ms;
    // A target that no longer offers the walk's choice ends it.
    if targeted.walk.is_some_and(|walk| !walk.wanted(&choices)) {
        targeted.walk = None;
    }
    // A choice done where the player stands: asked of the host, or a
    // revive of this body asked for and held.
    let mut act = |targeted: &mut Targeted, choice: Choice| match choice {
        Choice::Revive => {
            targeted.reviving = Some(Reviving::new(index, sim_ms));
            outgoing
                .0
                .extend(entity_of(index).map(|body| sim::action::Action::Revive(body.id)));
        }
        Choice::Item { item, .. } => {
            outgoing
                .0
                .extend(entity_of(index).map(|foe| sim::action::Action::UseItem {
                    who: me.id.0,
                    item,
                    target: foe.id.0,
                }));
        }
        _ => outgoing.0.extend(sim::target::ask(choice, index, kind)),
    };
    // E again while walking or reviving changes nothing, as a walk's
    // ignores it: a second press does not undo the first.
    let busy = targeted.walk.is_some() || targeted.reviving.is_some();
    // Where the choice is done from: a door's action point on the player's
    // side, else the target's feet.
    let spot = |choice: Choice| {
        sim::target::Spot::in_snapshot(level, snapshot, (index, kind), choice, to, &ps)
    };
    if let (true, false, Some(&choice)) = (pressed, busy, choices.get(targeted.selected)) {
        let spot = spot(choice);
        if Walk::near(&ps, spot) {
            act(&mut targeted, choice);
        } else {
            targeted.walk = Some(Walk::new(level, &state, &ps, index, spot, choice, sim_ms));
        }
    }
    autopilot.breaks = sim::target::WALK_BREAKS;
    if let Some(mut walk) = targeted.walk {
        match walk.stride(level, &state, &ps, spot(walk.choice), sim_ms) {
            Stride::Steer(command) => {
                autopilot.command = Some(command);
                targeted.walk = Some(walk);
            }
            Stride::Arrived => {
                targeted.walk = None;
                act(&mut targeted, walk.choice);
            }
            Stride::Unreachable => {
                told.0
                    .push(words(&sim::notice::Notice::OutOfReach, Some(&mut ui)));
                targeted.walk = None;
            }
        }
    }
    if let Some(hold) = targeted.reviving.as_mut() {
        match hold.keep(entity_of(index), me, sim_ms) {
            Some(command) => {
                autopilot.command = Some(command);
                autopilot.breaks = sim::target::HOLD_BREAKS;
            }
            None => targeted.reviving = None,
        }
    }
    let progress = entity_of(index).and_then(|body| revive_done(body, sim_ms));
    // The worn items' icons and names, by id.
    let looks: Vec<(u32, String, u32)> = worn
        .iter()
        .filter_map(|(choice, template)| {
            let Choice::Item { item, .. } = choice else {
                return None;
            };
            let icon = ui.assets.as_mut()?.item_ref(template)?.icon.clone();
            Some((*item, icon, catalog?.get(template)?.name))
        })
        .collect();
    let look = |choice: Choice| match choice {
        Choice::Item { item, .. } => looks.iter().find(|(id, ..)| *id == item),
        _ => None,
    };
    let assets = ui.assets.as_ref();
    let say = |strref: u32| {
        assets
            .and_then(|assets| assets.text(Source::Campaign, strref))
            .unwrap_or(Vec::new())
    };
    let spike_name = spike.as_ref().and_then(|(_, template)| {
        let name = catalog?.get(template)?.name;
        (name != u32::MAX).then(|| say(name))
    });
    let actions = choices
        .iter()
        .map(|&choice| {
            let mut label = say(match choice {
                Choice::Open => lines::OPEN,
                Choice::Use => lines::USE,
                Choice::Talk => lines::DIALOG,
                Choice::Unlock { .. } => lines::SECURITY,
                Choice::Revive => lines::TREAT_INJURY,
                Choice::Disarm => lines::DISARM_MINE,
                Choice::Recover => lines::RECOVER_MINE,
                Choice::Mine => lines::SET_MINE,
                Choice::Item { .. } => look(choice).map_or(u32::MAX, |(.., name)| *name),
            });
            if let (Choice::Unlock { spike: Some(_) }, Some(spike)) = (choice, &spike_name) {
                label.extend_from_slice(b": ");
                label.extend_from_slice(spike);
            }
            TargetAction {
                icon: look(choice)
                    .map_or_else(|| icon(choice, kind).into(), |(_, icon, _)| icon.clone()),
                label,
            }
        })
        .collect();
    ui.vm.target = Some(TargetView {
        name: name.into_bytes(),
        hostile,
        hp,
        actions,
        selected: targeted.selected,
        busy: targeted.walk.is_some() || targeted.reviving.is_some(),
        progress,
    });
    targeted.choices = choices;
    if dump_due.0 {
        let about = entity_of(index).map(|entity| {
            let facts = instances.of_entity(entity);
            serde_json::json!({
                "index": index,
                "distance_m": ((to[0] - feet[0]).powi(2) + (to[1] - feet[1]).powi(2)).sqrt(),
                "kind": format!("{kind:?}"),
                "name": name_of(entity, &instances),
                "tag": facts.map(|facts| facts.tag.clone()),
                "template": facts.map(|facts| facts.template.clone()),
            })
        });
        facts.0.insert(
            "target".into(),
            serde_json::json!({
                "object": about,
                "candidates": candidates.len(),
                "choices": targeted.choices.iter().map(|c| format!("{c:?}")).collect::<Vec<_>>(),
                "selected": targeted.selected,
                "walking": targeted.walk.is_some(),
                "reviving": targeted.reviving.is_some(),
                "progress": progress,
            }),
        );
    }
}
