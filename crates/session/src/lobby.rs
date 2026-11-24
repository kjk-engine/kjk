//! The co-op lobby's state and its rules, decided on the host. Nothing here
//! reads a file or the network. The host's game applies each guest's
//! request and its own player's choices here, and sends the result to every
//! guest.
//!
//! The lobby has three slots. Slot 0 is the host. Slots 1 and 2 are guests.
//! Exactly one player plays the story's character (`Playing::Hero`). That is
//! the save's main character, or the character the story makes the player
//! play in a part of the story with a switched character, such as KOTOR 2
//! played as T3-M4 alone. The host plays it when the lobby opens. Any other
//! player can take it, but only by swapping characters with the player who
//! has it, so the story always has a player. Every other player plays a
//! companion or, when the party has no companion left for them, a remote
//! droid. The party has two companion places beside the main character. A
//! companion that the story requires is always in the party. When no player
//! chooses it, the game controls it. The host can put other companions in a
//! free place, and the game controls them too.
//!
//! A slot is not a simulation seat. When the game starts, the slot that
//! plays the story's character sits in the first seat, whose player answers
//! the story's dialogue, and the other players sit in the seats after it
//! (`Lobby::seats`).

use sim::script::party_choice::Choice;

pub use collision_odyssey::Collision;
pub use sim::combat::passive::Game;

/// The lobby's player slots. Slot 0 is the host.
pub const SLOTS: usize = 3;
/// The companion places beside the main character.
pub const COMPANION_PLACES: usize = 2;

/// A recruited companion as the lobby shows it. `name` and `class` are game
/// text, kept in the game's code page.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Companion {
    pub npc: u8,
    pub name: Vec<u8>,
    pub portrait: String,
    pub class: Vec<u8>,
    pub level: u8,
    pub choice: Choice,
}

/// The save the lobby is about to play. `saved` is when it was saved, in
/// seconds since 1970. The `hero` fields describe the story's character:
/// the main character by the name the player gave it in character creation,
/// or, in a part of the story with a switched character, that character.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Summary {
    pub save: Vec<u8>,
    pub area: Vec<u8>,
    pub game: Game,
    pub saved: u64,
    pub hero: Vec<u8>,
    pub hero_class: Vec<u8>,
    pub hero_level: u8,
    pub hero_portrait: String,
}

/// What a slot's player plays.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Playing {
    /// The story's character (`Summary`'s hero), whose player sits in the
    /// first seat.
    Hero,
    /// The companion with this party number.
    Companion(u8),
    /// A remote droid, because the party has no companion for this player.
    Droid,
}

/// What a player asks for in the lobby: the story's character, or the
/// companion with this party number.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Pick {
    Hero,
    Companion(u8),
}

/// A player in the lobby. `name` is shown as the player's game sent it.
/// `delay_ms` is the connection's round trip time.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Slot {
    pub name: String,
    pub playing: Playing,
    pub ready: bool,
    pub delay_ms: u16,
}

/// The whole lobby. `summary` is `None` while the host is choosing a save.
/// `with_party` lists the companions the host put in a free place; the game
/// controls them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Lobby {
    pub summary: Option<Summary>,
    pub saber_rules: String,
    pub collision: Collision,
    /// Whether a reply a player takes is shown to the other players in the
    /// conversation before it runs ("Show chosen replies", on in a new
    /// lobby).
    pub show_replies: bool,
    pub companions: Vec<Companion>,
    pub with_party: Vec<u8>,
    pub slots: [Option<Slot>; SLOTS],
}

impl Lobby {
    /// A lobby with only its host, named `host`, and no save yet.
    pub fn new(host: String, saber_rules: String, collision: Collision) -> Lobby {
        Lobby {
            summary: None,
            saber_rules,
            collision,
            show_replies: true,
            companions: Vec::new(),
            with_party: Vec::new(),
            slots: [
                Some(Slot {
                    name: host,
                    playing: Playing::Hero,
                    ready: true,
                    delay_ms: 0,
                }),
                None,
                None,
            ],
        }
    }

    fn companion(&self, npc: u8) -> Option<&Companion> {
        self.companions
            .iter()
            .find(|companion| companion.npc == npc)
    }

