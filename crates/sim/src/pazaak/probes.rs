//! Pazaak's probes (`kjk sim probes`, the approved test `pazaak`): the
//! cards as the install writes them, the rules on tables set by hand, the
//! opponent's play, the codecs, a match a script starts played to its end
//! script on the proving ground, twice alike, and two players' matches at
//! once, paused and quit (R9).

use fmt_odyssey::ncs::{Const, Op, Pair};

use super::{Card, Decks, Kind, Match, Move, OPPONENT_MS, Outcome, PLAYER, Phase, SET_MS};
use crate::probe::{Checks, check};
use crate::rng::SimRng;
use crate::script::{MODULE, Pending, State};
use crate::wire::{Reader, Writer};
use crate::{Seat, SimWorld, Snapshot, StepReason, TickInput, WorldConfig};

const MSEC: u32 = 8;
const OPPONENT: usize = 1;

pub fn probes() -> Checks {
    vec![
        ("pazaak_cards", cards()),
        ("pazaak_owned", owned()),
        ("pazaak_wager", wager()),
        ("pazaak_deck_offered", deck_offered()),
        ("pazaak_bust", bust()),
        ("pazaak_bust_rescued", rescued()),
        ("pazaak_advice_wins", advice_wins()),
        ("pazaak_twenty_stands", twenty_stands()),
        ("pazaak_nine_cards_stand", nine_stand()),
        ("pazaak_tie_replayed", tie_replayed()),
        ("pazaak_three_sets", three_sets()),
        ("pazaak_opponent", opponent()),
        ("pazaak_round_trip", round_trip()),
        (
            "pazaak_scripted",
            scripted(1, false).map(|(detail, _)| detail).and_then(|r| r),
        ),
        (
            "pazaak_wager_held",
            scripted(1, true).map(|(detail, _)| detail).and_then(|r| r),
        ),
        ("pazaak_twice_alike", twice_alike()),
        ("pazaak_two_matches", two_matches()),
        ("pazaak_paused_resumes", paused_resumes()),
        ("pazaak_quit_on_module_change", quit_on_module_change()),
        ("pazaak_quit_on_exit", quit_on_exit()),
        ("pazaak_decided_settles", decided_settles()),
        ("pazaak_wager_sum_held", wager_sum_held()),
    ]
}

fn card(code: &str) -> Card {
    Card::parse(code).unwrap_or_else(|| panic!("{code} is a card"))
}

/// A set under way on the player's turn: each side's table and hand
/// (`pazaakdecks.2da` codes), the main deck drawn from its end.
fn table(player: (&[i8], &[&str]), opponent: (&[i8], &[&str]), main: &[u8]) -> Match {
    let deck = [card("+1"); super::SIDE_DECK];
    let mut table = Match::new(deck, Vec::new(), (0, 0), false, (String::new(), 0));
    for (side, (laid, hand)) in table.sides.iter_mut().zip([player, opponent]) {
        side.table = laid
            .iter()
            .map(|value| super::Laid {
                value: *value,
                card: None,
            })
            .collect();
        side.hand = std::array::from_fn(|at| hand.get(at).map(|code| card(code)));
    }
    table.main = main.to_vec();
    table.phase = Phase::Play;
    table
}

/// Every `pazaakdecks.2da` code both games write, as the card item of that
/// `ModelVariation` is named; faces with and without a sign chosen.
fn cards() -> Result<String, String> {
    let codes = [
        ("+1", 7),
        ("+6", 12),
        ("-1", 13),
        ("-6", 18),
        ("*1", 1),
        ("*6", 6),
        ("VV", 19),
        ("$$", 20),
        ("TT", 21),
        ("F1", 22),
        ("F2", 23),
    ];
    let parsed: Vec<_> = codes
        .iter()
        .map(|(code, _)| Card::parse(code).map(Card::variation))
        .collect();
    let wanted: Vec<_> = codes.iter().map(|(_, v)| Some(*v)).collect();
    let refused = ["+0", "+7", "*", "", "XX", "=3"]
        .iter()
        .all(|code| Card::parse(code).is_none());
    let faces = [
        card("*3").face(None),
        card("*3").face(Some((true, false))),
        card("-4").face(None),
        card("VV").face(Some((true, true))),
        card("F1").face(None),
    ];
    let kinds = card("-4").kind()
        == Kind::Value {
            value: -4,
            either: false,
        }
        && card("F2").kind() == Kind::Flip([3, 6])
        && card("*2").chosen()
        && !card("+2").chosen();
    check(
        parsed == wanted && refused && kinds && faces == ["\u{b1}3", "-3", "-4", "-2", "2&4"],
        format!("variations {parsed:?}, faces {faces:?}, odd codes refused {refused}"),
    )
}

