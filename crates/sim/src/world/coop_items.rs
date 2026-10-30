//! Co-op's items and levels (I30 P14, P15), on the proving ground: two
//! companions in the party table, following seat 0, a locker by the
//! entry, two seats and one avatar (the host's). Seat 1 drives the first companion (`a`); nobody plays
//! the second (`b`). Step by step:
//!
//! 0. Both seats open the locker; the host asks to dress `a` as seat 1
//!    takes it over (refused: `a` is seat 1's from this step).
//! 1. Both take the locker's first item (the lower seat's); seat 1 wears a
//!    blade on `a` (its own body); it cannot dress the host nor `b`; the
//!    host dresses `b`.
//! 2. The host cannot take `a`'s blade off; seat 1 cannot wear the host's
//!    crown (worn, not the party's) nor sell it or `a`'s blade (worn).
//! 3. Seat 1 takes the blade off `a` and sells it.
//! 4. The party has XP for two levels: `a` and `b` both wait.
//! 5. Seat 1 asks a level in a class `a` does not have: refused, told to
//!    seat 1. It asks one for `b` (the host's to give: refused); the host
//!    asks one for `b` and `b` takes it.
//! 6. Seat 1 asks a level with its own picks: `a` takes one, the next
//!    still waits.
//! 7. Seat 1 drops (no command), its picks sent anyway: nothing taken, `a`
//!    waits on; the host dresses `a`.
//! 8. The host asks `a`'s waiting level, and `a` takes it.
//!
//! Who drives `a` is `drivers::assign`'s, at the top of each step: seat 1
//! takes the first party member free in step 0, and drops it in step 7.
//!
//! For the proposed approved test `coop_items_and_levels`.

use super::{Fighting, Id, SimWorld};
use crate::action::Action;
use crate::input::Seat;
use crate::inventory::{Catalog, ItemFacts, PARTY, Terms, slots};
use crate::levels::{Choices, Class, Classes, SKILLS};
use crate::module::{ObjectKind, Placement};
use crate::notice::Notice;
use crate::probe::check;
use crate::script::State;
use crate::snapshot::SimId;
use crate::{Snapshot, StepReason, TickInput, WorldConfig};

/// The store the driven companion trades with (an id no entity has).
const STORE: u32 = 900_000;
/// Every item's value; the store buys at half of it.
const COST: u32 = 100;
/// The party's XP from step 4: levels 4 and 5 of `classes()`.
const XP: i32 = 10_000;
/// The companions' class and level as placed.
const START: (u8, u8) = (0, 3);
/// The locker's tag and the companions' (their templates too).
const LOCKER: &str = "locker";
const COMPANIONS: [&str; 2] = ["comp_a", "comp_b"];
/// The step seat 1 takes `a` over, and the one it drops.
const TAKEN: usize = 0;
const DROPPED: usize = 7;

/// An item worn in `slot`, worth `COST`.
fn wearable(template: &str, slot: u8) -> ItemFacts {
    ItemFacts {
        slots: 1 << slot,
        cost: COST,
        ..super::containers::plain(template)
    }
}

fn catalog() -> Catalog {
    Catalog::new([
        wearable("robe", slots::BODY),
        wearable("blade", slots::RIGHT_WEAPON),
        wearable("crown", slots::HEAD),
        super::containers::plain("medpac"),
    ])
}

/// Two classes of twenty levels alike: a d10, 2 skill points, every skill
/// its own and recommended in row order, nothing to pick.
fn classes() -> Classes {
    let class = Class {
        hit_die: 10,
        skill_points: 2,
        bab: (1..=20).collect(),
        saves: vec![[2, 0, 0]; 20],
        defence: vec![0; 20],
        feat_picks: vec![0; 20],
        power_picks: vec![0; 20],
        class_skills: [true; SKILLS],
        skill_order: (0..SKILLS as u8).collect(),
        ..Class::default()
    };
    Classes {
        classes: vec![class; 2],
        levels: vec![0, 1000, 3000, 6000, XP, 15_000],
        ..Classes::default()
    }
}

