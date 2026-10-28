//! Pazaak's side of the game (render side only: the presented snapshot's
//! match, `sim::pazaak::Match`; `docs/PAZAAK.md`):
//!
//! - the view model's table (`ui::vm::PazaakView`): what its player sees,
//!   for that player alone (whoever asked for the match, I30 P8, P20: the
//!   others play on); the cards in the campaign's own art;
//! - the screen of the match's stage open (wager, side deck, table), the
//!   others closed, all closed at its end (this owns them: the script's
//!   `PlayPazaak` notice opens nothing);
//! - a set's and the match's result over the table, KOTOR's words in its
//!   message box, and in tutorial mode the tutorial's pages as the side
//!   deck and the table first open;
//! - a screen's move as the sim's (`move_of`);
//! - watching another player's match (R9a, `Snapshot::watching`): its
//!   table read-only as its player sees it (kept while the match is
//!   paused, told why: R39), "Watching Carth" over it, its
//!   Leave button and the Leave key (`LeaveTalk`, the console's `ui leave`)
//!   stopping it (`Action::Watch` with no player), and on the HUD a click
//!   on a companion's pazaak icon (the console's `ui watch 2|3`) watching
//!   that player's match.
//! - the table's sounds, as K1's (`ui_sounds`): a main-deck card drawn, a
//!   side card played, the player's turn, a set and the match won or lost;
//!   and the match's credits won or lost, which the sim settles without a
//!   status, in the status summary (`progress::Told`).

use bevy::prelude::*;
use script_ncs::vm::ObjectId;
use sim::action::Action;
use sim::pazaak::{Card, Kind, Match, Move, Outcome, Phase};
use sim::{Seat, Snapshot};
use ui::draw::Textures;
use ui::intent::PazaakMove;
use ui::skin::Source;
use ui::vm::{
    PazaakCard, PazaakHeld, PazaakOffer, PazaakSide, PazaakStage, PazaakTold, PazaakView,
    PazaakWatch,
};

use crate::loot::Outgoing;
use crate::settings::{Action as KeyAction, KeyMap};
use crate::sim_host::{OwnSeat, PresentedSnapshot, SceneSwap};
use crate::ui_sounds as sounds;
use crate::ui_view::Ui;

/// Each stage's screen.
const SCREENS: [(PazaakStage, &str); 3] = [
    (PazaakStage::Wager, "pazaak_wager"),
    (PazaakStage::Setup, "pazaak_setup"),
    (PazaakStage::Game, "pazaak_game"),
];

/// The tutorial's pages (talk-table lines) as each stage first opens.
const TUTORIAL: [(PazaakStage, &[u32]); 2] = [
    (PazaakStage::Setup, &[38627, 38628, 38629]),
    (
        PazaakStage::Game,
        &[
            38631, 38632, 38633, 38634, 38635, 38636, 38637, 38638, 38646, 38648,
        ],
    ),
];

/// The message box's screen (`ui::screens::message`).
const MESSAGE: &str = "message";

/// "Maximum wager:"; a set's results (won, lost, tied) and the match's
/// (won, lost).
const MAXIMUM: u32 = 32321;
const SET_WON: u32 = 32334;
const SET_LOST: u32 = 32335;
const SET_TIED: u32 = 32338;
const MATCH_WON: u32 = 32336;
const MATCH_LOST: u32 = 32337;

/// A campaign's card art: the main deck's, +, −, ± showing + and −, TSL's
/// specials, a set won.
struct Art {
    main: &'static str,
    plus: &'static str,
    minus: &'static str,
    either_plus: &'static str,
    either_minus: &'static str,
    special: &'static str,
    mark: &'static str,
}

const K1_ART: Art = Art {
    main: "lbl_cardstand",
    plus: "lbl_cardmpos",
    minus: "lbl_cardmneg",
    either_plus: "lbl_cardrarem",
    either_minus: "lbl_cardraref",
    special: "lbl_cardraref",
    mark: "lbl_winmark02",
};

const TSL_ART: Art = Art {
    main: "pcards_generic_p",
    plus: "pcards_pos_p",
    minus: "pcards_neg_p",
    either_plus: "pcards_dblpos_p",
    either_minus: "pcards_dblneg_p",
    special: "pcards_gold_p",
    mark: "pz_rounds_on",
};

