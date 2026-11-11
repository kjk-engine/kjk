//! Party selection (`partyselection`, KOTOR 2's `partyselect_p`): who
//! travels with the player, over the party table the host tells
//! (`vm::PartySelectionView`): each available companion's portrait on its
//! button (`LBL_CHARn`), the others not available (`LBL_NAn`), those
//! picked framed in KOTOR's green, the one chosen in its yellow and named,
//! the others in the plain frame; Add or Remove for the chosen, how many
//! may still be picked. It opens with the party as it
//! stands and whom the script forces in, who stay.
//!
//! A click chooses a companion, a double click adds or removes it.
//!
//! A script's screen (`ShowPartySelectionGUI`): Done sends the picks, Back
//! the party unchanged (`ChooseParty(None)`), and the host runs its
//! script. The screen opened in play only shows the table: Add is greyed,
//! Done and Back close it. Until the table comes every slot shows not
//! available. Slots past the campaign's own (its layout's: KOTOR 1 has
//! nine companions, KOTOR 2 twelve) are hidden. KOTOR 2's last two slots
//! take the place of two others, and only one slot of each pair shows.

use fmt_odyssey::gui::{Border, Control, Gui};

use crate::draw::{DrawList, Quad};
use crate::intent::{GameAction, Intent};
use crate::paint::{ControlState, Painter, rect_of};
use crate::screen::{Screen, ScreenId};
use crate::screens::tab::{enabled, hidden, image, text, told};
use crate::vm::{CompanionView, PartySelectionView, SceneView, ViewModel};

/// The most companions beside the player (KOTOR's party is three).
const FOLLOWERS: usize = 2;
/// KOTOR's frame round a companion picked.
const PICKED: [f32; 3] = [0.0, 0.831_373, 0.090_196];
/// "Add" and "Remove": the same strrefs in both games' talk tables; the
/// layouts carry Add on `BTN_ACCEPT`, the executable swaps in Remove.
const ADD: u32 = 38455;
const REMOVE: u32 = 38456;

#[derive(Default)]
pub struct Party {
    /// The companion chosen (its name shows; Add and Remove act on it).
    chosen: Option<u8>,
    /// Those picked, once the player changed them; else the table's.
    picks: Option<Vec<u8>>,
}

/// A companion slot past the campaign's (`BTN_NPC9`, `LBL_CHAR10`, … in
/// KOTOR 1).
fn beyond(tag: &str, table: Option<&PartySelectionView>) -> bool {
    let slots = table.map_or(usize::MAX, |table| table.slots);
    slot(tag, &["BTN_NPC", "LBL_CHAR", "LBL_NA"]).is_some_and(|slot| usize::from(slot) >= slots)
}

/// KOTOR 2's slots that stand in for another, each with the slot whose
/// place it takes: Hanharr's (10) for Mira's (7), and the Disciple's (11)
/// for the Handmaiden's (4). Its layout puts each pair in one place.
const STAND_INS: [(u8, u8); 2] = [(10, 7), (11, 4)];

/// Whether a slot of a stand-in pair is the one not drawn. KOTOR 2's
/// executable shows the stand-in's slot while that companion is available,
/// and the other slot otherwise. A mod that moves one slot of a pair and
/// not the other would otherwise show both.
fn stood_in(tag: &str, table: Option<&PartySelectionView>) -> bool {
    let Some(slot) = slot(tag, &["BTN_NPC", "LBL_CHAR", "LBL_NA"]) else {
        return false;
    };
    STAND_INS.iter().any(|&(stand_in, place)| {
        let available = companion(table, stand_in).is_some();
        (slot == stand_in && !available) || (slot == place && available)
    })
}

/// The slot a tag with one of `prefixes` names.
fn slot(tag: &str, prefixes: &[&str]) -> Option<u8> {
    prefixes
        .iter()
        .find_map(|prefix| tag.strip_prefix(prefix)?.parse().ok())
}