/// What a party may choose from: the basic deck (row 0) and the card items
/// it holds, by stack; other items add nothing.
fn owned() -> Result<String, String> {
    let decks = Decks {
        rows: vec![
            [card("+1"); super::SIDE_DECK],
            [card("-2"); super::SIDE_DECK],
        ],
        cards: [("g_i_pazcard_013".to_string(), card("*1"))].into(),
    };
    let owned = decks.owned([("g_i_pazcard_013", 2), ("g_w_blstrpstl01", 1)]);
    let rows = (decks.row(1), decks.row(9), decks.row(-1));
    check(
        owned == [(card("*1"), 2), (card("+1"), 10)]
            && rows.0 == Some([card("-2"); super::SIDE_DECK])
            && rows.1 == decks.row(0)
            && rows.2 == decks.row(0),
        format!("owned {owned:?}"),
    )
}

/// The wager: within 1 and the most (the call's and the credits'); a call
/// with none starts at the side deck; Quit leaves with nothing settled.
fn wager() -> Result<String, String> {
    let deck = [card("+1"); super::SIDE_DECK];
    let new = |max, gold| Match::new(deck, Vec::new(), (max, gold), false, (String::new(), 0));
    let mut rng = SimRng::new(1, 2);
    let mut asked = new(50, 30);
    let most = asked.most;
    for wager in [31, 0, -5] {
        asked.apply(Move::Wager(wager), 0, &mut rng);
    }
    let refused = asked.phase == Phase::Wager;
    asked.apply(Move::Wager(30), 0, &mut rng);
    let taken = (asked.phase, asked.wager);
    let free = new(0, 500).phase;
    let mut quit = new(50, 30);
    quit.apply(Move::Quit, 7, &mut rng);
    let left = quit.advance(7, &mut rng);
    check(
        most == 30
            && refused
            && taken == (Phase::Setup, 30)
            && free == Phase::Setup
            && left == Some(false)
            && quit.settled(false) == 0,
        format!(
            "most {most}, refused {refused}, taken {taken:?}, no wager {free:?}, quit {left:?}"
        ),
    )
}

/// The side deck: ten of the cards offered, no more of one than held; then
/// each side's hand is four of its side deck, and the first set is dealt.
fn deck_offered() -> Result<String, String> {
    let deck = [card("-2"); super::SIDE_DECK];
    let offered = vec![(card("+1"), 8), (card("*3"), 2), (card("-5"), 1)];
    let mut table = Match::new(deck, offered, (0, 0), false, (String::new(), 0));
    let mut rng = SimRng::new(3, 4);
    let mut chosen = [card("+1"); super::SIDE_DECK];
    chosen[0] = card("+6");
    table.apply(Move::Deck(chosen), 0, &mut rng);
    let foreign = table.phase;
    chosen[0] = card("-5");
    chosen[1] = card("-5");
    table.apply(Move::Deck(chosen), 0, &mut rng);
    let too_many = table.phase;
    chosen[1] = card("*3");
    chosen[2] = card("*3");
    table.apply(Move::Deck(chosen), 0, &mut rng);
    let hands_ok = table.sides[PLAYER]
        .hand
        .iter()
        .all(|card| card.is_some_and(|card| chosen.contains(&card)))
        && table.sides[OPPONENT].hand == [Some(card("-2")); 4];
    let dealt = table.sides.iter().map(|s| s.table.len()).sum::<usize>();
    check(
        foreign == Phase::Setup
            && too_many == Phase::Setup
            && table.phase == Phase::Play
            && hands_ok
            && dealt >= 1
            && table.main.len() + dealt == 40,
        format!(
            "foreign {foreign:?}, too many {too_many:?}, then {:?}, hands {:?}, dealt {dealt}",
            table.phase, table.sides[PLAYER].hand
        ),
    )
}

/// Over 20 at the end of the turn loses the set.
fn bust() -> Result<String, String> {
    let mut table = table((&[10, 9, 5], &[]), (&[7], &[]), &[1, 2, 3]);
    table.apply(Move::EndTurn, 0, &mut SimRng::new(0, 0));
    check(
        table.phase == Phase::SetOver(Outcome::Opponent) && table.sides[OPPONENT].sets == 1,
        format!(
            "{:?}, opponent's sets {}",
            table.phase, table.sides[OPPONENT].sets
        ),
    )
}

