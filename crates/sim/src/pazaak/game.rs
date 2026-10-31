//! A match: the wager, the side deck, then sets to three won, each a main
//! deck shuffled anew, turns alternating as the talk table's rules page
//! (32842) and tutorial (38627–38649) say. The opponent's play is ours
//! (`choice`; KOTOR's is not in its data).

use super::{
    Card, HAND, Kind, LIMIT, SIDE_DECK, decode_card, decode_cards, decode_slot, encode_cards,
    encode_slot,
};
use crate::rng::SimRng;
use crate::wire::{DecodeError, Reader, Writer};

/// How much a good player wants `card` in a side deck of ten, of which
/// four are drawn for the whole match: a card of either sign (it rescues a
/// bust or makes 20), the Tie Breaker and ±1/2 alike, then a minus card,
/// then a plus card, each the nearer 3 the better (the middle of what a
/// rescue or a last card wants), then the Double; a flip least (it helps
/// only on a table holding its values). Played against Pato Ado's decks
/// with the default plus cards and his store's, ranking the flip first and
/// the +1s in lost about one game in fifteen.
fn worth(card: Card) -> i16 {
    let near = |value: i8| -i16::from((value.abs() - 3).abs());
    match card.kind() {
        Kind::Value {
            value,
            either: true,
        } => 100 + near(value),
        Kind::OneOrTwo | Kind::TieBreaker => 100,
        Kind::Value { value, .. } if value < 0 => 60 + near(value),
        Kind::Value { value, .. } => 40 + near(value),
        Kind::Double => 20,
        Kind::Flip(_) => 0,
    }
}

/// The sides by index: the player (seat 0's), then the opponent.
pub const PLAYER: usize = 0;
const OPPONENT: usize = 1;
/// The most cards a side has in play in a set: the ninth stands it.
pub const TABLE: usize = rules::pazaak::sets::TABLE;
pub const SETS_TO_WIN: u8 = rules::pazaak::sets::SETS_TO_WIN;
/// How long the opponent takes over its turn, a set's result shows and
/// the match's does before the end (ms).
pub const OPPONENT_MS: u64 = rules::pazaak::sets::OPPONENT_MS;
pub const SET_MS: u64 = rules::pazaak::sets::SET_MS;
pub const END_MS: u64 = rules::pazaak::sets::END_MS;
/// The opponent stands from this total, and takes a tie from this one
/// against a player standing.
const STAND_AT: i16 = rules::pazaak::opponent::STAND_AT;
const TIE_STAND_AT: i16 = rules::pazaak::opponent::TIE_STAND_AT;
/// The main deck: `MAIN_COPIES` each of 1 to `MAIN_HIGHEST`.
const MAIN_COPIES: u8 = rules::pazaak::deck::MAIN_COPIES;
const MAIN_HIGHEST: u8 = rules::pazaak::deck::MAIN_HIGHEST;

/// A card in play: its value as it counts now, and the side card it is
/// (none for the main deck's).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Laid {
    pub value: i8,
    pub card: Option<Card>,
}

/// One side of the table: its hand (a slot empties once played), its cards
/// in play this set, whether it stands, the sets it won, and whether it
/// played a hand card this turn.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Side {
    pub hand: [Option<Card>; HAND],
    pub table: Vec<Laid>,
    pub stood: bool,
    pub sets: u8,
    pub played: bool,
}

/// A hand card's play: its slot, and the sign (and ±1/2's size) chosen.
type Play = (u8, bool, bool);

impl Side {
    pub fn total(&self) -> i16 {
        self.table.iter().map(|laid| i16::from(laid.value)).sum()
    }

    fn tie_breaker(&self) -> bool {
        self.table.iter().any(|laid| {
            laid.card
                .is_some_and(|card| card.kind() == Kind::TieBreaker)
        })
    }

