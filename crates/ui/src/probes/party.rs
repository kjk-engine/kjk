//! Party selection over a layout holding KOTOR's tags: the table's
//! portraits, what may be picked, and what Done sends.

use super::chargen::presses;
use super::menus::{keys, root};
use super::paint::{Fake, button};
use super::vm_screens::{disabled, opened, words};
use super::{Check, check, expect};
use crate::intent::{GameAction, Intent};
use crate::screen::Screen;
use crate::vm::{CompanionView, PartyChange, PartySelectionView, SceneView, ViewModel};

/// Three slots: Trask in the first, none in the second, Carth in the
/// third, in his clothes. A script's screen (`open`) forces Trask in, and
/// opens with him alone picked; the one opened in play opens with Carth,
/// in the party, picked, and may change the party as `change` says.
fn table(open: bool, change: PartyChange) -> ViewModel {
    let companion = |portrait: &str, name: &str, armour: &str| CompanionView {
        portrait: Some(portrait.into()),
        appearance: Some(6),
        armour: Some(armour.into()),
        good_evil: None,
        name: name.as_bytes().to_vec(),
        selectable: true,
    };
    ViewModel {
        party_selection: Some(PartySelectionView {
            slots: 9,
            companions: vec![
                Some(companion("po_ptrask", "Trask", "g_a_class4001")),
                None,
                Some(companion("po_pcarth", "Carth", "g_a_clothes01")),
            ],
            opening: if open { vec![0] } else { vec![2] },
            forced: if open { vec![0] } else { Vec::new() },
            open,
            cancel: !open,
            change,
        }),
        ..ViewModel::default()
    }
}

/// The talk-table lines of the screen's message boxes (`screens::party`):
/// one forced in must stay, Back refused, and KOTOR 2's "You are unable to
/// change your party at this time."
const MUST_STAY: u32 = 42406;
const CANNOT_CANCEL: u32 = 42405;
const UNABLE: u32 = 108_426;

/// `tags` pressed in turn on the party screen alone, over `vm`: what it
/// raised (a message box it asks for among them).
fn told(vm: &ViewModel, tags: &[&str]) -> Vec<Intent> {
    let mut screen = crate::screens::party::Party::default();
    let mut out = Vec::new();
    for tag in tags {
        screen.press(tag, vm, &mut out);
    }
    out
}

/// A message box asked for with talk-table line `strref`.
fn says(strref: u32) -> Vec<Intent> {
    vec![Intent::OpenFor("message_box", strref.to_string())]
}

