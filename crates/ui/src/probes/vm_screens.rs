//! The screens the games' scripts open (and the journal) over layouts
//! holding KOTOR's tags, on hand-written view-models.

use fmt_odyssey::gui::{Control, Kind};

use super::chargen::{chargen_vm, layouts as chargen_layouts, presses};
use super::menus::{keys, root};
use super::paint::{Fake, button};
use super::{Check, check, expect};
use crate::input::Key;
use crate::intent::{Fitting, GameAction, Intent};
use crate::screen::Stack;
use crate::vm::{
    BreakdownView, CharacterView, Entry, ItemKind, LevelUpView, LostView, MemberView, RecipeView,
    StoreView, UpgradeChoice, UpgradeItem, UpgradeKind, UpgradeSlot, ViewModel, Ware,
    WorkbenchView,
};

pub(super) fn list(tag: &str, top: i32) -> Control {
    Control {
        kind: Kind::ListBox,
        proto: Some(Box::new(button("PROTOITEM", [0, top, 100, 20], b""))),
        ..button(tag, [0, top, 100, 100], b"")
    }
}

pub(super) fn label(tag: &str, top: i32) -> Control {
    Control {
        kind: Kind::Label,
        ..button(tag, [200, top, 100, 10], b"")
    }
}

fn layouts(assets: &mut Fake) {
    chargen_layouts(assets);
    let buttons = |tags: &[&str]| -> Vec<Control> {
        tags.iter()
            .enumerate()
            .map(|(at, tag)| button(tag, [400, 10 + at as i32 * 12, 100, 10], b""))
            .collect()
    };
    let with = |mut controls: Vec<Control>, more: Vec<Control>| {
        controls.extend(more);
        root(640, 480, controls)
    };
    for (name, gui) in [
        ("top", root(640, 480, Vec::new())),
        (
            "journal",
            with(
                vec![list("LB_ITEMS", 0), list("LBL_ITEM_DESCRIPTION", 200)],
                buttons(&["BTN_QUESTITEMS", "BTN_EXIT"]),
            ),
        ),
        (
            "store",
            with(
                vec![
                    list("LB_SHOPITEMS", 0),
                    list("LB_INVITEMS", 120),
                    list("LB_DESCRIPTION", 240),
                    label("LBL_COST_VALUE", 0),
                    label("LBL_STOCK_VALUE", 12),
                    label("LBL_CREDITS_VALUE", 24),
                    label("LBL_BUYSELL", 36),
                ],
                buttons(&[
                    "BTN_Accept",
                    "BTN_Examine",
                    "BTN_Cancel",
                    "BTN_ALL",
                    "BTN_WEAPONS",
                ]),
            ),
        ),
        (
            "upgradesel",
            root(
                640,
                480,
                [
                    vec![list("LB_UPGRADELIST", 0)],
                    buttons(&[
                        "BTN_RANGED",
                        "BTN_LIGHTSABER",
                        "BTN_UPGRADEITEMS",
                        "BTN_BACK",
                    ]),
                ]
                .concat(),
            ),
        ),
        (
            "upgrade",
            root(
                640,
                480,
                [
                    vec![list("LB_ITEMS", 0)],
                    buttons(&[
                        "BTN_UPGRADE31",
                        "BTN_UPGRADE31_LS",
                        "BTN_UPGRADE32_LS",
                        "BTN_UPGRADE33_LS",
                        "BTN_UPGRADE34_LS",
                        "BTN_ASSEMBLE",
                        "BTN_BACK",
                    ]),
                ]
                .concat(),
            ),
        ),
        (
            "confirm",
            with(vec![list("LB_MESSAGE", 0)], buttons(&["BTN_OK"])),
        ),
        (
            "component",
            with(
                vec![list("LB_SHOPITEMS", 0), list("LB_INVITEMS", 120)],
                buttons(&[
                    "BTN_CREATE_ARMOR",
                    "BTN_CREATE_RANGED",
                    "BTN_CREATE_MELEE",
                    "BTN_CREATE_LIGHTSABER",
                    "BTN_CREATE_MISC",
                    "BTN_BREAK_ALL",
                    "BTN_BREAK_WEAPONS",
                    "BTN_Accept",
                    "BTN_Examine",
                    "BTN_Cancel",
                ]),
            ),
        ),
        (
            "chemical",
            with(
                vec![list("LB_SHOPITEMS", 0), list("LB_INVITEMS", 120)],
                buttons(&[
                    "BTN_CREATE_HEALTH",
                    "BTN_CREATE_STIMS",
                    "BTN_CREATE_GRENADES",
                    "BTN_CREATE_MINES",
                    "BTN_Accept",
                    "BTN_Examine",
                    "BTN_Cancel",
                ]),
            ),
        ),
        (
            "leveluppnl",
            root(
                640,
                480,
                buttons(&[
                    "BTN_STEPNAME1",
                    "BTN_STEPNAME2",
                    "BTN_STEPNAME3",
                    "BTN_STEPNAME4",
                    "BTN_STEPNAME5",
                    "BTN_BACK",
                ]),
            ),
        ),
    ] {
        assets.layouts.insert(name, gui);
    }
}