    /// The slot whose player plays `npc`.
    fn played_by(&self, npc: u8) -> Option<usize> {
        (0..SLOTS).find(|&slot| self.playing(slot) == Some(Playing::Companion(npc)))
    }

    /// What slot `slot` plays, when a player is in it.
    pub fn playing(&self, slot: usize) -> Option<Playing> {
        Some(self.slots.get(slot)?.as_ref()?.playing)
    }

    /// The slot whose player plays the story's character. The lobby keeps
    /// exactly one; slot 0 is named if a caller broke that.
    pub fn hero_slot(&self) -> usize {
        (0..SLOTS)
            .find(|&slot| self.playing(slot) == Some(Playing::Hero))
            .unwrap_or(0)
    }

    /// Makes sure exactly one player plays the story's character: when no
    /// slot does, the host plays it, and when more than one does, every one
    /// after the first flies a droid. Whether anything changed.
    pub fn keep_one_hero(&mut self) -> bool {
        let heroes: Vec<usize> = (0..SLOTS)
            .filter(|&slot| self.playing(slot) == Some(Playing::Hero))
            .collect();
        match heroes.as_slice() {
            [_] => false,
            [] => {
                self.set_playing(0, Playing::Hero);
                true
            }
            [_, rest @ ..] => {
                for &slot in rest {
                    self.set_playing(slot, Playing::Droid);
                }
                true
            }
        }
    }

    /// The companions that start in the party: each player's companion in
    /// slot order, then the required companions no player chose, then those
    /// the host put in a free place. Never more than `COMPANION_PLACES`.
    pub fn party(&self) -> Vec<u8> {
        let played = (0..SLOTS).filter_map(|slot| match self.playing(slot) {
            Some(Playing::Companion(npc)) => Some(npc),
            _ => None,
        });
        let required = (self.companions.iter())
            .filter(|companion| companion.choice == Choice::Required)
            .map(|companion| companion.npc);
        let mut party = Vec::new();
        for npc in played
            .chain(required)
            .chain(self.with_party.iter().copied())
        {
            if !party.contains(&npc) {
                party.push(npc);
            }
        }
        party.truncate(COMPANION_PLACES);
        party
    }

    /// How many companions the party would have if `npc` joined it and
    /// `leaving` left it.
    fn party_with(&self, npc: u8, leaving: Option<u8>) -> usize {
        let mut party = self.party();
        party.retain(|&member| Some(member) != leaving || self.required(member));
        if !party.contains(&npc) {
            party.push(npc);
        }
        party.len()
    }

    fn required(&self, npc: u8) -> bool {
        self.companion(npc)
            .is_some_and(|companion| companion.choice == Choice::Required)
    }

    /// The companion a new guest is given: a required companion no player
    /// plays, else one the host put in the party (the save's party by
    /// default), else any other companion that can be chosen while a place
    /// is free. `None` when none is left; the guest then flies a droid.
    fn default_companion(&self) -> Option<u8> {
        let free = |npc: &u8| {
            self.played_by(*npc).is_none()
                && self
                    .companion(*npc)
                    .is_some_and(|companion| companion.choice != Choice::Unavailable)
        };
        let required = (self.companions.iter())
            .filter(|companion| companion.choice == Choice::Required)
            .map(|companion| companion.npc);
        let in_party = self.with_party.iter().copied();
        if let Some(npc) = required.chain(in_party).find(free) {
            return Some(npc);
        }
        (self.companions.iter())
            .map(|companion| companion.npc)
            .filter(free)
            .find(|&npc| self.party_with(npc, None) <= COMPANION_PLACES)
    }

    /// Puts a guest named `name` in the first free guest slot, with the
    /// default companion. The slot, or `None` when the lobby is full.
    pub fn join(&mut self, name: String) -> Option<usize> {
        let slot = (1..SLOTS).find(|&slot| self.slots[slot].is_none())?;
        self.join_at(slot, name).then_some(slot)
    }

    /// Puts a guest named `name` in guest slot `slot`, which the host's
    /// network gave the connection. Whether the slot was free.
    pub fn join_at(&mut self, slot: usize, name: String) -> bool {
        if slot == 0 || self.slots.get(slot).is_none_or(Option::is_some) {
            return false;
        }
        let playing = self.take_default();
        self.slots[slot] = Some(Slot {
            name,
            playing,
            ready: false,
            delay_ms: 0,
        });
        true
    }

