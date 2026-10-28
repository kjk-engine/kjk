//! The minimap: an area's map read from its ARE (the Endar Spire's
//! `m01aa` numbers), world points onto its picture, the arrow's turn for
//! each north, and the HUD's picture and arrow placed in `LBL_MAPVIEW`.

use std::f32::consts::{FRAC_PI_2, PI};

use asset_odyssey::map::AreaMap;
use fmt_odyssey::gff::{GffField, GffStruct, GffValue};
use fmt_odyssey::gui::{Border, Control, Gui};

use super::menus::root;
use super::paint::{Fake, button};
use super::{Check, check, expect, near};
use crate::draw::Quad;
use crate::screens::hud::{Minimap, minimap, state};
use crate::skin::Source;
use crate::vm::{FogView, HudView, MinimapView, ViewModel};

/// An ARE's root with a `Map` of `north` and the Endar Spire's points,
/// `world` in place of its second world point.
pub(super) fn are(north: i32, world: [f32; 2]) -> GffStruct {
    let field = |label: &str, value: GffValue| GffField {
        label: label.as_bytes().to_vec(),
        value,
    };
    let float = |label: &str, value: f32| field(label, GffValue::Float(value.to_bits()));
    let map = GffStruct {
        id: 0,
        fields: vec![
            field("NorthAxis", GffValue::Int(north)),
            field("MapZoom", GffValue::Int(1)),
            field("MapResX", GffValue::Int(15)),
            float("MapPt1X", 0.0818),
            float("MapPt1Y", 0.3259),
            float("MapPt2X", 0.9364),
            float("MapPt2Y", 0.5074),
            float("WorldPt1X", 15.4),
            float("WorldPt1Y", 16.8),
            float("WorldPt2X", world[0]),
            float("WorldPt2Y", world[1]),
        ],
    };
    GffStruct {
        id: u32::MAX,
        fields: vec![field("Map", GffValue::Struct(Box::new(map)))],
    }
}

/// The Endar Spire's second world point.
pub(super) const SECOND: [f32; 2] = [34.4, 158.0];

fn close(a: [f32; 2], b: [f32; 2]) -> bool {
    near(a[0], b[0]) && near(a[1], b[1])
}

pub(super) fn all() -> Vec<Check> {
    vec![points(), arrows(), placed()]
}

/// m01aa's north is −X (3): world Y runs across the picture, X down; read
/// with north +Y (0) the same numbers run X across. Each world point lands
/// on its map point; a north past 3 or two points sharing a coordinate fix
/// no map.
fn points() -> Check {
    let spire = AreaMap::read(&are(3, SECOND));
    let upright = AreaMap::read(&are(0, SECOND));
    let corner = [34.4, 16.8];
    let found = spire.zip(upright).map(|(spire, upright)| {
        (
            [spire.point([15.4, 16.8]), spire.point(SECOND)],
            spire.point([24.9, 87.4]),
            spire.point(corner),
            upright.point(corner),
            (spire.zoom, spire.res_x),
        )
    });
    let bare = GffStruct {
        id: u32::MAX,
        fields: Vec::new(),
    };
    let refused = [are(4, SECOND), are(3, [15.4, 158.0]), bare]
        .iter()
        .all(|root| AreaMap::read(root).is_none());
    check(
        "minimap_points",
        match found {
            None => Err("m01aa's map did not read".into()),
            Some((ends, middle, across, upright, sizes)) => expect(
                &format!("the world points on the map points: {ends:?}"),
                close(ends[0], [0.0818, 0.3259]) && close(ends[1], [0.9364, 0.5074]),
            )
            .and(expect(
                &format!("halfway between them halfway: {middle:?}"),
                close(middle, [0.5091, 0.41665]),
            ))
            .and(expect(
                &format!("north −X: world Y across, X down: {across:?}"),
                close(across, [0.0818, 0.5074]),
            ))
            .and(expect(
                &format!("north +Y: world X across, Y down: {upright:?}"),
                close(upright, [0.9364, 0.3259]),
            ))
            .and(expect(
                &format!("zoom, resolution {sizes:?}"),
                sizes == (1, 15),
            ))
            .and(expect(
                "no map from a bad north, shared points or none",
                refused,
            ))
            .map(|()| "m01aa's points, both orientations, refusals".into()),
        },
    )
}

