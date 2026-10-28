//! The probes of the second weapon set (`kjk sim probes "weapon sets"`),
//! TSL's rules in both games, on the proving ground, no install:
//! - equipping: a second-set hand takes what its first-set hand takes
//!   (`inventory::slots::rules`), and nothing else;
//! - not worn: an item in the second set gives nothing (its weapon is not in
//!   hand, its guards not in force) until a swap brings it to the first;
//! - the player's swap (`Action::SwitchWeapons`): the sets exchanged at
//!   once, an empty second set leaving the hand empty and the first kept in
//!   the second; instant out of a fight, the round's second in one
//!   (`combat::SWAP_MS`); none in a conversation; the equip screen's
//!   (`SwitchWeaponsOn`) for a companion the host dresses;
//! - `ActionSwitchWeapons`: queued, it clears its creature's queue,
//!   exchanges the sets, and attacks on in combat;
//! - the save: a world saved and loaded keeps both sets and a queued swap.

use super::{Fighting, Id, SimWorld};
use crate::action::Action;
use crate::combat::Sheet;
use crate::combat::sheet::{Blaster, Delivery, Guards, Weapon, build};
use crate::input::{Command, Seat, TickInput};
use crate::inventory::{Catalog, ItemFacts, PARTY, slots};
use crate::module::{ObjectKind, Placement};
use crate::probe::{Checks, check};
use crate::script::Pending;
use crate::snapshot::{Kind, SimId, Snapshot};
use crate::{Level, StepReason, WorldConfig};
use fmt_odyssey::ncs::{Const, Op};
use script_ncs::vm::ObjectId;

const MSEC: u32 = 8;
/// The companion's tag and template.
const ALLY: &str = "probe_sets_ally";
/// The script that swaps its caller's sets, then waits (`probe_swap`).
const SWAP: &str = "probe_swap";
const CATALOG: &str = "// 0\nvoid ActionSwitchWeapons();\n// 1\nvoid ActionWait(float fSeconds);\n";
/// The guard the sword gives (`DAMAGE_TYPES` row 0, percent).
const SWORD_GUARD: i8 = 25;

/// A one-handed weapon of base item `base` (told apart by it) for `slot`,
/// melee or a pistol.
fn weapon(template: &str, base: u16, slot: u8) -> ItemFacts {
    let delivery = if matches!(template, "blaster" | "pistol") {
        Delivery::Blaster(Blaster::Pistol)
    } else {
        Delivery::Melee
    };
    let held = Weapon {
        base_item: base,
        delivery,
        build: build::ONE_HANDED,
        ..Weapon::UNARMED
    };
    ItemFacts {
        slots: 1 << slot,
        weapon: Some(held),
        bare: Some(held),
        ..super::containers::plain(template)
    }
}

fn catalog() -> Catalog {
    let mut sword = weapon("sword", 1, slots::RIGHT_WEAPON);
    sword.guards = Guards {
        immunity: std::array::from_fn(|row| if row == 0 { SWORD_GUARD } else { 0 }),
        ..Guards::default()
    };
    let mut staff = weapon("staff", 5, slots::RIGHT_WEAPON);
    if let Some(held) = staff.weapon.as_mut() {
        held.build = 0;
    }
    staff.bare = staff.weapon;
    Catalog::new([
        sword,
        staff,
        weapon("blaster", 2, slots::RIGHT_WEAPON),
        weapon("knife", 3, slots::LEFT_WEAPON),
        weapon("pistol", 4, slots::LEFT_WEAPON),
        ItemFacts {
            slots: 1 << slots::BODY,
            ..super::containers::plain("robe")
        },
    ])
}

/// `probe_swap`: `ActionSwitchWeapons(); ActionWait(30.0);`.
fn library() -> crate::script::Library {
    let mut asm = script_ncs::probes::Asm::default();
    asm.op(Op::Action {
        routine: 0,
        args: 0,
    })
    .op(Op::Const(Const::Float(30.0)))
    .op(Op::Action {
        routine: 1,
        args: 1,
    })
    .op(Op::Retn);
    crate::probe::library(CATALOG, [(SWAP, asm.build())]).with_items(catalog())
}

/// The proving ground with a companion of the party beside the player,
/// the world, the companion's id and the player's.
struct Ground {
    world: SimWorld,
    ally: u32,
    me: u32,
}

