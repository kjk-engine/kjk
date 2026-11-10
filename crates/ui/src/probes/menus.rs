//! The game's own screens over layouts shaped like KOTOR's.

use fmt_odyssey::gui::{Control, Gui, Kind, MoveTo, Rect};

use super::paint::{Fake, button};
use super::{Check, check, expect};
use crate::input::{Key, UiInput};
use crate::intent::{GameAction, Intent, SessionIntent};
use crate::screen::Stack;
use crate::vm::ViewModel;

/// The rect reaches past the 640 × 480 frame on every side.
fn covers(rect: [f32; 4]) -> bool {
    rect[0] < 0.0 && rect[1] < 0.0 && rect[0] + rect[2] > 640.0 && rect[1] + rect[3] > 480.0
}

pub(super) fn root(width: i32, height: i32, controls: Vec<Control>) -> Gui {
    Gui {
        tag: "TGuiPanel".into(),
        extent: Rect {
            left: 0,
            top: 0,
            width,
            height,
        },
        border: None,
        color: None,
        alpha: None,
        controls,
        unread: Vec::new(),
    }
}

pub(super) fn linked(tag: &str, id: i32, top: i32, up: i32, down: i32) -> Control {
    Control {
        id,
        moveto: Some(MoveTo {
            up,
            down,
            left: -1,
            right: -1,
        }),
        ..button(tag, [300, top, 200, 40], b"?")
    }
}

pub(super) fn keys(keys: &[Key]) -> UiInput {
    UiInput {
        keys: keys.to_vec(),
        window: (800.0, 600.0),
        dt: 0.016,
        ..UiInput::default()
    }
}

fn main_menu() -> Gui {
    root(
        800,
        600,
        vec![
            Control {
                kind: Kind::ListBox,
                ..button("LB_MODULES", [20, 20, 200, 300], b"")
            },
            linked("BTN_LOADGAME", 6, 300, 7, 8),
            linked("BTN_NEWGAME", 7, 250, 13, 6),
            linked("BTN_MOVIES", 8, 350, 6, 10),
            linked("BTN_MUSIC", 10, 400, 8, 9),
            linked("BTN_OPTIONS", 9, 450, 10, 13),
            linked("BTN_WARP", 12, 550, 13, 7),
            linked("BTN_EXIT", 13, 500, 9, 7),
            Control {
                kind: Kind::Label,
                ..button("LBL_3DVIEW", [0, 0, 640, 480], b"")
            },
        ],
    )
}

pub(super) fn all() -> Vec<Check> {
    let mut assets = Fake::new();
    assets.layouts.insert("mainmenu16x12", main_menu());
    let vm = ViewModel::default();
    let mut stack = Stack::new(crate::screens::open);
    let opened = stack.open("main_menu", &vm);
    let mut path = Vec::new();
    let mut labels = Vec::new();
    stack.step(&mut assets, &vm, &keys(&[]));
    for _ in 0..5 {
        let tag = stack.focused().unwrap_or("none").to_string();
        labels.push(
            String::from_utf8_lossy(&stack.text_of(&tag, &vm).unwrap_or(Vec::new())).into_owned(),
        );
        path.push(tag);
        stack.step(&mut assets, &vm, &keys(&[Key::Down]));
    }
    let hidden = ["BTN_WARP", "LB_MODULES", "BTN_EXIT"]
        .iter()
        .all(|tag| stack.hidden(tag, &vm));
    // Down from the last shown button (Exit) goes nowhere shown.
    let quit = stack
        .step(&mut assets, &vm, &keys(&[Key::Activate]))
        .intents;
    let backdrop = stack
        .state_of("LBL_3DVIEW", &vm)
        .and_then(|state| state.scene);
    press(&mut stack, &mut assets, &vm, "BTN_NEWGAME");
    let solo = stack.top().map(|screen| screen.0);
    stack.pop();
    let stays = stack
        .step(&mut assets, &vm, &keys(&[Key::Back]))
        .intents
        .is_empty()
        && stack.is_open();
    vec![check("main_menu_buttons", {
        expect("opens", opened)
            .and_then(|()| {
                expect(
                    &format!("Solo, Host, Join, Options, Exit in order: {path:?}"),
                    path == [
                        "BTN_NEWGAME",
                        "BTN_LOADGAME",
                        "BTN_MOVIES",
                        "BTN_MUSIC",
                        "BTN_OPTIONS",
                    ],
                )
            })
            .and_then(|()| {
                expect(
                    &format!("our labels: {labels:?}"),
                    labels == ["Solo", "Host", "Join", "Options", "Exit"],
                )
            })
            .and_then(|()| expect("warp, the module list and KOTOR 2's exit hidden", hidden))
            .and_then(|()| expect(&format!("Exit quits: {quit:?}"), quit == vec![Intent::Quit]))
            .and_then(|()| {
                expect(
                    &format!("Solo opens its new-or-load step: {solo:?}"),
                    solo == Some("solo"),
                )
            })
            .and_then(|()| expect("Back keeps the main menu", stays))
            .and_then(|()| {
                expect(
                    &format!("the menu's model behind it: {backdrop:?}"),
                    backdrop == Some(crate::vm::SceneView::MainMenu),
                )
            })
            .map(|()| "five buttons, ours, the model behind".into())
    })]
}

fn savename() -> Gui {
    Gui {
        extent: Rect {
            left: 109,
            top: 179,
            width: 412,
            height: 130,
        },
        ..root(
            640,
            480,
            vec![
                linked("BTN_OK", 0, 75, -1, 1),
                linked("BTN_CANCEL", 1, 100, 0, -1),
                Control {
                    kind: Kind::Label,
                    ..button("EDITBOX", [20, 41, 372, 29], b"")
                },
                Control {
                    kind: Kind::Label,
                    ..button("LBL_TITLE", [16, 7, 380, 32], b"")
                },
            ],
        )
    }
}

pub(super) fn typed(text: &str) -> UiInput {
    UiInput {
        text: text.into(),
        ..keys(&[])
    }
}

/// Clears the edit box, types `text`, presses OK.
fn enter(stack: &mut Stack, assets: &mut Fake, vm: &ViewModel, text: &str) -> Vec<Intent> {
    stack.step(assets, vm, &keys(&[Key::Backspace; 40]));
    stack.step(assets, vm, &typed(text));
    stack.step(assets, vm, &keys(&[Key::Activate])).intents
}

pub(super) fn join() -> Vec<Check> {
    let mut assets = Fake::new();
    assets.layouts.insert("savename", savename());
    let vm = ViewModel::default();
    let mut stack = Stack::new(crate::screens::open);
    stack.open("join", &vm);
    stack.step(&mut assets, &vm, &keys(&[]));
    let asks_address = stack.text_of("LBL_TITLE", &vm);
    let bad = enter(&mut stack, &mut assets, &vm, "no such:host:99999");
    let still_address = stack.text_of("LBL_TITLE", &vm) == asks_address;
    let advanced = enter(&mut stack, &mut assets, &vm, "[::1]:29170");
    let asks_name = stack.text_of("LBL_TITLE", &vm);
    let joined = enter(&mut stack, &mut assets, &vm, "Bastila");
    let mut fresh = crate::edit::EditBox::with("Player", 16);
    fresh.type_text("Carth");
    let mut erased = crate::edit::EditBox::with("Player", 16);
    erased.backspace();
    let mut popup = Stack::new(crate::screens::open);
    popup.open("main_menu", &vm);
    popup.open("join", &vm);
    assets.layouts.insert("mainmenu16x12", main_menu());
    let layers = popup.step(&mut assets, &vm, &keys(&[])).layers;
    let dimmed = layers
        .get(1)
        .and_then(|layer| layer.quads.first())
        .map(|quad| (quad.rect, quad.color));
    let valid: Vec<bool> = [
        "host",
        "host:29170",
        "[::1]:29170",
        "192.0.2.2",
        "steam:76561197960287930",
        "Bastila Shan",
        "",
        "  ",
        "a b:29170",
        "h:0",
        "h:70000",
        "steam:spacewar",
    ]
    .iter()
    .map(|address| crate::screens::join::valid_address(address))
    .collect();
    vec![
        check("edit_default_replaced", {
            expect(
                &format!(
                    "typing replaces the default, Backspace clears it: {:?} {:?}",
                    fresh.as_str(),
                    erased.as_str()
                ),
                fresh.as_str() == "Carth" && erased.as_str().is_empty(),
            )
            .map(|()| "the default goes at the first key".into())
        }),
        check("popup_dims_below", {
            expect(
                &format!("a dark veil over the frame first: {dimmed:?}"),
                dimmed
                    .is_some_and(|(rect, color)| covers(rect) && color[3] > 0.3 && color[0] == 0.0),
            )
            .map(|()| "what is under a popup is dimmed".into())
        }),
        check("join_address_validates", {
            expect(
                &format!(
                    "host, host:port, [v6]:port, steam:<id> and a friend's name only: {valid:?}"
                ),
                valid
                    == [
                        true, true, true, true, true, true, false, false, false, false, false,
                        false,
                    ],
            )
            .and_then(|()| {
                expect(
                    &format!("a bad address stays on the address: {bad:?}"),
                    bad.is_empty() && still_address,
                )
            })
            .map(|()| "checked before joining".into())
        }),
        check("join_address_then_name", {
            expect(
                &format!("the address, then the name: {advanced:?} {asks_name:?}"),
                advanced.is_empty() && asks_name.is_some() && asks_name != asks_address,
            )
            .and_then(|()| {
                expect(
                    &format!("joins with both: {joined:?}"),
                    joined
                        == vec![Intent::Session(SessionIntent::Join {
                            address: "[::1]:29170".into(),
                            name: "Bastila".into(),
                        })],
                )
            })
            .map(|()| "address, name, join".into())
        }),
    ]
}

fn saveload() -> Gui {
    root(
        640,
        480,
        vec![
            Control {
                kind: Kind::ListBox,
                proto: Some(Box::new(button("PROTOITEM", [48, 83, 272, 30], b""))),
                ..button("LB_GAMES", [48, 83, 272, 323], b"")
            },
            Control {
                kind: Kind::Label,
                ..button("LBL_PANELNAME", [48, 40, 544, 21], b"")
            },
            Control {
                kind: Kind::Label,
                ..button("LBL_PLANETNAME", [317, 64, 272, 20], b"")
            },
            Control {
                kind: Kind::Label,
                ..button("LBL_PM1", [410, 320, 82, 82], b"")
            },
            Control {
                kind: Kind::Label,
                ..button("LBL_SCREENSHOT", [317, 90, 272, 200], b"")
            },
            linked("BTN_DELETE", 10, 410, -1, -1),
            linked("BTN_SAVELOAD", 11, 410, -1, -1),
            linked("BTN_BACK", 12, 410, -1, -1),
        ],
    )
}

/// A stack on screen `name`, one quiet frame stepped.
fn opened(name: &str, assets: &mut Fake, vm: &ViewModel) -> Stack {
    let mut stack = Stack::new(crate::screens::open);
    stack.open(name, vm);
    stack.step(assets, vm, &keys(&[]));
    stack
}

/// Presses `tag`, returning what the frame raised.
fn press(stack: &mut Stack, assets: &mut Fake, vm: &ViewModel, tag: &str) -> Vec<Intent> {
    stack.press(tag);
    stack.step(assets, vm, &keys(&[])).intents
}

