//! KOTOR's area transition panel (`areatransition`, KOTOR 2's
//! `areatrans_p` with the skin): the name of the area the door or exit the
//! player faces leads to (`sim::transition`), on its strip with the
//! transition icon beneath, drawn over the HUD and under the screens, and
//! not a screen: nothing on it takes the focus. It stands where KOTOR 2
//! puts it (`place`): across the middle of the window, near its top, or
//! under the bark bubble while one shows. The name is in the colour the
//! campaign's own layout gives it (K1's blue), as the bark bubble's words
//! are. While a member of the party is down, the strip, the icon and the
//! name are drawn grey, as KOTOR 2 draws them (`shown`).

use fmt_odyssey::gui::{Control, Gui, Rect};

use crate::assets::GuiAssets;
use crate::paint::ControlState;
use crate::skin::Source;

pub const LAYOUT: &str = "areatransition";
/// The label the name goes in.
const WORDS: &str = "LBL_DESCRIPTION";
/// The panel's layout as the screens draw it (the skin's), and the colour
/// of its name as the campaign's own layout has it.
pub struct Panel {
    pub gui: Gui,
    pub source: Source,
    pub words: Option<[f32; 3]>,
}

impl Panel {
    /// `None` when the screens' layout does not read.
    pub fn read(assets: &GuiAssets) -> Option<Panel> {
        let (gui, source) = assets.screen_layout(LAYOUT).ok()?;
        let own = assets.layout(LAYOUT).ok();
        let words = own.and_then(|own| own.find(WORDS)?.text.as_ref()?.color);
        Some(Panel { gui, source, words })
    }
}

/// What the panel says for a destination named `name`
/// (`TransitionDestin`): KOTOR's client drops what comes before the first
/// dash, the dash and the one character after it ("Taris - Upper City
/// South" shows "Upper City South"); a name without a dash shows whole.
pub fn words(name: &[u8]) -> Vec<u8> {
    match name.iter().position(|&byte| byte == b'-') {
        Some(dash) => name.get(dash + 2..).map_or(Vec::new(), <[u8]>::to_vec),
        None => name.to_vec(),
    }
}

/// The panel's width at a text size of 1, in physical pixels: KOTOR 2's
/// own number, which is also its layout's. KOTOR 2 makes the panel 82
/// tall, but nothing of the panel itself is drawn, only its labels.
const PANEL: f32 = 400.0;
/// The strip's height, as wide as the panel.
const STRIP: f32 = 32.0;
/// The icon's side and its top.
const ICON: (f32, f32) = (50.0, 27.0);
/// The name's width, height and top.
const NAME: (f32, f32, f32) = (348.0, 20.0, 6.0);
/// The strip's tag and the icon's (the name's is `WORDS`).
const STRIP_TAG: &str = "LBL_TEXTBG";
const ICON_TAG: &str = "LBL_ICON";

/// `gui` (the layout as read) placed as KOTOR 2 places it in a window of
/// `window` physical pixels, with Aspyr's text size `text`
/// (`draw::text_size`). The frame returned is the window, one layout pixel
/// to a physical pixel.
///
/// KOTOR 2 loads the layout stretched, as every screen, and then sets the
/// panel's size and its three labels' sizes itself. Each size is its own
/// number times the text size, and does not stretch with the window: the
/// panel is 400 × 82 at a text size of 1. The panel is centred across the
/// window. Its top is the layout's top stretched (80 pixels of 600 down
/// the window), and while a bark bubble shows, the panel stands right
/// under the bubble instead (`under`, its bottom in physical pixels). The
/// strip spans the panel's top, 32 tall. The icon (50 square, 27 down) and
/// the name (348 × 20, 6 down) are centred across the panel. KOTOR 2 sets
/// these from its own numbers whatever the layout says, so a mod's layout
/// only gives the art and the font.
pub fn place(gui: &Gui, window: (i32, i32), text: f32, under: Option<i32>) -> Gui {
    let sized = |at: f32| (at * text) as i32;
    let width = sized(PANEL);
    let stretched_top = (gui.extent.top as f32 * (window.1 as f32 / 600.0)) as i32;
    let (left, top) = (window.0 / 2 - width / 2, under.unwrap_or(stretched_top));
    let centred = |wide: i32, high: i32, down: i32| Rect {
        left: width / 2 - wide / 2,
        top: down,
        width: wide,
        height: high,
    };
    let controls = gui
        .controls
        .iter()
        .map(|control| {
            let mut control = control.clone();
            let own = control.extent;
            control.extent = match control.tag.as_str() {
                STRIP_TAG => Rect {
                    left: 0,
                    top: 0,
                    width,
                    height: sized(STRIP),
                },
                ICON_TAG => centred(sized(ICON.0), sized(ICON.0), sized(ICON.1)),
                WORDS => centred(sized(NAME.0), sized(NAME.1), sized(NAME.2)),
                _ => Rect {
                    left: sized(own.left as f32),
                    top: sized(own.top as f32),
                    width: sized(own.width as f32),
                    height: sized(own.height as f32),
                },
            };
            control.extent.left += left;
            control.extent.top += top;
            control
        })
        .collect();
    Gui {
        extent: Rect {
            left: 0,
            top: 0,
            width: window.0,
            height: window.1,
        },
        controls,
        ..gui.clone()
    }
}

/// The colour KOTOR 2 draws the strip and the icon in while the party is
/// not healthy (`healthy`), and the name's colour then.
const GREY: [f32; 3] = [0.35, 0.35, 0.35];
const GREY_WORDS: [f32; 3] = [0.4, 0.4, 0.3];

/// Whether KOTOR 2 counts the party healthy, for each member's current and
/// most hit points in `party`: every member has more than 0 hit points.
pub fn healthy(party: impl IntoIterator<Item = (i32, i32)>) -> bool {
    party.into_iter().all(|(current, _)| current > 0)
}

/// `gui` (placed by `place`) as KOTOR 2 draws it while the party is not
/// healthy, or as it was when it is: KOTOR 2 then gives the strip's and
/// the icon's borders its grey (`GREY`), and kjk draws a border's fill in
/// its border's colour only when the border has one.
pub fn shown(gui: &mut Gui, healthy: bool) {
    if healthy {
        return;
    }
    for control in &mut gui.controls {
        if [STRIP_TAG, ICON_TAG].contains(&control.tag.as_str())
            && let Some(border) = control.border.as_mut()
        {
            border.color = Some(GREY);
        }
    }
}

/// How the panel shows `control`: the label `words`, in `colour` where
/// there is one; the rest as the layout draws them. While the party is not
/// `healthy`, the strip and the icon are grey and the name greyish yellow
/// (`GREY`, `GREY_WORDS`), as KOTOR 2 draws them.
pub fn state(
    control: &Control,
    words: &[u8],
    colour: Option<[f32; 3]>,
    healthy: bool,
) -> ControlState {
    let tag = control.tag.as_str();
    let label = tag == WORDS;
    let tint = match (healthy, tag) {
        (true, _) => colour.filter(|_| label),
        (false, WORDS) => Some(GREY_WORDS),
        (false, STRIP_TAG | ICON_TAG) => Some(GREY),
        (false, _) => None,
    };
    ControlState {
        text: label.then(|| words.to_vec()),
        tint,
        ..ControlState::default()
    }
}
