//! Keys and clicks over screens: a tab swap drawn the frame it happens, the
//! loot's Take-all key, a terminal's line skipped by a click, an equip slot's
//! double click, a store row's double click buying and selling (K1's
//! question before a dear trade), and the controls' sounds.

use super::menus::{keys, root};
use super::paint::{Fake, button};
use super::vm_screens::list;
use super::{Check, check, expect};
use crate::input::UiInput;
use crate::intent::{GameAction, GuiSound, Intent};
use crate::screen::{Frame, Stack};
use crate::vm::{
    CharacterView, ComputerView, Entry, LootView, MemberView, Owned, StoreView, ViewModel, Ware,
};

/// A click at a layout point (the 640 × 480 layouts here are centred in the
/// 800 × 600 window).
fn click(x: f32, y: f32) -> UiInput {
    UiInput {
        pointer: Some([x + 80.0, y + 60.0]),
        moved: true,
        click: true,
        ..keys(&[])
    }
}

fn opened(name: &str, assets: &mut Fake, vm: &ViewModel) -> Stack {
    let mut stack = Stack::new(crate::screens::open);
    stack.open(name, vm);
    stack.step(assets, vm, &keys(&[]));
    stack
}

fn tab_swap() -> Check {
    let mut assets = Fake::new();
    let vm = ViewModel::default();
    let bar = vec![
        button("BTN_EQU", [400, 0, 40, 20], b""),
        button("BTN_INV", [450, 0, 40, 20], b""),
    ];
    assets.layouts.insert("top", root(640, 480, bar));
    assets.layouts.insert("equip", root(640, 480, vec![]));
    let credits = super::vm_screens::label("LBL_CREDITS_VALUE", 0);
    assets
        .layouts
        .insert("inventory", root(640, 480, vec![credits]));
    let mut stack = opened("equip", &mut assets, &vm);
    stack.press("BTN_INV");
    stack.step(&mut assets, &vm, &keys(&[]));
    let (top, drawn) = (
        stack.top().map(|screen| screen.0),
        stack.state_of("LBL_CREDITS_VALUE", &vm).is_some(),
    );
    check("tab_swap", {
        expect(
            &format!("the tab is swapped and laid out in the same frame: {top:?}, {drawn}"),
            top == Some("inventory") && drawn,
        )
        .map(|()| "a swapped tab is drawn the frame it opens".into())
    })
}

fn take_all() -> Check {
    let mut assets = Fake::new();
    assets.layouts.insert(
        "container",
        root(
            640,
            480,
            vec![
                list("LB_ITEMS", 0),
                button("BTN_OK", [0, 300, 80, 20], b""),
                button("BTN_CANCEL", [100, 300, 80, 20], b""),
            ],
        ),
    );
    let vm = ViewModel {
        loot: Some(LootView {
            container: 7,
            ..LootView::default()
        }),
        ..ViewModel::default()
    };
    let mut stack = opened("container", &mut assets, &vm);
    let key = UiInput {
        raw: vec!["KeyA".into()],
        ..keys(&[])
    };
    let intents = stack.step(&mut assets, &vm, &key).intents;
    check("loot_take_all_key", {
        expect(
            &format!("A takes everything and closes: {intents:?}"),
            intents
                == vec![
                    Intent::Game(GameAction::TakeAll(7)),
                    Intent::Game(GameAction::Close(7)),
                ]
                && !stack.is_open(),
        )
        .map(|()| "A takes all".into())
    })
}