fn named(name: &str, description: &str, count: Option<u32>) -> Entry {
    Entry {
        name: name.as_bytes().to_vec(),
        description: description.as_bytes().to_vec(),
        count,
        ..Entry::default()
    }
}

/// A ware: `item`, its name, description and count, its price; a useable
/// one but for a rifle.
fn ware(item: u32, name: &str, description: &str, count: Option<u32>, price: i32) -> Ware {
    let kind = if name == "Rifle" {
        ItemKind::Weapon
    } else {
        ItemKind::Useable
    };
    Ware {
        item,
        entry: Entry {
            kind: Some(kind),
            ..named(name, description, count)
        },
        price,
    }
}

/// A list's rows' words.
fn listed(stack: &Stack, vm: &ViewModel, tag: &str) -> Vec<String> {
    stack
        .state_of(tag, vm)
        .and_then(|state| state.list)
        .map_or(Vec::new(), |list| {
            list.rows
                .iter()
                .map(|row| String::from_utf8_lossy(&row.text).into_owned())
                .collect()
        })
}

pub(super) fn opened(name: &str, assets: &mut Fake, vm: &ViewModel) -> Stack {
    let mut stack = Stack::new(crate::screens::open);
    stack.open(name, vm);
    stack.step(assets, vm, &keys(&[]));
    stack
}

/// The first row's words of a list fitted to its text.
fn told(stack: &Stack, vm: &ViewModel, tag: &str) -> String {
    stack
        .state_of(tag, vm)
        .and_then(|state| state.list)
        .and_then(|list| list.rows.first().cloned())
        .map_or(String::new(), |row| {
            String::from_utf8_lossy(&row.text).into_owned()
        })
}

pub(super) fn words(stack: &Stack, vm: &ViewModel, tag: &str) -> String {
    String::from_utf8_lossy(&stack.text_of(tag, vm).unwrap_or(Vec::new())).into_owned()
}

pub(super) fn disabled(stack: &Stack, vm: &ViewModel, tag: &str) -> bool {
    stack.state_of(tag, vm).is_some_and(|state| state.disabled)
}