/// A negative hand card brings a draw back: 24 less 4 is 20, which stands
/// the player at once; the opponent's turn begins.
fn rescued() -> Result<String, String> {
    let mut table = table((&[10, 9, 5], &["-4", "+2"]), (&[7], &[]), &[1, 2, 3]);
    let mut rng = SimRng::new(0, 0);
    table.apply(
        Move::Play {
            slot: 0,
            minus: false,
            two: false,
        },
        0,
        &mut rng,
    );
    let me = &table.sides[PLAYER];
    check(
        table.phase == Phase::Play
            && me.total() == 20
            && me.stood
            && me.hand[0].is_none()
            && table.turn == OPPONENT as u8
            && table.sides[OPPONENT].table.len() == 2,
        format!(
            "{:?}, total {}, stood {}, turn {}",
            table.phase,
            me.total(),
            me.stood,
            table.turn
        ),
    )
}

/// A draw to exactly 20 stands the player before it may act.
fn twenty_stands() -> Result<String, String> {
    // The opponent draws the 2 (12: it ends its turn), the player the 5.
    let mut table = table((&[10, 5], &[]), (&[10], &[]), &[9, 9, 5, 2]);
    let mut rng = SimRng::new(0, 0);
    table.apply(Move::EndTurn, 0, &mut rng);
    table.advance(OPPONENT_MS, &mut rng);
    let me = &table.sides[PLAYER];
    check(
        me.total() == 20 && me.stood && table.turn == OPPONENT as u8,
        format!(
            "total {}, stood {}, turn {}",
            me.total(),
            me.stood,
            table.turn
        ),
    )
}

/// A ninth card in play stands the player, whatever its total.
fn nine_stand() -> Result<String, String> {
    let mut table = table((&[1; 7], &["+1"]), (&[10], &[]), &[9, 9, 1, 2]);
    let mut rng = SimRng::new(0, 0);
    table.apply(
        Move::Play {
            slot: 0,
            minus: false,
            two: false,
        },
        0,
        &mut rng,
    );
    let eight = table.sides[PLAYER].stood;
    table.apply(Move::EndTurn, 0, &mut rng);
    table.advance(OPPONENT_MS, &mut rng);
    let me = &table.sides[PLAYER];
    check(
        !eight && me.table.len() == 9 && me.total() == 9 && me.stood,
        format!(
            "after eight stood {eight}; nine: {} cards, total {}, stood {}",
            me.table.len(),
            me.total(),
            me.stood
        ),
    )
}

/// Both standing on one total: a tie, no set to either, the set replayed
/// by the side that began it; a Tie Breaker played wins it instead.
fn tie_replayed() -> Result<String, String> {
    let mut rng = SimRng::new(5, 6);
    let mut tied = table((&[10, 9], &[]), (&[10, 9], &[]), &[]);
    tied.sides[OPPONENT].stood = true;
    tied.began = OPPONENT as u8;
    tied.apply(Move::Stand, 0, &mut rng);
    let phase = tied.phase;
    tied.advance(SET_MS, &mut rng);
    let replay = (tied.phase, tied.turn, tied.sides.map(|s| s.sets));
    let mut broken = table((&[10, 8], &["TT"]), (&[10, 9], &[]), &[]);
    broken.sides[OPPONENT].stood = true;
    broken.apply(
        Move::Play {
            slot: 0,
            minus: false,
            two: false,
        },
        0,
        &mut rng,
    );
    broken.apply(Move::Stand, 0, &mut rng);
    check(
        phase == Phase::SetOver(Outcome::Tie)
            && replay == (Phase::Play, OPPONENT as u8, [0, 0])
            && broken.phase == Phase::SetOver(Outcome::Player),
        format!(
            "{phase:?}, then {replay:?}; with a tie breaker {:?}",
            broken.phase
        ),
    )
}

/// The third set won ends the match: won, its wager the party's.
fn three_sets() -> Result<String, String> {
    let mut table = table((&[10, 10], &[]), (&[10, 8], &[]), &[]);
    table.sides[PLAYER].sets = 2;
    table.sides[OPPONENT].stood = true;
    table.wager = 40;
    let mut rng = SimRng::new(0, 0);
    table.apply(Move::Stand, 100, &mut rng);
    let early = table.advance(100, &mut rng);
    let end = table.advance(100 + super::END_MS, &mut rng);
    check(
        table.phase == Phase::Over { won: true }
            && early.is_none()
            && end == Some(true)
            && table.settled(true) == 40
            && table.settled(false) == -40,
        format!("{:?}, at once {early:?}, after {end:?}", table.phase),
    )
}

