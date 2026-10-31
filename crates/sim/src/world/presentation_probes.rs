//! The probes of the presentation and flow routines (`kjk sim probes
//! presentation`; `script::presentation`), on the proving ground, no
//! install, with the executables' numbers:
//! - the time conversions (a round 3 s, an hour the IFO's 2 minutes, a turn
//!   ten rounds), `AngleToVector`, `GetGameDifficulty` (Normal),
//!   `IsMoviePlaying` (never);
//! - the movie queue: played in order as one run, each with its own skip,
//!   emptied; kept across a module change until played; TSL's ending
//!   counting the game complete;
//! - `NoClicksFor`: placeables and talk refused until its game time is up,
//!   doors not;
//! - barks: `ActionBarkString` queued behind a wait; `BarkString` and it
//!   dropped while a conversation holds the world;
//! - a conversation ending its parties' stealth, unless
//!   `SetKeepStealthInDialog`;
//! - `DisplayFeedBackText`'s `feedbacktext.2da` line over its creature;
//! - `ShowTutorialWindow`: once, not in a conversation, kept in a save;
//! - `DisableMap`, `SetDisableTransit`, `SetReturnStrref` in the snapshot,
//!   the map kept in a save;
//! - bodies: `ChangeObjectAppearance` (and refused under a disguise),
//!   `DuplicateHeadAppearance`, `PlayOverlayAnimation`;
//! - `YavinHackDoorClose`, `SoundObjectSetPosition`,
//!   `SoundObjectGetPitchVariance`, `DisplayDatapad`.

use fmt_odyssey::ncs::{Const, Ncs, Op};
use script_ncs::probes::Asm;
use script_ncs::vm::ObjectId;

use super::{Avatar, Fighting, Object, SimWorld};
use crate::action::{Action, Verb};
use crate::input::{Seat, TickInput};
use crate::module::{ObjectData, ObjectKind, Placement};
use crate::notice::{Notice, Screen};
use crate::probe::{Checks, check};
use crate::script::{Pending, Presentation};
use crate::snapshot::Snapshot;
use crate::{Level, StepReason};

const MSEC: u32 = 8;

const CATALOG: &str = r#"
// 0
void SetCustomToken(int nCustomTokenNumber, string sTokenValue);
// 1
string IntToString(int nInteger);
// 2
string FloatToString(float fFloat, int nWidth=18, int nDecimals=9);
// 3
float RoundsToSeconds(int nRounds);
// 4
float HoursToSeconds(int nHours);
// 5
float TurnsToSeconds(int nTurns);
// 6
vector AngleToVector(float fAngle);
// 7
int GetGameDifficulty();
// 8
int IsMoviePlaying();
// 9
void QueueMovie(string sMovie, int nSkippable = TRUE);
// 10
void PlayMovieQueue(int nAllowSkips = TRUE);
// 11
void NoClicksFor(float fDuration);
// 12
void ActionWait(float fSeconds);
// 13
void ActionBarkString(int strRef);
// 14
void BarkString(object oCreature, int strRef);
// 15
object GetFirstPC();
// 16
void DisplayFeedBackText(object oCreature, int nTextConstant);
// 17
void ShowTutorialWindow(int nWindow);
// 18
void DisableMap(int nFlag = FALSE);
// 19
void SetDisableTransit(int nFlag = FALSE);
// 20
void SetReturnStrref(int bShow, int srStringRef = 0, int srReturnQueryStrRef = 0);
// 21
void ChangeObjectAppearance(object oObjectToChange, int nAppearance);
// 22
int GetAppearanceType(object oCreature);
// 23
void DuplicateHeadAppearance(object oidCreatureToChange, object oidCreatureToMatch);
// 24
void PlayOverlayAnimation(object oTarget, int nAnimation);
// 25
void YavinHackDoorClose(object oCreature);
// 26
object GetObjectByTag(string sTag, int nNth = 0);
// 27
void SoundObjectSetPosition(object oSound, vector vPosition);
// 28
float SoundObjectGetPitchVariance(object oSound);
// 29
void DisplayDatapad(object oDatapad);
// 30
void SetKeepStealthInDialog(int nStealthState);
// 31
int BeginConversation(string sResRef = "", object oObjectToDialog = OBJECT_INVALID);
// 32
int GetLocalBoolean(object oObject, int nIndex);
// 33
int SetGlobalNumber(string sIdentifier, int nValue);
// 34
int GetGlobalNumber(string sIdentifier);
"#;