/// The workbench and the lab station as TSL opens them: the workbench on
/// its other tab, its close button Back where upgrading may be chosen,
/// else Close; the lab station on its medical tab, its third button mines
/// and fourth grenades, all it breaks down listed (no filters).
fn benches(assets: &mut Fake) -> Check {
    let recipe = |group: u8, name: &str| RecipeView {
        group,
        entry: named(name, "", None),
        ..RecipeView::default()
    };
    let broken = |item: u32, name: &str, kind| BreakdownView {
        item,
        entry: Entry {
            kind: Some(kind),
            ..named(name, "", None)
        },
        yields: 1,
    };
    let bench = |upgrade: bool, groups: &[&str]| ViewModel {
        workbench: Some(WorkbenchView {
            upgrade,
            recipes: groups
                .iter()
                .enumerate()
                .map(|(group, name)| recipe(group as u8, name))
                .collect(),
            breakdowns: vec![
                broken(1, "Blaster", ItemKind::Weapon),
                broken(2, "Medpac", ItemKind::Useable),
            ],
            ..WorkbenchView::default()
        }),
        ..ViewModel::default()
    };
    let kinds = ["Armour", "Ranged", "Melee", "Saber", "Other"];
    let close = |upgrade: bool, assets: &mut Fake| {
        let vm = bench(upgrade, &kinds);
        let stack = opened("workbench", assets, &vm);
        let state = stack.state_of("BTN_Cancel", &vm);
        state.and_then(|state| state.strref)
    };
    let vm = bench(true, &kinds);
    let workbench = opened("workbench", assets, &vm);
    let opens = (
        listed(&workbench, &vm, "LB_SHOPITEMS"),
        workbench
            .state_of("BTN_CREATE_MISC", &vm)
            .is_some_and(|state| state.selected),
    );
    let closes = (close(true, assets), close(false, assets));
    let vm = bench(false, &["Medical", "Stims", "Mines", "Grenades"]);
    let mut lab = opened("chemical", assets, &vm);
    let mut tab = |tag: &str| {
        presses(&mut lab, assets, &vm, &[tag]);
        listed(&lab, &vm, "LB_SHOPITEMS")
    };
    let tabs = [
        tab("BTN_CREATE_STIMS"),
        tab("BTN_CREATE_MINES"),
        tab("BTN_CREATE_GRENADES"),
    ];
    let medical = bench(false, &["Medical"]);
    let first = listed(
        &opened("chemical", assets, &medical),
        &medical,
        "LB_SHOPITEMS",
    );
    presses(&mut lab, assets, &vm, &["BTN_Examine"]);
    let broken_down = listed(&lab, &vm, "LB_INVITEMS");
    check(
        "bench_screens",
        expect(
            &format!(
                "the workbench opens on {opens:?}, closes with {closes:?}; the lab station opens \
                 on {first:?}, its tabs {tabs:?}, breaks down {broken_down:?}"
            ),
            opens == (vec!["Other".into()], true)
                && closes == (Some(219), Some(1582))
                && first == ["Medical"]
                && tabs == [["Stims"], ["Mines"], ["Grenades"]]
                && broken_down == ["Blaster", "Medpac"],
        )
        .map(|()| "the workbench's and lab station's tabs, close button and breakdown list".into()),
    )
}

/// The image a control shows (an upgrade slot's).
fn icon(stack: &Stack, vm: &ViewModel, tag: &str) -> String {
    stack
        .state_of(tag, vm)
        .and_then(|state| state.fill)
        .unwrap_or(String::new())
}

/// An upgrade slot chosen and its list's row `row` (by the cursor, from
/// the list's top): what was raised.
fn pick(
    bench: &mut Stack,
    assets: &mut Fake,
    vm: &ViewModel,
    (slot, row): (&str, usize),
) -> Vec<Intent> {
    presses(bench, assets, vm, &[slot]);
    bench.step(assets, vm, &keys(&vec![Key::Down; row]));
    presses(bench, assets, vm, &["LB_ITEMS"])
}