/// The opponent's play: a bust rescued; a standing player passed with a
/// hand card; 20 reached with one; standing from 18; drawing below.
fn opponent() -> Result<String, String> {
    let mut rng = SimRng::new(0, 0);
    let mut turn = |opponent: (&[i8], &[&str]), player: &[i8], stood: bool| {
        let mut table = table((player, &[]), opponent, &[]);
        table.sides[PLAYER].stood = stood;
        table.turn = OPPONENT as u8;
        table.advance(0, &mut rng);
        let side = &table.sides[OPPONENT];
        (side.total(), side.stood, side.played, table.phase)
    };
    let rescue = turn((&[10, 9, 4], &["+1", "-3", "*2"]), &[10], false);
    let past = turn((&[10, 6], &["+3", "*1"]), &[10, 8], true);
    let twenty = turn((&[10, 7], &["+3"]), &[10], false);
    let stands = turn((&[10, 8], &["+3"]), &[10], false);
    let draws = turn((&[10, 4], &["+1"]), &[10], false);
    check(
        rescue == (20, true, true, Phase::Play)
            && past == (19, true, true, Phase::SetOver(Outcome::Opponent))
            && twenty == (20, true, true, Phase::Play)
            && stands == (18, true, false, Phase::Play)
            && !draws.1
            && !draws.2,
        format!("rescue {rescue:?}, past {past:?}, to 20 {twenty:?}, 18 {stands:?}, 14 {draws:?}"),
    )
}

/// A match mid-set and watched, a move and an action through their bytes
/// and back; as many watchers as seats, or one twice, refused.
fn round_trip() -> Result<String, String> {
    use crate::action::Action;
    let mut table = table((&[10, 3], &["*4", "VV"]), (&[2, -1], &["TT"]), &[4, 4, 1]);
    table.sides[PLAYER].sets = 2;
    table.sides[OPPONENT].table[1].card = Some(card("-1"));
    table.available = vec![(card("+2"), 3), (card("F2"), 1)];
    table.end_script = "k_ptar_nikpazend".into();
    table.phase = Phase::SetOver(Outcome::Tie);
    (table.paused, table.away) = (Some(1234), Some(99));
    table.watchers = vec![7, 3];
    let encoded = |table: &Match| {
        let mut out = Writer::default();
        super::encode_tables(std::slice::from_ref(table), &mut out);
        out.0
    };
    let bytes = encoded(&table);
    let back =
        super::decode_tables(&mut Reader::new("pazaak", &bytes)).map_err(|e| e.to_string())?;
    // As many watchers as seats (the player's own seat watches nothing), or
    // one body twice.
    let refused = [(1..=u32::from(crate::MAX_SEATS)).collect(), vec![7, 3, 7]].map(|watchers| {
        let wrong = Match {
            watchers,
            ..table.clone()
        };
        super::decode_tables(&mut Reader::new("pazaak", &encoded(&wrong))).is_err()
    });
    let refused = refused == [true; 2];
    let watches = [table.player, u32::MAX].map(|player| Action::Watch { player });
    let moves = [
        Move::Wager(12),
        Move::Quit,
        Move::Deck([card("*1"); super::SIDE_DECK]),
        Move::Play {
            slot: 3,
            minus: true,
            two: true,
        },
        Move::EndTurn,
        Move::Stand,
        Move::Forfeit,
    ];
    let actions_back = (moves.iter().map(|played| Action::Pazaak(*played)))
        .chain(watches)
        .all(|action| Action::from_bytes(&action.to_bytes()).as_ref() == Ok(&action));
    check(
        back == [table] && actions_back && refused,
        format!(
            "{} bytes, moves and watches back {actions_back}, too many or twice refused {refused}",
            bytes.len()
        ),
    )
}

/// The probe's scripts: `start` calls `PlayPazaak(1, "done", 50)`; `done`
/// writes `GetLastPazaakResult() + 1` to the global `RESULT`, `done2` to
/// `RESULT2`; `leave` asks for another module (`StartNewModule`).
fn library() -> crate::script::Library {
    const CATALOG: &str = r#"
// 0
void PlayPazaak(int nOpponentPazaakDeck, string sEndScript, int nMaxWager, int bShowTutorial=FALSE, object oOpponent=OBJECT_INVALID);
// 1
int GetLastPazaakResult();
// 2
void SetGlobalNumber(string sIdentifier, int nValue);
// 3
void StartNewModule(string sModuleName, string sWayPoint="", string sMovie1="", string sMovie2="", string sMovie3="", string sMovie4="", string sMovie5="", string sMovie6="");
"#;
    let mut start = script_ncs::probes::Asm::default();
    start
        .op(Op::Const(Const::Object(1)))
        .op(Op::Const(Const::Int(0)))
        .op(Op::Const(Const::Int(50)))
        .op(Op::Const(Const::String(b"done".to_vec())))
        .op(Op::Const(Const::Int(1)))
        .op(Op::Action {
            routine: 0,
            args: 5,
        })
        .op(Op::Retn);
    let done = |global: &[u8]| {
        let mut done = script_ncs::probes::Asm::default();
        done.op(Op::Action {
            routine: 1,
            args: 0,
        })
        .op(Op::Const(Const::Int(1)))
        .op(Op::Add(Pair::IntInt))
        .op(Op::Const(Const::String(global.to_vec())))
        .op(Op::Action {
            routine: 2,
            args: 2,
        })
        .op(Op::Retn);
        done.build()
    };
    let mut leave = script_ncs::probes::Asm::default();
    for _ in 0..7 {
        leave.op(Op::Const(Const::String(Vec::new())));
    }
    leave
        .op(Op::Const(Const::String(b"elsewhere".to_vec())))
        .op(Op::Action {
            routine: 3,
            args: 8,
        })
        .op(Op::Retn);
    crate::probe::library(
        CATALOG,
        [
            ("start", start.build()),
            ("done", done(b"RESULT")),
            ("done2", done(b"RESULT2")),
            ("leave", leave.build()),
        ],
    )
    .with_pazaak(decks())
}