/// The talk-table lines and rows the probes use: Kreia's `ActionBarkString`
/// (101PER's `k_kreia_ud`), TSL's `feedbacktext.2da` (Resisted, Immune,
/// Saved), TSL's Kreia hint (`tutorial.2da` row 51).
const KREIA_BARK: i32 = 49465;
const FEEDBACK: [u32; 3] = [36847, 36848, 36849];
const IMMUNE: i32 = 1;
const KREIA_HINT: u8 = 51;
/// An appearance a script gives (Kreia's final form, 904MAL), and the head
/// table's row the talker's appearance (row 1) names.
const KREIA_FINAL: i32 = 651;
const TALKER_HEAD: i16 = 12;
/// The sound object's UTS pitch variation, the talker's tag.
const PITCH: f32 = 0.25;
const SOUND: &str = "remote_cry";
const TALKER: &str = "talker";
/// The proving ground's door and the objects added after its trigger.
const GATE: u32 = 0;
const PANEL: u32 = 2;
const TALKER_INDEX: u32 = 3;

fn call(routine: u16, args: u8) -> Op {
    Op::Action { routine, args }
}

fn int(value: i32) -> Op {
    Op::Const(Const::Int(value))
}

fn float(value: f32) -> Op {
    Op::Const(Const::Float(value))
}

fn text(value: &str) -> Op {
    Op::Const(Const::String(value.as_bytes().to_vec()))
}

const SELF: Op = Op::Const(Const::Object(0));

/// Custom token `n` set to what `value` leaves, an int or (with
/// `decimals`) a float, as a string.
fn token(asm: &mut Asm, n: i32, value: &[Op], decimals: Option<i32>) {
    if let Some(decimals) = decimals {
        asm.op(int(decimals)).op(int(18));
    }
    for op in value {
        asm.op(op.clone());
    }
    match decimals {
        Some(_) => asm.op(call(2, 3)),
        None => asm.op(call(1, 1)),
    };
    asm.op(int(n)).op(call(0, 2));
}

/// A script of `ops` then a return.
fn script(ops: &[Op]) -> Ncs {
    let mut asm = Asm::default();
    for op in ops {
        asm.op(op.clone());
    }
    asm.op(Op::Retn);
    asm.build()
}

/// `probe_numbers`: tokens 1 to 7, `RoundsToSeconds(10)`,
/// `HoursToSeconds(1)`, `TurnsToSeconds(1)`, `AngleToVector(90)`'s x and y,
/// `GetGameDifficulty`, `IsMoviePlaying`.
fn numbers() -> Ncs {
    let mut asm = Asm::default();
    token(&mut asm, 1, &[int(10), call(3, 1)], Some(1));
    token(&mut asm, 2, &[int(1), call(4, 1)], Some(1));
    token(&mut asm, 3, &[int(1), call(5, 1)], Some(1));
    for (n, keep) in [(4, 0), (5, 4)] {
        let component = [
            float(90.0),
            call(6, 1),
            Op::Destruct {
                size: 12,
                offset: keep,
                keep: 4,
            },
        ];
        token(&mut asm, n, &component, Some(2));
    }
    token(&mut asm, 6, &[call(7, 0)], None);
    token(&mut asm, 7, &[call(8, 0)], None);
    asm.op(Op::Retn);
    asm.build()
}