/// The container's give mode (KOTOR's): its button lists the party's items,
/// a row puts its whole stack in, A and OK take nothing there, and the
/// button again lists the container's.
fn give_mode() -> Check {
    let mut assets = Fake::new();
    assets.layouts.insert(
        "container",
        root(
            640,
            480,
            vec![
                list("LB_ITEMS", 0),
                button("BTN_OK", [0, 300, 80, 20], b""),
                button("BTN_CANCEL", [100, 300, 80, 20], b""),
                button("BTN_GIVEITEMS", [200, 300, 80, 20], b""),
            ],
        ),
    );
    let row = |items: Vec<u32>, pending: bool| crate::vm::LootRow {
        count: items.len() as u32,
        items,
        pending,
        ..crate::vm::LootRow::default()
    };
    let vm = ViewModel {
        loot: Some(LootView {
            container: 7,
            rows: vec![row(vec![1], false)],
            give: vec![row(vec![5, 6], false), row(vec![9], true)],
            ..LootView::default()
        }),
        ..ViewModel::default()
    };
    let shown = |stack: &Stack| {
        stack
            .state_of("LB_ITEMS", &vm)
            .and_then(|state| state.list)
            .map_or(0, |list| list.rows.len())
    };
    let mut stack = opened("container", &mut assets, &vm);
    stack.press("BTN_GIVEITEMS");
    stack.step(&mut assets, &vm, &keys(&[]));
    let party = shown(&stack);
    stack.double("LB_ITEMS", 0);
    let put = stack.step(&mut assets, &vm, &keys(&[])).intents;
    stack.double("LB_ITEMS", 1);
    let faded = stack.step(&mut assets, &vm, &keys(&[])).intents;
    let key = UiInput {
        raw: vec!["KeyA".into()],
        ..keys(&[])
    };
    let unkeyed = stack.step(&mut assets, &vm, &key).intents;
    stack.press("BTN_OK");
    let unpressed = stack.step(&mut assets, &vm, &keys(&[])).intents;
    stack.press("BTN_GIVEITEMS");
    stack.step(&mut assets, &vm, &keys(&[]));
    let back = shown(&stack);
    let puts = [5, 6].map(|item| Intent::Game(GameAction::Put { container: 7, item }));
    check("loot_give_mode", {
        expect(
            &format!(
                "party rows {party}, put {put:?}, a pending one {faded:?}, A {unkeyed:?}, \
                 OK {unpressed:?}, back to {back} rows"
            ),
            party == 2
                && puts.iter().all(|intent| put.contains(intent))
                && !put
                    .iter()
                    .any(|intent| matches!(intent, Intent::Game(GameAction::Take { .. })))
                && faded.is_empty()
                && unkeyed.is_empty()
                && unpressed.is_empty()
                && back == 1
                && stack.is_open(),
        )
        .map(|()| "the party's items listed, a stack put in, back to the contents".into())
    })
}

fn terminal_click() -> Check {
    let on = |replies: Vec<Vec<u8>>| {
        let mut assets = Fake::new();
        assets.layouts.insert(
            "computer",
            root(
                640,
                480,
                vec![list("LB_REPLIES", 0), list("LB_MESSAGE", 200)],
            ),
        );
        let vm = ViewModel {
            computer: Some(ComputerView {
                replies,
                ..ComputerView::default()
            }),
            ..ViewModel::default()
        };
        let mut stack = opened("computer", &mut assets, &vm);
        stack.step(&mut assets, &vm, &click(50.0, 250.0)).intents
    };
    let (alone, offered) = (on(Vec::new()), on(vec![b"Yes".to_vec()]));
    check("terminal_click_skips", {
        expect(
            &format!("a click ends a line with no reply: {alone:?}"),
            alone == vec![Intent::Answer(None)],
        )
        .and_then(|()| {
            expect(
                &format!("and leaves one with replies be: {offered:?}"),
                offered.is_empty(),
            )
        })
        .map(|()| "a click skips a terminal line".into())
    })
}

fn equip_double() -> Check {
    let mut assets = Fake::new();
    assets.layouts.insert("top", root(640, 480, vec![]));
    assets.layouts.insert(
        "equip",
        root(
            640,
            480,
            vec![
                button("BTN_INV_BODY", [100, 100, 40, 40], b""),
                list("LB_ITEMS", 200),
                button("BTN_EQUIP", [0, 400, 80, 20], b""),
            ],
        ),
    );
    let vm = ViewModel {
        character: Some(CharacterView {
            id: 5,
            worn: vec![(
                1,
                Owned {
                    item: 9,
                    slots: 1 << 1,
                    ..Owned::default()
                },
            )],
            ..CharacterView::default()
        }),
        ..ViewModel::default()
    };
    let mut stack = opened("equip", &mut assets, &vm);
    let first = stack.step(&mut assets, &vm, &click(120.0, 120.0)).intents;
    let second = stack.step(&mut assets, &vm, &click(120.0, 120.0)).intents;
    check("equip_double_click", {
        expect(
            &format!("one click only looks at the slot: {first:?}"),
            first.is_empty(),
        )
        .and_then(|()| {
            expect(
                &format!("a double click takes off what it wears: {second:?}"),
                second == vec![Intent::Game(GameAction::Unequip { who: 5, slot: 1 })],
            )
        })
        .map(|()| "a double click on a worn slot unequips".into())
    })
}

