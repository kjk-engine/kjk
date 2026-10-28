//! Party selection over a layout holding KOTOR's tags: the table's
//! portraits, what may be picked, and what Done sends.

use super::chargen::presses;
use super::menus::root;
use super::paint::{Fake, button};
use super::vm_screens::{disabled, opened, words};
use super::{Check, check, expect};
use crate::intent::{GameAction, Intent};
use crate::vm::{CompanionView, PartySelectionView, SceneView, ViewModel};

/// Three slots: Trask in the first (forced in by the script), none in
/// the second, Carth in the third, in his clothes.
fn table(open: bool) -> ViewModel {
    let companion = |portrait: &str, name: &str, armour: &str| CompanionView {
        portrait: Some(portrait.into()),
        appearance: Some(6),
        armour: Some(armour.into()),
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
            members: Vec::new(),
            forced: vec![0],
            open,
        }),
        ..ViewModel::default()
    }
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

    let vm = table(true);
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
    let forced = (
        words(&stack, &vm, "LBL_NPC_NAME"),
        disabled(&stack, &vm, "BTN_ACCEPT"),
    );
    presses(&mut stack, &mut assets, &vm, &["BTN_NPC2"]);
    let figure = stack.state_of("LBL_3D", &vm).and_then(|state| state.scene);
    presses(&mut stack, &mut assets, &vm, &["BTN_ACCEPT"]);
    let full = words(&stack, &vm, "LBL_COUNT");
    let done = (
        presses(&mut stack, &mut assets, &vm, &["BTN_DONE"]),
        stack.is_open(),
    );

    let looking = table(false);
    let mut shows = opened("party", &mut assets, &looking);
    presses(&mut shows, &mut assets, &looking, &["BTN_NPC2"]);
    let greyed = disabled(&shows, &looking, "BTN_ACCEPT");
    let closed = (
        presses(&mut shows, &mut assets, &looking, &["BTN_DONE"]),
        shows.is_open(),
    );

    vec![check("party_selection", {
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
                &format!("Trask named, forced in: {forced:?}"),
                forced == ("Trask".into(), true),
            )
        })
        .and_then(|()| {
            expect(
                &format!("Carth shown dressed in what he wears: {figure:?}"),
                figure
                    == Some(SceneView::Dressed {
                        appearance: 6,
                        armour: Some("g_a_clothes01".into()),
                    }),
            )
        })
        .and_then(|()| expect(&format!("Carth added: {full:?}"), full == "0"))
        .and_then(|()| {
            expect(
                &format!("Done sends both and closes: {done:?}"),
                done == (
                    vec![Intent::Game(GameAction::ChooseParty(Some([0, 2])))],
                    false,
                ),
            )
        })
        .and_then(|()| {
            expect(
                &format!("in play it only shows: Add {greyed}, {closed:?}"),
                greyed && closed == (Vec::new(), false),
            )
        })
        .map(|()| "portraits, picks, the one chosen dressed, Done".into())
    })]
}
