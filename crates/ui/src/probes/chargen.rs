//! Character creation over layouts holding KOTOR's tags: a quick walk to
//! Play, the point buys and what backing out undoes.

use fmt_odyssey::gui::{Control, Gui, Kind};

use super::menus::{keys, root, typed};
use super::paint::{Fake, button};
use super::{Check, check, expect};
use crate::input::{Key, UiInput};
use crate::intent::{CharacterDesc, Intent, SessionIntent};
use crate::paint::CellLook;
use crate::screen::Stack;
use crate::vm::{ChargenView, ClassView, Entry, PicksView, PortraitView, ViewModel};

/// A layout of buttons, one under another, by tag.
fn buttons(tags: &[&str]) -> Gui {
    root(
        640,
        480,
        tags.iter()
            .enumerate()
            .map(|(at, tag)| button(tag, [20, 10 + at as i32 * 12, 100, 10], b""))
            .collect::<Vec<Control>>(),
    )
}

/// `prefix_<suffix>` for each prefix and suffix.
fn crossed(prefixes: &[&str], suffixes: &[&str]) -> Vec<String> {
    prefixes
        .iter()
        .flat_map(|prefix| {
            suffixes
                .iter()
                .map(move |suffix| format!("{prefix}_{suffix}"))
        })
        .collect()
}

pub(super) fn layouts(assets: &mut Fake) {
    let abilities = crossed(
        &["STR", "DEX", "CON", "WIS", "INT", "CHA"],
        &["POINTS_BTN", "MINUS_BTN", "PLUS_BTN"],
    );
    let mut skills = crossed(
        &["COM", "DEM", "STE", "AWA", "PER", "REP", "SEC", "TRE"],
        &["MINUS_BTN", "PLUS_BTN"],
    );
    skills.extend(crossed(
        &[
            "COMPUTER_USE",
            "DEMOLITIONS",
            "STEALTH",
            "AWARENESS",
            "PERSUADE",
            "REPAIR",
            "SECURITY",
            "TREAT_INJURY",
        ],
        &["POINTS_BTN"],
    ));
    let picker = |extra: Vec<String>| {
        let mut tags: Vec<&str> = extra.iter().map(String::as_str).collect();
        tags.extend([
            "REMAINING_SELECTIONS_LBL",
            "BTN_RECOMMENDED",
            "BTN_ACCEPT",
            "BTN_BACK",
        ]);
        buttons(&tags)
    };
    let steps: Vec<String> = (1..=6).map(|step| format!("BTN_STEPNAME{step}")).collect();
    let steps: Vec<&str> = steps.iter().map(String::as_str).collect();
    for (name, gui) in [
        (
            "classsel",
            buttons(&[
                "BTN_SEL1",
                "BTN_SEL2",
                "BTN_SEL3",
                "BTN_SEL4",
                "BTN_SEL5",
                "BTN_SEL6",
                "LBL_CLASS",
                "BTN_BACK",
            ]),
        ),
        (
            "qorcpnl",
            buttons(&["QUICK_CHAR_BTN", "CUST_CHAR_BTN", "BTN_BACK"]),
        ),
        (
            "quickpnl",
            buttons(&[&steps[..3], &["BTN_BACK", "BTN_CANCEL"][..]].concat()),
        ),
        (
            "custpnl",
            buttons(&[&steps[..], &["BTN_BACK", "BTN_CANCEL"][..]].concat()),
        ),
        (
            "maincg",
            buttons(&[
                "LBL_NAME",
                "LBL_CLASS",
                "STR_AB_LBL",
                "LBL_VIT",
                "NEW_FORT_LBL",
            ]),
        ),
        (
            "portcust",
            buttons(&["BTN_ARRL", "BTN_ARRR", "BTN_ACCEPT", "BTN_BACK"]),
        ),
        ("abchrgen", picker(abilities)),
        ("skchrgen", picker(skills)),
        ("ftchrgen", feats_layout()),
        ("name", buttons(&["NAME_BOX_EDIT", "END_BTN", "BTN_BACK"])),
    ] {
        assets.layouts.insert(name, gui);
    }
}