/// The stage whose screen is open, the result shown, the stages whose
/// tutorial pages were shown, whether some were opened at this table (they
/// may still be up), and the table this player watches.
#[derive(Resource, Default)]
struct Seated {
    stage: Option<PazaakStage>,
    told: Option<PazaakTold>,
    tutored: Vec<PazaakStage>,
    paged: bool,
    watched: Option<Watched>,
}

/// The table a player watches: its player, its player's name, and the
/// snapshot (`Snapshot::tick`) the name was last asked in.
#[derive(Debug, Clone, PartialEq)]
struct Watched {
    player: u32,
    name: Vec<u8>,
    asked: u64,
}

/// This frame's asks from the screens and the console (`ui_view`): the
/// HUD's companion whose pazaak icon was clicked (its body,
/// `ui::vm::MemberView::id`), and a Leave (`ui leave`).
#[derive(Resource, Default)]
pub(crate) struct WatchAsked {
    pub icon: Option<u32>,
    pub leave: bool,
}

pub struct PazaakViewPlugin;

impl Plugin for PazaakViewPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Seated>()
            .init_resource::<WatchAsked>()
            .add_systems(Update, (show, heard).before(crate::ui_view::Stepped))
            .add_systems(Update, watch.after(crate::ui_view::Stepped));
    }
}

/// The match `seat`'s screens show: the one it plays, else the one its
/// body watches (R9a), and whether it watches.
fn table_of(snapshot: &Snapshot, seat: Seat) -> Option<(&Match, bool)> {
    match snapshot.pazaak_of(seat) {
        Some(table) => Some((table, false)),
        None => snapshot.watching(seat).map(|table| (table, true)),
    }
}

/// What `seat`'s asks send (R9a): a Leave while it watches stops watching;
/// a click on the pazaak icon of party member `icon` (its body) watches
/// that member's match.
fn asked(snapshot: &Snapshot, seat: Seat, icon: Option<u32>, leave: bool) -> Option<Action> {
    let stop =
        (leave && snapshot.watching(seat).is_some()).then_some(Action::Watch { player: u32::MAX });
    stop.or(icon.map(|player| Action::Watch { player }))
}

/// A screen's move as the sim's; none for a side deck of a card it does
/// not know.
pub fn move_of(played: PazaakMove) -> Option<Move> {
    Some(match played {
        PazaakMove::Wager(wager) => Move::Wager(wager),
        PazaakMove::Quit => Move::Quit,
        PazaakMove::Deck(cards) => {
            let cards: Vec<Card> = cards
                .iter()
                .map(|card| Card::of(*card))
                .collect::<Option<_>>()?;
            Move::Deck(cards.try_into().ok()?)
        }
        PazaakMove::Play { slot, minus, two } => Move::Play { slot, minus, two },
        PazaakMove::EndTurn => Move::EndTurn,
        PazaakMove::Stand => Move::Stand,
        PazaakMove::Forfeit => Move::Forfeit,
    })
}

/// A side card as it shows: laid on the table with `value`, else in a hand
/// (its faces for each sign and size, when they are the player's).
fn look(card: Card, laid: Option<i8>, art: &Art) -> PazaakCard {
    let negative = laid.is_some_and(|value| value < 0);
    let texture = match card.kind() {
        Kind::Value {
            value,
            either: false,
        } => {
            if value < 0 {
                art.minus
            } else {
                art.plus
            }
        }
        Kind::Value { .. } | Kind::OneOrTwo | Kind::TieBreaker if negative => art.either_minus,
        Kind::Value { .. } | Kind::OneOrTwo | Kind::TieBreaker => art.either_plus,
        Kind::Double | Kind::Flip(_) => art.special,
    };
    let chosen = |at: usize| card.face(Some((at & 1 != 0, at & 2 != 0)));
    PazaakCard {
        face: match laid {
            Some(value) if card.chosen() || card.kind() == Kind::Double => format!("{value:+}"),
            _ => card.face(None),
        },
        art: texture.into(),
        signed: (laid.is_none() && card.chosen()).then(|| std::array::from_fn(chosen)),
        minus_art: art.either_minus.into(),
        sized: laid.is_none() && card.kind() == Kind::OneOrTwo,
    }
}

