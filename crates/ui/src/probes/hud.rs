//! KOTOR's HUD: its controls placed as KOTOR 2 places them on any window,
//! and what it shows of the player.

use fmt_odyssey::gui::{Border, Control, Gui, Rect};

use super::menus::root;
use super::paint::{Fake, button};
use super::{Check, check, expect};
use crate::draw::{DrawList, TextureId};
use crate::paint::{Painter, Palette, rect_of};
use crate::screens::floaty::{OverheadArt, bars};
use crate::screens::hud::{
    BarArt, BusyMark, FEAT, FloatHold, SOLO, SWAP, action_keys, backed, bar_extra, busy,
    busy_marks, crosshair, cycle_badges, duel_bars, effect_icons, float_at, floated, known, place,
    portrait_at, solo_at, solo_beside_portrait, state, to_top, watch_at,
};
use crate::vm::{
    Bar, BarSlot, Busy, DuelBars, EffectIcon, HudView, KeyLabel, MemberView, OverheadView,
    RequestsView, TargetAction, TargetView, ViewModel,
};

fn at(gui: &Gui, tag: &str) -> Option<(i32, i32)> {
    gui.find(tag)
        .map(|control| (control.extent.left, control.extent.top))
}

/// The feat slot in the weapon swap button's stead. KOTOR 2's swap button
/// is hidden. The feat slot stands one step after the Form slot, or in the
/// Form slot's place when the character knows no Form. It shows the icon of
/// the feat attack in use, and it is hidden while the view has none.
pub(super) fn feat_slot() -> Vec<Check> {
    let feat = |prefix: &str| format!("{prefix}{FEAT}");
    let placed = |extras: [bool; 2]| {
        let mut gui = root(
            800,
            600,
            (0..6)
                .map(|n| button(&format!("BTN_ACTION{n}"), [100 + 40 * n, 500, 36, 60], b""))
                .collect(),
        );
        bar_extra(&mut gui, 6, extras);
        at(&gui, &feat("BTN_ACTION")).map(|(left, _)| left)
    };
    let places = (placed([true, true]), placed([false, true]));
    let vm = |slot: Option<BarSlot>| ViewModel {
        hud: Some(HudView {
            feat: slot,
            ..HudView::default()
        }),
        ..ViewModel::default()
    };
    let shown = vm(Some(BarSlot {
        icon: Some("ff_powrattck".into()),
        ..BarSlot::default()
    }));
    let state_of = |tag: &str, vm: &ViewModel| state(&button(tag, [0, 0, 1, 1], b""), vm);
    let icon = state_of(&feat("LBL_ACTION"), &shown).fill;
    let hidden = state_of(&feat("BTN_ACTION"), &vm(None)).hidden;
    let swap_hidden = state_of(SWAP, &shown).hidden;
    vec![check(
        "hud_feat_slot",
        expect(
            &format!("at {places:?}, icon {icon:?}, hidden without one {hidden}, swap hidden {swap_hidden}"),
            places == (Some(380), Some(340))
                && icon.as_deref() == Some("ff_powrattck")
                && hidden
                && swap_hidden,
        )
        .map(|()| "the feat slot stands after the Form slot, and the swap button is gone".into()),
    )]
}

/// The solo mode button beside the portrait. KOTOR 2's layout puts it far
/// left of the portrait (584, 562). The HUD moves it just left of the
/// vitality bar, at half the portrait's height, its bottom level with the
/// portrait's. It shows, lit, only while the party is in solo mode, and
/// only then does a click on it count.
pub(super) fn solo_button() -> Vec<Check> {
    let mut gui = root(
        800,
        600,
        vec![
            button("LBL_CHAR1", [707, 523, 72, 72], b""),
            button("PB_VIT1", [677, 520, 39, 78], b""),
            button(SOLO, [584, 562, 36, 36], b""),
        ],
    );
    solo_beside_portrait(&mut gui);
    let placed = gui.find(SOLO).map(|control| control.extent);
    let vm = |solo: bool| ViewModel {
        hud: Some(HudView {
            solo,
            ..HudView::default()
        }),
        ..ViewModel::default()
    };
    let (on, off) = (vm(true), vm(false));
    let shown = |vm: &ViewModel| state(&button(SOLO, [0, 0, 1, 1], b""), vm);
    let looks = (shown(&on).selected, shown(&off).hidden);
    let clicks = (
        solo_at(&gui, &on, [670.0, 580.0]),
        solo_at(&gui, &on, [600.0, 580.0]),
        solo_at(&gui, &off, [670.0, 580.0]),
    );
    let at = placed.map(|rect| [rect.left, rect.top, rect.width, rect.height]);
    vec![check(
        "hud_solo_button",
        expect(
            &format!("at {at:?}, lit and hidden {looks:?}, clicks {clicks:?}"),
            at == Some([650, 559, 36, 36])
                && looks == (true, true)
                && clicks == (true, false, false),
        )
        .map(|()| "lit beside the portrait in solo mode, hidden otherwise".into()),
    )]
}