fn ground() -> Result<Ground, String> {
    let ground = crate::determinism::proving_ground();
    let mut data = (**ground.module().ok_or("the ground has no module")?).clone();
    let entry = data.entry.position;
    data.placements
        .push(Placement::Own(crate::module::ObjectData {
            sheet: Some(Sheet::dev_soldier()),
            template: ALLY.to_string(),
            ..crate::module::ObjectData::bare(
                ObjectKind::Creature,
                ALLY,
                [entry[0] + 1.5, entry[1], entry[2]],
            )
        }));
    let level: Level = ground.with_module(data).with_scripts(library());
    let mut world =
        SimWorld::new(WorldConfig::one_seat(), level).map_err(|error| error.to_string())?;
    let start = world.snapshot();
    let objects = crate::Instances::in_snapshot(world.level(), &start);
    let find = |wanted: &dyn Fn(&crate::snapshot::EntityState) -> bool| {
        start.entities.iter().find(|e| wanted(e)).map(|e| e.id.0)
    };
    let ally = find(&|e| objects.of_entity(e).is_some_and(|data| data.tag == ALLY))
        .ok_or("no companion")?;
    let me = find(&|e| matches!(e.kind, Kind::Avatar(_))).ok_or("no player")?;
    let mut state = world.script_state().clone();
    state.party.available.insert(
        0,
        crate::script::party::Npc {
            template: ALLY.to_string(),
            object: Some(ally),
            kept: None,
            template_data: None,
        },
    );
    state.party.members.push(0);
    world.set_script_state(state);
    Ok(Ground { world, ally, me })
}

impl Ground {
    /// One step, seat 0 doing `actions`.
    fn step(&mut self, actions: &[Action]) -> Snapshot {
        let mut input = TickInput::default();
        input.insert(Seat(0), Command::default());
        for action in actions {
            input.act(Seat(0), action.clone());
        }
        let tick = self.world.tick() + 1;
        crate::step(&mut self.world, tick, &input, MSEC, StepReason::Authority)
    }

    /// Items of `templates` made on `holder` (in `slot`, or carried).
    fn make(&mut self, holder: u32, made: &[(&str, Option<u8>)]) -> Vec<u32> {
        let mut state = self.world.script_state().clone();
        let ids = made
            .iter()
            .filter_map(|(template, slot)| {
                state
                    .make_item(&catalog(), template, holder, 1, *slot)
                    .map(|id| id.0)
            })
            .collect();
        self.world.set_script_state(state);
        ids
    }

    /// What `who` has in each weapon slot, by template (sets 1 then 2,
    /// right then left; "-" for none).
    fn sets(&self, who: u32) -> [&'static str; 4] {
        let state = self.world.script_state();
        let names = ["sword", "blaster", "knife", "pistol", "staff"];
        let [[r1, l1], [r2, l2]] = slots::SETS;
        [r1, l1, r2, l2].map(|slot| {
            state
                .in_slot(who, slot)
                .and_then(|id| state.items.get(&id.0))
                .and_then(|item| names.iter().find(|name| **name == item.template))
                .copied()
                .unwrap_or("-")
        })
    }

    fn fighter(&mut self, who: u32, change: impl Fn(&mut crate::combat::Fighter)) {
        let mut fighters = self.world.world.query::<(&Id, &mut Fighting)>();
        for (id, mut fighting) in fighters.iter_mut(&mut self.world.world) {
            if id.0.0 == who {
                change(&mut fighting.0);
            }
        }
    }

    fn weapon_wait_ms(&mut self, who: u32) -> i32 {
        let mut fighters = self.world.world.query::<(&Id, &Fighting)>();
        fighters
            .iter(&self.world.world)
            .find(|(id, _)| id.0.0 == who)
            .map_or(i32::MIN, |(_, fighting)| fighting.0.weapon_wait_ms)
    }

    /// `who`'s right hand (its base item) and first guard, as the snapshot
    /// carries its sheet.
    fn hand(snapshot: &Snapshot, who: u32) -> (Option<u16>, i8) {
        let sheet = snapshot.get(SimId(who)).and_then(|entity| entity.sheet);
        (
            sheet.and_then(|sheet| sheet.weapons[0].map(|weapon| weapon.base_item)),
            sheet.map_or(0, |sheet| sheet.guards.immunity[0]),
        )
    }
}

/// A check run on its own.
type Case = fn() -> Result<String, String>;

