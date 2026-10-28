//! Pazaak's screens for a player who watches another's match (R9a): the
//! table read-only (the player's hand shown, nothing to play), "Watching
//! Carth" over it (shortened to fit) and Leave on TSL's Forfeit button,
//! a wager or a side deck being chosen told in words, as is a pause and
//! why (R39), and no move sent whatever is pressed, on any of the three
//! screens.

use fmt_odyssey::gui::Kind;

use super::menus::{keys, root};
use super::paint::{Fake, button};
use super::vm_screens::{disabled, opened, words};
use super::{Check, check, expect};
use crate::input::{Key, UiInput};
use crate::intent::{GameAction, Intent, PazaakMove};
use crate::paint::{Painter, Palette};
use crate::screen::{Screen, Stack};
use crate::screens::pazaak::Game;
use crate::vm::{
    PazaakCard, PazaakHeld, PazaakOffer, PazaakSide, PazaakStage, PazaakView, PazaakWatch,
    ViewModel,
};

/// Each screen, its layout, the stage it shows and the tags it acts on or
/// shows the table with.
fn screens() -> [(&'static str, &'static str, PazaakStage, Vec<String>); 3] {
    let numbered = |prefixes: &[&str], count: usize| -> Vec<String> {
        prefixes
            .iter()
            .flat_map(|prefix| (0..count).map(move |nth| format!("{prefix}{nth}")))
            .collect()
    };
    let named = |tags: &[&str]| tags.iter().map(|tag| tag.to_string()).collect::<Vec<_>>();
    let game = [
        numbered(&["BTN_PLRSIDE", "LBL_PLRSIDE", "BTN_FLIP", "BTN_CHANGE"], 4),
        named(&[
            "BTN_XTEXT",
            "BTN_YTEXT",
            "BTN_FORFEITGAME",
            "LBL_PLRNAME",
            "LBL_PLRSIDEDECK",
        ]),
    ]
    .concat();
    let setup = [
        named(&[
            "BTN_AVAIL00",
            "BTN_AVAIL10",
            "BTN_ATEXT",
            "BTN_YTEXT",
            "BTN_CLEARCARDS",
        ]),
        numbered(&["BTN_CHOSEN"], 10),
    ]
    .concat();
    [
        (
            "pazaak_wager",
            "pazaakwager",
            PazaakStage::Wager,
            named(&["BTN_LESS", "BTN_MORE", "BTN_WAGER", "BTN_QUIT"]),
        ),
        ("pazaak_setup", "pazaaksetup", PazaakStage::Setup, setup),
        ("pazaak_game", "pazaakgame", PazaakStage::Game, game),
    ]
}

fn layouts(assets: &mut Fake) {
    for (_, layout, _, tags) in screens() {
        let controls = (tags.iter().enumerate())
            .map(|(at, tag)| button(tag, [400, 10 + at as i32 * 12, 100, 10], b""))
            .collect();
        assets.layouts.insert(layout, root(640, 480, controls));
    }
}

/// A ± card: its sign and TSL's ±1/2 size the player's to choose.
fn card() -> PazaakCard {
    PazaakCard {
        face: "+1".into(),
        art: "lbl_cardrarem".into(),
        signed: Some(["+1", "-1", "+2", "-2"].map(String::from)),
        minus_art: "lbl_cardraref".into(),
        sized: true,
    }
}

/// Watching `name`'s table, its player at `stage`.
fn watch(name: &str, stage: PazaakStage) -> PazaakWatch {
    PazaakWatch {
        name: name.as_bytes().to_vec(),
        leave_key: b"L".to_vec(),
        stage,
        held: None,
    }
}

/// The table at `stage` on the player's turn, everything offered: the
/// player's or, `watching`, another player's (Carth's).
fn table(stage: PazaakStage, watching: bool) -> ViewModel {
    seen(stage, watching.then(|| watch("Carth", stage)))
}

/// The table at `stage` as `watching` says.
fn seen(stage: PazaakStage, watching: Option<PazaakWatch>) -> ViewModel {
    let hand = std::array::from_fn(|_| Some(card()));
    ViewModel {
        pazaak: Some(PazaakView {
            stage,
            watching,
            most: 50,
            offers: (1..=12)
                .map(|card| PazaakOffer {
                    card,
                    count: 2,
                    look: PazaakCard {
                        face: card.to_string(),
                        art: "lbl_cardmpos".into(),
                        ..PazaakCard::default()
                    },
                })
                .collect(),
            sides: [
                PazaakSide {
                    hand,
                    ..PazaakSide::default()
                },
                PazaakSide::default(),
            ],
            turn: Some(0),
            ..PazaakView::default()
        }),
        ..ViewModel::default()
    }
}

