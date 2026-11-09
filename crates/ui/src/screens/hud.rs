//! KOTOR's HUD (`mipc28x6`, KOTOR 2's `mipc28x6_p` with the skin), drawn
//! under the screens in play and not a screen: nothing on it takes the
//! focus. Its controls keep to the window's corners as the game places
//! them: a control whose left is in the frame's right half moves with the
//! right edge, one whose top is in the lower half with the bottom.
//!
//! It shows the player's portrait, vitality and Force bars (the Force bar
//! only for a character with Force points), the action slots (a slot's
//! frame, the action's icon and uses left in the square inside it), the
//! menu buttons, the level-up mark over each portrait whose member may take
//! a level (`MemberView::may_level`), and the status icons (journal, credits, experience,
//! items received and lost, alignment shifts) a moment after a status
//! summary told them (`HudView::lit`).
//! The companions show beside the portrait, each its portrait and bars;
//! a click on one makes it lead (`portrait_at`).
//! The target block (its name, a creature's vitality, up to three
//! actions, the chosen one lit and spelt out beneath) stands at the top
//! centre while there is a target (`docs/UI.md`), and
//! a crosshair dot at the middle in play; a downed party member's block
//! offers Revive, and while one is revived (I30 P11) its bar is how far
//! along, "Reviving" beneath (the downed player's own block too, its
//! reviver named).
//! In co-op each player's portrait carries an icon for what it is in
//! (R10: talking, trading, pazaak, asking the host in; `busy`).
//! Each portrait shows its effects' icons where KOTOR stacks its arrows
//! (`effect_icons`): the good ones down its left, the bad up its right,
//! each overlapping the one before; a debilitated member's portrait its
//! overlay instead (`LBL_DEBILATATED<n>`).
//! The stealth toggle shows, lit, while the character is in stealth mode.
//! The minimap shows the area's picture where it has one, the player's
//! place at its middle, the arrow turned the player's way. The combat
//! queue and messages, solo and pause are hidden: none has its data yet.
//! KOTOR 2's six slots show those with an action. Its weapon swap button
//! shows in both games (TSL's second weapon set; a remaster liberty in
//! KOTOR): a click on it swaps the player's sets (`swap_at`). A break's or
//! ready check's banner is its own layer (`banner`), in the combat
//! message's place.

use fmt_odyssey::gui::{Control, Gui, align};

use crate::draw::{DrawList, Quad, Textures};
use crate::paint::{ControlState, Painter, rect_of};
use crate::screens::tab::hidden;
use crate::skin::Source;
use crate::vm::{Busy, HudView, KeyLabel, MemberView, MinimapView, TargetView, ViewModel};

pub const LAYOUT: &str = "mipc28x6";

/// The tags left out, by prefix.
const HIDDEN: [&str; 12] = [
    "LBL_CMBT",
    "LBL_COMBATBG",
    "LBL_QUEUE",
    "BTN_CLEAR",
    "LBL_MAP",
    "BTN_MINIMAP",
    "LBL_ARROW",
    "LBL_DISABLE",
    "LBL_LEVELUP",
    "TB_",
    "BTN_ACTIONUP",
    "BTN_ACTIONDOWN",
];

/// The target block's controls, by prefix (in the layout's top left), and
/// the chosen action's words.
const BLOCK: [&str; 4] = ["LBL_NAME", "LBL_HEALTHBG", "PB_HEALTH", "BTN_TARGET"];
/// How far below the window's top the block stands, and its words below
/// it (layout pixels).
const BLOCK_TOP: i32 = 4;
const WORDS_GAP: i32 = 1;

/// A foe's name, its backing and its vitality bar (KOTOR's red).
const HOSTILE: [f32; 3] = [0.86, 0.16, 0.12];

/// A portrait's vitality bar as both games' executables colour it, over
/// the layout's red: their own red, green while a poison runs.
const VITALITY: [f32; 3] = [0.74, 0.11, 0.0];
const POISONED: [f32; 3] = [0.28, 0.92, 0.11];
/// The target's vitality bar while a poison runs in it (both games' art).
const POISON_BAR: &str = "poison_bar";

/// A portrait's vitality bar at `hp`.
fn vitality((hp, poisoned): ((i32, i32), bool)) -> ControlState {
    ControlState {
        value: Some(share(hp)),
        bar: (None, Some(if poisoned { POISONED } else { VITALITY })),
        ..ControlState::default()
    }
}

fn in_block(tag: &str) -> bool {
    BLOCK.iter().any(|prefix| tag.starts_with(prefix)) || tag.starts_with("LBL_TARGET")
}

/// The level-up mark over each portrait of a member who may take a level:
/// the campaign's arrow (KOTOR's yellow, not the skin's recolouring),
/// pulsing, over KOTOR's backing that darkens the portrait (half-clear
/// black), which KOTOR 2's layout lacks (`backed`).
const LEVEL_UP_ARROW: &str = "lbl_levelup";
/// The marks' and backings' tags but their party slot.
const MARKS: &str = "LBL_LEVELUP";
const BACKINGS: &str = "LBL_LVLUPBG";

/// The arrow, shown while its member may take a level.
pub fn level_up_mark(on: bool) -> ControlState {
    if on {
        ControlState {
            fill: Some(LEVEL_UP_ARROW.into()),
            pulsing: true,
            ..ControlState::default()
        }
    } else {
        hidden()
    }
}

/// A level-up control: the member's place in the party, and whether it is
/// the arrow (else its backing).
fn level_up_control(tag: &str) -> Option<(usize, bool)> {
    let (slot, arrow) = match (tag.strip_prefix(MARKS), tag.strip_prefix(BACKINGS)) {
        (Some(slot), _) => (slot, true),
        (_, Some(slot)) => (slot, false),
        _ => return None,
    };
    let place = slot.parse::<usize>().ok()?;
    (1..=3).contains(&place).then_some((place - 1, arrow))
}

