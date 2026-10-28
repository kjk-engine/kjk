//! Pazaak's three screens over the match the snapshot carries
//! (`vm::PazaakView`; `launcher::pazaak_view` opens each as the match
//! reaches it and closes them at its end): the wager (`pazaakwager`), the
//! side deck (`pazaaksetup`) and the table (`pazaakgame`), TSL's `_p`
//! layouts skinned. Only the player who plays it (whoever asked for it,
//! I30 P20) acts on them. Another player watching it (R9a) sees its table
//! read-only, "Watching Carth" over it, and Leave where TSL's Forfeit
//! stands.
//!
//! - The wager: less and more (or Left and Right) by one, between 1 and
//!   the most; Wager or Quit (Back).
//! - The side deck: a card offered is added, a chosen one taken back,
//!   Clear Cards (TSL) empties the ten; Play once ten are chosen; Back
//!   quits. K1's grid is +, −, ± by value; TSL's fourth row its specials.
//! - The table: a hand card played (a click, or 1 to 4), its sign flipped
//!   first (Flip, F1 to F4) and TSL's ±1/2 changed; End Turn (E), Stand
//!   (S), TSL's Forfeit. Back does nothing (K1 has no way out).

use fmt_odyssey::gui::Control;

use crate::input::Key;
use crate::intent::{GameAction, Intent, PazaakMove};
use crate::paint::ControlState;
use crate::screen::{Screen, ScreenId};
use crate::screens::tab::{enabled, hidden, image, shown, text};
use crate::strings;
use crate::vm::{PazaakCard, PazaakHeld, PazaakStage, PazaakView, ViewModel};

const SIDE_DECK: usize = 10;
/// The highest card number (TSL's Flip 3&6).
const CARDS: u8 = 23;

fn view(vm: &ViewModel) -> Option<&PazaakView> {
    vm.pazaak.as_ref()
}

/// The match as the player who plays it sees it now.
fn playing(vm: &ViewModel, stage: PazaakStage) -> Option<&PazaakView> {
    view(vm).filter(|table| table.plays() && table.stage == stage)
}

fn send(out: &mut Vec<Intent>, played: PazaakMove) {
    out.push(Intent::Game(GameAction::Pazaak(played)));
}

/// The number a tag ends in after `prefix` (`BTN_PLR3`: 3).
fn slot(tag: &str, prefix: &str) -> Option<usize> {
    tag.strip_prefix(prefix)?.parse().ok()
}

/// A card shown: its art on the button, its face on the label.
fn art(card: Option<&PazaakCard>) -> ControlState {
    match card {
        Some(card) => image(Some(card.art.clone())),
        None => hidden(),
    }
}

fn face(card: Option<&PazaakCard>, words: impl FnOnce(&PazaakCard) -> String) -> ControlState {
    card.map_or_else(hidden, |card| text(words(card)))
}

#[derive(Default)]
pub struct Wager {
    /// The wager asked; none for the most.
    asked: Option<i32>,
}

impl Wager {
    fn wager(&self, table: &PazaakView) -> i32 {
        self.asked
            .unwrap_or(table.most)
            .clamp(1.min(table.most), table.most)
    }

    fn step(&mut self, vm: &ViewModel, by: i32) {
        if let Some(table) = playing(vm, PazaakStage::Wager) {
            self.asked = Some((self.wager(table) + by).clamp(1, table.most.max(1)));
        }
    }
}

impl Screen for Wager {
    fn id(&self) -> ScreenId {
        ScreenId("pazaak_wager")
    }

    fn layout(&self) -> &str {
        "pazaakwager"
    }