/// The probes' library: each script by name, the TSL tables they read,
/// and a conversation to start.
fn library() -> crate::script::Library {
    let pc = || call(15, 0);
    let sound = || [int(0), text(SOUND), call(26, 2)];
    let talker = || [int(0), text(TALKER), call(26, 2)];
    let scripts: Vec<(&str, Ncs)> = vec![
        ("probe_numbers", numbers()),
        (
            "probe_queue",
            script(&[
                int(1),
                text("TelMov02"),
                call(9, 2),
                int(0),
                text("HypMov01"),
                call(9, 2),
            ]),
        ),
        ("probe_play", script(&[int(0), call(10, 1)])),
        (
            "probe_ending",
            script(&[int(1), text("MalMov12"), call(9, 2)]),
        ),
        ("probe_no_clicks", script(&[float(1.0), call(11, 1)])),
        (
            "probe_queued_bark",
            script(&[float(2.0), call(12, 1), int(KREIA_BARK), call(13, 1)]),
        ),
        ("probe_bark", script(&[int(KREIA_BARK), pc(), call(14, 2)])),
        ("probe_feedback", script(&[int(IMMUNE), pc(), call(16, 2)])),
        (
            "probe_tutorial",
            script(&[int(i32::from(KREIA_HINT)), call(17, 1)]),
        ),
        ("probe_jam", script(&[int(1), call(18, 1)])),
        ("probe_unjam", script(&[int(0), call(18, 1)])),
        (
            "probe_transit",
            script(&[
                int(1),
                call(19, 1),
                int(42466),
                int(38477),
                int(1),
                call(20, 3),
            ]),
        ),
        (
            "probe_appearance",
            script(&[int(KREIA_FINAL), pc(), call(21, 2)]),
        ),
        // The player takes the talker's head.
        ("probe_head", {
            let mut ops = talker().to_vec();
            ops.extend([pc(), call(23, 2)]);
            script(&ops)
        }),
        ("probe_overlay", script(&[int(123), pc(), call(24, 2)])),
        (
            "probe_close_door",
            script(&[int(0), text("gate"), call(26, 2), call(25, 1)]),
        ),
        ("probe_sound", {
            let mut ops = vec![float(1.0), float(2.0), float(3.0)];
            ops.extend(sound());
            ops.push(call(27, 2));
            let mut asm = Asm::default();
            for op in &ops {
                asm.op(op.clone());
            }
            token(
                &mut asm,
                8,
                &[
                    sound()[0].clone(),
                    sound()[1].clone(),
                    sound()[2].clone(),
                    call(28, 1),
                ],
                Some(2),
            );
            asm.op(Op::Retn);
            asm.build()
        }),
        ("probe_datapad", script(&[SELF, call(29, 1)])),
        ("probe_keep_stealth", script(&[int(1), call(30, 1)])),
        (
            "probe_talk",
            script(&[pc(), text("probe_dlg"), call(31, 2), Op::MovSp(-4)]),
        ),
    ];
    let dialogues = [(
        "probe_dlg".to_string(),
        crate::script::dialogue_probes::talking(2, ""),
    )]
    .into_iter()
    .collect();
    let mut tutorials = vec![false; 74];
    tutorials[usize::from(KREIA_HINT)] = true;
    let mut heads = vec![-1; 700];
    heads[1] = TALKER_HEAD;
    crate::probe::library(CATALOG, scripts)
        .with_dialogues(dialogues, Default::default())
        .with_presentation(Presentation {
            tutorials,
            feedback: FEEDBACK.to_vec(),
            heads,
        })
}

/// The proving ground with a panel, a creature that talks and a sound
/// object added, and the probes' library; its player stealthy (ranks and
/// a stealth unit), at the panel.
fn world() -> Result<(SimWorld, Level), String> {
    let ground = crate::determinism::proving_ground();
    let mut data = (**ground.module().ok_or("the ground has no module")?).clone();
    let at = |x, y| crate::determinism::ground_point(x, y, 0.0);
    let thing = |kind, tag: &str, place| ObjectData {
        kind,
        tag: tag.into(),
        useable: true,
        ..ObjectData::unknown(tag, place, 0.0)
    };
    data.placements.extend(
        [
            thing(ObjectKind::Placeable, "panel", at(-1.2, -1.0)),
            ObjectData {
                body: Some(crate::module::Body {
                    appearance: 1,
                    size: 3,
                    movement: 0,
                    walk_mps: 1.0,
                    run_mps: 3.0,
                    head_arc_h: 0.0,
                    bag: 0,
                    destroy_ms: 0,
                    fade_ms: None,
                    bulk: None,
                }),
                scripts: vec![("ScriptDialogue".into(), "probe_numbers".into())],
                ..thing(ObjectKind::Creature, TALKER, at(-1.6, 1.0))
            },
            ObjectData {
                pitch_variation: PITCH,
                ..thing(ObjectKind::Sound, SOUND, at(-3.0, 0.0))
            },
        ]
        .map(Placement::Own),
    );
    let level = ground.with_module(data).with_scripts(library());
    let mut sheet = crate::combat::Sheet::dev_soldier();
    sheet.flags |= crate::combat::sheet::flags::STEALTH_UNIT;
    sheet.skills[crate::combat::sheet::skills::STEALTH] = 4;
    let world = SimWorld::new(
        crate::WorldConfig {
            seed: 1,
            seats: 1,
            wanderers: 0,
            spawn: [0.0; 3],
            yaw: 0.0,
            sheet,
            guests: crate::Guests::Avatars,
        },
        level.clone(),
    )
    .map_err(|error| error.to_string())?;
    let mut world = world;
    stand(&mut world, [-1.2, 0.0]);
    Ok((world, level))
}

