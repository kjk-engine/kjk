//! The map page: the picture's used part fitted to `LBL_Map`, the notes
//! where the area's map puts their waypoints (the Endar Spire's `m01aa`
//! numbers), the chosen one larger and lit, its words in `LBL_MapNote`,
//! `BTN_UP` and `BTN_DOWN` choosing round, the arrow placed and turned.

use asset_odyssey::map::AreaMap;
use fmt_odyssey::gui::Gui;

use super::menus::root;
use super::minimap::{SECOND, are};
use super::paint::{Fake, button};
use super::{Check, check, expect, near};
use crate::draw::Quad;
use crate::paint::{Painter, Palette};
use crate::screens::map::Map;
use crate::screens::tab::Page;
use crate::vm::{FogView, MapNoteView, MapView, ViewModel};

pub(super) fn all() -> Vec<Check> {
    vec![placed(), chosen(), return_button()]
}

/// K1's page: `LBL_Map` 440 by 256 at (100, 100).
fn layout() -> Gui {
    root(
        640,
        480,
        vec![
            button("LBL_Map", [100, 100, 440, 256], b""),
            button("LBL_MapNote", [90, 376, 460, 33], b"Map Note"),
            button("BTN_UP", [70, 382, 20, 20], b""),
            button("BTN_DOWN", [551, 382, 20, 20], b""),
        ],
    )
}

/// The page's view with notes at `notes` (`Bridge`, `Quarters`, …).
fn vm(notes: &[([f32; 2], &str)]) -> ViewModel {
    ViewModel {
        map: Some(MapView {
            area: b"Command Deck".to_vec(),
            picture: Some("lbl_mapm01aa".into()),
            used: 440.0 / 512.0,
            player: Some(([0.5, 0.5], 1.0)),
            party: vec![[0.1, 0.9]],
            notes: notes
                .iter()
                .map(|(at, text)| MapNoteView {
                    at: *at,
                    text: text.as_bytes().to_vec(),
                })
                .collect(),
            // The left half unexplored.
            fog: Some(FogView {
                cells: [2, 1],
                explored: vec![false, true],
            }),
            return_button: None,
            return_hidden: false,
        }),
        ..ViewModel::default()
    }
}

fn quads(page: &Map, vm: &ViewModel) -> Vec<Quad> {
    let mut textures = Fake::new();
    let mut painter = Painter::new(&mut textures, Palette::K1);
    page.quads(&layout(), vm, &mut painter)
}

/// A quad's centre and side.
fn centre(quad: &Quad) -> ([f32; 2], f32) {
    let [left, top, width, _] = quad.rect;
    ([left + width / 2.0, top + quad.rect[3] / 2.0], width)
}

fn at(quad: &Quad, wanted: [f32; 2]) -> bool {
    let ([x, y], _) = centre(quad);
    near(x, wanted[0]) && near(y, wanted[1])
}

/// The probe's picture (16 square) uses 440/512 of its width: fitted to
/// 440 by 256 it is 220 by 256, centred (210 to 430). A note at the first
/// world point lands on its map point; the player's arrow at the middle,
/// turned; the party's circle at its place; black under it all, and over
/// the picture's unexplored left half (one cell, 110 by 256) KOTOR's soft
/// dot, four cells wide, centred on it.
fn placed() -> Check {
    let spire = AreaMap::read(&are(3, SECOND));
    let Some(spire) = spire else {
        return check("map_page_placed", Err("m01aa's map did not read".into()));
    };
    let bridge = spire.point([15.4, 16.8]);
    let drawn = quads(&Map::default(), &vm(&[(bridge, "Bridge")]));
    let rect = [210.0, 100.0, 220.0, 256.0];
    let on = |at: [f32; 2]| [rect[0] + at[0] * rect[2], rect[1] + at[1] * rect[3]];
    let shape: Vec<([f32; 4], [f32; 4], f32)> = drawn
        .iter()
        .map(|quad| (quad.rect, quad.uv, quad.spin))
        .collect();
    let laid = drawn.len() == 6
        && drawn[0].rect == [100.0, 100.0, 440.0, 256.0]
        && drawn[1].rect == rect
        && near(drawn[1].uv[2], 440.0 / 512.0)
        && drawn[2].rect == [45.0, -284.0, 440.0, 1024.0]
        && drawn[2].color == [1.0; 4]
        && at(&drawn[3], on([0.1, 0.9]))
        && at(&drawn[4], on(bridge))
        && at(&drawn[5], on([0.5, 0.5]))
        && drawn[5].spin == 1.0
        && drawn[2..].iter().all(|quad| quad.clip == Some(rect));
    check(
        "map_page_placed",
        expect(
            &format!(
                "black, the picture fitted, a dot over its unexplored half, the circle, the note on m01aa's first map point {bridge:?}, the arrow turned: {shape:?}"
            ),
            laid,
        )
        .map(|()| "picture fitted, fog, note by the area's map, arrow turned".into()),
    )
}