/// Facing world +Y the arrow turns, clockwise: none with north +Y, half a
/// turn with −Y, three quarters with +X, a quarter with −X (m01aa: +Y is
/// the picture's right); facing −X on m01aa it points up.
fn arrows() -> Check {
    let spins: Vec<Option<f32>> = (0..4)
        .map(|north| AreaMap::read(&are(north, SECOND)).map(|map| map.arrow(FRAC_PI_2)))
        .collect();
    let up = AreaMap::read(&are(3, SECOND)).map(|map| map.arrow(PI));
    let wanted = [0.0, PI, 3.0 * FRAC_PI_2, FRAC_PI_2];
    let matches = spins
        .iter()
        .zip(wanted)
        .all(|(spin, wanted)| spin.is_some_and(|spin| near(spin, wanted)));
    check(
        "minimap_arrow",
        expect(&format!("facing +Y, by north: {spins:?}"), matches)
            .and(expect(
                &format!("facing −X on m01aa: {up:?}"),
                up.is_some_and(|spin| near(spin, 0.0)),
            ))
            .map(|()| "a quarter turn a north, clockwise".into()),
    )
}

/// KOTOR 2's view (120 square at 17, 5) and arrow (20 square): the
/// picture (16 square here) moved so the player's point, a quarter across
/// and half down, is at the view's centre (77, 65), clipped to the view;
/// KOTOR's soft dot over each unexplored cell (of 2 by 2, 4 by 8, over its
/// used left half, all but the top left), four cells wide, centred on it;
/// the
/// arrow at its size there, turned. No picture, no minimap. The border
/// shows only with a minimap; the engine's own picture holder, the
/// layout's arrow and the minimap button stay hidden.
fn placed() -> Check {
    let arrow = Control {
        border: Some(Border {
            fill: "mm_barrow_p".into(),
            ..Border::default()
        }),
        ..button("LBL_ARROW", [58, 58, 20, 20], b"")
    };
    let gui: Gui = root(
        640,
        480,
        vec![button("LBL_MAPVIEW", [17, 5, 120, 120], b""), arrow],
    );
    let view = MinimapView {
        picture: "lbl_mapm01aa".into(),
        at: [0.25, 0.5],
        spin: 1.0,
        used: 0.5,
        // Of 2 by 2 cells over the picture's left half, the top left
        // explored.
        fog: Some(FogView {
            cells: [2, 2],
            explored: vec![true, false, false, false],
        }),
    };
    let mut textures = Fake::new();
    let shown = minimap(&gui, &view, &mut textures, Source::Skin);
    let missing = minimap(
        &gui,
        &MinimapView {
            picture: "missing_map".into(),
            ..view.clone()
        },
        &mut textures,
        Source::Skin,
    );
    let rects = shown.map(
        |Minimap {
             backing,
             picture,
             fog,
             arrow,
         }| {
            (
                backing.rect,
                picture.rect,
                picture.clip,
                picture.spin,
                fog.iter()
                    .map(|quad| (quad.rect, quad.clip))
                    .collect::<Vec<_>>(),
                arrow.map(|arrow: Quad| (arrow.rect, arrow.spin)),
            )
        },
    );
    let view_rect = Some([17.0, 5.0, 120.0, 120.0]);
    let wanted = (
        [17.0, 5.0, 120.0, 120.0],
        [73.0, 57.0, 16.0, 16.0],
        view_rect,
        0.0,
        vec![
            ([71.0, 45.0, 16.0, 32.0], view_rect),
            ([67.0, 53.0, 16.0, 32.0], view_rect),
            ([71.0, 53.0, 16.0, 32.0], view_rect),
        ],
        Some(([67.0, 55.0, 20.0, 20.0], 1.0)),
    );
    let vm = |minimap: Option<MinimapView>| ViewModel {
        hud: Some(HudView {
            minimap,
            ..HudView::default()
        }),
        ..ViewModel::default()
    };
    let (with, without) = (vm(Some(view)), vm(None));
    let hidden = |tag: &str, vm: &ViewModel| state(&button(tag, [0, 0, 1, 1], b""), vm).hidden;
    let border = (
        hidden("LBL_MAPBORDER", &with),
        hidden("LBL_MAPBORDER", &without),
    );
    let quiet = [
        "LBL_MAP",
        "LBL_MAPVIEW",
        "LBL_ARROW",
        "LBL_ARROW_MARGIN",
        "BTN_MINIMAP",
    ]
    .iter()
    .all(|tag| hidden(tag, &with));
    check(
        "minimap_quads",
        expect(
            &format!(
                "black over the view, the picture centred on the player, clipped, a dot four cells wide over each unexplored cell; the arrow turned: {rects:?}"
            ),
            rects == Some(wanted),
        )
        .and(expect("no picture, no minimap", missing.is_none()))
        .and(expect(
            &format!("the border with a minimap only: {border:?}"),
            border == (false, true),
        ))
        .and(expect("the layout's own map controls hidden", quiet))
        .map(|()| "placed, clipped, fogged, turned; the border with it".into()),
    )
}