/// Seat 0's player put at `[x, y]` on the ground's own axes.
fn stand(world: &mut SimWorld, [x, y]: [f32; 2]) {
    let at = crate::determinism::ground_point(x, y, 0.0);
    let mut players = world.world.query::<(&Avatar, &mut super::Player)>();
    for (_, mut player) in players.iter_mut(&mut world.world) {
        player.0.origin[0] = at[0] / collision::UNIT;
        player.0.origin[1] = at[1] / collision::UNIT;
    }
}

/// A step with seat 0's `actions`: the snapshot and the notices.
fn step_with(world: &mut SimWorld, actions: &[Action]) -> (Snapshot, Vec<Notice>) {
    let mut input = TickInput::default();
    input.insert(Seat(0), crate::input::Command::default());
    for action in actions {
        input.act(Seat(0), action.clone());
    }
    let tick = world.tick() + 1;
    let snapshot = crate::step(world, tick, &input, MSEC, StepReason::Authority);
    let notices = world
        .take_notices()
        .into_iter()
        .map(|(_, notice)| notice)
        .collect();
    (snapshot, notices)
}

fn step(world: &mut SimWorld) -> (Snapshot, Vec<Notice>) {
    step_with(world, &[])
}

/// `script` run for `caller` in the next step: that step's snapshot and
/// notices.
fn run_for(world: &mut SimWorld, script: &str, caller: ObjectId) -> (Snapshot, Vec<Notice>) {
    let mut state = world.script_state().clone();
    state.pending.push(Pending::new(script, caller));
    world.set_script_state(state);
    step(world)
}

fn run(world: &mut SimWorld, script: &str) -> (Snapshot, Vec<Notice>) {
    run_for(world, script, crate::script::MODULE)
}

fn tokens(snapshot: &Snapshot, numbers: std::ops::RangeInclusive<i32>) -> Vec<String> {
    numbers
        .map(|n| {
            let told = snapshot.talk.tokens.get(&n);
            told.map_or_else(|| "(unset)".into(), |t| t.trim().to_string())
        })
        .collect()
}

/// The player's entity.
fn player(snapshot: &Snapshot) -> Option<&crate::EntityState> {
    snapshot.body(Seat(0))
}

/// The talker's entity.
fn talker_of(snapshot: &Snapshot) -> Option<&crate::EntityState> {
    (snapshot.entities.iter())
        .find(|e| matches!(e.kind, crate::Kind::Object(ObjectKind::Creature, _)))
}

/// The movies `notices` show: resref, skippable, run.
fn movies(notices: &[Notice]) -> Vec<(String, bool, u16)> {
    notices
        .iter()
        .filter_map(|notice| match notice {
            Notice::Show(screen @ Screen::Movie { skippable, run, .. }) => {
                Some((screen.movie_name()?, *skippable, *run))
            }
            _ => None,
        })
        .collect()
}

/// The state round-tripped through a save's encoding.
fn saved(state: &crate::script::State) -> Result<crate::script::State, String> {
    let mut out = crate::wire::Writer::default();
    state.encode(&mut out);
    let mut input = crate::wire::Reader::new("state", &out.0);
    crate::script::State::decode(&mut input).map_err(|error| error.to_string())
}

pub fn probes() -> Checks {
    vec![
        ("time_and_angles", numbers_check()),
        ("movie_queue", queue_check()),
        ("movie_queue_kept", kept_queue_check()),
        ("game_complete", ending_check()),
        ("no_clicks", no_clicks_check()),
        ("queued_bark", queued_bark_check()),
        ("barks_in_conversation", conversation_barks_check()),
        ("conversation_ends_stealth", stealth_check(false)),
        ("keep_stealth_in_dialog", stealth_check(true)),
        ("feedback_text", feedback_check()),
        ("tutorial_window", tutorial_check()),
        ("map_and_transit", map_check()),
        ("bodies", bodies_check()),
        ("appearance_refused_disguised", disguised_check()),
        ("door_sound_datapad", objects_check()),
    ]
}