pub(super) fn all() -> Vec<Check> {
    let mut assets = Fake::new();
    layouts(&mut assets);
    // A lightsaber: its blue colour crystal, a Damind crystal (row 1) and
    // an empty crystal slot; the party carries a Rubat crystal and a lens of
    // a level above the saber's.
    let shown = |name: &str, icon: &str| Entry {
        icon: Some(icon.into()),
        ..named(name, "", None)
    };
    let choice = |id, name: &str, fits| UpgradeChoice {
        id,
        entry: shown(name, &format!("i_{}", name.to_lowercase())),
        fits,
    };
    let rubat = choice(41, "Rubat", true);
    let saber = UpgradeItem {
        id: 30,
        item: named("Lightsaber", "", None),
        template: "g_w_lghtsbr01".into(),
        kind: UpgradeKind::Lightsaber,
        slots: vec![
            UpgradeSlot {
                fitted: Some((None, shown("Blue", "i_blue"))),
                choices: Vec::new(),
                at: None,
            },
            UpgradeSlot {
                fitted: Some((Some(1), shown("Damind", "i_damind"))),
                choices: vec![rubat.clone()],
                at: None,
            },
            UpgradeSlot {
                fitted: None,
                choices: vec![rubat, choice(42, "Lens", false)],
                at: None,
            },
        ],
    };
    let vm = ViewModel {
        journal: vec![named("Escape", "Find Bastila.", None)],
        store: Some(StoreView {
            store: 7,
            stock: vec![
                ware(11, "Medpac", "Heals.", Some(5), 50),
                ware(12, "Stim", "Boosts.", Some(2), 120),
                ware(13, "Rifle", "Fires.", None, 400),
            ],
            offers: vec![ware(21, "Blaster", "Shoots.", Some(1), 40)],
            credits: 300,
            sells: true,
            buys: true,
        }),
        // A leader of level 4: a trade of up to 200 credits asks nothing.
        party: vec![MemberView {
            level: 4,
            ..MemberView::default()
        }],
        upgrades: vec![
            UpgradeItem {
                item: named("Blaster", "", None),
                ..UpgradeItem::default()
            },
            saber,
        ],
        levelup: Some(LevelUpView {
            class: 0,
            level: 4,
            skills: [2, 0, 0, 0, 0, 0, 0, 0],
            picks: crate::vm::PicksView::default(),
        }),
        character: Some(CharacterView {
            abilities: [14, 12, 12, 10, 10, 10],
            ..CharacterView::default()
        }),
        ..chargen_vm()
    };

    let mut journal = opened("journal", &mut assets, &vm);
    presses(&mut journal, &mut assets, &vm, &["LB_ITEMS"]);
    let quest = told(&journal, &vm, "LBL_ITEM_DESCRIPTION");

    // The stim chosen and bought; the rifle (never runs out) too dear,
    // K1's "not enough credits" told; the sell list's blaster sold; Close
    // leaves.
    let mut store = opened("store", &mut assets, &vm);
    store.step(&mut assets, &vm, &keys(&[Key::Down]));
    presses(&mut store, &mut assets, &vm, &["LB_SHOPITEMS"]);
    let chosen = (
        words(&store, &vm, "LBL_COST_VALUE"),
        words(&store, &vm, "LBL_STOCK_VALUE"),
        words(&store, &vm, "LBL_CREDITS_VALUE"),
        told(&store, &vm, "LB_DESCRIPTION"),
        disabled(&store, &vm, "BTN_Accept"),
    );
    let bought = presses(&mut store, &mut assets, &vm, &["BTN_Accept"]);
    // Back to the list, one row down.
    presses(&mut store, &mut assets, &vm, &["LB_SHOPITEMS"]);
    store.step(&mut assets, &vm, &keys(&[Key::Down]));
    presses(&mut store, &mut assets, &vm, &["LB_SHOPITEMS"]);
    let short = presses(&mut store, &mut assets, &vm, &["BTN_Accept"]);
    let dear = (
        words(&store, &vm, "LBL_STOCK_VALUE"),
        store.top().map(|screen| screen.0),
        short.iter().any(|intent| matches!(intent, Intent::Game(_))),
    );
    presses(&mut store, &mut assets, &vm, &["BTN_OK"]);
    presses(&mut store, &mut assets, &vm, &["BTN_Examine"]);
    let selling = (
        store.hidden("LB_SHOPITEMS", &vm) && !store.hidden("LB_INVITEMS", &vm),
        words(&store, &vm, "LBL_BUYSELL"),
        words(&store, &vm, "BTN_Accept"),
        disabled(&store, &vm, "BTN_Accept"),
    );
    let sold = presses(&mut store, &mut assets, &vm, &["LB_INVITEMS", "BTN_Accept"]);
    let left = presses(&mut store, &mut assets, &vm, &["BTN_Cancel"]);
    let store_closed = !store.is_open();
    let game = |intents: &[Intent]| -> Vec<GameAction> {
        intents
            .iter()
            .filter_map(|intent| match intent {
                Intent::Game(action) => Some(action.clone()),
                _ => None,
            })
            .collect()
    };

    // Weapons shows the rifle alone; All the three again.
    let mut filtered = opened("store", &mut assets, &vm);
    presses(&mut filtered, &mut assets, &vm, &["BTN_WEAPONS"]);
    let weapons = listed(&filtered, &vm, "LB_SHOPITEMS");
    presses(&mut filtered, &mut assets, &vm, &["BTN_ALL"]);
    let all = listed(&filtered, &vm, "LB_SHOPITEMS").len();

    let mut bench = opened("upgrade_select", &mut assets, &vm);
    presses(
        &mut bench,
        &mut assets,
        &vm,
        &["BTN_LIGHTSABER", "LB_UPGRADELIST", "BTN_UPGRADEITEMS"],
    );
    let bench_open = bench.top().map(|screen| screen.0);
    let slots = (
        bench.hidden("BTN_UPGRADE31", &vm) && bench.hidden("BTN_UPGRADE34_LS", &vm),
        icon(&bench, &vm, "BTN_UPGRADE31_LS"),
        icon(&bench, &vm, "BTN_UPGRADE32_LS"),
        icon(&bench, &vm, "BTN_UPGRADE33_LS"),
    );
    let greyed = disabled(&bench, &vm, "BTN_ASSEMBLE");
    // The Rubat crystal (the list's second row) into the empty slot.
    let chose = pick(&mut bench, &mut assets, &vm, ("BTN_UPGRADE33_LS", 1));
    let copied = (
        icon(&bench, &vm, "BTN_UPGRADE33_LS"),
        disabled(&bench, &vm, "BTN_ASSEMBLE"),
    );
    // The lens, greyed, refused with the game's line; Back to the slots.
    presses(&mut bench, &mut assets, &vm, &["BTN_UPGRADE33_LS"]);
    let rows = listed(&bench, &vm, "LB_ITEMS");
    let dimmed: Vec<bool> = bench
        .state_of("LB_ITEMS", &vm)
        .and_then(|state| state.list)
        .map_or(Vec::new(), |list| {
            list.rows.iter().map(|row| row.dimmed).collect()
        });
    presses(&mut bench, &mut assets, &vm, &["BTN_BACK"]);
    pick(&mut bench, &mut assets, &vm, ("BTN_UPGRADE33_LS", 2));
    let refused = (
        bench.top().map(|screen| screen.0),
        bench
            .state_of("LB_MESSAGE", &vm)
            .and_then(|state| state.strref),
    );
    presses(&mut bench, &mut assets, &vm, &["BTN_OK", "BTN_BACK"]);
    // The Damind crystal taken out (the empty row), then Assemble.
    let taken = pick(&mut bench, &mut assets, &vm, ("BTN_UPGRADE32_LS", 0));
    let assembled = presses(&mut bench, &mut assets, &vm, &["BTN_ASSEMBLE"]);
    let after = bench.top().map(|screen| screen.0);
    // Back throws a visit's choices away.
    presses(&mut bench, &mut assets, &vm, &["BTN_UPGRADEITEMS"]);
    let mut thrown = pick(&mut bench, &mut assets, &vm, ("BTN_UPGRADE33_LS", 1));
    thrown.extend(presses(&mut bench, &mut assets, &vm, &["BTN_BACK"]));
    let back_to = bench.top().map(|screen| screen.0);

    let message = opened("message_box:42", &mut assets, &vm);
    let strref = message
        .state_of("LB_MESSAGE", &vm)
        .and_then(|state| state.strref);
    let death = opened("death", &mut assets, &vm);
    let fallen = told(&death, &vm, "LB_MESSAGE");
    // The party fallen (I30 P13): the host's box loads, a guest's waits.
    let mut lost_box = |host: bool| {
        let vm = ViewModel {
            lost: Some(LostView { host }),
            ..vm.clone()
        };
        let mut stack = opened("lost", &mut assets, &vm);
        let ok = (words(&stack, &vm, "BTN_OK"), stack.hidden("BTN_OK", &vm));
        let raised = presses(&mut stack, &mut assets, &vm, &["BTN_OK"]);
        (ok, raised.contains(&Intent::ReloadLatest))
    };
    let (host_box, guest_box) = (lost_box(true), lost_box(false));

    // Level 4: the ability point, then skills; nothing below what it has.
    let mut levelup = opened("levelup", &mut assets, &vm);
    let first = levelup.focused().map(str::to_string);
    presses(&mut levelup, &mut assets, &vm, &["BTN_STEPNAME1"]);
    let floor = levelup.hidden("STR_MINUS_BTN", &vm);
    // Recommended is the creation's: greyed levelling up, and inert.
    presses(&mut levelup, &mut assets, &vm, &["BTN_RECOMMENDED"]);
    let kept_scores = (
        disabled(&levelup, &vm, "BTN_RECOMMENDED"),
        words(&levelup, &vm, "STR_POINTS_BTN"),
    );
    presses(
        &mut levelup,
        &mut assets,
        &vm,
        &["STR_PLUS_BTN", "STR_PLUS_BTN"],
    );
    let raised = (
        words(&levelup, &vm, "STR_POINTS_BTN"),
        words(&levelup, &vm, "REMAINING_SELECTIONS_LBL"),
    );
    presses(
        &mut levelup,
        &mut assets,
        &vm,
        &["BTN_ACCEPT", "BTN_STEPNAME2"],
    );
    let skill_points = words(&levelup, &vm, "REMAINING_SELECTIONS_LBL");
    let held = levelup.hidden("COM_MINUS_BTN", &vm);
    presses(&mut levelup, &mut assets, &vm, &["BTN_RECOMMENDED"]);
    let kept_ranks = (
        disabled(&levelup, &vm, "BTN_RECOMMENDED"),
        words(&levelup, &vm, "COMPUTER_USE_POINTS_BTN"),
        words(&levelup, &vm, "REMAINING_SELECTIONS_LBL"),
    );
    presses(
        &mut levelup,
        &mut assets,
        &vm,
        &["COM_PLUS_BTN", "BTN_ACCEPT"],
    );
    let hidden_steps = levelup.hidden("BTN_STEPNAME3", &vm) && levelup.hidden("BTN_STEPNAME4", &vm);
    let finish = levelup.focused().map(str::to_string);
    presses(&mut levelup, &mut assets, &vm, &["BTN_STEPNAME5"]);
    let finished = !levelup.is_open();
    let level_two = ViewModel {
        levelup: Some(LevelUpView {
            level: 2,
            ..LevelUpView::default()
        }),
        ..vm.clone()
    };
    let mut skills_first = opened("levelup", &mut assets, &level_two);
    let no_abilities = skills_first.hidden("BTN_STEPNAME1", &level_two)
        && skills_first.focused() == Some("BTN_STEPNAME2");
    let back = presses(&mut skills_first, &mut assets, &level_two, &["BTN_BACK"]);

    // TSL's sort keys out of item names, as its screens show them.
    let names: Vec<Vec<u8>> = [
        &b"{cr 02}Crystal, Rubat"[..],
        b"{sc 01}Pinpoint Scope",
        b"Medpac",
    ]
    .iter()
    .map(|raw| fmt_odyssey::tlk::unbraced(raw))
    .collect();
    let unbraced = check(
        "item_names_unbraced",
        expect(
            &format!("{names:?}"),
            names == [&b"Crystal, Rubat"[..], b"Pinpoint Scope", b"Medpac"],
        )
        .map(|()| "sort keys in braces left out".into()),
    );
    vec![
        benches(&mut assets),
        unbraced,
        check("script_screens", {
            expect(
            &format!("the quest's entry: {quest:?}"),
            quest == "Find Bastila.",
        )
        .and_then(|()| {
            expect(
                &format!("the stim's price, count, credits, text, buyable: {chosen:?}"),
                chosen
                    == (
                        "120".into(),
                        "2".into(),
                        "300".into(),
                        "Boosts.".into(),
                        false,
                    ),
            )
        })
        .and_then(|()| {
            expect(
                &format!("Accept buys one stim: {bought:?}"),
                game(&bought) == [GameAction::Buy { store: 7, item: 12 }],
            )
        })
        .and_then(|()| {
            expect(
                &format!("the rifle never runs out and is too dear: {dear:?}"),
                dear == (String::new(), Some("message"), false),
            )
        })
        .and_then(|()| {
            expect(
                &format!("Show Sell List shows the party's, to sell: {selling:?}"),
                selling == (true, "Sell".into(), "Sell".into(), true),
            )
        })
        .and_then(|()| {
            expect(
                &format!("the blaster sold: {sold:?}"),
                game(&sold) == [GameAction::Sell { store: 7, item: 21 }],
            )
        })
        .and_then(|()| {
            expect(
                &format!("Close leaves the store and closes it: {left:?}"),
                game(&left) == [GameAction::Leave(7)] && store_closed,
            )
        })
        .and_then(|()| {
            expect(
                &format!("the filters: weapons {weapons:?}, all {all}"),
                weapons == ["Rifle"] && all == 3,
            )
        })
        .and_then(|()| {
            expect(
                &format!("a lightsaber's three slots: {bench_open:?} {slots:?}"),
                bench_open == Some("upgrade")
                    && slots == (true, "i_blue".into(), "i_damind".into(), String::new()),
            )
        })
        .and_then(|()| {
            let made = GameAction::Assemble {
                item: 30,
                changes: vec![
                    Fitting::Out(1),
                    Fitting::In {
                        upgrade: 41,
                        replace: None,
                    },
                ],
            };
            expect(
                &format!(
                    "a visit on a copy: Assemble greyed ({greyed}) until a change; the Rubat \
                     chosen shows {copied:?} (nothing sent: {chose:?}); the lens greyed \
                     ({rows:?} {dimmed:?}) and refused ({refused:?}); the Damind taken out \
                     ({taken:?}); Assemble sends {:?} and closes to {after:?}; Back sends \
                     {:?} and closes to {back_to:?}",
                    game(&assembled),
                    game(&thrown)
                ),
                greyed
                    && game(&chose).is_empty()
                    && copied == ("i_rubat".into(), false)
                    && rows == ["", "Rubat", "Lens"]
                    && dimmed == [false, false, true]
                    && refused == (Some("message"), Some(116_713))
                    && game(&taken).is_empty()
                    && game(&assembled) == [made]
                    && after == Some("upgrade_select")
                    && game(&thrown).is_empty()
                    && back_to == Some("upgrade_select"),
            )
        })
        .and_then(|()| {
            expect(
                &format!("the message box's text by strref: {strref:?}"),
                strref == Some(42),
            )
        })
        .and_then(|()| expect(&format!("the fall: {fallen:?}"), !fallen.is_empty()))
        .and_then(|()| {
            expect(
                &format!(
                    "the party fallen: the host's Load, a guest's none: {host_box:?} {guest_box:?}"
                ),
                host_box == (("Load".into(), false), true) && guest_box.0.1 && !guest_box.1,
            )
        })
        .and_then(|()| {
            expect(
                &format!("level 4 starts at abilities: {first:?}"),
                first.as_deref() == Some("BTN_STEPNAME1"),
            )
        })
        .and_then(|()| expect("a score cannot go below what it was", floor))
        .and_then(|()| {
            expect(
                &format!("one point, one raise: {raised:?}"),
                raised == ("15".into(), "0".into()),
            )
        })
        .and_then(|()| {
            expect(
                &format!("(2 + 0) / 2 skill points: {skill_points:?}"),
                skill_points == "1",
            )
        })
        .and_then(|()| expect("ranks held cannot be taken back", held))
        .and_then(|()| {
            expect(
                &format!("no Recommended levelling up: {kept_scores:?} {kept_ranks:?}"),
                kept_scores == (true, "14".into()) && kept_ranks == (true, "2".into(), "1".into()),
            )
        })
        .and_then(|()| {
            expect(
                &format!("feats and powers hidden, on to Finish: {finish:?}"),
                hidden_steps && finish.as_deref() == Some("BTN_STEPNAME5"),
            )
        })
        .and_then(|()| expect("Finish closes", finished))
        .and_then(|()| expect("level 2 starts at skills", no_abilities))
        .and_then(|()| {
            expect(
                &format!("Back from the first step closes: {back:?}"),
                !skills_first.is_open(),
            )
        })
        .map(|()| "journal, store and its filters, upgrades, message box, level-up".into())
        }),
    ]
}