/// The companion available in slot `npc`.
fn companion(table: Option<&PartySelectionView>, npc: u8) -> Option<&CompanionView> {
    table?.companions.get(usize::from(npc))?.as_ref()
}

impl Party {
    fn table(vm: &ViewModel) -> Option<&PartySelectionView> {
        vm.party_selection.as_ref()
    }

    /// Those picked: the player's, else the forced and the party as it
    /// stands, `FOLLOWERS` at most.
    fn picks(&self, vm: &ViewModel) -> Vec<u8> {
        if let Some(picks) = &self.picks {
            return picks.clone();
        }
        let mut picks = Vec::new();
        if let Some(table) = Party::table(vm) {
            for &npc in table.forced.iter().chain(&table.members) {
                if companion(Some(table), npc).is_some()
                    && !picks.contains(&npc)
                    && picks.len() < FOLLOWERS
                {
                    picks.push(npc);
                }
            }
        }
        picks
    }

    /// What Add or Remove would do to the chosen companion: `Some(true)`
    /// adds it, `Some(false)` removes it, none when neither may.
    fn toggle(&self, vm: &ViewModel) -> Option<bool> {
        let table = Party::table(vm).filter(|table| table.open)?;
        let npc = self.chosen?;
        if !companion(Some(table), npc)?.selectable {
            return None;
        }
        let picks = self.picks(vm);
        if picks.contains(&npc) {
            (!table.forced.contains(&npc)).then_some(false)
        } else {
            (picks.len() < FOLLOWERS).then_some(true)
        }
    }

    /// The screen closes: a script's with the picks (`Some`) or the party
    /// unchanged.
    fn close(&self, picks: Option<Vec<u8>>, vm: &ViewModel, out: &mut Vec<Intent>) {
        if Party::table(vm).is_some_and(|table| table.open) {
            let picks = picks.map(|picks| {
                let mut two = [u8::MAX; 2];
                for (slot, npc) in two.iter_mut().zip(picks) {
                    *slot = npc;
                }
                two
            });
            out.push(Intent::Game(GameAction::ChooseParty(picks)));
        }
        out.push(Intent::Close);
    }
}