/// The status icons, each shown while lit (`HudView::lit`).
const STATUS: [&str; 8] = [
    "LBL_JOURNAL",
    "LBL_CASH",
    "LBL_PLOTXP",
    "LBL_ITEMRCVD",
    "LBL_ITEMLOST",
    "LBL_LIGHTSHIFT",
    "LBL_DARKSHIFT",
    "LBL_STEALTHXP",
];

/// `gui` (the skin's HUD) with the backing `campaign`'s HUD keeps under
/// each level-up mark (`LBL_LVLUPBG<n>` under `LBL_LEVELUP<n>`) where it
/// has the mark but not the backing: the mark's place, filled with the
/// campaign backing's art. KOTOR draws the backing, KOTOR 2's layout has
/// none (reone's HUD shows it in K1 only).
pub fn backed(gui: &mut Gui, campaign: &Gui) {
    for kept in &campaign.controls {
        let Some(slot) = kept.tag.strip_prefix(BACKINGS) else {
            continue;
        };
        let mark = format!("{MARKS}{slot}");
        let Some(at) = gui.controls.iter().position(|c| c.tag == mark) else {
            continue;
        };
        if gui.find(&kept.tag).is_some() {
            continue;
        }
        let mark = &gui.controls[at];
        let backing = Control {
            tag: kept.tag.clone(),
            border: mark
                .border
                .clone()
                .zip(kept.border.as_ref())
                .map(|(border, art)| fmt_odyssey::gui::Border {
                    fill: art.fill.clone(),
                    color: None,
                    ..border
                }),
            ..mark.clone()
        };
        gui.controls.insert(at, backing);
    }
}

/// How far the target block reaches from the layout's top left, where it
/// stands: its controls' extents together, right and down (layout
/// pixels; KOTOR 2's, 200 by 95).
fn block_reach(gui: &Gui) -> (i32, i32) {
    gui.controls
        .iter()
        .filter(|control| in_block(&control.tag))
        .map(|control| control.extent)
        .fold((0, 0), |(right, bottom), extent| {
            (
                right.max(extent.left + extent.width),
                bottom.max(extent.top + extent.height),
            )
        })
}

/// `gui` placed for a window of `window` pixels: its frame the window
/// (`fitted`), each control moved with the edges of its quadrant.
pub fn anchor(gui: &mut Gui, window: (f32, f32)) {
    let (width, height) = gui.frame();
    let (wide, tall) = fitted(gui, window);
    let (dx, dy) = (wide - width, tall - height);
    fn place(control: &mut Control, half: (i32, i32), by: (i32, i32)) {
        let shift = (
            if control.extent.left >= half.0 {
                by.0
            } else {
                0
            },
            if control.extent.top >= half.1 {
                by.1
            } else {
                0
            },
        );
        shift_all(control, shift);
        for child in &mut control.children {
            place(child, half, by);
        }
    }
    fn shift_all(control: &mut Control, (dx, dy): (i32, i32)) {
        control.extent.left += dx;
        control.extent.top += dy;
        for inner in control
            .proto
            .as_deref_mut()
            .into_iter()
            .chain(control.scrollbar.as_deref_mut())
        {
            shift_all(inner, (dx, dy));
        }
    }
    for control in &mut gui.controls {
        place(control, (width / 2, height / 2), (dx, dy));
    }
    // The target block to the top centre, its words beneath it.
    let centre = wide / 2;
    let (block_width, block_bottom) = block_reach(gui);
    for control in &mut gui.controls {
        let tag = control.tag.as_str();
        if in_block(tag) {
            shift_all(control, (centre - block_width / 2, BLOCK_TOP));
        } else if tag.starts_with("LBL_ACTIONDESC") {
            let extent = control.extent;
            shift_all(
                control,
                (
                    centre - extent.width / 2 - extent.left,
                    BLOCK_TOP + block_bottom + WORDS_GAP - extent.top,
                ),
            );
        }
    }
    gui.extent.left = 0;
    gui.extent.top = 0;
    gui.extent.width = wide;
    gui.extent.height = tall;
}

/// Where the target block ends in `gui` (placed by `anchor`): the bottom
/// of its words, else of the block itself (layout pixels).
pub fn block_bottom(gui: &Gui) -> i32 {
    gui.controls
        .iter()
        .filter(|control| in_block(&control.tag) || control.tag.starts_with("LBL_ACTIONDESC"))
        .map(|control| control.extent.top + control.extent.height)
        .max()
        .unwrap_or(0)
}

/// `gui`'s frame for a window of `window` pixels, drawn at the window's
/// scale (`draw::fit`): the window in layout pixels, or, in a window too
/// small for the layout at that scale, the window's shape (`shaped`).
pub fn fitted(gui: &Gui, window: (f32, f32)) -> (i32, i32) {
    let (width, height) = gui.frame();
    let scale = crate::draw::scale(window);
    let (wide, tall) = (window.0 / scale, window.1 / scale);
    if wide < width as f32 || tall < height as f32 {
        return shaped(gui, window);
    }
    (wide.floor() as i32, tall.floor() as i32)
}

/// `gui`'s frame for a window of `window` pixels, stretched to fill it
/// (`draw::fill`): the window's shape, the layout's height or width kept,
/// whichever fits.
pub fn shaped(gui: &Gui, window: (f32, f32)) -> (i32, i32) {
    let (width, height) = gui.frame();
    let aspect = window.0 / window.1.max(1.0);
    if aspect >= width as f32 / height as f32 {
        ((height as f32 * aspect).round() as i32, height)
    } else {
        (width, (width as f32 / aspect).round() as i32)
    }
}

/// The crosshair: a light dot ringed dark at the middle of `gui` (placed
/// by `anchor`).
pub fn crosshair(gui: &Gui) -> [Quad; 2] {
    let (width, height) = gui.frame();
    let (x, y) = (width as f32 / 2.0, height as f32 / 2.0);
    [
        Quad::solid([x - 3.0, y - 3.0, 6.0, 6.0], [0.0, 0.0, 0.0, 0.6]),
        Quad::solid([x - 2.0, y - 2.0, 4.0, 4.0], [1.0, 1.0, 1.0, 0.9]),
    ]
}