pub(super) fn new_or_load() -> Vec<Check> {
    let mut assets = Fake::new();
    assets.layouts.insert("saveload", saveload());
    super::chargen::layouts(&mut assets);
    let vm = super::chargen::chargen_vm();
    let mut solo = opened("solo", &mut assets, &vm);
    let no_delete = solo
        .state_of("BTN_DELETE", &vm)
        .is_some_and(|state| state.disabled && state.text.as_deref() == Some(&b"Delete"[..]));
    let first_row = solo
        .state_of("LB_GAMES", &vm)
        .and_then(|state| state.list)
        .and_then(|list| list.rows.first().cloned())
        .map(|row| String::from_utf8_lossy(&row.text).into_owned());
    press(&mut solo, &mut assets, &vm, "BTN_SAVELOAD");
    let solo_started = solo.top().map(|screen| screen.0);
    let mut host = opened("host", &mut assets, &vm);
    for _ in 0..2 {
        press(&mut host, &mut assets, &vm, "BTN_DELETE");
    }
    host.step(&mut assets, &vm, &keys(&[Key::Backspace; 8]));
    host.step(&mut assets, &vm, &typed("0"));
    let bad_port = press(&mut host, &mut assets, &vm, "BTN_SAVELOAD");
    host.step(&mut assets, &vm, &keys(&[Key::Backspace; 8]));
    host.step(&mut assets, &vm, &typed("29171"));
    press(&mut host, &mut assets, &vm, "BTN_SAVELOAD");
    let hosted = super::chargen::quick(&mut host, &mut assets, &vm);
    vec![check("new_or_load", {
        expect(
            &format!("New game first: {first_row:?}"),
            first_row.as_deref() == Some("New game"),
        )
        .and_then(|()| expect("alone, Delete in place of the seats, for a save", no_delete))
        .and_then(|()| {
            expect(
                &format!("Start makes a character to play alone: {solo_started:?}"),
                solo_started == Some("chargen"),
            )
        })
        .and_then(|()| {
            expect(
                &format!("port 0 refused: {bad_port:?}"),
                bad_port.is_empty(),
            )
        })
        .and_then(|()| {
            expect(
                &format!("two seats on 29171, as the character made: {hosted:?}"),
                hosted
                    == vec![Intent::Session(SessionIntent::Host {
                        save: None,
                        seats: 2,
                        port: 29171,
                        character: super::chargen::made(),
                        campaign: None,
                    })],
            )
        })
        .map(|()| "new game, seats, port".into())
    })]
}

/// `confirm`'s controls: the message's list, OK and Cancel.
fn confirm() -> Gui {
    let mut message = button("LB_MESSAGE", [20, 20, 300, 60], b"");
    message.kind = Kind::ListBox;
    message.proto = Some(Box::new(button("PROTOITEM", [20, 20, 300, 18], b"")));
    root(
        340,
        160,
        vec![
            message,
            button("BTN_OK", [120, 90, 100, 20], b""),
            button("BTN_CANCEL", [120, 120, 100, 20], b""),
        ],
    )
}

/// The menu's New Game with both games installed: the campaign asked
/// (The Sith Lords picked: the screens on its data, its character creation,
/// the session of it); backing out of its creation puts the screens on
/// K1's data again; with one game installed, no question.
pub(super) fn campaign() -> Vec<Check> {
    let mut assets = Fake::new();
    assets.layouts.insert("saveload", saveload());
    assets.layouts.insert("confirm", confirm());
    super::chargen::layouts(&mut assets);
    let both = ViewModel {
        campaigns: vec![vfs::Game::Kotor1, vfs::Game::Kotor2],
        ..super::chargen::chargen_vm()
    };
    let mut solo = opened("solo", &mut assets, &both);
    press(&mut solo, &mut assets, &both, "BTN_SAVELOAD");
    let asked = solo.top().map(|screen| screen.0);
    let picked = press(&mut solo, &mut assets, &both, "BTN_CANCEL");
    let creating = solo.top().map(|screen| screen.0);
    let backed = press(&mut solo, &mut assets, &both, "BTN_BACK");
    press(&mut solo, &mut assets, &both, "BTN_SAVELOAD");
    press(&mut solo, &mut assets, &both, "BTN_CANCEL");
    let played = super::chargen::quick(&mut solo, &mut assets, &both);
    let one = ViewModel {
        campaigns: vec![vfs::Game::Kotor1],
        ..super::chargen::chargen_vm()
    };
    let mut alone = opened("solo", &mut assets, &one);
    let unasked = press(&mut alone, &mut assets, &one, "BTN_SAVELOAD");
    let opened_alone = alone.top().map(|screen| screen.0);
    let made_k1 = super::chargen::quick(&mut alone, &mut assets, &one);
    let session = |campaign| {
        vec![Intent::Session(SessionIntent::Solo {
            save: None,
            character: super::chargen::made(),
            campaign: Some(campaign),
        })]
    };
    vec![check("campaign", {
        expect(
            &format!("both installed, New Game asks: {asked:?}"),
            asked == Some("campaign"),
        )
        .and_then(|()| {
            expect(
                &format!("The Sith Lords: its data, its creation: {picked:?}, {creating:?}"),
                picked == vec![Intent::Campaign(vfs::Game::Kotor2)] && creating == Some("chargen"),
            )
        })
        .and_then(|()| {
            expect(
                &format!("its creation backed out of: K1's data again: {backed:?}"),
                backed == vec![Intent::Campaign(vfs::Game::Kotor1)],
            )
        })
        .and_then(|()| {
            expect(
                &format!("the character made plays The Sith Lords: {played:?}"),
                played == session(vfs::Game::Kotor2),
            )
        })
        .and_then(|()| {
            expect(
                &format!("one installed, no question: {unasked:?}, {opened_alone:?}, {made_k1:?}"),
                unasked == vec![Intent::Campaign(vfs::Game::Kotor1)]
                    && opened_alone == Some("chargen")
                    && made_k1 == session(vfs::Game::Kotor1),
            )
        })
        .map(|()| "asked, picked, backed out, one game".into())
    })]
}

/// Two saves, the latest first (the second a copy of Ana's), and the area
/// a save would be made in.
fn saves_vm() -> ViewModel {
    let save = |id: &str, name: &str, module: &str| crate::vm::SaveView {
        id: id.into(),
        name: name.into(),
        area: "Taris".into(),
        module: module.into(),
        when: "2026-10-02 18:25 UTC".into(),
        ..crate::vm::SaveView::default()
    };
    ViewModel {
        saves: vec![
            save("quick", "Quick save", "tar_m02aa"),
            crate::vm::SaveView {
                from: Some("Ana".into()),
                ..save("0001", "First steps", "end_m01aa")
            },
        ],
        here: Some(save("", "Upper City", "tar_m02ab")),
        load_in_play: true,
        ..ViewModel::default()
    }
}

/// The rows `LB_GAMES` shows.
fn games(stack: &Stack, vm: &ViewModel) -> Vec<String> {
    stack
        .state_of("LB_GAMES", vm)
        .and_then(|state| state.list)
        .map_or(Vec::new(), |list| {
            list.rows
                .iter()
                .map(|row| String::from_utf8_lossy(&row.text).into_owned())
                .collect()
        })
}

/// The top screen and the name its box offers.
fn named(stack: &mut Stack) -> (&'static str, String) {
    let name = stack.edited("EDITBOX").unwrap_or(Vec::new());
    (
        stack.top().map_or("", |id| id.0),
        String::from_utf8_lossy(&name).into_owned(),
    )
}

/// A save is chosen as the cursor comes to it, so a pad's A on it loads,
/// as X (a double click) does.
pub(super) fn pad_saves() -> Vec<Check> {
    let mut assets = Fake::new();
    assets.layouts.insert("saveload", saveload());
    let vm = saves_vm();
    let mut by_a = opened("load", &mut assets, &vm);
    by_a.press("LB_GAMES");
    by_a.step(&mut assets, &vm, &keys(&[]));
    let a = by_a.step(&mut assets, &vm, &keys(&[Key::Activate])).intents;
    let mut by_x = opened("load", &mut assets, &vm);
    by_x.press("LB_GAMES");
    by_x.step(&mut assets, &vm, &keys(&[]));
    let x = by_x.step(&mut assets, &vm, &keys(&[Key::Primary])).intents;
    vec![check(
        "pad_saves",
        expect(
            &format!("A loads as X does: {a:?} / {x:?}"),
            !a.is_empty() && a == x,
        )
        .map(|()| format!("{a:?}")),
    )]
}

