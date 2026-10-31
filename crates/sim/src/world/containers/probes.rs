//! The probes of `containers` (`kjk sim probes`).

use super::super::Player;
use super::*;
use crate::action::Action;
use crate::input::Seat;
use crate::module::ObjectKind;
use crate::notice::Notice;

/// The containers' checks, on the proving ground with two containers
/// added, one by the entry and one far off; two seats: both open the near
/// one, the far one is out of reach, both take its first item in one step
/// (the lower seat gets it: the party's), the second takes what is left,
/// the first finds nothing more, the second closes and cannot take again;
/// the first, moved out of reach while holding it open, has it closed and
/// its take refused; opening it runs its script, which tells both players
/// something and its opener something else; opening the one behind runs
/// its script, which gives it an item its template never listed, and the
/// goods show it beside its own while it is held open (K1's civilian
/// footlockers, filled by their heartbeat); a save made while seat 0
/// holds two open loads with neither held nor open. Run twice, the two end
/// in the same state. For the approved test `ui_loot`.
pub fn probes() -> Vec<(&'static str, Result<String, String>)> {
    let Ran {
        snapshots,
        notices,
        loaded,
        ids,
    } = match scenario() {
        Ok(ran) => ran,
        Err(why) => return vec![("containers_scenario", Err(why))],
    };
    let (Some(first), Some(last)) = (snapshots.get(4), snapshots.last()) else {
        return vec![("containers_scenario", Err("too few steps".into()))];
    };
    let again = scenario().map(|ran| ran.snapshots.last().map(crate::Snapshot::hash));
    let container = |snapshot: &crate::Snapshot, index: u32| {
        snapshot
            .entities
            .iter()
            .find(|entity| entity.kind == crate::Kind::Object(ObjectKind::Placeable, index))
            .and_then(|entity| entity.object)
    };
    let near = |snapshot: &crate::Snapshot| container(snapshot, NEAR);
    // What a container held open shows (the goods), by template.
    let shown = |snapshot: &crate::Snapshot, id: u32| -> Vec<String> {
        let mut held: Vec<String> = (snapshot.goods.carried(id))
            .map(|(_, item)| item.template.clone())
            .collect();
        held.sort();
        held
    };
    // What the party carries, by template.
    let party = |snapshot: &crate::Snapshot| -> Vec<String> {
        let mut carried: Vec<String> = snapshot
            .goods
            .carried(crate::inventory::PARTY)
            .map(|(_, item)| item.template.clone())
            .collect();
        carried.sort();
        carried
    };
    let check = |ok: bool, what: String| if ok { Ok(what) } else { Err(what) };
    let gone = Notice::Gone {
        item: "medpac".into(),
        by: Some(Seat(0)),
    };
    vec![
        (
            "contested_take",
            check(
                party(&snapshots[1]) == ["medpac"] && notices.contains(&(1, Seat(1), gone.clone())),
                format!("the party {:?}, notices {notices:?}", party(&snapshots[1])),
            ),
        ),
        (
            "take_all_left",
            check(
                party(first) == ["grenade", "medpac", "stim"]
                    && notices.contains(&(3, Seat(0), Notice::Empty)),
                format!("the party {:?}", party(first)),
            ),
        ),
        (
            "out_of_reach",
            check(
                notices.contains(&(0, Seat(0), Notice::OutOfReach)),
                format!("{notices:?}"),
            ),
        ),
        (
            "closed_cannot_take",
            check(
                notices.contains(&(4, Seat(1), Notice::NotOpen))
                    && near(first).is_some_and(|object| object.tenants == 1 && object.state == 1)
                    && shown(first, ids.near).is_empty(),
                format!(
                    "near container {:?} showing {:?}",
                    near(first),
                    shown(first, ids.near)
                ),
            ),
        ),
        (
            "use_opens_faced",
            check(
                container(&snapshots[0], BEHIND).is_some_and(|object| object.tenants & 1 != 0),
                format!("behind {:?}", container(&snapshots[0], BEHIND)),
            ),
        ),
        (
            "out_of_reach_closes",
            check(
                near(&snapshots[5]).is_some_and(|object| {
                    object.tenants == 0 && object.state == crate::snapshot::look::CLOSED
                }) && notices.contains(&(5, Seat(0), Notice::OutOfReach)),
                format!("near container {:?}", near(&snapshots[5])),
            ),
        ),
        (
            "party_item_put_in",
            check(
                shown(last, ids.near) == ["grenade"]
                    && party(last) == ["medpac", "stim"]
                    && notices.contains(&(7, Seat(1), Notice::CannotUse)),
                format!(
                    "near shows {:?}, the party {:?}, notices {notices:?}",
                    shown(last, ids.near),
                    party(last)
                ),
            ),
        ),
        (
            "script_given_shown",
            check(
                shown(first, ids.behind) == ["credits", "rifle"],
                format!("behind shows {:?}", shown(first, ids.behind)),
            ),
        ),
        (
            "held_item_found_by_tag",
            check(
                notices
                    .iter()
                    .any(|(_, _, notice)| *notice == Notice::Said(GIVEN.into())),
                format!("{notices:?}"),
            ),
        ),
        (
            "scripts_tell_players",
            check(
                [Seat(0), Seat(1)].iter().all(|seat| {
                    notices
                        .iter()
                        .any(|(_, to, notice)| to == seat && *notice == Notice::Show(BOX))
                }) && notices
                    .iter()
                    // Before the near one is opened again for the put.
                    .filter(|(at, _, notice)| *at < 6 && *notice == Notice::Said(SAID.into()))
                    .count()
                    == 1,
                format!("{notices:?}"),
            ),
        ),
        (
            "load_holds_none_open",
            check(
                [NEAR, BEHIND].iter().all(|index| {
                    container(first, *index).is_some_and(|object| object.tenants & 1 != 0)
                        && container(&loaded, *index).is_some_and(|object| {
                            object.tenants == 0 && object.state == crate::snapshot::look::CLOSED
                        })
                }),
                format!(
                    "saved near {:?} behind {:?}, loaded near {:?} behind {:?}",
                    near(first),
                    container(first, BEHIND),
                    near(&loaded),
                    container(&loaded, BEHIND)
                ),
            ),
        ),
        (
            "same_every_run",
            check(
                again.as_ref().is_ok_and(|hash| *hash == Some(last.hash())),
                format!("{} then {again:?}", last.hash().short()),
            ),
        ),
    ]
}