/// The shape of a blaster's crosshair: it tells the kind of gun apart, and
/// its gap is the spread the next shot may stray by (`gap_px`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Family {
    /// A dot and four bars: pistols and rifles.
    Cross,
    /// A dot and three bars, two up and one down: repeaters.
    Prongs,
    /// A dot and two arcs, left and right: bowcasters.
    Brackets,
    /// A dot and four long thin bars: disruptors.
    Lines,
    /// A dot and a ring broken in four: ion weapons.
    Ring,
    /// A dot and a ring of eight dashes: sonic weapons.
    Dashes,
}

/// The smallest gap at a 1080-high layout (pixels): the bars stay clear of
/// the dot when the spread is none.
pub const MIN_GAP: f32 = 4.0;

/// The gap a spread of `cone_deg` makes on a layout `layout_height` high,
/// seen through a vertical field of view of `vfov_deg`: where a shot that
/// strays the whole spread lands on the screen, never under `MIN_GAP`.
pub fn gap_px(cone_deg: f32, vfov_deg: f32, layout_height: f32) -> f32 {
    let half = layout_height / 2.0;
    let gap = cone_deg.to_radians().tan() / (vfov_deg / 2.0).to_radians().tan() * half;
    gap.max(MIN_GAP * layout_height / 1080.0)
}

/// A crosshair's light: white, a touch see-through.
const RETICLE_LIGHT: [f32; 4] = [1.0, 1.0, 1.0, 0.9];
/// The dark edge under each of its strokes, so it reads on bright ground.
const RETICLE_EDGE: [f32; 4] = [0.0, 0.0, 0.0, 0.5];

/// A stroke `width` wide from `from` to `to` pixels out of `middle`, turned
/// `angle` radians clockwise from straight up, in `color`, over its dark
/// edge one pixel (`scale`) wider all round: the edge first.
pub fn stroke(
    middle: [f32; 2],
    (from, to): (f32, f32),
    angle: f32,
    (width, scale): (f32, f32),
    color: [f32; 4],
) -> [Quad; 2] {
    let (sin, cos) = angle.sin_cos();
    let reach = (from + to) / 2.0;
    let centre = [middle[0] + sin * reach, middle[1] - cos * reach];
    let bar = |width: f32, length: f32, color| Quad {
        spin: angle,
        ..Quad::solid(
            [
                centre[0] - width / 2.0,
                centre[1] - length / 2.0,
                width,
                length,
            ],
            color,
        )
    };
    let length = to - from;
    let edge = [
        RETICLE_EDGE[0],
        RETICLE_EDGE[1],
        RETICLE_EDGE[2],
        RETICLE_EDGE[3] * color[3],
    ];
    [
        bar(width + 2.0 * scale, length + 2.0 * scale, edge),
        bar(width, length, color),
    ]
}

/// An arc `radius` out of `middle`, from `start` to `end` degrees clockwise
/// from straight up, as `pieces` short strokes along it.
fn arc(
    middle: [f32; 2],
    radius: f32,
    (start, end): (f32, f32),
    pieces: usize,
    (width, scale): (f32, f32),
) -> Vec<Quad> {
    let span = (end - start) / pieces as f32;
    (0..pieces)
        .flat_map(|piece| {
            let at = (start + span * (piece as f32 + 0.5)).to_radians();
            let long = radius * span.to_radians();
            // Along the circle: a radial stroke turned a quarter, its middle
            // on the circle.
            let (sin, cos) = at.sin_cos();
            let on = [middle[0] + sin * radius, middle[1] - cos * radius];
            let turned = at + std::f32::consts::FRAC_PI_2;
            stroke(
                on,
                (-long / 2.0, long / 2.0),
                turned,
                (width, scale),
                RETICLE_LIGHT,
            )
        })
        .collect()
}

/// A blaster's crosshair of `family` at the middle of a layout `frame`
/// (width, height) big, its gap `gap` layout pixels from the middle: a dot,
/// then its strokes, each over its dark edge, two pixels wide at a
/// 1080-high layout.
pub fn reticle(frame: (i32, i32), family: Family, gap: f32) -> Vec<Quad> {
    let (width, height) = frame;
    let middle = [width as f32 / 2.0, height as f32 / 2.0];
    let scale = height as f32 / 1080.0;
    let thick = (2.0 * scale, scale);
    let gap = gap.max(MIN_GAP * scale);
    let dot = 2.0 * scale;
    let mut quads = vec![
        Quad::solid(
            [middle[0] - dot, middle[1] - dot, 2.0 * dot, 2.0 * dot],
            RETICLE_EDGE,
        ),
        Quad::solid(
            [middle[0] - dot / 2.0, middle[1] - dot / 2.0, dot, dot],
            RETICLE_LIGHT,
        ),
    ];
    let bars = |angles: &[f32], long: f32| -> Vec<Quad> {
        angles
            .iter()
            .flat_map(|angle| {
                stroke(
                    middle,
                    (gap, gap + long * scale),
                    angle.to_radians(),
                    thick,
                    RETICLE_LIGHT,
                )
            })
            .collect()
    };
    let arcs = |centres: &[f32], span: f32, pieces: usize| -> Vec<Quad> {
        centres
            .iter()
            .flat_map(|at| {
                arc(
                    middle,
                    gap,
                    (at - span / 2.0, at + span / 2.0),
                    pieces,
                    thick,
                )
            })
            .collect()
    };
    quads.extend(match family {
        Family::Cross => bars(&[0.0, 90.0, 180.0, 270.0], 6.0),
        Family::Prongs => bars(&[-60.0, 60.0, 180.0], 6.0),
        Family::Brackets => arcs(&[90.0, 270.0], 80.0, 3),
        Family::Lines => bars(&[0.0, 90.0, 180.0, 270.0], 14.0),
        Family::Ring => arcs(&[45.0, 135.0, 225.0, 315.0], 60.0, 3),
        Family::Dashes => arcs(
            &[0.0, 45.0, 90.0, 135.0, 180.0, 225.0, 270.0, 315.0],
            20.0,
            1,
        ),
    });
    quads
}