fn numbers_check() -> Result<String, String> {
    let (mut world, _) = world()?;
    step(&mut world);
    let (after, _) = run(&mut world, "probe_numbers");
    let mut seen = tokens(&after, 1..=7);
    // cos 90° comes out a hair either side of 0.
    if seen[3] == "-0.00" {
        seen[3] = "0.00".into();
    }
    let want = ["30.0", "120.0", "30.0", "0.00", "1.00", "1", "0"];
    check(
        seen == want,
        format!("rounds, hours, turns, x, y, difficulty, playing: {seen:?}"),
    )
}

fn queue_check() -> Result<String, String> {
    let (mut world, _) = world()?;
    step(&mut world);
    let (_, queued) = run(&mut world, "probe_queue");
    let held = world.script_state().movies.len();
    let (_, played) = run(&mut world, "probe_play");
    let shown = movies(&played);
    let run = shown.first().map_or(0, |(_, _, run)| *run);
    let want = vec![
        ("telmov02".to_string(), true, run),
        ("hypmov01".to_string(), false, run),
    ];
    check(
        movies(&queued).is_empty()
            && held == 2
            && run != 0
            && shown == want
            && world.script_state().movies.is_empty(),
        format!("held {held} till played; played {shown:?}"),
    )
}

/// Queued in one module, played in the next.
fn kept_queue_check() -> Result<String, String> {
    let (mut world, level) = world()?;
    step(&mut world);
    run(&mut world, "probe_queue");
    let mut elsewhere = (**level.module().ok_or("no module")?).clone();
    elsewhere.name = "elsewhere".into();
    let other = level.clone().with_module(elsewhere);
    world
        .install(other, None)
        .map_err(|error| error.to_string())?;
    step(&mut world);
    let (_, played) = run(&mut world, "probe_play");
    let shown: Vec<String> = movies(&played).into_iter().map(|(name, ..)| name).collect();
    check(
        shown == ["telmov02", "hypmov01"],
        format!("played in the next module: {shown:?}"),
    )
}

fn ending_check() -> Result<String, String> {
    let (mut world, _) = world()?;
    step(&mut world);
    run(&mut world, "probe_ending");
    let count = world
        .script_state()
        .numbers
        .get("GBL_GAME_COMPLETE")
        .copied();
    check(count == Some(1), format!("GBL_GAME_COMPLETE {count:?}"))
}

/// What a Use of `object` came to, `ms` after `NoClicksFor(1.0)`.
fn no_clicks_check() -> Result<String, String> {
    let (mut world, _) = world()?;
    step(&mut world);
    let (called, _) = run(&mut world, "probe_no_clicks");
    let mut now = called.time_ms;
    let mut outcomes = Vec::new();
    for (after_ms, object, verb) in [
        (500, PANEL, Verb::Use),
        (500, TALKER_INDEX, Verb::Talk),
        (500, GATE, Verb::Open),
        (1100, PANEL, Verb::Use),
    ] {
        while now + u64::from(MSEC) < called.time_ms + after_ms {
            now = step(&mut world).0.time_ms;
        }
        now = step_with(&mut world, &[Action::Use { object, verb }])
            .0
            .time_ms;
        outcomes.push(!world.uses_heard().is_empty());
    }
    check(
        outcomes == [false, false, true, true],
        format!("panel, talk at 0.5 s, door at 0.5 s, panel at 1.1 s: {outcomes:?}"),
    )
}

/// Kreia's bark waits for her wait: none at once, one after 2 s.
fn queued_bark_check() -> Result<String, String> {
    let (mut world, _) = world()?;
    step(&mut world);
    let pc = player(&step(&mut world).0)
        .map(|e| ObjectId(e.id.0))
        .ok_or("no player")?;
    run_for(&mut world, "probe_queued_bark", pc);
    let barked = |world: &SimWorld| !world.script_state().talk.barks.is_empty();
    let at_once = barked(&world);
    for _ in 0..(2100 / MSEC) {
        step(&mut world);
    }
    let later = barked(&world);
    check(
        !at_once && later,
        format!("at once {at_once}, after 2 s {later}"),
    )
}