    /// Whether the rules stand it now: a total of `LIMIT`, or `TABLE` cards.
    fn forced(&self) -> bool {
        self.total() == i16::from(LIMIT) || self.table.len() >= TABLE
    }

    fn may_play(&self) -> bool {
        !self.played && !self.stood && self.table.len() < TABLE
    }

    fn lay(&mut self, card: Card, minus: bool, two: bool) {
        let sign = if minus { -1 } else { 1 };
        let value = match card.kind() {
            Kind::Value { value, either } => {
                if either {
                    sign * value
                } else {
                    value
                }
            }
            Kind::OneOrTwo => sign * if two { 2 } else { 1 },
            Kind::TieBreaker => sign,
            Kind::Double => self.table.last().map_or(0, |laid| laid.value),
            Kind::Flip(values) => {
                for laid in &mut self.table {
                    if values.contains(&laid.value.abs()) {
                        laid.value = -laid.value;
                    }
                }
                0
            }
        };
        self.table.push(Laid {
            value,
            card: Some(card),
        });
    }

    /// Each play its hand allows now, with the total it makes.
    fn options(&self) -> Vec<(Play, i16)> {
        if !self.may_play() {
            return Vec::new();
        }
        let mut options = Vec::new();
        for (slot, card) in self.hand.iter().enumerate() {
            let Some(card) = *card else { continue };
            let signs: &[bool] = if card.chosen() {
                &[false, true]
            } else {
                &[false]
            };
            let sizes: &[bool] = if card.kind() == Kind::OneOrTwo {
                &[false, true]
            } else {
                &[false]
            };
            for &minus in signs {
                for &two in sizes {
                    let mut tried = self.clone();
                    tried.lay(card, minus, two);
                    options.push(((slot as u8, minus, two), tried.total()));
                }
            }
        }
        options
    }
}

/// How a set came out.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    Player,
    Opponent,
    Tie,
}

/// Where a match stands.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Phase {
    /// The wager screen (a call with a wager).
    Wager,
    /// The side deck's ten chosen.
    Setup,
    /// A set under way.
    Play,
    /// A set over, its result shown until `Match::due_ms`.
    SetOver(Outcome),
    /// The match over, won by the player or not, shown until `due_ms`; a
    /// quit is one with no wager, due at once.
    Over { won: bool },
}

/// What the player does at the table (`Action::Pazaak`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Move {
    Wager(i32),
    /// Leave before the match (the wager screen's Quit, the setup's Back).
    Quit,
    /// The side deck: ten of the cards offered.
    Deck([Card; SIDE_DECK]),
    /// Play hand card `slot`, negative when `minus`, ±1/2 as 2 when `two`.
    Play {
        slot: u8,
        minus: bool,
        two: bool,
    },
    EndTurn,
    Stand,
    /// Give the match up (TSL's button): lost, the wager with it.
    Forfeit,
}

/// A match: its phase, the opponent's side deck, the cards the player may
/// choose from, the wager and its most, the tutorial flag, the end script
/// and whom it runs for, the body playing it (whoever asked for it, I30
/// P8: its seat's moves and screen alone), the two sides, the set's main deck (drawn from
/// the end), whose turn, who began the set, when the next thing that
/// waits is due (sim ms), since when it is paused (R9: it takes no move and
/// its clock stands) and when its player's seat dropped (R12).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Match {
    pub phase: Phase,
    pub opponent_deck: [Card; SIDE_DECK],
    pub available: Vec<(Card, u8)>,
    pub most: i32,
    pub wager: i32,
    pub tutorial: bool,
    pub end_script: String,
    pub caller: u32,
    pub player: u32,
    pub sides: [Side; 2],
    pub main: Vec<u8>,
    pub turn: u8,
    pub began: u8,
    pub due_ms: u64,
    pub paused: Option<u64>,
    pub away: Option<u64>,
    /// Its player was hurt and is still in combat: one cause of a pause.
    pub hurt: bool,
    /// The other players' bodies watching it (R9a), in the order they came:
    /// one a seat but its player's, so fewer than `MAX_SEATS`, none twice.
    pub watchers: Vec<u32>,
}