/// A level of class `class` raising skill 1 (the game's would raise
/// skill 0 first).
fn picks(class: u8) -> Choices {
    let mut skills = [0; SKILLS];
    skills[1] = 1;
    Choices {
        class,
        skills,
        ..Choices::default()
    }
}

/// Who is who, by object id.
struct Cast {
    host: u32,
    a: u32,
    b: u32,
    robe: u32,
    blades: [u32; 2],
    crown: u32,
}

/// Each step's snapshot and scripts' state, and every notice with its step.
struct Ran {
    cast: Cast,
    snapshots: Vec<Snapshot>,
    states: Vec<State>,
    notices: Vec<(usize, Seat, Notice)>,
}

fn scenario() -> Result<Ran, String> {
    let ground = crate::determinism::proving_ground();
    let mut data = (**ground.module().ok_or("the ground has no module")?).clone();
    let entry = data.entry.position;
    let sheet = crate::combat::Sheet {
        level: START.1,
        skills: [0; SKILLS],
        ..crate::combat::Sheet::dev_soldier()
    };
    data.placements
        .push(Placement::Own(super::containers::container(
            LOCKER,
            entry,
            &["medpac"],
        )));
    for (at, tag) in COMPANIONS.iter().enumerate() {
        let x = entry[0] + 1.5 * (at as f32 + 1.0);
        data.placements
            .push(Placement::Own(crate::module::ObjectData {
                sheet: Some(sheet),
                classes: vec![START],
                template: tag.to_string(),
                ..crate::module::ObjectData::bare(
                    ObjectKind::Creature,
                    tag,
                    [x, entry[1], entry[2]],
                )
            }));
    }
    let library = crate::probe::library("", [] as [(&str, _); 0])
        .with_items(catalog())
        .with_classes(classes());
    // Two seats, one avatar, the host's: seat 1 plays a companion.
    let mut world = SimWorld::new(
        WorldConfig {
            seats: 2,
            guests: crate::Guests::Companions,
            ..WorldConfig::one_seat()
        },
        ground.with_module(data).with_scripts(library),
    )
    .map_err(|error| error.to_string())?;

    // Who stands where: the companions and the locker by tag.
    let start = world.snapshot();
    let objects = crate::Instances::in_snapshot(world.level(), &start);
    let tagged = |tag: &str| {
        start
            .entities
            .iter()
            .find(|entity| {
                objects
                    .of_entity(entity)
                    .is_some_and(|data| data.tag == tag)
            })
            .ok_or(format!("no {tag}"))
    };
    let host = start
        .body(Seat(0))
        .map(|entity| entity.id.0)
        .ok_or("no host")?;
    let (a, b) = (tagged(COMPANIONS[0])?.id.0, tagged(COMPANIONS[1])?.id.0);
    let locker_entity = tagged(LOCKER)?;
    let locker = match locker_entity.kind {
        crate::Kind::Object(_, index) => index,
        _ => return Err("the locker is no object".into()),
    };
    let first_in_locker = (world.script_state().loot(locker_entity.id.0).next())
        .map(|(item, _)| item)
        .ok_or("the locker holds nothing")?;

    // The party table, its items, the driven companion's trade.
    let mut state = world.script_state().clone();
    for (npc, (tag, object)) in COMPANIONS.iter().zip([a, b]).enumerate() {
        state.party.available.insert(
            npc as u8,
            crate::script::party::Npc {
                template: tag.to_string(),
                object: Some(object),
                kept: None,
                template_data: None,
            },
        );
        state.party.members.push(npc as u8);
    }
    let items = catalog();
    let mut made = |template: &str, holder: u32, slot: Option<u8>| {
        state
            .make_item(&items, template, holder, 1, slot)
            .map(|id| id.0)
            .ok_or(format!("no {template}"))
    };
    let robe = made("robe", PARTY, None)?;
    let blades = [made("blade", PARTY, None)?, made("blade", PARTY, None)?];
    let crown = made("crown", host, Some(slots::HEAD))?;
    let terms = Terms {
        markup: 100,
        markdown: 50,
        sells: true,
        buys: true,
    };
    state.open_trade(a, STORE, terms, (0, 0));
    world.set_script_state(state);
    // Both follow seat 0, as `AddPartyMember` leaves them.
    let mut fighters = world.world.query::<(&Id, &mut Fighting)>();
    for (id, mut fighting) in fighters.iter_mut(&mut world.world) {
        if [a, b].contains(&id.0.0) {
            fighting.0.mind.leader = Some(Seat(0));
        }
    }

    let cast = Cast {
        host,
        a,
        b,
        robe,
        blades,
        crown,
    };
    let take = Action::Take {
        container: locker,
        item: first_in_locker,
    };
    let sell = |item| Action::Sell { store: STORE, item };
    let equip_on = |who, item, slot| Action::EquipOn { who, item, slot };
    let weapon = slots::RIGHT_WEAPON;
    // Each step's actions, by seat.
    let level_up_on = |who| Action::LevelUpOn {
        who,
        choices: picks(START.0),
    };
    let steps: [Vec<(u8, Action)>; 9] = [
        vec![
            (0, Action::Open(locker)),
            (0, equip_on(a, robe, slots::BODY)),
            (1, Action::Open(locker)),
        ],
        vec![
            (0, take.clone()),
            (0, equip_on(b, blades[1], weapon)),
            (1, take),
            (
                1,
                Action::Equip {
                    item: blades[0],
                    slot: weapon,
                },
            ),
            (1, equip_on(host, robe, slots::BODY)),
            (1, equip_on(b, robe, slots::BODY)),
        ],
        vec![
            (
                0,
                Action::UnequipFrom {
                    who: a,
                    slot: weapon,
                },
            ),
            (
                1,
                Action::Equip {
                    item: crown,
                    slot: slots::HEAD,
                },
            ),
            (1, sell(crown)),
            (1, sell(blades[0])),
        ],
        vec![
            (
                1,
                Action::UnequipFrom {
                    who: a,
                    slot: weapon,
                },
            ),
            (1, sell(blades[0])),
        ],
        Vec::new(),
        vec![
            (1, Action::LevelUp(picks(1))),
            (1, level_up_on(b)),
            (0, level_up_on(b)),
        ],
        vec![(1, Action::LevelUp(picks(START.0)))],
        vec![
            (0, equip_on(a, robe, slots::BODY)),
            (1, Action::LevelUp(picks(START.0))),
        ],
        vec![(0, level_up_on(a))],
    ];
    let (mut snapshots, mut states, mut notices) = (Vec::new(), Vec::new(), Vec::new());
    for (at, actions) in steps.into_iter().enumerate() {
        if at == 4 {
            let mut state = world.script_state().clone();
            state.xp.insert(PARTY, XP);
            world.set_script_state(state);
        }
        let mut input = TickInput::default();
        // A seat that sends no command has dropped.
        let present: &[u8] = if at < DROPPED { &[0, 1] } else { &[0] };
        for seat in present {
            input.insert(Seat(*seat), crate::Command::default());
        }
        for (seat, action) in actions {
            input.act(Seat(seat), action);
        }
        snapshots.push(crate::step(
            &mut world,
            at as u64 + 1,
            &input,
            8,
            StepReason::Authority,
        ));
        states.push(world.script_state().clone());
        notices.extend(
            world
                .take_notices()
                .into_iter()
                .map(|(seat, notice)| (at, seat, notice)),
        );
    }
    Ok(Ran {
        cast,
        snapshots,
        states,
        notices,
    })
}