    /// The default companion for a new guest, taken out of the host's
    /// places.
    fn take_default(&mut self) -> Playing {
        match self.default_companion() {
            Some(npc) => {
                self.with_party.retain(|&held| held != npc);
                Playing::Companion(npc)
            }
            None => Playing::Droid,
        }
    }

    /// The guest in slot `slot` left. Their companion stays in the party
    /// only if the story requires it. When they played the story's
    /// character, the host plays it again, and the companion the host had
    /// becomes free.
    pub fn leave(&mut self, slot: usize) {
        if slot == 0 || slot >= SLOTS {
            return;
        }
        let hero = self.playing(slot) == Some(Playing::Hero);
        self.slots[slot] = None;
        if hero {
            self.set_playing(0, Playing::Hero);
        }
    }

    /// The player in slot `slot` asks for `pick`. Whether the choice was
    /// made.
    ///
    /// The story's character is only ever swapped: the player who asks for
    /// it gets it when `swap` says they asked to swap, and the player who
    /// had it gets what the asking player had (a companion or a droid).
    ///
    /// A companion nobody plays is taken, and the player's old companion
    /// becomes free. A companion another player plays is swapped with that
    /// player, but only when `swap` says the player asked for a swap. A
    /// request for a companion the player saw as free is refused when
    /// another player took it first. The player of the story's character
    /// can give it up only by such a swap, so a free companion is refused
    /// to them. Refused too: an empty slot, a companion the lobby does not
    /// list, one the story made unavailable, and a choice that would put a
    /// third companion in the party.
    pub fn choose(&mut self, slot: usize, pick: Pick, swap: bool) -> bool {
        let Some(own) = self.playing(slot) else {
            return false;
        };
        let before = self.slots.clone();
        let npc = match pick {
            Pick::Hero if own == Playing::Hero => return true,
            Pick::Hero if !swap => return false,
            Pick::Hero => {
                self.set_playing(self.hero_slot(), own);
                self.set_playing(slot, Playing::Hero);
                self.unready_changed(&before);
                return true;
            }
            Pick::Companion(npc) => npc,
        };
        if self
            .companion(npc)
            .is_none_or(|companion| companion.choice == Choice::Unavailable)
        {
            return false;
        }
        if own == Playing::Companion(npc) {
            return true;
        }
        match self.played_by(npc) {
            Some(other) if swap => self.set_playing(other, own),
            Some(_) => return false,
            None if own == Playing::Hero => return false,
            None => {
                let leaving = match own {
                    Playing::Companion(old) => Some(old),
                    _ => None,
                };
                if self.party_with(npc, leaving) > COMPANION_PLACES {
                    return false;
                }
                self.with_party.retain(|&held| held != npc);
            }
        }
        self.set_playing(slot, Playing::Companion(npc));
        self.unready_changed(&before);
        true
    }

    fn set_playing(&mut self, slot: usize, playing: Playing) {
        if let Some(Some(place)) = self.slots.get_mut(slot) {
            place.playing = playing;
        }
    }

    /// Clears the Ready mark of every slot whose character changed since
    /// `before`.
    fn unready_changed(&mut self, before: &[Option<Slot>; SLOTS]) {
        for (slot, was) in self.slots.iter_mut().zip(before).skip(1) {
            if let (Some(slot), Some(was)) = (slot, was)
                && slot.playing != was.playing
            {
                slot.ready = false;
            }
        }
    }

    /// The host puts `npc` in a free companion place, or takes it out again.
    /// Refused for a companion a guest plays, one the story made
    /// unavailable or requires, one the lobby does not list, and a third
    /// companion. Whether it changed.
    pub fn fill(&mut self, npc: u8) -> bool {
        let Some(companion) = self.companion(npc) else {
            return false;
        };
        if companion.choice != Choice::Open || self.played_by(npc).is_some() {
            return false;
        }
        if self.with_party.contains(&npc) {
            self.with_party.retain(|&held| held != npc);
            return true;
        }
        if self.party_with(npc, None) > COMPANION_PLACES {
            return false;
        }
        self.with_party.push(npc);
        true
    }

    /// Sets the Ready mark of the guest in slot `slot`.
    pub fn ready(&mut self, slot: usize, on: bool) {
        if slot > 0
            && let Some(Some(place)) = self.slots.get_mut(slot)
        {
            place.ready = on;
        }
    }

