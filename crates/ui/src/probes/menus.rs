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
                linked("BTN_KEYMAP", 12, 300, -1, -1),
                linked("BTN_BACK", 10, 400, -1, -1),
            ],
        ),
    );
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
            ],
        )
    });
    assets.layouts.insert(
        "optkeyentry",
        Gui {
            extent: Rect {
                left: 150,
                top: 150,
                width: 340,
                height: 180,
            },
            ..root(
                640,
                480,
                vec![
                    Control {
                        kind: Kind::Label,
                        ..button("LBL_EventName", [10, 10, 300, 20], b"")
                    },
                    linked("BTN_Cancel", 1, 140, -1, -1),
                ],
            )
        },
    );
    let vm = ViewModel {
        settings: [
            ("mouse.sensitivity".to_string(), "0.5".to_string()),
            ("camera.invert".to_string(), "false".to_string()),
        ]
        .into_iter()
        .collect(),
        keys: vec![crate::vm::KeyRow {
            action: "forward".into(),
            label: "Forward".into(),
            key: "KeyW".into(),
            movement: true,
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
        .map(|()| "sliders, check boxes, greyed, keys".into())
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
            owned: vec![owned],
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