/// The near container's `OnOpen`: a message box for every player
/// (`DisplayMessageBox(BOX)`), words for its opener
/// (`SendMessageToPC(GetLastOpenedBy(), SAID)`). The one behind's:
/// `CreateItemOnObject(GIVEN)` on itself, then its opener told the tag of
/// what `GetObjectByTag(GIVEN)` finds: the item it now holds.
const BOX: crate::notice::Screen = crate::notice::Screen::MessageBox { strref: 42 };
const SAID: &str = "opened";
const GIVEN: &str = "rifle";

fn opened_script() -> crate::script::Library {
    use fmt_odyssey::ncs::{Const, Op};
    const CATALOG: &str = r#"
// 0
void SendMessageToPC(object oPlayer, string szMessage);
// 1
void DisplayMessageBox(int nStrRef, string sIcon = "");
// 2
object GetLastOpenedBy();
// 3
object CreateItemOnObject(string sItemTemplate, object oTarget = OBJECT_SELF, int nStackSize = 1);
// 4
object GetObjectByTag(string sTag, int nNth=0);
// 5
string GetTag(object oObject);
"#;
    let mut asm = script_ncs::probes::Asm::default();
    asm.op(Op::Const(Const::String(Vec::new())))
        .op(Op::Const(Const::Int(42)))
        .op(Op::Action {
            routine: 1,
            args: 2,
        })
        .op(Op::Const(Const::String(SAID.as_bytes().to_vec())))
        .op(Op::Action {
            routine: 2,
            args: 0,
        })
        .op(Op::Action {
            routine: 0,
            args: 2,
        })
        .op(Op::Retn);
    let mut give = script_ncs::probes::Asm::default();
    give.op(Op::Const(Const::Int(1)))
        .op(Op::Const(Const::Object(0)))
        .op(Op::Const(Const::String(GIVEN.as_bytes().to_vec())))
        .op(Op::Action {
            routine: 3,
            args: 3,
        })
        .op(Op::MovSp(-4))
        .op(Op::Const(Const::Int(0)))
        .op(Op::Const(Const::String(GIVEN.as_bytes().to_vec())))
        .op(Op::Action {
            routine: 4,
            args: 2,
        })
        .op(Op::Action {
            routine: 5,
            args: 1,
        })
        .op(Op::Action {
            routine: 2,
            args: 0,
        })
        .op(Op::Action {
            routine: 0,
            args: 2,
        })
        .op(Op::Retn);
    crate::probe::library(
        CATALOG,
        [("probe_open", asm.build()), ("probe_give", give.build())],
    )
}

