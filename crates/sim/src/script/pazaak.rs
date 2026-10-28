//! Pazaak's routines and step (`crate::pazaak`, `docs/PAZAAK.md`):
//! `PlayPazaak` starts a match for one player body (whoever asked for it,
//! I30 P8), several playing at once (R9). Its player's moves and time play
//! it, and at its end the credits are settled, the result kept for
//! `GetLastPazaakResult` and the end script run. Each wager is held out of
//! the party's gold from when it is placed (another seat cannot spend it
//! meanwhile): back to the party with as much again won, back on a quit,
//! gone with a loss. Other players watch a match (R9a): those free and
//! near as it begins, and any free one who asks.

use script_ncs::vm::ObjectId;

use super::dialogue::{Pick, Release};
use super::{Event, Library, State, Step, View, key};
use crate::pazaak::{Match, Move, Phase};

/// What `PlayPazaak` asks: the opponent's deck (a `pazaakdecks.2da` row),
/// the end script, the most the player may wager, the tutorial.
pub(super) struct Call<'a> {
    pub deck: i32,
    pub end_script: &'a str,
    pub max_wager: i32,
    pub tutorial: bool,
}

impl State {
    /// Whether `body` plays a match.
    pub(crate) fn plays_pazaak(&self, body: ObjectId) -> bool {
        self.pazaak.iter().any(|table| table.player == body.0)
    }

    /// The match `body` watches (R9a), if any.
    pub(crate) fn watching(&self, body: ObjectId) -> Option<&Match> {
        self.pazaak
            .iter()
            .find(|table| table.watchers.contains(&body.0))
    }

    /// `body` watches no match any more.
    fn stop_watching(&mut self, body: ObjectId) {
        for table in &mut self.pazaak {
            table.watchers.retain(|watcher| *watcher != body.0);
        }
    }

    /// Every match ends as its module goes, a script's call or an exit
    /// (R9, R11): one decided settles as decided, the rest as a quit (the
    /// wager back, `GetLastPazaakResult` 0). No end script runs: the module
    /// it is of is going, and several of the games' book a loss or take
    /// credits.
    pub(crate) fn leave_matches(&mut self) {
        for table in std::mem::take(&mut self.pazaak) {
            let (won, settled) = match table.phase {
                Phase::Over { won } => (won, table.settled(won)),
                _ => (false, 0),
            };
            self.gold = self
                .gold
                .saturating_add(table.wager.saturating_add(settled));
            self.pazaak_won = won;
        }
    }
}

/// `PlayPazaak`: a match for the party played by `player`, its end script
/// run for `runs_for`; none while `player` plays one, or where the install
/// has no decks.
pub(super) fn start(
    library: &Library,
    state: &mut State,
    call: &Call,
    (runs_for, player): (ObjectId, ObjectId),
) -> bool {
    let decks = library.pazaak();
    let Some(opponent) = decks.row(call.deck).filter(|_| !state.plays_pazaak(player)) else {
        return false;
    };
    let held = state
        .items
        .held_by(crate::inventory::PARTY)
        .map(|(_, item)| (item.template.as_str(), item.stack));
    let available = decks.owned(held);
    state.pazaak.push(Match {
        player: player.0,
        ..Match::new(
            opponent,
            available,
            (call.max_wager, state.gold),
            call.tutorial,
            (key(call.end_script), runs_for.0),
        )
    });
    true
}

/// What watching's rule (R9a) reads besides the body it asks about: the
/// bodies the seats act through (`View::seats`), the host's two bodies
/// (`View::host`), the party members a fight is near (`View::embattled`),
/// the conversations under way and the matches. The scripts read it off
/// their view and state (`rule`); a use at a table (`world::with_tables`,
/// R41) off the step's resources, as a client predicting does.
pub(crate) struct WatchRule<'a> {
    pub seats: &'a [(u8, ObjectId)],
    pub host: [ObjectId; 2],
    pub embattled: &'a [ObjectId],
    pub conversations: &'a [super::dialogue::Conversation],
    pub tables: &'a [Match],
}

/// A body as watching's rule reads it: whether it is down (or no
/// character), in a fight of its own (`Fighter::combat_until`), and
/// trading.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Watcher {
    pub body: ObjectId,
    pub down: bool,
    pub in_combat: bool,
    pub trading: bool,
}