/// A mod's HUD (`known`) keeps only the controls whose tags the game's own
/// HUD has, children too. UI Redesign renames the combat message to
/// `*LBL_CMBTMODEMSG`, and that label is left out.
pub(super) fn unknown_tags() -> Vec<Check> {
    let stock = root(
        800,
        600,
        vec![
            button("LBL_CMBTMODEMSG", [219, 4, 314, 36], b""),
            button(SWAP, [600, 540, 60, 30], b""),
        ],
    );
    let mut swap = button(SWAP, [600, 540, 60, 30], b"");
    swap.children = vec![button("LBL_ADDED", [600, 540, 10, 10], b"")];
    let mut gui = root(
        800,
        600,
        vec![
            button(
                "*LBL_CMBTMODEMSG",
                [219, 4, 314, 36],
                b"Combat mode message",
            ),
            swap,
        ],
    );
    known(&mut gui, &stock);
    let tags: Vec<&str> = gui.walk().map(|control| control.tag.as_str()).collect();
    vec![check(
        "hud_unknown_tags",
        expect(&format!("kept {tags:?}"), tags == [SWAP])
            .map(|()| "a mod's renamed and added controls are left out".into()),
    )]
}

/// The pad's buttons that choose among a target's actions (`cycle_badges`):
/// with three actions, the back chord stands left of the first slot and the
/// on chord right of the third, both level with the slots' middle. With
/// two actions the on chord stands right of the second slot. There are none
/// while the player walks to the target, nor while the keys play (no hint).
pub(super) fn target_cycle() -> Vec<Check> {
    let slots = (0..3)
        .map(|n| button(&format!("BTN_TARGET{n}"), [100 + 40 * n, 50, 36, 60], b""))
        .collect();
    let gui = root(800, 600, slots);
    let badges = [
        "kjkpad_deck_lb+dpadleft".to_string(),
        "kjkpad_deck_lb+dpadright".to_string(),
    ];
    let target = |actions: usize, busy: bool, cycle: bool| TargetView {
        actions: vec![TargetAction::default(); actions],
        busy,
        cycle: cycle.then(|| badges.clone()),
        ..TargetView::default()
    };
    let drawn = |target: &TargetView| {
        cycle_badges(&gui, target)
            .into_iter()
            .map(|(badge, [x, y, w, h])| (badge, [x, y, w, h].map(|v| v.round() as i32)))
            .collect::<Vec<_>>()
    };
    let three = drawn(&target(3, false, true));
    let two = drawn(&target(2, false, true));
    let quiet = (
        drawn(&target(3, true, true)).is_empty(),
        drawn(&target(3, false, false)).is_empty(),
    );
    let wanted_three = vec![
        (badges[0].clone(), [20, 67, 78, 26]),
        (badges[1].clone(), [218, 67, 78, 26]),
    ];
    let wanted_two = vec![
        (badges[0].clone(), [20, 67, 78, 26]),
        (badges[1].clone(), [178, 67, 78, 26]),
    ];
    vec![check(
        "hud_target_cycle",
        expect(
            &format!("three {three:?}, two {two:?}, walking and keys none {quiet:?}"),
            three == wanted_three && two == wanted_two && quiet == (true, true),
        )
        .map(|()| "the pad's back and on buttons beside the first and last slots".into()),
    )]
}