fn side(side: &sim::pazaak::Side, mine: bool, art: &Art) -> PazaakSide {
    PazaakSide {
        table: side
            .table
            .iter()
            .map(|laid| match laid.card {
                Some(card) => look(card, Some(laid.value), art),
                None => PazaakCard {
                    face: laid.value.to_string(),
                    art: art.main.into(),
                    ..PazaakCard::default()
                },
            })
            .collect(),
        hand: side.hand.map(|held| {
            held.map(|card| {
                if mine {
                    look(card, None, art)
                } else {
                    PazaakCard::default()
                }
            })
        }),
        total: i32::from(side.total()),
        sets: side.sets,
        stood: side.stood,
    }
}

/// The match as its player's screens show it (`assets` the campaign's art
/// and words), or as a watcher's do: the table whatever the match's stage
/// (the wager and the side deck are the player's alone).
fn view(
    table: &Match,
    assets: &mut ui::assets::GuiAssets,
    watching: Option<PazaakWatch>,
) -> PazaakView {
    let art = match assets.game {
        vfs::Game::Kotor2 => &TSL_ART,
        _ => &K1_ART,
    };
    let mut maximum = assets.text(Source::Campaign, MAXIMUM).unwrap_or(Vec::new());
    maximum.extend_from_slice(format!(" {}", table.most).as_bytes());
    viewed(table, art, maximum, watching)
}

fn viewed(table: &Match, art: &Art, maximum: Vec<u8>, watching: Option<PazaakWatch>) -> PazaakView {
    // A result: won by the player (none: a tie), and whether the match's.
    let (stage, turn, result) = match table.phase {
        Phase::Wager => (PazaakStage::Wager, None, None),
        Phase::Setup => (PazaakStage::Setup, None, None),
        Phase::Play => (PazaakStage::Game, Some(usize::from(table.turn)), None),
        Phase::SetOver(outcome) => {
            let won = match outcome {
                Outcome::Player => Some(true),
                Outcome::Opponent => Some(false),
                Outcome::Tie => None,
            };
            (PazaakStage::Game, None, Some((won, false)))
        }
        Phase::Over { won } => (PazaakStage::Game, None, Some((Some(won), true))),
    };
    // The game's lines speak to the player; a watcher is told whose.
    let told = result.map(|(won, whole)| match &watching {
        Some(watch) => PazaakTold::Said(ui::strings::pazaak_result(&watch.name, won, whole)),
        None => PazaakTold::Line(match (won, whole) {
            (None, _) => SET_TIED,
            (Some(true), false) => SET_WON,
            (Some(false), false) => SET_LOST,
            (Some(true), true) => MATCH_WON,
            (Some(false), true) => MATCH_LOST,
        }),
    });
    let watching = watching.map(|watch| PazaakWatch {
        stage,
        held: held(table),
        ..watch
    });
    PazaakView {
        stage: if watching.is_some() {
            PazaakStage::Game
        } else {
            stage
        },
        watching,
        most: table.most,
        maximum,
        offers: table
            .available
            .iter()
            .map(|&(card, count)| PazaakOffer {
                card: card.variation(),
                count,
                look: look(card, None, art),
            })
            .collect(),
        sides: [
            side(&table.sides[sim::pazaak::PLAYER], true, art),
            side(&table.sides[1 - sim::pazaak::PLAYER], false, art),
        ],
        turn,
        told,
        mark: art.mark.into(),
    }
}

/// The table `before` shows watched, as `player` (none: not watching) is
/// now in snapshot `tick`: the name asked of `name` for a new table, and
/// asked again while it is not known (the scene not yet there), once a
/// snapshot rather than every frame. Whether the table changed.
fn watched_name(
    before: &mut Option<Watched>,
    (player, tick): (Option<u32>, u64),
    name: impl FnOnce(u32) -> Vec<u8>,
) -> bool {
    let changed = before.as_ref().map(|watched| watched.player) != player;
    let unnamed =
        (before.as_ref()).is_some_and(|watched| watched.name.is_empty() && watched.asked != tick);
    if changed || unnamed {
        *before = player.map(|player| Watched {
            player,
            name: name(player),
            asked: tick,
        });
    }
    changed
}