/// `ftchrgen`'s controls: the chart's list (its rows' tops 20, 68, 116 at
/// the flow's 44 and the list's padding 4), the name over the
/// description, Add, the count left.
fn feats_layout() -> Gui {
    let mut list = button("LB_FEATS", [20, 20, 300, 400], b"");
    list.kind = Kind::ListBox;
    list.padding = 4;
    list.proto = Some(Box::new(button("PROTOITEM", [20, 20, 262, 56], b"")));
    let mut controls = vec![list];
    controls.extend(
        [
            "LBL_NAME",
            "LB_DESC",
            "BTN_SELECT",
            "STD_REMAINING_SELECTIONS_LBL",
            "BTN_RECOMMENDED",
            "BTN_ACCEPT",
            "BTN_BACK",
        ]
        .iter()
        .enumerate()
        .map(|(at, tag)| button(tag, [400, 20 + at as i32 * 30, 200, 24], b"")),
    );
    root(640, 480, controls)
}

/// A soldier's first level: Flurry (11) open, Improved Flurry (91) and
/// Whirlwind (53) after it not; Power Attack (28) known, Improved Power
/// Attack (17) after it; Toughness (84) open; one pick.
fn feat_picks() -> PicksView {
    let feat = |row: u16, name: &str| {
        (
            row,
            Entry {
                name: name.as_bytes().to_vec(),
                icon: Some(format!("feat{row}")),
                description: format!("{name}.").into_bytes(),
                ..Entry::default()
            },
        )
    };
    let flurry = feat(11, "Flurry");
    let toughness = feat(84, "Toughness");
    PicksView {
        feats: vec![flurry.clone(), toughness.clone()],
        feat_count: 1,
        chart: vec![
            [
                Some(flurry),
                Some(feat(91, "Improved Flurry")),
                Some(feat(53, "Whirlwind")),
            ],
            [
                Some(feat(28, "Power Attack")),
                Some(feat(17, "Improved Power Attack")),
                None,
            ],
            [Some(toughness), None, None],
        ],
        known: vec![28],
        ..PicksView::default()
    }
}

/// The pointer moved to `at` (layout pixels of a 640 × 480 layout centred
/// in the probes' 800 × 600 window).
fn pointing(at: [f32; 2]) -> UiInput {
    UiInput {
        pointer: Some([at[0] + 80.0, at[1] + 60.0]),
        moved: true,
        ..keys(&[])
    }
}

const SOLDIER: [u8; 6] = [16, 14, 14, 10, 12, 10];

pub(super) fn chargen_vm() -> ViewModel {
    let class = |name: &str, abilities| ClassView {
        name: name.as_bytes().to_vec(),
        hit_die: 10,
        skill_points: 2,
        abilities,
        saves: [2, 0, 0],
        class_skills: [true, false, false, false, false, false, false, false],
        skill_order: vec![1, 0],
        picks: crate::vm::PicksView::default(),
    };
    let portrait = |texture: &str, female| PortraitView {
        texture: texture.into(),
        female,
    };
    ViewModel {
        chargen: Some(ChargenView {
            offer: &crate::screens::chargen::K1_OFFER,
            classes: vec![
                class("Soldier", SOLDIER),
                class("Scout", [12; 6]),
                class("Scoundrel", [10; 6]),
            ],
            genders: [b"Male".to_vec(), b"Female".to_vec()],
            portraits: vec![
                portrait("po_pmhc1", false),
                portrait("po_pfhc1", true),
                portrait("po_pmhc2", false),
            ],
            skill_text: [244, 246, 248, 250, 252, 254, 256, 258],
            names: None,
            whole_skill_points: false,
        }),
        ..ViewModel::default()
    }
}

/// Presses each tag in turn, one frame each, then a quiet frame (the
/// step's layout loaded); what the frames raised.
pub(super) fn presses(
    stack: &mut Stack,
    assets: &mut Fake,
    vm: &ViewModel,
    tags: &[&str],
) -> Vec<Intent> {
    let mut raised = Vec::new();
    for tag in tags {
        stack.press(tag);
        raised.extend(stack.step(assets, vm, &keys(&[])).intents);
    }
    raised.extend(stack.step(assets, vm, &keys(&[])).intents);
    raised
}