/// The player's Stamina bar (`duel_bars`): KOTOR 2's portrait bar art
/// upright left of the vitality bar's frame, as tall and level with it,
/// thinner; its backing, then its fill cut to its share from the bottom
/// (the art's lower part, not squeezed); none without it. Its width and
/// gap follow the HUD's stretch. Over a head the same art lies on its
/// side and fills rightward.
pub(super) fn duel() -> Vec<Check> {
    let gui = root(800, 600, vec![button("PB_VIT1", [700, 520, 64, 128], b"")]);
    let art = BarArt {
        texture: TextureId::WHITE,
        uv: [29.0 / 64.0, 2.0 / 128.0, 49.0 / 64.0, 126.0 / 128.0],
        back: [0.0; 3],
    };
    let bar = |share| Bar {
        share,
        color: [1.0; 4],
    };
    let with = |duel| ViewModel {
        hud: Some(HudView {
            duel,
            ..HudView::default()
        }),
        ..ViewModel::default()
    };
    let drawn_at = |vm: &ViewModel, stretch: f32| {
        let mut out = DrawList::default();
        duel_bars(&gui, vm, art, stretch, &mut out);
        out.quads
    };
    let drawn = |vm: &ViewModel| drawn_at(vm, 1.0);
    let one = drawn(&with(Some(DuelBars { points: bar(0.5) })));
    let none = drawn(&with(None));
    let rect = |nth: usize| one.get(nth).map_or([0.0; 4], |quad| quad.rect);
    let [points, points_fill] = [0, 1].map(rect);
    // The vitality bar's frame in its 64 x 128 control: x 729, y 522 to 646.
    let framed = |[left, top, wide, tall]: [f32; 4]| {
        top == 522.0 && tall == 124.0 && wide < 12.0 && left + wide < 729.0
    };
    let placed = one.len() == 2
        && framed(points)
        && points_fill == [points[0], 584.0, points[2], 62.0]
        && one[1].uv[1] == 64.0 / 128.0
        && one[1].uv[3] == one[0].uv[3];
    // Over a head the art lies on its side, filling rightward.
    let head = OverheadView {
        over: [0.5, 0.5],
        alpha: 1.0,
        bars: DuelBars { points: bar(0.25) },
        health: bar(0.5),
    };
    let lying = bars(&[head], OverheadArt { upper: art }, (200.0, 200.0)).quads;
    // The Stamina bar stands 4 pixels over the head and the health bar
    // right over it, a pixel between them.
    let lies = lying.len() == 4
        && lying.iter().all(|quad| quad.turns == 1)
        && lying[0].rect == [70.0, 90.0, 60.0, 6.0]
        && lying[1].rect == [70.0, 90.0, 15.0, 6.0]
        && lying[2].rect == [70.0, 83.0, 60.0, 6.0]
        && lying[3].rect == [70.0, 83.0, 30.0, 6.0]
        && lying[0].texture == art.texture;
    // On a HUD stretched twice down (`place`), the bar is twice as wide and
    // stands twice as far from the vitality bar's frame.
    let twice = drawn_at(&with(Some(DuelBars { points: bar(1.0) })), 2.0);
    let stretched = twice.first().is_some_and(|quad| {
        let [left, _, wide, _] = quad.rect;
        wide == 2.0 * points[2] && 729.0 - (left + wide) == 2.0 * (729.0 - (points[0] + points[2]))
    });
    let placed = placed && lies && stretched;
    vec![check(
        "hud_duel_bars",
        expect(
            &format!(
                "{:?}, over a head {:?}, without {}",
                one.iter()
                    .map(|quad| (quad.rect, quad.uv))
                    .collect::<Vec<_>>(),
                lying.iter().map(|quad| quad.rect).collect::<Vec<_>>(),
                none.len()
            ),
            placed && none.is_empty(),
        )
        .map(|()| {
            "K2's bar art left of the vitality bar, cut to share; a health bar and a Stamina bar over a head".into()
        }),
    )]
}

/// KOTOR 2's feedback option "Hide Quick Menu Buttons": the eight menu
/// buttons and their backing show while it is off (as it is unless set) and
/// hide while it is on.
pub(super) fn feedback_options() -> Vec<Check> {
    let menu = ["BTN_EQU", "BTN_OPT", "LBL_MENUBG"];
    let hidden_with = |value: Option<&str>| {
        let vm = ViewModel {
            hud: Some(HudView::default()),
            settings: value
                .map(|value| {
                    (
                        crate::feedback::HIDE_MENU_BUTTONS.to_string(),
                        value.to_string(),
                    )
                })
                .into_iter()
                .collect(),
            ..ViewModel::default()
        };
        menu.map(|tag| state(&button(tag, [0, 0, 1, 1], b""), &vm).hidden)
    };
    let (unset, on) = (hidden_with(None), hidden_with(Some("true")));
    vec![check(
        "hud_hides_menu_buttons",
        expect(
            &format!("hidden unset {unset:?}, on {on:?}"),
            unset == [false; 3] && on == [true; 3],
        )
        .map(|()| "shown by default, hidden with the option on".into()),
    )]
}

/// How far `rect` stands from the sides of a `frame` it is nearer to: from
/// the left or the right, and from the top or the bottom.
fn gaps(rect: Rect, (wide, tall): (i32, i32)) -> [i32; 2] {
    let near = |start: i32, size: i32, whole: i32| {
        if 2 * start + size < whole {
            start
        } else {
            whole - start - size
        }
    };
    [
        near(rect.left, rect.width, wide),
        near(rect.top, rect.height, tall),
    ]
}