/// The scripted match's decks: two opponents', and a ±1 card item.
pub(crate) fn decks() -> Decks {
    Decks {
        rows: vec![
            [
                card("+1"),
                card("+1"),
                card("+2"),
                card("+2"),
                card("+3"),
                card("+3"),
                card("+4"),
                card("+4"),
                card("+5"),
                card("+5"),
            ],
            [
                card("+3"),
                card("-3"),
                card("+4"),
                card("-4"),
                card("+5"),
                card("-5"),
                card("+5"),
                card("-3"),
                card("+4"),
                card("-5"),
            ],
        ],
        cards: [("g_i_pazcard_013".to_string(), card("*1"))].into(),
    }
}

/// A match the probe's `start` script opens on the proving ground (seed
/// `seed`), the party with 100 credits and two ±1 cards, played by the
/// first seat as `Match::advice` says until the end script has written
/// its result: the phases it passed, the wager of 50 held out of the
/// credits while it is played, the credits settled by the result, the
/// snapshot's table gone with the scripts', the notice sent. When `spent`,
/// the party's credits are spent once the match is under way: the wager
/// was held, so a loss leaves nothing and a win the wager and its winnings.
/// Also the last snapshot's hash.
fn scripted(seed: u64, spent: bool) -> Result<(Result<String, String>, String), String> {
    let mut ground = Ground::new(seed, 1, 100)?;
    ground.edit(|state| state.pending.push(Pending::new("start", MODULE)));
    let (mut phases, mut told, mut offered) = (Vec::new(), false, false);
    let mut held = None;
    for _ in 0..PLAYED_OUT {
        ground.advised();
        told |= ground.world.take_notices().iter().any(|(_, notice)| {
            *notice == crate::notice::Notice::Show(crate::notice::Screen::Pazaak)
        });
        if let Some(table) = ground.last.pazaak_of(Seat(0)) {
            offered |= table.available.contains(&(card("*1"), 2));
            let phase = std::mem::discriminant(&table.phase);
            if phases.last() != Some(&phase) {
                phases.push(phase);
            }
            if table.phase == Phase::Play && held.is_none() {
                held = Some(ground.world.script_state().gold);
                if spent {
                    ground.edit(|state| state.gold = 0);
                }
            }
        }
        if ground.world.script_state().numbers.contains_key("RESULT") {
            break;
        }
    }
    let (state, last) = (ground.world.script_state(), &ground.last);
    let result = state.numbers.get("RESULT").copied();
    let won = result == Some(2);
    let gold = state.gold;
    let detail = format!(
        "result {result:?}, gold {held:?} in play then {gold}, {} phase changes, told {told}, offered {offered}, ended at {:.1}s",
        phases.len(),
        last.time_ms as f64 / 1000.0
    );
    let left = if spent { 0 } else { 50 };
    Ok((
        check(
            result.is_some()
                && held == Some(50)
                && gold == if won { left + 100 } else { left }
                && state.pazaak.is_empty()
                && last.pazaak.is_empty()
                && told
                && offered
                && phases.len() >= 4,
            detail,
        ),
        last.hash().short(),
    ))
}

/// The scripted match twice on one seed: the same end, byte for byte.
fn twice_alike() -> Result<String, String> {
    let (first, again) = (scripted(9, false)?, scripted(9, false)?);
    check(
        first.1 == again.1 && first.0.is_ok(),
        format!("{} and {}", first.1, again.1),
    )
}

/// The steps a scripted match is played out in, at most.
const PLAYED_OUT: usize = 40_000;

/// The proving ground with the probe's scripts and decks, each seat a
/// player of its own, the party with its credits and two ±1 cards; its
/// last snapshot.
struct Ground {
    world: SimWorld,
    tick: u64,
    last: Snapshot,
    seats: u8,
}