/// Neither bark while a conversation holds the world.
fn conversation_barks_check() -> Result<String, String> {
    let (mut world, _) = world()?;
    step(&mut world);
    let pc = player(&step(&mut world).0)
        .map(|e| ObjectId(e.id.0))
        .ok_or("no player")?;
    let mut state = world.script_state().clone();
    state
        .talk
        .conversations
        .push(crate::script::dialogue::Conversation {
            party: true,
            ..Default::default()
        });
    state
        .pending
        .push(Pending::new("probe_bark", crate::script::MODULE));
    state.pending.push(Pending::new("probe_queued_bark", pc));
    world.set_script_state(state);
    let mut barks = 0;
    for _ in 0..(2100 / MSEC) {
        step(&mut world);
        barks += world.script_state().talk.barks.len();
    }
    check(barks == 0, format!("barks seen {barks}"))
}

/// A stealthy player talked to: out of stealth, unless kept.
fn stealth_check(keep: bool) -> Result<String, String> {
    let (mut world, _) = world()?;
    step(&mut world);
    let mut bodies = world.world.query::<(&Avatar, &mut Fighting)>();
    for (_, mut fighting) in bodies.iter_mut(&mut world.world) {
        fighting.0.covert.stealth = true;
    }
    if keep {
        run(&mut world, "probe_keep_stealth");
    }
    // The talker starts a conversation with the player.
    let (before, _) = step(&mut world);
    let talker = talker_of(&before)
        .map(|e| ObjectId(e.id.0))
        .ok_or("no talker")?;
    run_for(&mut world, "probe_talk", talker);
    let (after, _) = step(&mut world);
    let talking = !world.script_state().talk.conversations.is_empty();
    let stealth = player(&after)
        .and_then(|e| e.fighter)
        .is_some_and(|f| f.covert.stealth);
    check(
        talking && stealth == keep,
        format!("talking {talking}, still in stealth {stealth}"),
    )
}

fn feedback_check() -> Result<String, String> {
    let (mut world, _) = world()?;
    step(&mut world);
    let (after, _) = run(&mut world, "probe_feedback");
    let pc = player(&after).map(|e| e.id.0);
    let seen: Vec<(u32, u32)> = (after.progress.feedback.iter())
        .map(|f| (f.over, f.strref))
        .collect();
    // Gone once its five seconds are up.
    for _ in 0..(5100 / MSEC) {
        step(&mut world);
    }
    let (later, _) = step(&mut world);
    check(
        pc.is_some_and(|pc| seen == [(pc, FEEDBACK[1])]) && later.progress.feedback.is_empty(),
        format!(
            "over the player {seen:?}; after 5 s {:?}",
            later.progress.feedback
        ),
    )
}

fn tutorial_check() -> Result<String, String> {
    let (mut world, _) = world()?;
    step(&mut world);
    // In a conversation: nothing, the row not marked.
    let mut state = world.script_state().clone();
    state
        .talk
        .conversations
        .push(crate::script::dialogue::Conversation {
            party: true,
            ..Default::default()
        });
    world.set_script_state(state);
    let (_, talking) = run(&mut world, "probe_tutorial");
    let marked_talking = world.script_state().party.tutorials.shown(KREIA_HINT);
    let mut state = world.script_state().clone();
    state.talk.conversations.clear();
    world.set_script_state(state);
    let (_, first) = run(&mut world, "probe_tutorial");
    let (_, second) = run(&mut world, "probe_tutorial");
    let windows = |notices: &[Notice]| {
        let shown = |notice: &&Notice| matches!(notice, Notice::Show(Screen::Tutorial { .. }));
        notices.iter().filter(shown).count()
    };
    let kept = saved(world.script_state())?
        .party
        .tutorials
        .shown(KREIA_HINT);
    let told = [windows(&talking), windows(&first), windows(&second)];
    check(
        told == [0, 1, 0] && !marked_talking && kept,
        format!(
            "windows in talk, first, second {told:?}; marked in talk {marked_talking}; kept in a save {kept}"
        ),
    )
}

fn map_check() -> Result<String, String> {
    let (mut world, _) = world()?;
    step(&mut world);
    let (jammed, _) = run(&mut world, "probe_jam");
    let kept = saved(world.script_state())?.party.no_map;
    let (transit, _) = run(&mut world, "probe_transit");
    let (freed, _) = run(&mut world, "probe_unjam");
    let screens = transit.screens;
    let want = crate::snapshot::Transit {
        show: true,
        label: 38477,
        query: 42466,
        disabled: true,
    };
    check(
        jammed.screens.map_disabled
            && kept
            && screens.transit == want
            && !freed.screens.map_disabled,
        format!(
            "jammed {}, kept {kept}, transit {:?}, freed {}",
            jammed.screens.map_disabled, screens.transit, !freed.screens.map_disabled
        ),
    )
}