/// A time or none as a snapshot carries it: `u64::MAX` for none.
fn encode_time(time: Option<u64>, out: &mut Writer) {
    out.u64(time.unwrap_or(u64::MAX));
}

fn decode_time(input: &mut Reader) -> Result<Option<u64>, DecodeError> {
    Ok(Some(input.u64()?).filter(|time| *time != u64::MAX))
}

/// `cards` in a random order.
fn shuffled<T: Copy>(cards: &[T], rng: &mut SimRng) -> Vec<T> {
    let mut cards = cards.to_vec();
    for at in (1..cards.len()).rev() {
        cards.swap(at, rng.below(at as u32 + 1) as usize);
    }
    cards
}

impl Match {
    /// A match against `opponent_deck`, the player choosing from
    /// `available`; a wager of at most `max_wager` and the party's `gold`
    /// when `max_wager` is above 0, else none (no wager screen).
    pub fn new(
        opponent_deck: [Card; SIDE_DECK],
        available: Vec<(Card, u8)>,
        (max_wager, gold): (i32, i32),
        tutorial: bool,
        (end_script, caller): (String, u32),
    ) -> Match {
        let wagered = max_wager > 0;
        Match {
            phase: if wagered { Phase::Wager } else { Phase::Setup },
            opponent_deck,
            available,
            most: if wagered {
                max_wager.min(gold).max(0)
            } else {
                0
            },
            wager: 0,
            tutorial,
            end_script,
            caller,
            player: 0,
            sides: Default::default(),
            main: Vec::new(),
            turn: PLAYER as u8,
            began: PLAYER as u8,
            due_ms: 0,
            paused: None,
            away: None,
            hurt: false,
            watchers: Vec::new(),
        }
    }

    /// Paused while `held` (sim ms `now`). Resumed, what it waited for is
    /// due as much later as the pause lasted: it goes on where it stood.
    pub fn hold(&mut self, held: bool, now: u64) {
        match (held, self.paused) {
            (true, None) => self.paused = Some(now),
            (false, Some(since)) => {
                self.due_ms = self.due_ms.saturating_add(now.saturating_sub(since));
                self.paused = None;
            }
            _ => {}
        }
    }

    /// Ended as a quit, paused or not: the wager back (no longer held), the
    /// end at once, its result not won. One decided already only plays on
    /// to its end, as decided.
    pub fn quit(&mut self, now: u64) {
        self.hold(false, now);
        (self.away, self.hurt) = (None, false);
        if matches!(self.phase, Phase::Over { .. }) {
            return;
        }
        self.wager = 0;
        self.phase = Phase::Over { won: false };
        self.due_ms = now;
    }

    /// The player's move, if the match takes it now: none while paused.
    pub fn apply(&mut self, played: Move, now: u64, rng: &mut SimRng) {
        if self.paused.is_some() {
            return;
        }
        match (self.phase, played) {
            (Phase::Wager, Move::Wager(wager)) if (1..=self.most).contains(&wager) => {
                self.wager = wager;
                self.phase = Phase::Setup;
            }
            (Phase::Wager | Phase::Setup, Move::Quit) => self.quit(now),
            (Phase::Setup, Move::Deck(deck)) if self.offers(&deck) => {
                let first = rng.below(2) as usize;
                for (side, cards) in [(PLAYER, deck), (OPPONENT, self.opponent_deck)] {
                    let drawn = shuffled(&cards, rng);
                    self.sides[side].hand = std::array::from_fn(|at| drawn.get(at).copied());
                }
                self.begin_set(first, now, rng);
            }
            (Phase::Play, Move::Play { slot, minus, two }) if self.turn == PLAYER as u8 => {
                self.play(PLAYER, (slot, minus, two), now);
            }
            (Phase::Play, Move::EndTurn | Move::Stand) if self.turn == PLAYER as u8 => {
                self.end_turn(PLAYER, played == Move::Stand, now);
            }
            (Phase::Play | Phase::SetOver(_), Move::Forfeit) => {
                self.phase = Phase::Over { won: false };
                self.due_ms = now + END_MS;
            }
            _ => {}
        }
    }