impl Ground {
    fn new(seed: u64, seats: u8, gold: i32) -> Result<Ground, String> {
        let level = crate::determinism::proving_ground().with_scripts(library());
        let config = WorldConfig {
            seed,
            seats,
            ..WorldConfig::one_seat()
        };
        let mut world = SimWorld::new(config, level).map_err(|why| why.to_string())?;
        let last = world.snapshot();
        let mut ground = Ground {
            tick: last.tick,
            world,
            last,
            seats,
        };
        ground.edit(|state| {
            state.gold = gold;
            state.items.insert(
                crate::inventory::FIRST_ITEM + 900,
                crate::inventory::Item {
                    template: "g_i_pazcard_013".into(),
                    holder: crate::inventory::PARTY,
                    slot: None,
                    stack: 2,
                    charges: 0,
                    droppable: false,
                    plot: false,
                    identified: true,
                    infinite: false,
                    non_equippable: false,
                    upgrades: 0,
                    fitted: Vec::new(),
                },
            );
        });
        Ok(ground)
    }

    /// The body seat `seat` plays.
    fn body(&self, seat: u8) -> Result<u32, String> {
        self.last
            .body(Seat(seat))
            .map(|body| body.id.0)
            .ok_or(format!("seat {seat} has no body"))
    }

    /// The scripts' state changed as `change` has it, between steps.
    fn edit(&mut self, change: impl FnOnce(&mut State)) {
        let mut state = self.world.script_state().clone();
        change(&mut state);
        self.world.set_script_state(state);
    }

    /// `PlayPazaak` against deck `row` for `player`, a wager of 50 at
    /// most, its end script `end`.
    fn deal(&mut self, player: u32, row: i32, end: &str) {
        crate::script::deal_pazaak(&mut self.world, (player, MODULE.0), row, end, 50);
    }

    /// One step, each seat making its move in `moves`.
    fn step(&mut self, moves: &[(Seat, Move)]) {
        let mut input = TickInput::default();
        for (seat, played) in moves {
            input.act(*seat, crate::action::Action::Pazaak(*played));
        }
        self.tick += 1;
        self.last = crate::step(
            &mut self.world,
            self.tick,
            &input,
            MSEC,
            StepReason::Authority,
        );
    }

    /// One step, each seat playing the match it plays as `Match::advice`
    /// says.
    fn advised(&mut self) {
        let moves: Vec<(Seat, Move)> = (0..self.seats)
            .map(Seat)
            .filter_map(|seat| Some((seat, self.last.pazaak_of(seat)?.advice()?)))
            .collect();
        self.step(&moves);
    }

    /// The match `player` plays, in the scripts' state.
    fn table(&self, player: u32) -> Option<&Match> {
        let tables = &self.world.script_state().pazaak;
        tables.iter().find(|table| table.player == player)
    }
}

/// R9: two players play at once, each a match of their own. The second
/// seat's match is dealt; then `start` deals the first seat's (its body is
/// the first PC), and again, refused while that body plays one. Each plays
/// as `Match::advice` says until both end scripts have written their
/// results: each its own match's, both wagers of 50 held at once, the
/// credits settled by both results.
fn two_matches() -> Result<String, String> {
    let mut ground = Ground::new(3, 2, 100)?;
    let bodies = [ground.body(0)?, ground.body(1)?];
    ground.deal(bodies[1], 0, "done2");
    ground.edit(|state| {
        state.pending.push(Pending::new("start", MODULE));
        state.pending.push(Pending::new("start", MODULE));
    });
    ground.step(&[]);
    let dealt: Vec<u32> = ground
        .world
        .script_state()
        .pazaak
        .iter()
        .map(|t| t.player)
        .collect();
    let refused = ground
        .world
        .script_report()
        .trace
        .iter()
        .any(|line| line.contains("no pazaak"));
    let (mut won, mut together, mut both_held) = ([None; 2], false, false);
    for _ in 0..PLAYED_OUT {
        ground.advised();
        let tables: Vec<&Match> = bodies
            .iter()
            .filter_map(|body| ground.table(*body))
            .collect();
        together |= tables.len() == 2 && tables.iter().all(|table| table.phase == Phase::Play);
        both_held |= tables.len() == 2 && tables.iter().all(|table| table.wager == 50);
        for (at, body) in bodies.iter().enumerate() {
            if let Some(Phase::Over { won: end }) = ground.table(*body).map(|table| table.phase) {
                won[at] = Some(end);
            }
        }
        let numbers = &ground.world.script_state().numbers;
        if numbers.contains_key("RESULT") && numbers.contains_key("RESULT2") {
            break;
        }
    }
    let state = ground.world.script_state();
    let results = ["RESULT", "RESULT2"].map(|global| state.numbers.get(global).copied());
    let wanted = won.map(|end| end.map(|end| i32::from(end) + 1));
    let wins = won.iter().filter(|end| **end == Some(true)).count() as i32;
    check(
        dealt == [bodies[1], bodies[0]]
            && refused
            && together
            && both_held
            && results == wanted
            && state.gold == 100 * wins
            && state.pazaak.is_empty(),
        format!(
            "dealt to {dealt:?} of {bodies:?}, again refused {refused}, played together {together}, held {both_held}; results {results:?} for {won:?}, gold {}",
            state.gold
        ),
    )
}