    fn state(&self, control: &Control, vm: &ViewModel) -> ControlState {
        let Some(table) = view(vm) else {
            return ControlState::default();
        };
        let wager = self.wager(table);
        match control.tag.as_str() {
            "LBL_WAGERVAL" => text(wager),
            "LBL_MAXIMUM" => ControlState {
                text: Some(table.maximum.clone()),
                ..ControlState::default()
            },
            "BTN_LESS" => enabled(table.plays() && wager > 1),
            "BTN_MORE" => enabled(table.plays() && wager < table.most),
            "BTN_WAGER" => enabled(table.plays() && table.most >= 1),
            "BTN_QUIT" => enabled(table.plays()),
            _ => ControlState::default(),
        }
    }

    fn press(&mut self, tag: &str, vm: &ViewModel, out: &mut Vec<Intent>) {
        let Some(table) = playing(vm, PazaakStage::Wager) else {
            return;
        };
        match tag {
            "BTN_LESS" => self.step(vm, -1),
            "BTN_MORE" => self.step(vm, 1),
            "BTN_WAGER" if table.most >= 1 => send(out, PazaakMove::Wager(self.wager(table))),
            "BTN_QUIT" => send(out, PazaakMove::Quit),
            _ => {}
        }
    }

    fn key(&mut self, key: Key, vm: &ViewModel, out: &mut Vec<Intent>) -> bool {
        match key {
            Key::Left => self.step(vm, -1),
            Key::Right => self.step(vm, 1),
            Key::Back => self.press("BTN_QUIT", vm, out),
            _ => return false,
        }
        true
    }

    fn back(&mut self, _: &mut Vec<Intent>) {}

    fn first_focus(&self) -> Option<&str> {
        Some("BTN_WAGER")
    }
}

/// Where a card sits on the side deck screen's grid (row, column), by its
/// number: K1's rows +1..+6, −1..−6, ±1..±6; TSL's fourth Tie Breaker,
/// Double, Flip 2&4, Flip 3&6, ±1/2, as its layout's labels place them.
fn grid(card: u8) -> Option<(usize, usize)> {
    let card = usize::from(card);
    Some(match card {
        1..=6 => (2, card - 1),
        7..=12 => (0, card - 7),
        13..=18 => (1, card - 13),
        19 => (3, 4),
        20 => (3, 1),
        21 => (3, 0),
        22 => (3, 2),
        23 => (3, 3),
        _ => return None,
    })
}

/// The grid cell a tag names after `prefix` (`BTN_AVAIL12`: row 1, column 2).
fn cell(tag: &str, prefix: &str) -> Option<(usize, usize)> {
    let digits = tag.strip_prefix(prefix)?.as_bytes();
    match digits {
        [row, column] if row.is_ascii_digit() && column.is_ascii_digit() => {
            Some((usize::from(row - b'0'), usize::from(column - b'0')))
        }
        _ => None,
    }
}

#[derive(Default)]
pub struct Setup {
    /// The cards chosen, by number, in the order chosen.
    chosen: Vec<u8>,
    /// The card offered last focused (K1's Add Card adds it).
    pointed: Option<u8>,
}

impl Setup {
    /// The card offered at a grid cell, with how many are left to choose.
    fn offered<'a>(
        &self,
        table: &'a PazaakView,
        at: (usize, usize),
    ) -> Option<(&'a PazaakCard, u8, u8)> {
        let offer = table
            .offers
            .iter()
            .find(|offer| grid(offer.card) == Some(at))?;
        let taken = self
            .chosen
            .iter()
            .filter(|card| **card == offer.card)
            .count();
        let left = offer.count.saturating_sub(taken as u8);
        Some((&offer.look, offer.card, left))
    }

    fn look(table: &PazaakView, card: u8) -> Option<&PazaakCard> {
        table
            .offers
            .iter()
            .find(|offer| offer.card == card)
            .map(|offer| &offer.look)
    }

    fn add(&mut self, table: &PazaakView, card: u8) {
        let left = grid(card)
            .and_then(|at| self.offered(table, at))
            .map_or(0, |o| o.2);
        if left > 0 && self.chosen.len() < SIDE_DECK {
            self.chosen.push(card);
        }
    }
}