    /// A new save for the lobby: its summary, its companions and the
    /// companions in its party now (`current`). Every guest choice of the
    /// old save is dropped. The host plays the story's character again. The
    /// host's places start as the save's party, then each guest is given the
    /// default companion in slot order. Every Ready mark is cleared.
    pub fn reset(&mut self, summary: Summary, companions: Vec<Companion>, current: &[u8]) {
        self.set_playing(0, Playing::Hero);
        self.summary = Some(summary);
        self.companions = companions;
        self.with_party = (current.iter().copied())
            .filter(|&npc| {
                self.companion(npc)
                    .is_some_and(|companion| companion.choice == Choice::Open)
            })
            .collect();
        let guests: Vec<usize> = (1..SLOTS)
            .filter(|&slot| self.slots[slot].is_some())
            .collect();
        for &slot in &guests {
            self.set_playing(slot, Playing::Droid);
        }
        for slot in guests {
            let playing = self.take_default();
            if let Some(Some(place)) = self.slots.get_mut(slot) {
                place.playing = playing;
                place.ready = false;
            }
        }
    }

    /// Each occupied slot when the game starts, by slot: its simulation
    /// seat and what it plays. The slot that plays the story's character
    /// sits in the first seat. The other occupied slots sit in the seats
    /// after it, in slot order.
    pub fn seats(&self) -> Vec<(usize, u8, Playing)> {
        let hero = self.hero_slot();
        let mut next = 0u8;
        (0..SLOTS)
            .filter_map(|slot| {
                let playing = self.playing(slot)?;
                let seat = if slot == hero {
                    0
                } else {
                    next += 1;
                    next
                };
                Some((slot, seat, playing))
            })
            .collect()
    }
}

pub use probes::probes;

/// The lobby's checks (`kjk sim probes --only lobby`), and the sample lobbies
/// the network's and the screen's checks use.
pub mod probes {
    use super::*;
    use Choice::{Open, Required, Unavailable};
    use Playing::{Companion as Plays, Droid, Hero};

    /// A companion numbered `npc`, named "companion" and its number.
    pub fn companion(npc: u8, choice: Choice) -> Companion {
        Companion {
            npc,
            name: format!("companion {npc}").into_bytes(),
            portrait: String::new(),
            class: b"Soldier".to_vec(),
            level: 1,
            choice,
        }
    }

    /// A KOTOR save named "Test".
    pub fn summary() -> Summary {
        Summary {
            save: b"Test".to_vec(),
            area: b"Area".to_vec(),
            game: Game::K1,
            saved: 0,
            hero: b"Hero".to_vec(),
            hero_class: b"Scout".to_vec(),
            hero_level: 1,
            hero_portrait: String::new(),
        }
    }

    /// A lobby on a save whose companions are `companions` and whose party
    /// is `current`, joined by `guests` guests.
    pub fn lobby(companions: &[(u8, Choice)], current: &[u8], guests: usize) -> Lobby {
        let mut lobby = Lobby::new("host".into(), "ja".into(), Collision::Limited);
        let companions = (companions.iter())
            .map(|&(npc, choice)| companion(npc, choice))
            .collect();
        lobby.reset(summary(), companions, current);
        for guest in 0..guests {
            lobby.join(format!("guest {guest}"));
        }
        lobby
    }

    /// Bastila (0), Carth (1) and Mission (2) can be chosen, Zaalbar (3) is
    /// taken away, and the party is Bastila and Carth.
    pub fn usual(guests: usize) -> Lobby {
        lobby(
            &[(0, Open), (1, Open), (2, Open), (3, Unavailable)],
            &[0, 1],
            guests,
        )
    }

    /// What a case wants afterwards: whether the operation was made, what
    /// each slot plays, the party, each slot's Ready mark, and each occupied
    /// slot's simulation seat as (slot, seat).
    struct Want {
        made: bool,
        playing: [Option<Playing>; SLOTS],
        party: &'static [u8],
        ready: [bool; SLOTS],
        seats: &'static [(usize, u8)],
    }