/// What a blaster shot the player landed did, as the mark flashed about
/// the crosshair shows it: the colour tells a hit from a strong one, a
/// kill adds red ticks, a bolt a blade turned a grey mark.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mark {
    Turned,
    Hit,
    Strong,
    Kill { strong: bool },
}

/// How long a mark shows (ms), and how much of that it fades over.
pub const MARK_SHOWN_MS: f32 = 140.0;
const MARK_FADE_MS: f32 = 80.0;

/// The mark of `mark` `age_ms` after it came, about a crosshair whose gap
/// is `gap` at the middle of a layout `frame` big: four strokes on the
/// diagonals outside the gap, growing a little and fading; none once
/// `MARK_SHOWN_MS` has passed.
pub fn hitmarker(frame: (i32, i32), mark: Mark, gap: f32, age_ms: f32) -> Vec<Quad> {
    if !(0.0..=MARK_SHOWN_MS).contains(&age_ms) {
        return Vec::new();
    }
    let (width, height) = frame;
    let middle = [width as f32 / 2.0, height as f32 / 2.0];
    let scale = height as f32 / 1080.0;
    let grown = 1.0 + 0.15 * age_ms / MARK_SHOWN_MS;
    let alpha = ((MARK_SHOWN_MS - age_ms) / MARK_FADE_MS).min(1.0);
    let tint = |[r, g, b]: [f32; 3]| [r, g, b, alpha];
    let (color, long) = match mark {
        Mark::Turned => (MARK_GREY, 4.0),
        Mark::Hit | Mark::Kill { strong: false } => (MARK_WHITE, 8.0),
        Mark::Strong | Mark::Kill { strong: true } => (MARK_GOLD, 8.0),
    };
    let diagonals = [45.0f32, 135.0, 225.0, 315.0];
    let ring = |(from, to): (f32, f32), color: [f32; 3]| -> Vec<Quad> {
        diagonals
            .iter()
            .flat_map(|angle| {
                stroke(
                    middle,
                    ((gap + from * scale) * grown, (gap + to * scale) * grown),
                    angle.to_radians(),
                    (2.0 * scale, scale),
                    tint(color),
                )
            })
            .collect()
    };
    let mut quads = ring((6.0, 6.0 + long), color);
    if matches!(mark, Mark::Kill { .. }) {
        quads.extend(ring((18.0, 24.0), MARK_RED));
    }
    quads
}

/// The marks' colours: a hit, a strong hit, a kill's ticks, a turned bolt.
const MARK_WHITE: [f32; 3] = [1.0, 1.0, 1.0];
const MARK_GOLD: [f32; 3] = [0.95, 0.72, 0.2];
const MARK_RED: [f32; 3] = [0.85, 0.15, 0.1];
const MARK_GREY: [f32; 3] = [0.6, 0.6, 0.6];

/// The minimap's quads: the view's black, the area's picture over it and
/// black over what is unexplored, under the layout; the arrow over it.
#[derive(Debug, Clone, PartialEq)]
pub struct Minimap {
    /// Black over the whole view, so none of the world shows where the
    /// picture ends inside it (KOTOR's pictures are black round the plan).
    pub backing: Quad,
    pub picture: Quad,
    pub fog: Vec<Quad>,
    pub arrow: Option<Quad>,
}

impl Minimap {
    /// What goes under the layout, in order.
    pub fn under(&self) -> impl Iterator<Item = Quad> + '_ {
        [self.backing, self.picture]
            .into_iter()
            .chain(self.fog.iter().copied())
    }
}

/// The minimap of `view` in `gui` (placed by `anchor`; its own art read
/// from `source`): the area's picture at its own size, moved so the
/// player's place is at `LBL_MAPVIEW`'s centre and clipped to it, and
/// `LBL_ARROW`'s texture at its size at the centre, turned; the cells the
/// party has not explored dark over the picture's used part (`map::fog`; its
/// `MinimapView::used` share of its width). `None` without
/// the view or the picture. KOTOR's `LBL_MAP` (the picture's holder) and
/// `LBL_ARROW` themselves stay hidden.
pub fn minimap(
    gui: &Gui,
    view: &MinimapView,
    textures: &mut dyn Textures,
    source: Source,
) -> Option<Minimap> {
    let window = rect_of(gui.find("LBL_MAPVIEW")?);
    let centre = [window[0] + window[2] / 2.0, window[1] + window[3] / 2.0];
    // A texture as a quad, and its size.
    let mut image = |source: Source, name: &str| {
        let id = textures.texture(source, name)?;
        let (width, height) = textures.size(id);
        let quad = Quad {
            additive: textures.additive(id),
            ..Quad::image([0.0; 4], id, [1.0; 4])
        };
        Some((quad, [width as f32, height as f32]))
    };
    let (picture, [width, height]) = image(Source::Campaign, &view.picture)?;
    let [across, down] = view.at;
    let picture = Quad {
        rect: [
            centre[0] - across * width,
            centre[1] - down * height,
            width,
            height,
        ],
        clip: Some(window),
        ..picture
    };
    let arrow = gui.find("LBL_ARROW").and_then(|control| {
        let [_, _, width, height] = rect_of(control);
        let (arrow, _) = image(source, &control.border.as_ref()?.fill)?;
        Some(Quad {
            rect: [
                centre[0] - width / 2.0,
                centre[1] - height / 2.0,
                width,
                height,
            ],
            spin: view.spin,
            ..arrow
        })
    });
    let used = [picture.rect[0], picture.rect[1], width * view.used, height];
    Some(Minimap {
        backing: Quad::solid(window, [0.0, 0.0, 0.0, 1.0]),
        picture,
        fog: view.fog.as_ref().map_or(Vec::new(), |cells| {
            crate::screens::map::fog(cells, used, window, textures)
        }),
        arrow,
    })
}