impl Screen for Setup {
    fn id(&self) -> ScreenId {
        ScreenId("pazaak_setup")
    }

    fn layout(&self) -> &str {
        "pazaaksetup"
    }

    fn state(&self, control: &Control, vm: &ViewModel) -> ControlState {
        let Some(table) = view(vm) else {
            return ControlState::default();
        };
        let tag = control.tag.as_str();
        let full = self.chosen.len() >= SIDE_DECK;
        if let Some(at) = cell(tag, "LBL_AVAILNUM") {
            return self
                .offered(table, at)
                .map_or_else(hidden, |(_, _, left)| text(left));
        }
        if let Some(at) = cell(tag, "LBL_AVAIL") {
            return face(self.offered(table, at).map(|o| o.0), |card| {
                card.face.clone()
            });
        }
        if let Some(at) = cell(tag, "BTN_AVAIL") {
            return match self.offered(table, at) {
                Some((look, _, left)) => ControlState {
                    disabled: !table.plays() || left == 0 || full,
                    ..art(Some(look))
                },
                None => hidden(),
            };
        }
        if let Some(nth) = slot(tag, "BTN_CHOSEN") {
            let look = self
                .chosen
                .get(nth)
                .and_then(|card| Setup::look(table, *card));
            return match look {
                Some(look) => ControlState {
                    disabled: !table.plays(),
                    ..art(Some(look))
                },
                None => enabled(false),
            };
        }
        if let Some(nth) = slot(tag, "LBL_CHOSEN") {
            let look = self
                .chosen
                .get(nth)
                .and_then(|card| Setup::look(table, *card));
            return look.map_or_else(|| text(""), |card| text(&card.face));
        }
        match tag {
            "BTN_ATEXT" => enabled(table.plays() && full),
            "BTN_YTEXT" => enabled(table.plays() && self.pointed.is_some() && !full),
            "BTN_CLEARCARDS" => enabled(table.plays() && !self.chosen.is_empty()),
            _ => ControlState::default(),
        }
    }

    fn press(&mut self, tag: &str, vm: &ViewModel, out: &mut Vec<Intent>) {
        let Some(table) = playing(vm, PazaakStage::Setup) else {
            return;
        };
        if let Some(card) =
            cell(tag, "BTN_AVAIL").and_then(|at| self.offered(table, at).map(|o| o.1))
        {
            self.add(table, card);
        } else if let Some(nth) = slot(tag, "BTN_CHOSEN").filter(|nth| *nth < self.chosen.len()) {
            self.chosen.remove(nth);
        } else {
            match tag {
                "BTN_YTEXT" => {
                    if let Some(card) = self.pointed {
                        self.add(table, card);
                    }
                }
                "BTN_CLEARCARDS" => self.chosen.clear(),
                "BTN_ATEXT" => {
                    if let Ok(deck) = <[u8; SIDE_DECK]>::try_from(self.chosen.as_slice()) {
                        send(out, PazaakMove::Deck(deck));
                    }
                }
                _ => {}
            }
        }
    }

    fn focused(&mut self, tag: &str, _: &ViewModel) {
        if let Some(at) = cell(tag, "BTN_AVAIL") {
            self.pointed = (1..=CARDS).find(|card| grid(*card) == Some(at));
        }
    }

    fn back(&mut self, _: &mut Vec<Intent>) {}

    fn key(&mut self, key: Key, vm: &ViewModel, out: &mut Vec<Intent>) -> bool {
        if key == Key::Back && playing(vm, PazaakStage::Setup).is_some() {
            send(out, PazaakMove::Quit);
            return true;
        }
        false
    }

    fn first_focus(&self) -> Option<&str> {
        Some("BTN_AVAIL00")
    }
}

#[derive(Default)]
pub struct Game {
    /// Each hand slot's sign and size as chosen (`minus`, `two`).
    signs: [(bool, bool); 4],
}