fn journal_opens_on_a_quest() -> Check {
    let mut assets = Fake::new();
    assets.layouts.insert("top", root(640, 480, vec![]));
    assets.layouts.insert(
        "journal",
        root(
            640,
            480,
            vec![list("LB_ITEMS", 0), list("LBL_ITEM_DESCRIPTION", 200)],
        ),
    );
    let quest = |name: &str| Entry {
        name: name.as_bytes().to_vec(),
        description: format!("{name} log").into_bytes(),
        ..Entry::default()
    };
    let vm = ViewModel {
        journal: vec![quest("Latest"), quest("Older")],
        ..ViewModel::default()
    };
    let stack = opened("journal", &mut assets, &vm);
    let selected = stack
        .state_of("LB_ITEMS", &vm)
        .and_then(|state| state.list)
        .and_then(|list| list.selected);
    let log = stack
        .state_of("LBL_ITEM_DESCRIPTION", &vm)
        .and_then(|state| state.list)
        .and_then(|list| list.rows.first().map(|row| row.text.clone()));
    check("journal_first_quest", {
        expect(
            &format!("the first quest chosen and its log shown at once: {selected:?}, {log:?}"),
            selected == Some(0) && log == Some(b"Latest log".to_vec()),
        )
        .map(|()| "the journal opens on its first quest".into())
    })
}

/// A store of a medpac (40), a stim (300) and a rifle (900), 500 credits,
/// a blaster to sell (260), its leader of level 4 (a trade of 200 asks).
fn store_vm() -> ViewModel {
    let ware = |item: u32, name: &str, price: i32| Ware {
        item,
        entry: Entry {
            name: name.as_bytes().to_vec(),
            ..Entry::default()
        },
        price,
    };
    ViewModel {
        store: Some(StoreView {
            store: 7,
            stock: vec![
                ware(11, "Medpac", 40),
                ware(12, "Stim", 300),
                ware(13, "Rifle", 900),
            ],
            offers: vec![ware(21, "Blaster", 260)],
            credits: 500,
            sells: true,
            buys: true,
        }),
        party: vec![MemberView {
            level: 4,
            ..MemberView::default()
        }],
        ..ViewModel::default()
    }
}

fn store_assets() -> Fake {
    let mut assets = Fake::new();
    let confirm = vec![
        list("LB_MESSAGE", 0),
        button("BTN_OK", [0, 300, 80, 20], b""),
        button("BTN_CANCEL", [100, 300, 80, 20], b""),
    ];
    assets.layouts.insert("confirm", root(640, 480, confirm));
    assets.layouts.insert(
        "store",
        root(
            640,
            480,
            vec![
                list("LB_SHOPITEMS", 0),
                list("LB_INVITEMS", 0),
                button("BTN_Accept", [0, 400, 80, 20], b""),
                button("BTN_Examine", [100, 400, 80, 20], b""),
                button("BTN_Cancel", [200, 400, 80, 20], b""),
            ],
        ),
    );
    assets
}

fn games(frames: &[Frame]) -> Vec<GameAction> {
    frames
        .iter()
        .flat_map(|frame| &frame.intents)
        .filter_map(|intent| match intent {
            Intent::Game(action) => Some(action.clone()),
            _ => None,
        })
        .collect()
}