/// An item template with nothing to it but its name.
pub(crate) fn plain(template: &str) -> crate::inventory::ItemFacts {
    crate::inventory::ItemFacts {
        template: template.into(),
        tag: template.into(),
        cased_tag: String::new(),
        name: u32::MAX,
        base_item: 0,
        stack: 1,
        stacking: 1,
        charges: 0,
        cost: 0,
        plot: false,
        slots: 0,
        weapon: None,
        bare: None,
        armour: None,
        guards: crate::combat::sheet::Guards::default(),
        properties: Vec::new(),
        gear: Default::default(),
        limits: Vec::new(),
        rows: 0,
        spell: None,
        droids: None,
        upgrades: Vec::new(),
        upgrade: None,
        upgrade_level: 0,
        slot_kinds: Vec::new(),
        parts: Vec::new(),
        recolours: Vec::new(),
        tools: 0,
        needs: Vec::new(),
        trap: None,
        stealth_unit: false,
        shield: false,
    }
}

/// A container holding one of each of `items`.
pub(in crate::world) fn container(
    tag: &str,
    at: [f32; 3],
    items: &[&str],
) -> crate::module::ObjectData {
    crate::module::ObjectData {
        inventory: items
            .iter()
            .map(|item| crate::inventory::Held {
                template: item.to_string(),
                stack: 1,
                slot: None,
                droppable: true,
                infinite: false,
            })
            .collect(),
        container: true,
        useable: true,
        ..crate::module::ObjectData::bare(ObjectKind::Placeable, tag, at)
    }
}

/// The near container's, the far one's and the one behind's indexes among
/// the objects.
const NEAR: u32 = 2;
const FAR: u32 = 3;
const BEHIND: u32 = 4;

/// What the scenario gave: each step's snapshot, every notice with its
/// step (0 the first), the world loaded from a save made after step 4,
/// and the containers' ids.
struct Ran {
    snapshots: Vec<crate::Snapshot>,
    notices: Vec<(usize, Seat, Notice)>,
    loaded: crate::Snapshot,
    ids: Ids,
}

/// The near container's and the one behind's entity ids.
struct Ids {
    near: u32,
    behind: u32,
}