impl Screen for Party {
    /// A slot's double click adds or removes it, as Add or Remove says.
    fn double_button(&self, tag: &str) -> Option<&'static str> {
        slot(tag, &["BTN_NPC"]).map(|_| "BTN_ACCEPT")
    }

    /// Opened from the menu bar, the bumpers step its tabs (`key`).
    fn tab_bar(&self, vm: &ViewModel) -> bool {
        !Party::table(vm).is_some_and(|table| table.open)
    }

    fn id(&self) -> ScreenId {
        ScreenId("party")
    }

    fn layout(&self) -> &str {
        "partyselection"
    }

    fn state(&self, control: &Control, vm: &ViewModel) -> ControlState {
        let tag = control.tag.as_str();
        let table = Party::table(vm);
        // Shown: the one chosen, else the first picked (the view is never
        // empty while someone is in the party).
        let shown = self.chosen.or_else(|| self.picks(vm).first().copied());
        let chosen = shown.and_then(|npc| companion(table, npc));
        match tag {
            "LBL_COUNT" => text(FOLLOWERS - self.picks(vm).len()),
            "BTN_ACCEPT" => {
                let removes = self.chosen.is_some_and(|npc| self.picks(vm).contains(&npc));
                ControlState {
                    disabled: self.toggle(vm).is_none(),
                    ..told(Some(if removes { REMOVE } else { ADD }))
                }
            }
            "LBL_NPC_NAME" => match chosen {
                Some(chosen) => text(String::from_utf8_lossy(&chosen.name)),
                None => hidden(),
            },
            "LBL_NPC_LEVEL" => hidden(),
            "LBL_3D" => ControlState {
                scene: chosen.and_then(|chosen| {
                    Some(SceneView::Dressed {
                        appearance: chosen.appearance?,
                        armour: chosen.armour.clone(),
                    })
                }),
                ..ControlState::default()
            },
            tag if beyond(tag, table) || stood_in(tag, table) => hidden(),
            tag => {
                if let Some(npc) = slot(tag, &["LBL_CHAR"]) {
                    return match companion(table, npc) {
                        Some(shown) => image(shown.portrait.clone()),
                        None => ControlState::default(),
                    };
                }
                if let Some(npc) = slot(tag, &["LBL_NA"]) {
                    return ControlState {
                        hidden: companion(table, npc).is_some(),
                        ..ControlState::default()
                    };
                }
                // Its frame is drawn over it (`quads`): the button's own
                // highlight has none.
                if let Some(npc) = slot(tag, &["BTN_NPC"]) {
                    return enabled(companion(table, npc).is_some());
                }
                ControlState::default()
            }
        }
    }

    fn press(&mut self, tag: &str, vm: &ViewModel, out: &mut Vec<Intent>) {
        match tag {
            "BTN_DONE" => self.close(Some(self.picks(vm)), vm, out),
            "BTN_BACK" => self.close(None, vm, out),
            "BTN_ACCEPT" => {
                let (Some(add), Some(npc)) = (self.toggle(vm), self.chosen) else {
                    return;
                };
                let mut picks = self.picks(vm);
                if add {
                    picks.push(npc);
                } else {
                    picks.retain(|picked| *picked != npc);
                }
                self.picks = Some(picks);
            }
            tag => {
                if let Some(npc) = slot(tag, &["BTN_NPC"]) {
                    self.chosen = Some(npc);
                }
            }
        }
    }

    fn back(&mut self, _out: &mut Vec<Intent>) {}

    fn key(&mut self, key: crate::input::Key, vm: &ViewModel, out: &mut Vec<Intent>) -> bool {
        if key == crate::input::Key::Back {
            self.close(None, vm, out);
            return true;
        }
        // Opened from the menu bar (no script's table), the shoulders step
        // on to the bar's other tabs as the tabbed screens' do.
        !Party::table(vm).is_some_and(|table| table.open) && super::tabs::key(key, "party", out)
    }

    /// KOTOR's: a click chooses a companion (`press`), a double click adds
    /// or removes it; the pointer over one only lights it.
    fn double(&mut self, tag: &str, vm: &ViewModel, out: &mut Vec<Intent>) {
        if slot(tag, &["BTN_NPC"]).is_some() {
            self.press("BTN_ACCEPT", vm, out);
        }
    }

    fn first_focus(&self) -> Option<&str> {
        Some("BTN_DONE")
    }

    /// Each companion's frame: the one chosen in the highlight (KOTOR's
    /// yellow), one picked in KOTOR's green, the others in the plain
    /// colour; its button's frame drawn again, tinted.
    fn quads(&self, gui: &Gui, vm: &ViewModel, painter: &mut Painter) -> Vec<Quad> {
        let mut out = DrawList::default();
        let picks = self.picks(vm);
        let table = Party::table(vm);
        let slots = table.map_or(0, |table| table.companions.len());
        for npc in (0..slots).filter_map(|npc| u8::try_from(npc).ok()) {
            let tag = format!("BTN_NPC{npc}");
            if companion(table, npc).is_none() || stood_in(&tag, table) {
                continue;
            }
            let Some(button) = gui.find(&tag) else {
                continue;
            };
            let Some(border) = &button.border else {
                continue;
            };
            let color = if self.chosen == Some(npc) {
                painter.palette.hilight
            } else if picks.contains(&npc) {
                PICKED
            } else {
                painter.palette.base
            };
            let border = Border {
                color: Some(color),
                ..border.clone()
            };
            let [r, g, b] = color;
            painter.border(&border, rect_of(button), [r, g, b, 1.0], &mut out);
        }
        out.quads
    }
}
