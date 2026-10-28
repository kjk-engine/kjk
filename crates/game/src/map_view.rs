//! The area's map as this player sees it (`docs/UI.md`, "The map"): the
//! HUD's minimap and the map page's view-models, from the area's map (its
//! ARE's, `asset_odyssey::map::AreaMap`) and the snapshot.

use asset_odyssey::map::AreaMap;
use sim::module::{ModuleData, ObjectKind};
use sim::{EntityState, Kind, Seat, Snapshot};
use ui::assets::GuiAssets;
use ui::draw::Textures;
use ui::vm::{FogView, MapNoteView, MapView, MinimapView};

/// The area's picture (`lbl_map<area>`), when the campaign's install has
/// it.
fn picture(assets: &mut GuiAssets, module: &ModuleData) -> Option<String> {
    let picture = format!("lbl_map{}", module.area);
    assets.texture(ui::skin::Source::Campaign, &picture)?;
    Some(picture)
}

/// The share of a map picture's width its map takes: KOTOR 1's the left
/// 440 of their 512 columns (its map page's 440 wide), KOTOR 2's all.
fn used_width(game: &str) -> f32 {
    if game == "k1" { 440.0 / 512.0 } else { 1.0 }
}

/// Where `entity` stands on the map.
fn place(map: &AreaMap, entity: &EntityState) -> [f32; 2] {
    map.point([entity.position[0], entity.position[1]])
}

/// How far `body`'s arrow turns from pointing up: the way it looks.
fn spin(map: &AreaMap, body: &EntityState) -> f32 {
    let yaw = body.player.as_ref().map_or(0.0, |player| player.view[1]);
    map.arrow(yaw.to_radians())
}

/// What the party has explored of `module`'s map in `snapshot`, where the
/// module keeps that (`sim::fog`).
fn fog(module: &ModuleData, snapshot: &Snapshot) -> Option<FogView> {
    let cells = module.map?.cells.map(usize::from);
    let grid = sim::fog::of(&snapshot.explored, &module.name)
        .filter(|grid| grid.cells.map(usize::from) == cells);
    let [columns, rows] = cells;
    Some(FogView {
        cells,
        explored: (0..rows)
            .flat_map(|row| (0..columns).map(move |column| (column, row)))
            .map(|(column, row)| grid.is_some_and(|grid| grid.explored(column, row)))
            .collect(),
    })
}

/// Whether picture point `at` (fractions of the map's part) lies in a cell
/// the party explored; everywhere where no grid is kept.
fn seen(fog: Option<&FogView>, at: [f32; 2]) -> bool {
    let Some(fog) = fog else {
        return true;
    };
    let [columns, rows] = fog.cells;
    let cell = |at: f32, count: usize| {
        let cell = (at * count as f32).floor();
        (0.0..count as f32).contains(&cell).then_some(cell as usize)
    };
    match (cell(at[0], columns), cell(at[1], rows)) {
        (Some(column), Some(row)) => fog.explored.get(row * columns + column) == Some(&true),
        _ => false,
    }
}

/// The minimap, where the area has a map and the campaign's install its
/// picture: `body`'s place on the picture (fractions of the whole
/// texture), the arrow's turn for its view, and what the party explored.
pub(crate) fn minimap(
    assets: &mut GuiAssets,
    map: Option<&AreaMap>,
    module: &ModuleData,
    snapshot: &Snapshot,
    body: &EntityState,
) -> Option<MinimapView> {
    // A jammed map (TSL's `DisableMap`) hides the minimap.
    let map = map.filter(|_| !snapshot.screens.map_disabled)?;
    let [across, down] = place(map, body);
    let used = used_width(&module.game);
    Some(MinimapView {
        picture: picture(assets, module)?,
        at: [across * used, down],
        spin: spin(map, body),
        used,
        fog: fog(module, snapshot),
    })
}

/// The map page for `seat`: the area's name, its picture where it has a
/// map, the seat's body with its arrow, the rest of the party, and the
/// notes the area's waypoints show (as placed, or as scripts pinned them)
/// where the party explored, as KOTOR shows them.
pub(crate) fn page(
    assets: &mut GuiAssets,
    map: Option<&AreaMap>,
    level: &sim::Level,
    snapshot: &Snapshot,
    seat: Seat,
) -> Option<MapView> {
    let module = level.module()?;
    let area = assets
        .template_name(&module.area, fmt_odyssey::ResType::ARE)
        .unwrap_or_else(|| module.area.as_bytes().to_vec());
    let return_button = return_button(&snapshot.screens.transit);
    let return_hidden = return_hidden(&snapshot.screens.transit);
    // A jammed map (TSL's `DisableMap`) shows static in place of the
    // picture [verify: whether KOTOR draws the markers over it].
    if snapshot.screens.map_disabled {
        let picture = assets
            .texture(ui::skin::Source::Campaign, STATIC)
            .map(|_| STATIC.to_string());
        return Some(MapView {
            area,
            picture,
            used: 1.0,
            return_button,
            return_hidden,
            ..MapView::default()
        });
    }
    let Some(map) = map else {
        return Some(MapView {
            area,
            return_button,
            return_hidden,
            ..MapView::default()
        });
    };
    let body = snapshot.body(seat);
    let instances = sim::Instances::in_snapshot(level, snapshot);
    let fog = fog(module, snapshot);
    let notes = snapshot
        .entities
        .iter()
        .filter(|entity| matches!(entity.kind, Kind::Object(ObjectKind::Waypoint, _)))
        .filter_map(|entity| {
            let note = entity.object?.map_note(instances.of_entity(entity)?)?;
            let at = place(map, entity);
            seen(fog.as_ref(), at).then_some(())?;
            Some(MapNoteView {
                at,
                text: note.text.clone().into_bytes(),
            })
        })
        .collect();
    Some(MapView {
        area,
        picture: picture(assets, module),
        used: used_width(&module.game),
        player: body.map(|body| (place(map, body), spin(map, body))),
        party: snapshot
            .entities
            .iter()
            .filter(|entity| {
                snapshot.in_party(entity) && body.is_none_or(|body| body.id != entity.id)
            })
            .map(|entity| place(map, entity))
            .collect(),
        notes,
        fog,
        return_button,
        return_hidden,
    })
}

/// What the map page shows of a jammed map (TSL's skin's).
const STATIC: &str = "lbl_static";

/// The map page's return button's label, as `SetReturnStrref` (run by
/// `k_sup_guiopen` as the screen opened) left it, whether it may be used or
/// not ("Transit Disabled" is a label too); none where none was set.
fn return_button(transit: &sim::Transit) -> Option<u32> {
    (transit.label != 0).then_some(transit.label)
}

/// Whether a script turned the return button off: `SetReturnStrref` with
/// `bShow` false (its label set). Where no script set it, it shows greyed.
fn return_hidden(transit: &sim::Transit) -> bool {
    transit.label != 0 && !transit.show
}