fn store_double() -> Check {
    let mut assets = store_assets();
    let vm = store_vm();
    let mut stack = opened("store", &mut assets, &vm);
    // Row n of a list at the top: 20 high from 0.
    let twice = |stack: &mut Stack, assets: &mut Fake, row: f32| -> Vec<Frame> {
        let at = click(50.0, 10.0 + 20.0 * row);
        vec![stack.step(assets, &vm, &at), stack.step(assets, &vm, &at)]
    };
    // The medpac, under the asking price: its first click only chooses
    // it, the second buys it.
    let bought = twice(&mut stack, &mut assets, 0.0);
    let first = games(&bought[..1]);
    let medpac = games(&bought);
    let heard: Vec<GuiSound> = bought
        .iter()
        .flat_map(|frame| frame.sounds.clone())
        .collect();
    // The stim, dearer than 200: K1 asks; OK buys it.
    let asked = games(&twice(&mut stack, &mut assets, 1.0));
    let question = (
        stack.top().map(|screen| screen.0),
        stack
            .state_of("LB_MESSAGE", &vm)
            .map(|state| (state.strref, state.tokens)),
    );
    stack.press("BTN_OK");
    let stim = games(&[stack.step(&mut assets, &vm, &keys(&[]))]);
    // The rifle, past the credits: told so, nothing bought.
    let rifle = games(&twice(&mut stack, &mut assets, 2.0));
    let short = stack.top().map(|screen| screen.0);
    stack.press("BTN_OK");
    stack.step(&mut assets, &vm, &keys(&[]));
    // The sell list's blaster, worth more than 200: asked, then sold.
    stack.press("BTN_Examine");
    stack.step(&mut assets, &vm, &keys(&[]));
    twice(&mut stack, &mut assets, 0.0);
    stack.press("BTN_OK");
    let sale = stack.step(&mut assets, &vm, &keys(&[]));
    let blaster = (games(std::slice::from_ref(&sale)), sale.sounds.clone());
    check("store_double_click", {
        expect(
            &format!("one click only chooses a row: {first:?}"),
            first.is_empty(),
        )
        .and_then(|()| {
            expect(
                &format!(
                    "a double click buys the medpac, clicked and heard bought: {medpac:?} {heard:?}"
                ),
                medpac == [GameAction::Buy { store: 7, item: 11 }]
                    && heard
                        .iter()
                        .filter(|sound| **sound == GuiSound::Clicked)
                        .count()
                        == 2
                    && heard.last() == Some(&GuiSound::InventoryAdd),
            )
        })
        .and_then(|()| {
            expect(
                &format!("the stim asks first ({question:?}) and OK buys it: {asked:?} {stim:?}"),
                asked.is_empty()
                    && question
                        == (
                            Some("store_confirm"),
                            Some((Some(42020), vec![("CUSTOM0", b"200".to_vec())])),
                        )
                    && stim == [GameAction::Buy { store: 7, item: 12 }],
            )
        })
        .and_then(|()| {
            expect(
                &format!("the rifle is past the credits: {rifle:?} {short:?}"),
                rifle.is_empty() && short == Some("message"),
            )
        })
        .and_then(|()| {
            expect(
                &format!("the blaster asked and sold, heard dropped: {blaster:?}"),
                blaster.0 == [GameAction::Sell { store: 7, item: 21 }]
                    && blaster.1.contains(&GuiSound::InventoryDrop),
            )
        })
        .map(|()| "a double click on a store's row buys or sells it, as Accept".into())
    })
}

fn control_sounds() -> Check {
    let mut assets = Fake::new();
    let mut check_box = button("CB_ONE", [0, 100, 40, 20], b"");
    check_box.kind = fmt_odyssey::gui::Kind::CheckBox;
    assets.layouts.insert(
        "layout_a",
        root(
            640,
            480,
            vec![
                button("BTN_A", [0, 0, 80, 20], b""),
                button("BTN_B", [0, 40, 80, 20], b""),
                check_box,
            ],
        ),
    );
    let vm = ViewModel::default();
    let mut stack = Stack::new(crate::screens::open);
    stack.push(Box::new(crate::screens::Layout("layout_a".into())));
    let opened = stack.step(&mut assets, &vm, &keys(&[])).sounds;
    let hover = UiInput {
        pointer: Some([80.0 + 20.0, 60.0 + 50.0]),
        moved: true,
        ..keys(&[])
    };
    let entered = stack.step(&mut assets, &vm, &hover).sounds;
    let clicked = stack.step(&mut assets, &vm, &click(20.0, 50.0)).sounds;
    let checked = stack.step(&mut assets, &vm, &click(20.0, 110.0)).sounds;
    check("control_sounds", {
        expect(
            &format!("a screen's first focus is silent: {opened:?}"),
            opened.is_empty(),
        )
        .and_then(|()| {
            expect(
                &format!("the pointer coming to a button enters it: {entered:?}"),
                entered == [GuiSound::Entered],
            )
        })
        .and_then(|()| {
            expect(
                &format!("a click on it clicks: {clicked:?}"),
                clicked == [GuiSound::Clicked],
            )
        })
        .and_then(|()| {
            expect(
                &format!("a check box enters and checks: {checked:?}"),
                checked == [GuiSound::Entered, GuiSound::Check],
            )
        })
        .map(|()| "controls sound as K1's: Entered_Default, Clicked_Default, Checkbox_Check".into())
    })
}

pub(super) fn all() -> Vec<Check> {
    vec![
        store_double(),
        control_sounds(),
        tab_swap(),
        take_all(),
        give_mode(),
        terminal_click(),
        equip_double(),
        journal_opens_on_a_quest(),
    ]
}