/// The table `seat`'s screens show of `at` (`table_of`): none while its
/// player's match is paused (R9: a cutscene or a fight plays on its
/// player's screen); a watcher keeps it, told why, its Leave live (R39).
fn shown(at: Option<(&Match, bool)>) -> Option<&Match> {
    at.filter(|(table, watching)| *watching || table.paused.is_none())
        .map(|(table, _)| table)
}

/// Why `table` is paused, as its watchers are told (R39): its player's
/// seat dropped, or its player hurt and still in a fight.
fn held(table: &Match) -> Option<PazaakHeld> {
    table.paused?;
    if table.away.is_some() {
        Some(PazaakHeld::Away)
    } else {
        table.hurt.then_some(PazaakHeld::Hurt)
    }
}

/// The screens put away: each stage's, and a result or tutorial pages up.
fn put_away(ui: &mut Ui, seated: &mut Seated) {
    if seated.stage.is_none() {
        return;
    }
    for (_, name) in SCREENS {
        ui.stack.close(name);
    }
    if seated.told.is_some() || seated.paged {
        ui.stack.close(MESSAGE);
    }
    ui.vm.pazaak = None;
    (seated.stage, seated.told, seated.paged) = (None, None, false);
}

fn show(
    presented: Option<Res<PresentedSnapshot>>,
    own: Option<Res<OwnSeat>>,
    (swap, map): (Option<Res<SceneSwap>>, Option<Res<KeyMap>>),
    mut ui: ResMut<Ui>,
    mut seated: ResMut<Seated>,
) {
    let (Some(presented), Some(own)) = (presented, own) else {
        return;
    };
    let ui = &mut *ui;
    let snapshot = presented.0.current();
    let at = table_of(snapshot, own.0);
    // Another table than the one shown (watching it now, or playing): the
    // screens start over, its tutorial not yet shown.
    let watched = at
        .filter(|(_, watching)| *watching)
        .map(|(table, _)| table.player);
    let name = |player| {
        swap.as_deref().map_or(Vec::new(), |swap| {
            let objects = sim::Instances::in_snapshot(&swap.level, snapshot);
            crate::dialogue_view::name_of(snapshot, &objects, ObjectId(player))
        })
    };
    if watched_name(&mut seated.watched, (watched, snapshot.tick), name) {
        put_away(ui, &mut seated);
        seated.tutored.clear();
    }
    // A table not shown is put away, its result and tutorial pages with it
    // (those count as shown).
    let Some(table) = shown(at) else {
        put_away(ui, &mut seated);
        if at.is_none() {
            seated.tutored.clear();
        }
        return;
    };
    let Some(assets) = ui.assets.as_mut() else {
        return;
    };
    let watching = seated.watched.as_ref().map(|watched| PazaakWatch {
        name: watched.name.clone(),
        leave_key: map
            .as_deref()
            .unwrap_or(KeyMap::defaults())
            .shown(KeyAction::LeaveTalk),
        ..PazaakWatch::default()
    });
    let plays = watching.is_none();
    let shown = view(table, assets, watching);
    let (stage, told) = (shown.stage, shown.told.clone());
    ui.vm.pazaak = Some(shown);
    if seated.stage != Some(stage) {
        for (each, name) in SCREENS {
            ui.stack.sync(name, each == stage, &ui.vm);
        }
        // The tutorial is its player's.
        let pages = TUTORIAL
            .iter()
            .filter(|_| plays && table.tutorial && !seated.tutored.contains(&stage))
            .find(|(at, _)| *at == stage)
            .map_or(&[][..], |(_, pages)| *pages);
        // The first page on top.
        for page in pages.iter().rev() {
            ui.stack.open(&format!("message_box:{page}"), &ui.vm);
        }
        seated.paged |= !pages.is_empty();
        seated.tutored.push(stage);
        seated.stage = Some(stage);
    }
    // A result shows while the match holds it (the next set deals itself).
    if seated.told != told {
        if seated.told.is_some() {
            ui.stack.close(MESSAGE);
        }
        match &told {
            Some(PazaakTold::Line(strref)) => {
                ui.stack.open(&format!("message_box:{strref}"), &ui.vm);
            }
            Some(PazaakTold::Said(words)) => {
                ui.stack
                    .push(Box::new(ui::screens::message::Message::said(words.clone())));
            }
            None => {}
        }
        seated.told = told;
    }
}