pub(super) fn saves() -> Vec<Check> {
    let mut assets = Fake::new();
    assets.layouts.insert("saveload", saveload());
    assets.layouts.insert("savename", savename());
    assets.layouts.insert("confirm", confirm_box());
    let vm = saves_vm();
    // A new save: its name asked in KOTOR's box, the area's offered.
    let mut fresh = opened("save", &mut assets, &vm);
    let rows = games(&fresh, &vm);
    press(&mut fresh, &mut assets, &vm, "BTN_SAVELOAD");
    let offered = named(&mut fresh);
    press(&mut fresh, &mut assets, &vm, "BTN_CANCEL");
    let cancelled = fresh.top().map(|id| id.0);
    // An old save: overwriting it confirmed first, then its name offered,
    // typed over; OK saves.
    let mut save = opened("save", &mut assets, &vm);
    save.step(&mut assets, &vm, &keys(&[Key::Down]));
    let shot = save
        .state_of("LBL_SCREENSHOT", &vm)
        .and_then(|state| state.fill);
    press(&mut save, &mut assets, &vm, "BTN_SAVELOAD");
    let confirming = save.top().map(|id| id.0);
    press(&mut save, &mut assets, &vm, "BTN_OK");
    let over = named(&mut save);
    save.step(&mut assets, &vm, &typed("Mine"));
    let saved = press(&mut save, &mut assets, &vm, "BTN_OK");
    // A row chosen writes nothing until Save; a copy's name is offered
    // unmarked.
    let mut doomed = opened("save", &mut assets, &vm);
    doomed.step(&mut assets, &vm, &keys(&[Key::Down, Key::Down]));
    let chosen = doomed
        .step(&mut assets, &vm, &keys(&[Key::Activate]))
        .intents;
    let still = doomed.top().map(|id| id.0);
    press(&mut doomed, &mut assets, &vm, "BTN_SAVELOAD");
    press(&mut doomed, &mut assets, &vm, "BTN_OK");
    let over_copy = named(&mut doomed);
    press(&mut doomed, &mut assets, &vm, "BTN_CANCEL");
    let deleted = press(&mut doomed, &mut assets, &vm, "BTN_DELETE");
    let mut load = opened("load", &mut assets, &vm);
    let load_rows = games(&load, &vm);
    let loaded = press(&mut load, &mut assets, &vm, "BTN_SAVELOAD");
    let none = ViewModel::default();
    let empty = opened("load", &mut assets, &none);
    let nothing = empty
        .state_of("BTN_SAVELOAD", &none)
        .is_some_and(|state| state.disabled)
        && games(&empty, &none) == ["No saved games"];
    let mut solo = opened("solo", &mut assets, &vm);
    solo.step(&mut assets, &vm, &keys(&[Key::Up, Key::Down]));
    let menu = press(&mut solo, &mut assets, &vm, "BTN_SAVELOAD");
    // A save hosted for the seats chosen (I31: its guests take its
    // companions), the cycler not greyed while it is pointed at.
    let mut host = opened("host", &mut assets, &vm);
    host.step(&mut assets, &vm, &keys(&[Key::Up, Key::Down]));
    press(&mut host, &mut assets, &vm, "BTN_DELETE");
    let cycler = host.state_of("BTN_DELETE", &vm);
    let hosted = press(&mut host, &mut assets, &vm, "BTN_SAVELOAD");
    let load = |id: &str| {
        vec![Intent::Session(SessionIntent::Solo {
            save: Some(id.into()),
            character: None,
            campaign: None,
        })]
    };
    vec![check("save_load", {
        expect(
            &format!("a new save, then the saves, a copy marked: {rows:?}"),
            rows == ["New save", "Quick save", "First steps (from Ana)"],
        )
        .and_then(|()| {
            let name = |name: &str| ("save_name", name.to_string());
            expect(
                &format!(
                    "the area's name offered, Cancel back to the list; overwriting \
                     confirmed, the save's name over it, a copy's unmarked: {offered:?} \
                     {cancelled:?} {confirming:?} {over:?} {over_copy:?}"
                ),
                offered == name("Upper City")
                    && cancelled == Some("save")
                    && confirming == Some("save_overwrite")
                    && over == name("Quick save")
                    && over_copy == name("First steps"),
            )
        })
        .and_then(|()| {
            expect(
                &format!("a row chosen writes nothing: {chosen:?} {still:?}"),
                chosen.is_empty() && still == Some("save"),
            )
        })
        .and_then(|()| {
            expect(
                &format!("the save's module's image: {shot:?}"),
                shot.as_deref() == Some("load_tar_m02aa"),
            )
        })
        .and_then(|()| {
            expect(
                &format!("OK writes over the save pointed at, as typed: {saved:?}"),
                saved
                    == vec![Intent::Save {
                        slot: Some("quick".into()),
                        name: "Mine".into(),
                    }],
            )
        })
        .and_then(|()| {
            expect(
                &format!("Delete the save pointed at: {deleted:?}"),
                deleted == vec![Intent::DeleteSave("0001".into())],
            )
        })
        .and_then(|()| {
            expect(
                &format!("loading lists the saves and loads one: {load_rows:?} {loaded:?}"),
                load_rows == ["Quick save", "First steps (from Ana)"] && loaded == load("quick"),
            )
        })
        .and_then(|()| expect("with no saves, nothing to load", nothing))
        .and_then(|()| {
            expect(
                &format!("the main menu loads the save pointed at: {menu:?}"),
                menu == load("quick"),
            )
        })
        .and_then(|()| {
            expect(
                &format!("a save hosted for the seats chosen: {hosted:?}"),
                cycler.is_some_and(|state| !state.disabled)
                    && hosted
                        == vec![Intent::Session(SessionIntent::Host {
                            save: Some("quick".into()),
                            seats: crate::screens::main_menu::SEATS % 3 + 1,
                            port: crate::intent::DEFAULT_PORT,
                            character: None,
                            campaign: None,
                        })],
            )
        })
        .map(|()| "save, overwrite, delete, load, host a save".into())
    })]
}

fn bar() -> Gui {
    let icon = |tag: &str, left: i32| Control {
        kind: Kind::ProtoItem,
        border: Some(fmt_odyssey::gui::Border {
            fill: format!("{}_dim", tag.to_ascii_lowercase()),
            ..fmt_odyssey::gui::Border::default()
        }),
        hilight: Some(fmt_odyssey::gui::Border {
            fill: format!("{}_lit", tag.to_ascii_lowercase()),
            ..fmt_odyssey::gui::Border::default()
        }),
        ..button(tag, [left, 32, 64, 64], b"")
    };
    root(
        640,
        480,
        vec![
            icon("LBLH_EQU", 71),
            icon("LBLH_OPT", 505),
            button("BTN_EQU", [77, 43, 52, 40], b""),
            button("BTN_OPT", [511, 43, 52, 40], b""),
            button("LBL_CHAR2", [400, 420, 32, 32], b""),
            button("BTN_CHANGE2", [400, 420, 32, 32], b""),
        ],
    )
}

fn options_in_game() -> Gui {
    root(
        640,
        480,
        vec![
            linked("BTN_SAVEGAME", 1, 120, -1, 2),
            linked("BTN_GAMEPLAY", 2, 160, 1, 6),
            linked("BTN_AUTOPAUSE", 6, 200, 2, 3),
            linked("BTN_QUIT", 3, 240, 6, 10),
            linked("BTN_EXIT", 10, 400, 3, -1),
        ],
    )
}

/// Presses `quit`, then `answer` in the box it opens: the box's name, and
/// what the answer raised.
fn quit_through(
    stack: &mut Stack,
    assets: &mut Fake,
    vm: &ViewModel,
    quit: &str,
    answer: &str,
) -> (Option<&'static str>, Vec<Intent>) {
    press(stack, assets, vm, quit);
    let asked = stack.top().map(|screen| screen.0);
    (asked, press(stack, assets, vm, answer))
}

pub(super) fn in_game() -> Vec<Check> {
    let mut assets = Fake::new();
    assets.layouts.insert("confirm", confirm_box());
    assets.layouts.insert("top", bar());
    assets.layouts.insert("optionsingame", options_in_game());
    assets.layouts.insert("map", root(640, 480, Vec::new()));
    let vm = ViewModel::default();
    let mut stack = opened("options_in_game", &mut assets, &vm);
    let frame = stack.step(&mut assets, &vm, &keys(&[]));
    let lit = stack
        .state_of("LBLH_OPT", &vm)
        .is_some_and(|state| state.selected);
    let dim = stack
        .state_of("LBLH_EQU", &vm)
        .is_some_and(|state| !state.selected);
    let drawn_bar = frame
        .layers
        .first()
        .is_some_and(|layer| layer.quads.len() >= 2);
    let breaks = {
        let mut menu = opened("options_in_game", &mut assets, &vm);
        press(&mut menu, &mut assets, &vm, "BTN_AUTOPAUSE")
    };
    let unsaved = stack
        .state_of("BTN_SAVEGAME", &vm)
        .is_some_and(|state| state.disabled);
    let mut kept = opened("options_in_game", &mut assets, &vm);
    let (_, cancelled) = quit_through(&mut kept, &mut assets, &vm, "BTN_QUIT", "BTN_CANCEL");
    let kept = cancelled.is_empty() && kept.top().map(|screen| screen.0) == Some("options_in_game");
    let (asked, quit) = quit_through(
        &mut opened("options_in_game", &mut assets, &vm),
        &mut assets,
        &vm,
        "BTN_QUIT",
        "BTN_OK",
    );
    let (_, desktop) = quit_through(
        &mut opened("options_in_game", &mut assets, &vm),
        &mut assets,
        &vm,
        "BTN_QUITDESKTOP",
        "BTN_OK",
    );
    let placed = {
        // From Gameplay (the first focus) down past Break to Quit, then on.
        let mut menu = opened("options_in_game", &mut assets, &vm);
        let mut down = |times: usize| {
            for _ in 0..times {
                menu.step(&mut assets, &vm, &keys(&[Key::Down]));
            }
            menu.focused().map(str::to_string)
        };
        (down(3), down(1))
    };
    stack.step(&mut assets, &vm, &keys(&[Key::PrevTab]));
    let swapped = stack.top().map(|screen| screen.0);
    stack.step(&mut assets, &vm, &keys(&[Key::NextTab]));
    press(&mut stack, &mut assets, &vm, "BTN_EXIT");
    let exited = !stack.is_open();
    let mut back = opened("options_in_game", &mut assets, &vm);
    back.veil_world = true;
    let veiled = back
        .step(&mut assets, &vm, &keys(&[]))
        .layers
        .first()
        .and_then(|layer| layer.quads.first())
        .is_some_and(|quad| covers(quad.rect) && quad.color[3] > 0.3);
    let switched = back.switch("join", &vm) && back.top().map(|screen| screen.0) == Some("join");
    back.switch("options_in_game", &vm);
    back.step(&mut assets, &vm, &keys(&[Key::Back]));
    // A companion beside the player: its small portrait, which shows it.
    let party = ViewModel {
        party: vec![
            crate::vm::MemberView::default(),
            crate::vm::MemberView {
                portrait: Some("po_ptrask".into()),
                ..crate::vm::MemberView::default()
            },
        ],
        ..ViewModel::default()
    };
    let mut with_trask = opened("options_in_game", &mut assets, &party);
    let small = with_trask
        .state_of("LBL_CHAR2", &party)
        .and_then(|state| state.fill);
    let alone = opened("options_in_game", &mut assets, &vm).hidden("LBL_CHAR2", &vm);
    let viewed = press(&mut with_trask, &mut assets, &party, "BTN_CHANGE2");
    vec![check("menu_bar_tabs", {
        expect("the bar drawn with the tab", drawn_bar)
            .and_then(|()| expect("the options' icon lit, the others not", lit && dim))
            .and_then(|()| {
                expect(
                    &format!("the break button calls a break: {breaks:?}"),
                    breaks.contains(&Intent::Game(GameAction::Break)),
                )
            })
            .and_then(|()| expect("no saving without a world here", unsaved))
            .and_then(|()| {
                expect(
                    &format!("quit asks, then goes back to the menu: {asked:?} {quit:?}"),
                    asked == Some("quit_confirm")
                        && quit == vec![Intent::Session(SessionIntent::ToMenu)],
                )
            })
            .and_then(|()| expect("Cancel keeps the game", kept))
            .and_then(|()| {
                expect(
                    &format!("the desktop button under Quit quits: {desktop:?} {placed:?}"),
                    desktop == vec![Intent::Quit]
                        && placed.0.as_deref() == Some("BTN_QUITDESKTOP")
                        && placed.1.as_deref() == Some("BTN_EXIT"),
                )
            })
            .and_then(|()| {
                expect(
                    &format!("the shoulder swaps to the map: {swapped:?}"),
                    swapped == Some("map"),
                )
            })
            .and_then(|()| expect("Exit closes the menu", exited))
            .and_then(|()| expect("the live world dimmed under it", veiled))
            .and_then(|()| expect("a hotkey switches the open tab", switched))
            .and_then(|()| expect("Back closes the menu", !back.is_open()))
            .and_then(|()| {
                expect(
                    &format!("a companion's portrait, which shows it: {small:?} {viewed:?}"),
                    alone && small.as_deref() == Some("po_ptrask") && viewed == [Intent::View(1)],
                )
            })
            .map(|()| {
                "the bar, the options, quit (asked, to the menu or the desktop), the party".into()
            })
    })]
}

/// `confirm` as the game places it: a popup, with Cancel beside OK.
fn confirm_box() -> Gui {
    let mut list = button("LB_MESSAGE", [6, 6, 278, 40], b"");
    list.kind = Kind::ListBox;
    list.proto = Some(Box::new(Control {
        kind: Kind::Label,
        ..button("PROTOITEM", [8, 8, 258, 18], b"")
    }));
    Gui {
        extent: Rect {
            left: 174,
            top: 210,
            width: 290,
            height: 87,
        },
        ..root(
            640,
            480,
            vec![
                list,
                linked("BTN_OK", 1, 60, -1, -1),
                linked("BTN_CANCEL", 2, 60, -1, -1),
            ],
        )
    }
}

