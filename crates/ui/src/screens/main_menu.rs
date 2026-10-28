//! The main menu, KOTOR 2's (`mainmenu8x6_p`; KOTOR 1's `mainmenu16x12`
//! without the skin) with KOTOR 1's logos. Our entries take its first five
//! buttons by place, top to bottom, whatever the layout named them for
//! (new game, load, movies, music, options): Solo, Host, Join, Options,
//! Exit. Every other button and list (exit, more games, a mod's, the
//! developers' warp, the module list) is hidden; the downloadable-content
//! notice shows the build. Back does nothing here.

use fmt_odyssey::gui::Control;

use crate::intent::Intent;
use crate::paint::ControlState;
use crate::screen::{Screen, ScreenId};
use crate::screens::tab::{hidden, image};
use crate::skin::CAMPAIGN_LOGO;
use crate::strings;
use crate::vm::{SceneView, ViewModel};

pub struct MainMenu;

/// The seats a hosted game has unless told otherwise: KOTOR's party (I30
/// P1), the host and two guests.
pub const SEATS: u8 = 3;

/// Each button of ours: its control, its words, what it opens (none:
/// quit).
const ENTRIES: [(&str, &str, Option<&str>); 5] = [
    ("BTN_NEWGAME", strings::SOLO, Some("solo")),
    ("BTN_LOADGAME", strings::HOST, Some("host")),
    ("BTN_MOVIES", strings::JOIN, Some("join")),
    ("BTN_MUSIC", strings::OPTIONS, Some("options")),
    ("BTN_OPTIONS", strings::EXIT, None),
];

/// The downloadable-content notice's label, which shows the build instead.
const NEW_CONTENT: &str = "LBL_NEWCONTENT";

/// A button or list of the layout that none of ours is on.
fn unbound(tag: &str) -> bool {
    (tag.starts_with("BTN_") || tag.starts_with("LB_"))
        && !ENTRIES.iter().any(|(button, ..)| *button == tag)
}

impl Screen for MainMenu {
    fn id(&self) -> ScreenId {
        ScreenId("main_menu")
    }

    fn layout(&self) -> &str {
        "mainmenu16x12"
    }

    fn state(&self, control: &Control, vm: &ViewModel) -> ControlState {
        let tag = control.tag.as_str();
        if let Some((_, words, _)) = ENTRIES.iter().find(|(button, ..)| *button == tag) {
            return ControlState {
                text: Some(strings::bytes(words)),
                ..ControlState::default()
            };
        }
        match tag {
            "LBL_GAMELOGO" => image(Some(CAMPAIGN_LOGO.into())),
            // KOTOR 1's menu model turning behind the menu, over the
            // layout's backdrop.
            "LBL_3DVIEW" => ControlState {
                scene: Some(SceneView::MainMenu),
                ..ControlState::default()
            },
            "LBL_BW" => image(Some("lbl_bwlogo".into())),
            "LBL_LUCAS" => image(Some("lbl_lucasarts".into())),
            NEW_CONTENT => ControlState {
                text: Some(strings::bytes(&vm.session.version)),
                ..ControlState::default()
            },
            tag if unbound(tag) => hidden(),
            _ => ControlState::default(),
        }
    }

    fn press(&mut self, tag: &str, _: &ViewModel, out: &mut Vec<Intent>) {
        if let Some((.., opens)) = ENTRIES.iter().find(|(button, ..)| *button == tag) {
            out.push(opens.map_or(Intent::Quit, Intent::Open));
        }
    }

    fn back(&mut self, _: &mut Vec<Intent>) {}

    fn first_focus(&self) -> Option<&str> {
        Some("BTN_NEWGAME")
    }
}