/// The scenario's checks (`cases`), and that two runs end alike.
pub fn probes() -> Vec<(&'static str, Result<String, String>)> {
    let ran = match scenario() {
        Ok(ran) => ran,
        Err(why) => return vec![("coop_scenario", Err(why))],
    };
    let again = scenario().map(|again| again.snapshots.last().map(Snapshot::hash));
    let mut checks = cases(&ran);
    let last = ran.snapshots.last().map(Snapshot::hash);
    checks.push((
        "same_every_run",
        check(
            again.as_ref().is_ok_and(|hash| *hash == last),
            format!(
                "{:?} then {:?}",
                last.map(|hash| hash.short()),
                again.map(|hash| hash.map(|hash| hash.short()))
            ),
        ),
    ));
    checks
}

/// What the run shows, step by step.
struct Seen<'a>(&'a Ran);

impl Seen<'_> {
    fn told(&self, step: usize, seat: u8, notice: &Notice) -> usize {
        self.0
            .notices
            .iter()
            .filter(|(at, to, told)| *at == step && *to == Seat(seat) && told == notice)
            .count()
    }

    /// Where `item` is after `step`: its holder and slot.
    fn item(&self, step: usize, item: u32) -> Option<(u32, Option<u8>)> {
        let item = self.0.states[step].items.get(&item)?;
        Some((item.holder, item.slot))
    }

    /// `who`'s level and skill ranks after `step` (as placed until its
    /// sheet changed).
    fn sheet(&self, step: usize, who: u32) -> (u8, [u8; SKILLS]) {
        self.0.snapshots[step]
            .get(SimId(who))
            .and_then(|entity| entity.sheet)
            .map_or((START.1, [0; SKILLS]), |sheet| (sheet.level, sheet.skills))
    }

    /// The seat driving `who` after `step`.
    fn driver(&self, step: usize, who: u32) -> Option<Seat> {
        self.0.snapshots[step]
            .get(SimId(who))
            .and_then(|entity| entity.fighter)
            .and_then(|fighter| fighter.mind.driver)
    }

    /// The templates the party carries after `step`.
    fn party(&self, step: usize) -> Vec<String> {
        let state = &self.0.states[step];
        state
            .carried(PARTY)
            .iter()
            .filter_map(|id| Some(state.items.get(&id.0)?.template.clone()))
            .collect()
    }
}