    /// Time passing: the opponent's turn, the next set; once the match's
    /// end has shown, whether the player won it. Nothing while paused.
    pub fn advance(&mut self, now: u64, rng: &mut SimRng) -> Option<bool> {
        if self.paused.is_some() || now < self.due_ms {
            return None;
        }
        match self.phase {
            Phase::Play if self.turn == OPPONENT as u8 => {
                let (play, stand) = self.choice(OPPONENT);
                if let Some(play) = play {
                    self.play(OPPONENT, play, now);
                }
                if self.phase == Phase::Play && self.turn == OPPONENT as u8 {
                    self.end_turn(OPPONENT, stand, now);
                }
                None
            }
            Phase::SetOver(outcome) => {
                let first = match outcome {
                    Outcome::Player => OPPONENT,
                    Outcome::Opponent => PLAYER,
                    Outcome::Tie => usize::from(self.began),
                };
                self.begin_set(first, now, rng);
                None
            }
            Phase::Over { won } => Some(won),
            _ => None,
        }
    }

    /// The credits the match wins or loses the party at its end, the
    /// wager's (held meanwhile: `script::pazaak`).
    pub fn settled(&self, won: bool) -> i32 {
        if won { self.wager } else { -self.wager }
    }

    /// Whether `deck` is ten of the cards offered.
    fn offers(&self, deck: &[Card; SIDE_DECK]) -> bool {
        self.available.iter().all(|&(card, count)| {
            deck.iter().filter(|chosen| **chosen == card).count() <= usize::from(count)
        }) && deck
            .iter()
            .all(|card| self.available.iter().any(|(offered, _)| offered == card))
    }

    fn begin_set(&mut self, first: usize, now: u64, rng: &mut SimRng) {
        let deck: Vec<u8> = (1..=MAIN_HIGHEST)
            .flat_map(|value| std::iter::repeat_n(value, usize::from(MAIN_COPIES)))
            .collect();
        self.main = shuffled(&deck, rng);
        for side in &mut self.sides {
            side.table.clear();
            side.stood = false;
        }
        self.phase = Phase::Play;
        self.began = first as u8;
        self.begin_turn(first, now);
    }

    fn begin_turn(&mut self, side: usize, now: u64) {
        self.turn = side as u8;
        let drawn = self.main.pop();
        let me = &mut self.sides[side];
        me.played = false;
        if let Some(value) = drawn {
            me.table.push(Laid {
                value: value as i8,
                card: None,
            });
        }
        self.due_ms = now + if side == OPPONENT { OPPONENT_MS } else { 0 };
        if self.sides[side].forced() {
            self.end_turn(side, true, now);
        }
    }

    fn play(&mut self, side: usize, (slot, minus, two): Play, now: u64) {
        let me = &mut self.sides[side];
        let Some(card) = me.hand.get(usize::from(slot)).copied().flatten() else {
            return;
        };
        if !me.may_play() {
            return;
        }
        me.lay(card, minus, two);
        me.hand[usize::from(slot)] = None;
        me.played = true;
        if me.forced() {
            self.end_turn(side, true, now);
        }
    }

