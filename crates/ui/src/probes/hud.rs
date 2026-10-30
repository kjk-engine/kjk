//! KOTOR's HUD: its controls kept to their corners in a wide window, and
//! what it shows of the player.

use fmt_odyssey::gui::{Border, Control, Gui, Rect};

use super::menus::root;
use super::paint::{Fake, button};
use super::{Check, check, expect};
use crate::draw::DrawList;
use crate::paint::{Painter, Palette};
use crate::screens::hud::{
    BusyMark, SWAP, anchor, backed, busy, busy_marks, crosshair, effect_arrows, portrait_at, state,
    swap_at, watch_at,
};
use crate::vm::{Busy, HudView, MemberView, RequestsView, TargetAction, TargetView, ViewModel};

fn at(gui: &Gui, tag: &str) -> Option<(i32, i32)> {
    gui.find(tag)
        .map(|control| (control.extent.left, control.extent.top))
}

/// TSL's weapon swap button on the HUD, in both games: shown, moved with
/// its corner, and a click on it is a swap (none beside it, nor without a
/// character).
pub(super) fn swap_button() -> Vec<Check> {
    let mut gui = root(800, 600, vec![button(SWAP, [600, 540, 60, 30], b"")]);
    anchor(&mut gui, (1280.0, 720.0));
    let vm = ViewModel {
        hud: Some(HudView::default()),
        ..ViewModel::default()
    };
    let shown = !state(&button(SWAP, [0, 0, 1, 1], b""), &vm).hidden;
    let clicks = (
        swap_at(&gui, &vm, [1100.0, 670.0]),
        swap_at(&gui, &vm, [1000.0, 670.0]),
        swap_at(&gui, &ViewModel::default(), [1100.0, 670.0]),
    );
    vec![check(
        "hud_swap_button",
        expect(
            &format!("shown {shown}, at {:?}, clicks {clicks:?}", at(&gui, SWAP)),
            shown && at(&gui, SWAP) == Some((1080, 660)) && clicks == (true, false, false),
        )
        .map(|()| "shown at the lower right, a click on it swaps".into()),
    )]
}

/// KOTOR 2's effect arrows on the portraits (`effect_arrows`): stacked about
/// their labels, the leader's 10 apart, a companion's 5 and at most 5; none
/// on a debilitated member, whose overlay shows instead.
pub(super) fn effect_arrows_stack() -> Vec<Check> {
    let arrow = |tag: &str, rect: [i32; 4], fill: &str| Control {
        border: Some(Border {
            fill: fill.into(),
            ..Border::default()
        }),
        ..button(tag, rect, b"")
    };
    let gui = root(
        800,
        600,
        vec![
            button("LBL_CHAR1", [707, 523, 72, 72], b""),
            arrow("LBL_CMBTEFCTINC1", [712, 550, 18, 18], "lbl_micomeff02"),
            arrow("LBL_CMBTEFCTRED1", [756, 550, 18, 18], "lbl_micomeff01"),
            button("LBL_CHAR2", [752, 484, 32, 32], b""),
            arrow("LBL_CMBTEFCTINC2", [753, 495, 10, 10], "lbl_micomeff02"),
            button("LBL_CHAR3", [752, 445, 32, 32], b""),
            arrow("LBL_CMBTEFCTINC3", [753, 456, 10, 10], "lbl_micomeff02"),
        ],
    );
    let member = |effects: (u8, u8), debilitated: bool| MemberView {
        effects,
        debilitated,
        ..MemberView::default()
    };
    let vm = ViewModel {
        hud: Some(HudView::default()),
        party: vec![
            member((3, 1), false),
            member((7, 0), false),
            member((2, 2), true),
        ],
        ..ViewModel::default()
    };
    let tops = |fill: &str, left: f32| {
        effect_arrows(&gui, &vm)
            .into_iter()
            .filter(|arrow| arrow.fill == fill && arrow.rect[0] == left)
            .map(|arrow| arrow.rect[1] as i32)
            .collect::<Vec<_>>()
    };
    let stacks = (
        tops("lbl_micomeff02", 712.0),
        tops("lbl_micomeff01", 756.0),
        tops("lbl_micomeff02", 753.0),
    );
    let overlay = ["LBL_DEBILATATED1", "LBL_DEBILATATED3"]
        .map(|tag| !state(&button(tag, [0, 0, 1, 1], b""), &vm).hidden);
    let labels_hidden = state(&button("LBL_CMBTEFCTINC1", [0, 0, 1, 1], b""), &vm).hidden;
    vec![check(
        "hud_effect_arrows",
        expect(
            &format!("stacks {stacks:?}, overlays {overlay:?}"),
            stacks
                == (
                    vec![540, 550, 560],
                    vec![550],
                    vec![487, 492, 497, 502, 507],
                )
                && overlay == [false, true]
                && labels_hidden,
        )
        .map(|()| "good up and bad down, stacked, capped, none when debilitated".into()),
    )]
}