impl WatchRule<'_> {
    /// What keeps `who` from watching a match, a fight aside: it is no
    /// seat's standing body, or it stands in a conversation (a party one
    /// holds every seat; never conversation `beside`, a player's own it is
    /// joined to), trades or plays a match of its own.
    fn engaged(&self, who: Watcher, beside: Option<u32>) -> bool {
        let talks = self.conversations.iter().find(|c| c.holds(who.body));
        !self.seats.iter().any(|(_, body)| *body == who.body)
            || who.down
            || talks.is_some_and(|c| Some(c.id) != beside)
            || self.conversations.iter().any(|c| c.party)
            || who.trading
            || self.tables.iter().any(|table| table.player == who.body.0)
    }

    /// Whether `who` is in a fight (R34): its own, or a hostile one keeping
    /// the party in combat within reach of it.
    fn fighting(&self, who: Watcher) -> bool {
        who.in_combat || self.embattled.contains(&who.body)
    }

    /// Whether `who` is free to watch the match `player` plays (R9a):
    /// another player's (more than one seat plays a body, and `player` is
    /// neither `who` nor, for the host, its other body), `who` not
    /// `engaged` (but for conversation `beside`), not in a fight, and
    /// watching none. The one rule for a match's start, an ask and a use
    /// at a table: one player alone watches nothing.
    pub(crate) fn free(&self, who: Watcher, beside: Option<u32>, player: ObjectId) -> bool {
        let body = who.body;
        let own = body == player || self.host.contains(&body) && self.host.contains(&player);
        let watching = (self.tables.iter()).any(|table| table.watchers.contains(&body.0));
        self.seats.len() > 1
            && !own
            && !self.engaged(who, beside)
            && !self.fighting(who)
            && !watching
    }
}

/// Watching's rule as the scripts read it (`WatchRule`).
fn rule<'a>(view: &'a View, state: &'a State) -> WatchRule<'a> {
    WatchRule {
        seats: view.seats,
        host: view.host(&state.party),
        embattled: view.embattled,
        conversations: &state.talk.conversations,
        tables: &state.pazaak,
    }
}

/// `body` as the scripts see it (`Watcher`).
fn watcher(view: &View, state: &State, body: ObjectId) -> Watcher {
    let character = view.thing(body).and_then(|thing| thing.character);
    Watcher {
        body,
        down: character.is_none_or(|character| character.downed),
        in_combat: character.is_some_and(|character| character.in_combat),
        trading: state.trades.contains_key(&body.0),
    }
}

/// Whether `body` is free to watch the match `player` plays
/// (`WatchRule::free`), as the scripts see it.
fn free(
    view: &View,
    state: &State,
    (body, beside): (ObjectId, Option<u32>),
    player: ObjectId,
) -> bool {
    rule(view, state).free(watcher(view, state, body), beside, player)
}

/// Whether `body` stands within `GATHER_BEYOND_M` of `player` (R9a).
fn near(view: &View, body: ObjectId, player: ObjectId) -> bool {
    use crate::math::{length, sub};
    use crate::world::coop_talk::GATHER_BEYOND_M;
    let at = |who| view.thing(who).map(|thing| thing.position);
    at(body)
        .zip(at(player))
        .is_some_and(|(body, player)| length(sub(body, player)) <= GATHER_BEYOND_M)
}

/// `watchers` watch the match `player` plays, after any already watching.
fn watch(state: &mut State, player: ObjectId, watchers: &[ObjectId]) {
    if let Some(table) = state
        .pazaak
        .iter_mut()
        .find(|table| table.player == player.0)
    {
        table.watchers.extend(watchers.iter().map(|body| body.0));
    }
}