/// The sounds of what changed at the table shown since it was last seen
/// (`sounded`), and its player's credits as the match ends. A table first
/// seen (a join, a watch begun) sounds nothing.
fn heard(
    presented: Option<Res<PresentedSnapshot>>,
    own: Option<Res<OwnSeat>>,
    (cues, told): (
        Option<ResMut<crate::sound::Cues>>,
        Option<ResMut<crate::progress::Told>>,
    ),
    mut last: Local<Option<(Match, bool)>>,
) {
    let (Some(presented), Some(own)) = (presented, own) else {
        return;
    };
    let at = table_of(presented.0.current(), own.0);
    let now = shown(at).map(|table| (table.clone(), at.is_some_and(|(_, watching)| watching)));
    let before = std::mem::replace(&mut *last, now.clone());
    let (Some((before, _)), Some((now, watching))) = (before, now) else {
        return;
    };
    if before.player != now.player {
        return;
    }
    if let Some(mut cues) = cues {
        for sound in sounded(&before, &now) {
            cues.0.push(sounds::cue(&presented.0, sound));
        }
    }
    let ended = match (before.phase, now.phase) {
        (Phase::Over { .. }, _) => None,
        (_, Phase::Over { won }) => Some(won),
        _ => None,
    };
    if let (Some(won), Some(mut told), false) = (ended, told, watching)
        && now.wager > 0
    {
        told.statuses
            .push(sim::StatusKind::Credits(now.settled(won)));
    }
}

/// K1's pazaak sounds for the table going from `before` to `now`: each card
/// laid (a new set's tables start over), the player's turn coming, and a
/// set's or the match's end.
fn sounded(before: &Match, now: &Match) -> Vec<&'static str> {
    let mut out = Vec::new();
    for (was, is) in before.sides.iter().zip(&now.sides) {
        let kept = if is.table.len() < was.table.len() {
            0
        } else {
            was.table.len()
        };
        out.extend(is.table[kept..].iter().map(|laid| match laid.card {
            Some(_) => sounds::PAZAAK_SIDE,
            None => sounds::PAZAAK_DRAW,
        }));
    }
    let players_turn = |table: &Match| {
        table.phase == Phase::Play && usize::from(table.turn) == sim::pazaak::PLAYER
    };
    if players_turn(now) && !players_turn(before) {
        out.push(sounds::PAZAAK_TURN);
    }
    if before.phase != now.phase {
        match now.phase {
            Phase::SetOver(Outcome::Player) => out.push(sounds::PAZAAK_SET_WON),
            Phase::SetOver(Outcome::Opponent) => out.push(sounds::PAZAAK_SET_LOST),
            Phase::Over { won: true } => out.push(sounds::PAZAAK_MATCH_WON),
            Phase::Over { won: false } => out.push(sounds::PAZAAK_MATCH_LOST),
            _ => {}
        }
    }
    out
}

/// Watching's asks (R9a): the Leave key while this player watches (it is
/// taken from play), the console's `ui leave`, and a click on a HUD pazaak
/// icon, as `asked` sends them.
fn watch(
    (keys, map): (Option<Res<ButtonInput<KeyCode>>>, Option<Res<KeyMap>>),
    (presented, own): (Option<Res<PresentedSnapshot>>, Option<Res<OwnSeat>>),
    mut asks: ResMut<WatchAsked>,
    outgoing: Option<ResMut<Outgoing>>,
    mut swallowed: ResMut<crate::swallow::Swallowed>,
) {
    let WatchAsked { icon, leave } = std::mem::take(&mut *asks);
    let (Some(presented), Some(own), Some(mut outgoing)) = (presented, own, outgoing) else {
        return;
    };
    let snapshot = presented.0.current();
    let key = map
        .as_deref()
        .unwrap_or(KeyMap::defaults())
        .key(KeyAction::LeaveTalk);
    let pressed = keys
        .as_deref()
        .filter(|keys| keys.just_pressed(key) && snapshot.watching(own.0).is_some());
    if let Some(keys) = pressed {
        swallowed.key(key, keys);
    }
    if let Some(action) = asked(snapshot, own.0, icon, leave || pressed.is_some()) {
        outgoing.0.push(action);
    }
}

