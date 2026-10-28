//! Joining a game (`savename`'s edit box): the host's address (or a Steam
//! ID, `steam:<id>`, or a Steam friend's name), then the player's name; OK
//! on the name joins. An address that cannot be one is kept, greyed, until
//! it is changed.

use fmt_odyssey::gui::Control;

use crate::edit::EditBox;
use crate::intent::{Intent, SessionIntent};
use crate::paint::ControlState;
use crate::screen::{Screen, ScreenId};
use crate::strings;
use crate::vm::ViewModel;

/// The longest address or name typed.
const LONGEST: usize = 64;

pub struct Join {
    address: EditBox,
    name: EditBox,
    /// On the name (the address was taken).
    naming: bool,
    /// The address typed was refused, and has not changed since.
    refused: Option<Vec<u8>>,
}

impl Join {
    pub fn new() -> Join {
        Join {
            address: EditBox::with("127.0.0.1", LONGEST),
            name: EditBox::with(strings::DEFAULT_NAME, LONGEST),
            naming: false,
            refused: None,
        }
    }
}

impl Default for Join {
    fn default() -> Join {
        Join::new()
    }
}

/// `host`, `host:port` or `[v6]:port`, with a port from 1 to 65535; or
/// `steam:<id>`, or a Steam friend's name (no `:`, spaces allowed).
pub fn valid_address(address: &str) -> bool {
    if let Some(id) = address.strip_prefix("steam:") {
        return id.parse::<u64>().is_ok_and(|id| id > 0);
    }
    if address.trim().is_empty() {
        return false;
    }
    if !address.contains(':') && !address.starts_with('[') {
        return true;
    }
    if address.contains(char::is_whitespace) {
        return false;
    }
    let port_ok = |port: &str| port.parse::<u16>().is_ok_and(|port| port > 0);
    if let Some(rest) = address.strip_prefix('[') {
        return match rest.split_once(']') {
            Some((host, "")) => !host.is_empty(),
            Some((host, port)) => !host.is_empty() && port.strip_prefix(':').is_some_and(port_ok),
            None => false,
        };
    }
    match address.split_once(':') {
        None => true,
        Some((host, port)) => !host.is_empty() && !port.contains(':') && port_ok(port),
    }
}

impl Screen for Join {
    fn id(&self) -> ScreenId {
        ScreenId("join")
    }

    fn layout(&self) -> &str {
        "savename"
    }

    fn state(&self, control: &Control, _: &ViewModel) -> ControlState {
        let text = |words: &str| ControlState {
            text: Some(strings::bytes(words)),
            ..ControlState::default()
        };
        match control.tag.as_str() {
            "LBL_TITLE" if self.naming => text(strings::JOIN_NAME),
            "LBL_TITLE" => text(strings::JOIN_ADDRESS),
            "BTN_OK" => text(strings::OK),
            "BTN_CANCEL" => text(strings::CANCEL),
            "EDITBOX" => ControlState {
                disabled: !self.naming && self.refused.as_ref() == Some(&self.address.text),
                ..ControlState::default()
            },
            _ => ControlState::default(),
        }
    }

    fn press(&mut self, tag: &str, _: &ViewModel, out: &mut Vec<Intent>) {
        match tag {
            "BTN_OK" if !self.naming => {
                if valid_address(&self.address.as_str()) {
                    self.naming = true;
                } else {
                    self.refused = Some(self.address.text.clone());
                }
            }
            "BTN_OK" if !self.name.text.is_empty() => {
                out.push(Intent::Session(SessionIntent::Join {
                    address: self.address.as_str(),
                    name: self.name.as_str(),
                }));
            }
            "BTN_CANCEL" => self.back(out),
            _ => {}
        }
    }

    fn back(&mut self, out: &mut Vec<Intent>) {
        if self.naming {
            self.naming = false;
        } else {
            out.push(Intent::Close);
        }
    }

    fn edit(&mut self, tag: &str) -> Option<&mut EditBox> {
        match tag {
            "EDITBOX" if self.naming => Some(&mut self.name),
            "EDITBOX" => Some(&mut self.address),
            _ => None,
        }
    }

    fn typing(&self) -> Option<&str> {
        Some("EDITBOX")
    }

    fn first_focus(&self) -> Option<&str> {
        Some("BTN_OK")
    }
}