/// A match for `player` began (`PlayPazaak`), dealt in conversation `talk`
/// (`dialogue::running`, which remembers it: `Conversation::dealt`): every
/// other seat's body free and within `GATHER_BEYOND_M` of it watches it at
/// once (R9a), in seat order, and so does each player joined to that
/// conversation (a player's own), free but for it, stepping out of it
/// (R35).
pub(super) fn watch_begun(view: &View, state: &mut State, player: ObjectId, talk: Option<u32>) {
    if view.thing(player).is_none() {
        return;
    }
    let dealing = talk.and_then(|id| state.talk.conversations.iter().position(|c| c.id == id));
    if let Some(at) = dealing {
        state.talk.conversations[at].dealt = player;
    }
    let joined: Vec<ObjectId> = dealing
        .map(|at| &state.talk.conversations[at])
        .filter(|c| !c.party)
        .map_or_else(Vec::new, |c| c.joined.clone());
    let comes = |body: ObjectId| {
        let beside = talk.filter(|_| joined.contains(&body));
        (near(view, body, player) || beside.is_some()) && free(view, state, (body, beside), player)
    };
    let watchers: Vec<ObjectId> = view
        .seats
        .iter()
        .map(|(_, body)| *body)
        .filter(|body| comes(*body))
        .collect();
    if let Some(at) = dealing {
        for body in watchers.iter().filter(|body| joined.contains(body)) {
            state.talk.conversations[at].step_out(*body, view.seat_of(*body));
        }
    }
    watch(state, player, &watchers);
}

/// Conversation `c` ended, its end script run (R37): while the match its
/// line dealt plays on (`Conversation::dealt`), its listener and those
/// joined, free now and within `GATHER_BEYOND_M` of that match's player,
/// watch it, in seat order: the host whose conversation a joined guest
/// took the dealing reply in, once it ends.
pub(super) fn watch_after_talk(view: &View, state: &mut State, c: &super::dialogue::Conversation) {
    let player = c.dealt;
    if !state.plays_pazaak(player) {
        return;
    }
    let watchers: Vec<ObjectId> = view
        .seats
        .iter()
        .map(|(_, body)| *body)
        .filter(|body| {
            c.holds(*body) && near(view, *body, player) && free(view, state, (*body, None), player)
        })
        .collect();
    watch(state, player, &watchers);
}

/// Who stops and who starts watching (R9a), before the matches play: a
/// watcher hurt, or whose seat left (its Leave while the body it acts
/// through, `View::seats`, watches and stands in no conversation: that
/// Leave is spent, `Step::left_watching`, and leaves no conversation of
/// the seat's other body), stops; then each ask in seat order
/// (`View::watches`): none stops its asker's watching, a player its
/// match's, which a free asker then watches.
fn asked_to_watch(step: &mut Step) {
    let (view, state) = (step.view, &mut *step.state);
    let left: Vec<(u8, ObjectId)> = (view.picks.iter())
        .filter(|(_, answer)| answer.pick == Pick::Leave)
        .filter_map(|(seat, _)| view.seats.iter().find(|(of, _)| of == seat).copied())
        .filter(|(_, body)| state.talk.holding(*body).is_none() && state.watching(*body).is_some())
        .collect();
    let hurt = state.talk.hurt.clone();
    for body in hurt.into_iter().chain(left.iter().map(|(_, body)| *body)) {
        state.stop_watching(body);
    }
    step.left_watching = left.into_iter().map(|(seat, _)| seat).collect();
    for &(watcher, player) in view.watches {
        if player == u32::MAX {
            state.stop_watching(watcher);
        } else if free(view, state, (watcher, None), ObjectId(player))
            && let Some(table) = state.pazaak.iter_mut().find(|table| table.player == player)
        {
            table.watchers.push(watcher.0);
        }
    }
}

/// After the step's scripts and conversations (R9a): a watcher `engaged`
/// now (its seat gone or it down, a conversation holding it, a trade or a
/// match of its own begun), or that a fight reached (`fighting`, R39),
/// stops watching.
pub(super) fn engaged_stop_watching(step: &mut Step) {
    let (view, state) = (step.view, &mut *step.state);
    let rule = rule(view, state);
    let engaged: Vec<ObjectId> = (state.pazaak.iter())
        .flat_map(|table| &table.watchers)
        .map(|body| watcher(view, state, ObjectId(*body)))
        .filter(|who| rule.engaged(*who, None) || rule.fighting(*who))
        .map(|who| who.body)
        .collect();
    for body in engaged {
        state.stop_watching(body);
    }
}

