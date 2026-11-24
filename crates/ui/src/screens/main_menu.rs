//! The main menu, KOTOR 2's (`mainmenu8x6_p`; KOTOR 1's `mainmenu16x12`
//! without the skin), with KOTOR 1's logos in a KOTOR 1 campaign and its
//! own in a KOTOR 2 one. Our entries take its buttons by place, top to
//! bottom, whatever the layout named them for (new game, load, movies,
//! music, options, exit): Solo, Host, Join, Options, then when both games
//! are installed the other campaign's name (`The Sith Lords` / `KOTOR`) to
//! switch to it and Exit on the sixth; with one game installed, Exit on
//! the fifth and the sixth hidden. Every other button and list (more games,
//! a mod's, the developers' warp, the module list) is hidden; the
//! downloadable-content notice shows the build. Back does nothing here.

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

/// The first four buttons, always the same: Solo, Host, Join, Options.
const TOP: [(&str, &str, &str); 4] = [
    ("BTN_NEWGAME", strings::SOLO, "solo"),
    ("BTN_LOADGAME", strings::HOST, "host"),
    ("BTN_MOVIES", strings::JOIN, "join"),
    ("BTN_MUSIC", strings::OPTIONS, "options"),
];

/// The fifth and sixth buttons: Exit on the fifth when one game is
/// installed, or the campaign switch on the fifth and Exit on the sixth
/// when both are.
const FIFTH: &str = "BTN_OPTIONS";
const SIXTH: &str = "BTN_EXIT";

/// The downloadable-content notice's label, which shows the build instead.
const NEW_CONTENT: &str = "LBL_NEWCONTENT";

/// Whether both KOTOR games are installed, so the switch button is shown.
fn can_switch(vm: &ViewModel) -> bool {
    vm.campaigns.len() > 1
}

/// The campaign the switch button moves the menu to.
fn other_campaign(vm: &ViewModel) -> vfs::Game {
    match vm.session.campaign {
        Some(vfs::Game::Kotor2) => vfs::Game::Kotor1,
        _ => vfs::Game::Kotor2,
    }
}

/// A button or list of the layout that none of ours is on.
fn unbound(tag: &str, switch: bool) -> bool {
    (tag.starts_with("BTN_") || tag.starts_with("LB_"))
        && !TOP.iter().any(|(button, ..)| *button == tag)
        && tag != FIFTH
        && !(switch && tag == SIXTH)
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
        if let Some((_, words, _)) = TOP.iter().find(|(button, ..)| *button == tag) {
            return ControlState {
                text: Some(strings::bytes(words)),
                ..ControlState::default()
            };
        }
        let switch = can_switch(vm);
        if tag == FIFTH {
            let words = if switch {
                match other_campaign(vm) {
                    vfs::Game::Kotor2 => strings::KOTOR_2,
                    _ => strings::KOTOR_1,
                }
            } else {
                strings::EXIT
            };
            return ControlState {
                text: Some(strings::bytes(words)),
                ..ControlState::default()
            };
        }
        if switch && tag == SIXTH {
            return ControlState {
                text: Some(strings::bytes(strings::EXIT)),
                ..ControlState::default()
            };
        }
        let kotor2 = vm.session.campaign == Some(vfs::Game::Kotor2);
        match tag {
            "LBL_BW" | "LBL_LUCAS" => hidden(),
            // KOTOR 2's own logo on its campaign's menu.
            "LBL_GAMELOGO" if kotor2 => ControlState::default(),
            "LBL_GAMELOGO" => image(Some(CAMPAIGN_LOGO.into())),
            // The campaign's menu model behind the menu, over the layout's
            // backdrop (`gui_scene`).
            "LBL_3DVIEW" => ControlState {
                scene: Some(SceneView::MainMenu),
                wide: true,
                ..ControlState::default()
            },
            NEW_CONTENT => ControlState {
                text: Some(strings::bytes(&vm.session.version)),
                ..ControlState::default()
            },
            tag if unbound(tag, switch) => hidden(),
            _ => ControlState::default(),
        }
    }

    fn press(&mut self, tag: &str, vm: &ViewModel, out: &mut Vec<Intent>) {
        if let Some((.., opens)) = TOP.iter().find(|(button, ..)| *button == tag) {
            out.push(Intent::Open(opens));
            return;
        }
        let switch = can_switch(vm);
        if tag == FIFTH {
            if switch {
                out.push(Intent::Campaign(other_campaign(vm)));
            } else {
                out.push(Intent::Quit);
            }
        } else if switch && tag == SIXTH {
            out.push(Intent::Quit);
        }
    }

    fn back(&mut self, _: &mut Vec<Intent>) {}

    fn first_focus(&self) -> Option<&str> {
        Some("BTN_NEWGAME")
    }
}
