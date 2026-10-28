//! The map page (KOTOR 1's `map`, the skin's `map_p`): in `LBL_Map` the
//! part of the area's picture the map uses, as large as fits in
//! proportion, centred on black, black over what the party has not
//! explored of it (`fog`); on it the rest of the party as circles
//! (`lbl_mapcircle`), the notes the area's waypoints show (`whitetarget`,
//! the chosen one half again as large and lit), and the player's arrow
//! where it stands, turned its way. `LBL_MapNote` holds the chosen note's
//! words; `BTN_UP` and `BTN_DOWN` choose the one before and after, round,
//! and a click on a note's mark chooses it.

use fmt_odyssey::gui::{Control, Gui};

use crate::draw::{Quad, TextureId, Textures};
use crate::intent::Intent;
use crate::paint::{ControlState, Painter, rect_of};
use crate::screens::tab::{Page, disabled, enabled, hidden, image};
use crate::skin::Source;
use crate::vm::{FogView, MapNoteView, MapView, ViewModel};

/// A note's mark, and the chosen one's (layout pixels square).
const NOTE: f32 = 16.0;
const CHOSEN: f32 = NOTE * 1.5;
/// A party member's circle and the player's arrow [verify: the original's
/// sizes].
const CIRCLE: f32 = 16.0;
const ARROW: f32 = 32.0;
const BLACK: [f32; 4] = [0.0, 0.0, 0.0, 1.0];
const WHITE: [f32; 4] = [1.0; 4];

#[derive(Default)]
pub struct Map {
    /// The note chosen, among those shown.
    chosen: usize,
}

/// `size` as large as fits in `window` in proportion, centred in it.
pub fn fitted(window: [f32; 4], size: [f32; 2]) -> [f32; 4] {
    let [left, top, width, height] = window;
    let scale = (width / size[0].max(1.0)).min(height / size[1].max(1.0));
    let [w, h] = size.map(|side| side * scale);
    [left + (width - w) / 2.0, top + (height - h) / 2.0, w, h]
}

/// Texture `name` from `source` as a quad `size` square centred on
/// `centre`, tinted `colour`, clipped to `clip`; `None` where it does not
/// resolve.
fn mark(
    textures: &mut dyn Textures,
    (source, name): (Source, &str),
    centre: [f32; 2],
    size: f32,
    colour: [f32; 3],
    clip: [f32; 4],
) -> Option<Quad> {
    let id = textures.texture(source, name)?;
    let rect = [centre[0] - size / 2.0, centre[1] - size / 2.0, size, size];
    Some(Quad {
        additive: textures.additive(id),
        clip: Some(clip),
        ..Quad::image(rect, id, [colour[0], colour[1], colour[2], 1.0])
    })
}

/// KOTOR's fog mark: a soft dark dot (64 square, its alpha falling from
/// the middle to the edges), and how many cells wide it is drawn.
const FOG_DOT: &str = "blackdot";
const FOG_SPAN: f32 = 4.0;

/// The dark over what the party has not explored of `fog`'s cells, laid
/// over `rect` (the part of the picture the map uses), clipped to `clip`.
/// As KOTOR draws it (`CSWGuiMapHider`): over each unexplored cell the
/// campaign's `blackdot`, four cells wide, centred on it; the dots overlap
/// into dark, and spill a cell or two over explored ground, so its edge
/// fades. Where the texture does not resolve, a black quad for each run of
/// unexplored cells along a row.
pub fn fog(
    fog: &FogView,
    rect: [f32; 4],
    clip: [f32; 4],
    textures: &mut dyn Textures,
) -> Vec<Quad> {
    let [columns, rows] = fog.cells;
    let [left, top, width, height] = rect;
    let (across, down) = (width / columns.max(1) as f32, height / rows.max(1) as f32);
    let unexplored = fog
        .explored
        .chunks(columns.max(1))
        .take(rows)
        .enumerate()
        .flat_map(|(row, cells)| {
            cells
                .iter()
                .enumerate()
                .filter(|(_, explored)| !**explored)
                .map(move |(column, _)| (column, row))
        });
    if let Some(dot) = textures.texture(Source::Campaign, FOG_DOT) {
        let (w, h) = (across * FOG_SPAN, down * FOG_SPAN);
        return unexplored
            .map(|(column, row)| {
                let centre = [
                    left + (column as f32 + 0.5) * across,
                    top + (row as f32 + 0.5) * down,
                ];
                Quad {
                    clip: Some(clip),
                    ..Quad::image([centre[0] - w / 2.0, centre[1] - h / 2.0, w, h], dot, WHITE)
                }
            })
            .collect();
    }
    let mut out = Vec::new();
    for (row, cells) in fog.explored.chunks(columns.max(1)).enumerate().take(rows) {
        let mut column = 0;
        while column < cells.len() {
            let run = cells[column..]
                .iter()
                .take_while(|explored| !**explored)
                .count();
            if run > 0 {
                let at = [left + column as f32 * across, top + row as f32 * down];
                out.push(Quad {
                    clip: Some(clip),
                    ..Quad::solid([at[0], at[1], run as f32 * across, down], BLACK)
                });
            }
            column += run.max(1);
        }
    }
    out
}

/// The area's picture, and where the part of it the map uses lies in
/// `window`, when it resolves.
fn picture(
    map: &MapView,
    window: [f32; 4],
    textures: &mut dyn Textures,
) -> Option<(TextureId, [f32; 4])> {
    let picture = textures.texture(Source::Campaign, map.picture.as_ref()?)?;
    let (width, height) = textures.size(picture);
    Some((
        picture,
        fitted(window, [width as f32 * map.used, height as f32]),
    ))
}