/// `PlayPazaak(deck, end_script, max_wager)` for `player` in `world`, its
/// end script run for `runs_for`, as a script would ask it between steps
/// (the probes' tables).
#[cfg(feature = "probes")]
pub(crate) fn deal(
    world: &mut crate::SimWorld,
    (player, runs_for): (u32, u32),
    deck: i32,
    end_script: &str,
    max_wager: i32,
) -> bool {
    let call = Call {
        deck,
        end_script,
        max_wager,
        tutorial: false,
    };
    let mut state = world.script_state().clone();
    let dealt = world.level().scripts().is_some_and(|library| {
        start(
            library,
            &mut state,
            &call,
            (ObjectId(runs_for), ObjectId(player)),
        )
    });
    world.set_script_state(state);
    dealt
}

/// The matches' step: who watches (`asked_to_watch`), then each match in
/// its player's seat order (those no seat drives after, in the order they
/// began): what befell its player (`placed`), its player's moves (a wager
/// past the party's gold now refused), then time passing. A match that ends
/// settles its credits and runs its end script, `GetLastPazaakResult` its
/// own result, its player the run's asker (a conversation it starts is that
/// player's own).
pub(super) fn step(step: &mut Step) {
    asked_to_watch(step);
    let (view, now) = (step.view, step.view.now_ms);
    let mut players: Vec<u32> = step.state.pazaak.iter().map(|table| table.player).collect();
    players.sort_by_key(|player| view.seat_of(ObjectId(*player)).unwrap_or(u8::MAX));
    for player in players {
        let state = &mut *step.state;
        let Some(at) = state.pazaak.iter().position(|table| table.player == player) else {
            continue;
        };
        let held = state.pazaak[at].wager;
        placed(step.view, state, at);
        let table = &mut state.pazaak[at];
        for (_, played) in step.view.pazaak.iter().filter(|(body, _)| body.0 == player) {
            if !matches!(played, Move::Wager(wager) if *wager > state.gold) {
                table.apply(*played, now, step.rng);
            }
        }
        state.gold = state.gold.saturating_sub(table.wager - held);
        let Some(won) = table.advance(now, step.rng) else {
            continue;
        };
        let table = state.pazaak.remove(at);
        state.gold = state
            .gold
            .saturating_add(table.wager.saturating_add(table.settled(won)));
        state.pazaak_won = won;
        let caller = ObjectId(table.caller);
        if !table.end_script.is_empty() && !step.gone(caller) {
            let event = Event {
                asker: Some(ObjectId(player)),
                ..Event::default()
            };
            step.run(&table.end_script, caller, event, None);
        }
    }
}

/// What befell match `at`'s player this step (R9, R12). Its seat let it go
/// as it died or left the party: the match ends as a quit (one decided
/// settles as decided). Its seat dropped: paused while the seat is held for
/// it (`KEEP_MS`), as a quit after, unless a seat drives it again. While
/// more than one seat plays a body (R9), it is paused too while a party
/// conversation holds every seat, and from a hurt until its player is out
/// of combat (`Match::hurt`, apart from the other causes); one player's
/// match plays on, as the originals' does.
fn placed(view: &super::View, state: &mut State, at: usize) {
    use crate::world::drivers::KEEP_MS;
    let now = view.now_ms;
    let talk = &state.talk;
    let player = ObjectId(state.pazaak[at].player);
    let released = talk
        .released
        .iter()
        .find(|(body, _)| *body == player)
        .map(|(_, why)| *why);
    let hurt = talk.hurt.contains(&player);
    let gathered = talk.party().is_some();
    let fighting = view
        .thing(player)
        .and_then(|thing| thing.character)
        .is_some_and(|character| character.in_combat);
    let table = &mut state.pazaak[at];
    match released {
        Some(Release::Died | Release::Left) => return table.quit(now),
        Some(Release::Dropped) => table.away = table.away.or(Some(now)),
        None => {}
    }
    if view.seat_of(player).is_some() {
        table.away = None;
    }
    if table.away.is_some_and(|since| now >= since + KEEP_MS) {
        return table.quit(now);
    }
    let together = view.together();
    table.hurt = together && fighting && (hurt || table.hurt);
    table.hold(
        table.away.is_some() || table.hurt || together && gathered,
        now,
    );
}

/// A script asked for another module this step: every match ends
/// (`State::leave_matches`) while its module is still here.
pub(super) fn leaving_module(step: &mut Step) {
    if step.out.new_module.is_some() && !step.state.pazaak.is_empty() {
        step.state.leave_matches();
    }
}