/// KOTOR 2's HUD placement in a 4:3 window (1280 × 960), at 16:9 (1920 ×
/// 1080) and at 21:9 (2560 × 1080), with KOTOR 2's own pieces and one made
/// up to span the window's middle. Each piece in a corner or on an edge
/// stands as far from its sides as in the layout, times the window's height
/// over 600. Every square picture stays square, and a click on a
/// companion's portrait picks that companion. The spanning picture keeps
/// its shape only with kjk's change ("Maintain image aspect ratio"):
/// KOTOR 2's own placement widens it on a wide window.
pub(super) fn wide_windows() -> Vec<Check> {
    const SPANNING: &str = "LBL_SPANNING";
    let layout = root(
        800,
        600,
        [
            ("LBL_BACK1", [704, 520, 78, 78]),
            ("LBL_CHAR1", [707, 523, 72, 72]),
            ("LBL_CHAR2", [752, 484, 32, 32]),
            ("BTN_ACTION0", [12, 532, 36, 60]),
            ("LBL_MAPBORDER", [3, -9, 148, 148]),
            ("BTN_OPT", [763, 8, 28, 28]),
            (SPANNING, [360, 100, 100, 100]),
        ]
        .map(|(tag, rect)| button(tag, rect, b""))
        .into(),
    );
    let vm = ViewModel {
        hud: Some(HudView::default()),
        party: vec![
            MemberView::default(),
            MemberView {
                id: 7,
                ..MemberView::default()
            },
        ],
        ..ViewModel::default()
    };
    let mut faults = Vec::new();
    for window in [(1280.0, 960.0), (1920.0, 1080.0), (2560.0, 1080.0)] {
        for pictures in [true, false] {
            let mut gui = layout.clone();
            let stretch = place(&mut gui, window, pictures);
            let wide = window.0 / window.1 > 4.0 / 3.0;
            for read in &layout.controls {
                let Some(placed) = gui.find(&read.tag).map(|control| control.extent) else {
                    faults.push(format!("{window:?}: no {}", read.tag));
                    continue;
                };
                let want = gaps(read.extent, layout.frame()).map(|gap| gap as f32 * stretch);
                let got = gaps(placed, gui.frame());
                if want
                    .iter()
                    .zip(got)
                    .any(|(want, got)| (want - got as f32).abs() > 1.0)
                {
                    faults.push(format!(
                        "{window:?} {}: gaps {got:?}, not {want:?}",
                        read.tag
                    ));
                }
                let widened = read.tag == SPANNING && wide && !pictures;
                let square = (placed.width - placed.height).abs() <= 1;
                let picture = read.extent.width == read.extent.height;
                if picture && square == widened {
                    let shape = if widened { "square" } else { "out of shape" };
                    faults.push(format!("{window:?} {}: {shape}, {placed:?}", read.tag));
                }
            }
            let companion = gui
                .find("LBL_CHAR2")
                .map(crate::paint::rect_of)
                .map(|[left, top, width, height]| [left + width / 2.0, top + height / 2.0]);
            let picked = companion.and_then(|at| portrait_at(&gui, &vm, at));
            if picked != Some(7) {
                faults.push(format!(
                    "{window:?}: a click on the portrait picked {picked:?}"
                ));
            }
        }
    }
    vec![check(
        "hud_wide_windows",
        expect(&format!("{faults:?}"), faults.is_empty()).map(|()| {
            "corner pieces at their corners and pictures in shape at 4:3, 16:9 and 21:9; the portrait clicks".into()
        }),
    )]
}