fn bodies_check() -> Result<String, String> {
    let (mut world, _) = world()?;
    step(&mut world);
    // The player takes the talker's head (its appearance's), then its new
    // appearance, the head kept.
    let (headed, _) = run(&mut world, "probe_head");
    let headed_look = player(&headed).and_then(|e| e.fighter).map(|f| f.look);
    let (changed, _) = run(&mut world, "probe_appearance");
    let look = player(&changed).and_then(|e| e.fighter).map(|f| f.look);
    let (overlaid, _) = run(&mut world, "probe_overlay");
    let acting = player(&overlaid)
        .and_then(|e| e.fighter)
        .and_then(|f| f.acting)
        .map(|a| a.animation);
    // Kept in the snapshot a save holds.
    let reloaded = Snapshot::decode(&overlaid.encode()).map_err(|error| error.to_string())?;
    let kept = player(&reloaded).and_then(|e| e.fighter).map(|f| f.look);
    let changed_to = Some(KREIA_FINAL as u16);
    check(
        look.is_some_and(|look| {
            look.appearance == changed_to && look.head == Some(TALKER_HEAD as u8)
        }) && headed_look.is_some_and(|look| look.head == Some(TALKER_HEAD as u8))
            && acting == Some(123)
            && kept.is_some_and(|look| look.appearance == changed_to),
        format!(
            "headed {headed_look:?}, changed {look:?}, acting {acting:?}, after a save {kept:?}"
        ),
    )
}

/// A player in a disguise keeps its appearance.
fn disguised_check() -> Result<String, String> {
    let (mut world, _) = world()?;
    step(&mut world);
    let mut bodies = world.world.query::<(&Avatar, &mut super::PlayerSheet)>();
    for (_, mut sheet) in bodies.iter_mut(&mut world.world) {
        sheet.0.gear.disguise = Some(28);
    }
    let (after, _) = run(&mut world, "probe_appearance");
    let look = player(&after).and_then(|e| e.fighter).map(|f| f.look);
    check(
        look.is_some_and(|look| look.appearance.is_none()),
        format!("disguised: {look:?}"),
    )
}

fn objects_check() -> Result<String, String> {
    let (mut world, _) = world()?;
    step(&mut world);
    // The gate open, then shut by the hack.
    let mut objects = world.world.query::<&mut Object>();
    for mut object in objects.iter_mut(&mut world.world) {
        if object.index == GATE {
            object.state.state = 1;
        }
    }
    run(&mut world, "probe_close_door");
    let (after, _) = step(&mut world);
    let gate = (after.entities.iter())
        .find(|e| matches!(e.kind, crate::Kind::Object(ObjectKind::Door, GATE)))
        .and_then(|e| e.object)
        .map(|o| o.state);
    run(&mut world, "probe_sound");
    let (moved, _) = step(&mut world);
    let sound = (moved.entities.iter())
        .find(|e| matches!(e.kind, crate::Kind::Object(ObjectKind::Sound, _)))
        .map(|e| e.position);
    let pitch = tokens(&moved, 8..=8);
    // A datapad item read.
    let id = crate::inventory::FIRST_ITEM + 900;
    let mut state = world.script_state().clone();
    state.items.insert(
        id,
        crate::inventory::Item {
            template: "qu_fakewill".into(),
            holder: crate::inventory::PARTY,
            slot: None,
            stack: 1,
            charges: 0,
            droppable: true,
            plot: true,
            identified: true,
            infinite: false,
            non_equippable: false,
            upgrades: 0,
            fitted: Vec::new(),
        },
    );
    world.set_script_state(state);
    let (_, read) = run_for(&mut world, "probe_datapad", ObjectId(id));
    let shown = read.iter().any(|notice| {
        matches!(notice, Notice::Show(Screen::Datapad { template }) if template == "qu_fakewill")
    });
    let marked = world.script_state().locals.get(&(id, 0, 94)) == Some(&1);
    check(
        gate == Some(0) && sound == Some([1.0, 2.0, 3.0]) && pitch == ["0.25"] && shown && marked,
        format!(
            "gate {gate:?}, sound at {sound:?}, pitch {pitch:?}, datapad shown {shown} and read {marked}"
        ),
    )
}