impl Map {
    /// The chosen note's number, among `count` shown.
    fn chosen(&self, count: usize) -> usize {
        self.chosen % count.max(1)
    }

    /// The page's picture and what lies on it, in `window` (`LBL_Map`).
    fn drawn(&self, map: &MapView, window: [f32; 4], painter: &mut Painter) -> Vec<Quad> {
        let mut out = vec![Quad::solid(window, BLACK)];
        let textures = &mut *painter.textures;
        let Some((picture, rect)) = picture(map, window, textures) else {
            return out;
        };
        out.push(Quad {
            uv: [0.0, 0.0, map.used, 1.0],
            ..Quad::image(rect, picture, WHITE)
        });
        if let Some(cells) = &map.fog {
            out.extend(fog(cells, rect, rect, textures));
        }
        let palette = painter.palette;
        // Each mark at its place on the picture, clipped to it.
        let mut put = |what: (Source, &str), at: [f32; 2], size: f32, colour: [f32; 3]| {
            let centre = [rect[0] + at[0] * rect[2], rect[1] + at[1] * rect[3]];
            mark(textures, what, centre, size, colour, rect)
        };
        for at in &map.party {
            out.extend(put(
                (Source::Campaign, "lbl_mapcircle"),
                *at,
                CIRCLE,
                palette.base,
            ));
        }
        let chosen = self.chosen(map.notes.len());
        for (n, note) in map.notes.iter().enumerate() {
            let (size, colour) = if n == chosen {
                (CHOSEN, palette.hilight)
            } else {
                (NOTE, palette.base)
            };
            out.extend(put(
                (Source::Campaign, "whitetarget"),
                note.at,
                size,
                colour,
            ));
        }
        if let Some((at, spin)) = map.player {
            // The skin's arrow over its layouts, else the campaign's.
            let arrow = put((painter.source, "mm_barrow_p"), at, ARROW, [1.0; 3])
                .or_else(|| put((Source::Campaign, "mm_barrow"), at, ARROW, [1.0; 3]));
            out.extend(arrow.map(|arrow| Quad { spin, ..arrow }));
        }
        out
    }
}

impl Page for Map {
    const NAME: &'static str = "map";
    const LAYOUT: &'static str = "map";

    fn state(&self, control: &Control, vm: &ViewModel) -> ControlState {
        let notes = vm.map.as_ref().map_or(&[][..], |map| &map.notes[..]);
        match control.tag.as_str() {
            // Drawn by the page (`quads`).
            "LBL_Map" => image(None),
            "LBL_Area" => ControlState {
                text: vm.map.as_ref().map(|map| map.area.clone()),
                ..ControlState::default()
            },
            "LBL_MapNote" => match notes.get(self.chosen(notes.len())) {
                Some(note) => ControlState {
                    text: Some(note.text.clone()),
                    ..ControlState::default()
                },
                None => hidden(),
            },
            "BTN_UP" | "BTN_DOWN" => enabled(notes.len() > 1),
            // Labelled as the scripts set it (`SetReturnStrref`), or hidden
            // where they turned it off; still greyed: no return to the
            // Ebon Hawk yet.
            "BTN_RETURN" if vm.map.as_ref().is_some_and(|map| map.return_hidden) => hidden(),
            "BTN_RETURN" => ControlState {
                strref: vm.map.as_ref().and_then(|map| map.return_button),
                ..disabled()
            },
            _ => ControlState::default(),
        }
    }

    fn press(&mut self, tag: &str, vm: &ViewModel, _: &mut Vec<Intent>) {
        let count = vm.map.as_ref().map_or(0, |map| map.notes.len()).max(1);
        let chosen = self.chosen(count);
        self.chosen = match tag {
            "BTN_UP" => (chosen + count - 1) % count,
            "BTN_DOWN" => (chosen + 1) % count,
            _ => return,
        };
    }

    fn quads(&self, gui: &Gui, vm: &ViewModel, painter: &mut Painter) -> Vec<Quad> {
        match (vm.map.as_ref(), gui.find("LBL_Map")) {
            (Some(map), Some(label)) => self.drawn(map, rect_of(label), painter),
            _ => Vec::new(),
        }
    }

    /// The note whose mark is nearest `at`, within its mark, is chosen
    /// (KOTOR's notes are buttons over their marks, 14 square).
    fn click(&mut self, gui: &Gui, at: [f32; 2], textures: &mut dyn Textures, vm: &ViewModel) {
        let (Some(map), Some(label)) = (vm.map.as_ref(), gui.find("LBL_Map")) else {
            return;
        };
        let Some((_, rect)) = picture(map, rect_of(label), textures) else {
            return;
        };
        let away = |note: &MapNoteView| {
            let centre = [
                rect[0] + note.at[0] * rect[2],
                rect[1] + note.at[1] * rect[3],
            ];
            (centre[0] - at[0]).hypot(centre[1] - at[1])
        };
        let nearest = map
            .notes
            .iter()
            .enumerate()
            .map(|(n, note)| (n, away(note)))
            .filter(|(_, away)| *away <= NOTE / 2.0)
            .min_by(|a, b| a.1.total_cmp(&b.1));
        if let Some((n, _)) = nearest {
            self.chosen = n;
        }
    }
}