/// How the target block shows `tag`, when it is one of the block's.
fn target_state(tag: &str, target: Option<&TargetView>) -> Option<ControlState> {
    if !in_block(tag) && !tag.starts_with("LBL_ACTIONDESC") {
        return None;
    }
    // No target has more than three actions: the slots' arrows stay hidden.
    if tag.starts_with("BTN_TARGETUP") || tag.starts_with("BTN_TARGETDOWN") {
        return Some(hidden());
    }
    let Some(target) = target else {
        return Some(hidden());
    };
    // A revive under way: its bar in the vitality's, "Reviving" beneath.
    let words = match target.progress {
        Some(_) => Some(crate::strings::bytes(crate::strings::REVIVING)),
        None => target
            .actions
            .get(target.selected)
            .filter(|_| !target.busy)
            .map(|action| action.label.clone()),
    };
    let bar = target.progress.or(target.hp.map(share));
    let tint = (target.hostile && target.progress.is_none()).then_some(HOSTILE);
    let poisoned = target.poisoned && target.progress.is_none() && target.hp.is_some();
    let slot = |prefix: &str| tag.strip_prefix(prefix)?.parse::<usize>().ok();
    Some(match tag {
        "LBL_NAME" => ControlState {
            text: Some(target.name.clone()),
            tint,
            ..ControlState::default()
        },
        "LBL_NAMEBG" => ControlState {
            tint,
            ..ControlState::default()
        },
        "PB_HEALTH" if poisoned => ControlState {
            value: bar,
            bar: (Some(POISON_BAR.into()), None),
            ..ControlState::default()
        },
        "LBL_HEALTHBG" | "PB_HEALTH" => match bar {
            Some(bar) => ControlState {
                value: (tag == "PB_HEALTH").then_some(bar),
                tint,
                ..ControlState::default()
            },
            None => hidden(),
        },
        "LBL_ACTIONDESC" => match words {
            Some(words) => ControlState {
                text: Some(words),
                ..ControlState::default()
            },
            None => hidden(),
        },
        "LBL_ACTIONDESCBG" if words.is_none() => hidden(),
        _ => {
            if let Some(n) = slot("BTN_TARGET") {
                if n < target.actions.len() && !target.busy {
                    ControlState {
                        selected: n == target.selected,
                        ..ControlState::default()
                    }
                } else {
                    hidden()
                }
            } else if let Some(n) = slot("LBL_TARGET") {
                match target.actions.get(n).filter(|_| !target.busy) {
                    Some(action) => ControlState {
                        fill: Some(action.icon.clone()),
                        text: Some(Vec::new()),
                        ..ControlState::default()
                    },
                    None => hidden(),
                }
            } else {
                ControlState::default()
            }
        }
    })
}

fn share((current, most): (i32, i32)) -> f32 {
    current as f32 / most.max(1) as f32
}

/// A companion's controls, by prefix: its frame, portrait, button, bars.
const COMPANION: [&str; 5] = ["LBL_BACK", "LBL_CHAR", "BTN_CHAR", "PB_VIT", "PB_FORCE"];

/// A companion's control: its place in the party (the HUD's second and
/// third portraits are its first and second companions) and its prefix.
fn companion(tag: &str) -> Option<(usize, &'static str)> {
    COMPANION.iter().find_map(|prefix| {
        let place = tag.strip_prefix(prefix)?.parse::<usize>().ok()?;
        (2..=3).contains(&place).then_some((place - 1, *prefix))
    })
}

/// The companion (its body, `MemberView::id`) whose portrait is at
/// `point` (layout pixels of `gui`, placed by `anchor`): the second or
/// third portrait, when shown (KOTOR's click there makes that one lead).
pub fn portrait_at(gui: &Gui, vm: &ViewModel, [x, y]: [f32; 2]) -> Option<u32> {
    (1..=2usize)
        .find(|&place| {
            vm.party.get(place).is_some()
                && ["BTN_CHAR", "LBL_CHAR"].iter().any(|prefix| {
                    gui.find(&format!("{prefix}{}", place + 1))
                        .is_some_and(|control| within(rect_of(control), [x, y]))
                })
        })
        .and_then(|place| Some(vm.party.get(place)?.id))
}

/// Whether `point` (layout pixels of `gui`, placed by `anchor`) is on the
/// minimap, when it shows: KOTOR's click there (`BTN_MINIMAP`) opens the
/// map page.
pub fn minimap_at(gui: &Gui, vm: &ViewModel, point: [f32; 2]) -> bool {
    vm.hud.as_ref().is_some_and(|hud| hud.minimap.is_some())
        && gui
            .find("LBL_MAPBORDER")
            .is_some_and(|control| within(rect_of(control), point))
}

/// The weapon swap button (TSL's, `CSWGuiMainInterface::OnSwapWeaponsPressed`).
pub const SWAP: &str = "BTN_SWAPWEAPONS";

/// Whether `point` (layout pixels of `gui`, placed by `anchor`) is on the
/// weapon swap button, shown while the HUD has a character.
pub fn swap_at(gui: &Gui, vm: &ViewModel, point: [f32; 2]) -> bool {
    vm.hud.is_some()
        && gui
            .find(SWAP)
            .is_some_and(|control| within(rect_of(control), point))
}

/// Whether `point` is inside `rect` (left, top, width, height).
fn within([left, top, width, height]: [f32; 4], [x, y]: [f32; 2]) -> bool {
    (left..left + width).contains(&x) && (top..top + height).contains(&y)
}

