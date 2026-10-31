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
//! Each portrait shows its effects as KOTOR's do (`effect_arrows`): an
//! arrow up for each good one, down for each bad, stacked; a debilitated
//! member's portrait its overlay instead (`LBL_DEBILATATED<n>`).
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
use crate::vm::{Busy, HudView, MemberView, MinimapView, TargetView, ViewModel};

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

/// `gui`'s frame for a window of `window` pixels, drawn at the layout's own
/// size (`draw::fit`): the window itself, or, in a window smaller than
/// the layout, the window's shape (`shaped`).
pub fn fitted(gui: &Gui, window: (f32, f32)) -> (i32, i32) {
    let (width, height) = gui.frame();
    let (scale, _) = crate::draw::fit(window, (width as f32, height as f32));
    if scale < 1.0 {
        return shaped(gui, window);
    }
    // Whole pixels no larger than the window, so it is drawn unscaled.
    (window.0.floor() as i32, window.1.floor() as i32)
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
        "PB_VIT" => ControlState {
            value: Some(share(member.hp)),
            ..ControlState::default()
        },
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
/// `LBL_CMBTEFCTINC<n>`, an arrow up) and the bad's (`…RED<n>`, down).
const GOOD_ARROW: &str = "LBL_CMBTEFCTINC";
const BAD_ARROW: &str = "LBL_CMBTEFCTRED";
/// A member's debilitated overlay but its party slot.
const DEBILITATED: &str = "LBL_DEBILATATED";

/// How far apart a portrait's arrows stack (layout pixels), from
/// `CSWGuiMainInterfaceChar::DrawStack`: up to 4, 10 on the leader's
/// portrait and 5 on a companion's; past that the leader's close up (10,
/// 8, 7, 6, 5 for 5 to 9) and a companion's stay 5.
const STEP_FEW: [f32; 2] = [5.0, 10.0];
const STEP_MANY: [f32; 5] = [10.0, 8.0, 7.0, 6.0, 5.0];
/// The most arrows a companion's portrait and the leader's show.
const MOST: [u8; 2] = [5, 9];

/// One arrow where `DrawStack` puts it: its label's art, at its label's
/// size, and the portrait it is clipped to.
#[derive(Debug, Clone, PartialEq)]
pub struct Arrow {
    pub fill: String,
    pub rect: [f32; 4],
    pub clip: [f32; 4],
}

/// Every portrait's effect arrows (`MemberView::effects`), as KOTOR's
/// `CSWGuiMainInterface::UpdatePortraits` and `DrawEffects` draw them: none
/// on a debilitated member; else as many as its good effects in its
/// `LBL_CMBTEFCTINC<n>` (capped, `MOST`), centred on the label's place and
/// `STEP_*` apart, each drawn from the top down, and its bad ones in
/// `LBL_CMBTEFCTRED<n>`, drawn from the bottom up; all within the portrait.
pub fn effect_arrows(gui: &Gui, vm: &ViewModel) -> Vec<Arrow> {
    if vm.hud.is_none() {
        return Vec::new();
    }
    let mut arrows = Vec::new();
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
        for (prefix, count, down) in [
            (GOOD_ARROW, member.effects.0, true),
            (BAD_ARROW, member.effects.1, false),
        ] {
            let Some(label) = gui.find(&format!("{prefix}{slot}")) else {
                continue;
            };
            let Some(fill) = label.border.as_ref().map(|border| border.fill.clone()) else {
                continue;
            };
            let count = count.min(MOST[leader]);
            if count == 0 {
                continue;
            }
            let step = if count <= 4 || leader == 0 {
                STEP_FEW[leader]
            } else {
                STEP_MANY[usize::from(count - 5)]
            };
            let [left, top, width, height] = rect_of(label);
            let half = (step / 2.0).floor() * f32::from(count - 1);
            let (first, by) = if down {
                (top - half, step)
            } else {
                (top + half, -step)
            };
            arrows.extend((0..count).map(|nth| Arrow {
                fill: fill.clone(),
                rect: [left, first + by * f32::from(nth), width, height],
                clip,
            }));
        }
    }
    arrows
}

/// The effect arrows drawn (`effect_arrows`).
pub fn effects(gui: &Gui, vm: &ViewModel, painter: &mut Painter, out: &mut DrawList) {
    for arrow in effect_arrows(gui, vm) {
        if let Some(id) = painter.textures.texture(painter.source, &arrow.fill) {
            out.quads.push(Quad {
                additive: painter.textures.additive(id),
                clip: Some(arrow.clip),
                ..Quad::image(arrow.rect, id, [1.0; 4])
            });
        }
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
    for (nth, (_, left)) in hud.actions.iter().enumerate() {
        let Some(button) = gui.find(&format!("BTN_ACTION{nth}")) else {
            continue;
        };
        let [x, y, w, h] = rect_of(button);
        if let Some(key) = hud.action_keys.get(nth) {
            let rect = |dx: f32, dy: f32| [x + 2.0 + dx, y + 2.0 + dy, w - 4.0, 18.0];
            write(key, &rect, align::CENTRE | align::TOP, palette.hilight);
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
    // The arrows are drawn by `effects`, the labels only their places.
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
        "TB_STEALTH" if hud.stealth => {
            return ControlState {
                selected: true,
                ..ControlState::default()
            };
        }
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
        "PB_VIT1" => ControlState {
            value: Some(share(hud.hp)),
            ..ControlState::default()
        },
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