    fn end_turn(&mut self, side: usize, stand: bool, now: u64) {
        let other = 1 - side;
        if self.sides[side].total() > i16::from(LIMIT) {
            return self.finish_set(other, now);
        }
        self.sides[side].stood |= stand;
        if !self.sides[other].stood {
            self.begin_turn(other, now);
        } else if !self.sides[side].stood {
            self.begin_turn(side, now);
        } else {
            let [player, opponent] = &self.sides;
            let winner = match player.total().cmp(&opponent.total()) {
                std::cmp::Ordering::Greater => Some(PLAYER),
                std::cmp::Ordering::Less => Some(OPPONENT),
                std::cmp::Ordering::Equal => match (player.tie_breaker(), opponent.tie_breaker()) {
                    (true, false) => Some(PLAYER),
                    (false, true) => Some(OPPONENT),
                    _ => None,
                },
            };
            match winner {
                Some(winner) => self.finish_set(winner, now),
                None => {
                    self.phase = Phase::SetOver(Outcome::Tie);
                    self.due_ms = now + SET_MS;
                }
            }
        }
    }

    fn finish_set(&mut self, winner: usize, now: u64) {
        self.sides[winner].sets += 1;
        if self.sides[winner].sets >= SETS_TO_WIN {
            self.phase = Phase::Over {
                won: winner == PLAYER,
            };
            self.due_ms = now + END_MS;
        } else {
            self.phase = Phase::SetOver(if winner == PLAYER {
                Outcome::Player
            } else {
                Outcome::Opponent
            });
            self.due_ms = now + SET_MS;
        }
    }

    /// What `side` plays on its turn: the hand card, if one, and whether it
    /// then stands. Rescue a bust with the play nearest `LIMIT`; against a
    /// standing side, stand once past it (or tied from `TIE_STAND_AT`), else
    /// play past it if a card can; otherwise play only to go past
    /// `STAND_AT`, and stand from it.
    fn choice(&self, side: usize) -> (Option<Play>, bool) {
        let (me, them) = (&self.sides[side], &self.sides[1 - side]);
        let (total, theirs) = (me.total(), them.total());
        let options = me.options();
        let best = |at_least: i16| {
            options
                .iter()
                .enumerate()
                .filter(|(_, (_, made))| (at_least..=i16::from(LIMIT)).contains(made))
                .max_by_key(|(at, (_, made))| (*made, std::cmp::Reverse(*at)))
                .map(|(_, option)| *option)
        };
        if total > i16::from(LIMIT) {
            return match best(i16::MIN) {
                Some((play, made)) => (Some(play), made >= STAND_AT || them.stood && made > theirs),
                None => (None, false),
            };
        }
        if them.stood {
            if total > theirs || total == theirs && total >= TIE_STAND_AT {
                return (None, true);
            }
            return match best(theirs + 1) {
                Some((play, _)) => (Some(play), true),
                None => (None, false),
            };
        }
        match best(STAND_AT + 1).filter(|(_, made)| *made > total) {
            Some((play, _)) => (Some(play), true),
            None => (None, total >= STAND_AT),
        }
    }

    /// What a good player does on its turn (`advice`), its four hand cards
    /// kept for the whole match: a bust rescued with the play nearest
    /// `LIMIT`; against a standing side, stand once past it or level with
    /// it (a tie is played again), else beat it with a card if one can,
    /// else draw on (standing loses); otherwise play a card only to make
    /// `LIMIT` and stand, and stand from `STAND_AT`. Of plays making
    /// the same, a card of one sign goes before a ± one, which is kept for
    /// a bust. Against Pato Ado's decks (`pazaakdecks.2da` rows 1 and 3)
    /// with the default + cards and his store's, it wins 50% and 35% of
    /// matches where the opponent's own rule (`choice`) won 44% and 29%.
    fn advised(&self, side: usize) -> (Option<Play>, bool) {
        let (me, them) = (&self.sides[side], &self.sides[1 - side]);
        let (total, theirs) = (me.total(), them.total());
        let limit = i16::from(LIMIT);
        let options = me.options();
        let spare = |slot: u8| me.hand[usize::from(slot)].is_some_and(|card| card.chosen());
        // The best play making a total in `range`: the highest, then one
        // not ±.
        let best = |range: std::ops::RangeInclusive<i16>| {
            options
                .iter()
                .filter(|(_, made)| range.contains(made))
                .max_by_key(|((slot, _, _), made)| (*made, !spare(*slot), std::cmp::Reverse(*slot)))
                .copied()
        };
        if total > limit {
            return match best(i16::MIN..=limit) {
                Some((play, made)) => {
                    (Some(play), made >= STAND_AT || them.stood && made >= theirs)
                }
                None => (None, false),
            };
        }
        if them.stood {
            if total >= theirs {
                return (None, true);
            }
            return match best(theirs + 1..=limit) {
                Some((play, _)) => (Some(play), true),
                None => (None, false),
            };
        }
        match best(limit..=limit).filter(|(_, made)| *made > total) {
            Some((play, _)) => (Some(play), true),
            None => (None, total >= STAND_AT),
        }
    }