/// From class selection, a male soldier made the quick way, the second
/// portrait, named Revan, played (its skills the recommended ones).
pub(super) fn quick(stack: &mut Stack, assets: &mut Fake, vm: &ViewModel) -> Vec<Intent> {
    presses(
        stack,
        assets,
        vm,
        &[
            "BTN_SEL3",
            "QUICK_CHAR_BTN",
            "BTN_STEPNAME1",
            "BTN_ARRR",
            "BTN_ACCEPT",
            "BTN_STEPNAME2",
        ],
    );
    stack.step(assets, vm, &typed("Revan"));
    presses(stack, assets, vm, &["END_BTN", "BTN_STEPNAME3"])
}

pub(super) fn made() -> Option<CharacterDesc> {
    Some(CharacterDesc {
        class: 0,
        female: false,
        portrait: "po_pmhc2".into(),
        abilities: SOLDIER,
        skills: [0, 2, 0, 0, 0, 0, 0, 0],
        name: "Revan".into(),
        feats: Vec::new(),
    })
}

fn text(stack: &Stack, vm: &ViewModel, tag: &str) -> String {
    String::from_utf8_lossy(&stack.text_of(tag, vm).unwrap_or(Vec::new())).into_owned()
}

/// The feats step as KOTOR's chart: a chain a row, each feat's look; the
/// first feat chosen and named on opening; the pointer choosing the
/// square it is over, the keys moving along a row and between rows; Add
/// picking an open feat and dropping it, refusing one not open;
/// Recommended taking the first offered.
fn feat_chart() -> Check {
    let mut assets = Fake::new();
    layouts(&mut assets);
    let mut vm = chargen_vm();
    if let Some(chargen) = vm.chargen.as_mut() {
        chargen.classes[0].picks = feat_picks();
    }
    let mut stack = Stack::new(crate::screens::open);
    stack.open("chargen:solo", &vm);
    stack.step(&mut assets, &vm, &keys(&[]));
    presses(
        &mut stack,
        &mut assets,
        &vm,
        &[
            "BTN_SEL3",
            "CUST_CHAR_BTN",
            "BTN_STEPNAME1",
            "BTN_ACCEPT",
            "BTN_STEPNAME2",
            "BTN_RECOMMENDED",
            "BTN_ACCEPT",
            "BTN_STEPNAME3",
            "BTN_ACCEPT",
            "BTN_STEPNAME4",
        ],
    );
    let name = |stack: &Stack| text(stack, &vm, "LBL_NAME");
    let looks = |stack: &Stack| -> Vec<Vec<CellLook>> {
        let list = stack.state_of("LB_FEATS", &vm).and_then(|state| state.list);
        list.map_or(Vec::new(), |list| {
            list.rows
                .iter()
                .map(|row| row.cells.iter().map(|cell| cell.look).collect())
                .collect()
        })
    };
    let addable = |stack: &Stack| {
        stack
            .state_of("BTN_SELECT", &vm)
            .is_some_and(|state| !state.disabled)
    };
    let opened = (name(&stack), looks(&stack));
    // Whirlwind's square: the row's third, 218 along, 2 down.
    let mut step = |stack: &mut Stack, input: &UiInput| {
        stack.step(&mut assets, &vm, input);
    };
    step(
        &mut stack,
        &pointing([20.0 + 218.0 + 20.0, 20.0 + 2.0 + 20.0]),
    );
    let pointed = (name(&stack), addable(&stack));
    step(&mut stack, &keys(&[Key::Left]));
    let left = name(&stack);
    step(&mut stack, &keys(&[Key::Down]));
    let down = name(&stack);
    step(&mut stack, &keys(&[Key::Up]));
    step(&mut stack, &keys(&[Key::Left]));
    // The cursor alone chooses nothing: Add waits for Enter or a click.
    let read_only = (name(&stack), addable(&stack));
    step(&mut stack, &keys(&[Key::Activate]));
    stack.press("BTN_SELECT");
    step(&mut stack, &keys(&[]));
    let added = (
        name(&stack),
        looks(&stack).first().and_then(|row| row.first().copied()),
        text(&stack, &vm, "STD_REMAINING_SELECTIONS_LBL"),
    );
    stack.press("BTN_SELECT");
    step(&mut stack, &keys(&[]));
    let dropped = looks(&stack).first().and_then(|row| row.first().copied());
    // Toughness, the third row's only feat: the gap past its square still
    // chooses it.
    step(&mut stack, &pointing([20.0 + 150.0, 20.0 + 96.0 + 20.0]));
    let pointed_at = name(&stack);
    stack.press("BTN_RECOMMENDED");
    step(&mut stack, &keys(&[]));
    // Off the chart again, what was read gives way to what was chosen.
    let back = name(&stack);
    let recommended = (pointed_at, looks(&stack));
    // A double click on a feat is Add on it: Flurry, picked, is dropped.
    let flurry = [20.0 + 20.0, 20.0 + 2.0 + 20.0];
    let click = UiInput {
        click: true,
        ..pointing(flurry)
    };
    step(&mut stack, &click);
    step(&mut stack, &click);
    let double = looks(&stack).first().and_then(|row| row.first().copied());
    use CellLook::{Closed, Known, Open, Picked};
    check("feat_chart", {
        expect(
            &format!("three chains, Flurry chosen: {opened:?}"),
            opened
                == (
                    "Flurry".into(),
                    vec![vec![Open, Closed, Closed], vec![Known, Closed], vec![Open]],
                ),
        )
        .and_then(|()| {
            expect(
                &format!("the pointer on Whirlwind, not to add: {pointed:?}"),
                pointed == ("Whirlwind".into(), false),
            )
        })
        .and_then(|()| {
            expect(
                &format!("left, then down: {left:?}, {down:?}"),
                left == "Improved Flurry" && down == "Improved Power Attack",
            )
        })
        .and_then(|()| {
            expect(
                &format!("the cursor on Flurry reads it, adds nothing: {read_only:?}"),
                read_only == ("Flurry".into(), false),
            )
        })
        .and_then(|()| {
            expect(
                &format!("Flurry added: {added:?}"),
                added == ("Flurry".into(), Some(Picked), "0".into()),
            )
        })
        .and_then(|()| expect(&format!("and dropped: {dropped:?}"), dropped == Some(Open)))
        .and_then(|()| {
            expect(
                &format!("Toughness pointed, Recommended takes Flurry: {recommended:?}"),
                recommended
                    == (
                        "Toughness".into(),
                        vec![
                            vec![Picked, Closed, Closed],
                            vec![Known, Closed],
                            vec![Open],
                        ],
                    ),
            )
        })
        .and_then(|()| {
            expect(
                &format!("a double click on a picked feat drops it: {double:?}"),
                double == Some(Open),
            )
        })
        .and_then(|()| {
            expect(
                &format!("the pointer off the chart: the chosen feat is read again: {back:?}"),
                back == "Flurry",
            )
        })
        .map(|()| "chains, pointer, keys, Add, a double click, Recommended".into())
    })
}