pub(super) fn review() -> Vec<Check> {
    let mut assets = Fake::new();
    assets.layouts.insert("confirm", confirm_box());
    let vm = ViewModel {
        session: crate::vm::SessionView {
            error: Some("why".into()),
            ..crate::vm::SessionView::default()
        },
        ..ViewModel::default()
    };
    let mut broken = Stack::new(crate::screens::open);
    broken.push(Box::new(crate::screens::Layout("no_such_layout".into())));
    let refused = broken.step(&mut assets, &vm, &keys(&[])).refusals;
    let gone = !broken.is_open();
    let mut message = opened("message", &mut assets, &vm);
    let over_text = UiInput {
        // 20 px into the box (a short message leaves it as it is), the
        // layout at its own size 160 and 60 in.
        pointer: Some([160.0 + 174.0 + 20.0, 60.0 + 210.0 + 20.0]),
        moved: true,
        window: (960.0, 600.0),
        ..keys(&[])
    };
    message.step(&mut assets, &vm, &over_text);
    let still_ok = message.focused().map(str::to_string);
    let at = message.under_pointer(&vm, &over_text);
    vec![
        check("broken_layout_closes", {
            expect(
                &format!("refused once and closed: {refused:?}"),
                gone && refused.len() == 1,
            )
            .map(|()| "never a dead screen".into())
        }),
        check("message_text_not_focusable", {
            expect(
                &format!("the pointer over the message ({at:?}) leaves OK focused: {still_ok:?}"),
                still_ok.as_deref() == Some("BTN_OK") && at.as_deref() == Some("LB_MESSAGE"),
            )
            .map(|()| "OK keeps the focus".into())
        }),
        check("message_box_grows_as_kotors", message_box_grows()),
        check("tutorial_pages_grow_each", pages_grow()),
    ]
}

/// A message too long for `confirm` grows it as KOTOR's message box does:
/// taller to 161, wider by 40s to past 440 (290 → 450), taller past 279; the
/// buttons go down with the list, and what still overflows scrolls.
fn message_box_grows() -> Result<String, String> {
    use crate::screen::Screen;
    let shaped = |words: &str| {
        let mut gui = confirm_box();
        let mut message = crate::screens::message::Message::new(words);
        message.fit(&mut gui, &mut Fake::new(), crate::skin::Source::Campaign);
        let rows = message
            .state(
                gui.find("LB_MESSAGE").expect("the list"),
                &ViewModel::default(),
            )
            .list
            .map_or(0, |list| list.rows.len());
        let shown = crate::paint::rows_that_fit(gui.find("LB_MESSAGE").expect("the list"), None);
        let ok = gui.find("BTN_OK").expect("OK").extent;
        (gui.extent, ok, rows, shown)
    };
    let (short, ..) = shaped("why");
    // Words of the test font's four glyphs.
    let (long, ok, rows, shown) = shaped(&"\u{1}\u{2}\u{3} ".repeat(400));
    let grown = long.height - 87;
    expect(
        &format!("short {short:?}; long {long:?}, OK at {ok:?}, {rows} rows in {shown}"),
        (short.width, short.height) == (290, 87)
            && long.width == 450
            && long.left == 174 - 80
            && long.height > 279
            && (210 - grown / 2..210).contains(&long.top)
            && (ok.left, ok.top) == (300 + 80, 60 + grown)
            && rows > shown,
    )?;
    Ok(format!("{long:?}, {rows} lines in {shown} rows"))
}

/// Each of a tutorial's pages is a box of its own grown to its words, OK
/// on one opens the next, and on the last only closes.
fn pages_grow() -> Result<String, String> {
    use crate::screen::Screen;
    use crate::screens::message::Pages;
    let vm = ViewModel {
        pages: vec![b"why".to_vec(), "\u{1}\u{2}\u{3} ".repeat(400).into_bytes()],
        ..ViewModel::default()
    };
    let page = |at: &str| {
        let mut gui = confirm_box();
        let mut page = Pages::new(at, &vm).expect("a page");
        page.fit(&mut gui, &mut Fake::new(), crate::skin::Source::Campaign);
        let mut out = Vec::new();
        page.press("BTN_OK", &vm, &mut out);
        (gui.extent.height, out)
    };
    let (short, turned) = page("0");
    let (long, closed) = page("1");
    expect(
        &format!("heights {short} then {long}; OK {turned:?}, then {closed:?}"),
        short == 87
            && long > 279
            && turned == [Intent::Close, Intent::OpenFor("pages", "1".into())]
            && closed == [Intent::Close],
    )?;
    Ok(format!("pages {short} then {long} tall"))
}

fn slider(tag: &str, id: i32, top: i32) -> Control {
    Control {
        kind: Kind::Slider,
        thumb: Some(fmt_odyssey::gui::Image {
            image: "thumb".into(),
            ..fmt_odyssey::gui::Image::default()
        }),
        ..linked(tag, id, top, -1, -1)
    }
}

fn check_box(tag: &str, id: i32, top: i32) -> Control {
    Control {
        kind: Kind::CheckBox,
        ..linked(tag, id, top, -1, -1)
    }
}

/// The key mapping's page and its entry, as the layouts have them.
fn keymap_layouts(assets: &mut Fake) {
    assets.layouts.insert("optkeymapping", {
        let mut list = button("LST_EventList", [20, 60, 400, 300], b"");
        list.kind = Kind::ListBox;
        list.proto = Some(Box::new(button("PROTOITEM", [20, 60, 400, 30], b"")));
        root(
            640,
            480,
            vec![
                list,
                linked("BTN_Accept", 1, 400, -1, -1),
                linked("BTN_Cancel", 2, 440, -1, -1),
                linked("BTN_Default", 3, 460, -1, -1),
                linked("BTN_Filter_Move", 4, 20, -1, -1),
                linked("BTN_Filter_Game", 5, 30, -1, -1),
                linked("BTN_Filter_Mini", 6, 40, -1, -1),
            ],
        )
    });
    assets.layouts.insert("confirm", confirm());
}

/// Rebinding (`key_mapping`): the pad's tab drawn as its own buttons (the
/// unbound last), a
/// chord taken from the pad, a binding another action has named and then
/// swapped, one shared by design taken at once, Menu cancelling, a pad's
/// press let be by the keyboard's entry, and Default resetting the tab's
/// device alone.
pub(super) fn key_rebinding() -> Vec<Check> {
    use crate::vm::KeyRow;
    let mut assets = Fake::new();
    keymap_layouts(&mut assets);
    let row = |action: &str, label: &str, key: &str, shares: &[&str]| KeyRow {
        action: action.into(),
        label: label.into(),
        key: key.into(),
        movement: action == "jump",
        shares: shares.iter().map(|name| name.to_string()).collect(),
    };
    let vm = ViewModel {
        keys: vec![
            row("jump", "Jump", "Space", &[]),
            row("use", "Use", "KeyE", &[]),
        ],
        pad_keys: vec![
            row("report", "Report a bug", "", &[]),
            row("jump", "Jump", "South", &[]),
            row("crouch", "Crouch", "East", &["leave_talk"]),
            row("use", "Use", "West", &[]),
            row("leave_talk", "Leave", "North", &["crouch"]),
        ],
        pad_names: [
            ("South", "A"),
            ("East", "B"),
            ("West", "X"),
            ("North", "Y"),
            ("LeftTrigger", "LB"),
        ]
        .map(|(button, name)| (button.to_string(), name.to_string()))
        .to_vec(),
        pad_kind: "xbox".into(),
        ..ViewModel::default()
    };
    let pad = |button: &str| UiInput {
        raw: vec![format!("{}{button}", crate::screens::key_mapping::PAD_RAW)],
        keys: vec![Key::Activate],
        ..keys(&[])
    };
    let on = |stack: &Stack, screen: &str| stack.top().map(|id| id.0) == Some(screen);
    let mut page = opened("options_keymap", &mut assets, &vm);
    press(&mut page, &mut assets, &vm, "BTN_Filter_Mini");
    let rows: Vec<(String, Option<String>)> = page
        .state_of("LST_EventList", &vm)
        .and_then(|state| state.list)
        .map(|list| {
            list.rows
                .iter()
                .map(|row| {
                    (
                        String::from_utf8_lossy(&row.text).into_owned(),
                        row.badge.clone().or_else(|| {
                            let value = row.value.as_deref()?;
                            Some(String::from_utf8_lossy(value).into_owned())
                        }),
                    )
                })
                .collect()
        })
        .unwrap_or_else(Vec::new);
    // The tab's button keeps the focus; a row is chosen on the list.
    press(&mut page, &mut assets, &vm, "LST_EventList");
    let chorded = page.step(&mut assets, &vm, &pad("LB+South")).intents;
    let chord_closed = on(&page, "options_keymap");
    page.step(&mut assets, &vm, &keys(&[Key::Activate]));
    let first = page.step(&mut assets, &vm, &pad("West")).intents;
    let named = page
        .state_of("LB_MESSAGE", &vm)
        .and_then(|state| state.list?.rows.pop())
        .map(|row| String::from_utf8_lossy(&row.text).into_owned())
        .unwrap_or_else(String::new);
    let swapped = page.step(&mut assets, &vm, &pad("West")).intents;
    page.step(&mut assets, &vm, &keys(&[Key::Down, Key::Activate]));
    let shared = page.step(&mut assets, &vm, &pad("North")).intents;
    page.step(&mut assets, &vm, &keys(&[Key::Activate]));
    let cancelled = page.step(&mut assets, &vm, &pad("Start")).intents;
    let cancel_closed = on(&page, "options_keymap");
    let reset = press(&mut page, &mut assets, &vm, "BTN_Default");
    press(&mut page, &mut assets, &vm, "BTN_Filter_Move");
    press(&mut page, &mut assets, &vm, "LST_EventList");
    let ignored = page.step(&mut assets, &vm, &pad("South")).intents;
    let still_open = on(&page, "key_entry");
    let setting = |key: &str, value: &str| Intent::Setting(key.into(), value.into());
    vec![check("key_rebinding", {
        expect(
            &format!("the pad's tab as its own buttons, the unbound last: {rows:?}"),
            rows == [
                ("Jump".into(), Some("kjkpad_xbox_south".into())),
                ("Crouch".into(), Some("kjkpad_xbox_east".into())),
                ("Use".into(), Some("kjkpad_xbox_west".into())),
                ("Leave".into(), Some("kjkpad_xbox_north".into())),
                ("Report a bug".into(), Some("-".into())),
            ],
        )
        .and_then(|()| {
            expect(
                &format!("an LB chord taken: {chorded:?} {chord_closed}"),
                chorded == vec![setting("pad.jump", "LB+South")] && chord_closed,
            )
        })
        .and_then(|()| {
            expect(
                &format!("another's binding named first: {first:?} {named:?}"),
                first
                    .iter()
                    .all(|intent| !matches!(intent, Intent::Setting(..)))
                    && named == "X is Use's. Press X again to swap them.",
            )
        })
        .and_then(|()| {
            expect(
                &format!("pressed again, the two swap: {swapped:?}"),
                swapped == vec![setting("pad.use", "South"), setting("pad.jump", "West")],
            )
        })
        .and_then(|()| {
            expect(
                &format!("one shared by design taken at once: {shared:?}"),
                shared == vec![setting("pad.crouch", "North")],
            )
        })
        .and_then(|()| {
            expect(
                &format!("Menu cancels: {cancelled:?} {cancel_closed}"),
                cancelled.is_empty() && cancel_closed,
            )
        })
        .and_then(|()| {
            expect(
                &format!("Default resets the pad alone: {reset:?}"),
                reset == vec![setting("reset", "pad")],
            )
        })
        .and_then(|()| {
            expect(
                &format!("the keyboard's entry lets a pad's press be: {ignored:?} {still_open}"),
                ignored.is_empty() && still_open,
            )
        })
        .map(|()| {
            "pad tab, chord, named clash, swap, shared, cancel, default, devices apart".into()
        })
    })]
}