/// How the HUD shows a companion's `prefix` control.
fn companion_state(member: Option<&MemberView>, prefix: &str) -> ControlState {
    let Some(member) = member else {
        return hidden();
    };
    match prefix {
        "LBL_CHAR" => ControlState {
            fill: Some(member.portrait.clone().unwrap_or(String::new())),
            ..ControlState::default()
        },
        "PB_VIT" => vitality((member.hp, member.poisoned)),
        "PB_FORCE" if member.fp.1 > 0 => ControlState {
            value: Some(share(member.fp)),
            ..ControlState::default()
        },
        "PB_FORCE" => hidden(),
        _ => ControlState::default(),
    }
}

/// The icon a busy player's portrait carries (R10): the games' own art
/// (KOTOR 2's, as the skin's: the survey's Q6).
fn icon(busy: Busy) -> &'static str {
    match busy {
        Busy::Talking => "gui_mp_talkd",
        Busy::Shopping => "lbl_icredits",
        Busy::Pazaak => "pcards_pos_x",
        Busy::Asking => "lbl_iquest",
    }
}

/// A busy icon's side, half its portrait's width.
const ICON: f32 = 0.5;
/// The font of the exclamation over a request's icon.
const EXCLAIM_FONT: &str = "fnt_d16x16";

/// A busy icon where it stands: its portrait's top right, its art, and
/// whether an exclamation goes over it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BusyMark {
    pub rect: [f32; 4],
    pub icon: &'static str,
    pub exclaim: bool,
}

/// Each busy portrait's party place (0 the player's own), what it is in,
/// and where its icon stands: its portrait's top right.
fn placed_marks(gui: &Gui, vm: &ViewModel) -> Vec<(usize, Busy, [f32; 4])> {
    let own = vm.hud.as_ref().and_then(|hud| hud.busy);
    let companions = (1..=2).map(|place| vm.party.get(place).and_then(|member| member.busy));
    (0..)
        .zip(std::iter::once(own).chain(companions))
        .filter_map(|(place, busy)| {
            let portrait = gui.find(&format!("LBL_CHAR{}", place + 1))?;
            let [left, top, width, _] = rect_of(portrait);
            let side = (width * ICON).round();
            Some((place, busy?, [left + width - side, top, side, side]))
        })
        .collect()
}

/// The busy icons over `gui`'s portraits (placed by `anchor`): the
/// player's own (`HudView::busy`, `LBL_CHAR1`) and its companions'
/// (`MemberView::busy`, `LBL_CHAR2` and `3`), each at its portrait's top
/// right. On the host's screen (a request card up) a guest asking it in
/// has an exclamation over the icon, as text: the games have no such art.
pub fn busy_marks(gui: &Gui, vm: &ViewModel) -> Vec<BusyMark> {
    placed_marks(gui, vm)
        .into_iter()
        .map(|(_, busy, rect)| BusyMark {
            rect,
            icon: icon(busy),
            exclaim: busy == Busy::Asking && vm.requests.is_some(),
        })
        .collect()
}

/// The companion (its body, `MemberView::id`) whose pazaak icon is at `at`
/// (layout pixels): a click on it watches that player's match (R9a).
pub fn watch_at(gui: &Gui, vm: &ViewModel, [x, y]: [f32; 2]) -> Option<u32> {
    placed_marks(gui, vm)
        .into_iter()
        .find(|&(place, busy, rect)| place > 0 && busy == Busy::Pazaak && within(rect, [x, y]))
        .and_then(|(place, ..)| Some(vm.party.get(place)?.id))
}

/// The effect arrows' labels but their party slot: the good's (KOTOR's
/// `LBL_CMBTEFCTINC<n>`, an arrow up) and the bad's (`…RED<n>`, down), the
/// places their icons stack.
const GOOD_ARROW: &str = "LBL_CMBTEFCTINC";
const BAD_ARROW: &str = "LBL_CMBTEFCTRED";
/// A member's debilitated overlay but its party slot.
const DEBILITATED: &str = "LBL_DEBILATATED";

/// The most icons a companion's portrait and the leader's show (KOTOR's
/// most arrows, `CSWGuiMainInterfaceChar::DrawStack`).
const MOST: [usize; 2] = [5, 9];
/// An icon's side, in its arrow label's widths: small and compact, about
/// KOTOR's arrow (a remaster liberty: KOTOR draws arrows; its icons are on
/// the character sheet).
const ICON_SIDE: f32 = 0.8;
/// How much of each icon the next one leaves showing, in icon sides, while
/// the stack fits the portrait; a longer stack closes up to fit.
const ICON_REVEAL: f32 = 0.65;
/// The solid black square each icon sits on, its edge beyond the icon
/// (layout pixels): every icon a square however much of its picture is
/// clear (an item's, such as an energy shield's), each parted from the one
/// beneath.
const ICON_EDGE: f32 = 1.0;
const ICON_BACKING: [f32; 4] = [0.0, 0.0, 0.0, 1.0];

/// One running effect's icon where it stacks: its picture, its place, and
/// the portrait it is clipped to.
#[derive(Debug, Clone, PartialEq)]
pub struct StackedIcon {
    pub icon: String,
    pub rect: [f32; 4],
    pub clip: [f32; 4],
}