pub(super) fn all() -> Vec<Check> {
    let mut assets = Fake::new();
    layouts(&mut assets);
    let vm = chargen_vm();
    let opened = |assets: &mut Fake| {
        let mut stack = Stack::new(crate::screens::open);
        stack.open("chargen:solo", &vm);
        stack.step(assets, &vm, &keys(&[]));
        stack
    };
    let mut stack = opened(&mut assets);
    let focused_class = text(&stack, &vm, "LBL_CLASS");
    let played = quick(&mut stack, &mut assets, &vm);

    // The custom path's abilities: strength to 18, one back down.
    let mut stack = opened(&mut assets);
    presses(
        &mut stack,
        &mut assets,
        &vm,
        &[
            "BTN_SEL3",
            "CUST_CHAR_BTN",
            "BTN_STEPNAME1",
            "BTN_ACCEPT",
            "BTN_STEPNAME2",
        ],
    );
    let fresh = text(&stack, &vm, "REMAINING_SELECTIONS_LBL");
    presses(&mut stack, &mut assets, &vm, &["STR_PLUS_BTN"; 12]);
    let at_most = (
        text(&stack, &vm, "STR_POINTS_BTN"),
        text(&stack, &vm, "REMAINING_SELECTIONS_LBL"),
        stack.hidden("STR_PLUS_BTN", &vm),
        stack
            .state_of("BTN_ACCEPT", &vm)
            .is_some_and(|state| state.disabled),
    );
    presses(&mut stack, &mut assets, &vm, &["STR_MINUS_BTN"]);
    let refunded = text(&stack, &vm, "REMAINING_SELECTIONS_LBL");
    // Backed out: the picker opens again as it was.
    presses(&mut stack, &mut assets, &vm, &["BTN_BACK", "BTN_STEPNAME2"]);
    let undone = text(&stack, &vm, "STR_POINTS_BTN");
    // The recommended scores, accepted; skills: 4 points, the class skill
    // a point a rank up to 4, another two a rank up to 2.
    presses(
        &mut stack,
        &mut assets,
        &vm,
        &["BTN_RECOMMENDED", "BTN_ACCEPT", "BTN_STEPNAME3"],
    );
    let skill_points = text(&stack, &vm, "REMAINING_SELECTIONS_LBL");
    presses(&mut stack, &mut assets, &vm, &["COM_PLUS_BTN"; 5]);
    let class_skill = (
        text(&stack, &vm, "COMPUTER_USE_POINTS_BTN"),
        stack.hidden("COM_PLUS_BTN", &vm),
    );
    presses(
        &mut stack,
        &mut assets,
        &vm,
        &[
            "COM_MINUS_BTN",
            "COM_MINUS_BTN",
            "DEM_PLUS_BTN",
            "DEM_PLUS_BTN",
        ],
    );
    let cross = (
        text(&stack, &vm, "DEMOLITIONS_POINTS_BTN"),
        text(&stack, &vm, "REMAINING_SELECTIONS_LBL"),
    );
    // Recommended: demolitions first, as high as 4 points go.
    presses(&mut stack, &mut assets, &vm, &["BTN_RECOMMENDED"]);
    let recommended = (
        text(&stack, &vm, "DEMOLITIONS_POINTS_BTN"),
        text(&stack, &vm, "COMPUTER_USE_POINTS_BTN"),
    );
    // Back on the path undoes the last step; at class selection, closes.
    presses(&mut stack, &mut assets, &vm, &["BTN_ACCEPT", "BTN_BACK"]);
    let next = stack
        .state_of("BTN_STEPNAME3", &vm)
        .is_some_and(|state| !state.disabled);
    presses(
        &mut stack,
        &mut assets,
        &vm,
        &["BTN_CANCEL", "BTN_BACK", "BTN_BACK"],
    );
    let closed = !stack.is_open();

    vec![
        check("chargen", {
            expect(
                &format!("the focused class shown: {focused_class:?}"),
                focused_class == "Male Scoundrel",
            )
            .and_then(|()| {
                expect(
                    &format!("quick creation plays as the character made: {played:?}"),
                    played
                        == vec![Intent::Session(SessionIntent::Solo {
                            save: None,
                            character: made(),
                            campaign: None,
                        })],
                )
            })
            .and_then(|()| expect(&format!("30 points: {fresh:?}"), fresh == "30"))
            .and_then(|()| {
                expect(
                    &format!(
                        "18 at most for 16 points, not accepted while some are left: {at_most:?}"
                    ),
                    at_most == ("18".into(), "14".into(), true, true),
                )
            })
            .and_then(|()| expect(&format!("17 refunds 3: {refunded:?}"), refunded == "17"))
            .and_then(|()| expect(&format!("backing out undoes: {undone:?}"), undone == "8"))
            .and_then(|()| {
                expect(
                    &format!("(2 + 0) / 2 × 4 skill points: {skill_points:?}"),
                    skill_points == "4",
                )
            })
            .and_then(|()| {
                expect(
                    &format!("a class skill to 4: {class_skill:?}"),
                    class_skill == ("4".into(), true),
                )
            })
            .and_then(|()| {
                expect(
                    &format!("another for 2 a rank: {cross:?}"),
                    cross == ("1".into(), "0".into()),
                )
            })
            .and_then(|()| {
                expect(
                    &format!("the recommended skills in order: {recommended:?}"),
                    recommended == ("2".into(), "0".into()),
                )
            })
            .and_then(|()| expect("Back undoes the skills step", next))
            .and_then(|()| expect("Cancel, then Back twice, closes", closed))
            .map(|()| "class, quick walk, point buys, back and cancel".into())
        }),
        feat_chart(),
    ]
}