    type Case = (&'static str, Lobby, fn(&mut Lobby) -> bool, Want);

    fn mark_ready(lobby: &mut Lobby) {
        lobby.ready(1, true);
        lobby.ready(2, true);
    }

    /// Slot `slot` asks for companion `npc`.
    fn pick(lobby: &mut Lobby, slot: usize, npc: u8, swap: bool) -> bool {
        lobby.choose(slot, Pick::Companion(npc), swap)
    }

    /// Slot `slot` asks to swap for the story's character.
    fn take_hero(lobby: &mut Lobby, slot: usize) -> bool {
        lobby.choose(slot, Pick::Hero, true)
    }

    fn cases() -> Vec<Case> {
        vec![
            (
                "joining guests take the save's party",
                usual(2),
                |_| true,
                Want {
                    made: true,
                    playing: [Some(Hero), Some(Plays(0)), Some(Plays(1))],
                    party: &[0, 1],
                    ready: [true, false, false],
                    seats: &[(0, 0), (1, 1), (2, 2)],
                },
            ),
            (
                "a free pick frees the old companion",
                usual(1),
                |lobby| pick(lobby, 1, 2, false),
                Want {
                    made: true,
                    playing: [Some(Hero), Some(Plays(2)), None],
                    party: &[2, 1],
                    ready: [true, false, false],
                    seats: &[(0, 0), (1, 1)],
                },
            ),
            (
                "a swap trades characters and clears both Ready marks",
                usual(2),
                |lobby| {
                    mark_ready(lobby);
                    pick(lobby, 1, 1, true)
                },
                Want {
                    made: true,
                    playing: [Some(Hero), Some(Plays(1)), Some(Plays(0))],
                    party: &[1, 0],
                    ready: [true, false, false],
                    seats: &[(0, 0), (1, 1), (2, 2)],
                },
            ),
            (
                "an unavailable companion is refused",
                usual(1),
                |lobby| pick(lobby, 1, 3, false),
                Want {
                    made: false,
                    playing: [Some(Hero), Some(Plays(0)), None],
                    party: &[0, 1],
                    ready: [true, false, false],
                    seats: &[(0, 0), (1, 1)],
                },
            ),
            (
                "the host cannot give up the main character for a free companion",
                usual(1),
                |lobby| pick(lobby, 0, 2, false),
                Want {
                    made: false,
                    playing: [Some(Hero), Some(Plays(0)), None],
                    party: &[0, 1],
                    ready: [true, false, false],
                    seats: &[(0, 0), (1, 1)],
                },
            ),
            (
                "a third companion is refused",
                usual(0),
                |lobby| lobby.fill(2),
                Want {
                    made: false,
                    playing: [Some(Hero), None, None],
                    party: &[0, 1],
                    ready: [true, false, false],
                    seats: &[(0, 0)],
                },
            ),
            (
                "a required companion stays when nobody plays it",
                lobby(&[(0, Open), (4, Required)], &[4], 0),
                |_| true,
                Want {
                    made: true,
                    playing: [Some(Hero), None, None],
                    party: &[4],
                    ready: [true, false, false],
                    seats: &[(0, 0)],
                },
            ),
            (
                "a guest who leaves frees the companion",
                usual(2),
                |lobby| {
                    lobby.leave(2);
                    true
                },
                Want {
                    made: true,
                    playing: [Some(Hero), Some(Plays(0)), None],
                    party: &[0],
                    ready: [true, false, false],
                    seats: &[(0, 0), (1, 1)],
                },
            ),
            (
                "a new save drops the old choices and gives the main character to the host",
                usual(2),
                |lobby| {
                    pick(lobby, 1, 2, false);
                    take_hero(lobby, 2);
                    mark_ready(lobby);
                    let companions = vec![companion(5, Open), companion(6, Open)];
                    lobby.reset(summary(), companions, &[6]);
                    true
                },
                Want {
                    made: true,
                    playing: [Some(Hero), Some(Plays(6)), Some(Plays(5))],
                    party: &[6, 5],
                    ready: [true, false, false],
                    seats: &[(0, 0), (1, 1), (2, 2)],
                },
            ),
            (
                "a save with no companions gives droids and refuses choices",
                lobby(&[], &[], 2),
                |lobby| pick(lobby, 1, 0, false),
                Want {
                    made: false,
                    playing: [Some(Hero), Some(Droid), Some(Droid)],
                    party: &[],
                    ready: [true, false, false],
                    seats: &[(0, 0), (1, 1), (2, 2)],
                },
            ),
            (
                "a second request for a companion taken first is refused",
                usual(2),
                |lobby| pick(lobby, 1, 2, false) && !pick(lobby, 2, 2, false),
                Want {
                    made: true,
                    playing: [Some(Hero), Some(Plays(2)), Some(Plays(1))],
                    party: &[2, 1],
                    ready: [true, false, false],
                    seats: &[(0, 0), (1, 1), (2, 2)],
                },
            ),
            (
                "the host puts a companion in a free place and takes it out",
                usual(0),
                |lobby| lobby.fill(1) && lobby.fill(2) && lobby.fill(2),
                Want {
                    made: true,
                    playing: [Some(Hero), None, None],
                    party: &[0],
                    ready: [true, false, false],
                    seats: &[(0, 0)],
                },
            ),
            (
                "the host cannot take a guest's companion",
                usual(1),
                |lobby| lobby.fill(0),
                Want {
                    made: false,
                    playing: [Some(Hero), Some(Plays(0)), None],
                    party: &[0, 1],
                    ready: [true, false, false],
                    seats: &[(0, 0), (1, 1)],
                },
            ),
            (
                "a guest takes the main character and the host gets the guest's companion",
                usual(2),
                |lobby| {
                    mark_ready(lobby);
                    take_hero(lobby, 1)
                },
                Want {
                    made: true,
                    playing: [Some(Plays(0)), Some(Hero), Some(Plays(1))],
                    party: &[0, 1],
                    ready: [true, false, true],
                    seats: &[(0, 1), (1, 0), (2, 2)],
                },
            ),
            (
                "the host takes the main character back",
                usual(2),
                |lobby| take_hero(lobby, 1) && take_hero(lobby, 0),
                Want {
                    made: true,
                    playing: [Some(Hero), Some(Plays(0)), Some(Plays(1))],
                    party: &[0, 1],
                    ready: [true, false, false],
                    seats: &[(0, 0), (1, 1), (2, 2)],
                },
            ),
            (
                "a guest with a droid takes the main character and the host gets the droid",
                lobby(&[(0, Open)], &[0], 2),
                |lobby| take_hero(lobby, 2),
                Want {
                    made: true,
                    playing: [Some(Droid), Some(Plays(0)), Some(Hero)],
                    party: &[0],
                    ready: [true, false, false],
                    seats: &[(0, 1), (1, 2), (2, 0)],
                },
            ),
            (
                "a request for the main character that is not a swap is refused",
                usual(1),
                |lobby| lobby.choose(1, Pick::Hero, false),
                Want {
                    made: false,
                    playing: [Some(Hero), Some(Plays(0)), None],
                    party: &[0, 1],
                    ready: [true, false, false],
                    seats: &[(0, 0), (1, 1)],
                },
            ),
            (
                "the main character's player leaves and the host plays it again",
                usual(2),
                |lobby| {
                    take_hero(lobby, 1);
                    lobby.leave(1);
                    true
                },
                Want {
                    made: true,
                    playing: [Some(Hero), None, Some(Plays(1))],
                    party: &[1],
                    ready: [true, false, false],
                    seats: &[(0, 0), (2, 1)],
                },
            ),
            (
                "a guest who plays the main character cannot take a free companion",
                usual(1),
                |lobby| take_hero(lobby, 1) && !pick(lobby, 1, 2, false),
                Want {
                    made: true,
                    playing: [Some(Plays(0)), Some(Hero), None],
                    party: &[0, 1],
                    ready: [true, false, false],
                    seats: &[(0, 1), (1, 0)],
                },
            ),
            (
                "a host who plays a companion takes a free one",
                usual(1),
                |lobby| take_hero(lobby, 1) && pick(lobby, 0, 2, false),
                Want {
                    made: true,
                    playing: [Some(Plays(2)), Some(Hero), None],
                    party: &[2, 1],
                    ready: [true, false, false],
                    seats: &[(0, 1), (1, 0)],
                },
            ),
            (
                "the main character's player swaps it for another guest's companion",
                usual(2),
                |lobby| take_hero(lobby, 1) && pick(lobby, 1, 1, true),
                Want {
                    made: true,
                    playing: [Some(Plays(0)), Some(Plays(1)), Some(Hero)],
                    party: &[0, 1],
                    ready: [true, false, false],
                    seats: &[(0, 1), (1, 2), (2, 0)],
                },
            ),
            (
                "two guests ask for the main character in one step and the second swaps with the first",
                usual(2),
                |lobby| take_hero(lobby, 1) && take_hero(lobby, 2),
                Want {
                    made: true,
                    playing: [Some(Plays(0)), Some(Plays(1)), Some(Hero)],
                    party: &[0, 1],
                    ready: [true, false, false],
                    seats: &[(0, 1), (1, 2), (2, 0)],
                },
            ),
            (
                "a lobby left with no main character gives it to the host",
                usual(2),
                |lobby| {
                    take_hero(lobby, 1);
                    lobby.slots[1] = None;
                    lobby.keep_one_hero()
                },
                Want {
                    made: true,
                    playing: [Some(Hero), None, Some(Plays(1))],
                    party: &[1],
                    ready: [true, false, false],
                    seats: &[(0, 0), (2, 1)],
                },
            ),
        ]
    }

    /// The heroes among the occupied slots of `lobby`.
    fn heroes(lobby: &Lobby) -> usize {
        (0..SLOTS)
            .filter(|&slot| lobby.playing(slot) == Some(Hero))
            .count()
    }

    /// The operations a long run of lobby changes picks from, by number.
    const OPERATIONS: u64 = 7;

    /// Applies operation `which` (a number below `OPERATIONS`) with the numbers
    /// `slot` and `npc` to `lobby`.
    fn operate(lobby: &mut Lobby, which: u64, slot: usize, npc: u8) {
        match which {
            0 => {
                lobby.choose(slot, Pick::Hero, true);
            }
            1 => {
                pick(lobby, slot, npc, slot.is_multiple_of(2));
            }
            2 => {
                lobby.fill(npc);
            }
            3 => lobby.leave(slot),
            4 => {
                lobby.join(format!("guest {npc}"));
            }
            5 => {
                let companions = vec![companion(npc, Open), companion(npc + 1, Required)];
                lobby.reset(summary(), companions, &[npc]);
            }
            _ => lobby.ready(slot, npc.is_multiple_of(2)),
        }
    }

    /// Ten thousand changes picked by a fixed sequence of numbers: after each
    /// one, exactly one occupied slot plays the story's character.
    fn always_one_hero() -> Result<String, String> {
        let mut lobby = usual(2);
        let mut state = 0x2545_f491_4f6c_dd1d_u64;
        for step in 0..10_000 {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            let which = state % OPERATIONS;
            let slot = (state >> 8) as usize % SLOTS;
            let npc = (state >> 16) as u8 % 5;
            operate(&mut lobby, which, slot, npc);
            if heroes(&lobby) != 1 {
                return Err(format!(
                    "step {step}, operation {which} on slot {slot} and companion {npc}: slots {:?}",
                    lobby.slots
                ));
            }
        }
        Ok("10000 changes, one main character after each".into())
    }

    /// Each case's operation on its lobby, compared with what it wants;
    /// then a long run of changes that must always leave one main
    /// character.
    pub fn probes() -> Vec<(&'static str, Result<String, String>)> {
        let mut checks: Vec<_> = cases()
            .into_iter()
            .map(|(name, mut lobby, operation, want)| {
                let made = operation(&mut lobby);
                let playing: Vec<Option<Playing>> =
                    (0..SLOTS).map(|slot| lobby.playing(slot)).collect();
                let ready: Vec<bool> = (lobby.slots.iter())
                    .map(|slot| slot.as_ref().is_some_and(|slot| slot.ready))
                    .collect();
                let party = lobby.party();
                let seats: Vec<(usize, u8)> = (lobby.seats().into_iter())
                    .map(|(slot, seat, _)| (slot, seat))
                    .collect();
                let seen = format!(
                    "made {made}, playing {playing:?}, party {party:?}, ready {ready:?}, seats {seats:?}"
                );
                let ok = made == want.made
                    && playing == want.playing
                    && party == want.party
                    && ready == want.ready
                    && seats == want.seats;
                (name, if ok { Ok(seen) } else { Err(seen) })
            })
            .collect();
        checks.push((
            "no run of changes leaves the lobby without exactly one main character",
            always_one_hero(),
        ));
        checks
    }
}