/// The sides' tags: the player's, then the opponent's.
const SIDES: [&str; 2] = ["PLR", "NPC"];

type Takes = fn(&PazaakCard) -> bool;

/// A hand card's choices: Flip for a sign that is the player's, TSL's
/// Change for ±1/2's size.
const CHOICES: [(&str, Takes); 2] = [
    ("BTN_FLIP", |card| card.signed.is_some()),
    ("BTN_CHANGE", |card| card.sized),
];

impl Game {
    /// A hand card as its sign and size are chosen: its face and its art.
    fn hand(&self, nth: usize, card: &PazaakCard) -> PazaakCard {
        let (minus, two) = self.signs[nth];
        match &card.signed {
            Some(faces) => PazaakCard {
                face: faces[usize::from(minus) | usize::from(two) << 1].clone(),
                art: if minus { &card.minus_art } else { &card.art }.clone(),
                ..card.clone()
            },
            None => card.clone(),
        }
    }

    /// The player may play now: its turn, at the table.
    fn may_act(vm: &ViewModel) -> bool {
        playing(vm, PazaakStage::Game).is_some_and(|table| table.turn == Some(0))
    }

    fn play(&mut self, nth: usize, vm: &ViewModel, out: &mut Vec<Intent>) {
        let held = view(vm).and_then(|table| table.sides[0].hand.get(nth)?.as_ref());
        if Game::may_act(vm) && held.is_some() {
            let (minus, two) = self.signs[nth];
            send(
                out,
                PazaakMove::Play {
                    slot: nth as u8,
                    minus,
                    two,
                },
            );
            self.signs[nth] = (false, false);
        }
    }
}

impl Screen for Game {
    fn id(&self) -> ScreenId {
        ScreenId("pazaak_game")
    }

    fn layout(&self) -> &str {
        "pazaakgame"
    }

    fn state(&self, control: &Control, vm: &ViewModel) -> ControlState {
        let Some(table) = view(vm) else {
            return ControlState::default();
        };
        let tag = control.tag.as_str();
        let acts = Game::may_act(vm);
        for (side, name) in table.sides.iter().zip(SIDES) {
            let mine = name == SIDES[0];
            let rest = |prefix: &str| tag.strip_prefix(prefix);
            if let Some(nth) = rest("BTN_").and_then(|rest| slot(rest, name)) {
                return art(side.table.get(nth));
            }
            if let Some(nth) = rest("LBL_").and_then(|rest| slot(rest, name)) {
                return face(side.table.get(nth), |card| card.face.clone());
            }
            if let Some(nth) = rest("BTN_").and_then(|rest| slot(rest, &format!("{name}SIDE"))) {
                let held = side.hand.get(nth).and_then(Option::as_ref);
                return match (held, mine) {
                    (Some(card), true) => ControlState {
                        disabled: !acts,
                        ..art(Some(&self.hand(nth, card)))
                    },
                    (held, _) => shown(held.is_some()),
                };
            }
            if let Some(nth) = rest("LBL_").and_then(|rest| slot(rest, &format!("{name}SIDE"))) {
                let held = side.hand.get(nth).and_then(Option::as_ref).filter(|_| mine);
                return face(held, |card| self.hand(nth, card).face);
            }
            if let Some(nth) = rest("LBL_").and_then(|rest| slot(rest, &format!("{name}SCORE"))) {
                return if nth < usize::from(side.sets) {
                    image(Some(table.mark.clone()))
                } else {
                    ControlState::default()
                };
            }
            if tag == format!("LBL_{name}TOTAL") {
                return text(side.total);
            }
            if tag == format!("LBL_{name}TURN") {
                return shown(table.turn == Some(if mine { 0 } else { 1 }));
            }
        }
        for (prefix, takes) in CHOICES {
            if let Some(nth) = slot(tag, prefix) {
                let held = table.sides[0].hand.get(nth).and_then(Option::as_ref);
                return ControlState {
                    hidden: !held.is_some_and(takes),
                    disabled: !table.plays(),
                    ..ControlState::default()
                };
            }
        }
        match (tag, &table.watching) {
            ("BTN_XTEXT" | "BTN_YTEXT", _) => enabled(acts),
            ("BTN_FORFEITGAME", Some(watch)) => ControlState {
                text: Some(strings::leave(&watch.leave_key)),
                ..ControlState::default()
            },
            ("BTN_FORFEITGAME", None) => enabled(table.stage == PazaakStage::Game),
            ("LBL_PLRNAME", Some(watch)) => {
                let first = watch.name.split(|&byte| byte == b' ').next();
                ControlState {
                    text: Some(strings::watching(&watch.name)),
                    shorter: [Some(&watch.name[..]), first]
                        .into_iter()
                        .flatten()
                        .map(<[u8]>::to_vec)
                        .collect(),
                    ..ControlState::default()
                }
            }
            ("LBL_PLRSIDEDECK", Some(watch)) => {
                let (said, short) = match (watch.held, watch.stage) {
                    (Some(PazaakHeld::Away), _) => (strings::away(&watch.name), strings::AWAY),
                    (Some(PazaakHeld::Hurt), _) => (strings::hurt(&watch.name), strings::HURT),
                    (None, PazaakStage::Wager) => {
                        (strings::wagering(&watch.name), strings::WAGERING)
                    }
                    (None, PazaakStage::Setup) => {
                        (strings::choosing(&watch.name), strings::CHOOSING)
                    }
                    (None, PazaakStage::Game) => return ControlState::default(),
                };
                ControlState {
                    text: Some(said),
                    shorter: vec![strings::bytes(short)],
                    ..ControlState::default()
                }
            }
            _ => ControlState::default(),
        }
    }