/// A pad on a screen (the key mapping): its hints say what each button
/// does there, Menu presses OK, Y Default, the bumpers step its own tabs
/// (it has no menu bar), B cancels.
pub(super) fn pad_menus() -> Vec<Check> {
    let mut assets = Fake::new();
    keymap_layouts(&mut assets);
    let vm = ViewModel {
        keys: vec![crate::vm::KeyRow {
            action: "jump".into(),
            label: "Jump".into(),
            key: "Space".into(),
            movement: true,
            ..crate::vm::KeyRow::default()
        }],
        pad_playing: true,
        ..ViewModel::default()
    };
    let mut page = opened("options_keymap", &mut assets, &vm);
    let hints: Vec<(Vec<&str>, String)> = page
        .hints(&mut assets, &vm)
        .into_iter()
        .map(|hint| {
            (
                hint.buttons,
                String::from_utf8_lossy(&hint.words).into_owned(),
            )
        })
        .collect();
    page.step(&mut assets, &vm, &keys(&[Key::NextTab]));
    // A pad playing, it opened on the controller's tab, the last: on is
    // around, to the first.
    let tabbed = page
        .state_of("BTN_Filter_Move", &vm)
        .is_some_and(|state| state.selected);
    let defaulted = page.step(&mut assets, &vm, &keys(&[Key::Second])).intents;
    let accepted = page.step(&mut assets, &vm, &keys(&[Key::Accept]));
    let closed = !page.is_open();
    vec![check("pad_menus", {
        expect(
            &format!("its hints: {hints:?}"),
            hints
                .iter()
                .map(|(buttons, _)| buttons.clone())
                .collect::<Vec<_>>()
                == [
                    vec!["South"],
                    vec!["North"],
                    vec!["East"],
                    vec!["Start"],
                    vec!["LeftTrigger", "RightTrigger"],
                ]
                && [0, 2, 4].map(|at| hints[at].1.as_str()) == ["Select", "Cancel", "Tabs"],
        )
        .and_then(|()| expect("the bumpers step its own tabs, around", tabbed))
        .and_then(|()| {
            expect(
                &format!("Y presses Default: {defaulted:?}"),
                defaulted == vec![Intent::Setting("reset".into(), "keys".into())],
            )
        })
        .and_then(|()| {
            expect(
                &format!("Menu presses OK: {:?} closed {closed}", accepted.intents),
                closed,
            )
        })
        .map(|()| "hints, tabs, Default, OK".into())
    })]
}

/// Every kind of input said as a player reads it.
pub(super) fn key_names() -> Vec<Check> {
    use crate::screens::key_mapping::key_name;
    let said = [
        ("KeyW", "W"),
        ("Digit1", "1"),
        ("ShiftLeft", "Left Shift"),
        ("ControlRight", "Right Ctrl"),
        ("BracketLeft", "["),
        ("Numpad5", "Num 5"),
        ("NumpadEnter", "Num Enter"),
        ("PageUp", "Page Up"),
        ("F12", "F12"),
        ("MouseLeft", "Left mouse"),
        ("MouseMiddle", "Middle mouse"),
        ("MouseBack", "Mouse 4"),
        ("MouseForward", "Mouse 5"),
        ("Mouse8", "Mouse 8"),
    ];
    let wrong: Vec<String> = said
        .iter()
        .filter(|(key, shown)| key_name(key) != *shown)
        .map(|(key, _)| format!("{key} as {}", key_name(key)))
        .collect();
    vec![check(
        "key_names",
        expect(&format!("misnamed: {wrong:?}"), wrong.is_empty())
            .map(|()| format!("{} keys and buttons named", said.len())),
    )]
}

pub(super) fn options() -> Vec<Check> {
    let mut assets = Fake::new();
    assets.layouts.insert(
        "optmouse",
        root(
            640,
            480,
            vec![
                slider("SLI_MOUSESEN", 4, 147),
                check_box("CB_REVBUTTONS", 6, 200),
                linked("BTN_BACK", 2, 400, -1, -1),
            ],
        ),
    );
    assets.layouts.insert(
        "optgameplay",
        root(
            640,
            480,
            vec![
                check_box("CB_INVERTCAM", 0, 100),
                check_box("CB_LEVELUP", 1, 140),
                check_box("CB_AUTOSAVE", 2, 180),
                check_box("CB_REVERSE", 5, 220),
                linked("BTN_DIFFICULTY", 6, 60, -1, -1),
                linked("BTN_DIFFLEFT", 3, 60, -1, -1),
                linked("BTN_DIFFRIGHT", 4, 60, -1, -1),
                linked("BTN_KEYMAP", 12, 300, -1, -1),
                linked("BTN_MOUSE", 13, 340, -1, -1),
                linked("BTN_BACK", 10, 400, -1, -1),
                super::vm_screens::label("LB_DESC", 440),
            ],
        ),
    );
    assets.layouts.insert(
        "optionsmain",
        root(
            640,
            480,
            vec![
                linked("BTN_AUTOPAUSE", 0, 100, -1, -1),
                linked("BTN_BACK", 1, 400, -1, -1),
            ],
        ),
    );
    assets.layouts.insert(
        "optgraphics",
        root(
            640,
            480,
            vec![
                linked("BTN_RESOLUTION", 0, 100, -1, -1),
                linked("BTN_ADVANCED", 1, 140, -1, -1),
                check_box("CB_SHADOWS", 2, 180),
                linked("BTN_BACK", 3, 400, -1, -1),
            ],
        ),
    );
    keymap_layouts(&mut assets);
    let vm = ViewModel {
        settings: [
            ("mouse.sensitivity".to_string(), "0.5".to_string()),
            ("camera.invert".to_string(), "false".to_string()),
        ]
        .into_iter()
        .collect(),
        movie_duels: true,
        keys: vec![crate::vm::KeyRow {
            action: "forward".into(),
            label: "Forward".into(),
            key: "KeyW".into(),
            movement: true,
            ..crate::vm::KeyRow::default()
        }],
        ..ViewModel::default()
    };
    let mut mouse = opened("options_mouse", &mut assets, &vm);
    let slid = mouse.step(&mut assets, &vm, &keys(&[Key::Right])).intents;
    let thumb_drawn = mouse
        .step(&mut assets, &vm, &keys(&[]))
        .layers
        .first()
        .is_some_and(|layer| {
            layer
                .quads
                .iter()
                .any(|quad| quad.texture != crate::draw::TextureId::WHITE)
        });
    let mut gameplay = opened("options_gameplay", &mut assets, &vm);
    let greyed = gameplay
        .state_of("CB_LEVELUP", &vm)
        .is_some_and(|state| state.disabled);
    let inverted = press(&mut gameplay, &mut assets, &vm, "CB_INVERTCAM");
    // The gameplay page opens the controller's page from its difficulty
    // button and the sabers' from its reverse box.
    let opens = ["BTN_DIFFICULTY", "CB_REVERSE"].map(|tag| {
        let mut page = opened("options_gameplay", &mut assets, &vm);
        press(&mut page, &mut assets, &vm, tag);
        page.top().map(|screen| screen.0)
    });
    // On the sabers' page the rules step both ways from JA's (unset) to
    // MDA, the only other (Movie Duels' plain mode is not offered), a
    // box turns a duel option on, the hilts step on from KOTOR's (unset)
    // and a box turns the stances on.
    let mut sabers = opened("options_sabers", &mut assets, &vm);
    let picked = [
        "BTN_DIFFRIGHT",
        "BTN_DIFFLEFT",
        "CB_AUTOSAVE",
        "BTN_MOUSE",
        "BTN_KEYMAP",
        "CB_REVERSE",
    ]
    .map(|tag| press(&mut sabers, &mut assets, &vm, tag));
    // Without Movie Duels installed: MDA saved is shown and played as JA's,
    // the rules and stances greyed and stepping nowhere, the hilts
    // stepping among KOTOR's and JA's own (a Movie Duels hilt saved shown
    // as KOTOR's), the rules' description saying why.
    let bare = ViewModel {
        settings: [
            ("game.saber_rules", "mda"),
            ("saber.hilt.single", "luke_ep6"),
            ("saber.hilt.double", "dual_1"),
        ]
        .into_iter()
        .map(|(key, value)| (key.to_string(), value.to_string()))
        .collect(),
        ..ViewModel::default()
    };
    let mut plain = opened("options_sabers", &mut assets, &bare);
    let stepped = [
        "BTN_DIFFRIGHT",
        "BTN_DIFFLEFT",
        "CB_REVERSE",
        "BTN_MOUSE",
        "BTN_KEYMAP",
    ]
    .map(|tag| press(&mut plain, &mut assets, &bare, tag));
    let greyed_bare = [
        "BTN_DIFFICULTY",
        "BTN_DIFFLEFT",
        "BTN_DIFFRIGHT",
        "CB_REVERSE",
    ]
    .iter()
    .all(|tag| plain.state_of(tag, &bare).is_some_and(|state| state.greyed));
    let shown_bare = ["BTN_DIFFICULTY", "BTN_MOUSE"]
        .map(|tag| plain.state_of(tag, &bare).and_then(|state| state.text));
    press(&mut plain, &mut assets, &bare, "BTN_DIFFICULTY");
    let needs = crate::strings::bytes(crate::strings::NEEDS_MOVIE_DUELS);
    let told_bare = plain
        .state_of("LB_DESC", &bare)
        .and_then(|state| state.text)
        .is_some_and(|text| text.starts_with(&needs));
    let mut keymap = opened("options_keymap", &mut assets, &vm);
    keymap.step(&mut assets, &vm, &keys(&[Key::Activate]));
    let entry_open = keymap.top().map(|screen| screen.0) == Some("key_entry");
    let rebound = keymap
        .step(
            &mut assets,
            &vm,
            &UiInput {
                raw: vec!["KeyZ".into()],
                keys: vec![Key::Activate],
                ..keys(&[])
            },
        )
        .intents;
    let entry_closed = keymap.top().map(|screen| screen.0) == Some("options_keymap");
    let mut main = opened("options", &mut assets, &vm);
    let debug_label = main
        .state_of("BTN_AUTOPAUSE", &vm)
        .and_then(|state| state.text);
    let debug = press(&mut main, &mut assets, &vm, "BTN_AUTOPAUSE");
    vec![check("options_pages", {
        expect(
            &format!("right from 0.5: {slid:?}"),
            slid == vec![Intent::Setting("mouse.sensitivity".into(), "0.55".into())],
        )
        .and_then(|()| expect("the slider's thumb drawn", thumb_drawn))
        .and_then(|()| expect("what is not built yet is greyed", greyed))
        .and_then(|()| {
            expect(
                &format!("the camera inverted: {inverted:?}"),
                inverted == vec![Intent::Setting("camera.invert".into(), "true".into())],
            )
        })
        .and_then(|()| {
            expect(
                &format!("the controller's and sabers' pages open: {opens:?}"),
                opens == [Some("options_controller"), Some("options_sabers")],
            )
        })
        .and_then(|()| {
            let setting = |key: &str, value: &str| vec![Intent::Setting(key.into(), value.into())];
            expect(
                &format!("the sabers' page: rules, a duel option, hilts, stances: {picked:?}"),
                picked
                    == [
                        setting("game.saber_rules", "mda"),
                        setting("game.saber_rules", "mda"),
                        setting("game.duel_slow_motion", "true"),
                        setting("saber.hilt.single", "single_1"),
                        setting("saber.hilt.double", "dual_1"),
                        setting("game.duel_stances", "true"),
                    ],
            )
        })
        .and_then(|()| {
            let setting = |key: &str, value: &str| vec![Intent::Setting(key.into(), value.into())];
            let bytes = crate::strings::bytes;
            expect(
                &format!(
                    "without Movie Duels: rules and stances still, hilts JA's own, greyed, shown JA's, told why: {stepped:?} {greyed_bare} {shown_bare:?} {told_bare}"
                ),
                stepped
                    == [
                        vec![],
                        vec![],
                        vec![],
                        setting("saber.hilt.single", "single_1"),
                        setting("saber.hilt.double", ""),
                    ]
                    && greyed_bare
                    && shown_bare
                        == [
                            Some(bytes(crate::strings::SABER_RULES_JA)),
                            Some(bytes(crate::strings::HILT_KOTOR)),
                        ]
                    && told_bare,
            )
        })
        .and_then(|()| expect("a row opens the key entry", entry_open))
        .and_then(|()| {
            expect(
                &format!(
                    "the next key is the action's, the entry closes: {rebound:?} {entry_closed}"
                ),
                rebound == vec![Intent::Setting("key.forward".into(), "KeyZ".into())]
                    && entry_closed,
            )
        })
        .and_then(|()| {
            expect(
                &format!("the main page's Debug closes the menus, starts the sandbox: {debug_label:?} {debug:?}"),
                debug_label == Some(crate::strings::bytes(crate::strings::DEBUG))
                    && debug == vec![Intent::Session(SessionIntent::Sandbox)]
                    && main.top().is_none(),
            )
        })
        .map(|()| "sliders, check boxes, greyed, stepped choices, keys, Debug".into())
    })]
}