/// The effect icons on the portraits (`effect_icons`): stacked about KOTOR
/// 2's arrow labels, two thirds of an icon apart while that fits (a companion's
/// closing up to stay in its portrait), at most 5 on a companion, the good
/// flush left and the bad flush right; none on a debilitated member, whose
/// overlay shows instead.
pub(super) fn effect_icons_stack() -> Vec<Check> {
    let gui = root(
        800,
        600,
        [
            ("LBL_CHAR1", [707, 523, 72, 72]),
            ("LBL_CMBTEFCTINC1", [712, 550, 18, 18]),
            ("LBL_CMBTEFCTRED1", [756, 550, 18, 18]),
            ("LBL_CHAR2", [752, 484, 32, 32]),
            ("LBL_CMBTEFCTINC2", [753, 495, 10, 10]),
            ("LBL_CHAR3", [752, 445, 32, 32]),
            ("LBL_CMBTEFCTINC3", [753, 456, 10, 10]),
        ]
        .map(|(tag, rect)| button(tag, rect, b""))
        .into(),
    );
    let member = |(good, bad): (usize, usize), debilitated: bool| MemberView {
        effects: [(good, "ip_mind", true), (bad, "ip_slow", false)]
            .into_iter()
            .flat_map(|(count, icon, good)| {
                std::iter::repeat_n(
                    EffectIcon {
                        icon: icon.into(),
                        good,
                    },
                    count,
                )
            })
            .collect(),
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
    let tops = |icon: &str, left: f32| {
        effect_icons(&gui, &vm)
            .into_iter()
            .filter(|stacked| stacked.icon == icon && stacked.rect[0] == left)
            .map(|stacked| stacked.rect[1] as i32)
            .collect::<Vec<_>>()
    };
    let stacks = (
        tops("ip_mind", 712.0),
        tops("ip_slow", 760.0),
        tops("ip_mind", 753.0),
    );
    let overlay = ["LBL_DEBILATATED1", "LBL_DEBILATATED3"]
        .map(|tag| !state(&button(tag, [0, 0, 1, 1], b""), &vm).hidden);
    let labels_hidden = state(&button("LBL_CMBTEFCTINC1", [0, 0, 1, 1], b""), &vm).hidden;
    vec![check(
        "hud_effect_icons",
        expect(
            &format!("stacks {stacks:?}, overlays {overlay:?}"),
            stacks
                == (
                    vec![543, 552, 561],
                    vec![552],
                    vec![486, 491, 496, 501, 506],
                )
                && overlay == [false, true]
                && labels_hidden,
        )
        .map(|()| "good up and bad down, stacked, capped, none when debilitated".into()),
    )]
}

/// What the HUD shows of one bar slot: the icon, whether the icon is drawn
/// faded, whether the frame is lit, and whether a key is drawn in the
/// slot's top half.
type SlotShows = (Option<String>, bool, bool, bool);

/// The first four slots of each bar in the action bar probe: the icon, the
/// count, whether the slot is dimmed and whether it is lit. They are a
/// power, medpacs with none left, an empty slot and stealth mode while the
/// character is in stealth.
const BAR_START: [(Option<&str>, Option<u32>, bool, bool); 4] = [
    (Some("ip_push"), None, false, false),
    (Some("iu_medpac"), Some(0), true, false),
    (None, None, false, false),
    (Some("i_mistealth"), None, false, true),
];

/// The action bar on the HUD (`state`, `action_keys`). A controller's bar
/// draws four frames and the keyboard's bar draws six, each with its key.
/// The layout has six slots, and the four slots after them stand to their
/// right at the same step (`bar_extra`). The keyboard's bar draws them only
/// as far as the view lists them: nine slots listed draw nine frames.
/// A power shows its icon. Medpacs with none left show their icon dimmed.
/// An empty slot shows its frame and its key only. The
/// stealth mode slot is lit while the character is in stealth. A slot key
/// left unbound in Key Mapping draws no key at all.
pub(super) fn action_bar() -> Vec<Check> {
    let controls = (0..6)
        .flat_map(|n| {
            let left = 100 + 40 * n;
            [
                button(&format!("BTN_ACTION{n}"), [left, 500, 36, 60], b""),
                button(&format!("LBL_ACTION{n}"), [left + 3, 515, 30, 30], b""),
            ]
        })
        .collect();
    let mut gui = root(800, 600, controls);
    bar_extra(&mut gui, 6, [false; 2]);
    let misplaced: Vec<String> = (0..10)
        .flat_map(|n| {
            [("BTN_ACTION", 100), ("LBL_ACTION", 103)].map(|(prefix, first)| {
                let tag = format!("{prefix}{n}");
                let left = gui.find(&tag).map(|control| control.extent.left);
                (left != Some(first + 40 * n)).then(|| format!("{tag} at {left:?}"))
            })
        })
        .flatten()
        .collect();
    let bar = |keys: Vec<KeyLabel>| -> Vec<BarSlot> {
        keys.into_iter()
            .enumerate()
            .map(|(n, key)| {
                let (icon, count, dim, lit) = BAR_START
                    .get(n)
                    .copied()
                    .unwrap_or((None, None, false, false));
                BarSlot {
                    icon: icon.map(String::from),
                    count,
                    key,
                    dim,
                    lit,
                    kinds: 0,
                }
            })
            .collect()
    };
    let pad = bar(["a", "b", "x", "y"]
        .map(|button| KeyLabel {
            words: Vec::new(),
            badge: Some(format!("kjkpad_deck_rb+{button}")),
        })
        .to_vec());
    // The probe's font has glyphs for the bytes 0 to 3 alone, so each key's
    // words are one of those bytes.
    let keys = |slots: u8| {
        bar((0..slots)
            .map(|n| KeyLabel {
                words: vec![n % 4],
                badge: None,
            })
            .collect())
    };
    let keyboard = keys(6);
    let mut unbound = keyboard.clone();
    unbound[5].key = KeyLabel::default();
    let shows = |bar: &[BarSlot]| -> Vec<SlotShows> {
        let vm = ViewModel {
            hud: Some(HudView {
                bar: bar.to_vec(),
                ..HudView::default()
            }),
            ..ViewModel::default()
        };
        let mut fake = Fake::new();
        let mut out = DrawList::default();
        action_keys(
            &gui,
            &vm,
            &mut Painter::new(&mut fake, Palette::K1),
            &mut out,
        );
        (0..10)
            .filter_map(|n| {
                let frame = gui.find(&format!("BTN_ACTION{n}"))?;
                let framed = state(frame, &vm);
                let label = gui.find(&format!("LBL_ACTION{n}"))?;
                let icon = state(label, &vm);
                // A dimmed icon is drawn, and drawn faded and grey.
                let mut painted = DrawList::default();
                Painter::new(&mut fake, Palette::K1).control(label, &icon, &mut painted);
                let faded = !painted.quads.is_empty()
                    && painted
                        .quads
                        .iter()
                        .all(|quad| quad.color[3] < 1.0 && quad.grey);
                let [x, y, w, h] = rect_of(frame);
                let key = out.quads.iter().any(|quad| {
                    let [qx, qy, ..] = quad.rect;
                    qx >= x - 2.0 && qx < x + w + 2.0 && qy >= y && qy < y + h / 2.0
                });
                (!framed.hidden).then_some((icon.fill, faded, framed.selected, key))
            })
            .collect()
    };
    // Each of the first four slots shows its icon, dimmed and lit as it
    // says, and its key. The slots after them are empty, and `empty` says
    // whether each draws a key.
    let wanted = |empty: &[bool]| -> Vec<SlotShows> {
        BAR_START
            .iter()
            .map(|(icon, _, dim, lit)| (icon.map(String::from), *dim, *lit, true))
            .chain(empty.iter().map(|key| (None, false, false, *key)))
            .collect()
    };
    let cases = [
        ("a controller", shows(&pad), wanted(&[])),
        ("the keyboard", shows(&keyboard), wanted(&[true, true])),
        (
            "the keyboard with slot 6 unbound",
            shows(&unbound),
            wanted(&[true, false]),
        ),
        (
            "the keyboard with nine slots listed",
            shows(&keys(9)),
            wanted(&[true; 5]),
        ),
    ];
    let wrong: Vec<String> = cases
        .iter()
        .filter(|(_, seen, want)| seen != want)
        .map(|(name, seen, _)| format!("{name}: {seen:?}"))
        .collect();
    vec![check(
        "hud_action_bar",
        expect(
            &format!("wrong {wrong:?}, misplaced {misplaced:?}"),
            wrong.is_empty() && misplaced.is_empty(),
        )
        .map(|()| {
            "four frames on a controller, six on the keyboard and nine with nine slots listed, each with its key, the icons dimmed and lit".into()
        }),
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
    // The frame is 1280 × 720 for a 1280 × 720 window, and 800 × 800 for a
    // square window of that size. A window shorter than the layout, 1024 ×
    // 576, scales the frame down to 600 tall and 1067 wide. At 1280 × 720
    // the layout is stretched 1.6 times across and 1.2 times down, and then
    // each piece is narrowed by 0.75 towards its side.
    let mut gui = layout.clone();
    place(&mut gui, (1280.0, 720.0), true);
    to_top(&mut gui);
    let mut small = layout.clone();
    place(&mut small, (1024.0, 576.0), true);
    let mut square = layout;
    place(&mut square, (800.0, 800.0), true);
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
    // How far the dot's quads reach left, up, right and down of the middle
    // of the 1280 by 720 frame, and whether its top band is narrower than
    // the dot is wide.
    let dot = crosshair(&gui, 1.0, Palette::K1.base);
    let reach = dot.iter().fold([0.0f32; 4], |reach, quad| {
        let [left, top, width, height] = quad.rect;
        [
            reach[0].max(640.0 - left),
            reach[1].max(360.0 - top),
            reach[2].max(left + width - 640.0),
            reach[3].max(top + height - 360.0),
        ]
    });
    let round = dot
        .first()
        .is_some_and(|band| band.rect[2] < reach[0] + reach[2]);
    let vm = ViewModel {
        hud: Some(HudView {
            hp: (30, 120),
            fp: (0, 0),
            portrait: Some("po_pmhc1".into()),
            bar: [("ip_push", None, b"1"), ("iu_medpac", Some(3), b"2")]
                .map(|(icon, count, words)| BarSlot {
                    icon: Some(icon.into()),
                    count,
                    key: KeyLabel {
                        words: words.to_vec(),
                        badge: None,
                    },
                    ..BarSlot::default()
                })
                .to_vec(),
            bar_label: None,
            lit: vec!["LBL_PLOTXP"],
            minimap: None,
            stealth: false,
            solo: false,
            form: None,
            feat: None,
            busy: None,
            poisoned: false,
            duel: None,
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
    // Solo mode: KOTOR's solo toggle lit.
    let with = |stealth: bool, solo: bool| ViewModel {
        hud: vm.hud.clone().map(|hud| HudView {
            stealth,
            solo,
            ..hud
        }),
        ..vm.clone()
    };
    let (stealthed, solo) = (with(true, false), with(false, true));
    let toggle = |vm: &ViewModel, tag: &str| state(&button(tag, [0, 0, 1, 1], b""), vm);
    let stealth = (
        toggle(&vm, "TB_STEALTH").hidden,
        toggle(&stealthed, "TB_STEALTH").hidden,
        toggle(&stealthed, "TB_STEALTH").selected,
        toggle(&stealthed, "TB_PAUSE").hidden,
        toggle(&stealthed, "TB_SOLO").hidden,
        toggle(&solo, "TB_SOLO").hidden,
        toggle(&solo, "TB_SOLO").selected,
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
        portrait_at(&gui, &with_trask, [100.0, 680.0]),
        portrait_at(&gui, &with_trask, [138.0, 680.0]),
        portrait_at(&gui, &vm, [100.0, 680.0]),
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
        vec![(Some("gui_mp_talkd".into()), [108.0, 648.0, 24.0, 24.0])],
        vec![(Some("pcards_pos_x".into()), [42.0, 648.0, 30.0, 30.0])],
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
        rect: [168.0, 648.0, 24.0, 24.0],
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
        poisoned: false,
        actions: vec![
            action("isk_security", b"Security"),
            action("i_openplace", b"Open"),
        ],
        selected: 1,
        busy: false,
        progress: None,
        point: None,
        cycle: None,
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
        poisoned: false,
        actions: vec![action("isk_treatinj", b"Treat Injury")],
        selected: 0,
        busy: false,
        progress: None,
        point: None,
        cycle: None,
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
        watch_at(&gui, &at_table, [120.0, 660.0]),
        watch_at(&gui, &at_table, [95.0, 690.0]),
        watch_at(&gui, &busy_with(None, Some(Busy::Talking)), [120.0, 660.0]),
        watch_at(&gui, &busy_with(Some(Busy::Pazaak), None), [55.0, 660.0]),
    );
    let watch_click = check(
        "hud_pazaak_watch",
        expect(
            &format!("the icon's member, then none: {pazaak_clicks:?}"),
            pazaak_clicks == (Some(42), None, None, None),
        )
        .map(|()| format!("{pazaak_clicks:?}")),
    );
    // A poison running (both executables): the portrait's vitality bar
    // green over their own red, a companion's alike; the target's bar
    // drawn with the poisoned bar's art, not tinted.
    let sick = ViewModel {
        hud: vm.hud.clone().map(|hud| HudView {
            poisoned: true,
            ..hud
        }),
        party: vec![
            MemberView::default(),
            MemberView {
                poisoned: true,
                ..MemberView::default()
            },
        ],
        target: Some(TargetView {
            hp: Some((10, 40)),
            hostile: true,
            poisoned: true,
            ..TargetView::default()
        }),
        ..vm.clone()
    };
    let bar = |vm: &ViewModel, tag: &str| block_shows(vm, tag).bar;
    let poison = (
        bar(&vm, "PB_VIT1").1,
        bar(&sick, "PB_VIT1").1,
        bar(&sick, "PB_VIT2").1,
        bar(&sick, "PB_HEALTH"),
        block_shows(&sick, "PB_HEALTH").tint,
    );
    let poison_bars = check(
        "hud_poison",
        expect(
            &format!("red, then green; the target's poison_bar untinted: {poison:?}"),
            poison
                == (
                    Some([0.74, 0.11, 0.0]),
                    Some([0.28, 0.92, 0.11]),
                    Some([0.28, 0.92, 0.11]),
                    (Some("poison_bar".into()), None),
                    None,
                ),
        )
        .map(|()| format!("{poison:?}")),
    );
    let hud = check("hud", {
        expect(
            &format!("the frame the window, or its shape when smaller: {frame:?}"),
            frame == ((1280, 720), (1067, 600)),
        )
        .and_then(|()| {
            expect(
                &format!("each control stretched, then narrowed towards its side as KOTOR 2 does: {placed:?}"),
                placed
                    == [
                        Some((1160, 12)),
                        Some((12, 648)),
                        Some((1184, 648)),
                        Some((360, 120)),
                    ],
            )
        })
        .and_then(|()| {
            expect(
                &format!("a window narrower than 4:3 widens each piece to its shape, kept to its side: {lower:?}"),
                lower == ((800, 800), Some((13, 720)), Some((667, 13))),
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
                &format!(
                    "the stealth toggle lit only in stealth mode, the solo toggle only in solo mode: {stealth:?}"
                ),
                stealth == (true, false, true, true, true, false, true),
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
                block == (Some((520, 5)), Some((572, 46)), Some((501, 119))),
            )
        })
        .and_then(|()| {
            expect(
                &format!(
                    "the round dot at the middle, as far left as right and as far up as down: reach {reach:?}, round {round}"
                ),
                // At 720 high with one physical pixel to a layout pixel,
                // the dot is 2 pixels in radius and its edge 1 more.
                round && reach == [3.0; 4],
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
        .map(|()| "KOTOR 2's placement, bars, slots, marks, companions and their clicks, the target, Revive and its bar".into())
    });
    vec![hud, watch_click, poison_bars]
}

/// KOTOR 2's floating target block (`float_at`, `floated`), for a block 200
/// × 95 in a 1920 × 1080 frame whose area runs from (3, 40) 1914 wide and
/// 1000 tall, 32 pixels kept from the point. In front of the camera the
/// block is centred on the point with its bottom 32 over it, against a side
/// it would cross, at the area's top for a point near the top, and 32 over
/// the area's bottom for a point near the bottom. Behind the camera it
/// stands against the side away from the mirrored point, and with the point
/// in the frame's upper half its bottom as far over the frame's bottom as
/// the point stands 32 under the frame's top. On a layout, the block's
/// controls move by where it goes, the area comes from the arrow's margin,
/// the menu buttons' backing and the pause button, and the block is as tall
/// as its first action's button, or as its vitality bar without actions.
pub(super) fn floating_block() -> Vec<Check> {
    let area = Rect {
        left: 3,
        top: 40,
        width: 1914,
        height: 1000,
    };
    let spot = |x: f32, y: f32, behind: bool| {
        float_at((200, 95), ([x, y], behind), area, (1920, 1080), 32.0)
    };
    let front = [
        spot(960.0, 500.0, false),
        spot(50.0, 500.0, false),
        spot(1900.0, 500.0, false),
        spot(960.0, 50.0, false),
        spot(960.0, 1030.0, false),
    ];
    let behind = [
        spot(300.0, 300.0, true),
        spot(1500.0, 800.0, true),
        spot(300.0, 50.0, true),
    ];
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
                button("LBL_NAME", [0, 1, 200, 26], b""),
                button("PB_HEALTH", [0, 27, 200, 6], b""),
                button("BTN_TARGET0", [43, 35, 36, 60], b""),
                button("LBL_ARROW_MARGIN", [3, 129, 794, 333], b""),
                button("LBL_MENUBG", [536, 4, 260, 36], b""),
                button("TB_PAUSE", [656, 562, 36, 36], b""),
            ],
        )
    };
    let target = TargetView {
        actions: vec![TargetAction::default()],
        point: Some(crate::vm::TargetPoint {
            at: [0.5, 0.5],
            behind: false,
        }),
        ..TargetView::default()
    };
    let floats = |target: &TargetView| {
        let mut gui = layout.clone();
        floated(&mut gui, target, 1.0, &mut FloatHold::default());
        at(&gui, "LBL_NAME")
    };
    let acting = floats(&target);
    let busy = floats(&TargetView {
        busy: true,
        ..target.clone()
    });
    let unknown = floats(&TargetView {
        point: None,
        ..target.clone()
    });
    // The pointer over the name holds the block while the target moves to
    // the left edge; the block keeps its bottom when its button goes; it
    // lingers half a second after the pointer leaves, then follows.
    let mut hold = FloatHold::default();
    let mut step = |target: &TargetView, pointer: Option<[f32; 2]>, seconds: f32| {
        hold.tick(seconds);
        let mut gui = layout.clone();
        floated(&mut gui, target, 1.0, &mut hold);
        hold.point(Some((&gui, target)), pointer);
        at(&gui, "LBL_NAME")
    };
    let moved = TargetView {
        point: Some(crate::vm::TargetPoint {
            at: [0.0, 0.5],
            behind: false,
        }),
        ..target.clone()
    };
    let held = [
        step(&target, Some([310.0, 180.0]), 0.0),
        step(&moved, Some([310.0, 180.0]), 0.1),
        step(
            &TargetView {
                busy: true,
                ..moved.clone()
            },
            Some([310.0, 180.0]),
            0.1,
        ),
        step(&moved, None, 0.25),
        step(&moved, None, 0.125),
        step(&moved, None, 0.125),
    ];
    let holds = held
        == [
            Some((300, 174)),
            Some((300, 174)),
            Some((300, 236)),
            Some((300, 174)),
            Some((300, 174)),
            Some((3, 174)),
        ];
    vec![
        check(
            "hud_target_block_holds_under_pointer",
            expect(&format!("frame by frame {held:?}"), holds)
                .map(|()| "held under the pointer, then half a second".into()),
        ),
        check(
        "hud_target_block_floats_as_kotor2",
        expect(
            &format!(
                "in front {front:?}; behind {behind:?}; on a layout {acting:?}, busy {busy:?}, no point {unknown:?}"
            ),
            front == [(860, 373), (3, 373), (1717, 373), (860, 40), (860, 913)]
                && behind == [(1717, 653), (3, 673), (1717, 913)]
                && acting == Some((300, 174))
                && busy == Some((300, 236))
                && unknown == Some((3, 41)),
        )
        .map(|()| format!("centred over the point at {:?}", front[0])),
    )]
}
