//! KOTOR's bark bubble (`barkbubble`, KOTOR 2's `barkbubble_p` with the
//! skin): a one-liner's words or a script's `BarkString`, drawn over the
//! HUD and under the screens, and not a screen: nothing on it takes the
//! focus.
//!
//! The panel grows to the words: the label as tall as its lines (each its
//! font's line), the panel that and the label's left again above and below
//! it (reone's reading). Over a speaker that is drawn, the panel's bottom
//! centre is the point the view gives, kept inside the window; with none
//! (nobody speaks, or a minigame hides everyone) the panel stands where the
//! layout puts it: as far across from the frame's middle, and as far down
//! the window.
//!
//! It is drawn at `SCALE` of a layout stretched to the window's height,
//! filling the window (`DrawList::fill`), its words in the colour the
//! campaign's own layout gives them (K1's blue), not the skin's.

use fmt_odyssey::gui::{Control, Gui, Kind, Rect};

use crate::assets::GuiAssets;
use crate::draw::Textures;
use crate::paint::{ControlState, wrap};
use crate::skin::Source;
use crate::vm::BarkView;

pub const LAYOUT: &str = "barkbubble";
const LABEL: &str = "LBL_BARKTEXT";
/// The panel the layout's root becomes (`place`).
const PANEL: &str = "PNL_BARK";
/// The bubble's size beside a layout stretched to the window's height
/// (the owner's call, 2026-10-03, when the HUD was drawn so: at the HUD's
/// size its box and words were too big): its frame this much larger than
/// the window's shaped layout frame (`hud::shaped`).
pub const SCALE: f32 = 0.6;

/// The bubble's layout as the screens draw it (the skin's), and the colour
/// of its words as the campaign's own layout has them.
pub struct Bubble {
    pub gui: Gui,
    pub source: Source,
    pub words: Option<[f32; 3]>,
}

impl Bubble {
    /// `None` when the screens' layout does not read.
    pub fn read(assets: &GuiAssets) -> Option<Bubble> {
        let (gui, source) = assets.screen_layout(LAYOUT).ok()?;
        let own = assets.layout(LAYOUT).ok();
        let words = own.and_then(|own| own.find(LABEL)?.text.as_ref()?.color);
        Some(Bubble { gui, source, words })
    }
}

/// `gui` (the layout as read) placed for `bark` in a window of `window`
/// pixels: a frame of the window's shape, holding the layout's root as a
/// panel and its label, both sized to the words. `None` when the layout
/// has no label.
pub fn place(
    gui: &Gui,
    textures: &mut dyn Textures,
    source: Source,
    bark: &BarkView,
    window: (f32, f32),
) -> Option<Gui> {
    let mut label = gui.controls.iter().find(|c| c.tag == LABEL)?.clone();
    let font = label.text.as_ref().map(|words| words.font.clone());
    let lines = font
        .and_then(|font| textures.font(source, &font))
        .map_or((1, 0.0), |(_, font)| {
            let lines = wrap(&font, &bark.text, label.extent.width as f32).len();
            (lines.max(1), font.line_height())
        });
    let pad = label.extent.left;
    label.extent.height = (lines.0 as f32 * lines.1).ceil() as i32;
    let (width, height) = (gui.extent.width, label.extent.height + 2 * pad);
    let (frame_width, frame_height) = gui.frame();
    let (wide, tall) = crate::screens::hud::shaped(gui, window);
    let larger = |side: i32| (side as f32 / SCALE).round() as i32;
    let (wide, tall) = (larger(wide), larger(tall));
    let (left, top) = match bark.over {
        Some([x, y]) => (
            (x * wide as f32).round() as i32 - width / 2,
            (y * tall as f32).round() as i32 - height,
        ),
        None => (
            gui.extent.left + (wide - frame_width) / 2,
            gui.extent.top * tall / frame_height,
        ),
    };
    let (left, top) = (
        left.clamp(0, (wide - width).max(0)),
        top.clamp(0, (tall - height).max(0)),
    );
    label.extent.left += left;
    label.extent.top += top;
    let panel = Control {
        kind: Kind::Panel,
        tag: PANEL.into(),
        extent: Rect {
            left,
            top,
            width,
            height,
        },
        border: gui.border.clone(),
        ..Control::default()
    };
    Some(Gui {
        extent: Rect {
            left: 0,
            top: 0,
            width: wide,
            height: tall,
        },
        border: None,
        controls: vec![panel, label],
        ..gui.clone()
    })
}

/// How the bubble shows `control`: the label the words, in `words`'
/// colour where there is one; the panel as the layout draws it.
pub fn state(control: &Control, bark: &BarkView, words: Option<[f32; 3]>) -> ControlState {
    let label = control.tag == LABEL;
    ControlState {
        text: label.then(|| bark.text.clone()),
        tint: words.filter(|_| label),
        ..ControlState::default()
    }
}