/// What a frame of `input` raised, `pressed` pressed first.
fn frame(
    stack: &mut Stack,
    assets: &mut Fake,
    vm: &ViewModel,
    pressed: Option<&str>,
    input: &UiInput,
) -> Vec<Intent> {
    if let Some(tag) = pressed {
        stack.press(tag);
    }
    stack.step(assets, vm, input).intents
}

/// Everything a player can do on `screen`: each of its tags pressed, the
/// table's keys (1–4, F1–F4, E, S) and the screens' (Left, Right,
/// Activate, Back), a frame each; what they raised.
fn everything(assets: &mut Fake, screen: &str, tags: &[String], vm: &ViewModel) -> Vec<Intent> {
    let mut stack = opened(screen, assets, vm);
    let mut raised = Vec::new();
    for tag in tags {
        raised.extend(frame(&mut stack, assets, vm, Some(tag), &keys(&[])));
    }
    let raw = (1..=4)
        .flat_map(|n| [format!("Digit{n}"), format!("F{n}")])
        .chain(["KeyE", "KeyS"].map(String::from));
    for key in raw {
        let input = UiInput {
            raw: vec![key],
            ..keys(&[])
        };
        raised.extend(frame(&mut stack, assets, vm, None, &input));
    }
    for key in [Key::Left, Key::Right, Key::Activate, Key::Back] {
        raised.extend(frame(&mut stack, assets, vm, None, &keys(&[key])));
    }
    raised
}

fn moves(raised: &[Intent]) -> Vec<&PazaakMove> {
    raised
        .iter()
        .filter_map(|intent| match intent {
            Intent::Game(GameAction::Pazaak(played)) => Some(played),
            _ => None,
        })
        .collect()
}

