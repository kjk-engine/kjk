//! `kjk sim probes --only workbench`: TSL's workbench, lab station and
//! upgrade bench (`inventory::workshop`, `script::workbench`,
//! `script::items::State::assemble`) on the proving ground, with tables,
//! items and scripts of our own; no install.
//!
//! - The workbench: a blaster worth 400 (a price of 20) is broken down with
//!   Repair 10 (half of it: 10 components), a part worth 200 (10) is made of
//!   them, each with the game's sound, then refused for want of
//!   components; a plot item, one worth nothing and one with upgrades
//!   fitted are not broken down; a recipe above the maker's rank is
//!   refused, one far above it unlisted; gloves of Repair +2 make the rank
//!   reach it.
//! - The lab station: only what its table lists outside its last group is
//!   broken down (not a pistol, not an HK part), its yield by Treat Injury; too few chemicals is its own
//!   line.
//! - Components are a count wherever they come: taken from a container;
//!   made on the player (none made, the game's "Acquired" line); made on a
//!   creature outside the party, and among its starting items, its own count;
//!   added to the party's as it joins, its own kept.
//! - The upgrade bench's Assemble: an upgrade above the item's level
//!   refuses the whole visit; its changes made together; a crystal fits
//!   whatever its level; a worn item its wearer may no longer wear goes to
//!   the party's pack.

use super::SimWorld;
use crate::action::{Action, Fitting, Workbench};
use crate::combat::Sheet;
use crate::input::{Command, Seat, TickInput};
use crate::inventory::properties::{Limit, Property};
use crate::inventory::workshop::{self, Pool, Recipe, Workshop};
use crate::inventory::{COLOUR_SLOT, Catalog, ItemFacts, PARTY, slots};
use crate::notice::{Notice, Screen};
use crate::probe::{Checks, check};
use crate::script::Pending;
use crate::script::workbench::{ACQUIRED_COMPONENTS, BROKEN, MADE, sound};
use crate::{StepReason, WorldConfig};
use fmt_odyssey::ncs::{Const, Ncs, Op};
use script_ncs::probes::Asm;
use std::sync::Arc;

/// The base items: an ordinary one, and the components'.
const PLAIN: u16 = 1;
const COMPONENTS: u16 = 96;

/// The companion the scripts make, its number and tag.
const NPC: i32 = 3;
const COMPANION: &str = "probe_bench_npc";

const CATALOG: &str = r#"
// 0
object CreateItemOnObject(string sItemTemplate, object oTarget = OBJECT_SELF, int nStackSize = 1);
// 1
object GetFirstPC();
// 2
int GetIsObjectValid(object oObject);
// 3
void SetCustomToken(int nCustomTokenNumber, string sTokenValue);
// 4
string IntToString(int nInteger);
// 5
int AddAvailableNPCByTemplate(int nNPC, string sTemplate);
// 6
object SpawnAvailableNPC(int nNPC, location lPosition);
// 7
location GetLocation(object oObject);
// 8
int AddPartyMember(int nNPC, object oCreature);
// 9
object GetObjectByTag(string sTag, int nNth = 0);
"#;

fn call(routine: u16, args: u8) -> Op {
    Op::Action { routine, args }
}

fn int(value: i32) -> Op {
    Op::Const(Const::Int(value))
}

fn text(value: &str) -> Op {
    Op::Const(Const::String(value.as_bytes().to_vec()))
}

/// The item made on the object on top, taken off it, kept valid or not as
/// custom token `n`.
fn made_on_top(asm: &mut Asm, stack: i32, n: i32) {
    asm.op(int(stack))
        .op(Op::CpTopSp {
            offset: -8,
            size: 4,
        })
        .op(text("compont"))
        .op(call(0, 3))
        .op(call(2, 1))
        .op(call(4, 1))
        .op(int(n))
        .op(call(3, 2));
}

