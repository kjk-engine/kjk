//! The co-op lobby's state and its rules, decided on the host. Nothing here
//! reads a file or the network. The host's game applies each guest's
//! request and its own player's choices here, and sends the result to every
//! guest.
//!
//! The lobby has three slots. Slot 0 is the host, who plays the save's main
//! character. Slots 1 and 2 are guests. Each guest plays a companion or, when
//! the party has no companion left for them, a remote droid. The party has
//! two companion places beside the main character. A companion that the
//! story requires is always in the party. When no player chooses it, the
//! game controls it. The host can put other companions in a free place, and
//! the game controls them too.

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
/// seconds since 1970. The main character is shown by the name the player
/// gave it in character creation.
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
    /// The save's main character (the host).
    Hero,
    /// The companion with this party number.
    Companion(u8),
    /// A remote droid, because the party has no companion for this player.
    Droid,
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

    /// The guest slot that plays `npc`.
    fn played_by(&self, npc: u8) -> Option<usize> {
        (1..SLOTS).find(|&slot| self.playing(slot) == Some(Playing::Companion(npc)))
    }

    /// What slot `slot` plays, when a player is in it.
    pub fn playing(&self, slot: usize) -> Option<Playing> {
        Some(self.slots.get(slot)?.as_ref()?.playing)
    }

    /// The companions that start in the party: each guest's companion in
    /// slot order, then the required companions no player chose, then those
    /// the host put in a free place. Never more than `COMPANION_PLACES`.
    pub fn party(&self) -> Vec<u8> {
        let played = (1..SLOTS).filter_map(|slot| match self.playing(slot) {
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
    /// only if the story requires it.
    pub fn leave(&mut self, slot: usize) {
        if slot > 0
            && let Some(place) = self.slots.get_mut(slot)
        {
            *place = None;
        }
    }

    /// The guest in slot `slot` asks to play `npc`. A companion nobody plays
    /// is taken, and the guest's old companion becomes free. A companion
    /// another guest plays is swapped with that guest, but only when `swap`
    /// says the guest asked for a swap. A request for a companion the guest
    /// saw as free is refused when another guest took it first. Refused too:
    /// slot 0, an empty slot, a companion the lobby does not list, one the
    /// story made unavailable, and a choice that would put a third companion
    /// in the party. Whether the choice was made.
    pub fn choose(&mut self, slot: usize, npc: u8, swap: bool) -> bool {
        let Some(own) = self.playing(slot).filter(|_| slot > 0) else {
            return false;
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
        let before = self.slots.clone();
        match self.played_by(npc) {
            Some(other) if swap => self.set_playing(other, own),
            Some(_) => return false,
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
    /// old save is dropped. The host's places start as the save's party,
    /// then each guest is given the default companion in slot order. Every
    /// Ready mark is cleared.
    pub fn reset(&mut self, summary: Summary, companions: Vec<Companion>, current: &[u8]) {
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

    /// What each occupied slot plays when the game starts, by slot.
    pub fn seats(&self) -> Vec<(usize, Playing)> {
        (0..SLOTS)
            .filter_map(|slot| Some((slot, self.playing(slot)?)))
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
    /// each slot plays, the party, and each slot's Ready mark.
    struct Want {
        made: bool,
        playing: [Option<Playing>; SLOTS],
        party: &'static [u8],
        ready: [bool; SLOTS],
    }

    type Case = (&'static str, Lobby, fn(&mut Lobby) -> bool, Want);

    fn mark_ready(lobby: &mut Lobby) {
        lobby.ready(1, true);
        lobby.ready(2, true);
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
                },
            ),
            (
                "a free pick frees the old companion",
                usual(1),
                |lobby| lobby.choose(1, 2, false),
                Want {
                    made: true,
                    playing: [Some(Hero), Some(Plays(2)), None],
                    party: &[2, 1],
                    ready: [true, false, false],
                },
            ),
            (
                "a swap trades characters and clears both Ready marks",
                usual(2),
                |lobby| {
                    mark_ready(lobby);
                    lobby.choose(1, 1, true)
                },
                Want {
                    made: true,
                    playing: [Some(Hero), Some(Plays(1)), Some(Plays(0))],
                    party: &[1, 0],
                    ready: [true, false, false],
                },
            ),
            (
                "an unavailable companion is refused",
                usual(1),
                |lobby| lobby.choose(1, 3, false),
                Want {
                    made: false,
                    playing: [Some(Hero), Some(Plays(0)), None],
                    party: &[0, 1],
                    ready: [true, false, false],
                },
            ),
            (
                "the host's slot cannot choose",
                usual(1),
                |lobby| lobby.choose(0, 2, false),
                Want {
                    made: false,
                    playing: [Some(Hero), Some(Plays(0)), None],
                    party: &[0, 1],
                    ready: [true, false, false],
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
                },
            ),
            (
                "a new save drops the old choices",
                usual(2),
                |lobby| {
                    lobby.choose(1, 2, false);
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
                },
            ),
            (
                "a save with no companions gives droids and refuses choices",
                lobby(&[], &[], 2),
                |lobby| lobby.choose(1, 0, false),
                Want {
                    made: false,
                    playing: [Some(Hero), Some(Droid), Some(Droid)],
                    party: &[],
                    ready: [true, false, false],
                },
            ),
            (
                "a second request for a companion taken first is refused",
                usual(2),
                |lobby| lobby.choose(1, 2, false) && !lobby.choose(2, 2, false),
                Want {
                    made: true,
                    playing: [Some(Hero), Some(Plays(2)), Some(Plays(1))],
                    party: &[2, 1],
                    ready: [true, false, false],
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
                },
            ),
        ]
    }

    /// Each case's operation on its lobby, compared with what it wants;
    /// then the seats of a full lobby.
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
                let seen =
                    format!("made {made}, playing {playing:?}, party {party:?}, ready {ready:?}");
                let ok = made == want.made
                    && playing == want.playing
                    && party == want.party
                    && ready == want.ready;
                (name, if ok { Ok(seen) } else { Err(seen) })
            })
            .collect();
        let seats = usual(2).seats();
        let want = [(0, Hero), (1, Plays(0)), (2, Plays(1))];
        let seen = format!("{seats:?}");
        checks.push((
            "the seats list every player",
            if seats == want { Ok(seen) } else { Err(seen) },
        ));
        checks
    }
}