pub(super) fn all() -> Vec<Check> {
    let mut assets = Fake::new();
    layouts(&mut assets);
    let (watcher, player) = (
        table(PazaakStage::Game, true),
        table(PazaakStage::Game, false),
    );
    let game = |assets: &mut Fake, vm: &ViewModel| opened("pazaak_game", assets, vm);
    let shown = |stack: &Stack, vm: &ViewModel| {
        let lit = |tag: &str| !stack.hidden(tag, vm) && !disabled(stack, vm, tag);
        (
            [
                "BTN_PLRSIDE0",
                "BTN_XTEXT",
                "BTN_YTEXT",
                "BTN_FLIP0",
                "BTN_CHANGE0",
            ]
            .map(lit),
            words(stack, vm, "LBL_PLRSIDE0"),
            (lit("BTN_FORFEITGAME"), words(stack, vm, "BTN_FORFEITGAME")),
            words(stack, vm, "LBL_PLRNAME"),
        )
    };
    let (watching, playing) = (
        shown(&game(&mut assets, &watcher), &watcher),
        shown(&game(&mut assets, &player), &player),
    );
    let read_only = (
        [false; 5],
        "+1".to_string(),
        (true, "Leave (L)".to_string()),
        "Watching Carth".to_string(),
    );
    let live = (
        [true; 5],
        "+1".to_string(),
        (true, String::new()),
        String::new(),
    );
    // Whatever the watcher presses, on any screen: no move; its Leave the
    // only thing raised. A flip changes nothing it sees.
    let watched: Vec<(&str, Vec<Intent>)> = screens()
        .into_iter()
        .map(|(screen, _, stage, tags)| {
            let raised = everything(&mut assets, screen, &tags, &table(stage, true));
            (screen, raised)
        })
        .collect();
    let mut stack = game(&mut assets, &watcher);
    let left = frame(
        &mut stack,
        &mut assets,
        &watcher,
        Some("BTN_FORFEITGAME"),
        &keys(&[]),
    );
    let flip = UiInput {
        raw: vec!["F1".into()],
        ..keys(&[])
    };
    frame(&mut stack, &mut assets, &watcher, None, &flip);
    let face = words(&stack, &watcher, "LBL_PLRSIDE0");
    // The player's own table sends its moves.
    let mut stack = game(&mut assets, &player);
    let own = [
        frame(
            &mut stack,
            &mut assets,
            &player,
            Some("BTN_XTEXT"),
            &keys(&[]),
        ),
        frame(
            &mut stack,
            &mut assets,
            &player,
            Some("BTN_FORFEITGAME"),
            &keys(&[]),
        ),
    ];
    // The title as painted in a box of 16 test-font pixels (a letter's
    // advance 1): "Watching" dropped, then all but the first name.
    let title = |name: &str| {
        let control = fmt_odyssey::gui::Control {
            kind: Kind::Label,
            ..button("LBL_PLRNAME", [0, 0, 16, 20], b"")
        };
        let vm = seen(PazaakStage::Game, Some(watch(name, PazaakStage::Game)));
        let state = Game::default().state(&control, &vm);
        let mut fake = Fake::new();
        let words = Painter::new(&mut fake, Palette::K1).shown_words(&control, &state);
        String::from_utf8_lossy(&words).into_owned()
    };
    let titles = ["Carth", "Taris Citizen", "Canderous Ordo the Mandalorian"].map(title);
    // What the player is doing before the table, told the watcher (and its
    // shorter words); nothing at the table, nor on the player's own.
    let status = |vm: &ViewModel| {
        let state = Game::default().state(&button("LBL_PLRSIDEDECK", [0, 0, 1, 1], b""), vm);
        let shorter: Vec<String> = (state.shorter.iter())
            .map(|words| String::from_utf8_lossy(words).into_owned())
            .collect();
        (
            state
                .text
                .map(|words| String::from_utf8_lossy(&words).into_owned()),
            shorter,
        )
    };
    // A paused match told why, whatever the stage (R39).
    let watched_stage = |stage, held| {
        let watching = PazaakWatch {
            held,
            ..watch("Carth", stage)
        };
        seen(PazaakStage::Game, Some(watching))
    };
    let statuses = [
        status(&watched_stage(PazaakStage::Wager, None)),
        status(&watched_stage(PazaakStage::Setup, None)),
        status(&watched_stage(PazaakStage::Game, None)),
        status(&watched_stage(PazaakStage::Game, Some(PazaakHeld::Away))),
        status(&watched_stage(PazaakStage::Setup, Some(PazaakHeld::Hurt))),
        status(&player),
    ];
    let told = |text: &str, short: &str| (Some(text.to_string()), vec![short.to_string()]);
    let wanted_statuses = [
        told("Carth is placing a wager.", "Placing a wager."),
        told("Carth is choosing a side deck.", "Choosing a side deck."),
        (None, Vec::new()),
        told("Carth is away.", "Away."),
        told("Carth is hurt.", "Hurt."),
        (None, Vec::new()),
    ];
    let sent: Vec<(&str, Vec<&PazaakMove>)> = watched
        .iter()
        .map(|(screen, raised)| (*screen, moves(raised)))
        .collect();
    let others: Vec<&Intent> = watched
        .iter()
        .flat_map(|(_, raised)| raised)
        .filter(|intent| !matches!(intent, Intent::Game(GameAction::StopWatching)))
        .collect();
    vec![
        check(
            "pazaak_watch_table",
            expect(
                &format!(
                    "the watcher's table read-only, titled, Leave drawn {watching:?}; the player's live {playing:?}"
                ),
                watching == read_only && playing == live,
            )
            .map(|()| format!("{watching:?}")),
        ),
        check(
            "pazaak_watch_sends_no_move",
            expect(
                &format!(
                    "moves sent watching {sent:?}, anything else {others:?}; Leave {left:?}, the face flipped {face}; the player's own {own:?}"
                ),
                sent.iter().all(|(_, sent)| sent.is_empty())
                    && others.is_empty()
                    && left == [Intent::Game(GameAction::StopWatching)]
                    && face == "+1"
                    && own
                        == [
                            vec![Intent::Game(GameAction::Pazaak(PazaakMove::EndTurn))],
                            vec![Intent::Game(GameAction::Pazaak(PazaakMove::Forfeit))],
                        ],
            )
            .map(|()| "every tag and key of the three screens: no move; Leave stops watching".into()),
        ),
        check(
            "pazaak_watch_title_fits",
            expect(
                &format!("the titles painted in 16 px: {titles:?}"),
                titles == ["Watching Carth", "Taris Citizen", "Canderous"],
            )
            .map(|()| format!("{titles:?}")),
        ),
        check(
            "pazaak_watch_status",
            expect(
                &format!(
                    "the hand's label watching a wager, a side deck, the table, paused away, paused hurt; the player's: {statuses:?}"
                ),
                statuses == wanted_statuses,
            )
            .map(|()| format!("{:?}", statuses[1])),
        ),
    ]
}