/// `probe_pool`: five components made on the player (token 1 whether an
/// item came of it). `probe_hoard`: the companion made available, made
/// beside the player (its two components among its starting items), three
/// made on it at once (token 2). `probe_join`: it joins the party.
fn scripts() -> [(&'static str, Ncs); 3] {
    let mut pool = Asm::default();
    pool.op(call(1, 0));
    made_on_top(&mut pool, 5, 1);
    pool.op(Op::MovSp(-4)).op(Op::Retn);
    let mut hoard = Asm::default();
    hoard
        .op(text(COMPANION))
        .op(int(NPC))
        .op(call(5, 2))
        .op(Op::MovSp(-4));
    hoard
        .op(call(1, 0))
        .op(call(7, 1))
        .op(int(NPC))
        .op(call(6, 2));
    made_on_top(&mut hoard, 3, 2);
    hoard.op(Op::MovSp(-4)).op(Op::Retn);
    let mut join = Asm::default();
    join.op(int(0))
        .op(text(COMPANION))
        .op(call(9, 2))
        .op(int(NPC))
        .op(call(8, 2))
        .op(Op::MovSp(-4))
        .op(Op::Retn);
    [
        ("probe_pool", pool.build()),
        ("probe_hoard", hoard.build()),
        ("probe_join", join.build()),
    ]
}

fn item(template: &str, cost: u32, base_item: u16, plot: bool) -> ItemFacts {
    ItemFacts {
        cost,
        base_item,
        plot,
        stacking: 100,
        ..super::containers::probes::plain(template)
    }
}

/// An upgrade of kind `kind` (`upgradetypes.2da` row) and level `level`.
fn part(template: &str, kind: u8, level: u8, properties: Vec<Property>) -> ItemFacts {
    ItemFacts {
        upgrade: Some((0, kind)),
        upgrade_level: level,
        properties,
        ..item(template, 100, PLAIN, false)
    }
}

/// An item of TSL's slots of `kinds`, upgrade level 1, worn in `slot`.
fn host(template: &str, kinds: Vec<u8>, slot: u8) -> ItemFacts {
    ItemFacts {
        upgrade_level: 1,
        slot_kinds: kinds,
        slots: 1 << slot,
        stacking: 1,
        ..item(template, 1000, PLAIN, false)
    }
}

fn catalog() -> Catalog {
    let mut gloves = item("gloves", 100, PLAIN, false);
    gloves.slots = 1 << slots::HANDS;
    gloves.gear.skills[usize::from(workshop::REPAIR)] = 2;
    Catalog::new([
        item("blaster", 400, PLAIN, false),
        item("pistol", 400, PLAIN, false),
        item("part", 200, PLAIN, false),
        item("rifle", 4000, PLAIN, false),
        item("idol", 400, PLAIN, true),
        item("rag", 0, PLAIN, false),
        item("compont", 0, COMPONENTS, true),
        item("medkit", 400, PLAIN, false),
        item("hkpart", 400, PLAIN, false),
        gloves,
        host("scoped", vec![4, 5, 6], slots::RIGHT_WEAPON),
        host(
            "saber",
            vec![0, COLOUR_SLOT, 0, 1, 2, 3],
            slots::RIGHT_WEAPON,
        ),
        part("scope", 4, 1, Vec::new()),
        part("scope2", 4, 2, Vec::new()),
        part("pack", 5, 1, Vec::new()),
        part("crystal", 0, 3, Vec::new()),
        part(
            "lens",
            1,
            1,
            vec![Property::Limit(Limit::Tag("bastila".into()))],
        ),
    ])
}

/// The tables: the workbench's part (Repair 1), rifle (Repair 12: listed to
/// a rank of 4, not made) and an armour of Repair 30 (unlisted); the lab
/// station's medkit (Treat Injury) and an HK part in its last group.
fn workshop() -> Workshop {
    let recipe = |template: &str, skill, base_skill, group| Recipe {
        template: template.into(),
        skill,
        base_skill,
        group,
        level: 0,
        align: 0,
    };
    let bench = vec![
        recipe("part", 1, workshop::REPAIR, 4),
        recipe("rifle", 12, workshop::REPAIR, 4),
        recipe("blaster", 30, workshop::REPAIR, 4),
    ];
    let lab = vec![
        recipe("medkit", 1, workshop::TREAT_INJURY, 0),
        recipe("hkpart", 50, 1, workshop::UNMADE),
    ];
    Workshop {
        tables: [
            (workshop::BENCH.to_string(), bench),
            (workshop::LAB.to_string(), lab),
        ]
        .into(),
        pools: [(COMPONENTS, Pool::Components)].into(),
    }
}

fn world() -> SimWorld {
    let ground = crate::determinism::proving_ground();
    let mut companion =
        crate::module::ObjectData::creature(COMPANION, COMPANION, [0.0; 3], Sheet::dev_soldier());
    companion.inventory = vec![crate::inventory::Held {
        template: "compont".into(),
        stack: 2,
        slot: None,
        droppable: true,
        infinite: false,
    }];
    let library = crate::probe::library(CATALOG, scripts())
        .with_items(catalog())
        .with_workshop(workshop())
        .with_creatures([(COMPANION.to_string(), Arc::new(companion))].into());
    SimWorld::new(WorldConfig::one_seat(), ground.with_scripts(library))
        .expect("the proving ground makes a world")
}

/// One step, seat 0 doing `actions`: what it was told.
fn step(world: &mut SimWorld, actions: &[Action]) -> Vec<Notice> {
    let mut input = TickInput::default();
    input.insert(Seat(0), Command::default());
    for action in actions {
        input.act(Seat(0), action.clone());
    }
    let tick = world.tick() + 1;
    crate::step(world, tick, &input, 8, StepReason::Authority);
    world
        .take_notices()
        .into_iter()
        .map(|(_, notice)| notice)
        .collect()
}

/// Seat 0's `action` stepped: the components after, and what it was told.
fn act(world: &mut SimWorld, action: Action) -> (i32, Vec<Notice>) {
    let told = step(world, &[action]);
    (world.script_state().components, told)
}

/// `script` run, then stepped: what was told, and custom token `n`.
fn run(world: &mut SimWorld, script: &str, n: i32) -> (Vec<Notice>, Option<String>) {
    let mut state = world.script_state().clone();
    state
        .pending
        .push(Pending::new(script, crate::script::MODULE));
    world.set_script_state(state);
    let told = step(world, &[]);
    let token = world.snapshot().talk.tokens.get(&n).cloned();
    (told, token)
}

/// Items of `template` anywhere.
fn count(world: &SimWorld, template: &str) -> usize {
    world
        .script_state()
        .items
        .values()
        .filter(|held| held.template == template)
        .count()
}

/// `template` made for `holder` (worn in `slot`) by hand: its id.
fn give(world: &mut SimWorld, template: &str, holder: u32, slot: Option<u8>) -> u32 {
    let mut state = world.script_state().clone();
    let made = state
        .make_item(&catalog(), template, holder, 1, slot)
        .map_or(0, |id| id.0);
    world.set_script_state(state);
    made
}

pub fn probes() -> Checks {
    let mut world = world();
    let me = world
        .snapshot()
        .body(Seat(0))
        .map_or(0, |entity| entity.id.0);
    world.set_player_skill(usize::from(workshop::REPAIR), 10);
    world.set_player_skill(usize::from(workshop::TREAT_INJURY), 10);
    let mut made = |template: &str| give(&mut world, template, PARTY, None);
    let (blaster, idol, rag, fitted) = (made("blaster"), made("idol"), made("rag"), made("rifle"));
    let (medkit, hkpart, lab_blaster) = (made("medkit"), made("hkpart"), made("pistol"));
    let gloves = made("gloves");
    let crate_box = 0x7d00_0001;
    give(&mut world, "compont", crate_box, None);
    let mut state = world.script_state().clone();
    if let Some(mut held) = state.items.get_mut(&fitted) {
        held.fitted = vec!["part".into()];
    }
    if let Some(id) = state.carried(crate_box).first()
        && let Some(mut held) = state.items.get_mut(&id.0)
    {
        held.stack = 7;
    }
    world.set_script_state(state);
    let bench = Workbench {
        list: String::new(),
        lab: false,
        maker: me,
    };
    let lab = Workbench {
        lab: true,
        list: workshop::LAB.into(),
        ..bench.clone()
    };
    let breakdown = |bench: &Workbench, item| Action::Breakdown {
        bench: bench.clone(),
        item,
    };
    let create = |bench: &Workbench, row| Action::Create {
        bench: bench.clone(),
        row,
    };
    let mut checks: Checks = Vec::new();

    let (after, told) = act(&mut world, breakdown(&bench, blaster));
    let gone = !world.script_state().items.contains_key(&blaster);
    checks.push((
        "a breakdown: the price (400 / 20) by Repair 10 of 20, the item gone, the bench told, its sound",
        check(
            after == 10
                && gone
                && told.iter().any(|notice| {
                    matches!(notice, Notice::Show(Screen::Workbench(bench)) if bench.stock == 10)
                })
                && told.contains(&sound(BROKEN)),
            format!("{after} components, gone {gone}, told {told:?}"),
        ),
    ));
    let (after, told) = act(&mut world, create(&bench, 0));
    let parts = world
        .script_state()
        .items
        .values()
        .filter(|held| held.template == "part" && held.holder == PARTY)
        .count();
    checks.push((
        "made: its price (200 / 20) taken, one to the party, its sound",
        check(
            after == 0 && parts == 1 && told.contains(&sound(MADE)),
            format!("{after} components, {parts} parts, told {told:?}"),
        ),
    ));
    let (after, told) = act(&mut world, create(&bench, 0));
    checks.push((
        "too few components: refused with the game's line",
        check(
            after == 0 && told.contains(&Notice::Line(116_297)),
            format!("{after}, {told:?}"),
        ),
    ));
    let refusals: Vec<Vec<Notice>> = [idol, rag, fitted]
        .into_iter()
        .map(|item| act(&mut world, breakdown(&bench, item)).1)
        .collect();
    checks.push((
        "not broken down: a plot item, one worth nothing, one with upgrades fitted",
        check(
            refusals
                == vec![
                    vec![Notice::CannotUse],
                    vec![Notice::CannotUse],
                    vec![Notice::Line(124_119)],
                ],
            format!("{refusals:?}"),
        ),
    ));
    let mut state = world.script_state().clone();
    state.components = 1000;
    world.set_script_state(state);
    let rifle = act(&mut world, create(&bench, 1)).1;
    let armour = act(&mut world, create(&bench, 2)).1;
    checks.push((
        "a recipe above the rank refused with the game's line, one 8 above it unlisted",
        check(
            rifle.contains(&Notice::Line(116_292)) && armour.contains(&Notice::CannotUse),
            format!("{rifle:?}, {armour:?}"),
        ),
    ));
    step(
        &mut world,
        &[Action::Equip {
            item: gloves,
            slot: slots::HANDS,
        }],
    );
    let (after, told) = act(&mut world, create(&bench, 1));
    checks.push((
        "the rank with what the maker wears: Repair 10 and gloves of +2 make the rifle (12)",
        check(
            after == 1000 - 200 && told.contains(&sound(MADE)),
            format!("{after} components, told {told:?}"),
        ),
    ));

    let refused: Vec<Vec<Notice>> = [lab_blaster, hkpart]
        .into_iter()
        .map(|item| step(&mut world, &[breakdown(&lab, item)]))
        .collect();
    step(&mut world, &[breakdown(&lab, medkit)]);
    let chemicals = world.script_state().chemicals;
    let short = step(&mut world, &[create(&lab, 0)]);
    checks.push((
        "the lab station: a pistol and an HK part refused, a medkit by Treat Injury 10 (20 / 2), too few chemicals its own line",
        check(
            refused == vec![vec![Notice::CannotUse]; 2]
                && chemicals == 10
                && short.contains(&Notice::Line(116_298)),
            format!("refused {refused:?}, {chemicals} chemicals, then {short:?}"),
        ),
    ));

    let library = world.level().scripts().cloned();
    let mut state = world.script_state().clone();
    let before = state.components;
    let took = library.map(|library| state.take_rest(&library, crate_box));
    let counted = state.components - before;
    world.set_script_state(state);
    let kept = count(&world, "compont");
    checks.push((
        "components taken are the party's count, no item",
        check(
            took.is_some_and(|(any, gained)| any && gained.components == 7)
                && counted == 7
                && kept == 0,
            format!("took {took:?}, +{counted} components, items kept {kept}"),
        ),
    ));

    let before = world.script_state().components;
    let (told, valid) = run(&mut world, "probe_pool", 1);
    let gained = world.script_state().components - before;
    let line = Notice::Count {
        line: ACQUIRED_COMPONENTS,
        count: 5,
    };
    checks.push((
        "made on the player: the party's count (+5), no item made, the game's line",
        check(
            gained == 5
                && valid.as_deref() == Some("0")
                && count(&world, "compont") == 0
                && told.contains(&line),
            format!("+{gained}, an item {valid:?}, told {told:?}"),
        ),
    ));
    let before = world.script_state().components;
    let (_, valid) = run(&mut world, "probe_hoard", 2);
    step(&mut world, &[]);
    let state = world.script_state();
    let npc = state
        .party
        .available
        .get(&(NPC as u8))
        .and_then(|npc| npc.object);
    let hoard = npc.and_then(|npc| state.hoards.get(&npc).copied());
    let untouched = state.components == before;
    checks.push((
        "a creature outside the party: its starting two and three made on it are its own count, no item",
        check(
            hoard == Some((5, 0))
                && untouched
                && valid.as_deref() == Some("0")
                && count(&world, "compont") == 0,
            format!("its count {hoard:?}, the party's untouched {untouched}, an item {valid:?}"),
        ),
    ));
    run(&mut world, "probe_join", 0);
    let state = world.script_state();
    let joined = state.components - before;
    let kept = npc.and_then(|npc| state.hoards.get(&npc).copied());
    checks.push((
        "it joins: its count added to the party's, its own kept (Q-0232)",
        check(
            joined == 5 && kept == Some((5, 0)),
            format!("+{joined}, its own {kept:?}"),
        ),
    ));

    checks.extend(assembled(&mut world, me));
    checks
}

/// The upgrade bench's Assemble on a rifle the party carries and a
/// lightsaber the player wears.
fn assembled(world: &mut SimWorld, me: u32) -> Checks {
    let mut made = |template: &str| give(world, template, PARTY, None);
    let (rifle, scope, scope2, pack, crystal, lens) = (
        made("scoped"),
        made("scope"),
        made("scope2"),
        made("pack"),
        made("crystal"),
        made("lens"),
    );
    let saber = give(world, "saber", me, Some(slots::RIGHT_WEAPON));
    let assemble = |item, changes: &[Fitting]| Action::Assemble {
        item,
        changes: changes.to_vec(),
    };
    let fit = |upgrade| Fitting::In {
        upgrade,
        replace: None,
    };
    let fitted = |world: &SimWorld, item: u32| {
        world
            .script_state()
            .items
            .get(&item)
            .map_or_else(Vec::new, |held| held.fitted.clone())
    };
    let mut checks: Checks = Vec::new();
    let told = step(world, &[assemble(rifle, &[fit(pack), fit(scope2)])]);
    let unchanged = fitted(world, rifle).iter().all(String::is_empty)
        && world.script_state().items.contains_key(&pack);
    checks.push((
        "a level-2 scope in a level-1 rifle refuses the whole visit: the pack fitted with it is not",
        check(
            told.contains(&Notice::CannotUpgrade) && unchanged,
            format!("told {told:?}, unchanged {unchanged}"),
        ),
    ));
    step(world, &[assemble(rifle, &[fit(scope), fit(pack)])]);
    let both = fitted(world, rifle);
    let used = [scope, pack]
        .iter()
        .all(|part| !world.script_state().items.contains_key(part));
    checks.push((
        "Assemble makes its changes together: scope and pack fitted, taken from the party",
        check(
            both == ["scope", "pack", ""] && used,
            format!("fitted {both:?}, taken {used}"),
        ),
    ));
    step(world, &[assemble(saber, &[fit(crystal)])]);
    let worn = world
        .script_state()
        .items
        .get(&saber)
        .is_some_and(|held| held.holder == me && held.slot.is_some());
    let crystals = fitted(world, saber);
    checks.push((
        "a crystal of level 3 fits a level-1 lightsaber, which stays worn",
        check(
            crystals.first().map(String::as_str) == Some("crystal") && worn,
            format!("fitted {crystals:?}, worn {worn}"),
        ),
    ));
    step(world, &[assemble(saber, &[fit(lens)])]);
    let held = world.script_state().items.get(&saber).cloned();
    let packed = held
        .as_ref()
        .is_some_and(|held| held.holder == PARTY && held.slot.is_none());
    checks.push((
        "a lens only Bastila may wear: the lightsaber, off its wearer on the bench, goes to the pack",
        check(
            packed && held.as_ref().is_some_and(|held| held.fitted[3] == "lens"),
            format!("{held:?}"),
        ),
    ));
    checks
}
