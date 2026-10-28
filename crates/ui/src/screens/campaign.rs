//! Which campaign a new game begins (`campaign:<then>`, `then` as character
//! creation takes it: `solo`, `host:<seats>:<port>`), when both games are
//! installed: the message box (`confirm`) asking, its two buttons KOTOR and
//! The Sith Lords. One picked, the screens read its data (`Intent::
//! Campaign`) and its character creation opens (`chargen:<then>:<key>`).
//! Back closes it; character creation backed out of puts the screens on
//! K1's data again.

use fmt_odyssey::gui::{Control, Gui};

use crate::intent::Intent;
use crate::paint::ControlState;
use crate::screen::{Screen, ScreenId};
use crate::screens::pages::description;
use crate::strings;
use crate::vm::ViewModel;

/// The two buttons' campaigns and words.
const CHOICES: [(&str, vfs::Game, &str); 2] = [
    ("BTN_OK", vfs::Game::Kotor1, strings::KOTOR),
    ("BTN_CANCEL", vfs::Game::Kotor2, strings::SITH_LORDS),
];

pub struct Campaign {
    then: String,
}

impl Campaign {
    pub fn new(then: &str) -> Campaign {
        Campaign {
            then: then.to_string(),
        }
    }
}

/// The intents that begin a new game of `campaign` for `then`: the screens
/// on its data, then its character creation.
pub fn begin(campaign: vfs::Game, then: &str) -> [Intent; 2] {
    [
        Intent::Campaign(campaign),
        Intent::OpenFor("chargen", format!("{then}:{}", campaign.key())),
    ]
}

impl Screen for Campaign {
    fn id(&self) -> ScreenId {
        ScreenId("campaign")
    }

    fn layout(&self) -> &str {
        "confirm"
    }

    fn state(&self, control: &Control, _: &ViewModel) -> ControlState {
        let tag = control.tag.as_str();
        if let Some((_, _, words)) = CHOICES.iter().find(|(button, ..)| *button == tag) {
            return ControlState {
                text: Some(strings::bytes(words)),
                ..ControlState::default()
            };
        }
        match tag {
            "LB_MESSAGE" => description(&strings::bytes(strings::CAMPAIGN)),
            _ => ControlState::default(),
        }
    }

    fn press(&mut self, tag: &str, _: &ViewModel, out: &mut Vec<Intent>) {
        if let Some((_, campaign, _)) = CHOICES.iter().find(|(button, ..)| *button == tag) {
            out.push(Intent::Close);
            out.extend(begin(*campaign, &self.then));
        }
    }

    fn first_focus(&self) -> Option<&str> {
        Some("BTN_OK")
    }

    /// The box grown for its question, its buttons as wide as the question
    /// for the campaigns' names.
    fn shape(&self, gui: &mut Gui) {
        super::message::grow(gui);
        let Some(across) = gui
            .find("LB_MESSAGE")
            .map(|list| (list.extent.left, list.extent.width))
        else {
            return;
        };
        for control in &mut gui.controls {
            if CHOICES.iter().any(|(button, ..)| *button == control.tag) {
                (control.extent.left, control.extent.width) = across;
            }
        }
    }
}