/// The sound sliders, the advanced graphics page's stepped anti-aliasing
/// and its Cancel, and the screen resolution's list.
pub(super) fn display_options() -> Vec<Check> {
    use crate::vm::DisplayChoice;
    let mut assets = Fake::new();
    assets.layouts.insert(
        "optsound",
        root(
            640,
            480,
            vec![
                slider("SLI_MUSIC", 0, 100),
                slider("SLI_VO", 1, 140),
                linked("BTN_BACK", 2, 400, -1, -1),
            ],
        ),
    );
    assets.layouts.insert(
        "optgraphicsadv",
        root(
            640,
            480,
            vec![
                linked("BTN_ANTIALIAS", 0, 100, -1, -1),
                linked("BTN_ANTIALIASLEFT", 1, 140, -1, -1),
                check_box("CB_VSYNC", 2, 180),
                linked("BTN_CANCEL", 3, 400, -1, -1),
            ],
        ),
    );
    assets.layouts.insert("optresolution", {
        let mut list = button("LB_RESOLUTIONS", [20, 60, 400, 300], b"");
        list.kind = Kind::ListBox;
        list.proto = Some(Box::new(button("PROTOITEM", [20, 60, 400, 30], b"")));
        root(
            640,
            480,
            vec![
                list,
                linked("BTN_OK", 1, 400, -1, -1),
                linked("BTN_CANCEL", 2, 440, -1, -1),
            ],
        )
    });
    let setting = |key: &str, value: &str| (key.to_string(), value.to_string());
    let choice = |mode: &str, size: &str| DisplayChoice {
        mode: mode.into(),
        size: size.into(),
        label: format!("{mode} {size}"),
        current: size == "1280x720",
    };
    let vm = ViewModel {
        settings: [
            setting("sound.music", "1.00"),
            setting("display.msaa", "4"),
            setting("display.vsync", "true"),
        ]
        .into_iter()
        .collect(),
        displays: vec![
            choice("window", "1920x1080"),
            choice("window", "1280x720"),
            choice("borderless", ""),
        ],
        ..ViewModel::default()
    };
    let mut sound = opened("options_sound", &mut assets, &vm);
    let quieter = sound.step(&mut assets, &vm, &keys(&[Key::Left])).intents;
    let mut advanced = opened("options_graphics_advanced", &mut assets, &vm);
    let shown = advanced
        .text_of("BTN_ANTIALIAS", &vm)
        .map(|text| String::from_utf8_lossy(&text).into_owned());
    let on = press(&mut advanced, &mut assets, &vm, "BTN_ANTIALIAS");
    let back = press(&mut advanced, &mut assets, &vm, "BTN_ANTIALIASLEFT");
    let moved = ViewModel {
        settings: [
            setting("display.msaa", "1"),
            setting("display.vsync", "false"),
        ]
        .into_iter()
        .collect(),
        ..vm.clone()
    };
    let cancelled = press(&mut advanced, &mut assets, &moved, "BTN_CANCEL");
    let cancel_closed = advanced.top().is_none();
    let mut resolution = opened("options_resolution", &mut assets, &vm);
    let current = resolution
        .state_of("LB_RESOLUTIONS", &vm)
        .and_then(|state| state.list)
        .and_then(|list| list.selected);
    resolution.step(&mut assets, &vm, &keys(&[Key::Down, Key::Down]));
    resolution.step(&mut assets, &vm, &keys(&[Key::Activate]));
    let applied = press(&mut resolution, &mut assets, &vm, "BTN_OK");
    let ok_closed = resolution.top().is_none();
    vec![check("display_options", {
        expect(
            &format!("the music down from full: {quieter:?}"),
            quieter == vec![Intent::Setting("sound.music".into(), "0.95".into())],
        )
        .and_then(|()| {
            expect(
                &format!("anti-aliasing shows its choice: {shown:?}"),
                shown.as_deref() == Some("Anti-Aliasing: 4x"),
            )
        })
        .and_then(|()| {
            expect(
                &format!("4x steps on to off, back to 2x: {on:?} {back:?}"),
                on == vec![Intent::Setting("display.msaa".into(), "1".into())]
                    && back == vec![Intent::Setting("display.msaa".into(), "2".into())],
            )
        })
        .and_then(|()| {
            expect(
                &format!("Cancel puts the page back and closes: {cancelled:?}"),
                cancelled
                    == vec![
                        Intent::Setting("display.msaa".into(), "4".into()),
                        Intent::Setting("display.vsync".into(), "true".into()),
                    ]
                    && cancel_closed,
            )
        })
        .and_then(|()| {
            expect(
                &format!("the window in use is chosen: {current:?}"),
                current == Some(1),
            )
        })
        .and_then(|()| {
            expect(
                &format!("a row chosen, OK shows it and closes: {applied:?}"),
                applied
                    == vec![
                        Intent::Setting("display.size".into(), String::new()),
                        Intent::Setting("display.mode".into(), "borderless".into()),
                    ]
                    && ok_closed,
            )
        })
        .map(|()| "volumes, anti-aliasing, cancel, resolution".into())
    })]
}

pub(super) fn pages() -> Vec<Check> {
    use crate::vm::{CharacterView, Entry};
    let mut assets = Fake::new();
    let label = |tag: &str| Control {
        kind: Kind::Label,
        ..button(tag, [0, 0, 10, 10], b"")
    };
    let list = |tag: &str| Control {
        kind: Kind::ListBox,
        proto: Some(Box::new(button("PROTOITEM", [0, 0, 100, 20], b""))),
        ..button(tag, [0, 0, 100, 200], b"")
    };
    let mut top = bar();
    top.controls.push(Control {
        kind: Kind::Progress,
        ..label("PB_VIT1")
    });
    assets.layouts.insert("top", top);
    assets
        .layouts
        .insert("equip", root(640, 480, vec![label("LBL_INV_WEAP_R")]));
    assets
        .layouts
        .insert("inventory", root(640, 480, vec![list("LB_ITEMS")]));
    assets.layouts.insert(
        "character",
        root(640, 480, vec![label("LBL_STR"), label("LBL_STR_MOD")]),
    );
    assets.layouts.insert(
        "abilities",
        root(
            640,
            480,
            vec![list("LB_ABILITY"), linked("BTN_POWERS", 3, 400, -1, -1)],
        ),
    );
    assets
        .layouts
        .insert("messages", root(640, 480, vec![list("LB_MESSAGES")]));
    let named = |name: &str| Entry {
        name: name.as_bytes().to_vec(),
        ..Entry::default()
    };
    let vm = ViewModel {
        character: Some(CharacterView {
            hp: (7, 12),
            abilities: [14, 10, 10, 10, 10, 10],
            weapons: [
                Some(Entry {
                    icon: Some("iw_blstrpstl_001".into()),
                    ..named("Blaster pistol")
                }),
                None,
            ],
            owned: ["Medpac", "Stim"]
                .into_iter()
                .map(|name| crate::vm::Owned {
                    entry: named(name),
                    ..crate::vm::Owned::default()
                })
                .collect(),
            skills: vec![named("Repair")],
            powers: vec![named("Push"), named("Heal"), named("Stun")],
            ..CharacterView::default()
        }),
        feedback: vec!["one".into(), "two".into()],
        ..ViewModel::default()
    };
    let rows_of = |stack: &Stack, tag: &str| {
        stack
            .state_of(tag, &vm)
            .and_then(|state| state.list)
            .map_or(0, |list| list.rows.len())
    };
    let words = |stack: &Stack, tag: &str| {
        stack
            .text_of(tag, &vm)
            .map(|text| String::from_utf8_lossy(&text).into_owned())
    };
    let equip = opened("equip", &mut assets, &vm);
    let weapon = equip
        .state_of("LBL_INV_WEAP_R", &vm)
        .and_then(|state| state.fill);
    let vitality = equip.state_of("PB_VIT1", &vm).and_then(|state| state.value);
    let inventory = opened("inventory", &mut assets, &vm);
    let character = opened("character", &mut assets, &vm);
    let (strength, modifier) = (
        words(&character, "LBL_STR"),
        words(&character, "LBL_STR_MOD"),
    );
    let mut abilities = opened("abilities", &mut assets, &vm);
    let skills = rows_of(&abilities, "LB_ABILITY");
    press(&mut abilities, &mut assets, &vm, "BTN_POWERS");
    let powers = rows_of(&abilities, "LB_ABILITY");
    let messages = opened("messages", &mut assets, &vm);
    vec![check("pages_show_the_character", {
        expect(
            &format!("the right hand's icon: {weapon:?}"),
            weapon.as_deref() == Some("iw_blstrpstl_001"),
        )
        .and_then(|()| {
            expect(
                &format!("the bar's vitality {vitality:?}"),
                vitality.is_some_and(|share| (share - 7.0 / 12.0).abs() < 1e-6),
            )
        })
        .and_then(|()| expect("two items carried", rows_of(&inventory, "LB_ITEMS") == 2))
        .and_then(|()| {
            expect(
                &format!("strength and its modifier: {strength:?} {modifier:?}"),
                strength.as_deref() == Some("14") && modifier.as_deref() == Some("+2"),
            )
        })
        .and_then(|()| {
            expect(
                &format!("skills {skills}, then powers {powers}"),
                skills == 1 && powers == 3,
            )
        })
        .and_then(|()| expect("the feedback log", rows_of(&messages, "LB_MESSAGES") == 2))
        .map(|()| "equip, inventory, character, abilities, messages".into())
    })]
}