/// Three notes: the first chosen, half again as large and lit, its words
/// shown; Down chooses the next, Up from the first the last; a click on a
/// note's mark chooses it, one beside every mark nothing; one note greys
/// both.
fn chosen() -> Check {
    let notes = [
        ([0.2, 0.3], "Bridge"),
        ([0.5, 0.4], "Escape Pod Access"),
        ([0.8, 0.5], "Quarters"),
    ];
    let shown = vm(&notes);
    let mut page = Map::default();
    let note_of = |page: &Map| {
        page.state(&button("LBL_MapNote", [0, 0, 1, 1], b""), &shown)
            .text
            .map(|text| String::from_utf8_lossy(&text).into_owned())
    };
    // The notes' quads: after black, the picture, its fog and the circle.
    let marks = |page: &Map| -> Vec<(f32, [f32; 3])> {
        quads(page, &shown)[4..7]
            .iter()
            .map(|quad| {
                (
                    centre(quad).1,
                    [quad.color[0], quad.color[1], quad.color[2]],
                )
            })
            .collect()
    };
    let first = (note_of(&page), marks(&page));
    let mut out = Vec::new();
    page.press("BTN_DOWN", &shown, &mut out);
    let down = note_of(&page);
    page.press("BTN_UP", &shown, &mut out);
    page.press("BTN_UP", &shown, &mut out);
    let round = note_of(&page);
    // The second note's mark is at (320, 202.4) on the fitted picture.
    let mut textures = Fake::new();
    page.click(&layout(), [323.0, 205.0], &mut textures, &shown);
    let clicked = note_of(&page);
    page.click(&layout(), [400.0, 300.0], &mut textures, &shown);
    let beside = note_of(&page);
    let lone = vm(&notes[..1]);
    let greyed = ["BTN_UP", "BTN_DOWN"]
        .iter()
        .all(|tag| page.state(&button(tag, [0, 0, 1, 1], b""), &lone).disabled);
    let lit = |(side, colour): (f32, [f32; 3]), chosen: bool| {
        let palette = Palette::K1;
        if chosen {
            near(side, 24.0) && colour == palette.hilight
        } else {
            near(side, 16.0) && colour == palette.base
        }
    };
    let (words, sized) = &first;
    check(
        "map_page_notes",
        expect(
            &format!("the first chosen, larger and lit: {first:?}"),
            words.as_deref() == Some("Bridge")
                && sized.len() == 3
                && sized.iter().enumerate().all(|(n, mark)| lit(*mark, n == 0)),
        )
        .and(expect(
            &format!("Down the next: {down:?}"),
            down.as_deref() == Some("Escape Pod Access"),
        ))
        .and(expect(
            &format!("Up from the first the last: {round:?}"),
            round.as_deref() == Some("Quarters"),
        ))
        .and(expect(
            &format!("a click on a mark chooses it, beside them nothing: {clicked:?} {beside:?}"),
            clicked.as_deref() == Some("Escape Pod Access") && beside == clicked,
        ))
        .and(expect("one note greys Up and Down", greyed))
        .map(|()| "chosen larger and lit, its words; Up and Down round; clicked".into()),
    )
}

/// The return button: greyed and unlabelled where no script set it,
/// labelled and greyed where one did, hidden where one turned it off
/// (`SetReturnStrref`'s `bShow`).
fn return_button() -> Check {
    let page = Map::default();
    let state = |label: Option<u32>, hidden: bool| {
        let mut shown = vm(&[]);
        if let Some(map) = shown.map.as_mut() {
            (map.return_button, map.return_hidden) = (label, hidden);
        }
        let state = page.state(&button("BTN_RETURN", [0, 0, 1, 1], b""), &shown);
        (state.hidden, state.disabled, state.strref)
    };
    let seen = [
        state(None, false),
        state(Some(42), false),
        state(Some(42), true),
    ];
    check(
        "map_page_return_button",
        expect(
            &format!("(hidden, greyed, label): {seen:?}"),
            seen[0] == (false, true, None) && seen[1] == (false, true, Some(42)) && seen[2].0,
        )
        .map(|()| "greyed unset, labelled when set, hidden when turned off".into()),
    )
}