    /// What the player would do now as the opponent plays: a headless
    /// player's move (the playthrough's, `kjk play --pazaak`). The most it
    /// may wager; a side deck of the cards offered by their `worth`.
    pub fn advice(&self) -> Option<Move> {
        match self.phase {
            Phase::Wager if self.most > 0 => Some(Move::Wager(self.most)),
            Phase::Wager => Some(Move::Quit),
            Phase::Setup => {
                let mut offered: Vec<Card> = self
                    .available
                    .iter()
                    .flat_map(|&(card, count)| std::iter::repeat_n(card, usize::from(count)))
                    .collect();
                offered.sort_by_key(|card| std::cmp::Reverse(worth(*card)));
                let deck: [Card; SIDE_DECK] = offered.get(..SIDE_DECK)?.try_into().ok()?;
                Some(Move::Deck(deck))
            }
            Phase::Play if self.turn == PLAYER as u8 => {
                let (play, stand) = self.advised(PLAYER);
                Some(match play {
                    Some((slot, minus, two)) => Move::Play { slot, minus, two },
                    None if stand => Move::Stand,
                    None => Move::EndTurn,
                })
            }
            _ => None,
        }
    }

    pub(crate) fn encode(&self, out: &mut Writer) {
        match self.phase {
            Phase::Wager => out.u8(0),
            Phase::Setup => out.u8(1),
            Phase::Play => out.u8(2),
            Phase::SetOver(outcome) => {
                out.u8(3);
                out.u8(outcome as u8);
            }
            Phase::Over { won } => {
                out.u8(4);
                out.u8(u8::from(won));
            }
        }
        encode_cards(&self.opponent_deck, out);
        out.u8(self.available.len() as u8);
        for (card, count) in &self.available {
            encode_cards(&[*card], out);
            out.u8(*count);
        }
        out.u32(self.most as u32);
        out.u32(self.wager as u32);
        out.u8(u8::from(self.tutorial));
        out.string(&self.end_script);
        out.u32(self.caller);
        out.u32(self.player);
        for side in &self.sides {
            for card in side.hand {
                encode_slot(card, out);
            }
            out.u8(side.table.len() as u8);
            for laid in &side.table {
                out.i8(laid.value);
                encode_slot(laid.card, out);
            }
            out.u8(u8::from(side.stood) | u8::from(side.played) << 1);
            out.u8(side.sets);
        }
        out.section(&self.main);
        out.u8(self.turn);
        out.u8(self.began);
        out.u64(self.due_ms);
        encode_time(self.paused, out);
        encode_time(self.away, out);
        out.u8(u8::from(self.hurt));
        out.u8(u8::try_from(self.watchers.len()).expect("a watcher a seat"));
        for watcher in &self.watchers {
            out.u32(*watcher);
        }
    }

