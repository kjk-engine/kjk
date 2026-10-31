//! KOTOR's area transition panel (`areatransition`, KOTOR 2's
//! `areatrans_p` with the skin): the name of the area the door or exit the
//! player faces leads to (`sim::transition`), on its strip with the
//! transition icon beneath, drawn over the HUD and under the screens, and
//! not a screen: nothing on it takes the focus. KOTOR centres it across
//! the top of the screen; here it stands centred under the target block
//! and its words (`hud::block_bottom`), which a door leading out fills too.
//! The name is in the colour the campaign's own layout gives it (K1's
//! blue), as the bark bubble's words are.

use fmt_odyssey::gui::{Control, Gui, Rect};

use crate::assets::GuiAssets;
use crate::paint::ControlState;
use crate::skin::Source;

pub const LAYOUT: &str = "areatransition";
/// The label the name goes in.
const WORDS: &str = "LBL_DESCRIPTION";
/// How far below the target block's words the panel stands (layout
/// pixels).
const GAP: i32 = 4;

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

/// `gui` (the layout as read) placed in a frame of `frame` layout pixels
/// (the HUD's, `hud::anchor`): centred across it, its top `GAP` below
/// `below`.
pub fn place(gui: &Gui, frame: (i32, i32), below: i32) -> Gui {
    let (left, top) = ((frame.0 - gui.extent.width) / 2, below + GAP);
    let controls = gui
        .controls
        .iter()
        .map(|control| {
            let mut control = control.clone();
            control.extent.left += left;
            control.extent.top += top;
            control
        })
        .collect();
    Gui {
        extent: Rect {
            left: 0,
            top: 0,
            width: frame.0,
            height: frame.1,
        },
        controls,
        ..gui.clone()
    }
}

/// How the panel shows `control`: the label `words`, in `colour` where
/// there is one; the rest as the layout draws them.
pub fn state(control: &Control, words: &[u8], colour: Option<[f32; 3]>) -> ControlState {
    let label = control.tag == WORDS;
    ControlState {
        text: label.then(|| words.to_vec()),
        tint: colour.filter(|_| label),
        ..ControlState::default()
    }
}