/// R9: a match paused 5 s on the opponent's turn: it takes no move and its
/// opponent does not act; resumed, it plays on as it would have, as much
/// later as it waited.
fn paused_resumes() -> Result<String, String> {
    const PAUSE: u64 = 5000;
    let mut playing = table((&[10], &[]), (&[5], &["+1"]), &[2, 3, 4]);
    playing.turn = OPPONENT as u8;
    playing.due_ms = OPPONENT_MS;
    let mut paused = playing.clone();
    let mut rng = SimRng::new(0, 0);
    paused.hold(true, 100);
    paused.apply(Move::Forfeit, 200, &mut rng);
    let still = (OPPONENT_MS..=PAUSE + 100)
        .step_by(100)
        .all(|now| paused.advance(now, &mut rng).is_none())
        && paused.sides == playing.sides
        && paused.phase == Phase::Play;
    paused.hold(false, PAUSE + 100);
    paused.advance(OPPONENT_MS + PAUSE - 1, &mut rng);
    let early = paused.sides == playing.sides;
    playing.advance(OPPONENT_MS, &mut rng);
    paused.advance(OPPONENT_MS + PAUSE, &mut rng);
    let same = paused.sides == playing.sides
        && paused.turn == playing.turn
        && paused.phase == playing.phase
        && paused.due_ms == playing.due_ms + PAUSE
        && paused.paused.is_none();
    check(
        still && early && same,
        format!(
            "held still {still}, not before its time {early}, then as unpaused {same} ({:?}, turn {}, due {})",
            paused.phase, paused.turn, paused.due_ms
        ),
    )
}

/// One seat's match the probe's `start` deals on seed `seed`, played as
/// `Match::advice` says until a set is under way: its wager of 50 held of
/// the party's 100 credits.
fn under_way(seed: u64) -> Result<Ground, String> {
    let mut ground = Ground::new(seed, 1, 100)?;
    ground.edit(|state| state.pending.push(Pending::new("start", MODULE)));
    for _ in 0..PLAYED_OUT {
        ground.advised();
        if ground
            .last
            .pazaak_of(Seat(0))
            .is_some_and(|table| table.phase == Phase::Play)
        {
            return Ok(ground);
        }
    }
    Err("no set began".into())
}

/// The match left as a module goes (R9, R11): none in the scripts' state
/// or the snapshot, the wager back (100 credits), `GetLastPazaakResult` 0
/// (set to 1 before), its end script not run then or after. `ended` says
/// whether it was gone at once.
fn left_quit(mut ground: Ground, ended: bool) -> Result<String, String> {
    for _ in 0..steps(super::END_MS * 2) {
        ground.advised();
    }
    let state = ground.world.script_state();
    let result = state.numbers.get("RESULT").copied();
    check(
        ended
            && state.pazaak.is_empty()
            && state.gold == 100
            && !state.pazaak_won
            && result.is_none(),
        format!(
            "ended {ended}, gold {} after, last result {}, end script's {result:?}",
            state.gold, state.pazaak_won
        ),
    )
}

/// R9, R11: a script asks for another module mid-match. The match ends as a
/// quit (`left_quit`).
fn quit_on_module_change() -> Result<String, String> {
    let mut ground = under_way(5)?;
    ground.edit(|state| {
        state.pazaak_won = true;
        state.pending.push(Pending::new("leave", MODULE));
    });
    ground.advised();
    let ended = ground.world.script_state().pazaak.is_empty() && ground.last.pazaak.is_empty();
    left_quit(ground, ended)
}

/// R9, R11: the party goes through an exit mid-match (a forced one leaves
/// the host to install the next module, `SimWorld::install`, no script
/// asking): the match ends as a quit all the same (`left_quit`).
fn quit_on_exit() -> Result<String, String> {
    let mut ground = under_way(5)?;
    ground.edit(|state| state.pazaak_won = true);
    let level = ground.world.level().clone();
    ground.last = ground
        .world
        .install(level, None)
        .map_err(|why| why.to_string())?;
    let ended = ground.world.script_state().pazaak.is_empty() && ground.last.pazaak.is_empty();
    left_quit(ground, ended)
}