pub(super) fn all() -> Vec<Check> {
    let mut assets = Fake::new();
    let tags: Vec<String> = ["BTN_NPC", "LBL_CHAR", "LBL_NA"]
        .iter()
        .flat_map(|prefix| (0..3).map(move |slot| format!("{prefix}{slot}")))
        .chain(
            [
                "LBL_COUNT",
                "LBL_NPC_NAME",
                "LBL_3D",
                "BTN_ACCEPT",
                "BTN_DONE",
                "BTN_BACK",
            ]
            .map(String::from),
        )
        .collect();
    let controls = tags
        .iter()
        .enumerate()
        .map(|(at, tag)| button(tag, [20, 10 + at as i32 * 12, 100, 10], b""))
        .collect();
    assets
        .layouts
        .insert("partyselection", root(640, 480, controls));

    let vm = table(true, PartyChange::May);
    let mut stack = opened("party", &mut assets, &vm);
    let shown = |stack: &crate::screen::Stack, tag: &str| {
        stack
            .state_of(tag, &vm)
            .map(|state| (state.fill, state.hidden, state.disabled))
    };
    let slots = [
        shown(&stack, "LBL_CHAR0"),
        shown(&stack, "LBL_NA0"),
        shown(&stack, "LBL_NA1"),
        shown(&stack, "BTN_NPC1"),
    ];
    let fresh = words(&stack, &vm, "LBL_COUNT");
    presses(&mut stack, &mut assets, &vm, &["BTN_NPC0"]);
    let name = words(&stack, &vm, "LBL_NPC_NAME");
    let forced = told(&vm, &["BTN_NPC0", "BTN_ACCEPT"]);
    // Back on a screen whose script allows no backing out.
    let back = told(&vm, &["BTN_BACK"]);
    presses(&mut stack, &mut assets, &vm, &["BTN_NPC2"]);
    let figure = stack.state_of("LBL_3D", &vm).and_then(|state| state.scene);
    presses(&mut stack, &mut assets, &vm, &["BTN_ACCEPT"]);
    let full = words(&stack, &vm, "LBL_COUNT");
    let done = (
        presses(&mut stack, &mut assets, &vm, &["BTN_DONE"]),
        stack.is_open(),
    );

    // A pad: the slot its focus is on, X (a double click) adds it.
    let mut pad = opened("party", &mut assets, &vm);
    presses(&mut pad, &mut assets, &vm, &["BTN_NPC2"]);
    pad.step(&mut assets, &vm, &keys(&[crate::input::Key::Primary]));
    let pad_added = words(&pad, &vm, "LBL_COUNT");

    // Opened in play, Carth chosen, Add or Remove pressed, then Done.
    let in_play = |change: PartyChange, assets: &mut Fake| {
        let vm = table(false, change);
        let mut stack = opened("party", assets, &vm);
        presses(&mut stack, assets, &vm, &["BTN_NPC2"]);
        let greyed = disabled(&stack, &vm, "BTN_ACCEPT");
        (greyed, told(&vm, &["BTN_NPC2", "BTN_ACCEPT", "BTN_DONE"]))
    };
    let looking = in_play(PartyChange::LookOnly, &mut assets);
    let changing = in_play(PartyChange::May, &mut assets);
    let refused = in_play(PartyChange::Refused(UNABLE), &mut assets);
    let closed = |picks: [u8; 2]| vec![Intent::Game(GameAction::ChooseParty(Some(picks)))];
    let then_closed = |mut said: Vec<Intent>| {
        said.push(Intent::Close);
        said
    };

    vec![
        check("party_selection", {
            expect(
                &format!("Trask's portrait, Carth's slot, none in the second: {slots:?}"),
                slots
                    == [
                        Some((Some("po_ptrask".into()), false, false)),
                        Some((None, true, false)),
                        Some((None, false, false)),
                        Some((None, false, true)),
                    ],
            )
            .and_then(|()| expect(&format!("one more may come: {fresh:?}"), fresh == "1"))
            .and_then(|()| {
                expect(
                    &format!(
                        "Trask named, forced in, Remove says he must stay: {name:?} {forced:?}"
                    ),
                    name == "Trask" && forced == says(MUST_STAY),
                )
            })
            .and_then(|()| {
                expect(
                    &format!("Back says the party must be accepted: {back:?}"),
                    back == says(CANNOT_CANCEL),
                )
            })
            .and_then(|()| {
                expect(
                    &format!("Carth shown dressed in what he wears: {figure:?}"),
                    figure
                        == Some(SceneView::Dressed {
                            appearance: 6,
                            armour: Some("g_a_clothes01".into()),
                            good_evil: None,
                        }),
                )
            })
            .and_then(|()| expect(&format!("Carth added: {full:?}"), full == "0"))
            .and_then(|()| {
                expect(
                    &format!("a pad's X adds the slot it is on: {pad_added:?}"),
                    pad_added == "0",
                )
            })
            .and_then(|()| {
                expect(
                    &format!("Done sends both and closes: {done:?}"),
                    done == (closed([0, 2]), false),
                )
            })
            .and_then(|()| {
                expect(
                    &format!("in play in KOTOR 1 it only shows: {looking:?}"),
                    looking == (true, then_closed(Vec::new())),
                )
            })
            .and_then(|()| {
                expect(
                    &format!("in play in KOTOR 2 Carth is removed: {changing:?}"),
                    changing == (false, then_closed(closed([u8::MAX; 2]))),
                )
            })
            .and_then(|()| {
                expect(
                    &format!("in play where KOTOR 2 refuses, it says why: {refused:?}"),
                    refused == (false, then_closed(says(UNABLE))),
                )
            })
            .map(|()| "portraits, picks, the one chosen dressed, Done, Back, in play".into())
        }),
        check("party_stand_ins", stand_ins()),
    ]
}

/// KOTOR 2's stand-in slots: with Hanharr (10) available and the Disciple
/// (11) not, Hanharr's slot shows in place of Mira's (7), and the
/// Handmaiden's (4) shows, not the Disciple's.
fn stand_ins() -> Result<String, String> {
    let tags: Vec<String> = ["BTN_NPC", "LBL_CHAR", "LBL_NA"]
        .iter()
        .flat_map(|prefix| [4, 7, 10, 11].map(|slot| format!("{prefix}{slot}")))
        .collect();
    let mut assets = Fake::new();
    let controls = tags
        .iter()
        .enumerate()
        .map(|(at, tag)| button(tag, [20, 10 + at as i32 * 12, 100, 10], b""))
        .collect();
    assets
        .layouts
        .insert("partyselection", root(640, 480, controls));
    let mut companions = vec![None; 12];
    companions[10] = Some(CompanionView {
        portrait: Some("po_phanharr".into()),
        appearance: None,
        armour: None,
        good_evil: None,
        name: b"Hanharr".to_vec(),
        selectable: true,
    });
    let vm = ViewModel {
        party_selection: Some(PartySelectionView {
            slots: 12,
            companions,
            opening: Vec::new(),
            forced: Vec::new(),
            open: true,
            cancel: false,
            change: PartyChange::May,
        }),
        ..ViewModel::default()
    };
    let stack = opened("party", &mut assets, &vm);
    let hidden: Vec<&String> = tags
        .iter()
        .filter(|tag| stack.state_of(tag, &vm).is_some_and(|state| state.hidden))
        .collect();
    // LBL_NA10 is hidden too: Hanharr is available.
    let want = [
        "BTN_NPC7",
        "BTN_NPC11",
        "LBL_CHAR7",
        "LBL_CHAR11",
        "LBL_NA7",
        "LBL_NA10",
        "LBL_NA11",
    ];
    expect(&format!("hidden: {hidden:?}"), hidden == want)?;
    Ok("Hanharr's slot in Mira's place, the Handmaiden's shown".into())
}