/// Page `page` (`buttons` and the list `LB_ITEMS`) opened on the host
/// looking at Trask, the second member, dressed (`dresses`) or played by
/// another seat, carrying `owned`: what pressing `tags` raises, and whether
/// `watched` is greyed.
fn on_trask(
    page: &'static str,
    buttons: &[&str],
    owned: crate::vm::Owned,
    dresses: bool,
    tags: &[&str],
    watched: &str,
) -> (Vec<Intent>, bool) {
    let (mut assets, vm) = trask_page(page, buttons, vec![owned], dresses);
    let mut stack = opened(page, &mut assets, &vm);
    let pressed = tags
        .iter()
        .flat_map(|tag| press(&mut stack, &mut assets, &vm, tag))
        .collect();
    let greyed = stack
        .state_of(watched, &vm)
        .is_some_and(|state| state.disabled);
    (pressed, greyed)
}

/// `on_trask`'s page and view: the layout (`buttons`, the list `LB_ITEMS`)
/// and Trask, the second member, carrying `owned`.
fn trask_page(
    page: &'static str,
    buttons: &[&str],
    owned: Vec<crate::vm::Owned>,
    dresses: bool,
) -> (Fake, ViewModel) {
    use crate::vm::{CharacterView, MemberView};
    let mut assets = Fake::new();
    assets.layouts.insert("top", bar());
    let mut controls: Vec<Control> = buttons
        .iter()
        .enumerate()
        .map(|(at, tag)| button(tag, [0, 60 * at as i32, 10, 10], b""))
        .collect();
    controls.push(Control {
        kind: Kind::ListBox,
        proto: Some(Box::new(button("PROTOITEM", [0, 0, 100, 20], b""))),
        ..button("LB_ITEMS", [100, 0, 100, 200], b"")
    });
    assets.layouts.insert(page, root(640, 480, controls));
    let vm = ViewModel {
        character: Some(CharacterView {
            id: 7,
            owned,
            ..CharacterView::default()
        }),
        party: vec![
            MemberView {
                dresses: true,
                ..MemberView::default()
            },
            MemberView {
                dresses,
                ..MemberView::default()
            },
        ],
        viewed: 1,
        ..ViewModel::default()
    };
    (assets, vm)
}

/// An item Trask may carry: object `item`, worn in `slots`, usable on
/// oneself or not.
fn trask_owns(item: u32, name: &[u8], slots: u32, usable: bool) -> crate::vm::Owned {
    crate::vm::Owned {
        item,
        slots,
        usable,
        entry: crate::vm::Entry {
            name: name.to_vec(),
            ..crate::vm::Entry::default()
        },
        ..crate::vm::Owned::default()
    }
}

/// The equip page dressing another party member (I30 P14): the member the
/// bar's portraits bring up (`viewed`) is dressed when the bar marks it
/// (`MemberView::dresses`, the host's companions nobody plays), and Equip
/// names it; one another seat plays is shown, its Equip greyed.
pub(super) fn equip_dresses_members() -> Vec<Check> {
    use crate::intent::GameAction;
    let robe = trask_owns(31, b"Robe", 1 << 1, false);
    let raised = |dresses: bool| {
        on_trask(
            "equip",
            &["BTN_INV_BODY", "BTN_EQUIP"],
            robe.clone(),
            dresses,
            &["BTN_INV_BODY", "LB_ITEMS", "BTN_EQUIP"],
            "BTN_EQUIP",
        )
    };
    let wanted = Intent::Game(GameAction::Equip {
        who: 7,
        item: 31,
        slot: 1,
    });
    let (dressed, _) = raised(true);
    let (played, greyed) = raised(false);
    vec![check("equip_dresses_members", {
        expect(&format!("raised {dressed:?}"), dressed.contains(&wanted))
            .and_then(|()| {
                expect(
                    &format!("one another seat plays: Equip greyed ({greyed}), {played:?}"),
                    greyed && !played.contains(&wanted),
                )
            })
            .map(|()| "the host equips a companion nobody plays, not one played".into())
    })]
}

/// A body slot's list as both games' equip screens fill it
/// (`CSWGuiInGameEquip`): "None" and what is worn, marked equipped, first;
/// an item for another race (a droid's plating on Trask) left out, one he
/// may not wear (armour without its proficiency) listed in red with why
/// over its description, one he may wear as any row.
pub(super) fn equip_lists_as_kotor() -> Vec<Check> {
    use crate::vm::Owned;
    let body = 1 << 1;
    let robe = trask_owns(31, b"Robe", body, false);
    let plating = Owned {
        unfit: true,
        ..trask_owns(32, b"Plating", body, false)
    };
    let armour = Owned {
        refused: Some(b"Feat Required: Armor Proficiency: Heavy".to_vec()),
        ..trask_owns(33, b"Armour", body, false)
    };
    let (mut assets, mut vm) = trask_page(
        "equip",
        &["BTN_INV_BODY", "LB_DESC"],
        vec![armour, plating, robe],
        true,
    );
    if let Some(me) = vm.character.as_mut() {
        me.worn = vec![(1, trask_owns(30, b"Jacket", body, false))];
    }
    let mut stack = opened("equip", &mut assets, &vm);
    press(&mut stack, &mut assets, &vm, "BTN_INV_BODY");
    // Over to the list (on its first row), down to the armour's (after
    // "None" and the jacket).
    for _ in 0..4 {
        if stack.focused() == Some("LB_ITEMS") {
            break;
        }
        stack.step(&mut assets, &vm, &keys(&[Key::Down]));
    }
    for _ in 0..2 {
        stack.step(&mut assets, &vm, &keys(&[Key::Down]));
    }
    let rows: Vec<(String, bool)> = stack
        .state_of("LB_ITEMS", &vm)
        .and_then(|state| state.list)
        .map_or(Vec::new(), |list| {
            list.rows
                .iter()
                .map(|row| (String::from_utf8_lossy(&row.text).into_owned(), row.refused))
                .collect()
        });
    let wanted = [
        ("None", false),
        ("Jacket\n(Equipped)", false),
        ("Armour", true),
        ("Robe", false),
    ]
    .map(|(name, red)| (name.to_string(), red));
    // The armour's description: its refusal first, in red.
    let told = stack
        .state_of("LB_DESC", &vm)
        .and_then(|state| state.list)
        .and_then(|list| list.rows.first().cloned())
        .map(|row| (String::from_utf8_lossy(&row.text).into_owned(), row.refused));
    vec![check("equip_lists_as_kotor", {
        expect(&format!("rows {rows:?}"), rows == wanted)
            .and_then(|()| {
                expect(
                    &format!("description's first row {told:?}"),
                    told.as_ref().is_some_and(|(text, red)| {
                        *red && text.starts_with("Feat Required: Armor Proficiency: Heavy")
                    }),
                )
            })
            .map(|()| {
                "the worn item marked, another race's left out, a refused one in red, why told"
                    .into()
            })
    })]
}

/// An off hand's list (K1's pairing, `Owned::unpaired`): a weapon that does
/// not go with the main hand's is red, and its description says why, so
/// it is not taken for a proficiency missing (Discord, 2026-11-08: a
/// soldier's short sword red in the left hand, the right hand empty).
pub(super) fn equip_tells_off_hand() -> Vec<Check> {
    use crate::vm::Owned;
    let hands = 1 << 4 | 1 << 5;
    let sword = Owned {
        unpaired: [true, false],
        ..trask_owns(40, b"Short Sword", hands, false)
    };
    // The left hand's list and its description's first row, with `right`
    // worn in the right hand.
    let shown = |right: Option<Owned>| {
        let (mut assets, mut vm) = trask_page(
            "equip",
            &["BTN_INV_WEAP_L", "LB_DESC"],
            vec![sword.clone()],
            true,
        );
        if let Some(me) = vm.character.as_mut() {
            me.worn = right.into_iter().map(|owned| (4, owned)).collect();
        }
        let mut stack = opened("equip", &mut assets, &vm);
        press(&mut stack, &mut assets, &vm, "BTN_INV_WEAP_L");
        let red = stack
            .state_of("LB_ITEMS", &vm)
            .and_then(|state| state.list)
            .is_some_and(|list| list.rows.iter().any(|row| row.refused));
        let told = stack
            .state_of("LB_DESC", &vm)
            .and_then(|state| state.list)
            .and_then(|list| list.rows.first().cloned())
            .filter(|row| row.refused)
            .map(|row| String::from_utf8_lossy(&row.text).into_owned());
        (red, told)
    };
    let rifle = trask_owns(41, b"Blaster Rifle", 1 << 4, false);
    let cases = [
        (None, "Off hand: equip a weapon in the right hand first"),
        (
            Some(rifle),
            "Off hand: only a one-handed weapon of the right hand's kind",
        ),
    ];
    vec![check("equip_tells_off_hand", {
        cases
            .into_iter()
            .try_for_each(|(right, wanted)| {
                let (red, told) = shown(right);
                expect(
                    &format!("red {red}, told {told:?}, wanted {wanted:?}"),
                    red && told.is_some_and(|told| told.starts_with(wanted)),
                )
            })
            .map(|()| "an off hand's unpaired weapon red, and why told for both kinds".into())
    })]
}

