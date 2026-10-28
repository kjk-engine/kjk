//! Every layout of an install parsed and painted at rest: what does not
//! parse, control kinds we do not know, and what does not resolve.

use std::collections::BTreeMap;

use fmt_odyssey::gui::{Gui, Kind};
use vfs::{Discovery, Game};

use crate::assets::GuiAssets;
use crate::draw::DrawList;
use crate::paint::{ControlState, Painter, Palette};

pub fn palette(game: Game) -> Palette {
    match game {
        Game::Kotor2 => Palette::TSL,
        _ => Palette::K1,
    }
}

/// `gui` painted with every control as the layout has it, and what did
/// not resolve.
pub fn paint(assets: &mut GuiAssets, gui: &Gui) -> (DrawList, Vec<String>) {
    let game = assets.game;
    let mut out = DrawList::default();
    let mut painter = Painter::new(assets, palette(game));
    painter.layout(gui, &|_| ControlState::default(), &mut out);
    (out, painter.refusals)
}

/// Textures K1's layouts name that the game fills at run time
/// (portraits, save shots, planets): not missing.
pub const PLACEHOLDERS: [&str; 7] = [
    "galaxymap: lbl_live02",
    "galaxymap: lbl_live03",
    "galaxymap: lbl_live04",
    "galaxymap: lbl_live05",
    "partyselection: po_no",
    "saveload: screen",
    "saveload: georgem",
];

/// What a sweep of one install found.
#[derive(Default)]
pub struct Sweep {
    pub layouts: usize,
    pub failed: Vec<String>,
    /// Refusal kind (`texture`, `font`) to the layouts and names refused.
    pub refusals: BTreeMap<String, Vec<String>>,
    pub unknown_kinds: Vec<String>,
    /// Layouts whose root is not at 0, 0 (with its extent).
    pub offset_roots: Vec<String>,
    /// Unread field labels (without the tag) and how often.
    pub unread: BTreeMap<String, usize>,
}

impl Sweep {
    /// Every layout parses, with control kinds we know and fonts that
    /// resolve; for K1 every texture too, but the run-time placeholders.
    pub fn passes(&self, game: Game) -> bool {
        let textures_ok = game != Game::Kotor1
            || self
                .refusals
                .get("texture")
                .into_iter()
                .flatten()
                .all(|what| PLACEHOLDERS.contains(&what.as_str()));
        self.failed.is_empty()
            && self.unknown_kinds.is_empty()
            && !self.refusals.contains_key("font")
            && textures_ok
    }
}

pub fn sweep_install(discovery: &Discovery, game: Game) -> Result<Sweep, String> {
    let mut assets = GuiAssets::open(discovery, game)?;
    let mut sweep = Sweep::default();
    for name in assets.layouts() {
        sweep.layouts += 1;
        let gui = match assets.layout(&name) {
            Ok(gui) => gui,
            Err(error) => {
                sweep.failed.push(error);
                continue;
            }
        };
        if gui.extent.left != 0 || gui.extent.top != 0 {
            sweep.offset_roots.push(format!("{name} {:?}", gui.extent));
        }
        for control in gui.walk() {
            if let Kind::Other(number) = control.kind {
                sweep
                    .unknown_kinds
                    .push(format!("{name}/{} {number}", control.tag));
            }
        }
        for label in &gui.unread {
            let field = label.rsplit('.').next().unwrap_or(label.as_str());
            *sweep.unread.entry(field.to_string()).or_insert(0) += 1;
        }
        let (_, refusals) = paint(&mut assets, &gui);
        for why in refusals {
            let (kind, what) = why.split_once(' ').unwrap_or(("other", why.as_str()));
            sweep
                .refusals
                .entry(kind.to_string())
                .or_default()
                .push(format!("{name}: {what}"));
        }
    }
    Ok(sweep)
}