/// The launcher's side of watching (R9a; `kjk sim probes --only "pazaak
/// view"`; the approved test `pazaak_watch_view`, `--features rig`), on the
/// arena: the second seat drives a companion following the host, which
/// plays a match at its turn, the host watching it.
/// - Each seat's screens show that match: the host's as a watcher.
/// - Whatever the watcher clicks or keys on the table sends no move; its
///   Leave sends `Action::Watch` with no player; the player's End Turn its
///   move.
/// - The Leave key stops the watcher's watching and does nothing for the
///   player; the host's click on the companion's pazaak icon watches its
///   match.
/// - A result as the player is told it (the game's line) and as a watcher
///   is ("Carth won the set."); a watched name not yet known asked again,
///   once a snapshot.
/// - R39: the match paused, its player's seat dropped: the watcher keeps
///   the table, told why (away, or hurt), and its Leave stops watching;
///   the player's own table is put away.
#[cfg(feature = "probes")]
pub fn probes() -> sim::probe::Checks {
    use sim::probe::check;
    use ui::intent::Intent;
    use ui::screen::Screen;
    let config = sim::WorldConfig {
        seats: 2,
        ..sim::WorldConfig::one_seat()
    };
    let mut world = match sim::SimWorld::new(config, sim::Level::arena()) {
        Ok(world) => world,
        Err(error) => return vec![("pazaak view arena", Err(error.to_string()))],
    };
    let mut snapshot = world.snapshot();
    let (Some(host), Some(card)) = (snapshot.body(Seat(0)).copied(), Card::of(1)) else {
        return vec![("pazaak view arena", Err("no host, or no ±1 card".into()))];
    };
    let mut companion = sim::EntityState {
        id: sim::SimId(u32::MAX - 1),
        kind: sim::Kind::Object(sim::module::ObjectKind::Creature, 0),
        ..host
    };
    if let Some(fighter) = companion.fighter.as_mut() {
        (fighter.mind.leader, fighter.mind.driver) = (Some(Seat(0)), Some(Seat(1)));
    }
    snapshot.entities.push(companion);
    let mut table = Match {
        player: companion.id.0,
        phase: Phase::Play,
        turn: sim::pazaak::PLAYER as u8,
        watchers: vec![host.id.0],
        ..Match::new(
            [card; sim::pazaak::SIDE_DECK],
            vec![(card, 10)],
            (0, 0),
            false,
            (String::new(), companion.id.0),
        )
    };
    table.sides[sim::pazaak::PLAYER].hand = [Some(card); 4];
    snapshot.pazaak.push(table.clone());
    let seen = [Seat(0), Seat(1)]
        .map(|seat| table_of(&snapshot, seat).map(|(table, watching)| (table.player, watching)));
    let carth = |watching: bool| {
        watching.then(|| PazaakWatch {
            name: b"Carth".to_vec(),
            leave_key: b"L".to_vec(),
            ..PazaakWatch::default()
        })
    };
    let vm = |watching: bool| ui::vm::ViewModel {
        pazaak: Some(viewed(&table, &K1_ART, Vec::new(), carth(watching))),
        ..ui::vm::ViewModel::default()
    };
    // A set won, a tie, the match lost: as the player is told them, and a
    // watcher.
    let results = [
        Phase::SetOver(Outcome::Player),
        Phase::SetOver(Outcome::Tie),
        Phase::Over { won: false },
    ];
    let told = |watching: bool| {
        results.map(|phase| {
            let over = Match {
                phase,
                ..table.clone()
            };
            viewed(&over, &K1_ART, Vec::new(), carth(watching)).told
        })
    };
    let said = |words: &str| Some(PazaakTold::Said(words.as_bytes().to_vec()));
    let wanted_told = (
        [SET_WON, SET_TIED, MATCH_LOST].map(|line| Some(PazaakTold::Line(line))),
        [
            said("Carth won the set."),
            said("The set is tied."),
            said("Carth lost the match."),
        ],
    );
    let told = (told(false), told(true));
    // A watched table's name, asked again while not known, once a snapshot:
    // not known in the first, not asked again in it, known in the next, then
    // kept, and the table gone.
    let mut watched = None;
    let named = [
        (Some(7), 1, ""),
        (Some(7), 1, "Carth"),
        (Some(7), 2, "Carth"),
        (Some(7), 3, "Bastila"),
        (None, 3, "Bastila"),
    ]
    .map(|(player, tick, name)| {
        let changed = watched_name(&mut watched, (player, tick), |_| name.into());
        let now = watched
            .as_ref()
            .map(|w: &Watched| (w.player, w.name.clone()));
        (changed, now)
    });
    let known = Some((7, b"Carth".to_vec()));
    let unknown = Some((7, Vec::new()));
    let wanted_named = [
        (true, unknown.clone()),
        (false, unknown),
        (false, known.clone()),
        (false, known),
        (true, None),
    ];
    // R39: the player's seat dropped, its match paused: the watcher keeps
    // the table, told why (away; hurt), and its Leave stops it; the
    // player's own is put away.
    let paused = |away: bool, hurt: bool| Match {
        paused: Some(1),
        away: away.then_some(1),
        hurt,
        ..table.clone()
    };
    let mut dropped = snapshot.clone();
    dropped.pazaak = vec![paused(true, false)];
    let kept = [Seat(0), Seat(1)].map(|seat| shown(table_of(&dropped, seat)).map(|t| t.player));
    let why = [paused(true, false), paused(false, true)].map(|table| {
        viewed(&table, &K1_ART, Vec::new(), carth(true))
            .watching
            .and_then(|w| w.held)
    });
    let paused_leave = asked(&dropped, Seat(0), None, true);
    // What a screen's presses and keys send to the host.
    let sent = |vm: &ui::vm::ViewModel, tags: &[&str], keys: &[&str]| -> Vec<Action> {
        let (mut game, mut out) = (ui::screens::pazaak::Game::default(), Vec::new());
        for tag in tags {
            game.press(tag, vm, &mut out);
        }
        for key in keys {
            game.raw(key, vm, &mut out);
        }
        out.into_iter()
            .filter_map(|intent| match intent {
                Intent::Game(game) => crate::loot::action(game),
                _ => None,
            })
            .collect()
    };
    let tags = [
        "BTN_PLRSIDE0",
        "BTN_PLRSIDE3",
        "BTN_FLIP0",
        "BTN_CHANGE0",
        "BTN_XTEXT",
        "BTN_YTEXT",
    ];
    let keys = ["Digit1", "Digit4", "F1", "KeyE", "KeyS"];
    let stop = Action::Watch { player: u32::MAX };
    let clicks = (
        sent(&vm(true), &tags, &keys),
        sent(&vm(true), &["BTN_FORFEITGAME"], &[]),
        sent(&vm(false), &["BTN_XTEXT"], &[]),
    );
    let asks = (
        asked(&snapshot, Seat(0), None, true),
        asked(&snapshot, Seat(1), None, true),
        asked(&snapshot, Seat(0), Some(companion.id.0), false),
    );
    vec![
        (
            "watcher_told_third_person",
            check(
                told == wanted_told,
                format!("a set won, a tie, the match lost: the player's, the watcher's {told:?}"),
            ),
        ),
        (
            "watched_name_asked_again",
            check(
                named == wanted_named,
                format!(
                    "unknown, the same snapshot, the next, kept, gone (changed, kept): {named:?}"
                ),
            ),
        ),
        (
            "paused_watcher_keeps_table",
            check(
                kept == [Some(companion.id.0), None]
                    && why == [Some(PazaakHeld::Away), Some(PazaakHeld::Hurt)]
                    && paused_leave == Some(stop.clone()),
                format!(
                    "paused, shown the host watching and the guest playing {kept:?}, told {why:?}, the watcher's Leave {paused_leave:?}"
                ),
            ),
        ),
        (
            "watcher_sees_table",
            check(
                seen == [Some((companion.id.0, true)), Some((companion.id.0, false))],
                format!("the host's and the guest's (player, watching): {seen:?}"),
            ),
        ),
        (
            "watcher_clicks_send_no_move",
            check(
                clicks
                    == (
                        Vec::new(),
                        vec![stop.clone()],
                        vec![Action::Pazaak(Move::EndTurn)],
                    ),
                format!(
                    "the watcher's clicks and keys, its Leave, the player's End Turn: {clicks:?}"
                ),
            ),
        ),
        (
            "leave_key_and_icon_click",
            check(
                asks == (
                    Some(stop),
                    None,
                    Some(Action::Watch {
                        player: companion.id.0,
                    }),
                ),
                format!("the Leave key watching, then playing; the host's icon click: {asks:?}"),
            ),
        ),
    ]
}