fn scenario() -> Result<Ran, String> {
    use crate::module::ObjectData;
    use crate::{Seat, StepReason, TickInput, WorldConfig};

    let ground = crate::determinism::proving_ground();
    let mut data = (**ground.module().ok_or("the ground has no module")?).clone();
    let entry = data.entry.position;
    data.placements.extend(
        [
            ObjectData {
                scripts: vec![("OnOpen".into(), "probe_open".into())],
                ..container("near", entry, &["medpac", "stim", "grenade"])
            },
            container("far", [entry[0] + 8.0, entry[1], entry[2]], &["rifle"]),
            ObjectData {
                scripts: vec![("OnOpen".into(), "probe_give".into())],
                ..container("behind", [entry[0] - 1.5, entry[1], entry[2]], &["credits"])
            },
        ]
        .map(crate::module::Placement::Own),
    );
    let level = ground
        .with_module(data)
        .with_scripts(opened_script().with_items(crate::inventory::Catalog::new(
            ["medpac", "stim", "grenade", "rifle", "credits"].map(plain),
        )));
    let mut world = super::super::SimWorld::new(
        WorldConfig {
            seed: 1,
            seats: 2,
            wanderers: 0,
            spawn: [0.0; 3],
            yaw: 0.0,
            sheet: crate::combat::Sheet::dev_soldier(),
            guests: crate::Guests::Avatars,
        },
        level,
    )
    .map_err(|error| error.to_string())?;
    // The near one's items, as its template stocked them.
    let start = world.snapshot();
    let id_of = |index: u32| {
        (start.entities.iter())
            .find(|entity| entity.kind == crate::Kind::Object(ObjectKind::Placeable, index))
            .map(|entity| entity.id.0)
            .ok_or(format!("no container {index}"))
    };
    let ids = Ids {
        near: id_of(NEAR)?,
        behind: id_of(BEHIND)?,
    };
    let stocked: Vec<u32> = world
        .script_state()
        .loot(ids.near)
        .map(|(item, _)| item)
        .collect();
    let [medpac, stim, grenade] = stocked[..] else {
        return Err(format!("the near one holds {stocked:?}"));
    };
    let take = |item| Action::Take {
        container: NEAR,
        item,
    };
    let put = Action::Put {
        container: NEAR,
        item: grenade,
    };
    let steps: [Vec<(u8, Action)>; 8] = [
        vec![
            (0, Action::Open(NEAR)),
            (0, Action::Open(FAR)),
            (1, Action::Open(NEAR)),
        ],
        vec![(0, take(medpac)), (1, take(medpac))],
        vec![(1, Action::TakeAll(NEAR))],
        vec![(0, Action::TakeAll(NEAR)), (1, Action::Close(NEAR))],
        vec![(1, take(stim))],
        // Seat 0 is moved 8 m away first (below).
        vec![(0, take(medpac))],
        vec![(1, Action::Open(NEAR))],
        // The party's grenade put back, asked twice in one step.
        vec![(1, put.clone()), (1, put)],
    ];
    let mut notices = Vec::new();
    let mut snapshots = Vec::new();
    let mut loaded = None;
    for (at, actions) in steps.iter().enumerate() {
        if at == 5 {
            // Saved while seat 0 holds the near one and the one behind open.
            let save = crate::Save {
                hz: crate::TICK_HZ,
                level: world.level().id(),
                snapshot: world.snapshot(),
                script: world.script_state().clone(),
            };
            loaded = Some(save.world(world.level(), 2, None)?.snapshot());
            let mut players = world.world.query::<(&super::super::Avatar, &mut Player)>();
            for (avatar, mut player) in players.iter_mut(&mut world.world) {
                if avatar.0 == Seat(0) {
                    player.0.origin[1] += 8.0 / collision::UNIT;
                }
            }
        }
        let mut input = TickInput::default();
        if at == 0 {
            // Seat 0 turns to the one behind it and uses (E).
            input.insert(
                Seat(0),
                crate::Command {
                    angles: crate::moves::view_angles(0.0, 180.0),
                    buttons: crate::buttons::USE,
                    ..crate::Command::default()
                },
            );
        }
        for (seat, action) in actions {
            input.act(Seat(*seat), action.clone());
        }
        snapshots.push(crate::step(
            &mut world,
            at as u64 + 1,
            &input,
            8,
            StepReason::Authority,
        ));
        notices.extend(
            world
                .take_notices()
                .into_iter()
                .map(|(seat, notice)| (at, seat, notice)),
        );
    }
    Ok(Ran {
        snapshots,
        notices,
        loaded: loaded.ok_or("no save made")?,
        ids,
    })
}