    fn press(&mut self, tag: &str, vm: &ViewModel, out: &mut Vec<Intent>) {
        if let Some(nth) = slot(tag, "BTN_PLRSIDE") {
            return self.play(nth, vm, out);
        }
        if view(vm).is_some_and(|table| table.watching.is_some()) {
            if tag == "BTN_FORFEITGAME" {
                out.push(Intent::Game(GameAction::StopWatching));
            }
            return;
        }
        if let Some(nth) = slot(tag, "BTN_FLIP").filter(|nth| *nth < 4) {
            self.signs[nth].0 ^= true;
        } else if let Some(nth) = slot(tag, "BTN_CHANGE").filter(|nth| *nth < 4) {
            self.signs[nth].1 ^= true;
        }
        let acts = Game::may_act(vm);
        match tag {
            "BTN_XTEXT" if acts => send(out, PazaakMove::EndTurn),
            "BTN_YTEXT" if acts => send(out, PazaakMove::Stand),
            "BTN_FORFEITGAME" if playing(vm, PazaakStage::Game).is_some() => {
                send(out, PazaakMove::Forfeit);
            }
            _ => {}
        }
    }

    fn raw(&mut self, key: &str, vm: &ViewModel, out: &mut Vec<Intent>) -> bool {
        let digit = |prefix: &str| {
            key.strip_prefix(prefix)
                .and_then(|n| n.parse::<usize>().ok())
                .and_then(|n| n.checked_sub(1))
                .filter(|nth| *nth < 4)
        };
        if let Some(nth) = digit("Digit").or_else(|| digit("Numpad")) {
            self.play(nth, vm, out);
        } else if let Some(nth) = digit("F") {
            self.press(&format!("BTN_FLIP{nth}"), vm, out);
        } else {
            match key {
                "KeyE" => self.press("BTN_XTEXT", vm, out),
                "KeyS" => self.press("BTN_YTEXT", vm, out),
                _ => return false,
            }
        }
        true
    }

    fn back(&mut self, _: &mut Vec<Intent>) {}

    fn first_focus(&self) -> Option<&str> {
        Some("BTN_XTEXT")
    }
}