/// Every portrait's effect icons (`MemberView::effects`), stacked where
/// KOTOR's `CSWGuiMainInterface::UpdatePortraits` and `DrawEffects` stack
/// its arrows: none on a debilitated member; else its good ones (capped,
/// `MOST`) about its `LBL_CMBTEFCTINC<n>`, each the next one down, and its
/// bad ones about `LBL_CMBTEFCTRED<n>`, each the next one up; the good
/// flush with their label's left, the bad with its right. Each shows
/// `ICON_REVEAL` of the one before, less when that would leave the
/// portrait, and the stack stays within it. The latest to take hold lies
/// on top.
pub fn effect_icons(gui: &Gui, vm: &ViewModel) -> Vec<StackedIcon> {
    if vm.hud.is_none() {
        return Vec::new();
    }
    let mut icons = Vec::new();
    for (place, member) in vm.party.iter().enumerate().take(3) {
        if member.debilitated {
            continue;
        }
        let slot = place + 1;
        let leader = usize::from(place == 0);
        let Some(portrait) = gui.find(&format!("LBL_CHAR{slot}")) else {
            continue;
        };
        let clip = rect_of(portrait);
        for (prefix, good) in [(GOOD_ARROW, true), (BAD_ARROW, false)] {
            let Some(label) = gui.find(&format!("{prefix}{slot}")) else {
                continue;
            };
            let kind: Vec<&str> = member
                .effects
                .iter()
                .filter(|effect| effect.good == good)
                .map(|effect| effect.icon.as_str())
                .take(MOST[leader])
                .collect();
            let Some(last) = kind.len().checked_sub(1) else {
                continue;
            };
            let [left, top, width, height] = rect_of(label);
            let side = (width * ICON_SIDE).round();
            let left = if good { left } else { left + width - side };
            let room = (clip[3] - side).max(0.0);
            let step = if last == 0 {
                0.0
            } else {
                (side * ICON_REVEAL).min(room / last as f32).floor()
            };
            let span = step * last as f32;
            let highest = top + (height - side - span) / 2.0;
            let highest = highest.clamp(clip[1], clip[1] + room - span);
            let (first, by) = if good {
                (highest, step)
            } else {
                (highest + span, -step)
            };
            icons.extend(kind.into_iter().enumerate().map(|(nth, icon)| StackedIcon {
                icon: icon.to_owned(),
                rect: [left, first + by * nth as f32, side, side],
                clip,
            }));
        }
    }
    icons
}

/// The effect icons drawn (`effect_icons`), each on its black square.
pub fn effects(gui: &Gui, vm: &ViewModel, painter: &mut Painter, out: &mut DrawList) {
    for stacked in effect_icons(gui, vm) {
        let Some(id) = painter.textures.texture(painter.source, &stacked.icon) else {
            continue;
        };
        let [left, top, width, height] = stacked.rect;
        let backing = [
            left - ICON_EDGE,
            top - ICON_EDGE,
            width + 2.0 * ICON_EDGE,
            height + 2.0 * ICON_EDGE,
        ];
        out.quads.push(Quad {
            clip: Some(stacked.clip),
            ..Quad::solid(backing, ICON_BACKING)
        });
        out.quads.push(Quad {
            additive: painter.textures.additive(id),
            clip: Some(stacked.clip),
            ..Quad::image(stacked.rect, id, [1.0; 4])
        });
    }
}

/// The busy icons drawn (`busy_marks`).
pub fn busy(gui: &Gui, vm: &ViewModel, painter: &mut Painter, out: &mut DrawList) {
    for mark in busy_marks(gui, vm) {
        if let Some(id) = painter.textures.texture(painter.source, mark.icon) {
            out.quads.push(Quad {
                additive: painter.textures.additive(id),
                ..Quad::image(mark.rect, id, [1.0; 4])
            });
        }
        if mark.exclaim {
            let [r, g, b] = painter.palette.hilight;
            let centred = align::CENTRE | align::MIDDLE;
            painter.text(EXCLAIM_FONT, b"!", mark.rect, centred, [r, g, b, 1.0], out);
        }
    }
}

/// A pad's buttons' height over an action slot (layout pixels).
const BADGE_TALL: f32 = 14.0;

/// Each action slot's key at the top of its button and the uses left at
/// its bottom right, small and ringed in black (the layout's own count
/// shares the icon's corner with the key, so it is drawn here instead).
pub fn action_keys(gui: &Gui, vm: &ViewModel, painter: &mut Painter, out: &mut DrawList) {
    let Some(hud) = vm.hud.as_ref() else {
        return;
    };
    let palette = painter.palette;
    let ring = [[-1.0, 0.0], [1.0, 0.0], [0.0, -1.0], [0.0, 1.0]];
    let mut write =
        |words: &[u8], rect: &dyn Fn(f32, f32) -> [f32; 4], place: i32, color: [f32; 3]| {
            for [dx, dy] in ring {
                painter.text(
                    EXCLAIM_FONT,
                    words,
                    rect(dx, dy),
                    place,
                    [0.0, 0.0, 0.0, 1.0],
                    out,
                );
            }
            let [r, g, b] = color;
            painter.text(
                EXCLAIM_FONT,
                words,
                rect(0.0, 0.0),
                place,
                [r, g, b, 1.0],
                out,
            );
        };
    let mut badges = Vec::new();
    for (nth, (_, left)) in hud.actions.iter().enumerate() {
        let Some(button) = gui.find(&format!("BTN_ACTION{nth}")) else {
            continue;
        };
        let [x, y, w, h] = rect_of(button);
        match hud.action_keys.get(nth) {
            // A pad's buttons, as large as fit the slot's top.
            Some(KeyLabel {
                badge: Some(badge), ..
            }) => badges.push((badge, [x + 2.0, y + 2.0, w - 4.0, BADGE_TALL])),
            Some(key) => {
                let rect = |dx: f32, dy: f32| [x + 2.0 + dx, y + 2.0 + dy, w - 4.0, 18.0];
                write(
                    &key.words,
                    &rect,
                    align::CENTRE | align::TOP,
                    palette.hilight,
                );
            }
            None => {}
        }
        if let Some(left) = left {
            let rect = |dx: f32, dy: f32| [x + dx, y + h - 20.0 + dy, w - 4.0, 18.0];
            write(
                left.to_string().as_bytes(),
                &rect,
                align::RIGHT | align::BOTTOM,
                palette.base,
            );
        }
    }
    for (badge, area) in badges {
        painter.badge(badge, area, out);
    }
    action_label(gui, hud, painter, out);
}

/// The label's padding round its words, and its gap over the slot (layout
/// pixels).
const LABEL_PAD: [f32; 2] = [10.0, 4.0];
const LABEL_GAP: f32 = 4.0;