fn cases(ran: &Ran) -> Vec<(&'static str, Result<String, String>)> {
    let seen = Seen(ran);
    let Cast {
        host,
        a,
        b,
        robe,
        crown,
        blades,
    } = ran.cast;
    let not_yours = |by: Option<u8>| Notice::NotYours { by: by.map(Seat) };
    let gone = Notice::Gone {
        item: "medpac".into(),
        by: Some(Seat(0)),
    };
    let body = ran.snapshots[TAKEN].body(Seat(1)).map(|entity| entity.id);
    vec![
        (
            "guest_drives_a_companion",
            check(
                body == Some(SimId(a)),
                format!("seat 1's body {body:?}, a is #{a}"),
            ),
        ),
        (
            "contested_take_lower_seat",
            check(
                seen.party(1).contains(&"medpac".to_string()) && seen.told(1, 1, &gone) == 1,
                format!("the party {:?}, notices {:?}", seen.party(1), ran.notices),
            ),
        ),
        (
            "guest_equips_its_companion",
            check(
                seen.item(1, blades[0]) == Some((a, Some(slots::RIGHT_WEAPON))),
                format!("blade {:?}", seen.item(1, blades[0])),
            ),
        ),
        (
            "worn_not_taken_by_equip",
            check(
                seen.told(2, 1, &Notice::CannotEquip) == 1
                    && seen.item(2, crown) == Some((host, Some(slots::HEAD))),
                format!("crown {:?}", seen.item(2, crown)),
            ),
        ),
        (
            "worn_not_for_sale",
            check(
                seen.told(2, 1, &Notice::NotForSale) == 2,
                format!("{:?}", ran.notices),
            ),
        ),
        (
            "actions_round_trip",
            check(
                [
                    Action::EquipOn {
                        who: a,
                        item: robe,
                        slot: slots::BODY,
                    },
                    Action::UnequipFrom {
                        who: a,
                        slot: slots::BODY,
                    },
                ]
                .iter()
                .all(|action| Action::from_bytes(&action.to_bytes()).ok().as_ref() == Some(action)),
                "EquipOn and UnequipFrom through their bytes".into(),
            ),
        ),
        (
            "taken_over_in_the_step",
            check(
                seen.told(TAKEN, 0, &not_yours(Some(1))) == 1
                    && seen.item(TAKEN, robe) == Some((PARTY, None)),
                format!(
                    "robe {:?}, notices {:?}",
                    seen.item(TAKEN, robe),
                    ran.notices
                ),
            ),
        ),
        (
            "guest_cannot_dress_the_host",
            check(
                seen.told(1, 1, &not_yours(Some(0))) == 1
                    && seen.item(1, robe) == Some((PARTY, None)),
                format!("robe {:?}", seen.item(1, robe)),
            ),
        ),
        (
            "host_equips_undriven",
            check(
                seen.item(1, blades[1]) == Some((b, Some(slots::RIGHT_WEAPON))),
                format!("second blade {:?}", seen.item(1, blades[1])),
            ),
        ),
        (
            "guest_cannot_dress_undriven",
            check(
                seen.told(1, 1, &not_yours(None)) == 1,
                format!("{:?}", ran.notices),
            ),
        ),
        (
            "host_cannot_unequip_driven",
            check(
                seen.told(2, 0, &not_yours(Some(1))) == 1
                    && seen.item(2, blades[0]) == Some((a, Some(slots::RIGHT_WEAPON))),
                format!("blade {:?}", seen.item(2, blades[0])),
            ),
        ),
        (
            "driver_unequips_and_sells",
            check(
                seen.item(3, blades[0]) == Some((STORE, None)) && ran.states[3].gold == 50,
                format!(
                    "blade {:?}, gold {}",
                    seen.item(3, blades[0]),
                    ran.states[3].gold
                ),
            ),
        ),
        (
            "undriven_waits_for_the_host",
            check(
                seen.sheet(4, b).0 == START.1
                    && seen.sheet(5, b).0 == START.1 + 1
                    && ran.states[5].careers.get(&b) == Some(&vec![(START.0, START.1 + 1)])
                    && seen.told(5, 1, &Notice::NotYours { by: None }) == 1,
                format!(
                    "b {:?} then {:?}, career {:?}, notices {:?}",
                    seen.sheet(4, b),
                    seen.sheet(5, b),
                    ran.states[5].careers.get(&b),
                    ran.notices
                ),
            ),
        ),
        (
            "driven_waits_for_its_guest",
            check(
                seen.sheet(4, a).0 == START.1,
                format!("a {:?}", seen.sheet(4, a)),
            ),
        ),
        (
            "wrong_class_told_its_guest",
            check(
                seen.told(5, 1, &Notice::LevelRefused) == 1
                    && seen.told(5, 0, &Notice::LevelRefused) == 0
                    && seen.sheet(5, a).0 == START.1,
                format!("a {:?}, notices {:?}", seen.sheet(5, a), ran.notices),
            ),
        ),
        (
            "guest_picks_its_level",
            check(
                seen.sheet(6, a).1[1] == seen.sheet(5, a).1[1] + 1
                    && seen.sheet(6, a).1[0] == seen.sheet(5, a).1[0],
                format!(
                    "a's skills {:?} then {:?}",
                    seen.sheet(5, a).1,
                    seen.sheet(6, a).1
                ),
            ),
        ),
        (
            "next_level_still_waits",
            check(
                seen.sheet(6, a).0 == START.1 + 1
                    && ran.states[6].careers.get(&a) == Some(&vec![(START.0, START.1 + 1)]),
                format!("a {:?}", seen.sheet(6, a)),
            ),
        ),
        (
            "drop_leaves_the_level_waiting",
            check(
                seen.driver(DROPPED, a).is_none() && seen.sheet(DROPPED, a).0 == START.1 + 1,
                format!(
                    "a {:?}, driver {:?}",
                    seen.sheet(DROPPED, a),
                    seen.driver(DROPPED, a)
                ),
            ),
        ),
        (
            "dropped_picks_not_taken",
            check(
                seen.sheet(DROPPED, a).1 == seen.sheet(6, a).1,
                format!(
                    "a's skills {:?} then {:?}",
                    seen.sheet(6, a).1,
                    seen.sheet(DROPPED, a).1
                ),
            ),
        ),
        (
            "host_levels_after_drop",
            check(
                seen.sheet(DROPPED + 1, a).0 == START.1 + 2,
                format!("a {:?}", seen.sheet(DROPPED + 1, a)),
            ),
        ),
        (
            "host_dresses_after_drop",
            check(
                seen.driver(DROPPED, a).is_none()
                    && seen.item(DROPPED, robe) == Some((a, Some(slots::BODY))),
                format!(
                    "driver {:?}, robe {:?}",
                    seen.driver(DROPPED, a),
                    seen.item(DROPPED, robe)
                ),
            ),
        ),
    ]
}