pub(super) fn all() -> Vec<Check> {
    let layout = Gui {
        extent: Rect {
            left: 0,
            top: 0,
            width: 800,
            height: 600,
        },
        ..root(
            800,
            600,
            vec![
                button("BTN_EQU", [700, 10, 40, 40], b""),
                button("LBL_CHAR1", [10, 540, 50, 50], b""),
                button("BTN_CHAR2", [70, 540, 40, 50], b""),
                button("LBL_CHAR2", [70, 540, 40, 50], b""),
                button("LBL_CHAR3", [120, 540, 40, 50], b""),
                button("BTN_ACTION0", [720, 540, 40, 40], b""),
                button("LBL_MOULDING2", [300, 100, 40, 40], b""),
                button("LBL_NAME", [0, 1, 200, 26], b""),
                button("BTN_TARGET0", [43, 35, 36, 60], b""),
                button("LBL_ACTIONDESC", [14, 496, 232, 32], b""),
            ],
        )
    };
    // At its own size: 1280 × 720 is the frame, a square window 800 by
    // 800; a window shorter than the layout, 1024 × 576, scales it down to
    // 600 tall, 1067 wide.
    let mut gui = layout.clone();
    anchor(&mut gui, (1280.0, 720.0));
    let mut small = layout.clone();
    anchor(&mut small, (1024.0, 576.0));
    let mut square = layout;
    anchor(&mut square, (800.0, 800.0));
    let lower = (
        square.frame(),
        at(&square, "LBL_CHAR1"),
        at(&square, "BTN_EQU"),
    );
    let placed = [
        at(&gui, "BTN_EQU"),
        at(&gui, "LBL_CHAR1"),
        at(&gui, "BTN_ACTION0"),
        at(&gui, "LBL_MOULDING2"),
    ];
    let frame = (gui.frame(), small.frame());
    let block = (
        at(&gui, "LBL_NAME"),
        at(&gui, "BTN_TARGET0"),
        at(&gui, "LBL_ACTIONDESC"),
    );
    let dot = crosshair(&gui)[1].rect;
    let vm = ViewModel {
        hud: Some(HudView {
            hp: (30, 120),
            fp: (0, 0),
            portrait: Some("po_pmhc1".into()),
            actions: vec![("ip_push".into(), None), ("iu_medpac".into(), Some(3))],
            action_keys: vec![b"F".to_vec(), b"3".to_vec()],
            lit: vec!["LBL_PLOTXP"],
            minimap: None,
            stealth: false,
            busy: None,
        }),
        party: vec![MemberView {
            may_level: true,
            ..MemberView::default()
        }],
        ..ViewModel::default()
    };
    let shown = |tag: &str| state(&button(tag, [0, 0, 1, 1], b""), &vm);
    let vitality = shown("PB_VIT1").value;
    let no_force = shown("PB_FORCE1").hidden;
    let queue = shown("LBL_QUEUE0").hidden && shown("PB_VIT2").hidden;
    let marks = (
        shown("LBL_LEVELUP1").hidden,
        shown("LBL_LEVELUP2").hidden,
        shown("LBL_JOURNAL").hidden,
        shown("LBL_PLOTXP").hidden,
    );
    // The campaign's arrow, pulsing, over KOTOR's backing, which a layout
    // without one gets under its mark.
    let mark = shown("LBL_LEVELUP1");
    let mark = (mark.fill, mark.pulsing, shown("LBL_LVLUPBG1").hidden);
    let arrow = Control {
        border: Some(Border {
            fill: "lbl_levelup".into(),
            ..Border::default()
        }),
        ..button("LBL_LEVELUP1", [10, 540, 50, 50], b"")
    };
    let mut bare = root(
        800,
        600,
        vec![button("LBL_CHAR1", [10, 540, 50, 50], b""), arrow],
    );
    let campaign = root(
        800,
        600,
        vec![Control {
            border: Some(Border {
                fill: "dialog2".into(),
                ..Border::default()
            }),
            ..button("LBL_LVLUPBG1", [6, 536, 57, 57], b"")
        }],
    );
    backed(&mut bare, &campaign);
    let backing: Vec<_> = bare
        .controls
        .iter()
        .map(|c| (c.tag.as_str(), c.border.as_ref().map(|b| b.fill.as_str())))
        .collect();
    let slots = (
        shown("LBL_ACTION1").fill,
        shown("LBL_ACTION1").text,
        shown("BTN_ACTION2").hidden,
    );
    // Stealth mode: KOTOR's stealth toggle lit; pause and solo stay hidden.
    let stealthed = ViewModel {
        hud: vm.hud.clone().map(|hud| HudView {
            stealth: true,
            ..hud
        }),
        ..vm.clone()
    };
    let toggle = |vm: &ViewModel, tag: &str| state(&button(tag, [0, 0, 1, 1], b""), vm);
    let stealth = (
        toggle(&vm, "TB_STEALTH").hidden,
        toggle(&stealthed, "TB_STEALTH").hidden,
        toggle(&stealthed, "TB_STEALTH").selected,
        toggle(&stealthed, "TB_PAUSE").hidden,
    );
    // A companion: the second portrait and its bars; no third.
    let with_trask = ViewModel {
        party: vec![
            MemberView::default(),
            MemberView {
                portrait: Some("po_ptrask".into()),
                hp: (18, 36),
                id: 42,
                ..MemberView::default()
            },
        ],
        ..vm.clone()
    };
    let companion = |tag: &str| state(&button(tag, [0, 0, 1, 1], b""), &with_trask);
    let party = (
        companion("LBL_CHAR2").fill,
        companion("PB_VIT2").value,
        companion("PB_FORCE2").hidden,
        companion("LBL_CHAR3").hidden && companion("PB_VIT3").hidden,
    );
    // Each member's own mark: the companion's lit alone, the player's out.
    let trask_levels = ViewModel {
        party: vec![
            MemberView::default(),
            MemberView {
                may_level: true,
                ..MemberView::default()
            },
        ],
        ..vm.clone()
    };
    let own_mark = |tag: &str| state(&button(tag, [0, 0, 1, 1], b""), &trask_levels);
    let marks_by_member = (
        own_mark("LBL_LEVELUP1").hidden,
        own_mark("LBL_LVLUPBG1").hidden,
        own_mark("LBL_LEVELUP2").fill,
        own_mark("LBL_LEVELUP2").pulsing,
        own_mark("LBL_LVLUPBG2").hidden,
        own_mark("LBL_LEVELUP3").hidden,
    );
    // A click on the companion's portrait picks it (its body); nothing
    // beside it or without a companion there.
    let clicked = (
        portrait_at(&gui, &with_trask, [80.0, 680.0]),
        portrait_at(&gui, &with_trask, [130.0, 680.0]),
        portrait_at(&gui, &vm, [80.0, 680.0]),
    );
    // What each player is in (R10): an icon at its portrait's top right,
    // the game's own art; none while it plays.
    let busy_with = |own: Option<Busy>, companion: Option<Busy>| ViewModel {
        hud: vm.hud.clone().map(|hud| HudView { busy: own, ..hud }),
        party: vec![
            MemberView::default(),
            MemberView {
                busy: companion,
                ..with_trask.party[1].clone()
            },
        ],
        ..vm.clone()
    };
    let icons = |vm: &ViewModel| {
        let mut fake = Fake::new();
        let mut out = DrawList::default();
        busy(
            &gui,
            vm,
            &mut Painter::new(&mut fake, Palette::K1),
            &mut out,
        );
        let names = |id| fake.named(id);
        out.quads
            .iter()
            .map(|quad| (names(quad.texture), quad.rect))
            .collect::<Vec<_>>()
    };
    let busy_icons = [
        icons(&busy_with(None, Some(Busy::Talking))),
        icons(&busy_with(Some(Busy::Pazaak), None)),
        icons(&busy_with(None, None)),
    ];
    let wanted_icons = [
        vec![(Some("gui_mp_talkd".into()), [90.0, 660.0, 20.0, 20.0])],
        vec![(Some("pcards_pos_x".into()), [35.0, 660.0, 25.0, 25.0])],
        Vec::new(),
    ];
    // The third portrait asking the host in: the exclamation over it on the
    // host's screen (a request card up), not elsewhere.
    let asking = |requests: Option<RequestsView>| {
        let mut vm = busy_with(None, None);
        vm.party.push(MemberView {
            busy: Some(Busy::Asking),
            ..MemberView::default()
        });
        busy_marks(&gui, &ViewModel { requests, ..vm })
    };
    let asking = [Some(RequestsView::default()), None].map(asking);
    let request_mark = |exclaim| BusyMark {
        rect: [140.0, 660.0, 20.0, 20.0],
        icon: "lbl_iquest",
        exclaim,
    };
    let wanted_asking = [vec![request_mark(true)], vec![request_mark(false)]];
    let action = |icon: &str, label: &[u8]| TargetAction {
        icon: icon.into(),
        label: label.to_vec(),
    };
    let target = TargetView {
        name: b"Metal Box".to_vec(),
        hostile: false,
        hp: None,
        actions: vec![
            action("isk_security", b"Security"),
            action("i_openplace", b"Open"),
        ],
        selected: 1,
        busy: false,
        progress: None,
    };
    let looking = |target: Option<TargetView>| ViewModel {
        target,
        ..vm.clone()
    };
    let block_shows = |vm: &ViewModel, tag: &str| state(&button(tag, [0, 0, 1, 1], b""), vm);
    let with = looking(Some(target.clone()));
    let walking = looking(Some(TargetView {
        busy: true,
        ..target.clone()
    }));
    let creature = looking(Some(TargetView {
        hp: Some((10, 40)),
        ..target.clone()
    }));
    let foe = looking(Some(TargetView {
        hp: Some((10, 40)),
        hostile: true,
        ..target
    }));
    let tints = [&creature, &foe].map(|vm| {
        (
            block_shows(vm, "LBL_NAME").tint.is_some(),
            block_shows(vm, "PB_HEALTH").tint.is_some(),
        )
    });
    let none = looking(None);
    let shows = (
        block_shows(&with, "LBL_NAME").text,
        block_shows(&with, "PB_HEALTH").hidden,
        block_shows(&creature, "PB_HEALTH").value,
        block_shows(&with, "BTN_TARGET1").selected,
        block_shows(&with, "BTN_TARGET0").selected,
        block_shows(&with, "BTN_TARGET2").hidden,
        block_shows(&with, "LBL_TARGET0").fill,
        block_shows(&with, "LBL_ACTIONDESC").text,
    );
    let quiet = (
        block_shows(&walking, "BTN_TARGET0").hidden,
        block_shows(&walking, "LBL_ACTIONDESC").hidden,
        !block_shows(&walking, "LBL_NAME").hidden,
        block_shows(&none, "LBL_NAME").hidden && block_shows(&none, "BTN_TARGET0").hidden,
        block_shows(&with, "BTN_TARGETUP0").hidden && block_shows(&with, "BTN_TARGETDOWN1").hidden,
    );
    // A downed party member (I30 P11): its name, its vitality (none), Revive
    // lit; revived half way, the bar half full and "Reviving" beneath. On
    // the downed player's own screen: its reviver and the bar, no actions.
    let downed = TargetView {
        name: b"Player 1".to_vec(),
        hostile: false,
        hp: Some((0, 40)),
        actions: vec![action("isk_treatinj", b"Treat Injury")],
        selected: 0,
        busy: false,
        progress: None,
    };
    let revived_half = looking(Some(TargetView {
        progress: Some(0.5),
        ..downed.clone()
    }));
    let downed = looking(Some(downed));
    let own = looking(Some(TargetView {
        name: b"Player 2".to_vec(),
        progress: Some(0.25),
        ..TargetView::default()
    }));
    let revive = (
        (
            block_shows(&downed, "LBL_TARGET0").fill,
            block_shows(&downed, "BTN_TARGET0").selected,
            block_shows(&downed, "PB_HEALTH").value,
            block_shows(&downed, "LBL_ACTIONDESC").text,
        ),
        (
            block_shows(&revived_half, "PB_HEALTH").value,
            block_shows(&revived_half, "LBL_ACTIONDESC").text,
            block_shows(&revived_half, "LBL_NAME").text,
            !block_shows(&revived_half, "BTN_TARGET0").hidden,
        ),
        (
            block_shows(&own, "PB_HEALTH").value,
            block_shows(&own, "LBL_NAME").text,
            block_shows(&own, "BTN_TARGET0").hidden,
        ),
    );
    let wanted_revive = (
        (
            Some("isk_treatinj".into()),
            true,
            Some(0.0),
            Some(b"Treat Injury".to_vec()),
        ),
        (
            Some(0.5),
            Some(b"Reviving".to_vec()),
            Some(b"Player 1".to_vec()),
            true,
        ),
        (Some(0.25), Some(b"Player 2".to_vec()), true),
    );
    // A click on a companion's pazaak icon watches that player's match
    // (R9a); not on its portrait beside the icon, on another icon, nor on
    // the player's own.
    let at_table = busy_with(None, Some(Busy::Pazaak));
    let pazaak_clicks = (
        watch_at(&gui, &at_table, [100.0, 670.0]),
        watch_at(&gui, &at_table, [80.0, 690.0]),
        watch_at(&gui, &busy_with(None, Some(Busy::Talking)), [100.0, 670.0]),
        watch_at(&gui, &busy_with(Some(Busy::Pazaak), None), [45.0, 670.0]),
    );
    let watch_click = check(
        "hud_pazaak_watch",
        expect(
            &format!("the icon's member, then none: {pazaak_clicks:?}"),
            pazaak_clicks == (Some(42), None, None, None),
        )
        .map(|()| format!("{pazaak_clicks:?}")),
    );
    let hud = check("hud", {
        expect(
            &format!("the frame the window, or its shape when smaller: {frame:?}"),
            frame == ((1280, 720), (1067, 600)),
        )
        .and_then(|()| {
            expect(
                &format!("each control kept to its corner: {placed:?}"),
                placed
                    == [
                        Some((1180, 10)),
                        Some((10, 660)),
                        Some((1200, 660)),
                        Some((300, 100)),
                    ],
            )
        })
        .and_then(|()| {
            expect(
                &format!("a tall window moves the lower half down: {lower:?}"),
                lower == ((800, 800), Some((10, 740)), Some((700, 10))),
            )
        })
        .and_then(|()| {
            expect(
                &format!("vitality a quarter: {vitality:?}"),
                vitality == Some(0.25),
            )
        })
        .and_then(|()| expect("no Force bar without Force points", no_force))
        .and_then(|()| expect("the queue and the party hidden", queue))
        .and_then(|()| {
            expect(
                &format!("the stealth toggle lit only in stealth mode: {stealth:?}"),
                stealth == (true, false, true, true),
            )
        })
        .and_then(|()| {
            expect(
                &format!("the level-up mark and XP shown, the journal's not: {marks:?}"),
                marks == (false, true, true, false),
            )
        })
        .and_then(|()| {
            expect(
                &format!("the campaign's arrow pulsing, its backing shown: {mark:?}"),
                mark == (Some("lbl_levelup".into()), true, false),
            )
        })
        .and_then(|()| {
            expect(
                &format!("each member's own level-up mark: {marks_by_member:?}"),
                marks_by_member
                    == (true, true, Some("lbl_levelup".into()), true, false, true),
            )
        })
        .and_then(|()| {
            expect(
                &format!("the backing under the mark: {backing:?}"),
                backing[1..]
                    == [
                        ("LBL_LVLUPBG1", Some("dialog2")),
                        ("LBL_LEVELUP1", Some("lbl_levelup")),
                    ],
            )
        })
        .and_then(|()| {
            expect(
                &format!("an item's slot, its own label blank (the uses and key are drawn at its corners), no third: {slots:?}"),
                slots == (Some("iu_medpac".into()), Some(Vec::new()), true),
            )
        })
        .and_then(|()| {
            expect(
                &format!("a companion's portrait and vitality, no third: {party:?}"),
                party == (Some("po_ptrask".into()), Some(0.5), true, true),
            )
        })
        .and_then(|()| {
            expect(
                &format!(
                    "a busy icon in the busy one's portrait's top right, none while all play: {busy_icons:?}"
                ),
                busy_icons == wanted_icons,
            )
        })
        .and_then(|()| {
            expect(
                &format!(
                    "the third portrait asking, its exclamation on the host's screen only: {asking:?}"
                ),
                asking == wanted_asking,
            )
        })
        .and_then(|()| {
            expect(
                &format!("a click on the companion's portrait picks it, nothing else: {clicked:?}"),
                clicked == (Some(42), None, None),
            )
        })
        .and_then(|()| {
            expect(
                &format!("the target block at the top centre, its words beneath: {block:?}"),
                block == (Some((540, 5)), Some((583, 39)), Some((524, 100))),
            )
        })
        .and_then(|()| {
            expect(
                &format!("the crosshair at the middle: {dot:?}"),
                dot == [638.0, 358.0, 4.0, 4.0],
            )
        })
        .and_then(|()| {
            expect(
                &format!("the target's name, vitality for a creature, the chosen lit: {shows:?}"),
                shows
                    == (
                        Some(b"Metal Box".to_vec()),
                        true,
                        Some(0.25),
                        true,
                        false,
                        true,
                        Some("isk_security".into()),
                        Some(b"Open".to_vec()),
                    ),
            )
        })
        .and_then(|()| {
            expect(
                &format!(
                    "walking there hides the actions, no target the block, no arrows: {quiet:?}"
                ),
                quiet == (true, true, true, true, true),
            )
        })
        .and_then(|()| {
            expect(
                &format!(
                    "a downed member offers Revive; revived, the bar and Reviving; the downed one's own block its reviver: {revive:?}"
                ),
                revive == wanted_revive,
            )
        })
        .and_then(|()| {
            expect(
                &format!("a foe's name and vitality tinted, another's not: {tints:?}"),
                tints == [(false, false), (true, true)],
            )
        })
        .map(|()| "anchoring, bars, slots, marks, companions and their clicks, the target, Revive and its bar".into())
    });
    vec![hud, watch_click]
}