pub fn probes() -> Checks {
    let cases: [(&'static str, Case); 9] = [
        ("set_two_takes_set_one_rules", rules),
        ("hands_pair", hands),
        ("set_two_is_not_worn", not_worn),
        ("player_swap", player_swap),
        ("swap_in_a_fight_takes_a_second", fight_swap),
        ("equip_screen_swaps_a_companion", companion),
        ("no_swap_in_a_conversation", talking),
        ("action_switch_weapons", routine),
        ("save_keeps_both_sets", saved),
    ];
    cases.into_iter().map(|(name, run)| (name, run())).collect()
}

/// A blaster into the second right hand and a pistol into the second left
/// are worn there; a robe into a second hand and the blaster into the
/// second left are refused.
fn rules() -> Result<String, String> {
    let mut ground = ground()?;
    ground.step(&[]);
    let [blaster, pistol, robe] = ground.make(
        PARTY,
        &[("blaster", None), ("pistol", None), ("robe", None)],
    )[..] else {
        return Err("items not made".into());
    };
    let equip = |item, slot| Action::Equip { item, slot };
    ground.step(&[
        equip(robe, slots::RIGHT_WEAPON_2),
        equip(blaster, slots::LEFT_WEAPON_2),
    ]);
    let refused = ground.sets(ground.me);
    // The off hand pairs with the main hand's weapon (`hands`), so in turn.
    ground.step(&[equip(blaster, slots::RIGHT_WEAPON_2)]);
    ground.step(&[equip(pistol, slots::LEFT_WEAPON_2)]);
    let worn = ground.sets(ground.me);
    check(
        refused == ["-"; 4] && worn == ["-", "-", "blaster", "pistol"],
        format!("refused {refused:?}, then {worn:?}"),
    )
}

/// The hands pair (`inventory::pairs`): an off hand takes nothing beside an
/// empty main hand, no pistol beside a sword, a knife; a two-handed staff
/// sends the knife back; the main hand taken off takes the off hand with
/// it; and a swap's new hands leave the dev sheet's own left pistol behind.
fn hands() -> Result<String, String> {
    let mut swapped = ground()?;
    let mut ground = ground()?;
    let base = |snapshot: &Snapshot, me: u32| {
        let sheet = snapshot.get(SimId(me)).and_then(|entity| entity.sheet);
        [0, 1].map(|hand| sheet.and_then(|sheet| sheet.weapons[hand].map(|w| w.base_item)))
    };
    let dev = base(&ground.step(&[]), ground.me);
    let me = ground.me;
    let [sword, knife, pistol, staff] = ground.make(
        PARTY,
        &[
            ("sword", None),
            ("knife", None),
            ("pistol", None),
            ("staff", None),
        ],
    )[..] else {
        return Err("items not made".into());
    };
    let equip = |item, slot| Action::Equip { item, slot };
    ground.step(&[equip(knife, slots::LEFT_WEAPON)]);
    let alone = ground.sets(me);
    ground.step(&[equip(sword, slots::RIGHT_WEAPON)]);
    ground.step(&[equip(pistol, slots::LEFT_WEAPON)]);
    let mixed = ground.sets(me);
    ground.step(&[equip(knife, slots::LEFT_WEAPON)]);
    let pair = ground.sets(me);
    ground.step(&[equip(staff, slots::RIGHT_WEAPON)]);
    let two_handed = ground.sets(me);
    ground.step(&[equip(sword, slots::RIGHT_WEAPON)]);
    ground.step(&[equip(knife, slots::LEFT_WEAPON)]);
    ground.step(&[Action::Unequip(slots::RIGHT_WEAPON)]);
    let off = ground.sets(me);
    swapped.step(&[]);
    swapped.make(swapped.me, &[("sword", Some(slots::RIGHT_WEAPON_2))]);
    swapped.step(&[Action::SwitchWeapons]);
    let hands = base(&swapped.step(&[]), swapped.me);
    check(
        alone == ["-"; 4]
            && mixed == ["sword", "-", "-", "-"]
            && pair == ["sword", "knife", "-", "-"]
            && two_handed == ["staff", "-", "-", "-"]
            && off == ["-"; 4]
            && dev[1].is_some()
            && hands == [Some(1), None],
        format!(
            "knife alone {alone:?}; pistol by the sword {mixed:?}; knife {pair:?}; staff \
             {two_handed:?}; main hand off {off:?}; dev hands {dev:?}, swapped {hands:?}"
        ),
    )
}

/// The sword in the second set leaves the hand and the guards as they
/// were; swapped to the first, its weapon is in hand and its guard in force.
fn not_worn() -> Result<String, String> {
    let mut ground = ground()?;
    let before = ground.step(&[]);
    let [sword] = ground.make(PARTY, &[("sword", None)])[..] else {
        return Err("no sword".into());
    };
    let me = ground.me;
    ground.step(&[Action::Equip {
        item: sword,
        slot: slots::RIGHT_WEAPON_2,
    }]);
    let held = ground.step(&[]);
    ground.step(&[Action::SwitchWeapons]);
    let swapped = ground.step(&[]);
    let (was, ready, now) = (
        Ground::hand(&before, me),
        Ground::hand(&held, me),
        Ground::hand(&swapped, me),
    );
    check(
        ready == was && now == (Some(1), SWORD_GUARD),
        format!("hand and guard: {was:?}, the sword in set 2 {ready:?}, swapped in {now:?}"),
    )
}

/// The player's swap exchanges both hands of both sets; again with the
/// second set empty, the hand is left empty and the first set kept in the
/// second. Out of a fight the weapon does not wait.
fn player_swap() -> Result<String, String> {
    let mut ground = ground()?;
    ground.step(&[]);
    let me = ground.me;
    ground.make(
        me,
        &[
            ("blaster", Some(slots::RIGHT_WEAPON)),
            ("sword", Some(slots::RIGHT_WEAPON_2)),
            ("knife", Some(slots::LEFT_WEAPON_2)),
        ],
    );
    ground.step(&[Action::SwitchWeapons]);
    let once = ground.sets(me);
    let waits = ground.weapon_wait_ms(me);
    ground.step(&[Action::SwitchWeapons]);
    let back = ground.sets(me);
    let mut empty = Ground {
        world: ground.world,
        ..ground
    };
    let robe = empty.make(me, &[("robe", Some(slots::BODY))]);
    let mut state = empty.world.script_state().clone();
    for slot in [slots::RIGHT_WEAPON_2, slots::LEFT_WEAPON_2] {
        if let Some(id) = state.in_slot(me, slot) {
            state.items.remove(&id.0);
        }
    }
    empty.world.set_script_state(state);
    empty.step(&[Action::SwitchWeapons]);
    let bare = empty.sets(me);
    let robe_kept = robe
        .first()
        .and_then(|id| empty.world.script_state().items.get(id))
        .is_some_and(|item| item.slot == Some(slots::BODY));
    check(
        once == ["sword", "knife", "blaster", "-"]
            && back == ["blaster", "-", "sword", "knife"]
            && bare == ["-", "-", "blaster", "-"]
            && robe_kept
            && waits <= 0,
        format!(
            "swapped {once:?}, back {back:?}, set 2 empty {bare:?} (robe kept {robe_kept}); \
             weapon waits {waits} ms out of a fight"
        ),
    )
}

/// In a fight the swap is made and the weapon waits `SWAP_MS` after it.
fn fight_swap() -> Result<String, String> {
    let mut ground = ground()?;
    ground.step(&[]);
    let me = ground.me;
    ground.make(me, &[("sword", Some(slots::RIGHT_WEAPON_2))]);
    ground.fighter(me, |fighter| {
        fighter.combat_until = u64::MAX / 2;
        fighter.weapon_wait_ms = 0;
    });
    ground.step(&[Action::SwitchWeapons]);
    let sets = ground.sets(me);
    let waits = ground.weapon_wait_ms(me);
    let most = crate::combat::SWAP_MS;
    check(
        sets[0] == "sword" && (most - 2 * MSEC as i32..=most).contains(&waits),
        format!("{sets:?}, the weapon waits {waits} ms (the round's {most})"),
    )
}

/// The equip screen's swap on the companion the host dresses exchanges
/// its sets, not the player's.
fn companion() -> Result<String, String> {
    let mut ground = ground()?;
    ground.step(&[]);
    let (ally, me) = (ground.ally, ground.me);
    ground.make(ally, &[("blaster", Some(slots::RIGHT_WEAPON))]);
    ground.make(me, &[("sword", Some(slots::RIGHT_WEAPON_2))]);
    ground.step(&[Action::SwitchWeaponsOn { who: ally }]);
    let (its, mine) = (ground.sets(ally), ground.sets(me));
    check(
        its == ["-", "-", "blaster", "-"] && mine == ["-", "-", "sword", "-"],
        format!("the companion's {its:?}, the player's {mine:?}"),
    )
}

/// A player standing in a conversation swaps nothing.
fn talking() -> Result<String, String> {
    let mut ground = ground()?;
    ground.step(&[]);
    let me = ground.me;
    ground.make(me, &[("sword", Some(slots::RIGHT_WEAPON_2))]);
    let mut state = ground.world.script_state().clone();
    state
        .talk
        .conversations
        .push(crate::script::dialogue::Conversation {
            listener: ObjectId(me),
            pc: ObjectId(me),
            ..Default::default()
        });
    ground.world.set_script_state(state);
    ground.step(&[Action::SwitchWeapons]);
    let sets = ground.sets(me);
    check(
        sets == ["-", "-", "sword", "-"],
        format!("in a conversation: {sets:?}"),
    )
}

/// `probe_swap` run by the player (whose target no mind of its own picks
/// again): its sets exchanged, the wait queued after the swap cleared with
/// it; in combat with a target (the companion), an attack on that target
/// queued instead.
fn routine() -> Result<String, String> {
    let run = |fighting: bool| -> Result<([&'static str; 4], Vec<&'static str>), String> {
        let mut ground = ground()?;
        ground.step(&[]);
        let (ally, me) = (ground.ally, ground.me);
        ground.make(
            me,
            &[
                ("blaster", Some(slots::RIGHT_WEAPON)),
                ("sword", Some(slots::RIGHT_WEAPON_2)),
            ],
        );
        if fighting {
            ground.fighter(me, |fighter| {
                fighter.combat_until = u64::MAX / 2;
                fighter.target = Some(SimId(ally));
            });
        }
        let mut state = ground.world.script_state().clone();
        state.pending.push(Pending::new(SWAP, ObjectId(me)));
        ground.world.set_script_state(state);
        ground.step(&[]);
        let queue = ground
            .world
            .script_state()
            .actions
            .get(&me)
            .map(|queue| queue.iter().map(|q| q.action.kind()).collect())
            .unwrap_or_else(Vec::new);
        Ok((ground.sets(me), queue))
    };
    let (calm, calm_queue) = run(false)?;
    let (fight, fight_queue) = run(true)?;
    let swapped = ["sword", "-", "blaster", "-"];
    check(
        calm == swapped && calm_queue.is_empty() && fight == swapped && fight_queue == ["attack"],
        format!(
            "out of combat {calm:?}, queue {calm_queue:?}; in combat {fight:?}, queue {fight_queue:?}"
        ),
    )
}

/// A world saved with both sets filled and a swap queued loads with them
/// (`save::Save`), and the swap then runs.
fn saved() -> Result<String, String> {
    let mut ground = ground()?;
    ground.step(&[]);
    let ally = ground.ally;
    ground.make(
        ally,
        &[
            ("blaster", Some(slots::RIGHT_WEAPON)),
            ("sword", Some(slots::RIGHT_WEAPON_2)),
            ("knife", Some(slots::LEFT_WEAPON_2)),
        ],
    );
    let mut state = ground.world.script_state().clone();
    state.queue(
        ObjectId(ally),
        crate::script::actions::Action::SwitchWeapons,
    );
    ground.world.set_script_state(state);
    let before = ground.sets(ally);
    let level = ground.world.level().clone();
    let save = crate::Save {
        hz: crate::TICK_HZ,
        level: level.id(),
        snapshot: ground.world.snapshot(),
        script: ground.world.script_state().clone(),
    };
    let loaded = crate::Save::decode(&save.encode()).map_err(|error| error.to_string())?;
    let queued = loaded.script.actions.get(&ally).is_some_and(|queue| {
        queue
            .iter()
            .any(|queued| queued.action == crate::script::actions::Action::SwitchWeapons)
    });
    let world = loaded.world(&level, 1, None)?;
    let mut again = Ground { world, ..ground };
    let kept = again.sets(ally);
    again.step(&[]);
    let after = again.sets(ally);
    check(
        kept == before && queued && after == ["sword", "knife", "blaster", "-"],
        format!("saved {before:?}, loaded {kept:?} (swap queued {queued}), then {after:?}"),
    )
}