    pub(crate) fn decode(input: &mut Reader) -> Result<Match, DecodeError> {
        let at = input.at();
        let phase = match input.u8()? {
            0 => Phase::Wager,
            1 => Phase::Setup,
            2 => Phase::Play,
            3 => Phase::SetOver(match input.u8()? {
                0 => Outcome::Player,
                1 => Outcome::Opponent,
                2 => Outcome::Tie,
                _ => return Err(input.invalid("pazaak outcome", at)),
            }),
            4 => Phase::Over {
                won: input.u8()? != 0,
            },
            _ => return Err(input.invalid("pazaak phase", at)),
        };
        let opponent_deck = decode_cards(input)?;
        let mut available = Vec::new();
        for _ in 0..input.u8()? {
            available.push((decode_card(input)?, input.u8()?));
        }
        let most = input.u32()? as i32;
        let wager = input.u32()? as i32;
        let tutorial = input.u8()? != 0;
        let end_script = input.string()?;
        let caller = input.u32()?;
        let player = input.u32()?;
        let mut sides: [Side; 2] = Default::default();
        for side in &mut sides {
            for slot in &mut side.hand {
                *slot = decode_slot(input)?;
            }
            for _ in 0..input.u8()? {
                let value = input.i8()?;
                let card = decode_slot(input)?;
                side.table.push(Laid { value, card });
            }
            let flags = input.u8()?;
            side.stood = flags & 1 != 0;
            side.played = flags & 2 != 0;
            side.sets = input.u8()?;
        }
        let main = input.section()?.to_vec();
        let at = input.at();
        let (turn, began) = (input.u8()?, input.u8()?);
        if turn > 1 || began > 1 {
            return Err(input.invalid("pazaak turn", at));
        }
        let (due_ms, paused, away) = (input.u64()?, decode_time(input)?, decode_time(input)?);
        let hurt = input.u8()? != 0;
        // One a seat but the player's own, none twice.
        let at = input.at();
        let count = input.u8()?;
        if count >= crate::MAX_SEATS {
            return Err(input.invalid("pazaak watchers", at));
        }
        let watchers: Vec<u32> = (0..count).map(|_| input.u32()).collect::<Result<_, _>>()?;
        if (1..watchers.len()).any(|nth| watchers[..nth].contains(&watchers[nth])) {
            return Err(input.invalid("pazaak watchers", at));
        }
        Ok(Match {
            phase,
            opponent_deck,
            available,
            most,
            wager,
            tutorial,
            end_script,
            caller,
            player,
            sides,
            main,
            turn,
            began,
            due_ms,
            paused,
            away,
            hurt,
            watchers,
        })
    }
}

impl Move {
    pub(crate) fn encode(&self, out: &mut Writer) {
        match *self {
            Move::Wager(wager) => {
                out.u8(0);
                out.u32(wager as u32);
            }
            Move::Quit => out.u8(1),
            Move::Deck(deck) => {
                out.u8(2);
                encode_cards(&deck, out);
            }
            Move::Play { slot, minus, two } => {
                out.u8(3);
                out.u8(slot);
                out.u8(u8::from(minus) | u8::from(two) << 1);
            }
            Move::EndTurn => out.u8(4),
            Move::Stand => out.u8(5),
            Move::Forfeit => out.u8(6),
        }
    }

    pub(crate) fn decode(input: &mut Reader) -> Result<Move, DecodeError> {
        let at = input.at();
        Ok(match input.u8()? {
            0 => Move::Wager(input.u32()? as i32),
            1 => Move::Quit,
            2 => Move::Deck(decode_cards(input)?),
            3 => {
                let slot = input.u8()?;
                let at = input.at();
                let signs = input.u8()?;
                if usize::from(slot) >= HAND || signs > 3 {
                    return Err(input.invalid("pazaak play", at));
                }
                Move::Play {
                    slot,
                    minus: signs & 1 != 0,
                    two: signs & 2 != 0,
                }
            }
            4 => Move::EndTurn,
            5 => Move::Stand,
            6 => Move::Forfeit,
            _ => return Err(input.invalid("pazaak move", at)),
        })
    }
}