/// A match decided while its end shows settles as decided: a quit asked
/// of it (a death, a drop) leaves it won, and a module change mid-end pays
/// the win (its wager back and as much again: 150 credits of the 100), its
/// result read 1, its end script still not run.
fn decided_settles() -> Result<String, String> {
    let mut ground = under_way(5)?;
    let now = ground.last.time_ms;
    let decide = |table: &mut Match| {
        table.phase = Phase::Over { won: true };
        table.due_ms = now + super::END_MS;
    };
    let mut quit = ground.world.script_state().pazaak[0].clone();
    decide(&mut quit);
    quit.quit(now);
    let kept = quit.phase == Phase::Over { won: true } && quit.wager == 50;
    ground.edit(|state| {
        state.pazaak.iter_mut().for_each(decide);
        state.pending.push(Pending::new("leave", MODULE));
    });
    ground.advised();
    let state = ground.world.script_state();
    let result = state.numbers.get("RESULT").copied();
    check(
        kept && state.pazaak.is_empty()
            && state.gold == 150
            && state.pazaak_won
            && result.is_none(),
        format!(
            "a quit kept it won {kept}; the module left: gold {}, last result {}, end script's {result:?}",
            state.gold, state.pazaak_won
        ),
    )
}

/// R9: each wager is held on its own, decided in seat order. With 60
/// credits, both seats wager 50 in one step: the first seat's match takes
/// it (though dealt second), the second's is refused (10 left), then takes
/// 10. Both quit: both back.
fn wager_sum_held() -> Result<String, String> {
    let mut ground = Ground::new(7, 2, 60)?;
    let bodies = [ground.body(0)?, ground.body(1)?];
    ground.deal(bodies[1], 1, "");
    ground.deal(bodies[0], 0, "");
    let wager = |amount| {
        [
            (Seat(0), Move::Wager(amount)),
            (Seat(1), Move::Wager(amount)),
        ]
    };
    ground.step(&wager(50));
    let wagers = |ground: &Ground| bodies.map(|body| ground.table(body).map(|table| table.wager));
    let first = (wagers(&ground), ground.world.script_state().gold);
    ground.step(&[(Seat(1), Move::Wager(10))]);
    let second = (wagers(&ground), ground.world.script_state().gold);
    ground.step(&[(Seat(0), Move::Quit), (Seat(1), Move::Quit)]);
    let back = (wagers(&ground), ground.world.script_state().gold);
    check(
        first == ([Some(50), Some(0)], 10)
            && second == ([Some(50), Some(10)], 0)
            && back == ([None, None], 60),
        format!("both 50: {first:?}; the second 10: {second:?}; both quit: {back:?}"),
    )
}

/// How many probe steps last `ms`.
fn steps(ms: u64) -> usize {
    (ms / u64::from(MSEC)) as usize
}

/// The share of `games` matches (seeds 0 on) the headless player
/// (`Match::advice`) wins against `opponent`, choosing its side deck from
/// `available`, no wager.
fn win_rate(opponent: [&str; super::SIDE_DECK], available: &[(&str, u8)], games: u64) -> f32 {
    let opponent = opponent.map(card);
    let available: Vec<(Card, u8)> = available.iter().map(|(code, n)| (card(code), *n)).collect();
    let won = (0..games)
        .filter(|seed| {
            let mut rng = SimRng::new(*seed, 7);
            let mut game = Match::new(
                opponent,
                available.clone(),
                (0, 0),
                false,
                (String::new(), 0),
            );
            let mut now = 0;
            loop {
                if let Some(played) = game.advice() {
                    game.apply(played, now, &mut rng);
                }
                if let Some(won) = game.advance(now, &mut rng) {
                    break won;
                }
                now += 100;
            }
        })
        .count();
    won as f32 / games as f32
}

/// The headless player plays well enough to win Pato Ado's games as a
/// player would (`Match::advice`): with the default + cards (row 0) and his
/// store's (+3, ±1, ±6, Flip 2&4), it wins about half its matches against
/// his first stake's deck (`pazaakdecks.2da` row 1, 50% of 2000) and a
/// third against his armor game's, all ± (row 3, 35%), where playing the
/// opponent's own rule with a flip and +1s in its deck won 44% and 29%.
fn advice_wins() -> Result<String, String> {
    let mine = [
        ("+1", 2),
        ("+2", 2),
        ("+3", 3),
        ("+4", 2),
        ("+5", 2),
        ("*1", 1),
        ("*6", 1),
        ("F1", 1),
    ];
    let average = win_rate(
        ["+3", "-3", "+4", "-4", "+5", "-5", "+5", "-3", "+4", "-5"],
        &mine,
        2000,
    );
    let all_either = win_rate(
        ["*3", "*3", "*2", "*2", "*4", "*4", "*2", "*2", "*1", "*1"],
        &mine,
        2000,
    );
    check(
        average >= 0.48 && all_either >= 0.33,
        format!(
            "won {average:.3} against his first stake's deck, {all_either:.3} against his armor game's"
        ),
    )
}