/// The head slot's eye: kjk's own button in its top right corner, shown
/// only while something is worn there; open while the headgear shows,
/// struck through while hidden, and a press flips the member's setting.
pub(super) fn equip_headgear_eye() -> Vec<Check> {
    const SETTING: &str = "headgear.hidden.seat0";
    const EYE: &str = "BTN_HEADGEAR";
    let (mut assets, mut vm) = trask_page("equip", &["BTN_INV_HEAD"], Vec::new(), true);
    let mask = trask_owns(40, b"Mask", 1, false);
    if let Some(me) = vm.character.as_mut() {
        me.headgear_setting = Some(SETTING.into());
        me.worn = vec![(0, mask)];
    }
    let mut stack = opened("equip", &mut assets, &vm);
    let drawn = |stack: &Stack, vm: &ViewModel| {
        stack
            .state_of(EYE, vm)
            .and_then(|state| (!state.hidden).then_some(state.fill?))
    };
    let mut seen = Vec::new();
    let mut told = Vec::new();
    for hidden in [false, true] {
        vm.settings.insert(SETTING.into(), hidden.to_string());
        seen.push(drawn(&stack, &vm));
        told.push(press(&mut stack, &mut assets, &vm, EYE));
    }
    let flips = told.iter().zip([true, false]).all(|(raised, wanted)| {
        raised.contains(&Intent::Setting(SETTING.into(), wanted.to_string()))
    });
    let corner = (stack.extent_of("BTN_INV_HEAD"))
        .zip(stack.extent_of(EYE))
        .is_some_and(|(slot, eye)| {
            eye.left + eye.width == slot.left + slot.width
                && eye.top == slot.top
                && eye.width < slot.width
        });
    if let Some(me) = vm.character.as_mut() {
        me.worn.clear();
    }
    let bare = drawn(&stack, &vm);
    let wanted = [Some("kjkeye_shown".into()), Some("kjkeye_hidden".into())];
    vec![check("equip_headgear_eye", {
        expect(&format!("icons {seen:?}"), seen == wanted)
            .and_then(|()| expect(&format!("presses raised {told:?}"), flips))
            .and_then(|()| expect("the eye in the head slot's top right corner", corner))
            .and_then(|()| expect(&format!("with nothing worn {bare:?}"), bare.is_none()))
            .map(|()| "the eye shows over worn headgear and flips its setting".into())
    })]
}

/// The character page's Stance: kjk's own button beside Close, as wide as
/// the member's view reaches; with Movie Duels' stances offered and on,
/// it shows the member's pick and a press steps it on (its row's, then
/// each skeleton, back to its row's); without them, greyed, a press doing
/// nothing; hidden for a member the choice cannot be kept for.
pub(super) fn character_stance() -> Vec<Check> {
    use crate::vm::CharacterView;
    const SETTING: &str = "saber.stance.seat0";
    const STANCE: &str = "BTN_STANCE";
    let mut assets = Fake::new();
    assets.layouts.insert("top", bar());
    let controls = vec![
        button("BTN_EXIT", [75, 516, 120, 28], b"Close"),
        button("LBL_3DCHAR", [84, 134, 310, 360], b""),
    ];
    assets.layouts.insert("character", root(800, 600, controls));
    let mut vm = ViewModel {
        character: Some(CharacterView {
            stance_setting: Some(SETTING.into()),
            ..CharacterView::default()
        }),
        movie_duels: true,
        stances: [
            ("_humanoid_kotor", "KOTOR Jedi"),
            ("_humanoid_obi", "Obi-Wan"),
        ]
        .map(|(folder, name)| (folder.to_string(), name.to_string()))
        .to_vec(),
        settings: [("game.duel_stances", "true")]
            .map(|(key, value)| (key.to_string(), value.to_string()))
            .into(),
        ..ViewModel::default()
    };
    let mut stack = opened("character", &mut assets, &vm);
    let shown = |stack: &Stack, vm: &ViewModel| {
        stack.state_of(STANCE, vm).map(|state| {
            let words = String::from_utf8_lossy(&state.text.unwrap_or(Vec::new())).into_owned();
            (words, state.greyed, state.hidden)
        })
    };
    let mut steps = Vec::new();
    for set in ["", "_humanoid_kotor", "_humanoid_obi"] {
        vm.settings.insert(SETTING.into(), set.into());
        steps.push((
            shown(&stack, &vm),
            press(&mut stack, &mut assets, &vm, STANCE),
        ));
    }
    let picked = |words: &str, next: &str| {
        (
            Some((format!("Stance: {words}"), false, false)),
            vec![Intent::Setting(SETTING.into(), next.into())],
        )
    };
    let wanted = vec![
        picked("Its Own", "_humanoid_kotor"),
        picked("KOTOR Jedi", "_humanoid_obi"),
        picked("Obi-Wan", ""),
    ];
    vm.settings
        .insert("game.duel_stances".into(), "false".into());
    let off = (
        shown(&stack, &vm),
        press(&mut stack, &mut assets, &vm, STANCE),
    );
    if let Some(me) = vm.character.as_mut() {
        me.stance_setting = None;
    }
    let unkept = shown(&stack, &vm).is_some_and(|(_, _, hidden)| hidden);
    let beside = (stack.extent_of("BTN_EXIT"))
        .zip(stack.extent_of(STANCE))
        .is_some_and(|(close, stance)| {
            stance.top == close.top
                && stance.left > close.left + close.width
                && stance.left + stance.width == 84 + 310
        });
    vec![check("character_stance", {
        expect(
            &format!("picks shown and stepped {steps:?}"),
            steps == wanted,
        )
        .and_then(|()| {
            expect(
                &format!("off: greyed, a press doing nothing {off:?}"),
                off == (Some(("Stance: Its Own".into(), true, false)), vec![]),
            )
        })
        .and_then(|()| expect("hidden for a member not kept", unkept))
        .and_then(|()| expect("beside Close, to the member's view's edge", beside))
        .map(|()| "a member's stance shown and stepped, greyed without Movie Duels' stances".into())
    })]
}

/// A body's locked slots on the equip page (TSL's one-handed Kreia, her off
/// hands): their buttons in red, the main hand's not; one chosen opens no
/// list but TSL's "You cannot equip or remove an item from this slot.", so
/// Equip wears nothing there.
pub(super) fn equip_locks_slots() -> Vec<Check> {
    let blade = trask_owns(34, b"Blade", 1 << 4 | 1 << 5, false);
    let (mut assets, mut vm) = trask_page(
        "equip",
        &[
            "BTN_INV_WEAP_L",
            "BTN_INV_WEAP_R",
            "BTN_INV_WEAP_L2",
            "BTN_EQUIP",
        ],
        vec![blade],
        true,
    );
    assets.layouts.insert("confirm", confirm());
    if let Some(me) = vm.character.as_mut() {
        me.locked = 1 << 5;
    }
    let mut stack = opened("equip", &mut assets, &vm);
    let red = ["BTN_INV_WEAP_L", "BTN_INV_WEAP_L2", "BTN_INV_WEAP_R"]
        .map(|tag| stack.state_of(tag, &vm).is_some_and(|state| state.refused));
    let raised = press(&mut stack, &mut assets, &vm, "BTN_INV_WEAP_L");
    let told = stack.top().map(|screen| screen.0);
    vec![check("equip_locks_slots", {
        expect(&format!("red {red:?}"), red == [true, true, false])
            .and_then(|()| {
                expect(
                    &format!("chosen: on top {told:?}, raised {raised:?}"),
                    told == Some("message")
                        && !raised
                            .iter()
                            .any(|intent| matches!(intent, Intent::Game(_))),
                )
            })
            .map(|()| "both off hands red and refused with TSL's line, the main hand open".into())
    })]
}

/// TSL's second weapon set on the equip page, in both games: its right
/// slot lists what the first right hand takes and Equip wears it there
/// (slot 18); it shows what is worn there; the swap button exchanges the
/// sets of the member shown, greyed for one another seat plays.
pub(super) fn equip_swaps_sets() -> Vec<Check> {
    use crate::intent::GameAction;
    use crate::vm::CharacterView;
    let blaster = trask_owns(33, b"Blaster", 1 << 4, false);
    let raised = |dresses: bool| {
        on_trask(
            "equip",
            &["BTN_INV_WEAP_R2", "BTN_EQUIP", "BTN_SWAPWEAPONS"],
            blaster.clone(),
            dresses,
            &[
                "BTN_INV_WEAP_R2",
                "LB_ITEMS",
                "BTN_EQUIP",
                "BTN_SWAPWEAPONS",
            ],
            "BTN_SWAPWEAPONS",
        )
    };
    let equip = Intent::Game(GameAction::Equip {
        who: 7,
        item: 33,
        slot: 18,
    });
    let swap = Intent::Game(GameAction::SwitchWeapons { who: 7 });
    let (dressed, lit) = raised(true);
    let (played, greyed) = raised(false);
    let mut assets = Fake::new();
    assets.layouts.insert("top", bar());
    let slot = |tag: &str| Control {
        kind: Kind::Label,
        ..button(tag, [0, 0, 10, 10], b"")
    };
    assets.layouts.insert(
        "equip",
        root(
            640,
            480,
            vec![slot("LBL_INV_WEAP_R2"), slot("LBL_INV_WEAP_L2")],
        ),
    );
    let icon = |icon: &str| crate::vm::Entry {
        icon: Some(icon.into()),
        ..crate::vm::Entry::default()
    };
    let vm = ViewModel {
        character: Some(CharacterView {
            worn: vec![(
                18,
                crate::vm::Owned {
                    entry: icon("iw_blstrpstl_001"),
                    ..crate::vm::Owned::default()
                },
            )],
            ..CharacterView::default()
        }),
        ..ViewModel::default()
    };
    let page = opened("equip", &mut assets, &vm);
    let shown = |tag: &str| {
        page.state_of(tag, &vm)
            .filter(|state| !state.hidden)
            .and_then(|state| state.fill)
    };
    let (right, left) = (shown("LBL_INV_WEAP_R2"), shown("LBL_INV_WEAP_L2"));
    vec![check("equip_swaps_sets", {
        expect(
            &format!("raised {dressed:?}, swap greyed {lit}"),
            dressed.contains(&equip) && dressed.contains(&swap) && !lit,
        )
        .and_then(|()| {
            expect(
                &format!("one another seat plays: swap greyed ({greyed}), {played:?}"),
                greyed && !played.contains(&swap),
            )
        })
        .and_then(|()| {
            expect(
                &format!("the second set shown: right {right:?}, left {left:?}"),
                right.as_deref() == Some("iw_blstrpstl_001") && left.as_deref() == Some("iweap_l"),
            )
        })
        .map(|()| "slot 18 listed and worn, the sets swapped for the member shown".into())
    })]
}

/// The inventory's Use Item (the owner's bug of 2026-10-04): the chosen
/// item used on the member shown when it may use it on itself
/// (`Owned::usable`) and this player dresses that member; greyed for an
/// item it may not use (a grenade), and for a member another seat plays.
pub(super) fn inventory_uses_items() -> Vec<Check> {
    use crate::intent::GameAction;
    let raised = |usable: bool, dresses: bool| {
        on_trask(
            "inventory",
            &["BTN_USEITEM"],
            trask_owns(32, b"Medpac", 0, usable),
            dresses,
            &["LB_ITEMS", "BTN_USEITEM"],
            "BTN_USEITEM",
        )
    };
    let wanted = Intent::Game(GameAction::UseItem { who: 7, item: 32 });
    let (used, lit) = raised(true, true);
    let (grenade, unusable) = raised(false, true);
    let (played, not_his) = raised(true, false);
    vec![check("inventory_uses_items", {
        expect(
            &format!("raised {used:?}, greyed {lit}"),
            used.contains(&wanted) && !lit,
        )
        .and_then(|()| {
            expect(
                &format!("one it may not use: greyed ({unusable}), {grenade:?}"),
                unusable && !grenade.contains(&wanted),
            )
        })
        .and_then(|()| {
            expect(
                &format!("one another seat plays: greyed ({not_his}), {played:?}"),
                not_his && !played.contains(&wanted),
            )
        })
        .map(|()| "Use Item uses a medpac on the member shown; greyed for a grenade and for a member another plays".into())
    })]
}