/// What is being chosen over its action slot, fading (`HudView::action_label`),
/// in the HUD's own look: the target's action words (`LBL_ACTIONDESC`: its
/// font and colour) in the frame round the action bar (`LBL_MOULDING3`, else
/// the words' own backing), sized to the words, at least the frame's corners
/// tall, and kept inside the layout.
fn action_label(gui: &Gui, hud: &HudView, painter: &mut Painter, out: &mut DrawList) {
    let Some((nth, words, alpha)) = hud.action_label.as_ref() else {
        return;
    };
    let (Some(button), Some(backing), Some(desc)) = (
        gui.find(&format!("BTN_ACTION{nth}")),
        gui.find("LBL_MOULDING3")
            .or_else(|| gui.find("LBL_ACTIONDESCBG")),
        gui.find("LBL_ACTIONDESC"),
    ) else {
        return;
    };
    let source = painter.source;
    let Some((width, line)) = desc
        .text
        .as_ref()
        .and_then(|text| painter.textures.font(source, &text.font))
        .map(|(_, font)| (font.measure(words), font.line_height()))
    else {
        return;
    };
    let [x, y, w, _] = rect_of(button);
    let right = gui.extent.width as f32;
    let corners = backing
        .border
        .as_ref()
        .map_or(0.0, |border| 2.0 * border.dimension as f32);
    let size = [
        (width + 2.0 * LABEL_PAD[0]).max(corners),
        (line + 2.0 * LABEL_PAD[1]).max(corners),
    ];
    let left =
        (x + (w - size[0]) / 2.0).clamp(LABEL_GAP, (right - size[0] - LABEL_GAP).max(LABEL_GAP));
    let top = y - LABEL_GAP - size[1];
    let placed = |control: &Control| {
        let mut control = control.clone();
        control.extent.left = left.round() as i32;
        control.extent.top = top.round() as i32;
        control.extent.width = size[0].round() as i32;
        control.extent.height = size[1].round() as i32;
        control
    };
    let from = out.quads.len();
    painter.control(&placed(backing), &ControlState::default(), out);
    let state = ControlState {
        text: Some(words.clone()),
        ..ControlState::default()
    };
    painter.control(&placed(desc), &state, out);
    for quad in &mut out.quads[from..] {
        quad.color[3] *= alpha;
    }
}

/// How the banner layer shows `control`: the line in KOTOR's combat message
/// (the top middle) on its backing; nothing else.
pub fn banner(control: &Control, vm: &ViewModel) -> ControlState {
    match (control.tag.as_str(), &vm.calls.banner) {
        ("LBL_CMBTMODEMSG", Some(line)) => crate::screens::tab::text(line),
        ("LBL_CMBTMSGBG", Some(_)) => ControlState::default(),
        _ => hidden(),
    }
}

/// A toggle of the layout's lit: KOTOR's stealth and solo mode.
fn lit() -> ControlState {
    ControlState {
        selected: true,
        ..ControlState::default()
    }
}

/// How the HUD shows `control`.
pub fn state(control: &Control, vm: &ViewModel) -> ControlState {
    let hud = vm.hud.clone().unwrap_or(HudView::default());
    let tag = control.tag.as_str();
    let hidden = hidden();
    let shown = |on: bool| {
        if on {
            ControlState::default()
        } else {
            hidden.clone()
        }
    };
    if let Some((place, arrow)) = level_up_control(tag) {
        let on = vm.hud.is_some() && vm.party.get(place).is_some_and(|member| member.may_level);
        return if arrow { level_up_mark(on) } else { shown(on) };
    }
    // The icons are drawn by `effects`, the labels only their places.
    if tag.starts_with(GOOD_ARROW) || tag.starts_with(BAD_ARROW) {
        return hidden;
    }
    if let Some(slot) = tag.strip_prefix(DEBILITATED) {
        let place = slot
            .parse::<usize>()
            .ok()
            .and_then(|slot| slot.checked_sub(1));
        let member = place.and_then(|place| vm.party.get(place));
        return shown(vm.hud.is_some() && member.is_some_and(|member| member.debilitated));
    }
    match tag {
        "LBL_MAPBORDER" => return shown(hud.minimap.is_some()),
        "TB_STEALTH" if hud.stealth => return lit(),
        "TB_SOLO" if hud.solo => return lit(),
        _ => {}
    }
    if let Some(shown) = target_state(tag, vm.target.as_ref()) {
        return shown;
    }
    if let Some((place, prefix)) = companion(tag) {
        return companion_state(vm.party.get(place), prefix);
    }
    if STATUS.contains(&tag) {
        return shown(hud.lit.contains(&tag));
    }
    // The swap button answers only while a character is on the HUD
    // (`swap_at`).
    if HIDDEN.iter().any(|prefix| tag.starts_with(prefix)) || (tag == SWAP && vm.hud.is_none()) {
        return hidden;
    }
    let slot = |prefix: &str| {
        tag.strip_prefix(prefix)
            .and_then(|slot| slot.parse::<usize>().ok())
    };
    if let Some(slot) = slot("BTN_ACTION") {
        return if slot < hud.actions.len() {
            ControlState::default()
        } else {
            hidden
        };
    }
    if let Some(slot) = slot("LBL_ACTION") {
        return match hud.actions.get(slot) {
            Some((icon, _)) => ControlState {
                fill: Some(icon.clone()),
                text: Some(Vec::new()),
                ..ControlState::default()
            },
            None => hidden,
        };
    }
    match tag {
        "PB_VIT1" => vitality((hud.hp, hud.poisoned)),
        "PB_FORCE1" if hud.fp.1 > 0 => ControlState {
            value: Some(share(hud.fp)),
            ..ControlState::default()
        },
        "PB_FORCE1" => hidden,
        "LBL_CHAR1" => ControlState {
            fill: Some(hud.portrait.unwrap_or(String::new())),
            ..ControlState::default()
        },
        _ => ControlState::default(),
    }
}
